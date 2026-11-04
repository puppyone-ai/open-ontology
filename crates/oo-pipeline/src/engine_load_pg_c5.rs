use super::*;
use oo_connectors::engine::RangeDigest;
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
        if !same_ranges(&previous.plan, plan)
            || previous.parts.len() != plan.chunks.len()
            || digests.len() != plan.chunks.len()
        {
            return Err("c5_rebaseline_required_plan");
        }
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
        next.plan = plan.clone();
        for (chunk, digest) in plan.chunks.iter().zip(&digests) {
            if digest.table != chunk.table || digest.lo != chunk.lo || digest.hi != chunk.hi {
                return Err("c5_digest_scope");
            }
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
        // An established no-change cycle writes no target rows or metadata.
        if replacements.is_empty()
            && next.source_digests == previous.source_digests
            && next.lineage == previous.lineage
        {
            return Ok(());
        }
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
                .bind(&self.plan)
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
            next.run = self.run.clone();
        }
        sqlx::query(
            "INSERT INTO _oo_load.c5_state VALUES($1,$2,$3::jsonb)
             ON CONFLICT(identity) DO UPDATE SET run=EXCLUDED.run,state=EXCLUDED.state",
        )
        .bind(&self.identity)
        .bind(&next.run)
        .bind(serde_json::to_string(&next).map_err(|_| "c5_state")?)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
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
