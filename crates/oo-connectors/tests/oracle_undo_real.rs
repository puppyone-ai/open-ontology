#![cfg(feature = "oracle")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use arrow_array::{Array, RecordBatch, StringArray};
use oo_connectors::SourceError;
use oo_connectors::engine::*;
use oo_connectors::oracle::engine::{
    OracleConnectConfig, OracleEngine, RetryableChunkSink, map_ora,
};
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
    rows: BTreeMap<String, String>,
    batches: Vec<usize>,
    discarded_rows: Vec<usize>,
    start: Option<mpsc::Sender<()>>,
    finished: mpsc::Receiver<()>,
}

impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.batches.push(batch.num_rows());
        let id = batch
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Int64Array>()
            .unwrap();
        let value = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        for i in 0..batch.num_rows() {
            assert!(!id.is_null(i));
            assert!(
                self.rows
                    .insert(id.value(i).to_string(), value.value(i).into())
                    .is_none(),
                "retry duplicated a key"
            );
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

impl RetryableChunkSink for Sink {
    fn reset(&mut self) -> Result<(), SourceError> {
        self.discarded_rows.push(self.rows.len());
        self.rows.clear();
        self.batches.clear();
        Ok(())
    }
}

#[tokio::test]
#[ignore = "owned undo-stress fixture only; never run against a customer source"]
async fn real_ora_01555_restarts_whole_chunk_at_fresh_scn_with_zero_diff() {
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
        // Cross the already flushed cursor; resuming instead of restarting
        // would miss the moved row and retain the deleted old key.
        writer
            .execute("UPDATE UNDO_PROBE SET ID=0 WHERE ID=12000", &[])
            .map_err(|e| map_ora(&e))
            .unwrap();
        writer.commit().map_err(|e| map_ora(&e)).unwrap();
        finished.send(()).unwrap();
        transactions
    });
    let mut sink = Sink {
        rows: BTreeMap::new(),
        batches: Vec::new(),
        discarded_rows: Vec::new(),
        start: Some(start),
        finished: done,
    };
    let result = engine
        .read_chunk_recovering(&plan.chunks[0], &budget, &mut sink)
        .await;
    let transactions = writer.join().unwrap();
    let successful_receipt = result.is_ok();
    let error = result.as_ref().err().map(|e| e.fact());
    std::fs::write(
        f.out.join("undo-result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "transactions": transactions,
            "last_ora": engine.last_ora_code(),
            "stats": engine.stats(), "flushed_rows": sink.rows.len(),
            "flushed_batches": sink.batches,
            "discarded_rows": sink.discarded_rows,
            "successful_receipt": successful_receipt,
            "error": error,
            "interpretation": "fresh_scn_whole_chunk_retry_requires_independent_comparison"
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
    let receipt = result.expect("lost undo must recover, not merely fail closed");
    assert!(engine.stats().fresh_scn_retries > 0);
    assert!(sink.discarded_rows.iter().any(|n| *n > 4096));
    assert_eq!(receipt.rows, 12000);
    assert_eq!(sink.rows.len(), 12000);
    let cfg = oracledb::Config::default()
        .set_credentials("OO_READER", &f.reader_password)
        .set_connect_string(&format!("127.0.0.1:{}/{}", f.port, f.service))
        .unwrap();
    let reference = oracledb::connect(cfg).unwrap();
    let Some(Watermark::OracleScn { scn: high }) = receipt.high else {
        panic!("SCN")
    };
    let expected: BTreeMap<String, String> = reference
        .query(
            &format!("SELECT TO_CHAR(ID),PAYLOAD FROM OO_FIXTURE.UNDO_PROBE AS OF SCN {high}"),
            &[],
        )
        .unwrap()
        .map(|row| {
            let row = row.unwrap();
            (row.get(0).unwrap(), row.get(1).unwrap())
        })
        .collect();
    assert_eq!(sink.rows, expected, "fresh-SCN range must have zero diff");
    // The writer is stopped. A C5 range recheck must also remain stable;
    // the independent row comparison above verifies the staged payload.
    let digest = engine.digest_range(&plan.table, None, None).await.unwrap();
    assert_eq!(digest.rows, 12000);
    assert_eq!(
        digest,
        engine.digest_range(&plan.table, None, None).await.unwrap()
    );
    std::fs::write(
        f.out.join("undo-verified.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "rows": expected.len(), "diff": 0, "digest_recheck": true,
            "key_moved_behind_old_cursor": true, "receipt": receipt,
            "stats": engine.stats(),
            "target": "in-memory staging, not Agent DB"
        }))
        .unwrap(),
    )
    .unwrap();
}
