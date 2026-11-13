//! Owned fixture only; the Python harness grows the table after planning.
use arrow_array::{Decimal128Array, Int64Array, RecordBatch, UInt64Array};
use oo_connectors::engine::*;
use oo_connectors::mysql::{EngineOptions, MysqlConfig, MysqlEngine};
use oo_connectors::postgres::PostgresEngine;
use oo_connectors::*;
use serde_json::Value;
use std::{collections::VecDeque, path::Path, time::Duration};

#[derive(Default)]
struct Sink(Vec<u64>);
impl ChunkSink for Sink {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        let column = batch.column(0);
        for i in 0..batch.num_rows() {
            self.0.push(
                if let Some(a) = column.as_any().downcast_ref::<Int64Array>() {
                    a.value(i).try_into().unwrap()
                } else if let Some(a) = column.as_any().downcast_ref::<UInt64Array>() {
                    a.value(i)
                } else {
                    column
                        .as_any()
                        .downcast_ref::<Decimal128Array>()
                        .unwrap()
                        .value(i)
                        .try_into()
                        .unwrap()
                },
            );
        }
        Ok(())
    }
}

async fn verify<E: SourceEngine>(engine: &mut E, table: TableRef, directory: &Path) {
    let budget = ReadBudget {
        max_chunk_rows: 10,
        ..Default::default()
    };
    let plan = engine.plan_chunks(&table, &budget).await.unwrap();
    oo_pipeline::engine_load::validate_plan(&plan, &table).unwrap();
    std::fs::write(directory.join("planned"), b"ready").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    while !directory.join("grown").exists() {
        assert!(std::time::Instant::now() < deadline, "admin growth timeout");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut pending: VecDeque<_> = plan.chunks.into();
    let mut ids = pending
        .iter()
        .map(|c| c.chunk_id)
        .collect::<std::collections::BTreeSet<_>>();
    let mut rows = Vec::new();
    let mut splits = 0;
    while let Some(chunk) = pending.pop_front() {
        let mut sink = Sink::default();
        match engine.read_chunk(&chunk, &budget, &mut sink).await {
            Ok(receipt) => {
                assert_eq!(receipt.rows, sink.0.len() as u64);
                assert!(receipt.rows <= budget.max_chunk_rows);
                rows.extend(sink.0);
            }
            Err(error) => {
                assert_eq!(error.kind(), SourceErrorKind::Budget);
                let children = engine.split_chunk(&chunk, &budget).await.unwrap().unwrap();
                assert!(children.len() >= 2);
                assert_eq!(children.first().unwrap().lo, chunk.lo);
                assert_eq!(children.last().unwrap().hi, chunk.hi);
                assert!(
                    children
                        .windows(2)
                        .all(|c| c[0].hi.is_some() && c[0].hi == c[1].lo)
                );
                for child in &children {
                    assert!(child.plan_revision > chunk.plan_revision);
                    assert_eq!(child.table, table);
                    assert!(ids.insert(child.chunk_id));
                }
                for child in children.into_iter().rev() {
                    pending.push_front(child);
                }
                splits += 1;
                assert!(splits <= 20, "split did not converge");
            }
        }
    }
    rows.sort_unstable();
    let expected: Vec<u64> = (1..=8).map(|n| n * 10).chain(81..=100).collect();
    assert_eq!(rows, expected, "growth lost or duplicated keys");
    assert!(splits > 0, "fixture must exercise the trait split hook");
    std::fs::write(
        directory.join("split-result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"rows": rows.len(), "splits": splits}))
            .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "requires the owned tools/oo-test/split_probe.py fixture"]
async fn real_growing_table_uses_trait_split() {
    let path = std::env::var("OO_LOAD_SPLIT_CONFIG").expect("private fixture config path");
    let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let directory = Path::new(&path).parent().unwrap();
    let password = std::fs::read_to_string(directory.join("source-password")).unwrap();
    let port = v["port"].as_u64().unwrap() as u16;
    let table = TableRef {
        schema: if v["engine"] == "postgres" {
            "public"
        } else if v["engine"] == "oracle" {
            "FIXTURE"
        } else {
            "fixture"
        }
        .into(),
        name: if v["engine"] == "oracle" {
            "GROWTH_EVENTS"
        } else {
            "growth_events"
        }
        .into(),
    };
    if v["engine"] == "postgres" {
        let config = PgSourceConfig::try_new(
            SourceId::new("split_fixture").unwrap(),
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
            ])
            .unwrap(),
        )
        .unwrap();
        verify(&mut PostgresEngine::new(config), table, directory).await;
    } else if v["engine"] == "oracle" {
        let mut config = oo_connectors::oracle::engine::OracleConnectConfig::new(
            "127.0.0.1",
            port,
            "FREEPDB1",
            "fixture_reader",
            zeroize::Zeroizing::new(password),
        );
        config.scope = vec![table.clone()];
        let mut engine = oo_connectors::oracle::engine::OracleEngine::connect(config).unwrap();
        verify(&mut engine, table, directory).await;
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
