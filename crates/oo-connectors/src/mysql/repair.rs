//! Bounded source-side snapshot/log merge and an all-binding C5 -> C1 barrier.
//! These are proposals, not Agent DB receipts. A caller must durably stage the
//! repair and tombstones before confirming each binding at its exact L_end.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering},
};

use super::*;
use crate::engine::{ChangeBatch, ChangeSource, RowChange};

/// Complete chunk plan of one destination binding. Several bindings may read
/// the same source table, but the table is repaired only once.
#[derive(Clone)]
pub struct RepairBinding {
    pub binding_id: String,
    pub plan: TableChunkPlan,
}

/// Total in-memory repair limit, separate from the per-read/transaction budget.
/// Exhaustion fails the proposal without offering any checkpoint to commit.
#[derive(Clone, Copy)]
pub struct RepairBudget {
    pub max_keys: u64,
    pub max_bytes: u64,
    /// Longest cycle interval plus recovery allowance, not an expiry setting.
    pub required_history_seconds: u64,
}

pub type RepairKey = (TableRef, Vec<KeyValue>);

/// The value None is a retained tombstone, not a missing/unknown row.
#[derive(Default)]
struct Overlay {
    rows: BTreeMap<RepairKey, Option<RecordBatch>>,
    log_keys: BTreeSet<RepairKey>,
    charged_bytes: u64,
}

impl Overlay {
    fn charge(
        &mut self,
        key: &RepairKey,
        image: Option<&RecordBatch>,
        budget: RepairBudget,
    ) -> Result<()> {
        // Conservative cumulative accounting also bounds update churn and
        // shared Arrow slice buffers, not just the number of surviving keys.
        let key_bytes = serde_json::to_vec(key)
            .map_err(|_| SourceError::protocol("repair_key"))?
            .len() as u64;
        self.charged_bytes = self
            .charged_bytes
            .saturating_add(key_bytes)
            .saturating_add(image.map_or(0, |b| b.get_array_memory_size() as u64));
        let count = self.rows.len() as u64 + u64::from(!self.rows.contains_key(key));
        if count > budget.max_keys || self.charged_bytes > budget.max_bytes {
            return Err(SourceError::budget("mysql_repair_staging_budget"));
        }
        Ok(())
    }

    fn log(&mut self, batch: &ChangeBatch, budget: RepairBudget) -> Result<()> {
        if batch.changes.len() != batch.images.len() {
            return Err(SourceError::protocol("repair_image_alignment"));
        }
        for (change, image) in batch.changes.iter().zip(&batch.images) {
            if image.num_rows() != 1 {
                return Err(SourceError::protocol("repair_image_alignment"));
            }
            let (table, key, old, delete) = match change {
                RowChange::Insert { table, key } => (table, key, None, false),
                RowChange::Update {
                    table,
                    key,
                    old_key,
                } => (table, key, old_key.as_ref(), false),
                RowChange::Delete { table, key } => (table, key, None, true),
            };
            if let Some(old) = old {
                let old = (table.clone(), old.clone());
                self.charge(&old, None, budget)?;
                self.log_keys.insert(old.clone());
                self.rows.insert(old, None);
            }
            let key = (table.clone(), key.clone());
            self.charge(&key, (!delete).then_some(image), budget)?;
            self.log_keys.insert(key.clone());
            self.rows.insert(key, (!delete).then(|| image.clone()));
        }
        Ok(())
    }

    fn snapshot(&mut self, key: RepairKey, image: RecordBatch, budget: RepairBudget) -> Result<()> {
        // Never compare source keys with Rust ordering for range membership:
        // the source selected the chunk; this is only exact identity lookup.
        if !self.log_keys.contains(&key) {
            self.charge(&key, Some(&image), budget)?;
            if self.rows.insert(key, Some(image)).is_some() {
                return Err(SourceError::protocol("repair_overlapping_snapshot_keys"));
            }
        }
        Ok(())
    }
}

/// A completed connector-side repair, still awaiting durable target application.
pub struct SourceRepair {
    source_id: String,
    bindings: BTreeMap<String, TableRef>,
    l0: Watermark,
    end: Watermark,
    rows: BTreeMap<RepairKey, Option<RecordBatch>>,
    empty_batches: BTreeMap<TableRef, RecordBatch>,
    receipts: Vec<ChunkReceipt>,
    during_read: u64,
}
impl SourceRepair {
    pub fn source_id(&self) -> &str {
        &self.source_id
    }
    pub fn bindings(&self) -> &BTreeMap<String, TableRef> {
        &self.bindings
    }
    pub fn l0(&self) -> &Watermark {
        &self.l0
    }
    pub fn end(&self) -> &Watermark {
        &self.end
    }
    pub fn rows(&self) -> &BTreeMap<RepairKey, Option<RecordBatch>> {
        &self.rows
    }
    /// One schema-bearing empty batch per table, including fully deleted tables.
    pub fn empty_batches(&self) -> &BTreeMap<TableRef, RecordBatch> {
        &self.empty_batches
    }
    pub fn receipts(&self) -> &[ChunkReceipt] {
        &self.receipts
    }
    pub fn changes_during_reads(&self) -> u64 {
        self.during_read
    }
}

fn covers(end: &Watermark, position: &Watermark) -> Result<bool> {
    match (end, position) {
        (
            Watermark::MysqlGtidSet { gtid_set: end },
            Watermark::MysqlGtidSet { gtid_set: position },
        ) => Ok(GtidSet::parse(end)?.contains(&GtidSet::parse(position)?)),
        (
            Watermark::MysqlFilePos {
                file: end,
                position: ep,
            },
            Watermark::MysqlFilePos { file, position: p },
        ) => {
            let split = |name: &str| -> Result<(String, u64)> {
                let (prefix, n) = name.rsplit_once('.').ok_or_else(|| {
                    SourceError::protocol("mysql_file_lineage_new_generation_rebaseline")
                })?;
                Ok((
                    prefix.to_owned(),
                    n.parse().map_err(|_| {
                        SourceError::protocol("mysql_file_lineage_new_generation_rebaseline")
                    })?,
                ))
            };
            let (a, an) = split(end)?;
            let (b, bn) = split(file)?;
            if a != b {
                return Err(SourceError::protocol(
                    "mysql_file_lineage_new_generation_rebaseline",
                ));
            }
            Ok((an, ep) >= (bn, p))
        }
        _ => Err(SourceError::protocol(
            "mysql_position_kind_new_generation_rebaseline",
        )),
    }
}

impl MysqlEngine {
    /// Record L0 before any full read, continuously consume during each chunk,
    /// then drain to L_end. The result repairs all bindings for this source.
    /// No old C5 rows may be retained outside this complete replacement scope.
    pub async fn repair_to_c1(
        &mut self,
        bindings: &[RepairBinding],
        client_server_id: u32,
        read: &ReadBudget,
        staging: RepairBudget,
    ) -> Result<SourceRepair> {
        if bindings.is_empty() || staging.max_keys == 0 || staging.max_bytes == 0 {
            return Err(SourceError::config("repair_scope_budget"));
        }
        let mut names = BTreeMap::new();
        let mut plans = BTreeMap::new();
        for binding in bindings {
            if binding.binding_id.is_empty()
                || names
                    .insert(binding.binding_id.clone(), binding.plan.table.clone())
                    .is_some()
            {
                return Err(SourceError::config("repair_binding"));
            }
            let plan = &binding.plan;
            if plan.chunks.iter().any(|c| c.table != plan.table)
                || matches!(plan.key, KeyShape::NoKey)
            {
                return Err(SourceError::config("repair_plan"));
            }
            crate::mysql::c5::Cycle::new(plan.chunks.clone(), 1)?;
            if let Some(old) = plans.insert(plan.table.clone(), plan.clone())
                && old != *plan
            {
                return Err(SourceError::config("repair_conflicting_plans"));
            }
        }
        let tables: Vec<_> = plans.keys().cloned().collect();
        let mut changes = self.changes(&tables, client_server_id).await?;
        let result = async {
            changes
                .retention(staging.required_history_seconds, 0)
                .await?
                .require_sufficient()
                .map_err(|_| {
                    SourceError::protocol("mysql_history_short_new_generation_rebaseline")
                })?;
            self.capture_repair(names, plans, &mut changes, read, staging)
                .await
        }
        .await;
        changes.close().await;
        result
    }

    async fn capture_repair(
        &mut self,
        bindings: BTreeMap<String, TableRef>,
        plans: BTreeMap<TableRef, TableChunkPlan>,
        changes: &mut crate::mysql::MysqlChanges,
        read: &ReadBudget,
        staging: RepairBudget,
    ) -> Result<SourceRepair> {
        let l0 = self
            .watermark()
            .await?
            .ok_or_else(|| SourceError::config("repair_watermark_required"))?;
        let mut cursor = l0.clone();
        let mut overlay = Overlay::default();
        let mut receipts = Vec::new();
        let mut empty_batches = BTreeMap::new();
        let mut during_read = 0;
        for plan in plans.values() {
            for chunk in &plan.chunks {
                let done = AtomicBool::new(false);
                let snapshot = async {
                    let result = self.read_inner(chunk, read).await;
                    done.store(true, Ordering::Release);
                    result
                };
                let consume = async {
                    while !done.load(Ordering::Acquire) {
                        match changes.next_batch(&cursor, read).await? {
                            Some(batch) => {
                                overlay.log(&batch, staging)?;
                                during_read += batch.changes.len() as u64;
                                cursor = batch.position;
                            }
                            None => tokio::time::sleep(Duration::from_millis(2)).await,
                        }
                    }
                    Ok::<_, SourceError>(())
                };
                let (snapshot, consumed) = tokio::join!(snapshot, consume);
                consumed?;
                let (receipt, batches, prints) = snapshot?;
                let mut keys = prints.into_iter();
                for batch in batches {
                    if !empty_batches.contains_key(&plan.table) {
                        let empty = RecordBatch::new_empty(batch.schema());
                        overlay.charged_bytes = overlay
                            .charged_bytes
                            .saturating_add(ipc_digest(std::slice::from_ref(&empty))?.0);
                        if overlay.charged_bytes > staging.max_bytes {
                            return Err(SourceError::budget("mysql_repair_staging_budget"));
                        }
                        empty_batches.insert(plan.table.clone(), empty);
                    }
                    for i in 0..batch.num_rows() {
                        let key = keys
                            .next()
                            .ok_or_else(|| SourceError::protocol("repair_snapshot_keys"))?
                            .key;
                        overlay.snapshot((plan.table.clone(), key), batch.slice(i, 1), staging)?;
                    }
                }
                receipts.push(receipt);
            }
        }
        let end = self
            .watermark()
            .await?
            .ok_or_else(|| SourceError::config("repair_watermark_required"))?;
        if !covers(&end, &cursor)? {
            return Err(SourceError::protocol(
                "mysql_repair_lineage_new_generation_rebaseline",
            ));
        }
        while cursor != end {
            let Some(batch) = changes.next_batch(&cursor, read).await? else {
                // File positions can include a trailing ROTATE/FDE, not a commit.
                if matches!(cursor, Watermark::MysqlFilePos { .. }) {
                    break;
                }
                return Err(SourceError::protocol(
                    "mysql_repair_gap_new_generation_rebaseline",
                ));
            };
            if !covers(&end, &batch.position)? {
                // Nothing after L_end is part of this proposal. It is replayed
                // from the returned cursor when C1 begins after the barrier.
                if matches!(end, Watermark::MysqlFilePos { .. }) {
                    break;
                }
                return Err(SourceError::protocol(
                    "mysql_repair_gap_new_generation_rebaseline",
                ));
            }
            overlay.log(&batch, staging)?;
            cursor = batch.position;
        }
        // Revalidate retention/lineage at the final boundary, including no-op
        // repairs. Do not consume or acknowledge any events after the barrier.
        changes.validate_position(&end).await?;
        Ok(SourceRepair {
            source_id: self.options.source_id.clone(),
            bindings,
            l0,
            end,
            rows: overlay.rows,
            empty_batches,
            receipts,
            during_read,
        })
    }
}

/// Expected complete source/binding scope. All members must stage a repair and
/// explicitly confirm durable application before any C1 cursor is released.
pub struct HandoffBarrier {
    scope: BTreeMap<String, BTreeSet<String>>,
    repairs: BTreeMap<String, SourceRepair>,
    applied: BTreeSet<(String, String)>,
}
impl HandoffBarrier {
    pub fn new(scope: BTreeMap<String, BTreeSet<String>>) -> Result<Self> {
        if scope.is_empty()
            || scope
                .iter()
                .any(|(s, b)| s.is_empty() || b.is_empty() || b.iter().any(String::is_empty))
        {
            return Err(SourceError::config("handoff_scope"));
        }
        Ok(Self {
            scope,
            repairs: BTreeMap::new(),
            applied: BTreeSet::new(),
        })
    }
    pub fn stage(&mut self, repair: SourceRepair) -> Result<()> {
        if self.scope.get(&repair.source_id) != Some(&repair.bindings.keys().cloned().collect())
            || self.repairs.contains_key(&repair.source_id)
        {
            return Err(SourceError::protocol("handoff_repair_scope"));
        }
        self.repairs.insert(repair.source_id.clone(), repair);
        Ok(())
    }
    pub fn repair(&self, source: &str) -> Option<&SourceRepair> {
        self.repairs.get(source)
    }
    /// The loader calls this only after persisting the complete replacement,
    /// tombstones and end position for this binding in one durable operation.
    pub fn confirm_applied(&mut self, source: &str, binding: &str, end: &Watermark) -> Result<()> {
        if self.repairs.len() != self.scope.len() {
            return Err(SourceError::protocol("handoff_scope_incomplete"));
        }
        let repair = self
            .repairs
            .get(source)
            .ok_or_else(|| SourceError::protocol("handoff_source"))?;
        if !repair.bindings.contains_key(binding) || &repair.end != end {
            return Err(SourceError::protocol("handoff_binding_position"));
        }
        self.applied.insert((source.into(), binding.into()));
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.repairs.len() == self.scope.len()
            && self.scope.iter().all(|(s, bindings)| {
                bindings
                    .iter()
                    .all(|b| self.applied.contains(&(s.clone(), b.clone())))
            })
    }
    /// This is a vector of per-source barriers, not a global source snapshot.
    pub fn switch(self) -> Result<BTreeMap<String, Watermark>> {
        if !self.ready() {
            return Err(SourceError::protocol("handoff_scope_incomplete"));
        }
        Ok(self.repairs.into_iter().map(|(s, r)| (s, r.end)).collect())
    }
}

#[cfg(test)]
#[path = "repair_tests.rs"]
mod tests;
