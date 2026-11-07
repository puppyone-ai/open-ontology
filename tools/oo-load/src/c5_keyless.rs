use super::*;
use oo_connectors::engine::ChunkReceipt;
use oo_pipeline::engine_load_pg::{C5Baseline, TableContentDigest};

const PAYLOAD: &str = "#payload-sha256-sum256/v1:";

fn source_digest(digest: &RangeDigest) -> RangeDigest {
    RangeDigest {
        digest: digest.digest.split(PAYLOAD).next().unwrap().into(),
        ..digest.clone()
    }
}

async fn capture<E: SourceEngine, T: Target>(
    engine: &mut E,
    plan: &TableChunkPlan,
    budget: &ReadBudget,
    target: &mut T,
    stage: bool,
) -> Result<(Vec<ChunkReceipt>, u64, String, u64), String> {
    let mut receipts = Vec::new();
    let mut bytes = 0;
    let mut rows = 0u64;
    let mut content = TableContentDigest::default();
    for chunk in &plan.chunks {
        let mut sink = oo_load::IpcSink::new(budget);
        let receipt = engine
            .read_chunk(chunk, budget, &mut sink)
            .await
            .map_err(|e| e.to_string())?;
        let ipc = sink.finish()?;
        bytes += ipc.len() as u64;
        let decoded = oo_pipeline::engine_chunk::decode_chunk(chunk, &receipt, &ipc, budget)?;
        rows = rows.checked_add(receipt.rows).ok_or("c5_row_overflow")?;
        content.add(&decoded)?;
        if stage {
            if receipts.is_empty() {
                target.begin(plan).await?;
            }
            target.stage(&receipt, &decoded).await?;
            receipts.push(receipt);
        }
    }
    Ok((receipts, bytes, content.finish(), rows))
}

pub(super) async fn read<E: SourceEngine, T: Target>(
    engine: &mut E,
    plan: &TableChunkPlan,
    previous: &C5Baseline,
    budget: &ReadBudget,
    target: &mut T,
    policy: &Config,
) -> Result<(RangeDigest, Vec<ChunkReceipt>, u64), String> {
    // Hints name durable units; a keyless table has exactly one, with id zero.
    if policy
        .positive_hints
        .union(&policy.recent_chunks)
        .any(|id| *id != 0)
    {
        return Err("c5_unknown_hint_chunk".into());
    }
    let digest = engine
        .digest_range(&plan.table, None, None)
        .await
        .map_err(|e| e.to_string())?;
    let old = previous
        .source_digests
        .first()
        .filter(|_| previous.source_digests.len() == 1);
    let changed =
        old.is_none_or(|old| source_digest(old) != digest || !old.digest.contains(PAYLOAD));
    if !changed
        && !policy.cycle.is_multiple_of(policy.full_read_every)
        && !policy.positive_hints.contains(&0)
    {
        return Ok((old.unwrap().clone(), vec![], 0));
    }
    let (mut receipts, mut bytes, content, rows) =
        capture(engine, plan, budget, target, changed).await?;
    let complete = RangeDigest {
        digest: format!("{}{PAYLOAD}{content}", digest.digest),
        ..digest.clone()
    };
    // Periodic full reads still catch a source-digest collision. Only the rare
    // collision needs a second bounded pass; unchanged reads never stage data.
    if !changed && old != Some(&complete) {
        let (replacement, read_bytes, again, count) =
            capture(engine, plan, budget, target, true).await?;
        if again != content || count != rows {
            return Err("c5_range_changed_retry".into());
        }
        receipts = replacement;
        bytes += read_bytes;
    }
    let after = engine
        .digest_range(&plan.table, None, None)
        .await
        .map_err(|e| e.to_string())?;
    if after != digest || rows != digest.rows {
        return Err("c5_range_changed_retry".into());
    }
    Ok((complete, receipts, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{ArrayRef, Int64Array, RecordBatch};
    use oo_connectors::{SourceError, engine::*};
    use oo_pipeline::native_arrow::DecodedStream;
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    struct Source {
        family: EngineFamily,
        values: Vec<i64>,
        reads: usize,
    }
    impl SourceEngine for Source {
        fn family(&self) -> EngineFamily {
            self.family
        }
        async fn probe(&mut self) -> Result<ProbeReport, SourceError> {
            unreachable!()
        }
        async fn plan_chunks(
            &mut self,
            _: &TableRef,
            _: &ReadBudget,
        ) -> Result<TableChunkPlan, SourceError> {
            unreachable!()
        }
        async fn watermark(&mut self) -> Result<Option<Watermark>, SourceError> {
            Ok(None)
        }
        async fn digest_range(
            &mut self,
            table: &TableRef,
            lo: Option<&[KeyValue]>,
            hi: Option<&[KeyValue]>,
        ) -> Result<RangeDigest, SourceError> {
            assert!(lo.is_none() && hi.is_none());
            // Deliberately collide on same-count edits to exercise full reads.
            Ok(RangeDigest {
                table: table.clone(),
                lo: None,
                hi: None,
                rows: self.values.len() as u64,
                digest: "colliding-source-digest".into(),
            })
        }
        async fn read_chunk(
            &mut self,
            chunk: &ChunkSpec,
            budget: &ReadBudget,
            sink: &mut dyn ChunkSink,
        ) -> Result<ChunkReceipt, SourceError> {
            self.reads += 1;
            let batch = RecordBatch::try_from_iter([(
                "value",
                Arc::new(Int64Array::from(self.values.clone())) as ArrayRef,
            )])
            .unwrap();
            let mut copy = oo_load::IpcSink::new(budget);
            copy.push(batch.clone())?;
            let ipc = copy.finish().unwrap();
            sink.push(batch)?;
            Ok(ChunkReceipt {
                table: chunk.table.clone(),
                plan_revision: chunk.plan_revision,
                chunk_id: chunk.chunk_id,
                rows: self.values.len() as u64,
                bytes: ipc.len() as u64,
                sha256: format!("{:x}", Sha256::digest(ipc)),
                low: None,
                high: None,
            })
        }
    }
    #[derive(Default)]
    struct Sink {
        writes: usize,
    }
    impl Target for Sink {
        async fn begin(&mut self, _: &TableChunkPlan) -> oo_pipeline::engine_load::Result<()> {
            Ok(())
        }
        async fn stage(
            &mut self,
            _: &ChunkReceipt,
            _: &DecodedStream,
        ) -> oo_pipeline::engine_load::Result<()> {
            self.writes += 1;
            Ok(())
        }
        async fn publish(&mut self, _: &TableChunkPlan) -> oo_pipeline::engine_load::Result<()> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn every_keyless_family_is_idle_and_full_reads_detect_collisions() {
        for family in [
            EngineFamily::Postgres,
            EngineFamily::Mysql,
            EngineFamily::Oracle,
            EngineFamily::Sqlserver,
            EngineFamily::Mariadb,
            EngineFamily::Tidb,
            EngineFamily::Oceanbase,
            EngineFamily::Db2,
            EngineFamily::Kingbase,
        ] {
            let table = TableRef {
                schema: "fixture".into(),
                name: "no_key".into(),
            };
            let plan = TableChunkPlan {
                protocol: ENGINE_PROTOCOL.into(),
                table: table.clone(),
                key: KeyShape::NoKey,
                strategy: ChunkStrategy::SingleRead,
                estimated_rows: None,
                chunks: vec![ChunkSpec {
                    table,
                    plan_revision: 9,
                    chunk_id: 0,
                    lo: None,
                    hi: None,
                    estimated_rows: None,
                }],
            };
            let mut previous = C5Baseline {
                run: "base".into(),
                plan: plan.clone(),
                parts: vec![],
                source_digests: vec![],
                lineage: None,
            };
            let mut source = Source {
                family,
                values: vec![1, 2, 2],
                reads: 0,
            };
            let mut sink = Sink::default();
            let budget = ReadBudget::default();
            let mut policy = Config {
                cycle: 1,
                full_read_every: 2,
                positive_hints: BTreeSet::new(),
                recent_chunks: BTreeSet::new(),
            };
            let (digest, receipts, _) =
                read(&mut source, &plan, &previous, &budget, &mut sink, &policy)
                    .await
                    .unwrap();
            assert_eq!(receipts.len(), 1);
            assert_eq!(sink.writes, 1);
            previous.source_digests = vec![digest];
            let (_, receipts, bytes) =
                read(&mut source, &plan, &previous, &budget, &mut sink, &policy)
                    .await
                    .unwrap();
            assert!(receipts.is_empty());
            assert_eq!(bytes, 0);
            assert_eq!(source.reads, 1);
            policy.cycle = 2;
            source.values.reverse();
            let (_, receipts, _) = read(&mut source, &plan, &previous, &budget, &mut sink, &policy)
                .await
                .unwrap();
            assert!(receipts.is_empty());
            assert_eq!(sink.writes, 1);
            source.values = vec![1, 1, 2];
            let (new, receipts, _) =
                read(&mut source, &plan, &previous, &budget, &mut sink, &policy)
                    .await
                    .unwrap();
            assert_ne!(previous.source_digests[0], new);
            assert_eq!(receipts.len(), 1);
            assert_eq!(sink.writes, 2);
            assert_eq!(source.reads, 4);
            previous.source_digests = vec![new];
            source.values.clear();
            policy.cycle = 3;
            let (empty, receipts, _) =
                read(&mut source, &plan, &previous, &budget, &mut sink, &policy)
                    .await
                    .unwrap();
            assert_eq!(empty.rows, 0);
            assert_eq!(receipts.len(), 1);
            assert_eq!(receipts[0].rows, 0);
        }
    }
}
