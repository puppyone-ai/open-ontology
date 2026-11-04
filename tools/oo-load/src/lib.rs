//! Bounded canonical IPC collection for the engine-neutral pipeline.
use arrow_array::RecordBatch;
use arrow_ipc::writer::StreamWriter;
use oo_connectors::{
    SourceError,
    engine::{ChunkSink, ReadBudget},
};
use oo_pipeline::engine_load::{Capture, Result};
use std::io::Write;

struct Bounded {
    bytes: Vec<u8>,
    cap: usize,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.cap.saturating_sub(self.bytes.len()) {
            return Err(std::io::ErrorKind::OutOfMemory.into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct IpcSink {
    writer: Option<StreamWriter<Bounded>>,
    cap: usize,
    rows: u64,
    max_rows: u64,
}
impl IpcSink {
    pub fn new(budget: &ReadBudget) -> Self {
        Self {
            writer: None,
            cap: budget.max_chunk_bytes.min(32 * 1024 * 1024) as usize,
            rows: 0,
            max_rows: budget.max_chunk_rows.min(100_000),
        }
    }
}
impl ChunkSink for IpcSink {
    fn push(&mut self, batch: RecordBatch) -> std::result::Result<(), SourceError> {
        self.rows = self
            .rows
            .checked_add(batch.num_rows() as u64)
            .ok_or_else(|| SourceError::budget("sink_rows"))?;
        if self.rows > self.max_rows {
            return Err(SourceError::budget("sink_rows"));
        }
        if self.writer.is_none() {
            self.writer = Some(
                StreamWriter::try_new(
                    Bounded {
                        bytes: Vec::new(),
                        cap: self.cap,
                    },
                    &batch.schema(),
                )
                .map_err(|_| SourceError::budget("sink_bytes"))?,
            );
        }
        self.writer
            .as_mut()
            .unwrap()
            .write(&batch)
            .map_err(|_| SourceError::budget("sink_bytes"))
    }
}
impl Capture for IpcSink {
    fn finish(self) -> Result<Vec<u8>> {
        let mut writer = self.writer.ok_or("load_missing_empty_batch")?;
        writer.finish().map_err(|_| "load_sink_finish")?;
        Ok(writer.into_inner().map_err(|_| "load_sink_finish")?.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::engine::*;
    use oo_pipeline::engine_load::{Target, load};
    use oo_pipeline::native_arrow::DecodedStream;
    use sha2::{Digest, Sha256};

    struct Fake {
        revision: u32,
        fail_once: bool,
        corrupt: bool,
        empty: bool,
    }
    impl Fake {
        fn new() -> Self {
            Self {
                revision: 0,
                fail_once: false,
                corrupt: false,
                empty: false,
            }
        }
    }
    fn table() -> TableRef {
        TableRef {
            schema: "fixture".into(),
            name: "items".into(),
        }
    }
    impl SourceEngine for Fake {
        fn family(&self) -> EngineFamily {
            EngineFamily::Postgres
        }
        async fn probe(&mut self) -> std::result::Result<ProbeReport, SourceError> {
            Err(SourceError::protocol("unused"))
        }
        async fn plan_chunks(
            &mut self,
            table: &TableRef,
            _: &ReadBudget,
        ) -> std::result::Result<TableChunkPlan, SourceError> {
            self.revision += 1;
            let mut chunks = vec![ChunkSpec {
                table: table.clone(),
                plan_revision: self.revision,
                chunk_id: 0,
                lo: None,
                hi: None,
                estimated_rows: None,
            }];
            if self.revision > 1 {
                chunks[0].hi = Some(vec![KeyValue::Int(2)]);
                chunks.push(ChunkSpec {
                    chunk_id: 1,
                    lo: chunks[0].hi.clone(),
                    hi: None,
                    ..chunks[0].clone()
                });
            }
            Ok(TableChunkPlan {
                protocol: ENGINE_PROTOCOL.into(),
                table: table.clone(),
                key: KeyShape::IntegerPrimaryKey {
                    column: "id".into(),
                },
                strategy: ChunkStrategy::Keyset {
                    columns: vec!["id".into()],
                },
                estimated_rows: None,
                chunks,
            })
        }
        async fn read_chunk(
            &mut self,
            chunk: &ChunkSpec,
            budget: &ReadBudget,
            sink: &mut dyn ChunkSink,
        ) -> std::result::Result<ChunkReceipt, SourceError> {
            let bytes = include_bytes!(
                "../../../crates/oo-pipeline/tests/fixtures/c1_parts/fill-00000001.arrow"
            );
            let mut reader = arrow_ipc::reader::StreamReader::try_new(&bytes[..], None).unwrap();
            let batch = reader.next().unwrap().unwrap();
            let batch = if self.empty {
                batch.slice(0, 0)
            } else if self.revision > 1 {
                if chunk.chunk_id == 0 {
                    batch.slice(0, 2)
                } else {
                    batch.slice(2, 1)
                }
            } else {
                batch
            };
            let mut witness = IpcSink::new(budget);
            witness.push(batch.clone())?;
            sink.push(batch.clone())?;
            if self.fail_once {
                self.fail_once = false;
                return Err(SourceError::budget("growth"));
            }
            let ipc = witness.finish().unwrap();
            Ok(ChunkReceipt {
                table: chunk.table.clone(),
                plan_revision: chunk.plan_revision,
                chunk_id: chunk.chunk_id,
                rows: batch.num_rows() as u64,
                bytes: ipc.len() as u64,
                sha256: if self.corrupt {
                    "0".repeat(64)
                } else {
                    format!("{:x}", Sha256::digest(ipc))
                },
                low: Some(Watermark::PgLsn { lsn: 1 }),
                high: Some(Watermark::PgLsn { lsn: 2 }),
            })
        }
        async fn watermark(&mut self) -> std::result::Result<Option<Watermark>, SourceError> {
            Ok(None)
        }
        async fn digest_range(
            &mut self,
            _: &TableRef,
            _: Option<&[KeyValue]>,
            _: Option<&[KeyValue]>,
        ) -> std::result::Result<RangeDigest, SourceError> {
            Err(SourceError::protocol("unused"))
        }
    }
    #[derive(Default)]
    struct Memory {
        revisions: Vec<u32>,
        rows: usize,
        published: bool,
    }
    impl Target for Memory {
        async fn begin(&mut self, p: &TableChunkPlan) -> Result<()> {
            self.revisions.push(p.chunks[0].plan_revision);
            self.rows = 0;
            Ok(())
        }
        async fn stage(&mut self, _: &ChunkReceipt, d: &DecodedStream) -> Result<()> {
            self.rows += d.rows;
            Ok(())
        }
        async fn publish(&mut self, _: &TableChunkPlan) -> Result<()> {
            self.published = true;
            Ok(())
        }
    }

    #[tokio::test]
    async fn fake_engine_growth_replans_and_only_publishes_complete_revision() {
        let mut engine = Fake {
            fail_once: true,
            ..Fake::new()
        };
        let mut target = Memory::default();
        let receipts = load(
            &mut engine,
            &table(),
            &ReadBudget::default(),
            IpcSink::new,
            &mut target,
        )
        .await
        .unwrap();
        assert_eq!(target.revisions, vec![1, 2]);
        assert_eq!(target.rows, 3);
        assert!(target.published);
        assert_eq!(receipts.len(), 2);
        assert!(
            receipts
                .iter()
                .all(|r| r.plan_revision == 2 && r.low.is_some() && r.high.is_some())
        );
    }

    #[tokio::test]
    async fn corrupt_receipt_never_reaches_target_and_empty_schema_does() {
        let mut engine = Fake {
            corrupt: true,
            ..Fake::new()
        };
        let mut target = Memory::default();
        assert_eq!(
            load(
                &mut engine,
                &table(),
                &ReadBudget::default(),
                IpcSink::new,
                &mut target
            )
            .await
            .unwrap_err(),
            "engine_chunk_digest"
        );
        assert!(!target.published);
        assert_eq!(target.rows, 0);
        let mut engine = Fake {
            empty: true,
            ..Fake::new()
        };
        load(
            &mut engine,
            &table(),
            &ReadBudget::default(),
            IpcSink::new,
            &mut target,
        )
        .await
        .unwrap();
        assert!(target.published);
        assert_eq!(target.rows, 0);
        assert!(IpcSink::new(&ReadBudget::default()).finish().is_err());
    }

    #[tokio::test]
    #[ignore = "requires a dedicated synthetic Agent DB; OO_LOAD_TEST_TARGET is a private config path"]
    async fn fake_source_engine_to_real_target_replays_without_rewrite() {
        use sqlx::{
            Connection,
            postgres::{PgConnectOptions, PgConnection, PgSslMode},
        };
        let config: serde_json::Value = serde_json::from_slice(
            &std::fs::read(std::env::var("OO_LOAD_TEST_TARGET").expect("private config path"))
                .unwrap(),
        )
        .unwrap();
        let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
        let options = PgConnectOptions::new()
            .host("127.0.0.1")
            .port(config["port"].as_u64().unwrap() as u16)
            .username("postgres")
            .database("postgres")
            .password(password.trim())
            .ssl_mode(PgSslMode::Disable);
        let mut conn = PgConnection::connect_with(&options).await.unwrap();
        let mut target =
            oo_pipeline::engine_load_pg::PgTarget::new(&mut conn, "fake", "fake-1", &table())
                .await
                .unwrap();
        let mut engine = Fake {
            fail_once: true,
            ..Fake::new()
        };
        let receipts = load(
            &mut engine,
            &table(),
            &ReadBudget::default(),
            IpcSink::new,
            &mut target,
        )
        .await
        .unwrap();
        let relation = target.relation();
        let bytes = target.write_bytes().await.unwrap();
        assert!(bytes > 0);
        assert_eq!(target.completed().await.unwrap(), Some(receipts));
        assert_eq!(target.write_bytes().await.unwrap(), bytes);
        drop(target);
        let rows: i64 = sqlx::query_scalar(sqlx_core::sql_str::AssertSqlSafe(format!(
            "SELECT count(*) FROM {relation}"
        )))
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(rows, 3);
        conn.close().await.unwrap();
        let mut conn = PgConnection::connect_with(&options).await.unwrap();
        let mut replay =
            oo_pipeline::engine_load_pg::PgTarget::new(&mut conn, "fake", "fake-1", &table())
                .await
                .unwrap();
        assert_eq!(replay.completed().await.unwrap().unwrap().len(), 2);
        assert_eq!(replay.write_bytes().await.unwrap(), bytes);
        drop(replay);
        conn.close().await.unwrap();
        let mut conn = PgConnection::connect_with(&options).await.unwrap();
        let mut replacement =
            oo_pipeline::engine_load_pg::PgTarget::new(&mut conn, "fake", "fake-2", &table())
                .await
                .unwrap();
        let mut empty = Fake {
            empty: true,
            ..Fake::new()
        };
        load(
            &mut empty,
            &table(),
            &ReadBudget::default(),
            IpcSink::new,
            &mut replacement,
        )
        .await
        .unwrap();
        assert_eq!(replacement.completed().await.unwrap().unwrap()[0].rows, 0);
        drop(replacement);
        let rows: i64 = sqlx::query_scalar(sqlx_core::sql_str::AssertSqlSafe(format!(
            "SELECT count(*) FROM {relation}"
        )))
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(rows, 0);
        conn.close().await.unwrap();
        let mut conn = PgConnection::connect_with(&options).await.unwrap();
        let mut old =
            oo_pipeline::engine_load_pg::PgTarget::new(&mut conn, "fake", "fake-1", &table())
                .await
                .unwrap();
        assert_eq!(old.completed().await.unwrap_err(), "load_run_superseded");
    }

    struct PauseAfterStage<'a, T> {
        target: &'a mut T,
        pause: bool,
    }
    async fn checkpoint() {
        std::fs::write(std::env::var("OO_LOAD_TEST_MARKER").unwrap(), b"committed").unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        panic!("kill harness did not terminate the process");
    }
    impl<T: Target> Target for PauseAfterStage<'_, T> {
        async fn begin(&mut self, plan: &TableChunkPlan) -> Result<()> {
            self.target.begin(plan).await
        }
        async fn stage(&mut self, receipt: &ChunkReceipt, decoded: &DecodedStream) -> Result<()> {
            self.target.stage(receipt, decoded).await?;
            if self.pause {
                checkpoint().await;
            }
            Ok(())
        }
        async fn publish(&mut self, plan: &TableChunkPlan) -> Result<()> {
            self.target.publish(plan).await
        }
    }

    #[tokio::test]
    #[ignore = "subprocess fixture for test-target.py; actually killed twice by that harness"]
    async fn sigkill_target_fixture() {
        use sqlx::{
            Connection,
            postgres::{PgConnectOptions, PgConnection, PgSslMode},
        };
        let config: serde_json::Value = serde_json::from_slice(
            &std::fs::read(std::env::var("OO_LOAD_TEST_TARGET").unwrap()).unwrap(),
        )
        .unwrap();
        let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
        let options = PgConnectOptions::new()
            .host("127.0.0.1")
            .port(config["port"].as_u64().unwrap() as u16)
            .username("postgres")
            .database("postgres")
            .password(password.trim())
            .ssl_mode(PgSslMode::Disable);
        let phase = std::env::var("OO_LOAD_TEST_PHASE").unwrap();
        let mut conn = PgConnection::connect_with(&options).await.unwrap();
        let mut target =
            oo_pipeline::engine_load_pg::PgTarget::new(&mut conn, "kill-fake", "kill-1", &table())
                .await
                .unwrap();
        match target.completed().await.unwrap() {
            Some(receipts) => {
                assert_eq!(phase, "complete");
                assert_eq!(receipts.len(), 1);
                assert_eq!(receipts[0].rows, 3);
            }
            None => {
                load(
                    &mut Fake::new(),
                    &table(),
                    &ReadBudget::default(),
                    IpcSink::new,
                    &mut PauseAfterStage {
                        target: &mut target,
                        pause: phase == "stage",
                    },
                )
                .await
                .unwrap();
            }
        }
        if phase == "published" {
            checkpoint().await;
        }
        let relation = target.relation();
        drop(target);
        let rows: i64 = sqlx::query_scalar(sqlx_core::sql_str::AssertSqlSafe(format!(
            "SELECT count(*) FROM {relation}"
        )))
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(rows, 3);
    }
}
