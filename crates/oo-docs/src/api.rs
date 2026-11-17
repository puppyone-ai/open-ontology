//! Docs control API, `docs-api/1.0` (10 §8). Board WS-BE5.
//!
//! One axum fallback handler owns routing, so the guards run in one fixed
//! order for every request, including the 404s and 405s:
//!
//! 1. `Origin` present -> `403 DOCS_ENGINE_FORBIDDEN_ORIGIN`
//! 2. `Host` not in the allow list -> `400 DOCS_ENGINE_BAD_HOST` (DNS rebinding)
//! 3. `OO-Docs-Api-Min` above 1.0 -> `400 DOCS_ENGINE_VERSION_UNSUPPORTED`
//! 4. `GET /livez` answers `204` here, the only unauthenticated path
//! 5. Bearer token (SHA-256, constant-time compare) -> `401`
//! 6. 200 requests per second per token -> `429`
//! 7. body at most 1 MiB -> `413`
//!
//! Every response, errors included, carries `OO-Docs-Api: 1.0`.
//!
//! L2 serves: `livez`, `health`, drives (list, create, get, patch, delete),
//! `:sync` (full, deep, confirm, targeted), files, tree and the event stream.
//! Other documented routes answer `404 DOCS_ENGINE_NOT_FOUND` until their
//! boards land.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, Response, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::{Notify, Semaphore, watch};

use crate::health::HealthProbe;
use crate::ingest::reconcile::{TargetedReport, targeted_sync};
use crate::ingest::{DriveRef, IngestError, IngestLimits, SourceFactory};
use oo_connectors::objectstore::DocSourceError;

pub mod auth;
pub mod cursor;
pub mod drives;
pub mod error;
pub mod events;
pub mod file_detail;
pub mod files;
pub mod freshness;
pub mod http;
pub mod idem;
pub mod lifecycle;
pub mod metrics;
pub mod packs;
pub mod progress;
pub mod shares;

use error::{ApiError, code};
use http::Query;

/// Request body ceiling.
pub const MAX_BODY: usize = 1024 * 1024;
/// Requests per second per token.
pub const RATE_PER_SECOND: u32 = 200;
/// Concurrent event streams.
pub const MAX_STREAMS: usize = 16;

/// A request after the guards.
pub struct Req {
    /// Method.
    pub method: Method,
    /// Path without the query.
    pub path: String,
    /// Query string.
    pub query: Query,
    /// Headers.
    pub headers: HeaderMap,
    /// Body (read for POST, PATCH and DELETE only).
    pub body: Bytes,
}

type SyncFuture<'a> =
    Pin<Box<dyn Future<Output = Result<TargetedReport, IngestError>> + Send + 'a>>;

/// What the API needs from the ingest stage that is source specific.
pub trait SyncPort: Send + Sync + 'static {
    /// Targeted sync of `paths` in a Drive (G11).
    fn targeted(&self, drive_id: &str, paths: Vec<String>) -> SyncFuture<'_>;
    /// Registration probe: can this config be opened read only?
    fn probe(&self, config: &Value) -> Result<(), DocSourceError>;
    /// Bytes `[offset, offset + len)` of an object of a Drive (the original
    /// behind `GET /files/{fid}/content`). Sources that cannot serve this say
    /// so with `Unavailable`.
    fn read_original(
        &self,
        _drive_id: &str,
        _raw_key: Vec<u8>,
        _offset: u64,
        _len: u64,
    ) -> ReadFuture<'_> {
        Box::pin(async { Err(DocSourceError::Unavailable) })
    }
}

/// Result of [`SyncPort::read_original`].
pub type ReadFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<u8>, DocSourceError>> + Send + 'a>>;

/// [`SyncPort`] over a [`SourceFactory`].
pub struct FactorySyncPort<F: SourceFactory> {
    pool: PgPool,
    factory: F,
    limits: IngestLimits,
}

impl<F: SourceFactory> FactorySyncPort<F> {
    /// New port.
    pub fn new(pool: PgPool, factory: F, limits: IngestLimits) -> Self {
        Self {
            pool,
            factory,
            limits,
        }
    }
}

impl<F: SourceFactory> SyncPort for FactorySyncPort<F> {
    fn targeted(&self, drive_id: &str, paths: Vec<String>) -> SyncFuture<'_> {
        let drive_id = drive_id.to_owned();
        Box::pin(async move {
            let drive = crate::ingest::reconcile::load_drive(&self.pool, &drive_id)
                .await?
                .ok_or(IngestError::DriveNotFound)?;
            let source = self.factory.open(&drive).map_err(|e| {
                IngestError::Protocol(format!("source cannot be opened: {}", e.code()))
            })?;
            targeted_sync(&self.pool, &drive, &source, &paths, &self.limits).await
        })
    }

    fn probe(&self, config: &Value) -> Result<(), DocSourceError> {
        let drive = DriveRef {
            id: String::new(),
            backend: "fs".to_owned(),
            config: config.clone(),
            status: "active".to_owned(),
            sync_generation: 0,
            max_files: 0,
        };
        self.factory.open(&drive).map(|_| ())
    }

    fn read_original(
        &self,
        drive_id: &str,
        raw_key: Vec<u8>,
        offset: u64,
        len: u64,
    ) -> ReadFuture<'_> {
        let drive_id = drive_id.to_owned();
        Box::pin(async move {
            let drive = crate::ingest::reconcile::load_drive(&self.pool, &drive_id)
                .await
                .map_err(|_| DocSourceError::Unavailable)?
                .ok_or(DocSourceError::NotFound)?;
            let source = self.factory.open(&drive)?;
            oo_connectors::objectstore::DocumentSource::read(
                &source,
                &raw_key,
                offset,
                len,
                &oo_connectors::objectstore::ReadExpect::default(),
            )
            .await
        })
    }
}

struct Bucket {
    tokens: f64,
    at: Instant,
}

/// Shared state of the API.
pub struct AppState {
    /// Runtime role pool.
    pub pool: PgPool,
    /// SHA-256 of every accepted token.
    pub tokens: Vec<[u8; 32]>,
    /// Accepted `Host` values.
    pub hosts: Vec<String>,
    /// Source specific operations.
    pub sync: Arc<dyn SyncPort>,
    /// Health sources.
    pub health: Arc<HealthProbe>,
    /// Content store for packs; `None` leaves the pack routes answering 500.
    pub cas: Option<Arc<crate::cas::Cas>>,
    /// Poked after a job is enqueued.
    pub wake: Arc<Notify>,
    /// Poked when the event log grew (see `events::spawn_hub`).
    pub events: Arc<Notify>,
    /// Event stream slots.
    pub sse_slots: Arc<Semaphore>,
    /// Event stream timing.
    pub sse: events::SseConfig,
    /// Requests per second per token.
    pub rate_per_second: u32,
    /// Transient `file.progress` events from the stages.
    pub progress: progress::Bus,
    stop: watch::Sender<bool>,
    buckets: Mutex<HashMap<[u8; 32], Bucket>>,
}

impl AppState {
    /// State with the spec defaults.
    pub fn new(
        pool: PgPool,
        tokens: Vec<[u8; 32]>,
        hosts: Vec<String>,
        sync: Arc<dyn SyncPort>,
        health: Arc<HealthProbe>,
    ) -> Self {
        Self {
            pool,
            tokens,
            hosts,
            sync,
            health,
            cas: None,
            wake: Arc::new(Notify::new()),
            events: Arc::new(Notify::new()),
            sse_slots: Arc::new(Semaphore::new(MAX_STREAMS)),
            sse: events::SseConfig::default(),
            rate_per_second: RATE_PER_SECOND,
            progress: progress::Bus::new(),
            stop: watch::channel(false).0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Use `bus` for `file.progress` (the stages publish on the same bus).
    pub fn with_progress(mut self, bus: progress::Bus) -> Self {
        self.progress = bus;
        self
    }

    /// Attach the content store (pack routes).
    pub fn with_cas(mut self, cas: Arc<crate::cas::Cas>) -> Self {
        self.cas = Some(cas);
        self
    }

    /// A receiver that flips to true on shutdown.
    pub fn stop_signal(&self) -> watch::Receiver<bool> {
        self.stop.subscribe()
    }

    /// End every event stream and refuse new work.
    pub fn shutdown(&self) {
        let _ = self.stop.send(true);
    }

    /// Token bucket; `Err(retry_after_ms)` when empty.
    fn take(&self, token: &[u8; 32]) -> Result<(), u64> {
        let rate = f64::from(self.rate_per_second);
        let mut map = self.buckets.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let b = map.entry(*token).or_insert(Bucket {
            tokens: rate,
            at: now,
        });
        b.tokens = (b.tokens + now.duration_since(b.at).as_secs_f64() * rate).min(rate);
        b.at = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            Ok(())
        } else {
            Err((((1.0 - b.tokens) / rate) * 1000.0).ceil() as u64)
        }
    }
}

/// The router: everything goes through one handler.
pub fn router(app: Arc<AppState>) -> Router {
    Router::new().fallback(entry).with_state(app)
}

/// Serve until `shutdown` completes.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Arc<AppState>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let router = router(app);
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

async fn entry(State(app): State<Arc<AppState>>, req: Request) -> Response<Body> {
    let started = Instant::now();
    // Only the template of the route is ever logged, never the path or query.
    let route_text = crate::obs::debug_enabled()
        .then(|| crate::obs::route_template(req.method().as_str(), req.uri().path()));
    let mut resp = match route(&app, req).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    };
    if let Some(route_text) = route_text {
        crate::obs::emit_debug(
            "api.request",
            &crate::obs::Fields {
                route: Some(&route_text),
                status: Some(resp.status().as_u16()),
                duration_ms: Some(i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)),
                ..crate::obs::Fields::default()
            },
        );
    }
    resp.headers_mut().insert(
        "oo-docs-api",
        HeaderValue::from_static(crate::API_VERSION_NUMBER),
    );
    resp
}

/// `OO-Docs-Api-Min: 1.x` -> `(major, minor)`.
fn parse_min(text: &str) -> Option<(u32, u32)> {
    let (a, b) = text.trim().split_once('.')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn method_allowed(req: &Req, allowed: &[&str]) -> Result<(), ApiError> {
    if allowed.iter().any(|m| *m == req.method.as_str()) {
        Ok(())
    } else {
        Err(
            ApiError::new(code::METHOD_NOT_ALLOWED, "method not allowed")
                .header("allow", allowed.join(", ")),
        )
    }
}

async fn route(app: &Arc<AppState>, req: Request) -> Result<Response<Body>, ApiError> {
    let (parts, body) = req.into_parts();
    let headers = parts.headers;
    if headers.contains_key("origin") {
        return Err(ApiError::new(
            code::FORBIDDEN_ORIGIN,
            "requests with an Origin header are refused",
        ));
    }
    let host_ok = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| app.hosts.iter().any(|a| a.eq_ignore_ascii_case(h)));
    if !host_ok {
        return Err(ApiError::new(code::BAD_HOST, "Host is not allowed"));
    }
    if let Some(min) = headers.get("oo-docs-api-min") {
        let ours = parse_min(crate::API_VERSION_NUMBER);
        let theirs = min.to_str().ok().and_then(parse_min);
        match (theirs, ours) {
            (Some(t), Some(o)) if t <= o => {}
            _ => {
                return Err(ApiError::new(
                    code::VERSION_UNSUPPORTED,
                    "server is older than OO-Docs-Api-Min",
                )
                .details(json!({ "server": crate::API_VERSION_NUMBER })));
            }
        }
    }
    let path = parts.uri.path().to_owned();
    if parts.method == Method::GET && path == "/livez" {
        return Ok(Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Body::empty())
            .unwrap_or_default());
    }
    let presented = auth::bearer(headers.get("authorization").and_then(|v| v.to_str().ok()));
    let Some(token) = presented.filter(|t| auth::token_matches(&app.tokens, t)) else {
        return Err(
            ApiError::new(code::UNAUTHENTICATED, "a valid bearer token is required")
                .header("www-authenticate", "Bearer".into()),
        );
    };
    if let Err(ms) = app.take(&auth::token_hash(token)) {
        return Err(ApiError::new(code::RATE_LIMITED, "too many requests")
            .details(json!({ "retry_after_ms": ms }))
            .header("retry-after", ms.div_ceil(1000).max(1).to_string()));
    }
    let body = if matches!(
        parts.method,
        Method::POST | Method::PATCH | Method::DELETE | Method::PUT
    ) {
        axum::body::to_bytes(body, MAX_BODY).await.map_err(|_| {
            ApiError::new(code::PAYLOAD_TOO_LARGE, "request body exceeds 1 MiB")
                .details(json!({ "max_bytes": MAX_BODY }))
        })?
    } else {
        Bytes::new()
    };
    let req = Req {
        method: parts.method,
        path,
        query: Query::parse(parts.uri.query())?,
        headers,
        body,
    };
    dispatch(app, &req).await
}

/// Paths the contract names that this engine build does not serve yet.
fn known_unserved(segs: &[&str]) -> bool {
    matches!(
        segs,
        ["v1", "docs", "entities", ..]
            | ["v1", "docs", "resolve"]
            | ["v1", "docs", "review", ..]
            | ["v1", "docs", "review:decide"]
            | ["v1", "docs", "context-packs", ..]
            | ["v1", "docs", "context-packs:compile"]
            | ["v1", "docs", "shares", ..]
            | ["v1", "docs", "usage"]
            | ["v1", "docs", "ontology", ..]
            | ["v1", "docs", "config", ..]
            | ["v1", "docs", "refs", ..]
    ) || matches!(segs, ["v1", "docs", s] if s.contains(':'))
}

async fn dispatch(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let segs: Vec<&str> = req.path.trim_start_matches('/').split('/').collect();
    match segs.as_slice() {
        ["v1", "docs", "health"] => {
            method_allowed(req, &["GET"])?;
            Ok(http::json(200, &app.health.snapshot().await))
        }
        ["v1", "docs", "drives"] => {
            method_allowed(req, &["GET", "POST"])?;
            if req.method == Method::GET {
                drives::list(app, req).await
            } else {
                drives::create(app, req).await
            }
        }
        ["v1", "docs", "drives", last] => {
            if let Some(id) = last.strip_suffix(":sync") {
                method_allowed(req, &["POST"])?;
                return drives::sync(app, id, req).await;
            }
            if last.contains(':') {
                return Err(ApiError::not_found("route"));
            }
            method_allowed(req, &["GET", "PATCH", "DELETE"])?;
            match req.method {
                Method::GET => drives::get(app, last).await,
                Method::PATCH => drives::patch(app, last, req).await,
                _ => drives::delete(app, last, req).await,
            }
        }
        ["v1", "docs", "drives", id, "files"] => {
            method_allowed(req, &["GET"])?;
            let q = files::FileQuery::parse(id, &req.query, false)?;
            let mut conn = app.pool.acquire().await?;
            ensure_drive(&mut conn, id).await?;
            Ok(http::json(200, &files::list_files(&mut conn, &q).await?))
        }
        ["v1", "docs", "drives", id, "tree"] => {
            method_allowed(req, &["GET"])?;
            let q = files::FileQuery::parse(id, &req.query, true)?;
            let mut conn = app.pool.acquire().await?;
            ensure_drive(&mut conn, id).await?;
            Ok(http::json(200, &files::tree(&mut conn, &q).await?))
        }
        ["v1", "docs", "metrics"] => {
            method_allowed(req, &["GET"])?;
            metrics::get(app).await
        }
        ["v1", "docs", "events"] => {
            method_allowed(req, &["GET"])?;
            events::stream(app, req).await
        }
        ["v1", "docs", "files:retry"] => {
            method_allowed(req, &["POST"])?;
            file_detail::retry(app, req).await
        }
        ["v1", "docs", "files", fid] if !fid.contains(':') => {
            method_allowed(req, &["GET"])?;
            file_detail::detail(app, fid, &req.query).await
        }
        ["v1", "docs", "files", fid, "blocks"] => {
            method_allowed(req, &["GET"])?;
            file_detail::blocks(app, fid, &req.query).await
        }
        ["v1", "docs", "files", fid, "content"] => {
            method_allowed(req, &["GET"])?;
            file_detail::content(app, fid, req).await
        }
        ["v1", "docs", "context-packs:compile"] => {
            method_allowed(req, &["POST"])?;
            packs::compile(app, req).await
        }
        ["v1", "docs", "context-packs", pid] => {
            method_allowed(req, &["GET"])?;
            packs::get(app, pid).await
        }
        ["v1", "docs", "context-packs", pid, "files", rest @ ..] if !rest.is_empty() => {
            method_allowed(req, &["GET"])?;
            // Percent-decode each segment; `+` is a plus in a path.
            let path = rest
                .iter()
                .map(|seg| http::pct_decode(&seg.replace('+', "%2B")))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| ApiError::bad("/path", "malformed percent encoding"))?
                .join("/");
            if path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            {
                return Err(ApiError::not_found("pack file"));
            }
            packs::file(app, pid, &path).await
        }
        ["v1", "docs", "shares"] => {
            method_allowed(req, &["POST"])?;
            shares::create(app, req).await
        }
        ["v1", "docs", "shares", sid] if !sid.contains(':') => {
            method_allowed(req, &["GET"])?;
            shares::get(app, sid).await
        }
        s if known_unserved(s) => Err(ApiError::not_found("route")),
        _ => Err(ApiError::not_found("route")),
    }
}

async fn ensure_drive(conn: &mut sqlx::PgConnection, id: &str) -> Result<(), ApiError> {
    let not_found = || {
        ApiError::new(code::DRIVE_NOT_FOUND, "drive not found").details(json!({ "drive_id": id }))
    };
    if !http::is_uuid(id) {
        return Err(not_found());
    }
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM _oo_docs.drive WHERE id = $1::uuid")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
    match status.as_deref() {
        None | Some("deleted") => Err(not_found()),
        Some(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_floor_parsing() {
        assert_eq!(parse_min("1.0"), Some((1, 0)));
        assert_eq!(parse_min(" 1.3 "), Some((1, 3)));
        assert_eq!(parse_min("1"), None);
        assert_eq!(parse_min("x.y"), None);
    }

    #[test]
    fn unserved_routes_are_the_documented_ones() {
        assert!(known_unserved(&["v1", "docs", "entities", "abc"]));
        assert!(known_unserved(&["v1", "docs", "review:decide"]));
        assert!(known_unserved(&["v1", "docs", "context-packs:compile"]));
        assert!(!known_unserved(&["v1", "docs", "nothing"]));
    }
}
