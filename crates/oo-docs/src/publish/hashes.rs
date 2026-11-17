//! Dependency-chain hashes with early cutoff (10 §16.1, D53).
//!
//! * `assertion_set_hash(file version)` = BLAKE3 of the sorted lines
//!   `fact_key ‖ value ‖ valid_from ‖ valid_to ‖ anchor_key` of the facts that
//!   cite the version. A new version of a file with the same hash changes
//!   nothing the ontology says ([`early_cutoff`]): only evidence moves.
//! * `entity_state_hash(entity)` = BLAKE3 of the sorted lines `fact_key ‖ value`
//!   of its live published facts and its published aliases. The publisher
//!   records it per version only when it changed, so an unchanged hash stops
//!   the chain: no Share drift check for that entity.

use std::collections::BTreeSet;

use sqlx::PgConnection;

use crate::store::StoreError;

/// Hash of sorted lines, lowercase hex of BLAKE3.
pub fn hash_lines(mut lines: Vec<String>) -> String {
    lines.sort();
    lines.dedup();
    let mut h = blake3::Hasher::new();
    h.update(b"oo.docs.hash/v1\0");
    for l in &lines {
        h.update(l.as_bytes());
        h.update(b"\n");
    }
    h.finalize().to_hex().to_string()
}

/// `assertion_set_hash` of one file version; `None` when nothing cites it.
pub async fn assertion_set_hash(
    conn: &mut PgConnection,
    version_id: &str,
) -> Result<Option<String>, StoreError> {
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    );
    let rows: Vec<Row> =
        sqlx::query_as(
            "SELECT f.fact_key, f.value::text, f.valid_from::text, f.valid_to::text, \
                    ev.block_id || '@' || ev.anchor::text \
               FROM _oo_docs.evidence ev JOIN _oo_docs.fact f ON f.id = ev.target_id \
              WHERE ev.target_kind = 'fact' AND ev.version_id = $1::uuid AND ev.withdrawn_at IS NULL",
        )
        .bind(version_id)
        .fetch_all(&mut *conn)
        .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(hash_lines(
        rows.into_iter()
            .map(|(k, v, a, b, anchor)| {
                format!(
                    "{k}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{anchor}",
                    v.unwrap_or_default(),
                    a.unwrap_or_default(),
                    b.unwrap_or_default()
                )
            })
            .collect(),
    )))
}

/// Compute and store the hash of a file version. Returns `(hash, same_as_before)`:
/// `same_as_before` is true when an earlier version of the same file has the
/// same hash (early cutoff).
pub async fn store_assertion_hash(
    conn: &mut PgConnection,
    version_id: &str,
) -> Result<Option<(String, bool)>, StoreError> {
    let Some(hash) = assertion_set_hash(conn, version_id).await? else {
        return Ok(None);
    };
    sqlx::query(
        "UPDATE _oo_docs.file_version SET assertion_set_hash = $2 WHERE id = $1::uuid \
           AND assertion_set_hash IS DISTINCT FROM $2",
    )
    .bind(version_id)
    .bind(&hash)
    .execute(&mut *conn)
    .await?;
    let same: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.file_version cur \
                          JOIN _oo_docs.file_version old ON old.file_id = cur.file_id \
                           AND old.id <> cur.id AND old.created_at < cur.created_at \
                         WHERE cur.id = $1::uuid AND old.assertion_set_hash = $2)",
    )
    .bind(version_id)
    .bind(&hash)
    .fetch_one(&mut *conn)
    .await?;
    Ok(Some((hash, same)))
}

/// Whether the file version repeats the assertions of an earlier version.
pub async fn early_cutoff(conn: &mut PgConnection, version_id: &str) -> Result<bool, StoreError> {
    Ok(store_assertion_hash(conn, version_id)
        .await?
        .is_some_and(|(_, same)| same))
}

/// `entity_state_hash` of entities (by id), current published state.
pub async fn entity_state_hashes(
    conn: &mut PgConnection,
    entity_ids: &[String],
) -> Result<Vec<(String, String)>, StoreError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT e.id::text, l FROM _oo_docs.entity e, LATERAL ( \
            SELECT f.fact_key || E'\\x1f' || COALESCE(f.value::text, '') AS l FROM _oo_docs.fact f \
             WHERE (f.subject_id = e.id OR f.object_id = e.id) AND f.status = 'published' \
               AND f.retracted_at IS NULL \
            UNION ALL \
            SELECT 'alias' || E'\\x1f' || a.norm_alias FROM _oo_docs.alias a \
             WHERE a.entity_id = e.id AND a.status = 'published') x \
          WHERE e.id = ANY($1::text[]::uuid[]) ORDER BY 1, 2",
    )
    .bind(entity_ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut by: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for id in entity_ids {
        by.entry(id.clone()).or_default();
    }
    for (id, l) in rows {
        by.entry(id).or_default().push(l);
    }
    Ok(by
        .into_iter()
        .map(|(id, lines)| (id, hash_lines(lines)))
        .collect())
}

/// Record the entity states of version `n` for `touched` entities; only a
/// changed hash writes a row (early cutoff). Returns the changed entity ids.
pub async fn snapshot_entity_states(
    conn: &mut PgConnection,
    n: i64,
    touched: &BTreeSet<String>,
) -> Result<Vec<String>, StoreError> {
    let ids: Vec<String> = touched.iter().cloned().collect();
    let hashes = entity_state_hashes(conn, &ids).await?;
    let mut changed = Vec::new();
    for (id, hash) in hashes {
        let before: Option<String> = sqlx::query_scalar(
            "SELECT hash FROM _oo_docs.entity_state WHERE entity_id = $1::uuid \
              ORDER BY ontology_version DESC LIMIT 1",
        )
        .bind(&id)
        .fetch_optional(&mut *conn)
        .await?;
        if before.as_deref() == Some(hash.as_str()) {
            continue;
        }
        sqlx::query(
            "INSERT INTO _oo_docs.entity_state (entity_id, ontology_version, hash) \
             VALUES ($1::uuid, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(&id)
        .bind(n)
        .bind(&hash)
        .execute(&mut *conn)
        .await?;
        changed.push(id);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ignores_order_and_duplicates_but_not_content() {
        let a = hash_lines(vec!["b".into(), "a".into(), "a".into()]);
        let b = hash_lines(vec!["a".into(), "b".into()]);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(a, hash_lines(vec!["a".into(), "c".into()]));
        assert_ne!(hash_lines(vec![]), hash_lines(vec!["".into()]));
    }
}
