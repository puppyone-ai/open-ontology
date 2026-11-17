//! `GET /v1/docs/usage?drive_id=&days=7` (10 §8.3, D19): LLM calls per Drive
//! and UTC day, the Drive's limit, cache hits, tokens and the files that hit
//! their own cap, plus the model component status (`not_configured` when no
//! Drive has the model enabled, `limited` when today's limit is used up).

use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use serde_json::json;

use super::error::ApiError;
use super::http::{self, is_uuid};
use super::{AppState, Req};
use crate::extract::llm::FILE_CALL_LIMIT;
use crate::extract::llm::usage::read;

/// `GET /v1/docs/usage`.
pub async fn get(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let drive = req.query.get("drive_id").filter(|d| !d.is_empty());
    if drive.is_some_and(|d| !is_uuid(d)) {
        return Err(ApiError::bad("/query/drive_id", "expected a uuid"));
    }
    let days = match req.query.get("days") {
        None => 7,
        Some(d) => d
            .parse::<i32>()
            .ok()
            .filter(|n| (1..=90).contains(n))
            .ok_or_else(|| ApiError::bad("/query/days", "days must be 1 to 90"))?,
    };
    let mut conn = app.pool.acquire().await?;
    let mut usage = read(&mut conn, drive, days, FILE_CALL_LIMIT).await?;
    let enabled: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.drive WHERE model_enabled AND status <> 'deleted'",
    )
    .fetch_one(&mut *conn)
    .await?;
    let limited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.llm_usage u JOIN _oo_docs.drive d ON d.id = u.drive_id \
          WHERE u.day = (clock_timestamp() AT TIME ZONE 'UTC')::date AND u.reserved >= d.llm_daily_calls",
    )
    .fetch_one(&mut *conn)
    .await?;
    let status = if enabled == 0 {
        "not_configured"
    } else if limited > 0 {
        "limited"
    } else {
        "ok"
    };
    usage["model"] = json!({"status": status});
    Ok(http::json(200, &usage))
}
