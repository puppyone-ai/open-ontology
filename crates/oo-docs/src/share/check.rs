//! `POST /shares/{sid}:check` (10 §16.5, 00 §5.6.3): the gate before a commit.
//!
//! Exit codes: 0 up to date or only `info`, 10 a `warn`, 20 a `block`, 30 the
//! share ended (expired, revoked, refreshed). 40 is the client's: it could not
//! reach the engine, and a gate that cannot answer counts as closed. Under
//! `at-checkpoint` a check that comes out 0 or 10 refreshes before it returns.

use serde_json::{Value, json};
use sqlx::PgConnection;

use super::updates::{self, rank};
use crate::api::AppState;
use crate::api::error::ApiError;
use crate::share::{group, lifecycle};

/// The result for `sid` as it stands (no refresh).
pub async fn evaluate(conn: &mut PgConnection, sid: &str, since: i64) -> Result<Value, ApiError> {
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT status, expires_at <= clock_timestamp() FROM _oo_docs.share WHERE id = $1::uuid",
    )
    .bind(sid)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((status, late)) = row else {
        return Err(lifecycle::share_not_found(sid));
    };
    if !matches!(status.as_str(), "active" | "drifted") || late {
        return Ok(json!({"exit_code": 30, "level": null, "items": [],
                         "action": "stop_share_ended", "state": if late && status == "active" { "expired" } else { status.as_str() }}));
    }
    let items = updates::list(conn, sid, since, 1000).await?;
    let worst = items
        .iter()
        .filter_map(|i| i["level"].as_str())
        .max_by_key(|l| rank(l));
    let (exit, action) = match worst.map_or(0, rank) {
        3 => (20, "stop"),
        2 => (10, "refresh_recommended"),
        _ => (0, "continue"),
    };
    Ok(
        json!({"exit_code": exit, "level": worst, "items": items, "action": action, "state": status}),
    )
}

/// `evaluate`, then the at-checkpoint refresh when it applies.
pub async fn run(
    app: &AppState,
    sid: &str,
    since: i64,
    public_host: &str,
    auto_refresh: bool,
) -> Result<Value, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let mut result = evaluate(&mut conn, sid, since).await?;
    let exit = result["exit_code"].as_i64().unwrap_or(40);
    if !auto_refresh || (exit != 0 && exit != 10) {
        return Ok(result);
    }
    let old = lifecycle::load_old(&mut conn, sid).await?;
    if old.policy != "at-checkpoint" {
        return Ok(result);
    }
    let (share_v, latest): (i64, i64) = sqlx::query_as(
        "SELECT p.version_n, (SELECT COALESCE(max(n), 0) FROM _oo_docs.ontology_version) \
           FROM _oo_docs.share s JOIN _oo_docs.context_pack p ON p.id = s.pack_id WHERE s.id = $1::uuid",
    )
    .bind(sid)
    .fetch_one(&mut *conn)
    .await?;
    drop(conn);
    if latest <= share_v {
        return Ok(result);
    }
    let payload = if let Some(gid) = &old.group_id {
        let g = group::refresh(app, gid, public_host).await?;
        let idx = g["results"]
            .as_array()
            .and_then(|r| r.iter().position(|x| x["id"] == sid))
            .unwrap_or(0);
        g["refreshed"][idx].clone()
    } else {
        lifecycle::refresh(app, sid, public_host).await?
    };
    result["refreshed_to"] = payload["id"].clone();
    result["refreshed"] = payload;
    result["upgraded_to_version"] = json!(latest);
    Ok(result)
}
