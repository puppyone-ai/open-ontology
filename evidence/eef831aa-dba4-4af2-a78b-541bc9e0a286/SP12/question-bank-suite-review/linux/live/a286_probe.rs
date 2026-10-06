//! Evidence driver for one purpose-built TLS fixture.
//!
//! The table is `review.cell` with one integer column and one row. This is not
//! a business table and it is not an authentication claim. The process calls
//! `connect_require_tls` and `install_column_acl`, then the actual `query-pg`,
//! adapter, and comparator binaries.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use oo_gateway::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, InstallEffect, LogicalTable, PiiClass,
    TlsEndpoint, connect_require_tls, install_column_acl,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::{Client, Transaction};

const HOST: &str = "oo-a286-pg";
const DATABASE: &str = "oo_a286";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a286_login";
const OWNER: &str = "a2860000-0000-4000-8000-000000000286";
const SHARD: &str = "shard_a286";
const SOURCE: &str = "review";
const CONFIG: &str = "a286a286a286a286a286a286a286a286a286a286a286a286a286a286a286a286";
const TYPE_DIGEST: &str = "b286b286b286b286b286b286b286b286b286b286b286b286b286b286b286b286";
const INPUT_DIGEST: &str = "c286c286c286c286c286c286c286c286c286c286c286c286c286c286c286c286";
const SCHEMA_DIGEST: &str = "d286d286d286d286d286d286d286d286d286d286d286d286d286d286d286d286";
const NAMESPACE: &str = "review";
const TABLE: &str = "cell";
const COLUMN: &str = "v";
const GENERATION: i64 = 1;
const ADVISORY_LOCK: i64 = 64_042_026_286;
const NOT_FOUND: &str = "{\"kind\":\"not_found\",\"completion\":\"complete\",\"warnings\":[]}\n";

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|_| stop("runtime"));
    runtime.block_on(prepare());
    drop(runtime);
    let zero = pipeline("zero", "SELECT v FROM review.cell WHERE v = 9");
    let nonzero = pipeline("nonzero", "SELECT v FROM review.cell WHERE v = 4");
    let mut oracle = String::new();
    oracle.push_str(&format!("zero_query_exit={}\n", zero.query_exit));
    oracle.push_str(&format!("zero_adapter_exit={}\n", zero.adapter_exit));
    oracle.push_str(&format!("zero_comparator_exit={}\n", zero.comparator_exit));
    oracle.push_str(&format!("zero_verdict={}\n", zero.verdict));
    oracle.push_str(&format!("zero_returned_rows={}\n", zero.returned_rows));
    oracle.push_str(&format!("nonzero_query_exit={}\n", nonzero.query_exit));
    oracle.push_str(&format!("nonzero_adapter_exit={}\n", nonzero.adapter_exit));
    oracle.push_str(&format!("nonzero_comparator_exit={}\n", nonzero.comparator_exit));
    oracle.push_str(&format!("nonzero_verdict={}\n", nonzero.verdict));
    oracle.push_str(&format!("nonzero_kind={}\n", nonzero.kind));
    oracle.push_str(&format!("nonzero_returned_rows={}\n", nonzero.returned_rows));
    let out = out_dir();
    fs::write(out.join("oracle.txt"), &oracle).unwrap_or_else(|_| stop("oracle-write"));
    let pass = zero.query_exit == 0
        && zero.adapter_exit == 0
        && zero.comparator_exit == 0
        && zero.verdict == "match"
        && zero.returned_rows == 0
        && zero.adapter_exact
        && nonzero.query_exit == 0
        && nonzero.adapter_exit == 0
        && nonzero.comparator_exit == 1
        && nonzero.verdict == "mismatch"
        && nonzero.kind == "rows"
        && nonzero.returned_rows == 1
        && !nonzero.adapter_exact;
    if !pass {
        stop("verdict");
    }
}

struct Trial {
    query_exit: i32,
    adapter_exit: i32,
    comparator_exit: i32,
    verdict: String,
    kind: String,
    returned_rows: i64,
    adapter_exact: bool,
}

fn pipeline(name: &str, sql: &str) -> Trial {
    let out = out_dir();
    let request = out.join(format!("{name}-request.json"));
    let audit = out.join(format!("{name}-audit.jsonl"));
    let _ = fs::remove_file(&audit);
    write_request(&request, &audit, sql);
    let query = spawn(
        &env_path("OO_A286_QUERY_PG"),
        &["--request", request.to_str().unwrap_or("")],
        None,
    );
    fs::write(out.join(format!("{name}.query.stdout")), &query.stdout)
        .unwrap_or_else(|_| stop("query-stdout"));
    fs::write(out.join(format!("{name}.query.stderr")), &query.stderr)
        .unwrap_or_else(|_| stop("query-stderr"));
    let header = adapter_header();
    let mut adapter_in = header.into_bytes();
    adapter_in.extend_from_slice(&query.stdout);
    let adapter = spawn(&env_path("OO_A286_ADAPTER"), &[], Some(&adapter_in));
    fs::write(out.join(format!("{name}.adapter.stdout")), &adapter.stdout)
        .unwrap_or_else(|_| stop("adapter-stdout"));
    fs::write(out.join(format!("{name}.adapter.stderr")), &adapter.stderr)
        .unwrap_or_else(|_| stop("adapter-stderr"));
    let actual: Value = serde_json::from_slice(trim_nl(&adapter.stdout)).unwrap_or(Value::Null);
    let document = json!({
        "actual": actual,
        "expected": {"completion": "complete", "kind": "not_found", "warnings": []},
        "row_comparator": "ordered",
        "version": "oo.benchmark.result-comparison/v1"
    });
    let body = serde_json::to_vec(&document).unwrap_or_else(|_| stop("compare-json"));
    let comparator = spawn(&env_path("OO_A286_COMPARATOR"), &[], Some(&body));
    fs::write(out.join(format!("{name}.comparator.stdout")), &comparator.stdout)
        .unwrap_or_else(|_| stop("comparator-stdout"));
    fs::write(
        out.join(format!("{name}.comparator.stderr")),
        &comparator.stderr,
    )
    .unwrap_or_else(|_| stop("comparator-stderr"));
    let verdict_value: Value =
        serde_json::from_slice(trim_nl(&comparator.stdout)).unwrap_or(Value::Null);
    let secret = fs::read(env_path("OO_A286_LOGIN_FILE")).unwrap_or_default();
    for bytes in [
        &query.stdout,
        &query.stderr,
        &adapter.stdout,
        &adapter.stderr,
        &comparator.stdout,
        &comparator.stderr,
    ] {
        if contains(bytes, &secret) {
            stop("secret-in-output");
        }
    }
    Trial {
        query_exit: query.code,
        adapter_exit: adapter.code,
        comparator_exit: comparator.code,
        verdict: verdict_value["verdict"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        kind: actual["kind"].as_str().unwrap_or("").to_string(),
        returned_rows: returned_rows(&query.stdout),
        adapter_exact: adapter.stdout == NOT_FOUND.as_bytes(),
    }
}

fn returned_rows(stdout: &[u8]) -> i64 {
    let text = String::from_utf8_lossy(stdout);
    let Some(line) = text.lines().last() else {
        return -1;
    };
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return -1;
    };
    value["returned_rows"].as_i64().unwrap_or(-1)
}

fn adapter_header() -> String {
    let value = json!({
        "columns": [{"name": COLUMN, "ordinal": 0, "type": "int64"}],
        "empty_result": "not_found_if_complete_empty",
        "max_result_bytes": 4096,
        "max_rows": 20,
        "record": "header",
        "turn": 0,
        "version": "oo.benchmark.query-result-adapter/v1"
    });
    let mut text = serde_json::to_string(&value).unwrap_or_else(|_| stop("header"));
    text.push('\n');
    text
}

fn write_request(path: &Path, audit: &Path, sql: &str) {
    let value = json!({
        "version": "oo.gateway.query-pg/v1",
        "endpoint": {
            "host": HOST,
            "port": 5432,
            "database": DATABASE,
            "user": LOGIN
        },
        "password_file": env_path("OO_A286_LOGIN_FILE"),
        "ca_file": env_path("OO_A286_CA"),
        "audit_file": audit,
        "identity": {
            "project": SOURCE,
            "role": "oo_r_cell",
            "policy_id": CONFIG
        },
        "policies": [{
            "project": SOURCE,
            "config_digest": CONFIG,
            "generation": GENERATION,
            "role": "cell",
            "login": LOGIN,
            "table": {
                "namespace": NAMESPACE,
                "name": TABLE,
                "columns": [{
                    "name": COLUMN,
                    "status": "confirmed",
                    "semantic_type": "value",
                    "pii": {"class": "none"}
                }]
            }
        }],
        "catalog": {
            "max_tables": 4,
            "max_columns": 64,
            "max_functions": 1,
            "max_operators": 64,
            "large_table_row_threshold": 1_000_000,
            "unknown_rows": "refuse"
        },
        "statement": {
            "source": "request",
            "sql": sql,
            "max_rows": 20,
            "max_result_bytes": 4096,
            "wall_clock_ms": 30000
        }
    });
    let bytes = serde_json::to_vec(&value).unwrap_or_else(|_| stop("request"));
    fs::write(path, bytes).unwrap_or_else(|_| stop("request-write"));
}

struct Captured {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn spawn(program: &str, args: &[&str], stdin: Option<&[u8]>) -> Captured {
    let mut child = Command::new(program);
    child
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child.spawn().unwrap_or_else(|_| stop("spawn"));
    if let Some(bytes) = stdin {
        let mut pipe = child.stdin.take().unwrap_or_else(|| stop("stdin"));
        pipe.write_all(bytes).unwrap_or_else(|_| stop("stdin-write"));
    }
    let output = child.wait_with_output().unwrap_or_else(|_| stop("wait"));
    Captured {
        code: output.status.code().unwrap_or(127),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

async fn prepare() {
    let admin_pass = read_secret("OO_A286_ADMIN_FILE");
    let login_pass = read_secret("OO_A286_LOGIN_FILE");
    let ca = fs::read(env_path("OO_A286_CA")).unwrap_or_else(|_| stop("ca"));
    let maintenance = connect("postgres", ADMIN, &admin_pass, &ca).await;
    batch(
        &maintenance,
        "create-database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
    drop(maintenance);
    let mut admin = connect(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_schema(&mut admin).await;
    batch(
        &admin,
        "create-login",
        &format!(
            "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{login_pass}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
        ),
    )
    .await;
    batch(
        &admin,
        "grant-mapping",
        &format!(
            "GRANT USAGE ON SCHEMA _oo TO \"{LOGIN}\";\n\
             GRANT SELECT ON TABLE _oo.physical_mappings TO \"{LOGIN}\";\n\
             GRANT EXECUTE ON FUNCTION _oo.lookup_physical_mapping(text, text, bigint, text, text, text, text) TO \"{LOGIN}\""
        ),
    )
    .await;
    let physical = prepare_slot(&mut admin).await;
    batch(
        &admin,
        "analyze",
        &format!("ANALYZE \"_oo\".{}", quote_physical(&physical.table, "pt_")),
    )
    .await;
    let policy = ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: CONFIG.to_string(),
        generation: GENERATION,
        role: "cell".to_string(),
        login: LOGIN.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            columns: vec![ColumnSpec {
                name: COLUMN.to_string(),
                status: ClassStatus::Confirmed,
                semantic_type: "value".to_string(),
                pii: PiiClass::None,
            }],
        },
        row_filter: None,
    };
    let installed = match install_column_acl(&mut admin, &policy).await {
        Ok(installed) => installed,
        Err(err) => fail_detail("install-column-acl", &err.to_string(), &login_pass, &admin_pass),
    };
    if installed.effect != InstallEffect::Installed || installed.role != "oo_r_cell" {
        stop("install-effect");
    }
}

async fn apply_schema(client: &mut Client) {
    let root = PathBuf::from(env_path("OO_A286_MIGRATIONS"));
    let names = [
        "0000_bootstrap_roles.sql",
        "0001_target_fence_expand.sql",
        "0003_target_segment_preflight_expand.sql",
        "0004_physical_mapping.sql",
        "0005_full_import_expand.sql",
    ];
    let mut files = Vec::new();
    for name in names {
        let bytes = fs::read(root.join(name)).unwrap_or_else(|_| stop("migration"));
        let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| stop("migration-utf8"));
        files.push((name, text, sha256_hex(&bytes)));
    }
    let tx = begin(client).await;
    tx_batch(&tx, "0000", &files[0].1).await;
    tx_batch(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    tx_batch(&tx, "0001", &files[1].1).await;
    tx_batch(
        &tx,
        "row-lock",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    tx_batch(&tx, "0003", &files[2].1).await;
    tx_batch(&tx, "0004", &files[3].1).await;
    tx_batch(
        &tx,
        "markers",
        &marker_sql(&[
            ("target-bootstrap/1", &files[0].2, files[0].0),
            ("target-fence-receipts/1", &files[1].2, files[1].0),
            (
                "target-fence-receipts/3",
                &files[2].2,
                files[2].0,
            ),
            ("physical-mapping/v1", &files[3].2, files[3].0),
        ]),
    )
    .await;
    tx.commit().await.unwrap_or_else(|_| stop("commit-schema"));
    let tx = begin(client).await;
    tx_batch(
        &tx,
        "row-lock-import",
        "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE",
    )
    .await;
    tx_batch(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    tx_batch(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    tx_batch(&tx, "0005", &files[4].1).await;
    tx_batch(
        &tx,
        "marker-import",
        &marker_sql(&[("full-import/v1", &files[4].2, files[4].0)]),
    )
    .await;
    tx.commit().await.unwrap_or_else(|_| stop("commit-import"));
}

struct Slot {
    table: String,
}

async fn prepare_slot(client: &mut Client) -> Slot {
    let target: i64 = query_i64(
        client,
        "adopt",
        "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
        &[&SHARD, &OWNER],
    )
    .await;
    exec_params(
        client,
        "bind",
        "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
        &[&SHARD, &OWNER, &target],
    )
    .await;
    register(client, "namespace", "n", NAMESPACE, "", "").await;
    register(client, "table", "t", NAMESPACE, TABLE, "").await;
    register(client, "column", "c", NAMESPACE, TABLE, COLUMN).await;
    let table = lookup(client, "table", NAMESPACE, TABLE, "").await;
    let column = lookup(client, "column", NAMESPACE, TABLE, COLUMN).await;
    let oid = type_oid(client, "int8").await;
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![table.clone()];
    let run_id: String = query_text(
        client,
        "open-run",
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
    .await;
    let idents = vec![COLUMN.to_string()];
    let oids = vec![oid];
    let typmods = vec![-1_i32];
    let required = vec![true];
    let index = 0_i32;
    let prepared = query_text(
        client,
        "prepare",
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
    .await;
    if prepared != table {
        stop("prepare-name");
    }
    let sql = format!(
        "INSERT INTO \"_oo\".{table} (\"_oo_pos\", {column}) VALUES (($1::text)::numeric, $2)",
        table = quote_physical(&table, "pt_"),
        column = quote_physical(&column, "pc_"),
    );
    let pos = "1".to_string();
    let value = 4_i64;
    exec_params(client, "insert", &sql, &[&pos, &value]).await;
    Slot { table }
}

async fn register(
    client: &Client,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let digest: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &[&SOURCE, &CONFIG, &GENERATION, &letter, &namespace, &table, &column],
        )
        .await
        .unwrap_or_else(|err| sqlstate("digest", &err))
        .get(0);
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &[
                &SOURCE, &CONFIG, &GENERATION, &level, &namespace, &table, &column, &digest,
            ],
        )
        .await
        .unwrap_or_else(|err| sqlstate("register", &err));
}

async fn lookup(
    client: &Client,
    level: &str,
    namespace: &str,
    table: &str,
    column: &str,
) -> String {
    client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &[&SOURCE, &CONFIG, &GENERATION, &level, &namespace, &table, &column],
        )
        .await
        .unwrap_or_else(|err| sqlstate("lookup", &err))
        .get(0)
}

async fn type_oid(client: &Client, name: &str) -> i64 {
    let oid: i32 = client
        .query_one(
            "SELECT type_row.oid::int4 \
             FROM pg_catalog.pg_type AS type_row \
             JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid = type_row.typnamespace \
             WHERE namespace.nspname = 'pg_catalog' AND type_row.typname = $1 AND type_row.typtype = 'b'",
            &[&name],
        )
        .await
        .unwrap_or_else(|err| sqlstate("type-oid", &err))
        .get(0);
    i64::from(oid)
}

async fn query_i64(client: &Client, name: &str, sql: &str, params: &[&(dyn ToSql + Sync)]) -> i64 {
    client
        .query_one(sql, params)
        .await
        .unwrap_or_else(|err| sqlstate(name, &err))
        .get(0)
}

async fn query_text(
    client: &Client,
    name: &str,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> String {
    client
        .query_one(sql, params)
        .await
        .unwrap_or_else(|err| sqlstate(name, &err))
        .get(0)
}

async fn exec_params(client: &Client, name: &str, sql: &str, params: &[&(dyn ToSql + Sync)]) {
    client
        .execute(sql, params)
        .await
        .unwrap_or_else(|err| sqlstate(name, &err));
}

async fn connect(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::from("none");
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(err) => {
                let text = err.to_string();
                if text.contains(password) {
                    last = "redacted".to_string();
                } else {
                    last = text;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    eprintln!("probe stage=connect detail={last}");
    std::process::exit(1);
}

async fn begin(client: &mut Client) -> Transaction<'_> {
    let tx = client
        .transaction()
        .await
        .unwrap_or_else(|err| sqlstate("begin", &err));
    tx_batch(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    tx_batch(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    tx_batch(
        &tx,
        "advisory-lock",
        &format!("SELECT pg_catalog.pg_advisory_xact_lock({ADVISORY_LOCK})"),
    )
    .await;
    tx
}

async fn batch(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| sqlstate(name, &err));
}

async fn tx_batch(tx: &Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| sqlstate(name, &err));
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        if checksum.len() != 64 {
            stop("checksum");
        }
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!(
            "  ('{version}', '{checksum}', 'expand', '{filename}')"
        ));
    }
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}")
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    if name.len() != 63 || rest.len() != 60 {
        stop("physical");
    }
    if !rest
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        stop("physical-hex");
    }
    format!("\"{name}\"")
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read_secret(key: &str) -> String {
    let bytes = fs::read(env_path(key)).unwrap_or_else(|_| stop("secret"));
    let trimmed = if bytes.ends_with(b"\n") {
        &bytes[..bytes.len() - 1]
    } else {
        &bytes
    };
    let text = std::str::from_utf8(trimmed).unwrap_or_else(|_| stop("secret-utf8"));
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        stop("secret-shape");
    }
    text.to_string()
}

fn env_path(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| stop("env"))
}

fn out_dir() -> PathBuf {
    PathBuf::from(env_path("OO_A286_OUT"))
}

fn trim_nl(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn sqlstate(name: &str, err: &tokio_postgres::Error) -> ! {
    let state = err
        .code()
        .map(|code| code.code().to_string())
        .unwrap_or_default();
    eprintln!("{name} sqlstate={state}");
    std::process::exit(1);
}

fn fail_detail(stage: &str, detail: &str, login: &str, admin: &str) -> ! {
    if detail.contains(login) || detail.contains(admin) {
        stop(stage);
    }
    eprintln!("probe stage={stage} detail={detail}");
    std::process::exit(1);
}

fn stop(stage: &str) -> ! {
    eprintln!("probe stage={stage}");
    std::process::exit(1);
}
