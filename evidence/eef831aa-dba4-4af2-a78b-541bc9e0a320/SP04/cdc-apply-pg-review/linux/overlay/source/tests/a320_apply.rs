//! Independent checks for the fenced CDC writer.
//! Expected row text, the lineage intersection, and the position decimal are
//! written in this file. The author test helpers are not used.

use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cdc_apply_pg::{
    apply, binding_from, config_preimage, digest_text, plan_tables, schema_preimage, type_preimage,
    ColumnDecl, Endpoint, KeyDecl, PgType, Request, Segment, APPLY_LIMIT, CONTRACT,
};
use cdc_spool::{
    encode_position, parse_commit_lsn, recover, Limits, OutcomeKind, Recovery, Spool,
};
use oo_pipeline::physical_mapping::{
    plan_physical_mapping, quote_physical_name, Level, MappingRequest, REGISTER_SQL,
};
use oo_pipeline::target_installer::install;
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::Connection;
use sqlx::PgConnection;
use sqlx::Row;

const LINEAGE: &str = "a320a320a320a320a320a320a320a320a320a320a320a320a320a320a320a320";
const LSN: &str = "1/20";
const POSITION: &str = "18446744078004518944";
const TS: &str = "2026-10-07T03:04:05.123456Z";
const SOURCE: &str = "reviewa";
const SHARD: &str = "review";
const TABLE_GENERATION: &str = "g320";
const OWNER_A: &str = "32000000-0000-4000-8000-000000000320";
const OWNER_B: &str = "32000000-0000-4000-8000-000000000321";
const ROLE: &str = "oo_a320_runtime";
const DB: &str = "oo_a320";
const BARE: &str = "oo_a320_bare";
const DRIFT: &str = "oo_a320_drift";
const RECEIPT_LINEAGE: &str = "^[a-z][a-z0-9_.:-]{0,127}$";
const INT8_MAX: &str = "9223372036854775807";
const INT8_MIN: &str = "-9223372036854775808";
const INT4_MIN: &str = "-2147483648";
const INT2_MIN: &str = "-32768";

struct Env {
    host: String,
    port: u16,
    admin_user: String,
    admin_pass: String,
    runtime_pass: String,
    runtime_pass_file: PathBuf,
    ca_file: PathBuf,
    ca: Vec<u8>,
}

struct Layout {
    orders_ns: String,
    orders_rel: String,
    lines_ns: String,
    lines_rel: String,
    orders: Vec<String>,
    lines: Vec<String>,
}

struct Prepared {
    env: Env,
    db: PgConnection,
    layout: Layout,
    shape: Request,
    config_hash: String,
    schema_hash: String,
    type_hash: String,
}

struct Sealed {
    spool: PathBuf,
    request: PathBuf,
}

fn hand_position_matches() {
    let lsn = (1u128 << 32) + 0x20;
    let pos = (1u128 << 64) + lsn;
    assert_eq!(pos.to_string(), POSITION);
    let parsed = parse_commit_lsn(LSN).expect("lsn");
    assert_eq!(u128::from(parsed), lsn);
    assert_eq!(encode_position(1, parsed).expect("position"), POSITION);
}

fn receipt_lineage(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(*byte, b'_' | b'.' | b':' | b'-')
    })
}

fn hex64(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 64 && bytes.iter().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn intersection(text: &str) -> bool {
    hex64(text) && receipt_lineage(text)
}

fn load_env() -> Env {
    let host = std::env::var("OO_A320_HOST").expect("OO_A320_HOST");
    let port = std::env::var("OO_A320_PORT").expect("OO_A320_PORT").parse().expect("port");
    let admin_user = std::env::var("OO_A320_ADMIN_USER").expect("OO_A320_ADMIN_USER");
    let admin_file = std::env::var("OO_A320_ADMIN_PASS_FILE").expect("admin file");
    let runtime_pass_file = PathBuf::from(std::env::var("OO_A320_RUNTIME_PASS_FILE").expect("runtime file"));
    let ca_file = PathBuf::from(std::env::var("OO_A320_CA").expect("ca"));
    let admin_pass = password(&admin_file);
    let runtime_pass = password(&runtime_pass_file.to_string_lossy());
    let ca = fs::read(&ca_file).expect("ca bytes");
    ident(&admin_user);
    ident(ROLE);
    Env {
        host,
        port,
        admin_user,
        admin_pass,
        runtime_pass,
        runtime_pass_file,
        ca_file,
        ca,
    }
}

fn password(path: &str) -> String {
    let text = fs::read_to_string(path).expect("password").trim().to_owned();
    assert!(text.chars().all(|ch| ch.is_ascii_hexdigit()) && !text.is_empty());
    text
}

fn ident(name: &str) {
    assert!(!name.is_empty() && name.len() <= 63);
    assert!(name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'));
}

fn work_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("oo-a320-{name}-{}", std::process::id()));
    fs::create_dir_all(&path).expect("workdir");
    path
}

fn missing_secret(dir: &Path) -> PathBuf {
    let path = dir.join("password-absent");
    assert!(!path.exists());
    path
}

fn base_shape(spool: PathBuf) -> Request {
    Request {
        contract: CONTRACT.to_owned(),
        spool_dir: spool,
        endpoint: Endpoint {
            host: "pending".to_owned(),
            port: 1,
            database: DB.to_owned(),
            user: ROLE.to_owned(),
            password_file: PathBuf::from("/missing"),
            ca_file: PathBuf::from("/missing"),
            server_name: "pending".to_owned(),
        },
        shard: SHARD.to_owned(),
        owner: OWNER_A.to_owned(),
        generation: 1,
        table_generation: TABLE_GENERATION.to_owned(),
        run_generation: 1,
        lineage: LINEAGE.to_owned(),
        source: SOURCE.to_owned(),
        epoch: 1,
        control_lease_valid: false,
        segment: None,
        keys: vec![
            key("public.lines", "id"),
            key("public.orders", "id"),
        ],
        columns: vec![
            column("public.lines", 0, "amount", "int8", true),
            column("public.lines", 1, "n2", "int2", true),
            column("public.lines", 2, "n4", "int4", true),
            column("public.orders", 0, "qty", "int8", true),
            column("public.orders", 1, "note", "text", true),
            column("public.orders", 2, "active", "bool", false),
            column("public.orders", 3, "n2", "int2", true),
            column("public.orders", 4, "n4", "int4", true),
        ],
    }
}

fn key(table: &str, name: &str) -> KeyDecl {
    KeyDecl {
        table: table.to_owned(),
        ordinal: 0,
        name: name.to_owned(),
        pg_type: "int8".to_owned(),
    }
}

fn column(table: &str, ordinal: u16, name: &str, pg_type: &str, nullable: bool) -> ColumnDecl {
    ColumnDecl {
        table: table.to_owned(),
        ordinal,
        name: name.to_owned(),
        pg_type: pg_type.to_owned(),
        nullable,
    }
}

fn hashes(request: &Request, selected: &[String]) -> (String, String, String, Vec<cdc_apply_pg::TablePlan>) {
    let tables = plan_tables(request, selected).expect("plan");
    let binding = binding_from(request, &tables);
    (
        digest_text(&config_preimage(&binding)),
        digest_text(&schema_preimage(&tables)),
        digest_text(&type_preimage(&tables)),
        tables,
    )
}

#[allow(clippy::too_many_arguments)]
fn data_line(
    txn: &str,
    global: u64,
    collection: u64,
    table: &str,
    key_value: &str,
    op: &str,
    payload: &str,
    unavailable: &[u16],
) -> String {
    let missing = unavailable.iter().map(|ordinal| ordinal.to_string()).collect::<Vec<_>>().join(",");
    format!(
        r#"{{"kind":"data","txn_id":"{txn}","global_txn_sequence":{global},"data_collection_sequence":{collection},"table":"{table}","key":[{{"type":"int","ordinal":0,"value":"{key_value}"}}],"op":"{op}","payload":{payload},"unavailable_ordinals":[{missing}]}}"#
    )
}

#[allow(clippy::too_many_arguments)]
fn seal(
    name: &str,
    txn: &str,
    tables: &[&str],
    rows: &[String],
    counts: &[(&str, u64)],
    schema_hash: &str,
    config_hash: &str,
    type_hash: &str,
) -> PathBuf {
    hand_position_matches();
    let total: u64 = counts.iter().map(|(_, count)| *count).sum();
    assert_eq!(total, u64::try_from(rows.len()).expect("rows"));
    let root = work_dir(name);
    let dir = root.join("spool");
    let _ = fs::remove_dir_all(&dir);
    let mut names = tables.to_vec();
    names.sort_unstable();
    let listed = names.iter().map(|table| format!("\"{table}\"")).collect::<Vec<_>>().join(",");
    let mut pairs = counts.to_vec();
    pairs.sort_by(|left, right| left.0.cmp(right.0));
    let body = pairs.iter().map(|(table, count)| format!("\"{table}\":{count}")).collect::<Vec<_>>().join(",");
    let mut bytes = Vec::new();
    let bind = format!(
        r#"{{"kind":"bind","source":"{SOURCE}","lineage":"{LINEAGE}","epoch":1,"selected_tables":[{listed}],"schema_hash":"{schema_hash}","config_hash":"{config_hash}","type_hash":"{type_hash}","hook_hashes":[],"commit_position_provenance":"caller_declared"}}"#
    );
    push_line(&mut bytes, &bind);
    push_line(&mut bytes, &format!(r#"{{"kind":"begin","txn_id":"{txn}"}}"#));
    for row in rows {
        push_line(&mut bytes, row);
    }
    push_line(
        &mut bytes,
        &format!(
            r#"{{"kind":"end","txn_id":"{txn}","count":{total},"per_collection_counts":{{{body}}},"commit_lsn":"{LSN}","source_commit_timestamp":"{TS}"}}"#
        ),
    );
    let mut spool = Spool::create_new(&dir, Limits::product()).expect("create");
    let outcome = spool.ingest_reader(std::io::Cursor::new(bytes)).expect("ingest");
    assert!(matches!(outcome.kind, OutcomeKind::Sealed(_)));
    let Recovery::Committed(committed) = recover(&dir).expect("recover") else {
        panic!("spool did not recover committed");
    };
    assert_eq!(committed.manifest.commit_position, POSITION);
    assert_eq!(committed.manifest.commit_position_provenance, "caller_declared");
    assert!(!committed.manifest.ack_allowed);
    assert!(!committed.manifest.target_applied);
    assert!(!committed.manifest.cross_table_consistent);
    assert!(!committed.manifest.upstream_commit_lsn_verified);
    assert!(!committed.manifest.debezium_normalizer);
    assert!(!committed.manifest.p2_verified);
    assert!(!committed.manifest.p11_verified);
    assert!(committed.manifest.applied.is_null());
    dir
}

fn push_line(bytes: &mut Vec<u8>, line: &str) {
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
}

fn write_request(env: &Env, shape: &Request, spool: &Path, database: &str, lease: bool) -> PathBuf {
    let mut request = shape.clone();
    request.spool_dir = spool.to_owned();
    request.control_lease_valid = lease;
    request.endpoint = Endpoint {
        host: env.host.clone(),
        port: env.port,
        database: database.to_owned(),
        user: ROLE.to_owned(),
        password_file: env.runtime_pass_file.clone(),
        ca_file: env.ca_file.clone(),
        server_name: env.host.clone(),
    };
    let path = spool.parent().expect("parent").join("request.json");
    fs::write(&path, serde_json::to_vec(&request).expect("json")).expect("request");
    path
}

#[allow(clippy::too_many_arguments)]
fn seal_request(
    env: &Env,
    name: &str,
    txn: &str,
    shape: &Request,
    selected: &[&str],
    rows: &[String],
    counts: &[(&str, u64)],
    database: &str,
    lease: bool,
) -> Sealed {
    let owned: Vec<String> = selected.iter().map(|table| (*table).to_owned()).collect();
    let (config_hash, schema_hash, type_hash, _) = hashes(shape, &owned);
    let spool = seal(name, txn, selected, rows, counts, &schema_hash, &config_hash, &type_hash);
    let request = write_request(env, shape, &spool, database, lease);
    Sealed { spool, request }
}

fn run_cli(request: &Path, extra: &[(&str, &str)]) -> (i32, Value) {
    let bin = env!("CARGO_BIN_EXE_cdc-apply-pg");
    let mut command = Command::new(bin);
    command.arg("apply").arg(request);
    for (key_name, value) in extra {
        command.env(key_name, value);
    }
    let output = command.output().expect("run");
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(127);
    let value = serde_json::from_str(stdout.trim()).unwrap_or_else(|err| {
        panic!("json {err} exit={code} stdout={stdout} stderr={stderr}")
    });
    (code, value)
}

fn closed(value: &Value) {
    assert_eq!(value["contract"], CONTRACT);
    assert_eq!(value["ack_allowed"], false);
    assert_eq!(value["source_cross_table_consistent"], false);
    assert_eq!(value["upstream_commit_lsn_verified"], false);
    assert_eq!(value["debezium_normalizer"], false);
    assert_eq!(value["p2_verified"], false);
    assert_eq!(value["p11_verified"], false);
    assert_eq!(value["control_lease_consulted"], false);
    assert_eq!(value["commit_position_provenance"], "caller_declared");
}

fn orders_payload(qty: &str, note: &str, active: bool, n2: &str, n4: &str, unavailable_note: bool) -> String {
    let note_cell = if unavailable_note {
        r#"{"type":"unavailable","ordinal":1}"#.to_owned()
    } else if note == "null" {
        r#"{"type":"null","ordinal":1}"#.to_owned()
    } else {
        format!(r#"{{"type":"text","ordinal":1,"value":"{note}"}}"#)
    };
    let active_text = if active { "true" } else { "false" };
    let qty_cell = if qty == "null" {
        r#"{"type":"null","ordinal":0}"#.to_owned()
    } else if qty == "unavailable" {
        r#"{"type":"unavailable","ordinal":0}"#.to_owned()
    } else {
        format!(r#"{{"type":"int","ordinal":0,"value":"{qty}"}}"#)
    };
    let n2_cell = if n2 == "null" {
        r#"{"type":"null","ordinal":3}"#.to_owned()
    } else {
        format!(r#"{{"type":"int","ordinal":3,"value":"{n2}"}}"#)
    };
    let n4_cell = if n4 == "null" {
        r#"{"type":"null","ordinal":4}"#.to_owned()
    } else {
        format!(r#"{{"type":"int","ordinal":4,"value":"{n4}"}}"#)
    };
    format!("[{qty_cell},{note_cell},{{\"type\":\"bool\",\"ordinal\":2,\"value\":{active_text}}},{n2_cell},{n4_cell}]")
}

fn lines_payload(amount: &str, n2: &str, n4: &str) -> String {
    format!(
        r#"[{{"type":"int","ordinal":0,"value":"{amount}"}},{{"type":"int","ordinal":1,"value":"{n2}"}},{{"type":"int","ordinal":2,"value":"{n4}"}}]"#
    )
}

async fn dial(env: &Env, database: &str) -> PgConnection {
    let options = PgConnectOptions::new()
        .host(&env.host)
        .port(env.port)
        .username(&env.admin_user)
        .password(&env.admin_pass)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(env.ca.clone());
    PgConnection::connect_with(&options).await.expect("admin dial")
}

async fn dial_runtime(env: &Env, database: &str) -> PgConnection {
    let options = PgConnectOptions::new()
        .host(&env.host)
        .port(env.port)
        .username(ROLE)
        .password(&env.runtime_pass)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(env.ca.clone());
    PgConnection::connect_with(&options).await.expect("runtime dial")
}

async fn raw(conn: &mut PgConnection, sql: &str, label: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("{label} failed: {err}"));
}

async fn generation(conn: &mut PgConnection) -> (i64, i64) {
    let count: String = sqlx::query_scalar("SELECT count(*)::text FROM _oo.target_generations")
        .fetch_one(&mut *conn)
        .await
        .expect("count");
    let value: String = sqlx::query_scalar("SELECT COALESCE(max(generation), 0)::text FROM _oo.target_generations")
        .fetch_one(&mut *conn)
        .await
        .expect("generation");
    (count.parse().expect("count"), value.parse().expect("generation"))
}

async fn reset_cluster(env: &Env) {
    let mut admin = dial(env, "postgres").await;
    for database in [DB, BARE, DRIFT, "oo_a301", "oo_a301_bare", "oo_a301_drift"] {
        raw(&mut admin, &format!("DROP DATABASE IF EXISTS {database} WITH (FORCE)"), "drop db").await;
    }
    for role in [ROLE, "oo_a301_runtime", "oo_pipeline_runtime", "oo_target_owner"] {
        raw(&mut admin, &format!("DROP ROLE IF EXISTS {role}"), "drop role").await;
    }
    raw(
        &mut admin,
        &format!(
            "CREATE ROLE {ROLE} LOGIN PASSWORD '{pass}' NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT NOBYPASSRLS NOREPLICATION",
            pass = env.runtime_pass
        ),
        "create role",
    )
    .await;
    raw(&mut admin, &format!("CREATE DATABASE {DB}"), "create db").await;
    raw(&mut admin, &format!("CREATE DATABASE {BARE}"), "create bare").await;
}

fn selected() -> Vec<String> {
    vec!["public.lines".to_owned(), "public.orders".to_owned()]
}

fn physical(config: &str, level: Level, namespace: &str, table: &str, column: &str) -> (String, Vec<u8>) {
    let plan = plan_physical_mapping(&MappingRequest {
        source: SOURCE.to_owned(),
        config_digest: config.to_owned(),
        run_generation: 1,
        level,
        logical_namespace: namespace.to_owned(),
        logical_table: table.to_owned(),
        logical_column: column.to_owned(),
    })
    .expect("mapping plan");
    let quoted = quote_physical_name(&plan.physical_name).expect("quote");
    (quoted, plan.identity_digest.to_vec())
}

async fn register(
    conn: &mut PgConnection,
    config: &str,
    level: Level,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let (_quoted, digest) = physical(config, level, namespace, table, column);
    sqlx::query(REGISTER_SQL)
        .bind(SOURCE)
        .bind(config)
        .bind(1i64)
        .bind(level.as_str())
        .bind(namespace)
        .bind(table)
        .bind(column)
        .bind(digest)
        .execute(&mut *conn)
        .await
        .expect("register");
}

async fn create_tables(conn: &mut PgConnection, config: &str, tables: &[cdc_apply_pg::TablePlan]) -> Layout {
    let mut schemas = std::collections::BTreeSet::new();
    let mut layout = Layout {
        orders_ns: String::new(),
        orders_rel: String::new(),
        lines_ns: String::new(),
        lines_rel: String::new(),
        orders: Vec::new(),
        lines: Vec::new(),
    };
    for table in tables {
        let ns = physical(config, Level::Namespace, &table.namespace, "", "").0;
        let relation = physical(config, Level::Table, &table.namespace, &table.table, "").0;
        if schemas.insert(ns.clone()) {
            raw(conn, &format!("CREATE SCHEMA {ns}"), "schema").await;
        }
        let mut defs = Vec::new();
        let mut quoted_keys = Vec::new();
        let mut quoted_columns = Vec::new();
        for field in &table.keys {
            let name = physical(config, Level::Column, &table.namespace, &table.table, &field.name).0;
            defs.push(format!("{name} {} NOT NULL", sql_type(field.pg_type)));
            quoted_keys.push(name.clone());
            quoted_columns.push(name);
        }
        for field in &table.columns {
            let name = physical(config, Level::Column, &table.namespace, &table.table, &field.name).0;
            let nulls = if field.nullable { "NULL" } else { "NOT NULL" };
            defs.push(format!("{name} {} {nulls}", sql_type(field.pg_type)));
            quoted_columns.push(name);
        }
        let keys = quoted_keys.join(", ");
        defs.push(format!("PRIMARY KEY ({keys})"));
        raw(conn, &format!("CREATE TABLE {ns}.{relation} ({})", defs.join(", ")), "table").await;
        if table.logical == "public.orders" {
            layout.orders_ns = ns;
            layout.orders_rel = relation;
            layout.orders = quoted_columns;
        } else if table.logical == "public.lines" {
            layout.lines_ns = ns;
            layout.lines_rel = relation;
            layout.lines = quoted_columns;
        }
    }
    assert_eq!(layout.orders.len(), 6);
    assert_eq!(layout.lines.len(), 4);
    layout
}

fn sql_type(pg_type: PgType) -> &'static str {
    match pg_type {
        PgType::Bool => "bool",
        PgType::Int2 => "int2",
        PgType::Int4 => "int4",
        PgType::Int8 => "int8",
        PgType::Text => "text",
    }
}

async fn seed(conn: &mut PgConnection, layout: &Layout) {
    let o = &layout.orders;
    let ns = &layout.orders_ns;
    let rel = &layout.orders_rel;
    let sql = format!(
        "INSERT INTO {ns}.{rel} ({}, {}, {}, {}, {}, {}) VALUES ($1::int8, $2::int8, $3::text, $4::bool, $5::int2, $6::int4)",
        o[0], o[1], o[2], o[3], o[4], o[5]
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(320i64)
        .bind(11i64)
        .bind("kept320")
        .bind(false)
        .bind(7i16)
        .bind(9i32)
        .execute(&mut *conn)
        .await
        .expect("seed orders");
    let l = &layout.lines;
    let ns = &layout.lines_ns;
    let rel = &layout.lines_rel;
    let sql = format!(
        "INSERT INTO {ns}.{rel} ({}, {}, {}, {}) VALUES ($1::int8, $2::int8, $3::int2, $4::int4)",
        l[0], l[1], l[2], l[3]
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(70i64)
        .bind(5i64)
        .bind(3i16)
        .bind(4i32)
        .execute(&mut *conn)
        .await
        .expect("seed lines");
}

async fn persistence(conn: &mut PgConnection, namespace: &str, table: &str) -> String {
    let sql = "SELECT c.relpersistence::text FROM pg_catalog.pg_class AS c \
               INNER JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
               WHERE n.nspname = $1 AND c.relname = $2";
    sqlx::query_scalar(sql)
        .bind(namespace.trim_matches('"'))
        .bind(table.trim_matches('"'))
        .fetch_one(conn)
        .await
        .expect("persistence")
}

async fn order_rows(conn: &mut PgConnection, layout: &Layout) -> Vec<Vec<Option<String>>> {
    let o = &layout.orders;
    let sql = format!(
        "SELECT {}::text, {}::text, {}::text, {}::text, {}::text, {}::text FROM {}.{} ORDER BY {}::int8",
        o[0], o[1], o[2], o[3], o[4], o[5], layout.orders_ns, layout.orders_rel, o[0]
    );
    fetch_grid(conn, &sql, 6).await
}

async fn line_rows(conn: &mut PgConnection, layout: &Layout) -> Vec<Vec<Option<String>>> {
    let l = &layout.lines;
    let sql = format!(
        "SELECT {}::text, {}::text, {}::text, {}::text FROM {}.{} ORDER BY {}::int8",
        l[0], l[1], l[2], l[3], layout.lines_ns, layout.lines_rel, l[0]
    );
    fetch_grid(conn, &sql, 4).await
}

async fn fetch_grid(conn: &mut PgConnection, sql: &str, width: usize) -> Vec<Vec<Option<String>>> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_all(&mut *conn)
        .await
        .expect("grid");
    rows.into_iter()
        .map(|row| (0..width).map(|index| row.try_get(index).expect("cell")).collect())
        .collect()
}

struct Receipt {
    fence: String,
    commit_at: String,
    cross: String,
    partial: String,
    complete: String,
    segments_null: bool,
    bitmap_len: i32,
    payload: String,
    applied_pos: String,
    lineage: String,
    codec: String,
}

async fn receipt(conn: &mut PgConnection, txn: &str) -> Option<Receipt> {
    let sql = "SELECT fence_generation::text, target_commit_at::text, cross_table_consistent::text, \
               partial_txn::text, txn_complete::text, (segment_index IS NULL AND segment_count IS NULL), \
               octet_length(segment_bitmap)::int4, payload_digest, applied_pos::text, position_lineage, \
               position_codec \
               FROM _oo.apply_receipts \
               WHERE source = $1 AND table_generation = $2 AND txn_or_batch_id = $3";
    let row = sqlx::query(sql)
        .bind(SOURCE)
        .bind(TABLE_GENERATION)
        .bind(txn)
        .fetch_optional(&mut *conn)
        .await
        .expect("receipt");
    row.map(|row| Receipt {
        fence: row.try_get(0).expect("fence"),
        commit_at: row.try_get(1).expect("at"),
        cross: row.try_get(2).expect("cross"),
        partial: row.try_get(3).expect("partial"),
        complete: row.try_get(4).expect("complete"),
        segments_null: row.try_get(5).expect("segments"),
        bitmap_len: row.try_get(6).expect("bitmap"),
        payload: row.try_get(7).expect("payload"),
        applied_pos: row.try_get(8).expect("pos"),
        lineage: row.try_get(9).expect("lineage"),
        codec: row.try_get(10).expect("codec"),
    })
}

fn payload_digest(spool: &Path) -> String {
    let Recovery::Committed(committed) = recover(spool).expect("recover") else {
        panic!("payload spool is not committed");
    };
    let mut hashes = Vec::new();
    let iter = committed.iter().expect("iter");
    for item in iter {
        let event = item.expect("event");
        hashes.push(event.event_hash);
    }
    digest_text(&cdc_apply_pg::payload_preimage(&hashes))
}

async fn types_of(conn: &mut PgConnection, namespace: &str, table: &str) -> Vec<String> {
    let sql = "SELECT t.typname FROM pg_catalog.pg_attribute AS a \
               INNER JOIN pg_catalog.pg_class AS c ON c.oid = a.attrelid \
               INNER JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
               INNER JOIN pg_catalog.pg_type AS t ON t.oid = a.atttypid \
               WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
               ORDER BY a.attnum";
    let rows = sqlx::query(sql)
        .bind(namespace.trim_matches('"'))
        .bind(table.trim_matches('"'))
        .fetch_all(&mut *conn)
        .await
        .expect("types");
    rows.into_iter().map(|row| row.try_get(0).expect("typname")).collect()
}

async fn runtime_pids(conn: &mut PgConnection) -> Vec<String> {
    let sql = "SELECT pid::text FROM pg_stat_activity WHERE usename = $1 AND datname = $2 ORDER BY pid";
    let rows = sqlx::query(sql)
        .bind(ROLE)
        .bind(DB)
        .fetch_all(&mut *conn)
        .await
        .expect("backends");
    rows.into_iter().map(|row| row.try_get(0).expect("pid")).collect()
}

async fn advisory_pids(conn: &mut PgConnection) -> Vec<String> {
    let sql = "SELECT a.pid::text FROM pg_catalog.pg_locks AS l \
               INNER JOIN pg_catalog.pg_stat_activity AS a ON a.pid = l.pid \
               WHERE l.locktype = 'advisory' AND l.granted AND a.usename = $1 AND a.datname = $2 \
               ORDER BY a.pid";
    let rows = sqlx::query(sql)
        .bind(ROLE)
        .bind(DB)
        .fetch_all(&mut *conn)
        .await
        .expect("locks");
    rows.into_iter().map(|row| row.try_get(0).expect("pid")).collect()
}

fn snapshot_orders() -> Vec<Vec<Option<String>>> {
    vec![row6("320", Some("11"), Some("kept320"), Some("false"), Some("7"), Some("9"))]
}

fn happy_orders() -> Vec<Vec<Option<String>>> {
    vec![
        row6("320", Some(INT8_MAX), Some("kept320"), Some("true"), Some(INT2_MIN), Some(INT4_MIN)),
        row6("321", None, Some("row321"), Some("false"), None, Some("0")),
    ]
}

fn row6(
    id: &str,
    qty: Option<&str>,
    note: Option<&str>,
    active: Option<&str>,
    n2: Option<&str>,
    n4: Option<&str>,
) -> Vec<Option<String>> {
    vec![
        Some(id.to_owned()),
        qty.map(str::to_owned),
        note.map(str::to_owned),
        active.map(str::to_owned),
        n2.map(str::to_owned),
        n4.map(str::to_owned),
    ]
}

fn assert_same_connection(pids: &[String], locks: &[String]) {
    assert_eq!(pids.len(), 1, "writer backends {pids:?}");
    assert_eq!(locks, pids, "advisory lock is not held by that backend");
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn a320_upfront_refuses_before_credentials() {
    hand_position_matches();
    assert_eq!(APPLY_LIMIT, 200_000);
    assert_eq!(LINEAGE.len(), 64);
    assert!(intersection(LINEAGE));
    let digit = format!("0{}", "a".repeat(63));
    let non_hex = format!("g{}", "a".repeat(63));
    let receipt_only = "billing".to_owned();
    let leading_f = format!("f{}", "0".repeat(63));
    assert!(hex64(&digit) && !receipt_lineage(&digit) && !intersection(&digit));
    assert!(!hex64(&non_hex) && receipt_lineage(&non_hex) && !intersection(&non_hex));
    assert!(!hex64(&receipt_only) && receipt_lineage(&receipt_only) && !intersection(&receipt_only));
    assert!(intersection(&leading_f));
    for source in [
        include_str!("../src/apply.rs"),
        include_str!("../src/main.rs"),
        include_str!("../src/model.rs"),
        include_str!("../src/tls.rs"),
    ] {
        assert!(!source.contains("XACK"));
        assert!(!source.contains("pg_advisory_lock"));
        assert!(!source.contains("GRANT "));
        assert!(!source.contains("TRIM "));
    }
    assert!(include_str!("../src/model.rs").contains(RECEIPT_LINEAGE));

    let dir = work_dir("upfront");
    let absent = missing_secret(&dir);
    let mut refused = base_shape(dir.join("missing-spool"));
    refused.endpoint.password_file = absent.clone();
    refused.lineage = digit;
    let output = apply(&refused).await;
    assert_eq!(output.fault.as_deref(), Some("identity"));
    assert!(!output.target_applied);
    assert!(!absent.exists());

    refused.lineage = non_hex;
    let output = apply(&refused).await;
    assert_eq!(output.fault.as_deref(), Some("identity"));
    refused.lineage = receipt_only;
    let output = apply(&refused).await;
    assert_eq!(output.fault.as_deref(), Some("identity"));
    refused.lineage = leading_f;
    let output = apply(&refused).await;
    assert_eq!(output.fault.as_deref(), Some("spool_io"));
    assert!(!absent.exists());

    refused.lineage = LINEAGE.to_owned();
    refused.segment = Some(Segment { index: 0, count: 2 });
    let output = apply(&refused).await;
    assert_eq!(output.fault.as_deref(), Some("needs_segments"));
    assert!(!output.target_applied);
    assert!(!output.ack_allowed);
    assert_eq!(output.commit_position_provenance, "caller_declared");
    assert!(!absent.exists());

    let shape = base_shape(PathBuf::new());
    let selected = selected();
    let (config_hash, schema_hash, type_hash, _) = hashes(&shape, &selected);
    let row = data_line("wide0", 1, 1, "public.orders", "320", "upsert", &orders_payload("1", "z", false, "1", "1", false), &[]);
    let spool = seal("unsupported", "wide0", &["public.lines", "public.orders"], &[row], &[("public.orders", 1)], &schema_hash, &config_hash, &type_hash);
    let mut typed = base_shape(spool);
    typed.endpoint.password_file = absent.clone();
    typed.columns[3].pg_type = "numeric".to_owned();
    let output = apply(&typed).await;
    assert_eq!(output.fault.as_deref(), Some("unsupported_type"));
    typed.columns[3].pg_type = "float8".to_owned();
    assert_eq!(apply(&typed).await.fault.as_deref(), Some("unsupported_type"));
    typed.columns[3].pg_type = "jsonb".to_owned();
    assert_eq!(apply(&typed).await.fault.as_deref(), Some("unsupported_type"));
    typed.columns[3].pg_type = "int".to_owned();
    assert_eq!(apply(&typed).await.fault.as_deref(), Some("unsupported_type"));
    assert!(!absent.exists());

    let mismatch_row = data_line(
        "mix1",
        1,
        1,
        "public.orders",
        "320",
        "upsert",
        &orders_payload("1", "null", false, "1", "1", false).replace(
            r#"{"type":"null","ordinal":1}"#,
            r#"{"type":"int","ordinal":1,"value":"1"}"#,
        ),
        &[],
    );
    let mismatch = seal(
        "mismatch",
        "mix1",
        &["public.lines", "public.orders"],
        &[mismatch_row],
        &[("public.orders", 1)],
        &schema_hash,
        &config_hash,
        &type_hash,
    );
    let mut crossed = base_shape(mismatch);
    crossed.endpoint.password_file = absent.clone();
    let output = apply(&crossed).await;
    assert_eq!(output.fault.as_deref(), Some("type_mismatch"));
    assert!(!output.target_applied);
    assert!(!absent.exists());

    let scale_dir = work_dir("scale");
    let feed_path = scale_dir.join("feed.jsonl");
    let scale_spool = scale_dir.join("spool");
    let count = APPLY_LIMIT + 1;
    {
        let mut feed = File::create(&feed_path).expect("feed");
        let bind = format!(
            r#"{{"kind":"bind","source":"{SOURCE}","lineage":"{LINEAGE}","epoch":1,"selected_tables":["public.orders"],"schema_hash":"{LINEAGE}","config_hash":"{LINEAGE}","type_hash":"{LINEAGE}","hook_hashes":[],"commit_position_provenance":"caller_declared"}}"#
        );
        writeln!(feed, "{bind}").unwrap();
        writeln!(feed, r#"{{"kind":"begin","txn_id":"wide320"}}"#).unwrap();
        for seq in 1..=count {
            let line = format!(
                r#"{{"kind":"data","txn_id":"wide320","global_txn_sequence":{seq},"data_collection_sequence":{seq},"table":"public.orders","key":[{{"type":"int","ordinal":0,"value":"{seq}"}}],"op":"upsert","payload":[{{"type":"int","ordinal":0,"value":"1"}}],"unavailable_ordinals":[]}}"#
            );
            writeln!(feed, "{line}").unwrap();
        }
        writeln!(
            feed,
            r#"{{"kind":"end","txn_id":"wide320","count":{count},"per_collection_counts":{{"public.orders":{count}}},"commit_lsn":"{LSN}","source_commit_timestamp":"{TS}"}}"#
        )
        .unwrap();
        feed.sync_all().unwrap();
    }
    let _ = fs::remove_dir_all(&scale_spool);
    let mut spool = Spool::create_new(&scale_spool, Limits::product()).expect("scale create");
    let outcome = spool
        .ingest_reader(BufReader::new(File::open(&feed_path).expect("feed open")))
        .expect("scale ingest");
    assert!(matches!(outcome.kind, OutcomeKind::Sealed(_)));
    let mut wide = base_shape(scale_spool);
    wide.endpoint.password_file = absent.clone();
    wide.keys.clear();
    wide.columns.clear();
    let started = Instant::now();
    let output = apply(&wide).await;
    assert_eq!(output.fault.as_deref(), Some("needs_segments"));
    assert!(output.generation.is_none());
    assert!(!output.target_applied);
    assert!(!absent.exists());
    println!("phase=needs_segments count={count} elapsed_ms={}", started.elapsed().as_millis());
}

#[tokio::test]
async fn a320_fenced_writer_on_private_postgres() {
    let mut live = prepare().await;
    adopted_generation_survives_rollback(&mut live).await;
    happy_replay_and_conflict(&mut live).await;
    unavailable_insert_and_api_delete(&mut live).await;
    fence_and_old_generation(&mut live).await;
    sigkill_windows(&mut live).await;
    hidden_commit_and_unknown(&mut live).await;
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn prepare() -> Prepared {
    let env = load_env();
    reset_cluster(&env).await;
    let mut db = dial(&env, DB).await;
    install(&mut db).await.expect("install");
    raw(&mut db, &format!("GRANT oo_pipeline_runtime TO {ROLE}"), "member").await;
    raw(&mut db, &format!("GRANT CONNECT ON DATABASE {DB} TO {ROLE}"), "connect").await;
    raw(&mut db, &format!("GRANT CONNECT ON DATABASE {BARE} TO {ROLE}"), "connect bare").await;
    let shape = base_shape(PathBuf::from("/unused"));
    let names = selected();
    let (config_hash, schema_hash, type_hash, tables) = hashes(&shape, &names);
    for table in &tables {
        register(&mut db, &config_hash, Level::Namespace, &table.namespace, "", "").await;
        register(&mut db, &config_hash, Level::Table, &table.namespace, &table.table, "").await;
        for field in table.keys.iter().chain(table.columns.iter()) {
            register(&mut db, &config_hash, Level::Column, &table.namespace, &table.table, &field.name).await;
        }
    }
    let layout = create_tables(&mut db, &config_hash, &tables).await;
    for (ns, rel) in [(&layout.orders_ns, &layout.orders_rel), (&layout.lines_ns, &layout.lines_rel)] {
        raw(&mut db, &format!("GRANT USAGE ON SCHEMA {ns} TO {ROLE}"), "usage").await;
        raw(&mut db, &format!("GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE {ns}.{rel} TO {ROLE}"), "dml").await;
    }
    seed(&mut db, &layout).await;
    assert_eq!(persistence(&mut db, &layout.orders_ns, &layout.orders_rel).await, "p");
    assert_eq!(persistence(&mut db, &layout.lines_ns, &layout.lines_rel).await, "p");
    assert_eq!(types_of(&mut db, &layout.orders_ns, &layout.orders_rel).await, vec!["int8", "int8", "text", "bool", "int2", "int4"]);
    assert_eq!(types_of(&mut db, &layout.lines_ns, &layout.lines_rel).await, vec!["int8", "int8", "int2", "int4"]);
    assert_eq!(generation(&mut db).await, (0, 0));
    let versions: String = sqlx::query_scalar("SELECT COALESCE(string_agg(version, ',' ORDER BY version), '') FROM _oo.schema_migrations")
        .fetch_one(&mut db)
        .await
        .expect("versions");
    for required in ["physical-mapping/v1", "target-bootstrap/1", "target-fence-receipts/1", "target-fence-receipts/3"] {
        assert!(versions.contains(required), "{versions}");
    }
    println!("phase=installed versions={versions}");
    let function_def: String = sqlx::query_scalar(
        "SELECT pg_get_functiondef(p.oid) FROM pg_proc AS p \
         INNER JOIN pg_namespace AS n ON n.oid = p.pronamespace \
         WHERE n.nspname = '_oo' AND p.proname = 'commit_apply_receipt'",
    )
    .fetch_one(&mut db)
    .await
    .expect("function");
    let constraint_def: String = sqlx::query_scalar(
        "SELECT pg_get_constraintdef(c.oid) FROM pg_constraint AS c \
         INNER JOIN pg_class AS rel ON rel.oid = c.conrelid \
         INNER JOIN pg_namespace AS n ON n.oid = rel.relnamespace \
         WHERE n.nspname = '_oo' AND c.conname = 'apply_receipts_lineage_check'",
    )
    .fetch_one(&mut db)
    .await
    .expect("constraint");
    assert!(function_def.contains(RECEIPT_LINEAGE));
    assert!(constraint_def.contains(RECEIPT_LINEAGE));
    println!("phase=lineage_regex function=1 constraint=1");

    let bare = seal_request(
        &env,
        "bare",
        "bare320",
        &shape,
        &["public.lines", "public.orders"],
        &[data_line("bare320", 1, 1, "public.orders", "320", "upsert", &orders_payload("1", "z", false, "1", "1", false), &[])],
        &[("public.orders", 1)],
        BARE,
        false,
    );
    let bare_out = run_cli(&bare.request, &[]);
    assert_eq!(bare_out.0, 2, "{:?}", bare_out.1);
    assert_eq!(bare_out.1["fault"], "capability_absent");
    closed(&bare_out.1);
    assert_eq!(generation(&mut db).await, (0, 0));

    drop(db);
    let mut admin = dial(&env, "postgres").await;
    raw(
        &mut admin,
        "DO $$ BEGIN PERFORM pg_catalog.pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity WHERE datname = 'oo_a320' AND pid <> pg_catalog.pg_backend_pid(); END $$",
        "quiet template",
    )
    .await;
    raw(&mut admin, &format!("CREATE DATABASE {DRIFT} TEMPLATE {DB}"), "clone").await;
    drop(admin);
    let mut db = dial(&env, DB).await;
    let mut drift = dial(&env, DRIFT).await;
    raw(&mut drift, &format!("GRANT CONNECT ON DATABASE {DRIFT} TO {ROLE}"), "connect drift").await;
    raw(
        &mut drift,
        "INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES ('target-fence-receipts/2', 'cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd', 'expand', '0002_target_segment_null_expand.sql')",
        "drift insert",
    )
    .await;
    let drifted = seal_request(
        &env,
        "drift",
        "drift320",
        &shape,
        &["public.lines", "public.orders"],
        &[data_line("drift320", 1, 1, "public.orders", "320", "upsert", &orders_payload("1", "z", false, "1", "1", false), &[])],
        &[("public.orders", 1)],
        DRIFT,
        false,
    );
    let drift_out = run_cli(&drifted.request, &[]);
    assert_eq!(drift_out.0, 2, "{:?}", drift_out.1);
    assert_eq!(drift_out.1["fault"], "migration_drift");
    assert_eq!(generation(&mut drift).await, (0, 0));
    assert_eq!(generation(&mut db).await, (0, 0));
    drop(drift);

    let mut flagged = shape.clone();
    flagged.columns.push(column("public.orders", 5, "flag", "bool", true));
    let flagged_names = selected();
    let (flag_config, flag_schema, flag_type, _) = hashes(&flagged, &flagged_names);
    assert_eq!(flag_config, config_hash);
    let missing = seal(
        "missing-map",
        "miss320",
        &["public.lines", "public.orders"],
        &[data_line(
            "miss320",
            1,
            1,
            "public.orders",
            "320",
            "upsert",
            &format!("{},{{\"type\":\"bool\",\"ordinal\":5,\"value\":true}}]", &orders_payload("1", "z", false, "1", "1", false).trim_end_matches(']')),
            &[],
        )],
        &[("public.orders", 1)],
        &flag_schema,
        &flag_config,
        &flag_type,
    );
    let missing_request = write_request(&env, &flagged, &missing, DB, false);
    let missing_out = run_cli(&missing_request, &[]);
    assert_eq!(missing_out.0, 2, "{:?}", missing_out.1);
    assert_eq!(missing_out.1["fault"], "mapping_absent");
    assert_eq!(generation(&mut db).await, (0, 0));
    assert_eq!(order_rows(&mut db, &layout).await, snapshot_orders());

    Prepared { env, db, layout, shape, config_hash, schema_hash, type_hash }
}

async fn adopted_generation_survives_rollback(live: &mut Prepared) {
    let rows = vec![data_line(
        "rfail",
        1,
        1,
        "public.orders",
        "320",
        "upsert",
        &orders_payload("12", "kept320", false, "7", "9", true),
        &[1],
    )];
    let sealed = seal_request(&live.env, "rfail", "rfail", &live.shape, &["public.lines", "public.orders"], &rows, &[("public.orders", 1)], DB, false);
    let failed = run_cli(&sealed.request, &[("CDC_APPLY_INJECT", "row_failure")]);
    assert_eq!(failed.0, 2, "{:?}", failed.1);
    assert_eq!(failed.1["fault"], "row_failure");
    assert_eq!(failed.1["target_applied"], false);
    closed(&failed.1);
    let (count, value) = generation(&mut live.db).await;
    assert_eq!(count, 1);
    assert_eq!(value, 1, "first adopt inserts generation 1 and the rollback does not remove it");
    assert!(receipt(&mut live.db, "rfail").await.is_none());
    assert_eq!(order_rows(&mut live.db, &live.layout).await, snapshot_orders());
    println!("phase=generation_durable_before_transaction count={count} generation={value} receipt=absent qty=11");
}

fn happy_rows() -> Vec<String> {
    vec![
        data_line("r320", 1, 1, "public.orders", "320", "upsert", &orders_payload(INT8_MAX, "kept320", true, INT2_MIN, INT4_MIN, true), &[1]),
        data_line("r320", 2, 1, "public.lines", "70", "delete", "[]", &[]),
        data_line("r320", 3, 2, "public.lines", "90", "upsert", &lines_payload(INT8_MIN, "32767", "2147483647"), &[]),
        data_line("r320", 4, 2, "public.orders", "321", "upsert", &orders_payload("null", "row321", false, "null", "0", false), &[]),
    ]
}

async fn happy_replay_and_conflict(live: &mut Prepared) {
    let rows = happy_rows();
    let sealed = seal_request(
        &live.env,
        "r320",
        "r320",
        &live.shape,
        &["public.lines", "public.orders"],
        &rows,
        &[("public.lines", 2), ("public.orders", 2)],
        DB,
        false,
    );
    let applied = run_cli(&sealed.request, &[]);
    assert_eq!(applied.0, 0, "{:?}", applied.1);
    assert_eq!(applied.1["kind"], "applied");
    assert_eq!(applied.1["outcome"], "inserted");
    assert_eq!(applied.1["target_applied"], true);
    assert_eq!(applied.1["db_cross_table_consistent"], true);
    closed(&applied.1);
    assert_eq!(applied.1["applied_pos"], POSITION);
    assert_eq!(order_rows(&mut live.db, &live.layout).await, happy_orders());
    assert_eq!(
        line_rows(&mut live.db, &live.layout).await,
        vec![row4("90", Some(INT8_MIN), Some("32767"), Some("2147483647"))]
    );
    let stored = receipt(&mut live.db, "r320").await.expect("receipt");
    assert_eq!(stored.cross, "true");
    assert_eq!(stored.partial, "false");
    assert_eq!(stored.complete, "true");
    assert!(stored.segments_null);
    assert_eq!(stored.bitmap_len, 0);
    assert_eq!(stored.applied_pos, POSITION);
    assert_eq!(stored.lineage, LINEAGE);
    assert_eq!(stored.codec, "pg-commit-lsn/v1");
    assert_eq!(stored.payload, payload_digest(&sealed.spool));
    assert_eq!(stored.fence, applied.1["generation"].as_i64().unwrap().to_string());
    let (count, value) = generation(&mut live.db).await;
    assert_eq!(count, 1);
    assert_eq!(value, 2);
    assert_eq!(stored.fence, "2");
    println!("phase=applied generation={value} receipt_fence={} rows=2", stored.fence);

    let replay = run_cli(&sealed.request, &[]);
    assert_eq!(replay.0, 0, "{:?}", replay.1);
    assert_eq!(replay.1["outcome"], "replay");
    assert_eq!(replay.1["row_writes"], 0);
    assert_eq!(replay.1["db_cross_table_consistent"], true);
    closed(&replay.1);
    let again = receipt(&mut live.db, "r320").await.expect("replay receipt");
    assert_eq!(again.commit_at, stored.commit_at);
    assert_eq!(again.payload, stored.payload);
    assert_eq!(again.fence, stored.fence);
    assert_eq!(order_rows(&mut live.db, &live.layout).await.len(), 2);
    let (_count, after_replay) = generation(&mut live.db).await;
    assert!(after_replay > stored.fence.parse::<i64>().unwrap());
    println!("phase=replay generation={after_replay} receipt_fence_unchanged={}", again.fence);

    let mut conflict_rows = rows;
    conflict_rows[0] = data_line("r320", 1, 1, "public.orders", "320", "upsert", &orders_payload("4", "kept320", true, INT2_MIN, INT4_MIN, true), &[1]);
    let conflict = seal_request(
        &live.env,
        "r320b",
        "r320",
        &live.shape,
        &["public.lines", "public.orders"],
        &conflict_rows,
        &[("public.lines", 2), ("public.orders", 2)],
        DB,
        false,
    );
    let before = generation(&mut live.db).await.1;
    let conflicted = run_cli(&conflict.request, &[]);
    assert_eq!(conflicted.0, 2, "{:?}", conflicted.1);
    assert_eq!(conflicted.1["fault"], "payload_conflict");
    assert_eq!(conflicted.1["sqlstate"], "OO010");
    assert_eq!(conflicted.1["target_applied"], false);
    closed(&conflicted.1);
    let after = receipt(&mut live.db, "r320").await.expect("original receipt");
    assert_eq!(after.payload, stored.payload);
    assert_eq!(after.commit_at, stored.commit_at);
    assert_eq!(after.fence, stored.fence);
    assert_eq!(order_rows(&mut live.db, &live.layout).await[0][1].as_deref(), Some(INT8_MAX));
    let (count, value) = generation(&mut live.db).await;
    assert_eq!(count, 1);
    assert_eq!(value, before + 1, "OO010 rolls back the receipt, not the adopted generation");
    println!("phase=oo010 generation={value} receipt_fence={} qty={INT8_MAX}", after.fence);
}

fn row4(id: &str, amount: Option<&str>, n2: Option<&str>, n4: Option<&str>) -> Vec<Option<String>> {
    vec![Some(id.to_owned()), amount.map(str::to_owned), n2.map(str::to_owned), n4.map(str::to_owned)]
}

async fn unavailable_insert_and_api_delete(live: &mut Prepared) {
    let refused = seal_request(
        &live.env,
        "r322",
        "r322",
        &live.shape,
        &["public.lines", "public.orders"],
        &[data_line("r322", 1, 1, "public.orders", "322", "upsert", &orders_payload("unavailable", "nope", false, "1", "1", false), &[0])],
        &[("public.orders", 1)],
        DB,
        false,
    );
    let output = run_cli(&refused.request, &[]);
    assert_eq!(output.0, 2, "{:?}", output.1);
    assert_eq!(output.1["fault"], "unavailable_insert");
    assert!(receipt(&mut live.db, "r322").await.is_none());
    assert!(order_rows(&mut live.db, &live.layout).await.iter().all(|row| row[0].as_deref() != Some("322")));

    let rows = vec![data_line("rdel", 1, 1, "public.lines", "91", "delete", "[]", &[])];
    let owned = vec!["public.lines".to_owned(), "public.orders".to_owned()];
    let (config_hash, schema_hash, type_hash, _) = hashes(&live.shape, &owned);
    assert_eq!(config_hash, live.config_hash);
    assert_eq!(schema_hash, live.schema_hash);
    assert_eq!(type_hash, live.type_hash);
    let spool = seal("rdel", "rdel", &["public.lines", "public.orders"], &rows, &[("public.lines", 1)], &schema_hash, &config_hash, &type_hash);
    let mut request = live.shape.clone();
    request.spool_dir = spool;
    request.endpoint = Endpoint {
        host: live.env.host.clone(),
        port: live.env.port,
        database: DB.to_owned(),
        user: ROLE.to_owned(),
        password_file: live.env.runtime_pass_file.clone(),
        ca_file: live.env.ca_file.clone(),
        server_name: live.env.host.clone(),
    };
    let output = apply(&request).await;
    assert_eq!(output.fault, None);
    assert_eq!(output.outcome.as_deref(), Some("inserted"));
    assert!(output.target_applied);
    assert!(!output.ack_allowed);
    assert_eq!(output.commit_position_provenance, "caller_declared");
    assert!(receipt(&mut live.db, "rdel").await.is_some());
    assert_eq!(line_rows(&mut live.db, &live.layout).await.len(), 1);
    assert_eq!(line_rows(&mut live.db, &live.layout).await[0][0].as_deref(), Some("90"));
    println!("phase=delete_missing api=inserted lines=1");
}

async fn fence_and_old_generation(live: &mut Prepared) {
    let held_request = seal_request(
        &live.env,
        "fence",
        "rfence",
        &live.shape,
        &["public.lines", "public.orders"],
        &[data_line("rfence", 1, 1, "public.orders", "320", "upsert", &orders_payload("5", "kept320", true, INT2_MIN, INT4_MIN, true), &[1])],
        &[("public.orders", 1)],
        DB,
        true,
    );
    let before = generation(&mut live.db).await.1;
    let mut held = dial_runtime(&live.env, DB).await;
    let held_gen: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1::text, $2::text::uuid)")
        .bind(SHARD)
        .bind(OWNER_B)
        .fetch_one(&mut held)
        .await
        .expect("adopt");
    assert_eq!(held_gen, before + 1);
    let blocked = run_cli(&held_request.request, &[]);
    assert_eq!(blocked.0, 2, "{:?}", blocked.1);
    assert_eq!(blocked.1["fault"], "fence_held");
    assert_eq!(blocked.1["sqlstate"], "55P03");
    assert_eq!(blocked.1["control_lease_consulted"], false);
    assert_eq!(blocked.1["target_applied"], false);
    assert!(receipt(&mut live.db, "rfence").await.is_none());
    assert_eq!(generation(&mut live.db).await.1, held_gen);
    let stale = if held_gen == 1 { 2 } else { held_gen - 1 };
    let err = sqlx::query(
        "SELECT _oo.commit_apply_receipt( \
            $1::text, $2::text::uuid, $3::int8, $4::text, $5::text, $6::text, \
            $7::text, $8::text, $9::text, $10::text, $11::text, $12::text, $13::text, \
            $14::int4, $15::text::numeric, $16::bool, $17::bool, $18::bool, \
            $19::int4, $20::int4, $21::bytea)",
    )
    .bind(SHARD)
    .bind(OWNER_B)
    .bind(stale)
    .bind(SOURCE)
    .bind(TABLE_GENERATION)
    .bind("stale320")
    .bind(&live.config_hash)
    .bind(&live.type_hash)
    .bind(digest_text("cdc-apply-hooks/v1\n"))
    .bind(&live.schema_hash)
    .bind(LINEAGE)
    .bind("pg-commit-lsn/v1")
    .bind(LINEAGE)
    .bind(1i32)
    .bind(POSITION)
    .bind(false)
    .bind(true)
    .bind(true)
    .bind(None::<i32>)
    .bind(None::<i32>)
    .bind(Vec::<u8>::new())
    .fetch_one(&mut held)
    .await
    .expect_err("stale generation");
    let code = err.as_database_error().and_then(|db| db.code()).map(|code| code.to_string()).unwrap_or_default();
    let shown = err.to_string();
    assert_eq!(code, "OO013");
    assert!(!shown.contains("OO013"), "sqlx display now contains the code: {shown}");
    drop(held);
    assert_eq!(order_rows(&mut live.db, &live.layout).await[0][1].as_deref(), Some(INT8_MAX));
    println!("phase=fence_held sqlstate=55P03 stale={code} display_omits_code=1 generation={held_gen}");
}

async fn sigkill_windows(live: &mut Prepared) {
    let before = seal_request(
        &live.env,
        "r400",
        "r400",
        &live.shape,
        &["public.lines", "public.orders"],
        &[data_line("r400", 1, 1, "public.orders", "400", "upsert", &orders_payload("400", "pre", false, "1", "1", false), &[])],
        &[("public.orders", 1)],
        DB,
        false,
    );
    let generation_before = generation(&mut live.db).await.1;
    let mark = before.request.parent().expect("parent").join("before_commit.mark");
    let bin = env!("CARGO_BIN_EXE_cdc-apply-pg");
    let _ = fs::remove_file(&mark);
    let mut child = Command::new(bin)
        .arg("apply")
        .arg(&before.request)
        .env("CDC_APPLY_CHECKPOINT", "before_commit")
        .env("CDC_APPLY_MARK", &mark)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let started = Instant::now();
    while !mark.exists() {
        if started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            panic!("before_commit mark missing");
        }
        assert!(child.try_wait().expect("try").is_none());
        std::thread::sleep(Duration::from_millis(50));
    }
    let pids = runtime_pids(&mut live.db).await;
    let locks = advisory_pids(&mut live.db).await;
    assert_same_connection(&pids, &locks);
    assert!(receipt(&mut live.db, "r400").await.is_none());
    assert!(order_rows(&mut live.db, &live.layout).await.iter().all(|row| row[0].as_deref() != Some("400")));
    let parked = generation(&mut live.db).await.1;
    assert_eq!(parked, generation_before + 1);
    child.kill().expect("kill");
    let status = child.wait().expect("wait");
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(9));
    assert_eq!(generation(&mut live.db).await.1, parked);
    assert!(receipt(&mut live.db, "r400").await.is_none());
    let resumed = run_cli(&before.request, &[]);
    assert_eq!(resumed.0, 0, "{:?}", resumed.1);
    assert_eq!(resumed.1["outcome"], "inserted");
    assert_eq!(order_rows(&mut live.db, &live.layout).await.iter().filter(|row| row[0].as_deref() == Some("400")).count(), 1);
    println!("phase=sigkill_before_commit signal=9 generation_kept={parked} resumed=inserted");

    let after = seal_request(
        &live.env,
        "r401",
        "r401",
        &live.shape,
        &["public.lines", "public.orders"],
        &[data_line("r401", 1, 1, "public.orders", "401", "upsert", &orders_payload("401", "post", false, "1", "1", false), &[])],
        &[("public.orders", 1)],
        DB,
        false,
    );
    kill_after_commit(&mut live.db, &live.layout, &after.request).await;
    let committed = receipt(&mut live.db, "r401").await.expect("committed");
    let replayed = run_cli(&after.request, &[]);
    assert_eq!(replayed.0, 0, "{:?}", replayed.1);
    assert_eq!(replayed.1["outcome"], "replay");
    assert_eq!(replayed.1["row_writes"], 0);
    let again = receipt(&mut live.db, "r401").await.expect("replay");
    assert_eq!(again.commit_at, committed.commit_at);
    assert_eq!(again.fence, committed.fence);
    assert_eq!(order_rows(&mut live.db, &live.layout).await.iter().filter(|row| row[0].as_deref() == Some("401")).count(), 1);
    println!("phase=sigkill_after_commit signal=9 replay_fence={}", again.fence);
}

async fn kill_after_commit(db: &mut PgConnection, layout: &Layout, request: &Path) {
    let mark = request.parent().expect("parent").join("after_commit_before_response.mark");
    let bin = env!("CARGO_BIN_EXE_cdc-apply-pg");
    let _ = fs::remove_file(&mark);
    let mut child = Command::new(bin)
        .arg("apply")
        .arg(request)
        .env("CDC_APPLY_CHECKPOINT", "after_commit_before_response")
        .env("CDC_APPLY_MARK", &mark)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let started = Instant::now();
    while !mark.exists() {
        if started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            panic!("after_commit mark missing");
        }
        assert!(child.try_wait().expect("try").is_none());
        std::thread::sleep(Duration::from_millis(50));
    }
    let pids = runtime_pids(db).await;
    let locks = advisory_pids(db).await;
    assert_same_connection(&pids, &locks);
    assert!(receipt(db, "r401").await.is_some());
    assert_eq!(order_rows(db, layout).await.iter().filter(|row| row[0].as_deref() == Some("401")).count(), 1);
    child.kill().expect("kill");
    let status = child.wait().expect("wait");
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(9));
}

async fn hidden_commit_and_unknown(live: &mut Prepared) {
    let hidden = seal_request(
        &live.env,
        "r402",
        "r402",
        &live.shape,
        &["public.lines", "public.orders"],
        &[data_line("r402", 1, 1, "public.orders", "402", "upsert", &orders_payload("402", "hide", false, "1", "1", false), &[])],
        &[("public.orders", 1)],
        DB,
        false,
    );
    let hidden_out = run_cli(&hidden.request, &[("CDC_APPLY_INJECT", "hide_commit")]);
    assert_eq!(hidden_out.0, 0, "{:?}", hidden_out.1);
    assert_eq!(hidden_out.1["target_applied"], true);
    assert!(hidden_out.1["outcome"] == "inserted" || hidden_out.1["outcome"] == "replay");
    closed(&hidden_out.1);
    assert_eq!(order_rows(&mut live.db, &live.layout).await.iter().filter(|row| row[0].as_deref() == Some("402")).count(), 1);

    let unknown = seal_request(
        &live.env,
        "r403",
        "r403",
        &live.shape,
        &["public.lines", "public.orders"],
        &[data_line("r403", 1, 1, "public.orders", "403", "upsert", &orders_payload("403", "unk", false, "1", "1", false), &[])],
        &[("public.orders", 1)],
        DB,
        false,
    );
    let unknown_out = run_cli(&unknown.request, &[("CDC_APPLY_INJECT", "hide_commit"), ("CDC_APPLY_RECONCILE", "down")]);
    assert_eq!(unknown_out.0, 3, "{:?}", unknown_out.1);
    assert_eq!(unknown_out.1["kind"], "commit_unknown");
    assert_eq!(unknown_out.1["target_applied"], false);
    assert_eq!(unknown_out.1["db_cross_table_consistent"], false);
    closed(&unknown_out.1);
    let settled = run_cli(&unknown.request, &[]);
    assert_eq!(settled.0, 0, "{:?}", settled.1);
    assert_eq!(settled.1["target_applied"], true);
    assert_eq!(order_rows(&mut live.db, &live.layout).await.iter().filter(|row| row[0].as_deref() == Some("403")).count(), 1);
    let final_receipt = receipt(&mut live.db, "r403").await.expect("settled");
    assert_eq!(final_receipt.cross, "true");
    println!("phase=commit_unknown then_one_row fence={}", final_receipt.fence);
}
