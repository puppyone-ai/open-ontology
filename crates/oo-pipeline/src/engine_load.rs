//! Engine-neutral snapshot admission. No legacy MySQL topology is fabricated.
//! The IPC sink belongs to the caller, so this crate needs no Arrow dependency.
use crate::{engine_chunk::decode_chunk, native_arrow::DecodedStream};
use oo_connectors::engine::{
    ChunkReceipt, ChunkSink, ENGINE_PROTOCOL, ReadBudget, SourceEngine, TableChunkPlan, TableRef,
};

pub type Result<T> = std::result::Result<T, &'static str>;

/// A bounded, canonical Arrow IPC sink. `finish` must close the stream.
pub trait Capture: ChunkSink {
    fn finish(self) -> Result<Vec<u8>>;
}

/// Transactional target boundary. A failed/replaced plan must never publish.
pub trait Target {
    fn begin(&mut self, plan: &TableChunkPlan) -> impl std::future::Future<Output = Result<()>>;
    fn stage(
        &mut self,
        receipt: &ChunkReceipt,
        decoded: &DecodedStream,
    ) -> impl std::future::Future<Output = Result<()>>;
    fn publish(&mut self, plan: &TableChunkPlan) -> impl std::future::Future<Output = Result<()>>;
}

/// Bounds are opaque source values: check adjacency, never compare collation.
pub fn validate_plan(plan: &TableChunkPlan, table: &TableRef) -> Result<()> {
    let first = plan.chunks.first().ok_or("load_empty_plan")?;
    if plan.protocol != ENGINE_PROTOCOL
        || &plan.table != table
        || first.lo.is_some()
        || plan.chunks.last().is_some_and(|c| c.hi.is_some())
    {
        return Err("load_plan");
    }
    let mut ids = std::collections::BTreeSet::new();
    for chunk in &plan.chunks {
        if &chunk.table != table
            || !ids.insert(chunk.chunk_id)
            || (chunk.lo.is_some() && chunk.lo == chunk.hi)
        {
            return Err("load_plan");
        }
    }
    if plan
        .chunks
        .windows(2)
        .any(|c| c[0].hi.is_none() || c[0].hi != c[1].lo)
    {
        return Err("load_plan_gap");
    }
    Ok(())
}

/// One table at a time, one bounded chunk resident. Growth splits only the
/// failed source range when supported. Replaced staging remains private.
/// This is snapshot admission, not CDC or a cross-table snapshot guarantee.
pub async fn load<E: SourceEngine, S: Capture, T: Target>(
    engine: &mut E,
    table: &TableRef,
    budget: &ReadBudget,
    make_sink: impl Fn(&ReadBudget) -> S,
    target: &mut T,
) -> Result<Vec<ChunkReceipt>> {
    let mut budget = budget.clone();
    if budget.max_chunk_rows == 0 || budget.max_chunk_bytes == 0 {
        return Err("load_budget");
    }
    // No successful partial read may be mistaken for a complete table.
    let mut plan = engine
        .plan_chunks(table, &budget)
        .await
        .map_err(|error| error.fact().unwrap_or("load_plan_source"))?;
    for attempt in 0..16 {
        validate_plan(&plan, table)?;
        target.begin(&plan).await?;
        let mut receipts = Vec::new();
        let mut replan = false;
        for (index, chunk) in plan.chunks.iter().enumerate() {
            let mut sink = make_sink(&budget);
            let receipt = match engine.read_chunk(chunk, &budget, &mut sink).await {
                Ok(receipt) => receipt,
                Err(error)
                    if error.kind() == oo_connectors::SourceErrorKind::Budget && attempt < 15 =>
                {
                    match engine
                        .split_chunk(chunk, &budget)
                        .await
                        .map_err(|e| e.fact().unwrap_or("load_split_source"))?
                    {
                        Some(children) => {
                            if children.len() < 2
                                || children.first().unwrap().lo != chunk.lo
                                || children.last().unwrap().hi != chunk.hi
                                || children
                                    .iter()
                                    .any(|c| c.plan_revision <= chunk.plan_revision)
                            {
                                return Err("load_split_coverage");
                            }
                            plan.chunks.splice(index..=index, children);
                            validate_plan(&plan, table)?;
                        }
                        None => {
                            if budget.max_chunk_rows == 1 {
                                return Err(error.fact().unwrap_or("load_read_source"));
                            }
                            budget.max_chunk_rows = (budget.max_chunk_rows / 2).max(1);
                            plan = engine
                                .plan_chunks(table, &budget)
                                .await
                                .map_err(|e| e.fact().unwrap_or("load_plan_source"))?;
                        }
                    }
                    replan = true;
                    break;
                }
                Err(error) => return Err(error.fact().unwrap_or("load_read_source")),
            };
            let ipc = sink.finish()?;
            let decoded = decode_chunk(chunk, &receipt, &ipc, &budget)?;
            target.stage(&receipt, &decoded).await?;
            receipts.push(receipt);
        }
        if replan {
            continue;
        }
        target.publish(&plan).await?;
        return Ok(receipts);
    }
    Err("load_replan_exhausted")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::engine::{ChunkSpec, ChunkStrategy, KeyShape, KeyValue};

    #[test]
    fn coverage_is_checked_without_client_collation_ordering() {
        let table = TableRef {
            schema: "s".into(),
            name: "t".into(),
        };
        let chunk = ChunkSpec {
            table: table.clone(),
            plan_revision: 3,
            chunk_id: 0,
            lo: None,
            hi: Some(vec![KeyValue::Text("Z".into())]),
            estimated_rows: None,
        };
        let mut plan = TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key: KeyShape::StringPrimaryKey {
                column: "id".into(),
                collation: None,
            },
            strategy: ChunkStrategy::Keyset {
                columns: vec!["id".into()],
            },
            estimated_rows: None,
            chunks: vec![
                chunk.clone(),
                ChunkSpec {
                    chunk_id: 1,
                    lo: chunk.hi.clone(),
                    hi: None,
                    ..chunk
                },
            ],
        };
        validate_plan(&plan, &table).unwrap();
        plan.chunks[1].table.name = "other".into();
        assert!(validate_plan(&plan, &table).is_err());
        plan.chunks[1].table = table.clone();
        plan.chunks[1].lo = None;
        assert!(validate_plan(&plan, &table).is_err());
        plan.chunks[1].lo = plan.chunks[0].hi.clone();
        plan.chunks[1].chunk_id = 0;
        assert!(validate_plan(&plan, &table).is_err());
    }
}
