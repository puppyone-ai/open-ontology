//! Integration test for the chunked-typed/v2 worker driver (T9).
//!
//! It runs the REAL producer (`mysql-preview c1-baseline-stream-v2`) against an owned
//! MySQL 5.7 fixture, and the REAL loader (`oo-pipeline native-c1-baseline-stage-v2`)
//! against an owned PostgreSQL 17 with the migration chain installed. Source writes
//! run through the `mysql` client while the baseline is in flight.
//!
//! The tests are `#[ignore]`d. They need the environment described in
//! `tests/fixtures/c1_v2_driver/README.md`. Run them with
//! `cargo test --test c1_v2_driver_live -- --ignored --test-threads=1`.
//!
//! The typing of the fixture tables comes from the test (`Typing::Fixed`). The
//! producer's `open` frame does not carry target typing yet (see the report).

use job_worker::native_schedule::Executable;
use job_worker::native_schedule::native_schedule_v2::{
    Done, Fault, OpenFacts, Plan, Port, Stage, Typed, Typing, run,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

const DB: &str = "oo_cb_t9";
const ITEMS_ROWS: u64 = 1024;
const CHUNK_ROWS: u64 = 200;
const LARGE_ROWS: u64 = 40_000;

#[derive(Clone)]
struct Fx {
    mysql_host: String,
    mysql_port: u16,
    mysql_cli: String,
    mysql_root_pw: String,
    writer_pw: String,
    cdc_pw_file: PathBuf,
    ca: PathBuf,
    tls_name: String,
    pg_host: String,
    pg_port: u16,
    pg_db: String,
    pg_super_pw: String,
    runtime_pw_file: PathBuf,
    producer_bin: PathBuf,
    loader_bin: PathBuf,
    work: PathBuf,
}

fn env_var(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("set {name} (see tests/fixtures/c1_v2_driver/README.md)"))
}

fn secret(path: &str) -> String {
    std::fs::read_to_string(path)
        .expect("secret file")
        .trim()
        .to_owned()
}

fn fixture() -> Fx {
    let work = PathBuf::from(env_var("OO_CB_T9_WORK"));
    std::fs::create_dir_all(&work).expect("work dir");
    Fx {
        mysql_host: env_var("OO_CB_T9_MYSQL_HOST"),
        mysql_port: env_var("OO_CB_T9_MYSQL_PORT").parse().expect("port"),
        mysql_cli: std::env::var("OO_CB_T9_MYSQL_CLI").unwrap_or_else(|_| "mysql".into()),
        mysql_root_pw: secret(&env_var("OO_CB_T9_MYSQL_ROOT_PW_FILE")),
        writer_pw: secret(&env_var("OO_CB_T9_MYSQL_WRITER_PW_FILE")),
        cdc_pw_file: PathBuf::from(env_var("OO_CB_T9_MYSQL_CDC_PW_FILE")),
        ca: PathBuf::from(env_var("OO_CB_T9_CA")),
        tls_name: env_var("OO_CB_T9_TLS_NAME"),
        pg_host: env_var("OO_CB_T9_PG_HOST"),
        pg_port: env_var("OO_CB_T9_PG_PORT").parse().expect("port"),
        pg_db: env_var("OO_CB_T9_PG_DATABASE"),
        pg_super_pw: secret(&env_var("OO_CB_T9_PG_SUPER_PW_FILE")),
        runtime_pw_file: PathBuf::from(env_var("OO_CB_T9_PG_RUNTIME_PW_FILE")),
        producer_bin: PathBuf::from(env_var("OO_CB_T9_PRODUCER_BIN")),
        loader_bin: PathBuf::from(env_var("OO_CB_T9_LOADER_BIN")),
        work,
    }
}

/// Runs one SQL batch on the source. The password goes through the environment, not argv.
fn mysql(fx: &Fx, user: &str, password: &str, sql: &str) -> String {
    let out = std::process::Command::new(&fx.mysql_cli)
        .env("MYSQL_PWD", password)
        .args([
            "--skip-ssl",
            "-h",
            &fx.mysql_host,
            "-P",
            &fx.mysql_port.to_string(),
            "-u",
            user,
            "-D",
            DB,
            "-N",
            "-B",
            "-e",
            sql,
        ])
        .stdin(Stdio::null())
        .output()
        .expect("mysql client");
    assert!(
        out.status.success(),
        "mysql failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8")
}

fn source_write(fx: &Fx, sql: &str) {
    mysql(fx, "oo_writer", &fx.writer_pw, sql);
}

/// Creates the two fixture tables and fills `items` with `ITEMS_ROWS` rows.
fn seed(fx: &Fx) {
    let root = fx.mysql_root_pw.clone();
    mysql(
        fx,
        "root",
        &root,
        "DROP TABLE IF EXISTS items; DROP TABLE IF EXISTS small;",
    );
    // Signed BIGINT keys. BIGINT UNSIGNED maps to numeric(20,0) and its parts carry a decimal
    // key cell, which the loader's chunk path refuses (finding F3 in the report).
    mysql(
        fx,
        "root",
        &root,
        "CREATE TABLE items (id BIGINT NOT NULL PRIMARY KEY, payload VARCHAR(200) CHARACTER SET utf8mb4 NOT NULL) ENGINE=InnoDB",
    );
    mysql(
        fx,
        "root",
        &root,
        "CREATE TABLE small (id INT NOT NULL PRIMARY KEY, note VARCHAR(64) CHARACTER SET utf8mb4 NOT NULL) ENGINE=InnoDB",
    );
    mysql(
        fx,
        "root",
        &root,
        "INSERT INTO items VALUES (1, CONCAT(LPAD(1,12,'0'), REPEAT('p',100)))",
    );
    let mut count = 1u64;
    while count < ITEMS_ROWS {
        mysql(
            fx,
            "root",
            &root,
            &format!(
                "INSERT INTO items (id, payload) SELECT id + {count}, CONCAT(LPAD(id + {count}, 12, '0'), REPEAT('p',100)) FROM items"
            ),
        );
        count *= 2;
    }
    mysql(
        fx,
        "root",
        &root,
        "INSERT INTO small VALUES (1,'a'),(2,'é'),(3,'c'),(4,'d'),(5,'e')",
    );
    mysql(
        fx,
        "root",
        &root,
        "GRANT SELECT ON oo_cb_t9.* TO 'oo_cdc'@'%'",
    );
}

/// Sorts rows by their integer key, so both sides compare in the same order.
fn by_key(mut rows: Vec<(String, String)>) -> Vec<(String, String)> {
    rows.sort_by_key(|(key, _)| key.parse::<i128>().expect("integer key"));
    rows
}

fn source_rows(fx: &Fx, sql: &str) -> Vec<(String, String)> {
    let out = mysql(fx, "oo_writer", &fx.writer_pw, sql);
    by_key(
        out.lines()
            .map(|line| {
                let (key, value) = line.split_once('\t').expect("two columns");
                (key.to_owned(), value.to_owned())
            })
            .collect(),
    )
}

/// Superuser connection to the target database (fixture only: registration and the epoch).
async fn pg_connect(fx: &Fx, database: &str) -> tokio_postgres::Client {
    let conn = format!(
        "host={} port={} user=postgres password={} dbname={} sslmode=disable",
        fx.pg_host, fx.pg_port, fx.pg_super_pw, database
    );
    let (client, connection) = tokio_postgres::connect(&conn, tokio_postgres::NoTls)
        .await
        .expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

/// What the local port does at one chunk. The source writes or the producer kill happen
/// after the loader has committed that chunk.
#[derive(Clone)]
enum Script {
    None,
    WritesAfterChunk(u64, String),
    KillAfterChunk(u64),
}

/// The integration port. Loader calls are real subprocesses; registration and the epoch
/// come from the target over the superuser connection, as the production port does.
struct LocalPort {
    fx: Fx,
    pg: tokio_postgres::Client,
    script: Script,
    producer_pids: Vec<u32>,
    chunk_calls: Vec<u64>,
    operations: Vec<String>,
}

async fn run_loader(fx: &Fx, operation: &Value) -> Result<Value, Fault> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let input = serde_json::to_vec(operation).map_err(|_| Fault::Token("request_json"))?;
    let mut child = Command::new(&fx.loader_bin)
        .arg("native-c1-baseline-stage-v2")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| Fault::Token("transport"))?;
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(&input)
        .await
        .map_err(|_| Fault::Token("transport"))?;
    drop(stdin);
    let mut stdout = child.stdout.take().expect("stdout");
    let mut bytes = Vec::new();
    stdout
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| Fault::Token("transport"))?;
    let status = child.wait().await.map_err(|_| Fault::Token("transport"))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| Fault::Token("transport"))?;
    if value["outcome"].as_str() == Some("refused") {
        let reason = value["reason"].as_str().unwrap_or("");
        eprintln!("loader refused {} : {reason}", operation["operation"]);
        return Err(Fault::Token(Box::leak(reason.to_owned().into_boxed_str())));
    }
    if !status.success() {
        return Err(Fault::Token("transport"));
    }
    Ok(value)
}

impl Port for LocalPort {
    async fn rebind(&mut self, stage: &Stage) -> Result<Option<Value>, Fault> {
        let row = self
            .pg
            .query_opt(
                "SELECT r.source_epoch::bigint FROM _oo.native_c1_baseline_v2_format f \
                 JOIN _oo.native_c1_baseline_run r ON r.source=f.source AND r.task_id=f.task_id \
                 WHERE f.source=$1 AND f.task_id=$2::text::uuid",
                &[&stage.source, &stage.task],
            )
            .await
            .map_err(|_| Fault::Token("sql"))?;
        let Some(row) = row else { return Ok(None) };
        let mut current = stage.clone();
        current.epoch = row.get::<_, i64>(0);
        self.operations.push("rebind".into());
        run_loader(
            &self.fx,
            &job_worker::native_schedule::native_schedule_v2::map_rebind(&current),
        )
        .await
        .map(Some)
    }

    async fn register(&mut self, stage: &Stage, open: &OpenFacts) -> Result<i64, Fault> {
        let text: String = self
            .pg
            .query_one(
                "SELECT _oo.native_source_instance_register($1,$2::text::uuid,$3,$4,$5,$6,$7,$8,$9::text::uuid)::text",
                &[&stage.source, &stage.owner, &stage.generation, &stage.source, &"mysql", &format!("mysql.{}", open.uuid),
                  &open.config_digest, &open.schema_digest, &stage.task],
            )
            .await
            .map_err(|err| {
                let detail = err.as_db_error().map_or_else(|| err.to_string(), |db| format!("{} {}", db.code().code(), db.message()));
                eprintln!("register: {detail}");
                Fault::Token("source_instance")
            })?
            .get(0);
        let instance: Value =
            serde_json::from_str(&text).map_err(|_| Fault::Token("source_instance"))?;
        instance["source_epoch"]
            .as_i64()
            .filter(|e| *e > 0)
            .ok_or(Fault::Token("source_epoch"))
    }

    async fn call(&mut self, operation: Value) -> Result<Value, Fault> {
        let name = operation["operation"].as_str().unwrap_or("").to_owned();
        self.operations.push(name.clone());
        let receipt = run_loader(&self.fx, &operation).await?;
        if name == "chunk" {
            let number = operation["chunk_no"].as_u64().unwrap_or(0);
            let table = operation["table"].as_str().unwrap_or("");
            self.chunk_calls
                .push(number * 1000 + if table == "items" { 1 } else { 2 });
            match self.script.clone() {
                Script::WritesAfterChunk(after, sql) if after == number && table == "items" => {
                    source_write(&self.fx, &sql)
                }
                Script::KillAfterChunk(after) if after == number && table == "items" => {
                    if let Some(pid) = self.producer_pids.last() {
                        let status = std::process::Command::new("kill")
                            .args(["-9", &pid.to_string()])
                            .status()
                            .expect("kill");
                        assert!(status.success());
                    }
                }
                Script::None | Script::WritesAfterChunk(..) | Script::KillAfterChunk(_) => {}
            }
        }
        Ok(receipt)
    }

    async fn beat(&mut self) -> Result<(), Fault> {
        Ok(())
    }

    fn producer_started(&mut self, pid: u32) {
        self.producer_pids.push(pid);
    }
}

/// The typing the producer's mapping gives these columns (`oo-connectors` `mysql/map.rs`):
/// BIGINT -> int8, INT -> int4, VARCHAR(n) -> varchar (oid 1043, typmod n + 4).
/// Every column is NOT NULL here.
fn fixed_typing() -> Typing {
    let mut map = BTreeMap::new();
    map.insert(
        format!("{DB}.big"),
        vec![
            Typed {
                name: "id".into(),
                oid: 20,
                typmod: -1,
                not_null: true,
            },
            Typed {
                name: "payload".into(),
                oid: 1043,
                typmod: 1200 + 4,
                not_null: true,
            },
        ],
    );
    map.insert(
        format!("{DB}.items"),
        vec![
            Typed {
                name: "id".into(),
                oid: 20,
                typmod: -1,
                not_null: true,
            },
            Typed {
                name: "payload".into(),
                oid: 1043,
                typmod: 200 + 4,
                not_null: true,
            },
        ],
    );
    map.insert(
        format!("{DB}.small"),
        vec![
            Typed {
                name: "id".into(),
                oid: 23,
                typmod: -1,
                not_null: true,
            },
            Typed {
                name: "note".into(),
                oid: 1043,
                typmod: 64 + 4,
                not_null: true,
            },
        ],
    );
    Typing::Fixed(map)
}

fn producer_config(fx: &Fx, dir: &Path, tables: &[&str]) -> PathBuf {
    let qualified: Vec<String> = tables.iter().map(|t| format!("{DB}.{t}")).collect();
    let columns: serde_json::Map<String, Value> = tables
        .iter()
        .map(|t| {
            let names = if *t == "small" {
                json!(["id", "note"])
            } else {
                json!(["id", "payload"])
            };
            (format!("{DB}.{t}"), names)
        })
        .collect();
    let config = json!({
        "database": DB, "host": fx.mysql_host, "port": fx.mysql_port,
        "user": "oo_cdc", "password_file": fx.cdc_pw_file,
        "dba_user": "oo_cdc", "dba_password_file": fx.cdc_pw_file,
        "ca_file": fx.ca, "tls_server_name": fx.tls_name, "server_id": 4242,
        "health_every_ms": 500, "reconcile_every_ms": 1000, "preserve_dir": dir,
        "fixture": "owned-mysql-57", "secret_ref": "secret://mysql-owned-fixture",
        "tables": qualified, "columns": columns,
    });
    let path = dir.join("producer-config.json");
    std::fs::write(&path, config.to_string()).expect("config");
    path
}

struct Outcome {
    done: Done,
    port: LocalPort,
    out: PathBuf,
    source: String,
    task: String,
}

/// A per-run suffix. The loader keeps state per source and task, so a rerun needs new ids.
fn run_nonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{:x}", nanos % 0xffff_ffff_ffff)
}

/// Runs one baseline end to end up to `end`, with the producer and the loader as subprocesses.
async fn baseline(fx: Fx, name: &str, script: Script, tables: &[&str], chunk_rows: u64) -> Outcome {
    let nonce = run_nonce();
    let source = format!("mysql_t9_{name}_{nonce}");
    let task = uuid_for(&format!("{name}{nonce}"));
    let dir = fx.work.join(format!("{name}-{nonce}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let out = dir.join(&task).join("capture");
    let config = producer_config(&fx, &dir, tables);
    let pg = pg_connect(&fx, &fx.pg_db).await;
    // The installer makes the runtime role without a login. The loader connects as that role.
    let runtime_pw = secret(fx.runtime_pw_file.to_str().expect("utf8 path"));
    pg.batch_execute(&format!(
        "ALTER ROLE oo_pipeline_runtime LOGIN PASSWORD '{runtime_pw}'"
    ))
    .await
    .expect("runtime login");
    // Fixture-only. The installed chain grants oo_target_owner pg_advisory_xact_lock(bigint),
    // but native_source_instance_register (0024) calls the (integer, integer) overload.
    // Without this grant the register step fails with 42501. See the report, finding F2.
    pg.batch_execute("GRANT EXECUTE ON FUNCTION pg_catalog.pg_advisory_xact_lock(integer, integer) TO oo_target_owner")
        .await
        .expect("fixture advisory grant");
    let owner = pg
        .query_one("SELECT gen_random_uuid()::text", &[])
        .await
        .expect("uuid")
        .get::<_, String>(0);
    pg.execute(
        "INSERT INTO jobs.tasks (id, state, generation, lease_until, project_id) VALUES ($1::text::uuid, 'running', 1, clock_timestamp() + interval '1 hour', gen_random_uuid()) ON CONFLICT (id) DO UPDATE SET state='running', generation=1, lease_until=clock_timestamp() + interval '1 hour'",
        &[&task],
    )
    .await
    .expect("task row");
    let stage = Stage {
        source: source.clone(),
        task: task.clone(),
        generation: 1,
        epoch: 0,
        target: json!({"host": fx.pg_host, "port": fx.pg_port, "database": fx.pg_db,
            "runtime_principal": "oo_pipeline_runtime", "password_file": fx.runtime_pw_file, "ssl": "disable",
            "shard": source, "owner": owner}),
        shard: source.clone(),
        owner,
    };
    let plan = Plan {
        source: source.clone(),
        base: 0,
        baseline: json!({"chunk_rows": chunk_rows}),
        producer: Executable {
            path: fx.producer_bin.to_string_lossy().into_owned(),
            sha256: String::new(),
            args: vec![
                "c1-baseline-stream-v2".into(),
                "--config".into(),
                config.to_string_lossy().into_owned(),
                "--out".into(),
                out.to_string_lossy().into_owned(),
            ],
        },
        out: out.clone(),
        task: task.clone(),
        config_sha256: sha256_file(&config),
        previous: Value::Null,
        typing: fixed_typing(),
    };
    let mut port = LocalPort {
        fx: fx.clone(),
        pg,
        script,
        producer_pids: Vec::new(),
        chunk_calls: Vec::new(),
        operations: Vec::new(),
    };
    let done = run(&mut port, &plan, stage).await;
    let done = match done {
        Ok(done) => done,
        Err(fault) => panic!("baseline failed: {fault:?}"),
    };
    Outcome {
        done,
        port,
        out,
        source,
        task,
    }
}

fn sha256_file(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("read");
    job_worker::sha256::sha256_hex(&bytes)
}

fn uuid_for(name: &str) -> String {
    // A fixed, unique uuid per scenario. The loader only checks the shape.
    let mut hex = job_worker::sha256::sha256_hex(name.as_bytes());
    hex.truncate(32);
    format!(
        "{}-{}-4{}-8{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

/// The target's rows of one table, as the loader stored them (keys and the text column).
async fn target_rows(
    pg: &tokio_postgres::Client,
    source: &str,
    task: &str,
    schema: &str,
    table: &str,
    value_column: usize,
) -> Vec<(String, String)> {
    let row = pg
        .query_one(
            "SELECT physical_name, key_column, physical_columns FROM _oo.native_c1_baseline_v2_table \
             WHERE source=$1 AND task_id=$2::text::uuid AND schema_ident=$3 AND table_ident=$4",
            &[&source, &task, &schema, &table],
        )
        .await
        .expect("table registration");
    let physical: String = row.get(0);
    let key: String = row.get(1);
    let columns: Vec<String> = row.get(2);
    let sql = format!(
        "SELECT \"{key}\"::text, \"{}\" FROM _oo.\"{physical}\" ORDER BY 1",
        columns[value_column]
    );
    by_key(
        pg.query(sql.as_str(), &[])
            .await
            .expect("target rows")
            .iter()
            .map(|r| (r.get(0), r.get::<_, String>(1)))
            .collect(),
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs OO_CB_T9_* fixture environment; see tests/fixtures/c1_v2_driver/README.md"]
async fn multi_table_baseline_with_concurrent_writes_matches_the_source() {
    let fx = fixture();
    seed(&fx);
    let writes = "START TRANSACTION; UPDATE items SET payload = CONCAT('u', payload) WHERE id BETWEEN 10 AND 19; COMMIT; \
        DELETE FROM items WHERE id BETWEEN 200 AND 209; \
        UPDATE items SET id = id + 5000 WHERE id BETWEEN 300 AND 304; \
        INSERT INTO items VALUES (2001,'new-1'),(2002,'new-2'); \
        START TRANSACTION; INSERT INTO items VALUES (2010,'txn-a'); UPDATE items SET payload='txn-b' WHERE id=2010; COMMIT; \
        START TRANSACTION; INSERT INTO items VALUES (2099,'never'); ROLLBACK; \
        UPDATE small SET note='changed' WHERE id=2; DELETE FROM small WHERE id=5;";
    let outcome = baseline(
        fx.clone(),
        "writes",
        Script::WritesAfterChunk(2, writes.into()),
        &["items", "small"],
        CHUNK_ROWS,
    )
    .await;
    let Done::Ended(summary) = &outcome.done else {
        panic!("expected the end of the stream: {:?}", outcome.done)
    };
    assert!(summary.batches >= 1, "CDC batches were sent: {summary:?}");
    let (source, task) = (outcome.source.as_str(), outcome.task.as_str());
    let items_target = target_rows(&outcome.port.pg, source, task, DB, "items", 1).await;
    let items_source = source_rows(&fx, "SELECT id, payload FROM items ORDER BY id");
    assert_eq!(
        items_target, items_source,
        "items: target equals the source at L_end"
    );
    let small_target = target_rows(&outcome.port.pg, source, task, DB, "small", 1).await;
    let small_source = source_rows(&fx, "SELECT id, note FROM small ORDER BY id");
    assert_eq!(
        small_target, small_source,
        "small: target equals the source at L_end"
    );
    assert!(outcome.out.join("worker-frames.ndjson").exists());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs OO_CB_T9_* fixture environment; see tests/fixtures/c1_v2_driver/README.md"]
async fn a_killed_producer_resumes_and_no_loaded_chunk_is_read_again() {
    let fx = fixture();
    seed(&fx);
    let outcome = baseline(
        fx.clone(),
        "kill",
        Script::KillAfterChunk(3),
        &["items", "small"],
        CHUNK_ROWS,
    )
    .await;
    assert!(
        matches!(outcome.done, Done::Ended(_)),
        "the run resumes after the kill: {:?}",
        outcome.done
    );
    let items_chunks: Vec<u64> = outcome
        .port
        .chunk_calls
        .iter()
        .filter(|c| *c % 1000 == 1)
        .map(|c| c / 1000)
        .collect();
    let mut unique = items_chunks.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        items_chunks.len(),
        unique.len(),
        "no chunk is loaded twice: {items_chunks:?}"
    );
    assert!(
        items_chunks.len() >= 6,
        "the table takes several chunks: {items_chunks:?}"
    );
    let items_target = target_rows(
        &outcome.port.pg,
        &outcome.source,
        &outcome.task,
        DB,
        "items",
        1,
    )
    .await;
    assert_eq!(
        items_target,
        source_rows(&fx, "SELECT id, payload FROM items ORDER BY id")
    );
    let restarts = outcome.port.producer_pids.len();
    assert_eq!(restarts, 2, "one producer was killed, one restarted");
    eprintln!(
        "KILL-RUN chunks={items_chunks:?} operations={}",
        outcome.port.operations.len()
    );
}

/// A table over 32 MiB, in chunks. The byte budget (8 MiB) cuts it into several parts, and
/// every part stays under the loader's 32 MiB bound. The default chunk settings apply.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs OO_CB_T9_* fixture environment; see tests/fixtures/c1_v2_driver/README.md"]
async fn a_table_over_32_mib_loads_in_bounded_chunks() {
    let fx = fixture();
    let root = fx.mysql_root_pw.clone();
    mysql(&fx, "root", &root, "DROP TABLE IF EXISTS big");
    mysql(
        &fx,
        "root",
        &root,
        "CREATE TABLE big (id BIGINT NOT NULL PRIMARY KEY, payload VARCHAR(1200) CHARACTER SET utf8mb4 NOT NULL) ENGINE=InnoDB",
    );
    mysql(
        &fx,
        "root",
        &root,
        "INSERT INTO big VALUES (1, CONCAT(LPAD(1,12,'0'), REPEAT('q',1000)))",
    );
    let mut count = 1u64;
    while count < LARGE_ROWS {
        mysql(
            &fx,
            "root",
            &root,
            &format!(
                "INSERT INTO big (id, payload) SELECT id + {count}, CONCAT(LPAD(id + {count}, 12, '0'), REPEAT('q',1000)) FROM big"
            ),
        );
        count *= 2;
    }
    mysql(
        &fx,
        "root",
        &root,
        &format!("DELETE FROM big WHERE id > {LARGE_ROWS}"),
    );
    let outcome = baseline(fx.clone(), "large", Script::None, &["big"], 50_000).await;
    assert!(matches!(outcome.done, Done::Ended(_)), "{:?}", outcome.done);
    // Only `big` is in this run, so every chunk call is one of its chunks.
    let big_chunks = outcome.port.chunk_calls.len();
    assert!(
        big_chunks >= 5,
        "a table over 32 MiB is cut into several chunks, got {big_chunks}"
    );
    let (source, task) = (outcome.source.as_str(), outcome.task.as_str());
    let target = target_rows(&outcome.port.pg, source, task, DB, "big", 1).await;
    let source_side = source_rows(&fx, "SELECT id, payload FROM big ORDER BY id");
    assert_eq!(target.len() as u64, LARGE_ROWS);
    assert!(
        target == source_side,
        "big: target equals the source at L_end"
    );
    eprintln!("LARGE-RUN chunks={big_chunks} rows={LARGE_ROWS}");
}
