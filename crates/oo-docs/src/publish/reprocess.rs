//! Reprocess with the diff gate (10 4.2, 6.3, D23, B-S10-02): what happens when a file that already has facts is
//! extracted again after a parser or prompt upgrade.
//!
//! The old facts stay valid while the file waits as `outdated` and while the new parse runs. When the new
//! extraction is stored, the old and the new set of `fact_key`s are compared (DOC-G10): a small difference
//! replaces the old extraction at once (facts the new run no longer finds are retracted by the next publish, new
//! ones are drafts like any others); a difference over 30% of the file's facts, or one that drops a fact a live
//! Share's pack uses, is not applied: the old evidence stays, the new facts are held as drafts, and one
//! `reprocess_diff` review item lists both sides. A person decides: accept applies the new extraction, reject
//! keeps the old one and rejects the new facts. Everything here runs in one transaction, so a crash leaves
//! either the old extraction untouched or the new one in place, never half.

use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, Row};

use super::gates::{DOC_G10, ReprocessDiff};
use super::write::{DocumentInput, IndexSummary, index_document};
use crate::store::StoreError;

/// `review_item.kind`.
pub const KIND: &str = "reprocess_diff";

/// What [`index_gated`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reprocess {
    /// The version had no facts yet: a plain first extraction.
    Fresh(IndexSummary),
    /// A reprocess whose diff passed DOC-G10 (it may be empty).
    Applied {
        diff: ReprocessDiff,
        summary: IndexSummary,
    },
    /// A reprocess that failed DOC-G10: old extraction kept, one `reprocess_diff` item queued.
    Held {
        diff: ReprocessDiff,
        reason: &'static str,
        summary: IndexSummary,
    },
}

/// Live fact evidence of one file version: `(evidence id, fact id, fact_key)`.
async fn live_fact_evidence(
    conn: &mut PgConnection,
    version_id: &str,
) -> Result<Vec<(String, String, String)>, StoreError> {
    let rows = sqlx::query(
        "SELECT e.id::text AS ev, f.id::text AS fact, f.fact_key \
           FROM _oo_docs.evidence e JOIN _oo_docs.fact f ON f.id = e.target_id \
          WHERE e.target_kind = 'fact' AND e.version_id = $1::uuid AND e.withdrawn_at IS NULL \
            AND f.status IN ('draft', 'published') \
          ORDER BY f.fact_key, e.id",
    )
    .bind(version_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push((r.try_get("ev")?, r.try_get("fact")?, r.try_get("fact_key")?));
    }
    Ok(out)
}

fn distinct_keys(rows: &[(String, String, String)]) -> Vec<String> {
    let mut k: Vec<String> = rows.iter().map(|r| r.2.clone()).collect();
    k.sort();
    k.dedup();
    k
}

/// Extract `doc` into the store. A version that already has live facts is a reprocess and goes through DOC-G10.
pub async fn index_gated(
    conn: &mut PgConnection,
    doc: &DocumentInput<'_>,
) -> Result<Reprocess, StoreError> {
    let mut tx = conn.begin().await?;
    let prior_rows = live_fact_evidence(&mut tx, doc.version_id).await?;
    if prior_rows.is_empty() {
        let summary = index_document(&mut tx, doc).await?;
        tx.commit().await?;
        return Ok(Reprocess::Fresh(summary));
    }
    let prior_ev: Vec<String> = prior_rows.iter().map(|r| r.0.clone()).collect();
    sqlx::query(
        "UPDATE _oo_docs.evidence SET withdrawn_at = clock_timestamp(), withdrawn_reason = 'reprocessed' \
          WHERE id = ANY($1::text[]::uuid[])",
    )
    .bind(&prior_ev)
    .execute(&mut *tx)
    .await?;
    let summary = index_document(&mut tx, doc).await?;
    let now_rows = live_fact_evidence(&mut tx, doc.version_id).await?;
    let (mut prior_keys, now_keys) = (distinct_keys(&prior_rows), distinct_keys(&now_rows));
    // D90/D91: facts the quality gate now leaves out (a Decision that is a bare word) are not a change a person
    // has to judge; they leave with their evidence.
    if !summary.quality.dropped_names.is_empty() {
        let gone: Vec<String> = prior_keys
            .iter()
            .filter(|k| !now_keys.contains(k))
            .cloned()
            .collect();
        let (tys, names): (Vec<String>, Vec<String>) =
            summary.quality.dropped_names.iter().cloned().unzip();
        let explained: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT f.fact_key FROM _oo_docs.fact f \
               JOIN _oo_docs.entity s ON s.id = f.subject_id \
               LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
              WHERE f.fact_key = ANY($1::text[]) \
                AND ((s.type, s.norm_name) IN (SELECT * FROM unnest($2::text[], $3::text[])) \
                     OR (o.type, o.norm_name) IN (SELECT * FROM unnest($2::text[], $3::text[])))",
        )
        .bind(&gone)
        .bind(&tys)
        .bind(&names)
        .fetch_all(&mut *tx)
        .await?;
        prior_keys.retain(|k| !explained.contains(k));
    }
    let removed_only: Vec<String> = prior_keys
        .iter()
        .filter(|k| !now_keys.contains(k))
        .cloned()
        .collect();
    let share_used: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT f.fact_key FROM _oo_docs.share_fact_use u \
           JOIN _oo_docs.share s ON s.id = u.share_id JOIN _oo_docs.fact f ON f.id = u.fact_id \
          WHERE s.status IN ('active', 'drifted') AND f.fact_key = ANY($1::text[])",
    )
    .bind(&removed_only)
    .fetch_all(&mut *tx)
    .await?;
    let mut diff = ReprocessDiff::of(&prior_keys, &now_keys, &share_used);
    // D93: what a confirmed structure adds (its slots) or takes out (literal facts it has no slot for) is the person's
    // decision already; it does not count against the gate.
    crate::structure::store::explain_diff(&mut tx, doc.version_id, &mut diff).await?;
    match diff.g10() {
        Ok(()) => {
            tx.commit().await?;
            Ok(Reprocess::Applied { diff, summary })
        }
        Err(reason) => {
            // Not applied: the old evidence of the facts the new run lost is live again.
            let restore: Vec<String> = prior_rows
                .iter()
                .filter(|r| diff.removed.contains(&r.2))
                .map(|r| r.0.clone())
                .collect();
            sqlx::query(
                "UPDATE _oo_docs.evidence SET withdrawn_at = NULL, withdrawn_reason = NULL \
                  WHERE id = ANY($1::text[]::uuid[])",
            )
            .bind(&restore)
            .execute(&mut *tx)
            .await?;
            queue_item(&mut tx, doc.version_id, &diff, reason).await?;
            tx.commit().await?;
            Ok(Reprocess::Held {
                diff,
                reason,
                summary,
            })
        }
    }
}

async fn queue_item(
    conn: &mut PgConnection,
    version_id: &str,
    diff: &ReprocessDiff,
    reason: &str,
) -> Result<(), StoreError> {
    let mut h = blake3::Hasher::new();
    h.update(version_id.as_bytes());
    for k in diff.added.iter().chain(&diff.removed) {
        h.update(k.as_bytes());
    }
    let subject = json!({
        "gate": DOC_G10, "prior": diff.prior, "added": diff.added, "removed": diff.removed,
        "share_used_removed": diff.share_used_removed, "version_id": version_id,
    });
    sqlx::query(
        "INSERT INTO _oo_docs.review_item \
           (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, confidence_bp, reason_code) \
         SELECT gen_random_uuid(), 'reprocess_diff', $1, f.drive_id, f.id, fv.id, $2::jsonb, NULL, $3 \
           FROM _oo_docs.file_version fv JOIN _oo_docs.file f ON f.id = fv.file_id WHERE fv.id = $4::uuid \
         ON CONFLICT (dedupe_key) DO NOTHING",
    )
    .bind(format!("reprocess_diff:{}", &h.finalize().to_hex()[..32]))
    .bind(subject.to_string())
    .bind(reason)
    .bind(version_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Accept a `reprocess_diff`: the new extraction replaces the old one. Facts only the old run had lose their
/// evidence of this version and are retracted by the next publish; the held new facts are released.
pub async fn apply(conn: &mut PgConnection, subject: &Value) -> Result<u32, StoreError> {
    let (Some(version_id), Some(removed)) = (
        subject.get("version_id").and_then(Value::as_str),
        subject.get("removed").and_then(Value::as_array),
    ) else {
        return Ok(0);
    };
    let keys: Vec<String> = removed
        .iter()
        .filter_map(|k| k.as_str().map(str::to_owned))
        .collect();
    sqlx::query(
        "UPDATE _oo_docs.evidence e SET withdrawn_at = clock_timestamp(), withdrawn_reason = 'reprocessed' \
          FROM _oo_docs.fact f WHERE e.target_kind = 'fact' AND e.target_id = f.id \
            AND e.version_id = $1::uuid AND e.withdrawn_at IS NULL AND f.fact_key = ANY($2::text[])",
    )
    .bind(version_id)
    .bind(&keys)
    .execute(&mut *conn)
    .await?;
    let n = sqlx::query(
        "UPDATE _oo_docs.fact f SET status = 'retracted', retracted_at = clock_timestamp() \
          WHERE f.origin <> 'human' AND f.status IN ('draft', 'published') AND f.fact_key = ANY($1::text[]) \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'fact' \
                              AND e.target_id = f.id AND e.withdrawn_at IS NULL)",
    )
    .bind(&keys)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// Reject a `reprocess_diff`: the old extraction stays, the held new facts are rejected (drafts only).
pub async fn discard(conn: &mut PgConnection, subject: &Value) -> Result<u32, StoreError> {
    let keys: Vec<String> = subject
        .get("added")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|k| k.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let n = sqlx::query(
        "UPDATE _oo_docs.fact SET status = 'rejected' WHERE status = 'draft' AND fact_key = ANY($1::text[])",
    )
    .bind(&keys)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// SQL condition on the alias `f` (a draft fact): its key is listed in a pending `reprocess_diff` as added.
pub const HELD_SQL: &str = "EXISTS (SELECT 1 FROM _oo_docs.review_item ri WHERE ri.kind = 'reprocess_diff' \
    AND ri.status = 'pending' AND ri.subject->'added' ? f.fact_key)";
