//! Operator install of one role across two mapped tables.
//!
//! Offline cases refuse a bad document before a connection. The live case runs
//! when `OO_A235_LIVE=1`. It applies frozen target scripts `0000`, `0001`,
//! `0003`, `0004`, and `0005` itself. It does not link `oo-pipeline` and does
//! not apply `0002`. Passwords stay in files. The command under test does not
//! grant `physical_mappings`, does not `ANALYZE`, and does not write an audit file.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use oo_gateway::{connect_require_tls, TlsEndpoint};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const PROTOCOL: &str = "oo.gateway.install-column-acl-cli/v1";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const HOST: &str = "oo-a235-pg";
const DATABASE: &str = "oo_a235";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a235_login";
const SECRET_DIR: &str = "/run/oo-a235/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a2350000-0000-4000-8000-000000000235";
const SHARD: &str = "shard_a235";
const SOURCE: &str = "trade";
const CONFIG: &str = "a235a235a235a235a235a235a235a235a235a235a235a235a235a235a235a235";
const TYPE_DIGEST: &str = "b235b235b235b235b235b235b235b235b235b235b235b235b235b235b235b235";
const INPUT_DIGEST: &str = "c235c235c235c235c235c235c235c235c235c235c235c235c235c235c235c235";
const SCHEMA_DIGEST: &str = "d235d235d235d235d235d235d235d235d235d235d235d235d235d235d235d235";
const NAMESPACE: &str = "trade";
const PARTY: &str = "party";
const LEDGER: &str = "ledger";
const STALE_GENERATION: i64 = 3;
const CURRENT_GENERATION: i64 = 7;

#[test]
fn usage_exits_2() {
    let output = run_cli_blocking(b"{}", &["secret-arg-token"]);
    assert_line(&output, 2, "install-column-acl: usage");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("secret-arg-token"));
}

#[test]
fn malformed_json_is_scan() {
    let output = run_cli_blocking(b"{", &[]);
    assert_line(&output, 4, "install-column-acl: scan");
}

#[test]
fn overlong_stdin_is_budget() {
    let bytes = vec![b'a'; 1_048_577];
    let output = run_cli_blocking(&bytes, &[]);
    assert_line(&output, 3, "install-column-acl: budget");
}

#[test]
fn wrong_protocol_is_request() {
    let mut body = base_doc();
    body["protocol"] = json!("oo.gateway.reviewed-policy-cli/v1");
    body["policies"] = json!([sample_policy(Value::Null)]);
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "install-column-acl: request");
}

#[test]
fn unknown_field_is_request() {
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["audit_file"] = json!("/tmp/audit.jsonl");
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "install-column-acl: request");
}

#[test]
fn row_filter_is_policy_filter() {
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(json!({
        "kind": "claims",
        "column": "id",
        "claim": "party"
    }))]);
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 7, "install-column-acl: policy filter");
}

#[test]
fn empty_set_is_policy_empty() {
    let output = run_cli_blocking(&serde_json::to_vec(&base_doc()).expect("json"), &[]);
    assert_line(&output, 7, "install-column-acl: policy empty");
}

#[test]
fn missing_password_file_is_request() {
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["password_file"] = json!("/no/such/install-column-acl-password");
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "install-column-acl: request");
}

#[test]
fn password_inside_document_is_request() {
    let dir = TempDir::new("password-echo");
    let password = dir.path.join("password");
    fs::write(&password, "shop\n").expect("password");
    let ca = dir.path.join("ca.pem");
    fs::write(&ca, b"not-a-key").expect("ca");
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["password_file"] = json!(password.display().to_string());
    body["ca_file"] = json!(ca.display().to_string());
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "install-column-acl: request");
}

#[test]
fn private_key_ca_is_tls() {
    let dir = TempDir::new("private-key");
    let password = dir.path.join("password");
    fs::write(&password, "Z9\n").expect("password");
    let ca = dir.path.join("ca.pem");
    fs::write(&ca, b"-----BEGIN PRIVATE KEY-----\n").expect("ca");
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["password_file"] = json!(password.display().to_string());
    body["ca_file"] = json!(ca.display().to_string());
    let text = serde_json::to_vec(&body).expect("json");
    assert!(!text.windows(2).any(|window| window == b"Z9"));
    let output = run_cli_blocking(&text, &[]);
    assert_line(&output, 6, "install-column-acl: tls");
}

#[test]
fn symlink_password_is_request() {
    let dir = TempDir::new("symlink");
    let real = dir.path.join("real");
    fs::write(&real, "Z9\n").expect("real");
    let link = dir.path.join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let mut body = base_doc();
    body["policies"] = json!([sample_policy(Value::Null)]);
    body["password_file"] = json!(link.display().to_string());
    let output = run_cli_blocking(&serde_json::to_vec(&body).expect("json"), &[]);
    assert_line(&output, 5, "install-column-acl: request");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_tls_two_tables_two_roles() {
    if env::var("OO_A235_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    assert_eq!(CONFIG.len(), 64);
    let admin_pass = read_hex("admin.pass");
    let login_pass = read_hex("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let bans = vec![admin_pass.clone(), login_pass.clone()];
    let mut oracle = Oracle::new(&bans);

    reset_database(&admin_pass, &ca).await;
    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, LOGIN, &login_pass).await;
    let shadow = prepare_shadow(&mut admin).await;
    assert_eq!(
        max_generation(&admin, CONFIG).await,
        Some(CURRENT_GENERATION)
    );

    let before = role_count(&admin, "oo_r_").await;
    let scan = run_cli(b"{".to_vec()).await;
    assert_line(&scan, 4, "install-column-acl: scan");
    assert_quiet(&scan.stderr, &bans, "scan stderr");
    let malformed = run_cli(
        br#"{"protocol":"oo.gateway.install-column-acl-cli/v1"}"#.to_vec(),
    )
    .await;
    assert_line(&malformed, 5, "install-column-acl: request");
    assert_eq!(role_count(&admin, "oo_r_").await, before);
    oracle.fact("malformed_scan", "scan");
    oracle.fact("malformed_request", "request");
    oracle.fact("malformed_roles", "unchanged");

    let stale = run_cli(live_doc(
        "stale_gen",
        STALE_GENERATION,
        &[("party", &["party_id"]), ("ledger", &["entry_id"])],
    ))
    .await;
    assert_line(&stale, 7, "install-column-acl: policy stale");
    assert_quiet(&stale.stdout, &bans, "stale stdout");
    assert_quiet(&stale.stderr, &bans, "stale stderr");
    assert!(!role_present(&admin, "oo_r_stale_gen").await);
    oracle.fact("stale", "policy stale");
    oracle.fact("stale_role", "absent");

    let bad = run_cli(live_doc(
        "bad_map",
        CURRENT_GENERATION,
        &[
            ("party", &["party_id", "display_name", "note"]),
            ("missing_table", &["entry_id"]),
        ],
    ))
    .await;
    assert_line(&bad, 7, "install-column-acl: policy mapping");
    assert!(!role_present(&admin, "oo_r_bad_map").await);
    assert_eq!(column_grant_count(&admin, "oo_r_bad_map").await, 0);
    oracle.fact("bad_second", "policy mapping");
    oracle.fact("bad_second_role", "absent");

    let clerk = run_cli(live_doc(
        "clerk",
        CURRENT_GENERATION,
        &[
            ("party", &["party_id", "display_name", "note"]),
            ("ledger", &["entry_id", "party_id", "memo"]),
        ],
    ))
    .await;
    let clerk_body = assert_ok(&clerk, &bans);
    assert!(clerk.stdout.starts_with(
        br#"{"protocol":"oo.gateway.install-column-acl-cli/v1","effect":"installed""#
    ));
    assert_eq!(clerk_body["effect"], "installed");
    assert_eq!(clerk_body["role"], "oo_r_clerk");
    assert_eq!(clerk_body["grantor"], ADMIN);
    assert_eq!(clerk_body["tables"][0]["physical_table"], shadow.party.table);
    assert_eq!(clerk_body["tables"][1]["physical_table"], shadow.ledger.table);
    assert_eq!(
        strings(&clerk_body["tables"][1]["physical_columns"]),
        vec![
            shadow.ledger.entry_id.clone(),
            shadow.ledger.party_id.clone(),
            shadow.ledger.memo.clone(),
        ]
    );
    assert_eq!(
        strings(&clerk_body["tables"][0]["physical_columns"]),
        vec![
            shadow.party.party_id.clone(),
            shadow.party.display_name.clone(),
            shadow.party.note.clone(),
        ]
    );
    assert!(!strings(&clerk_body["tables"][0]["physical_columns"]).contains(&shadow.party.ratio));
    assert!(!clerk.stdout.windows(5).any(|window| window == b"audit"));
    oracle.fact("clerk_effect", "installed");
    oracle.fact("grantor", ADMIN);

    let peek = run_cli(live_doc(
        "peek",
        CURRENT_GENERATION,
        &[
            ("party", &["party_id", "display_name"]),
            ("ledger", &["entry_id", "party_id"]),
        ],
    ))
    .await;
    let peek_body = assert_ok(&peek, &bans);
    assert_eq!(peek_body["effect"], "installed");
    assert_eq!(peek_body["role"], "oo_r_peek");
    oracle.fact("peek_effect", "installed");

    let replay = run_cli(live_doc(
        "clerk",
        CURRENT_GENERATION,
        &[
            ("party", &["party_id", "display_name", "note"]),
            ("ledger", &["entry_id", "party_id", "memo"]),
        ],
    ))
    .await;
    let replay_body = assert_ok(&replay, &bans);
    assert!(replay.stdout.starts_with(
        br#"{"protocol":"oo.gateway.install-column-acl-cli/v1","effect":"replay""#
    ));
    assert_eq!(replay_body["tables"], clerk_body["tables"]);
    oracle.fact("clerk_replay", "replay");
    oracle.fact("replay_tables", "same");

    assert_eq!(column_grant_count(&admin, "oo_r_clerk").await, 6);
    assert_eq!(column_grant_count(&admin, "oo_r_peek").await, 4);
    assert_eq!(table_privilege_count(&admin, "oo_r_clerk").await, 0);
    assert_eq!(table_privilege_count(&admin, "oo_r_peek").await, 0);
    assert_eq!(mapping_privilege_count(&admin).await, 0);
    assert_eq!(publication_count(&admin).await, 0);
    assert!(reltuples_unknown(&admin, &shadow.party.table).await);
    assert_role_shape(&admin, "oo_r_clerk").await;
    assert_role_shape(&admin, "oo_r_peek").await;
    assert_eq!(
        select_columns(&admin, "oo_r_clerk", &shadow.party.table).await,
        BTreeSet::from([
            shadow.party.party_id.clone(),
            shadow.party.display_name.clone(),
            shadow.party.note.clone(),
        ])
    );
    oracle.fact("column_grants_clerk", "6");
    oracle.fact("column_grants_peek", "4");
    oracle.fact("table_privileges", "0");
    oracle.fact("mapping_select", "0");
    oracle.fact("publication", "0");
    oracle.fact("role_shape", "nologin");
    oracle.fact("audit_wal", "absent");
    oracle.fact("authentication", "not_claimed");

    let mut reader = connect_db(DATABASE, LOGIN, &login_pass, &ca).await;
    let party = quote_physical(&shadow.party.table, "pt_");
    let ledger = quote_physical(&shadow.ledger.table, "pt_");
    let party_id = quote_physical(&shadow.party.party_id, "pc_");
    let memo = quote_physical(&shadow.ledger.memo, "pc_");
    let allowed = query_as_role(
        &mut reader,
        "oo_r_clerk",
        &format!("SELECT {party_id}::int4 FROM \"_oo\".{party} ORDER BY {party_id}::int4 LIMIT 1"),
    )
    .await;
    assert_eq!(allowed.get::<_, i32>(0), 1);
    oracle.fact("query_party", "1");
    expect_42501(
        &mut reader,
        Some("oo_r_clerk"),
        &format!("INSERT INTO \"_oo\".{party} (\"_oo_pos\") VALUES (9)"),
    )
    .await;
    expect_42501(
        &mut reader,
        Some("oo_r_clerk"),
        &format!("UPDATE \"_oo\".{ledger} SET {memo} = 'rewritten'"),
    )
    .await;
    oracle.fact("hidden_write", "42501");
    assert_eq!(publication_count(&admin).await, 0);
    oracle.flush();
}

fn bin_path() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_install-column-acl") {
        return PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_install_column_acl") {
        return PathBuf::from(path);
    }
    panic!("install-column-acl binary path is missing");
}

fn run_cli_blocking(bytes: &[u8], args: &[&str]) -> Output {
    let mut command = Command::new(bin_path());
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("PGPASSWORD")
        .env_remove("PGPASSFILE");
    let mut child = command.spawn().expect("spawn install-column-acl");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = stdin.write_all(bytes);
    }
    child
        .wait_with_output()
        .expect("wait install-column-acl")
}

async fn run_cli(bytes: Vec<u8>) -> Output {
    tokio::task::spawn_blocking(move || run_cli_blocking(&bytes, &[]))
        .await
        .expect("cli join")
}

fn assert_line(output: &Output, code: i32, line: &str) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "stdout was not empty");
    assert_eq!(output.stderr, format!("{line}\n").as_bytes());
}

fn assert_ok(output: &Output, bans: &[String]) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.ends_with(b"\n"));
    assert_quiet(&output.stdout, bans, "stdout");
    assert_quiet(&output.stderr, bans, "stderr");
    serde_json::from_slice(&output.stdout).expect("outcome")
}

fn assert_quiet(bytes: &[u8], bans: &[String], label: &str) {
    let text = String::from_utf8_lossy(bytes);
    for ban in bans {
        assert!(!text.contains(ban.as_str()), "{label} contained a secret");
    }
}

fn base_doc() -> Value {
    json!({
        "protocol": PROTOCOL,
        "policies": [],
        "endpoint": {
            "host": "db",
            "port": 5432,
            "database": "oo_a235",
            "user": "oo_fixture_admin"
        },
        "password_file": "pw",
        "ca_file": "ca"
    })
}

fn sample_policy(row_filter: Value) -> Value {
    json!({
        "project": "shop",
        "config_digest": DIGEST,
        "generation": 1,
        "role": "clerk",
        "login": "clerk_login",
        "table": {
            "namespace": "public",
            "name": "customer",
            "columns": [{
                "name": "id",
                "status": "confirmed",
                "semantic_type": "identifier",
                "pii": {"class": "none"}
            }]
        },
        "row_filter": row_filter
    })
}

fn live_doc(role: &str, generation: i64, tables: &[(&str, &[&str])]) -> Vec<u8> {
    let policies: Vec<Value> = tables
        .iter()
        .map(|(table, columns)| {
            json!({
                "project": SOURCE,
                "config_digest": CONFIG,
                "generation": generation,
                "role": role,
                "login": LOGIN,
                "table": {
                    "namespace": NAMESPACE,
                    "name": table,
                    "columns": columns.iter().map(|name| json!({
                        "name": name,
                        "status": "confirmed",
                        "semantic_type": "identifier",
                        "pii": {"class": "none"}
                    })).collect::<Vec<_>>()
                },
                "row_filter": Value::Null
            })
        })
        .collect();
    serde_json::to_vec(&json!({
        "protocol": PROTOCOL,
        "policies": policies,
        "endpoint": {
            "host": HOST,
            "port": 5432,
            "database": DATABASE,
            "user": ADMIN
        },
        "password_file": format!("{SECRET_DIR}/admin.pass"),
        "ca_file": format!("{SECRET_DIR}/ca.crt")
    }))
    .expect("live request")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("oo-a235-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn read_hex(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    let value = raw.trim();
    let hex = (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    assert!(hex, "{name} is not lowercase hex");
    value.to_string()
}

struct Oracle {
    lines: Vec<String>,
    bans: Vec<String>,
}

impl Oracle {
    fn new(bans: &[String]) -> Self {
        Self {
            lines: Vec::new(),
            bans: bans.to_vec(),
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        assert_quiet(value.as_bytes(), &self.bans, key);
        self.lines.push(format!("{key}={value}"));
        self.flush();
    }

    fn flush(&self) {
        let path = Path::new(ORACLE_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(path, self.lines.join("\n") + "\n").expect("oracle write");
    }
}

async fn reset_database(admin_pass: &str, ca: &[u8]) {
    let maintenance = connect_db("postgres", ADMIN, admin_pass, ca).await;
    expect_sql(
        &maintenance,
        "terminate database sessions",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    expect_sql(
        &maintenance,
        "drop database",
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    expect_sql(
        &maintenance,
        "drop fixture roles",
        "DO $$\n\
         DECLARE granted_name text;\n\
         DECLARE member_name text;\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           FOR granted_name, member_name IN\n\
             SELECT granted.rolname, member_role.rolname\n\
             FROM pg_catalog.pg_auth_members AS membership\n\
             JOIN pg_catalog.pg_roles AS granted ON granted.oid = membership.roleid\n\
             JOIN pg_catalog.pg_roles AS member_role ON member_role.oid = membership.member\n\
             WHERE granted.rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
                OR member_role.rolname = 'oo_a235_login'\n\
           LOOP\n\
             EXECUTE format('REVOKE %I FROM %I', granted_name, member_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a235_login') THEN\n\
             DROP ROLE oo_a235_login;\n\
           END IF;\n\
           FOR role_name IN\n\
             SELECT rolname FROM pg_catalog.pg_roles\n\
             WHERE rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
           LOOP\n\
             EXECUTE format('DROP ROLE %I', role_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN\n\
             DROP ROLE oo_pipeline_runtime;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') THEN\n\
             DROP ROLE oo_target_owner;\n\
           END IF;\n\
         END\n\
         $$",
    )
    .await;
    expect_sql(
        &maintenance,
        "create database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_named(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_named(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_named(
        &tx,
        "row-lock",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_named(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_named(
        &tx,
        "markers-target",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            (
                "target-fence-receipts/3",
                &preflight.1,
                "0003_target_segment_preflight_expand.sql",
            ),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");

    let tx = begin_locked(client).await;
    exec_named(
        &tx,
        "row-lock-import",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    exec_named(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_named(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_named(
        &tx,
        "marker-import",
        &marker_sql(&[(
            "full-import/v1",
            &full_import.1,
            "0005_full_import_expand.sql",
        )]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {name}: {err}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} is not utf-8"));
    (text, sha256_hex(&bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        assert!(closed_token(version), "version token");
        assert!(closed_token(filename), "filename token");
        assert_eq!(checksum.len(), 64, "checksum length");
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
    }
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}")
}

fn closed_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '.' | '-'))
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_named(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_named(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_named(
        &tx,
        "advisory-lock",
        "SELECT pg_catalog.pg_advisory_xact_lock(6404202635)",
    )
    .await;
    tx
}

async fn expect_sql(client: &Client, name: &str, sql: &str) {
    client.batch_execute(sql).await.unwrap_or_else(|err| {
        panic!("{name} failed: {}", db_detail(&err));
    });
}

fn db_detail(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.to_string())
        .unwrap_or_else(|| err.to_string())
}

async fn exec_named(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql).await.unwrap_or_else(|err| {
        panic!("{name} failed: {}", db_detail(&err));
    });
}

async fn create_login(client: &Client, name: &str, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(name
        .bytes()
        .all(|byte| byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()));
    client
        .batch_execute(&format!(
            "CREATE ROLE \"{name}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{name}\""
        ))
        .await
        .expect("create login");
}

struct PartyMap {
    table: String,
    party_id: String,
    display_name: String,
    note: String,
    ratio: String,
}

struct LedgerMap {
    table: String,
    entry_id: String,
    party_id: String,
    memo: String,
}

struct Shadow {
    party: PartyMap,
    ledger: LedgerMap,
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .expect("adopt")
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .expect("bind");
    for generation in [STALE_GENERATION, CURRENT_GENERATION] {
        register(
            client,
            &map_key(CONFIG, generation, "namespace", "n", "", ""),
        )
        .await;
        register(client, &map_key(CONFIG, generation, "table", "t", PARTY, "")).await;
        register(client, &map_key(CONFIG, generation, "table", "t", LEDGER, "")).await;
        for column in ["party_id", "display_name", "note", "ratio"] {
            register(
                client,
                &map_key(CONFIG, generation, "column", "c", PARTY, column),
            )
            .await;
        }
        for column in ["entry_id", "party_id", "memo"] {
            register(
                client,
                &map_key(CONFIG, generation, "column", "c", LEDGER, column),
            )
            .await;
        }
    }
    let party_table = lookup(client, "table", PARTY, "").await;
    let ledger_table = lookup(client, "table", LEDGER, "").await;
    let party_cols = vec![
        (
            "party_id".to_string(),
            lookup(client, "column", PARTY, "party_id").await,
        ),
        (
            "display_name".to_string(),
            lookup(client, "column", PARTY, "display_name").await,
        ),
        (
            "note".to_string(),
            lookup(client, "column", PARTY, "note").await,
        ),
        (
            "ratio".to_string(),
            lookup(client, "column", PARTY, "ratio").await,
        ),
    ];
    let ledger_cols = vec![
        (
            "entry_id".to_string(),
            lookup(client, "column", LEDGER, "entry_id").await,
        ),
        (
            "party_id".to_string(),
            lookup(client, "column", LEDGER, "party_id").await,
        ),
        (
            "memo".to_string(),
            lookup(client, "column", LEDGER, "memo").await,
        ),
    ];
    let run_id = open_run(client, target, &party_table, &ledger_table).await;
    prepare_one(
        client,
        target,
        &run_id,
        0,
        PARTY,
        &["party_id", "display_name", "note", "ratio"],
        &[23, 25, 25, 700],
    )
    .await;
    prepare_one(
        client,
        target,
        &run_id,
        1,
        LEDGER,
        &["entry_id", "party_id", "memo"],
        &[23, 23, 25],
    )
    .await;
    insert_rows(client, &party_table, &party_cols, &ledger_table, &ledger_cols).await;
    Shadow {
        party: PartyMap {
            table: party_table,
            party_id: party_cols[0].1.clone(),
            display_name: party_cols[1].1.clone(),
            note: party_cols[2].1.clone(),
            ratio: party_cols[3].1.clone(),
        },
        ledger: LedgerMap {
            table: ledger_table,
            entry_id: ledger_cols[0].1.clone(),
            party_id: ledger_cols[1].1.clone(),
            memo: ledger_cols[2].1.clone(),
        },
    }
}

async fn open_run(client: &Client, target: i64, party: &str, ledger: &str) -> String {
    let schemas = vec![NAMESPACE.to_string(), NAMESPACE.to_string()];
    let tables = vec![PARTY.to_string(), LEDGER.to_string()];
    let physical = vec![party.to_string(), ledger.to_string()];
    let params: [&(dyn ToSql + Sync); 12] = [
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &CURRENT_GENERATION,
        &CONFIG,
        &TYPE_DIGEST,
        &INPUT_DIGEST,
        &SCHEMA_DIGEST,
        &schemas,
        &tables,
        &physical,
    ];
    client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &params,
        )
        .await
        .expect("open run")
        .get(0)
}

async fn prepare_one(
    client: &Client,
    target: i64,
    run_id: &str,
    index: i32,
    table: &str,
    idents: &[&str],
    oids: &[i64],
) {
    let run_owned = run_id.to_string();
    let names: Vec<String> = idents.iter().map(|name| (*name).to_string()).collect();
    let oid_values = oids.to_vec();
    let typmods = vec![-1_i32; idents.len()];
    let required = vec![true; idents.len()];
    let namespace = NAMESPACE.to_string();
    let table_name = table.to_string();
    let params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &target,
        &run_owned,
        &index,
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &namespace,
        &table_name,
        &names,
        &oid_values,
        &typmods,
        &required,
    ];
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &params,
        )
        .await
        .expect("prepare shadow")
        .get(0);
    assert!(prepared.starts_with("pt_"), "{prepared}");
}

async fn insert_rows(
    client: &mut Client,
    party_table: &str,
    party_cols: &[(String, String)],
    ledger_table: &str,
    ledger_cols: &[(String, String)],
) {
    let party_sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {party_id}, {display_name}, {note}, {ratio}) \
         VALUES (($1::text)::numeric, $2, $3, $4, ($5::text)::real)",
        table = quote_physical(party_table, "pt_"),
        party_id = quote_physical(&party_cols[0].1, "pc_"),
        display_name = quote_physical(&party_cols[1].1, "pc_"),
        note = quote_physical(&party_cols[2].1, "pc_"),
        ratio = quote_physical(&party_cols[3].1, "pc_"),
    );
    let ledger_sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {entry_id}, {party_id}, {memo}) \
         VALUES (($1::text)::numeric, $2, $3, $4)",
        table = quote_physical(ledger_table, "pt_"),
        entry_id = quote_physical(&ledger_cols[0].1, "pc_"),
        party_id = quote_physical(&ledger_cols[1].1, "pc_"),
        memo = quote_physical(&ledger_cols[2].1, "pc_"),
    );
    let tx = client.transaction().await.expect("insert begin");
    let first: [&(dyn ToSql + Sync); 5] = [&"1", &1_i32, &"alpha", &"secret-note", &"1.5"];
    let second: [&(dyn ToSql + Sync); 5] = [&"2", &2_i32, &"beta", &"other-note", &"2.5"];
    tx.execute(&party_sql, &first).await.expect("insert party 1");
    tx.execute(&party_sql, &second).await.expect("insert party 2");
    let entry_a: [&(dyn ToSql + Sync); 4] = [&"10", &10_i32, &1_i32, &"memo-a"];
    let entry_b: [&(dyn ToSql + Sync); 4] = [&"11", &11_i32, &2_i32, &"memo-b"];
    tx.execute(&ledger_sql, &entry_a).await.expect("insert ledger 10");
    tx.execute(&ledger_sql, &entry_b).await.expect("insert ledger 11");
    tx.commit().await.expect("insert commit");
}

struct MapKey<'a> {
    digest: &'a str,
    generation: i64,
    level: &'a str,
    letter: &'a str,
    table: &'a str,
    column: &'a str,
}

fn map_key<'a>(
    digest: &'a str,
    generation: i64,
    level: &'a str,
    letter: &'a str,
    table: &'a str,
    column: &'a str,
) -> MapKey<'a> {
    MapKey {
        digest,
        generation,
        level,
        letter,
        table,
        column,
    }
}

async fn register(client: &Client, key: &MapKey<'_>) {
    let namespace = NAMESPACE;
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &key.digest,
        &key.generation,
        &key.letter,
        &namespace,
        &key.table,
        &key.column,
    ];
    let digest_bytes: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &key.digest,
        &key.generation,
        &key.level,
        &namespace,
        &key.table,
        &key.column,
        &digest_bytes,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("register");
}

async fn lookup(client: &Client, level: &str, table: &str, column: &str) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &level,
        &NAMESPACE,
        &table,
        &column,
    ];
    let name: String = client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .expect("lookup")
        .get(0);
    let prefix = if level == "table" { "pt_" } else { "pc_" };
    let _ = quote_physical(&name, prefix);
    name
}

async fn max_generation(client: &Client, digest: &str) -> Option<i64> {
    client
        .query_one(
            "SELECT max(run_generation) FROM _oo.physical_mappings WHERE source = $1 AND config_digest = $2",
            &[&SOURCE, &digest],
        )
        .await
        .expect("max generation")
        .get(0)
}

async fn role_count(client: &Client, prefix: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_roles WHERE left(rolname, length($1)) = $1",
            &[&prefix],
        )
        .await
        .expect("role count")
        .get(0)
}

async fn role_present(client: &Client, name: &str) -> bool {
    client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = $1)",
            &[&name],
        )
        .await
        .expect("role present")
        .get(0)
}

async fn column_grant_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .expect("column grants")
        .get(0)
}

async fn table_privilege_count(client: &Client, role: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.table_privileges WHERE grantee = $1",
            &[&role],
        )
        .await
        .expect("table privileges")
        .get(0)
}

async fn mapping_privilege_count(client: &Client) -> i64 {
    client
        .query_one(
            "SELECT \
               (SELECT count(*) FROM information_schema.column_privileges \
                 WHERE grantee IN ('oo_r_clerk', 'oo_r_peek', 'oo_a235_login') \
                   AND table_schema = '_oo' AND table_name = 'physical_mappings') \
             + (SELECT count(*) FROM information_schema.table_privileges \
                 WHERE grantee IN ('oo_r_clerk', 'oo_r_peek', 'oo_a235_login') \
                   AND table_schema = '_oo' AND table_name = 'physical_mappings')",
            &[],
        )
        .await
        .expect("mapping privileges")
        .get(0)
}

async fn select_columns(client: &Client, role: &str, table: &str) -> BTreeSet<String> {
    client
        .query(
            "SELECT column_name::text FROM information_schema.column_privileges \
             WHERE grantee = $1 AND table_schema = '_oo' AND table_name = $2 \
               AND privilege_type = 'SELECT' \
             ORDER BY column_name",
            &[&role, &table],
        )
        .await
        .expect("select columns")
        .into_iter()
        .map(|row| row.get(0))
        .collect()
}

async fn publication_count(client: &Client) -> i64 {
    client
        .query_one("SELECT count(*)::int8 FROM pg_catalog.pg_publication", &[])
        .await
        .expect("publications")
        .get(0)
}

async fn reltuples_unknown(client: &Client, table: &str) -> bool {
    client
        .query_one(
            "SELECT relation.reltuples < 0 \
             FROM pg_catalog.pg_class AS relation \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1",
            &[&table],
        )
        .await
        .expect("reltuples")
        .get(0)
}

async fn assert_role_shape(client: &Client, role: &str) {
    let row = client
        .query_one(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, \
                    rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&role],
        )
        .await
        .expect("role flags");
    let flags = [
        row.get::<_, bool>(0),
        row.get::<_, bool>(1),
        row.get::<_, bool>(2),
        row.get::<_, bool>(3),
        row.get::<_, bool>(4),
        row.get::<_, bool>(5),
        row.get::<_, bool>(6),
    ];
    assert_eq!(flags, [false; 7], "{role}");
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length {name}");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "physical hex"
    );
    format!("\"{name}\"")
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::new();
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(err) => {
                last = err.to_string();
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
}

async fn query_as_role(client: &mut Client, role: &str, sql: &str) -> tokio_postgres::Row {
    let tx = client.transaction().await.expect("begin read");
    let quoted = quote_role(role);
    tx.batch_execute(&format!("SET LOCAL ROLE {quoted}"))
        .await
        .expect("set role");
    let row = tx.query_one(sql, &[]).await.expect("selected read");
    tx.rollback().await.expect("rollback read");
    row
}

async fn expect_42501(client: &mut Client, role: Option<&str>, sql: &str) {
    let tx = client.transaction().await.expect("begin denial");
    if let Some(role) = role {
        let quoted = quote_role(role);
        tx.batch_execute(&format!("SET LOCAL ROLE {quoted}"))
            .await
            .expect("set role");
    }
    let err = tx.query_one(sql, &[]).await.expect_err("expected denial");
    let state = err.code().map(|code| code.code().to_string()).unwrap_or_default();
    assert_eq!(state, "42501", "{err}");
    tx.rollback().await.expect("rollback denial");
}

fn quote_role(role: &str) -> String {
    assert!(
        role.bytes().all(|byte| {
            byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit()
        }) && !role.is_empty()
    );
    format!("\"{role}\"")
}
