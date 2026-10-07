//! Independent review oracle. It does not change the product.
//! A passing run records what the fixed tree does, including two source sockets.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use local_pg_mirror::{
    copy, discover, parse_config_text, read_local, CopyRequest, CopyStop, DiscoverRequest, LocalRead, Manifest,
    ReadRequest,
};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};

const WIDE_ROWS: i64 = 20000;
const GOLD_OWNER: &str = "a3350001-0000-4000-8000-000000000001";
const CANCEL_OWNER: &str = "a3350002-0000-4000-8000-000000000002";
const PARTIAL_OWNER: &str = "a3350003-0000-4000-8000-000000000003";
const ODD_OWNER: &str = "a3350004-0000-4000-8000-000000000004";
const TIME_OWNER: &str = "a3350005-0000-4000-8000-000000000005";
const WIDE_OWNER: &str = "a3350006-0000-4000-8000-000000000006";

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn independent_local_copy() {
    let note_path = std::env::var("OO_REVIEW_NOTE").unwrap_or_else(|_| panic!("note path"));
    let _ = std::fs::remove_file(&note_path);
    note("oracle=independent_local_copy");
    let fixture = load_fixture();
    assert_eq!(fixture.host, "oo-a335-pg");
    assert_eq!(fixture.source_database, "revsrc");
    assert_eq!(fixture.target_database, "revmirror");
    assert_eq!(fixture.source_user, "oo_rev_src");
    assert_eq!(fixture.runtime_user, "oo_rev_runtime");
    assert_eq!(fixture.reader_user, "oo_rev_read");
    check_closed_budgets(&fixture);
    check_engine_before_credentials(&fixture);

    let root = std::env::temp_dir().join("a335-mirror");
    std::fs::create_dir_all(&root).unwrap_or_else(|_| panic!("workdir"));
    let admin_pass = hex_secret(&fixture.admin_password);
    let src_pass = hex_secret(&fixture.source_password);
    let runtime_pass = hex_secret(&fixture.runtime_password);
    let reader_pass = hex_secret(&fixture.reader_password);
    let ca = std::fs::read(&fixture.ca).unwrap_or_else(|_| panic!("ca file"));

    let mut admin = dial(&fixture, &fixture.admin_user, &admin_pass, "postgres", &ca).await;
    raw(&mut admin, "CREATE DATABASE revsrc TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
    raw(&mut admin, "CREATE DATABASE revmirror TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
    raw(&mut admin, &format!("CREATE ROLE oo_rev_src LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '{src_pass}'")).await;
    raw(&mut admin, &format!("CREATE ROLE oo_rev_runtime LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '{runtime_pass}'")).await;
    raw(&mut admin, &format!("CREATE ROLE oo_rev_read LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '{reader_pass}'")).await;
    drop(admin);

    let mut src = dial(&fixture, &fixture.admin_user, &admin_pass, "revsrc", &ca).await;
    raw(&mut src, "REVOKE ALL ON DATABASE revsrc FROM PUBLIC").await;
    raw(&mut src, "REVOKE CREATE ON DATABASE revsrc FROM PUBLIC").await;
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
    raw(&mut src, "CREATE TABLE public.wide (id int8 PRIMARY KEY, n int8)").await;
    raw(&mut src, &format!("INSERT INTO public.wide SELECT g, g * 2 FROM generate_series(1, {WIDE_ROWS}) AS g")).await;
    raw(&mut src, "GRANT CONNECT ON DATABASE revsrc TO oo_rev_src").await;
    raw(&mut src, "GRANT USAGE ON SCHEMA public TO oo_rev_src").await;
    raw(&mut src, "GRANT SELECT ON TABLE public.amounts, public.events, public.flags, public.odd, public.wide TO oo_rev_src").await;
    drop(src);

    let mut reader = dial(&fixture, &fixture.source_user, &src_pass, "revsrc", &ca).await;
    match sqlx::query("INSERT INTO public.amounts (id, qty, price, note) VALUES (9, 1, 1.00, 'no')")
        .execute(&mut reader)
        .await
    {
        Ok(_) => panic!("source insert was allowed"),
        Err(err) => assert_eq!(fail_class(&err), "42501"),
    }
    drop(reader);
    note("source_insert_sqlstate=42501");

    let gold_tables = [("public", "amounts"), ("public", "events"), ("public", "flags")];
    let gold = run_copy(&fixture, &root, "gold", "rgold", "gold", GOLD_OWNER, &gold_tables, 300_000, 1000, CopyStop::Finish).await;
    note(&format!("gold_state={} gold_failure={} gold_rows={}", gold.manifest.mirror_state, gold.manifest.failure, gold.manifest.rows));
    note(&format!(
        "gold_manifest_peak={:?} gold_manifest_fact={} gold_recheck={}",
        gold.manifest.source_socket_peak, gold.manifest.source_socket_fact, gold.manifest.recheck_connections
    ));
    assert_eq!((gold.manifest.mirror_state.as_str(), gold.manifest.failure.as_str()), ("complete", "none"));
    assert_eq!(gold.manifest.rows, 5);
    assert_eq!(gold.manifest.consistency, "single_snapshot");
    assert!(!gold.manifest.customer_complete);
    assert_eq!(gold.manifest.enterprise, "not_configured");
    assert!(!gold.manifest.source_count_star);
    assert!(!gold.manifest.source_explain);
    assert!(!gold.manifest.source_ddl);
    assert_eq!(gold.manifest.batch_rows, 1000);
    assert_eq!(gold.manifest.statement_timeout_ms, 30000);
    assert_eq!(gold.manifest.lock_timeout_ms, 1000);
    assert_eq!(gold.manifest.snapshot_deadline_ms, 300_000);
    assert_eq!(gold.manifest.mapping_algorithm, "physical-mapping/v1");
    assert_eq!(gold.manifest.system_position, "_oo_pos numeric(38,0)");
    assert_eq!(gold.manifest.source_socket_peak, Some(2));
    assert_eq!(gold.manifest.source_socket_fact, "verified_reader_guard");
    assert_logical_physical(&gold.manifest);
    assert_secret_free(&root.join("gold.manifest.json"), &src_pass);
    assert_target_config(&root.join("gold.target.json"));
    let local = read_back(&fixture, &root, "gold").await;
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
    note("hand_gold=int8_null_numeric_12_2_timestamp_json_bytea_bool_empty");

    let mut src = dial(&fixture, &fixture.admin_user, &admin_pass, "revsrc", &ca).await;
    raw(&mut src, "ALTER TABLE public.amounts ADD COLUMN extra text").await;
    drop(src);
    let drifted = copy(copy_request(&fixture, &root, "gold", "gold-drift", CopyStop::Finish)).await.unwrap_or_else(|err| panic!("drift {err}"));
    note(&format!("drift_state={} drift_failure={} drift_peak={:?}", drifted.manifest.mirror_state, drifted.manifest.failure, drifted.manifest.source_socket_peak));
    assert_eq!(drifted.manifest.mirror_state, "refused");
    assert_eq!(drifted.manifest.failure, "catalog_drift");
    assert_eq!(table(&read_back(&fixture, &root, "gold").await, "amounts").len(), 2);

    let flags = [("public", "flags")];
    let cancelled = run_copy(&fixture, &root, "cancel", "rcancel", "cancel", CANCEL_OWNER, &flags, 300_000, 1000, CopyStop::CancelBeforePull).await;
    note(&format!("cancel_state={} cancel_failure={} cancel_rows={}", cancelled.manifest.mirror_state, cancelled.manifest.failure, cancelled.manifest.rows));
    assert_eq!(cancelled.manifest.mirror_state, "incomplete");
    assert_eq!(cancelled.manifest.failure, "cancel");
    assert_eq!(cancelled.manifest.rows, 0);
    assert_ne!(cancelled.manifest.consistency, "single_snapshot");

    let partial = run_copy(&fixture, &root, "partial", "rpartial", "partial", PARTIAL_OWNER, &flags, 300_000, 1000, CopyStop::StopAfterBatches(1)).await;
    note(&format!("partial_state={} partial_failure={}", partial.manifest.mirror_state, partial.manifest.failure));
    assert_eq!(partial.manifest.mirror_state, "incomplete");
    assert_eq!(partial.manifest.failure, "stop_after_batches");
    assert!(partial.manifest.tables_completed.is_empty());
    let physical = partial.manifest.tables_incomplete.iter().find_map(|table| table.physical_name.clone()).unwrap_or_else(|| panic!("partial physical"));

    let odd = [("public", "odd")];
    let unsupported = run_copy(&fixture, &root, "odd", "rodd", "odd", ODD_OWNER, &odd, 300_000, 1000, CopyStop::Finish).await;
    note(&format!(
        "quarantine_state={} quarantine_failure={} quarantine_oids={}",
        unsupported.manifest.mirror_state,
        unsupported.manifest.failure,
        unsupported.manifest.quarantine.iter().map(|row| row.oid.to_string()).collect::<Vec<_>>().join(",")
    ));
    assert_eq!(unsupported.manifest.mirror_state, "refused");
    assert_eq!(unsupported.manifest.failure, "unsupported_column");
    assert!(unsupported.manifest.quarantine.iter().any(|row| row.oid == 3802 && row.reason == "unsupported_column" && row.column == "doc"));

    let timed = run_copy(&fixture, &root, "timed", "rtime", "timed", TIME_OWNER, &flags, 1, 1000, CopyStop::Finish).await;
    note(&format!("deadline_state={} deadline_failure={}", timed.manifest.mirror_state, timed.manifest.failure));
    assert_eq!(timed.manifest.mirror_state, "incomplete");
    assert_eq!(timed.manifest.failure, "deadline");

    let absent = root.join("absent-secret");
    assert!(!absent.exists());
    let resume_err = copy(CopyRequest {
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
    note(&format!("resume_code={} resume_fact={}", resume_err.code, resume_err.fact));
    assert_eq!(resume_err.code, "resume_refused");
    assert!(!absent.exists());

    let refused = read_local(ReadRequest {
        target_config: root.join("partial.target.json"),
        manifest: root.join("partial.manifest.json"),
        reader_password: absent.clone(),
        ca: fixture.ca.clone(),
    })
    .await
    .expect_err("partial read");
    note(&format!("partial_read_code={} partial_read_fact={}", refused.code, refused.fact));
    assert_eq!((refused.code, refused.fact), ("refused", "not_complete"));
    assert!(!absent.exists());
    let quoted = oo_pipeline::physical_mapping::quote_physical_name(&physical).unwrap_or_else(|_| panic!("physical name"));
    let mut local_reader = dial(&fixture, &fixture.reader_user, &reader_pass, "revmirror", &ca).await;
    match sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SELECT 1 FROM \"_oo\".{quoted}"))).execute(&mut local_reader).await {
        Ok(_) => panic!("partial select was allowed"),
        Err(err) => assert_eq!(fail_class(&err), "42501"),
    }
    drop(local_reader);
    note("partial_local_select_sqlstate=42501");

    let wide_tables = [("public", "wide")];
    let stop = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicU64::new(0));
    let names = Arc::new(Mutex::new(BTreeSet::<String>::new()));
    let watch = tokio::spawn(watch_source(
        fixture.host.clone(),
        fixture.port,
        fixture.admin_user.clone(),
        admin_pass.clone(),
        ca.clone(),
        stop.clone(),
        peak.clone(),
        names.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let wide = run_copy(&fixture, &root, "wide", "rwide", "wide", WIDE_OWNER, &wide_tables, 300_000, 1000, CopyStop::Finish).await;
    stop.store(true, Ordering::Relaxed);
    let _ = watch.await;
    let observed = peak.load(Ordering::Relaxed);
    let observed_names = names.lock().unwrap_or_else(|err| panic!("names {err}")).iter().cloned().collect::<Vec<_>>().join(",");
    note(&format!("wide_state={} wide_rows={} wide_manifest_peak={:?}", wide.manifest.mirror_state, wide.manifest.rows, wide.manifest.source_socket_peak));
    note(&format!("observed_source_backends_max={observed} observed_application_names={observed_names}"));
    assert_eq!(wide.manifest.mirror_state, "complete");
    assert_eq!(wide.manifest.rows, u64::try_from(WIDE_ROWS).unwrap_or_else(|_| panic!("rows")));
    assert_eq!(observed, 2, "source backend peak");
    note("requirement_one_source_connection=unmet");

    let mut admin = dial(&fixture, &fixture.admin_user, &admin_pass, "postgres", &ca).await;
    raw(&mut admin, "REVOKE CONNECT ON DATABASE revsrc FROM oo_rev_src").await;
    raw(&mut admin, "SELECT pg_catalog.pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity WHERE datname = 'revsrc' AND usename = 'oo_rev_src'").await;
    drop(admin);
    let source_after = dial_result(&fixture, &fixture.source_user, &src_pass, "revsrc", &ca).await;
    note(&format!("source_connect_after_revoke={}", if source_after.is_err() { "refused" } else { "allowed" }));
    assert!(source_after.is_err(), "source connect was allowed");
    let offline = read_back(&fixture, &root, "gold").await;
    assert_eq!(table(&offline, "amounts"), vec![
        vec![Some("1".to_string()), Some("9223372036854775807".to_string()), Some("10.50".to_string()), None],
        vec![Some("2".to_string()), Some("-5".to_string()), Some("0.00".to_string()), Some("kept".to_string())],
    ]);
    assert_eq!(table(&offline, "events")[0][3].as_deref(), Some("0001ff"));
    assert_eq!(row_count(&offline), 5);
    note("local_read_after_revoke_rows=5");

    let before = source_backends(&fixture, &admin_pass, &ca).await;
    let again = copy(CopyRequest {
        plan_path: root.join("gold.plan.json"),
        source_password: absent.clone(),
        source_ca: fixture.ca.clone(),
        admin_password: absent.clone(),
        runtime_password: absent.clone(),
        manifest_path: root.join("gold.manifest.json"),
        target_config_path: root.join("gold.target.json"),
        target_ca: fixture.ca.clone(),
        stop: CopyStop::Finish,
    })
    .await
    .unwrap_or_else(|err| panic!("replay {err}"));
    let after = source_backends(&fixture, &admin_pass, &ca).await;
    note(&format!("replay_disposition={} replay_source_backends_before={before} replay_source_backends_after={after}", again.disposition));
    assert_eq!(again.disposition, "replay_complete");
    assert_eq!(again.manifest.rows, 5);
    assert!(!again.manifest.customer_complete);
    assert!(!absent.exists());
    assert_eq!(before, 0);
    assert_eq!(after, 0);
    assert_eq!(row_count(&read_back(&fixture, &root, "gold").await), 5);
    note("oracle_finished=1");
}

fn check_closed_budgets(fixture: &Fixture) {
    let ceiling = document(fixture, "ceiling", "ceiling", GOLD_OWNER, &[("public", "amounts")], 300_000, 1000, 30_000, 1000);
    assert!(parse_config_text(&ceiling).is_ok());
    let over_batch = document(fixture, "ceiling", "ceiling", GOLD_OWNER, &[("public", "amounts")], 300_000, 1001, 30_000, 1000);
    assert_eq!(parse_config_text(&over_batch).expect_err("batch").fact, "batch_rows");
    let over_lock = document(fixture, "ceiling", "ceiling", GOLD_OWNER, &[("public", "amounts")], 300_000, 1000, 30_000, 1001);
    assert_eq!(parse_config_text(&over_lock).expect_err("lock").fact, "lock_timeout");
    let over_deadline = document(fixture, "ceiling", "ceiling", GOLD_OWNER, &[("public", "amounts")], 300_001, 1000, 30_000, 1000);
    assert_eq!(parse_config_text(&over_deadline).expect_err("deadline").fact, "snapshot_deadline");
    let over_statement = document(fixture, "ceiling", "ceiling", GOLD_OWNER, &[("public", "amounts")], 300_000, 1000, 30_001, 1000);
    assert_eq!(parse_config_text(&over_statement).expect_err("statement").fact, "statement_timeout");
    note("closed_budget_ceilings=batch_1000_statement_30000_lock_1000_snapshot_300000");
}

fn check_engine_before_credentials(fixture: &Fixture) {
    let root = std::env::temp_dir().join("a335-engine");
    std::fs::create_dir_all(&root).unwrap_or_else(|_| panic!("engine workdir"));
    let missing = root.join("absent-secret");
    let _ = std::fs::remove_file(&missing);
    let config = root.join("mysql.json");
    std::fs::write(&config, r#"{"engine":"mysql","statement_timeout_ms":30001}"#).unwrap_or_else(|_| panic!("mysql config"));
    let (code, stdout, stderr) = run_discover(&config, &root.join("mysql.plan.json"), &missing, &fixture.ca);
    note(&format!("mysql_exit={code} mysql_stderr={}", stderr.trim()));
    assert_eq!(code, 3);
    assert_eq!(stderr.trim(), "unsupported_engine engine");
    assert!(stdout.is_empty());
    assert!(!missing.exists());
    assert!(!root.join("mysql.plan.json").exists());
    let budget = root.join("budget.json");
    std::fs::write(
        &budget,
        document(fixture, "budgetx", "budget", GOLD_OWNER, &[("public", "amounts")], 300_000, 1000, 30_001, 1000),
    )
    .unwrap_or_else(|_| panic!("budget config"));
    let (code, stdout, stderr) = run_discover(&budget, &root.join("budget.plan.json"), &missing, &fixture.ca);
    note(&format!("over_statement_exit={code} over_statement_stderr={}", stderr.trim()));
    assert_eq!(code, 1);
    assert_eq!(stderr.trim(), "budget statement_timeout");
    assert!(stdout.is_empty());
    assert!(!missing.exists());
}

fn assert_logical_physical(manifest: &Manifest) {
    let amounts = manifest.tables_completed.iter().find(|table| table.name == "amounts").unwrap_or_else(|| panic!("amounts"));
    assert_eq!(amounts.schema, "public");
    let physical = amounts.physical_name.as_deref().unwrap_or_else(|| panic!("amounts physical"));
    assert!(physical.starts_with("pt_"), "table physical prefix");
    assert_eq!(physical.len(), 63);
    assert!(physical.bytes().skip(3).all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    let mut oids = BTreeSet::new();
    for column in &amounts.columns {
        assert!(column.physical_name.starts_with("pc_"));
        assert_eq!(column.physical_name.len(), 63);
        assert_ne!(column.physical_name, column.name);
        oids.insert((column.name.as_str(), column.oid, column.attnum));
    }
    assert!(oids.contains(&("id", 20, 1)));
    assert!(oids.contains(&("qty", 20, 2)));
    assert!(oids.contains(&("price", 1700, 3)));
    assert!(oids.contains(&("note", 25, 4)));
    let events = manifest.tables_completed.iter().find(|table| table.name == "events").unwrap_or_else(|| panic!("events"));
    assert_eq!(events.schema, "public");
    let event_oids: BTreeSet<_> = events.columns.iter().map(|column| (column.name.as_str(), column.oid)).collect();
    assert!(event_oids.contains(&("seen", 1114)));
    assert!(event_oids.contains(&("body", 114)));
    assert!(event_oids.contains(&("blob", 17)));
    let flags = manifest.tables_completed.iter().find(|table| table.name == "flags").unwrap_or_else(|| panic!("flags"));
    assert!(flags.columns.iter().any(|column| column.name == "on_flag" && column.oid == 16));
    note(&format!("logical_physical=public.amounts:{physical}"));
}

fn assert_secret_free(path: &Path, password: &str) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("manifest text"));
    assert!(!text.contains(password));
    assert!(!text.contains("\"password\""));
}

fn assert_target_config(path: &Path) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("target text"));
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("target json"));
    let object = value.as_object().unwrap_or_else(|| panic!("target object"));
    assert_eq!(object.get("protocol").and_then(Value::as_str), Some("oo.pipeline.local-pg-mirror-target/v1"));
    assert_eq!(object.get("database").and_then(Value::as_str), Some("revmirror"));
    assert_eq!(object.get("user").and_then(Value::as_str), Some("oo_rev_read"));
    assert_eq!(object.get("tls").and_then(Value::as_str), Some("verify-full"));
    assert!(object.get("password").is_none());
    assert!(object.get("secret_ref").is_none());
    assert!(object.get("source_id").is_none());
    note("target_config=secret_free_verify_full");
}

#[allow(clippy::too_many_arguments)]
async fn watch_source(
    host: String,
    port: u16,
    admin: String,
    password: String,
    ca: Vec<u8>,
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    names: Arc<Mutex<BTreeSet<String>>>,
) {
    let mut conn = match dial_owned(&host, port, &admin, &password, "postgres", &ca).await {
        Ok(conn) => conn,
        Err(err) => {
            note(&format!("watch_dial={}", fail_class(&err)));
            return;
        }
    };
    while !stop.load(Ordering::Relaxed) {
        let rows = sqlx::query(
            "SELECT application_name, COUNT(*)::bigint \
             FROM pg_catalog.pg_stat_activity \
             WHERE datname = 'revsrc' AND usename = 'oo_rev_src' \
             GROUP BY application_name",
        )
        .fetch_all(&mut conn)
        .await;
        let Ok(rows) = rows else {
            break;
        };
        let mut total = 0u64;
        for row in rows {
            let name: String = row.try_get::<Option<String>, _>(0).unwrap_or(None).unwrap_or_default();
            let count: i64 = row.try_get(1).unwrap_or(0);
            if count > 0 {
                total = total.saturating_add(u64::try_from(count).unwrap_or(0));
                names.lock().unwrap_or_else(|err| panic!("names {err}")).insert(name);
            }
        }
        peak.fetch_max(total, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn source_backends(fixture: &Fixture, admin_pass: &str, ca: &[u8]) -> u64 {
    let mut conn = dial(fixture, &fixture.admin_user, admin_pass, "postgres", ca).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM pg_catalog.pg_stat_activity WHERE datname = 'revsrc' AND usename = 'oo_rev_src'",
    )
    .fetch_one(&mut conn)
    .await
    .unwrap_or_else(|_| panic!("backend count"));
    u64::try_from(count).unwrap_or_else(|_| panic!("backend count"))
}

fn run_discover(config: &Path, plan: &Path, secret: &Path, ca: &Path) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_local-pg-mirror");
    let output = Command::new(bin)
        .arg("discover")
        .arg(config)
        .arg(plan)
        .arg(secret)
        .arg(ca)
        .output()
        .unwrap_or_else(|_| panic!("discover spawn"));
    let code = output.status.code().unwrap_or_else(|| panic!("discover status"));
    let stdout = String::from_utf8(output.stdout).unwrap_or_else(|_| panic!("discover stdout"));
    let stderr = String::from_utf8(output.stderr).unwrap_or_else(|_| panic!("discover stderr"));
    (code, stdout, stderr)
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
    batch_rows: u64,
    stop: CopyStop,
) -> local_pg_mirror::CopyOutcome {
    let config = root.join(format!("{name}.json"));
    std::fs::write(&config, document(fixture, source_id, shard, owner, tables, deadline, batch_rows, 30_000, 1000)).unwrap_or_else(|_| panic!("config write"));
    let plan = discover(DiscoverRequest {
        config_path: config,
        plan_path: root.join(format!("{name}.plan.json")),
        source_password: fixture.source_password.clone(),
        source_ca: fixture.ca.clone(),
    })
    .await
    .unwrap_or_else(|err| panic!("discover {err}"));
    assert_eq!(plan.plan_sha256.len(), 64);
    copy(copy_request(fixture, root, name, name, stop)).await.unwrap_or_else(|err| panic!("copy {name} {err}"))
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
    .unwrap_or_else(|err| panic!("read {err}"))
}

fn table(read: &LocalRead, name: &str) -> Vec<Vec<Option<String>>> {
    let mut rows = read.tables.iter().find(|table| table.name == name).unwrap_or_else(|| panic!("table")).rows.clone();
    rows.sort();
    rows
}

fn row_count(read: &LocalRead) -> usize {
    read.tables.iter().map(|table| table.rows.len()).sum()
}

#[allow(clippy::too_many_arguments)]
fn document(
    fixture: &Fixture,
    source_id: &str,
    shard: &str,
    owner: &str,
    tables: &[(&str, &str)],
    deadline: u64,
    batch_rows: u64,
    statement_timeout_ms: u64,
    lock_timeout_ms: u64,
) -> String {
    let mut listed = String::from("[");
    for (index, (schema, table)) in tables.iter().enumerate() {
        if index > 0 {
            listed.push(',');
        }
        listed.push_str(&format!(r#"{{"schema":"{schema}","table":"{table}"}}"#));
    }
    listed.push(']');
    format!(
        r#"{{"protocol":"oo.pipeline.local-pg-mirror/v1","engine":"postgresql","source_id":"{source_id}","host":"{host}","port":{port},"database":"{database}","user":"{user}","server_name":"{server}","ca_sha256":"{ca}","secret_ref":"secret://fixture/src","tables":{listed},"connect_timeout_ms":10000,"statement_timeout_ms":{statement_timeout_ms},"lock_timeout_ms":{lock_timeout_ms},"max_rows":100000,"max_bytes":1048576,"batch_rows":{batch_rows},"snapshot_deadline_ms":{deadline},"generation":1,"shard":"{shard}","owner":"{owner}","target_host":"{host}","target_port":{port},"target_database":"{target_database}","target_admin_user":"{admin}","target_runtime_user":"{runtime}","target_reader_user":"{reader}","target_server_name":"{server}","target_ca_sha256":"{ca}"}}"#,
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

fn hex_secret(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("secret file"));
    let text = text.strip_suffix('\n').unwrap_or(&text);
    if text.len() != 48 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("secret shape");
    }
    text.to_string()
}

fn note(line: &str) {
    let path = std::env::var("OO_REVIEW_NOTE").unwrap_or_else(|_| panic!("note path"));
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap_or_else(|_| panic!("note open"));
    writeln!(file, "{line}").unwrap_or_else(|_| panic!("note write"));
}

async fn dial(fixture: &Fixture, user: &str, password: &str, database: &str, ca: &[u8]) -> PgConnection {
    dial_result(fixture, user, password, database, ca).await.unwrap_or_else(|err| panic!("dial {}", fail_class(&err)))
}

async fn dial_owned(host: &str, port: u16, user: &str, password: &str, database: &str, ca: &[u8]) -> Result<PgConnection, sqlx::Error> {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    if options.get_options().is_some() {
        panic!("connect options");
    }
    let options = options.disable_statement_logging();
    let mut conn = tokio::time::timeout(Duration::from_secs(30), options.connect()).await.unwrap_or_else(|_| panic!("dial timeout"))?;
    sqlx::query("SET statement_timeout = '30s'").execute(&mut conn).await?;
    sqlx::query("SET lock_timeout = '1s'").execute(&mut conn).await?;
    Ok(conn)
}

async fn dial_result(fixture: &Fixture, user: &str, password: &str, database: &str, ca: &[u8]) -> Result<PgConnection, sqlx::Error> {
    dial_owned(&fixture.host, fixture.port, user, password, database, ca).await
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
