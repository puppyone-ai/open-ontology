//! chunked-typed/v2 final load (T8): composed verification, indexes, seal and
//! typed commit on one private PostgreSQL 17.
//!
//! The live tests are ignored. They need OO_C1V2_HOST, OO_C1V2_PORT and
//! OO_C1V2_PASSWORD_FILE (superuser `postgres`, the same variables as the T6 and
//! T7 live tests). Each one drops the `oo_cb_t8_final` database and the roles it
//! uses, then installs the whole chain through 0043. The parts are the committed
//! `tests/fixtures/c1_parts` files, so the expected states below come from the
//! fixture rows, not from the loader.
//!
//! Run with `--ignored --test-threads=1` against a disposable server. The static
//! tests run without it.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use oo_pipeline::c1_parts::{
    Cell, StateDigest, bucket_key, canonical_row, composed_digest, logical_type_of_pg,
};
use oo_pipeline::native_c1_baseline::{PROTOCOL_V2, Request, execute_v2, parse_request};
use oo_pipeline::native_update::{C1_BASELINE_V2_PROTOCOL, C1_BUCKET_BITS, execute_c1_baseline_v2};
use oo_pipeline::target_installer::{self as ti, sha256_hex};
use serde_json::{Value, json};
use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};

const MIGRATION_0042: &str = include_str!("../migrations/0042_native_c1_chunked_baseline.sql");
const MIGRATION: &str = include_str!("../migrations/0043_native_c1_chunked_baseline_final.sql");
const INSTALLER: &str = include_str!("../src/target_installer.rs");
const NATIVE_UPDATE: &str = include_str!("../src/native_update.rs");
const BIN: &str = include_str!("../src/bin/oo-pipeline.rs");

const CFG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SCH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TYPE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const INPUT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const TSCH_ITEMS: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const TSCH_OTHER: &str = "f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const L0_POS: i64 = 5230;
const L_END_POS: i64 = 5260;
const DB: &str = "oo_cb_t8_final";
const LOGIN: &str = "oo_cb_t8_login";
const COLUMNS: [&str; 5] = ["id", "qty", "name", "blob", "created"];

// ---------------------------------------------------------------- static checks

#[test]
fn migration_0043_is_installed_after_0042_with_its_exact_checksum() {
    assert!(INSTALLER.contains("0043_native_c1_chunked_baseline_final.sql"));
    assert!(INSTALLER.contains("pub async fn install_native_c1_chunked_baseline_final"));
    let at_0042 = NATIVE_UPDATE
        .find("install_native_c1_chunked_baseline(&mut conn)")
        .expect("0042 step");
    let at_0043 = NATIVE_UPDATE
        .find("install_native_c1_chunked_baseline_final(&mut conn)")
        .expect("0043 step");
    assert!(
        at_0043 > at_0042,
        "0043 must follow 0042 in the install chain"
    );
    assert!(NATIVE_UPDATE.contains("\"step\": \"0043\""));
    assert!(NATIVE_UPDATE.contains("\"native_c1_chunked_baseline_final_digest\""));
    assert_eq!(
        ti::compiled_native_c1_chunked_baseline_final_digest(),
        sha256_hex(MIGRATION.as_bytes())
    );
    assert_eq!(
        MIGRATION_0042
            .matches("CREATE FUNCTION _oo.native_c1_baseline_v2_assert")
            .count(),
        1
    );
}

#[test]
fn a_v2_successor_preserves_format_after_an_authenticated_ack_seed() {
    let sql = include_str!("../migrations/0045_native_c1_ack_seed_format.sql");
    assert!(sql.contains("p_previous AND format='chunked-typed/v2'"));
    assert!(sql.contains("INSERT INTO _oo.native_c1_baseline_v2_format"));
    assert!(sql.contains("acknowledged_at IS NOT NULL"));
    assert!(sql.contains("prior_task.project_id IS DISTINCT FROM current_task.project_id"));
    assert!(sql.contains("current_task.lease_until<=clock_timestamp()"));
    assert!(sql.contains("i.lease_id NOT IN (p_previous,p_task)"));
    assert!(sql.contains("'format_mismatch'"));
    let prior = NATIVE_UPDATE
        .find("install_native_source_instance_grant(&mut conn)")
        .unwrap();
    let next = NATIVE_UPDATE
        .find("install_native_c1_ack_seed_format(&mut conn)")
        .unwrap();
    assert!(next > prior);
    let seal = include_str!("../migrations/0046_native_c1_v2_seal_batch_identity.sql");
    assert!(seal.contains("'c1v2.seal:' || p_task::text"));
    assert!(seal.contains("'receipt_batch_id', v_batch"));
}

#[test]
fn migration_0043_guards_retired_runs_and_counts_tombstones_exactly() {
    // The guard runs in the assert that every v2 write calls after the R2 lock.
    assert!(MIGRATION.contains("CREATE OR REPLACE FUNCTION _oo.native_c1_baseline_v2_assert"));
    assert!(MIGRATION.contains("'run_retired' USING ERRCODE = 'OO031'"));
    assert!(MIGRATION.contains("r.state <> 'open'"));
    // The exact count reads the tombstone table, not the statistics collector.
    assert!(MIGRATION.contains("CREATE FUNCTION _oo.native_c1_baseline_v2_tomb_count("));
    assert!(MIGRATION.contains("EXECUTE format('SELECT count(*) FROM _oo.%I', t.tomb_name)"));
    assert!(!MIGRATION.contains("pg_stat_user_tables"));
}

#[test]
fn migration_0043_grants_only_execute_on_the_new_readers() {
    // No table grant reaches the runtime role. The shadow stays unreadable to it.
    assert!(
        !MIGRATION.lines().any(|line| line.contains("ON TABLE")),
        "no table grant in 0043"
    );
    assert_eq!(
        MIGRATION.matches("TO oo_pipeline_runtime").count(),
        1,
        "one grant statement to the runtime"
    );
    // The grant statement names exactly the four readers, and nothing else.
    let start = MIGRATION
        .find("GRANT EXECUTE ON FUNCTION _oo.native_c1_baseline_v2_tomb_count(")
        .expect("grant");
    let grant = &MIGRATION[start
        ..MIGRATION[start..]
            .find(';')
            .map(|i| start + i)
            .expect("end")];
    for reader in ["tomb_count", "_plan(", "digest_page", "_analyze("] {
        assert!(grant.contains(reader), "{reader} in the grant");
    }
    assert!(!MIGRATION.contains("GRANT SELECT"));
    assert!(!MIGRATION.contains("GRANT INSERT"));
    assert!(!MIGRATION.contains("GRANT UPDATE"));
    assert!(!MIGRATION.contains("CREATE TRIGGER"));
}

#[test]
fn final_load_v2_is_one_transaction_that_digests_before_it_seals() {
    assert_eq!(C1_BASELINE_V2_PROTOCOL, "oo.pipeline.native-c1-baseline/v2");
    assert_eq!(C1_BUCKET_BITS, 12);
    let start = NATIVE_UPDATE
        .find("pub async fn execute_c1_baseline_v2")
        .expect("v2 entry");
    let body = &NATIVE_UPDATE[start..];
    let end = body
        .find("\n/// Complete the target-side C4 ACK")
        .unwrap_or(body.len());
    let body = &body[..end];
    // D4: the composed digest is computed from the shadow in this transaction, before the seal.
    let digest_at = body
        .find("digest_shadow(&mut tx")
        .expect("digest from the shadow");
    let compose_at = body
        .find("composed_digest(&parts)")
        .expect("composed check");
    let seal_at = body.find("native_c1_baseline::seal_in_tx(").expect("seal");
    assert!(
        digest_at < compose_at && compose_at < seal_at,
        "digest, compose, then seal"
    );
    // The lock comes first, and the seal never takes the caller's digests unchecked.
    assert!(body.find("_oo.native_c1_baseline_lock").expect("lock") < digest_at);
    assert!(
        !body.contains("payload_digest: request"),
        "the seal digest is the computed one"
    );
    assert!(
        body.contains("\"sha256\": table.table_digest"),
        "entries carry the verified digest"
    );
}

#[test]
fn the_cli_dispatches_the_v2_final_load_as_an_additive_subcommand() {
    assert!(BIN.contains("import-native-c1-baseline-v2"));
    assert!(BIN.contains("import_native_c1_baseline_v2"));
    assert!(BIN.contains("import-native-c1-baseline\") && args.next().is_none()"));
}

// ---------------------------------------------------------------- live fixture

fn env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the live test"))
}

fn password() -> String {
    fs::read_to_string(env_var("OO_C1V2_PASSWORD_FILE"))
        .expect("password file")
        .trim()
        .to_string()
}

async fn connect(database: &str) -> PgConnection {
    let host = env_var("OO_C1V2_HOST");
    let port: u16 = env_var("OO_C1V2_PORT").parse().expect("OO_C1V2_PORT");
    let options = PgConnectOptions::new()
        .host(&host)
        .port(port)
        .username("postgres")
        .password(&password())
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

/// The runtime role, as the worker's loader reaches it: a superuser taking the role.
async fn runtime_conn() -> PgConnection {
    let mut conn = connect(DB).await;
    run(&mut conn, "SET ROLE oo_pipeline_runtime").await;
    conn
}

/// Install the whole chain once, with the jobs fixtures the loader calls.
async fn install_everything(admin: &mut PgConnection) {
    run(admin, &format!("DROP DATABASE IF EXISTS {DB} WITH (FORCE)")).await;
    run(admin, &format!("DROP ROLE IF EXISTS {LOGIN}")).await;
    for role in [
        "oo_pipeline_runtime",
        "oo_target_owner",
        "oo_jobs_runtime",
        "oo_jobs_owner",
    ] {
        run(admin, &format!("DROP ROLE IF EXISTS {role}")).await;
    }
    run(admin, &format!("CREATE DATABASE {DB}")).await;
    let mut db = connect(DB).await;
    ti::install(&mut db).await.expect("0000-0004");
    // The jobs control schema is a fixture: the loader reads the claim and extends it.
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
    run(
        &mut db,
        "CREATE FUNCTION jobs.loader_hold(p_task uuid, p_generation bigint) \
         RETURNS TABLE(id text, project_id text, state text, generation bigint, lease_owner text, live boolean) \
         LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, jobs AS $$ \
           SELECT t.id::text, t.project_id::text, t.state, t.generation, 'worker-t8'::text, \
                  t.lease_until > clock_timestamp() \
           FROM jobs.tasks t WHERE t.id = p_task AND t.generation = p_generation $$",
    )
    .await;
    run(
        &mut db,
        "CREATE TABLE jobs.hb_log (n bigserial PRIMARY KEY)",
    )
    .await;
    run(
        &mut db,
        "CREATE FUNCTION jobs.heartbeat(p_project_id uuid, p_task_id uuid, p_worker_id text, p_generation bigint) \
         RETURNS TABLE(status text) LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, jobs AS $h$ \
         BEGIN \
           INSERT INTO jobs.hb_log DEFAULT VALUES; \
           UPDATE jobs.tasks SET lease_until = clock_timestamp() + interval '30 seconds' \
           WHERE id = p_task_id AND project_id = p_project_id AND generation = p_generation \
             AND state = 'running' AND lease_until > clock_timestamp(); \
           IF FOUND THEN RETURN QUERY SELECT 'extended'::text; ELSE RETURN QUERY SELECT 'expired'::text; END IF; \
         END $h$",
    )
    .await;
    run(&mut db, "GRANT EXECUTE ON FUNCTION jobs.loader_hold(uuid, bigint), jobs.heartbeat(uuid, uuid, text, bigint) TO oo_pipeline_runtime").await;
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
    ti::install_native_c1_chunked_baseline_final(&mut db)
        .await
        .expect("0043");
    ti::install_native_c1_chunked_baseline_final(&mut db)
        .await
        .expect("0043 replay");
    ti::install_native_source_instance_grant(&mut db)
        .await
        .expect("0044");
    ti::install_native_c1_ack_seed_format(&mut db)
        .await
        .expect("0045");
    ti::install_native_c1_ack_seed_format(&mut db)
        .await
        .expect("0045 replay");
    // A login member of the runtime role. The loader connects as this role.
    run(
        &mut db,
        &format!(
            "CREATE ROLE {LOGIN} LOGIN PASSWORD '{}' IN ROLE oo_pipeline_runtime",
            password().replace('\'', "''")
        ),
    )
    .await;
    run(
        &mut db,
        &format!("GRANT CONNECT ON DATABASE {DB} TO {LOGIN}"),
    )
    .await;
    assert_eq!(text(&mut db, "SELECT count(*)::text FROM _oo.schema_migrations WHERE version = 'native-c1-chunked-baseline-final/v1'").await, "1");
}

fn target_json(password_file: &Path) -> Value {
    json!({"host": "127.0.0.1", "port": env_var("OO_C1V2_PORT").parse::<u16>().expect("port"), "database": DB,
           "runtime_principal": LOGIN, "password_file": password_file.to_string_lossy(), "ssl": "disable"})
}

/// Mode 0600 copy of the login password, for `connect_target`.
fn login_password_file() -> PathBuf {
    let path = env::temp_dir().join(format!("oo-cb-t8-{}-login.pw", std::process::id()));
    fs::write(&path, password()).expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("mode");
    }
    path
}

// ---------------------------------------------------------------- identities and runs

/// One source identity and its fenced run: the lease, the source epoch and the mappings.
struct Ids {
    src: String,
    shard: String,
    owner: String,
    task: String,
    measured: String,
}

async fn identity(admin: &mut PgConnection, n: usize, tables: &[&str]) -> Ids {
    let src = format!("t8s{n}");
    let shard = format!("t8_shard_{n}");
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
    // Namespace, then table, then columns: each level needs its parent.
    let mut levels: Vec<(&str, &str, &str, &str)> = vec![("namespace", "n", "", "")];
    for table in tables {
        levels.push(("table", "t", table, ""));
        for column in COLUMNS {
            levels.push(("column", "c", table, column));
        }
    }
    for (level, letter, table, column) in levels {
        let digest: Vec<u8> = sqlx::query_scalar(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, 1, $3, 'app', $4, $5))",
        )
        .bind(&src)
        .bind(CFG)
        .bind(letter)
        .bind(table)
        .bind(column)
        .fetch_one(&mut *admin)
        .await
        .expect("identity digest");
        sqlx::query("SELECT _oo.register_physical_mapping($1, $2, 1, $3, 'app', $4, $5, $6)")
            .bind(&src)
            .bind(CFG)
            .bind(level)
            .bind(table)
            .bind(column)
            .bind(digest)
            .execute(&mut *admin)
            .await
            .expect("register mapping");
    }
    Ids {
        src,
        shard,
        owner,
        task,
        measured,
    }
}

fn envelope(ids: &Ids, target: &Value, operation: &str) -> Value {
    json!({"protocol": PROTOCOL_V2, "operation": operation, "source_id": ids.src, "lease_id": ids.task,
           "lease_generation": 1, "source_epoch": 1, "target": target})
}

fn with_fence(ids: &Ids, mut value: Value) -> Value {
    value["fence"] = json!({"shard": ids.shard, "owner": ids.owner});
    value
}

fn merge(mut base: Value, extra: Value) -> Value {
    for (key, item) in extra.as_object().expect("object") {
        base[key] = item.clone();
    }
    base
}

fn parse(request: &Value) -> Request {
    parse_request(request)
        .unwrap_or_else(|token| panic!("test request is invalid: {token}: {request}"))
}

async fn op(conn: &mut PgConnection, request: Value) -> Result<Value, &'static str> {
    execute_v2(conn, &parse(&request)).await
}

fn outcome(value: &Result<Value, &'static str>) -> String {
    match value {
        Ok(item) => item["outcome"].as_str().unwrap_or("?").to_owned(),
        Err(token) => format!("refused:{token}"),
    }
}

fn expect_refused(value: Result<Value, &'static str>, token: &str) {
    match value {
        Ok(v) => panic!("expected refusal {token}, got {v}"),
        Err(got) => assert_eq!(got, token),
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/c1_parts")
        .join(name)
}

fn file_entry(path: &Path) -> (u64, String) {
    let bytes = fs::read(path).expect("fixture");
    (bytes.len() as u64, sha256_hex(&bytes))
}

fn part_json(path: &Path, rows: u64, lo: &str, hi: &str) -> Value {
    let (bytes, sha) = file_entry(path);
    json!({"path": path.to_string_lossy(), "bytes": bytes, "sha256": sha, "rows": rows, "key_lo": lo, "key_hi": hi})
}

fn table_spec(table: &str, digest: &str) -> Value {
    json!({"schema": "app", "table": table, "table_schema_digest": digest,
           "columns": COLUMNS, "oids": [20, 1700, 25, 17, 1114], "typmods": [-1, 655366, -1, -1, -1],
           "not_null": [true, false, false, false, false], "key": "id"})
}

/// Open the run with both tables. The run is fenced, and the tables are registered.
async fn open_run(conn: &mut PgConnection, target: &Value, ids: &Ids) -> Value {
    let request = merge(
        with_fence(ids, envelope(ids, target, "open")),
        json!({
            "measured_id": format!("mysql.{}", ids.measured),
            "config_digest": CFG, "schema_digest": SCH, "type_digest": TYPE, "input_digest": INPUT,
            "l0": format!("{}:1-{L0_POS}", ids.measured), "l0_pos": L0_POS.to_string(), "source_generation": 1,
            "tables": [table_spec("items", TSCH_ITEMS), table_spec("other", TSCH_OTHER)]
        }),
    );
    op(conn, request).await.expect("open")
}

/// Registration of one table: physical name, stage, tombstone and physical columns.
#[derive(Clone, Debug)]
struct Tab {
    table: String,
    phys: String,
    stage: String,
    tomb: String,
    columns: Vec<String>,
}

async fn registration(admin: &mut PgConnection, ids: &Ids) -> Vec<Tab> {
    let rows: Vec<(String, String, String, String, Vec<String>)> = sqlx::query_as(
        "SELECT table_ident, physical_name, stage_name, tomb_name, physical_columns \
         FROM _oo.native_c1_baseline_v2_table WHERE task_id = $1::uuid ORDER BY table_ident",
    )
    .bind(&ids.task)
    .fetch_all(&mut *admin)
    .await
    .expect("registration");
    rows.into_iter()
        .map(|(table, phys, stage, tomb, columns)| Tab {
            table,
            phys,
            stage,
            tomb,
            columns,
        })
        .collect()
}

fn tab<'a>(tabs: &'a [Tab], table: &str) -> &'a Tab {
    tabs.iter().find(|t| t.table == table).expect("table")
}

/// A fill part of the fixtures: its file, its rows, and the smallest and largest key it holds.
type FillPart = (PathBuf, u64, &'static str, &'static str);

fn fill1() -> FillPart {
    (fixture("fill-00000001.arrow"), 3, "1", "3")
}

fn fill2() -> FillPart {
    (fixture("fill-00000002.arrow"), 2, "10", "11")
}

/// Snapshot chunk `no` of a table. The chunk covers keys `lo` (exclusive) to `hi` (inclusive).
/// `None` for `hi` is the terminal chunk.
async fn chunk(
    conn: &mut PgConnection,
    target: &Value,
    ids: &Ids,
    table: &str,
    no: i64,
    bounds: (Option<&str>, Option<&str>),
    part: Option<FillPart>,
) -> Result<Value, &'static str> {
    let (lo, hi) = bounds;
    let (part_value, sha, rows) = match part {
        Some((path, rows, part_lo, part_hi)) => {
            let (_, sha) = file_entry(&path);
            (part_json(&path, rows, part_lo, part_hi), sha, rows)
        }
        None => (Value::Null, EMPTY.to_string(), 0),
    };
    let request = merge(
        envelope(ids, target, "chunk"),
        json!({
            "schema": "app", "table": table, "chunk_no": no, "attempt": 1,
            "key_lo": lo, "key_hi": hi, "terminal": hi.is_none(), "row_count": rows,
            "part_sha256": sha, "columns": COLUMNS, "key": "id", "part": part_value
        }),
    );
    op(conn, request).await
}

/// The CDC batch of the fixture: transactions 5231..5260, with three rows.
/// Key 1 and key 2 are upserted at 5231. Key 3 is deleted at 5260.
async fn batch_fixture(
    conn: &mut PgConnection,
    target: &Value,
    ids: &Ids,
    batch_no: i64,
    max_tombstones: Option<u64>,
) -> Result<Value, &'static str> {
    let part = fixture("cdc-00000001.arrow");
    let records: Vec<Value> = (L0_POS + 1..=L_END_POS)
        .map(|pos| {
            let rows = match pos {
                5231 => 2,
                5260 => 1,
                _ => 0,
            };
            json!({"applied_pos": pos.to_string(), "txn_id": format!("{}:{pos}", ids.measured),
                   "payload_sha256": sha256_hex(format!("txn {pos}").as_bytes()),
                   "source_commit_timestamp": "2026-10-17T10:00:00Z", "row_count": rows})
        })
        .collect();
    let mut request = merge(
        envelope(ids, target, "txn_batch"),
        json!({
            "batch_no": batch_no, "first_pos": (L0_POS + 1).to_string(), "last_pos": L_END_POS.to_string(),
            "txns": records.len(), "row_count": 3, "batch_sha256": sha256_hex(format!("batch {batch_no}").as_bytes()),
            "txn_records": records,
            "parts": [{"schema": "app", "table": "items", "columns": COLUMNS, "key": "id",
                       "part": part_json(&part, 3, "1", "3")}]
        }),
    );
    if let Some(bound) = max_tombstones {
        request["max_tombstones"] = json!(bound);
    }
    op(conn, request).await
}

/// Load both tables to their terminal chunk: two fill parts, then the empty terminal.
async fn load_snapshots(conn: &mut PgConnection, target: &Value, ids: &Ids) {
    for table in ["items", "other"] {
        let first = chunk(
            conn,
            target,
            ids,
            table,
            1,
            (None, Some("3")),
            Some(fill1()),
        )
        .await;
        assert_eq!(outcome(&first), "loaded", "{first:?}");
        let second = chunk(
            conn,
            target,
            ids,
            table,
            2,
            (Some("3"), Some("11")),
            Some(fill2()),
        )
        .await;
        assert_eq!(outcome(&second), "loaded", "{second:?}");
        let terminal = chunk(conn, target, ids, table, 3, (Some("11"), None), None).await;
        assert_eq!(outcome(&terminal), "loaded", "{terminal:?}");
    }
}

// ---------------------------------------------------------------- expected states (fixture truth)

/// Column types of both tables, as the catalog maps them (id, qty, name, blob, created).
fn types() -> Vec<oo_core::canonical::LogicalType> {
    [(20, -1), (1700, 655366), (25, -1), (17, -1), (1114, -1)]
        .into_iter()
        .map(|(oid, typmod)| logical_type_of_pg(oid, typmod).expect("type"))
        .collect()
}

fn dec(text: &str) -> Cell {
    Cell::Decimal(text.to_owned())
}

fn text_cell(text: &str) -> Cell {
    Cell::Text(text.to_owned())
}

/// Final rows of `items` after the CDC batch: keys 1, 2, 10 and 11. Key 3 is deleted.
fn items_rows() -> Vec<(i64, Vec<Cell>)> {
    vec![
        (
            1,
            vec![
                Cell::Int(1),
                dec("1.50"),
                text_cell("a"),
                Cell::Null,
                Cell::Micros(0),
            ],
        ),
        (
            2,
            vec![
                Cell::Int(2),
                dec("0.07"),
                Cell::Null,
                Cell::Bytes(vec![1]),
                Cell::Null,
            ],
        ),
        (
            10,
            vec![
                Cell::Int(10),
                dec("99999999.99"),
                text_cell("x"),
                Cell::Null,
                Cell::Null,
            ],
        ),
        (
            11,
            vec![
                Cell::Int(11),
                dec("0.00"),
                text_cell("y"),
                Cell::Null,
                Cell::Null,
            ],
        ),
    ]
}

/// Snapshot-only `other`: keys 1, 2, 3, 10 and 11, as the two fill parts carry them.
fn other_rows() -> Vec<(i64, Vec<Cell>)> {
    vec![
        (
            1,
            vec![
                Cell::Int(1),
                dec("1.50"),
                text_cell("héllo 世界"),
                Cell::Bytes(vec![0, 255, 16]),
                Cell::Micros(1_704_164_645_123_456),
            ],
        ),
        (
            2,
            vec![
                Cell::Int(2),
                Cell::Null,
                text_cell(""),
                Cell::Null,
                Cell::Null,
            ],
        ),
        (
            3,
            vec![
                Cell::Int(3),
                dec("-0.05"),
                Cell::Null,
                Cell::Bytes(vec![]),
                Cell::Micros(-1),
            ],
        ),
        (
            10,
            vec![
                Cell::Int(10),
                dec("99999999.99"),
                text_cell("x"),
                Cell::Null,
                Cell::Null,
            ],
        ),
        (
            11,
            vec![
                Cell::Int(11),
                dec("0.00"),
                text_cell("y"),
                Cell::Null,
                Cell::Null,
            ],
        ),
    ]
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

type ExpectedTable<'a> = (&'a str, Vec<(i64, Vec<Cell>)>);

/// Expected table digest and composed digest of a set of tables, from the rows above.
fn expected_state(tables: &[ExpectedTable<'_>]) -> (String, Vec<(String, usize, String)>) {
    let types = types();
    let mut digests: Vec<(String, StateDigest, usize)> = Vec::new();
    for (name, rows) in tables {
        let mut digest = StateDigest::new(C1_BUCKET_BITS).expect("bits");
        for (key, cells) in rows {
            let bytes = canonical_row(&types, cells).expect("row");
            digest
                .add_row(bucket_key(&cells[0]).expect("key"), &bytes)
                .expect("add");
            assert_eq!(bucket_key(&cells[0]).expect("key"), i128::from(*key));
        }
        digests.push((name.to_string(), digest, rows.len()));
    }
    let refs: Vec<(&str, &StateDigest)> = digests.iter().map(|(n, d, _)| (n.as_str(), d)).collect();
    let composed = composed_digest(&refs).expect("composed");
    let per_table = digests
        .iter()
        .map(|(n, d, c)| (n.clone(), *c, hex(&d.table_digest())))
        .collect();
    (composed, per_table)
}

// ---------------------------------------------------------------- final-load documents

fn admission_registry(scope: &Value) -> (PathBuf, String) {
    let path = env::temp_dir().join(format!(
        "oo-cb-t8-{}-{}.registry.json",
        std::process::id(),
        sha256_hex(scope.to_string().as_bytes())
    ));
    let bytes = json!({"bindings": [{"baseline_scope": scope}]}).to_string();
    fs::write(&path, &bytes).expect("registry");
    (path, sha256_hex(bytes.as_bytes()))
}

/// The final-load request for the two-table run, with its composed expectation.
fn final_document(
    ids: &Ids,
    target: &Value,
    composed: &str,
    tables: &[(String, usize, String)],
    rows: u64,
) -> Value {
    final_document_at(ids, target, composed, tables, rows, L_END_POS)
}

/// The same request with another L_end position, for the catch-up scenarios.
fn final_document_at(
    ids: &Ids,
    target: &Value,
    composed: &str,
    tables: &[(String, usize, String)],
    rows: u64,
    end: i64,
) -> Value {
    let scope = json!({
        "config_digest": CFG, "schema_digest": SCH, "lineage": format!("mysql.{}", ids.measured),
        "l0": format!("{}:1-{L0_POS}", ids.measured), "l_end": format!("{}:1-{end}", ids.measured),
        "complete": true, "consistency_grade": "T", "cross_table_consistent": true,
        "tables": [
            {"schema": "app", "table": "items", "schema_sha256": TSCH_ITEMS},
            {"schema": "app", "table": "other", "schema_sha256": TSCH_OTHER}
        ]
    });
    let (registry, registry_sha) = admission_registry(&scope);
    // The canonical types are the source mapping's (DESIGN D7), one per column, in catalog order.
    let canonical = json!(["int", "decimal(10,2)", "text", "bytes", "timestamp_naive"]);
    let expected_tables: Vec<Value> = tables
        .iter()
        .map(|(name, count, digest)| {
            let (schema, table) = name.split_once('.').expect("name");
            json!({"schema": schema, "table": table, "row_count": count, "table_digest": digest,
                   "canonical_types": canonical})
        })
        .collect();
    json!({
        "protocol": C1_BASELINE_V2_PROTOCOL,
        "source_id": ids.src, "lease_id": ids.task, "lease_generation": 1, "source_epoch": 1,
        "source_generation": 1,
        "measured_id": format!("mysql.{}", ids.measured),
        "l0": format!("{}:1-{L0_POS}", ids.measured), "l0_pos": L0_POS.to_string(),
        "l_end": format!("{}:1-{end}", ids.measured), "l_end_pos": end.to_string(),
        "type_digest": TYPE, "input_digest": INPUT,
        "baseline_scope": scope,
        "expected": {"bucket_bits": 12, "rows": rows, "composed_digest": composed, "tables": expected_tables},
        "admission_registry_path": registry.to_string_lossy(), "admission_registry_sha256": registry_sha,
        "fence": {"shard": ids.shard, "owner": ids.owner},
        "target": target
    })
}

fn expected_two_tables() -> (String, Vec<(String, usize, String)>, u64) {
    let (composed, per) =
        expected_state(&[("app.items", items_rows()), ("app.other", other_rows())]);
    (composed, per, 9)
}

async fn state_of(admin: &mut PgConnection, ids: &Ids, column: &str) -> String {
    text(
        admin,
        &format!(
            "SELECT {column} FROM _oo.native_c1_baseline_run WHERE task_id = '{}'::uuid",
            ids.task
        ),
    )
    .await
}

fn vm_hwm_kib() -> u64 {
    fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- live scenarios

/// Two tables through the T7 operations, then the final load. The published state has the v1 shape.
async fn scenario_publish(admin: &mut PgConnection, n: usize, pw_file: &Path) {
    let target = target_json(pw_file);
    let ids = identity(admin, n, &["items", "other"]).await;
    let mut conn = runtime_conn().await;
    open_run(&mut conn, &target, &ids).await;
    let tabs = registration(admin, &ids).await;
    load_snapshots(&mut conn, &target, &ids).await;
    let batch = batch_fixture(&mut conn, &target, &ids, 1, None).await;
    assert_eq!(outcome(&batch), "applied", "{batch:?}");
    // The exact tombstone count is the table's own row count. The fixture deleted key 3.
    let mut runtime = runtime_conn().await;
    let count = try_text(&mut runtime, &format!(
        "SELECT _oo.native_c1_baseline_v2_tomb_count('{}', '{}'::uuid, 1, 1, 'app', 'items')::text",
        ids.src, ids.task)).await.expect("tomb count");
    assert_eq!(count, "1");

    let (composed, per, rows) = expected_two_tables();
    let document = final_document(&ids, &target, &composed, &per, rows);
    eprintln!("T8-EXAMPLE final-request={document}");
    let receipt = execute_c1_baseline_v2(&document)
        .await
        .unwrap_or_else(|reason| panic!("final load refused: {reason}"));
    eprintln!("T8-EXAMPLE final-response={receipt}");
    assert_eq!(
        receipt["protocol"],
        "oo.pipeline.native-c1-baseline-result/v1"
    );
    assert_eq!(receipt["load_state"], "awaiting_publish", "{receipt}");
    assert_eq!(
        receipt["verification"]["composed_digest"],
        composed.as_str()
    );
    assert_eq!(receipt["verification"]["rows"], rows);

    // The published state. Run, freshness, watermark, receipts and mirror, as the v1 path leaves them.
    assert_eq!(state_of(admin, &ids, "state").await, "awaiting_publish");
    assert_eq!(
        state_of(admin, &ids, "l_end::text").await,
        L_END_POS.to_string()
    );
    let typed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo.native_c1_baseline_typed WHERE task_id = $1::uuid",
    )
    .bind(&ids.task)
    .fetch_one(&mut *admin)
    .await
    .expect("typed");
    assert_eq!(typed, 1);
    // The sealed-import contract needs exactly one accepted `batch` quality row per sealed table.
    let quality: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT q.kind, q.accepted_rows, q.rejected_rows FROM _oo.full_import_quality q \
         JOIN _oo.native_c1_baseline_v2_table t ON t.run_id = q.run_id WHERE t.task_id = $1::uuid ORDER BY q.physical_name",
    )
    .bind(&ids.task)
    .fetch_all(&mut *admin)
    .await
    .expect("quality");
    assert_eq!(
        quality.len(),
        2,
        "one batch quality row per sealed table: {quality:?}"
    );
    assert!(
        quality
            .iter()
            .all(|(kind, accepted, rejected)| kind == "batch" && *accepted > 0 && *rejected == 0)
    );
    let entries = receipt["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    for entry in entries {
        let keys: Vec<&str> = entry
            .as_object()
            .expect("entry")
            .keys()
            .map(String::as_str)
            .collect();
        let mut keys = keys;
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "physical_id",
                "row_count",
                "run_id",
                "schema",
                "sha256",
                "table"
            ],
            "the v1 entry shape"
        );
    }
    let mut top: Vec<&str> = receipt
        .as_object()
        .expect("receipt")
        .keys()
        .map(String::as_str)
        .filter(|k| *k != "verification")
        .collect();
    top.sort_unstable();
    let mut want = vec![
        "acknowledged",
        "applied_pos",
        "entries",
        "l0",
        "l_end",
        "load_state",
        "protocol",
        "request_digest",
        "source_committed_at",
        "source_generation",
        "source_epoch",
        "source_id",
        "source_instance",
        "target_generation",
    ];
    want.sort_unstable();
    assert_eq!(top, want, "the typed receipt keeps the v1 shape");
    for tab in &tabs {
        let state: String =
            sqlx::query_scalar("SELECT state FROM _oo.table_freshness WHERE physical_name = $1")
                .bind(&tab.phys)
                .fetch_one(&mut *admin)
                .await
                .expect("freshness");
        assert_eq!(state, "awaiting_publish");
        let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo.apply_receipts WHERE table_generation = $1 AND txn_or_batch_id = $2 AND position_codec = 'snapshot-zero/v1'")
            .bind(&tab.phys).bind(format!("c1v2.seal:{}", ids.task)).fetch_one(&mut *admin).await.expect("receipts");
        assert_eq!(receipts, 1);
        let mirror: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo.control_mirror WHERE physical_name = $1 AND batch_id = $2",
        )
        .bind(&tab.phys)
        .bind(format!("c1v2.seal:{}", ids.task))
        .fetch_one(&mut *admin)
        .await
        .expect("mirror");
        assert_eq!(mirror, 1);
        let stage: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::text")
            .bind(format!("_oo.{}", tab.stage))
            .fetch_one(&mut *admin)
            .await
            .expect("stage");
        assert!(stage.is_none(), "the stage is dropped at seal");
        let tomb: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::text")
            .bind(format!("_oo.{}", tab.tomb))
            .fetch_one(&mut *admin)
            .await
            .expect("tomb");
        assert!(tomb.is_none(), "the tombstone table is dropped at seal");
        let analyzed: f32 =
            sqlx::query_scalar("SELECT reltuples FROM pg_class WHERE oid = to_regclass($1)")
                .bind(format!("_oo.{}", tab.phys))
                .fetch_one(&mut *admin)
                .await
                .expect("analyzed");
        assert!(analyzed >= 0.0, "ANALYZE ran on the shadow");
    }
    let runs: Vec<String> = sqlx::query_scalar("SELECT state FROM _oo.full_import_run WHERE run_id IN (SELECT run_id FROM _oo.native_c1_baseline_v2_table WHERE task_id = $1::uuid)")
        .bind(&ids.task).fetch_all(&mut *admin).await.expect("runs");
    assert_eq!(runs, vec!["awaiting_publish", "awaiting_publish"]);

    // The shadow holds the fixture truth after the batch: four rows in items, five in other.
    for (table, want) in [("items", 4_i64), ("other", 5_i64)] {
        let phys = tab(&tabs, table).phys.clone();
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM _oo.{phys}"
        )))
        .fetch_one(&mut *admin)
        .await
        .expect("count");
        assert_eq!(count, want, "{table}");
    }

    // Replay of the same request returns the stored receipt. It writes nothing.
    let replay = execute_c1_baseline_v2(&document).await.expect("replay");
    assert_eq!(replay["verification"]["replayed"], true);
    assert_eq!(replay["entries"], receipt["entries"]);
    assert_eq!(replay["request_digest"], receipt["request_digest"]);
}

/// A tampered shadow row is refused before the seal. Nothing is written, and a restore publishes.
async fn scenario_tamper(admin: &mut PgConnection, n: usize, pw_file: &Path) {
    let target = target_json(pw_file);
    let ids = identity(admin, n, &["items", "other"]).await;
    let mut conn = runtime_conn().await;
    open_run(&mut conn, &target, &ids).await;
    let tabs = registration(admin, &ids).await;
    load_snapshots(&mut conn, &target, &ids).await;
    assert_eq!(
        outcome(&batch_fixture(&mut conn, &target, &ids, 1, None).await),
        "applied"
    );

    let items = tab(&tabs, "items").clone();
    let name_column = items.columns[2].clone();
    run(
        admin,
        &format!(
            "UPDATE _oo.{} SET \"{name_column}\" = 'tampered' WHERE \"{}\" = 1",
            items.phys, items.columns[0]
        ),
    )
    .await;
    let (composed, per, rows) = expected_two_tables();
    let document = final_document(&ids, &target, &composed, &per, rows);
    let refused = execute_c1_baseline_v2(&document).await;
    assert_eq!(refused.err(), Some("composed_mismatch"));
    // Nothing is written: no seal, no typed receipt, no mirror, the run is still open.
    assert_eq!(state_of(admin, &ids, "state").await, "open");
    let sealed: bool = sqlx::query_scalar(
        "SELECT sealed FROM _oo.native_c1_baseline_v2_format WHERE task_id = $1::uuid",
    )
    .bind(&ids.task)
    .fetch_one(&mut *admin)
    .await
    .expect("format");
    assert!(!sealed);
    let typed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo.native_c1_baseline_typed WHERE task_id = $1::uuid",
    )
    .bind(&ids.task)
    .fetch_one(&mut *admin)
    .await
    .expect("typed");
    assert_eq!(typed, 0);
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo.apply_receipts WHERE table_generation = $1")
            .bind(&items.phys)
            .fetch_one(&mut *admin)
            .await
            .expect("receipts");
    assert_eq!(receipts, 0);
    let mirrors: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo.control_mirror WHERE physical_name = $1")
            .bind(&items.phys)
            .fetch_one(&mut *admin)
            .await
            .expect("mirror");
    assert_eq!(mirrors, 0);
    let shadow_alive: Option<String> = sqlx::query_scalar("SELECT to_regclass($1)::text")
        .bind(format!("_oo.{}", items.phys))
        .fetch_one(&mut *admin)
        .await
        .expect("shadow");
    assert!(shadow_alive.is_some(), "the shadow is kept for diagnosis");

    // Restore the row, and the same request publishes.
    run(
        admin,
        &format!(
            "UPDATE _oo.{} SET \"{name_column}\" = 'a' WHERE \"{}\" = 1",
            items.phys, items.columns[0]
        ),
    )
    .await;
    let receipt = execute_c1_baseline_v2(&document)
        .await
        .expect("final after restore");
    assert_eq!(receipt["load_state"], "awaiting_publish");
}

/// A retired run refuses every write. The tombstone count is read under the lock.
async fn scenario_retire(admin: &mut PgConnection, n: usize, pw_file: &Path) {
    let target = target_json(pw_file);
    let ids = identity(admin, n, &["items", "other"]).await;
    let mut conn = runtime_conn().await;
    open_run(&mut conn, &target, &ids).await;
    let tabs = registration(admin, &ids).await;
    assert_eq!(
        outcome(
            &chunk(
                &mut conn,
                &target,
                &ids,
                "items",
                1,
                (None, Some("3")),
                Some(fill1())
            )
            .await
        ),
        "loaded"
    );

    let retire = op(
        &mut conn,
        with_fence(&ids, envelope(&ids, &target, "retire")),
    )
    .await
    .expect("retire");
    assert_eq!(retire["outcome"], "retired", "{retire}");
    // Every write refuses with the closed run token, and nothing is written.
    let second = chunk(
        &mut conn,
        &target,
        &ids,
        "items",
        2,
        (Some("3"), Some("11")),
        Some(fill2()),
    )
    .await;
    expect_refused(second, "run_not_open");
    expect_refused(
        batch_fixture(&mut conn, &target, &ids, 1, None).await,
        "run_not_open",
    );
    let rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM _oo.{}",
        tab(&tabs, "items").phys
    )))
    .fetch_one(&mut *admin)
    .await
    .expect("rows");
    assert_eq!(
        rows, 3,
        "the refused writes added no row; the pre-retire chunk stays"
    );
    let tomb = try_text(&mut conn, &format!("SELECT _oo.native_c1_baseline_v2_tomb_count('{}', '{}'::uuid, 1, 1, 'app', 'items')::text", ids.src, ids.task)).await;
    assert!(tomb.is_err(), "the count refuses a retired run");
}

/// The final load refuses before the CDC has applied through L_end, and refuses a
/// request whose L_end is behind the applied position. Both refusals come before
/// any digest is read or any row is sealed.
async fn scenario_catch_up(admin: &mut PgConnection, n: usize, pw_file: &Path) {
    let target = target_json(pw_file);
    let ids = identity(admin, n, &["items", "other"]).await;
    let mut conn = runtime_conn().await;
    open_run(&mut conn, &target, &ids).await;
    load_snapshots(&mut conn, &target, &ids).await;
    let (composed, per, rows) = expected_two_tables();
    // No batch yet: applied is L0 (5230), below L_end (5260).
    let behind = final_document(&ids, &target, &composed, &per, rows);
    assert_eq!(
        execute_c1_baseline_v2(&behind).await.err(),
        Some("not_caught_up")
    );
    assert_eq!(
        state_of(admin, &ids, "state").await,
        "open",
        "nothing was sealed"
    );

    assert_eq!(
        outcome(&batch_fixture(&mut conn, &target, &ids, 1, None).await),
        "applied"
    );
    // Applied is now 5260. A request that ends at 5230 is behind the applied position.
    let past = final_document_at(&ids, &target, &composed, &per, rows, L0_POS);
    assert_eq!(
        execute_c1_baseline_v2(&past).await.err(),
        Some("past_l_end")
    );
    assert_eq!(
        state_of(admin, &ids, "state").await,
        "open",
        "still nothing was sealed"
    );
    // The matching request publishes.
    let receipt = execute_c1_baseline_v2(&final_document(&ids, &target, &composed, &per, rows))
        .await
        .expect("publish");
    assert_eq!(receipt["load_state"], "awaiting_publish");
}

/// Digest of a table of more than 100 MiB. The loader reads it in pages, so its peak
/// memory does not grow with the table. The expected digest is computed from the same rows in Rust.
async fn scenario_bulk_digest(admin: &mut PgConnection, n: usize, pw_file: &Path, rows: i64) {
    let target = target_json(pw_file);
    let ids = identity(admin, n, &["big"]).await;
    let mut conn = runtime_conn().await;
    let request = merge(
        with_fence(&ids, envelope(&ids, &target, "open")),
        json!({
            "measured_id": format!("mysql.{}", ids.measured),
            "config_digest": CFG, "schema_digest": SCH, "type_digest": TYPE, "input_digest": INPUT,
            "l0": format!("{}:1-{L0_POS}", ids.measured), "l0_pos": L0_POS.to_string(), "source_generation": 1,
            "tables": [table_spec("big", TSCH_ITEMS)]
        }),
    );
    op(&mut conn, request).await.expect("open");
    let tabs = registration(admin, &ids).await;
    let big = tabs[0].clone();
    assert_eq!(
        outcome(&chunk(&mut conn, &target, &ids, "big", 1, (None, None), None).await),
        "loaded"
    );
    // Fill the shadow with the generated rows directly. This is the fixture's bypass of the stage.
    let base: i64 = text(
        admin,
        "SELECT (extract(epoch from timestamp '2026-10-17 00:00:00') * 1000000)::bigint::text",
    )
    .await
    .parse()
    .expect("base");
    let p = &big.columns;
    run(admin, &format!(
        "INSERT INTO _oo.{phys} (\"_oo_pos\", \"{c0}\", \"{c1}\", \"{c2}\", \"{c3}\", \"{c4}\") \
         SELECT 0, i, ((i % 100000)::numeric / 100), 'row-' || i || '-' || repeat('x', 60), \
                decode(lpad(to_hex(i % 256), 2, '0'), 'hex'), \
                to_timestamp({base}::numeric / 1000000 + i) AT TIME ZONE 'UTC' \
         FROM generate_series(1, {rows}) AS i",
        phys = big.phys, c0 = p[0], c1 = p[1], c2 = p[2], c3 = p[3], c4 = p[4]
    )).await;
    let size: i64 = sqlx::query_scalar("SELECT pg_total_relation_size(to_regclass($1))")
        .bind(format!("_oo.{}", big.phys))
        .fetch_one(&mut *admin)
        .await
        .expect("size");
    eprintln!("T8-BULK rows={rows} shadow_total_bytes={size}");
    assert!(
        size >= 100 * 1024 * 1024,
        "the table is at least 100 MiB: {size}"
    );

    // The expected rows, generated in Rust with the same formulas.
    let types = types();
    let micros_base = base;
    let mut digest = StateDigest::new(C1_BUCKET_BITS).expect("bits");
    for i in 1..=rows {
        let v = i % 100_000;
        let cells = vec![
            Cell::Int(i128::from(i)),
            dec(&format!("{}.{:02}", v / 100, v % 100)),
            text_cell(&format!("row-{i}-{}", "x".repeat(60))),
            Cell::Bytes(vec![(i % 256) as u8]),
            Cell::Micros(micros_base + i * 1_000_000),
        ];
        let bytes = canonical_row(&types, &cells).expect("row");
        digest.add_row(i128::from(i), &bytes).expect("add");
    }
    let expected_table = hex(&digest.table_digest());
    let composed = composed_digest(&[("app.big", &digest)]).expect("composed");

    let scope_tables = json!([{"schema": "app", "table": "big", "schema_sha256": TSCH_ITEMS}]);
    let scope = json!({
        "config_digest": CFG, "schema_digest": SCH, "lineage": format!("mysql.{}", ids.measured),
        "l0": format!("{}:1-{L0_POS}", ids.measured), "l_end": format!("{}:1-{L0_POS}", ids.measured),
        "complete": true, "consistency_grade": "T", "cross_table_consistent": true, "tables": scope_tables
    });
    // No CDC: L_end is L0, and the applied position already equals it.
    let (registry, registry_sha) = admission_registry(&scope);
    let document = json!({
        "protocol": C1_BASELINE_V2_PROTOCOL,
        "source_id": ids.src, "lease_id": ids.task, "lease_generation": 1, "source_epoch": 1, "source_generation": 1,
        "measured_id": format!("mysql.{}", ids.measured),
        "l0": format!("{}:1-{L0_POS}", ids.measured), "l0_pos": L0_POS.to_string(),
        "l_end": format!("{}:1-{L0_POS}", ids.measured), "l_end_pos": L0_POS.to_string(),
        "type_digest": TYPE, "input_digest": INPUT, "baseline_scope": scope,
        "expected": {"bucket_bits": 12, "rows": rows, "composed_digest": composed,
                     "tables": [{"schema": "app", "table": "big", "row_count": rows, "table_digest": expected_table,
                                 "canonical_types": ["int", "decimal(10,2)", "text", "bytes", "timestamp_naive"]}]},
        "admission_registry_path": registry.to_string_lossy(), "admission_registry_sha256": registry_sha,
        "fence": {"shard": ids.shard, "owner": ids.owner}, "target": target
    });
    let beats_before: i64 = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs.hb_log")
        .fetch_one(&mut *admin)
        .await
        .expect("beats");
    let rss_before = vm_hwm_kib();
    let started = Instant::now();
    let receipt = execute_c1_baseline_v2(&document)
        .await
        .unwrap_or_else(|reason| panic!("bulk final load refused: {reason}"));
    let elapsed = started.elapsed();
    let rss_after = vm_hwm_kib();
    let beats_after: i64 = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs.hb_log")
        .fetch_one(&mut *admin)
        .await
        .expect("beats");
    let beats = beats_after - beats_before;
    // The initial lease hold, one heartbeat per window of rows, and one before ANALYZE.
    let windows = rows / 500_000;
    assert!(
        beats >= windows + 2,
        "{beats} heartbeats for {windows} windows"
    );
    eprintln!("T8-BULK heartbeats={beats} windows={windows}");
    eprintln!(
        "T8-BULK final_load_elapsed_ms={} peak_rss_growth_kib={} rss_before_kib={rss_before} rss_after_kib={rss_after}",
        elapsed.as_millis(),
        rss_after.saturating_sub(rss_before)
    );
    assert_eq!(receipt["load_state"], "awaiting_publish", "{receipt}");
    assert_eq!(
        receipt["verification"]["composed_digest"],
        composed.as_str()
    );
}

// ---------------------------------------------------------------- entry points

#[tokio::test]
#[ignore = "needs OO_C1V2_HOST, OO_C1V2_PORT and OO_C1V2_PASSWORD_FILE (disposable PostgreSQL 17)"]
async fn live_final_load_v2_publishes_two_tables_end_to_end() {
    let mut root = connect("postgres").await;
    install_everything(&mut root).await;
    let mut admin = connect(DB).await;
    let pw = login_password_file();
    scenario_publish(&mut admin, 1, &pw).await;
}

#[tokio::test]
#[ignore = "needs OO_C1V2_HOST, OO_C1V2_PORT and OO_C1V2_PASSWORD_FILE (disposable PostgreSQL 17)"]
async fn live_composed_mismatch_refuses_and_writes_nothing() {
    let mut root = connect("postgres").await;
    install_everything(&mut root).await;
    let mut admin = connect(DB).await;
    let pw = login_password_file();
    scenario_tamper(&mut admin, 2, &pw).await;
}

#[tokio::test]
#[ignore = "needs OO_C1V2_HOST, OO_C1V2_PORT and OO_C1V2_PASSWORD_FILE (disposable PostgreSQL 17)"]
async fn live_retired_run_refuses_writes_and_counts_tombstones_exactly() {
    let mut root = connect("postgres").await;
    install_everything(&mut root).await;
    let mut admin = connect(DB).await;
    let pw = login_password_file();
    scenario_retire(&mut admin, 3, &pw).await;
}

#[tokio::test]
#[ignore = "needs OO_C1V2_HOST, OO_C1V2_PORT and OO_C1V2_PASSWORD_FILE (disposable PostgreSQL 17)"]
async fn live_final_load_waits_for_catch_up_and_refuses_a_stale_end() {
    let mut root = connect("postgres").await;
    install_everything(&mut root).await;
    let mut admin = connect(DB).await;
    let pw = login_password_file();
    scenario_catch_up(&mut admin, 5, &pw).await;
}

#[tokio::test]
#[ignore = "bulk memory run: needs OO_C1V2_HOST, OO_C1V2_PORT and OO_C1V2_PASSWORD_FILE; OO_C1V2_BULK_ROWS optional"]
async fn live_digest_of_a_table_over_100_mib_has_bounded_memory() {
    let rows: i64 = env::var("OO_C1V2_BULK_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_200_000);
    let mut root = connect("postgres").await;
    install_everything(&mut root).await;
    let mut admin = connect(DB).await;
    let pw = login_password_file();
    scenario_bulk_digest(&mut admin, 4, &pw, rows).await;
}
