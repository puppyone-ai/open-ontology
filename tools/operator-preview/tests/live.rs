//! Local preview against one private PostgreSQL. Skipped unless OO_DL01W121_LIVE=1.
//!
//! Rows are inserted by this fixture. No source is dialed. The draft pack is the
//! committed compiler generation re-sealed by the writer test. This file's
//! expected cells are not written into that pack.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use gateway_audit_wal::inspect;
use oo_builder::compile_role_pack;
use oo_gateway::policy::catalog::{
    grantable_base_type, load_policy_catalog, CatalogError, CatalogLimits, UnknownRows,
};
use oo_gateway::query_executor::{
    observe_backend, CancelOutcome, Cell, CompletionStatus, DedicatedSession, ExecError,
    SelectRequest, StopClass,
};
use oo_gateway::policy::install_column_acl_set;
use oo_gateway::{
    compile_column_acl, connect_require_tls, install_column_acl, rustls_connector, ClassStatus,
    ColumnSpec, PiiClass, PolicyError, PolicyIdentity, TlsEndpoint,
};
use operator_preview::{load_policy, CANDIDATE_PROTOCOL, PACK_RECEIPT_PROTOCOL};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;

const DATABASE: &str = "preview";
const LOGIN: &str = "oo_preview_login";
const ADMIN: &str = "oo_fixture_admin";
const SOURCE: &str = "a407bulk";
const CONFIG: &str = "379615f4f792d2afa6ecceb851355edc309f1473162f5f6ea7ccdf835c42363d";
const IMPORT_HASH: &str = "f7c21ea98c20b88b8a7986da37f77772f9d8874f2efc055b48afdb0fef31aed1";
const SNAPSHOT: &str = "0000000D-0000000C-1";
const MANIFEST: &str = "77adb990b8aac4ec42ab2e328e7b38cf5cef5c103efc6b13dd7168d1306debc7";
const TYPE_DIGEST: &str = "d121d121d121d121d121d121d121d121d121d121d121d121d121d121d121d121";
const INPUT_DIGEST: &str = "e121e121e121e121e121e121e121e121e121e121e121e121e121e121e121e121";
const SCHEMA_DIGEST: &str = "f121f121f121f121f121f121f121f121f121f121f121f121f121f121f121f121";
const OWNER: &str = "d1210000-0000-4000-8000-000000000021";
const SHARD: &str = "shard_w121";
const NAMESPACE: &str = "public";
const TABLE: &str = "batch_c";
const DATASET: &str = "public.batch_c";
const GENERATION: i64 = 2;
const SENTINEL: &str = "hidden-sentinel-w121";
const GOLD_MIN: &str = "-9223372036854775808";
const GOLD_MAX: &str = "9223372036854775807";
const QTY_LOW: i64 = -7_000_000_000_000_000_001;
const QTY_HIGH: i64 = 8_100_000_000_000_000_003;

struct Proof {
    path: PathBuf,
    value: Value,
}

impl Proof {
    fn new(path: PathBuf) -> Self {
        let proof = Self {
            path,
            value: json!({
                "surface": "trusted-local-operator-preview",
                "public_untrusted_gate": "pending",
                "source_dialed": false,
                "builder_chain": "not_claimed",
                "fixture": "sealed_local_synthetic_target",
            }),
        };
        proof.flush();
        proof
    }

    fn set(&mut self, key: &str, value: Value) {
        self.value[key] = value;
        self.flush();
    }

    fn flush(&self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.value) {
            let _ = fs::write(&self.path, bytes);
        }
    }
}

impl Drop for Proof {
    fn drop(&mut self) {
        self.flush();
    }
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
    json: Option<Value>,
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_operator-preview")
}

fn run_cli(args: &[String]) -> Ran {
    let output = Command::new(bin())
        .args(args)
        .output()
        .unwrap_or_else(|_| panic!("spawn operator-preview"));
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let json = serde_json::from_str(stdout.trim()).ok();
    Ran {
        code: output.status.code().unwrap_or(127),
        stdout,
        stderr,
        json,
    }
}

async fn cli(args: &[&str]) -> Ran {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    tokio::task::spawn_blocking(move || run_cli(&owned))
        .await
        .expect("cli join")
}

fn require_env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("missing {key}"))
}

fn read_pass(path: &Path) -> String {
    let mut text = fs::read_to_string(path).unwrap_or_else(|_| panic!("password file missing"));
    while text.ends_with('\n') || text.ends_with('\r') {
        text.pop();
    }
    assert!(
        !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "password file is not hex"
    );
    text
}

fn sqlstate(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.code().code().to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn connect_panic(err: &PolicyError) -> ! {
    match err {
        PolicyError::Database { sqlstate, .. } => panic!("connect sqlstate={sqlstate}"),
        PolicyError::CertificateRejected => panic!("connect certificate rejected"),
        PolicyError::TlsRequired => panic!("connect tls required"),
        _ => panic!("connect refused"),
    }
}

async fn connect(host: &str, database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let (client, connection) = connect_require_tls(&endpoint, password, ca)
        .await
        .unwrap_or_else(|err| connect_panic(&err));
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn exec(client: &Client, sql: &str) {
    if let Err(err) = client.batch_execute(sql).await {
        panic!("statement failed sqlstate={}", sqlstate(&err));
    }
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

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|_| panic!("missing migration {name}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} encoding"));
    (text, sha256_hex(&bytes))
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!("  ('{version}', '{checksum}', 'expand', '{filename}')"));
    }
    format!(
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}"
    )
}

async fn apply_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_tx(&tx, &bootstrap.0).await;
    exec_tx(&tx, "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, &fence.0).await;
    exec_tx(&tx, "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_tx(&tx, &preflight.0).await;
    exec_tx(&tx, &mapping.0).await;
    exec_tx(
        &tx,
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
    exec_tx(&tx, "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_tx(
        &tx,
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_tx(&tx, "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, &full_import.0).await;
    exec_tx(
        &tx,
        &marker_sql(&[("full-import/v1", &full_import.1, "0005_full_import_expand.sql")]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_tx(&tx, "SET LOCAL lock_timeout = '15s'").await;
    exec_tx(&tx, "SET LOCAL statement_timeout = '60s'").await;
    exec_tx(&tx, "SELECT pg_catalog.pg_advisory_xact_lock(6404212121)").await;
    tx
}

async fn exec_tx(tx: &tokio_postgres::Transaction<'_>, sql: &str) {
    if let Err(err) = tx.batch_execute(sql).await {
        panic!("transaction statement failed sqlstate={}", sqlstate(&err));
    }
}

struct Shadow {
    table: String,
    id: String,
    qty: String,
    amount: String,
    note: String,
    secret: String,
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical name length");
    assert_eq!(rest.len(), 60, "physical body length");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "physical name is not lowercase hex"
    );
    format!("\"{name}\"")
}

async fn register(
    client: &Client,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn tokio_postgres::types::ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let digest: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .unwrap_or_else(|err| panic!("mapping digest sqlstate={}", sqlstate(&err)))
        .get(0);
    let bind: [&(dyn tokio_postgres::types::ToSql + Sync); 8] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
        &digest,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .unwrap_or_else(|err| panic!("register mapping sqlstate={}", sqlstate(&err)));
}

async fn lookup(client: &Client, level: &str, namespace: &str, table: &str, column: &str) -> String {
    let generation = GENERATION;
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
    ];
    client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .unwrap_or_else(|err| panic!("lookup mapping sqlstate={}", sqlstate(&err)))
        .get(0)
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
        .await
        .unwrap_or_else(|err| panic!("adopt sqlstate={}", sqlstate(&err)))
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .unwrap_or_else(|err| panic!("bind session sqlstate={}", sqlstate(&err)));
    register(client, GENERATION, "namespace", "n", NAMESPACE, "", "").await;
    register(client, GENERATION, "table", "t", NAMESPACE, TABLE, "").await;
    for column in ["id", "qty", "amount", "note", "secret", "wide"] {
        register(client, GENERATION, "column", "c", NAMESPACE, TABLE, column).await;
    }
    let shadow = Shadow {
        table: lookup(client, "table", NAMESPACE, TABLE, "").await,
        id: lookup(client, "column", NAMESPACE, TABLE, "id").await,
        qty: lookup(client, "column", NAMESPACE, TABLE, "qty").await,
        amount: lookup(client, "column", NAMESPACE, TABLE, "amount").await,
        note: lookup(client, "column", NAMESPACE, TABLE, "note").await,
        secret: lookup(client, "column", NAMESPACE, TABLE, "secret").await,
    };
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![shadow.table.clone()];
    let run_id: String = client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &[
                &SHARD,
                &OWNER,
                &target,
                &SOURCE,
                &GENERATION,
                &CONFIG,
                &TYPE_DIGEST,
                &INPUT_DIGEST,
                &SCHEMA_DIGEST,
                &schemas,
                &tables,
                &physical,
            ],
        )
        .await
        .unwrap_or_else(|err| panic!("open run sqlstate={}", sqlstate(&err)))
        .get(0);
    let idents = vec![
        "id".to_string(),
        "qty".to_string(),
        "amount".to_string(),
        "note".to_string(),
        "secret".to_string(),
        "wide".to_string(),
    ];
    let oids: Vec<i64> = vec![20, 20, 1700, 25, 25, 701];
    let typmods: Vec<i32> = vec![-1, -1, 2490376, -1, -1, -1];
    let required = vec![true, true, true, false, false, false];
    let index = 0_i32;
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &[
                &SHARD,
                &OWNER,
                &target,
                &run_id,
                &index,
                &SOURCE,
                &CONFIG,
                &GENERATION,
                &NAMESPACE,
                &TABLE,
                &idents,
                &oids,
                &typmods,
                &required,
            ],
        )
        .await
        .unwrap_or_else(|err| panic!("prepare shadow sqlstate={}", sqlstate(&err)))
        .get(0);
    assert_eq!(prepared, shadow.table);
    insert_rows(client, &shadow).await;
    exec(
        client,
        &format!("ANALYZE \"_oo\".{}", quote_physical(&shadow.table, "pt_")),
    )
    .await;
    shadow
}

async fn insert_rows(client: &mut Client, shadow: &Shadow) {
    let sql = format!(
        "INSERT INTO \"_oo\".{} (\"_oo_pos\", {}, {}, {}, {}, {}) \
         VALUES (($1::text)::numeric, $2, $3, ($4::text)::numeric, $5, $6)",
        quote_physical(&shadow.table, "pt_"),
        quote_physical(&shadow.id, "pc_"),
        quote_physical(&shadow.qty, "pc_"),
        quote_physical(&shadow.amount, "pc_"),
        quote_physical(&shadow.note, "pc_"),
        quote_physical(&shadow.secret, "pc_")
    );
    let tx = client.transaction().await.expect("insert begin");
    let rows: [(i64, i64, &str, Option<&str>); 2] = [
        (i64::MIN, QTY_LOW, "-12.5000", None),
        (i64::MAX, QTY_HIGH, "100.2500", Some("alpha")),
    ];
    for (index, row) in rows.iter().enumerate() {
        let pos = (index + 1).to_string();
        let note = row.3.map(str::to_string);
        let amount = row.2.to_string();
        let secret = SENTINEL.to_string();
        tx.execute(&sql, &[&pos, &row.0, &row.1, &amount, &note, &secret])
            .await
            .unwrap_or_else(|err| panic!("insert sqlstate={}", sqlstate(&err)));
    }
    tx.commit().await.expect("insert commit");
}

fn write_json(path: &Path, value: &Value) {
    let mut text = serde_json::to_string_pretty(value).expect("json");
    text.push('\n');
    fs::write(path, text).expect("write json");
}

fn authority_value(
    work: &Path,
    pack: &Path,
    receipt: &Path,
    ca: &Path,
    password: &Path,
    audit: &Path,
) -> Value {
    json!({
        "protocol": "oo.operator-preview.config/v1",
        "authority": {"kind": "trusted-operator-file", "binds": "import+mapping+role-policy+pack+target-tls"},
        "project": SOURCE,
        "role": "oo_r_reader",
        "policy_pack_id": "pol-preview",
        "policy_version": "1",
        "knowledge": {"id": "know-preview", "version": "1"},
        "import": {
            "import_generation": GENERATION,
            "import_hash": IMPORT_HASH,
            "snapshot_id": SNAPSHOT,
            "source_id": SOURCE,
            "manifest_sha256": MANIFEST,
            "target_config_sha256": CONFIG
        },
        "policy_path": work.join("policy.json").to_string_lossy(),
        "pack_dir": pack.to_string_lossy(),
        "pack_receipt": receipt.to_string_lossy(),
        "role_pack_dir": work.join("role-pack").to_string_lossy(),
        "target": {
            "host": "oo-dl01w1-21-pg",
            "port": 5432,
            "database": DATABASE,
            "user": LOGIN,
            "ca_path": ca.to_string_lossy(),
            "password_path": password.to_string_lossy()
        },
        "state_dir": work.join("state").to_string_lossy(),
        "audit_dir": audit.to_string_lossy(),
        "receipt_dir": work.join("receipts").to_string_lossy(),
        "audit_max_bytes": 67_108_864,
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "zero"
        }
    })
}

fn policy_value() -> Value {
    policy_table(
        NAMESPACE,
        TABLE,
        &[
            ("id", "int8"),
            ("qty", "int8"),
            ("amount", "numeric"),
            ("note", "text"),
        ],
    )
}

fn policy_table(namespace: &str, name: &str, columns: &[(&str, &str)]) -> Value {
    let columns: Vec<Value> = columns
        .iter()
        .map(|(column, semantic)| {
            json!({
                "name": column,
                "status": "confirmed",
                "semantic_type": semantic,
                "pii": {"class": "none"}
            })
        })
        .collect();
    json!({
        "protocol": "oo.operator-preview.policy/v1",
        "project": SOURCE,
        "config_digest": CONFIG,
        "generation": GENERATION,
        "role": "reader",
        "login": LOGIN,
        "table": {
            "namespace": namespace,
            "name": name,
            "columns": columns
        }
    })
}

fn query_value(sql: &str, max_rows: u32, max_bytes: u64, wall_ms: u64) -> Value {
    json!({
        "protocol": "oo.operator-preview.query/v1",
        "sql": sql,
        "max_rows": max_rows,
        "max_result_bytes": max_bytes,
        "wall_clock_ms": wall_ms
    })
}

fn role_documents() -> (String, String) {
    role_pack_documents(
        NAMESPACE,
        DATASET,
        &["id", "qty", "amount", "note"],
        &["secret"],
    )
}

fn role_pack_documents(
    schema: &str,
    dataset: &str,
    visible: &[&str],
    hidden: &[&str],
) -> (String, String) {
    let column = |name: &str, pii: &str| {
        json!({"name": name, "pii": pii, "classification": {"state": "confirmed"}})
    };
    let mut columns = Vec::new();
    for name in visible {
        columns.push(column(name, "none"));
    }
    for name in hidden {
        columns.push(column(name, "sensitive"));
    }
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": SOURCE,
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-preview", "version": "1"},
        "knowledge": {"id": "know-preview", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "acme",
            "locale": "zh-CN",
            "include_schemas": [schema],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [{
            "schema_version": "oos/v1",
            "dataset": dataset,
            "label": "Batch",
            "tier": "file",
            "grain": "one row",
            "primary_key": [visible[0]],
            "columns": columns
        }]
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": SOURCE,
        "principal": "operator",
        "role": "oo_r_reader",
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-preview", "version": "1"},
        "knowledge": {"id": "know-preview", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": dataset,
            "row_security": "none",
            "global_stats": "denied",
            "columns": visible
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (
        serde_json::to_string(&effective).expect("effective"),
        serde_json::to_string(&visibility).expect("visibility"),
    )
}

fn tree_has(dir: &Path, needle: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && tree_has(&path, needle) {
            return true;
        }
        if path.is_file()
            && let Ok(text) = fs::read_to_string(&path)
            && text.contains(needle)
        {
            return true;
        }
    }
    false
}

fn assert_clean(ran: &Ran, password: &str) {
    assert!(!ran.stdout.contains(password), "stdout contains the password");
    assert!(!ran.stderr.contains(password), "stderr contains the password");
    assert!(!ran.stdout.contains(SENTINEL), "stdout contains the hidden sentinel");
    assert!(!ran.stderr.contains(SENTINEL), "stderr contains the hidden sentinel");
}

fn labels_ok(value: &Value) {
    assert_eq!(value["surface"], json!("trusted-local-operator-preview"));
    assert_eq!(value["public_untrusted_gate"], json!("pending"));
    assert_eq!(value["untrusted_admission"], json!("blocked"));
    assert_eq!(value["pi_session"], json!("not_launched"));
    assert_eq!(value["publication"], json!("not_published"));
    assert_eq!(value["policy_source"], json!("trusted-operator-file"));
    assert_eq!(value["pack_text_is_policy"], json!(false));
    assert_eq!(value["import"]["source_dialed"], json!(false));
    assert_eq!(value["import"]["builder_chain"], json!("not_claimed"));
}

fn no_rows(ran: &Ran) {
    let value = ran.json.as_ref().expect("json stdout");
    assert!(value.get("rows").is_none(), "refusal stdout has rows");
    assert!(!ran.stdout.contains("\"rows\""), "refusal bytes contain rows");
}

fn gold_rows(value: &Value) {
    let rows = value["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0]["kind"], json!("int"));
    assert_eq!(rows[0][0]["value"], json!(GOLD_MIN));
    assert_eq!(rows[0][1]["kind"], json!("int"));
    assert_eq!(rows[0][1]["value"], json!(QTY_LOW.to_string()));
    assert_eq!(rows[0][2]["kind"], json!("decimal"));
    assert_eq!(rows[0][2]["negative"], json!(true));
    assert_eq!(rows[0][2]["coefficient"], json!("125000"));
    assert_eq!(rows[0][2]["scale"], json!(4));
    assert_eq!(rows[0][3], json!({"kind": "null"}));
    assert_eq!(rows[1][0]["value"], json!(GOLD_MAX));
    assert_eq!(rows[1][1]["value"], json!(QTY_HIGH.to_string()));
    assert_eq!(rows[1][2]["negative"], json!(false));
    assert_eq!(rows[1][2]["coefficient"], json!("1002500"));
    assert_eq!(rows[1][2]["scale"], json!(4));
    assert_eq!(rows[1][3], json!({"kind": "text", "value": "alpha"}));
}

fn execution_of(value: &Value) -> String {
    value["execution_id"]
        .as_str()
        .expect("execution id")
        .to_string()
}

async fn login_backends(admin: &Client) -> i64 {
    admin
        .query_one(
            "SELECT count(*)::int8 FROM pg_catalog.pg_stat_activity WHERE usename = $1",
            &[&LOGIN],
        )
        .await
        .unwrap_or_else(|err| panic!("backend count sqlstate={}", sqlstate(&err)))
        .get(0)
}

async fn hold_lock(client: Client, table: String, release: tokio::sync::oneshot::Receiver<()>) {
    let sql = format!(
        "BEGIN; SET LOCAL lock_timeout = '15s'; LOCK TABLE \"_oo\".{} IN ACCESS EXCLUSIVE MODE",
        quote_physical(&table, "pt_")
    );
    exec(&client, &sql).await;
    let _ = release.await;
    let _ = client.batch_execute("ROLLBACK").await;
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|_| panic!("mkdir {}", to.display()));
    for entry in fs::read_dir(from).unwrap_or_else(|_| panic!("read {}", from.display())) {
        let entry = entry.expect("pack entry");
        let dest = to.join(entry.file_name());
        let file_type = entry.file_type().expect("pack file type");
        assert!(!file_type.is_symlink(), "pack symlink");
        if file_type.is_dir() {
            copy_dir(&entry.path(), &dest);
        } else if file_type.is_file() {
            fs::copy(entry.path(), &dest).unwrap_or_else(|_| panic!("copy {}", dest.display()));
        }
    }
}

async fn expect_pack(authority: &Path, query: &Path, audit: &Path, password: &str) {
    let ran = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        query.to_str().unwrap(),
    ])
    .await;
    assert_clean(&ran, password);
    assert_eq!(ran.code, 9, "pack stderr={} stdout={}", ran.stderr, ran.stdout);
    assert_eq!(ran.stderr, "operator-preview: pack\n");
    no_rows(&ran);
    assert_eq!(ran.json.as_ref().unwrap()["class"], json!("pack_mismatch"));
    assert!(!audit.exists(), "audit opened at {}", audit.display());
}

#[test]
fn shared_catalog_admits_decimal256_and_the_exact_extra_bases() {
    fn typmod(precision: i32, scale: i32) -> i32 {
        ((precision << 16) | (scale & 0x7FF)) + 4
    }
    assert_eq!(typmod(76, 10), 4_980_750);
    assert!(grantable_base_type("numeric", typmod(1, 0)));
    assert!(grantable_base_type("numeric", typmod(38, 4)));
    assert!(grantable_base_type("numeric", typmod(76, 10)));
    assert!(grantable_base_type("bool", -1));
    assert!(grantable_base_type("int2", -1));
    assert!(grantable_base_type("int4", -1));
    assert!(grantable_base_type("int8", -1));
    assert!(grantable_base_type("text", -1));
    assert!(grantable_base_type("varchar", -1));
    assert!(grantable_base_type("float8", -1));
    assert!(grantable_base_type("timestamptz", -1));
    assert!(grantable_base_type("json", -1));
    assert!(!grantable_base_type("numeric", -1));
    assert!(!grantable_base_type("numeric", typmod(0, 0)));
    assert!(!grantable_base_type("numeric", typmod(77, 10)));
    assert!(!grantable_base_type("numeric", typmod(1000, 0)));
    assert!(grantable_base_type("bytea", -1));
    assert!(grantable_base_type("uuid", -1));
    assert!(!grantable_base_type("uuid", 0));
    assert!(!grantable_base_type("uuid", 4));
    assert!(grantable_base_type("bpchar", -1));
    assert!(grantable_base_type("bpchar", 10));
    assert!(!grantable_base_type("bpchar", 0));
    assert!(!grantable_base_type("bpchar", 3));
    assert!(!grantable_base_type("bpchar", 4));
    assert!(grantable_base_type("date", -1));
    assert!(!grantable_base_type("date", 0));
    assert!(!grantable_base_type("date", 4));
    assert!(grantable_base_type("timestamp", -1));
    assert!(!grantable_base_type("timestamp", 0));
    assert!(!grantable_base_type("timestamp", 6));
    assert!(grantable_base_type("jsonb", -1));
    assert!(!grantable_base_type("jsonb", 0));
    assert!(!grantable_base_type("jsonb", 4));
    assert!(!grantable_base_type("float4", -1));
    assert!(!grantable_base_type("interval", -1));
    assert!(!grantable_base_type("time", -1));
    assert!(!grantable_base_type("numeric", typmod(77, 10)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_preview_loop() {
    if std::env::var("OO_DL01W121_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    assert_eq!(
        PACK_RECEIPT_PROTOCOL,
        minimal_product_loop::PACK_RECEIPT_PROTOCOL
    );
    assert_eq!(
        CANDIDATE_PROTOCOL,
        "oo.minimal-product-loop.draft-candidate/v1"
    );
    let fresh = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a411/SP11/minimal-product-loop/linux/fresh-copy",
    );
    let catalog: Value = serde_json::from_str(
        &fs::read_to_string(fresh.join("catalog.json")).expect("catalog json"),
    )
    .expect("catalog");
    let profile: Value = serde_json::from_str(
        &fs::read_to_string(fresh.join("profile.json")).expect("profile json"),
    )
    .expect("profile");
    let generation: Value = serde_json::from_str(
        &fs::read_to_string(fresh.join("generation.json")).expect("generation json"),
    )
    .expect("generation");
    let binding = json!({
        "import_generation": GENERATION,
        "import_hash": IMPORT_HASH,
        "manifest_sha256": MANIFEST,
        "snapshot_id": SNAPSHOT,
        "source_id": SOURCE,
        "target_config_sha256": CONFIG
    });
    let tables = json!([
        {"schema": "public", "name": "batch_a", "rows": 2000},
        {"schema": "public", "name": "batch_b", "rows": 8000},
        {"schema": "public", "name": "batch_c", "rows": 90000}
    ]);
    minimal_product_loop::sealed_pack_input(&catalog, &profile, &generation, &binding, &tables)
        .expect("sealed_pack_input");

    let host = require_env("OO_DL01W121_HOST");
    let pack = PathBuf::from(require_env("OO_DL01W121_PACK"));
    let receipt = PathBuf::from(require_env("OO_DL01W121_RECEIPT"));
    let ca_path = PathBuf::from(require_env("OO_DL01W121_CA"));
    let admin_pass_path = PathBuf::from(require_env("OO_DL01W121_ADMIN_PASS"));
    let login_pass_path = PathBuf::from(require_env("OO_DL01W121_LOGIN_PASS"));
    let proof_path = PathBuf::from(require_env("OO_DL01W121_PROOF"));
    let mut proof = Proof::new(proof_path);
    assert!(pack.is_dir(), "sealed pack directory is missing");
    assert!(receipt.is_file(), "sealed pack receipt is missing");
    assert!(!tree_has(&pack, SENTINEL), "pack contains the hidden sentinel");
    assert!(!tree_has(&pack, GOLD_MIN), "pack contains a gold integer");

    let admin_pass = read_pass(&admin_pass_path);
    let login_pass = read_pass(&login_pass_path);
    let ca = fs::read(&ca_path).expect("ca");
    let maintenance = connect(&host, "postgres", ADMIN, &admin_pass, &ca).await;
    exec(
        &maintenance,
        &format!(
            "SELECT pg_catalog.pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_catalog.pg_backend_pid()"
        ),
    )
    .await;
    exec(
        &maintenance,
        &format!("DROP DATABASE IF EXISTS \"{DATABASE}\""),
    )
    .await;
    // 0000_bootstrap_roles.sql always CREATE ROLE. Those roles are cluster-wide
    // and survive DROP DATABASE, so a second proof on this cluster must drop
    // them before bootstrap. The login and column-ACL roles are the same kind.
    exec(
        &maintenance,
        "DO $reset$\n\
         DECLARE name text;\n\
         BEGIN\n\
           FOREACH name IN ARRAY ARRAY[\
             'oo_preview_login', 'oo_r_reader', 'oo_pipeline_runtime', 'oo_target_owner'\
           ]\n\
           LOOP\n\
             IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = name) THEN\n\
               EXECUTE format('DROP OWNED BY %I', name);\n\
               EXECUTE format('DROP ROLE %I', name);\n\
             END IF;\n\
           END LOOP;\n\
         END\n\
         $reset$",
    )
    .await;
    exec(
        &maintenance,
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
    drop(maintenance);
    let mut admin = connect(&host, DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_schema(&mut admin).await;
    exec(
        &admin,
        &format!(
            "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{login_pass}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\";\n\
             GRANT USAGE ON SCHEMA _oo TO \"{LOGIN}\";\n\
             GRANT SELECT ON TABLE _oo.physical_mappings TO \"{LOGIN}\";\n\
             GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO \"{LOGIN}\";\n\
             GRANT pg_read_all_stats TO \"{LOGIN}\" WITH INHERIT TRUE"
        ),
    )
    .await;
    let shadow = prepare_shadow(&mut admin).await;
    proof.set("physical_table_len", json!(shadow.table.len()));
    let work = PathBuf::from(require_env("OO_DL01W121_WORK"));
    if work.exists() {
        fs::remove_dir_all(&work).expect("reset work");
    }
    fs::create_dir_all(&work).expect("work");
    write_json(&work.join("policy.json"), &policy_value());
    let loaded = load_policy(&work.join("policy.json")).expect("policy");
    install_column_acl(&mut admin, &loaded.acl)
        .await
        .unwrap_or_else(|err| match err {
            PolicyError::Database { sqlstate, .. } => panic!("install sqlstate={sqlstate}"),
            PolicyError::TypeRefused { .. } => panic!("install type refused"),
            _ => panic!("install refused"),
        });
    let mut wide_policy = loaded.acl.clone();
    wide_policy.table.columns.push(ColumnSpec {
        name: "wide".to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "float8".to_string(),
        pii: PiiClass::None,
    });
    // The single-table installer does not inspect atttypid. The set installer
    // refuses a non-grantable base type before it compares an existing grant.
    match install_column_acl_set(&mut admin, std::slice::from_ref(&wide_policy)).await {
        Err(PolicyError::GrantMismatch) => proof.set("float_install", json!("grant_mismatch")),
        Err(PolicyError::TypeRefused { .. }) => {
            panic!("float8 is an admitted native type and was type_refused")
        }
        Ok(_) => panic!("float8 column install changed the existing grant set"),
        Err(PolicyError::Database { sqlstate, .. }) => panic!("float install sqlstate={sqlstate}"),
        Err(err) => panic!("float install ended as {err}"),
    }
    exec(
        &admin,
        &format!(
            "GRANT SELECT ({}) ON TABLE \"_oo\".{} TO \"oo_r_reader\"",
            quote_physical(&shadow.secret, "pc_"),
            quote_physical(&shadow.table, "pt_")
        ),
    )
    .await;
    let (effective, visibility) = role_documents();
    let role_pack = compile_role_pack(&effective, &visibility)
        .unwrap_or_else(|err| panic!("role pack compile failed: {err}"));
    assert!(!role_pack.artifact.contains(SENTINEL));
    assert!(!role_pack.artifact.contains(GOLD_MIN));
    let role_dir = work.join("role-pack");
    fs::create_dir_all(&role_dir).expect("role dir");
    fs::write(role_dir.join("preview.json"), &role_pack.artifact).expect("role artifact");
    let audit = work.join("audit");
    let authority = work.join("authority.json");
    write_json(
        &authority,
        &authority_value(&work, &pack, &receipt, &ca_path, &login_pass_path, &audit),
    );
    let success_sql = "SELECT id, qty, amount, note FROM public.batch_c ORDER BY id";
    write_json(
        &work.join("success.json"),
        &query_value(success_sql, 10, 8192, 5000),
    );
    write_json(
        &work.join("hidden.json"),
        &query_value("SELECT secret FROM public.batch_c", 10, 8192, 5000),
    );
    write_json(
        &work.join("write.json"),
        &query_value("UPDATE public.batch_c SET note = note", 10, 8192, 5000),
    );
    write_json(
        &work.join("cast.json"),
        &query_value(
            "SELECT id::float8 AS wide FROM public.batch_c",
            10,
            8192,
            5000,
        ),
    );
    write_json(
        &work.join("rows.json"),
        &query_value(success_sql, 1, 8192, 5000),
    );
    write_json(
        &work.join("bytes.json"),
        &query_value(
            "SELECT id FROM public.batch_c ORDER BY id",
            10,
            20,
            5000,
        ),
    );
    write_json(
        &work.join("secret-query.json"),
        &query_value("SELECT secret FROM public.batch_c", 10, 8192, 5000),
    );
    write_json(
        &work.join("absent-table-query.json"),
        &query_value("SELECT n_i8 FROM preview.sample", 10, 8192, 5000),
    );
    write_json(
        &work.join("wall.json"),
        &query_value(success_sql, 10, 8192, 1200),
    );
    write_json(
        &work.join("cancel.json"),
        &query_value(success_sql, 10, 8192, 20000),
    );
    write_json(
        &work.join("policy-query.json"),
        &json!({
            "protocol": "oo.operator-preview.query/v1",
            "sql": success_sql,
            "policies": [],
            "max_rows": 10,
            "max_result_bytes": 8192,
            "wall_clock_ms": 5000
        }),
    );

    let help = cli(&["--help"]).await;
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains("not a public untrusted query gate"));
    proof.set("help_exit", json!(help.code));

    let described = cli(&["describe", "--authority", authority.to_str().unwrap()]).await;
    assert_clean(&described, &login_pass);
    assert_eq!(described.code, 0, "describe stderr={}", described.stderr);
    let described_json = described.json.expect("describe json");
    labels_ok(&described_json);
    assert_eq!(described_json["outcome"], json!("described"));
    assert_eq!(described_json["knowledge"]["dataset"], json!(DATASET));
    assert_eq!(described_json["draft_pack"]["dataset"], json!(DATASET));
    let mut names: Vec<&str> = described_json["knowledge"]["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .map(|column| {
            assert_eq!(column["table"], json!(DATASET));
            column["name"].as_str().unwrap_or("")
        })
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["amount", "id", "note", "qty"]);
    let admitted = described_json["draft_pack"]["admitted_fields"]
        .as_array()
        .expect("admitted fields");
    assert!(admitted.iter().any(|field| {
        field["dataset"] == json!(DATASET) && field["column"] == json!("note")
    }));
    assert!(admitted
        .iter()
        .all(|field| field["dataset"] != json!("preview.sample") && field["column"] != json!("secret") && field["data_type"] != json!("bool")));
    assert!(!described.stdout.contains(SENTINEL));
    proof.set("describe_exit", json!(0));
    proof.set("bound_dataset", json!(DATASET));
    proof.set("sealed_bool_column", json!(false));

    let before = login_backends(&admin).await;
    for (name, patch) in [
        ("role", json!("oo_r_other")),
        ("knowledge_version", json!("9")),
        ("import_generation", json!(9)),
    ] {
        let mut doc = authority_value(&work, &pack, &receipt, &ca_path, &login_pass_path, &work.join(format!("missing-audit-{name}")));
        match name {
            "role" => doc["role"] = patch,
            "knowledge_version" => doc["knowledge"]["version"] = patch,
            "import_generation" => doc["import"]["import_generation"] = patch,
            _ => unreachable!(),
        }
        let path = work.join(format!("authority-{name}.json"));
        write_json(&path, &doc);
        let audit_path = work.join(format!("missing-audit-{name}"));
        let ran = cli(&[
            "query",
            "--authority",
            path.to_str().unwrap(),
            "--query",
            work.join("success.json").to_str().unwrap(),
        ])
        .await;
        assert_clean(&ran, &login_pass);
        assert_eq!(ran.code, 9, "{name} stderr={}", ran.stderr);
        assert_eq!(ran.stderr, "operator-preview: pack\n");
        no_rows(&ran);
        assert_eq!(ran.json.as_ref().unwrap()["class"], json!("pack_mismatch"));
        assert!(!audit_path.exists(), "{name} opened the audit spool");
        proof.set(&format!("mismatch_{name}"), json!(ran.code));
    }
    assert_eq!(login_backends(&admin).await, before);

    let mut source_doc = authority_value(
        &work,
        &pack,
        &receipt,
        &ca_path,
        &login_pass_path,
        &work.join("missing-audit-source"),
    );
    source_doc["project"] = json!("otherbulk");
    source_doc["import"]["source_id"] = json!("otherbulk");
    let source_path = work.join("authority-source.json");
    write_json(&source_path, &source_doc);
    expect_pack(
        &source_path,
        &work.join("success.json"),
        &work.join("missing-audit-source"),
        &login_pass,
    )
    .await;
    proof.set("mismatch_source", json!(9));

    write_json(
        &work.join("policy-preview-sample.json"),
        &policy_table(
            "preview",
            "sample",
            &[
                ("n_i8", "int8"),
                ("n_text", "text"),
                ("n_flag", "bool"),
                ("amount", "numeric"),
                ("note", "text"),
            ],
        ),
    );
    let sample_audit = work.join("missing-audit-sample");
    let mut sample_doc = authority_value(&work, &pack, &receipt, &ca_path, &login_pass_path, &sample_audit);
    sample_doc["policy_path"] = json!(work.join("policy-preview-sample.json").to_string_lossy());
    let sample_authority = work.join("authority-sample.json");
    write_json(&sample_authority, &sample_doc);
    expect_pack(
        &sample_authority,
        &work.join("absent-table-query.json"),
        &sample_audit,
        &login_pass,
    )
    .await;
    proof.set("mismatch_table_absent", json!(9));

    write_json(
        &work.join("policy-narrow.json"),
        &policy_table(NAMESPACE, TABLE, &[("id", "int8"), ("qty", "int8"), ("amount", "numeric")]),
    );
    let narrow_audit = work.join("missing-audit-narrow");
    let mut narrow_doc = authority_value(&work, &pack, &receipt, &ca_path, &login_pass_path, &narrow_audit);
    narrow_doc["policy_path"] = json!(work.join("policy-narrow.json").to_string_lossy());
    let narrow_authority = work.join("authority-narrow.json");
    write_json(&narrow_authority, &narrow_doc);
    expect_pack(
        &narrow_authority,
        &work.join("success.json"),
        &narrow_audit,
        &login_pass,
    )
    .await;
    proof.set("mismatch_column_narrow", json!(9));

    write_json(
        &work.join("policy-batch-a.json"),
        &policy_table(
            "public",
            "batch_a",
            &[("id", "int8"), ("amount", "numeric"), ("label", "text")],
        ),
    );
    let batch_a_audit = work.join("missing-audit-batch-a");
    let mut batch_a_doc =
        authority_value(&work, &pack, &receipt, &ca_path, &login_pass_path, &batch_a_audit);
    batch_a_doc["policy_path"] = json!(work.join("policy-batch-a.json").to_string_lossy());
    let batch_a_authority = work.join("authority-batch-a.json");
    write_json(&batch_a_authority, &batch_a_doc);
    expect_pack(
        &batch_a_authority,
        &work.join("success.json"),
        &batch_a_audit,
        &login_pass,
    )
    .await;
    proof.set("mismatch_table_not_role_visible", json!(9));

    write_json(
        &work.join("policy-secret.json"),
        &policy_table(
            NAMESPACE,
            TABLE,
            &[
                ("id", "int8"),
                ("qty", "int8"),
                ("amount", "numeric"),
                ("note", "text"),
                ("secret", "text"),
            ],
        ),
    );
    let secret_audit = work.join("missing-audit-secret");
    let mut secret_doc = authority_value(&work, &pack, &receipt, &ca_path, &login_pass_path, &secret_audit);
    secret_doc["policy_path"] = json!(work.join("policy-secret.json").to_string_lossy());
    let secret_authority = work.join("authority-secret.json");
    write_json(&secret_authority, &secret_doc);
    expect_pack(
        &secret_authority,
        &work.join("secret-query.json"),
        &secret_audit,
        &login_pass,
    )
    .await;
    proof.set("mismatch_hidden_column", json!(9));

    let mutated = work.join("pack-mutated");
    copy_dir(&pack, &mutated);
    let readme = mutated.join("README.md");
    let original_readme = fs::read(pack.join("README.md")).expect("original readme");
    let mut mutated_readme = fs::read_to_string(&readme).expect("mutated readme");
    mutated_readme.push_str("\nGRANT SELECT (secret) ON public.batch_c;\n");
    fs::write(&readme, mutated_readme).expect("write mutated readme");
    assert_ne!(fs::read(&readme).expect("reread mutated"), original_readme);
    assert_eq!(fs::read(pack.join("README.md")).expect("original stays"), original_readme);
    let mutated_audit = work.join("missing-audit-pack-text");
    let mutated_doc = authority_value(
        &work,
        &mutated,
        &receipt,
        &ca_path,
        &login_pass_path,
        &mutated_audit,
    );
    let mutated_authority = work.join("authority-pack-text.json");
    write_json(&mutated_authority, &mutated_doc);
    expect_pack(
        &mutated_authority,
        &work.join("secret-query.json"),
        &mutated_audit,
        &login_pass,
    )
    .await;
    proof.set("pack_text_not_sql", json!(9));

    let (batch_effective, batch_visibility) = role_pack_documents(
        "public",
        "public.batch_a",
        &["id", "amount", "label"],
        &[],
    );
    let batch_pack = compile_role_pack(&batch_effective, &batch_visibility)
        .unwrap_or_else(|err| panic!("batch_a role pack compile failed: {err}"));
    let unmapped_dir = work.join("role-pack-unmapped");
    fs::create_dir_all(&unmapped_dir).expect("unmapped role dir");
    fs::write(unmapped_dir.join("batch-a.json"), &batch_pack.artifact).expect("unmapped artifact");
    let unmapped_audit = work.join("missing-audit-mapping");
    let mut unmapped_doc =
        authority_value(&work, &pack, &receipt, &ca_path, &login_pass_path, &unmapped_audit);
    unmapped_doc["policy_path"] = json!(work.join("policy-batch-a.json").to_string_lossy());
    unmapped_doc["role_pack_dir"] = json!(unmapped_dir.to_string_lossy());
    let unmapped_authority = work.join("authority-unmapped.json");
    write_json(&unmapped_authority, &unmapped_doc);
    let unmapped_described = cli(&[
        "describe",
        "--authority",
        unmapped_authority.to_str().unwrap(),
    ])
    .await;
    assert_clean(&unmapped_described, &login_pass);
    assert_eq!(unmapped_described.code, 0, "unmapped describe stderr={}", unmapped_described.stderr);
    assert_eq!(
        unmapped_described.json.as_ref().unwrap()["knowledge"]["dataset"],
        json!("public.batch_a")
    );
    let before_mapping = login_backends(&admin).await;
    expect_pack(
        &unmapped_authority,
        &work.join("success.json"),
        &unmapped_audit,
        &login_pass,
    )
    .await;
    assert_eq!(login_backends(&admin).await, before_mapping);
    proof.set("mismatch_mapping_absent", json!(9));

    let policy_query = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("policy-query.json").to_str().unwrap(),
    ])
    .await;
    assert_eq!(policy_query.code, 3);
    assert!(policy_query.stdout.is_empty());
    proof.set("policy_in_query_exit", json!(3));

    let hidden = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("hidden.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&hidden, &login_pass);
    assert_eq!(hidden.code, 6, "hidden stderr={}", hidden.stderr);
    no_rows(&hidden);
    assert_eq!(hidden.json.as_ref().unwrap()["class"], json!("guard"));
    let inspected = inspect(&audit).expect("inspect refusal");
    assert!(inspected.frames.iter().any(|frame| frame.kind == "refusal"));
    proof.set("hidden_exit", json!(6));

    let first = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&first, &login_pass);
    assert_eq!(first.code, 0, "query stderr={}", first.stderr);
    let first_json = first.json.expect("query json");
    labels_ok(&first_json);
    gold_rows(&first_json);
    assert_eq!(first_json["outcome"], json!("succeeded"));
    assert_eq!(first_json["truncated"], json!(false));
    let first_id = execution_of(&first_json);
    let receipt_text = fs::read_to_string(work.join("receipts").join(format!("{first_id}.json")))
        .expect("receipt");
    assert_eq!(receipt_text, first.stdout.trim_end());
    let reopened = inspect(&audit).expect("inspect reopen");
    assert!(reopened.frames.iter().any(|frame| frame.kind == "admission"));
    assert!(reopened.frames.iter().any(|frame| frame.kind == "completion"));
    assert!(reopened.frames.iter().any(|frame| frame.kind == "refusal"));
    proof.set("query_exit", json!(0));
    proof.set("query_rows", first_json["rows"].clone());

    let second = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("success.json").to_str().unwrap(),
    ])
    .await;
    assert_eq!(second.code, 0, "second stderr={}", second.stderr);
    let second_json = second.json.expect("second json");
    gold_rows(&second_json);
    let second_id = execution_of(&second_json);
    assert_ne!(first_id, second_id);
    assert!(first_id.contains('-') && second_id.contains('-'));
    proof.set("distinct_execution_ids", json!([first_id, second_id]));

    let write = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("write.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&write, &login_pass);
    assert_eq!(write.code, 6, "write stderr={}", write.stderr);
    no_rows(&write);
    proof.set("write_exit", json!(6));

    let cast = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("cast.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&cast, &login_pass);
    assert!(cast.code == 6 || cast.code == 8, "cast exit {}", cast.code);
    no_rows(&cast);
    proof.set("cast_exit", json!(cast.code));
    proof.set(
        "cast_class",
        cast.json
            .as_ref()
            .and_then(|value| value.get("class").cloned())
            .unwrap_or(Value::Null),
    );

    let limited = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("rows.json").to_str().unwrap(),
    ])
    .await;
    assert_eq!(limited.code, 0, "row limit stderr={}", limited.stderr);
    let limited_json = limited.json.expect("row limit");
    assert_eq!(limited_json["truncated"], json!(true));
    assert_eq!(limited_json["returned_rows"], json!(1));
    assert_eq!(limited_json["rows"][0][0]["value"], json!(GOLD_MIN));
    proof.set("row_limit_exit", json!(0));

    let bytes = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("bytes.json").to_str().unwrap(),
    ])
    .await;
    assert_eq!(bytes.code, 0, "byte limit stderr={}", bytes.stderr);
    let bytes_json = bytes.json.expect("byte limit");
    assert_eq!(bytes_json["truncated"], json!(true));
    assert_eq!(bytes_json["rows"].as_array().expect("byte rows").len(), 1);
    assert_eq!(bytes_json["rows"][0][0]["value"], json!(GOLD_MIN));
    proof.set("byte_limit_exit", json!(0));

    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let locker = connect(&host, DATABASE, ADMIN, &admin_pass, &ca).await;
    let lock_task = tokio::spawn(hold_lock(locker, shadow.table.clone(), release_rx));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let wall = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("wall.json").to_str().unwrap(),
    ])
    .await;
    let _ = release_tx.send(());
    let _ = lock_task.await;
    assert_clean(&wall, &login_pass);
    assert_eq!(wall.code, 8, "wall stderr={}", wall.stderr);
    no_rows(&wall);
    assert_eq!(wall.json.as_ref().unwrap()["outcome"], json!("timeout"));
    proof.set("wall_exit", json!(8));

    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let locker = connect(&host, DATABASE, ADMIN, &admin_pass, &ca).await;
    let lock_task = tokio::spawn(hold_lock(locker, shadow.table.clone(), release_rx));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let authority_text = authority.to_str().unwrap().to_string();
    let cancel_query = work.join("cancel.json").to_str().unwrap().to_string();
    let query_task = tokio::task::spawn_blocking(move || {
        run_cli(&[
            "query".to_string(),
            "--authority".to_string(),
            authority_text,
            "--query".to_string(),
            cancel_query,
        ])
    });
    let mut running = None;
    for _ in 0..200 {
        let dir = work.join("state/executions");
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let Ok(text) = fs::read_to_string(entry.path()) else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if value.get("status").and_then(Value::as_str) == Some("running") {
                    running = Some(value);
                    break;
                }
            }
        }
        if running.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let running = running.expect("cancel target did not stay running");
    let pid = running["backend_pid"].as_i64().expect("pid") as i32;
    let mut saw_active = false;
    for _ in 0..40 {
        match observe_backend(&admin, pid).await {
            Ok(Some(view)) if view.state() == "active" => {
                saw_active = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(saw_active, "cancel backend was not active");
    let cancel_id = running["execution_id"].as_str().unwrap().to_string();
    let authority_text = authority.to_str().unwrap().to_string();
    let cancel_ran = tokio::task::spawn_blocking(move || {
        run_cli(&[
            "cancel".to_string(),
            "--authority".to_string(),
            authority_text,
            "--execution".to_string(),
            cancel_id.clone(),
        ])
    })
    .await
    .expect("cancel join");
    let query_ran = tokio::time::timeout(Duration::from_secs(30), query_task)
        .await
        .expect("cancel query hung")
        .expect("query join");
    let _ = release_tx.send(());
    let _ = lock_task.await;
    assert_clean(&query_ran, &login_pass);
    assert_clean(&cancel_ran, &login_pass);
    assert_eq!(cancel_ran.code, 0, "cancel stderr={}", cancel_ran.stderr);
    assert_eq!(cancel_ran.json.as_ref().unwrap()["outcome"], json!("sent"));
    assert_eq!(query_ran.code, 0, "canceled query stderr={}", query_ran.stderr);
    let canceled = query_ran.json.as_ref().unwrap();
    assert_eq!(canceled["outcome"], json!("canceled"));
    assert_eq!(canceled["sqlstate"], json!("57014"));
    assert_eq!(canceled["rollback_confirmed"], json!(true));
    no_rows(&query_ran);
    proof.set("cancel_exit", json!(0));
    proof.set("cancel_sqlstate", json!("57014"));
    proof.set("cancel_generation", canceled["generation"].clone());
    proof.set("backend_active_before_cancel", json!(saw_active));

    let late_id = execution_of(&first_json);
    let late = cli(&[
        "cancel",
        "--authority",
        authority.to_str().unwrap(),
        "--execution",
        &late_id,
    ])
    .await;
    assert_eq!(late.code, 10, "late stderr={}", late.stderr);
    assert!(!work.join("state/cancel").join(format!("{late_id}.request")).exists());
    proof.set("late_cancel_exit", json!(10));
    let after_late = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("success.json").to_str().unwrap(),
    ])
    .await;
    assert_eq!(after_late.code, 0, "after late stderr={}", after_late.stderr);
    let after_json = after_late.json.expect("after late");
    gold_rows(&after_json);
    assert_ne!(execution_of(&after_json), late_id);
    proof.set("after_late_exit", json!(0));

    let mut quota_doc = authority_value(
        &work,
        &pack,
        &receipt,
        &ca_path,
        &login_pass_path,
        &work.join("audit-quota"),
    );
    quota_doc["audit_max_bytes"] = json!(64);
    quota_doc["state_dir"] = json!(work.join("state-quota").to_string_lossy());
    quota_doc["receipt_dir"] = json!(work.join("receipts-quota").to_string_lossy());
    let quota_authority = work.join("authority-quota.json");
    write_json(&quota_authority, &quota_doc);
    let quota = cli(&[
        "query",
        "--authority",
        quota_authority.to_str().unwrap(),
        "--query",
        work.join("success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&quota, &login_pass);
    assert_eq!(quota.code, 7, "audit stderr={}", quota.stderr);
    no_rows(&quota);
    let fault = fs::read_to_string(work.join("audit-quota/last-fault.txt")).unwrap_or_default();
    assert!(fault.contains("quota"), "fault={fault}");
    proof.set("audit_exit", json!(7));
    proof.set("audit_fault", json!(fault.trim()));

    prove_generation(&host, &login_pass, &ca, &admin, &shadow.table, &work, &mut proof).await;
    proof.set("generation_proved", json!(true));
    // The stale generation below cannot be undone. The native fixture uses a
    // different source and digest, and it has to be registered first.
    prove_native_consumer(
        &host,
        &mut admin,
        &login_pass,
        &ca,
        &ca_path,
        &login_pass_path,
        &work,
        &mut proof,
    )
    .await;
    register(&admin, 3, "namespace", "n", NAMESPACE, "", "").await;
    register(&admin, 3, "table", "t", NAMESPACE, TABLE, "").await;
    register(&admin, 3, "column", "c", NAMESPACE, TABLE, "id").await;
    let stale_audit = work.join("missing-audit-stale-generation");
    let stale_doc = authority_value(
        &work,
        &pack,
        &receipt,
        &ca_path,
        &login_pass_path,
        &stale_audit,
    );
    let stale_authority = work.join("authority-stale.json");
    write_json(&stale_authority, &stale_doc);
    expect_pack(
        &stale_authority,
        &work.join("success.json"),
        &stale_audit,
        &login_pass,
    )
    .await;
    proof.set("mismatch_stale_generation", json!(9));
    proof.set("source_core_5bd6", json!("not_consumed"));
    proof.set("status", json!("proved"));
}

async fn prove_generation(
    host: &str,
    login_pass: &str,
    ca: &[u8],
    admin: &Client,
    table: &str,
    work: &Path,
    proof: &mut Proof,
) {
    let policy = load_policy(&work.join("policy.json")).expect("policy");
    let compiled = compile_column_acl(&policy.acl).expect("compile");
    let identity = PolicyIdentity {
        project: SOURCE.to_string(),
        role: "oo_r_reader".to_string(),
        policy_id: CONFIG.to_string(),
    };
    let limits = CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 1,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows: UnknownRows::Zero,
    };
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port: 5432,
        database: DATABASE.to_string(),
        user: LOGIN.to_string(),
    };
    let tls = rustls_connector(ca).expect("connector");
    let (mut client, mut connection) = connect_require_tls(&endpoint, login_pass, ca)
        .await
        .unwrap_or_else(|err| connect_panic(&err));
    let catalog = {
        let mut load = std::pin::pin!(load_policy_catalog(
            &mut client,
            &identity,
            std::slice::from_ref(&compiled),
            &limits,
        ));
        tokio::select! {
            biased;
            result = &mut load => result.expect("catalog"),
            _ = &mut connection => panic!("catalog connection closed"),
        }
    };
    let mut session = DedicatedSession::bind(client, connection, tls)
        .await
        .expect("bind");
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let locker = connect(host, DATABASE, ADMIN, &read_pass(&PathBuf::from(require_env("OO_DL01W121_ADMIN_PASS"))), ca).await;
    let lock_task = tokio::spawn(hold_lock(locker, table.to_string(), release_rx));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let sql = "SELECT id, qty, amount, note FROM public.batch_c ORDER BY id";
    let mut stream = session
        .execute_trusted_select(
            SelectRequest {
                identity: &identity,
                catalog: &catalog,
                original_sql: sql,
                role: &identity.role,
                max_rows: 10,
                max_result_bytes: 8192,
                wall_clock: Duration::from_millis(20_000),
            },
            open_wal(work),
        )
        .await
        .expect("generation stream");
    let during = session.generation();
    let handle = stream.cancel_handle();
    let pid = session.backend_pid();
    let mut active = false;
    for _ in 0..40 {
        if matches!(observe_backend(admin, pid).await, Ok(Some(view)) if view.state() == "active") {
            active = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(active, "generation query was not active");
    let (observer, obs_conn) = connect_require_tls(&endpoint, login_pass, ca)
        .await
        .unwrap_or_else(|err| connect_panic(&err));
    let obs_driver = tokio::spawn(async move {
        let _ = obs_conn.await;
    });
    let sent = handle.cancel(&observer).await.expect("cancel handle");
    assert_eq!(sent, CancelOutcome::Sent);
    let mut saw_cancel = false;
    while let Some(item) = stream.next_row().await.transpose() {
        match item {
            Ok(_) => {}
            Err(ExecError::Canceled { sqlstate }) => {
                assert_eq!(sqlstate, "57014");
                saw_cancel = true;
                break;
            }
            Err(err) => panic!("generation read ended as {err:?}"),
        }
    }
    assert!(saw_cancel, "generation cancel was not observed");
    drop(stream);
    session.wait_idle().await.expect("idle");
    let summary = session.summary().expect("summary");
    assert_eq!(summary.stop(), Some(StopClass::Canceled));
    assert!(summary.rollback_confirmed());
    assert_eq!(summary.sqlstate(), Some("57014"));
    assert_ne!(summary.status(), CompletionStatus::Succeeded);
    let after = session.generation();
    assert!(after > during, "generation did not advance");
    let late = handle.cancel(&observer).await.expect("late handle");
    assert_eq!(late, CancelOutcome::NotSent);
    let _ = release_tx.send(());
    let _ = lock_task.await;
    // A stopped query sets reuse to false and closes this dedicated session.
    // The next read is a new session. The old handle still names the old backend.
    drop(session);
    let tls = rustls_connector(ca).expect("connector");
    let (client, connection) = connect_require_tls(&endpoint, login_pass, ca)
        .await
        .unwrap_or_else(|err| connect_panic(&err));
    let mut session = DedicatedSession::bind(client, connection, tls)
        .await
        .expect("rebind");
    let mut next = session
        .execute_trusted_select(
            SelectRequest {
                identity: &identity,
                catalog: &catalog,
                original_sql: sql,
                role: &identity.role,
                max_rows: 10,
                max_result_bytes: 8192,
                wall_clock: Duration::from_millis(5_000),
            },
            open_wal(work),
        )
        .await
        .expect("next stream");
    let next_generation = session.generation();
    let next_pid = session.backend_pid();
    assert_ne!(next_pid, pid);
    let mut count = 0;
    while let Some(row) = next.next_row().await.expect("next row") {
        count += 1;
        assert_eq!(row.len(), 4);
    }
    assert_eq!(count, 2);
    drop(next);
    session.wait_idle().await.expect("next idle");
    let still = handle.cancel(&observer).await.expect("old handle");
    assert_eq!(still, CancelOutcome::NotSent);
    drop(session);
    drop(observer);
    obs_driver.abort();
    proof.set("generation_during", json!(during));
    proof.set("generation_after", json!(after));
    proof.set("generation_next", json!(next_generation));
    proof.set("generation_next_pid_differs", json!(true));
}

fn open_wal(work: &Path) -> gateway_audit_wal::DiskWal {
    let dir = work.join("audit-generation");
    gateway_audit_wal::DiskWal::open(
        &dir,
        gateway_audit_wal::WalConfig {
            max_bytes: 67_108_864,
        },
    )
    .expect("generation wal")
}

const NATIVE_SOURCE: &str = "oo_dl01w1_44";
const NATIVE_CONFIG: &str = "9109a02440e61d68175e85b01a2a2cd62f7bdc8f97fc9ac0b40a430c2d7ab718";
const NATIVE_HASH: &str = "2e9dc3510a50fefecae55a3e472b96ec6b02ad2f01dfb9b6ced59e1906a3f95c";
const NATIVE_SNAPSHOT: &str = "3044ba35-ba44-46db-b0e5-50f71c78a3a8";
const NATIVE_MANIFEST: &str = "f8e423f743214b740c5219b109a0892a4f102c98e50f520003cdeb669a6fe019";
const NATIVE_GENERATION: i64 = 1;
const NATIVE_NAMESPACE: &str = "oo_dl01w1_44";
const NATIVE_TABLE: &str = "synthetic_docs";
const NATIVE_DATASET: &str = "oo_dl01w1_44.synthetic_docs";
const NATIVE_TYPE: &str = "a044a044a044a044a044a044a044a044a044a044a044a044a044a044a044a044";
const NATIVE_INPUT: &str = "b044b044b044b044b044b044b044b044b044b044b044b044b044b044b044b044";
const NATIVE_SCHEMA: &str = "c044c044c044c044c044c044c044c044c044c044c044c044c044c044c044c044";

const NATIVE_FIELDS: &[(&str, i64, &str)] = &[
    ("source_doc_id", 1043, "varchar"),
    ("source_index", 1043, "varchar"),
    ("doc_key", 1043, "varchar"),
    ("double_value", 701, "float8"),
    ("event_ms", 1184, "timestamptz"),
    ("flag", 16, "bool"),
    ("long_value", 20, "int8"),
    ("note", 1043, "varchar"),
    ("payload_json", 1043, "varchar"),
    ("quantities", 1043, "varchar"),
    ("scaled_value", 20, "int8"),
    ("seq_key", 20, "int8"),
    ("tags", 114, "json"),
    ("thing", 1043, "varchar"),
];

const BYTEA_TABLE: &str = "byte_docs";
const BYTEA_DATASET: &str = "oo_dl01w1_44.byte_docs";
const BYTEA_FIELDS: &[&str] = &[
    "note",
    "code",
    "code_or_null",
    "label",
    "blob",
    "uid",
    "object_id",
];
const BYTEA_HIDDEN: &str = "sealed";
const CHAR_COLUMN: &str = "c6";
const UUID_COLUMN: &str = "row_uid";
const DATE_COLUMN: &str = "day";
const TIMESTAMP_COLUMN: &str = "d";
const JSONB_COLUMN: &str = "doc";

fn native_pack_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-product/native-run/builder/out/pack",
    )
}

async fn register_at(
    client: &Client,
    source: &str,
    config: &str,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn tokio_postgres::types::ToSql + Sync); 7] = [
        &source,
        &config,
        &generation,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let digest: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .unwrap_or_else(|err| panic!("native mapping digest sqlstate={}", sqlstate(&err)))
        .get(0);
    let bind: [&(dyn tokio_postgres::types::ToSql + Sync); 8] = [
        &source,
        &config,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
        &digest,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .unwrap_or_else(|err| panic!("native register sqlstate={}", sqlstate(&err)));
}

async fn lookup_at(
    client: &Client,
    source: &str,
    config: &str,
    generation: i64,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> String {
    let params: [&(dyn tokio_postgres::types::ToSql + Sync); 7] = [
        &source,
        &config,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
    ];
    client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .unwrap_or_else(|err| panic!("native lookup sqlstate={}", sqlstate(&err)))
        .get(0)
}

fn native_role_documents() -> (String, String) {
    let mut visible = Vec::new();
    let mut columns = Vec::new();
    for (name, _, _) in NATIVE_FIELDS {
        visible.push(*name);
        columns.push(json!({"name": name, "pii": "none", "classification": {"state": "confirmed"}}));
    }
    columns.push(json!({"name": "raw_source", "pii": "sensitive", "classification": {"state": "confirmed"}}));
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": NATIVE_SOURCE,
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-native", "version": "1"},
        "knowledge": {"id": "know-native", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "acme",
            "locale": "zh-CN",
            "include_schemas": [NATIVE_NAMESPACE],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [{
            "schema_version": "oos/v1",
            "dataset": NATIVE_DATASET,
            "label": "Synthetic",
            "tier": "file",
            "grain": "one row",
            "primary_key": ["source_doc_id"],
            "columns": columns
        }]
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": NATIVE_SOURCE,
        "principal": "operator",
        "role": "oo_r_reader",
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-native", "version": "1"},
        "knowledge": {"id": "know-native", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": NATIVE_DATASET,
            "row_security": "none",
            "global_stats": "denied",
            "columns": visible
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (
        serde_json::to_string(&effective).expect("native effective"),
        serde_json::to_string(&visibility).expect("native visibility"),
    )
}

fn native_policy() -> Value {
    let columns: Vec<Value> = NATIVE_FIELDS
        .iter()
        .map(|(name, _, semantic)| {
            json!({
                "name": name,
                "status": "confirmed",
                "semantic_type": semantic,
                "pii": {"class": "none"}
            })
        })
        .collect();
    json!({
        "protocol": "oo.operator-preview.policy/v1",
        "project": NATIVE_SOURCE,
        "config_digest": NATIVE_CONFIG,
        "generation": NATIVE_GENERATION,
        "role": "reader",
        "login": LOGIN,
        "table": {
            "namespace": NATIVE_NAMESPACE,
            "name": NATIVE_TABLE,
            "columns": columns
        }
    })
}

fn native_authority(work: &Path, pack: &Path, ca: &Path, password: &Path, host: &str, audit: &Path) -> Value {
    json!({
        "protocol": "oo.operator-preview.config/v1",
        "authority": {"kind": "trusted-operator-file", "binds": "import+mapping+role-policy+pack+target-tls"},
        "project": NATIVE_SOURCE,
        "role": "oo_r_reader",
        "policy_pack_id": "pol-native",
        "policy_version": "1",
        "knowledge": {"id": "know-native", "version": "1"},
        "import": {
            "import_generation": NATIVE_GENERATION,
            "import_hash": NATIVE_HASH,
            "snapshot_id": NATIVE_SNAPSHOT,
            "source_id": NATIVE_SOURCE,
            "manifest_sha256": NATIVE_MANIFEST,
            "target_config_sha256": NATIVE_CONFIG
        },
        "policy_path": work.join("native-policy.json").to_string_lossy(),
        "pack_dir": pack.to_string_lossy(),
        "pack_receipt": pack.join("receipt.json").to_string_lossy(),
        "role_pack_dir": work.join("native-role-pack").to_string_lossy(),
        "target": {
            "host": host,
            "port": 5432,
            "database": DATABASE,
            "user": LOGIN,
            "ca_path": ca.to_string_lossy(),
            "password_path": password.to_string_lossy()
        },
        "state_dir": work.join("native-state").to_string_lossy(),
        "audit_dir": audit.to_string_lossy(),
        "receipt_dir": work.join("native-receipts").to_string_lossy(),
        "audit_max_bytes": 67_108_864,
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "zero"
        }
    })
}

fn native_sql() -> &'static str {
    "SELECT source_doc_id, source_index, doc_key, double_value, event_ms, flag, long_value, note, payload_json, quantities, scaled_value, seq_key, tags, thing FROM oo_dl01w1_44.synthetic_docs ORDER BY source_doc_id"
}

async fn prove_native_consumer(
    host: &str,
    admin: &mut Client,
    login_pass: &str,
    ca: &[u8],
    ca_path: &Path,
    login_pass_path: &Path,
    work: &Path,
    proof: &mut Proof,
) {
    let pack = native_pack_dir();
    let receipt = pack.join("receipt.json");
    assert!(pack.is_dir(), "pinned native pack is missing");
    assert!(receipt.is_file(), "pinned native receipt is missing");
    let receipt_value: Value = serde_json::from_str(&fs::read_to_string(&receipt).expect("receipt")).expect("receipt json");
    assert_eq!(receipt_value["binding"]["source_id"], json!(NATIVE_SOURCE));
    assert_eq!(receipt_value["binding"]["target_config_sha256"], json!(NATIVE_CONFIG));
    assert_eq!(receipt_value["publication"], json!("not_published"));
    proof.set("native_pack_digest", receipt_value["pack_digest"].clone());
    proof.set("native_source", json!(NATIVE_SOURCE));

    let target: i64 = admin
        .query_one(
            "SELECT generation FROM _oo.full_import_session WHERE shard_id = $1",
            &[&SHARD],
        )
        .await
        .unwrap_or_else(|err| panic!("native target sqlstate={}", sqlstate(&err)))
        .get(0);
    register_at(admin, NATIVE_SOURCE, NATIVE_CONFIG, NATIVE_GENERATION, "namespace", "n", NATIVE_NAMESPACE, "", "").await;
    register_at(admin, NATIVE_SOURCE, NATIVE_CONFIG, NATIVE_GENERATION, "table", "t", NATIVE_NAMESPACE, NATIVE_TABLE, "").await;
    for (name, _, _) in NATIVE_FIELDS {
        register_at(
            admin,
            NATIVE_SOURCE,
            NATIVE_CONFIG,
            NATIVE_GENERATION,
            "column",
            "c",
            NATIVE_NAMESPACE,
            NATIVE_TABLE,
            name,
        )
        .await;
    }
    register_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        "c",
        NATIVE_NAMESPACE,
        NATIVE_TABLE,
        "raw_source",
    )
    .await;
    for column in ["amount", "wide"] {
        register_at(
            admin,
            NATIVE_SOURCE,
            NATIVE_CONFIG,
            NATIVE_GENERATION,
            "column",
            "c",
            NATIVE_NAMESPACE,
            NATIVE_TABLE,
            column,
        )
        .await;
    }
    register_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "table",
        "t",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        "",
    )
    .await;
    for name in BYTEA_FIELDS {
        register_at(
            admin,
            NATIVE_SOURCE,
            NATIVE_CONFIG,
            NATIVE_GENERATION,
            "column",
            "c",
            NATIVE_NAMESPACE,
            BYTEA_TABLE,
            name,
        )
        .await;
    }
    register_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        "c",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        BYTEA_HIDDEN,
    )
    .await;
    for name in [CHAR_COLUMN, UUID_COLUMN, DATE_COLUMN, TIMESTAMP_COLUMN, JSONB_COLUMN] {
        register_at(
            admin,
            NATIVE_SOURCE,
            NATIVE_CONFIG,
            NATIVE_GENERATION,
            "column",
            "c",
            NATIVE_NAMESPACE,
            BYTEA_TABLE,
            name,
        )
        .await;
    }
    let physical_table = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "table",
        NATIVE_NAMESPACE,
        NATIVE_TABLE,
        "",
    )
    .await;
    let mut physical_columns = Vec::new();
    for (name, _, _) in NATIVE_FIELDS {
        physical_columns.push(
            lookup_at(
                admin,
                NATIVE_SOURCE,
                NATIVE_CONFIG,
                NATIVE_GENERATION,
                "column",
                NATIVE_NAMESPACE,
                NATIVE_TABLE,
                name,
            )
            .await,
        );
    }
    let amount_column = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        NATIVE_NAMESPACE,
        NATIVE_TABLE,
        "amount",
    )
    .await;
    let raw_column = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        NATIVE_NAMESPACE,
        NATIVE_TABLE,
        "raw_source",
    )
    .await;
    let byte_table = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "table",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        "",
    )
    .await;
    let mut byte_columns = Vec::new();
    for name in BYTEA_FIELDS {
        byte_columns.push(
            lookup_at(
                admin,
                NATIVE_SOURCE,
                NATIVE_CONFIG,
                NATIVE_GENERATION,
                "column",
                NATIVE_NAMESPACE,
                BYTEA_TABLE,
                name,
            )
            .await,
        );
    }
    let sealed_column = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        BYTEA_HIDDEN,
    )
    .await;
    let char_column = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        CHAR_COLUMN,
    )
    .await;
    let uuid_column = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        UUID_COLUMN,
    )
    .await;
    let date_column = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        DATE_COLUMN,
    )
    .await;
    let timestamp_column = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        TIMESTAMP_COLUMN,
    )
    .await;
    let doc_column = lookup_at(
        admin,
        NATIVE_SOURCE,
        NATIVE_CONFIG,
        NATIVE_GENERATION,
        "column",
        NATIVE_NAMESPACE,
        BYTEA_TABLE,
        JSONB_COLUMN,
    )
    .await;
    let schemas = vec![NATIVE_NAMESPACE.to_string(), NATIVE_NAMESPACE.to_string()];
    let tables = vec![NATIVE_TABLE.to_string(), BYTEA_TABLE.to_string()];
    let physical = vec![physical_table.clone(), byte_table.clone()];
    let source_generation = NATIVE_GENERATION;
    let run_id: String = admin
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &[
                &SHARD,
                &OWNER,
                &target,
                &NATIVE_SOURCE,
                &source_generation,
                &NATIVE_CONFIG,
                &NATIVE_TYPE,
                &NATIVE_INPUT,
                &NATIVE_SCHEMA,
                &schemas,
                &tables,
                &physical,
            ],
        )
        .await
        .unwrap_or_else(|err| panic!("native open run sqlstate={}", sqlstate(&err)))
        .get(0);
    let mut idents = Vec::new();
    let mut oids = Vec::new();
    let mut typmods = Vec::new();
    let mut required = Vec::new();
    for (name, oid, _) in NATIVE_FIELDS {
        idents.push((*name).to_string());
        oids.push(*oid);
        typmods.push(-1_i32);
        required.push(*name != "note");
    }
    idents.push("raw_source".to_string());
    oids.push(17_i64);
    typmods.push(-1_i32);
    required.push(true);
    idents.push("amount".to_string());
    oids.push(1700_i64);
    typmods.push(4_980_750);
    required.push(false);
    idents.push("wide".to_string());
    oids.push(1700_i64);
    typmods.push(((77 << 16) | 10) + 4);
    required.push(false);
    let index = 0_i32;
    let prepared: String = admin
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &[
                &SHARD,
                &OWNER,
                &target,
                &run_id,
                &index,
                &NATIVE_SOURCE,
                &NATIVE_CONFIG,
                &source_generation,
                &NATIVE_NAMESPACE,
                &NATIVE_TABLE,
                &idents,
                &oids,
                &typmods,
                &required,
            ],
        )
        .await
        .unwrap_or_else(|err| panic!("native shadow sqlstate={}", sqlstate(&err)))
        .get(0);
    assert_eq!(prepared, physical_table);
    insert_native_rows(
        admin,
        &physical_table,
        &physical_columns,
        &raw_column,
        &amount_column,
    )
    .await;
    exec(
        admin,
        &format!("ANALYZE \"_oo\".{}", quote_physical(&physical_table, "pt_")),
    )
    .await;
    let mut granted = physical_columns
        .iter()
        .map(|name| quote_physical(name, "pc_"))
        .collect::<Vec<_>>();
    granted.push(quote_physical(&raw_column, "pc_"));
    granted.push(quote_physical(&amount_column, "pc_"));
    exec(
        admin,
        &format!(
            "GRANT SELECT ({}) ON TABLE \"_oo\".{} TO \"oo_r_reader\"",
            granted.join(", "),
            quote_physical(&physical_table, "pt_")
        ),
    )
    .await;
    let mut byte_idents = Vec::new();
    let mut byte_oids = Vec::new();
    let mut byte_typmods = Vec::new();
    let mut byte_required = Vec::new();
    for name in BYTEA_FIELDS {
        byte_idents.push((*name).to_string());
        byte_oids.push(17_i64);
        byte_typmods.push(-1_i32);
        byte_required.push(false);
    }
    byte_idents.push(BYTEA_HIDDEN.to_string());
    byte_oids.push(17_i64);
    byte_typmods.push(-1_i32);
    byte_required.push(false);
    byte_idents.push(CHAR_COLUMN.to_string());
    byte_oids.push(1042_i64);
    byte_typmods.push(10_i32);
    byte_required.push(false);
    byte_idents.push(UUID_COLUMN.to_string());
    byte_oids.push(2950_i64);
    byte_typmods.push(-1_i32);
    byte_required.push(false);
    byte_idents.push(DATE_COLUMN.to_string());
    byte_oids.push(1082_i64);
    byte_typmods.push(-1_i32);
    byte_required.push(false);
    byte_idents.push(TIMESTAMP_COLUMN.to_string());
    byte_oids.push(1114_i64);
    byte_typmods.push(-1_i32);
    byte_required.push(false);
    // full_import_type_sql refuses oid 3802. The sealed name is created as json,
    // then altered to jsonb before any row is inserted.
    byte_idents.push(JSONB_COLUMN.to_string());
    byte_oids.push(114_i64);
    byte_typmods.push(-1_i32);
    byte_required.push(false);
    let byte_index = 1_i32;
    let byte_prepared: String = admin
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &[
                &SHARD,
                &OWNER,
                &target,
                &run_id,
                &byte_index,
                &NATIVE_SOURCE,
                &NATIVE_CONFIG,
                &source_generation,
                &NATIVE_NAMESPACE,
                &BYTEA_TABLE,
                &byte_idents,
                &byte_oids,
                &byte_typmods,
                &byte_required,
            ],
        )
        .await
        .unwrap_or_else(|err| panic!("bytea shadow sqlstate={}", sqlstate(&err)))
        .get(0);
    assert_eq!(byte_prepared, byte_table);
    assert_eq!(column_typmod(admin, &byte_table, &char_column).await, 10);
    assert_eq!(column_typmod(admin, &byte_table, &uuid_column).await, -1);
    assert_eq!(column_typmod(admin, &byte_table, &date_column).await, -1);
    assert_eq!(column_typmod(admin, &byte_table, &timestamp_column).await, -1);
    let import_refused = admin
        .query_one("SELECT _oo.full_import_type_sql(3802, -1)", &[])
        .await
        .expect_err("jsonb copy type was accepted");
    assert_eq!(sqlstate(&import_refused), "22023");
    proof.set("jsonb_import_sqlstate", json!("22023"));
    let doc_physical = quote_physical(&doc_column, "pc_");
    exec(
        admin,
        &format!(
            "ALTER TABLE \"_oo\".{table} ALTER COLUMN {column} TYPE jsonb USING {column}::jsonb",
            table = quote_physical(&byte_table, "pt_"),
            column = doc_physical
        ),
    )
    .await;
    assert_eq!(column_typmod(admin, &byte_table, &doc_column).await, -1);
    assert_eq!(column_oid(admin, &byte_table, &doc_column).await, 3802);
    insert_bytea_rows(
        admin,
        &byte_table,
        &byte_columns,
        &sealed_column,
        &char_column,
        &uuid_column,
        &date_column,
        &timestamp_column,
        &doc_column,
    )
    .await;
    exec(
        admin,
        &format!("ANALYZE \"_oo\".{}", quote_physical(&byte_table, "pt_")),
    )
    .await;
    let mut byte_granted = byte_columns
        .iter()
        .map(|name| quote_physical(name, "pc_"))
        .collect::<Vec<_>>();
    byte_granted.push(quote_physical(&sealed_column, "pc_"));
    byte_granted.push(quote_physical(&char_column, "pc_"));
    byte_granted.push(quote_physical(&uuid_column, "pc_"));
    byte_granted.push(quote_physical(&date_column, "pc_"));
    byte_granted.push(quote_physical(&timestamp_column, "pc_"));
    byte_granted.push(quote_physical(&doc_column, "pc_"));
    exec(
        admin,
        &format!(
            "GRANT SELECT ({}) ON TABLE \"_oo\".{} TO \"oo_r_reader\"",
            byte_granted.join(", "),
            quote_physical(&byte_table, "pt_")
        ),
    )
    .await;

    let (effective, visibility) = native_role_documents();
    let role_pack = compile_role_pack(&effective, &visibility)
        .unwrap_or_else(|err| panic!("native role pack compile failed: {err}"));
    assert!(!role_pack.artifact.contains(SENTINEL));
    let role_dir = work.join("native-role-pack");
    fs::create_dir_all(&role_dir).expect("native role dir");
    fs::write(role_dir.join("preview.json"), &role_pack.artifact).expect("native role artifact");
    write_json(&work.join("native-policy.json"), &native_policy());
    let success_audit = work.join("native-audit-success");
    let authority_doc = native_authority(work, &pack, ca_path, login_pass_path, host, &success_audit);
    let authority = work.join("native-authority.json");
    write_json(&authority, &authority_doc);
    write_json(
        &work.join("native-success.json"),
        &query_value(native_sql(), 20, 65_536, 10_000),
    );
    write_json(
        &work.join("native-hidden.json"),
        &query_value(
            "SELECT raw_source FROM oo_dl01w1_44.synthetic_docs",
            10,
            8192,
            5000,
        ),
    );
    write_json(
        &work.join("native-cancel.json"),
        &query_value(native_sql(), 20, 65_536, 20_000),
    );

    let policy = load_policy(&work.join("native-policy.json")).expect("native policy");
    let compiled = compile_column_acl(&policy.acl).expect("native compile");
    let identity = PolicyIdentity {
        project: NATIVE_SOURCE.to_string(),
        role: "oo_r_reader".to_string(),
        policy_id: NATIVE_CONFIG.to_string(),
    };
    let limits = CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 1,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows: UnknownRows::Zero,
    };
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port: 5432,
        database: DATABASE.to_string(),
        user: LOGIN.to_string(),
    };
    let (mut client, mut connection) = connect_require_tls(&endpoint, login_pass, ca)
        .await
        .unwrap_or_else(|err| connect_panic(&err));
    let loaded = {
        let mut load = std::pin::pin!(load_policy_catalog(
            &mut client,
            &identity,
            std::slice::from_ref(&compiled),
            &limits,
        ));
        tokio::select! {
            biased;
            result = &mut load => result,
            _ = &mut connection => panic!("native catalog connection closed"),
        }
    };
    drop(client);
    let catalog = loaded.expect("shared catalog admits varchar, float8, timestamptz, and json");
    let type_oids: Vec<u32> = catalog
        .relations
        .iter()
        .flat_map(|relation| relation.columns.iter().map(|column| column.type_oid))
        .collect();
    assert!(type_oids.contains(&1043), "varchar was not admitted");
    assert!(type_oids.contains(&701), "float8 was not admitted");
    assert!(type_oids.contains(&1184), "timestamptz was not admitted");
    assert!(type_oids.contains(&114), "json was not admitted");
    assert!(grantable_base_type("numeric", 4_980_750));
    assert!(!grantable_base_type("numeric", ((77 << 16) | 10) + 4));
    assert!(grantable_base_type("bytea", -1));
    assert!(grantable_base_type("uuid", -1));
    assert!(!grantable_base_type("uuid", 0));
    assert!(!grantable_base_type("uuid", 4));
    assert!(grantable_base_type("bpchar", -1));
    assert!(grantable_base_type("bpchar", 10));
    assert!(!grantable_base_type("bpchar", 4));
    assert!(grantable_base_type("date", -1));
    assert!(!grantable_base_type("date", 0));
    assert!(grantable_base_type("timestamp", -1));
    assert!(!grantable_base_type("timestamp", 0));
    assert!(!grantable_base_type("timestamp", 6));
    assert!(grantable_base_type("jsonb", -1));
    assert!(!grantable_base_type("jsonb", 0));
    assert!(!grantable_base_type("jsonb", 4));
    assert!(grantable_base_type("json", -1));
    assert!(!grantable_base_type("interval", -1));
    assert!(!grantable_base_type("time", -1));
    assert!(!type_oids.contains(&17), "hidden bytea was published on the OpenSearch policy");
    assert!(!type_oids.contains(&2950), "uuid was published on the OpenSearch policy");
    assert!(!type_oids.contains(&1042), "bpchar was published on the OpenSearch policy");
    assert!(!type_oids.contains(&1082), "date was published on the OpenSearch policy");
    assert!(!type_oids.contains(&1114), "timestamp was published on the OpenSearch policy");
    assert!(!type_oids.contains(&3802), "jsonb was published on the OpenSearch policy");
    proof.set("shared_catalog", json!("admitted"));
    proof.set("bytea_grantable", json!(true));
    proof.set("uuid_grantable", json!(true));
    proof.set("bpchar_grantable", json!(true));
    proof.set("date_grantable", json!(true));
    proof.set("timestamp_grantable", json!(true));
    proof.set("jsonb_grantable", json!(true));
    proof.set("json_grantable", json!(true));
    prove_decimal256(
        host,
        admin,
        login_pass,
        ca,
        &physical_table,
        &amount_column,
        work,
        proof,
    )
    .await;

    let described = cli(&["describe", "--authority", authority.to_str().unwrap()]).await;
    assert_clean(&described, login_pass);
    assert_eq!(described.code, 0, "native describe stderr={}", described.stderr);
    let described_json = described.json.expect("native describe");
    assert_eq!(described_json["outcome"], json!("described"));
    assert_eq!(described_json["knowledge"]["dataset"], json!(NATIVE_DATASET));
    let mut names: Vec<&str> = described_json["knowledge"]["columns"]
        .as_array()
        .expect("native columns")
        .iter()
        .map(|column| column["name"].as_str().unwrap_or(""))
        .collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = NATIVE_FIELDS.iter().map(|(name, _, _)| *name).collect();
    expected.sort_unstable();
    assert_eq!(names, expected);
    assert!(!names.contains(&"raw_source"));
    proof.set("native_describe_exit", json!(0));

    let success = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("native-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&success, login_pass);
    assert_eq!(success.code, 0, "native query stderr={}", success.stderr);
    let success_json = success.json.expect("native query");
    assert_eq!(success_json["outcome"], json!("succeeded"));
    assert_eq!(success_json["type_path"], json!("native_contract"));
    assert_eq!(success_json["shared_catalog"], json!("admitted"));
    assert_eq!(success_json["compatibility_audit"], json!("inspect_segment"));
    assert_eq!(success_json["rollback_confirmed"], json!(true));
    assert_eq!(success_json["end_audit_ok"], json!(true));
    assert_eq!(success_json["truncated"], json!(false));
    assert_eq!(success_json["returned_rows"], json!(2));
    assert_native_rows(&success_json);
    let oracle = native_oracle(admin, &physical_table, &physical_columns).await;
    assert_eq!(success_json["rows"][0][3]["bits"], oracle[0]["bits"]);
    assert_eq!(success_json["rows"][1][3]["bits"], oracle[1]["bits"]);
    assert_eq!(
        success_json["rows"][0][3]["bits"],
        json!(format!("{:016x}", 1.5_f64.to_bits()))
    );
    assert_eq!(
        success_json["rows"][1][3]["bits"],
        json!(format!("{:016x}", (-1.5_f64).to_bits()))
    );
    assert_eq!(success_json["rows"][0][4]["value"], oracle[0]["event"]);
    assert_eq!(success_json["rows"][1][4]["value"], oracle[1]["event"]);
    let left_tags: Value = serde_json::from_str(success_json["rows"][0][12]["value"].as_str().unwrap()).expect("tags");
    let right_tags: Value = serde_json::from_str(oracle[0]["tags"].as_str().unwrap()).expect("oracle tags");
    assert_eq!(left_tags, right_tags);
    assert_eq!(success_json["rows"][0][8]["kind"], json!("varchar"));
    assert_eq!(success_json["rows"][0][8]["value"], oracle[0]["payload"]);
    let view = inspect(&success_audit).expect("native wal");
    assert_eq!(view.requests.len(), 1);
    assert_eq!(view.requests[0].state, "succeeded");
    assert_eq!(view.requests[0].project, NATIVE_SOURCE);
    assert_eq!(view.requests[0].role, "oo_r_reader");
    assert_eq!(view.frames.len(), 2);
    assert!(!view.torn_tail);
    let success_pid = success_json["backend_pid"].as_i64().expect("pid");
    proof.set("native_query_exit", json!(0));
    proof.set("native_rows", json!(2));
    proof.set("native_backend_pid", json!(success_pid));

    let hidden_audit = work.join("native-audit-hidden");
    let mut hidden_doc = authority_doc.clone();
    hidden_doc["audit_dir"] = json!(hidden_audit.to_string_lossy());
    let hidden_authority = work.join("native-authority-hidden.json");
    write_json(&hidden_authority, &hidden_doc);
    let hidden = cli(&[
        "query",
        "--authority",
        hidden_authority.to_str().unwrap(),
        "--query",
        work.join("native-hidden.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&hidden, login_pass);
    assert_eq!(hidden.code, 6, "native hidden stderr={}", hidden.stderr);
    assert_eq!(hidden.stderr, "operator-preview: guard\n");
    no_rows(&hidden);
    assert_eq!(hidden.json.as_ref().unwrap()["class"], json!("guard"));
    let hidden_view = inspect(&hidden_audit).expect("hidden wal");
    assert!(hidden_view.requests.iter().any(|item| item.state == "not_admitted"));
    proof.set("native_hidden_exit", json!(6));

    let mut wide = native_policy();
    wide["table"]["columns"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "name": "raw_source",
            "status": "confirmed",
            "semantic_type": "bytea",
            "pii": {"class": "none"}
        }));
    write_json(&work.join("native-policy-wide.json"), &wide);
    let wide_audit = work.join("native-audit-wide");
    let mut wide_doc = authority_doc.clone();
    wide_doc["policy_path"] = json!(work.join("native-policy-wide.json").to_string_lossy());
    wide_doc["audit_dir"] = json!(wide_audit.to_string_lossy());
    let wide_authority = work.join("native-authority-wide.json");
    write_json(&wide_authority, &wide_doc);
    expect_pack(
        &wide_authority,
        &work.join("native-success.json"),
        &wide_audit,
        login_pass,
    )
    .await;
    proof.set("native_pack_mismatch_exit", json!(9));

    let missing = cli(&[
        "cancel",
        "--authority",
        authority.to_str().unwrap(),
        "--execution",
        "00000000-0000-4000-8000-000000000099",
    ])
    .await;
    assert_clean(&missing, login_pass);
    assert_eq!(missing.code, 10, "native absent stderr={}", missing.stderr);
    assert_eq!(missing.json.as_ref().unwrap()["outcome"], json!("not_sent"));
    assert_eq!(missing.json.as_ref().unwrap()["reason"], json!("absent"));
    assert!(!missing.stdout.contains("57014"));
    proof.set("native_absent_cancel_exit", json!(10));

    let cancel_audit = work.join("native-audit-cancel");
    let mut cancel_doc = authority_doc.clone();
    cancel_doc["audit_dir"] = json!(cancel_audit.to_string_lossy());
    let cancel_authority = work.join("native-authority-cancel.json");
    write_json(&cancel_authority, &cancel_doc);
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let locker = connect(host, DATABASE, ADMIN, &read_pass(&PathBuf::from(require_env("OO_DL01W121_ADMIN_PASS"))), ca).await;
    let lock_task = tokio::spawn(hold_lock(locker, physical_table.clone(), release_rx));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let authority_text = cancel_authority.to_str().unwrap().to_string();
    let cancel_query = work.join("native-cancel.json").to_str().unwrap().to_string();
    let query_task = tokio::task::spawn_blocking(move || {
        run_cli(&[
            "query".to_string(),
            "--authority".to_string(),
            authority_text,
            "--query".to_string(),
            cancel_query,
        ])
    });
    let mut running = None;
    for _ in 0..200 {
        let dir = work.join("native-state/executions");
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let Ok(text) = fs::read_to_string(entry.path()) else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if value.get("status").and_then(Value::as_str) == Some("running")
                    && value.get("execution_id").and_then(Value::as_str).is_some()
                {
                    let path_name = entry.file_name();
                    let id = value["execution_id"].as_str().unwrap_or("");
                    if path_name.to_string_lossy().contains(id) {
                        running = Some(value);
                        break;
                    }
                }
            }
        }
        if running.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let running = running.expect("native cancel target did not stay running");
    let pid = running["backend_pid"].as_i64().expect("pid") as i32;
    let mut saw_active = false;
    for _ in 0..40 {
        match observe_backend(admin, pid).await {
            Ok(Some(view)) if view.state() == "active" => {
                saw_active = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(saw_active, "native cancel backend was not active");
    let cancel_id = running["execution_id"].as_str().unwrap().to_string();
    let authority_text = cancel_authority.to_str().unwrap().to_string();
    let cancel_ran = tokio::task::spawn_blocking(move || {
        run_cli(&[
            "cancel".to_string(),
            "--authority".to_string(),
            authority_text,
            "--execution".to_string(),
            cancel_id,
        ])
    })
    .await
    .expect("native cancel join");
    let query_ran = tokio::time::timeout(Duration::from_secs(30), query_task)
        .await
        .expect("native cancel query hung")
        .expect("native query join");
    let _ = release_tx.send(());
    let _ = lock_task.await;
    assert_clean(&query_ran, login_pass);
    assert_clean(&cancel_ran, login_pass);
    assert_eq!(cancel_ran.code, 0, "native cancel stderr={}", cancel_ran.stderr);
    assert_eq!(cancel_ran.json.as_ref().unwrap()["outcome"], json!("sent"));
    assert_eq!(query_ran.code, 0, "native canceled stderr={}", query_ran.stderr);
    let canceled = query_ran.json.as_ref().unwrap();
    assert_eq!(canceled["outcome"], json!("canceled"));
    assert_eq!(canceled["sqlstate"], json!("57014"));
    assert_eq!(canceled["rollback_confirmed"], json!(true));
    assert_eq!(canceled["stop"], json!("canceled"));
    no_rows(&query_ran);
    let cancel_view = inspect(&cancel_audit).expect("cancel wal");
    assert_eq!(cancel_view.requests[0].state, "failed");
    proof.set("native_cancel_exit", json!(0));
    proof.set("native_cancel_sqlstate", json!("57014"));
    proof.set("native_cancel_active", json!(saw_active));

    let late_id = execution_of(&success_json);
    let late = cli(&[
        "cancel",
        "--authority",
        authority.to_str().unwrap(),
        "--execution",
        &late_id,
    ])
    .await;
    assert_eq!(late.code, 10, "native late stderr={}", late.stderr);
    assert_eq!(late.json.as_ref().unwrap()["reason"], json!("late"));
    assert!(!late.stdout.contains("57014"));
    proof.set("native_late_cancel_exit", json!(10));

    let reopen_audit = work.join("native-audit-reopen");
    let mut reopen_doc = authority_doc.clone();
    reopen_doc["audit_dir"] = json!(reopen_audit.to_string_lossy());
    let reopen_authority = work.join("native-authority-reopen.json");
    write_json(&reopen_authority, &reopen_doc);
    let reopened = cli(&[
        "query",
        "--authority",
        reopen_authority.to_str().unwrap(),
        "--query",
        work.join("native-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&reopened, login_pass);
    assert_eq!(reopened.code, 0, "native reopen stderr={}", reopened.stderr);
    let reopened_json = reopened.json.expect("reopen");
    let reopen_pid = reopened_json["backend_pid"].as_i64().expect("reopen pid");
    assert_ne!(reopen_pid, success_pid);
    let mut gone = false;
    for _ in 0..40 {
        let count: i64 = admin
            .query_one(
                "SELECT count(*)::int8 FROM pg_catalog.pg_stat_activity WHERE pid = $1",
                &[&(success_pid as i32)],
            )
            .await
            .unwrap_or_else(|err| panic!("pid probe sqlstate={}", sqlstate(&err)))
            .get(0);
        if count == 0 {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(gone, "closed native backend is still present");
    proof.set("native_reopen_exit", json!(0));
    proof.set("native_reopen_pid_differs", json!(true));
    proof.set("native_closed_pid_gone", json!(true));

    exec(
        admin,
        &format!(
            "ALTER TABLE \"_oo\".{} ALTER COLUMN {} TYPE interval USING '0'::interval",
            quote_physical(&physical_table, "pt_"),
            quote_physical(&physical_columns[12], "pc_")
        ),
    )
    .await;
    let undefined_audit = work.join("native-audit-undefined");
    let mut undefined_doc = authority_doc.clone();
    undefined_doc["audit_dir"] = json!(undefined_audit.to_string_lossy());
    let undefined_authority = work.join("native-authority-undefined.json");
    write_json(&undefined_authority, &undefined_doc);
    let undefined = cli(&[
        "query",
        "--authority",
        undefined_authority.to_str().unwrap(),
        "--query",
        work.join("native-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&undefined, login_pass);
    assert_eq!(undefined.code, 5, "native undefined stderr={} stdout={}", undefined.stderr, undefined.stdout);
    assert_eq!(undefined.stderr, "operator-preview: policy\n");
    no_rows(&undefined);
    assert_eq!(undefined.json.as_ref().unwrap()["class"], json!("unsupported_type"));
    assert_eq!(undefined.json.as_ref().unwrap()["type_path"], json!("native_contract"));
    assert!(!undefined_audit.exists(), "undefined type opened an audit directory");
    proof.set("native_undefined_exit", json!(5));
    prove_bytea(
        host,
        admin,
        login_pass,
        ca,
        ca_path,
        login_pass_path,
        work,
        proof,
        &byte_table,
        &byte_columns,
    )
    .await;
    prove_char(
        host,
        admin,
        login_pass,
        ca,
        ca_path,
        login_pass_path,
        work,
        proof,
        &byte_table,
        &char_column,
        &uuid_column,
    )
    .await;
    prove_temporal(
        host,
        admin,
        login_pass,
        ca,
        ca_path,
        login_pass_path,
        work,
        proof,
        &byte_table,
        &date_column,
        &timestamp_column,
    )
    .await;
    prove_jsonb(
        host,
        admin,
        login_pass,
        ca,
        ca_path,
        login_pass_path,
        work,
        proof,
        &byte_table,
        &doc_column,
    )
    .await;
    proof.set("native_status", json!("proved"));
}

fn assert_native_rows(value: &Value) {
    let rows = value["rows"].as_array().expect("native rows");
    assert_eq!(rows.len(), 2);
    let kinds = [
        "varchar", "varchar", "varchar", "float8", "timestamptz", "bool", "int8", "null", "varchar", "varchar",
        "int8", "int8", "json", "varchar",
    ];
    for (index, kind) in kinds.iter().enumerate() {
        if *kind == "null" {
            assert_eq!(rows[0][index]["kind"], json!("null"));
        } else {
            assert_eq!(rows[0][index]["kind"], json!(kind), "column {index}");
        }
    }
    assert_eq!(rows[0][0]["value"], json!("doc-a"));
    assert_eq!(rows[0][5]["value"], json!(true));
    assert_eq!(rows[0][6]["value"], json!("11"));
    assert_eq!(rows[0][12]["kind"], json!("json"));
    assert_eq!(rows[1][0]["value"], json!("doc-b"));
    assert_eq!(rows[1][3]["kind"], json!("float8"));
    assert_eq!(rows[1][5]["value"], json!(false));
    assert_eq!(rows[1][7]["kind"], json!("varchar"));
    assert_eq!(rows[1][7]["value"], json!("beta"));
    assert_eq!(rows[1][12]["kind"], json!("json"));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].as_array().expect("row width").len(), 14);
}

fn bytea_policy() -> Value {
    let mut policy = native_policy();
    policy["table"]["name"] = json!(BYTEA_TABLE);
    policy["table"]["columns"] = json!(BYTEA_FIELDS
        .iter()
        .map(|name| json!({
            "name": name,
            "status": "confirmed",
            "semantic_type": "bytea",
            "pii": {"class": "none"}
        }))
        .collect::<Vec<_>>());
    policy
}

fn bytea_role_documents() -> (String, String) {
    let mut columns = Vec::new();
    for name in BYTEA_FIELDS {
        columns.push(json!({"name": name, "pii": "none", "classification": {"state": "confirmed"}}));
    }
    columns.push(json!({"name": BYTEA_HIDDEN, "pii": "sensitive", "classification": {"state": "confirmed"}}));
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": NATIVE_SOURCE,
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-bytea", "version": "1"},
        "knowledge": {"id": "know-bytea", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "acme",
            "locale": "zh-CN",
            "include_schemas": [NATIVE_NAMESPACE],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [{
            "schema_version": "oos/v1",
            "dataset": BYTEA_DATASET,
            "label": "Binary",
            "tier": "file",
            "grain": "one row",
            "primary_key": ["object_id"],
            "columns": columns
        }]
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": NATIVE_SOURCE,
        "principal": "operator",
        "role": "oo_r_reader",
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-bytea", "version": "1"},
        "knowledge": {"id": "know-bytea", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": BYTEA_DATASET,
            "row_security": "none",
            "global_stats": "denied",
            "columns": BYTEA_FIELDS
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (
        serde_json::to_string(&effective).expect("bytea effective"),
        serde_json::to_string(&visibility).expect("bytea visibility"),
    )
}

fn write_bytea_pack(dir: &Path) -> String {
    let fields: Vec<Value> = BYTEA_FIELDS
        .iter()
        .map(|name| {
            json!({
                "admission": "sealed_import",
                "admitted": true,
                "column": name,
                "data_type": "bytea",
                "description": "",
                "label": name
            })
        })
        .collect();
    let candidate = json!({
        "dataset": BYTEA_DATASET,
        "fields": fields,
        "protocol": CANDIDATE_PROTOCOL
    });
    let bytes = serde_json::to_vec(&candidate).expect("bytea candidate");
    let digest = sha256_hex(&bytes);
    let relative = "candidates/oo_dl01w1_44.byte_docs.json";
    fs::create_dir_all(dir.join("candidates")).expect("bytea candidate dir");
    fs::write(dir.join(relative), &bytes).expect("bytea candidate file");
    let receipt = json!({
        "binding": {
            "import_generation": NATIVE_GENERATION,
            "import_hash": NATIVE_HASH,
            "manifest_sha256": NATIVE_MANIFEST,
            "snapshot_id": NATIVE_SNAPSHOT,
            "source_id": NATIVE_SOURCE,
            "target_config_sha256": NATIVE_CONFIG
        },
        "files": {"candidates": [{"path": relative, "sha256": digest}]},
        "pack_digest": digest,
        "protocol": PACK_RECEIPT_PROTOCOL,
        "publication": "not_published",
        "review": "unreviewed"
    });
    write_json(&dir.join("receipt.json"), &receipt);
    digest
}

fn bytea_sql() -> &'static str {
    "SELECT note, code, code_or_null, label, blob, uid, object_id FROM oo_dl01w1_44.byte_docs ORDER BY object_id"
}

async fn prove_bytea(
    host: &str,
    admin: &Client,
    login_pass: &str,
    ca: &[u8],
    ca_path: &Path,
    login_pass_path: &Path,
    work: &Path,
    proof: &mut Proof,
    table: &str,
    columns: &[String],
) {
    let catalog = load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "bytea-policy-direct.json",
        &bytea_policy(),
    )
    .await
    .expect("shared catalog admits published bytea");
    assert_eq!(catalog.relations.len(), 1);
    assert_eq!(catalog.relations[0].columns.len(), BYTEA_FIELDS.len());
    for column in &catalog.relations[0].columns {
        assert_eq!(column.type_oid, 17, "{}", column.name);
    }
    proof.set("bytea_catalog", json!("admitted"));

    let mut missing = bytea_policy();
    missing["table"]["columns"] = json!([{
        "name": "missing_bin",
        "status": "confirmed",
        "semantic_type": "bytea",
        "pii": {"class": "none"}
    }]);
    match load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "bytea-policy-unmapped.json",
        &missing,
    )
    .await
    {
        Err(CatalogError::MappingAbsent) => {}
        other => panic!("unmapped bytea ended as {other:?}"),
    }
    proof.set("bytea_unmapped", json!("mapping_absent"));

    let pack_dir = work.join("bytea-pack");
    let pack_digest = write_bytea_pack(&pack_dir);
    let (effective, visibility) = bytea_role_documents();
    let role_pack = compile_role_pack(&effective, &visibility)
        .unwrap_or_else(|err| panic!("bytea role pack compile failed: {err}"));
    assert!(!role_pack.artifact.contains(SENTINEL));
    let role_dir = work.join("bytea-role-pack");
    fs::create_dir_all(&role_dir).expect("bytea role dir");
    fs::write(role_dir.join("preview.json"), &role_pack.artifact).expect("bytea role artifact");
    write_json(&work.join("bytea-policy.json"), &bytea_policy());
    let success_audit = work.join("bytea-audit-success");
    let mut authority_doc = native_authority(work, &pack_dir, ca_path, login_pass_path, host, &success_audit);
    authority_doc["policy_pack_id"] = json!("pol-bytea");
    authority_doc["knowledge"] = json!({"id": "know-bytea", "version": "1"});
    authority_doc["policy_path"] = json!(work.join("bytea-policy.json").to_string_lossy());
    authority_doc["role_pack_dir"] = json!(role_dir.to_string_lossy());
    authority_doc["state_dir"] = json!(work.join("bytea-state").to_string_lossy());
    let authority = work.join("bytea-authority.json");
    write_json(&authority, &authority_doc);
    write_json(
        &work.join("bytea-success.json"),
        &query_value(bytea_sql(), 20, 65_536, 10_000),
    );
    write_json(
        &work.join("bytea-hidden.json"),
        &query_value(
            "SELECT sealed FROM oo_dl01w1_44.byte_docs",
            10,
            8192,
            5000,
        ),
    );

    let described = cli(&["describe", "--authority", authority.to_str().unwrap()]).await;
    assert_clean(&described, login_pass);
    assert_eq!(described.code, 0, "bytea describe stderr={}", described.stderr);
    let described_json = described.json.expect("bytea describe");
    assert_eq!(described_json["outcome"], json!("described"));
    assert_eq!(described_json["knowledge"]["dataset"], json!(BYTEA_DATASET));
    let mut names: Vec<&str> = described_json["knowledge"]["columns"]
        .as_array()
        .expect("bytea columns")
        .iter()
        .map(|column| column["name"].as_str().unwrap_or(""))
        .collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = BYTEA_FIELDS.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    assert!(!names.contains(&BYTEA_HIDDEN));
    proof.set("bytea_describe_exit", json!(0));

    let success = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("bytea-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&success, login_pass);
    assert_eq!(success.code, 0, "bytea query stderr={}", success.stderr);
    let success_json = success.json.expect("bytea query");
    assert_eq!(success_json["outcome"], json!("succeeded"));
    assert_eq!(success_json["type_path"], json!("native_contract"));
    assert_eq!(success_json["shared_catalog"], json!("admitted"));
    assert_eq!(success_json["rollback_confirmed"], json!(true));
    assert_eq!(success_json["returned_rows"], json!(2));
    let oracle = bytea_oracle(admin, table, columns).await;
    assert_eq!(oracle.len(), 2);
    let rows = success_json["rows"].as_array().expect("bytea rows");
    assert_eq!(rows.len(), 2);
    for (row_index, oracle_row) in oracle.iter().enumerate() {
        assert_eq!(rows[row_index].as_array().expect("width").len(), BYTEA_FIELDS.len());
        for (column_index, hex) in oracle_row.iter().enumerate() {
            let cell = &rows[row_index][column_index];
            match hex {
                None => {
                    assert_eq!(cell["kind"], json!("null"), "row {row_index} column {column_index}");
                    assert!(cell.get("value").is_none());
                }
                Some(text) => {
                    assert_eq!(cell["kind"], json!("bytea"), "row {row_index} column {column_index}");
                    assert_ne!(cell["kind"], json!("text"));
                    assert_eq!(cell["value"], json!(text));
                }
            }
        }
    }
    assert_eq!(rows[0][0]["kind"], json!("null"));
    assert_eq!(rows[0][1]["value"], json!("fffe0041"));
    assert_eq!(rows[0][2]["kind"], json!("null"));
    assert_eq!(rows[0][3]["value"], json!(""));
    assert_eq!(rows[0][4]["value"], json!("004100"));
    assert_eq!(rows[0][5]["value"], json!("11111111222240008000000000000000"));
    assert_eq!(rows[0][6]["value"], json!("c141d04141eef04100000000"));
    assert_eq!(rows[1][0]["value"], json!("00000000"));
    assert_eq!(rows[1][3]["value"], json!("00"));
    assert_eq!(rows[1][4]["value"], json!(""));
    assert_eq!(rows[1][5]["value"], json!("11111111222240008000000000000001"));
    assert_eq!(rows[1][6]["value"], json!("c141d04141eef04100000001"));
    assert!(!success.stdout.contains("5e1e"));
    let view = inspect(&success_audit).expect("bytea wal");
    assert_eq!(view.requests.len(), 1);
    assert_eq!(view.requests[0].state, "succeeded");
    assert_eq!(view.requests[0].project, NATIVE_SOURCE);
    assert_eq!(view.requests[0].role, "oo_r_reader");
    assert_eq!(view.frames.len(), 2);
    assert!(!view.torn_tail);
    proof.set("bytea_query_exit", json!(0));
    proof.set("bytea_rows", json!(2));
    proof.set("bytea_null", json!(true));
    proof.set("bytea_empty", json!(true));
    proof.set("bytea_zero", json!(true));
    proof.set("bytea_non_utf8", json!(true));
    proof.set("bytea_len_12", json!(true));
    proof.set("bytea_len_16", json!(true));
    proof.set("bytea_oracle_match", json!(true));
    proof.set("bytea_pack_digest", json!(pack_digest));
    proof.set("bytea_audit", json!("succeeded"));

    let hidden_audit = work.join("bytea-audit-hidden");
    let mut hidden_doc = authority_doc.clone();
    hidden_doc["audit_dir"] = json!(hidden_audit.to_string_lossy());
    let hidden_authority = work.join("bytea-authority-hidden.json");
    write_json(&hidden_authority, &hidden_doc);
    let hidden = cli(&[
        "query",
        "--authority",
        hidden_authority.to_str().unwrap(),
        "--query",
        work.join("bytea-hidden.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&hidden, login_pass);
    assert_eq!(hidden.code, 6, "bytea hidden stderr={}", hidden.stderr);
    assert_eq!(hidden.stderr, "operator-preview: guard\n");
    no_rows(&hidden);
    assert_eq!(hidden.json.as_ref().unwrap()["class"], json!("guard"));
    assert!(!hidden.stdout.contains("5e1e"));
    let hidden_view = inspect(&hidden_audit).expect("bytea hidden wal");
    assert!(hidden_view.requests.iter().any(|item| item.state == "not_admitted"));
    proof.set("bytea_hidden_exit", json!(6));

    let mut wide = bytea_policy();
    wide["table"]["columns"].as_array_mut().unwrap().push(json!({
        "name": BYTEA_HIDDEN,
        "status": "confirmed",
        "semantic_type": "bytea",
        "pii": {"class": "none"}
    }));
    write_json(&work.join("bytea-policy-wide.json"), &wide);
    let wide_audit = work.join("bytea-audit-wide");
    let mut wide_doc = authority_doc.clone();
    wide_doc["policy_path"] = json!(work.join("bytea-policy-wide.json").to_string_lossy());
    wide_doc["audit_dir"] = json!(wide_audit.to_string_lossy());
    let wide_authority = work.join("bytea-authority-wide.json");
    write_json(&wide_authority, &wide_doc);
    expect_pack(
        &wide_authority,
        &work.join("bytea-success.json"),
        &wide_audit,
        login_pass,
    )
    .await;
    proof.set("bytea_pack_mismatch_exit", json!(9));

    let pinned = native_pack_dir();
    let mut pinned_policy = native_policy();
    pinned_policy["table"]["columns"].as_array_mut().unwrap().push(json!({
        "name": "code",
        "status": "confirmed",
        "semantic_type": "bytea",
        "pii": {"class": "none"}
    }));
    write_json(&work.join("bytea-policy-pinned-wide.json"), &pinned_policy);
    let pinned_audit = work.join("bytea-audit-pinned-wide");
    let mut pinned_doc = native_authority(
        work,
        &pinned,
        ca_path,
        login_pass_path,
        host,
        &pinned_audit,
    );
    pinned_doc["policy_path"] = json!(work.join("bytea-policy-pinned-wide.json").to_string_lossy());
    let pinned_authority = work.join("bytea-authority-pinned-wide.json");
    write_json(&pinned_authority, &pinned_doc);
    expect_pack(
        &pinned_authority,
        &work.join("native-success.json"),
        &pinned_audit,
        login_pass,
    )
    .await;
    proof.set("bytea_pinned_pack_mismatch_exit", json!(9));
    proof.set("bytea_clickhouse_source", json!("3748d9fd2959026fddfe6a928810b70383c937ec"));
    proof.set("bytea_clickhouse_seal", json!("fde00f49a455616e09ea4d1a254af9f80def7542"));
    proof.set("bytea_mongo_source", json!("82d6d6fa0ba4ce4c8b72d57a73306e12e0d835c1"));
    proof.set("bytea_mongo_receipt", json!("ca3eee43a927bf2af445485bd3b0b70e637ecaa8"));
    proof.set("bytea_status", json!("proved"));
}

async fn bytea_oracle(admin: &Client, table: &str, columns: &[String]) -> Vec<Vec<Option<String>>> {
    let exprs: Vec<String> = columns
        .iter()
        .map(|name| format!("encode({}, 'hex')", quote_physical(name, "pc_")))
        .collect();
    let sql = format!(
        "SELECT {exprs} FROM \"_oo\".{table} ORDER BY {key}",
        exprs = exprs.join(", "),
        table = quote_physical(table, "pt_"),
        key = quote_physical(&columns[6], "pc_")
    );
    let rows = admin
        .query(&sql, &[])
        .await
        .unwrap_or_else(|err| panic!("bytea oracle sqlstate={}", sqlstate(&err)));
    assert_eq!(rows.len(), 2);
    rows.iter()
        .map(|row| {
            (0..BYTEA_FIELDS.len())
                .map(|index| row.get::<_, Option<String>>(index))
                .collect()
        })
        .collect()
}

async fn column_oid(admin: &Client, table: &str, column: &str) -> u32 {
    let oid: i32 = admin
        .query_one(
            "SELECT attribute.atttypid::int4 \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1 AND attribute.attname = $2 \
               AND attribute.attnum > 0 AND NOT attribute.attisdropped",
            &[&table, &column],
        )
        .await
        .unwrap_or_else(|err| panic!("oid sqlstate={} err={err}", sqlstate(&err)))
        .get(0);
    u32::try_from(oid).expect("column oid")
}

async fn column_typmod(admin: &Client, table: &str, column: &str) -> i32 {
    admin
        .query_one(
            "SELECT attribute.atttypmod::int4 \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = relation.relnamespace \
             WHERE namespace.nspname = '_oo' AND relation.relname = $1 AND attribute.attname = $2 \
               AND attribute.attnum > 0 AND NOT attribute.attisdropped",
            &[&table, &column],
        )
        .await
        .unwrap_or_else(|err| panic!("typmod sqlstate={} err={err}", sqlstate(&err)))
        .get(0)
}

struct ByteRow {
    pos: &'static str,
    note: Option<Vec<u8>>,
    code: Option<Vec<u8>>,
    code_or_null: Option<Vec<u8>>,
    label: Option<Vec<u8>>,
    blob: Option<Vec<u8>>,
    uid: Option<Vec<u8>>,
    object_id: Option<Vec<u8>>,
    c6: Option<String>,
    row_uid: Option<String>,
    day: Option<String>,
    stamp: Option<String>,
    doc: Option<String>,
}

const JSONB_DOCUMENT: &str = r#"{"a":{"n":9007199254740993},"box":{},"missing":null,"tags":["red","blue"],"z":1}"#;

async fn insert_bytea_rows(
    admin: &mut Client,
    table: &str,
    columns: &[String],
    sealed: &str,
    char_column: &str,
    uuid_column: &str,
    date_column: &str,
    timestamp_column: &str,
    doc_column: &str,
) {
    let mut names = vec!["\"_oo_pos\"".to_string()];
    for column in columns {
        names.push(quote_physical(column, "pc_"));
    }
    names.push(quote_physical(sealed, "pc_"));
    names.push(quote_physical(char_column, "pc_"));
    names.push(quote_physical(uuid_column, "pc_"));
    names.push(quote_physical(date_column, "pc_"));
    names.push(quote_physical(timestamp_column, "pc_"));
    names.push(quote_physical(doc_column, "pc_"));
    let sql = format!(
        "INSERT INTO \"_oo\".{table} ({names}) VALUES (\
         ($1::text)::numeric, $2::bytea, $3::bytea, $4::bytea, $5::bytea, $6::bytea, $7::bytea, \
         $8::bytea, $9::bytea, ($10::text)::character(6), ($11::text)::uuid, \
         ($12::text)::date, ($13::text)::timestamp without time zone, ($14::text)::jsonb)",
        table = quote_physical(table, "pt_"),
        names = names.join(", ")
    );
    let object_a = vec![0xc1, 0x41, 0xd0, 0x41, 0x41, 0xee, 0xf0, 0x41, 0x00, 0x00, 0x00, 0x00];
    let object_b = vec![0xc1, 0x41, 0xd0, 0x41, 0x41, 0xee, 0xf0, 0x41, 0x00, 0x00, 0x00, 0x01];
    let uid_a = vec![
        0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let uid_b = vec![
        0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
    ];
    let sealed_bytes: Option<Vec<u8>> = Some(vec![0x5e, 0x1e]);
    let rows = [
        ByteRow {
            pos: "1",
            note: None,
            code: Some(vec![0xff, 0xfe, 0x00, 0x41]),
            code_or_null: None,
            label: Some(Vec::new()),
            blob: Some(vec![0x00, 0x41, 0x00]),
            uid: Some(uid_a),
            object_id: Some(object_a),
            c6: Some("ab".to_string()),
            row_uid: None,
            day: Some("2026-03-11".to_string()),
            stamp: None,
            doc: Some(JSONB_DOCUMENT.to_string()),
        },
        ByteRow {
            pos: "2",
            note: Some(vec![0x00, 0x00, 0x00, 0x00]),
            code: Some(vec![0x80, 0xff]),
            code_or_null: Some(vec![0xff]),
            label: Some(vec![0x00]),
            blob: Some(Vec::new()),
            uid: Some(uid_b),
            object_id: Some(object_b),
            c6: None,
            row_uid: Some("11111111-2222-4000-8000-000000000000".to_string()),
            day: None,
            stamp: Some("2024-03-15 12:34:56.123456".to_string()),
            doc: None,
        },
    ];
    let tx = admin.transaction().await.expect("bytea insert");
    for row in &rows {
        let params: [(&(dyn tokio_postgres::types::ToSql + Sync), tokio_postgres::types::Type); 14] = [
            (&row.pos, tokio_postgres::types::Type::TEXT),
            (&row.note, tokio_postgres::types::Type::BYTEA),
            (&row.code, tokio_postgres::types::Type::BYTEA),
            (&row.code_or_null, tokio_postgres::types::Type::BYTEA),
            (&row.label, tokio_postgres::types::Type::BYTEA),
            (&row.blob, tokio_postgres::types::Type::BYTEA),
            (&row.uid, tokio_postgres::types::Type::BYTEA),
            (&row.object_id, tokio_postgres::types::Type::BYTEA),
            (&sealed_bytes, tokio_postgres::types::Type::BYTEA),
            (&row.c6, tokio_postgres::types::Type::TEXT),
            (&row.row_uid, tokio_postgres::types::Type::TEXT),
            (&row.day, tokio_postgres::types::Type::TEXT),
            (&row.stamp, tokio_postgres::types::Type::TEXT),
            (&row.doc, tokio_postgres::types::Type::TEXT),
        ];
        tx.execute_typed(&sql, &params).await.unwrap_or_else(|err| {
            let mut detail = String::new();
            let mut source = std::error::Error::source(&err);
            while let Some(inner) = source {
                detail.push_str(" | ");
                detail.push_str(&inner.to_string());
                source = inner.source();
            }
            panic!("bytea insert sqlstate={} err={err}{detail} sql={sql}", sqlstate(&err))
        });
    }
    tx.commit().await.expect("bytea insert commit");
}

fn char_policy() -> Value {
    let mut policy = native_policy();
    policy["table"]["name"] = json!(BYTEA_TABLE);
    policy["table"]["columns"] = json!([
        {
            "name": CHAR_COLUMN,
            "status": "confirmed",
            "semantic_type": "bpchar",
            "pii": {"class": "none"}
        },
        {
            "name": UUID_COLUMN,
            "status": "confirmed",
            "semantic_type": "uuid",
            "pii": {"class": "none"}
        }
    ]);
    policy
}

fn char_role_documents() -> (String, String) {
    let columns = json!([
        {"name": CHAR_COLUMN, "pii": "none", "classification": {"state": "confirmed"}},
        {"name": UUID_COLUMN, "pii": "none", "classification": {"state": "confirmed"}}
    ]);
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": NATIVE_SOURCE,
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-char", "version": "1"},
        "knowledge": {"id": "know-char", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "acme",
            "locale": "zh-CN",
            "include_schemas": [NATIVE_NAMESPACE],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [{
            "schema_version": "oos/v1",
            "dataset": BYTEA_DATASET,
            "label": "Character",
            "tier": "file",
            "grain": "one row",
            "primary_key": [CHAR_COLUMN],
            "columns": columns
        }]
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": NATIVE_SOURCE,
        "principal": "operator",
        "role": "oo_r_reader",
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-char", "version": "1"},
        "knowledge": {"id": "know-char", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": BYTEA_DATASET,
            "row_security": "none",
            "global_stats": "denied",
            "columns": [CHAR_COLUMN, UUID_COLUMN]
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (
        serde_json::to_string(&effective).expect("char effective"),
        serde_json::to_string(&visibility).expect("char visibility"),
    )
}

fn write_char_pack(dir: &Path) -> String {
    let fields = [
        (CHAR_COLUMN, "bpchar"),
        (UUID_COLUMN, "uuid"),
    ]
    .into_iter()
    .map(|(name, data_type)| {
        json!({
            "admission": "sealed_import",
            "admitted": true,
            "column": name,
            "data_type": data_type,
            "description": "",
            "label": name
        })
    })
    .collect::<Vec<_>>();
    let candidate = json!({
        "dataset": BYTEA_DATASET,
        "fields": fields,
        "protocol": CANDIDATE_PROTOCOL
    });
    let bytes = serde_json::to_vec(&candidate).expect("char candidate");
    let digest = sha256_hex(&bytes);
    let relative = "candidates/oo_dl01w1_44.byte_docs.json";
    fs::create_dir_all(dir.join("candidates")).expect("char candidate dir");
    fs::write(dir.join(relative), &bytes).expect("char candidate file");
    let receipt = json!({
        "binding": {
            "import_generation": NATIVE_GENERATION,
            "import_hash": NATIVE_HASH,
            "manifest_sha256": NATIVE_MANIFEST,
            "snapshot_id": NATIVE_SNAPSHOT,
            "source_id": NATIVE_SOURCE,
            "target_config_sha256": NATIVE_CONFIG
        },
        "files": {"candidates": [{"path": relative, "sha256": digest}]},
        "pack_digest": digest,
        "protocol": PACK_RECEIPT_PROTOCOL,
        "publication": "not_published",
        "review": "unreviewed"
    });
    write_json(&dir.join("receipt.json"), &receipt);
    digest
}

fn char_sql() -> &'static str {
    "SELECT c6, row_uid FROM oo_dl01w1_44.byte_docs ORDER BY c6"
}

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("hex"))
        .collect()
}

async fn char_oracle(admin: &Client, table: &str, char_column: &str, uuid_column: &str) -> Vec<(Option<String>, Option<String>)> {
    let sql = format!(
        "SELECT encode(pg_catalog.bpcharsend({c6}), 'hex'), ({uid})::text \
         FROM \"_oo\".{table} ORDER BY {c6}",
        c6 = quote_physical(char_column, "pc_"),
        uid = quote_physical(uuid_column, "pc_"),
        table = quote_physical(table, "pt_")
    );
    let rows = admin
        .query(&sql, &[])
        .await
        .unwrap_or_else(|err| panic!("char oracle sqlstate={} err={err} sql={sql}", sqlstate(&err)));
    assert_eq!(rows.len(), 2);
    rows.iter()
        .map(|row| (row.get::<_, Option<String>>(0), row.get::<_, Option<String>>(1)))
        .collect()
}

async fn prove_char(
    host: &str,
    admin: &Client,
    login_pass: &str,
    ca: &[u8],
    ca_path: &Path,
    login_pass_path: &Path,
    work: &Path,
    proof: &mut Proof,
    table: &str,
    char_column: &str,
    uuid_column: &str,
) {
    assert_eq!(column_typmod(admin, table, char_column).await, 10);
    assert_eq!(column_typmod(admin, table, uuid_column).await, -1);
    proof.set("char_atttypmod_c6", json!(10));
    proof.set("char_atttypmod_uuid", json!(-1));

    let catalog = load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "char-policy-direct.json",
        &char_policy(),
    )
    .await
    .expect("shared catalog admits uuid and bpchar");
    assert_eq!(catalog.relations.len(), 1);
    assert_eq!(catalog.relations[0].columns.len(), 2);
    assert_eq!(catalog.relations[0].columns[0].name, CHAR_COLUMN);
    assert_eq!(catalog.relations[0].columns[0].type_oid, 1042);
    assert_eq!(catalog.relations[0].columns[1].name, UUID_COLUMN);
    assert_eq!(catalog.relations[0].columns[1].type_oid, 2950);
    proof.set("char_catalog", json!("admitted"));

    let pack_dir = work.join("char-pack");
    let pack_digest = write_char_pack(&pack_dir);
    let (effective, visibility) = char_role_documents();
    let role_pack = compile_role_pack(&effective, &visibility)
        .unwrap_or_else(|err| panic!("char role pack compile failed: {err}"));
    assert!(!role_pack.artifact.contains(SENTINEL));
    let role_dir = work.join("char-role-pack");
    fs::create_dir_all(&role_dir).expect("char role dir");
    fs::write(role_dir.join("preview.json"), &role_pack.artifact).expect("char role artifact");
    write_json(&work.join("char-policy.json"), &char_policy());
    let success_audit = work.join("char-audit-success");
    let mut authority_doc = native_authority(work, &pack_dir, ca_path, login_pass_path, host, &success_audit);
    authority_doc["policy_pack_id"] = json!("pol-char");
    authority_doc["knowledge"] = json!({"id": "know-char", "version": "1"});
    authority_doc["policy_path"] = json!(work.join("char-policy.json").to_string_lossy());
    authority_doc["role_pack_dir"] = json!(role_dir.to_string_lossy());
    authority_doc["state_dir"] = json!(work.join("char-state").to_string_lossy());
    let authority = work.join("char-authority.json");
    write_json(&authority, &authority_doc);
    write_json(
        &work.join("char-success.json"),
        &query_value(char_sql(), 20, 65_536, 10_000),
    );
    write_json(
        &work.join("char-hidden.json"),
        &query_value("SELECT note FROM oo_dl01w1_44.byte_docs", 10, 8192, 5000),
    );

    let described = cli(&["describe", "--authority", authority.to_str().unwrap()]).await;
    assert_clean(&described, login_pass);
    assert_eq!(described.code, 0, "char describe stderr={}", described.stderr);
    let described_json = described.json.expect("char describe");
    assert_eq!(described_json["outcome"], json!("described"));
    assert_eq!(described_json["knowledge"]["dataset"], json!(BYTEA_DATASET));
    let mut names: Vec<&str> = described_json["knowledge"]["columns"]
        .as_array()
        .expect("char columns")
        .iter()
        .map(|column| column["name"].as_str().unwrap_or(""))
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec![CHAR_COLUMN, UUID_COLUMN]);
    proof.set("char_describe_exit", json!(0));

    let success = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("char-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&success, login_pass);
    assert_eq!(success.code, 0, "char query stderr={}", success.stderr);
    let success_json = success.json.expect("char query");
    assert_eq!(success_json["outcome"], json!("succeeded"));
    assert_eq!(success_json["type_path"], json!("native_contract"));
    assert_eq!(success_json["shared_catalog"], json!("admitted"));
    assert_eq!(success_json["rollback_confirmed"], json!(true));
    assert_eq!(success_json["returned_rows"], json!(2));
    assert!(!char_sql().contains("::"));
    let oracle = char_oracle(admin, table, char_column, uuid_column).await;
    let rows = success_json["rows"].as_array().expect("char rows");
    assert_eq!(rows.len(), 2);
    let stored = oracle[0].0.as_deref().expect("padded bpchar hex");
    let stored_text = String::from_utf8(decode_hex(stored)).expect("bpchar utf8");
    assert_eq!(stored_text, "ab    ");
    assert_ne!(stored_text, "ab");
    assert_eq!(rows[0][0]["kind"], json!("bpchar"));
    assert_eq!(rows[0][0]["value"], json!(stored_text));
    assert_eq!(rows[0][1]["kind"], json!("null"));
    assert!(rows[0][1].get("value").is_none());
    assert!(oracle[1].0.is_none());
    assert_eq!(rows[1][0]["kind"], json!("null"));
    assert!(rows[1][0].get("value").is_none());
    let canonical = oracle[1].1.as_deref().expect("uuid text");
    assert_eq!(canonical, "11111111-2222-4000-8000-000000000000");
    assert_eq!(rows[1][1]["kind"], json!("uuid"));
    assert_eq!(rows[1][1]["value"], json!(canonical));
    let view = inspect(&success_audit).expect("char wal");
    assert_eq!(view.requests.len(), 1);
    assert_eq!(view.requests[0].state, "succeeded");
    assert_eq!(view.requests[0].project, NATIVE_SOURCE);
    assert_eq!(view.requests[0].role, "oo_r_reader");
    assert_eq!(view.frames.len(), 2);
    proof.set("char_query_exit", json!(0));
    proof.set("char_rows", json!(2));
    proof.set("char_bpchar_value", json!(stored_text));
    proof.set("char_bpchar_untrimmed", json!(true));
    proof.set("char_uuid_canonical", json!(canonical));
    proof.set("char_null", json!(true));
    proof.set("char_oracle_match", json!(true));
    proof.set("char_pack_digest", json!(pack_digest));
    proof.set("char_audit", json!("succeeded"));

    let hidden_audit = work.join("char-audit-hidden");
    let mut hidden_doc = authority_doc.clone();
    hidden_doc["audit_dir"] = json!(hidden_audit.to_string_lossy());
    let hidden_authority = work.join("char-authority-hidden.json");
    write_json(&hidden_authority, &hidden_doc);
    let hidden = cli(&[
        "query",
        "--authority",
        hidden_authority.to_str().unwrap(),
        "--query",
        work.join("char-hidden.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&hidden, login_pass);
    assert_eq!(hidden.code, 6, "char hidden stderr={}", hidden.stderr);
    assert_eq!(hidden.stderr, "operator-preview: guard\n");
    no_rows(&hidden);
    let hidden_view = inspect(&hidden_audit).expect("char hidden wal");
    assert!(hidden_view.requests.iter().any(|item| item.state == "not_admitted"));
    proof.set("char_guard_exit", json!(6));

    let mut wide = char_policy();
    wide["table"]["columns"].as_array_mut().unwrap().push(json!({
        "name": "note",
        "status": "confirmed",
        "semantic_type": "bytea",
        "pii": {"class": "none"}
    }));
    write_json(&work.join("char-policy-wide.json"), &wide);
    let wide_audit = work.join("char-audit-wide");
    let mut wide_doc = authority_doc.clone();
    wide_doc["policy_path"] = json!(work.join("char-policy-wide.json").to_string_lossy());
    wide_doc["audit_dir"] = json!(wide_audit.to_string_lossy());
    let wide_authority = work.join("char-authority-wide.json");
    write_json(&wide_authority, &wide_doc);
    expect_pack(
        &wide_authority,
        &work.join("char-success.json"),
        &wide_audit,
        login_pass,
    )
    .await;
    proof.set("char_pack_mismatch_exit", json!(9));
    proof.set("char_pg_uuid_source", json!("f395b0015711920ed0fe66a6c7ce1b928a48635f"));
    proof.set("char_pg_uuid_receipt", json!("6fd28c439820ad9262e49ef5f2a119fd97e28590"));
    proof.set("char_oracle_source", json!("9cc26104c7a40d23d38d2d9c436027b7876eee8e"));
    proof.set("char_oracle_receipt", json!("a6b350652b2bcfc41c349206b38ffbf159141518"));
    proof.set("char_status", json!("proved"));
}

fn temporal_policy() -> Value {
    let mut policy = native_policy();
    policy["table"]["name"] = json!(BYTEA_TABLE);
    policy["table"]["columns"] = json!([
        {
            "name": DATE_COLUMN,
            "status": "confirmed",
            "semantic_type": "date",
            "pii": {"class": "none"}
        },
        {
            "name": TIMESTAMP_COLUMN,
            "status": "confirmed",
            "semantic_type": "timestamp",
            "pii": {"class": "none"}
        }
    ]);
    policy
}

fn temporal_role_documents() -> (String, String) {
    let columns = json!([
        {"name": DATE_COLUMN, "pii": "none", "classification": {"state": "confirmed"}},
        {"name": TIMESTAMP_COLUMN, "pii": "none", "classification": {"state": "confirmed"}}
    ]);
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": NATIVE_SOURCE,
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-temporal", "version": "1"},
        "knowledge": {"id": "know-temporal", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "acme",
            "locale": "zh-CN",
            "include_schemas": [NATIVE_NAMESPACE],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [{
            "schema_version": "oos/v1",
            "dataset": BYTEA_DATASET,
            "label": "Temporal",
            "tier": "file",
            "grain": "one row",
            "primary_key": [DATE_COLUMN],
            "columns": columns
        }]
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": NATIVE_SOURCE,
        "principal": "operator",
        "role": "oo_r_reader",
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-temporal", "version": "1"},
        "knowledge": {"id": "know-temporal", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": BYTEA_DATASET,
            "row_security": "none",
            "global_stats": "denied",
            "columns": [DATE_COLUMN, TIMESTAMP_COLUMN]
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (
        serde_json::to_string(&effective).expect("temporal effective"),
        serde_json::to_string(&visibility).expect("temporal visibility"),
    )
}

fn write_temporal_pack(dir: &Path) -> String {
    let fields = [
        (DATE_COLUMN, "date"),
        (TIMESTAMP_COLUMN, "timestamp"),
    ]
    .into_iter()
    .map(|(name, data_type)| {
        json!({
            "admission": "sealed_import",
            "admitted": true,
            "column": name,
            "data_type": data_type,
            "description": "",
            "label": name
        })
    })
    .collect::<Vec<_>>();
    let candidate = json!({
        "dataset": BYTEA_DATASET,
        "fields": fields,
        "protocol": CANDIDATE_PROTOCOL
    });
    let bytes = serde_json::to_vec(&candidate).expect("temporal candidate");
    let digest = sha256_hex(&bytes);
    let relative = "candidates/oo_dl01w1_44.byte_docs.json";
    fs::create_dir_all(dir.join("candidates")).expect("temporal candidate dir");
    fs::write(dir.join(relative), &bytes).expect("temporal candidate file");
    let receipt = json!({
        "binding": {
            "import_generation": NATIVE_GENERATION,
            "import_hash": NATIVE_HASH,
            "manifest_sha256": NATIVE_MANIFEST,
            "snapshot_id": NATIVE_SNAPSHOT,
            "source_id": NATIVE_SOURCE,
            "target_config_sha256": NATIVE_CONFIG
        },
        "files": {"candidates": [{"path": relative, "sha256": digest}]},
        "pack_digest": digest,
        "protocol": PACK_RECEIPT_PROTOCOL,
        "publication": "not_published",
        "review": "unreviewed"
    });
    write_json(&dir.join("receipt.json"), &receipt);
    digest
}

fn temporal_sql() -> &'static str {
    "SELECT day, d FROM oo_dl01w1_44.byte_docs ORDER BY day"
}

async fn temporal_oracle(
    admin: &Client,
    table: &str,
    date_column: &str,
    timestamp_column: &str,
) -> Vec<(Option<String>, Option<String>)> {
    let sql = format!(
        "SELECT to_char({day}, 'YYYY-MM-DD'), \
                to_char({stamp}, 'YYYY-MM-DD\"T\"HH24:MI:SS.US') \
         FROM \"_oo\".{table} ORDER BY {day}",
        day = quote_physical(date_column, "pc_"),
        stamp = quote_physical(timestamp_column, "pc_"),
        table = quote_physical(table, "pt_")
    );
    let rows = admin
        .query(&sql, &[])
        .await
        .unwrap_or_else(|err| panic!("temporal oracle sqlstate={} err={err} sql={sql}", sqlstate(&err)));
    assert_eq!(rows.len(), 2);
    rows.iter()
        .map(|row| (row.get::<_, Option<String>>(0), row.get::<_, Option<String>>(1)))
        .collect()
}

async fn prove_temporal(
    host: &str,
    admin: &Client,
    login_pass: &str,
    ca: &[u8],
    ca_path: &Path,
    login_pass_path: &Path,
    work: &Path,
    proof: &mut Proof,
    table: &str,
    date_column: &str,
    timestamp_column: &str,
) {
    assert_eq!(column_typmod(admin, table, date_column).await, -1);
    assert_eq!(column_typmod(admin, table, timestamp_column).await, -1);
    proof.set("temporal_atttypmod_day", json!(-1));
    proof.set("temporal_atttypmod_timestamp", json!(-1));

    let catalog = load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "temporal-policy-direct.json",
        &temporal_policy(),
    )
    .await
    .expect("shared catalog admits date and timestamp");
    assert_eq!(catalog.relations.len(), 1);
    assert_eq!(catalog.relations[0].columns.len(), 2);
    assert_eq!(catalog.relations[0].columns[0].name, DATE_COLUMN);
    assert_eq!(catalog.relations[0].columns[0].type_oid, 1082);
    assert_eq!(catalog.relations[0].columns[1].name, TIMESTAMP_COLUMN);
    assert_eq!(catalog.relations[0].columns[1].type_oid, 1114);
    proof.set("temporal_catalog", json!("admitted"));

    let pack_dir = work.join("temporal-pack");
    let pack_digest = write_temporal_pack(&pack_dir);
    let (effective, visibility) = temporal_role_documents();
    let role_pack = compile_role_pack(&effective, &visibility)
        .unwrap_or_else(|err| panic!("temporal role pack compile failed: {err}"));
    assert!(!role_pack.artifact.contains(SENTINEL));
    let role_dir = work.join("temporal-role-pack");
    fs::create_dir_all(&role_dir).expect("temporal role dir");
    fs::write(role_dir.join("preview.json"), &role_pack.artifact).expect("temporal role artifact");
    write_json(&work.join("temporal-policy.json"), &temporal_policy());
    let success_audit = work.join("temporal-audit-success");
    let mut authority_doc = native_authority(work, &pack_dir, ca_path, login_pass_path, host, &success_audit);
    authority_doc["policy_pack_id"] = json!("pol-temporal");
    authority_doc["knowledge"] = json!({"id": "know-temporal", "version": "1"});
    authority_doc["policy_path"] = json!(work.join("temporal-policy.json").to_string_lossy());
    authority_doc["role_pack_dir"] = json!(role_dir.to_string_lossy());
    authority_doc["state_dir"] = json!(work.join("temporal-state").to_string_lossy());
    let authority = work.join("temporal-authority.json");
    write_json(&authority, &authority_doc);
    write_json(
        &work.join("temporal-success.json"),
        &query_value(temporal_sql(), 20, 65_536, 10_000),
    );
    write_json(
        &work.join("temporal-hidden.json"),
        &query_value("SELECT note FROM oo_dl01w1_44.byte_docs", 10, 8192, 5000),
    );

    let described = cli(&["describe", "--authority", authority.to_str().unwrap()]).await;
    assert_clean(&described, login_pass);
    assert_eq!(described.code, 0, "temporal describe stderr={}", described.stderr);
    let described_json = described.json.expect("temporal describe");
    assert_eq!(described_json["outcome"], json!("described"));
    assert_eq!(described_json["knowledge"]["dataset"], json!(BYTEA_DATASET));
    let mut names: Vec<&str> = described_json["knowledge"]["columns"]
        .as_array()
        .expect("temporal columns")
        .iter()
        .map(|column| column["name"].as_str().unwrap_or(""))
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec![TIMESTAMP_COLUMN, DATE_COLUMN]);
    proof.set("temporal_describe_exit", json!(0));

    let success = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("temporal-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&success, login_pass);
    assert_eq!(success.code, 0, "temporal query stderr={}", success.stderr);
    let success_json = success.json.expect("temporal query");
    assert_eq!(success_json["outcome"], json!("succeeded"));
    assert_eq!(success_json["type_path"], json!("native_contract"));
    assert_eq!(success_json["shared_catalog"], json!("admitted"));
    assert_eq!(success_json["rollback_confirmed"], json!(true));
    assert_eq!(success_json["returned_rows"], json!(2));
    assert!(!temporal_sql().contains("::"));
    let oracle = temporal_oracle(admin, table, date_column, timestamp_column).await;
    let rows = success_json["rows"].as_array().expect("temporal rows");
    assert_eq!(rows.len(), 2);
    let day_text = oracle[0].0.as_deref().expect("date text");
    assert_eq!(day_text, "2026-03-11");
    assert!(oracle[0].1.is_none());
    assert_eq!(rows[0][0]["kind"], json!("date"));
    assert_eq!(rows[0][0]["value"], json!(day_text));
    assert_eq!(rows[0][1]["kind"], json!("null"));
    assert!(rows[0][1].get("value").is_none());
    assert!(oracle[1].0.is_none());
    let stamp_text = oracle[1].1.as_deref().expect("timestamp text");
    assert_eq!(stamp_text, "2024-03-15T12:34:56.123456");
    assert!(!stamp_text.contains('Z'));
    assert!(stamp_text.contains(".123456"));
    assert_eq!(rows[1][0]["kind"], json!("null"));
    assert!(rows[1][0].get("value").is_none());
    assert_eq!(rows[1][1]["kind"], json!("timestamp"));
    assert_eq!(rows[1][1]["value"], json!(stamp_text));
    assert!(!rows[1][1]["value"].as_str().unwrap().contains('Z'));
    let view = inspect(&success_audit).expect("temporal wal");
    assert_eq!(view.requests.len(), 1);
    assert_eq!(view.requests[0].state, "succeeded");
    assert_eq!(view.requests[0].project, NATIVE_SOURCE);
    assert_eq!(view.requests[0].role, "oo_r_reader");
    assert_eq!(view.frames.len(), 2);
    proof.set("temporal_query_exit", json!(0));
    proof.set("temporal_rows", json!(2));
    proof.set("temporal_date_value", json!(day_text));
    proof.set("temporal_timestamp_value", json!(stamp_text));
    proof.set("temporal_timestamp_naive", json!(true));
    proof.set("temporal_null", json!(true));
    proof.set("temporal_oracle_match", json!(true));
    proof.set("temporal_pack_digest", json!(pack_digest));
    proof.set("temporal_audit", json!("succeeded"));

    let hidden_audit = work.join("temporal-audit-hidden");
    let mut hidden_doc = authority_doc.clone();
    hidden_doc["audit_dir"] = json!(hidden_audit.to_string_lossy());
    let hidden_authority = work.join("temporal-authority-hidden.json");
    write_json(&hidden_authority, &hidden_doc);
    let hidden = cli(&[
        "query",
        "--authority",
        hidden_authority.to_str().unwrap(),
        "--query",
        work.join("temporal-hidden.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&hidden, login_pass);
    assert_eq!(hidden.code, 6, "temporal hidden stderr={}", hidden.stderr);
    assert_eq!(hidden.stderr, "operator-preview: guard\n");
    no_rows(&hidden);
    let hidden_view = inspect(&hidden_audit).expect("temporal hidden wal");
    assert!(hidden_view.requests.iter().any(|item| item.state == "not_admitted"));
    proof.set("temporal_guard_exit", json!(6));

    let mut wide = temporal_policy();
    wide["table"]["columns"].as_array_mut().unwrap().push(json!({
        "name": "note",
        "status": "confirmed",
        "semantic_type": "bytea",
        "pii": {"class": "none"}
    }));
    write_json(&work.join("temporal-policy-wide.json"), &wide);
    let wide_audit = work.join("temporal-audit-wide");
    let mut wide_doc = authority_doc.clone();
    wide_doc["policy_path"] = json!(work.join("temporal-policy-wide.json").to_string_lossy());
    wide_doc["audit_dir"] = json!(wide_audit.to_string_lossy());
    let wide_authority = work.join("temporal-authority-wide.json");
    write_json(&wide_authority, &wide_doc);
    expect_pack(
        &wide_authority,
        &work.join("temporal-success.json"),
        &wide_audit,
        login_pass,
    )
    .await;
    proof.set("temporal_pack_mismatch_exit", json!(9));
    proof.set("temporal_date_oid", json!(1082));
    proof.set("temporal_timestamp_oid", json!(1114));
    proof.set("temporal_arrow_source", json!("4a2ebdb6e3dbd2f24aefda6d9b9a781a5b01b913"));
    proof.set("temporal_arrow_receipt", json!("e88cb4dd8943e74f20f7483fa7612198b5d4acf5"));
    proof.set("temporal_doris_source", json!("910106e99158276c1ed2e5f0554527f17c724149"));
    proof.set("temporal_oracle_source", json!("9cc26104c7a40d23d38d2d9c436027b7876eee8e"));
    proof.set("temporal_oracle_receipt", json!("a6b350652b2bcfc41c349206b38ffbf159141518"));
    proof.set("temporal_status", json!("proved"));
}

fn jsonb_policy() -> Value {
    let mut policy = native_policy();
    policy["table"]["name"] = json!(BYTEA_TABLE);
    policy["table"]["columns"] = json!([{
        "name": JSONB_COLUMN,
        "status": "confirmed",
        "semantic_type": "jsonb",
        "pii": {"class": "none"}
    }]);
    policy
}

fn jsonb_role_documents() -> (String, String) {
    let effective = json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": NATIVE_SOURCE,
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-jsonb", "version": "1"},
        "knowledge": {"id": "know-jsonb", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "acme",
            "locale": "zh-CN",
            "include_schemas": [NATIVE_NAMESPACE],
            "default_tier_preference": ["file"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": [{
            "schema_version": "oos/v1",
            "dataset": BYTEA_DATASET,
            "label": "Document",
            "tier": "file",
            "grain": "one row",
            "primary_key": [JSONB_COLUMN],
            "columns": [{"name": JSONB_COLUMN, "pii": "none", "classification": {"state": "confirmed"}}]
        }]
    });
    let visibility = json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": NATIVE_SOURCE,
        "principal": "operator",
        "role": "oo_r_reader",
        "ontology_version": "ontology-v1",
        "catalog_version": "catalog-v1",
        "policy": {"id": "pol-jsonb", "version": "1"},
        "knowledge": {"id": "know-jsonb", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": [{
            "dataset": BYTEA_DATASET,
            "row_security": "none",
            "global_stats": "denied",
            "columns": [JSONB_COLUMN]
        }],
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    });
    (
        serde_json::to_string(&effective).expect("jsonb effective"),
        serde_json::to_string(&visibility).expect("jsonb visibility"),
    )
}

fn write_jsonb_pack(dir: &Path, data_type: &str, admission: &str) -> String {
    let candidate = json!({
        "dataset": BYTEA_DATASET,
        "fields": [{
            "admission": admission,
            "admitted": true,
            "column": JSONB_COLUMN,
            "data_type": data_type,
            "description": "",
            "label": JSONB_COLUMN
        }],
        "protocol": CANDIDATE_PROTOCOL
    });
    let bytes = serde_json::to_vec(&candidate).expect("jsonb candidate");
    let digest = sha256_hex(&bytes);
    let relative = "candidates/oo_dl01w1_44.byte_docs.json";
    fs::create_dir_all(dir.join("candidates")).expect("jsonb candidate dir");
    fs::write(dir.join(relative), &bytes).expect("jsonb candidate file");
    let receipt = json!({
        "binding": {
            "import_generation": NATIVE_GENERATION,
            "import_hash": NATIVE_HASH,
            "manifest_sha256": NATIVE_MANIFEST,
            "snapshot_id": NATIVE_SNAPSHOT,
            "source_id": NATIVE_SOURCE,
            "target_config_sha256": NATIVE_CONFIG
        },
        "files": {"candidates": [{"path": relative, "sha256": digest}]},
        "pack_digest": digest,
        "protocol": PACK_RECEIPT_PROTOCOL,
        "publication": "not_published",
        "review": "unreviewed"
    });
    write_json(&dir.join("receipt.json"), &receipt);
    digest
}

fn jsonb_sql() -> &'static str {
    "SELECT doc FROM oo_dl01w1_44.byte_docs ORDER BY doc"
}

fn jsonb_authority(
    work: &Path,
    pack_dir: &Path,
    role_dir: &Path,
    ca_path: &Path,
    login_pass_path: &Path,
    host: &str,
    audit: &Path,
) -> Value {
    let mut authority_doc = native_authority(work, pack_dir, ca_path, login_pass_path, host, audit);
    authority_doc["policy_pack_id"] = json!("pol-jsonb");
    authority_doc["knowledge"] = json!({"id": "know-jsonb", "version": "1"});
    authority_doc["policy_path"] = json!(work.join("jsonb-policy.json").to_string_lossy());
    authority_doc["role_pack_dir"] = json!(role_dir.to_string_lossy());
    authority_doc["state_dir"] = json!(work.join("jsonb-state").to_string_lossy());
    authority_doc["receipt_dir"] = json!(work.join("jsonb-receipts").to_string_lossy());
    authority_doc
}

async fn jsonb_oracle(admin: &Client, table: &str, column: &str) -> Vec<Option<String>> {
    let sql = format!(
        "SELECT CASE WHEN {column} IS NULL THEN NULL ELSE {column}::text END \
         FROM \"_oo\".{table} ORDER BY {column}",
        column = quote_physical(column, "pc_"),
        table = quote_physical(table, "pt_")
    );
    let rows = admin
        .query(&sql, &[])
        .await
        .unwrap_or_else(|err| panic!("jsonb oracle sqlstate={} err={err}", sqlstate(&err)));
    assert_eq!(rows.len(), 2);
    rows.iter().map(|row| row.get(0)).collect()
}

fn assert_jsonb_document(text: &str) {
    assert!(text.contains("9007199254740993"), "document lost the integer digits: {text}");
    assert!(!text.contains("9007199254740992"), "document rounded the integer: {text}");
    assert!(
        !text.contains("e+") && !text.contains("e-") && !text.contains("E+") && !text.contains("E-"),
        "document used exponent form: {text}"
    );
    assert!(text.contains(' '), "canonical jsonb text keeps spaces: {text}");
    let parsed: Value = serde_json::from_str(text).expect("jsonb text");
    assert_eq!(parsed["a"]["n"].as_i64(), Some(9_007_199_254_740_993));
    assert_eq!(parsed["missing"], Value::Null);
    assert_eq!(parsed["box"], json!({}));
    assert_eq!(parsed["tags"], json!(["red", "blue"]));
    assert_eq!(parsed["z"].as_i64(), Some(1));
}

async fn prove_jsonb(
    host: &str,
    admin: &mut Client,
    login_pass: &str,
    ca: &[u8],
    ca_path: &Path,
    login_pass_path: &Path,
    work: &Path,
    proof: &mut Proof,
    table: &str,
    doc_column: &str,
) {
    assert_eq!(column_oid(admin, table, doc_column).await, 3802);
    assert_eq!(column_typmod(admin, table, doc_column).await, -1);
    assert!(grantable_base_type("jsonb", -1));
    assert!(!grantable_base_type("jsonb", 0));
    assert!(!grantable_base_type("jsonb", 4));
    proof.set("jsonb_atttypid", json!(3802));
    proof.set("jsonb_atttypmod", json!(-1));

    let catalog = load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "jsonb-policy-direct.json",
        &jsonb_policy(),
    )
    .await
    .expect("shared catalog admits sealed jsonb");
    assert_eq!(catalog.relations.len(), 1);
    assert_eq!(catalog.relations[0].columns.len(), 1);
    assert_eq!(catalog.relations[0].columns[0].name, JSONB_COLUMN);
    assert_eq!(catalog.relations[0].columns[0].type_oid, 3802);
    proof.set("jsonb_catalog", json!("admitted"));

    let pack_dir = work.join("jsonb-pack");
    let pack_digest = write_jsonb_pack(&pack_dir, "jsonb", "sealed_import");
    let (effective, visibility) = jsonb_role_documents();
    let role_pack = compile_role_pack(&effective, &visibility)
        .unwrap_or_else(|err| panic!("jsonb role pack compile failed: {err}"));
    assert!(!role_pack.artifact.contains(SENTINEL));
    let role_dir = work.join("jsonb-role-pack");
    fs::create_dir_all(&role_dir).expect("jsonb role dir");
    fs::write(role_dir.join("preview.json"), &role_pack.artifact).expect("jsonb role artifact");
    write_json(&work.join("jsonb-policy.json"), &jsonb_policy());
    let success_audit = work.join("jsonb-audit-success");
    let authority_doc = jsonb_authority(
        work,
        &pack_dir,
        &role_dir,
        ca_path,
        login_pass_path,
        host,
        &success_audit,
    );
    let authority = work.join("jsonb-authority.json");
    write_json(&authority, &authority_doc);
    write_json(
        &work.join("jsonb-success.json"),
        &query_value(jsonb_sql(), 20, 65_536, 10_000),
    );
    write_json(
        &work.join("jsonb-hidden.json"),
        &query_value("SELECT note FROM oo_dl01w1_44.byte_docs", 10, 8192, 5000),
    );
    write_json(
        &work.join("jsonb-cancel.json"),
        &query_value(jsonb_sql(), 20, 65_536, 20_000),
    );

    let described = cli(&["describe", "--authority", authority.to_str().unwrap()]).await;
    assert_clean(&described, login_pass);
    assert_eq!(described.code, 0, "jsonb describe stderr={}", described.stderr);
    let described_json = described.json.expect("jsonb describe");
    assert_eq!(described_json["outcome"], json!("described"));
    assert_eq!(described_json["knowledge"]["dataset"], json!(BYTEA_DATASET));
    let names: Vec<&str> = described_json["knowledge"]["columns"]
        .as_array()
        .expect("jsonb columns")
        .iter()
        .map(|column| column["name"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(names, vec![JSONB_COLUMN]);
    proof.set("jsonb_describe_exit", json!(0));

    let success = cli(&[
        "query",
        "--authority",
        authority.to_str().unwrap(),
        "--query",
        work.join("jsonb-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&success, login_pass);
    assert_eq!(success.code, 0, "jsonb query stderr={}", success.stderr);
    let success_json = success.json.expect("jsonb query");
    assert_eq!(success_json["outcome"], json!("succeeded"));
    assert_eq!(success_json["type_path"], json!("native_contract"));
    assert_eq!(success_json["shared_catalog"], json!("admitted"));
    assert_eq!(success_json["rollback_confirmed"], json!(true));
    assert_eq!(success_json["returned_rows"], json!(2));
    assert!(!jsonb_sql().contains("::"));
    let oracle = jsonb_oracle(admin, table, doc_column).await;
    let rows = success_json["rows"].as_array().expect("jsonb rows");
    assert_eq!(rows.len(), 2);
    let document = oracle[0].as_deref().expect("jsonb document");
    assert!(oracle[1].is_none());
    assert_jsonb_document(document);
    assert_eq!(rows[0][0]["kind"], json!("jsonb"));
    assert_eq!(rows[0][0]["value"], json!(document));
    assert!(rows[0][0]["value"].is_string());
    assert_eq!(rows[1][0]["kind"], json!("null"));
    assert!(rows[1][0].get("value").is_none());
    let view = inspect(&success_audit).expect("jsonb wal");
    assert_eq!(view.requests.len(), 1);
    assert_eq!(view.requests[0].state, "succeeded");
    assert_eq!(view.requests[0].project, NATIVE_SOURCE);
    assert_eq!(view.requests[0].role, "oo_r_reader");
    assert_eq!(view.frames.len(), 2);
    let success_pid = success_json["backend_pid"].as_i64().expect("jsonb pid");
    proof.set("jsonb_query_exit", json!(0));
    proof.set("jsonb_rows", json!(2));
    proof.set("jsonb_document", json!(document));
    proof.set("jsonb_integer_digits", json!("9007199254740993"));
    proof.set("jsonb_sql_null", json!(true));
    proof.set("jsonb_oracle_match", json!(true));
    proof.set("jsonb_pack_digest", json!(pack_digest));
    proof.set("jsonb_audit", json!("succeeded"));
    proof.set("jsonb_rollback", json!(true));

    let hidden_audit = work.join("jsonb-audit-hidden");
    let mut hidden_doc = authority_doc.clone();
    hidden_doc["audit_dir"] = json!(hidden_audit.to_string_lossy());
    let hidden_authority = work.join("jsonb-authority-hidden.json");
    write_json(&hidden_authority, &hidden_doc);
    let hidden = cli(&[
        "query",
        "--authority",
        hidden_authority.to_str().unwrap(),
        "--query",
        work.join("jsonb-hidden.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&hidden, login_pass);
    assert_eq!(hidden.code, 6, "jsonb hidden stderr={}", hidden.stderr);
    assert_eq!(hidden.stderr, "operator-preview: guard\n");
    no_rows(&hidden);
    assert!(!hidden.stdout.contains("9007199254740993"));
    let hidden_view = inspect(&hidden_audit).expect("jsonb hidden wal");
    assert!(hidden_view.requests.iter().any(|item| item.state == "not_admitted"));
    proof.set("jsonb_guard_exit", json!(6));

    let mut wide = jsonb_policy();
    wide["table"]["columns"].as_array_mut().unwrap().push(json!({
        "name": "note",
        "status": "confirmed",
        "semantic_type": "bytea",
        "pii": {"class": "none"}
    }));
    write_json(&work.join("jsonb-policy-wide.json"), &wide);
    let wide_audit = work.join("jsonb-audit-wide");
    let mut wide_doc = authority_doc.clone();
    wide_doc["policy_path"] = json!(work.join("jsonb-policy-wide.json").to_string_lossy());
    wide_doc["audit_dir"] = json!(wide_audit.to_string_lossy());
    let wide_authority = work.join("jsonb-authority-wide.json");
    write_json(&wide_authority, &wide_doc);
    expect_pack(
        &wide_authority,
        &work.join("jsonb-success.json"),
        &wide_audit,
        login_pass,
    )
    .await;
    proof.set("jsonb_pack_mismatch_exit", json!(9));

    let fake_dir = work.join("jsonb-pack-fake");
    write_jsonb_pack(&fake_dir, "json", "sealed_import");
    let fake_audit = work.join("jsonb-audit-fake");
    let mut fake_doc = authority_doc.clone();
    fake_doc["pack_dir"] = json!(fake_dir.to_string_lossy());
    fake_doc["pack_receipt"] = json!(fake_dir.join("receipt.json").to_string_lossy());
    fake_doc["audit_dir"] = json!(fake_audit.to_string_lossy());
    let fake_authority = work.join("jsonb-authority-fake.json");
    write_json(&fake_authority, &fake_doc);
    expect_pack(
        &fake_authority,
        &work.join("jsonb-success.json"),
        &fake_audit,
        login_pass,
    )
    .await;
    proof.set("jsonb_fake_seal_exit", json!(9));

    let claimed_dir = work.join("jsonb-pack-claimed");
    write_jsonb_pack(&claimed_dir, "jsonb", "claimed");
    let claimed_audit = work.join("jsonb-audit-claimed");
    let mut claimed_doc = authority_doc.clone();
    claimed_doc["pack_dir"] = json!(claimed_dir.to_string_lossy());
    claimed_doc["pack_receipt"] = json!(claimed_dir.join("receipt.json").to_string_lossy());
    claimed_doc["audit_dir"] = json!(claimed_audit.to_string_lossy());
    let claimed_authority = work.join("jsonb-authority-claimed.json");
    write_json(&claimed_authority, &claimed_doc);
    expect_pack(
        &claimed_authority,
        &work.join("jsonb-success.json"),
        &claimed_audit,
        login_pass,
    )
    .await;
    proof.set("jsonb_unsealed_admission_exit", json!(9));

    let other_audit = work.join("jsonb-audit-other-role");
    let mut other_doc = authority_doc.clone();
    other_doc["role"] = json!("oo_r_other");
    other_doc["audit_dir"] = json!(other_audit.to_string_lossy());
    let other_authority = work.join("jsonb-authority-other-role.json");
    write_json(&other_authority, &other_doc);
    let other = cli(&[
        "describe",
        "--authority",
        other_authority.to_str().unwrap(),
    ])
    .await;
    assert_clean(&other, login_pass);
    assert_eq!(other.code, 9, "jsonb cross-role stderr={}", other.stderr);
    assert!(!other.stdout.contains("9007199254740993"));
    proof.set("jsonb_cross_role_exit", json!(9));

    let quoted_doc = quote_physical(doc_column, "pc_");
    let quoted_table = quote_physical(table, "pt_");
    exec(
        admin,
        &format!("REVOKE SELECT ({quoted_doc}) ON TABLE \"_oo\".{quoted_table} FROM \"oo_r_reader\""),
    )
    .await;
    let revoked = load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "jsonb-policy-revoked.json",
        &jsonb_policy(),
    )
    .await
    .expect("revoked jsonb catalog");
    assert!(
        revoked.relations.iter().all(|relation| relation.columns.iter().all(|column| column.type_oid != 3802)),
        "revoked role still published jsonb"
    );
    let revoke_audit = work.join("jsonb-audit-revoke");
    let mut revoke_doc = authority_doc.clone();
    revoke_doc["audit_dir"] = json!(revoke_audit.to_string_lossy());
    let revoke_authority = work.join("jsonb-authority-revoke.json");
    write_json(&revoke_authority, &revoke_doc);
    let revoke = cli(&[
        "query",
        "--authority",
        revoke_authority.to_str().unwrap(),
        "--query",
        work.join("jsonb-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&revoke, login_pass);
    assert_eq!(revoke.code, 6, "jsonb revoke stderr={} stdout={}", revoke.stderr, revoke.stdout);
    no_rows(&revoke);
    assert!(!revoke.stdout.contains("9007199254740993"));
    proof.set("jsonb_revoke_exit", json!(6));
    exec(
        admin,
        &format!("GRANT SELECT ({quoted_doc}) ON TABLE \"_oo\".{quoted_table} TO \"oo_r_reader\""),
    )
    .await;

    exec(
        admin,
        &format!(
            "ALTER TABLE \"_oo\".{quoted_table} ALTER COLUMN {quoted_doc} TYPE json USING {quoted_doc}::json"
        ),
    )
    .await;
    assert_eq!(column_oid(admin, table, doc_column).await, 114);
    let wrong = load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "jsonb-policy-wrong-oid.json",
        &jsonb_policy(),
    )
    .await
    .expect("json oid is grantable but not jsonb");
    assert_eq!(wrong.relations[0].columns[0].type_oid, 114);
    let wrong_audit = work.join("jsonb-audit-wrong-oid");
    let mut wrong_doc = authority_doc.clone();
    wrong_doc["audit_dir"] = json!(wrong_audit.to_string_lossy());
    let wrong_authority = work.join("jsonb-authority-wrong-oid.json");
    write_json(&wrong_authority, &wrong_doc);
    expect_pack(
        &wrong_authority,
        &work.join("jsonb-success.json"),
        &wrong_audit,
        login_pass,
    )
    .await;
    proof.set("jsonb_wrong_oid", json!(114));
    proof.set("jsonb_wrong_oid_exit", json!(9));
    exec(
        admin,
        &format!(
            "ALTER TABLE \"_oo\".{quoted_table} ALTER COLUMN {quoted_doc} TYPE jsonb USING {quoted_doc}::jsonb"
        ),
    )
    .await;
    assert_eq!(column_oid(admin, table, doc_column).await, 3802);
    assert_eq!(column_typmod(admin, table, doc_column).await, -1);
    let restore_audit = work.join("jsonb-audit-restore");
    let mut restore_doc = authority_doc.clone();
    restore_doc["audit_dir"] = json!(restore_audit.to_string_lossy());
    let restore_authority = work.join("jsonb-authority-restore.json");
    write_json(&restore_authority, &restore_doc);
    let restored = cli(&[
        "query",
        "--authority",
        restore_authority.to_str().unwrap(),
        "--query",
        work.join("jsonb-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&restored, login_pass);
    assert_eq!(restored.code, 0, "jsonb restore stderr={}", restored.stderr);
    let restored_json = restored.json.expect("jsonb restore");
    let restored_text = restored_json["rows"][0][0]["value"].as_str().expect("restored text");
    assert_jsonb_document(restored_text);
    assert_eq!(restored_json["rows"][0][0]["kind"], json!("jsonb"));
    proof.set("jsonb_restored_exit", json!(0));

    let missing = cli(&[
        "cancel",
        "--authority",
        authority.to_str().unwrap(),
        "--execution",
        "00000000-0000-4000-8000-000000000098",
    ])
    .await;
    assert_clean(&missing, login_pass);
    assert_eq!(missing.code, 10, "jsonb absent stderr={}", missing.stderr);
    assert_eq!(missing.json.as_ref().unwrap()["outcome"], json!("not_sent"));
    assert_eq!(missing.json.as_ref().unwrap()["reason"], json!("absent"));
    proof.set("jsonb_absent_cancel_exit", json!(10));

    let cancel_audit = work.join("jsonb-audit-cancel");
    let mut cancel_doc = authority_doc.clone();
    cancel_doc["audit_dir"] = json!(cancel_audit.to_string_lossy());
    let cancel_authority = work.join("jsonb-authority-cancel.json");
    write_json(&cancel_authority, &cancel_doc);
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let locker = connect(
        host,
        DATABASE,
        ADMIN,
        &read_pass(&PathBuf::from(require_env("OO_DL01W121_ADMIN_PASS"))),
        ca,
    )
    .await;
    let lock_task = tokio::spawn(hold_lock(locker, table.to_string(), release_rx));
    tokio::time::sleep(Duration::from_millis(200)).await;
    let authority_text = cancel_authority.to_str().unwrap().to_string();
    let cancel_query = work.join("jsonb-cancel.json").to_str().unwrap().to_string();
    let query_task = tokio::task::spawn_blocking(move || {
        run_cli(&[
            "query".to_string(),
            "--authority".to_string(),
            authority_text,
            "--query".to_string(),
            cancel_query,
        ])
    });
    let mut running = None;
    for _ in 0..200 {
        let dir = work.join("jsonb-state/executions");
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let Ok(text) = fs::read_to_string(entry.path()) else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if value.get("status").and_then(Value::as_str) == Some("running")
                    && value.get("execution_id").and_then(Value::as_str).is_some()
                {
                    let path_name = entry.file_name();
                    let id = value["execution_id"].as_str().unwrap_or("");
                    if path_name.to_string_lossy().contains(id) {
                        running = Some(value);
                        break;
                    }
                }
            }
        }
        if running.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let running = running.expect("jsonb cancel target did not stay running");
    let pid = running["backend_pid"].as_i64().expect("pid") as i32;
    let mut saw_active = false;
    for _ in 0..40 {
        match observe_backend(admin, pid).await {
            Ok(Some(view)) if view.state() == "active" => {
                saw_active = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(saw_active, "jsonb cancel backend was not active");
    let cancel_id = running["execution_id"].as_str().unwrap().to_string();
    let authority_text = cancel_authority.to_str().unwrap().to_string();
    let cancel_ran = tokio::task::spawn_blocking(move || {
        run_cli(&[
            "cancel".to_string(),
            "--authority".to_string(),
            authority_text,
            "--execution".to_string(),
            cancel_id,
        ])
    })
    .await
    .expect("jsonb cancel join");
    let query_ran = tokio::time::timeout(Duration::from_secs(30), query_task)
        .await
        .expect("jsonb cancel query hung")
        .expect("jsonb query join");
    let _ = release_tx.send(());
    let _ = lock_task.await;
    assert_clean(&query_ran, login_pass);
    assert_clean(&cancel_ran, login_pass);
    assert_eq!(cancel_ran.code, 0, "jsonb cancel stderr={}", cancel_ran.stderr);
    assert_eq!(cancel_ran.json.as_ref().unwrap()["outcome"], json!("sent"));
    assert_eq!(query_ran.code, 0, "jsonb canceled stderr={}", query_ran.stderr);
    let canceled = query_ran.json.as_ref().unwrap();
    assert_eq!(canceled["outcome"], json!("canceled"));
    assert_eq!(canceled["sqlstate"], json!("57014"));
    assert_eq!(canceled["rollback_confirmed"], json!(true));
    assert_eq!(canceled["stop"], json!("canceled"));
    no_rows(&query_ran);
    assert!(!query_ran.stdout.contains("9007199254740993"));
    let cancel_view = inspect(&cancel_audit).expect("jsonb cancel wal");
    assert_eq!(cancel_view.requests[0].state, "failed");
    proof.set("jsonb_cancel_exit", json!(0));
    proof.set("jsonb_cancel_sqlstate", json!("57014"));
    proof.set("jsonb_cancel_active", json!(saw_active));
    proof.set("jsonb_cancel_rollback", json!(true));

    let late_id = execution_of(&success_json);
    let late = cli(&[
        "cancel",
        "--authority",
        authority.to_str().unwrap(),
        "--execution",
        &late_id,
    ])
    .await;
    assert_eq!(late.code, 10, "jsonb late stderr={}", late.stderr);
    assert_eq!(late.json.as_ref().unwrap()["reason"], json!("late"));
    proof.set("jsonb_late_cancel_exit", json!(10));

    let reopen_audit = work.join("jsonb-audit-reopen");
    let mut reopen_doc = authority_doc.clone();
    reopen_doc["audit_dir"] = json!(reopen_audit.to_string_lossy());
    let reopen_authority = work.join("jsonb-authority-reopen.json");
    write_json(&reopen_authority, &reopen_doc);
    let reopened = cli(&[
        "query",
        "--authority",
        reopen_authority.to_str().unwrap(),
        "--query",
        work.join("jsonb-success.json").to_str().unwrap(),
    ])
    .await;
    assert_clean(&reopened, login_pass);
    assert_eq!(reopened.code, 0, "jsonb reopen stderr={}", reopened.stderr);
    let reopened_json = reopened.json.expect("jsonb reopen");
    let reopen_text = reopened_json["rows"][0][0]["value"].as_str().expect("reopen text");
    assert_jsonb_document(reopen_text);
    assert_ne!(reopened_json["backend_pid"].as_i64().expect("reopen pid"), success_pid);
    proof.set("jsonb_reopen_exit", json!(0));
    proof.set("jsonb_status", json!("proved"));
}

fn numeric_column_policy(name: &str) -> Value {
    let mut policy = native_policy();
    policy["table"]["columns"] = json!([{
        "name": name,
        "status": "confirmed",
        "semantic_type": "numeric",
        "pii": {"class": "none"}
    }]);
    policy
}

async fn load_named_catalog(
    host: &str,
    login_pass: &str,
    ca: &[u8],
    work: &Path,
    file_name: &str,
    policy: &Value,
) -> Result<oo_gateway::PolicyCatalog, CatalogError> {
    let path = work.join(file_name);
    write_json(&path, policy);
    let loaded = load_policy(&path).expect("numeric policy");
    let compiled = compile_column_acl(&loaded.acl).expect("numeric compile");
    let identity = PolicyIdentity {
        project: NATIVE_SOURCE.to_string(),
        role: "oo_r_reader".to_string(),
        policy_id: NATIVE_CONFIG.to_string(),
    };
    let limits = CatalogLimits {
        max_tables: 4,
        max_columns: 64,
        max_functions: 1,
        max_operators: 64,
        large_table_row_threshold: 1_000_000,
        unknown_rows: UnknownRows::Zero,
    };
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port: 5432,
        database: DATABASE.to_string(),
        user: LOGIN.to_string(),
    };
    let (mut client, mut connection) = connect_require_tls(&endpoint, login_pass, ca)
        .await
        .unwrap_or_else(|err| connect_panic(&err));
    let result = {
        let mut load = std::pin::pin!(load_policy_catalog(
            &mut client,
            &identity,
            std::slice::from_ref(&compiled),
            &limits,
        ));
        tokio::select! {
            biased;
            result = &mut load => result,
            _ = &mut connection => panic!("numeric catalog connection closed"),
        }
    };
    drop(client);
    result
}

async fn prove_decimal256(
    host: &str,
    admin: &Client,
    login_pass: &str,
    ca: &[u8],
    table: &str,
    amount_column: &str,
    work: &Path,
    proof: &mut Proof,
) {
    let digits = "9".repeat(76);
    let literal = format!("{}.{}", &digits[..66], &digits[66..]);
    let stored_type: i32 = admin
        .query_one(
            "SELECT attribute.atttypmod \
             FROM pg_catalog.pg_attribute AS attribute \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = attribute.attrelid \
             WHERE relation.relname = $1 AND attribute.attname = $2",
            &[&table, &amount_column],
        )
        .await
        .unwrap_or_else(|err| panic!("decimal typmod sqlstate={}", sqlstate(&err)))
        .get(0);
    assert_eq!(stored_type, 4_980_750, "amount column is not numeric(76,10)");
    let stored: String = admin
        .query_one(
            &format!(
                "SELECT {column}::text FROM \"_oo\".{table} LIMIT 1",
                table = quote_physical(table, "pt_"),
                column = quote_physical(amount_column, "pc_"),
            ),
            &[],
        )
        .await
        .unwrap_or_else(|err| panic!("decimal text sqlstate={}", sqlstate(&err)))
        .get(0);
    assert_eq!(stored, literal, "postgres did not keep the 76-digit coefficient");

    let catalog = load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "native-policy-amount.json",
        &numeric_column_policy("amount"),
    )
    .await
    .expect("shared catalog admits numeric(76,10)");
    assert_eq!(catalog.relations.len(), 1);
    assert_eq!(catalog.relations[0].columns.len(), 1);
    assert_eq!(catalog.relations[0].columns[0].name, "amount");
    assert_eq!(catalog.relations[0].columns[0].type_oid, 1700);
    let identity = PolicyIdentity {
        project: NATIVE_SOURCE.to_string(),
        role: "oo_r_reader".to_string(),
        policy_id: NATIVE_CONFIG.to_string(),
    };
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port: 5432,
        database: DATABASE.to_string(),
        user: LOGIN.to_string(),
    };
    let tls = rustls_connector(ca).expect("decimal connector");
    let (client, connection) = connect_require_tls(&endpoint, login_pass, ca)
        .await
        .unwrap_or_else(|err| connect_panic(&err));
    let mut session = DedicatedSession::bind(client, connection, tls)
        .await
        .expect("decimal bind");
    let audit = work.join("native-audit-decimal");
    let wal = gateway_audit_wal::DiskWal::open(
        &audit,
        gateway_audit_wal::WalConfig {
            max_bytes: 67_108_864,
        },
    )
    .expect("decimal wal");
    let sql = "SELECT amount FROM oo_dl01w1_44.synthetic_docs";
    let mut stream = session
        .execute_trusted_select(
            SelectRequest {
                identity: &identity,
                catalog: &catalog,
                original_sql: sql,
                role: &identity.role,
                max_rows: 10,
                max_result_bytes: 8192,
                wall_clock: Duration::from_millis(5_000),
            },
            wal,
        )
        .await
        .expect("decimal stream");
    let mut rows = Vec::new();
    while let Some(row) = stream.next_row().await.expect("decimal row") {
        rows.push(row);
    }
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row.len(), 1);
        match &row[0] {
            Cell::Decimal(value) => {
                assert_eq!(value.coefficient(), digits.as_str());
                assert_eq!(value.scale(), 10);
                assert!(!value.is_negative());
            }
            other => panic!("decimal cell was {other:?}"),
        }
    }
    drop(stream);
    session.wait_idle().await.expect("decimal idle");
    drop(session);
    proof.set("decimal256_coefficient_digits", json!(76));

    let wide = load_named_catalog(
        host,
        login_pass,
        ca,
        work,
        "native-policy-wide-precision.json",
        &numeric_column_policy("wide"),
    )
    .await;
    match wide {
        Err(CatalogError::UnsupportedType) => {}
        other => panic!("precision 77 catalog ended as {other:?}"),
    }
    proof.set("decimal256_precision_77", json!("unsupported_type"));
}

async fn native_oracle(admin: &Client, table: &str, columns: &[String]) -> Vec<Value> {
    let sql = format!(
        "SELECT encode(float8send({double}), 'hex'), \
                to_char({event} AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
                {tags}::text, {payload}::text \
         FROM \"_oo\".{table} ORDER BY {doc}",
        double = quote_physical(&columns[3], "pc_"),
        event = quote_physical(&columns[4], "pc_"),
        tags = quote_physical(&columns[12], "pc_"),
        payload = quote_physical(&columns[8], "pc_"),
        table = quote_physical(table, "pt_"),
        doc = quote_physical(&columns[0], "pc_")
    );
    let rows = admin
        .query(&sql, &[])
        .await
        .unwrap_or_else(|err| panic!("native oracle sqlstate={}", sqlstate(&err)));
    assert_eq!(rows.len(), 2);
    rows.iter()
        .map(|row| {
            json!({
                "bits": row.get::<_, String>(0),
                "event": row.get::<_, String>(1),
                "tags": row.get::<_, String>(2),
                "payload": row.get::<_, String>(3),
            })
        })
        .collect()
}

async fn insert_native_rows(
    admin: &mut Client,
    table: &str,
    columns: &[String],
    raw: &str,
    amount: &str,
) {
    let mut names = vec!["\"_oo_pos\"".to_string()];
    for column in columns {
        names.push(quote_physical(column, "pc_"));
    }
    names.push(quote_physical(raw, "pc_"));
    names.push(quote_physical(amount, "pc_"));
    let digits = "9".repeat(76);
    let amount_text = format!("{}.{}", &digits[..66], &digits[66..]);
    let sql = format!(
        "INSERT INTO \"_oo\".{table} ({names}) VALUES (\
         ($1::text)::numeric, $2, $3, $4, $5, ($6::text)::timestamptz, $7, $8, $9, $10, $11, $12, $13, ($14::text)::json, $15, $16, ($17::text)::numeric(76,10))",
        table = quote_physical(table, "pt_"),
        names = names.join(", ")
    );
    let tx = admin.transaction().await.expect("native insert");
    let rows: [NativeRow; 2] = [
        NativeRow {
            pos: "1",
            doc: "doc-a",
            index: "idx-a",
            key: "key-a",
            double: 1.5,
            event: "2024-03-15 12:34:56+00",
            flag: true,
            long: 11,
            note: None,
            payload: "{\"a\":1}",
            quantities: "[1,2]",
            scaled: 21,
            seq: 31,
            tags: "{\"t\":\"x\"}",
            thing: "{\"k\":\"v\"}",
        },
        NativeRow {
            pos: "2",
            doc: "doc-b",
            index: "idx-b",
            key: "key-b",
            double: -1.5,
            event: "2024-03-16 01:02:03+00",
            flag: false,
            long: 12,
            note: Some("beta"),
            payload: "{\"a\":2}",
            quantities: "[3]",
            scaled: 22,
            seq: 32,
            tags: "{\"t\":\"y\"}",
            thing: "{\"k\":\"w\"}",
        },
    ];
    for row in &rows {
        let note = row.note.map(str::to_string);
        let raw_bytes = SENTINEL.as_bytes().to_vec();
        let params: [&(dyn tokio_postgres::types::ToSql + Sync); 17] = [
            &row.pos,
            &row.doc,
            &row.index,
            &row.key,
            &row.double,
            &row.event,
            &row.flag,
            &row.long,
            &note,
            &row.payload,
            &row.quantities,
            &row.scaled,
            &row.seq,
            &row.tags,
            &row.thing,
            &raw_bytes,
            &amount_text,
        ];
        tx.execute(&sql, &params)
            .await
            .unwrap_or_else(|err| panic!("native insert sqlstate={}", sqlstate(&err)));
    }
    tx.commit().await.expect("native insert commit");
}

struct NativeRow {
    pos: &'static str,
    doc: &'static str,
    index: &'static str,
    key: &'static str,
    double: f64,
    event: &'static str,
    flag: bool,
    long: i64,
    note: Option<&'static str>,
    payload: &'static str,
    quantities: &'static str,
    scaled: i64,
    seq: i64,
    tags: &'static str,
    thing: &'static str,
}
