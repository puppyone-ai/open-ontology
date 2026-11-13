use super::*;
use oo_connectors::engine::{ChunkSpec, ChunkStrategy, KeyShape, RangeDigest};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct C5Part {
    pub receipt: ChunkReceipt,
    pub stage: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct C5Baseline {
    pub run: String,
    pub plan: TableChunkPlan,
    pub parts: Vec<C5Part>,
    pub source_digests: Vec<RangeDigest>,
    #[serde(default)]
    pub lineage: Option<String>,
}

/// Bounded, order-independent payload evidence. Addition, rather than XOR,
/// preserves duplicate multiplicity even when physical chunk boundaries move.
#[derive(Default)]
pub struct TableContentDigest([u8; 32]);

impl TableContentDigest {
    pub fn add(&mut self, decoded: &DecodedStream) -> Result<()> {
        for row in 0..decoded.rows {
            let mut hash = Sha256::new();
            for (field, cells) in decoded.fields.iter().zip(&decoded.columns) {
                let (oid, ty) = column(field)?;
                let mut value = Vec::new();
                if let NativeCell::UInt64(value_u64) = &cells[row] {
                    cell_text(
                        &mut value,
                        oid,
                        -1,
                        &NativeCell::Decimal {
                            precision: 20,
                            scale: 0,
                            coefficient: i128::from(*value_u64),
                        },
                    )?;
                } else {
                    cell_text(&mut value, oid, -1, &cells[row])?;
                }
                for bytes in [field.name.as_bytes(), ty.as_bytes(), value.as_slice()] {
                    hash.update((bytes.len() as u64).to_be_bytes());
                    hash.update(bytes);
                }
            }
            let row_hash = hash.finalize();
            let mut carry = 0u16;
            for (sum, byte) in self.0.iter_mut().rev().zip(row_hash.iter().rev()) {
                carry += u16::from(*sum) + u16::from(*byte);
                *sum = carry as u8;
                carry >>= 8;
            }
        }
        Ok(())
    }

    pub fn finish(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl PgTarget<'_> {
    pub async fn c5_baseline(&mut self) -> Result<C5Baseline> {
        let installed: bool =
            sqlx::query_scalar("SELECT to_regclass('_oo_load.sync_bindings') IS NOT NULL")
                .fetch_one(&mut *self.conn)
                .await
                .map_err(db)?;
        if installed {
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM _oo_load.sync_bindings WHERE identity=$1)",
            )
            .bind(&self.identity)
            .fetch_one(&mut *self.conn)
            .await
            .map_err(db)?;
            if active {
                return Err("c5_rebaseline_required_active_sync");
            }
        }
        sqlx::raw_sql(include_str!("../migrations/0121_engine_c5.sql"))
            .execute(&mut *self.conn)
            .await
            .map_err(db)?;
        let (run, stage, plan): (String, String, String) = sqlx::query_as(
            "SELECT p.run,p.stage,p.plan::text FROM _oo_load.publications p
             JOIN _oo_load.current c USING(identity,run) WHERE p.identity=$1",
        )
        .bind(&self.identity)
        .fetch_one(&mut *self.conn)
        .await
        .map_err(db)?;
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state::text FROM _oo_load.c5_state WHERE identity=$1 AND run=$2",
        )
        .bind(&self.identity)
        .bind(&run)
        .fetch_optional(&mut *self.conn)
        .await
        .map_err(db)?;
        if let Some(state) = state {
            return serde_json::from_str(&state).map_err(|_| "c5_state");
        }
        let receipts: Vec<String> = sqlx::query_scalar(
            "SELECT receipt::text FROM _oo_load.chunks WHERE stage=$1 ORDER BY chunk",
        )
        .bind(&stage)
        .fetch_all(&mut *self.conn)
        .await
        .map_err(db)?;
        let seeds: bool =
            sqlx::query_scalar("SELECT to_regclass('_oo_load.c5_load_digests') IS NOT NULL")
                .fetch_one(&mut *self.conn)
                .await
                .map_err(db)?;
        let seeded: Option<String> = if seeds {
            sqlx::query_scalar(
                "SELECT digests::text FROM _oo_load.c5_load_digests WHERE identity=$1 AND run=$2",
            )
            .bind(&self.identity)
            .bind(&run)
            .fetch_optional(&mut *self.conn)
            .await
            .map_err(db)?
        } else {
            None
        };
        let mut baseline = C5Baseline {
            run,
            plan: serde_json::from_str(&plan).map_err(|_| "c5_plan")?,
            parts: receipts
                .into_iter()
                .map(|r| {
                    Ok(C5Part {
                        receipt: serde_json::from_str(&r).map_err(|_| "c5_receipt")?,
                        stage: stage.clone(),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            source_digests: vec![],
            lineage: None,
        };
        if let Some(seeded) = seeded {
            let proofs: Vec<RangeDigest> =
                serde_json::from_str(&seeded).map_err(|_| "c5_load_digests")?;
            baseline.source_digests = seeded_digests(&baseline, &proofs);
        }
        Ok(baseline)
    }

    /// Record the per-range digests an initial load proved in each payload's
    /// own read transaction as this publication's C5 source digests, so the
    /// first C5 cycle transfers only ranges whose digest changed instead of
    /// the whole table. Only digests that exactly match a published keyset
    /// range and its row count are kept; anything else is read as before.
    /// Returns how many ranges were seeded.
    pub async fn seed_c5_digests(&mut self, proofs: &[RangeDigest]) -> Result<usize> {
        let mut baseline = match self.c5_baseline().await {
            Ok(baseline) => baseline,
            // A C1 binding owns this publication; C5 never reads it.
            Err("c5_rebaseline_required_active_sync") => return Ok(0),
            Err(error) => return Err(error),
        };
        let digests = seeded_digests(&baseline, proofs);
        if baseline.run != self.run || !baseline.source_digests.is_empty() || digests.is_empty() {
            return Ok(0);
        }
        // Kept apart from c5_state: a publication with C5 state is one a C5
        // cycle published, which C1 must not adopt without a handoff.
        sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS _oo_load.c5_load_digests (
               identity text PRIMARY KEY, run text NOT NULL, digests jsonb NOT NULL)",
        )
        .execute(&mut *self.conn)
        .await
        .map_err(db)?;
        sqlx::query(
            "INSERT INTO _oo_load.c5_load_digests VALUES($1,$2,$3::jsonb)
             ON CONFLICT(identity) DO UPDATE SET run=EXCLUDED.run,digests=EXCLUDED.digests",
        )
        .bind(&self.identity)
        .bind(&baseline.run)
        .bind(serde_json::to_string(&digests).map_err(|_| "c5_load_digests")?)
        .execute(&mut *self.conn)
        .await
        .map_err(db)?;
        baseline.source_digests = digests;
        Ok(baseline.source_digests.len())
    }

    /// Confirm source evidence and atomically compose unchanged parent ranges
    /// with staged replacements. No payload copies from the parent are made.
    pub async fn publish_c5(
        &mut self,
        previous: &C5Baseline,
        plan: &TableChunkPlan,
        digests: Vec<RangeDigest>,
        replacements: &[ChunkReceipt],
        lineage: &str,
    ) -> Result<()> {
        crate::engine_load::validate_plan(plan, &plan.table)?;
        let whole_table = keyless_transition(&previous.plan, plan);
        validate_c5_replacement(previous, plan, &digests, replacements)?;
        let mut next = previous.clone();
        if lineage.is_empty()
            || previous
                .lineage
                .as_deref()
                .is_some_and(|old| old != lineage)
        {
            return Err("c5_rebaseline_required_lineage");
        }
        next.lineage = Some(lineage.into());
        if !whole_table || !replacements.is_empty() {
            next.plan = plan.clone();
        }
        for (chunk, digest) in plan.chunks.iter().zip(&digests).filter(|_| !whole_table) {
            if digest.table != chunk.table || digest.lo != chunk.lo || digest.hi != chunk.hi {
                return Err("c5_digest_scope");
            }
        }
        if whole_table && !replacements.is_empty() {
            next.parts = replacements
                .iter()
                .map(|receipt| C5Part {
                    receipt: receipt.clone(),
                    stage: self.stage.clone(),
                })
                .collect();
        }
        let mut ids = std::collections::BTreeSet::new();
        for receipt in replacements {
            if !ids.insert(receipt.chunk_id)
                || receipt.table != plan.table
                || !plan.chunks.iter().any(|c| {
                    c.chunk_id == receipt.chunk_id && c.plan_revision == receipt.plan_revision
                })
            {
                return Err("c5_replacement_scope");
            }
        }
        if !whole_table {
            next.parts = compose_parts(previous, plan, replacements, &self.stage)?;
        }
        next.source_digests = digests;
        let sealed_columns = self.sealed_columns().await?;
        let catalog_changed = match (&self.source_columns, sealed_columns) {
            (Some(live), Some(sealed)) => live != &sealed.columns,
            _ => false,
        };
        // An established no-change cycle writes no target rows or metadata.
        if replacements.is_empty()
            && next.source_digests == previous.source_digests
            && next.lineage == previous.lineage
            && !catalog_changed
        {
            return Ok(());
        }
        if !replacements.is_empty() {
            next.run = self.run.clone();
        }
        let encoded = self.encode_metadata(&next)?;
        let publication_plan = self.encode_metadata(plan)?;
        let mut tx = self.conn.begin().await.map_err(db)?;
        let current: String =
            sqlx::query_scalar("SELECT run FROM _oo_load.current WHERE identity=$1 FOR UPDATE")
                .bind(&self.identity)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if current != previous.run {
            return Err("c5_stale_parent");
        }
        if !replacements.is_empty() {
            if serde_json::to_string(plan).map_err(|_| "c5_plan")? != self.plan {
                return Err("c5_plan_changed");
            }
            let fields = self.fields.as_ref().ok_or("c5_schema")?;
            let names = fields
                .iter()
                .map(|f| quote(&f.name))
                .collect::<Result<Vec<_>>>()?
                .join(",");
            let sealed = generation::seal(
                &mut tx,
                &self.identity,
                &self.run,
                plan,
                &publication_plan,
                &next.parts,
                fields,
            )
            .await?;
            sqlx::query(AssertSqlSafe(format!(
                "CREATE OR REPLACE VIEW oo_load_data.{} AS SELECT {names} FROM _oo_load.{}",
                quote(&self.view)?,
                quote(&sealed)?
            )))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            sqlx::query("INSERT INTO _oo_load.publications VALUES($1,$2,$3::jsonb,$4)")
                .bind(&self.identity)
                .bind(&self.run)
                .bind(&publication_plan)
                .bind(&self.stage)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            sqlx::query("UPDATE _oo_load.current SET run=$2 WHERE identity=$1")
                .bind(&self.identity)
                .bind(&self.run)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        sqlx::query(
            "INSERT INTO _oo_load.c5_state VALUES($1,$2,$3::jsonb)
             ON CONFLICT(identity) DO UPDATE SET run=EXCLUDED.run,state=EXCLUDED.state",
        )
        .bind(&self.identity)
        .bind(&next.run)
        .bind(encoded)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        if catalog_changed {
            sqlx::query("UPDATE _oo_load.column_catalog SET columns=$2::jsonb WHERE identity=$1")
                .bind(&self.identity)
                .bind(
                    serde_json::to_string(&self.source_columns)
                        .map_err(|_| "load_column_catalog")?,
                )
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        #[cfg(feature = "failpoints")]
        crate::failpoint::hit("c5_commit");
        tx.commit().await.map_err(db)
    }

    pub async fn generations(&mut self) -> Result<i64> {
        sqlx::query_scalar("SELECT count(*) FROM _oo_load.publications WHERE identity=$1")
            .bind(&self.identity)
            .fetch_one(&mut *self.conn)
            .await
            .map_err(db)
    }
}

/// Proofs that describe exactly one published keyset range: same table and
/// bounds and the same row count as the part published for that range.
fn seeded_digests(baseline: &C5Baseline, proofs: &[RangeDigest]) -> Vec<RangeDigest> {
    if baseline.plan.key == KeyShape::NoKey
        || !matches!(baseline.plan.strategy, ChunkStrategy::Keyset { .. })
    {
        return Vec::new();
    }
    let mut digests = Vec::new();
    for chunk in &baseline.plan.chunks {
        let mut matching = proofs
            .iter()
            .filter(|d| d.table == chunk.table && d.lo == chunk.lo && d.hi == chunk.hi);
        let (Some(proof), None) = (matching.next(), matching.next()) else {
            continue;
        };
        let part = baseline.parts.iter().find(|p| {
            p.receipt.chunk_id == chunk.chunk_id && p.receipt.plan_revision == chunk.plan_revision
        });
        if part.is_some_and(|p| p.receipt.rows == proof.rows && p.receipt.table == chunk.table) {
            digests.push(proof.clone());
        }
    }
    digests
}

fn keyless_transition(previous: &TableChunkPlan, plan: &TableChunkPlan) -> bool {
    previous.protocol == plan.protocol
        && previous.table == plan.table
        && previous.key == KeyShape::NoKey
        && plan.key == KeyShape::NoKey
}

fn validate_c5_replacement(
    previous: &C5Baseline,
    plan: &TableChunkPlan,
    digests: &[RangeDigest],
    replacements: &[ChunkReceipt],
) -> Result<()> {
    if keyless_transition(&previous.plan, plan) {
        // Physical chunks are a generation's private storage layout, never
        // durable ranges. The only durable source evidence is the whole table.
        if digests.len() != 1
            || digests[0].table != plan.table
            || digests[0].lo.is_some()
            || digests[0].hi.is_some()
        {
            return Err("c5_digest_scope");
        }
        if replacements.is_empty() && digests != previous.source_digests {
            return Err("c5_incomplete_table_replacement");
        }
        if !replacements.is_empty()
            && (replacements.len() != plan.chunks.len()
                || plan.chunks.iter().any(|chunk| {
                    replacements
                        .iter()
                        .filter(|r| {
                            r.table == chunk.table
                                && r.chunk_id == chunk.chunk_id
                                && r.plan_revision == chunk.plan_revision
                        })
                        .count()
                        != 1
                })
                || replacements
                    .iter()
                    .try_fold(0u64, |n, r| n.checked_add(r.rows))
                    != Some(digests[0].rows))
        {
            return Err("c5_incomplete_table_replacement");
        }
    } else if !refines(&previous.plan, plan, replacements)
        || previous.parts.len() != previous.plan.chunks.len()
        || digests.len() != plan.chunks.len()
    {
        return Err("c5_rebaseline_required_plan");
    }
    Ok(())
}

fn same_chunk(a: &ChunkSpec, b: &ChunkSpec) -> bool {
    a.table == b.table
        && a.plan_revision == b.plan_revision
        && a.chunk_id == b.chunk_id
        && a.lo == b.lo
        && a.hi == b.hi
}

/// A keyed C5 plan may keep its parent's ranges or refine them: a parent range
/// that outgrew the chunk budget (an append tail, say) is covered exactly by
/// newer-revision children with fresh ids, and every child is a staged
/// replacement. No parent payload is carried into a range it did not prove.
pub fn refines(
    previous: &TableChunkPlan,
    plan: &TableChunkPlan,
    replaced: &[ChunkReceipt],
) -> bool {
    if same_ranges(previous, plan) {
        return true;
    }
    if previous.protocol != plan.protocol
        || previous.table != plan.table
        || previous.key != plan.key
        || previous.strategy != plan.strategy
        || !matches!(
            plan.strategy,
            oo_connectors::engine::ChunkStrategy::Keyset { .. }
        )
    {
        return false;
    }
    let old_ids: std::collections::BTreeSet<u32> =
        previous.chunks.iter().map(|c| c.chunk_id).collect();
    let mut next = plan.chunks.iter().peekable();
    for parent in &previous.chunks {
        match next.peek() {
            Some(chunk) if same_chunk(chunk, parent) => {
                next.next();
                continue;
            }
            Some(chunk) if chunk.lo == parent.lo => {}
            _ => return false,
        }
        loop {
            let Some(child) = next.next() else {
                return false;
            };
            if child.table != parent.table
                || child.plan_revision <= parent.plan_revision
                || old_ids.contains(&child.chunk_id)
                || !replaced.iter().any(|r| {
                    r.table == child.table
                        && r.chunk_id == child.chunk_id
                        && r.plan_revision == child.plan_revision
                })
            {
                return false;
            }
            if child.hi == parent.hi {
                break;
            }
            if child.hi.is_none() {
                return false;
            }
        }
    }
    next.next().is_none()
}

/// Parts in plan order: staged replacements, else the parent's unchanged part.
fn compose_parts(
    previous: &C5Baseline,
    plan: &TableChunkPlan,
    replacements: &[ChunkReceipt],
    stage: &str,
) -> Result<Vec<C5Part>> {
    plan.chunks
        .iter()
        .map(|chunk| {
            if let Some(receipt) = replacements.iter().find(|r| r.chunk_id == chunk.chunk_id) {
                if receipt.plan_revision != chunk.plan_revision {
                    return Err("c5_replacement_scope");
                }
                return Ok(C5Part {
                    receipt: receipt.clone(),
                    stage: stage.into(),
                });
            }
            previous
                .parts
                .iter()
                .find(|p| {
                    p.receipt.chunk_id == chunk.chunk_id
                        && p.receipt.plan_revision == chunk.plan_revision
                })
                .cloned()
                .ok_or("c5_replacement_scope")
        })
        .collect()
}

pub fn same_ranges(a: &TableChunkPlan, b: &TableChunkPlan) -> bool {
    a.protocol == b.protocol
        && a.table == b.table
        && a.key == b.key
        && a.strategy == b.strategy
        && a.chunks.len() == b.chunks.len()
        && a.chunks.iter().zip(&b.chunks).all(|(a, b)| {
            a.table == b.table
                && a.plan_revision == b.plan_revision
                && a.chunk_id == b.chunk_id
                && a.lo == b.lo
                && a.hi == b.hi
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_connectors::engine::{
        ChunkSpec, ChunkStrategy, ENGINE_PROTOCOL, KeyValue, PhysicalRangeKind, TableRef,
    };

    #[test]
    fn table_payload_digest_ignores_layout_but_preserves_duplicates() {
        let stream = |values: &[i64]| DecodedStream {
            rows: values.len(),
            fields: vec![DecodedField {
                name: "value".into(),
                nullable: true,
                arrow_type: "int64".into(),
                precision: None,
                scale: None,
                timezone: None,
            }],
            columns: vec![
                values
                    .iter()
                    .map(|value| NativeCell::Int {
                        bit_width: 64,
                        value: *value,
                    })
                    .collect(),
            ],
        };
        let digest = |parts: &[&[i64]]| {
            let mut digest = TableContentDigest::default();
            for part in parts {
                digest.add(&stream(part)).unwrap();
            }
            digest.finish()
        };
        let baseline = digest(&[&[1, 2, 2, 3]]);
        assert_eq!(baseline, digest(&[&[2, 3], &[1, 2]]));
        assert_ne!(baseline, digest(&[&[1, 2, 3]]));
        assert_ne!(baseline, digest(&[&[1, 2, 2, 2]]));
        assert_ne!(digest(&[&[]]), digest(&[&[1, 1]]));
    }

    fn plan(strategy: ChunkStrategy, revision: u32, split: bool) -> TableChunkPlan {
        let table = TableRef {
            schema: "fixture".into(),
            name: "no_key".into(),
        };
        let edge = Some(vec![KeyValue::Locator("(10,0)".into())]);
        let chunk = ChunkSpec {
            table: table.clone(),
            plan_revision: revision,
            chunk_id: 0,
            lo: None,
            hi: if split { edge.clone() } else { None },
            estimated_rows: None,
        };
        let mut chunks = vec![chunk.clone()];
        if split {
            chunks.push(ChunkSpec {
                chunk_id: 1,
                lo: edge,
                hi: None,
                ..chunk
            });
        }
        TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table,
            key: KeyShape::NoKey,
            strategy,
            estimated_rows: None,
            chunks,
        }
    }

    #[test]
    fn load_proofs_seed_only_exactly_matching_keyset_ranges() {
        let table = TableRef {
            schema: "public".into(),
            name: "bulk".into(),
        };
        let edge = |v: i64| Some(vec![KeyValue::Int(v.into())]);
        let chunk = |id: u32, lo, hi| ChunkSpec {
            table: table.clone(),
            plan_revision: 1,
            chunk_id: id,
            lo,
            hi,
            estimated_rows: None,
        };
        let chunks = vec![
            chunk(0, None, edge(10)),
            chunk(1, edge(10), edge(20)),
            chunk(2, edge(20), None),
        ];
        let part = |id: u32, rows: u64| C5Part {
            receipt: ChunkReceipt {
                table: table.clone(),
                plan_revision: 1,
                chunk_id: id,
                rows,
                bytes: 1,
                sha256: "payload".into(),
                low: None,
                high: None,
            },
            stage: "stage".into(),
        };
        let baseline = C5Baseline {
            run: "load".into(),
            plan: TableChunkPlan {
                protocol: ENGINE_PROTOCOL.into(),
                table: table.clone(),
                key: KeyShape::IntegerPrimaryKey {
                    column: "id".into(),
                },
                strategy: ChunkStrategy::Keyset {
                    columns: vec!["id".into()],
                },
                estimated_rows: None,
                chunks: chunks.clone(),
            },
            parts: vec![part(0, 9), part(1, 10), part(2, 5)],
            source_digests: vec![],
            lineage: None,
        };
        let proof = |c: &ChunkSpec, rows: u64| RangeDigest {
            table: table.clone(),
            lo: c.lo.clone(),
            hi: c.hi.clone(),
            rows,
            digest: format!("d{}", c.chunk_id),
        };
        // Chunk 1's proof disagrees with its published row count; chunk 2
        // has two proofs. Neither may skip the first cycle's transfer.
        let proofs = vec![
            proof(&chunks[0], 9),
            proof(&chunks[1], 11),
            proof(&chunks[2], 5),
            RangeDigest {
                digest: "other".into(),
                ..proof(&chunks[2], 5)
            },
        ];
        assert_eq!(
            seeded_digests(&baseline, &proofs),
            vec![proof(&chunks[0], 9)]
        );
        let mut unbounded = proof(&chunks[0], 9);
        unbounded.hi = None;
        assert!(seeded_digests(&baseline, &[unbounded]).is_empty());
        for (key, strategy) in [
            (KeyShape::NoKey, baseline.plan.strategy.clone()),
            (baseline.plan.key.clone(), ChunkStrategy::SingleRead),
        ] {
            let mut other = baseline.clone();
            other.plan.key = key;
            other.plan.strategy = strategy;
            assert!(seeded_digests(&other, &proofs).is_empty());
        }
    }

    #[test]
    fn keyless_engines_replace_all_parts_without_reusing_physical_ranges() {
        for (engine, strategy) in [
            ("postgres13", ChunkStrategy::SingleRead),
            (
                "postgres14-18",
                ChunkStrategy::PhysicalRange {
                    kind: PhysicalRangeKind::Ctid,
                },
            ),
            (
                "oracle",
                ChunkStrategy::PhysicalRange {
                    kind: PhysicalRangeKind::Rowid,
                },
            ),
            ("mysql", ChunkStrategy::SingleRead),
            ("sqlserver", ChunkStrategy::SingleRead),
            ("mariadb", ChunkStrategy::SingleRead),
            ("tidb", ChunkStrategy::SingleRead),
            ("oceanbase", ChunkStrategy::SingleRead),
            ("db2", ChunkStrategy::SingleRead),
            ("kingbase", ChunkStrategy::SingleRead),
        ] {
            let physical = matches!(strategy, ChunkStrategy::PhysicalRange { .. });
            let old_plan = plan(strategy.clone(), 1, physical);
            let live = plan(strategy, 19, false);
            let digest = RangeDigest {
                table: live.table.clone(),
                lo: None,
                hi: None,
                rows: 2,
                digest: "whole".into(),
            };
            let mut previous = C5Baseline {
                run: "baseline".into(),
                plan: old_plan,
                parts: vec![],
                source_digests: vec![],
                lineage: None,
            };
            let receipt = ChunkReceipt {
                table: live.table.clone(),
                plan_revision: 19,
                chunk_id: 0,
                rows: 2,
                bytes: 10,
                sha256: "payload".into(),
                low: None,
                high: None,
            };
            assert!(!same_ranges(&previous.plan, &live), "{engine}");
            assert_eq!(
                validate_c5_replacement(
                    &previous,
                    &live,
                    std::slice::from_ref(&digest),
                    std::slice::from_ref(&receipt)
                ),
                Ok(()),
                "{engine}"
            );
            assert!(
                validate_c5_replacement(&previous, &live, std::slice::from_ref(&digest), &[])
                    .is_err(),
                "{engine}"
            );
            let mut stale = receipt.clone();
            stale.plan_revision = 1;
            assert!(
                validate_c5_replacement(&previous, &live, std::slice::from_ref(&digest), &[stale])
                    .is_err()
            );
            let mut missing = receipt.clone();
            missing.rows = 1;
            assert!(
                validate_c5_replacement(
                    &previous,
                    &live,
                    std::slice::from_ref(&digest),
                    &[missing]
                )
                .is_err()
            );
            assert!(
                validate_c5_replacement(
                    &previous,
                    &live,
                    std::slice::from_ref(&digest),
                    &[receipt.clone(), receipt]
                )
                .is_err()
            );
            previous.source_digests = vec![digest.clone()];
            assert_eq!(
                validate_c5_replacement(&previous, &live, std::slice::from_ref(&digest), &[]),
                Ok(()),
                "{engine}"
            );
            let mut bounded = digest;
            bounded.lo = Some(vec![KeyValue::Locator("old".into())]);
            assert!(validate_c5_replacement(&previous, &live, &[bounded], &[]).is_err());
            let mut keyed = live.clone();
            keyed.key = KeyShape::IntegerPrimaryKey {
                column: "id".into(),
            };
            assert!(!keyless_transition(&previous.plan, &keyed));
        }
    }

    fn keyed(chunks: &[(u32, u32, Option<i64>, Option<i64>)]) -> TableChunkPlan {
        let table = TableRef {
            schema: "fixture".into(),
            name: "other".into(),
        };
        let key = |v: Option<i64>| v.map(|v| vec![KeyValue::Int(v.into())]);
        TableChunkPlan {
            protocol: ENGINE_PROTOCOL.into(),
            table: table.clone(),
            key: KeyShape::IntegerPrimaryKey {
                column: "id".into(),
            },
            strategy: ChunkStrategy::Keyset {
                columns: vec!["id".into()],
            },
            estimated_rows: None,
            chunks: chunks
                .iter()
                .map(|(id, revision, lo, hi)| ChunkSpec {
                    table: table.clone(),
                    plan_revision: *revision,
                    chunk_id: *id,
                    lo: key(*lo),
                    hi: key(*hi),
                    estimated_rows: None,
                })
                .collect(),
        }
    }

    fn staged(plan: &TableChunkPlan, ids: &[u32]) -> Vec<ChunkReceipt> {
        plan.chunks
            .iter()
            .filter(|c| ids.contains(&c.chunk_id))
            .map(|c| ChunkReceipt {
                table: c.table.clone(),
                plan_revision: c.plan_revision,
                chunk_id: c.chunk_id,
                rows: 1,
                bytes: 1,
                sha256: format!("new-{}", c.chunk_id),
                low: None,
                high: None,
            })
            .collect()
    }

    #[test]
    fn grown_append_tail_refines_only_into_replaced_newer_children() {
        let previous = keyed(&[(0, 1, None, Some(100)), (1, 1, Some(100), None)]);
        // The open tail outgrew its budget: two fresh, newer children.
        let split = keyed(&[
            (0, 1, None, Some(100)),
            (2, 2, Some(100), Some(200)),
            (3, 2, Some(200), None),
        ]);
        assert!(!same_ranges(&previous, &split));
        assert!(refines(&previous, &split, &staged(&split, &[2, 3])));
        assert!(refines(&previous, &previous, &[]));
        // Every child must be proven by a staged replacement.
        assert!(!refines(&previous, &split, &staged(&split, &[2])));
        // Children may not reuse a parent id or an older revision.
        let reused = keyed(&[
            (0, 1, None, Some(100)),
            (1, 2, Some(100), Some(200)),
            (3, 2, Some(200), None),
        ]);
        assert!(!refines(&previous, &reused, &staged(&reused, &[1, 3])));
        let older = keyed(&[
            (0, 1, None, Some(100)),
            (2, 1, Some(100), Some(200)),
            (3, 1, Some(200), None),
        ]);
        assert!(!refines(&previous, &older, &staged(&older, &[2, 3])));
        // A moved boundary or dropped tail is not a refinement.
        let moved = keyed(&[(0, 1, None, Some(150)), (2, 2, Some(150), None)]);
        assert!(!refines(&previous, &moved, &staged(&moved, &[0, 2])));
        let short = keyed(&[(0, 1, None, Some(100)), (2, 2, Some(100), Some(200))]);
        assert!(!refines(&previous, &short, &staged(&short, &[2])));
    }

    #[test]
    fn refined_parts_keep_unchanged_parents_and_drop_the_split_parent() {
        let previous_plan = keyed(&[(0, 1, None, Some(100)), (1, 1, Some(100), None)]);
        let old = staged(&previous_plan, &[0, 1]);
        let previous = C5Baseline {
            run: "baseline".into(),
            plan: previous_plan.clone(),
            parts: old
                .iter()
                .map(|receipt| C5Part {
                    receipt: receipt.clone(),
                    stage: "s_old".into(),
                })
                .collect(),
            source_digests: vec![],
            lineage: None,
        };
        let split = keyed(&[
            (0, 1, None, Some(100)),
            (2, 2, Some(100), Some(200)),
            (3, 2, Some(200), None),
        ]);
        let replacements = staged(&split, &[2, 3]);
        let digests: Vec<RangeDigest> = split
            .chunks
            .iter()
            .map(|c| RangeDigest {
                table: c.table.clone(),
                lo: c.lo.clone(),
                hi: c.hi.clone(),
                rows: 1,
                digest: "d".into(),
            })
            .collect();
        assert_eq!(
            validate_c5_replacement(&previous, &split, &digests, &replacements),
            Ok(())
        );
        assert!(validate_c5_replacement(&previous, &split, &digests[..2], &replacements).is_err());
        let parts = compose_parts(&previous, &split, &replacements, "s_new").unwrap();
        let ids: Vec<(u32, &str)> = parts
            .iter()
            .map(|p| (p.receipt.chunk_id, p.stage.as_str()))
            .collect();
        assert_eq!(ids, [(0, "s_old"), (2, "s_new"), (3, "s_new")]);
        assert!(compose_parts(&previous, &split, &replacements[..1], "s_new").is_err());
    }
}
