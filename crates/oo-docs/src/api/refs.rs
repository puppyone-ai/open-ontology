//! `GET /v1/docs/refs/{b3}` (10 §7.8): the original bytes of a file the share
//! cites, authenticated by the share's refs credential. Out of scope 403,
//! expired or revoked 410 (answered before the route), 60 reads a minute.

use axum::body::Body;
use axum::http::{Response, StatusCode, header};
use serde_json::json;
use std::sync::Arc;

use super::error::{ApiError, code};
use super::file_detail::{RangeSpec, content_type, parse_range};
use super::{AppState, Req};
use crate::cas::CasKind;
use crate::share::credential::{self, RATE_PER_MINUTE, ShareCred};
use crate::store::events;

pub async fn get(
    app: &Arc<AppState>,
    b3: &str,
    req: &Req,
    cred: &ShareCred,
) -> Result<Response<Body>, ApiError> {
    if !cred.scopes.iter().any(|s| s == "refs:read") {
        return Err(ApiError::new(
            code::SHARE_REFS_OUT_OF_SCOPE,
            "the credential cannot read refs",
        ));
    }
    if let Err(ms) = credential::take(&cred.cred_id, RATE_PER_MINUTE) {
        return Err(
            ApiError::new(code::RATE_LIMITED, "too many reads for this credential")
                .details(json!({"retry_after_ms": ms}))
                .header("retry-after", ms.div_ceil(1000).max(1).to_string()),
        );
    }
    let mut conn = app.pool.acquire().await?;
    let (drive, raw_key, size) = credential::locate(&mut conn, cred, b3).await?;
    credential::touch(&mut conn, &cred.cred_id).await?;
    events::append_event(
        &mut conn,
        "refs.accessed",
        Some(&drive),
        Some(&cred.share_id),
        &json!({"share_id": cred.share_id, "b3": &b3[..8]}).to_string(),
    )
    .await?;
    let format: Option<String> =
        sqlx::query_scalar("SELECT format FROM _oo_docs.blob WHERE b3 = $1")
            .bind(b3)
            .fetch_optional(&mut *conn)
            .await?
            .flatten();
    drop(conn);
    let size = u64::try_from(size).unwrap_or(0);
    let (start, end, partial) = match parse_range(
        req.headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
        size,
    ) {
        RangeSpec::Unsatisfiable => {
            let mut e = ApiError::bad("/headers/range", "range is outside the file")
                .header("content-range", format!("bytes */{size}"));
            e.status = 416;
            return Err(e);
        }
        RangeSpec::Bytes(a, b) => (a, b, true),
        RangeSpec::Whole => (0, size.saturating_sub(1), false),
    };
    let len = if size == 0 { 0 } else { end - start + 1 };
    let cached = app
        .cas
        .as_ref()
        .filter(|c| c.exists(CasKind::Orig, b3).unwrap_or(false))
        .and_then(|c| c.get(CasKind::Orig, b3).ok());
    let bytes = match cached {
        Some(all) => all
            .get(start as usize..((start + len) as usize).min(all.len()))
            .map(<[u8]>::to_vec)
            .unwrap_or_default(),
        None => app
            .sync
            .read_original(&drive, raw_key, start, len)
            .await
            .map_err(|e| {
                let mut err = ApiError::new(code::INTERNAL, e.code());
                err.status = if e.retryable() { 503 } else { 502 };
                err
            })?,
    };
    if !partial && blake3::hash(&bytes).to_hex().as_str() != b3 {
        let mut e = ApiError::new(
            code::INTERNAL,
            "the file changed on disk since it was hashed",
        );
        e.status = 409;
        return Err(e);
    }
    let mut b = Response::builder()
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, content_type(format.as_deref()))
        .header(header::CONTENT_DISPOSITION, "attachment")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::ETAG, format!("\"{b3}\""))
        .header(header::CONTENT_LENGTH, bytes.len());
    if partial {
        b = b.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{size}"));
    }
    Ok(b.body(Body::from(bytes)).unwrap_or_default())
}
