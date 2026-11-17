//! Light drift (D50, G14) and the delivery of changes (D55): when a version is
//! published, every live share whose pack used a fact that this version
//! retracted or superseded, or whose used entities hit a new constraint
//! violation, gets `UPDATES.md` entries and turns `drifted`. Entries are per
//! section (the section digests of the pack say which section used the fact).
//!
//! `apply` runs inside the publish transaction, so a share is `drifted` the
//! moment the version exists (L4 acceptance 9: 5 s).
//!
//! Levels (10 §16.3): `block` for a used fact retracted without a successor or
//! an entity of a used fact under a new constraint violation; `warn` for a used
//! fact superseded; `info` (one per share and version) for new facts in scope
//! that touch no used fact. Only `warn` and `block` change the state.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use super::updates::{self, NewUpdate};
use crate::extract::model::Predicate;
use crate::store::{StoreError, events};

/// What one publish did to the shares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DriftOutcome {
    /// Shares that turned `drifted`.
    pub drifted: Vec<String>,
    /// Entries appended.
    pub updates: u32,
}

/// `path` lies in a share scope (`s.scope` is a jsonb array of prefixes; empty
/// means the whole drive). `$path` is the SQL expression of the path.
fn in_scope(path: &str) -> String {
    format!(
        "(jsonb_array_length(s.scope) = 0 OR EXISTS (SELECT 1 FROM jsonb_array_elements_text(s.scope) sp(p) \
           WHERE {path} = sp.p OR {path} LIKE replace(replace(replace(sp.p, '\\', '\\\\'), '%', '\\%'), '_', '\\_') || '/%'))"
    )
}

fn value_text(v: &Value) -> String {
    for k in ["text", "date", "enum"] {
        if let Some(s) = v.get(k).and_then(Value::as_str) {
            return s.to_owned();
        }
    }
    v.to_string()
}

/// `李甲 负责 项目`: the wording of a fact for an update line.
async fn label(conn: &mut PgConnection, fact_id: &str) -> Result<String, StoreError> {
    let r = sqlx::query(
        "SELECT s.canonical_name AS sn, f.predicate, o.canonical_name AS on_, f.value::text AS v \
           FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
           LEFT JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.id = $1::uuid",
    )
    .bind(fact_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(r) = r else { return Ok(String::new()) };
    let predicate: String = r.try_get("predicate")?;
    let pred = Predicate::parse(&predicate)
        .map(|p| crate::compile::render::predicate_cn(&p))
        .unwrap_or(predicate);
    let object = match (
        r.try_get::<Option<String>, _>("on_")?,
        r.try_get::<Option<String>, _>("v")?,
    ) {
        (Some(o), _) => o,
        (None, Some(v)) => value_text(&serde_json::from_str(&v).unwrap_or(Value::Null)),
        _ => String::new(),
    };
    Ok(format!(
        "{} {} {}",
        r.try_get::<String, _>("sn")?,
        pred,
        object
    ))
}

/// Label of the superseding fact, only when the share's scope has evidence
/// for it (D26: a new value seen only outside the scope is not named).
async fn visible_new_label(
    conn: &mut PgConnection,
    share_id: &str,
    sup_id: &str,
) -> Result<Option<String>, StoreError> {
    let sql = format!(
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.share s, _oo_docs.evidence e \
           JOIN _oo_docs.file_version fv ON fv.id = e.version_id \
          WHERE s.id = $1::uuid AND e.target_kind = 'fact' AND e.target_id = $2::uuid \
            AND e.withdrawn_at IS NULL AND {})",
        in_scope("fv.path_at_version")
    );
    let ok: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(share_id)
        .bind(sup_id)
        .fetch_one(&mut *conn)
        .await?;
    if ok {
        Ok(Some(label(conn, sup_id).await?))
    } else {
        Ok(None)
    }
}

/// Section of a used fact: its BRIEF section when it has one, else the first
/// section that holds it.
async fn section_of(
    conn: &mut PgConnection,
    share_id: &str,
    fact_key: &str,
) -> Result<Option<String>, StoreError> {
    Ok(sqlx::query_scalar(
        "SELECT section FROM _oo_docs.share_section_use WHERE share_id = $1::uuid AND fact_key = $2 \
          ORDER BY (section LIKE 'BRIEF.md#%') DESC, section LIMIT 1",
    )
    .bind(share_id)
    .bind(fact_key)
    .fetch_optional(&mut *conn)
    .await?)
}

#[derive(Default)]
struct Group {
    fact_keys: Vec<String>,
    labels: Vec<String>,
    new: Vec<String>,
}

/// Apply version `n` to every live share. Call inside the publish
/// transaction after the version's facts are stamped.
pub async fn apply(conn: &mut PgConnection, n: i64) -> Result<DriftOutcome, StoreError> {
    sqlx::query(
        "UPDATE _oo_docs.constraint_violation SET detected_in = $1 \
          WHERE detected_in IS NULL AND status = 'open'",
    )
    .bind(n)
    .execute(&mut *conn)
    .await?;

    let live: Vec<(String, i64)> = sqlx::query_as(
        "SELECT s.id::text, p.version_n FROM _oo_docs.share s \
           JOIN _oo_docs.context_pack p ON p.id = s.pack_id \
          WHERE s.status IN ('active', 'drifted') AND s.expires_at > clock_timestamp() \
            AND p.version_n < $1 ORDER BY s.id",
    )
    .bind(n)
    .fetch_all(&mut *conn)
    .await?;

    let mut out = DriftOutcome::default();
    for (sid, pack_version) in live {
        // (level, section) -> facts
        let mut groups: BTreeMap<(u8, String, &'static str), Group> = BTreeMap::new();

        let cands = sqlx::query(
            "SELECT f.id::text AS fid, f.fact_key, f.retracted_in = $2 AS retracted, \
                    f.superseded_by::text AS sup \
               FROM _oo_docs.share_fact_use u JOIN _oo_docs.fact f ON f.id = u.fact_id \
              WHERE u.share_id = $1::uuid \
                AND (f.retracted_in = $2 OR EXISTS (SELECT 1 FROM _oo_docs.fact x \
                       WHERE x.id = f.superseded_by AND x.published_in = $2)) \
              ORDER BY f.fact_key",
        )
        .bind(&sid)
        .bind(n)
        .fetch_all(&mut *conn)
        .await?;
        for r in &cands {
            let (fid, key): (String, String) = (r.try_get("fid")?, r.try_get("fact_key")?);
            let sup: Option<String> = r.try_get("sup")?;
            let new_label = match &sup {
                Some(s) => visible_new_label(conn, &sid, s).await?,
                None => None,
            };
            let (rank, summary) = if sup.is_some() {
                (2u8, "facts_superseded")
            } else {
                (3u8, "facts_retracted")
            };
            let section = section_of(conn, &sid, &key).await?.unwrap_or_default();
            let g = groups.entry((rank, section, summary)).or_default();
            g.fact_keys.push(key);
            g.labels.push(label(conn, &fid).await?);
            if let Some(l) = new_label {
                g.new.push(l);
            }
        }

        let viol = sqlx::query(
            "SELECT DISTINCT f.fact_key, e.canonical_name AS entity \
               FROM _oo_docs.share_fact_use u JOIN _oo_docs.fact f ON f.id = u.fact_id \
               JOIN _oo_docs.entity e ON e.id = f.subject_id OR e.id = f.object_id \
               JOIN _oo_docs.constraint_violation cv ON cv.entity_key = e.key \
                    AND cv.status = 'open' AND cv.detected_in = $2 \
              WHERE u.share_id = $1::uuid ORDER BY f.fact_key, e.canonical_name",
        )
        .bind(&sid)
        .bind(n)
        .fetch_all(&mut *conn)
        .await?;
        for r in &viol {
            let (key, entity): (String, String) = (r.try_get("fact_key")?, r.try_get("entity")?);
            let section = section_of(conn, &sid, &key).await?.unwrap_or_default();
            let g = groups
                .entry((3, section, "constraint_conflict"))
                .or_default();
            if !g.fact_keys.contains(&key) {
                g.fact_keys.push(key);
            }
            if !g.labels.contains(&entity) {
                g.labels.push(entity);
            }
        }

        let from_version: i64 = sqlx::query_scalar(
            "SELECT GREATEST($2, COALESCE(max(to_version), 0)) FROM _oo_docs.share_update WHERE share_id = $1::uuid",
        )
        .bind(&sid)
        .bind(pack_version)
        .fetch_one(&mut *conn)
        .await?;

        let mut worst = 0u8;
        // Highest level first, then section, then wording key.
        for ((rank, section, summary), g) in groups.iter().rev() {
            worst = worst.max(*rank);
            let level = if *rank == 3 { "block" } else { "warn" };
            let shown: Vec<&String> = g.labels.iter().take(5).collect();
            let args = json!({"labels": shown, "new": g.new.iter().take(5).collect::<Vec<_>>(),
                              "count": g.fact_keys.len()});
            let sec = (!section.is_empty()).then_some(section.as_str());
            updates::append(
                conn,
                &NewUpdate {
                    share_id: &sid,
                    from_version,
                    to_version: n,
                    level,
                    section: sec,
                    fact_keys: &g.fact_keys,
                    summary_key: summary,
                    summary_args: args,
                },
            )
            .await?;
            out.updates += 1;
        }

        // One info per share and version, when something new is in scope.
        let sql = format!(
            "SELECT count(DISTINCT f.id) FROM _oo_docs.share s, _oo_docs.fact f \
               JOIN _oo_docs.evidence e ON e.target_kind = 'fact' AND e.target_id = f.id \
                    AND e.withdrawn_at IS NULL \
               JOIN _oo_docs.file_version fv ON fv.id = e.version_id \
               JOIN _oo_docs.file fl ON fl.id = fv.file_id \
              WHERE s.id = $1::uuid AND f.published_in = $2 AND fl.drive_id = s.drive_id \
                AND {} AND NOT EXISTS (SELECT 1 FROM _oo_docs.share_fact_use u \
                                        WHERE u.share_id = s.id AND u.fact_id = f.id)",
            in_scope("fv.path_at_version")
        );
        let fresh: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
            .bind(&sid)
            .bind(n)
            .fetch_one(&mut *conn)
            .await?;
        if fresh > 0 {
            updates::append(
                conn,
                &NewUpdate {
                    share_id: &sid,
                    from_version,
                    to_version: n,
                    level: "info",
                    section: None,
                    fact_keys: &[],
                    summary_key: "new_facts",
                    summary_args: json!({"count": fresh}),
                },
            )
            .await?;
            out.updates += 1;
        }

        if worst >= 2 {
            let moved = sqlx::query(
                "UPDATE _oo_docs.share SET status = 'drifted', drifted_at = clock_timestamp(), \
                        revision = revision + 1 WHERE id = $1::uuid AND status = 'active'",
            )
            .bind(&sid)
            .execute(&mut *conn)
            .await?
            .rows_affected();
            if moved > 0 {
                events::append_event(
                    conn,
                    "share.state_changed",
                    None,
                    Some(&sid),
                    &json!({"share_id": sid, "from": "active", "to": "drifted", "version": n})
                        .to_string(),
                )
                .await?;
                out.drifted.push(sid.clone());
            }
        }
    }
    Ok(out)
}
