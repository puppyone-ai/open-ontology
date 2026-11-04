#![cfg(feature = "oracle")]

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use arrow_array::{Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use oo_connectors::SourceError;
use oo_connectors::engine::*;
use oo_connectors::oracle::engine::{OracleConnectConfig, OracleEngine, map_ora as thin_error};
use oo_connectors::oracle::worker::{DriverError, Worker};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::process::Command;
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Fixture {
    port: u16,
    service: String,
    version: u32,
    reader_password: String,
    writer_password: String,
    out: PathBuf,
    #[serde(default)]
    worker_container: Option<String>,
    #[serde(default)]
    miner_password: Option<String>,
    #[serde(default)]
    root_service: Option<String>,
}

#[path = "oracle_incremental_cases/mod.rs"]
mod incremental;

enum Connection {
    Thin(oracledb::Connection),
    Thick(Worker),
}
struct Row(Vec<Option<String>>);
impl Row {
    fn get(&self, i: usize) -> Result<Option<String>, String> {
        self.0.get(i).cloned().ok_or("fixture_column".to_owned())
    }
}
fn worker_error(e: DriverError) -> String {
    eprintln!("fixture driver code: {e:?}");
    "fixture_driver".to_owned()
}
impl Connection {
    fn query(
        &self,
        sql: &str,
        _binds: &[()],
    ) -> Result<Box<dyn Iterator<Item = Result<Row, String>> + '_>, String> {
        match self {
            Self::Thin(conn) => {
                let cursor = conn
                    .query(sql, &[])
                    .map_err(|e| thin_error(&e).to_string())?;
                let width = cursor.columns().len();
                Ok(Box::new(cursor.map(move |row| {
                    let row = row.map_err(|e| thin_error(&e).to_string())?;
                    (0..width)
                        .map(|i| {
                            row.get::<Option<String>>(i)
                                .map_err(|e| thin_error(&e).to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map(Row)
                })))
            }
            Self::Thick(conn) => Ok(Box::new(
                conn.query(sql, &[], 2_000_001)
                    .map_err(worker_error)?
                    .map(|row| row.map(Row).map_err(worker_error)),
            )),
        }
    }
    fn execute(&self, sql: &str, _binds: &[()]) -> Result<(), String> {
        match self {
            Self::Thin(conn) => {
                conn.execute(sql, &[])
                    .map_err(|e| thin_error(&e).to_string())?;
            }
            Self::Thick(conn) => {
                for row in conn.query(sql, &[], 1).map_err(worker_error)? {
                    row.map_err(worker_error)?;
                }
            }
        }
        Ok(())
    }
    fn commit(&self) -> Result<(), String> {
        match self {
            Self::Thin(conn) => conn.commit().map_err(|e| thin_error(&e).to_string()),
            Self::Thick(_) => self.execute("COMMIT", &[]),
        }
    }
}

fn worker_command(f: &Fixture, admin: bool) -> Command {
    let mut command = Command::new("docker");
    command.args([
        "exec",
        "-i",
        f.worker_container.as_ref().unwrap(),
        "python",
        "-u",
        if admin {
            "/fixture_admin.py"
        } else {
            "/worker.py"
        },
    ]);
    command
}

fn connect(f: &Fixture, writer: bool) -> Connection {
    let (user, password) = if writer {
        ("OO_FIXTURE", &f.writer_password)
    } else {
        ("OO_READER", &f.reader_password)
    };
    if f.worker_container.is_some() {
        let cfg = OracleConnectConfig::new(
            "host.docker.internal",
            f.port,
            &f.service,
            user,
            Zeroizing::new(password.clone()),
        );
        return Connection::Thick(
            Worker::connect(&mut worker_command(f, writer), &cfg)
                .map_err(worker_error)
                .unwrap(),
        );
    }
    let cfg = oracledb::Config::default()
        .set_credentials(user, password)
        .set_connect_string(&format!("127.0.0.1:{}/{}", f.port, f.service))
        .map_err(|e| thin_error(&e))
        .unwrap();
    let conn = oracledb::connect(cfg).map_err(|e| thin_error(&e)).unwrap();
    conn.set_call_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| thin_error(&e))
        .unwrap();
    Connection::Thin(conn)
}

fn engine_connect(
    f: &Fixture,
    mut config: OracleConnectConfig,
) -> Result<OracleEngine, SourceError> {
    if f.worker_container.is_some() {
        config.host = "host.docker.internal".into();
        OracleEngine::connect_worker(config, &mut worker_command(f, false))
    } else {
        OracleEngine::connect(config)
    }
}

fn execute(conn: &Connection, sql: &str) {
    conn.execute(sql, &[]).unwrap();
}

type Rows = BTreeMap<String, [u8; 32]>;

fn row_key(cells: &[Option<String>], keys: usize) -> String {
    serde_json::to_string(&cells[..keys]).unwrap()
}

fn row_hash(cells: &[Option<String>]) -> [u8; 32] {
    Sha256::digest(serde_json::to_vec(cells).unwrap()).into()
}

struct Sink {
    keys: usize,
    rows: Rows,
    ipc: Option<StreamWriter<Vec<u8>>>,
}

impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        if self.ipc.is_none() {
            self.ipc = Some(StreamWriter::try_new(Vec::new(), &batch.schema()).unwrap());
        }
        self.ipc.as_mut().unwrap().write(&batch).unwrap();
        for i in 0..batch.num_rows() {
            let cells: Vec<Option<String>> = batch
                .columns()
                .iter()
                .map(|col| {
                    if let Some(col) = col.as_any().downcast_ref::<arrow_array::Int64Array>() {
                        return (!col.is_null(i)).then(|| col.value(i).to_string());
                    }
                    let col = col.as_any().downcast_ref::<StringArray>().unwrap();
                    (!col.is_null(i)).then(|| col.value(i).to_owned())
                })
                .collect();
            assert!(
                self.rows
                    .insert(row_key(&cells, self.keys), row_hash(&cells))
                    .is_none(),
                "duplicate delivered key"
            );
        }
        Ok(())
    }
}

fn reference(conn: &Connection, table: &str, scn: u64, keys: usize) -> Rows {
    let projection = match table {
        "NUMERIC_PK" => "TO_CHAR(ID,'TM9'), PAYLOAD, EMPTY_VALUE",
        "COMPOSITE_PK" => "TO_CHAR(ID,'TM9'), CODE, PAYLOAD",
        "HEAP" | "GROWING" => "TO_CHAR(ID,'TM9'), PAYLOAD",
        _ => "ID, PAYLOAD",
    };
    let sql = format!("SELECT {projection} FROM OO_FIXTURE.{table} AS OF SCN {scn}");
    let mut result = Rows::new();
    for row in conn.query(&sql, &[]).unwrap() {
        let row = row.unwrap();
        let width = if matches!(table, "NUMERIC_PK" | "COMPOSITE_PK") {
            3
        } else {
            2
        };
        let cells: Vec<Option<String>> = (0..width).map(|i| row.get(i).unwrap()).collect();
        assert!(
            result
                .insert(row_key(&cells, keys), row_hash(&cells))
                .is_none()
        );
    }
    result
}

struct StopWriter(Arc<AtomicBool>);
impl Drop for StopWriter {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
#[ignore = "requires the owned --lob-only fixture"]
async fn oracle_lob_budget_and_hash() {
    let path = std::env::var("OO_ORACLE_FIXTURE_CONFIG").unwrap();
    let f: Fixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let table = TableRef {
        schema: "OO_FIXTURE".into(),
        name: "LOB_EDGES".into(),
    };
    let mut config = OracleConnectConfig::new(
        "127.0.0.1",
        f.port,
        &f.service,
        "OO_READER",
        Zeroizing::new(f.reader_password.clone()),
    );
    config.scope = vec![table.clone()];
    let mut engine = engine_connect(&f, config).unwrap();
    let probe = engine.probe().await.unwrap();
    let budget = ReadBudget {
        max_chunk_bytes: 1024 * 1024,
        duty_cycle_percent: 100,
        ..ReadBudget::default()
    };
    let plan = engine.plan_chunks(&table, &budget).await.unwrap();
    let mut rows = Rows::new();
    for chunk in &plan.chunks {
        let mut sink = Sink {
            keys: 1,
            rows: Rows::new(),
            ipc: None,
        };
        let receipt = engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
        if let Some(mut ipc) = sink.ipc {
            ipc.finish().unwrap();
            let bytes = ipc.into_inner().unwrap();
            assert_eq!(receipt.bytes, bytes.len() as u64);
            assert_eq!(receipt.sha256, format!("{:x}", Sha256::digest(bytes)));
        }
        rows.extend(sink.rows);
    }
    let expected = [
        vec![
            Some("1".into()),
            Some("x".repeat(24_000)),
            Some("\u{4e2d}\u{1f600}".repeat(6000)),
            Some("00ff".repeat(12_000)),
        ],
        vec![
            Some("2".into()),
            Some("".into()),
            Some("".into()),
            Some("".into()),
        ],
        vec![Some("3".into()), None, None, None],
    ]
    .into_iter()
    .map(|row| (row_key(&row, 1), row_hash(&row)))
    .collect::<Rows>();
    assert_eq!(
        rows, expected,
        "all LOB bytes, empty locators and NULLs must survive"
    );
    let tiny = ReadBudget {
        max_chunk_bytes: 4096,
        ..budget
    };
    let mut sink = Sink {
        keys: 1,
        rows: Rows::new(),
        ipc: None,
    };
    let error = engine
        .read_chunk(&plan.chunks[0], &tiny, &mut sink)
        .await
        .unwrap_err();
    assert_eq!(error.fact(), Some("lob_value_over_budget"));
    assert!(sink.rows.is_empty(), "over-limit row must not be delivered");
    std::fs::write(
        f.out.join("lob-result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": probe.version, "rows": rows.len(), "sha256_verified": true,
            "empty_distinct_from_null": true, "oversize_error": error.fact(),
        }))
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "requires the owned fixture from testing/engines/oracle/run.py"]
async fn oracle_engine_t0_t1_t8() {
    let path = std::env::var("OO_ORACLE_FIXTURE_CONFIG").expect("owned fixture config required");
    let f: Fixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let tables = [
        "NUMERIC_PK",
        "TEXT_PK",
        "COMPOSITE_PK",
        "UNIQUE_ONLY",
        "HEAP",
        "GROWING",
    ];
    let table_ref = |name: &str| TableRef {
        schema: "OO_FIXTURE".into(),
        name: name.into(),
    };
    let mut config = OracleConnectConfig::new(
        "127.0.0.1",
        f.port,
        &f.service,
        "OO_READER",
        Zeroizing::new(f.reader_password.clone()),
    );
    config.scope = tables.iter().map(|t| table_ref(t)).collect();
    let mut engine = match engine_connect(&f, config) {
        Ok(e) => e,
        Err(e) => {
            std::fs::write(f.out.join("connect-error.txt"), format!("{e:?}\n")).unwrap();
            panic!("fixture connection failed: {e:?}");
        }
    };
    let probe = engine.probe().await.unwrap();
    std::fs::write(
        f.out.join("probe.json"),
        serde_json::to_vec_pretty(&probe).unwrap(),
    )
    .unwrap();
    assert_eq!(probe.version.major, f.version);
    assert_eq!(probe.settings["logminer.dictionary"], "online_catalog");
    assert!(probe.settings["logminer.redo_log_catalog"].starts_with("disabled"));
    assert_eq!(
        probe.capabilities[&Capability::ChunkedSnapshot],
        CapabilityState::Available
    );
    assert!(
        !probe.requirements().is_empty(),
        "fixture intentionally lacks C1 setup"
    );
    let budget = ReadBudget {
        max_chunk_rows: 100_000,
        statement_timeout_ms: 60_000,
        ..ReadBudget::default()
    };
    let mut plans = Vec::new();
    for table in tables {
        let mut table_budget = budget.clone();
        table_budget.max_chunk_rows = match table {
            "TEXT_PK" => 2,
            "COMPOSITE_PK" | "UNIQUE_ONLY" => 500,
            "HEAP" => 1000,
            _ => 100_000,
        };
        let plan = engine
            .plan_chunks(&table_ref(table), &table_budget)
            .await
            .unwrap_or_else(|e| panic!("plan {table}: {e:?}; ORA={:?}", engine.last_ora_code()));
        std::fs::write(
            f.out.join(format!("plan-{table}.json")),
            serde_json::to_vec_pretty(&(plan.clone(), engine.plan_facts(&plan.table))).unwrap(),
        )
        .unwrap();
        if table == "NUMERIC_PK" {
            assert!(plan.chunks.len() >= 20);
        }
        if table == "UNIQUE_ONLY" {
            assert!(matches!(plan.key, KeyShape::UniqueIndex { .. }));
        }
        if table == "HEAP" {
            assert!(matches!(plan.strategy, ChunkStrategy::PhysicalRange { .. }));
            assert!(plan.chunks.len() > 1);
        }
        plans.push((plan, table_budget));
    }
    let reference_conn = connect(&f, false);
    let writer_conn = connect(&f, true);
    let stop = StopWriter(Arc::new(AtomicBool::new(false)));
    let signal = stop.0.clone();
    let writer = std::thread::spawn(move || {
        let mut cycles = 0u64;
        while !signal.load(Ordering::SeqCst) {
            cycles += 1;
            execute(
                &writer_conn,
                &format!("UPDATE NUMERIC_PK SET PAYLOAD='changed-{cycles}' WHERE ID <= 10"),
            );
            execute(&writer_conn, "DELETE FROM GROWING WHERE ID=1");
            execute(
                &writer_conn,
                &format!("INSERT INTO GROWING VALUES ({}, 'new')", 10000 + cycles),
            );
            execute(
                &writer_conn,
                &format!(
                    "UPDATE COMPOSITE_PK SET ID={} WHERE CODE='1'",
                    500 + cycles % 2
                ),
            );
            writer_conn.commit().unwrap();
            std::thread::sleep(Duration::from_millis(200));
        }
        cycles
    });
    let mut results = Vec::new();
    for (plan, table_budget) in &plans {
        let keys = if plan.table.name == "COMPOSITE_PK" {
            2
        } else {
            1
        };
        let mut target = Rows::new();
        let mut lows = Vec::new();
        let mut pending: VecDeque<ChunkSpec> = plan.chunks.clone().into();
        let mut splits = 0;
        while let Some(chunk) = pending.pop_front() {
            let mut sink = Sink {
                keys,
                rows: Rows::new(),
                ipc: None,
            };
            let receipt = match engine.read_chunk(&chunk, table_budget, &mut sink).await {
                Ok(receipt) => receipt,
                Err(e)
                    if e.fact() == Some("chunk_over_budget_split")
                        && matches!(plan.strategy, ChunkStrategy::Keyset { .. }) =>
                {
                    // Failed sinks are private staging: discard before retry.
                    splits += 1;
                    assert!(splits < 1000, "unbounded split retry");
                    let children = engine.split_chunk(&chunk, table_budget).unwrap();
                    for child in children.into_iter().rev() {
                        pending.push_front(child);
                    }
                    continue;
                }
                Err(e) => panic!(
                    "read {} chunk {}: {e:?}; ORA={:?}",
                    plan.table,
                    chunk.chunk_id,
                    engine.last_ora_code()
                ),
            };
            if let Some(mut ipc) = sink.ipc {
                ipc.finish().unwrap();
                let bytes = ipc.into_inner().unwrap();
                assert_eq!(receipt.bytes, bytes.len() as u64);
                assert_eq!(receipt.sha256, format!("{:x}", Sha256::digest(bytes)));
            }
            let Some(Watermark::OracleScn { scn }) = receipt.low else {
                panic!("SCN")
            };
            lows.push((chunk, scn));
            for (key, hash) in sink.rows {
                assert!(target.insert(key, hash).is_none(), "overlapping chunks");
            }
        }
        let Some(Watermark::OracleScn { scn: high }) = engine.watermark().await.unwrap() else {
            panic!("SCN")
        };
        let physical = matches!(plan.strategy, ChunkStrategy::PhysicalRange { .. });
        let reference_scn = if physical { lows[0].1 } else { high };
        let expected = reference(&reference_conn, &plan.table.name, reference_scn, keys);
        let mut corrections = 0;
        if !physical {
            for (chunk, low) in lows {
                let changed = engine
                    .changed_keys(
                        &plan.table,
                        chunk.lo.as_deref(),
                        chunk.hi.as_deref(),
                        low,
                        high,
                    )
                    .unwrap();
                for key in changed {
                    let key: Vec<Option<String>> = key
                        .into_iter()
                        .map(|k| {
                            Some(match k {
                                KeyValue::Int(n) => n.to_string(),
                                KeyValue::Text(s) | KeyValue::Decimal(s) => s,
                                _ => panic!("fixture key"),
                            })
                        })
                        .collect();
                    let key = row_key(&key, keys);
                    target.remove(&key);
                    if let Some(hash) = expected.get(&key) {
                        target.insert(key, *hash);
                    }
                    corrections += 1;
                }
            }
        }
        assert_eq!(target.len(), expected.len(), "row count: {}", plan.table);
        let diff = target
            .iter()
            .filter(|(k, v)| expected.get(*k) != Some(*v))
            .count();
        assert_eq!(diff, 0, "row hash diff: {}", plan.table);
        results.push(serde_json::json!({
            "table": plan.table.name, "rows": expected.len(), "diff": diff,
            "corrections": corrections, "reference_scn": reference_scn,
                "growth_splits": splits,
            "target": "in-memory canonical row hashes (not Agent DB)",
        }));
        let digest1 = engine.digest_range(&plan.table, None, None).await.unwrap();
        assert!(!digest1.digest.is_empty());
    }
    drop(stop);
    let cycles = writer.join().unwrap();
    assert!(cycles > 0);
    assert_eq!(engine.stats().sessions, 1);
    assert!(engine.stats().paced_sleep_ms + 100 >= engine.stats().read_ms * 3);

    // Delimiter-like data must not alias a different row's digest.
    let admin = connect(&f, true);
    let edge = table_ref("DIGEST_EDGE");
    let before = engine.digest_range(&edge, None, None).await.unwrap();
    execute(&admin, "UPDATE DIGEST_EDGE SET A='x', B='y'||CHR(31)||'vz'");
    admin.commit().unwrap();
    let after = engine.digest_range(&edge, None, None).await.unwrap();
    assert_eq!(before.rows, after.rows);
    assert_ne!(before.digest, after.digest);
    execute(&admin, "INSERT INTO DIGEST_EDGE VALUES (NULL, 'tail')");
    admin.commit().unwrap();
    let null_digest = engine.digest_range(&edge, None, None).await.unwrap();
    execute(&admin, "UPDATE DIGEST_EDGE SET A='' WHERE A IS NULL");
    admin.commit().unwrap();
    assert_eq!(
        null_digest,
        engine.digest_range(&edge, None, None).await.unwrap()
    );

    // Growing last ranges must fail atomically, then split without truncation.
    let growth_budget = ReadBudget {
        max_chunk_rows: 100,
        ..budget.clone()
    };
    let growth = engine
        .plan_chunks(&table_ref("GROWING"), &growth_budget)
        .await
        .unwrap();
    execute(
        &admin,
        "INSERT INTO GROWING SELECT 30000+LEVEL, 'growth' FROM DUAL CONNECT BY LEVEL<=1001",
    );
    admin.commit().unwrap();
    let last = growth.chunks.last().unwrap();
    let failure = engine
        .read_chunk(
            last,
            &growth_budget,
            &mut Sink {
                keys: 1,
                rows: Rows::new(),
                ipc: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.fact(), Some("chunk_over_budget_split"));
    let splits = engine.split_chunk(last, &growth_budget).unwrap();
    assert!(splits.len() > 1);
    let mut split_rows = Rows::new();
    for split in splits {
        let mut sink = Sink {
            keys: 1,
            rows: Rows::new(),
            ipc: None,
        };
        engine
            .read_chunk(&split, &growth_budget, &mut sink)
            .await
            .unwrap();
        for (key, hash) in sink.rows {
            assert!(split_rows.insert(key, hash).is_none());
        }
    }
    assert!(split_rows.len() >= 1001);

    // An actually active fixture session must pause the read and hit the gate.
    let mut config = OracleConnectConfig::new(
        "127.0.0.1",
        f.port,
        &f.service,
        "OO_READER",
        Zeroizing::new(f.reader_password.clone()),
    );
    config.active_session_limit = Some(0);
    config.pause_poll = Duration::from_millis(20);
    config.pause_max = Duration::from_millis(100);
    let mut guarded = engine_connect(&f, config).unwrap();
    let guarded_plan = guarded
        .plan_chunks(&table_ref("TEXT_PK"), &budget)
        .await
        .unwrap();
    let sleep_sql = if f.version == 11 {
        "BEGIN DBMS_LOCK.SLEEP(2); END;"
    } else {
        "BEGIN DBMS_SESSION.SLEEP(2); END;"
    };
    let sleeper = std::thread::spawn(move || {
        execute(&admin, sleep_sql);
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let busy = guarded
        .read_chunk(
            &guarded_plan.chunks[0],
            &budget,
            &mut Sink {
                keys: 1,
                rows: Rows::new(),
                ipc: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(busy.fact(), Some("source_busy"));
    sleeper.join().unwrap();

    let timeout_budget = ReadBudget {
        statement_timeout_ms: 1,
        ..budget.clone()
    };
    let timed_out = engine
        .read_chunk(
            &plans[0].0.chunks[0],
            &timeout_budget,
            &mut Sink {
                keys: 1,
                rows: Rows::new(),
                ipc: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        timed_out.kind(),
        oo_connectors::SourceErrorKind::Budget,
        "timeout result: {timed_out:?}; ORA={:?}",
        engine.last_ora_code()
    );
    std::fs::write(
        f.out.join("result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "tables": results, "writer_cycles": cycles, "stats": engine.stats(),
            "active_gate": guarded.stats(), "timeout": timed_out.fact(),
            "digest_collision_regression": "passed", "growth_split_rows": split_rows.len()
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(engine.stats().paced_sleep_ms > 0);
}
