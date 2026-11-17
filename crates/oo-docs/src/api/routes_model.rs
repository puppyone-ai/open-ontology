//! `GET/PUT /v1/docs/config/model` (10 §8.8 G3, D18, D19).
//!
//! The model is the engine's, not a Drive's: provider, `base_url`, model id and
//! `key_ref`. The key is only ever a reference (`file:/abs/path` or `env:NAME`);
//! a value in its place is a 400 that does not echo it, and no response carries
//! a secret. `PUT` needs `If-Match: "r<revision>"` (an unconfigured engine is
//! revision 1). Whether the egress behind the configuration is up shows as
//! `status` (`ok`, `unavailable`) and, for a refusal, `code`.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use serde_json::{Value, json};

use super::error::{ApiError, code};
use super::http::{self, etag, if_match, parse_json};
use super::{AppState, Req};
use crate::extract::llm::config::{ModelRuntime, PutError, validate_put};

fn view(m: &ModelRuntime) -> (Value, i64) {
    match m.config() {
        None => (
            json!({"configured": false, "revision": 1, "status": "not_configured"}),
            1,
        ),
        Some(c) => {
            let mut v = c.to_json(m.status());
            if let Some(e) = m.last_error().filter(|_| m.status() == "unavailable") {
                v["code"] = json!(e);
            }
            (v, c.revision)
        }
    }
}

/// `GET /v1/docs/config/model`.
pub fn get(app: &Arc<AppState>) -> Result<Response<Body>, ApiError> {
    let (v, rev) = view(&app.model);
    Ok(http::json_with(200, &v, &[("ETag", etag(rev))]))
}

/// `PUT /v1/docs/config/model`.
pub async fn put(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let want = if_match(&req.headers)?;
    let body = parse_json(&req.body)?;
    let parts = validate_put(&body).map_err(|e| {
        let pointer = if e.pointer.is_empty() {
            String::new()
        } else {
            format!("/{}", e.pointer)
        };
        ApiError::bad(&pointer, e.message)
    })?;
    let rt = app.model.clone();
    // Starting the egress waits for its port: off the async threads.
    let done = tokio::task::spawn_blocking(move || rt.put(want, parts))
        .await
        .map_err(|_| ApiError::new(code::INTERNAL, "model configuration failed"))?;
    match done {
        Ok(_) => {
            let (v, rev) = view(&app.model);
            Ok(http::json_with(200, &v, &[("ETag", etag(rev))]))
        }
        Err(PutError::Conflict { current }) => Err(ApiError::new(
            code::PRECONDITION_FAILED,
            format!("the model configuration is at revision r{current}"),
        )),
        Err(PutError::Store) => Err(ApiError::new(
            code::INTERNAL,
            "the model configuration could not be saved",
        )),
    }
}
