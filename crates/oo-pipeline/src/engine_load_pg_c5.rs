use super::*;
use oo_connectors::engine::{KeyShape, RangeDigest};
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
        Ok(C5Baseline {
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
        })
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
            if !ids.insert(receipt.chunk_id) || receipt.table != plan.table {
                return Err("c5_replacement_scope");
            }
            let part = next
                .parts
                .iter_mut()
                .find(|p| p.receipt.chunk_id == receipt.chunk_id)
                .ok_or("c5_replacement_scope")?;
            if receipt.plan_revision != part.receipt.plan_revision {
                return Err("c5_replacement_scope");
            }
            *part = C5Part {
                receipt: receipt.clone(),
                stage: self.stage.clone(),
            };
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
    } else if !same_ranges(&previous.plan, plan)
        || previous.parts.len() != plan.chunks.len()
        || digests.len() != plan.chunks.len()
    {
        return Err("c5_rebaseline_required_plan");
    }
    Ok(())
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
}
