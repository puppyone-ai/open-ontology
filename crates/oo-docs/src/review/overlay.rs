//! Overlay rows (10 §3.2, §5.3). The table is insert-only: a state change is a
//! new row that names the row it replaces in `payload.supersedes_overlay`; the
//! view `overlay_current` hides the replaced ones.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use sqlx::PgConnection;

use crate::store::StoreError;

/// Insert one overlay row; returns its id.
#[allow(clippy::too_many_arguments)]
pub async fn insert(
    conn: &mut PgConnection,
    kind: &str,
    binds: &str,
    payload: Value,
    author: &str,
    review_id: Option<&str>,
    status: &str,
    supersedes: Option<&str>,
) -> Result<String, StoreError> {
    insert_rev(
        conn, kind, binds, payload, author, review_id, status, supersedes, None,
    )
    .await
}

/// [`insert`] with the `drive_rev` the correction advanced (16.2); the table
/// is insert-only, so the rev has to be given with the row.
#[allow(clippy::too_many_arguments)]
pub async fn insert_rev(
    conn: &mut PgConnection,
    kind: &str,
    binds: &str,
    mut payload: Value,
    author: &str,
    review_id: Option<&str>,
    status: &str,
    supersedes: Option<&str>,
    drive_rev: Option<i64>,
) -> Result<String, StoreError> {
    if let (Some(old), Some(obj)) = (supersedes, payload.as_object_mut()) {
        obj.insert("supersedes_overlay".into(), json!(old));
    }
    let id: String = sqlx::query_scalar(
        "INSERT INTO _oo_docs.overlay (id, kind, binds, payload, author, review_id, status, drive_rev) \
         VALUES (gen_random_uuid(), $1, $2, $3::jsonb, $4, $5::uuid, $6, $7) RETURNING id::text",
    )
    .bind(kind)
    .bind(binds)
    .bind(payload.to_string())
    .bind(author)
    .bind(review_id)
    .bind(status)
    .bind(drive_rev)
    .fetch_one(&mut *conn)
    .await?;
    Ok(id)
}

/// Fact keys among `keys` that a person rejected or replaced: a re-extracted
/// fact with such a key is not a draft again (DOC-G4).
pub async fn rejected_keys(
    conn: &mut PgConnection,
    keys: &[String],
) -> Result<BTreeSet<String>, StoreError> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT binds FROM _oo_docs.overlay_current \
          WHERE kind IN ('fact_reject', 'fact_edit') AND status = 'active' \
            AND binds = ANY($1::text[])",
    )
    .bind(keys)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Fact keys among `keys` that a person asserted.
pub async fn asserted_keys(
    conn: &mut PgConnection,
    keys: &[String],
) -> Result<BTreeSet<String>, StoreError> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT binds FROM _oo_docs.overlay_current \
          WHERE kind = 'fact_assert' AND status = 'active' AND binds = ANY($1::text[])",
    )
    .bind(keys)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Active overlays bound to a fact whose evidence has all disappeared become
/// `orphaned` (a new row) and queue an `orphaned_overlay` item (D22: a person's
/// decision is never retracted by itself). Returns the items queued.
pub async fn sweep_orphans(conn: &mut PgConnection) -> Result<u32, StoreError> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT o.id::text, o.kind, o.binds FROM _oo_docs.overlay_current o \
          WHERE o.status = 'active' AND o.kind IN ('fact_assert', 'fact_edit') \
            AND EXISTS (SELECT 1 FROM _oo_docs.fact f WHERE f.fact_key = o.binds) \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.fact f \
                             JOIN _oo_docs.evidence e ON e.target_kind = 'fact' AND e.target_id = f.id \
                            WHERE f.fact_key = o.binds AND e.withdrawn_at IS NULL) \
          ORDER BY o.id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut queued = 0u32;
    for (id, kind, binds) in rows {
        let new_id = insert(
            conn,
            &kind,
            &binds,
            json!({"orphaned": true}),
            "engine",
            None,
            "orphaned",
            Some(&id),
        )
        .await?;
        let done = sqlx::query(
            "INSERT INTO _oo_docs.review_item (id, kind, dedupe_key, subject, reason_code) \
             VALUES (gen_random_uuid(), 'orphaned_overlay', $1, \
                     jsonb_build_object('overlay_id', $2::text, 'binds', $3::text, \
                                        'missing_evidence', true), 'evidence_gone') \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(format!("orphaned_overlay:{binds}"))
        .bind(&new_id)
        .bind(&binds)
        .execute(&mut *conn)
        .await?;
        queued += u32::try_from(done.rows_affected()).unwrap_or(0);
    }
    Ok(queued)
}
