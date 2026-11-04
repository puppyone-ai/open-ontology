//! Owned S2 fixture, compiled as an Oracle engine unit-test module.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::SourceError;
use crate::engine::*;
use crate::oracle::engine::{OracleConnectConfig, OracleEngine, map_ora as thin_error};
use crate::oracle::worker::{DriverError, Worker};
use arrow_array::{Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
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
        "C5_HINT" => "TO_CHAR(ID,'TM9'), PAYLOAD, TO_CHAR(UPDATED_AT,'YYYY-MM-DD\"T\"HH24:MI:SS')",
        "COMPOSITE_PK" => "TO_CHAR(ID,'TM9'), CODE, PAYLOAD",
        "HEAP" | "GROWING" => "TO_CHAR(ID,'TM9'), PAYLOAD",
        _ => "ID, PAYLOAD",
    };
    let sql = format!("SELECT {projection} FROM OO_FIXTURE.{table} AS OF SCN {scn}");
    let mut result = Rows::new();
    for row in conn.query(&sql, &[]).unwrap() {
        let row = row.unwrap();
        let width = if matches!(table, "NUMERIC_PK" | "COMPOSITE_PK" | "C5_HINT") {
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

use crate::oracle::engine::c5::OracleC5;
use crate::oracle::engine::logminer::OracleLogMiner;

fn position(w: &Watermark) -> u64 {
    match w {
        Watermark::OracleScn { scn } => *scn,
        _ => panic!("Oracle SCN required"),
    }
}

fn encoded_key(key: &[KeyValue]) -> String {
    let cells: Vec<Option<String>> = key
        .iter()
        .map(|key| {
            Some(match key {
                KeyValue::Int(n) => n.to_string(),
                KeyValue::Decimal(s) | KeyValue::Text(s) => s.clone(),
                _ => panic!("fixture key"),
            })
        })
        .collect();
    row_key(&cells, key.len())
}

struct WindowSink {
    inner: Sink,
    writer: Option<Connection>,
}

impl ChunkSink for WindowSink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.inner.push(batch)?;
        if let Some(writer) = self.writer.take() {
            execute(&writer, "INSERT INTO GROWING VALUES(20001,'window')");
            execute(&writer, "UPDATE GROWING SET PAYLOAD='window' WHERE ID=50");
            execute(&writer, "UPDATE GROWING SET ID=-1 WHERE ID=9000");
            execute(&writer, "DELETE FROM GROWING WHERE ID=60");
            writer.commit().unwrap();
        }
        Ok(())
    }
}

#[tokio::test]
#[ignore = "owned incremental fixture only"]
async fn real_incremental_cycles() {
    let f: Fixture = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_ORACLE_FIXTURE_CONFIG").unwrap()).unwrap(),
    )
    .unwrap();
    let table = TableRef {
        schema: "OO_FIXTURE".into(),
        name: "GROWING".into(),
    };
    let mut cfg = OracleConnectConfig::new(
        "127.0.0.1",
        f.port,
        &f.service,
        "OO_READER",
        Zeroizing::new(f.reader_password.clone()),
    );
    cfg.scope = vec![table.clone()];
    let mut engine = engine_connect(&f, cfg).unwrap();
    let writer = connect(&f, true);
    let reference_conn = connect(&f, false);
    let budget = ReadBudget {
        max_chunk_rows: 20000,
        max_chunk_bytes: 32 * 1024 * 1024,
        statement_timeout_ms: 120000,
        duty_cycle_percent: 50,
        ..ReadBudget::default()
    };
    let probe = engine.probe().await.unwrap();
    assert!(matches!(
        &probe.capabilities[&Capability::LogCdc],
        CapabilityState::NeedsSetup { requirements }
            if requirements.iter().any(|r| r.kind == RequirementKind::Feature
                && r.required.contains("Explicit Oracle C1 opt-in"))
    ));
    std::fs::write(
        f.out.join("probe.json"),
        serde_json::to_vec_pretty(&probe).unwrap(),
    )
    .unwrap();
    let plan = engine
        .plan_chunks(&table, &budget)
        .await
        .unwrap_or_else(|e| panic!("probe-first plan: {e:?}; ORA={:?}", engine.last_ora_code()));
    std::fs::write(
        f.out.join("probe-first-plan.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "order": ["probe", "plan", "read"],
            "plan": plan,
            "facts": engine.plan_facts(&table),
            "stats": engine.stats(),
        }))
        .unwrap(),
    )
    .unwrap();
    let mut miner = f.miner_password.as_ref().map(|password| {
        let cfg = OracleConnectConfig::new(
            "127.0.0.1",
            f.port,
            f.root_service.as_ref().unwrap(),
            if f.version == 11 {
                "OO_MINER"
            } else {
                "C##OO_MINER"
            },
            Zeroizing::new(password.clone()),
        );
        let mut connection = engine_connect(&f, cfg).unwrap();
        let session_sql = if f.version == 11 {
            "SELECT SYS_CONTEXT('USERENV','SESSIONID'), TO_CHAR(sid)||'.'||TO_CHAR(serial#), NULL FROM v$session WHERE sid=TO_NUMBER(SYS_CONTEXT('USERENV','SID'))"
        } else {
            "SELECT SYS_CONTEXT('USERENV','SESSIONID'), TO_CHAR(sid)||'.'||TO_CHAR(serial#), SYS_CONTEXT('USERENV','UNIFIED_AUDIT_SESSIONID') FROM v$session WHERE sid=TO_NUMBER(SYS_CONTEXT('USERENV','SID'))"
        };
        let session = connection
            .query(
                session_sql,
                &[],
                1,
            )
            .unwrap();
        let mut sessions = BTreeMap::from([(
            session[0][0].clone().unwrap(),
            session[0][1].clone().unwrap(),
        )]);
        if let Some(unified_id) = &session[0][2] {
            sessions.insert(unified_id.clone(), session[0][1].clone().unwrap());
        }
        std::fs::write(
            f.out.join("miner-session.json"),
            serde_json::to_vec_pretty(&sessions).unwrap(),
        )
        .unwrap();
        OracleLogMiner::new(connection, &mut engine, &[table.clone()]).unwrap()
    });
    let mut watermark = engine.watermark().await.unwrap().unwrap();
    if let Some(miner) = &mut miner {
        assert!(position(&miner.earliest_available().await.unwrap()) <= position(&watermark));
    }
    let mut target = Rows::new();
    let mut chunk_highs = Vec::new();
    for (i, chunk) in plan.chunks.iter().enumerate() {
        let mut sink = WindowSink {
            inner: Sink {
                keys: 1,
                rows: Rows::new(),
                ipc: None,
            },
            writer: (miner.is_some() && i == 0).then(|| connect(&f, true)),
        };
        let receipt = engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
        chunk_highs.push(position(receipt.high.as_ref().unwrap()));
        target.extend(sink.inner.rows);
    }
    let mut c5_ranges: BTreeMap<u32, Rows> = BTreeMap::new();
    let mut scheduler = OracleC5::new(plan.clone(), None, 3).unwrap();
    let mut c1_generation = 0;
    let mut results = Vec::new();
    for cycle in 0..3 {
        if cycle == 0 {
            // Commit in cycle 1, after cycle 0 advances the commit watermark.
            execute(&writer, "UPDATE GROWING SET PAYLOAD='long-tx' WHERE ID=7");
        }
        if cycle == 1 {
            execute(&writer, "INSERT INTO GROWING VALUES(10001,'new')");
            execute(&writer, "UPDATE GROWING SET PAYLOAD='changed' WHERE ID=5");
            execute(&writer, "UPDATE GROWING SET ID=0 WHERE ID=10000");
            execute(&writer, "DELETE FROM GROWING WHERE ID=6");
            writer.commit().unwrap();
        }
        let mut c1_events = 0;
        let mut c1_payload = 0;
        if let Some(miner) = &mut miner {
            let batch = miner
                .next_batch(&watermark, &budget)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "C1 cycle {cycle}: {e:?}; ORA={:?}; rebaseline={:?}",
                        miner.last_ora_code(),
                        miner.rebaseline_tables()
                    )
                })
                .unwrap();
            assert!(position(&batch.position) >= position(&watermark));
            assert_eq!(batch.changes.len(), batch.images.len());
            c1_events = batch.changes.len();
            c1_payload = batch
                .images
                .iter()
                .map(|image| image.get_array_memory_size())
                .sum::<usize>();
            if cycle == 0 {
                assert!(
                    chunk_highs
                        .iter()
                        .all(|high| *high <= position(&batch.position))
                );
                assert!(c1_events >= 4, "must correct real in-window source changes");
            }
            for (change, image) in batch.changes.iter().zip(batch.images) {
                match change {
                    RowChange::Delete { key, .. } => {
                        target.remove(&encoded_key(key));
                    }
                    RowChange::Insert { key, .. } | RowChange::Update { key, .. } => {
                        if let RowChange::Update {
                            old_key: Some(old), ..
                        } = change
                        {
                            target.remove(&encoded_key(old));
                        }
                        let mut sink = Sink {
                            keys: 1,
                            rows: Rows::new(),
                            ipc: None,
                        };
                        sink.push(image).unwrap();
                        assert!(sink.rows.contains_key(&encoded_key(key)));
                        target.extend(sink.rows);
                    }
                }
            }
            watermark = batch.position;
            if c1_events > 0 {
                c1_generation += 1;
            }
            let expected = reference(&reference_conn, &table.name, position(&watermark), 1);
            assert_eq!(target, expected, "C1 row diff in cycle {cycle}");
        }
        let staged = scheduler.cycle(&mut engine, &budget).await.unwrap();
        let payload = staged.metrics.payload_bytes;
        let rewrites = staged.metrics.rewrites;
        let c5_generation = staged.metrics.generation;
        for (chunk, batches) in staged.replacements {
            let mut sink = Sink {
                keys: 1,
                rows: Rows::new(),
                ipc: None,
            };
            for batch in batches {
                sink.push(batch).unwrap();
            }
            c5_ranges.insert(chunk.chunk_id, sink.rows);
        }
        let high = engine.watermark().await.unwrap().unwrap();
        let c5: Rows = c5_ranges.values().flat_map(|rows| rows.clone()).collect();
        let expected = reference(&reference_conn, &table.name, position(&high), 1);
        assert_eq!(c5, expected, "C5 row diff in cycle {cycle}");
        if cycle == 2 {
            assert_eq!(payload, 0);
            assert_eq!(rewrites, 0);
            assert_eq!(c5_generation, 2);
            assert_eq!(c1_events, 0);
            assert_eq!(c1_payload, 0);
            if miner.is_some() {
                assert_eq!(c1_generation, 2);
            }
        }
        results.push(serde_json::json!({
            "cycle": cycle, "rows": expected.len(), "diff": 0,
            "c1_events": c1_events, "c1_generation": c1_generation,
            "c1_payload_bytes": c1_payload, "c1_position": watermark,
            "chunk_window_correction": miner.is_some() && cycle == 0,
            "c5_payload_bytes": payload, "c5_rewrites": rewrites,
            "c5_generation": c5_generation, "position": high, "c5_layers": staged.metrics,
        }));
        std::fs::write(
            f.out.join("incremental-result.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": f.version, "cycles": results, "c1_exercised": miner.is_some(),
                "target": "in-memory row hashes, not Agent DB",
                "c5_layers": "OracleC5: optional time hint, recent undo window, chunk digest, periodic full read"
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let full = scheduler.cycle(&mut engine, &budget).await.unwrap();
    assert_eq!(full.metrics.full_reads, plan.chunks.len());
    assert!(full.metrics.source_read_bytes > 0);
    assert_eq!(full.metrics.payload_bytes, 0);
    assert_eq!(full.metrics.rewrites, 0);
    assert_eq!(full.metrics.generation, 2);
    std::fs::write(
        f.out.join("c5-full-read.json"),
        serde_json::to_vec_pretty(&full.metrics).unwrap(),
    )
    .unwrap();
    if let Some(miner) = &mut miner {
        execute(&writer, "ALTER TABLE GROWING ADD EXTRA_VALUE VARCHAR2(20)");
        let error = miner.next_batch(&watermark, &budget).await.unwrap_err();
        assert!(matches!(
            error.fact(),
            Some("logminer_table_rebaseline" | "logminer_dictionary_rebaseline")
        ));
        assert_eq!(miner.rebaseline_tables().len(), 1);
        assert!(miner.rebaseline_tables().contains(&table));
        std::fs::write(
            f.out.join("dictionary-rebaseline.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "affected_table": table, "fact": error.fact(),
                "watermark_not_advanced": watermark
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let hint_table = TableRef {
        schema: "OO_FIXTURE".into(),
        name: "C5_HINT".into(),
    };
    let hint_budget = ReadBudget {
        max_chunk_rows: 30,
        ..budget.clone()
    };
    let hint_plan = engine.plan_chunks(&hint_table, &hint_budget).await.unwrap();
    let mut hinted = OracleC5::new(hint_plan.clone(), Some("UPDATED_AT".into()), 10).unwrap();
    let mut hint_ranges: BTreeMap<u32, Rows> = BTreeMap::new();
    let mut hint_results = Vec::new();
    for cycle in 0..4 {
        if cycle == 1 {
            // Stale/NULL timestamps and deletes must not be skipped.
            execute(&writer, "UPDATE C5_HINT SET PAYLOAD='stale' WHERE ID=2");
            execute(&writer, "UPDATE C5_HINT SET PAYLOAD='null' WHERE ID=1");
            execute(&writer, "DELETE FROM C5_HINT WHERE ID=4");
            writer.commit().unwrap();
        }
        if cycle == 2 {
            execute(
                &writer,
                "UPDATE C5_HINT SET UPDATED_AT=DATE '2026-01-01' WHERE ID=99",
            );
            writer.commit().unwrap();
        }
        let result = hinted.cycle(&mut engine, &hint_budget).await.unwrap();
        assert_eq!(result.metrics.digests, hint_plan.chunks.len());
        if cycle == 1 {
            assert_eq!(result.metrics.time_hints, 0);
            assert!(result.metrics.rewrites > 0);
        }
        if cycle == 2 {
            assert!(result.metrics.time_hints > 0);
        }
        if cycle == 3 {
            assert_eq!(result.metrics.payload_bytes, 0);
            assert_eq!(result.metrics.rewrites, 0);
            assert_eq!(result.metrics.generation, 3);
        }
        for (chunk, batches) in result.replacements {
            let mut sink = Sink {
                keys: 1,
                rows: Rows::new(),
                ipc: None,
            };
            for batch in batches {
                sink.push(batch).unwrap();
            }
            hint_ranges.insert(chunk.chunk_id, sink.rows);
        }
        let high = engine.current_scn().unwrap();
        let actual: Rows = hint_ranges.values().flat_map(|r| r.clone()).collect();
        assert_eq!(actual, reference(&reference_conn, "C5_HINT", high, 1));
        hint_results.push(result.metrics);
    }
    std::fs::write(
        f.out.join("c5-hints.json"),
        serde_json::to_vec_pretty(&hint_results).unwrap(),
    )
    .unwrap();
}
