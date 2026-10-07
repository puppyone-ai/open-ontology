//! Two databases on one private PostgreSQL. The fixture applies the fixed
//! target scripts and calls the platform fence functions. The product reads.

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::ClientConfig;
use tokio_postgres::config::SslMode;
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres::Transaction;
use tokio_postgres_rustls::MakeRustlsConnect;

use cdc_offset_store::{hex, sha256, BOOTSTRAP_SHA256, FENCE_SHA256, PREFLIGHT_SHA256, PROVENANCE};

const OWNER: &str = "11111111-1111-4111-8111-111111111111";
const SOURCE: &str = "billing";
const CODEC: &str = "pg-commit-lsn/v1";
const LINEAGE: &str = "alineage";

struct Env {
    host: String,
    port: u16,
    admin_pass: String,
    ca: Vec<u8>,
    ca_file: String,
    migrations: PathBuf,
    counts: PathBuf,
    secrets: PathBuf,
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    value: serde_json::Value,
}

struct RowSpec {
    table: &'static str,
    txn: &'static str,
    pos: &'static str,
    epoch: i32,
    partial: bool,
    cross: bool,
    complete: bool,
    index: Option<i32>,
    count: Option<i32>,
    bitmap: &'static [u8],
    payload: &'static str,
}

#[test]
fn statements_keep_dollar_blocks() {
    let sql = "CREATE ROLE a;\nDO $$ BEGIN PERFORM 1; END $$;\nDO $tag$ BEGIN PERFORM 2; END $tag$;\n";
    let parts = statements(sql);
    assert_eq!(parts.len(), 3, "{parts:?}");
    assert!(parts[1].contains("PERFORM 1;"));
    assert!(parts[2].contains("PERFORM 2;"));
}

#[tokio::test]
async fn live_chain() {
    if std::env::var_os("CDC_OFFSET_HOST").is_none() {
        panic!("CDC_OFFSET_HOST is required");
    }
    let env = load_env();
    let _ = fs::remove_dir_all(&env.secrets);
    fs::create_dir_all(&env.secrets).unwrap();
    let reader_pass = write_secret(&env.secrets, "reader.pass");
    let writer_pass = write_secret(&env.secrets, "writer.pass");
    let runtime_pass = write_secret(&env.secrets, "runtime.pass");
    let control_admin_pass = write_secret(&env.secrets, "control-admin.pass");
    let reader_file = env.secrets.join("reader.pass");
    let runtime_file = env.secrets.join("runtime.pass");
    let files = Files { env: &env, reader: &reader_file, runtime: &runtime_file };
    let control_admin_file = env.secrets.join("control-admin.pass");

    let admin = connect(&env, "postgres", "oo_fixture_admin", &env.admin_pass).await.expect("admin");
    create_role(&admin, "oo_cdc_offset_admin", true, false).await;
    create_role(&admin, "oo_cdc_offset_runtime", false, false).await;
    create_role(&admin, "oo_a310_runtime", true, true).await;
    create_role(&admin, "oo_a310_reader", true, false).await;
    create_role(&admin, "oo_a310_writer", true, true).await;
    set_password(&admin, "oo_cdc_offset_admin", &control_admin_pass).await;
    set_password(&admin, "oo_a310_runtime", &runtime_pass).await;
    set_password(&admin, "oo_a310_reader", &reader_pass).await;
    set_password(&admin, "oo_a310_writer", &writer_pass).await;
    admin
        .batch_execute("GRANT oo_cdc_offset_runtime TO oo_a310_runtime WITH INHERIT TRUE")
        .await
        .expect("runtime membership");
    admin.batch_execute("CREATE DATABASE oo_a310_target").await.expect("target database");
    admin.batch_execute("CREATE DATABASE oo_a310_control OWNER oo_cdc_offset_admin").await.expect("control database");
    admin
        .batch_execute(
            "REVOKE ALL ON DATABASE oo_a310_control FROM PUBLIC; \
             GRANT CONNECT ON DATABASE oo_a310_control TO oo_cdc_offset_admin; \
             GRANT CONNECT ON DATABASE oo_a310_control TO oo_a310_runtime;",
        )
        .await
        .expect("control connect");

    let mut target = connect(&env, "oo_a310_target", "oo_fixture_admin", &env.admin_pass)
        .await
        .expect("target");
    eprintln!("phase=bootstrap");
    apply_file(&mut target, &env.migrations.join("0000_bootstrap_roles.sql"), false, "").await;
    target
        .batch_execute("GRANT oo_pipeline_runtime TO oo_a310_writer WITH INHERIT TRUE")
        .await
        .expect("writer membership");
    let markers = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES \
         ('target-bootstrap/1', '{BOOTSTRAP_SHA256}', 'expand', '0000_bootstrap_roles.sql'), \
         ('target-fence-receipts/1', '{FENCE_SHA256}', 'expand', '0001_target_fence_expand.sql')"
    );
    eprintln!("phase=fence");
    apply_file(&mut target, &env.migrations.join("0001_target_fence_expand.sql"), true, &markers).await;
    target
        .batch_execute(
            "GRANT CONNECT ON DATABASE oo_a310_target TO oo_a310_reader; \
             GRANT USAGE ON SCHEMA _oo TO oo_a310_reader; \
             GRANT SELECT ON _oo.apply_receipts TO oo_a310_reader; \
             GRANT SELECT ON _oo.schema_migrations TO oo_a310_reader;",
        )
        .await
        .expect("reader grants");

    let control = connect(&env, "oo_a310_control", "oo_cdc_offset_admin", &control_admin_pass)
        .await
        .expect("control admin");
    eprintln!("phase=install");
    let installed = run_install(&env, &control_admin_file);
    assert_install(&installed, 0, "installed");
    let replay_install = run_install(&env, &control_admin_file);
    assert_install(&replay_install, 0, "replay");

    eprintln!("phase=migration-drift");
    let drifted = mirror(
        &files,
        "orders",
        "txn-missing",
        "10",
        &["orders"],
        serde_json::Value::Null,
        ("txn-next", "11"),
    );
    assert_out(&drifted, 2, "refused", false, None);
    assert_eq!(drifted.value["fault"], "migration_drift", "{}", drifted.stdout);
    assert_eq!(drifted.value["durable_control_mirror"], false);
    assert_eq!(count_control(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await, 0);

    eprintln!("phase=preflight");
    let v3 = format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES \
         ('target-fence-receipts/3', '{PREFLIGHT_SHA256}', 'expand', '0003_target_segment_preflight_expand.sql')"
    );
    apply_file(
        &mut target,
        &env.migrations.join("0003_target_segment_preflight_expand.sql"),
        true,
        &v3,
    )
    .await;

    eprintln!("phase=least-privilege");
    let reader = connect(&env, "oo_a310_target", "oo_a310_reader", &reader_pass).await.expect("reader");
    assert_denied(&reader, "INSERT INTO _oo.apply_receipts (source) VALUES ('billing')").await;
    assert_denied(&reader, "SELECT count(*) FROM _oo.target_generations").await;
    assert_connect_denied(&env, "oo_a310_control", "oo_a310_reader", &reader_pass).await;
    assert_connect_denied(&env, "oo_a310_target", "oo_a310_runtime", &runtime_pass).await;
    let runtime = connect(&env, "oo_a310_control", "oo_a310_runtime", &runtime_pass).await.expect("runtime");
    control
        .batch_execute(
            "CREATE SCHEMA probe_foreign; REVOKE ALL ON SCHEMA probe_foreign FROM PUBLIC; \
             CREATE TABLE probe_foreign.secret (id integer);",
        )
        .await
        .expect("foreign schema");
    assert_denied(&runtime, "SELECT count(*) FROM probe_foreign.secret").await;
    assert_denied(&runtime, "CREATE TABLE oo_cdc_offset.runtime_denied (id integer)").await;

    eprintln!("phase=absent");
    let before_target = count_target(&target).await;
    let absent = mirror(&files, "orders", "txn-absent", "10", &["orders"], serde_json::Value::Null, ("txn-absent-next", "11"));
    assert_out(&absent, 2, "refused", false, None);
    assert_eq!(absent.value["fault"], "target_receipt_absent", "{}", absent.stdout);
    assert_eq!(count_target(&target).await, before_target);

    let mut writer = connect(&env, "oo_a310_target", "oo_a310_writer", &writer_pass).await.expect("writer");
    let generation: i64 = writer
        .query_one("SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)", &[&"billing", &OWNER])
        .await
        .expect("adopt")
        .get(0);
    assert!(generation >= 1);

    eprintln!("phase=txn-1");
    write_rows(&mut writer, generation, &[complete("orders", "txn-1", "10", 1), complete("lines", "txn-1", "10", 1)]).await;
    let first = mirror(
        &files,
        "orders",
        "txn-1",
        "10",
        &["lines", "orders"],
        serde_json::Value::Null,
        ("txn-2", "20"),
    );
    assert_out(&first, 0, "mirrored", true, None);
    assert_eq!(plain_pos(first.value["watermark_applied_pos"].as_str().unwrap()), "10");
    let lines = mirror(
        &files,
        "lines",
        "txn-1",
        "10",
        &["lines", "orders"],
        serde_json::Value::Null,
        ("txn-2", "20"),
    );
    assert_out(&lines, 0, "mirrored", false, None);
    let hash = first.value["receipt_hash"].as_str().unwrap().to_owned();

    eprintln!("phase=replay");
    let target_before_replay = count_target(&target).await;
    let replay = mirror(
        &files,
        "orders",
        "txn-1",
        "10",
        &["lines", "orders"],
        serde_json::Value::Null,
        ("txn-2", "20"),
    );
    assert_out(&replay, 0, "replay", false, None);
    assert_eq!(replay.value["receipt_hash"], hash);
    assert_eq!(count_target(&target).await, target_before_replay);
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");

    eprintln!("phase=payload-conflict");
    let conflict = conflict_payload(&mut writer, generation).await;
    assert_eq!(conflict, "OO010");
    let still = mirror(
        &files,
        "orders",
        "txn-1",
        "10",
        &["lines", "orders"],
        serde_json::Value::Null,
        ("txn-2", "20"),
    );
    assert_out(&still, 0, "replay", false, None);
    assert_eq!(still.value["receipt_hash"], hash);

    eprintln!("phase=partial");
    write_rows(&mut writer, generation, &[partial("orders", "txn-p", "15", 1)]).await;
    let partial_run = mirror(
        &files,
        "orders",
        "txn-p",
        "15",
        &["orders"],
        serde_json::Value::Null,
        ("txn-p2", "16"),
    );
    assert_out(&partial_run, 0, "mirrored", false, Some("partial_segment"));
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");

    eprintln!("phase=gap");
    write_rows(&mut writer, generation, &[complete("orders", "txn-3", "40", 1), complete("lines", "txn-3", "40", 1)]).await;
    let gap = mirror(
        &files,
        "orders",
        "txn-3",
        "40",
        &["lines", "orders"],
        previous("10", "txn-1", 1),
        ("txn-3b", "41"),
    );
    assert_out(&gap, 0, "mirrored", false, Some("source_gap"));
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "10");
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");

    eprintln!("phase=epoch-barrier");
    write_rows(&mut writer, generation, &[complete("orders", "txn-e2", "100", 2)]).await;
    let barrier = mirror(
        &files,
        "orders",
        "txn-e2",
        "100",
        &["orders"],
        previous("10", "txn-1", 1),
        ("txn-e3", "110"),
    );
    assert_out(&barrier, 0, "mirrored", false, Some("epoch_barrier"));
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");
    assert_eq!(count_control(&control, "SELECT count(*) FROM oo_cdc_offset.watermark WHERE source_epoch = 2").await, 0);
    let epoch_open = mirror(
        &files,
        "orders",
        "txn-e2",
        "100",
        &["orders"],
        serde_json::Value::Null,
        ("txn-e3", "110"),
    );
    assert_out(&epoch_open, 0, "replay", true, None);
    assert_eq!(watermark_txn(&control, 1).await, "txn-1");
    assert_eq!(watermark_txn(&control, 2).await, "txn-e2");

    eprintln!("phase=kill");
    write_rows(&mut writer, generation, &[complete("orders", "txn-2", "20", 1), complete("lines", "txn-2", "20", 1)]).await;
    let killed = kill_mirror(&env, &reader_file, &runtime_file).await;
    assert_out(&killed, 0, "mirrored", true, None);
    assert_eq!(watermark_txn(&control, 1).await, "txn-2");
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "20");
    assert_eq!(
        count_control(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt WHERE txn_or_batch_id = 'txn-2'").await,
        1
    );

    eprintln!("phase=hide-commit");
    write_rows(&mut writer, generation, &[complete("orders", "txn-4", "50", 1), complete("lines", "txn-4", "50", 1)]).await;
    let hidden = run_call(
        &files,
        Call {
            table: "orders",
            txn: "txn-4",
            pos: "50",
            tables: &["lines", "orders"],
            expected: previous("20", "txn-2", 1),
            next_txn: "txn-5",
            next_pos: "60",
            extra: &[("CDC_OFFSET_INJECT", "hide_commit")],
        },
    );
    assert_out(&hidden, 0, "replay", true, None);
    assert_eq!(watermark_txn(&control, 1).await, "txn-4");
    assert_eq!(
        count_control(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt WHERE txn_or_batch_id = 'txn-4'").await,
        1
    );

    eprintln!("phase=commit-unknown");
    let before_unknown = count_control(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await;
    let epoch1_before = watermark_pos(&control, 1).await;
    write_rows(&mut writer, generation, &[partial("orders", "txn-u", "17", 1)]).await;
    let unknown = run_call(
        &files,
        Call {
            table: "orders",
            txn: "txn-u",
            pos: "17",
            tables: &["orders"],
            expected: serde_json::Value::Null,
            next_txn: "txn-u2",
            next_pos: "18",
            extra: &[
                ("CDC_OFFSET_INJECT", "hide_commit"),
                ("CDC_OFFSET_RECONCILE", "down"),
            ],
        },
    );
    assert_out(&unknown, 3, "commit_unknown", false, None);
    assert_eq!(unknown.value["durable_control_mirror"], false);
    let recovered = mirror(
        &files,
        "orders",
        "txn-u",
        "17",
        &["orders"],
        serde_json::Value::Null,
        ("txn-u2", "18"),
    );
    assert_out(&recovered, 0, "replay", false, Some("partial_segment"));
    assert_eq!(
        count_control(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await,
        before_unknown + 1
    );
    assert_eq!(watermark_pos(&control, 1).await, epoch1_before);

    eprintln!("phase=immutable");
    assert_denied(
        &runtime,
        "UPDATE oo_cdc_offset.mirrored_receipt SET receipt_hash = repeat('ab', 32) WHERE txn_or_batch_id = 'txn-1'",
    )
    .await;
    let admin_update = control
        .batch_execute(
            "UPDATE oo_cdc_offset.mirrored_receipt SET receipt_hash = repeat('ab', 32) WHERE txn_or_batch_id = 'txn-1'",
        )
        .await;
    match admin_update {
        Ok(_) => panic!("admin updated an immutable receipt"),
        Err(err) => assert_eq!(sqlstate(&err), "23514", "{}", show(&err)),
    }

    eprintln!("phase=install-drift");
    control
        .batch_execute("CREATE TABLE oo_cdc_offset.drift_probe (id integer)")
        .await
        .expect("drift table");
    let drift = run_install(&env, &control_admin_file);
    assert_install(&drift, 2, "refused");
    assert_eq!(drift.value["fault"], "install_drift", "{}", drift.stdout);
    control.batch_execute("DROP TABLE oo_cdc_offset.drift_probe").await.expect("drop drift");
    let repaired = run_install(&env, &control_admin_file);
    assert_install(&repaired, 0, "replay");

    let receipts = count_control(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await;
    let watermarks = count_control(&control, "SELECT count(*) FROM oo_cdc_offset.watermark").await;
    assert!(receipts >= 8, "receipts={receipts}");
    assert_eq!(watermarks, 2);
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), "50");
    assert_eq!(watermark_txn(&control, 1).await, "txn-4");
    assert_eq!(plain_pos(&watermark_pos(&control, 2).await), "100");
    let body = serde_json::json!({
        "receipts": receipts,
        "watermarks": watermarks,
        "epoch1_pos": plain_pos(&watermark_pos(&control, 1).await),
        "epoch1_txn": watermark_txn(&control, 1).await,
        "epoch2_pos": plain_pos(&watermark_pos(&control, 2).await),
        "epoch2_txn": watermark_txn(&control, 2).await,
    });
    fs::write(&env.counts, serde_json::to_vec(&body).unwrap()).unwrap();
    eprintln!("phase=counts {}", body);
}

#[tokio::test]
async fn restart_counts() {
    if std::env::var_os("CDC_OFFSET_RESTART").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let env = load_env();
    let recorded: serde_json::Value = serde_json::from_slice(&fs::read(&env.counts).expect("counts")).unwrap();
    let admin_pass = env.admin_pass.clone();
    let control = connect(&env, "oo_a310_control", "oo_fixture_admin", &admin_pass).await.expect("control");
    let receipts = count_control(&control, "SELECT count(*) FROM oo_cdc_offset.mirrored_receipt").await;
    let watermarks = count_control(&control, "SELECT count(*) FROM oo_cdc_offset.watermark").await;
    assert_eq!(receipts, recorded["receipts"].as_i64().unwrap(), "{recorded}");
    assert_eq!(watermarks, recorded["watermarks"].as_i64().unwrap(), "{recorded}");
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), recorded["epoch1_pos"].as_str().unwrap());
    assert_eq!(watermark_txn(&control, 1).await, recorded["epoch1_txn"].as_str().unwrap());
    assert_eq!(plain_pos(&watermark_pos(&control, 2).await), recorded["epoch2_pos"].as_str().unwrap());
    let reader_file = env.secrets.join("reader.pass");
    let runtime_file = env.secrets.join("runtime.pass");
    let files = Files { env: &env, reader: &reader_file, runtime: &runtime_file };
    let replay = mirror(
        &files,
        "orders",
        "txn-1",
        "10",
        &["lines", "orders"],
        serde_json::Value::Null,
        ("txn-2", "20"),
    );
    assert_out(&replay, 0, "replay", false, None);
    assert_eq!(plain_pos(&watermark_pos(&control, 1).await), recorded["epoch1_pos"].as_str().unwrap());
    assert_eq!(watermark_txn(&control, 1).await, recorded["epoch1_txn"].as_str().unwrap());
    eprintln!("restart_counts_ok");
}

fn load_env() -> Env {
    let host = std::env::var("CDC_OFFSET_HOST").unwrap();
    let port = std::env::var("CDC_OFFSET_PORT").unwrap().parse().unwrap();
    let admin_pass = fs::read_to_string(std::env::var("CDC_OFFSET_ADMIN_PASS_FILE").unwrap()).unwrap();
    let ca_file = std::env::var("CDC_OFFSET_CA_FILE").unwrap();
    let ca = fs::read(&ca_file).unwrap();
    Env {
        host,
        port,
        admin_pass: admin_pass.trim().to_owned(),
        ca,
        ca_file,
        migrations: PathBuf::from(std::env::var("CDC_OFFSET_MIGRATIONS").unwrap()),
        counts: PathBuf::from(std::env::var("CDC_OFFSET_COUNTS").unwrap()),
        secrets: PathBuf::from(std::env::var("CDC_OFFSET_SECRET_DIR").unwrap()),
    }
}

fn write_secret(dir: &Path, name: &str) -> String {
    let mut bytes = [0_u8; 24];
    fs::File::open("/dev/urandom").unwrap().read_exact(&mut bytes).unwrap();
    let text = hex(&bytes);
    let path = dir.join(name);
    fs::write(&path, &text).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    text
}

fn statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut dollar: Option<Vec<u8>> = None;
    while index < bytes.len() {
        if let Some(tag) = &dollar {
            if bytes[index..].starts_with(tag) {
                index += tag.len();
                dollar = None;
                continue;
            }
            index += 1;
            continue;
        }
        if bytes[index] == b'$'
            && let Some(end) = bytes[index + 1..].iter().position(|byte| *byte == b'$')
        {
            let end = index + 1 + end;
            let inner = &bytes[index + 1..end];
            if inner.iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_') {
                dollar = Some(bytes[index..=end].to_vec());
                index = end + 1;
                continue;
            }
        }
        if bytes[index] == b';' {
            push_statement(&mut out, &bytes[start..index]);
            start = index + 1;
        }
        index += 1;
    }
    push_statement(&mut out, &bytes[start..]);
    out
}

fn push_statement(out: &mut Vec<String>, bytes: &[u8]) {
    let text = std::str::from_utf8(bytes).unwrap().trim();
    if text.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with("--")
    }) {
        out.push(text.to_owned());
    }
}

async fn apply_file(client: &mut Client, path: &Path, locked: bool, markers: &str) {
    let sql = fs::read_to_string(path).unwrap_or_else(|_| panic!("missing {}", path.display()));
    if !locked {
        for statement in statements(&sql) {
            client.batch_execute(&statement).await.unwrap_or_else(|err| panic!("{} {}", path.display(), show(&err)));
        }
        return;
    }
    let tx = client.transaction().await.unwrap();
    tx.batch_execute(
        "SET LOCAL lock_timeout = '15s'; SET LOCAL statement_timeout = '60s'; \
         SET LOCAL ROLE oo_target_owner; SELECT pg_catalog.pg_advisory_xact_lock(6404202605)",
    )
    .await
    .unwrap_or_else(|err| panic!("lock {}", show(&err)));
    tx.batch_execute(&sql).await.unwrap_or_else(|err| panic!("{} {}", path.display(), show(&err)));
    if !markers.is_empty() {
        tx.batch_execute(markers).await.unwrap_or_else(|err| panic!("marker {}", show(&err)));
    }
    tx.commit().await.unwrap_or_else(|err| panic!("commit {}", show(&err)));
}

async fn create_role(client: &Client, name: &str, login: bool, inherit: bool) {
    let login = if login { "LOGIN" } else { "NOLOGIN" };
    let inherit = if inherit { "INHERIT" } else { "NOINHERIT" };
    let sql = format!(
        "CREATE ROLE {name} {login} {inherit} NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS"
    );
    client.batch_execute(&sql).await.unwrap_or_else(|err| panic!("{name} {}", show(&err)));
}

async fn set_password(client: &Client, role: &str, password: &str) {
    let template = format!("ALTER ROLE {role} PASSWORD %L");
    let sql: String = client
        .query_one("SELECT format($1::text, $2::text)", &[&template, &password])
        .await
        .unwrap_or_else(|err| panic!("{}", show(&err)))
        .get(0);
    client.batch_execute(&sql).await.unwrap_or_else(|err| panic!("{}", show(&err)));
}

async fn connect(env: &Env, database: &str, user: &str, password: &str) -> Result<Client, tokio_postgres::Error> {
    let certificate = CertificateDer::from_pem_slice(&env.ca).expect("ca");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).expect("ca");
    let config = ClientConfig::builder_with_provider(rustls::crypto::aws_lc_rs::default_provider().into())
        .with_safe_default_protocol_versions()
        .expect("tls")
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tls = MakeRustlsConnect::new(config);
    let mut builder = Config::new();
    builder
        .host(&env.host)
        .port(env.port)
        .dbname(database)
        .user(user)
        .password(password)
        .ssl_mode(SslMode::Require)
        .connect_timeout(Duration::from_secs(20));
    let (client, task) = builder.connect(tls).await?;
    tokio::spawn(async move {
        let _ = task.await;
    });
    Ok(client)
}

async fn assert_connect_denied(env: &Env, database: &str, user: &str, password: &str) {
    match connect(env, database, user, password).await {
        Ok(_) => panic!("{user} connected to {database}"),
        Err(err) => assert_eq!(sqlstate(&err), "42501", "{}", show(&err)),
    }
}

async fn assert_denied(client: &Client, sql: &str) {
    match client.batch_execute(sql).await {
        Ok(_) => panic!("allowed: {sql}"),
        Err(err) => assert_eq!(sqlstate(&err), "42501", "{} {}", show(&err), sql),
    }
}

fn sqlstate(err: &tokio_postgres::Error) -> String {
    err.code().map(|code| code.code().to_owned()).unwrap_or_default()
}

fn show(err: &tokio_postgres::Error) -> String {
    match err.as_db_error() {
        Some(db) => format!("{} {}", db.code().code(), db.message()),
        None => err.to_string(),
    }
}

async fn count_target(client: &Client) -> i64 {
    client.query_one("SELECT count(*) FROM _oo.apply_receipts", &[]).await.unwrap().get(0)
}

async fn count_control(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap_or_else(|err| panic!("{} {}", sql, show(&err))).get(0)
}

async fn watermark_pos(client: &Client, epoch: i32) -> String {
    client
        .query_one(
            "SELECT applied_pos::text FROM oo_cdc_offset.watermark WHERE source = $1 AND source_epoch = $2",
            &[&SOURCE, &epoch],
        )
        .await
        .unwrap_or_else(|err| panic!("{}", show(&err)))
        .get(0)
}

async fn watermark_txn(client: &Client, epoch: i32) -> String {
    client
        .query_one(
            "SELECT applied_txn_id FROM oo_cdc_offset.watermark WHERE source = $1 AND source_epoch = $2",
            &[&SOURCE, &epoch],
        )
        .await
        .unwrap_or_else(|err| panic!("{}", show(&err)))
        .get(0)
}

fn plain_pos(value: &str) -> String {
    let value = value.trim();
    let whole = match value.split_once('.') {
        Some((whole, fraction)) if fraction.chars().all(|ch| ch == '0') => whole,
        _ => value,
    };
    let trimmed = whole.trim_start_matches('0');
    if trimmed.is_empty() { "0".to_owned() } else { trimmed.to_owned() }
}

fn complete(table: &'static str, txn: &'static str, pos: &'static str, epoch: i32) -> RowSpec {
    RowSpec {
        table,
        txn,
        pos,
        epoch,
        partial: false,
        cross: true,
        complete: true,
        index: None,
        count: None,
        bitmap: &[],
        payload: "payload-a",
    }
}

fn partial(table: &'static str, txn: &'static str, pos: &'static str, epoch: i32) -> RowSpec {
    RowSpec {
        table,
        txn,
        pos,
        epoch,
        partial: true,
        cross: false,
        complete: false,
        index: Some(0),
        count: Some(2),
        bitmap: &[0x01],
        payload: "payload-a",
    }
}

async fn write_rows(writer: &mut Client, generation: i64, rows: &[RowSpec]) {
    let tx = writer.transaction().await.unwrap();
    for row in rows {
        let status = insert_receipt(&tx, generation, row, row.payload).await.unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(status, "inserted", "{}", row.txn);
    }
    tx.commit().await.unwrap();
}

async fn conflict_payload(writer: &mut Client, generation: i64) -> String {
    let tx = writer.transaction().await.unwrap();
    let row = complete("orders", "txn-1", "10", 1);
    let err = insert_receipt(&tx, generation, &row, "payload-b").await.unwrap_err();
    let _ = tx.rollback().await;
    err
}

async fn insert_receipt(tx: &Transaction<'_>, generation: i64, row: &RowSpec, payload: &str) -> Result<String, String> {
    let config = digest("config");
    let type_digest = digest("type");
    let hook = digest("hook");
    let schema = digest("schema");
    let payload_digest = digest(payload);
    let index = row.index;
    let count = row.count;
    let bitmap = row.bitmap.to_vec();
    let partial = row.partial;
    let cross = row.cross;
    let complete = row.complete;
    let epoch = row.epoch;
    let params: [&(dyn ToSql + Sync); 21] = [
        &"billing",
        &OWNER,
        &generation,
        &SOURCE,
        &row.table,
        &row.txn,
        &config,
        &type_digest,
        &hook,
        &schema,
        &payload_digest,
        &CODEC,
        &LINEAGE,
        &epoch,
        &row.pos,
        &partial,
        &cross,
        &complete,
        &index,
        &count,
        &bitmap,
    ];
    tx.query_one(
        "SELECT _oo.commit_apply_receipt($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, ($15::text)::numeric, $16, $17, $18, $19, $20, $21)",
        &params,
    )
    .await
    .map(|row| row.get(0))
    .map_err(|err| sqlstate(&err))
}

fn digest(label: &str) -> String {
    hex(&sha256(label.as_bytes()).unwrap())
}

fn previous(pos: &str, txn: &str, epoch: i32) -> serde_json::Value {
    serde_json::json!({
        "position_codec": CODEC,
        "position_lineage": LINEAGE,
        "source_epoch": epoch,
        "applied_pos": pos,
        "txn_or_batch_id": txn,
    })
}

struct Files<'a> {
    env: &'a Env,
    reader: &'a Path,
    runtime: &'a Path,
}

struct Call<'a> {
    table: &'a str,
    txn: &'a str,
    pos: &'a str,
    tables: &'a [&'a str],
    expected: serde_json::Value,
    next_txn: &'a str,
    next_pos: &'a str,
    extra: &'a [(&'a str, &'a str)],
}

fn mirror(
    files: &Files<'_>,
    table: &str,
    txn: &str,
    pos: &str,
    tables: &[&str],
    expected: serde_json::Value,
    next: (&str, &str),
) -> Run {
    run_call(
        files,
        Call {
            table,
            txn,
            pos,
            tables,
            expected,
            next_txn: next.0,
            next_pos: next.1,
            extra: &[],
        },
    )
}

fn run_call(files: &Files<'_>, call: Call<'_>) -> Run {
    let body = serde_json::json!({
        "contract": "cdc-offset-store-v1",
        "target": endpoint(files.env, "oo_a310_target", "oo_a310_reader", files.reader),
        "control": endpoint(files.env, "oo_a310_control", "oo_a310_runtime", files.runtime),
        "source": SOURCE,
        "table_generation": call.table,
        "txn_or_batch_id": call.txn,
        "order_binding": {
            "provenance": PROVENANCE,
            "expected_previous": call.expected,
            "declared_transaction": {
                "txn_or_batch_id": call.txn,
                "applied_pos": call.pos,
                "required_table_generations": call.tables,
            },
            "declared_next": {"txn_or_batch_id": call.next_txn, "applied_pos": call.next_pos},
            "source_gap": false,
            "epoch_barrier": false,
        }
    });
    run_cli("mirror", &body, call.extra)
}

fn run_install(env: &Env, admin_file: &Path) -> Run {
    let body = serde_json::json!({
        "contract": "cdc-offset-store-v1",
        "control": endpoint(env, "oo_a310_control", "oo_cdc_offset_admin", admin_file),
    });
    run_cli("install", &body, &[])
}

fn endpoint(env: &Env, database: &str, user: &str, password_file: &Path) -> serde_json::Value {
    serde_json::json!({
        "host": env.host,
        "port": env.port,
        "database": database,
        "user": user,
        "password_file": password_file,
        "ca_file": env.ca_file,
        "server_name": env.host,
    })
}

fn run_cli(command: &str, body: &serde_json::Value, extra: &[(&str, &str)]) -> Run {
    let path = std::env::temp_dir().join(format!("cdc-offset-{command}-{}.json", std::process::id()));
    fs::write(&path, serde_json::to_vec(body).unwrap()).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cdc-offset-store"));
    cmd.arg(command).arg(&path);
    for key in ["CDC_OFFSET_INJECT", "CDC_OFFSET_RECONCILE", "CDC_OFFSET_CHECKPOINT", "CDC_OFFSET_MARK"] {
        cmd.env_remove(key);
    }
    for (key, value) in extra {
        cmd.env(key, value);
    }
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let value = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    Run { code: output.status.code(), stdout, stderr, value }
}

fn assert_install(run: &Run, code: i32, kind: &str) {
    assert_eq!(run.code, Some(code), "{}\n{}", run.stdout, run.stderr);
    assert_eq!(run.value["kind"], kind, "{}", run.stdout);
    assert_eq!(run.value["contract"], "cdc-offset-store-v1", "{}", run.stdout);
    assert_eq!(run.value["target_touched"], false, "{}", run.stdout);
}

fn assert_out(run: &Run, code: i32, kind: &str, advanced: bool, hold: Option<&str>) {
    assert_eq!(run.code, Some(code), "{}\n{}", run.stdout, run.stderr);
    assert_eq!(run.value["kind"], kind, "{}", run.stdout);
    assert_eq!(run.value["contract"], "cdc-offset-store-v1", "{}", run.stdout);
    assert_eq!(run.value["checkpoint_provenance"], PROVENANCE, "{}", run.stdout);
    for key in [
        "source_applied",
        "fence_proof",
        "ack_allowed",
        "external_xack",
        "external_xtrim",
        "routing_verified",
        "real_source_provenance_verified",
        "debezium_normalizer",
    ] {
        assert_eq!(run.value[key], false, "{key} {}", run.stdout);
    }
    if kind == "mirrored" || kind == "replay" {
        assert_eq!(run.value["checkpoint_advanced"], advanced, "{}", run.stdout);
        assert_eq!(run.value["durable_control_mirror"], true, "{}", run.stdout);
    }
    if let Some(hold) = hold {
        assert_eq!(run.value["watermark_hold"], hold, "{}", run.stdout);
    }
}

async fn kill_mirror(env: &Env, reader_file: &Path, runtime_file: &Path) -> Run {
    let body = serde_json::json!({
        "contract": "cdc-offset-store-v1",
        "target": endpoint(env, "oo_a310_target", "oo_a310_reader", reader_file),
        "control": endpoint(env, "oo_a310_control", "oo_a310_runtime", runtime_file),
        "source": SOURCE,
        "table_generation": "orders",
        "txn_or_batch_id": "txn-2",
        "order_binding": {
            "provenance": PROVENANCE,
            "expected_previous": previous("10", "txn-1", 1),
            "declared_transaction": {
                "txn_or_batch_id": "txn-2",
                "applied_pos": "20",
                "required_table_generations": ["lines", "orders"],
            },
            "declared_next": {"txn_or_batch_id": "txn-4", "applied_pos": "50"},
            "source_gap": false,
            "epoch_barrier": false,
        }
    });
    let path = std::env::temp_dir().join("cdc-offset-kill.json");
    fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
    let mark = std::env::temp_dir().join("cdc-offset-mark");
    let _ = fs::remove_file(&mark);
    let mut child = Command::new(env!("CARGO_BIN_EXE_cdc-offset-store"))
        .arg("mirror")
        .arg(&path)
        .env("CDC_OFFSET_CHECKPOINT", "before_commit")
        .env("CDC_OFFSET_MARK", &mark)
        .env_remove("CDC_OFFSET_INJECT")
        .env_remove("CDC_OFFSET_RECONCILE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = wait_pid(&mark, &mut child).await;
    let killer = connect(env, "postgres", "oo_fixture_admin", &env.admin_pass).await.expect("killer");
    let killed: bool = killer
        .query_one("SELECT pg_catalog.pg_terminate_backend($1)", &[&pid])
        .await
        .unwrap_or_else(|err| panic!("{}", show(&err)))
        .get(0);
    assert!(killed, "backend {pid} was not terminated");
    let output = tokio::task::spawn_blocking(move || child.wait_with_output()).await.unwrap().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let value = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    Run { code: output.status.code(), stdout, stderr, value }
}

async fn wait_pid(path: &Path, child: &mut std::process::Child) -> i32 {
    for _ in 0..400 {
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse::<i32>()
        {
            return pid;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("mirror exited {status} before the checkpoint mark");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("checkpoint mark was not written");
}
