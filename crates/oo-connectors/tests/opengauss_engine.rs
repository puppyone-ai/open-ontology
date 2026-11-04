//! Live tests only: testing/engines/opengauss/run.py owns the fixture.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use arrow_array::{Array, Int64Array, RecordBatch, StringArray};
use oo_connectors::engine::*;
use oo_connectors::postgres::opengauss::OpenGaussEngine;
use oo_connectors::postgres::reconcile::C5Cycle;
use oo_connectors::*;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Executor, Row};
use sqlx_core::sql_str::AssertSqlSafe;

const TABLES: &[&str] = &[
    "text_pk",
    "composite",
    "unique_only",
    "no_key",
    "growing",
    "empty_table",
    "integer_pk",
    "wide",
];

#[derive(Clone, Debug, Default, PartialEq)]
struct Gold {
    rows: u64,
    a: i128,
    b: i128,
}
impl Gold {
    fn row(&mut self, cells: Vec<Option<String>>) {
        let text = cells
            .into_iter()
            .map(|c| match c {
                None => "N".into(),
                Some(v) => format!("V{}:{v}", v.len()),
            })
            .collect::<Vec<_>>()
            .join("|");
        let hash = Sha256::digest(text.as_bytes());
        self.rows += 1;
        self.a += i64::from_be_bytes(hash[..8].try_into().unwrap()) as i128;
        self.b += i64::from_be_bytes(hash[8..16].try_into().unwrap()) as i128;
    }
    fn add(&mut self, g: &Self) {
        self.rows += g.rows;
        self.a += g.a;
        self.b += g.b;
    }
}
#[derive(Default)]
struct Sink {
    gold: Gold,
    ipc: Option<arrow_ipc::writer::StreamWriter<Vec<u8>>>,
}
impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        if self.ipc.is_none() {
            self.ipc = Some(
                arrow_ipc::writer::StreamWriter::try_new(Vec::new(), &batch.schema()).unwrap(),
            );
        }
        self.ipc.as_mut().unwrap().write(&batch).unwrap();
        for row in 0..batch.num_rows() {
            self.gold.row(
                batch
                    .columns()
                    .iter()
                    .map(|col| {
                        if col.is_null(row) {
                            None
                        } else if let Some(a) = col.as_any().downcast_ref::<Int64Array>() {
                            Some(a.value(row).to_string())
                        } else {
                            Some(
                                col.as_any()
                                    .downcast_ref::<StringArray>()
                                    .expect("fixture type")
                                    .value(row)
                                    .to_owned(),
                            )
                        }
                    })
                    .collect(),
            );
        }
        Ok(())
    }
}
fn config(dir: &Path, port: u16, user: &str, tables: &[&str]) -> PgSourceConfig {
    PgSourceConfig::try_new(
        SourceId::new("og_fixture").unwrap(),
        PgEndpoint::new("localhost", port, "postgres").unwrap(),
        PgCredentials::new(
            user,
            std::fs::read_to_string(dir.join("private").join(if user == "oo_reader" {
                "reader"
            } else {
                "shaonly"
            }))
            .unwrap(),
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
            tables
                .iter()
                .map(|t| SelectedTable::new("fixture", *t).unwrap())
                .collect(),
        )
        .unwrap(),
    )
    .unwrap()
}
async fn admin(dir: &Path, port: u16) -> PgConnection {
    PgConnectOptions::from_explicit()
        .host("localhost")
        .port(port)
        .database("postgres")
        .username("oo_admin")
        .password(&std::fs::read_to_string(dir.join("private/admin")).unwrap())
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(std::fs::read(dir.join("private/ca.crt")).unwrap())
        .disable_statement_logging()
        .connect()
        .await
        .unwrap()
}
fn table(name: &str) -> TableRef {
    TableRef {
        schema: "fixture".into(),
        name: name.into(),
    }
}
fn inputs() -> (PathBuf, u16) {
    (
        PathBuf::from(std::env::var("OO_OG_TEST_DIR").expect("owned runner")),
        std::env::var("OO_OG_TEST_PORT").unwrap().parse().unwrap(),
    )
}
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
async fn oracle(conn: &mut PgConnection, name: &str) -> Gold {
    let columns:Vec<String>=sqlx::query("SELECT a.attname::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='fixture' AND c.relname=$1 AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum")
        .bind(name).fetch_all(&mut *conn).await.unwrap().iter().map(|r|r.get::<String,_>(0)).collect();
    let sql = format!(
        "SELECT {} FROM fixture.{}",
        columns
            .iter()
            .map(|c| format!("{}::text", quote(c)))
            .collect::<Vec<_>>()
            .join(","),
        quote(name)
    );
    let mut stream = sqlx::query(AssertSqlSafe(sql)).fetch(&mut *conn);
    let mut gold = Gold::default();
    while let Some(r) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        let r = r.unwrap();
        gold.row(
            (0..columns.len())
                .map(|i| r.get::<Option<String>, _>(i))
                .collect(),
        );
    }
    gold
}
async fn reference_conn(engine: &mut OpenGaussEngine, dir: &Path, port: u16) -> PgConnection {
    let snapshot = engine.export_snapshot().await.unwrap();
    assert!(snapshot.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'));
    let mut conn = admin(dir, port).await;
    conn.execute("SET timezone='UTC'; SET datestyle='ISO, YMD'; SET bytea_output='hex'")
        .await
        .unwrap();
    conn.execute("START TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(format!(
        "SET TRANSACTION SNAPSHOT '{snapshot}'"
    )))
    .execute(&mut conn)
    .await
    .unwrap();
    conn
}

#[tokio::test]
#[ignore = "owned TLS openGauss fixture"]
async fn t0_t1_t2_real_engine() {
    let (dir, port) = inputs();
    // Independently exercise the unmodified driver before the sanitized engine.
    let mut wire = PgConnectOptions::from_explicit()
        .host("localhost")
        .port(port)
        .database("postgres")
        .username("oo_reader")
        .password(&std::fs::read_to_string(dir.join("private/reader")).unwrap())
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(std::fs::read(dir.join("private/ca.crt")).unwrap())
        .disable_statement_logging()
        .connect()
        .await
        .unwrap();
    wire.execute("SELECT 1").await.unwrap();
    drop(wire);
    let mut denied = OpenGaussEngine::new(config(&dir, port, "oo_reader", &["denied"]));
    let report = denied.probe().await.unwrap();
    assert!(
        report
            .requirements()
            .iter()
            .any(|r| r.name == "select:fixture.denied")
    );
    std::fs::write(
        dir.join("T0-missing.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    drop(denied);
    let sha_wire = PgConnectOptions::from_explicit()
        .host("localhost")
        .port(port)
        .database("postgres")
        .username("shaonly")
        .password(&std::fs::read_to_string(dir.join("private/shaonly")).unwrap())
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(std::fs::read(dir.join("private/ca.crt")).unwrap())
        .disable_statement_logging()
        .connect()
        .await;
    if let Err(error) = sha_wire {
        println!("SHA-only fixture driver refusal: {error}");
    }
    let mut sha = OpenGaussEngine::new(config(&dir, port, "shaonly", &["integer_pk"]));
    let report = sha.probe().await.unwrap();
    assert_eq!(
        report.version.major, 0,
        "auth failure must not invent a server version"
    );
    assert_eq!(report.requirements().len(), 3);
    std::fs::write(
        dir.join("T0-shaonly.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    drop(sha);
    let mut engine = OpenGaussEngine::new(config(&dir, port, "oo_reader", TABLES));
    let report = engine.probe().await.unwrap();
    assert_eq!(report.family, EngineFamily::Opengauss);
    assert_eq!(
        report.version.to_string(),
        std::env::var("OO_OG_TEST_VERSION").unwrap()
    );
    for c in [
        Capability::Catalog,
        Capability::ChunkedSnapshot,
        Capability::DigestDiff,
    ] {
        assert_eq!(report.capabilities[&c], CapabilityState::Available);
    }
    assert!(matches!(
        report.capabilities[&Capability::LogCdc],
        CapabilityState::Unsupported { .. }
    ));
    std::fs::write(
        dir.join("T0.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    let mut reference = reference_conn(&mut engine, &dir, port).await;
    let mut writer = admin(&dir, port).await;
    // Mutate after the shared snapshot, while the bounded snapshot is read.
    let writes = tokio::spawn(async move {
        for cycle in 0..12 {
            let sql = format!(
                "INSERT INTO fixture.growing VALUES ({},'insert'); UPDATE fixture.integer_pk SET payload='changed' WHERE id={}; DELETE FROM fixture.growing WHERE id={}; UPDATE fixture.composite SET region=region+100 WHERE code='code-{}'",
                10000 + cycle,
                1 + cycle,
                1 + cycle,
                1 + cycle
            );
            writer.execute(AssertSqlSafe(sql)).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    let budget = ReadBudget {
        max_chunk_rows: 100_000,
        statement_timeout_ms: 120_000,
        ..ReadBudget::default()
    };
    let mut baseline = BTreeMap::<String, Vec<(RangeDigest, Gold)>>::new();
    let mut total = 0;
    for name in TABLES {
        let t = table(name);
        let plan = engine.plan_chunks(&t, &budget).await.unwrap();
        if *name == "integer_pk" {
            assert!(plan.chunks.len() > 1);
        }
        let expected = oracle(&mut reference, name).await;
        let mut actual = Gold::default();
        let mut chunks = Vec::new();
        for chunk in &plan.chunks {
            let d = engine
                .digest_range(&t, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .unwrap();
            let mut sink = Sink::default();
            let receipt = engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
            let mut ipc = sink.ipc.take().unwrap();
            ipc.finish().unwrap();
            let bytes = ipc.into_inner().unwrap();
            assert_eq!(receipt.sha256, format!("{:x}", Sha256::digest(&bytes)));
            assert_eq!(receipt.bytes, bytes.len() as u64);
            assert_eq!(receipt.rows, sink.gold.rows);
            assert_eq!(receipt.rows, d.rows);
            actual.add(&sink.gold);
            chunks.push((d, sink.gold));
        }
        assert_eq!(actual, expected, "T1 {name}");
        total += actual.rows;
        baseline.insert((*name).into(), chunks);
    }
    writes.await.unwrap();
    assert!(total >= 2_000_000);
    println!(
        "T1: {total} rows, 8 tables including 300 columns, zero SHA256/count differences under concurrent insert/update/delete/PK-update"
    );
    drop(reference);
    drop(engine);
    let mut changed_payloads = Vec::new();
    let mut last_lsn = 0;
    for cycle in 0..3 {
        if cycle == 2 {
            let mut writer = admin(&dir, port).await;
            writer.execute("UPDATE fixture.growing SET payload='equal-count-update' WHERE id=10000; DELETE FROM fixture.unique_only WHERE id=1; INSERT INTO fixture.unique_only VALUES (20000,'new'); DELETE FROM fixture.text_pk; UPDATE fixture.composite SET code='moved' WHERE code='code-100'").await.unwrap();
        }
        let mut engine = OpenGaussEngine::new(config(&dir, port, "oo_reader", TABLES));
        let mut reference = reference_conn(&mut engine, &dir, port).await;
        let Some(Watermark::PgLsn { lsn }) = engine.watermark().await.unwrap() else {
            panic!("missing watermark")
        };
        assert!(lsn >= last_lsn);
        last_lsn = lsn;
        let mut reads = 0;
        let mut next = BTreeMap::new();
        for name in TABLES {
            let t = table(name);
            let plan = engine.plan_chunks(&t, &budget).await.unwrap();
            let previous = baseline[*name]
                .iter()
                .map(|(d, _)| d.clone())
                .collect::<Vec<_>>();
            let inspection = engine
                .inspect_c5(
                    &t,
                    &previous,
                    &C5Cycle {
                        cycle: cycle + 1,
                        full_read_every: 1000,
                        positive_hints: Default::default(),
                        recent_chunks: Default::default(),
                    },
                )
                .await
                .unwrap();
            let mut chunks = Vec::new();
            let mut actual = Gold::default();
            for chunk in &plan.chunks {
                let d = inspection
                    .source_digests
                    .iter()
                    .find(|d| d.lo == chunk.lo && d.hi == chunk.hi)
                    .unwrap()
                    .clone();
                let prior = baseline[*name].iter().find(|(old, _)| *old == d);
                assert_eq!(inspection.read_chunks.contains(chunk), prior.is_none());
                let gold = if let Some((_, gold)) = prior {
                    gold.clone()
                } else {
                    reads += 1;
                    let mut sink = Sink::default();
                    engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
                    sink.gold
                };
                actual.add(&gold);
                chunks.push((d, gold));
            }
            assert_eq!(
                actual,
                oracle(&mut reference, name).await,
                "T2 cycle {cycle} {name}"
            );
            next.insert((*name).into(), chunks);
        }
        if cycle == 1 {
            assert_eq!(reads, 0, "unchanged cycle must read/rewrite no payload");
        } else {
            assert!(reads > 0);
        }
        println!("T2 cycle {cycle}: {reads} changed range payloads, zero SHA256/count differences");
        changed_payloads.push(reads);
        baseline = next;
    }
    std::fs::write(dir.join("T1-T2.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "snapshot_rows":total,"snapshot_tables":8,"c5_cycles":3,"changed_payloads":changed_payloads,
        "oracle":"independent source text SHA256 sums versus decoded Arrow, common snapshot",
        "target":"in-memory range replacement, not Agent DB publication"
    })).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "owned TLS openGauss fixture"]
async fn types_budgets_and_periodic_full_read() {
    use arrow_array::{BinaryArray, BooleanArray, Decimal128Array, TimestampMicrosecondArray};
    struct Special(Vec<RecordBatch>);
    impl ChunkSink for Special {
        fn push(&mut self, b: RecordBatch) -> Result<(), SourceError> {
            self.0.push(b);
            Ok(())
        }
    }
    let (dir, port) = inputs();
    let mut engine = OpenGaussEngine::new(config(&dir, port, "oo_reader", &["special"]));
    let budget = ReadBudget::default();
    let t = table("special");
    let plan = engine.plan_chunks(&t, &budget).await.unwrap();
    assert_eq!(plan.chunks.len(), 1);
    let mut sink = Special(vec![]);
    let r = engine
        .read_chunk(&plan.chunks[0], &budget, &mut sink)
        .await
        .unwrap();
    assert_eq!(r.rows, 2);
    let receipt_bytes = r.bytes;
    let b = &sink.0[0];
    assert_eq!(
        b.column(1)
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap()
            .value(0),
        1_234_567_890_123_456_789_012_123_456_78
    );
    assert_eq!(
        b.column(2)
            .as_any()
            .downcast_ref::<BinaryArray>()
            .unwrap()
            .value(0),
        [0, 1, 255]
    );
    assert!(
        b.column(3)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap()
            .value(0)
    );
    assert!(b.column(1).is_null(1));
    assert_eq!(
        b.column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "{\"x\": 1}"
    );
    let mut admin = admin(&dir, port).await;
    let expected: String = sqlx::query(
        "SELECT (extract(epoch FROM stamp)*1000000)::bigint::text FROM fixture.special WHERE id=1",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap()
    .get(0);
    assert_eq!(
        b.column(4)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap()
            .value(0),
        expected.parse::<i64>().unwrap()
    );
    let digest = engine.digest_range(&t, None, None).await.unwrap();
    for (cycle, full, positive, expected) in [
        (1, 5, false, false),
        (5, 5, false, true),
        (1, 5, true, true),
    ] {
        let inspection = engine
            .inspect_c5(
                &t,
                std::slice::from_ref(&digest),
                &C5Cycle {
                    cycle,
                    full_read_every: full,
                    positive_hints: if positive {
                        [0].into()
                    } else {
                        Default::default()
                    },
                    recent_chunks: Default::default(),
                },
            )
            .await
            .unwrap();
        assert_eq!(!inspection.read_chunks.is_empty(), expected);
    }
    let mut stale = plan.chunks[0].clone();
    stale.plan_revision += 1;
    assert!(
        engine
            .read_chunk(&stale, &budget, &mut Special(vec![]))
            .await
            .is_err()
    );
    let small = ReadBudget {
        max_chunk_rows: 1,
        ..budget.clone()
    };
    assert!(
        engine
            .read_chunk(&plan.chunks[0], &small, &mut Special(vec![]))
            .await
            .is_err()
    );
    assert!(
        engine
            .read_chunk(&plan.chunks[0], &budget, &mut Special(vec![]))
            .await
            .is_err(),
        "failed stream invalidates its snapshot and plans"
    );
    let mut engine = OpenGaussEngine::new(config(&dir, port, "oo_reader", &["special"]));
    let plan = engine.plan_chunks(&t, &budget).await.unwrap();
    let byte_budget = ReadBudget {
        max_chunk_bytes: receipt_bytes - 1,
        ..budget
    };
    assert!(
        engine
            .read_chunk(&plan.chunks[0], &byte_budget, &mut Special(vec![]))
            .await
            .is_err(),
        "the IPC end marker also counts against the byte budget"
    );
    println!("typed decimal/binary/bool/timestamp/JSON/NULL and stale/row budgets: passed");
}
