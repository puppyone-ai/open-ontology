//! Which draft facts publish, wait or stay drafts (10 §4.8, §5.3, D24).
//!
//! [`classify_drafts`] reads every draft fact with the signals of 5.3 and
//! returns its [`Class`]; [`enqueue`] writes the queue items (deduplicated by
//! `low_confidence_fact:<fact_key>`) and retires pending items that no longer
//! apply (the fact was published, retracted, rejected or now scores above the
//! threshold); the publisher takes [`Class::Eligible`] facts.
//!
//! * a person's overlay `fact_assert` makes a fact eligible, `fact_reject` or
//!   `fact_edit` keeps the key out for good (DOC-G4);
//! * a `supersedes` that is not rule-certain (LLM, suggestion, `name.version`)
//!   is always queued, whatever its score (5.3, D18);
//! * `human` facts are eligible.

use sqlx::{PgConnection, Row};

use crate::extract::confidence::{
    AUTO_PUBLISH_BP, Disposition, QUEUE_MIN_BP, Signals, disposition, effective_bp,
};
use crate::store::StoreError;
use crate::temporal::{Origin, certain_supersedes};

/// Thresholds of a run.
#[derive(Clone, Copy, Debug)]
pub struct Thresholds {
    pub auto_publish_bp: i32,
    pub queue_min_bp: i32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            auto_publish_bp: AUTO_PUBLISH_BP,
            queue_min_bp: QUEUE_MIN_BP,
        }
    }
}

/// What happens to a draft fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Publishes without a person.
    Eligible,
    /// Waits in the review queue, with the reason code.
    Queue(&'static str),
    /// Stays a draft, not queued.
    Hold,
    /// A person rejected or replaced this content.
    Rejected,
}

/// One classified draft fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classified {
    pub fact_id: String,
    pub fact_key: String,
    pub effective_bp: i32,
    pub class: Class,
}

/// Pure decision for one fact.
#[allow(clippy::too_many_arguments)]
pub fn classify(
    origin: Origin,
    rule_id: Option<&str>,
    predicate: &str,
    base_bp: i32,
    signals: Signals,
    asserted: bool,
    rejected: bool,
    th: Thresholds,
) -> (i32, Class) {
    let bp = effective_bp(base_bp, signals);
    if rejected {
        return (bp, Class::Rejected);
    }
    if asserted || origin == Origin::Human {
        return (bp, Class::Eligible);
    }
    if predicate == "supersedes" && !certain_supersedes(rule_id, origin) {
        return (bp, Class::Queue("supersede_candidate"));
    }
    let class = match disposition(bp, th.auto_publish_bp, th.queue_min_bp) {
        Disposition::Auto => Class::Eligible,
        Disposition::Queue => Class::Queue("low_confidence"),
        Disposition::Hold => Class::Hold,
    };
    (bp, class)
}

/// Classify every draft fact.
pub async fn classify_drafts(
    conn: &mut PgConnection,
    th: Thresholds,
) -> Result<Vec<Classified>, StoreError> {
    let once = |col: &str| {
        format!(
            "((SELECT count(*) FROM _oo_docs.evidence e WHERE e.target_kind = 'entity' \
                 AND e.target_id = {col} AND e.withdrawn_at IS NULL) <= 1 \
              AND NOT EXISTS (SELECT 1 FROM _oo_docs.fact x WHERE x.id <> f.id \
                 AND x.status IN ('draft', 'published') AND {col} IN (x.subject_id, x.object_id)))"
        )
    };
    let (once_s, once_o) = (once("f.subject_id"), once("f.object_id"));
    let sql = format!(
        "SELECT f.id::text AS id, f.fact_key, f.predicate, f.origin, f.rule_id, f.confidence_bp, \
                COALESCE(o.type = 'Thing', false) AS object_thing, \
                GREATEST((SELECT count(DISTINCT e.version_id) FROM _oo_docs.evidence e \
                           WHERE e.target_kind = 'fact' AND e.target_id = f.id \
                             AND e.withdrawn_at IS NULL) - 1, 0) AS extra_versions, \
                EXISTS (SELECT 1 FROM _oo_docs.fact r WHERE r.fact_key = f.fact_key \
                         AND r.origin = 'rule' AND r.id <> f.id \
                         AND r.status IN ('draft', 'published')) AS by_rule, \
                ({once_s} \
                 OR (f.object_id IS NOT NULL AND {once_o})) AS single_entity, \
                EXISTS (SELECT 1 FROM _oo_docs.overlay_current v WHERE v.binds = f.fact_key \
                         AND v.kind = 'fact_assert' AND v.status = 'active') AS asserted, \
                EXISTS (SELECT 1 FROM _oo_docs.overlay_current v WHERE v.binds = f.fact_key \
                         AND v.kind IN ('fact_reject', 'fact_edit') AND v.status = 'active') AS rejected \
           FROM _oo_docs.fact f LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
          WHERE f.status = 'draft' ORDER BY f.fact_key, f.id"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let origin_text: String = r.try_get("origin")?;
        let origin = Origin::parse(&origin_text)
            .ok_or_else(|| StoreError::Protocol(format!("unknown origin {origin_text}")))?;
        let extra: i64 = r.try_get("extra_versions")?;
        let signals = Signals {
            confirmed_by_rule: origin != Origin::Rule && r.try_get::<bool, _>("by_rule")?,
            extra_versions: u32::try_from(extra).unwrap_or(0),
            new_single_entity: r.try_get("single_entity")?,
            object_is_thing: r.try_get("object_thing")?,
        };
        let predicate: String = r.try_get("predicate")?;
        let rule_id: Option<String> = r.try_get("rule_id")?;
        let (bp, class) = classify(
            origin,
            rule_id.as_deref(),
            &predicate,
            r.try_get("confidence_bp")?,
            signals,
            r.try_get("asserted")?,
            r.try_get("rejected")?,
            th,
        );
        out.push(Classified {
            fact_id: r.try_get("id")?,
            fact_key: r.try_get("fact_key")?,
            effective_bp: bp,
            class,
        });
    }
    Ok(out)
}

/// What [`enqueue`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnqueueReport {
    pub queued: u32,
    pub retired: u32,
    pub rejected: u32,
    /// Every draft fact with its class, for the publisher.
    pub classified: Vec<Classified>,
}

impl EnqueueReport {
    /// Ids of the facts that publish by themselves.
    pub fn eligible(&self) -> Vec<String> {
        self.classified
            .iter()
            .filter(|c| c.class == Class::Eligible)
            .map(|c| c.fact_id.clone())
            .collect()
    }
}

/// Queue the facts that wait, retire items that no longer apply and mark the
/// facts a person rejected.
pub async fn enqueue(conn: &mut PgConnection, th: Thresholds) -> Result<EnqueueReport, StoreError> {
    let classified = classify_drafts(conn, th).await?;
    let mut queued = 0u32;
    for c in &classified {
        let Class::Queue(reason) = c.class else {
            continue;
        };
        let done = sqlx::query(
            "INSERT INTO _oo_docs.review_item \
               (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, \
                confidence_bp, reason_code) \
             SELECT gen_random_uuid(), 'low_confidence_fact', $1, src.drive_id, src.file_id, \
                    src.version_id, jsonb_build_object('fact_id', f.id::text), $3, $4 \
               FROM _oo_docs.fact f \
               LEFT JOIN LATERAL ( \
                    SELECT fl.drive_id, fl.id AS file_id, fv.id AS version_id \
                      FROM _oo_docs.evidence e \
                      JOIN _oo_docs.file_version fv ON fv.id = e.version_id \
                      JOIN _oo_docs.file fl ON fl.id = fv.file_id \
                     WHERE e.target_kind = 'fact' AND e.target_id = f.id AND e.withdrawn_at IS NULL \
                     ORDER BY e.created_at, e.id LIMIT 1) src ON true \
              WHERE f.id = $2::uuid \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(format!("low_confidence_fact:{}", c.fact_key))
        .bind(&c.fact_id)
        .bind(c.effective_bp)
        .bind(reason)
        .execute(&mut *conn)
        .await?;
        queued += u32::try_from(done.rows_affected()).unwrap_or(0);
    }
    // Facts a person rejected or replaced do not stay drafts.
    let rejected_ids: Vec<String> = classified
        .iter()
        .filter(|c| c.class == Class::Rejected)
        .map(|c| c.fact_id.clone())
        .collect();
    let rejected = sqlx::query(
        "UPDATE _oo_docs.fact SET status = 'rejected' WHERE id = ANY($1::text[]::uuid[]) AND status = 'draft'",
    )
    .bind(&rejected_ids)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    // Pending fact items whose fact is no longer waiting (4.8).
    let waiting: Vec<String> = classified
        .iter()
        .filter(|c| matches!(c.class, Class::Queue(_)))
        .map(|c| c.fact_id.clone())
        .collect();
    let retired = sqlx::query(
        "UPDATE _oo_docs.review_item r SET status = 'superseded', decided_at = clock_timestamp(), \
                decided_by = 'engine', revision = revision + 1 \
          WHERE r.kind = 'low_confidence_fact' AND r.status = 'pending' \
            AND r.reason_code IN ('low_confidence', 'supersede_candidate') \
            AND r.subject ? 'fact_id' \
            AND NOT ((r.subject->>'fact_id') = ANY($1::text[])) \
            AND (r.reason_code <> 'supersede_candidate' \
                 OR NOT EXISTS (SELECT 1 FROM _oo_docs.fact held \
                                 WHERE held.id = (r.subject->>'fact_id')::uuid AND held.status = 'draft'))",
    )
    .bind(&waiting)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    refresh_pending_counts(conn).await?;
    Ok(EnqueueReport {
        queued,
        retired: u32::try_from(retired).unwrap_or(u32::MAX),
        rejected: u32::try_from(rejected).unwrap_or(u32::MAX),
        classified,
    })
}

/// `file.pending_review` = pending items of the file (what the tree shows).
pub async fn refresh_pending_counts(conn: &mut PgConnection) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE _oo_docs.file f SET pending_review = c.n \
           FROM (SELECT f2.id, \
                        (SELECT count(*) FROM _oo_docs.review_item r \
                          WHERE r.source_file_id = f2.id AND r.status = 'pending')::int AS n \
                   FROM _oo_docs.file f2) c \
          WHERE f.id = c.id AND f.pending_review <> c.n",
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        origin: Origin,
        rule: Option<&str>,
        pred: &str,
        base: i32,
        asserted: bool,
        rejected: bool,
    ) -> (i32, Class) {
        classify(
            origin,
            rule,
            pred,
            base,
            Signals::default(),
            asserted,
            rejected,
            Thresholds::default(),
        )
    }

    #[test]
    fn thresholds_split_auto_queue_hold() {
        assert_eq!(
            run(Origin::Rule, Some("md.kv"), "owns", 9000, false, false).1,
            Class::Eligible
        );
        assert_eq!(
            run(Origin::Llm, None, "owns", 8500, false, false).1,
            Class::Eligible
        );
        assert_eq!(
            run(Origin::Llm, None, "owns", 7000, false, false).1,
            Class::Queue("low_confidence")
        );
        assert_eq!(
            run(Origin::Llm, None, "owns", 3999, false, false).1,
            Class::Hold
        );
        // A rule below the threshold (pptx.title, 7000) is queued like the rest.
        assert_eq!(
            run(
                Origin::Rule,
                Some("pptx.title"),
                "about",
                7000,
                false,
                false
            )
            .1,
            Class::Queue("low_confidence")
        );
    }

    #[test]
    fn people_and_overlays_override_the_score() {
        assert_eq!(
            run(Origin::Human, None, "owns", 100, false, false).1,
            Class::Eligible
        );
        assert_eq!(
            run(Origin::Llm, None, "owns", 5000, true, false).1,
            Class::Eligible
        );
        assert_eq!(
            run(Origin::Rule, Some("md.kv"), "owns", 9800, true, true).1,
            Class::Rejected
        );
        assert_eq!(
            run(Origin::Llm, None, "owns", 9000, false, true).1,
            Class::Rejected
        );
    }

    #[test]
    fn uncertain_supersedes_is_always_queued() {
        for (o, r) in [
            (Origin::Llm, None),
            (Origin::Suggest, None),
            (Origin::Rule, Some("name.version")),
        ] {
            assert_eq!(
                run(o, r, "supersedes", 9900, false, false).1,
                Class::Queue("supersede_candidate")
            );
        }
        assert_eq!(
            run(
                Origin::Rule,
                Some("fm.fields"),
                "supersedes",
                9800,
                false,
                false
            )
            .1,
            Class::Eligible
        );
        // A person's decision still goes through.
        assert_eq!(
            run(Origin::Llm, None, "supersedes", 6000, true, false).1,
            Class::Eligible
        );
    }

    #[test]
    fn adjustments_can_lift_or_sink_a_fact() {
        let (bp, class) = classify(
            Origin::Llm,
            None,
            "owns",
            7000,
            Signals {
                confirmed_by_rule: true,
                ..Signals::default()
            },
            false,
            false,
            Thresholds::default(),
        );
        assert_eq!((bp, class), (8000, Class::Eligible));
        let (bp, class) = classify(
            Origin::Llm,
            None,
            "owns",
            8500,
            Signals {
                new_single_entity: true,
                object_is_thing: true,
                ..Signals::default()
            },
            false,
            false,
            Thresholds::default(),
        );
        assert_eq!((bp, class), (7000, Class::Queue("low_confidence")));
    }
}
