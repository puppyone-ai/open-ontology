#![cfg(feature = "oracle")]

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use arrow_array::{Array, RecordBatch, StringArray};
use oo_connectors::SourceError;
use oo_connectors::engine::*;
use oo_connectors::oracle::engine::{OracleConnectConfig, OracleEngine, map_ora};
use serde::Deserialize;
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Fixture {
    port: u16,
    service: String,
    reader_password: String,
    writer_password: String,
    out: PathBuf,
}

struct Sink {
    rows: usize,
    batches: Vec<usize>,
    start: Option<mpsc::Sender<()>>,
    finished: mpsc::Receiver<()>,
}

impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.batches.push(batch.num_rows());
        let id = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let value = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        for i in 0..batch.num_rows() {
            assert!(!id.is_null(i));
            assert_eq!(id.value(i).parse::<usize>().unwrap(), self.rows + 1);
            assert_eq!(
                value.value(i),
                "a".repeat(1000),
                "snapshot changed on retry"
            );
            self.rows += 1;
        }
        if let Some(start) = self.start.take() {
            start.send(()).unwrap();
            self.finished
                .recv_timeout(Duration::from_secs(180))
                .unwrap();
        }
        Ok(())
    }
}

#[tokio::test]
#[ignore = "owned undo-stress fixture only; never run against a customer source"]
async fn real_ora_01555_preserves_flushed_prefix_or_fails_closed() {
    let f: Fixture = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_ORACLE_FIXTURE_CONFIG").unwrap()).unwrap(),
    )
    .unwrap();
    let mut config = OracleConnectConfig::new(
        "127.0.0.1",
        f.port,
        &f.service,
        "OO_READER",
        Zeroizing::new(f.reader_password.clone()),
    );
    config.snapshot_retries = 4;
    let mut engine = OracleEngine::connect(config).unwrap();
    let budget = ReadBudget {
        max_chunk_rows: 20000,
        max_chunk_bytes: 32 * 1024 * 1024,
        statement_timeout_ms: 240000,
        duty_cycle_percent: 25,
        ..ReadBudget::default()
    };
    let plan = engine
        .plan_chunks(
            &TableRef {
                schema: "OO_FIXTURE".into(),
                name: "UNDO_PROBE".into(),
            },
            &budget,
        )
        .await
        .unwrap();
    assert_eq!(plan.chunks.len(), 1);
    let (start, wait) = mpsc::channel();
    let (finished, done) = mpsc::channel();
    let cfg = oracledb::Config::default()
        .set_credentials("OO_FIXTURE", &f.writer_password)
        .set_connect_string(&format!("127.0.0.1:{}/{}", f.port, f.service))
        .map_err(|e| map_ora(&e))
        .unwrap();
    let writer = oracledb::connect(cfg).map_err(|e| map_ora(&e)).unwrap();
    let writer = std::thread::spawn(move || {
        wait.recv_timeout(Duration::from_secs(60)).unwrap();
        let mut transactions = 0;
        for cycle in 0..12 {
            for lo in (8193..=12000).step_by(32) {
                let sql = format!(
                    "UPDATE UNDO_PROBE SET PAYLOAD=RPAD('{cycle}',1000,'x') WHERE ID BETWEEN {lo} AND {}",
                    (lo + 31).min(12000),
                );
                writer.execute(&sql, &[]).map_err(|e| map_ora(&e)).unwrap();
                writer.commit().map_err(|e| map_ora(&e)).unwrap();
                transactions += 1;
            }
        }
        finished.send(()).unwrap();
        transactions
    });
    let mut sink = Sink {
        rows: 0,
        batches: Vec::new(),
        start: Some(start),
        finished: done,
    };
    let result = engine.read_chunk(&plan.chunks[0], &budget, &mut sink).await;
    let transactions = writer.join().unwrap();
    let successful_receipt = result.is_ok();
    let error = result.as_ref().err().map(|e| e.fact());
    std::fs::write(
        f.out.join("undo-result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "transactions": transactions,
            "last_ora": engine.last_ora_code(),
            "stats": engine.stats(), "flushed_rows": sink.rows,
            "flushed_batches": sink.batches,
            "flushed_prefix_diff": 0, "successful_receipt": successful_receipt,
            "error": error,
            "interpretation": if successful_receipt { "recovered_same_snapshot" }
                else { "undo_lost_same_scn_retry_exhausted_not_recovery_acceptance" }
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        engine.last_ora_code(),
        Some(1555),
        "must observe real ORA-01555"
    );
    assert!(
        engine.stats().snapshot_retries > 0,
        "must exercise shrink/retry"
    );
    assert!(
        sink.rows > 4096,
        "retry must continue beyond the first flushed batch"
    );
    if let Ok(receipt) = result {
        assert_eq!(receipt.rows, 12000);
        assert_eq!(sink.rows, 12000);
    } else {
        assert_eq!(error, Some(Some("ora_01555_snapshot_too_old")));
        assert!(sink.rows < 12000);
        assert_eq!(
            engine.stats().chunks,
            0,
            "failed chunk must not have a receipt"
        );
    }
}
