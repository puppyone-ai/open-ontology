//! chunked-typed/v2 loader operations (T7) on one private PostgreSQL 17.
//!
//! The live tests are ignored. They need OO_C1V2_HOST, OO_C1V2_PORT and
//! OO_C1V2_PASSWORD_FILE (superuser `postgres`). Each one drops the
//! `oo_cb_t7_apply` database and the runtime and owner roles, then installs the
//! whole chain through 0042. Run them with `--ignored --test-threads=1` against
//! a disposable server. The bulk tests also need OO_C1V2_BULK_DIR and
//! OO_C1V2_BULK_ROWS_PER_PART, set by the scratch generator (see the fixture
//! README in `tests/fixtures/c1_apply/gen_apply_ipc.rs.txt`).
//!
//! The non-ignored tests are static checks of the migration and the protocol.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use oo_pipeline::c1_parts::PART_MAX_BYTES;
use oo_pipeline::native_c1_baseline::{PROTOCOL_V2, Request, execute_v2, parse_request};
use oo_pipeline::target_installer::{self as ti, sha256_hex};
use serde_json::{Value, json};
use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};

const MIGRATION: &str = include_str!("../migrations/0042_native_c1_chunked_baseline.sql");
const STAGE_SRC: &str = include_str!("../src/native_c1_baseline.rs");
const APPLY_SRC: &str = include_str!("../src/native_c1_apply.rs");

const CFG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TYPE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const INPUT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const TSCH: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const HOOK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const COMPOSED: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const L0_POS: i64 = 100;
const DB: &str = "oo_cb_t7_apply";

// ---------------------------------------------------------------- static checks

#[test]
fn v2_stage_protocol_is_the_documented_one_and_v1_is_untouched() {
    assert_eq!(PROTOCOL_V2, "oo.pipeline.native-c1-baseline-stage/v2");
    assert!(
        STAGE_SRC.contains("const PROTOCOL: &str = \"oo.pipeline.native-c1-baseline-stage/v1\";")
    );
    assert!(STAGE_SRC.contains("const MAX_REQUEST: usize = 32 * 1024 * 1024;"));
    assert!(STAGE_SRC.contains("pub const MAX_REQUEST_V2: usize = 1024 * 1024;"));
}

#[test]
fn v2_loader_builds_no_sql_from_row_values_and_grants_only_the_stage_insert() {
    // Dynamic SQL is limited to the COPY statement, which names only the stage and
    // the catalog columns. Values travel as bind parameters.
    let compact: String = APPLY_SRC.chars().filter(|c| !c.is_whitespace()).collect();
    let apply_dynamic: Vec<&str> = compact
        .split("format!")
        .filter(|line| line.starts_with("(\"COPY"))
        .collect();
    assert_eq!(
        apply_dynamic.len(),
        1,
        "one dynamic COPY statement: {apply_dynamic:?}"
    );
    assert!(!STAGE_SRC.contains("format!(\"SELECT"));
    assert!(!APPLY_SRC.contains("format!(\"SELECT"));
    assert!(!STAGE_SRC.contains("AssertSqlSafe"));
    // The runtime role gets only the INSERT grant on a stage. No stage read or update.
    assert!(MIGRATION.contains("GRANT INSERT ON TABLE _oo.%I TO oo_pipeline_runtime"));
}

#[test]
fn v2_request_cap_is_one_mib_and_part_bound_is_32_mib() {
    assert!(STAGE_SRC.contains("MAX_REQUEST_V2"));
    const { assert!(PART_MAX_BYTES == 32 * 1024 * 1024) };
}

// ---------------------------------------------------------------- live fixture

fn env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the live test"))
}

fn endpoint() -> (String, u16, String) {
    let host = env_var("OO_C1V2_HOST");
    let port: u16 = env_var("OO_C1V2_PORT").parse().expect("OO_C1V2_PORT");
    let password = fs::read_to_string(env_var("OO_C1V2_PASSWORD_FILE"))
        .expect("password file")
        .trim()
        .to_string();
    (host, port, password)
}

async fn connect(database: &str) -> PgConnection {
    let (host, port, password) = endpoint();
    let options = PgConnectOptions::new()
        .host(&host)
        .port(port)
        .username("postgres")
        .password(&password)
        .database(database)
        .ssl_mode(PgSslMode::Disable);
    PgConnection::connect_with(&options).await.expect("connect")
}

async fn run(conn: &mut PgConnection, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("{sql}: {err}"));
}

async fn text(conn: &mut PgConnection, sql: &str) -> String {
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .unwrap_or_else(|err| panic!("{sql}: {err}"))
}

/// The runtime connection. Superuser connects and takes the runtime role.
async fn runtime_conn() -> PgConnection {
    let mut conn = connect(DB).await;
    run(&mut conn, "SET ROLE oo_pipeline_runtime").await;
    conn
}

async fn install_everything(admin: &mut PgConnection) {
    run(admin, &format!("DROP DATABASE IF EXISTS {DB} WITH (FORCE)")).await;
    run(admin, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
    run(admin, "DROP ROLE IF EXISTS oo_target_owner").await;
    run(admin, "DROP ROLE IF EXISTS oo_jobs_runtime").await;
    run(admin, "DROP ROLE IF EXISTS oo_jobs_owner").await;
    run(admin, &format!("CREATE DATABASE {DB}")).await;
    let mut db = connect(DB).await;
    ti::install(&mut db).await.expect("0000-0004");
    // The control schema reads below are a test fixture, as in the T6 live test.
    run(&mut db, "CREATE SCHEMA jobs").await;
    run(&mut db, "CREATE TABLE jobs.tasks (id uuid PRIMARY KEY, state text NOT NULL, generation bigint NOT NULL, \
        lease_until timestamptz NOT NULL, project_id uuid NOT NULL)").await;
    run(
        &mut db,
        "CREATE ROLE oo_jobs_owner NOLOGIN NOSUPERUSER NOINHERIT",
    )
    .await;
    run(
        &mut db,
        "CREATE ROLE oo_jobs_runtime NOLOGIN NOSUPERUSER NOINHERIT",
    )
    .await;
    run(
        &mut db,
        "GRANT USAGE ON SCHEMA jobs TO oo_target_owner, oo_pipeline_runtime",
    )
    .await;
    run(&mut db, "GRANT SELECT ON jobs.tasks TO oo_target_owner").await;
    ti::install_full_import(&mut db).await.expect("0005");
    ti::install_catalog_before_empty(&mut db)
        .await
        .expect("0006-0008");
    ti::install_jsonb_document(&mut db).await.expect("0009");
    ti::install_native_update(&mut db).await.expect("0010");
    ti::install_native_partition(&mut db).await.expect("0011");
    ti::install_native_partition_rebind(&mut db)
        .await
        .expect("0012");
    ti::install_native_interval(&mut db).await.expect("0013");
    ti::install_native_olap_partition(&mut db)
        .await
        .expect("0014-0016");
    ti::install_native_pg_transaction_group(&mut db)
        .await
        .expect("0017");
    ti::install_native_empty_catalog(&mut db)
        .await
        .expect("0018");
    ti::install_native_pg_group_receipt(&mut db)
        .await
        .expect("0019");
    ti::install_native_pg_group_prior(&mut db)
        .await
        .expect("0020");
    ti::install_native_pg_group_generation(&mut db)
        .await
        .expect("0021");
    ti::install_native_pg_single_transaction(&mut db)
        .await
        .expect("0022");
    ti::install_native_sqlserver_ct(&mut db)
        .await
        .expect("0023");
    ti::install_native_gtid_transaction(&mut db)
        .await
        .expect("0024");
    ti::install_native_c1_source_epoch(&mut db)
        .await
        .expect("0025");
    ti::install_native_c1_source_epoch_runtime(&mut db)
        .await
        .expect("0026");
    ti::install_native_c1_baseline_stage(&mut db)
        .await
        .expect("0028");
    ti::install_native_c1_gtid_stage_identity(&mut db)
        .await
        .expect("0029");
    ti::install_native_c1_gtid_measured_identity(&mut db)
        .await
        .expect("0030");
    ti::install_native_c1_baseline_typed_bridge(&mut db)
        .await
        .expect("0031");
    ti::install_native_c1_baseline_lease_rebind(&mut db)
        .await
        .expect("0032");
    ti::install_native_c1_baseline_date_quality(&mut db)
        .await
        .expect("0033");
    ti::install_native_c1_baseline_key_join(&mut db)
        .await
        .expect("0034");
    ti::install_native_c1_ack_seed(&mut db).await.expect("0035");
    ti::install_native_publication(&mut db).await.expect("0027");
    ti::install_native_c1_collection_publication(&mut db)
        .await
        .expect("0036");
    ti::install_native_c1_timed_receipt(&mut db)
        .await
        .expect("0037");
    ti::install_native_c1_quiet_baseline_time(&mut db)
        .await
        .expect("0038");
    ti::install_native_per_table_publication(&mut db)
        .await
        .expect("0039");
    ti::install_native_source_progress_read(&mut db)
        .await
        .expect("0040");
    ti::install_native_partition_observation_read(&mut db)
        .await
        .expect("0041");
    ti::install_native_c1_chunked_baseline(&mut db)
        .await
        .expect("0042");
    // 0043 holds the exact tombstone count that the batch guard calls (T8).
    ti::install_native_c1_chunked_baseline_final(&mut db)
        .await
        .expect("0043");
}

/// One source identity: a live jobs lease and a registered source epoch.
struct Scenario {
    src: String,
    shard: String,
    owner: String,
    task: String,
    measured: String,
    phys: String,
    stage: String,
    tomb: String,
    run_id: String,
    key_pc: String,
    val_pc: String,
}

async fn source_identity(
    admin: &mut PgConnection,
    n: usize,
) -> (String, String, String, String, String) {
    let src = format!("t7s{n}");
    let shard = format!("t7_shard_{n}");
    let owner = text(admin, "SELECT gen_random_uuid()::text").await;
    let task = text(admin, "SELECT gen_random_uuid()::text").await;
    let measured = text(admin, "SELECT gen_random_uuid()::text").await;
    run(
        admin,
        &format!(
            "INSERT INTO jobs.tasks (id, state, generation, lease_until, project_id) \
             VALUES ('{task}'::uuid, 'running', 1, clock_timestamp() + interval '1 hour', gen_random_uuid())"
        ),
    )
    .await;
    run(
        admin,
        &format!(
            "INSERT INTO _oo.source_instance (source, source_epoch, engine, measured_id, config_digest, \
             schema_digest, shard_id, owner_token, lease_id, lease_generation) \
             VALUES ('{src}', 1, 'mysql', 'mysql.{measured}', '{CFG}', '{SCH}', '{shard}', '{owner}'::uuid, '{task}'::uuid, 1)"
        ),
    )
    .await;
    (src, shard, owner, task, measured)
}

fn target() -> Value {
    json!({"host": "127.0.0.1", "port": 5432, "database": DB, "runtime_principal": "oo_pipeline_runtime",
           "password_file": "/unused", "ssl": "disable"})
}

fn envelope(sc: &Scenario, operation: &str) -> Value {
    json!({"protocol": PROTOCOL_V2, "operation": operation, "source_id": sc.src, "lease_id": sc.task,
           "lease_generation": 1, "source_epoch": 1, "target": target()})
}

fn with_fence(sc: &Scenario, mut value: Value) -> Value {
    value["fence"] = json!({"shard": sc.shard, "owner": sc.owner});
    value
}

fn merge(mut base: Value, extra: Value) -> Value {
    for (key, item) in extra.as_object().expect("object") {
        base[key] = item.clone();
    }
    base
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/c1_apply")
        .join(name)
}

fn file_entry(path: &Path) -> (u64, String) {
    let bytes = fs::read(path).expect("fixture");
    (bytes.len() as u64, sha256_hex(&bytes))
}

/// A unique directory with one part under its canonical name.
fn temp_part(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = env::temp_dir().join(format!("oo-cb-t7-{}-{}", std::process::id(), name));
    fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(name);
    fs::write(&path, bytes).expect("write");
    path
}

fn open_request(sc: &Scenario) -> Value {
    let mut value = with_fence(sc, envelope(sc, "open"));
    value = merge(
        value,
        json!({
            "measured_id": format!("mysql.{}", sc.measured),
            "config_digest": CFG, "schema_digest": SCH, "type_digest": TYPE, "input_digest": INPUT,
            "l0": format!("{}:1-{L0_POS}", sc.measured), "l0_pos": L0_POS.to_string(), "source_generation": 1,
            "tables": [{"schema": "app", "table": "items", "table_schema_digest": TSCH,
                        "columns": ["id", "val"], "oids": [20, 25], "typmods": [-1, -1],
                        "not_null": [true, false], "key": "id"}]
        }),
    );
    value
}

fn chunk_request(
    sc: &Scenario,
    no: i64,
    key_lo: Option<&str>,
    key_hi: Option<&str>,
    part: Option<(PathBuf, u64, &str, &str)>,
) -> Value {
    let mut value = envelope(sc, "chunk");
    let (part_json, sha, rows) = match part {
        Some((path, rows, lo, hi)) => {
            let (bytes, sha) = file_entry(&path);
            (
                json!({"path": path.to_string_lossy(), "bytes": bytes, "sha256": sha, "rows": rows, "key_lo": lo, "key_hi": hi}),
                sha,
                rows,
            )
        }
        None => (Value::Null, EMPTY.to_string(), 0),
    };
    value = merge(
        value,
        json!({
            "schema": "app", "table": "items", "chunk_no": no, "attempt": 1,
            "key_lo": key_lo, "key_hi": key_hi, "terminal": key_hi.is_none(), "row_count": rows,
            "part_sha256": sha, "columns": ["id", "val"], "key": "id", "part": part_json
        }),
    );
    value
}

fn batch_request(
    sc: &Scenario,
    no: i64,
    first: i64,
    parts: Vec<Value>,
    rows: u64,
    txns: &[(i64, u64)],
) -> Value {
    let records: Vec<Value> = txns
        .iter()
        .map(|(pos, count)| {
            json!({"applied_pos": pos.to_string(), "txn_id": format!("{}:{}", sc.measured, pos),
                   "payload_sha256": sha256_hex(format!("txn {pos}").as_bytes()),
                   "source_commit_timestamp": "2026-10-17T10:00:00Z", "row_count": count})
        })
        .collect();
    let last = first + txns.len() as i64 - 1;
    merge(
        envelope(sc, "txn_batch"),
        json!({
            "batch_no": no, "first_pos": first.to_string(), "last_pos": last.to_string(),
            "txns": txns.len(), "row_count": rows, "batch_sha256": sha256_hex(format!("batch {no} {first}").as_bytes()),
            "txn_records": records, "parts": parts
        }),
    )
}

fn items_batch_part(path: &Path, rows: u64, lo: &str, hi: &str) -> Value {
    let (bytes, sha) = file_entry(path);
    json!({"schema": "app", "table": "items", "columns": ["id", "val"], "key": "id",
           "part": {"path": path.to_string_lossy(), "bytes": bytes, "sha256": sha, "rows": rows, "key_lo": lo, "key_hi": hi}})
}

fn parse(request: &Value) -> Request {
    parse_request(request)
        .unwrap_or_else(|token| panic!("test request is invalid: {token}: {request}"))
}

/// Run one operation. `Ok` is the JSON result, `Err` the closed refusal token.
async fn op(conn: &mut PgConnection, request: Value) -> Result<Value, &'static str> {
    execute_v2(conn, &parse(&request)).await
}

fn outcome(value: &Result<Value, &'static str>) -> String {
    match value {
        Ok(item) => item["outcome"].as_str().unwrap_or("?").to_owned(),
        Err(token) => format!("refused:{token}"),
    }
}

async fn open_scenario(admin: &mut PgConnection, conn: &mut PgConnection, n: usize) -> Scenario {
    let (src, shard, owner, task, measured) = source_identity(admin, n).await;
    let mut sc = Scenario {
        src,
        shard,
        owner,
        task,
        measured,
        phys: String::new(),
        stage: String::new(),
        tomb: String::new(),
        run_id: String::new(),
        key_pc: String::new(),
        val_pc: String::new(),
    };
    let opened = op(conn, open_request(&sc)).await.expect("open");
    assert_eq!(opened["outcome"], "opened", "{opened}");
    eprintln!("T7-EXAMPLE open={opened}");
    assert_eq!(opened["format"], "chunked-typed/v2");
    let table = &opened["tables"][0];
    sc.phys = table["physical"].as_str().expect("physical").to_owned();
    sc.run_id = table["run_id"].as_str().expect("run").to_owned();
    sc.key_pc = table["key_column"].as_str().expect("key").to_owned();
    let names: (String, String) = sqlx::query_as(
        "SELECT stage_name, tomb_name FROM _oo.native_c1_baseline_v2_table WHERE task_id = $1::uuid",
    )
    .bind(&sc.task)
    .fetch_one(&mut *admin)
    .await
    .expect("registration");
    sc.stage = names.0;
    sc.tomb = names.1;
    let pcs: Vec<String> = sqlx::query_scalar("SELECT unnest(physical_columns) FROM _oo.native_c1_baseline_v2_table WHERE task_id = $1::uuid ORDER BY table_index")
        .bind(&sc.task)
        .fetch_all(&mut *admin)
        .await
        .expect("columns");
    sc.val_pc = pcs[1].clone();
    assert_eq!(sc.key_pc, pcs[0]);
    sc
}

fn fill1() -> PathBuf {
    fixture("fill-00000001.arrow")
}
fn fill2() -> PathBuf {
    fixture("fill-00000002.arrow")
}
fn cdc1() -> PathBuf {
    fixture("cdc-00000001.arrow")
}

async fn load_fill1(conn: &mut PgConnection, sc: &Scenario) -> Result<Value, &'static str> {
    op(
        conn,
        chunk_request(sc, 1, None, Some("3"), Some((fill1(), 3, "1", "3"))),
    )
    .await
}

async fn load_fill2(conn: &mut PgConnection, sc: &Scenario) -> Result<Value, &'static str> {
    op(
        conn,
        chunk_request(sc, 2, Some("3"), Some("7"), Some((fill2(), 2, "5", "7"))),
    )
    .await
}

async fn load_terminal(conn: &mut PgConnection, sc: &Scenario) -> Result<Value, &'static str> {
    op(conn, chunk_request(sc, 3, Some("7"), None, None)).await
}

/// The CDC batch of the fixture: txns 101, 102 and 103, six rows, one PK change.
async fn batch_fixture(
    conn: &mut PgConnection,
    sc: &Scenario,
    max: Option<u64>,
) -> Result<Value, &'static str> {
    let part = items_batch_part(&cdc1(), 6, "1", "11");
    let mut request = batch_request(sc, 1, 101, vec![part], 6, &[(101, 2), (102, 2), (103, 2)]);
    if let Some(bound) = max {
        request["max_tombstones"] = json!(bound);
    }
    op(conn, request).await
}

async fn target_rows(admin: &mut PgConnection, sc: &Scenario) -> Vec<(i64, Option<String>)> {
    let sql = format!(
        "SELECT \"{}\"::text, \"{}\" FROM _oo.{} ORDER BY 1",
        sc.key_pc, sc.val_pc, sc.phys
    );
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *admin)
        .await
        .expect("rows");
    let mut out: Vec<(i64, Option<String>)> = rows
        .into_iter()
        .map(|(k, v)| (k.parse().expect("key"), v))
        .collect();
    out.sort_by_key(|row| row.0);
    out
}

async fn stage_count(admin: &mut PgConnection, sc: &Scenario) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM _oo.{}",
        sc.stage
    )))
    .fetch_one(&mut *admin)
    .await
    .expect("stage")
}

async fn tombstones(admin: &mut PgConnection, sc: &Scenario) -> Vec<(i64, i64)> {
    let rows: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT k::text, pos::text FROM _oo.{} ORDER BY k",
        sc.tomb
    )))
    .fetch_all(&mut *admin)
    .await
    .expect("tombs");
    rows.into_iter()
        .map(|(k, p)| (k.parse().expect("key"), p.parse().expect("pos")))
        .collect()
}

fn expect_rows(rows: &[(i64, Option<String>)], want: &[(i64, Option<&str>)]) {
    let want: Vec<(i64, Option<String>)> = want
        .iter()
        .map(|(k, v)| (*k, v.map(str::to_owned)))
        .collect();
    assert_eq!(rows, want.as_slice());
}

// ---------------------------------------------------------------- live scenarios

/// The truth table of the fixture: snapshot chunks, then CDC, end to end.
async fn scenario_truth(admin: &mut PgConnection, conn: &mut PgConnection, n: usize) {
    let sc = open_scenario(admin, conn, n).await;
    let c1 = load_fill1(conn, &sc).await.expect("chunk 1");
    assert_eq!(
        (
            c1["outcome"].as_str(),
            c1["inserted"].as_i64(),
            c1["skipped_tomb"].as_i64()
        ),
        (Some("loaded"), Some(3), Some(0))
    );
    let c2 = load_fill2(conn, &sc).await.expect("chunk 2");
    assert_eq!(c2["inserted"], 2, "{c2}");
    let c3 = load_terminal(conn, &sc).await.expect("terminal");
    assert_eq!(c3["outcome"], "loaded", "{c3}");
    eprintln!("T7-EXAMPLE chunk1={c1} terminal={c3}");
    assert_eq!(
        stage_count(admin, &sc).await,
        0,
        "stages are empty after each commit"
    );

    let b1 = batch_fixture(conn, &sc, None).await.expect("batch 1");
    assert_eq!(
        (
            b1["outcome"].as_str(),
            b1["upserted"].as_i64(),
            b1["deleted"].as_i64()
        ),
        (Some("applied"), Some(3), Some(3)),
        "{b1}"
    );
    eprintln!("T7-EXAMPLE batch1={b1}");
    assert_eq!(b1["applied_pos"], "103");
    expect_rows(
        &target_rows(admin, &sc).await,
        &[
            (2, Some("TWO")),
            (7, Some("seven")),
            (9, Some("nine")),
            (11, Some("eleven")),
        ],
    );
    assert_eq!(
        tombstones(admin, &sc).await,
        vec![(1, 103), (3, 101), (5, 102)]
    );
    assert_eq!(stage_count(admin, &sc).await, 0);

    let rebound = execute_v2(conn, &parse(&with_fence(&sc, envelope(&sc, "rebind"))))
        .await
        .expect("rebind");
    eprintln!("T7-EXAMPLE rebind={rebound}");
    let table = &rebound["tables"][0];
    assert_eq!(
        (
            table["last_chunk_no"].as_i64(),
            table["terminal"].as_bool(),
            table["rows"].as_i64()
        ),
        (Some(3), Some(true), Some(5)),
        "{rebound}"
    );
    assert_eq!(rebound["batches"]["last_batch_no"], 1);
    assert_eq!(rebound["batches"]["last_pos"], "103");
}

/// Replay is idempotent. A replay that changes the identity is refused, and so is a gap.
async fn scenario_replay(admin: &mut PgConnection, conn: &mut PgConnection, n: usize) {
    let sc = open_scenario(admin, conn, n).await;
    assert_eq!(outcome(&load_fill1(conn, &sc).await), "loaded");
    let again = load_fill1(conn, &sc).await.expect("replay");
    assert_eq!(
        (again["outcome"].as_str(), again["inserted"].as_i64()),
        (Some("replay"), Some(3)),
        "{again}"
    );
    // Same chunk number, same digest, different key range: a conflicting replay.
    let conflict = op(
        conn,
        chunk_request(&sc, 1, None, Some("4"), Some((fill1(), 3, "1", "3"))),
    )
    .await;
    assert_eq!(outcome(&conflict), "refused:chunk_conflict");
    // Chunk 3 before chunk 2 is a sequence fault.
    let gap = load_terminal(conn, &sc).await;
    assert_eq!(outcome(&gap), "refused:chunk_sequence");
    assert_eq!(
        target_rows(admin, &sc).await.len(),
        3,
        "the refusals wrote nothing"
    );
    assert_eq!(stage_count(admin, &sc).await, 0);

    assert_eq!(outcome(&load_fill2(conn, &sc).await), "loaded");
    assert_eq!(outcome(&load_terminal(conn, &sc).await), "loaded");
    assert_eq!(outcome(&batch_fixture(conn, &sc, None).await), "applied");
    let replay = batch_fixture(conn, &sc, None).await.expect("batch replay");
    assert_eq!(
        (replay["outcome"].as_str(), replay["applied_pos"].as_str()),
        (Some("replay"), Some("103")),
        "{replay}"
    );
    // Same batch number with another digest is a conflict.
    let mut other = batch_request(
        &sc,
        1,
        101,
        vec![items_batch_part(&cdc1(), 6, "1", "11")],
        6,
        &[(101, 2), (102, 2), (103, 2)],
    );
    other["batch_sha256"] = json!(sha256_hex(b"another batch"));
    assert_eq!(outcome(&op(conn, other).await), "refused:batch_conflict");
    // A batch past the applied position is a sequence fault.
    // The part is batch 2's file (the canonical name), with batch 1's rows.
    let second = temp_part("cdc-00000002.arrow", &fs::read(cdc1()).expect("fixture"));
    let skipped = batch_request(
        &sc,
        2,
        105,
        vec![items_batch_part(&second, 6, "1", "11")],
        6,
        &[(105, 2), (106, 2), (107, 2)],
    );
    assert_eq!(outcome(&op(conn, skipped).await), "refused:batch_sequence");
    assert_eq!(stage_count(admin, &sc).await, 0);
}

/// Part integrity: digest, oversize, keys and column order are refused before any row is written.
async fn scenario_part_refusals(admin: &mut PgConnection, conn: &mut PgConnection, n: usize) {
    let sc = open_scenario(admin, conn, n).await;

    // The request declares the fixture's digest, but the file on disk differs.
    let mut bytes = fs::read(fill1()).expect("fixture");
    let last = bytes.len() - 1;
    bytes[last] ^= 0x5a;
    let tampered = temp_part("fill-00000001.arrow", &bytes);
    let declared = file_entry(&fill1());
    let mut request = chunk_request(
        &sc,
        1,
        None,
        Some("3"),
        Some((tampered.clone(), 3, "1", "3")),
    );
    request["part"]["sha256"] = json!(declared.1.clone());
    request["part_sha256"] = json!(declared.1);
    request["part"]["bytes"] = json!(bytes.len());
    assert_eq!(outcome(&op(conn, request).await), "refused:part_sha256");

    // A file over the part bound is refused by size before it is decoded.
    let huge = temp_part(
        "fill-00000001.arrow",
        &vec![0u8; PART_MAX_BYTES as usize + 1],
    );
    let mut request = chunk_request(&sc, 1, None, Some("3"), Some((fill1(), 3, "1", "3")));
    request["part"]["path"] = json!(huge.to_string_lossy());
    request["part"]["bytes"] = json!(1000);
    assert_eq!(outcome(&op(conn, request).await), "refused:part_too_large");

    // The entry claims bounds the rows do not have.
    let lying = chunk_request(&sc, 1, None, Some("3"), Some((fill1(), 3, "1", "2")));
    assert_eq!(outcome(&op(conn, lying).await), "refused:part_keys");

    // The part names its columns in another order.
    let mut swapped = chunk_request(&sc, 1, None, Some("3"), Some((fill1(), 3, "1", "3")));
    swapped["columns"] = json!(["val", "id"]);
    swapped["key"] = json!("id");
    assert_eq!(outcome(&op(conn, swapped).await), "refused:part_schema");

    // A file under another name is refused: the name must be the canonical one.
    let renamed = temp_part("fill-00000009.arrow", &fs::read(fill1()).expect("fixture"));
    let wrong_name = chunk_request(
        &sc,
        1,
        None,
        Some("3"),
        Some((renamed.clone(), 3, "1", "3")),
    );
    assert_eq!(outcome(&op(conn, wrong_name).await), "refused:part_path");

    assert_eq!(
        target_rows(admin, &sc).await.len(),
        0,
        "no refusal wrote a row"
    );
    assert_eq!(stage_count(admin, &sc).await, 0);
    assert_eq!(text(admin, &format!("SELECT count(*)::text FROM _oo.native_c1_baseline_chunk WHERE task_id = '{}'::uuid", sc.task)).await, "0");
}

/// N6: the tombstone bound refuses a batch before it writes, and a larger bound applies it.
async fn scenario_tombstone_bound(admin: &mut PgConnection, conn: &mut PgConnection, n: usize) {
    let sc = open_scenario(admin, conn, n).await;
    assert_eq!(outcome(&load_fill1(conn, &sc).await), "loaded");
    assert_eq!(outcome(&load_fill2(conn, &sc).await), "loaded");
    assert_eq!(outcome(&load_terminal(conn, &sc).await), "loaded");
    // The batch deletes three keys. A bound of two refuses it.
    let refused = batch_fixture(conn, &sc, Some(2)).await;
    assert_eq!(outcome(&refused), "refused:tombstone_overflow");
    assert_eq!(
        tombstones(admin, &sc).await.len(),
        0,
        "the refusal wrote no tombstone"
    );
    // A bound of three is exactly enough, and the default bound also applies it.
    let applied = batch_fixture(conn, &sc, Some(3)).await;
    assert_eq!(outcome(&applied), "applied", "{applied:?}");
    assert_eq!(tombstones(admin, &sc).await.len(), 3);
}

/// Seal with the caller's digests (the T8 interface), then retire an unsealed run (N1).
async fn scenario_seal_and_retire(admin: &mut PgConnection, conn: &mut PgConnection, n: usize) {
    let sc = open_scenario(admin, conn, n).await;
    assert_eq!(outcome(&load_fill1(conn, &sc).await), "loaded");
    assert_eq!(outcome(&load_fill2(conn, &sc).await), "loaded");
    assert_eq!(outcome(&load_terminal(conn, &sc).await), "loaded");
    assert_eq!(outcome(&batch_fixture(conn, &sc, None).await), "applied");

    let seal = |rows: u64| {
        merge(
            with_fence(&sc, envelope(&sc, "seal")),
            json!({"l_end": format!("{}:1-103", sc.measured), "l_end_pos": "103", "hook_digest": HOOK,
                   "tables": [{"schema": "app", "table": "items", "row_count": rows, "payload_digest": COMPOSED}]}),
        )
    };
    // The expected count is wrong: the SQL refuses it and writes nothing.
    assert_eq!(
        outcome(&op(conn, seal(5)).await),
        "refused:composed_row_count"
    );
    let sealed = op(conn, seal(4)).await.expect("seal");
    assert_eq!(sealed["outcome"], "sealed", "{sealed}");
    eprintln!("T7-EXAMPLE seal={sealed}");
    let state = text(
        admin,
        &format!(
            "SELECT state FROM _oo.native_c1_baseline_run WHERE task_id = '{}'::uuid",
            sc.task
        ),
    )
    .await;
    assert_eq!(state, "awaiting_publish");
    let again = op(conn, seal(4)).await.expect("seal replay");
    assert_eq!(again["outcome"], "replay", "{again}");

    // A second, unsealed run is retired. Its full-import run leaves `open`.
    let m = scenario_retire_run(admin, conn, n + 100).await;
    assert_eq!(m, "retired");
}

async fn scenario_retire_run(
    admin: &mut PgConnection,
    conn: &mut PgConnection,
    n: usize,
) -> String {
    let sc = open_scenario(admin, conn, n).await;
    assert_eq!(outcome(&load_fill1(conn, &sc).await), "loaded");
    let retired = execute_v2(conn, &parse(&with_fence(&sc, envelope(&sc, "retire"))))
        .await
        .expect("retire");
    assert_eq!(retired["outcome"], "retired", "{retired}");
    assert_eq!(retired["runs"][0], sc.run_id.as_str());
    let state = text(
        admin,
        &format!(
            "SELECT state FROM _oo.full_import_run WHERE run_id = '{}'::uuid",
            sc.run_id
        ),
    )
    .await;
    let again = execute_v2(conn, &parse(&with_fence(&sc, envelope(&sc, "retire"))))
        .await
        .expect("retire again");
    assert_eq!(again["outcome"], "retired", "retire is idempotent");
    state
}

#[tokio::test]
#[ignore = "needs OO_C1V2_HOST, OO_C1V2_PORT and OO_C1V2_PASSWORD_FILE (disposable PostgreSQL 17)"]
async fn live_v2_loader_operations_match_the_design() {
    let mut root = connect("postgres").await;
    install_everything(&mut root).await;
    let mut admin = connect(DB).await;
    scenario_truth(&mut admin, &mut runtime_conn().await, 1).await;
    scenario_replay(&mut admin, &mut runtime_conn().await, 2).await;
    scenario_part_refusals(&mut admin, &mut runtime_conn().await, 3).await;
    scenario_tombstone_bound(&mut admin, &mut runtime_conn().await, 4).await;
    scenario_seal_and_retire(&mut admin, &mut runtime_conn().await, 5).await;
}

// ---------------------------------------------------------------- bulk: memory and throughput

fn vm_hwm_kib() -> u64 {
    let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

/// Load every part of a bulk directory as consecutive chunks, timing each one.
async fn bulk_load(label: &str) {
    let dir = PathBuf::from(env_var("OO_C1V2_BULK_DIR"));
    let per_part: i64 = env_var("OO_C1V2_BULK_ROWS_PER_PART")
        .parse()
        .expect("rows per part");
    let total: i64 = env_var("OO_C1V2_BULK_ROWS").parse().expect("rows");
    let mut root = connect("postgres").await;
    install_everything(&mut root).await;
    let mut admin = connect(DB).await;
    let mut conn = runtime_conn().await;
    let sc = open_scenario(&mut admin, &mut conn, 7).await;
    let started = Instant::now();
    let mut chunk_times = Vec::new();
    let mut key = 0i64;
    let mut no = 1i64;
    while key < total {
        let last = (key + per_part).min(total);
        let path = dir.join(format!("fill-{no:08}.arrow"));
        let lo = if key == 0 {
            None
        } else {
            Some(key.to_string())
        };
        let hi = last.to_string();
        let first = (key + 1).to_string();
        let request = chunk_request(
            &sc,
            no,
            lo.as_deref(),
            Some(hi.as_str()),
            Some((path, (last - key) as u64, first.as_str(), hi.as_str())),
        );
        let step = Instant::now();
        let result = op(&mut conn, request).await;
        chunk_times.push(step.elapsed().as_secs_f64());
        assert_eq!(outcome(&result), "loaded", "chunk {no}: {result:?}");
        key = last;
        no += 1;
    }
    let elapsed = started.elapsed().as_secs_f64();
    let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM _oo.{}",
        sc.phys
    )))
    .fetch_one(&mut admin)
    .await
    .expect("count");
    assert_eq!(count, total, "every key landed once");
    let mut sorted = chunk_times.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    let p50 = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() * 95 / 100).min(sorted.len() - 1)];
    eprintln!(
        "T7-BULK {label}: rows={total} chunks={} elapsed_s={elapsed:.2} rows_per_s={:.0} chunk_p50_s={p50:.3} chunk_p95_s={p95:.3} peak_rss_kib={}",
        chunk_times.len(),
        total as f64 / elapsed,
        vm_hwm_kib()
    );
}

#[tokio::test]
#[ignore = "bulk memory run: OO_C1V2_BULK_DIR and friends, see module docs"]
async fn live_bulk_multi_part_load_reports_peak_memory_and_rows_per_second() {
    bulk_load("multi-part").await;
}
