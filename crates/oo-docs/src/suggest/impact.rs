//! Cross-share consistency around suggestions (10 §16, B-S7-07): which other
//! running Shares used a fact before it is superseded (the check before an
//! accept), and the broadcast into their update stream afterwards (the
//! `UPDATES.md` stream, which the MCP resource
//! `puppyone://share/<sid>/updates` mirrors).

use serde_json::{Value, json};
use sqlx::PgConnection;

use crate::api::error::ApiError;
use crate::share::updates::{self, NewUpdate};

/// A live Share that used one of the facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Affected {
    pub share_id: String,
    pub task: Option<String>,
    pub group_id: Option<String>,
    pub sections: Vec<String>,
    pub version: i64,
}

/// Live Shares (active or drifted, not expired) that used any of `fact_ids`.
pub async fn affected_shares(
    conn: &mut PgConnection,
    fact_ids: &[String],
) -> Result<Vec<Affected>, ApiError> {
    if fact_ids.is_empty() {
        return Ok(Vec::new());
    }
    #[allow(clippy::type_complexity)]
    let rows: Vec<(String, Option<String>, Option<String>, Vec<String>, i64)> = sqlx::query_as(
        "SELECT s.id::text, s.task, s.group_id::text, array_agg(DISTINCT u.section ORDER BY u.section), \
                p.version_n \
           FROM _oo_docs.share_fact_use u JOIN _oo_docs.share s ON s.id = u.share_id \
           JOIN _oo_docs.context_pack p ON p.id = s.pack_id \
          WHERE u.fact_id = ANY($1::uuid[]) AND s.status IN ('active','drifted') \
            AND s.expires_at > clock_timestamp() \
          GROUP BY s.id, s.task, s.group_id, p.version_n ORDER BY s.id",
    )
    .bind(fact_ids)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(share_id, task, group_id, sections, version)| Affected {
            share_id,
            task,
            group_id,
            sections,
            version,
        })
        .collect())
}

/// The same for one optional fact.
pub async fn affected_shares_of_fact(
    conn: &mut PgConnection,
    fact: Option<&str>,
) -> Result<Vec<Affected>, ApiError> {
    match fact {
        Some(f) => affected_shares(conn, &[f.to_owned()]).await,
        None => Ok(Vec::new()),
    }
}

/// JSON form for the API: `[{share_id, task, group_id, sections, version}]`.
pub async fn affected(
    conn: &mut PgConnection,
    fact_ids: &[String],
    skip: Option<&str>,
) -> Result<Vec<Value>, ApiError> {
    Ok(affected_shares(conn, fact_ids)
        .await?
        .into_iter()
        .filter(|a| Some(a.share_id.as_str()) != skip)
        .map(|a| {
            json!({"share_id": a.share_id, "task": a.task, "group_id": a.group_id,
                        "sections": a.sections, "version": a.version})
        })
        .collect())
}

/// Append one `suggestion_accepted` entry to every affected Share's update stream (level `warn`: the
/// fact the Share used is superseded). Same transaction as the accept, so the entry exists exactly
/// when the new fact is accepted. Returns the number of Shares told.
pub async fn broadcast(
    conn: &mut PgConnection,
    suggestion_id: &str,
    shares: &[Affected],
    old_fact_key: Option<&str>,
    new_fact: &str,
) -> Result<usize, ApiError> {
    let latest: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(n), 0) FROM _oo_docs.ontology_version")
            .fetch_one(&mut *conn)
            .await?;
    let keys: Vec<String> = old_fact_key.into_iter().map(str::to_owned).collect();
    let label: Option<String> = sqlx::query_scalar(
        "SELECT s.canonical_name || ' ' || f.predicate || ' ' || COALESCE(o.canonical_name, f.value::text) \
           FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
           LEFT JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.id = $1::uuid",
    )
    .bind(new_fact)
    .fetch_optional(&mut *conn)
    .await?;
    for a in shares {
        updates::append(
            conn,
            &NewUpdate {
                share_id: &a.share_id,
                from_version: a.version,
                to_version: latest.max(a.version),
                level: "warn",
                section: a.sections.first().map(String::as_str),
                fact_keys: &keys,
                summary_key: "suggestion_accepted",
                summary_args: json!({"suggestion_id": suggestion_id, "count": 1,
                                     "labels": label.iter().collect::<Vec<_>>(), "sections": a.sections}),
            },
        )
        .await?;
    }
    Ok(shares.len())
}
