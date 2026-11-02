//! chunked-typed/v2 C1 baseline (migration 0042): static checks and SQL truth
//! tests on one private PostgreSQL 17.
//!
//! The live test is ignored. It needs OO_C1V2_HOST, OO_C1V2_PORT and
//! OO_C1V2_PASSWORD_FILE (superuser `postgres`). It drops the oo_cb_t6_c1v2
//! database and the oo_target_owner and oo_pipeline_runtime roles, then
//! installs the whole chain once. Run it with `--ignored` against a
//! disposable server.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::time::Duration;

use oo_pipeline::target_installer::{self as ti, sha256_hex};
use serde_json::{Value, json};
use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};

const MIGRATION: &str = include_str!("../migrations/0042_native_c1_chunked_baseline.sql");
const INSTALLER: &str = include_str!("../src/target_installer.rs");
const NATIVE_UPDATE: &str = include_str!("../src/native_update.rs");

const CFG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TYPE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const INPUT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const TSCH: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const HOOK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const COMPOSED: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const L0_POS: i64 = 100;
const KEYS: i64 = 24;
const DB: &str = "oo_cb_t6_c1v2";

// ---------------------------------------------------------------- static checks

#[test]
fn migration_0042_is_installed_after_0041_with_its_exact_checksum() {
    assert!(INSTALLER.contains("0042_native_c1_chunked_baseline.sql"));
    assert!(INSTALLER.contains("pub async fn install_native_c1_chunked_baseline"));
    let after_0041 = NATIVE_UPDATE
        .find("install_native_partition_observation_read(&mut conn)")
        .expect("0041 step");
    let at_0042 = NATIVE_UPDATE
        .find("install_native_c1_chunked_baseline(&mut conn)")
        .expect("0042 step");
    assert!(
        at_0042 > after_0041,
        "0042 must follow 0041 in the install chain"
    );
    assert!(NATIVE_UPDATE.contains("\"step\": \"0042\""));
    assert_eq!(
        ti::compiled_native_c1_chunked_baseline_digest(),
        sha256_hex(MIGRATION.as_bytes())
    );
}

#[test]
fn migration_0042_public_writes_take_the_task_lock_first() {
    // open, open_target, chunk_apply, txn_batch, seal and rebind each take R2 first.
    assert_eq!(
        MIGRATION
            .matches("PERFORM _oo.native_c1_baseline_lock(p_source, p_task);")
            .count(),
        6
    );
    // The unlocked cores are never granted; only the public wrappers are.
    assert!(!MIGRATION.contains("GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_v2_chunk_core"));
    assert!(!MIGRATION.contains("GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_v2_batch_core"));
}

#[test]
fn migration_0042_leaves_v1_objects_and_grants_no_table_access_to_the_runtime() {
    assert!(!MIGRATION.contains("CREATE OR REPLACE"));
    assert!(!MIGRATION.contains("ALTER TABLE _oo.native_c1_baseline_run ADD"));
    assert!(!MIGRATION.contains("GRANT SELECT"));
    assert!(!MIGRATION.contains("GRANT UPDATE"));
    assert!(!MIGRATION.contains("GRANT DELETE"));
    // Table grants to the runtime: only the part stage INSERT, which is generated per table.
    let table_grants: Vec<&str> = MIGRATION
        .lines()
        .filter(|line| line.contains("ON TABLE") && line.contains("TO oo_pipeline_runtime"))
        .collect();
    assert_eq!(
        table_grants.len(),
        1,
        "only the part stage INSERT grant: {table_grants:?}"
    );
    assert!(table_grants[0].contains("GRANT INSERT ON TABLE _oo.%I TO oo_pipeline_runtime"));
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

async fn try_text(conn: &mut PgConnection, sql: &str) -> Result<String, String> {
    sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&mut *conn)
        .await
        .map_err(|err| err.to_string())
}

fn hex_of(seed: &str) -> String {
    sha256_hex(seed.as_bytes())
}

/// Xorshift64 with a fixed seed, so randomized interleavings are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// One source event. Keys within one transaction are distinct (producer contract).
#[derive(Clone, Debug, PartialEq)]
enum Ev {
    Up(i64, String),
    Del(i64),
}

impl Ev {
    fn key(&self) -> i64 {
        match self {
            Ev::Up(k, _) | Ev::Del(k) => *k,
        }
    }
}

#[derive(Clone, Debug)]
struct Txn {
    pos: i64,
    evs: Vec<Ev>,
}

/// A snapshot chunk: keys in (lo, hi]. hi None marks the terminal chunk.
#[derive(Clone, Debug)]
struct Chunk {
    no: i32,
    lo: Option<i64>,
    hi: Option<i64>,
    rows: Vec<(i64, String)>,
}

#[derive(Clone, Debug)]
enum Step {
    Chunk(Chunk),
    Batch(i32, Vec<Txn>),
}

/// One table `app.items(id int8 primary key, val text)` in one fenced run.
#[derive(Clone, Debug)]
struct Scenario {
    src: String,
    shard: String,
    owner: String,
    task: String,
    measured: String,
    tgen: i64,
    phys: String,
    stage: String,
    tomb: String,
    key_pc: String,
    val_pc: String,
}

impl Scenario {
    fn l_end(&self, pos: i64) -> String {
        format!("{}:1-{}", self.measured, pos)
    }
}

async fn insert_mappings(admin: &mut PgConnection, src: &str) {
    // Namespace first, then table, then columns: each level needs its parent.
    for (level, letter, table, column) in [
        ("namespace", "n", "", ""),
        ("table", "t", "items", ""),
        ("column", "c", "items", "id"),
        ("column", "c", "items", "val"),
    ] {
        let digest: Vec<u8> = sqlx::query_scalar(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, 1, $3, 'app', $4, $5))",
        )
        .bind(src)
        .bind(CFG)
        .bind(letter)
        .bind(table)
        .bind(column)
        .fetch_one(&mut *admin)
        .await
        .expect("identity digest");
        sqlx::query("SELECT _oo.register_physical_mapping($1, $2, 1, $3, 'app', $4, $5, $6)")
            .bind(src)
            .bind(CFG)
            .bind(level)
            .bind(table)
            .bind(column)
            .bind(digest)
            .execute(&mut *admin)
            .await
            .expect("register mapping");
    }
}

/// Source identity for one run: jobs lease, source instance and mappings.
/// Returns (src, shard, owner, task, measured).
async fn source_identity(
    admin: &mut PgConnection,
    n: usize,
    with_mappings: bool,
) -> (String, String, String, String, String) {
    let src = format!("c1v2_s{n}");
    let shard = format!("c1v2_shard_{n}");
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
    if with_mappings {
        insert_mappings(admin, &src).await;
    }
    (src, shard, owner, task, measured)
}

/// Full run: fence adopted and bound, v2 open and the table opened.
/// Returns the scenario, the fence connection (holds the shard lock) and a loader connection.
async fn create_scenario(
    admin: &mut PgConnection,
    n: usize,
) -> (Scenario, PgConnection, PgConnection) {
    let (src, shard, owner, task, measured) = source_identity(admin, n, true).await;
    let mut fence = connect(DB).await;
    let tgen: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1, $2::uuid)")
        .bind(&shard)
        .bind(&owner)
        .fetch_one(&mut fence)
        .await
        .expect("adopt");
    run(
        &mut fence,
        &format!("SELECT _oo.full_import_bind_session('{shard}', '{owner}'::uuid, {tgen})"),
    )
    .await;
    let mut loader = connect(DB).await;
    run(&mut loader, "SET ROLE oo_pipeline_runtime").await;
    let l0 = format!("{measured}:1-{L0_POS}");
    let opened = try_text(
        &mut loader,
        &format!(
            "SELECT _oo.native_c1_baseline_v2_open('{src}', '{task}'::uuid, 1, 1, 'mysql.{measured}', '{CFG}', '{SCH}', '{l0}', {L0_POS})::text"
        ),
    )
    .await
    .expect("v2 open");
    assert!(opened.contains("chunked-typed/v2"), "{opened}");
    let target = try_text(
        &mut fence,
        &format!(
            "SELECT _oo.native_c1_baseline_v2_open_target('{src}', '{task}'::uuid, 1, 1, '{shard}', '{owner}'::uuid, {tgen}, 1, \
             '{TYPE}', '{INPUT}', '{TSCH}', 'app', 'items', ARRAY['id','val'], ARRAY[20,25]::bigint[], \
             ARRAY[-1,-1]::integer[], ARRAY[true,false], 'id')::text"
        ),
    )
    .await
    .expect("open_target");
    let target: Value = serde_json::from_str(&target).expect("open_target json");
    assert_eq!(target["outcome"], "opened", "{target}");
    let mut admin_row = connect(DB).await;
    let row: (String, String, String, String, String) = sqlx::query_as(
        "SELECT physical_name, stage_name, tomb_name, physical_columns[1], physical_columns[2] \
         FROM _oo.native_c1_baseline_v2_table WHERE task_id = $1::uuid",
    )
    .bind(&task)
    .fetch_one(&mut admin_row)
    .await
    .expect("registration");
    let scenario = Scenario {
        src,
        shard,
        owner,
        task,
        measured,
        tgen,
        phys: row.0,
        stage: row.1,
        tomb: row.2,
        key_pc: row.3,
        val_pc: row.4,
    };
    (scenario, fence, loader)
}

// ---------------------------------------------------------------- staging helpers

fn lit(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

async fn stage_chunk_rows(loader: &mut PgConnection, sc: &Scenario, c: &Chunk) {
    for (k, v) in &c.rows {
        let sql = format!(
            "INSERT INTO _oo.{} (_oo_part_kind, _oo_part_no, _oo_op, _oo_pos, \"{}\", \"{}\") VALUES ('snapshot', {}, NULL, NULL, {k}, {})",
            sc.stage,
            sc.key_pc,
            sc.val_pc,
            c.no,
            lit(v)
        );
        run(loader, &sql).await;
    }
}

async fn stage_batch_rows(loader: &mut PgConnection, sc: &Scenario, no: i32, txns: &[Txn]) {
    for t in txns {
        for ev in &t.evs {
            let (op, val) = match ev {
                Ev::Up(_, v) => ("upsert", lit(v)),
                Ev::Del(_) => ("delete", "NULL".to_string()),
            };
            let sql = format!(
                "INSERT INTO _oo.{} (_oo_part_kind, _oo_part_no, _oo_op, _oo_pos, \"{}\", \"{}\") VALUES ('cdc', {no}, '{op}', {}, {}, {val})",
                sc.stage,
                sc.key_pc,
                sc.val_pc,
                t.pos,
                ev.key()
            );
            run(loader, &sql).await;
        }
    }
}

/// Stage and apply one chunk inside the caller's transaction.
async fn chunk_in_txn(
    loader: &mut PgConnection,
    sc: &Scenario,
    c: &Chunk,
    attempt: i32,
    sha: &str,
) -> Result<Value, String> {
    stage_chunk_rows(loader, sc, c).await;
    let lo = c.lo.map(|v| v.to_string()).unwrap_or_else(|| "NULL".into());
    let hi = c.hi.map(|v| v.to_string()).unwrap_or_else(|| "NULL".into());
    let sql = format!(
        "SELECT _oo.native_c1_baseline_v2_chunk_apply('{}', '{}'::uuid, 1, 1, 'app', 'items', {}, {attempt}, {lo}::numeric, {hi}::numeric, {}, {}, '{sha}')::text",
        sc.src,
        sc.task,
        c.no,
        c.hi.is_none(),
        c.rows.len()
    );
    try_text(loader, &sql)
        .await
        .map(|out| serde_json::from_str(&out).expect("chunk json"))
}

fn chunk_sha(c: &Chunk) -> String {
    hex_of(&format!(
        "chunk {} {:?} {:?} {:?}",
        c.no, c.lo, c.hi, c.rows
    ))
}

/// Load one chunk in its own transaction. Commits on success, rolls back on error.
async fn chunk(loader: &mut PgConnection, sc: &Scenario, c: &Chunk) -> Result<Value, String> {
    run(loader, "BEGIN").await;
    let result = chunk_in_txn(loader, sc, c, 1, &chunk_sha(c)).await;
    finish(loader, &result).await;
    result
}

fn recs(sc: &Scenario, txns: &[Txn]) -> String {
    Value::Array(
        txns.iter()
            .map(|t| {
                json!({
                    "applied_pos": t.pos.to_string(),
                    "txn_id": format!("{}:{}", sc.measured, t.pos),
                    "payload_sha256": hex_of(&format!("txn {}", t.pos)),
                    "source_commit_timestamp": "2026-10-17T10:00:00Z",
                    "row_count": t.evs.len(),
                })
            })
            .collect(),
    )
    .to_string()
}

fn batch_sha(txns: &[Txn]) -> String {
    hex_of(&format!("batch {txns:?}"))
}

/// Stage and apply one CDC batch inside the caller's transaction.
async fn batch_in_txn(
    loader: &mut PgConnection,
    sc: &Scenario,
    no: i32,
    txns: &[Txn],
    sha: &str,
) -> Result<Value, String> {
    stage_batch_rows(loader, sc, no, txns).await;
    let rows: usize = txns.iter().map(|t| t.evs.len()).sum();
    let first = txns[0].pos;
    let last = txns[txns.len() - 1].pos;
    let sql = format!(
        "SELECT _oo.native_c1_baseline_v2_txn_batch('{}', '{}'::uuid, 1, 1, {no}, {first}::numeric, {last}::numeric, {}, {rows}, '{sha}', {}::jsonb)::text",
        sc.src,
        sc.task,
        txns.len(),
        lit(&recs(sc, txns))
    );
    try_text(loader, &sql)
        .await
        .map(|out| serde_json::from_str(&out).expect("batch json"))
}

/// Apply one CDC batch in its own transaction. Commits on success, rolls back on error.
async fn batch(
    loader: &mut PgConnection,
    sc: &Scenario,
    no: i32,
    txns: &[Txn],
) -> Result<Value, String> {
    run(loader, "BEGIN").await;
    let result = batch_in_txn(loader, sc, no, txns, &batch_sha(txns)).await;
    finish(loader, &result).await;
    result
}

async fn finish(loader: &mut PgConnection, result: &Result<Value, String>) {
    if result.is_ok() {
        run(loader, "COMMIT").await;
    } else {
        run(loader, "ROLLBACK").await;
    }
}

async fn target_rows(
    admin: &mut PgConnection,
    sc: &Scenario,
) -> Vec<(i64, Option<String>, String)> {
    let sql = format!(
        "SELECT \"{}\"::text, \"{}\", _oo_pos::text FROM _oo.{} ORDER BY \"{}\"",
        sc.key_pc, sc.val_pc, sc.phys, sc.key_pc
    );
    let rows: Vec<(String, Option<String>, String)> =
        sqlx::query_as(sqlx::AssertSqlSafe(sql.clone()))
            .fetch_all(&mut *admin)
            .await
            .expect("target rows");
    // Sort in Rust by numeric key: ORDER BY on the ::text output column would sort lexicographically.
    let mut out: Vec<(i64, Option<String>, String)> = rows
        .into_iter()
        .map(|(k, v, p)| (k.parse().expect("key"), v, p))
        .collect();
    out.sort_by_key(|row| row.0);
    out
}

async fn tombstones(admin: &mut PgConnection, sc: &Scenario) -> Vec<(i64, String)> {
    let sql = format!("SELECT k::text, pos::text FROM _oo.{} ORDER BY k", sc.tomb);
    let rows: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(sql.clone()))
        .fetch_all(&mut *admin)
        .await
        .expect("tombstones");
    let mut out: Vec<(i64, String)> = rows
        .into_iter()
        .map(|(k, p)| (k.parse().expect("key"), p))
        .collect();
    out.sort_by_key(|row| row.0);
    out
}

fn up(k: i64, v: &str) -> Ev {
    Ev::Up(k, v.to_string())
}

fn txn(pos: i64, evs: Vec<Ev>) -> Txn {
    Txn { pos, evs }
}

fn chunk_of(no: i32, lo: Option<i64>, hi: Option<i64>, rows: &[(i64, &str)]) -> Chunk {
    Chunk {
        no,
        lo,
        hi,
        rows: rows.iter().map(|(k, v)| (*k, v.to_string())).collect(),
    }
}

fn outcome(v: &Value) -> &str {
    v["outcome"].as_str().unwrap_or("")
}

fn expect_err(result: Result<Value, String>, token: &str) {
    match result {
        Ok(v) => panic!("expected {token}, got {v}"),
        Err(err) => assert!(err.contains(token), "expected {token}: {err}"),
    }
}

/// Install the whole chain once and create the fixtures the v2 functions read.
async fn install_everything(admin: &mut PgConnection) {
    run(admin, &format!("DROP DATABASE IF EXISTS {DB} WITH (FORCE)")).await;
    run(admin, "DROP ROLE IF EXISTS oo_pipeline_runtime").await;
    run(admin, "DROP ROLE IF EXISTS oo_target_owner").await;
    run(admin, "DROP ROLE IF EXISTS oo_jobs_runtime").await;
    run(admin, "DROP ROLE IF EXISTS oo_jobs_owner").await;
    run(admin, &format!("CREATE DATABASE {DB}")).await;
    let mut db = connect(DB).await;
    ti::install(&mut db).await.expect("0000-0004");
    // The partition rebind installer reads the jobs control schema. This is the test fixture.
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
    // A second install with the same checksum is a replay: no script runs, no error.
    ti::install_native_c1_chunked_baseline(&mut db)
        .await
        .expect("0042 replay");
    // Snapshot the registration check used by the public read tests.
    assert_eq!(text(&mut db, "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'native-c1-chunked-baseline/v1'").await, "1");
}

// ---------------------------------------------------------------- per-key truth table (DESIGN s3.1)

/// Apply steps in the given order on one run, asserting every step succeeds.
async fn play(admin: &mut PgConnection, loader: &mut PgConnection, sc: &Scenario, steps: &[Step]) {
    for step in steps {
        match step {
            Step::Chunk(c) => {
                let out = chunk(loader, sc, c)
                    .await
                    .unwrap_or_else(|e| panic!("chunk {}: {e}", c.no));
                assert_eq!(outcome(&out), "loaded", "{out}");
            }
            Step::Batch(no, txns) => {
                let out = batch(loader, sc, *no, txns)
                    .await
                    .unwrap_or_else(|e| panic!("batch {no}: {e}"));
                assert_eq!(outcome(&out), "applied", "{out}");
            }
        }
    }
    let _ = admin;
}

async fn truth_table(admin: &mut PgConnection, n: &mut usize) {
    // Each case: (name, steps, expected rows, expected tombstones).
    type Case = (
        &'static str,
        Vec<Step>,
        Vec<(i64, Option<&'static str>, &'static str)>,
        Vec<(i64, &'static str)>,
    );
    let terminal = |lo: i64| Step::Chunk(chunk_of(2, Some(lo), None, &[]));
    let cases: Vec<Case> = vec![
        (
            "no events: snapshot rows land at position zero",
            vec![
                Step::Chunk(chunk_of(1, None, Some(10), &[(1, "a"), (2, "b")])),
                Step::Chunk(chunk_of(2, Some(10), None, &[])),
            ],
            vec![(1, Some("a"), "0"), (2, Some("b"), "0")],
            vec![],
        ),
        (
            "upsert after fill replaces the snapshot value",
            vec![
                Step::Chunk(chunk_of(1, None, Some(10), &[(1, "snap")])),
                Step::Batch(1, vec![txn(101, vec![up(1, "cdc")])]),
                terminal(10),
            ],
            vec![(1, Some("cdc"), "101")],
            vec![],
        ),
        (
            "upsert before fill: the later snapshot row is not applied",
            vec![
                Step::Batch(1, vec![txn(101, vec![up(1, "cdc")])]),
                Step::Chunk(chunk_of(1, None, Some(10), &[(1, "snap")])),
                terminal(10),
            ],
            vec![(1, Some("cdc"), "101")],
            vec![],
        ),
        (
            "delete after fill removes the row and leaves a tombstone",
            vec![
                Step::Chunk(chunk_of(1, None, Some(10), &[(1, "snap"), (2, "b")])),
                Step::Batch(1, vec![txn(101, vec![Ev::Del(1)])]),
                terminal(10),
            ],
            vec![(2, Some("b"), "0")],
            vec![(1, "101")],
        ),
        (
            "delete before fill: the snapshot row is blocked by the tombstone",
            vec![
                Step::Batch(1, vec![txn(101, vec![Ev::Del(1)])]),
                Step::Chunk(chunk_of(1, None, Some(10), &[(1, "snap"), (2, "b")])),
                terminal(10),
            ],
            vec![(2, Some("b"), "0")],
            vec![(1, "101")],
        ),
        (
            "insert then delete before fill: nothing survives",
            vec![
                Step::Batch(1, vec![txn(101, vec![up(1, "x")])]),
                Step::Batch(2, vec![txn(102, vec![Ev::Del(1)])]),
                Step::Chunk(chunk_of(1, None, Some(10), &[(1, "snap")])),
                terminal(10),
            ],
            vec![],
            vec![(1, "102")],
        ),
        (
            "PK change after fill: old key deleted, new key upserted",
            vec![
                Step::Chunk(chunk_of(1, None, Some(10), &[(5, "five")])),
                Step::Batch(1, vec![txn(101, vec![Ev::Del(5), up(6, "six")])]),
                terminal(10),
            ],
            vec![(6, Some("six"), "101")],
            vec![(5, "101")],
        ),
        (
            "PK change before fill: the old key's snapshot row is blocked",
            vec![
                Step::Batch(1, vec![txn(101, vec![Ev::Del(5), up(6, "six")])]),
                Step::Chunk(chunk_of(1, None, Some(10), &[(5, "five"), (6, "old6")])),
                terminal(10),
            ],
            vec![(6, Some("six"), "101")],
            vec![(5, "101")],
        ),
        (
            "delete then re-insert: the later upsert survives its tombstone",
            vec![
                Step::Chunk(chunk_of(1, None, Some(10), &[(1, "snap")])),
                Step::Batch(1, vec![txn(101, vec![Ev::Del(1)])]),
                Step::Batch(2, vec![txn(102, vec![up(1, "back")])]),
                terminal(10),
            ],
            vec![(1, Some("back"), "102")],
            vec![(1, "101")],
        ),
        (
            "delete then re-insert before fill: the snapshot row is still blocked",
            vec![
                Step::Batch(1, vec![txn(101, vec![Ev::Del(1)])]),
                Step::Batch(2, vec![txn(102, vec![up(1, "back")])]),
                Step::Chunk(chunk_of(1, None, Some(10), &[(1, "snap")])),
                terminal(10),
            ],
            vec![(1, Some("back"), "102")],
            vec![(1, "101")],
        ),
    ];
    for (name, steps, rows, tombs) in cases {
        *n += 1;
        let (sc, _fence, mut loader) = create_scenario(admin, *n).await;
        play(admin, &mut loader, &sc, &steps).await;
        let got = target_rows(admin, &sc).await;
        let want: Vec<(i64, Option<String>, String)> = rows
            .iter()
            .map(|(k, v, p)| (*k, v.map(str::to_string), p.to_string()))
            .collect();
        assert_eq!(got, want, "truth table: {name}");
        let got_tombs = tombstones(admin, &sc).await;
        let want_tombs: Vec<(i64, String)> =
            tombs.iter().map(|(k, p)| (*k, p.to_string())).collect();
        assert_eq!(got_tombs, want_tombs, "tombstones: {name}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "one private PostgreSQL 17 (OO_C1V2_HOST, OO_C1V2_PORT, OO_C1V2_PASSWORD_FILE)"]
async fn c1_chunked_baseline_v2_on_one_postgres() {
    let mut server = connect("postgres").await;
    install_everything(&mut server).await;
    let mut admin = connect(DB).await;
    let mut n = 0usize;

    truth_table(&mut admin, &mut n).await;
    replay_and_conflicts(&mut admin, &mut n).await;
    format_pinning(&mut admin, &mut n).await;
    lock_serializes_writers(&mut admin, &mut n).await;
    control_without_r2_resurrects(&mut admin, &mut n).await;
    runtime_privileges(&mut admin, &mut n).await;
    seal_requires_complete_snapshot_and_writes_receipts(&mut admin, &mut n).await;
    rebind_after_fence_generation_change(&mut admin, &mut n).await;
    order_independence(&mut admin, &mut n).await;
}

// ---------------------------------------------------------------- replay, conflict, sequence (DESIGN s3.4)

async fn replay_and_conflicts(admin: &mut PgConnection, n: &mut usize) {
    *n += 1;
    let (sc, _fence, mut loader) = create_scenario(admin, *n).await;
    let c1 = chunk_of(1, None, Some(10), &[(1, "a"), (2, "b")]);
    let first = chunk(&mut loader, &sc, &c1).await.expect("chunk 1");
    assert_eq!(outcome(&first), "loaded");
    assert_eq!(first["inserted"], 2);

    // Same chunk again: idempotent replay, no data written.
    let again = chunk(&mut loader, &sc, &c1).await.expect("replay");
    assert_eq!(outcome(&again), "replay", "{again}");
    assert_eq!(target_rows(admin, &sc).await.len(), 2);

    // Same number, different bytes: refused.
    let changed = chunk_of(1, None, Some(10), &[(1, "a"), (2, "c")]);
    expect_err(chunk(&mut loader, &sc, &changed).await, "chunk_conflict");
    // Same number, different key range: refused.
    let moved = chunk_of(1, None, Some(11), &[(1, "a"), (2, "b")]);
    expect_err(chunk(&mut loader, &sc, &moved).await, "chunk_conflict");
    // Skipping a chunk: refused.
    expect_err(
        chunk(&mut loader, &sc, &chunk_of(3, Some(10), Some(20), &[])).await,
        "chunk_sequence",
    );
    // Wrong lower bound for the next chunk: refused.
    expect_err(
        chunk(&mut loader, &sc, &chunk_of(2, Some(9), None, &[])).await,
        "chunk_sequence",
    );
    // Terminal chunk with rows: refused before any write.
    let bad_terminal = Chunk {
        no: 2,
        lo: Some(10),
        hi: None,
        rows: vec![(11, "x".into())],
    };
    expect_err(
        chunk(&mut loader, &sc, &bad_terminal).await,
        "invalid_chunk",
    );

    // Batches: GTID contiguity and replay identity.
    let t1 = vec![txn(101, vec![up(3, "x")])];
    let applied = batch(&mut loader, &sc, 1, &t1).await.expect("batch 1");
    assert_eq!(outcome(&applied), "applied");
    let replay = batch(&mut loader, &sc, 1, &t1).await.expect("batch replay");
    assert_eq!(outcome(&replay), "replay", "{replay}");
    let other = vec![txn(101, vec![up(3, "y")])];
    run(&mut loader, "BEGIN").await;
    let conflict = batch_in_txn(&mut loader, &sc, 1, &other, &batch_sha(&other)).await;
    run(&mut loader, "ROLLBACK").await;
    expect_err(conflict, "batch_conflict");
    expect_err(
        batch(&mut loader, &sc, 2, &[txn(103, vec![up(4, "z")])]).await,
        "batch_sequence",
    );
    expect_err(
        batch(
            &mut loader,
            &sc,
            2,
            &[txn(102, vec![up(4, "z"), up(4, "w")])],
        )
        .await,
        "invalid_batch_events",
    );
    // A replay of batch 1 did not apply twice; key 3 holds the first value.
    let rows = target_rows(admin, &sc).await;
    assert!(
        rows.iter()
            .any(|(k, v, p)| *k == 3 && v.as_deref() == Some("x") && p == "101"),
        "{rows:?}"
    );
}

// ---------------------------------------------------------------- format pinning (N-series, DESIGN s3.10)

async fn format_pinning(admin: &mut PgConnection, n: &mut usize) {
    // A v1-only task: v2 open must refuse, v2 chunk writes must refuse.
    *n += 1;
    let (src, _shard, _owner, task, measured) = source_identity(admin, *n, false).await;
    let mut loader = connect(DB).await;
    run(&mut loader, "SET ROLE oo_pipeline_runtime").await;
    let l0 = format!("{measured}:1-{L0_POS}");
    try_text(
        &mut loader,
        &format!("SELECT _oo.native_c1_baseline_open('{src}', '{task}'::uuid, 1, 1, 'mysql.{measured}', '{CFG}', '{SCH}', '{l0}', {L0_POS})::text"),
    )
    .await
    .expect("v1 open");
    let refused = try_text(
        &mut loader,
        &format!("SELECT _oo.native_c1_baseline_v2_open('{src}', '{task}'::uuid, 1, 1, 'mysql.{measured}', '{CFG}', '{SCH}', '{l0}', {L0_POS})::text"),
    )
    .await;
    assert!(
        refused.unwrap_err().contains("format_mismatch"),
        "v2 open on a v1 task"
    );
    let v1_row = format!(
        "SELECT _oo.native_c1_baseline_v2_chunk_apply('{src}', '{task}'::uuid, 1, 1, 'app', 'items', 1, 1, NULL, 10::numeric, false, 0, '{}')::text",
        hex_of("v1")
    );
    assert!(
        try_text(&mut loader, &v1_row)
            .await
            .unwrap_err()
            .contains("format_mismatch")
    );

    // A v2 task: every v1 write path refuses (JSON rows, transactions, seal).
    *n += 1;
    let (sc, _fence, mut v2) = create_scenario(admin, *n).await;
    let fill = format!(
        "SELECT _oo.native_c1_baseline_fill('{}', '{}'::uuid, 1, 1, '[{{\"schema\":\"app\",\"table\":\"items\",\"key\":1,\"value\":{{\"id\":1}}}}]'::jsonb)::text",
        sc.src, sc.task
    );
    assert!(
        try_text(&mut v2, &fill)
            .await
            .unwrap_err()
            .contains("format_mismatch"),
        "v1 fill on v2"
    );
    let v1_txn = format!(
        "SELECT _oo.native_c1_baseline_apply_txn('{}', '{}'::uuid, 1, 1, {}::numeric, '{}:{}', '[]'::jsonb, '{}')::text",
        sc.src,
        sc.task,
        L0_POS + 1,
        sc.measured,
        L0_POS + 1,
        hex_of("v1 txn")
    );
    assert!(
        try_text(&mut v2, &v1_txn)
            .await
            .unwrap_err()
            .contains("format_mismatch"),
        "v1 txn on v2"
    );
    let v1_seal = format!(
        "SELECT _oo.native_c1_baseline_seal('{}', '{}'::uuid, 1, 1, '{}', {L0_POS}::numeric)::text",
        sc.src,
        sc.task,
        sc.l_end(L0_POS)
    );
    assert!(
        try_text(&mut v2, &v1_seal)
            .await
            .unwrap_err()
            .contains("format_mismatch"),
        "v1 seal on v2"
    );
    // The v2 write path still works on the same run.
    let ok = chunk(&mut v2, &sc, &chunk_of(1, None, Some(10), &[(1, "a")]))
        .await
        .expect("v2 chunk");
    assert_eq!(outcome(&ok), "loaded");
}

// ---------------------------------------------------------------- lock and control (R2)

async fn wait_for_lock_wait(admin: &mut PgConnection) {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = $1 AND wait_event_type = 'Lock' \
             AND query LIKE '%native_c1_baseline_v2_chunk_apply%'",
        )
        .bind(DB)
        .fetch_one(&mut *admin)
        .await
        .expect("pg_stat_activity");
        if waiting >= 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the second writer never waited on the task lock");
}

async fn lock_serializes_writers(admin: &mut PgConnection, n: &mut usize) {
    *n += 1;
    let (sc, _fence, mut first) = create_scenario(admin, *n).await;
    let mut second = connect(DB).await;
    run(&mut second, "SET ROLE oo_pipeline_runtime").await;

    // The first writer holds the task lock inside an open transaction.
    run(&mut first, "BEGIN").await;
    let delete = vec![txn(101, vec![Ev::Del(5)])];
    let held = batch_in_txn(&mut first, &sc, 1, &delete, &batch_sha(&delete))
        .await
        .expect("held batch");
    assert_eq!(outcome(&held), "applied");

    // The second writer is a snapshot chunk with key 5. It must block until the first commits.
    let blocked_sc = sc.clone();
    let snap = chunk_of(1, None, Some(10), &[(5, "snap")]);
    let handle = tokio::spawn(async move {
        run(&mut second, "BEGIN").await;
        let result = chunk_in_txn(&mut second, &blocked_sc, &snap, 1, &chunk_sha(&snap)).await;
        finish(&mut second, &result).await;
        result
    });
    wait_for_lock_wait(admin).await;
    run(&mut first, "COMMIT").await;
    let out = handle
        .await
        .expect("join")
        .expect("chunk after the lock is released");
    assert_eq!(out["skipped_tomb"], 1, "{out}");
    assert_eq!(out["inserted"], 0, "{out}");
    let rows = target_rows(admin, &sc).await;
    assert!(rows.is_empty(), "the delete is not undone: {rows:?}");
    assert_eq!(tombstones(admin, &sc).await, vec![(5, "101".to_string())]);
}

/// Expected failure: with R2 off, the same two transactions resurrect key 5.
/// The control calls the unlocked cores directly. The test passes when the bug reproduces.
async fn control_without_r2_resurrects(admin: &mut PgConnection, n: &mut usize) {
    *n += 1;
    let (sc, _fence, _loader) = create_scenario(admin, *n).await;
    let mut delete_side = connect(DB).await;
    let mut fill_side = connect(DB).await;
    let delete = vec![txn(101, vec![Ev::Del(5)])];
    let core_batch = format!(
        "SELECT _oo.native_c1_baseline_v2_batch_core('{}', '{}'::uuid, 1, 1, 1, 101::numeric, 101::numeric, 1, 1, '{}', {}::jsonb)::text",
        sc.src,
        sc.task,
        batch_sha(&delete),
        lit(&recs(&sc, &delete))
    );
    run(&mut delete_side, "BEGIN").await;
    stage_batch_rows(&mut delete_side, &sc, 1, &delete).await;
    text(&mut delete_side, &core_batch).await;

    run(&mut fill_side, "BEGIN").await;
    let snap = chunk_of(1, None, Some(10), &[(5, "snap")]);
    stage_chunk_rows(&mut fill_side, &sc, &snap).await;
    let core_chunk = format!(
        "SELECT _oo.native_c1_baseline_v2_chunk_core('{}', '{}'::uuid, 1, 1, 'app', 'items', 1, 1, NULL, 10::numeric, false, 1, '{}')::text",
        sc.src,
        sc.task,
        chunk_sha(&snap)
    );
    let filled: Value =
        serde_json::from_str(&text(&mut fill_side, &core_chunk).await).expect("core chunk");
    assert_eq!(
        filled["inserted"], 1,
        "the fill cannot see the uncommitted tombstone"
    );
    run(&mut fill_side, "COMMIT").await;
    run(&mut delete_side, "COMMIT").await;

    let rows = target_rows(admin, &sc).await;
    assert_eq!(
        rows,
        vec![(5, Some("snap".to_string()), "0".to_string())],
        "resurrection reproduced"
    );
    assert_eq!(tombstones(admin, &sc).await, vec![(5, "101".to_string())]);
}

// ---------------------------------------------------------------- runtime privileges

async fn runtime_privileges(admin: &mut PgConnection, n: &mut usize) {
    *n += 1;
    let (sc, _fence, _loader) = create_scenario(admin, *n).await;
    let mut rt = connect(DB).await;
    run(&mut rt, "SET ROLE oo_pipeline_runtime").await;
    let direct = [
        format!("SELECT count(*) FROM _oo.{}", sc.phys),
        format!("SELECT count(*) FROM _oo.{}", sc.tomb),
        format!("DELETE FROM _oo.{}", sc.phys),
        "SELECT count(*) FROM _oo.native_c1_baseline_chunk".to_string(),
        "SELECT pg_advisory_xact_lock(1)".to_string(),
        "SELECT _oo.native_c1_baseline_v2_chunk_core('x', gen_random_uuid(), 1, 1, 'a', 'b', 1, 1, NULL, NULL, true, 0, repeat('a', 64))::text".to_string(),
    ];
    for sql in &direct {
        let err = try_text(&mut rt, sql).await.expect_err(sql);
        assert!(err.contains("permission denied"), "{sql}: {err}");
        // Each failed statement aborted the implicit transaction; the session is still usable.
    }
    // The public lock wrapper is the only advisory path the runtime has.
    assert_eq!(
        try_text(
            &mut rt,
            &format!(
                "SELECT _oo.native_c1_baseline_lock('{}', '{}'::uuid)::text",
                sc.src, sc.task
            )
        )
        .await
        .ok()
        .map(|_| "ok"),
        Some("ok")
    );
}

// ---------------------------------------------------------------- seal and receipts (N2)

fn seal_sql(sc: &Scenario, pos: i64, tables: &str) -> String {
    format!(
        "SELECT _oo.native_c1_baseline_v2_seal('{}', '{}'::uuid, 1, 1, '{}', '{}'::uuid, {}, '{}', {pos}::numeric, '{HOOK}', '{}'::jsonb)::text",
        sc.src,
        sc.task,
        sc.shard,
        sc.owner,
        sc.tgen,
        sc.l_end(pos),
        tables
    )
}

fn table_vector(rows: i64, digest: &str) -> String {
    json!([{ "schema": "app", "table": "items", "row_count": rows, "payload_digest": digest }])
        .to_string()
}

async fn seal_requires_complete_snapshot_and_writes_receipts(
    admin: &mut PgConnection,
    n: &mut usize,
) {
    *n += 1;
    let (sc, mut fence, mut loader) = create_scenario(admin, *n).await;
    chunk(
        &mut loader,
        &sc,
        &chunk_of(1, None, Some(10), &[(1, "a"), (2, "b")]),
    )
    .await
    .expect("chunk 1");
    batch(&mut loader, &sc, 1, &[txn(101, vec![up(3, "c")])])
        .await
        .expect("batch 1");
    // The snapshot is not complete: no terminal chunk yet.
    expect_err(
        try_text(&mut fence, &seal_sql(&sc, 101, &table_vector(3, COMPOSED)))
            .await
            .map(|s| serde_json::from_str(&s).unwrap()),
        "snapshot_incomplete",
    );
    chunk(&mut loader, &sc, &chunk_of(2, Some(10), None, &[]))
        .await
        .expect("terminal chunk");
    // Wrong table vector and wrong row count are refused.
    expect_err(
        try_text(&mut fence, &seal_sql(&sc, 101, "[]"))
            .await
            .map(|s| serde_json::from_str(&s).unwrap()),
        "seal_tables",
    );
    expect_err(
        try_text(&mut fence, &seal_sql(&sc, 101, &table_vector(99, COMPOSED)))
            .await
            .map(|s| serde_json::from_str(&s).unwrap()),
        "composed_row_count",
    );
    // Wrong final position is refused.
    expect_err(
        try_text(&mut fence, &seal_sql(&sc, 100, &table_vector(3, COMPOSED)))
            .await
            .map(|s| serde_json::from_str(&s).unwrap()),
        "final barrier",
    );

    let sealed: Value = serde_json::from_str(
        &try_text(&mut fence, &seal_sql(&sc, 101, &table_vector(3, COMPOSED)))
            .await
            .expect("seal"),
    )
    .expect("seal json");
    assert_eq!(outcome(&sealed), "sealed", "{sealed}");
    assert_eq!(sealed["tables"][0]["receipt_batch_id"], "c1v2.seal");
    assert_eq!(target_rows(admin, &sc).await.len(), 3);
    // N2: the existing quality and run-seal steps accept the v2 seal output with no Arrow input.
    let run_id = text(
        admin,
        &format!(
            "SELECT run_id::text FROM _oo.native_c1_baseline_v2_table WHERE task_id = '{}'::uuid",
            sc.task
        ),
    )
    .await;
    run(
        &mut fence,
        &format!(
            "SELECT _oo.native_update_seal_quality('{}', '{}'::uuid, {}, '{run_id}'::uuid, '{}', 1, 'app', 'items', '{}', \
             'c1v2.seal', '{COMPOSED}', 101::numeric, 'snapshot-zero/v1', 'full-import.snapshot', 'c1_cdc', 'native', 'T', \
             true, NULL, 3, 'never', 0, 'full')",
            sc.shard, sc.owner, sc.tgen, sc.src, sc.phys
        ),
    )
    .await;
    let run_state = text(
        &mut fence,
        &format!(
            "SELECT _oo.full_import_seal_run('{}', '{}'::uuid, {}, '{run_id}'::uuid, '{}', 1, '{CFG}', '{TYPE}', '{INPUT}', '{TSCH}', \
             ARRAY['app'], ARRAY['items'])::text",
            sc.shard, sc.owner, sc.tgen, sc.src
        ),
    )
    .await;
    assert_eq!(run_state, "awaiting_publish");
    let tomb_gone = text(
        admin,
        &format!("SELECT (to_regclass('_oo.{}') IS NULL)::text", sc.tomb),
    )
    .await;
    assert_eq!(tomb_gone, "true", "the tombstone table is dropped at seal");
    let stage_gone = text(
        admin,
        &format!("SELECT (to_regclass('_oo.{}') IS NULL)::text", sc.stage),
    )
    .await;
    assert_eq!(stage_gone, "true", "the part stage is dropped at seal");
    let coverage = text(
        admin,
        &format!(
            "SELECT applied_rows::text || ':' || exhausted::text || ':' || applied_pos::text FROM _oo.full_import_table \
             WHERE run_id = (SELECT run_id FROM _oo.native_c1_baseline_v2_table WHERE task_id = '{}'::uuid)",
            sc.task
        ),
    )
    .await;
    assert_eq!(coverage, "3:true:101");
    let receipts = text(
        admin,
        &format!(
            "SELECT count(*)::text FROM _oo.apply_receipts WHERE table_generation = '{}' AND txn_or_batch_id = 'c1v2.seal' AND payload_digest = '{COMPOSED}'",
            sc.phys
        ),
    )
    .await;
    assert_eq!(receipts, "1");
    let state = text(admin, &format!("SELECT state || ':' || l_end::text FROM _oo.native_c1_baseline_run WHERE task_id = '{}'::uuid", sc.task)).await;
    assert_eq!(state, "awaiting_publish:101");
    // A sealed run takes no more writes, and a repeat of the same seal is a replay.
    expect_err(
        chunk(&mut loader, &sc, &chunk_of(3, Some(10), None, &[])).await,
        "sealed",
    );
    let again: Value = serde_json::from_str(
        &try_text(&mut fence, &seal_sql(&sc, 101, &table_vector(3, COMPOSED)))
            .await
            .expect("replay"),
    )
    .unwrap();
    assert_eq!(outcome(&again), "replay", "{again}");
}

// ---------------------------------------------------------------- N1: fence generation changes mid-run

async fn rebind_after_fence_generation_change(admin: &mut PgConnection, n: &mut usize) {
    *n += 1;
    let (sc, fence1, mut loader) = create_scenario(admin, *n).await;
    chunk(&mut loader, &sc, &chunk_of(1, None, Some(10), &[(1, "a")]))
        .await
        .expect("chunk 1");
    // The loader process ends. Its fence session releases the shard lock.
    fence1.close().await.expect("close fence 1");

    // A new loader adopts the same shard. The generation advances.
    let mut fence2 = connect(DB).await;
    let g2: i64 = sqlx::query_scalar("SELECT _oo.adopt_target_generation($1, $2::uuid)")
        .bind(&sc.shard)
        .bind(&sc.owner)
        .fetch_one(&mut fence2)
        .await
        .expect("re-adopt");
    assert!(g2 > sc.tgen, "generation advances: {g2} > {}", sc.tgen);
    run(
        &mut fence2,
        &format!(
            "SELECT _oo.full_import_bind_session('{}', '{}'::uuid, {g2})",
            sc.shard, sc.owner
        ),
    )
    .await;

    // Chunk writes need only the lease, so they continue without rebinding.
    chunk(&mut loader, &sc, &chunk_of(2, Some(10), None, &[]))
        .await
        .expect("terminal chunk under the new fence");
    // Seal against the stale run binding is refused (the run still names the old generation).
    let tables = table_vector(1, COMPOSED);
    let sealed_fail = try_text(&mut fence2, &format!(
        "SELECT _oo.native_c1_baseline_v2_seal('{}', '{}'::uuid, 1, 1, '{}', '{}'::uuid, {g2}, '{}', 100::numeric, '{HOOK}', '{}'::jsonb)::text",
        sc.src, sc.task, sc.shard, sc.owner, sc.l_end(100), tables
    ))
    .await;
    assert!(
        sealed_fail.unwrap_err().contains("stale"),
        "seal before rebind must be refused"
    );

    // Rebind the open full-import run to the current fence generation.
    let rebound: Value = serde_json::from_str(
        &try_text(&mut fence2, &format!(
            "SELECT _oo.native_c1_baseline_v2_rebind('{}', '{}'::uuid, 1, '{}', '{}'::uuid, {g2})::text",
            sc.src, sc.task, sc.shard, sc.owner
        ))
        .await
        .expect("rebind"),
    )
    .expect("rebind json");
    assert_eq!(rebound["tables"][0]["rows"], 1, "{rebound}");
    assert_eq!(rebound["tables"][0]["terminal"], true, "{rebound}");
    assert_eq!(rebound["batches"]["last_batch_no"], 0, "{rebound}");
    let target = text(admin, &format!("SELECT target_generation::text FROM _oo.full_import_run WHERE run_id = (SELECT run_id FROM _oo.native_c1_baseline_v2_table WHERE task_id = '{}'::uuid)", sc.task)).await;
    assert_eq!(target, g2.to_string());

    let sealed: Value = serde_json::from_str(
        &try_text(&mut fence2, &format!(
            "SELECT _oo.native_c1_baseline_v2_seal('{}', '{}'::uuid, 1, 1, '{}', '{}'::uuid, {g2}, '{}', 100::numeric, '{HOOK}', '{}'::jsonb)::text",
            sc.src, sc.task, sc.shard, sc.owner, sc.l_end(100), tables
        ))
        .await
        .expect("seal after rebind"),
    )
    .expect("seal json");
    assert_eq!(outcome(&sealed), "sealed", "{sealed}");
}

// ---------------------------------------------------------------- order independence (DESIGN s3.1 theorem)

/// One random source history: initial state, transactions, chunks (terminal last).
/// Each chunk reads the state after a random prefix r of the history, r in 0..=T,
/// as DESIGN s3.1 permits (G_r contains L0 and is contained in L_end).
fn random_case(rng: &mut Rng) -> (BTreeMap<i64, String>, Vec<Txn>, Vec<Chunk>) {
    let mut l0 = BTreeMap::new();
    for k in 1..=KEYS {
        if rng.below(10) < 6 {
            l0.insert(k, format!("L0-{k}"));
        }
    }
    let t_count = 6 + rng.below(10) as usize;
    let mut txns = Vec::new();
    for i in 0..t_count {
        let pos = L0_POS + 1 + i as i64;
        let mut evs = Vec::new();
        if rng.below(5) == 0 {
            let a = 1 + rng.below(KEYS as u64) as i64;
            let mut b = a;
            while b == a {
                b = 1 + rng.below(KEYS as u64) as i64;
            }
            evs.push(Ev::Del(a));
            evs.push(up(b, &format!("pk{pos}-{b}")));
        } else {
            let m = 1 + rng.below(3) as usize;
            let mut used: Vec<i64> = Vec::new();
            while used.len() < m {
                let k = 1 + rng.below(KEYS as u64) as i64;
                if used.contains(&k) {
                    continue;
                }
                used.push(k);
                if rng.below(10) < 6 {
                    evs.push(up(k, &format!("p{pos}-{k}")));
                } else {
                    evs.push(Ev::Del(k));
                }
            }
        }
        txns.push(txn(pos, evs));
    }
    let mut states = vec![l0.clone()];
    for t in &txns {
        let mut s = states.last().expect("state").clone();
        for ev in &t.evs {
            match ev {
                Ev::Up(k, v) => {
                    s.insert(*k, v.clone());
                }
                Ev::Del(k) => {
                    s.remove(k);
                }
            }
        }
        states.push(s);
    }
    let chunk_count = 1 + rng.below(4) as usize;
    let mut cuts = std::collections::BTreeSet::new();
    while cuts.len() < chunk_count - 1 {
        cuts.insert(1 + rng.below((KEYS - 1) as u64) as i64);
    }
    let his: Vec<i64> = cuts.into_iter().chain(std::iter::once(KEYS)).collect();
    let mut chunks = Vec::new();
    let mut lo: Option<i64> = None;
    for (i, hi) in his.iter().enumerate() {
        let r = rng.below(t_count as u64 + 1) as usize;
        let rows: Vec<(i64, String)> = states[r]
            .iter()
            .filter(|(k, _)| lo.is_none_or(|l| **k > l) && **k <= *hi)
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        chunks.push(Chunk {
            no: i as i32 + 1,
            lo,
            hi: Some(*hi),
            rows,
        });
        lo = Some(*hi);
    }
    chunks.push(Chunk {
        no: his.len() as i32 + 1,
        lo,
        hi: None,
        rows: Vec::new(),
    });
    (l0, txns, chunks)
}

/// The final T_new: every key of the source state at L_end, with _oo_pos of its last upsert, else 0.
fn expected_rows(l0: &BTreeMap<i64, String>, txns: &[Txn]) -> Vec<(i64, Option<String>, String)> {
    let mut state = l0.clone();
    let mut last: BTreeMap<i64, (i64, bool)> = BTreeMap::new();
    for t in txns {
        for ev in &t.evs {
            match ev {
                Ev::Up(k, v) => {
                    state.insert(*k, v.clone());
                    last.insert(*k, (t.pos, true));
                }
                Ev::Del(k) => {
                    state.remove(k);
                    last.insert(*k, (t.pos, false));
                }
            }
        }
    }
    state
        .iter()
        .map(|(k, v)| {
            let pos = match last.get(k) {
                Some((p, true)) => p.to_string(),
                _ => "0".to_string(),
            };
            (*k, Some(v.clone()), pos)
        })
        .collect()
}

/// Batches are cut from the history in order. Chunks keep their order. The two
/// sequences are merged at random, which is the only freedom the protocol allows.
fn interleave(rng: &mut Rng, chunks: &[Chunk], txns: &[Txn]) -> Vec<Step> {
    let mut batches = Vec::new();
    let mut i = 0usize;
    let mut no = 1;
    while i < txns.len() {
        let g = (1 + rng.below(4) as usize).min(txns.len() - i);
        batches.push(Step::Batch(no, txns[i..i + g].to_vec()));
        no += 1;
        i += g;
    }
    let mut out = Vec::new();
    let (mut ci, mut bi) = (0usize, 0usize);
    while ci < chunks.len() || bi < batches.len() {
        let take_chunk = if ci >= chunks.len() {
            false
        } else if bi >= batches.len() {
            true
        } else {
            rng.below(2) == 0
        };
        if take_chunk {
            out.push(Step::Chunk(chunks[ci].clone()));
            ci += 1;
        } else {
            out.push(batches[bi].clone());
            bi += 1;
        }
    }
    out
}

async fn order_independence(admin: &mut PgConnection, n: &mut usize) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut runs = 0usize;
    for _case in 0..24 {
        let (l0, txns, chunks) = random_case(&mut rng);
        let want = expected_rows(&l0, &txns);
        for _interleaving in 0..2 {
            *n += 1;
            let (sc, _fence, mut loader) = create_scenario(admin, *n).await;
            // Snapshot rows at L0 are part of every prefix; the chunks carry them.
            let steps = interleave(&mut rng, &chunks, &txns);
            play(admin, &mut loader, &sc, &steps).await;
            let got = target_rows(admin, &sc).await;
            assert_eq!(got, want, "case {_case}: steps {steps:?}");
            runs += 1;
        }
    }
    assert_eq!(runs, 48);
}
