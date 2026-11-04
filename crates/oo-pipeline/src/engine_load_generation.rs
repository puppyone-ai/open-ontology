//! Retain a C5 composition independently of its mutable current-view alias.
//! These storage receipts grant nothing and are not native-library receipts.
use super::*;
use sqlx::{Postgres, Transaction};
use std::collections::{BTreeMap, BTreeSet};

pub(super) async fn seal(
    tx: &mut Transaction<'_, Postgres>,
    identity: &str,
    run: &str,
    plan: &TableChunkPlan,
    parts: &[C5Part],
    fields: &[DecodedField],
) -> Result<String> {
    crate::engine_load::validate_plan(plan, &plan.table)?;
    if parts.len() != plan.chunks.len() || fields.is_empty() {
        return Err("generation_incomplete");
    }
    let mut ids = BTreeSet::new();
    let mut groups = BTreeMap::<&str, Vec<String>>::new();
    let definition = fields
        .iter()
        .map(|f| {
            let (_, ty) = column(f)?;
            Ok(format!(
                "{} {ty}{}",
                quote(&f.name)?,
                if f.nullable { "" } else { " NOT NULL" }
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .join(",");
    for part in parts {
        let receipt = &part.receipt;
        if !ids.insert(receipt.chunk_id)
            || !plan.chunks.iter().any(|c| {
                c.chunk_id == receipt.chunk_id
                    && c.plan_revision == receipt.plan_revision
                    && c.table == receipt.table
            })
        {
            return Err("generation_incomplete");
        }
        let stored: Option<(String, String)> = sqlx::query_as(
            "SELECT c.receipt::text,s.definition FROM _oo_load.chunks c
             JOIN _oo_load.schemas s USING(stage)
             WHERE c.stage=$1 AND c.chunk=$2 AND s.identity=$3",
        )
        .bind(&part.stage)
        .bind(i64::from(receipt.chunk_id))
        .bind(identity)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db)?;
        let (stored, schema) = stored.ok_or("generation_part_absent")?;
        if serde_json::from_str::<ChunkReceipt>(&stored).map_err(|_| "load_receipt")? != *receipt
            || schema != definition
        {
            return Err("generation_part_mismatch");
        }
        groups
            .entry(&part.stage)
            .or_default()
            .push(receipt.chunk_id.to_string());
    }
    let names = fields
        .iter()
        .map(|f| quote(&f.name))
        .collect::<Result<Vec<_>>>()?
        .join(",");
    let selects = groups
        .into_iter()
        .map(|(stage, chunks)| {
            Ok(format!(
                "SELECT {names} FROM _oo_load.{} WHERE __oo_chunk IN ({})",
                quote(stage)?,
                chunks.join(",")
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .join(" UNION ALL ");
    let relation = format!(
        "g_{}",
        &hash(&serde_json::to_vec(&(identity, run)).map_err(|_| "load_identity")?)[..40]
    );
    let recipe = serde_json::json!({
        "protocol":"oo.load.storage-generation/v1",
        "plan":plan,"parts":parts,"definition":definition
    });
    // Never replace a retained view: even a replay must match its sealed recipe.
    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT relation_name,recipe::text FROM _oo_load.generations
         WHERE identity=$1 AND run=$2",
    )
    .bind(identity)
    .bind(run)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db)?;
    if let Some((name, old)) = existing {
        if name != relation
            || serde_json::from_str::<serde_json::Value>(&old).map_err(|_| "generation_recipe")?
                != recipe
        {
            return Err("generation_conflict");
        }
        return Ok(relation);
    }
    sqlx::query(AssertSqlSafe(format!(
        "CREATE VIEW _oo_load.{} AS {selects}",
        quote(&relation)?
    )))
    .execute(&mut **tx)
    .await
    .map_err(db)?;
    sqlx::query(
        "INSERT INTO _oo_load.generations(identity,run,relation_name,recipe)
         VALUES($1,$2,$3,$4::jsonb)",
    )
    .bind(identity)
    .bind(run)
    .bind(&relation)
    .bind(recipe.to_string())
    .execute(&mut **tx)
    .await
    .map_err(db)?;
    Ok(relation)
}

impl PgTarget<'_> {
    /// Return a sealed snapshot/C5 storage view, never the mutable current alias.
    /// No consumer grants or Gateway publication are implied. C1 overlays are
    /// deliberately absent: their live delta tables are not immutable cuts.
    pub async fn generation_relation(&mut self, run: &str) -> Result<Option<String>> {
        let relation: Option<String> = sqlx::query_scalar(
            "SELECT relation_name FROM _oo_load.generations WHERE identity=$1 AND run=$2",
        )
        .bind(&self.identity)
        .bind(run)
        .fetch_optional(&mut *self.conn)
        .await
        .map_err(db)?;
        relation
            .map(|name| quote(&name).map(|name| format!("_oo_load.{name}")))
            .transpose()
    }
}
