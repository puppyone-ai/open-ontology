//! `POST /v1/docs/drives/{id}:events` (10 §4.3.6, §11.3, B-S11-02): change hints from bucket event notifications.
//!
//! The body carries the notification messages exactly as the cloud's queue delivered them
//! (`{"messages": [<object or string>, ...]}`, at most 100). Each is parsed by
//! `oo_connectors::objectstore_events::parse`, repeats and out-of-order events are dropped by the Drive's
//! deduper (`(raw_key, version_id, op, sequencer)` LRU of 100,000 and the per-key highest sequencer), and every
//! key that is left gets one targeted sync, which asks the source (`stat`) and so decides by the object, never
//! by the event (D10). Reconciliation keeps running; a lost event costs time, not correctness. Deletions are only
//! reported (`maybe_deleted`): the next full reconcile confirms them (the same rule as a targeted sync).

use axum::body::Body;
use axum::http::Response;
use oo_connectors::objectstore_events::Verdict;
use oo_connectors::objectstore_events::parse::parse_hints;
use serde_json::{Value, json};

use super::error::{ApiError, code};
use super::{AppState, Req, http::json_with};
use crate::ingest::IngestError;

const MAX_MESSAGES: usize = 100;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub async fn post(app: &AppState, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = super::http::parse_json(&req.body)?;
    let msgs = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::bad("/messages", "required array"))?;
    if msgs.is_empty() || msgs.len() > MAX_MESSAGES {
        return Err(ApiError::bad("/messages", "1 to 100 messages"));
    }
    let drive = crate::ingest::reconcile::load_drive(&app.pool, id)
        .await?
        .filter(|d| d.status == "active")
        .ok_or_else(|| ApiError::new(code::NOT_FOUND, "no such active drive"))?;
    let prefix = drive
        .config
        .get("prefix")
        .and_then(Value::as_str)
        .map(|p| {
            let p = p.trim_start_matches('/');
            if p.is_empty() || p.ends_with('/') {
                p.to_owned()
            } else {
                format!("{p}/")
            }
        })
        .unwrap_or_default();
    let policy = crate::policy::DrivePolicy::from_config(&drive.config)
        .map_err(|_| ApiError::new(code::INVALID_CONFIG, "the drive policy is not valid"))?;
    let (mut fresh, mut duplicates, mut stale, mut rejected, mut denied) =
        (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut paths: Vec<String> = Vec::new();
    {
        let mut map = app.hint_dedupe.lock().unwrap_or_else(|p| p.into_inner());
        let dedupe = map.entry(id.to_owned()).or_default();
        for (i, m) in msgs.iter().enumerate() {
            let text = match m {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let hints = match parse_hints(&text, &prefix, now_ms()) {
                Ok(h) => h,
                Err(_) => {
                    rejected += 1;
                    let _ = i;
                    continue;
                }
            };
            for h in hints {
                match dedupe.admit(&h) {
                    Verdict::Fresh => {
                        fresh += 1;
                        match String::from_utf8(h.raw_key.clone()) {
                            Ok(p) if policy.permits(&p) => {
                                if !paths.contains(&p) {
                                    paths.push(p);
                                }
                            }
                            Ok(_) => denied += 1,
                            Err(_) => rejected += 1,
                        }
                    }
                    Verdict::Duplicate => duplicates += 1,
                    Verdict::Stale => stale += 1,
                }
            }
        }
    }
    let (mut added, mut changed, mut maybe_deleted) = (0, 0, Vec::new());
    if !paths.is_empty() {
        let report = app
            .sync
            .targeted(id, paths.clone())
            .await
            .map_err(|e| match e {
                IngestError::DriveNotFound => {
                    ApiError::new(code::NOT_FOUND, "no such active drive")
                }
                other => ApiError::from(other),
            })?;
        added = report.added;
        changed = report.changed;
        maybe_deleted = report.maybe_deleted;
    }
    Ok(json_with(
        200,
        &json!({
            "received": msgs.len(), "fresh": fresh, "duplicates": duplicates, "stale": stale,
            "rejected": rejected, "denied_by_policy": denied, "looked_at": paths.len(),
            "added": added, "changed": changed, "maybe_deleted": maybe_deleted,
        }),
        &[],
    ))
}
