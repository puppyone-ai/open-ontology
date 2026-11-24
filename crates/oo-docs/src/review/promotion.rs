//! Open type promotion (10 4.8, 5.3, D16, board B-S9-02).
//!
//! After each publish the engine counts `Thing` entities by label: a label with at least [`MIN_INSTANCES`]
//! different instances in at least [`MIN_FILES`] files becomes one `type_promotion` review item. Only a person
//! promotes: accepting writes overlay `type_promote{label, type_name}`, which adds the type to the constrained
//! type table (the labels the LLM prompt may use, `extract::llm::ConstrainedType`), changes the prompt version
//! and sends the files that hold instances of the label back through extraction (`outdated`, low priority, the
//! old facts stay valid meanwhile). Rejecting closes the item for good: the dedupe key keeps the label from
//! being proposed again. Nothing is promoted automatically.

use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use super::overlay;
use crate::extract::llm::ConstrainedType;
use crate::state::{self, FileState, Transition, TransitionError};
use crate::store::StoreError;

/// Instances of one label before it is proposed.
pub const MIN_INSTANCES: i64 = 5;
/// Files the instances are spread over before the label is proposed.
pub const MIN_FILES: i64 = 3;
/// `review_item.kind`.
pub const KIND: &str = "type_promotion";
/// `review_item.reason_code`.
pub const REASON: &str = "open_type_candidate";
/// The core types: a promoted type must not reuse their names (D15).
const CORE: [&str; 9] = [
    "Person",
    "Organization",
    "Project",
    "Decision",
    "Document",
    "Topic",
    "Event",
    "Term",
    "Thing",
];

/// One label that reached the thresholds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    pub instances: i64,
    pub files: i64,
    /// Up to five names, sorted.
    pub examples: Vec<String>,
}

/// Counts that hold a label back or let it through (pure, for the tests and the docs).
pub fn qualifies(instances: i64, files: i64) -> bool {
    instances >= MIN_INSTANCES && files >= MIN_FILES
}

/// Valid name of a promoted type: 1 to 20 characters, no core type name.
pub fn valid_type_name(name: &str) -> bool {
    let n = name.trim();
    let c = n.chars().count();
    (1..=20).contains(&c)
        && !CORE.iter().any(|k| k.eq_ignore_ascii_case(n))
        && n.chars().all(|ch| !ch.is_control())
}

/// The constrained type table: every active `type_promote` overlay.
pub async fn constrained_types(
    conn: &mut PgConnection,
) -> Result<Vec<ConstrainedType>, StoreError> {
    let rows = sqlx::query(
        "SELECT payload->>'label' AS label, payload->>'type_name' AS type_name \
           FROM _oo_docs.overlay_current WHERE kind = 'type_promote' AND status = 'active' \
          ORDER BY binds",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        if let (Some(label), Some(type_name)) = (
            r.try_get::<Option<String>, _>("label")?,
            r.try_get::<Option<String>, _>("type_name")?,
        ) {
            out.push(ConstrainedType { label, type_name });
        }
    }
    Ok(out)
}

/// Labels that reached the thresholds and are not promoted yet.
pub async fn candidates(conn: &mut PgConnection) -> Result<Vec<Candidate>, StoreError> {
    let rows = sqlx::query(
        "SELECT e.thing_label AS label, count(DISTINCT e.id) AS instances, \
                count(DISTINCT fv.file_id) AS files, \
                (array_agg(DISTINCT e.canonical_name ORDER BY e.canonical_name))[1:5] AS examples \
           FROM _oo_docs.entity e \
           JOIN _oo_docs.evidence ev ON ev.target_kind = 'entity' AND ev.target_id = e.id \
                                    AND ev.withdrawn_at IS NULL \
           JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
           JOIN _oo_docs.file f ON f.id = fv.file_id AND f.deleted_at IS NULL \
          WHERE e.type = 'Thing' AND e.status = 'active' AND e.thing_label IS NOT NULL \
            AND NOT EXISTS (SELECT 1 FROM _oo_docs.overlay_current v WHERE v.kind = 'type_promote' \
                              AND v.status = 'active' AND v.binds = e.thing_label) \
          GROUP BY e.thing_label \
         HAVING count(DISTINCT e.id) >= $1 AND count(DISTINCT fv.file_id) >= $2 \
          ORDER BY e.thing_label",
    )
    .bind(MIN_INSTANCES)
    .bind(MIN_FILES)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(Candidate {
            label: r.try_get("label")?,
            instances: r.try_get("instances")?,
            files: r.try_get("files")?,
            examples: r.try_get("examples")?,
        });
    }
    Ok(out)
}

/// Queue one `type_promotion` item per candidate (once per label, ever). Returns how many were added.
pub async fn propose(conn: &mut PgConnection) -> Result<u32, StoreError> {
    let mut added = 0;
    for c in candidates(conn).await? {
        // The amounts and dates two documents share are anchors, not a type a person would name.
        if [
            crate::extract::anchors::AMOUNT,
            crate::extract::anchors::DATE,
        ]
        .contains(&c.label.as_str())
            // D89: the labels of the profile extractors are registered, not candidates.
            || crate::profile::is_profile_label(&c.label)
        {
            continue;
        }
        let subject = json!({
            "label": c.label, "instances": c.instances, "files": c.files, "examples": c.examples,
        });
        let done = sqlx::query(
            "INSERT INTO _oo_docs.review_item \
               (id, kind, dedupe_key, drive_id, subject, confidence_bp, reason_code) \
             SELECT gen_random_uuid(), 'type_promotion', $1, \
                    (SELECT id FROM _oo_docs.drive ORDER BY created_at LIMIT 1), $2::jsonb, NULL, $3 \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(format!("type_promotion:{}", c.label))
        .bind(subject.to_string())
        .bind(REASON)
        .execute(&mut *conn)
        .await?;
        added += u32::try_from(done.rows_affected()).unwrap_or(0);
    }
    Ok(added)
}

/// What accepting did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Promoted {
    /// Overlay written; `outdated` files were sent back through extraction.
    Done { outdated: u32 },
    /// The type name is empty, too long or a core type name.
    BadName,
}

/// Accept a candidate as `type_name` (default: the label itself).
pub async fn accept(
    conn: &mut PgConnection,
    item_id: &str,
    item_subject: &Value,
    type_name: Option<&str>,
    by: &str,
) -> Result<Promoted, StoreError> {
    let Some(label) = item_subject.get("label").and_then(Value::as_str) else {
        return Ok(Promoted::BadName);
    };
    let type_name = type_name.unwrap_or(label).trim();
    if !valid_type_name(type_name) {
        return Ok(Promoted::BadName);
    }
    overlay::insert(
        conn,
        "type_promote",
        label,
        json!({"label": label, "type_name": type_name}),
        by,
        Some(item_id),
        "active",
        None,
    )
    .await?;
    let outdated = outdate_files_with(conn, label).await?;
    Ok(Promoted::Done { outdated })
}

/// Files that hold instances of `label` go back through extraction (`outdated`): the prompt version changed.
async fn outdate_files_with(conn: &mut PgConnection, label: &str) -> Result<u32, StoreError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT f.id::text, f.current_version_id::text \
           FROM _oo_docs.entity e \
           JOIN _oo_docs.evidence ev ON ev.target_kind = 'entity' AND ev.target_id = e.id \
                                    AND ev.withdrawn_at IS NULL \
           JOIN _oo_docs.file_version fv ON fv.id = ev.version_id \
           JOIN _oo_docs.file f ON f.id = fv.file_id AND f.current_version_id = fv.id \
          WHERE e.type = 'Thing' AND e.thing_label = $1 AND f.deleted_at IS NULL \
            AND f.state IN ('published', 'extract_partial') \
          ORDER BY 1 LIMIT 500",
    )
    .bind(label)
    .fetch_all(&mut *conn)
    .await?;
    let mut n = 0;
    for (file_id, version_id) in rows {
        match state::transition_in_tx(
            conn,
            &Transition {
                file_id: &file_id,
                expect_from: None,
                to: FileState::Outdated,
                code: None,
                retryable: false,
                version_id: Some(&version_id),
                attempt: None,
                detail_json: Some("{\"reason\":\"prompt_version\"}"),
            },
        )
        .await
        {
            Ok(_) => n += 1,
            Err(
                TransitionError::Conflict { .. }
                | TransitionError::NotFound
                | TransitionError::Illegal { .. },
            ) => {}
            Err(TransitionError::Store(e)) => return Err(e),
            Err(TransitionError::UnknownState(_)) => {}
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_are_five_instances_in_three_files() {
        assert!(qualifies(5, 3));
        assert!(!qualifies(4, 3) && !qualifies(5, 2) && !qualifies(100, 1));
    }

    #[test]
    fn a_promoted_type_may_not_take_a_core_name() {
        assert!(valid_type_name("合同") && valid_type_name("Contract"));
        for bad in [
            "",
            "  ",
            "Person",
            "thing",
            "TERM",
            &"长".repeat(21),
            "a\nb",
        ] {
            assert!(!valid_type_name(bad), "{bad:?}");
        }
    }
}
