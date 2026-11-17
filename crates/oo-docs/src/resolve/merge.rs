//! Merge, redirect and split of entities (10 §3.6). Every function runs
//! inside the caller's transaction and locks the entity rows it touches in
//! key order, so two concurrent merges cannot deadlock.
//!
//! * merge `A` into `B` (same type, both active): `A.status = 'merged'`,
//!   `entity_redirect(A -> B)`, facts and aliases move to `B` (the originals
//!   are kept in `original_*`), `B` takes the union of the identities. A
//!   redirect that pointed at `A` is rewritten to `B`, so the chain depth is
//!   at most one (DOC-G5 checks it). `A`'s canonical name becomes an alias of
//!   `B`, so the old name still finds the entity.
//! * a contradicting identity (same kind, other value) refuses the merge
//!   ([`MergeError::IdentityConflict`]); the caller queues an `entity_merge`
//!   review item.
//! * split undoes a merge from `original_*`, drops the redirect row and
//!   writes the overlay `entity_split`.

use std::collections::BTreeMap;
use std::fmt;

use sqlx::PgConnection;

use crate::store::StoreError;

#[derive(Debug)]
pub enum MergeError {
    NotFound(String),
    SameEntity,
    TypeMismatch,
    NotActive(String),
    /// Same identity kind, different values: `(kind, a, b)`.
    IdentityConflict(String, String, String),
    NotMerged,
    Store(StoreError),
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(k) => write!(f, "entity {k} not found"),
            Self::SameEntity => f.write_str("an entity cannot merge into itself"),
            Self::TypeMismatch => f.write_str("entities of different types cannot merge"),
            Self::NotActive(k) => write!(f, "entity {k} is not active"),
            Self::IdentityConflict(k, a, b) => {
                write!(f, "identity {k} differs: {a} vs {b}")
            }
            Self::NotMerged => f.write_str("entity is not merged"),
            Self::Store(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for MergeError {}

impl From<sqlx::Error> for MergeError {
    fn from(e: sqlx::Error) -> Self {
        Self::Store(e.into())
    }
}

impl From<StoreError> for MergeError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

/// What a merge changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeReport {
    pub from_key: String,
    pub into_key: String,
    pub facts_moved: u64,
    pub aliases_moved: u64,
    pub redirects_rewritten: u64,
}

struct Row {
    id: String,
    key: String,
    ty: String,
    status: String,
    canonical_name: String,
    norm_name: String,
    identity: BTreeMap<String, String>,
}

async fn lock_rows(conn: &mut PgConnection, keys: &[&str]) -> Result<Vec<Row>, MergeError> {
    let keys: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
    let rows: Vec<(String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id::text, key, type, status, canonical_name, norm_name, identity::text \
               FROM _oo_docs.entity WHERE key = ANY($1::text[]) ORDER BY key FOR UPDATE",
    )
    .bind(&keys)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(
            |(id, key, ty, status, canonical_name, norm_name, identity)| {
                let identity = serde_json::from_str(&identity)
                    .map_err(|e| StoreError::Protocol(format!("entity identity: {e}")))?;
                Ok(Row {
                    id,
                    key,
                    ty,
                    status,
                    canonical_name,
                    norm_name,
                    identity,
                })
            },
        )
        .collect()
}

/// Union of two identities, or the first kind that contradicts.
pub fn union_identity(
    a: &BTreeMap<String, String>,
    b: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, (String, String, String)> {
    let mut out = a.clone();
    for (k, v) in b {
        match out.get(k) {
            Some(have) if have != v => return Err((k.clone(), have.clone(), v.clone())),
            Some(_) => {}
            None => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(out)
}

/// Merge `from_key` into `into_key`. `into_key` may itself have been merged
/// away; the merge then goes to the entity it was merged into. `reason` is
/// `merge`, `identity` or `human`.
pub async fn merge_entities(
    conn: &mut PgConnection,
    from_key: &str,
    into_key: &str,
    reason: &str,
    review_id: Option<&str>,
) -> Result<MergeReport, MergeError> {
    if from_key == into_key {
        return Err(MergeError::SameEntity);
    }
    // Follow a redirect on the target first (depth is at most one).
    let target: Option<String> =
        sqlx::query_scalar("SELECT to_key FROM _oo_docs.entity_redirect WHERE from_key = $1")
            .bind(into_key)
            .fetch_optional(&mut *conn)
            .await?;
    let into_key = target.as_deref().unwrap_or(into_key);
    if from_key == into_key {
        return Err(MergeError::SameEntity);
    }
    let rows = lock_rows(conn, &[from_key, into_key]).await?;
    let from = rows
        .iter()
        .find(|r| r.key == from_key)
        .ok_or_else(|| MergeError::NotFound(from_key.to_owned()))?;
    let into = rows
        .iter()
        .find(|r| r.key == into_key)
        .ok_or_else(|| MergeError::NotFound(into_key.to_owned()))?;
    if from.ty != into.ty {
        return Err(MergeError::TypeMismatch);
    }
    for r in [from, into] {
        if r.status != "active" {
            return Err(MergeError::NotActive(r.key.clone()));
        }
    }
    let union = union_identity(&into.identity, &from.identity)
        .map_err(|(k, a, b)| MergeError::IdentityConflict(k, a, b))?;

    sqlx::query(
        "UPDATE _oo_docs.entity SET status = 'merged', merged_into = $2::uuid, \
                revision = revision + 1 WHERE id = $1::uuid",
    )
    .bind(&from.id)
    .bind(&into.id)
    .execute(&mut *conn)
    .await?;
    if union != into.identity {
        sqlx::query(
            "UPDATE _oo_docs.entity SET identity = $2::jsonb, revision = revision + 1 \
              WHERE id = $1::uuid",
        )
        .bind(&into.id)
        .bind(serde_json::to_string(&union).unwrap_or_else(|_| "{}".into()))
        .execute(&mut *conn)
        .await?;
    }
    let subj =
        sqlx::query("UPDATE _oo_docs.fact SET subject_id = $2::uuid WHERE subject_id = $1::uuid")
            .bind(&from.id)
            .bind(&into.id)
            .execute(&mut *conn)
            .await?;
    let obj =
        sqlx::query("UPDATE _oo_docs.fact SET object_id = $2::uuid WHERE object_id = $1::uuid")
            .bind(&from.id)
            .bind(&into.id)
            .execute(&mut *conn)
            .await?;
    let aliases = sqlx::query(
        "UPDATE _oo_docs.alias SET entity_id = $2::uuid, \
                original_entity_id = COALESCE(original_entity_id, entity_id) \
          WHERE entity_id = $1::uuid \
            AND norm_alias NOT IN (SELECT norm_alias FROM _oo_docs.alias WHERE entity_id = $2::uuid)",
    )
    .bind(&from.id)
    .bind(&into.id)
    .execute(&mut *conn)
    .await?;
    // The old canonical name keeps finding the entity (as a published-grade
    // alias when it was a name of a live entity).
    if from.norm_name != into.norm_name {
        sqlx::query(
            "INSERT INTO _oo_docs.alias \
               (id, entity_id, alias, norm_alias, origin, confidence_bp, status, original_entity_id) \
             VALUES (gen_random_uuid(), $1::uuid, $2, $3, $4, 9000, 'draft', $5::uuid) \
             ON CONFLICT (entity_id, norm_alias) DO NOTHING",
        )
        .bind(&into.id)
        .bind(&from.canonical_name)
        .bind(&from.norm_name)
        .bind(if reason == "human" { "human" } else { "rule" })
        .bind(&from.id)
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query(
        "INSERT INTO _oo_docs.entity_redirect (from_key, to_key, reason, review_id, via_key) \
         VALUES ($1, $2, $3, $4::uuid, $2) \
         ON CONFLICT (from_key) DO UPDATE SET to_key = EXCLUDED.to_key, via_key = EXCLUDED.via_key",
    )
    .bind(from_key)
    .bind(into_key)
    .bind(reason)
    .bind(review_id)
    .execute(&mut *conn)
    .await
    .map_err(|e| MergeError::Store(e.into()))?;
    // Keep the depth at one: whoever pointed at `from` now points at `into`.
    let flat = sqlx::query(
        "UPDATE _oo_docs.entity_redirect SET to_key = $2 WHERE to_key = $1 AND from_key <> $1",
    )
    .bind(from_key)
    .bind(into_key)
    .execute(&mut *conn)
    .await?;
    Ok(MergeReport {
        from_key: from_key.to_owned(),
        into_key: into_key.to_owned(),
        facts_moved: subj.rows_affected() + obj.rows_affected(),
        aliases_moved: aliases.rows_affected(),
        redirects_rewritten: flat.rows_affected(),
    })
}

/// Undo the merge of `from_key` (a merged entity): facts and aliases return
/// to it by their `original_*` columns, the redirect row is deleted and the
/// overlay `entity_split` records who did it.
pub async fn split_merged(
    conn: &mut PgConnection,
    from_key: &str,
    author: &str,
) -> Result<(), MergeError> {
    let target: Option<String> =
        sqlx::query_scalar("SELECT to_key FROM _oo_docs.entity_redirect WHERE from_key = $1")
            .bind(from_key)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(into_key) = target else {
        return Err(MergeError::NotMerged);
    };
    let rows = lock_rows(conn, &[from_key, &into_key]).await?;
    let from = rows
        .iter()
        .find(|r| r.key == from_key)
        .ok_or_else(|| MergeError::NotFound(from_key.to_owned()))?;
    if from.status != "merged" {
        return Err(MergeError::NotMerged);
    }
    // Everything merged into `from` (directly or through others) goes back
    // to it: the entity itself plus its descendants in the merge tree.
    const TREE: &str = "WITH RECURSIVE d(id) AS ( \
           SELECT $1::uuid UNION ALL \
           SELECT e.id FROM _oo_docs.entity e JOIN d ON e.merged_into = d.id)";
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{TREE} UPDATE _oo_docs.alias SET status = 'rejected' \
          WHERE original_entity_id = $1::uuid AND norm_alias = $2 AND entity_id <> $1::uuid"
    )))
    .bind(&from.id)
    .bind(&from.norm_name)
    .execute(&mut *conn)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{TREE} UPDATE _oo_docs.alias a SET entity_id = $1::uuid \
          WHERE a.original_entity_id IN (SELECT id FROM d) AND a.entity_id <> $1::uuid \
            AND a.status <> 'rejected' \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.alias o \
                             WHERE o.entity_id = $1::uuid AND o.norm_alias = a.norm_alias)"
    )))
    .bind(&from.id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{TREE} UPDATE _oo_docs.fact SET subject_id = $1::uuid \
          WHERE original_subject_id IN (SELECT id FROM d) AND subject_id <> $1::uuid"
    )))
    .bind(&from.id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{TREE} UPDATE _oo_docs.fact SET object_id = $1::uuid \
          WHERE original_object_id IN (SELECT id FROM d) AND object_id <> $1::uuid"
    )))
    .bind(&from.id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE _oo_docs.entity SET status = 'active', merged_into = NULL, \
                revision = revision + 1 WHERE id = $1::uuid",
    )
    .bind(&from.id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM _oo_docs.entity_redirect WHERE from_key = $1")
        .bind(from_key)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE _oo_docs.entity_redirect SET to_key = $1 WHERE via_key = $1 AND to_key <> $1",
    )
    .bind(from_key)
    .execute(&mut *conn)
    .await?;
    let payload = serde_json::json!({"from": from_key, "was_merged_into": into_key});
    sqlx::query(
        "INSERT INTO _oo_docs.overlay (id, kind, binds, payload, author) \
         VALUES (gen_random_uuid(), 'entity_split', $1, $2::jsonb, $3)",
    )
    .bind(from_key)
    .bind(payload.to_string())
    .bind(author)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn identity_union_and_conflict() {
        let a = id(&[("email", "a@x.io")]);
        let b = id(&[("employee_id", "E1")]);
        assert_eq!(
            union_identity(&a, &b).unwrap(),
            id(&[("email", "a@x.io"), ("employee_id", "E1")])
        );
        let c = id(&[("email", "c@x.io")]);
        assert_eq!(
            union_identity(&a, &c).unwrap_err(),
            ("email".into(), "a@x.io".into(), "c@x.io".into())
        );
        assert_eq!(union_identity(&a, &a).unwrap(), a);
    }
}
