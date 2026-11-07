//! Opt-in synthetic engine matrix; launched by testing/engines/postgres/run.py.
use arrow_array::{Array, Int64Array, RecordBatch, StringArray};
use oo_connectors::engine::*;
use oo_connectors::postgres::PostgresEngine;
use oo_connectors::*;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use sqlx::{ConnectOptions, Connection, Executor, Row};
use sqlx_core::sql_str::AssertSqlSafe;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const TABLES: &[&str] = &[
    "text_pk",
    "composite",
    "unique_only",
    "no_key",
    "growing",
    "integer_pk",
];

struct OracleSink {
    rows: u64,
    a: i128,
    b: i128,
    ipc: Option<arrow_ipc::writer::StreamWriter<Vec<u8>>>,
}
impl OracleSink {
    fn new() -> Self {
        Self {
            rows: 0,
            a: 0,
            b: 0,
            ipc: None,
        }
    }
}
impl ChunkSink for OracleSink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        if self.ipc.is_none() {
            self.ipc = Some(
                arrow_ipc::writer::StreamWriter::try_new(Vec::new(), &batch.schema()).unwrap(),
            );
        }
        self.ipc.as_mut().unwrap().write(&batch).unwrap();
        for i in 0..batch.num_rows() {
            let mut cells = Vec::new();
            for col in batch.columns() {
                if col.is_null(i) {
                    cells.push("N".to_owned());
                    continue;
                }
                let value = if let Some(v) = col.as_any().downcast_ref::<Int64Array>() {
                    v.value(i).to_string()
                } else {
                    col.as_any()
                        .downcast_ref::<StringArray>()
                        .expect("fixture type")
                        .value(i)
                        .to_owned()
                };
                cells.push(format!("V{}:{value}", value.len()));
            }
            let digest = Sha256::digest(cells.join("|").as_bytes());
            self.a += i64::from_be_bytes(digest[..8].try_into().unwrap()) as i128;
            self.b += i64::from_be_bytes(digest[8..16].try_into().unwrap()) as i128;
            self.rows += 1;
        }
        Ok(())
    }
}

fn config(dir: &std::path::Path, port: u16) -> PgSourceConfig {
    PgSourceConfig::try_new(
        SourceId::new("pg_matrix").unwrap(),
        PgEndpoint::new("localhost", port, "postgres").unwrap(),
        PgCredentials::new(
            "oo_reader",
            std::fs::read_to_string(dir.join("private/reader")).unwrap(),
        )
        .unwrap(),
        SecretRef::new("secret://fixture/reader").unwrap(),
        PgTlsConfig::from_ca_pem(std::fs::read(dir.join("private/ca.crt")).unwrap()).unwrap(),
        PgBudget::new(
            Duration::from_secs(10),
            Duration::from_secs(120),
            Duration::from_secs(1),
            100_000,
            32 * 1024 * 1024,
        )
        .unwrap(),
        TableSelection::new(
            TABLES
                .iter()
                .map(|t| SelectedTable::new("fixture", *t).unwrap())
                .collect(),
        )
        .unwrap(),
    )
    .unwrap()
}

fn admin_options(dir: &std::path::Path, port: u16) -> PgConnectOptions {
    PgConnectOptions::from_explicit()
        .host("localhost")
        .port(port)
        .database("postgres")
        .username("postgres")
        .password(&std::fs::read_to_string(dir.join("private/admin")).unwrap())
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(std::fs::read(dir.join("private/ca.crt")).unwrap())
        .disable_statement_logging()
}

#[tokio::test]
#[ignore = "requires the owned TLS fixture runner"]
async fn matrix_t0_t1_t8() {
    let dir = PathBuf::from(std::env::var("OO_PG_TEST_DIR").expect("owned runner"));
    let port = std::env::var("OO_PG_TEST_PORT").unwrap().parse().unwrap();
    let major: u32 = std::env::var("OO_PG_TEST_MAJOR").unwrap().parse().unwrap();
    let options = admin_options(&dir, port);
    let mut admin = options.connect().await.unwrap();
    let mut engine = PostgresEngine::new(config(&dir, port));
    admin.execute("ALTER ROLE oo_reader NOREPLICATION; REVOKE SELECT ON fixture.text_pk FROM oo_reader; DROP PUBLICATION oo_fixture").await.unwrap();
    let missing = engine.probe().await.unwrap();
    let names = missing
        .requirements()
        .iter()
        .map(|r| r.name.as_str())
        .collect::<Vec<_>>();
    for name in ["replication", "publication", "select:fixture.text_pk"] {
        assert!(names.contains(&name));
    }
    assert!(
        missing
            .requirements()
            .iter()
            .all(|r| r.dba_statement.is_some())
    );
    std::fs::write(
        dir.join("T0-missing.json"),
        serde_json::to_vec_pretty(&missing).unwrap(),
    )
    .unwrap();
    admin.execute("ALTER ROLE oo_reader REPLICATION; GRANT SELECT ON fixture.text_pk TO oo_reader; CREATE PUBLICATION oo_fixture FOR ALL TABLES").await.unwrap();
    let report = engine.probe().await.unwrap();
    assert_eq!(report.version.major, major);
    assert!(report.requirements().is_empty(), "{report:?}");
    std::fs::write(
        dir.join("T0.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    // The external exporter is a test/coordinator connection, not a product read task.
    admin
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .unwrap();
    let snap: String = sqlx::query_scalar("SELECT pg_export_snapshot()")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    engine = engine.with_snapshot(snap).unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let writes = Arc::new(AtomicU64::new(0));
    let peak = Arc::new(AtomicU64::new(0));
    let backend_peak = Arc::new(AtomicU64::new(0));
    let writer = {
        let running = running.clone();
        let writes = writes.clone();
        let peak = peak.clone();
        let backend_peak = backend_peak.clone();
        tokio::spawn(async move {
            let mut c = options.connect().await.unwrap();
            let mut n = 0i64;
            while running.load(Ordering::Relaxed) {
                let mut tx = c.begin().await.unwrap();
                for t in ["integer_pk", "unique_only", "no_key", "growing"] {
                    sqlx::query(AssertSqlSafe(format!(
                        "UPDATE fixture.{t} SET payload='changed' WHERE id=$1"
                    )))
                    .bind(1 + n % 1000)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                    sqlx::query(AssertSqlSafe(format!(
                        "DELETE FROM fixture.{t} WHERE id=$1"
                    )))
                    .bind(2001 + n % 1000)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                    sqlx::query(AssertSqlSafe(format!(
                        "INSERT INTO fixture.{t} VALUES ($1,'inserted')"
                    )))
                    .bind(3_000_000 + n)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                    sqlx::query(AssertSqlSafe(format!(
                        "UPDATE fixture.{t} SET id=$1 WHERE id=$2"
                    )))
                    .bind(6_000_000 + n)
                    .bind(4001 + n % 1000)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                }
                sqlx::query("UPDATE fixture.text_pk SET payload='changed' WHERE code=$1")
                    .bind(format!("code-{}", 1 + n % 1000))
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                sqlx::query(
                    "UPDATE fixture.composite SET payload='changed' WHERE region=$1 AND code=$2",
                )
                .bind((1 + n % 1000) % 11)
                .bind(format!("code-{}", 1 + n % 1000))
                .execute(&mut *tx)
                .await
                .unwrap();
                tx.commit().await.unwrap();
                writes.fetch_add(1, Ordering::Relaxed);
                n += 1;
                let sample = sqlx::query("SELECT count(*) FILTER (WHERE backend_type='client backend')::int8, count(*)::int8 FROM pg_stat_activity WHERE usename='oo_reader'")
                    .fetch_one(&mut c).await.unwrap();
                let count: i64 = sample.get(0);
                let backends: i64 = sample.get(1);
                peak.fetch_max(count as u64, Ordering::Relaxed);
                if backend_peak.fetch_max(backends as u64, Ordering::Relaxed) < backends as u64 {
                    eprintln!("source resource sample: clients={count}, all backends={backends}");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    };
    let mut results = Vec::new();
    let mut timings = Vec::new();
    for name in TABLES {
        let table = TableRef {
            schema: "fixture".into(),
            name: (*name).into(),
        };
        let budget = ReadBudget {
            max_chunk_rows: if *name == "integer_pk" {
                100_000
            } else {
                20_000
            },
            statement_timeout_ms: 120_000,
            ..ReadBudget::default()
        };
        let plan = engine.plan_chunks(&table, &budget).await.unwrap();
        if *name == "no_key" {
            assert_eq!(
                matches!(plan.strategy, ChunkStrategy::PhysicalRange { .. }),
                major >= 14
            );
        }
        if *name == "integer_pk" {
            assert!(plan.chunks.len() >= 40, "{plan:?}");
        }
        if *name == "composite" {
            assert!(engine.explains[&table].contains("Index Cond"));
        }
        std::fs::write(
            dir.join(format!("plan-{name}.json")),
            serde_json::to_vec_pretty(&plan).unwrap(),
        )
        .unwrap();
        let columns = match *name {
            "text_pk" => vec!["code", "payload"],
            "composite" => vec!["region", "code", "payload"],
            _ => vec!["id", "payload"],
        };
        let canonical=columns.iter().map(|c|format!("CASE WHEN {c} IS NULL THEN 'N' ELSE 'V'||octet_length({c}::text)::text||':'||{c}::text END")).collect::<Vec<_>>().join("||'|'||");
        let oracle=sqlx::query(AssertSqlSafe(format!("SELECT count(*)::int8, sum(('x'||substr(encode(h,'hex'),1,16))::bit(64)::bigint::numeric)::text, sum(('x'||substr(encode(h,'hex'),17,16))::bit(64)::bigint::numeric)::text FROM (SELECT sha256(convert_to({canonical},'UTF8')) h FROM fixture.{name}) q"))).fetch_one(&mut admin).await.unwrap();
        let (mut rows, mut a, mut b) = (0u64, 0i128, 0i128);
        let start = Instant::now();
        let mut receipts = Vec::new();
        for chunk in &plan.chunks {
            let mut sink = OracleSink::new();
            let receipt = engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
            let timing = engine.last_read_timing.clone().unwrap();
            assert!(timing.rest_ns >= timing.work_ns * 3);
            timings.push(timing);
            let mut ipc = sink.ipc.take().unwrap();
            ipc.finish().unwrap();
            let bytes = ipc.into_inner().unwrap();
            assert_eq!(receipt.sha256, format!("{:x}", Sha256::digest(&bytes)));
            assert_eq!(receipt.bytes, bytes.len() as u64);
            assert_eq!(receipt.rows, sink.rows);
            rows += sink.rows;
            a += sink.a;
            b += sink.b;
            receipts.push(receipt);
            eprintln!("PG {major} {name}: chunk {} complete", chunk.chunk_id);
        }
        assert_eq!(rows, oracle.get::<i64, _>(0) as u64, "{name} count");
        assert_eq!(
            a.to_string(),
            oracle.get::<String, _>(1),
            "{name} hash first half"
        );
        assert_eq!(
            b.to_string(),
            oracle.get::<String, _>(2),
            "{name} hash second half"
        );
        let digest = engine.digest_range(&table, None, None).await.unwrap();
        assert_eq!(digest.rows, rows);
        assert_eq!(digest.digest, format!("pg-text-sha256-sum128/v1:{a}:{b}"));
        std::fs::write(
            dir.join(format!("receipts-{name}.json")),
            serde_json::to_vec_pretty(&receipts).unwrap(),
        )
        .unwrap();
        results.push(serde_json::json!({"table":name,"rows":rows,"chunks":plan.chunks.len(),"zero_diff":true,"seconds":start.elapsed().as_secs_f64()}));
        eprintln!("PG {major} {name}: {rows} rows, zero diff");
    }
    running.store(false, Ordering::Relaxed);
    writer.await.unwrap();
    assert!(writes.load(Ordering::Relaxed) > 0);
    assert!(peak.load(Ordering::Relaxed) <= 1);
    // Closing the exporter invalidates old physical locators; server must reject reuse.
    admin.execute("ROLLBACK").await.unwrap();
    let table = TableRef {
        schema: "fixture".into(),
        name: "no_key".into(),
    };
    assert!(engine.digest_range(&table, None, None).await.is_err());
    // A later cycle grows past the old single-chunk budget; a fresh plan covers
    // the entire relation instead of clipping it to the former high key.
    admin.execute("INSERT INTO fixture.growing SELECT i,'next-cycle' FROM generate_series(10000000,10100000) i; ANALYZE fixture.growing").await.unwrap();
    let growing = TableRef {
        schema: "fixture".into(),
        name: "growing".into(),
    };
    let mut next = PostgresEngine::new(config(&dir, port));
    let budget = ReadBudget {
        statement_timeout_ms: 120_000,
        ..ReadBudget::default()
    };
    let grown = next.plan_chunks(&growing, &budget).await.unwrap();
    assert!(grown.chunks.len() > 1);
    let mut total = 0;
    for chunk in &grown.chunks {
        let mut sink = OracleSink::new();
        total += next
            .read_chunk(chunk, &budget, &mut sink)
            .await
            .unwrap()
            .rows;
    }
    let expected: i64 = sqlx::query_scalar("SELECT count(*) FROM fixture.growing")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(total, expected as u64);
    std::fs::write(
        dir.join("growth.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"rows":total,"chunks":grown.chunks.len(),"zero_count_diff":true}),
        )
        .unwrap(),
    )
    .unwrap();
    let mut stale = grown.chunks[0].clone();
    stale.plan_revision += 1;
    assert_eq!(
        next.read_chunk(&stale, &budget, &mut OracleSink::new())
            .await
            .unwrap_err()
            .fact(),
        Some("stale_chunk")
    );
    let tiny = ReadBudget {
        max_chunk_rows: 1,
        ..budget.clone()
    };
    assert_eq!(
        next.read_chunk(&grown.chunks[0], &tiny, &mut OracleSink::new())
            .await
            .unwrap_err()
            .fact(),
        Some("chunk_rows_replan")
    );
    // A live competing active session must block dispatch, then timeout without reading.
    let mut blocker = admin_options(&dir, port).connect().await.unwrap();
    let sleeping = tokio::spawn(async move {
        sqlx::query("SELECT pg_sleep(2)")
            .execute(&mut blocker)
            .await
            .unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut guarded = PostgresEngine::new(config(&dir, port))
        .with_protection(oo_connectors::postgres::Protection {
            active_session_limit: 1,
            connection_reserve: 5,
            pause_timeout: Duration::from_millis(200),
        })
        .unwrap();
    assert_eq!(
        guarded
            .plan_chunks(&growing, &budget)
            .await
            .unwrap_err()
            .fact(),
        Some("active_sessions")
    );
    sleeping.await.unwrap();
    let timed = ReadBudget {
        statement_timeout_ms: 1,
        ..budget.clone()
    };
    let timeout = next
        .read_chunk(&grown.chunks[0], &timed, &mut OracleSink::new())
        .await;
    assert!(
        timeout.is_err(),
        "one millisecond is not enough for this full chunk"
    );
    std::fs::write(dir.join("T1.json"),serde_json::to_vec_pretty(&serde_json::json!({"consistency":"one exported source snapshot","target":"independent Arrow sink oracle; not Agent DB integration","writer_commits":writes.load(Ordering::Relaxed),"tables":results})).unwrap()).unwrap();
    std::fs::write(dir.join("T8.json"),serde_json::to_vec_pretty(&serde_json::json!({"reader_connection_peak":peak.load(Ordering::Relaxed),"reader_backend_peak_including_parallel_workers":backend_peak.load(Ordering::Relaxed),"connection_budget":1,"configured_duty_cycle_percent":25,"read_timings":timings,"target_write_bytes":"not measured; no Agent DB","model_tokens":0})).unwrap()).unwrap();
}
