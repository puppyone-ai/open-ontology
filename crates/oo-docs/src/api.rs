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
//! `:sync` (full, deep, confirm, targeted), `:rebuild` (confirmed full rebuild), files, tree and the event stream.
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

pub mod agent_tokens;
pub mod auth;
pub mod cloud_tokens;
pub mod cursor;
pub mod drives;
pub mod error;
pub mod events;
pub mod file_detail;
pub mod files;
pub mod freshness;
pub mod hints;
pub mod http;
pub mod idem;
pub mod lifecycle;
pub mod links;
pub mod metrics;
pub mod packs;
pub mod profiles;
pub mod progress;
pub mod refs;
pub mod routes_correct;
pub mod routes_entities;
pub mod routes_model;
pub mod routes_review;
pub mod routes_suggest;
pub mod routes_usage;
pub mod search;
pub mod shares;
pub mod structure;

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
    /// Set when the caller presented a share's refs credential instead of the
    /// control token (task-time calls only, see `cred_allowed`).
    pub cred: Option<crate::share::credential::ShareCred>,
    /// Set when the caller presented a cloud proxy token on the cloud entry.
    pub cloud: Option<crate::cloud::CloudPrincipal>,
    /// Set when the caller presented an Agent token (`ooa_`) on the local entry.
    pub agent_token: Option<crate::agent::AgentPrincipal>,
    /// `local`, `cloud:<token id>`, `cred:<credential id>` or `agent:<name>`: the actor of the audit log.
    pub actor: String,
    /// `OO-Docs-Agent` header: who a share is for (audit only), at most 200 characters.
    pub agent: Option<String>,
}

/// Which listener a request came in on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// The loopback control API: control token and `oodr_` credentials only.
    Local,
    /// The cloud entry: `oodc_` tokens only, an allow list of routes, Drive bound (`cloud_guard`).
    Cloud,
}

type SyncFuture<'a> =
    Pin<Box<dyn Future<Output = Result<TargetedReport, IngestError>> + Send + 'a>>;

/// What the API needs from the ingest stage that is source specific.
pub trait SyncPort: Send + Sync + 'static {
    /// Targeted sync of `paths` in a Drive (G11).
    fn targeted(&self, drive_id: &str, paths: Vec<String>) -> SyncFuture<'_>;
    /// Registration probe: can this config be opened read only?
    fn probe(&self, backend: &str, config: &Value) -> Result<(), DocSourceError>;
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

    fn probe(&self, backend: &str, config: &Value) -> Result<(), DocSourceError> {
        let drive = DriveRef {
            id: String::new(),
            backend: backend.to_owned(),
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
    /// The engine's model (`GET/PUT /config/model`); rules only until configured.
    pub model: Arc<crate::extract::llm::config::ModelRuntime>,
    /// Per Drive memory of the event notifications seen (`:events`, B-S11-02).
    pub hint_dedupe: Mutex<HashMap<String, oo_connectors::objectstore_events::Deduper>>,
    /// Accepted `Host` values of the cloud entry (empty: the cloud entry refuses everything).
    pub cloud_hosts: Vec<String>,
    /// Where the Agent DB of the projection is (D83); `None`: the link routes answer "unbound".
    pub agent_db: Option<crate::publish::agentdb::AgentDbConfig>,
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
            model: crate::extract::llm::config::ModelRuntime::disabled(),
            hint_dedupe: Mutex::new(HashMap::new()),
            cloud_hosts: Vec::new(),
            agent_db: None,
            stop: watch::channel(false).0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Use `bus` for `file.progress` (the stages publish on the same bus).
    pub fn with_progress(mut self, bus: progress::Bus) -> Self {
        self.progress = bus;
        self
    }

    /// Use `model` (the same runtime the stage handlers call).
    pub fn with_model(mut self, model: Arc<crate::extract::llm::config::ModelRuntime>) -> Self {
        self.model = model;
        self
    }

    /// The `Host` values the cloud entry answers to (a public name and port).
    pub fn with_cloud_hosts(mut self, hosts: Vec<String>) -> Self {
        self.cloud_hosts = hosts.into_iter().map(|h| h.to_ascii_lowercase()).collect();
        self
    }

    /// Where the projection finds its Agent DB (secret reference and secrets directory).
    pub fn with_agent_db(mut self, cfg: crate::publish::agentdb::AgentDbConfig) -> Self {
        self.agent_db = Some(cfg);
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

/// The cloud entry (10 11.3, B-S12-01): the same handler chain on a second listener, with cloud tokens only.
pub fn cloud_router(app: Arc<AppState>) -> Router {
    Router::new().fallback(cloud_entry).with_state(app)
}

/// Serve the cloud entry until `shutdown` completes.
pub async fn serve_cloud(
    listener: tokio::net::TcpListener,
    app: Arc<AppState>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(listener, cloud_router(app))
        .with_graceful_shutdown(shutdown)
        .await
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
    entry_for(app, req, Entry::Local).await
}

async fn cloud_entry(State(app): State<Arc<AppState>>, req: Request) -> Response<Body> {
    entry_for(app, req, Entry::Cloud).await
}

async fn entry_for(app: Arc<AppState>, req: Request, which: Entry) -> Response<Body> {
    let started = Instant::now();
    // Only the template of the route is ever logged, never the path or query.
    let route_text = crate::obs::debug_enabled()
        .then(|| crate::obs::route_template(req.method().as_str(), req.uri().path()));
    let mut resp = match route(&app, req, which).await {
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

async fn route(
    app: &Arc<AppState>,
    req: Request,
    which: Entry,
) -> Result<Response<Body>, ApiError> {
    let (parts, body) = req.into_parts();
    let headers = parts.headers;
    if headers.contains_key("origin") {
        return Err(ApiError::new(
            code::FORBIDDEN_ORIGIN,
            "requests with an Origin header are refused",
        ));
    }
    let allowed_hosts = match which {
        Entry::Local => &app.hosts,
        Entry::Cloud => &app.cloud_hosts,
    };
    let host_ok = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| allowed_hosts.iter().any(|a| a.eq_ignore_ascii_case(h)));
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
    let mut cred = None;
    let mut cloud = None;
    let mut agent_token = None;
    let mut actor = "local".to_owned();
    let token = match (which, presented) {
        // The cloud entry takes cloud tokens and nothing else; the local entry never takes one.
        (Entry::Cloud, Some(t)) if t.starts_with(crate::cloud::PREFIX) => {
            let mut conn = app.pool.acquire().await?;
            let p = crate::cloud::authenticate(&mut conn, t).await?;
            actor = p.actor();
            cloud = Some(p);
            t
        }
        (Entry::Local, Some(t)) if t.starts_with(crate::agent::PREFIX) => {
            let mut conn = app.pool.acquire().await?;
            let p = crate::agent::authenticate(&mut conn, t, true).await?;
            actor = p.actor();
            agent_token = Some(p);
            t
        }
        (Entry::Local, Some(t)) if t.starts_with("oodr_") => {
            let mut conn = app.pool.acquire().await?;
            let c = crate::share::credential::authenticate(&mut conn, t).await?;
            actor = format!("cred:{}", c.cred_id);
            cred = Some(c);
            t
        }
        (Entry::Local, Some(t)) if auth::token_matches(&app.tokens, t) => t,
        _ => {
            return Err(
                ApiError::new(code::UNAUTHENTICATED, "a valid bearer token is required")
                    .header("www-authenticate", "Bearer".into()),
            );
        }
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
    let client_header = headers
        .get("oo-docs-client")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let headers_agent = headers
        .get("oo-docs-agent")
        .and_then(|v| v.to_str().ok())
        .filter(|a| !a.is_empty() && a.chars().count() <= 200 && !a.chars().any(char::is_control))
        .map(str::to_owned);
    let req = Req {
        method: parts.method,
        path,
        query: Query::parse(parts.uri.query())?,
        headers,
        body,
        cred,
        cloud,
        agent_token,
        actor,
        agent: headers_agent,
    };
    if let Some(p) = &req.cloud {
        cloud_guard(app, p, &req).await?;
    }
    if let Some(p) = &req.agent_token {
        // Every call of an Agent token is one audit row, and so is every refusal.
        let verdict = agent_tokens::guard(app, p, &req).await;
        let mut conn = app.pool.acquire().await?;
        let (action, drive) = match &verdict {
            Ok(d) => (crate::audit::AGENT_READ, d.clone()),
            Err(_) => (crate::audit::AGENT_DENIED, None),
        };
        crate::audit::record(
            &mut conn,
            &crate::audit::Entry {
                action,
                actor: &req.actor,
                tenant: None,
                drive_id: drive.as_deref(),
                share_id: None,
                agent: Some(&p.name),
                detail: json!({
                    "route": format!("{} {}", req.method, agent_tokens::audit_route(&req.path)),
                    "token_id": p.id,
                    "reason": verdict.as_ref().err().and_then(|e| e.details["reason"].as_str()),
                }),
            },
        )
        .await?;
        drop(conn);
        verdict?;
    }
    let who = crate::actor::derive(
        client_header.as_deref(),
        req.agent.as_deref(),
        req.agent_token.as_ref().map(|p| p.name.as_str()),
        req.cloud.is_some(),
    );
    crate::actor::scope(who, async {
        if req.method != Method::GET {
            note_touched(&req);
        }
        dispatch(app, &req).await
    })
    .await
}

/// Tell the actor ledger which Drive or file a changing request names, so the
/// events that background workers append for it later carry its actor.
fn note_touched(req: &Req) {
    let id_like = |s: &str| s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    for seg in req.path.trim_start_matches('/').split('/') {
        let id = seg.split(':').next().unwrap_or("");
        if id_like(id) {
            crate::actor::note(id);
        }
    }
    if let Some(d) = req.query.get("drive_id").filter(|d| id_like(d)) {
        crate::actor::note(&d.to_ascii_lowercase());
    }
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
            | ["v1", "docs", "config", ..]
            | ["v1", "docs", "refs", ..]
    ) || matches!(segs, ["v1", "docs", s] if s.contains(':'))
}

/// What a refs credential may call: its own share's check, updates and read,
/// the originals it cites, and entity detail (restricted to the share's scope).
fn cred_allowed(
    segs: &[&str],
    method: &Method,
    cred: &crate::share::credential::ShareCred,
) -> bool {
    match (method, segs) {
        (&Method::GET, ["v1", "docs", "refs", _]) => true,
        (&Method::POST, ["v1", "docs", "shares", last]) => {
            last.strip_suffix(":check") == Some(cred.share_id.as_str())
        }
        (&Method::GET, ["v1", "docs", "shares", sid, "updates"]) => *sid == cred.share_id,
        (&Method::GET, ["v1", "docs", "shares", sid]) => *sid == cred.share_id,
        (&Method::GET, ["v1", "docs", "entities", _]) => true,
        (&Method::POST, ["v1", "docs", "suggestions"]) => true,
        _ => false,
    }
}

/// What a cloud token may call, and on which Drives (B-S12-01). Everything else on the cloud entry is 403,
/// including every route that is not about shares, packs and their files. The Drive of the call (a body field, a
/// share, a pack) must be in the token's set.
async fn cloud_guard(
    app: &Arc<AppState>,
    p: &crate::cloud::CloudPrincipal,
    req: &Req,
) -> Result<(), ApiError> {
    let denied = |why: &'static str| {
        ApiError::new(
            code::SHARE_REFS_OUT_OF_SCOPE,
            "a cloud token cannot do this",
        )
        .details(json!({"reason": why}))
    };
    let segs: Vec<&str> = req.path.trim_start_matches('/').split('/').collect();
    let mut conn = app.pool.acquire().await?;
    let drive_of_share = |sid: String| async move {
        let mut c = app.pool.acquire().await?;
        let d: Option<String> =
            sqlx::query_scalar("SELECT drive_id::text FROM _oo_docs.share WHERE id::text = $1")
                .bind(sid)
                .fetch_optional(&mut *c)
                .await?;
        Ok::<_, ApiError>(d)
    };
    let drive: Option<String> = match (&req.method, segs.as_slice()) {
        (&Method::POST, ["v1", "docs", "shares"]) => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            body["compile"]["drive_id"].as_str().map(str::to_owned)
        }
        (&Method::POST, ["v1", "docs", "context-packs:compile"]) => {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            body["drive_id"].as_str().map(str::to_owned)
        }
        (&Method::GET, ["v1", "docs", "shares", sid]) => drive_of_share((*sid).to_owned()).await?,
        (&Method::POST, ["v1", "docs", "shares", last]) if last.ends_with(":revoke") => {
            drive_of_share(last.trim_end_matches(":revoke").to_owned()).await?
        }
        (&Method::GET, ["v1", "docs", "context-packs", pid, ..]) => {
            sqlx::query_scalar(
                "SELECT drive_id::text FROM _oo_docs.context_pack WHERE id::text = $1",
            )
            .bind(*pid)
            .fetch_optional(&mut *conn)
            .await?
        }
        _ => return Err(denied("route")),
    };
    match drive {
        Some(d) if p.allows_drive(&d) => Ok(()),
        Some(_) => Err(denied("drive")),
        None if req.method == Method::GET => Err(ApiError::new(code::NOT_FOUND, "not found")),
        None => Err(denied("drive")),
    }
}

async fn dispatch(app: &Arc<AppState>, req: &Req) -> Result<Response<Body>, ApiError> {
    let segs: Vec<&str> = req.path.trim_start_matches('/').split('/').collect();
    if let Some(c) = &req.cred
        && !cred_allowed(&segs, &req.method, c)
    {
        return Err(ApiError::new(
            code::SHARE_REFS_OUT_OF_SCOPE,
            "a refs credential cannot call this route",
        ));
    }
    match segs.as_slice() {
        ["v1", "docs", "refs", b3] => {
            method_allowed(req, &["GET"])?;
            let cred = req.cred.as_ref().ok_or_else(|| {
                ApiError::new(
                    code::SHARE_REFS_CREDENTIAL_INVALID,
                    "refs need a share credential",
                )
            })?;
            refs::get(app, b3, req, cred).await
        }
        ["v1", "docs", "share-groups", last] if last.ends_with(":refresh") => {
            method_allowed(req, &["POST"])?;
            shares::group_refresh(app, last.trim_end_matches(":refresh"), req).await
        }
        ["v1", "docs", "share-groups", gid] if !gid.contains(':') => {
            method_allowed(req, &["GET"])?;
            shares::group_get(app, gid).await
        }
        ["v1", "docs", "cloud-tokens"] => {
            method_allowed(req, &["POST"])?;
            cloud_tokens::issue(app, req).await
        }
        ["v1", "docs", "cloud-tokens", id] => {
            method_allowed(req, &["DELETE"])?;
            cloud_tokens::revoke(app, id, req).await
        }
        ["v1", "docs", "agent-tokens"] => {
            method_allowed(req, &["GET", "POST"])?;
            if req.method == Method::GET {
                agent_tokens::list(app, req).await
            } else {
                agent_tokens::issue(app, req).await
            }
        }
        ["v1", "docs", "agent-tokens:introspect"] => {
            method_allowed(req, &["POST"])?;
            agent_tokens::introspect(app, req).await
        }
        ["v1", "docs", "agent-tokens", "self"] => {
            method_allowed(req, &["GET"])?;
            match &req.agent_token {
                Some(p) => agent_tokens::myself(app, p).await,
                None => Err(ApiError::new(
                    code::NOT_FOUND,
                    "only an agent token has a self",
                )),
            }
        }
        ["v1", "docs", "agent-tokens", id] => {
            method_allowed(req, &["DELETE"])?;
            agent_tokens::revoke(app, id, req).await
        }
        ["v1", "docs", "audit", "agents"] => {
            method_allowed(req, &["GET"])?;
            agent_tokens::audit(app, req).await
        }
        ["v1", "docs", "links"] => {
            method_allowed(req, &["GET"])?;
            links::list(app, req).await
        }
        ["v1", "docs", "links:decide"] => {
            method_allowed(req, &["POST"])?;
            links::decide(app, req).await
        }
        ["v1", "docs", "links:project"] => {
            method_allowed(req, &["POST"])?;
            links::project(app, req).await
        }
        ["v1", "docs", "link-rules"] => {
            method_allowed(req, &["GET", "PUT", "DELETE"])?;
            match req.method {
                Method::GET => links::rules_list(app, req).await,
                Method::PUT => links::rules_put(app, req).await,
                _ => links::rules_delete(app, req).await,
            }
        }
        ["v1", "docs", "agent-db-binding"] => {
            method_allowed(req, &["GET", "PUT", "DELETE"])?;
            match req.method {
                Method::GET => links::binding_get(app).await,
                Method::PUT => links::binding_put(app, req).await,
                _ => links::binding_delete(app).await,
            }
        }
        ["v1", "docs", "audit", "shares"] => {
            method_allowed(req, &["GET"])?;
            cloud_tokens::shares(app, req).await
        }
        ["v1", "docs", "audit", "verify"] => {
            method_allowed(req, &["GET"])?;
            cloud_tokens::verify(app).await
        }
        ["v1", "docs", "health"] => {
            method_allowed(req, &["GET"])?;
            let mut snap = app.health.snapshot().await;
            // The model component follows the runtime, not the static default.
            if let Some(m) = snap["components"]
                .get_mut("model")
                .and_then(Value::as_object_mut)
            {
                m.insert("status".into(), json!(app.model.status()));
                if let Some(code) = app
                    .model
                    .last_error()
                    .filter(|_| app.model.status() == "unavailable")
                {
                    m.insert("code".into(), json!(code));
                }
            }
            Ok(http::json(200, &snap))
        }
        ["v1", "docs", "config", "model"] => {
            method_allowed(req, &["GET", "PUT"])?;
            if req.method == Method::GET {
                routes_model::get(app)
            } else {
                routes_model::put(app, req).await
            }
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
            if let Some(id) = last.strip_suffix(":events") {
                method_allowed(req, &["POST"])?;
                return hints::post(app, id, req).await;
            }
            if let Some(id) = last.strip_suffix(":rebuild") {
                method_allowed(req, &["POST"])?;
                return drives::rebuild(app, id, req).await;
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
        ["v1", "docs", "drives", id, "structure"] => {
            method_allowed(req, &["GET"])?;
            structure::get(app, id, req).await
        }
        ["v1", "docs", "drives", id, "structure:induce"] => {
            method_allowed(req, &["POST"])?;
            structure::induce(app, id).await
        }
        ["v1", "docs", "drives", id, "structure:decide"] => {
            method_allowed(req, &["POST"])?;
            structure::decide(app, id, req).await
        }
        [
            "v1",
            "docs",
            "drives",
            id,
            "structure",
            "classes",
            class_key,
            "instances",
        ] => {
            method_allowed(req, &["GET"])?;
            structure::instances(app, id, class_key, req).await
        }
        ["v1", "docs", "drives", id, "structure", "yaml"] => {
            method_allowed(req, &["GET"])?;
            structure::yaml(app, id, req).await
        }
        ["v1", "docs", "drives", id, "profile-overrides"] => {
            method_allowed(req, &["GET", "PUT"])?;
            profiles::handle(app, id, req).await
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
        ["v1", "docs", "events:publish"] => {
            method_allowed(req, &["POST"])?;
            events::publish(app, req).await
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
        ["v1", "docs", "entities"] => {
            method_allowed(req, &["GET"])?;
            routes_entities::list(app, req).await
        }
        ["v1", "docs", "search"] => {
            method_allowed(req, &["GET"])?;
            search::search(app, req).await
        }
        ["v1", "docs", "ontology", "versions"] => {
            method_allowed(req, &["GET"])?;
            routes_entities::versions(app, req).await
        }
        ["v1", "docs", "constraints"] => {
            method_allowed(req, &["GET"])?;
            routes_review::constraints(app).await
        }
        ["v1", "docs", "entities:merge"] => {
            method_allowed(req, &["POST"])?;
            routes_correct::merge(app, req).await
        }
        ["v1", "docs", "facts", last] if last.ends_with(":correct") => {
            method_allowed(req, &["POST"])?;
            routes_correct::correct_fact(app, last.trim_end_matches(":correct"), req).await
        }
        ["v1", "docs", "usage"] => {
            method_allowed(req, &["GET"])?;
            routes_usage::get(app, req).await
        }
        ["v1", "docs", "resolve"] => {
            method_allowed(req, &["POST"])?;
            routes_entities::resolve(app, req).await
        }
        // `eid` is an id or a key (`person:<24 hex>`, which contains a colon).
        ["v1", "docs", "entities", eid] if eid.ends_with(":split") => {
            method_allowed(req, &["POST"])?;
            routes_correct::split(app, eid.trim_end_matches(":split"), req).await
        }
        ["v1", "docs", "entities", eid] => {
            method_allowed(req, &["GET"])?;
            routes_entities::detail(app, eid, req).await
        }
        ["v1", "docs", "review"] => {
            method_allowed(req, &["GET"])?;
            routes_review::list(app, req).await
        }
        ["v1", "docs", "review:decide"] => {
            method_allowed(req, &["POST"])?;
            routes_review::decide(app, req).await
        }
        ["v1", "docs", "suggestions"] => {
            method_allowed(req, &["GET", "POST"])?;
            if req.method == Method::GET {
                routes_suggest::list(app, req).await
            } else {
                routes_suggest::create(app, req).await
            }
        }
        ["v1", "docs", "suggestions:digest"] => {
            method_allowed(req, &["GET"])?;
            routes_suggest::digest(app, req).await
        }
        ["v1", "docs", "suggestions", last] if last.ends_with(":decide") => {
            method_allowed(req, &["POST"])?;
            routes_suggest::decide(app, last.trim_end_matches(":decide"), req).await
        }
        ["v1", "docs", "suggestions", id, "diff"] => {
            method_allowed(req, &["GET"])?;
            routes_suggest::diff(app, id).await
        }
        ["v1", "docs", "suggestions", id] if !id.contains(':') => {
            method_allowed(req, &["GET"])?;
            routes_suggest::get(app, id).await
        }
        ["v1", "docs", "shares"] => {
            method_allowed(req, &["GET", "POST"])?;
            if req.method == Method::GET {
                shares::list(app, req).await
            } else {
                shares::create(app, req).await
            }
        }
        ["v1", "docs", "shares", last] if last.ends_with(":refresh") => {
            method_allowed(req, &["POST"])?;
            shares::refresh(app, last.trim_end_matches(":refresh"), req).await
        }
        ["v1", "docs", "shares", last] if last.ends_with(":revoke") => {
            method_allowed(req, &["POST"])?;
            shares::revoke(app, last.trim_end_matches(":revoke"), req).await
        }
        ["v1", "docs", "shares", last] if last.ends_with(":check") => {
            method_allowed(req, &["POST"])?;
            shares::check(app, last.trim_end_matches(":check"), req).await
        }
        ["v1", "docs", "shares", sid, "updates"] => {
            method_allowed(req, &["GET"])?;
            shares::updates(app, sid, req).await
        }
        ["v1", "docs", "shares", sid] if !sid.contains(':') => {
            method_allowed(req, &["GET", "PATCH"])?;
            if req.method == Method::PATCH {
                shares::patch(app, sid, req).await
            } else {
                shares::get(app, sid).await
            }
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
        assert!(known_unserved(&["v1", "docs", "context-packs:compile"]));
        assert!(!known_unserved(&["v1", "docs", "nothing"]));
    }
}
