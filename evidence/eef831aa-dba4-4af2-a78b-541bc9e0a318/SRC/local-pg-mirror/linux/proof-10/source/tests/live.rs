//! Public-fixture proof. One PostgreSQL, two databases. Not an enterprise copy.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use local_pg_mirror::{
    copy, discover, read_local, CopyRequest, CopyStop, DiscoverRequest, LocalRead, Manifest, ReadRequest,
};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::ConnectOptions;

const GOLD_OWNER: &str = "11111111-1111-4111-8111-111111111111";
const CANCEL_OWNER: &str = "22222222-2222-4222-8222-222222222222";
const PARTIAL_OWNER: &str = "33333333-3333-4333-8333-333333333333";
const ODD_OWNER: &str = "44444444-4444-4444-8444-444444444444";
const TIMEOUT_OWNER: &str = "55555555-5555-4555-8555-555555555555";

struct Fixture {
    host: String,
    port: u16,
    server_name: String,
    source_database: String,
    target_database: String,
    admin_user: String,
    source_user: String,
    runtime_user: String,
    reader_user: String,
    admin_password: PathBuf,
    source_password: PathBuf,
    runtime_password: PathBuf,
    reader_password: PathBuf,
    ca: PathBuf,
    ca_sha256: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn public_fixture_copy_is_local() {
    let fixture = load_fixture();
    let root = std::env::temp_dir().join("a318-mirror");
    std::fs::create_dir_all(&root).unwrap_or_else(|_| panic!("workdir"));
    let admin_pass = secret(&fixture.admin_password);
    let src_pass = secret(&fixture.source_password);
    let runtime_pass = secret(&fixture.runtime_password);
    let reader_pass = secret(&fixture.reader_password);
    let ca = std::fs::read(&fixture.ca).unwrap_or_else(|_| panic!("ca file"));

    let mut admin = dial(&fixture, &fixture.admin_user, &admin_pass, "postgres", &ca).await;
    raw(&mut admin, "CREATE DATABASE srcdb TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
    raw(&mut admin, "CREATE DATABASE mirrordb TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
    raw(&mut admin, &format!("CREATE ROLE oo_src_read LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '{src_pass}'")).await;
    raw(&mut admin, &format!("CREATE ROLE oo_mirror_runtime LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '{runtime_pass}'")).await;
    raw(&mut admin, &format!("CREATE ROLE oo_mirror_read LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '{reader_pass}'")).await;
    drop(admin);

    let mut src = dial(&fixture, &fixture.admin_user, &admin_pass, &fixture.source_database, &ca).await;
    raw(&mut src, "REVOKE ALL ON DATABASE srcdb FROM PUBLIC").await;
    raw(&mut src, "REVOKE CREATE ON DATABASE srcdb FROM PUBLIC").await;
    raw(&mut src, "REVOKE ALL ON SCHEMA public FROM PUBLIC").await;
    raw(&mut src, "REVOKE CREATE ON SCHEMA public FROM PUBLIC").await;
    raw(&mut src, "CREATE TABLE public.amounts (id int8 PRIMARY KEY, qty int8, price numeric(12,2), note text)").await;
    raw(&mut src, "INSERT INTO public.amounts VALUES (1, 9223372036854775807, 10.50, NULL), (2, -5, 0.00, 'kept')").await;
    raw(&mut src, "CREATE TABLE public.events (id int8 PRIMARY KEY, seen timestamp, body json, blob bytea)").await;
    raw(&mut src, "INSERT INTO public.events VALUES (7, timestamp '2024-03-04 05:06:07', '{\"k\":\"v\"}'::json, '\\x0001ff'::bytea)").await;
    raw(&mut src, "CREATE TABLE public.flags (id int8 PRIMARY KEY, on_flag bool, label text)").await;
    raw(&mut src, "INSERT INTO public.flags VALUES (1, true, 'yes'), (2, false, '')").await;
    raw(&mut src, "CREATE TABLE public.odd (id int8 PRIMARY KEY, doc jsonb)").await;
    raw(&mut src, "INSERT INTO public.odd VALUES (1, '{\"a\":1}'::jsonb)").await;
    raw(&mut src, "GRANT CONNECT ON DATABASE srcdb TO oo_src_read").await;
    raw(&mut src, "GRANT USAGE ON SCHEMA public TO oo_src_read").await;
    raw(&mut src, "GRANT SELECT ON TABLE public.amounts, public.events, public.flags, public.odd TO oo_src_read").await;
    drop(src);

    let mut reader = dial(&fixture, &fixture.source_user, &src_pass, &fixture.source_database, &ca).await;
    match sqlx::query("INSERT INTO public.amounts (id, qty, price, note) VALUES (9, 1, 1.00, 'no')")
        .execute(&mut reader)
        .await
    {
        Ok(_) => panic!("source insert was allowed"),
        Err(err) => assert_eq!(fail_class(&err), "42501"),
    }
    drop(reader);

    let gold_tables = [("public", "amounts"), ("public", "events"), ("public", "flags")];
    let gold = run_copy(
        &fixture,
        &root,
        "gold",
        "fixa",
        "gold",
        GOLD_OWNER,
        &gold_tables,
        300_000,
        CopyStop::Finish,
    )
    .await;
    assert_eq!(
        (gold.manifest.mirror_state.as_str(), gold.manifest.failure.as_str()),
        ("complete", "none")
    );
    assert_eq!(gold.manifest.rows, 5);
    assert_eq!(gold.manifest.consistency, "single_snapshot");
    assert_eq!(gold.manifest.source_socket_peak, Some(2));
    assert_eq!(gold.manifest.source_socket_fact, "verified_reader_guard");
    assert_eq!(gold.manifest.recheck_connections, 1);
    assert_eq!(gold.manifest.tables_completed.len(), 3);
    assert!(gold.manifest.tables_incomplete.is_empty());
    assert_public(&gold.manifest);
    let local = read_back(&fixture, &root, "gold").await;
    assert_eq!(local.tables.len(), 3);
    assert_eq!(table(&local, "amounts"), vec![
        vec![Some("1".to_string()), Some("9223372036854775807".to_string()), Some("10.50".to_string()), None],
        vec![Some("2".to_string()), Some("-5".to_string()), Some("0.00".to_string()), Some("kept".to_string())],
    ]);
    assert_eq!(table(&local, "events"), vec![vec![
        Some("7".to_string()),
        Some("2024-03-04 05:06:07".to_string()),
        Some("{\"k\":\"v\"}".to_string()),
        Some("0001ff".to_string()),
    ]]);
    assert_eq!(table(&local, "flags"), vec![
        vec![Some("1".to_string()), Some("true".to_string()), Some("yes".to_string())],
        vec![Some("2".to_string()), Some("false".to_string()), Some(String::new())],
    ]);

    let mut src = dial(&fixture, &fixture.admin_user, &admin_pass, &fixture.source_database, &ca).await;
    raw(&mut src, "ALTER TABLE public.amounts ADD COLUMN extra text").await;
    drop(src);
    let drifted = copy(copy_request(&fixture, &root, "gold", "gold-drift", CopyStop::Finish))
        .await
        .unwrap_or_else(|err| panic!("drift {} {}", err.code, err.fact));
    assert_eq!(drifted.manifest.mirror_state, "refused");
    assert_eq!(drifted.manifest.failure, "catalog_drift");
    assert_public(&drifted.manifest);
    assert_eq!(read_back(&fixture, &root, "gold").await.tables.len(), 3);
    assert_eq!(table(&read_back(&fixture, &root, "gold").await, "amounts").len(), 2);

    let flags = [("public", "flags")];
    let cancelled = run_copy(&fixture, &root, "cancel", "fixc", "cancel", CANCEL_OWNER, &flags, 300_000, CopyStop::CancelBeforePull).await;
    assert_eq!(cancelled.manifest.mirror_state, "incomplete");
    assert_eq!(cancelled.manifest.rows, 0);
    assert_eq!(cancelled.manifest.failure, "cancel");
    assert_ne!(cancelled.manifest.consistency, "single_snapshot");
    assert_public(&cancelled.manifest);

    let partial = run_copy(&fixture, &root, "partial", "fixp", "partial", PARTIAL_OWNER, &flags, 300_000, CopyStop::StopAfterBatches(1)).await;
    assert_eq!(partial.manifest.mirror_state, "incomplete");
    assert_eq!(partial.manifest.failure, "stop_after_batches");
    assert!(partial.manifest.tables_completed.is_empty());
    let physical = partial
        .manifest
        .tables_incomplete
        .iter()
        .find_map(|table| table.physical_name.clone())
        .unwrap_or_else(|| panic!("partial physical"));
    assert_public(&partial.manifest);

    let odd = [("public", "odd")];
    let unsupported = run_copy(&fixture, &root, "odd", "fixu", "unsupported", ODD_OWNER, &odd, 300_000, CopyStop::Finish).await;
    assert_eq!(unsupported.manifest.mirror_state, "refused");
    assert_eq!(unsupported.manifest.failure, "unsupported_column");
    assert!(unsupported.manifest.tables_completed.is_empty());
    assert!(unsupported.manifest.quarantine.iter().any(|row| row.oid == 3802 && row.reason == "unsupported_column"));
    assert_public(&unsupported.manifest);

    let timed = run_copy(&fixture, &root, "timed", "fixt", "timeout", TIMEOUT_OWNER, &flags, 1, CopyStop::Finish).await;
    assert_eq!(timed.manifest.mirror_state, "incomplete");
    assert_eq!(timed.manifest.failure, "deadline");
    assert_public(&timed.manifest);

    let absent = root.join("absent-secret");
    assert!(!absent.exists());
    let replay_err = copy(CopyRequest {
        plan_path: root.join("partial.plan.json"),
        source_password: absent.clone(),
        source_ca: fixture.ca.clone(),
        admin_password: absent.clone(),
        runtime_password: absent.clone(),
        manifest_path: root.join("partial.manifest.json"),
        target_config_path: root.join("partial.target.json"),
        target_ca: fixture.ca.clone(),
        stop: CopyStop::Finish,
    })
    .await
    .expect_err("resume");
    assert_eq!(replay_err.code, "resume_refused");
    assert!(!absent.exists());
    assert_eq!(table(&read_back(&fixture, &root, "gold").await, "amounts").len(), 2);

    let refused = read_local(ReadRequest {
        target_config: root.join("partial.target.json"),
        manifest: root.join("partial.manifest.json"),
        reader_password: fixture.reader_password.clone(),
        ca: fixture.ca.clone(),
    })
    .await
    .expect_err("partial read");
    assert_eq!(refused.code, "refused");
    assert_eq!(refused.fact, "not_complete");
    let quoted = oo_pipeline::physical_mapping::quote_physical_name(&physical).unwrap_or_else(|_| panic!("physical name"));
    let mut local_reader = dial(&fixture, &fixture.reader_user, &reader_pass, &fixture.target_database, &ca).await;
    match sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SELECT 1 FROM \"_oo\".{quoted}"))).execute(&mut local_reader).await {
        Ok(_) => panic!("partial select was allowed"),
        Err(err) => assert_eq!(fail_class(&err), "42501"),
    }
    drop(local_reader);

    let mut admin = dial(&fixture, &fixture.admin_user, &admin_pass, "postgres", &ca).await;
    raw(&mut admin, "REVOKE CONNECT ON DATABASE srcdb FROM oo_src_read").await;
    raw(&mut admin, "SELECT pg_catalog.pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity WHERE datname = 'srcdb' AND usename = 'oo_src_read'").await;
    drop(admin);
    assert!(
        dial_result(&fixture, &fixture.source_user, &src_pass, &fixture.source_database, &ca)
            .await
            .is_err(),
        "source connect was allowed"
    );
    let offline = read_back(&fixture, &root, "gold").await;
    assert_eq!(table(&offline, "events")[0][3].as_deref(), Some("0001ff"));
    assert_eq!(row_count(&offline), 5);

    let again = copy(CopyRequest {
        plan_path: root.join("gold.plan.json"),
        source_password: absent.clone(),
        source_ca: fixture.ca.clone(),
        admin_password: absent.clone(),
        runtime_password: absent,
        manifest_path: root.join("gold.manifest.json"),
        target_config_path: root.join("gold.target.json"),
        target_ca: fixture.ca.clone(),
        stop: CopyStop::Finish,
    })
    .await
    .unwrap_or_else(|err| panic!("replay {} {}", err.code, err.fact));
    assert_eq!(again.disposition, "replay_complete");
    assert_eq!(again.manifest.rows, 5);
    assert!(!again.manifest.customer_complete);
    assert_eq!(row_count(&read_back(&fixture, &root, "gold").await), 5);
}

fn assert_public(manifest: &Manifest) {
    assert!(!manifest.customer_complete);
    assert_eq!(manifest.enterprise, "not_configured");
    assert!(!manifest.source_count_star);
    assert!(!manifest.source_explain);
    assert!(!manifest.source_ddl);
}

#[allow(clippy::too_many_arguments)]
async fn run_copy(
    fixture: &Fixture,
    root: &Path,
    name: &str,
    source_id: &str,
    shard: &str,
    owner: &str,
    tables: &[(&str, &str)],
    deadline: u64,
    stop: CopyStop,
) -> local_pg_mirror::CopyOutcome {
    let config = root.join(format!("{name}.json"));
    std::fs::write(&config, document(fixture, source_id, shard, owner, tables, deadline)).unwrap_or_else(|_| panic!("config write"));
    let plan = discover(DiscoverRequest {
        config_path: config,
        plan_path: root.join(format!("{name}.plan.json")),
        source_password: fixture.source_password.clone(),
        source_ca: fixture.ca.clone(),
    })
    .await
    .unwrap_or_else(|err| panic!("discover {} {}", err.code, err.fact));
    assert_eq!(plan.plan_sha256.len(), 64);
    copy(copy_request(fixture, root, name, name, stop))
        .await
        .unwrap_or_else(|err| panic!("copy {} {}", err.code, err.fact))
}

fn copy_request(fixture: &Fixture, root: &Path, plan: &str, manifest: &str, stop: CopyStop) -> CopyRequest {
    CopyRequest {
        plan_path: root.join(format!("{plan}.plan.json")),
        source_password: fixture.source_password.clone(),
        source_ca: fixture.ca.clone(),
        admin_password: fixture.admin_password.clone(),
        runtime_password: fixture.runtime_password.clone(),
        manifest_path: root.join(format!("{manifest}.manifest.json")),
        target_config_path: root.join(format!("{manifest}.target.json")),
        target_ca: fixture.ca.clone(),
        stop,
    }
}

async fn read_back(fixture: &Fixture, root: &Path, name: &str) -> LocalRead {
    read_local(ReadRequest {
        target_config: root.join(format!("{name}.target.json")),
        manifest: root.join(format!("{name}.manifest.json")),
        reader_password: fixture.reader_password.clone(),
        ca: fixture.ca.clone(),
    })
    .await
    .unwrap_or_else(|err| panic!("read {} {}", err.code, err.fact))
}

fn table(read: &LocalRead, name: &str) -> Vec<Vec<Option<String>>> {
    let mut rows = read
        .tables
        .iter()
        .find(|table| table.name == name)
        .unwrap_or_else(|| panic!("table"))
        .rows
        .clone();
    rows.sort();
    rows
}

fn row_count(read: &LocalRead) -> usize {
    read.tables.iter().map(|table| table.rows.len()).sum()
}

fn document(fixture: &Fixture, source_id: &str, shard: &str, owner: &str, tables: &[(&str, &str)], deadline: u64) -> String {
    let mut listed = String::from("[");
    for (index, (schema, table)) in tables.iter().enumerate() {
        if index > 0 {
            listed.push(',');
        }
        listed.push_str(&format!(r#"{{"schema":"{schema}","table":"{table}"}}"#));
    }
    listed.push(']');
    format!(
        r#"{{"protocol":"oo.pipeline.local-pg-mirror/v1","engine":"postgresql","source_id":"{source_id}","host":"{host}","port":{port},"database":"{database}","user":"{user}","server_name":"{server}","ca_sha256":"{ca}","secret_ref":"secret://fixture/src","tables":{listed},"connect_timeout_ms":10000,"statement_timeout_ms":30000,"lock_timeout_ms":1000,"max_rows":10000,"max_bytes":1048576,"batch_rows":1000,"snapshot_deadline_ms":{deadline},"generation":1,"shard":"{shard}","owner":"{owner}","target_host":"{host}","target_port":{port},"target_database":"{target_database}","target_admin_user":"{admin}","target_runtime_user":"{runtime}","target_reader_user":"{reader}","target_server_name":"{server}","target_ca_sha256":"{ca}"}}"#,
        host = fixture.host,
        port = fixture.port,
        database = fixture.source_database,
        user = fixture.source_user,
        server = fixture.server_name,
        ca = fixture.ca_sha256,
        target_database = fixture.target_database,
        admin = fixture.admin_user,
        runtime = fixture.runtime_user,
        reader = fixture.reader_user,
    )
}

fn load_fixture() -> Fixture {
    let path = std::env::var("OO_MIRROR_FIXTURE").unwrap_or_else(|_| panic!("fixture env"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("fixture file"));
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("fixture json"));
    let ca = PathBuf::from(field(&value, "ca"));
    let ca_bytes = std::fs::read(&ca).unwrap_or_else(|_| panic!("ca file"));
    Fixture {
        host: field(&value, "host"),
        port: u16::try_from(value.get("port").and_then(Value::as_u64).unwrap_or_else(|| panic!("fixture port"))).unwrap_or_else(|_| panic!("fixture port")),
        server_name: field(&value, "server_name"),
        source_database: field(&value, "source_database"),
        target_database: field(&value, "target_database"),
        admin_user: field(&value, "admin_user"),
        source_user: field(&value, "source_user"),
        runtime_user: field(&value, "runtime_user"),
        reader_user: field(&value, "reader_user"),
        admin_password: PathBuf::from(field(&value, "admin_password")),
        source_password: PathBuf::from(field(&value, "source_password")),
        runtime_password: PathBuf::from(field(&value, "runtime_password")),
        reader_password: PathBuf::from(field(&value, "reader_password")),
        ca_sha256: oo_pipeline::target_installer::sha256_hex(&ca_bytes),
        ca,
    }
}

fn field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_else(|| panic!("fixture field")).to_string()
}

fn secret(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("secret file"));
    let text = text.strip_suffix('\n').unwrap_or(&text);
    if text.len() != 48 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("secret shape");
    }
    text.to_string()
}

async fn dial(fixture: &Fixture, user: &str, password: &str, database: &str, ca: &[u8]) -> PgConnection {
    dial_result(fixture, user, password, database, ca).await.unwrap_or_else(|err| panic!("dial {}", fail_class(&err)))
}

async fn dial_result(fixture: &Fixture, user: &str, password: &str, database: &str, ca: &[u8]) -> Result<PgConnection, sqlx::Error> {
    let options = PgConnectOptions::from_explicit()
        .host(&fixture.host)
        .port(fixture.port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    if options.get_options().is_some() {
        panic!("connect options");
    }
    let options = options.disable_statement_logging();
    let mut conn = tokio::time::timeout(Duration::from_secs(30), options.connect())
        .await
        .unwrap_or_else(|_| panic!("dial timeout"))?;
    sqlx::query("SET statement_timeout = '30s'").execute(&mut conn).await?;
    sqlx::query("SET lock_timeout = '1s'").execute(&mut conn).await?;
    Ok(conn)
}

async fn raw(conn: &mut PgConnection, sql: &str) {
    if let Err(err) = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string())).execute(&mut *conn).await {
        panic!("sql {}", fail_class(&err));
    }
}

fn fail_class(err: &sqlx::Error) -> String {
    if let Some(state) = err.as_database_error().and_then(|db| db.code()) {
        let state = state.into_owned();
        if state.len() == 5 && state.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return state;
        }
    }
    match err {
        sqlx::Error::Tls(_) => "tls".to_string(),
        sqlx::Error::Io(_) => "io".to_string(),
        sqlx::Error::Protocol(_) => "protocol".to_string(),
        _ => "driver".to_string(),
    }
}
