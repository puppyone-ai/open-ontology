use super::*;
use oo_connectors::oracle::engine::logminer::OracleLogMiner;

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
    let plan = engine.plan_chunks(&table, &budget).await.unwrap();
    let mut miner = f.miner_password.as_ref().map(|password| {
        let cfg = OracleConnectConfig::new(
            "127.0.0.1",
            f.port,
            f.root_service.as_ref().unwrap(),
            "C##OO_MINER",
            Zeroizing::new(password.clone()),
        );
        OracleLogMiner::new(
            OracleEngine::connect(cfg).unwrap(),
            &mut engine,
            std::slice::from_ref(&table),
        )
        .unwrap()
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
    let mut digests = BTreeMap::new();
    let mut c5_generation = 0;
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
        let mut payload = 0u64;
        let mut rewrites = 0;
        for chunk in &plan.chunks {
            let digest = engine
                .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .unwrap();
            if digests.get(&chunk.chunk_id) != Some(&digest) {
                let mut sink = Sink {
                    keys: 1,
                    rows: Rows::new(),
                    ipc: None,
                };
                let receipt = engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
                payload += receipt.bytes;
                rewrites += 1;
                // No concurrent writer in these C5 cycles; recheck before staging.
                assert_eq!(
                    digest,
                    engine
                        .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                        .await
                        .unwrap()
                );
                c5_ranges.insert(chunk.chunk_id, sink.rows);
                digests.insert(chunk.chunk_id, digest);
            }
        }
        if rewrites > 0 {
            c5_generation += 1;
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
            "c5_generation": c5_generation, "position": high,
        }));
        std::fs::write(
            f.out.join("incremental-result.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": f.version, "cycles": results, "c1_exercised": miner.is_some(),
                "target": "in-memory row hashes, not Agent DB",
                "c5_layers": "digest only; shared four-layer scheduler integration pending"
            }))
            .unwrap(),
        )
        .unwrap();
    }
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
}
