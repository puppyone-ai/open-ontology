//! The day's digest (10 §11.1, B-S7-03): everything that came in on one UTC day, by what happened to
//! it. Auto accepted and silently superseded items are already in effect and can be undone; pending
//! ones wait in the review queue. One call is the whole morning review.

use serde_json::{Value, json};
use sqlx::PgConnection;

use super::store;
use crate::api::error::ApiError;

pub const STATUSES: [&str; 8] = [
    "auto_accepted",
    "superseded_silently",
    "pending",
    "accepted",
    "rejected",
    "undone",
    "duplicate",
    "invalid",
];

/// `day` is `YYYY-MM-DD` (UTC); `None` is today.
pub async fn day(conn: &mut PgConnection, day: Option<&str>) -> Result<Value, ApiError> {
    let day: String = match day {
        Some(d) => d.to_owned(),
        None => {
            sqlx::query_scalar(
                "SELECT to_char((clock_timestamp() AT TIME ZONE 'UTC')::date, 'YYYY-MM-DD')",
            )
            .fetch_one(&mut *conn)
            .await?
        }
    };
    let counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, count(*) FROM _oo_docs.suggestion \
          WHERE (created_at AT TIME ZONE 'UTC')::date = $1::date GROUP BY status",
    )
    .bind(&day)
    .fetch_all(&mut *conn)
    .await?;
    let mut by = serde_json::Map::new();
    for s in STATUSES {
        by.insert(
            s.into(),
            json!(counts.iter().find(|(k, _)| k == s).map_or(0, |c| c.1)),
        );
    }
    let items = store::list(conn, None, None, Some(&day), 500).await?;
    let in_effect: Vec<&Value> = items
        .iter()
        .filter(|i| {
            matches!(
                i["status"].as_str(),
                Some("auto_accepted" | "superseded_silently")
            )
        })
        .collect();
    let pending: Vec<&Value> = items.iter().filter(|i| i["status"] == "pending").collect();
    Ok(json!({
        "day": day, "counts": by, "total": items.len(),
        "in_effect": in_effect, "pending": pending,
        "human_operations": {"review_pending_in_one_call": !pending.is_empty(), "undo_per_item": true},
    }))
}
