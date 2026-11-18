//! Drive freshness (00 §5.5, D48, G12): where the ontology of a Drive stands
//! against its files. Priority: held > degraded > syncing > processing >
//! publishing > waiting_review > up_to_date.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::error::ApiError;
use super::http::FMT;
use crate::store::events;

/// States by priority, highest first.
pub const PRIORITY: [&str; 7] = [
    "held",
    "degraded",
    "syncing",
    "processing",
    "publishing",
    "waiting_review",
    "up_to_date",
];

/// Facts about one Drive that decide its freshness.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    /// Latest generation is held.
    pub held: bool,
    /// Reason code when a dependency is down (failed sync, root missing).
    pub degraded: Option<String>,
    /// A sync is queued or running.
    pub syncing: bool,
    /// Files between `stored` and `extracting`.
    pub processing: i64,
    /// A publish job is queued or running.
    pub publishing: bool,
    /// Files with `pending_review > 0`, summed.
    pub pending_review: i64,
}

/// Pick the state and its reason code.
pub fn decide(i: &Inputs) -> (&'static str, Option<String>) {
    if i.held {
        ("held", Some("DOCS_SOURCE_MASS_DELETE_HELD".into()))
    } else if let Some(reason) = &i.degraded {
        ("degraded", Some(reason.clone()))
    } else if i.syncing {
        ("syncing", None)
    } else if i.processing > 0 {
        ("processing", None)
    } else if i.publishing {
        ("publishing", None)
    } else if i.pending_review > 0 {
        ("waiting_review", None)
    } else {
        ("up_to_date", None)
    }
}

/// Highest priority among `states` (`up_to_date` when empty).
pub fn highest<'a>(states: impl IntoIterator<Item = &'a str>) -> &'static str {
    let mut best = PRIORITY.len() - 1;
    for s in states {
        if let Some(p) = PRIORITY.iter().position(|x| *x == s) {
            best = best.min(p);
        }
    }
    PRIORITY[best]
}

/// `Freshness` objects for Drives, by id.
pub async fn compute(
    conn: &mut PgConnection,
    drive_ids: &[String],
) -> Result<HashMap<String, Value>, ApiError> {
    compute_with(conn, drive_ids, None).await
}

/// [`compute`]; `except_job` leaves one publish job (the caller, about to finish) out of "publishing".
async fn compute_with(
    conn: &mut PgConnection,
    drive_ids: &[String],
    except_job: Option<&str>,
) -> Result<HashMap<String, Value>, ApiError> {
    if drive_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let version: Option<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT n, to_char(published_at AT TIME ZONE 'UTC', {FMT}) \
         FROM _oo_docs.ontology_version ORDER BY n DESC LIMIT 1"
    )))
    .fetch_optional(&mut *conn)
    .await?;
    let now: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC', {FMT})"
    )))
    .fetch_one(&mut *conn)
    .await?;
    let (n, as_of) = version.unwrap_or((0, now));
    let covers: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT covers_drive_rev FROM _oo_docs.ontology_version ORDER BY n DESC LIMIT 1), 0)",
    )
    .fetch_one(&mut *conn)
    .await?;
    let publishing: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _oo_docs.job WHERE kind = 'publish' \
                          AND state IN ('queued', 'running') AND ($1::text IS NULL OR id::text <> $1))",
    )
    .bind(except_job)
    .fetch_one(&mut *conn)
    .await?;
    let rows = sqlx::query(
        "SELECT d.id::text AS id, \
         EXISTS (SELECT 1 FROM _oo_docs.drive_sync s WHERE s.drive_id = d.id \
                 AND s.generation = d.sync_generation + 1 AND s.status = 'held') AS held, \
         (SELECT s.error_code FROM _oo_docs.drive_sync s WHERE s.drive_id = d.id \
                 AND s.generation = d.sync_generation + 1 AND s.status = 'failed') AS failed_code, \
         (EXISTS (SELECT 1 FROM _oo_docs.drive_sync s WHERE s.drive_id = d.id AND s.status = 'running') \
          OR EXISTS (SELECT 1 FROM _oo_docs.job j WHERE j.drive_id = d.id AND j.kind = 'sync' \
                     AND j.state IN ('queued', 'running'))) AS syncing, \
         (SELECT count(*) FROM _oo_docs.file f WHERE f.drive_id = d.id AND f.deleted_at IS NULL \
          AND f.state IN ('stored', 'queued', 'parsing', 'parsed', 'extracting')) AS processing, \
         GREATEST((SELECT COALESCE(sum(f.pending_review), 0) FROM _oo_docs.file f \
                    WHERE f.drive_id = d.id AND f.deleted_at IS NULL), \
                  (SELECT count(*) FROM _oo_docs.review_item r \
                    WHERE r.drive_id = d.id AND r.status = 'pending')) AS pending \
         FROM _oo_docs.drive d WHERE d.id = ANY($1::text[]::uuid[])",
    )
    .bind(drive_ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = HashMap::new();
    for r in rows {
        let id: String = r.try_get("id")?;
        let inputs = Inputs {
            held: r.try_get("held")?,
            degraded: r.try_get("failed_code")?,
            syncing: r.try_get("syncing")?,
            processing: r.try_get("processing")?,
            publishing,
            pending_review: r.try_get("pending")?,
        };
        let (state, reason) = decide(&inputs);
        out.insert(
            id,
            json!({
                "state": state,
                "ontology_version": n,
                "covers_drive_rev": covers,
                "as_of": as_of,
                "processing": inputs.processing,
                "pending_review": inputs.pending_review,
                "reason_code": reason,
            }),
        );
    }
    Ok(out)
}

/// The terminal signal of a publish that made no version (a decision that changed nothing to publish, 10 4.9):
/// `drive.freshness_changed` for each active Drive that was "publishing" only because of the finishing job,
/// with the state it has once that job is done, so a client waiting on "publishing" settles without a
/// version event. A Drive whose state the job did not decide gets nothing.
pub async fn announce_settled(
    conn: &mut PgConnection,
    finishing_job: &str,
) -> Result<(), ApiError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.drive WHERE status IN ('active', 'paused')",
    )
    .fetch_all(&mut *conn)
    .await?;
    let during = compute_with(conn, &ids, None).await?;
    for (id, fresh) in compute_with(conn, &ids, Some(finishing_job)).await? {
        if during.get(&id).map(|d| &d["state"]) == Some(&fresh["state"]) {
            continue;
        }
        let payload = json!({ "drive_id": id, "freshness": fresh }).to_string();
        events::append_event(conn, "drive.freshness_changed", Some(&id), None, &payload).await?;
    }
    Ok(())
}

/// Watches every active Drive and appends `drive.freshness_changed` when the
/// state moves. The first observation of a Drive is a silent baseline.
pub fn spawn_watcher(
    pool: PgPool,
    every: Duration,
    mut stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut last: HashMap<String, String> = HashMap::new();
        loop {
            if *stop.borrow() {
                return;
            }
            let _ = tick(&pool, &mut last).await;
            tokio::select! {
                _ = tokio::time::sleep(every) => {}
                _ = stop.changed() => {}
            }
        }
    })
}

async fn tick(pool: &PgPool, last: &mut HashMap<String, String>) -> Result<(), ApiError> {
    let mut conn = pool.acquire().await?;
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.drive WHERE status IN ('active', 'paused')",
    )
    .fetch_all(&mut *conn)
    .await?;
    let now = compute(&mut conn, &ids).await?;
    for (id, fresh) in now {
        let state = fresh["state"].as_str().unwrap_or("up_to_date").to_owned();
        let changed = last.get(&id).is_some_and(|prev| *prev != state);
        last.insert(id.clone(), state);
        if changed {
            let payload = json!({ "drive_id": id, "freshness": fresh }).to_string();
            events::append_event(
                &mut conn,
                "drive.freshness_changed",
                Some(&id),
                None,
                &payload,
            )
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_order_decides() {
        let all = Inputs {
            held: true,
            degraded: Some("X".into()),
            syncing: true,
            processing: 3,
            publishing: true,
            pending_review: 2,
        };
        assert_eq!(decide(&all).0, "held");
        let mut i = Inputs {
            degraded: Some("DOCS_SOURCE_ROOT_MISSING".into()),
            ..all.clone()
        };
        i.held = false;
        assert_eq!(
            decide(&i),
            ("degraded", Some("DOCS_SOURCE_ROOT_MISSING".into()))
        );
        i.degraded = None;
        assert_eq!(decide(&i).0, "syncing");
        i.syncing = false;
        assert_eq!(decide(&i).0, "processing");
        i.processing = 0;
        assert_eq!(decide(&i).0, "publishing");
        i.publishing = false;
        assert_eq!(decide(&i).0, "waiting_review");
        i.pending_review = 0;
        assert_eq!(decide(&i).0, "up_to_date");
    }

    #[test]
    fn highest_of_many() {
        assert_eq!(highest(["up_to_date", "processing", "syncing"]), "syncing");
        assert_eq!(highest([]), "up_to_date");
        assert_eq!(highest(["waiting_review", "held"]), "held");
    }
}
