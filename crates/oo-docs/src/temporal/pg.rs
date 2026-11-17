//! Database side of supersession (10 §5.5, D51): load the facts a publish run
//! touches, plan, end the superseded facts and queue what needs a person.
//!
//! Run inside the publisher's transaction (single writer), after the eligible
//! drafts are selected and before the version is written.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgConnection;

use super::{
    EndFact, Held, Origin, Plan, ReviewReason, TFact, certain_supersedes, functional_group,
    plan_decision, plan_group,
};
use crate::store::StoreError;

type Row = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i32,
    String,
    Option<String>,
    bool,
);

const SELECT: &str = "SELECT f.id::text, f.fact_key, s.key, f.predicate, o.key, o.type, \
        f.value::text, f.valid_from::text, f.valid_to::text, f.confidence_bp, f.origin, \
        f.rule_id, (f.status = 'published') \
   FROM _oo_docs.fact f \
   JOIN _oo_docs.entity s ON s.id = f.subject_id \
   LEFT JOIN _oo_docs.entity o ON o.id = f.object_id";

fn to_fact(r: Row) -> Result<TFact, StoreError> {
    let value = match r.6 {
        Some(t) => Some(
            serde_json::from_str(&t)
                .map_err(|e| StoreError::Protocol(format!("fact value: {e}")))?,
        ),
        None => None,
    };
    Ok(TFact {
        id: r.0,
        fact_key: r.1,
        subject_key: r.2,
        predicate: r.3,
        object_key: r.4,
        object_type: r.5,
        value,
        valid_from: r.7,
        valid_to: r.8,
        confidence_bp: r.9,
        origin: Origin::parse(&r.10)
            .ok_or_else(|| StoreError::Protocol(format!("unknown origin {}", r.10)))?,
        rule_id: r.11,
        published: r.12,
    })
}

/// What reconcile decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// Facts whose `valid_to` was set.
    pub ended: Vec<EndFact>,
    /// Facts (and pairs) that wait for a person.
    pub held: Vec<Held>,
    /// Incoming facts that must not be published in this run.
    pub excluded: BTreeSet<String>,
}

async fn load(
    conn: &mut PgConnection,
    sql_tail: &str,
    bind: &[&str],
) -> Result<Vec<TFact>, StoreError> {
    let sql = format!("{SELECT} {sql_tail}");
    let mut q = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(sql));
    for b in bind {
        q = q.bind(b.to_owned());
    }
    q.fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(to_fact)
        .collect()
}

/// Which of `fact_ids` a live Share used (D51).
pub async fn used_by_live_share(
    conn: &mut PgConnection,
    fact_ids: &[String],
) -> Result<BTreeSet<String>, StoreError> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT u.fact_id::text FROM _oo_docs.share_fact_use u \
           JOIN _oo_docs.share s ON s.id = u.share_id \
          WHERE s.status IN ('active', 'drifted') AND u.fact_id = ANY($1::text[]::uuid[])",
    )
    .bind(fact_ids)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Plan and apply the functional supersession of the groups that `incoming`
/// (eligible drafts of this run) touch, and the decision supersession of
/// incoming `supersedes` facts. Facts that end are updated in place; facts
/// that need a person are returned in `held` (and `excluded` when they are
/// incoming ones that must not publish yet). Call [`queue_held`] to write the
/// review items.
pub async fn reconcile(
    conn: &mut PgConnection,
    incoming: &[String],
) -> Result<Reconciled, StoreError> {
    let mut out = Reconciled::default();
    let incoming_set: BTreeSet<&str> = incoming.iter().map(String::as_str).collect();

    // ---- functional groups
    let inc_rows: Vec<TFact> = {
        let sql = format!("{SELECT} WHERE f.id = ANY($1::text[]::uuid[])");
        sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(sql))
            .bind(incoming)
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(to_fact)
            .collect::<Result<_, _>>()?
    };
    let mut subjects: BTreeSet<String> = BTreeSet::new();
    let mut objects: BTreeSet<String> = BTreeSet::new();
    for f in &inc_rows {
        if functional_group(f).is_some() {
            if f.predicate == "owns" {
                if let Some(o) = &f.object_key {
                    objects.insert(o.clone());
                }
            } else {
                subjects.insert(f.subject_key.clone());
            }
        }
    }
    if !subjects.is_empty() || !objects.is_empty() {
        let subjects: Vec<String> = subjects.into_iter().collect();
        let objects: Vec<String> = objects.into_iter().collect();
        let sql = format!(
            "{SELECT} WHERE (f.status = 'published' OR f.id = ANY($3::text[]::uuid[])) \
               AND ((f.predicate IN ('attr.status', 'attr.role') AND s.key = ANY($1::text[])) \
                 OR (f.predicate = 'owns' AND o.type = 'Project' AND o.key = ANY($2::text[])))"
        );
        let rows: Vec<TFact> = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(sql))
            .bind(&subjects)
            .bind(&objects)
            .bind(incoming)
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(to_fact)
            .collect::<Result<_, _>>()?;
        let mut groups: BTreeMap<String, Vec<TFact>> = BTreeMap::new();
        for f in rows {
            if let Some(g) = functional_group(&f) {
                groups.entry(g).or_default().push(f);
            }
        }
        for (_, mut group) in groups {
            group.sort_by(|a, b| a.id.cmp(&b.id));
            let plan = plan_group(&group);
            merge_plan(&mut out, plan, &incoming_set);
        }
    }

    // ---- decision supersession and candidates
    for f in inc_rows.iter().filter(|f| f.predicate == "supersedes") {
        let certain = certain_supersedes(f.rule_id.as_deref(), f.origin);
        let Some(old) = f.object_key.as_deref() else {
            continue;
        };
        let decisions = f.object_type.as_deref() == Some("Decision");
        if !certain {
            out.held.push(Held {
                fact_id: f.id.clone(),
                other_id: f.id.clone(),
                reason: ReviewReason::SupersedeCandidate,
            });
            out.excluded.insert(f.id.clone());
            continue;
        }
        if !decisions {
            continue;
        }
        let related = load(
            conn,
            "WHERE f.status IN ('draft', 'published') \
               AND ((f.predicate = 'decides' AND o.key = $1) OR (f.predicate = 'about' AND s.key = $1))",
            &[old],
        )
        .await?;
        let plan = plan_decision(&f.id, old, f.valid_from.as_deref(), &related);
        merge_plan(&mut out, plan, &incoming_set);
    }

    // ---- Shares in use turn a certain supersession into a prompt (D51).
    let ending: Vec<String> = out.ended.iter().map(|e| e.fact_id.clone()).collect();
    let used = used_by_live_share(conn, &ending).await?;
    let mut applied = Vec::new();
    for e in std::mem::take(&mut out.ended) {
        if used.contains(&e.fact_id) {
            out.held.push(Held {
                fact_id: e.superseded_by.clone(),
                other_id: e.fact_id.clone(),
                reason: ReviewReason::SupersedeInUse,
            });
        } else {
            applied.push(e);
        }
    }
    for e in &applied {
        sqlx::query(
            "UPDATE _oo_docs.fact SET valid_to = $2::date, superseded_by = $3::uuid \
              WHERE id = $1::uuid AND (valid_to IS NULL OR valid_to > $2::date)",
        )
        .bind(&e.fact_id)
        .bind(&e.valid_to)
        .bind(&e.superseded_by)
        .execute(&mut *conn)
        .await?;
    }
    out.ended = applied;
    out.held.sort();
    out.held.dedup();
    Ok(out)
}

fn merge_plan(out: &mut Reconciled, plan: Plan, incoming: &BTreeSet<&str>) {
    for h in &plan.held {
        if matches!(
            h.reason,
            ReviewReason::FunctionalConflict | ReviewReason::SupersedeCandidate
        ) && incoming.contains(h.fact_id.as_str())
        {
            out.excluded.insert(h.fact_id.clone());
        }
    }
    out.ended.extend(plan.end);
    out.held.extend(plan.held);
}

/// Write the review items of `held` (deduplicated by reason and fact key).
pub async fn queue_held(conn: &mut PgConnection, held: &[Held]) -> Result<u32, StoreError> {
    let mut n = 0u32;
    for h in held {
        let done = sqlx::query(
            "INSERT INTO _oo_docs.review_item \
               (id, kind, dedupe_key, drive_id, source_file_id, source_version_id, subject, \
                confidence_bp, reason_code) \
             SELECT gen_random_uuid(), 'low_confidence_fact', \
                    'low_confidence_fact:' || $3 || ':' || f.fact_key || ':' || o.fact_key, \
                    src.drive_id, src.file_id, src.version_id, \
                    jsonb_build_object('fact_id', f.id::text, 'other_fact_id', o.id::text), \
                    f.confidence_bp, $3 \
               FROM _oo_docs.fact f \
               JOIN _oo_docs.fact o ON o.id = $2::uuid \
               LEFT JOIN LATERAL ( \
                    SELECT fl.drive_id, fl.id AS file_id, fv.id AS version_id \
                      FROM _oo_docs.evidence e \
                      JOIN _oo_docs.file_version fv ON fv.id = e.version_id \
                      JOIN _oo_docs.file fl ON fl.id = fv.file_id \
                     WHERE e.target_kind = 'fact' AND e.target_id = f.id AND e.withdrawn_at IS NULL \
                     ORDER BY e.created_at, e.id LIMIT 1) src ON true \
              WHERE f.id = $1::uuid \
             ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(&h.fact_id)
        .bind(&h.other_id)
        .bind(h.reason.code())
        .execute(&mut *conn)
        .await?;
        n += u32::try_from(done.rows_affected()).unwrap_or(0);
    }
    Ok(n)
}

/// Two live facts of one functional group whose intervals overlap with
/// different values (DOC-G3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overlap {
    pub a_id: String,
    pub b_id: String,
    pub a_key: String,
    pub b_key: String,
    pub a_published: bool,
    pub b_published: bool,
}

/// Overlaps, after supersession was applied, that involve one of `ids`.
pub async fn overlaps_after(
    conn: &mut PgConnection,
    ids: &[String],
) -> Result<Vec<Overlap>, StoreError> {
    let sql = format!("{SELECT} WHERE f.id = ANY($1::text[]::uuid[])");
    let inc: Vec<TFact> = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(sql))
        .bind(ids)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(to_fact)
        .collect::<Result<_, _>>()?;
    let mut subjects: BTreeSet<String> = BTreeSet::new();
    let mut objects: BTreeSet<String> = BTreeSet::new();
    for f in &inc {
        if functional_group(f).is_some() {
            if f.predicate == "owns" {
                if let Some(o) = &f.object_key {
                    objects.insert(o.clone());
                }
            } else {
                subjects.insert(f.subject_key.clone());
            }
        }
    }
    if subjects.is_empty() && objects.is_empty() {
        return Ok(Vec::new());
    }
    let subjects: Vec<String> = subjects.into_iter().collect();
    let objects: Vec<String> = objects.into_iter().collect();
    let sql = format!(
        "{SELECT} WHERE (f.status = 'published' OR f.id = ANY($3::text[]::uuid[])) \
           AND ((f.predicate IN ('attr.status', 'attr.role') AND s.key = ANY($1::text[])) \
             OR (f.predicate = 'owns' AND o.type = 'Project' AND o.key = ANY($2::text[])))"
    );
    let rows: Vec<TFact> = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(sql))
        .bind(&subjects)
        .bind(&objects)
        .bind(ids)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(to_fact)
        .collect::<Result<_, _>>()?;
    let mut groups: BTreeMap<String, Vec<TFact>> = BTreeMap::new();
    for f in rows {
        if let Some(g) = functional_group(&f) {
            groups.entry(g).or_default().push(f);
        }
    }
    let incoming: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for group in groups.values() {
        for (a, b) in super::overlaps(group) {
            if !incoming.contains(a.as_str()) && !incoming.contains(b.as_str()) {
                continue;
            }
            let get = |id: &str| group.iter().find(|f| f.id == id);
            if let (Some(fa), Some(fb)) = (get(&a), get(&b)) {
                out.push(Overlap {
                    a_id: a,
                    b_id: b,
                    a_key: fa.fact_key.clone(),
                    b_key: fb.fact_key.clone(),
                    a_published: fa.published,
                    b_published: fb.published,
                });
            }
        }
    }
    out.sort_by(|x, y| x.a_id.cmp(&y.a_id).then(x.b_id.cmp(&y.b_id)));
    Ok(out)
}
