//! Integrity constraints (10 §16.7, D57): the four core constraints of
//! `schema/constraints.v1.yaml`, checked when facts are about to publish.
//!
//! A violation keeps the facts involved out of the version (the older value
//! stays, or nothing), writes a `constraint_violation` row, queues a
//! `constraint_conflict` review item with both sides and fires
//! `ontology.constraint_violated`; every other entity publishes as usual
//! (partial blocking). A rule-certain supersession never violates anything:
//! the old value's `valid_to` is set to the new value's `valid_from` first, so
//! the intervals do not overlap.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use crate::store::{StoreError, events};
use crate::temporal::{Held, ReviewReason};

/// `schema/constraints.v1.yaml`, compiled in.
pub const SCHEMA_YAML: &str = include_str!("../../schema/constraints.v1.yaml");

/// One core constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Constraint {
    pub id: &'static str,
    pub core: bool,
    pub description: &'static str,
}

pub const PROJECT_SINGLE_OWNER: &str = "project_single_owner";
pub const DECISION_SINGLE_STATUS: &str = "decision_single_status";
pub const IDENTITY_UNIQUE_PERSON: &str = "identity_unique_person";
pub const REJECTED_NOT_ADOPTED: &str = "rejected_not_adopted";

/// The core constraints.
pub const CORE: [Constraint; 4] = [
    Constraint {
        id: PROJECT_SINGLE_OWNER,
        core: true,
        description: "A Project has at most one owner at any time.",
    },
    Constraint {
        id: DECISION_SINGLE_STATUS,
        core: true,
        description: "A Decision has at most one status at any time.",
    },
    Constraint {
        id: IDENTITY_UNIQUE_PERSON,
        core: true,
        description: "An identity attribute belongs to at most one Person.",
    },
    Constraint {
        id: REJECTED_NOT_ADOPTED,
        core: true,
        description: "An alternative is not rejected and decided at the same time.",
    },
];

/// Ids named in the YAML file (`  - id: <id>` lines).
pub fn schema_ids() -> Vec<String> {
    SCHEMA_YAML
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- id:"))
        .map(|s| s.trim().to_owned())
        .collect()
}

/// Which constraint two conflicting facts violate, from their shape.
pub fn constraint_of(
    predicate: &str,
    subject_type: &str,
    object_type: Option<&str>,
) -> Option<&'static str> {
    match (predicate, subject_type, object_type) {
        ("owns", _, Some("Project")) => Some(PROJECT_SINGLE_OWNER),
        ("attr.status", "Decision", _) => Some(DECISION_SINGLE_STATUS),
        _ => None,
    }
}

struct FactInfo {
    id: String,
    predicate: String,
    subject_key: String,
    subject_type: String,
    object_key: Option<String>,
    object_type: Option<String>,
}

async fn info(conn: &mut PgConnection, id: &str) -> Result<Option<FactInfo>, StoreError> {
    let row = sqlx::query(
        "SELECT f.id::text AS id, f.predicate, s.key AS sk, s.type AS st, o.key AS ok, o.type AS ot \
           FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
           LEFT JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.id = $1::uuid",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(r) = row else { return Ok(None) };
    Ok(Some(FactInfo {
        id: r.try_get("id")?,
        predicate: r.try_get("predicate")?,
        subject_key: r.try_get("sk")?,
        subject_type: r.try_get("st")?,
        object_key: r.try_get("ok")?,
        object_type: r.try_get("ot")?,
    }))
}

/// Record a violation: the row, the review item and the event. Returns true
/// when it is new (the same constraint and entity is only recorded once while
/// it is open).
pub async fn record_violation(
    conn: &mut PgConnection,
    constraint_id: &str,
    entity_key: &str,
    fact_ids: &[String],
    details: Value,
) -> Result<bool, StoreError> {
    let dedupe = format!("{constraint_id}:{entity_key}");
    let done = sqlx::query(
        "INSERT INTO _oo_docs.constraint_violation (id, constraint_id, entity_key, dedupe_key, fact_ids, details) \
         VALUES (gen_random_uuid(), $1, $2, $3, $4::text[]::uuid[], $5::jsonb) \
         ON CONFLICT (dedupe_key) DO UPDATE SET status = 'open', resolved_at = NULL, \
                fact_ids = EXCLUDED.fact_ids, details = EXCLUDED.details \
           WHERE _oo_docs.constraint_violation.status = 'resolved'",
    )
    .bind(constraint_id)
    .bind(entity_key)
    .bind(&dedupe)
    .bind(fact_ids)
    .bind(details.to_string())
    .execute(&mut *conn)
    .await?;
    if done.rows_affected() == 0 {
        return Ok(false);
    }
    let review: Option<String> = sqlx::query_scalar(
        "INSERT INTO _oo_docs.review_item (id, kind, dedupe_key, drive_id, source_file_id, \
                source_version_id, subject, reason_code) \
         SELECT gen_random_uuid(), 'constraint_conflict', $1, src.drive_id, src.file_id, src.version_id, \
                $2::jsonb, $3 \
           FROM (SELECT 1) one \
           LEFT JOIN LATERAL (SELECT fl.drive_id, fl.id AS file_id, fv.id AS version_id \
                 FROM _oo_docs.evidence e JOIN _oo_docs.file_version fv ON fv.id = e.version_id \
                 JOIN _oo_docs.file fl ON fl.id = fv.file_id \
                WHERE e.target_kind = 'fact' AND e.target_id = ANY($4::text[]::uuid[]) \
                  AND e.withdrawn_at IS NULL ORDER BY e.created_at, e.id LIMIT 1) src ON true \
         ON CONFLICT (dedupe_key) DO UPDATE SET status = 'pending', revision = _oo_docs.review_item.revision + 1 \
           WHERE _oo_docs.review_item.status <> 'pending' \
         RETURNING id::text",
    )
    .bind(format!("constraint_conflict:{dedupe}"))
    .bind(
        json!({"constraint": constraint_id, "entity_key": entity_key, "fact_ids": fact_ids,
               "details": details})
        .to_string(),
    )
    .bind(format!("constraint:{constraint_id}"))
    .bind(fact_ids)
    .fetch_optional(&mut *conn)
    .await?;
    events::append_event(
        conn,
        "ontology.constraint_violated",
        None,
        review.as_deref(),
        &json!({"constraint_id": constraint_id, "entity_key": entity_key, "fact_ids": fact_ids,
                "review_id": review})
        .to_string(),
    )
    .await?;
    Ok(true)
}

/// Queue what waits: a functional conflict on a constrained shape is a
/// constraint violation, anything else a `low_confidence_fact` item.
pub async fn route_held(conn: &mut PgConnection, held: &[Held]) -> Result<u32, StoreError> {
    let mut plain: Vec<Held> = Vec::new();
    let mut n = 0u32;
    for h in held {
        if h.reason != ReviewReason::FunctionalConflict {
            plain.push(h.clone());
            continue;
        }
        let (Some(a), Some(b)) = (
            info(conn, &h.fact_id).await?,
            info(conn, &h.other_id).await?,
        ) else {
            plain.push(h.clone());
            continue;
        };
        match constraint_of(&a.predicate, &a.subject_type, a.object_type.as_deref()) {
            Some(c) => {
                let entity = if c == PROJECT_SINGLE_OWNER {
                    a.object_key.clone().unwrap_or_default()
                } else {
                    a.subject_key.clone()
                };
                let ids = [b.id.clone(), a.id.clone()];
                if record_violation(
                    conn,
                    c,
                    &entity,
                    &ids,
                    json!({"kept": b.id, "blocked": a.id}),
                )
                .await?
                {
                    n += 1;
                }
            }
            None => plain.push(h.clone()),
        }
    }
    n += crate::temporal::pg::queue_held(conn, &plain).await?;
    Ok(n)
}

/// Incoming facts that would make an alternative both rejected and decided:
/// `(incoming fact, other fact, alternative key)`.
pub async fn rejected_not_adopted(
    conn: &mut PgConnection,
    incoming: &[String],
) -> Result<Vec<(String, String, String)>, StoreError> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT a.id::text, b.id::text, x.key \
           FROM _oo_docs.fact a \
           JOIN _oo_docs.fact b ON b.object_id = a.object_id AND b.id <> a.id \
                AND b.status = 'published' AND b.retracted_at IS NULL \
                AND ((a.predicate = 'rejects' AND b.predicate = 'decides') \
                  OR (a.predicate = 'decides' AND b.predicate = 'rejects')) \
                AND (a.valid_to IS NULL OR b.valid_from IS NULL OR b.valid_from < a.valid_to) \
                AND (b.valid_to IS NULL OR a.valid_from IS NULL OR a.valid_from < b.valid_to) \
           JOIN _oo_docs.entity x ON x.id = a.object_id \
          WHERE a.id = ANY($1::text[]::uuid[]) AND a.predicate IN ('rejects', 'decides') \
          ORDER BY 1, 2",
    )
    .bind(incoming)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows)
}

/// Persons that share an identity attribute and could not be merged
/// (`identity_unique_person`): `(entity key a, entity key b, kind, value)`.
pub async fn duplicate_identities(
    conn: &mut PgConnection,
) -> Result<Vec<(String, String, String, String)>, StoreError> {
    let rows = sqlx::query_as(
        "SELECT a.key, b.key, ja.key, ja.value \
           FROM _oo_docs.entity a \
           JOIN _oo_docs.entity b ON b.type = 'Person' AND b.status = 'active' AND b.key > a.key \
           JOIN LATERAL jsonb_each_text(a.identity) ja ON true \
           JOIN LATERAL jsonb_each_text(b.identity) jb ON jb.key = ja.key AND jb.value = ja.value \
          WHERE a.type = 'Person' AND a.status = 'active' ORDER BY 1, 2, 3",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows)
}

/// Record the three checks that read the whole store or the incoming set;
/// returns the incoming fact ids that must not publish.
pub async fn check_publish(
    conn: &mut PgConnection,
    incoming: &[String],
) -> Result<BTreeSet<String>, StoreError> {
    let mut blocked = BTreeSet::new();
    for (inc, other, key) in rejected_not_adopted(conn, incoming).await? {
        let ids = [other.clone(), inc.clone()];
        record_violation(
            conn,
            REJECTED_NOT_ADOPTED,
            &key,
            &ids,
            json!({"kept": other, "blocked": inc}),
        )
        .await?;
        blocked.insert(inc);
    }
    for (a, b, kind, value) in duplicate_identities(conn).await? {
        record_violation(
            conn,
            IDENTITY_UNIQUE_PERSON,
            &a,
            &[],
            json!({"entities": [a, b], "identity": {kind: value}}),
        )
        .await?;
    }
    Ok(blocked)
}

/// Mark the open violation of a review item resolved.
pub async fn resolve_violation(conn: &mut PgConnection, review_id: &str) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE _oo_docs.constraint_violation SET status = 'resolved', resolved_at = clock_timestamp() \
          WHERE dedupe_key = (SELECT substr(dedupe_key, length('constraint_conflict:') + 1) \
                                FROM _oo_docs.review_item WHERE id = $1::uuid)",
    )
    .bind(review_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_and_code_list_the_same_core_constraints() {
        let mut yaml = schema_ids();
        let mut code: Vec<String> = CORE.iter().map(|c| c.id.to_owned()).collect();
        yaml.sort();
        code.sort();
        assert_eq!(yaml, code);
        assert!(CORE.iter().all(|c| c.core));
    }

    #[test]
    fn shapes_map_to_constraints() {
        assert_eq!(
            constraint_of("owns", "Person", Some("Project")),
            Some(PROJECT_SINGLE_OWNER)
        );
        assert_eq!(constraint_of("owns", "Person", Some("Document")), None);
        assert_eq!(
            constraint_of("attr.status", "Decision", None),
            Some(DECISION_SINGLE_STATUS)
        );
        assert_eq!(constraint_of("attr.status", "Project", None), None);
        assert_eq!(constraint_of("attr.role", "Person", None), None);
    }
}
