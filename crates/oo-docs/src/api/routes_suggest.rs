//! Suggestions (10 §11.1, §13 B-S7/B-S8; docs-api 1.3):
//!
//! * `POST /suggestions`: a share's `oodr_` credential with scope `suggest:write` (the share is the
//!   credential's), or the engine token with `share_id` in the body (the daemon's collector). Body: one
//!   record, `{"items": [...]}` (at most 50) or `{"documents": [{"name","format","text"}]}` (at most 200
//!   `.md` / `.json` files read from `.context/suggest/`). Every record is idempotent on its
//!   `idempotency_key` (or its content hash) per share. Answer `{"results": [...]}`, one entry per record
//!   or per document that did not parse.
//! * `GET /suggestions`, `GET /suggestions/{id}`, `GET /suggestions/{id}/diff`, `GET /suggestions:digest`.
//! * `POST /suggestions/{id}:decide` `{"decision": "accept|reject|undo", "revision"?, "note"?}`.
//!
//! Pending suggestions are also `suggestion` items in the review queue; deciding the item there does the
//! same as `:decide`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use serde_json::{Value, json};
use sqlx::Connection;

use super::error::{ApiError, code};
use super::http::{self, is_uuid, parse_json};
use super::{AppState, Req};
use crate::share::credential::{self, RATE_PER_MINUTE, ShareCred};
use crate::suggest::schema::{self, MAX_RECORDS_PER_FILE};
use crate::suggest::{diff, digest, store};

const MAX_ITEMS: usize = 50;
const MAX_DOCUMENTS: usize = 200;

/// `POST /v1/docs/suggestions`.
pub async fn create(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = parse_json(&req.body)?;
    let o = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    let share_id = match (&req.cred, o.get("share_id").and_then(Value::as_str)) {
        (Some(c), given) => {
            if !c.scopes.iter().any(|s| s == "suggest:write") {
                return Err(ApiError::new(
                    code::SUGGEST_SCOPE_MISSING,
                    "the credential cannot write suggestions",
                ));
            }
            if given.is_some_and(|g| g != c.share_id) {
                return Err(ApiError::new(
                    code::SHARE_REFS_OUT_OF_SCOPE,
                    "the credential belongs to another share",
                ));
            }
            c.share_id.clone()
        }
        (None, Some(g)) if is_uuid(g) => g.to_owned(),
        (None, _) => return Err(ApiError::bad("/share_id", "expected a share id")),
    };
    if let Some(c) = &req.cred
        && let Err(ms) = credential::take(&c.cred_id, RATE_PER_MINUTE)
    {
        return Err(
            ApiError::new(code::RATE_LIMITED, "too many calls for this credential")
                .details(json!({"retry_after_ms": ms}))
                .header("retry-after", ms.div_ceil(1000).max(1).to_string()),
        );
    }
    // (name, record) pairs; a document that does not parse is its own result.
    let mut records: Vec<(Option<String>, Value)> = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    if let Some(docs) = o.get("documents") {
        let docs = docs
            .as_array()
            .filter(|d| d.len() <= MAX_DOCUMENTS)
            .ok_or_else(|| ApiError::bad("/documents", "expected an array of at most 200"))?;
        for d in docs {
            let name = d["name"].as_str().unwrap_or("?").to_owned();
            let parsed = schema::parse_document(
                d["format"].as_str().unwrap_or_default(),
                d["text"].as_str().unwrap_or_default(),
            );
            match parsed {
                Ok(list) => records.extend(list.into_iter().map(|r| (Some(name.clone()), r))),
                Err(e) => failed.push(json!({"document": name, "status": "invalid", "error": e})),
            }
        }
    } else if let Some(items) = o.get("items") {
        let items = items
            .as_array()
            .filter(|i| !i.is_empty() && i.len() <= MAX_ITEMS)
            .ok_or_else(|| ApiError::bad("/items", "expected 1 to 50 records"))?;
        records.extend(items.iter().cloned().map(|r| (None, r)));
    } else {
        let mut one = o.clone();
        one.remove("share_id");
        records.push((None, Value::Object(one)));
    }
    if records.len() > MAX_DOCUMENTS * MAX_RECORDS_PER_FILE {
        return Err(ApiError::bad("", "too many records"));
    }
    let mut conn = app.pool.acquire().await?;
    let share = store::load_share(&mut conn, &share_id).await?;
    let by = if req.cred.is_some() {
        "credential"
    } else {
        "collector"
    };
    let cred_id = req.cred.as_ref().map(|c: &ShareCred| c.cred_id.as_str());
    let mut results = failed;
    for (doc, rec) in records {
        let mut r = match schema::validate(&rec) {
            Err(e) => json!({"status": "invalid", "error": e}),
            Ok(s) => {
                let mut tx = conn.begin().await?;
                match store::submit(&mut tx, &share, &s, cred_id, by).await {
                    Ok(v) => {
                        tx.commit().await?;
                        v
                    }
                    Err(e) => {
                        drop(tx);
                        json!({"status": "invalid", "error": e.message, "code": e.code.as_str()})
                    }
                }
            }
        };
        if let Some(d) = doc {
            r["document"] = json!(d);
        }
        results.push(r);
    }
    if let Some(c) = cred_id {
        credential::touch(&mut conn, c).await?;
    }
    app.wake.notify_waiters();
    let ok = results.iter().filter(|r| r["status"] != "invalid").count();
    Ok(http::json(
        200,
        &json!({"share_id": share_id, "accepted": ok, "rejected": results.len() - ok, "results": results}),
    ))
}

/// `GET /v1/docs/suggestions`.
pub async fn list(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let status = req.query.get("status").filter(|s| !s.is_empty());
    if status.is_some_and(|s| !digest::STATUSES.contains(&s)) {
        return Err(ApiError::bad("/query/status", "unknown status"));
    }
    let share = req.query.get("share_id").filter(|s| !s.is_empty());
    if share.is_some_and(|s| !is_uuid(s)) {
        return Err(ApiError::bad("/query/share_id", "expected a uuid"));
    }
    let day = req.query.get("day").filter(|s| !s.is_empty());
    if day.is_some_and(|d| d.len() != 10) {
        return Err(ApiError::bad("/query/day", "expected YYYY-MM-DD"));
    }
    let limit = req.query.limit()?;
    let mut conn = app.pool.acquire().await?;
    let items = store::list(&mut conn, status, share, day, limit).await?;
    Ok(http::json(200, &json!({"items": items})))
}

/// `GET /v1/docs/suggestions:digest`.
pub async fn digest(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let day = req.query.get("day").filter(|s| !s.is_empty());
    if day.is_some_and(|d| d.len() != 10) {
        return Err(ApiError::bad("/query/day", "expected YYYY-MM-DD"));
    }
    let mut conn = app.pool.acquire().await?;
    Ok(http::json(200, &digest::day(&mut conn, day).await?))
}

/// `GET /v1/docs/suggestions/{id}`.
pub async fn get(app: &Arc<AppState>, id: &str) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let v = store::get(&mut conn, id).await?;
    let rev = v["revision"].as_i64().unwrap_or(1);
    Ok(http::json_with(200, &v, &[("etag", http::etag(rev))]))
}

/// `GET /v1/docs/suggestions/{id}/diff`.
pub async fn diff(app: &Arc<AppState>, id: &str) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    Ok(http::json(200, &diff::draft(&mut conn, id).await?))
}

/// `POST /v1/docs/suggestions/{id}:decide`.
pub async fn decide(app: &Arc<AppState>, id: &str, req: &Req) -> Result<Response<Body>, ApiError> {
    let body = parse_json(&req.body)?;
    let o = body
        .as_object()
        .ok_or_else(|| ApiError::bad("", "expected an object"))?;
    if let Some(k) = o
        .keys()
        .find(|k| !["decision", "revision", "note"].contains(&k.as_str()))
    {
        return Err(ApiError::bad(&format!("/{k}"), "unknown field"));
    }
    let decision = o
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !["accept", "reject", "undo"].contains(&decision) {
        return Err(ApiError::bad(
            "/decision",
            "expected accept, reject or undo",
        ));
    }
    let note = o.get("note").and_then(Value::as_str);
    if note.is_some_and(|n| n.chars().count() > 500) {
        return Err(ApiError::bad("/note", "at most 500 characters"));
    }
    let revision = o.get("revision").and_then(Value::as_i64);
    let mut conn = app.pool.acquire().await?;
    let mut tx = conn.begin().await?;
    let v = store::decide(&mut tx, id, decision, revision, store::BY_HUMAN, note).await?;
    tx.commit().await?;
    app.wake.notify_waiters();
    Ok(http::json(200, &v))
}
