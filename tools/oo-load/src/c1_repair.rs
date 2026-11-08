//! Bounded full-table snapshot plus commit-ordered log correction.
use super::*;
use arrow_array::RecordBatch;
use oo_connectors::engine::{
    ChangeSource, ChunkReceipt, ChunkSink, ChunkSpec, ChunkStrategy, KeyValue, RowChange, Watermark,
};
use oo_pipeline::{
    engine_load::Capture,
    engine_sync::at_or_after,
    engine_sync_pg::{AlignedRebaseline, PgSync},
    native_arrow::{NativeCell, decode_arrow_stream},
};
use sha2::{Digest, Sha256};

struct Rows {
    batches: Vec<RecordBatch>,
    bytes: u64,
    rows: u64,
    budget: ReadBudget,
}
impl ChunkSink for Rows {
    fn push(&mut self, batch: RecordBatch) -> Result<(), SourceError> {
        self.bytes = self
            .bytes
            .saturating_add(batch.get_array_memory_size() as u64);
        self.rows = self.rows.saturating_add(batch.num_rows() as u64);
        if self.bytes > self.budget.max_chunk_bytes || self.rows > self.budget.max_chunk_rows {
            return Err(SourceError::budget("sink_bytes"));
        }
        self.batches.push(batch);
        Ok(())
    }
}

fn decimal(coefficient: i128, scale: i8) -> KeyValue {
    let negative = coefficient < 0;
    let mut text = coefficient.unsigned_abs().to_string();
    if scale < 0 {
        text.extend(std::iter::repeat_n('0', usize::from(scale.unsigned_abs())));
    } else if scale > 0 {
        let scale = scale as usize;
        if text.len() <= scale {
            text = format!("{}{}", "0".repeat(scale + 1 - text.len()), text);
        }
        text.insert(text.len() - scale, '.');
        text = text.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    if negative && text != "0" {
        text.insert(0, '-');
    }
    text.parse()
        .map(KeyValue::Int)
        .unwrap_or(KeyValue::Decimal(text))
}

fn key(cell: &NativeCell) -> Result<KeyValue, &'static str> {
    Ok(match cell {
        NativeCell::Int { value, .. } => KeyValue::Int(i128::from(*value)),
        NativeCell::UInt64(value) => KeyValue::Int(i128::from(*value)),
        NativeCell::UInt8(value) => KeyValue::Int(i128::from(*value)),
        NativeCell::Decimal {
            coefficient, scale, ..
        } => decimal(*coefficient, *scale),
        NativeCell::Utf8(value) => KeyValue::Text(value.clone()),
        NativeCell::Binary(value) => {
            KeyValue::Bytes(value.iter().map(|b| format!("{b:02x}")).collect())
        }
        _ => return Err("sync_repair_key_type"),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run<E: epoch::Epoch, C: ChangeSource>(
    engine: &mut E,
    changes: &mut C,
    conn: &mut PgConnection,
    source: &Source,
    store: &SecretStore,
    tables: &[TableRef],
    table: &TableRef,
    config: &TargetConfig,
    budget: &ReadBudget,
    lineage: &str,
    initial: &Watermark,
) -> Result<serde_json::Value, String> {
    let first = run_once(
        engine, changes, conn, source, store, tables, table, config, budget, lineage, initial,
    )
    .await;
    match first {
        Err(ref error) if oo_pipeline::engine_isolation::stale_reason(error).is_some() => {
            run_once(
                engine, changes, conn, source, store, tables, table, config, budget, lineage,
                initial,
            )
            .await
        }
        result => result,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_once<E: epoch::Epoch, C: ChangeSource>(
    engine: &mut E,
    changes: &mut C,
    conn: &mut PgConnection,
    source: &Source,
    store: &SecretStore,
    tables: &[TableRef],
    table: &TableRef,
    config: &TargetConfig,
    budget: &ReadBudget,
    lineage: &str,
    initial: &Watermark,
) -> Result<serde_json::Value, String> {
    let binding = binding(source)?;
    let before_epoch = engine.epoch(source, store, tables).await?;
    let columns = engine
        .column_shape(table, budget)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("schema_unproven")?;
    let mut target = PgTarget::new(conn, &binding, &config.run_id, table).await?;
    let projection = target
        .sealed_columns()
        .await?
        .ok_or("schema_unproven")?
        .projection;
    drop(target);
    let mut plan = engine
        .plan_chunks(table, budget)
        .await
        .map_err(|e| e.to_string())?;
    let ChunkStrategy::Keyset { columns: keys } = &plan.strategy else {
        return Err("sync_repair_requires_key".into());
    };
    let low = engine
        .watermark()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("sync_repair_position")?;
    let earliest = changes
        .earliest_available()
        .await
        .map_err(|e| e.to_string())?;
    if !at_or_after(&low, &earliest)? {
        return Err("sync_rebaseline_required_retention".into());
    }
    let mut rows = BTreeMap::<Vec<KeyValue>, RecordBatch>::new();
    let mut empty = None;
    let mut charged = 0u64;
    for chunk in &plan.chunks {
        let mut sink = Rows {
            batches: Vec::new(),
            bytes: 0,
            rows: 0,
            budget: budget.clone(),
        };
        engine
            .read_chunk(chunk, budget, &mut sink)
            .await
            .map_err(|e| e.to_string())?;
        charged = charged.saturating_add(sink.bytes);
        for batch in sink.batches {
            empty.get_or_insert_with(|| RecordBatch::new_empty(batch.schema()));
            let mut ipc = oo_load::IpcSink::new(budget);
            ipc.push(batch.clone()).map_err(|e| e.to_string())?;
            let decoded = decode_arrow_stream(&ipc.finish()?).map_err(|_| "sync_repair_decode")?;
            let indexes = keys
                .iter()
                .map(|name| {
                    decoded
                        .fields
                        .iter()
                        .position(|f| &f.name == name)
                        .ok_or("sync_repair_key")
                })
                .collect::<Result<Vec<_>, _>>()?;
            for i in 0..decoded.rows {
                let key = indexes
                    .iter()
                    .map(|j| key(&decoded.columns[*j][i]))
                    .collect::<Result<Vec<_>, _>>()?;
                // A key moving between chunks can be seen twice. The complete
                // log interval below replaces every changed key, including it.
                rows.insert(key, batch.slice(i, 1));
            }
        }
        if charged > budget.max_chunk_bytes || rows.len() as u64 > budget.max_chunk_rows {
            return Err("source error: budget fact=sink_bytes".into());
        }
    }
    let high = engine
        .watermark()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("sync_repair_position")?;
    let mut cursor = low.clone();
    let mut end = high.clone();
    let mut drained = false;
    let mut log_events = 0u64;
    let snapshot_rows = rows.len();
    for _ in 0..10_000 {
        if at_or_after(&cursor, &high)? {
            end = cursor.clone();
            drained = true;
            break;
        }
        let Some(batch) = changes
            .next_batch(&cursor, budget)
            .await
            .map_err(|e| e.to_string())?
        else {
            if matches!(high, Watermark::OracleScn { .. }) {
                return Err("sync_repair_oracle_window".into());
            }
            drained = true;
            break;
        };
        if !at_or_after(&batch.position, &cursor)?
            || batch.position == cursor
            || batch.changes.len() != batch.images.len()
        {
            return Err("sync_repair_batch".into());
        }
        // PG/MySQL deliver one transaction. A transaction beyond the snapshot
        // upper bound stays in the source for normal C1 replay.
        if !matches!(high, Watermark::OracleScn { .. }) && !at_or_after(&high, &batch.position)? {
            drained = true;
            break;
        }
        for (change, image) in batch.changes.iter().zip(&batch.images) {
            log_events += 1;
            let (row_table, key, old, delete) = match change {
                RowChange::Insert { table, key } => (table, key, None, false),
                RowChange::Update {
                    table,
                    key,
                    old_key,
                } => (table, key, old_key.as_ref(), false),
                RowChange::Delete { table, key } => (table, key, None, true),
            };
            if row_table != table || image.num_rows() != 1 {
                return Err("sync_repair_scope".into());
            }
            if let Some(old) = old {
                rows.remove(old);
            }
            rows.remove(key);
            charged = charged.saturating_add(image.get_array_memory_size() as u64);
            if !delete {
                rows.insert(key.clone(), image.clone());
            }
            if charged > budget.max_chunk_bytes || rows.len() as u64 > budget.max_chunk_rows {
                return Err("source error: budget fact=sink_bytes".into());
            }
        }
        cursor = batch.position;
    }
    if !drained {
        return Err("sync_repair_drain_budget".into());
    }
    if engine.epoch(source, store, tables).await? != before_epoch
        || engine
            .column_shape(table, budget)
            .await
            .map_err(|e| e.to_string())?
            .as_ref()
            != Some(&columns)
        || !at_or_after(
            &low,
            &changes
                .earliest_available()
                .await
                .map_err(|e| e.to_string())?,
        )?
    {
        return Err("sync_repair_source_changed".into());
    }
    let mut capture = oo_load::IpcSink::new(budget);
    capture
        .push(empty.ok_or("sync_repair_empty_schema")?)
        .map_err(|e| e.to_string())?;
    for row in rows.values() {
        capture.push(row.clone()).map_err(|e| e.to_string())?;
    }
    let ipc = capture.finish()?;
    let chunk = ChunkSpec {
        table: table.clone(),
        plan_revision: 1,
        chunk_id: 0,
        lo: None,
        hi: None,
        estimated_rows: Some(rows.len() as u64),
    };
    plan.chunks = vec![chunk.clone()];
    let receipt = ChunkReceipt {
        table: table.clone(),
        plan_revision: 1,
        chunk_id: 0,
        rows: rows.len() as u64,
        bytes: ipc.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&ipc)),
        low: Some(low.clone()),
        high: Some(end.clone()),
    };
    let decoded = oo_pipeline::engine_chunk::decode_chunk(&chunk, &receipt, &ipc, budget)?;
    let mut sink = PgSync::open(conn, &binding, lineage, tables, initial).await?;
    sink.rejoin(AlignedRebaseline {
        run: &config.run_id,
        plan: &plan,
        receipt: &receipt,
        rows: &decoded,
        columns: &columns,
        projection: &projection,
    })
    .await?;
    Ok(serde_json::json!({
        "method":"snapshot_plus_commit_log", "low":low, "high":end,
        "log_events":log_events, "snapshot_rows":snapshot_rows,
        "replacement_rows":rows.len(), "atomic_rejoin":true
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_and_log_numeric_keys_share_exact_identity() {
        for (coefficient, scale, expected) in [
            (100, 2, KeyValue::Int(1)),
            (-150, 2, KeyValue::Decimal("-1.5".into())),
            (1, 3, KeyValue::Decimal("0.001".into())),
            (12, -2, KeyValue::Int(1200)),
            (0, 4, KeyValue::Int(0)),
        ] {
            assert_eq!(decimal(coefficient, scale), expected);
        }
    }
}
