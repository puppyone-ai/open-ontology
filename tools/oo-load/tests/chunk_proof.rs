//! Deterministic admin interleavings; the product only reads owned fixtures.
use arrow_array::{Int64Array, RecordBatch};
use oo_connectors::engine::*;
use oo_connectors::mysql::{EngineOptions, MysqlConfig, MysqlEngine};
use oo_connectors::postgres::PostgresEngine;
use oo_connectors::*;
use serde_json::Value;
use std::{path::Path, time::Duration};

#[derive(Default)]
struct Sink(Vec<(i64, i64)>);
impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        let id = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        let value = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        self.0
            .extend((0..batch.num_rows()).map(|i| (id.value(i), value.value(i))));
        Ok(())
    }
}
trait Proven: SourceEngine {
    fn proof(&mut self) -> RangeDigest;
}
impl Proven for PostgresEngine {
    fn proof(&mut self) -> RangeDigest {
        self.last_chunk_digest
            .take()
            .expect("same-transaction proof")
    }
}
impl Proven for MysqlEngine {
    fn proof(&mut self) -> RangeDigest {
        self.last_chunk_digest
            .take()
            .expect("same-transaction proof")
    }
}
async fn admin(directory: &Path, name: &str, body: Value) {
    std::fs::write(
        directory.join(format!("{name}.tmp")),
        serde_json::to_vec(&body).unwrap(),
    )
    .unwrap();
    std::fs::rename(
        directory.join(format!("{name}.tmp")),
        directory.join(format!("{name}.json")),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !directory.join(format!("{name}.done")).exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "fixture admin handshake timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
async fn verify<E: Proven>(engine: &mut E, table: TableRef, directory: &Path) {
    let budget = ReadBudget {
        max_chunk_rows: 4,
        ..Default::default()
    };
    let mirror = TableRef {
        schema: table.schema.clone(),
        name: "proof_rows".into(),
    };
    let mut results = Vec::new();
    for case in ["update", "delete", "key_move"] {
        let plan = engine.plan_chunks(&table, &budget).await.unwrap();
        assert!(plan.chunks.len() > 1);
        let first = &plan.chunks[0];
        let discovery = engine
            .digest_range(&table, first.lo.as_deref(), first.hi.as_deref())
            .await
            .unwrap();
        admin(directory, &format!("{case}-mutate"), serde_json::json!({})).await;
        let mut published = Vec::new();
        let mut proofs = Vec::new();
        for (index, chunk) in plan.chunks.iter().enumerate() {
            let mut sink = Sink::default();
            let receipt = engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
            let proof = engine.proof();
            assert_eq!(receipt.rows, proof.rows);
            assert!(
                receipt.rows <= budget.max_chunk_rows && receipt.bytes <= budget.max_chunk_bytes
            );
            assert_eq!((&proof.lo, &proof.hi), (&chunk.lo, &chunk.hi));
            if index == 0 {
                assert_ne!(proof.digest, discovery.digest);
                // Restore A before later chunks: a published generation may span snapshots.
                admin(directory, &format!("{case}-restore"), serde_json::json!({})).await;
                let after = engine
                    .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                    .await
                    .unwrap();
                assert_eq!(discovery, after, "A-to-B-to-A fixture did not restore A");
            }
            published.extend(sink.0);
            proofs.push(proof);
        }
        admin(
            directory,
            &format!("{case}-mirror"),
            serde_json::json!(published),
        )
        .await;
        engine.plan_chunks(&mirror, &budget).await.unwrap();
        // Independently hash the exact transferred rows through the source canonical oracle.
        for proof in &proofs {
            let oracle = engine
                .digest_range(&mirror, proof.lo.as_deref(), proof.hi.as_deref())
                .await
                .unwrap();
            assert_eq!((proof.rows, &proof.digest), (oracle.rows, &oracle.digest));
        }
        let mut corrected = Vec::new();
        let mut changed = 0;
        for (chunk, previous) in plan.chunks.iter().zip(&proofs) {
            let current = engine
                .digest_range(&table, chunk.lo.as_deref(), chunk.hi.as_deref())
                .await
                .unwrap();
            changed += usize::from(current != *previous);
            let mut sink = Sink::default();
            engine.read_chunk(chunk, &budget, &mut sink).await.unwrap();
            assert_eq!(engine.proof(), current);
            corrected.extend(sink.0);
        }
        corrected.sort_unstable();
        assert_eq!(corrected, (1..=8).map(|n| (n * 10, 1)).collect::<Vec<_>>());
        assert!(changed > 0);
        results.push(serde_json::json!({"case":case,"range_proofs":proofs.len(),"changed_ranges":changed,"converged":true}));
    }
    std::fs::write(
        directory.join("proof-result.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "requires the owned tools/oo-test/chunk_proof.py fixture"]
async fn real_chunk_proofs_survive_aba_and_next_cycle_converges() {
    let path = std::env::var("OO_LOAD_PROOF_CONFIG").expect("private fixture config path");
    let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let directory = Path::new(&path).parent().unwrap();
    let password = std::fs::read_to_string(directory.join("source-password")).unwrap();
    let port = v["port"].as_u64().unwrap() as u16;
    let table = TableRef {
        schema: if v["engine"] == "postgres" {
            "public"
        } else {
            "fixture"
        }
        .into(),
        name: "growth_events".into(),
    };
    if v["engine"] == "postgres" {
        let config = PgSourceConfig::try_new(
            SourceId::new("proof_fixture").unwrap(),
            PgEndpoint::new("127.0.0.1", port, "fixture").unwrap(),
            PgCredentials::new("fixture_reader", password).unwrap(),
            SecretRef::new("secret://fixture-reader").unwrap(),
            PgTlsConfig::from_ca_pem(std::fs::read(directory.join("source-ca")).unwrap()).unwrap(),
            PgBudget::new(
                Duration::from_secs(10),
                Duration::from_secs(30),
                Duration::from_secs(1),
                100_000,
                32 * 1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(vec![
                SelectedTable::new(&table.schema, &table.name).unwrap(),
                SelectedTable::new(&table.schema, "proof_rows").unwrap(),
            ])
            .unwrap(),
        )
        .unwrap();
        verify(&mut PostgresEngine::new(config), table, directory).await;
    } else {
        assert_eq!(v["engine"], "mysql");
        let config = MysqlConfig {
            mysql_host: "127.0.0.1".into(),
            mysql_port: port,
            mysql_database: "fixture".into(),
            mysql_user: "fixture_reader".into(),
            mysql_ca_file: directory.join("source-ca"),
            mysql_tls_server_name: None,
            secret_ref: SecretRef::new("secret://fixture-reader").unwrap(),
            connect_timeout_ms: 10000,
        };
        verify(
            &mut MysqlEngine::new(
                config,
                zeroize::Zeroizing::new(password),
                EngineOptions::default(),
            ),
            table,
            directory,
        )
        .await;
    }
}
