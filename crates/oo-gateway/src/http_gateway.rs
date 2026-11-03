//! HTTP router for loopback development and TLS external clients.
//!
//! Caller headers are not authority. The process derives project, principal,
//! role, and scope from the hashed bearer token, then calls the accepted
//! `lookup`. Session tokens fail that binding check because this process is
//! not an egress proxy. A verified client certificate supplies a workload URI
//! and does not select a role.

use std::collections::HashMap;
use std::fs;
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes, HttpBody};
use axum::extract::connect_info::{ConnectInfo, Connected};
use axum::extract::{RawQuery, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::Response;
use axum::routing::{get, post};
use axum::serve::{IncomingStream, Listener};
use axum::{Extension, Router};
use rustls::RootCertStore;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::server::{ServerConfig, WebPkiClientVerifier};
use serde_json::{Map as JsonMap, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_postgres::config::SslMode;
use tokio_rustls::TlsAcceptor;
use tower::limit::GlobalConcurrencyLimitLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use zeroize::Zeroize;

#[path = "auth_lifecycle.rs"]
mod auth_lifecycle;
#[path = "../../../tools/gateway-token-store/src/lifecycle.rs"]
pub mod token_lifecycle;

pub use crate::accepted_token_store::{CurrentTuple, Endpoint, IssueGrant, TokenKind};
use crate::accepted_token_store::{
    DeclaredTransport, LookupCache, LookupRequest, lookup, rustls_connector, token_hash,
};
use crate::http_error::{ApiCode, ApiError, JSON_MEDIA_TYPE};
use crate::pack_catalog::{PackCatalog, PackMetadata};
use oo_core::api::ErrorCode;

const READ_TOKEN: &str = "SELECT token_kind, project_id, principal_id, role_name, claims_canonical, policy_version, knowledge_version, audience_url, sandbox_id, session_id, issued_at_ms, idle_deadline_ms, absolute_deadline_ms, revocation_floor FROM oo_token_control.token_record WHERE token_sha256 = $1";
const TLS_FACT: &str =
    "SELECT ssl, version FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()";
const TUPLE_CACHE_TTL: Duration = Duration::from_millis(5_000);
const RUNTIME_USER: &str = "oo_token_runtime";
const ADMIN_USER: &str = "oo_token_admin";
pub const PRODUCTION_BODY_LIMIT: usize = 1024 * 1024;
const WORKLOAD_PREFIX: &str = "urn:oo:workload:";
const SAN_OID: &[u8] = &[0x55, 0x1D, 0x11];
const FORGED_CERT_HEADERS: &[&str] = &[
    "x-forwarded-client-cert",
    "x-client-cert",
    "ssl-client-cert",
    "x-ssl-client-cert",
    "x-ssl-client-s-dn",
    "x-forwarded-tls-client-cert",
    "x-oo-workload",
    "x-oo-peer",
];

enum Transport {
    Loopback,
    Tls {
        acceptor: TlsAcceptor,
        handshake_timeout: Duration,
    },
}

struct TupleEntry {
    tuple: CurrentTuple,
    endpoint: Endpoint,
    expires: Instant,
}

struct Inner {
    lifecycle: Option<auth_lifecycle::Lifecycle>,
    audience: String,
    endpoint_path: PathBuf,
    password: String,
    ca_pem: Vec<u8>,
    catalog: Arc<PackCatalog>,
    reloading_packs: Option<crate::pack_reload::ReloadingPacks>,
    cache: LookupCache,
    tuples: Mutex<HashMap<String, TupleEntry>>,
    body_limit: usize,
    request_timeout: Duration,
    global_concurrency: usize,
    probe: Option<Arc<ProbeHooks>>,
    log_probe_paths: bool,
    transport: Transport,
    plane: Option<Arc<crate::public_surface::Plane>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

pub struct ProbeHooks {
    pub hold_rx: Mutex<Option<oneshot::Receiver<()>>>,
    pub hold_entered: AtomicBool,
    pub health_started: AtomicUsize,
}

struct Identity {
    tuple: CurrentTuple,
    scope: &'static str,
    auth_remaining_ms: u128,
    auth_cache_hit: bool,
}

/// Authenticated public caller. The tuple comes from the token store, not from headers.
pub(crate) struct PublicCaller {
    pub tuple: CurrentTuple,
    pub scope: &'static str,
    /// Milliseconds left on the token-store positive cache. Zero when absent.
    pub auth_remaining_ms: u128,
    /// True when this lookup did not read the token database.
    pub auth_cache_hit: bool,
}

pub(crate) type PublicPeer = Option<Extension<ConnectInfo<TlsPeer>>>;

pub struct OperatorLoad {
    pub state: AppState,
    pub listen: SocketAddr,
}

pub fn router(state: AppState) -> Router {
    assemble(state)
}

impl AppState {
    #[cfg(test)]
    pub(crate) fn for_owned_binding_test(
        catalog: PackCatalog,
        endpoint_path: PathBuf,
        password: String,
        ca_pem: Vec<u8>,
        plane: Arc<crate::public_surface::Plane>,
    ) -> Self {
        let mut state = Self::local(catalog, PRODUCTION_BODY_LIMIT, None, false);
        let inner = Arc::get_mut(&mut state.inner).expect("new state");
        inner.endpoint_path = endpoint_path;
        inner.password = password;
        inner.ca_pem = ca_pem;
        inner.plane = Some(plane);
        inner.global_concurrency = 4;
        inner.request_timeout = Duration::from_secs(10);
        state
    }

    pub fn for_probe(catalog: PackCatalog, body_limit: usize, hooks: Arc<ProbeHooks>) -> Self {
        Self::local(catalog, body_limit, Some(hooks), true)
    }

    /// Loopback limits with no probe routes. Production `load_operator_dir` also leaves the probe unset.
    pub fn for_limits(catalog: PackCatalog, body_limit: usize) -> Self {
        Self::local(catalog, body_limit, None, false)
    }

    fn local(
        catalog: PackCatalog,
        body_limit: usize,
        probe: Option<Arc<ProbeHooks>>,
        log_probe_paths: bool,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                audience: "https://gateway.loopback.invalid/v1".to_owned(),
                lifecycle: None,
                endpoint_path: PathBuf::from("endpoint.json"),
                password: String::new(),
                ca_pem: Vec::new(),
                catalog: Arc::new(catalog),
                reloading_packs: None,
                cache: LookupCache::new(),
                tuples: Mutex::new(HashMap::new()),
                body_limit,
                request_timeout: Duration::from_secs(5),
                global_concurrency: 1,
                probe,
                log_probe_paths,
                transport: Transport::Loopback,
                plane: None,
            }),
        }
    }

    pub(crate) fn pack_catalog(&self) -> Arc<PackCatalog> {
        REQUEST_PACKS
            .try_with(Arc::clone)
            .unwrap_or_else(|_| Arc::clone(&self.inner.catalog))
    }

    pub(crate) fn data_plane(&self) -> Option<Arc<crate::public_surface::Plane>> {
        self.inner.plane.clone()
    }
}

pub fn load_operator_dir(dir: &Path) -> Result<OperatorLoad, &'static str> {
    let text = fs::read_to_string(dir.join("config.json")).map_err(|_| "config")?;
    let value: Value = serde_json::from_str(&text).map_err(|_| "config")?;
    let object = value.as_object().ok_or("config")?;
    let deployment_mode = oo_core::config::DeploymentMode::from_installed(&value)?;
    let data_limits = deployment_mode.query_limits(&value)?;
    let transport_name = transport_name(object)?;
    let tls = transport_name == "tls";
    for key in object.keys() {
        if !allowed_config_key(tls, key) {
            return Err("config");
        }
    }
    let listen = required_text(object, "listen")?
        .parse::<SocketAddr>()
        .map_err(|_| "listen")?;
    if !tls && listen.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err("listen");
    }
    let audience = required_text(object, "audience")?.to_owned();
    if required_text(object, "runtime_user")? != RUNTIME_USER {
        return Err("runtime_user");
    }
    let password_file = one_segment(required_text(object, "password_file")?)?;
    let ca_file = one_segment(required_text(object, "ca_file")?)?;
    let endpoint_file = one_segment(required_text(object, "endpoint_file")?)?;
    let pack_dir = one_segment(required_text(object, "pack_dir")?)?;
    let body_limit = usize_field(object, "body_limit")?;
    // HTTP also covers authentication and metadata; the data plane separately caps SQL.
    let timeout_ms = u64_field(object, "request_timeout_ms")?;
    let mut global_concurrency = usize_field(object, "global_concurrency")?;
    if deployment_mode == oo_core::config::DeploymentMode::AllInOne {
        global_concurrency = global_concurrency.min(data_limits.0);
    }
    if body_limit == 0 || timeout_ms == 0 || global_concurrency == 0 {
        return Err("limits");
    }
    let loaded_tls = if tls {
        Some(load_tls_acceptor(dir, object)?)
    } else {
        None
    };
    let mut password = fs::read_to_string(dir.join(password_file)).map_err(|_| "password")?;
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    if password.is_empty() || password.contains('\0') {
        password.zeroize();
        return Err("password");
    }
    let ca_pem = match fs::read(dir.join(ca_file)) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => {
            password.zeroize();
            return Err("ca");
        }
    };
    let pack_ceilings: oo_builder::RolePackCeilings =
        if let Some(name) = object.get("pack_ceilings_file") {
            let name = one_segment(name.as_str().ok_or("pack_limits")?)?;
            serde_json::from_slice(&fs::read(dir.join(name)).map_err(|_| "pack_limits")?)
                .map_err(|_| "pack_limits")?
        } else {
            Default::default()
        };
    let reloading_packs =
        match crate::pack_reload::ReloadingPacks::load(dir.join(pack_dir), pack_ceilings) {
            Ok(packs) => packs,
            Err(_) => {
                password.zeroize();
                return Err("packs");
            }
        };
    let catalog = reloading_packs.snapshot().map_err(|_| "packs")?;
    if let Err(reason) = read_endpoint(&dir.join(endpoint_file)) {
        password.zeroize();
        return Err(reason);
    }
    let plane = match crate::public_surface::load_plane(dir, object) {
        Ok(plane) => plane,
        Err(reason) => {
            password.zeroize();
            return Err(reason);
        }
    };
    let lifecycle = if let Some(name) = object.get("token_admin_password_file") {
        if !tls {
            return Err("lifecycle_requires_tls");
        }
        let name = one_segment(name.as_str().ok_or("token_admin_password_file")?)?;
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        if fs::metadata(&path)
            .map_err(|_| "admin_password")?
            .permissions()
            .mode()
            & 0o777
            != 0o600
        {
            return Err("admin_password_permissions");
        }
        let secret =
            zeroize::Zeroizing::new(fs::read_to_string(path).map_err(|_| "admin_password")?);
        let workload = required_text(object, "token_admin_workload")?;
        if !workload.starts_with(WORKLOAD_PREFIX) {
            return Err("admin_workload");
        }
        Some(auth_lifecycle::Lifecycle::new(
            token_lifecycle::Control::new(
                read_endpoint(&dir.join(endpoint_file))?,
                secret.trim_end().into(),
                ca_pem.clone(),
            ),
            workload.into(),
        ))
    } else {
        None
    };
    let transport = match loaded_tls {
        Some((acceptor, handshake_timeout)) => Transport::Tls {
            acceptor,
            handshake_timeout,
        },
        None => Transport::Loopback,
    };
    Ok(OperatorLoad {
        state: AppState {
            inner: Arc::new(Inner {
                audience,
                lifecycle,
                endpoint_path: dir.join(endpoint_file),
                password,
                ca_pem,
                catalog,
                reloading_packs: Some(reloading_packs),
                cache: LookupCache::new(),
                tuples: Mutex::new(HashMap::new()),
                body_limit,
                request_timeout: Duration::from_millis(timeout_ms),
                global_concurrency,
                probe: None,
                log_probe_paths: false,
                transport,
                plane,
            }),
        },
        listen,
    })
}

pub async fn serve(
    state: AppState,
    listen: SocketAddr,
    addr_path: &Path,
) -> Result<(), &'static str> {
    if let Some(plane) = state.data_plane() {
        plane.warm_planning_catalogs().await;
    }
    let tls = match &state.inner.transport {
        Transport::Loopback => None,
        Transport::Tls {
            acceptor,
            handshake_timeout,
        } => Some((acceptor.clone(), *handshake_timeout)),
    };
    if let Some((acceptor, handshake_timeout)) = tls {
        return serve_tls(state, listen, addr_path, acceptor, handshake_timeout).await;
    }
    if listen.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err("listen");
    }
    let listener = TcpListener::bind(listen).await.map_err(|_| "bind")?;
    let bound = listener.local_addr().map_err(|_| "bind")?;
    if bound.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err("listen");
    }
    fs::write(addr_path, format!("{bound}\n")).map_err(|_| "addr")?;
    axum::serve(listener, router(state))
        .await
        .map_err(|_| "serve")
}

async fn serve_tls(
    state: AppState,
    listen: SocketAddr,
    addr_path: &Path,
    acceptor: TlsAcceptor,
    handshake_timeout: Duration,
) -> Result<(), &'static str> {
    let tcp = TcpListener::bind(listen).await.map_err(|_| "bind")?;
    let bound = tcp.local_addr().map_err(|_| "bind")?;
    if bound.ip() != listen.ip() {
        return Err("listen");
    }
    fs::write(addr_path, format!("{bound}\n")).map_err(|_| "addr")?;
    let listener = TlsListener {
        tcp,
        acceptor,
        handshake_timeout,
    };
    let make = router(state).into_make_service_with_connect_info::<TlsPeer>();
    axum::serve(listener, make).await.map_err(|_| "serve")
}

fn assemble(state: AppState) -> Router {
    let timeout = state.inner.request_timeout;
    let body_limit = state.inner.body_limit;
    let concurrency = state.inner.global_concurrency;
    let probe = state.inner.probe.is_some();
    let mut router = Router::new();
    if probe {
        // The short deadline applies to `/slow` only. Routes added after this layer are outside it.
        router = router
            .route("/slow", get(slow))
            .route_layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_millis(150),
            ));
    }
    router = router
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/v1/session/current", get(session_current))
        .route("/v1/pack/metadata", get(pack_metadata))
        .route("/v1/sql", post(sql_unsupported));
    router = crate::public_surface::mount(router);
    router = auth_lifecycle::mount(router);
    router = router.route_layer(TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        timeout,
    ));
    if probe {
        router = router.route("/hold", get(hold));
    }
    let router = router
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(RequestBodyLimitLayer::new(body_limit));
    let router = if probe {
        router.layer(GlobalConcurrencyLimitLayer::new(concurrency))
    } else {
        let data = Arc::new(tokio::sync::Semaphore::new(concurrency));
        let control = Arc::new(tokio::sync::Semaphore::new(4));
        router.layer(axum::middleware::from_fn(
            move |request: Request, next: Next| {
                let capacity = if control_path(request.uri().path()) {
                    Arc::clone(&control)
                } else {
                    Arc::clone(&data)
                };
                async move {
                    let Ok(_permit) = capacity.try_acquire_owned() else {
                        return crate::public_surface::error_response(
                            ErrorCode::Overloaded,
                            "HTTP capacity is full; request was not queued",
                            None,
                            None,
                        );
                    };
                    next.run(request).await
                }
            },
        ))
    };
    router
        .layer(from_fn_with_state(state.clone(), access_log))
        .with_state(state)
}

fn control_path(path: &str) -> bool {
    matches!(path, "/health" | "/ready" | "/v1/operations")
        || path.starts_with("/v1/queries/")
        || path.starts_with("/v1/operations/")
}

impl AppState {
    pub(crate) fn query_request_budget(&self) -> Duration {
        self.inner
            .request_timeout
            .saturating_sub(Duration::from_millis(100))
    }
}

tokio::task_local! {
    static REQUEST_PACKS: Arc<PackCatalog>;
}

async fn access_log(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path_raw = request.uri().path().to_owned();
    let token_admin = path_raw == "/internal/v1/tokens"
        && state.inner.lifecycle.is_some()
        && matches!(state.inner.transport, Transport::Tls { .. });
    if (path_raw == "/internal" || path_raw.starts_with("/internal/")) && !token_admin {
        eprintln!("method={method} path=internal status=403");
        return json_error(ApiError::new(ApiCode::Forbidden));
    }
    if forged_certificate_header(request.headers()) {
        let path = logged_path(&path_raw, state.inner.log_probe_paths);
        eprintln!("method={method} path={path} status=403");
        return json_error(ApiError::new(ApiCode::Forbidden));
    }
    let path = logged_path(&path_raw, state.inner.log_probe_paths);
    let uses_pack = matches!(
        path_raw.as_str(),
        "/v1/query" | "/v1/freshness" | "/v1/tables" | "/v1/sql/validate" | "/v1/pack/metadata"
    ) || path_raw.starts_with("/v1/knowledge/")
        || path_raw.starts_with("/v1/tables/");
    let response = if uses_pack {
        let catalog = match state
            .inner
            .reloading_packs
            .as_ref()
            .map(|packs| packs.snapshot())
            .transpose()
        {
            Ok(Some(catalog)) => catalog,
            Ok(None) => Arc::clone(&state.inner.catalog),
            Err(_) => {
                return crate::public_surface::error_response(
                    ErrorCode::SchemaInvalid,
                    "published knowledge pack is unavailable; verify the installed artifact",
                    None,
                    None,
                );
            }
        };
        REQUEST_PACKS.scope(catalog, next.run(request)).await
    } else {
        next.run(request).await
    };
    let response = normalize_layer_timeout(response);
    eprintln!(
        "method={method} path={path} status={}",
        response.status().as_u16()
    );
    response
}

fn normalize_layer_timeout(response: Response) -> Response {
    if response.status() == StatusCode::REQUEST_TIMEOUT
        && !response.headers().contains_key(header::CONTENT_TYPE)
        && response.body().is_end_stream()
    {
        // Tower has no query receipt; never invent an ID or replace an existing SQL error.
        crate::public_surface::error_response(
            ErrorCode::Timeout,
            "HTTP request deadline exceeded",
            None,
            None,
        )
    } else {
        response
    }
}

fn logged_path(path: &str, probe: bool) -> &'static str {
    match path {
        "/health" => "/health",
        "/v1/auth/enroll" => "/v1/auth/enroll",
        "/v1/auth/renew" => "/v1/auth/renew",
        "/internal/v1/tokens" => "/internal/v1/tokens",
        "/ready" => "/ready",
        "/v1/session/current" => "/v1/session/current",
        "/v1/pack/metadata" => "/v1/pack/metadata",
        "/v1/sql" => "/v1/sql",
        "/v1/tables" => "/v1/tables",
        "/v1/sql/validate" => "/v1/sql/validate",
        "/v1/query" => "/v1/query",
        "/v1/freshness" => "/v1/freshness",
        "/v1/knowledge/search" => "/v1/knowledge/search",
        "/v1/operations" => "/v1/operations",
        "/slow" if probe => "/slow",
        "/hold" if probe => "/hold",
        path if path.starts_with("/v1/tables/") => "/v1/tables/item",
        path if path.starts_with("/v1/knowledge/") => "/v1/knowledge/item",
        path if path.starts_with("/v1/queries/") && path.ends_with("/cancel") => {
            "/v1/queries/cancel"
        }
        path if path.starts_with("/v1/queries/") => "/v1/queries/item",
        path if path.starts_with("/v1/operations/") && path.ends_with("/cancel") => {
            "/v1/operations/cancel"
        }
        path if path.starts_with("/v1/operations/") => "/v1/operations/item",
        _ => "unknown",
    }
}

pub(crate) async fn public_caller(
    state: &AppState,
    headers: &HeaderMap,
    query: &Option<String>,
    peer: PublicPeer,
) -> Result<PublicCaller, Box<Response>> {
    if query.is_some() {
        return Err(Box::new(crate::public_surface::error_response(
            ErrorCode::SchemaInvalid,
            "query string is not allowed",
            None,
            None,
        )));
    }
    if let Err(code) = verified_peer(state, peer) {
        let public = match code {
            ApiCode::Internal => (ErrorCode::Internal, "peer is not verified"),
            _ => (ErrorCode::Unauthenticated, "authentication failed"),
        };
        return Err(Box::new(crate::public_surface::error_response(
            public.0, public.1, None, None,
        )));
    }
    match authenticate(state, headers).await {
        Ok(identity) if identity.scope == "session" => {
            Err(Box::new(crate::public_surface::error_response(
                ErrorCode::SchemaInvalid,
                "session token is not served",
                None,
                None,
            )))
        }
        Ok(identity) => Ok(PublicCaller {
            tuple: identity.tuple,
            scope: identity.scope,
            auth_remaining_ms: identity.auth_remaining_ms,
            auth_cache_hit: identity.auth_cache_hit,
        }),
        Err(_) => Err(Box::new(crate::public_surface::error_response(
            ErrorCode::Unauthenticated,
            "authentication failed",
            None,
            None,
        ))),
    }
}

async fn health(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<TlsPeer>>>,
    RawQuery(query): RawQuery,
) -> Response {
    let peer = match gate(&state, &query, peer) {
        Ok(peer) => peer,
        Err(code) => return json_error(ApiError::new(code)),
    };
    if let Some(probe) = &state.inner.probe {
        probe.health_started.fetch_add(1, Ordering::SeqCst);
    }
    json_value(
        200,
        boundary_body(
            "up",
            peer.as_ref(),
            &[("sql_execution", Value::String("unsupported".to_owned()))],
        ),
    )
}

async fn ready(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<TlsPeer>>>,
    RawQuery(query): RawQuery,
) -> Response {
    let peer = match gate(&state, &query, peer) {
        Ok(peer) => peer,
        Err(code) => return json_error(ApiError::new(code)),
    };
    json_value(
        200,
        boundary_body(
            "ready",
            peer.as_ref(),
            &[
                ("database", Value::String("not_probed".to_owned())),
                ("pack_catalog", Value::String("loaded".to_owned())),
            ],
        ),
    )
}

async fn session_current(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<TlsPeer>>>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let peer = match gate(&state, &query, peer) {
        Ok(peer) => peer,
        Err(code) => return json_error(ApiError::new(code)),
    };
    match authenticate(&state, &headers).await {
        Ok(identity) => json_value(200, session_body(&identity, peer.as_ref())),
        Err(error) => json_error(error),
    }
}

async fn pack_metadata(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<TlsPeer>>>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let peer = match gate(&state, &query, peer) {
        Ok(peer) => peer,
        Err(code) => return json_error(ApiError::new(code)),
    };
    let identity = match authenticate(&state, &headers).await {
        Ok(identity) => identity,
        Err(error) => return json_error(error),
    };
    if !header_agrees(&headers, "x-oo-project", &identity.tuple.project_id)
        || !header_agrees(&headers, "x-oo-role", &identity.tuple.role)
        || !header_agrees(&headers, "x-oo-scope", identity.scope)
    {
        return json_error(ApiError::new(ApiCode::Forbidden));
    }
    let Some(pack) = state.pack_catalog().find(
        &identity.tuple.project_id,
        &identity.tuple.role,
        &identity.tuple.policy_version,
        &identity.tuple.knowledge_version,
    ) else {
        return json_error(ApiError::new(ApiCode::Forbidden));
    };
    json_value(200, pack_body(&identity, &pack, peer.as_ref()))
}

async fn sql_unsupported(RawQuery(query): RawQuery, _body: Bytes) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
    json_error(ApiError::new(ApiCode::SqlUnsupported))
}

async fn slow(RawQuery(query): RawQuery) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    json_value(200, serde_json::json!({"status": "late"}))
}

async fn hold(State(state): State<AppState>, RawQuery(query): RawQuery) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
    let Some(probe) = &state.inner.probe else {
        return json_error(ApiError::new(ApiCode::NotFound));
    };
    let receiver = match probe.hold_rx.lock() {
        Ok(mut guard) => guard.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    let Some(receiver) = receiver else {
        return json_error(ApiError::new(ApiCode::Internal));
    };
    probe.hold_entered.store(true, Ordering::SeqCst);
    let _ = receiver.await;
    json_value(200, serde_json::json!({"status": "held"}))
}

async fn not_found(RawQuery(query): RawQuery) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
    json_error(ApiError::new(ApiCode::NotFound))
}

async fn method_not_allowed(RawQuery(query): RawQuery) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
    json_error(ApiError::new(ApiCode::MethodNotAllowed))
}

fn reject_query(query: &Option<String>) -> Option<Response> {
    if query.is_some() {
        Some(json_error(ApiError::new(ApiCode::QueryNotAllowed)))
    } else {
        None
    }
}

fn gate(
    state: &AppState,
    query: &Option<String>,
    peer: Option<Extension<ConnectInfo<TlsPeer>>>,
) -> Result<Option<TlsPeer>, ApiCode> {
    if query.is_some() {
        return Err(ApiCode::QueryNotAllowed);
    }
    verified_peer(state, peer)
}

fn verified_peer(
    state: &AppState,
    peer: Option<Extension<ConnectInfo<TlsPeer>>>,
) -> Result<Option<TlsPeer>, ApiCode> {
    match &state.inner.transport {
        Transport::Loopback => Ok(None),
        Transport::Tls { .. } => match peer {
            Some(Extension(ConnectInfo(peer))) => Ok(Some(peer)),
            None => Err(ApiCode::Internal),
        },
    }
}

fn forged_certificate_header(headers: &HeaderMap) -> bool {
    FORGED_CERT_HEADERS
        .iter()
        .any(|name| headers.contains_key(*name))
}

async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<Identity, ApiError> {
    let mut token = bearer(headers).map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    let bound = headers.contains_key(crate::publication_binding::EXPECTED)
        || headers.contains_key(crate::publication_binding::EXPECTED_VECTOR);
    let result = authenticate_token(state, &token, bound).await;
    token.zeroize();
    result
}

/// Recheck after waiting for the per-role SQL session. The original request
/// already passed peer verification; neither headers nor a pin select a role.
pub(crate) async fn recheck_bound_caller(
    state: &AppState,
    headers: &HeaderMap,
    caller: &PublicCaller,
) -> bool {
    matches!(authenticate(state, headers).await,
        Ok(identity) if identity.tuple == caller.tuple && identity.scope == caller.scope)
}

async fn authenticate_token(
    state: &AppState,
    token: &str,
    fresh: bool,
) -> Result<Identity, ApiError> {
    let (_kind, hash) = token_hash(token).map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    let (tuple, endpoint, from_cache) = load_tuple(state, &hash, fresh).await?;
    // A question binding is not a credential. Recheck revocation on every use.
    let fresh_cache = LookupCache::new();
    let cache = if fresh {
        &fresh_cache
    } else {
        &state.inner.cache
    };
    let transport = DeclaredTransport::ExternalClient;
    let request = LookupRequest {
        endpoint: &endpoint,
        user: RUNTIME_USER,
        password: &state.inner.password,
        ca_pem: &state.inner.ca_pem,
        cache,
        presented_token: token,
        current: &tuple,
        gateway_audience: &state.inner.audience,
        transport: &transport,
    };
    match lookup(request).await {
        Ok(receipt) => {
            if !from_cache {
                remember_tuple(state, &hash, tuple.clone(), endpoint);
            }
            let scope = match receipt.kind {
                TokenKind::Client => "client",
                TokenKind::Session => "session",
            };
            let auth_remaining_ms = if fresh {
                0
            } else {
                cache.remaining_ms(&hash).ok().flatten().unwrap_or(0)
            };
            Ok(Identity {
                tuple,
                scope,
                auth_remaining_ms,
                auth_cache_hit: !receipt.database_used,
            })
        }
        Err(err) => {
            eprintln!("auth_reason={}", err.reason());
            forget_tuple(state, &hash);
            Err(ApiError::new(ApiCode::Unauthenticated))
        }
    }
}

async fn load_tuple(
    state: &AppState,
    hash: &str,
    fresh: bool,
) -> Result<(CurrentTuple, Endpoint, bool), ApiError> {
    if let Some(hit) = (!fresh).then(|| fresh_tuple(state, hash)).flatten() {
        return Ok((hit.0, hit.1, true));
    }
    let endpoint = read_endpoint(&state.inner.endpoint_path)
        .map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    let client = connect(state, &endpoint)
        .await
        .map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    let row = client
        .query_opt(READ_TOKEN, &[&hash])
        .await
        .map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    let Some(row) = row else {
        return Err(ApiError::new(ApiCode::Unauthenticated));
    };
    let tuple = tuple_from_row(&row).map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    Ok((tuple, endpoint, false))
}

fn fresh_tuple(state: &AppState, hash: &str) -> Option<(CurrentTuple, Endpoint)> {
    let mut guard = match state.inner.tuples.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let expired = guard
        .get(hash)
        .is_some_and(|entry| Instant::now() >= entry.expires);
    if expired {
        guard.remove(hash);
        return None;
    }
    guard
        .get(hash)
        .map(|entry| (entry.tuple.clone(), entry.endpoint.clone()))
}

fn remember_tuple(state: &AppState, hash: &str, tuple: CurrentTuple, endpoint: Endpoint) {
    let Some(expires) = Instant::now().checked_add(TUPLE_CACHE_TTL) else {
        return;
    };
    let mut guard = match state.inner.tuples.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.insert(
        hash.to_owned(),
        TupleEntry {
            tuple,
            endpoint,
            expires,
        },
    );
}

fn forget_tuple(state: &AppState, hash: &str) {
    let mut guard = match state.inner.tuples.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.remove(hash);
}

async fn connect(state: &AppState, endpoint: &Endpoint) -> Result<tokio_postgres::Client, ()> {
    if endpoint.host.is_empty()
        || endpoint.database.is_empty()
        || endpoint.port == 0
        || state.inner.password.is_empty()
    {
        return Err(());
    }
    let mut config = tokio_postgres::Config::new();
    config.user(RUNTIME_USER);
    config.password(&state.inner.password);
    config.host(&endpoint.host);
    config.port(endpoint.port);
    config.dbname(&endpoint.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("oo-gateway-http");
    config.connect_timeout(Duration::from_secs(5));
    let tls = rustls_connector(&state.inner.ca_pem).map_err(|_| ())?;
    let (client, connection) = config.connect(tls).await.map_err(|_| ())?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let row = client.query_one(TLS_FACT, &[]).await.map_err(|_| ())?;
    let ssl: bool = row.try_get(0).map_err(|_| ())?;
    let version: Option<String> = row.try_get(1).map_err(|_| ())?;
    match version {
        Some(version) if ssl && version.starts_with("TLSv1.") => Ok(client),
        _ => Err(()),
    }
}

fn tuple_from_row(row: &tokio_postgres::Row) -> Result<CurrentTuple, ()> {
    let claims = claims_from_canonical(row.try_get::<_, String>(4).map_err(|_| ())?.as_str())?;
    let _issued: i64 = row.try_get(10).map_err(|_| ())?;
    let _idle: Option<i64> = row.try_get(11).map_err(|_| ())?;
    let _absolute: i64 = row.try_get(12).map_err(|_| ())?;
    let _floor: i64 = row.try_get(13).map_err(|_| ())?;
    Ok(CurrentTuple {
        project_id: row.try_get(1).map_err(|_| ())?,
        principal_id: row.try_get(2).map_err(|_| ())?,
        role: row.try_get(3).map_err(|_| ())?,
        claims,
        policy_version: row.try_get(5).map_err(|_| ())?,
        knowledge_version: row.try_get(6).map_err(|_| ())?,
        audience_url: row.try_get(7).map_err(|_| ())?,
        sandbox_id: row.try_get(8).map_err(|_| ())?,
        session_id: row.try_get(9).map_err(|_| ())?,
    })
}

fn claims_from_canonical(text: &str) -> Result<Vec<(String, String)>, ()> {
    let value: Value = serde_json::from_str(text).map_err(|_| ())?;
    let object = value.as_object().ok_or(())?;
    let mut pairs = Vec::new();
    for (key, value) in object {
        let text = value.as_str().ok_or(())?;
        pairs.push((key.clone(), text.to_owned()));
    }
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(pairs)
}

fn read_endpoint(path: &Path) -> Result<Endpoint, &'static str> {
    let text = fs::read_to_string(path).map_err(|_| "endpoint")?;
    let value: Value = serde_json::from_str(&text).map_err(|_| "endpoint")?;
    let object = value.as_object().ok_or("endpoint")?;
    for key in object.keys() {
        if key != "host" && key != "port" && key != "database" {
            return Err("endpoint");
        }
    }
    let host = required_text(object, "host")?.to_owned();
    let database = required_text(object, "database")?.to_owned();
    let port = u16_field(object, "port")?;
    if host.is_empty() || database.is_empty() || port == 0 {
        return Err("endpoint");
    }
    Ok(Endpoint {
        host,
        port,
        database,
    })
}

fn bearer(headers: &HeaderMap) -> Result<String, ()> {
    let value = headers.get(header::AUTHORIZATION).ok_or(())?;
    let text = value.to_str().map_err(|_| ())?;
    let token = text.strip_prefix("Bearer ").ok_or(())?;
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return Err(());
    }
    Ok(token.to_owned())
}

fn header_agrees(headers: &HeaderMap, name: &str, expected: &str) -> bool {
    match headers.get(name) {
        None => true,
        Some(value) => value.to_str().ok() == Some(expected),
    }
}

fn boundary_body(status: &str, peer: Option<&TlsPeer>, extra: &[(&str, Value)]) -> Value {
    let mut body = JsonMap::new();
    body.insert("status".to_owned(), Value::String(status.to_owned()));
    insert_auth(&mut body, peer);
    for (key, value) in extra {
        body.insert((*key).to_owned(), value.clone());
    }
    Value::Object(body)
}

fn session_body(identity: &Identity, peer: Option<&TlsPeer>) -> Value {
    Value::Object(session_map(identity, peer))
}

fn session_map(identity: &Identity, peer: Option<&TlsPeer>) -> JsonMap<String, Value> {
    let mut body = JsonMap::new();
    insert_auth(&mut body, peer);
    body.insert(
        "project_id".to_owned(),
        Value::String(identity.tuple.project_id.clone()),
    );
    body.insert(
        "principal_id".to_owned(),
        Value::String(identity.tuple.principal_id.clone()),
    );
    body.insert(
        "role".to_owned(),
        Value::String(identity.tuple.role.clone()),
    );
    body.insert(
        "policy_version".to_owned(),
        Value::String(identity.tuple.policy_version.clone()),
    );
    body.insert(
        "knowledge_version".to_owned(),
        Value::String(identity.tuple.knowledge_version.clone()),
    );
    body.insert(
        "audience_url".to_owned(),
        Value::String(identity.tuple.audience_url.clone()),
    );
    body.insert("scope".to_owned(), Value::String(identity.scope.to_owned()));
    body.insert(
        "sandbox_id".to_owned(),
        identity
            .tuple
            .sandbox_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    body.insert(
        "session_id".to_owned(),
        identity
            .tuple
            .session_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    body
}

fn insert_auth(body: &mut JsonMap<String, Value>, peer: Option<&TlsPeer>) {
    match peer {
        None => {
            body.insert(
                "authentication".to_owned(),
                Value::String("loopback-http".to_owned()),
            );
            body.insert("external_mtls".to_owned(), Value::Bool(false));
        }
        Some(peer) if peer.workload.is_empty() => {
            body.insert("authentication".into(), Value::String("tls".into()));
            body.insert("external_mtls".into(), Value::Bool(false));
        }
        Some(peer) => {
            body.insert(
                "authentication".to_owned(),
                Value::String("verified-certificate".to_owned()),
            );
            body.insert("external_mtls".to_owned(), Value::Bool(true));
            body.insert(
                "peer_workload".to_owned(),
                Value::String(peer.workload.clone()),
            );
            body.insert(
                "peer_source".to_owned(),
                Value::String("verified-certificate".to_owned()),
            );
        }
    }
    body.insert("oauth".to_owned(), Value::Bool(false));
    body.insert("idp".to_owned(), Value::Bool(false));
}

fn pack_body(identity: &Identity, pack: &PackMetadata, peer: Option<&TlsPeer>) -> Value {
    let mut object = session_map(identity, peer);
    object.insert(
        "pack_digest".to_owned(),
        Value::String(pack.pack_digest.clone()),
    );
    object.insert(
        "protocol".to_owned(),
        Value::String(crate::pack_catalog::ROLE_PACK_PROTOCOL.to_owned()),
    );
    let files = pack
        .files
        .iter()
        .map(|file| {
            serde_json::json!({
                "path": file.path,
                "bytes": file.bytes,
                "sha256": file.sha256,
            })
        })
        .collect::<Vec<_>>();
    let columns = pack
        .columns
        .iter()
        .map(|column| {
            serde_json::json!({
                "table": column.table,
                "name": column.name,
            })
        })
        .collect::<Vec<_>>();
    object.insert("files".to_owned(), Value::Array(files));
    object.insert("columns".to_owned(), Value::Array(columns));
    Value::Object(object)
}

fn json_value(status: u16, value: Value) -> Response {
    match serde_json::to_vec(&value) {
        Ok(body) => json_bytes(status, body),
        Err(_) => json_error(ApiError::new(ApiCode::Internal)),
    }
}

fn json_error(error: ApiError) -> Response {
    json_bytes(error.status(), error.body())
}

fn json_bytes(status: u16, body: Vec<u8>) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = code;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(JSON_MEDIA_TYPE),
    );
    response
}

fn required_text<'a>(
    object: &'a JsonMap<String, Value>,
    key: &str,
) -> Result<&'a str, &'static str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("config")
}

fn one_segment(value: &str) -> Result<&str, &'static str> {
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value == "."
        || value == ".."
    {
        Err("config")
    } else {
        Ok(value)
    }
}

fn usize_field(object: &JsonMap<String, Value>, key: &str) -> Result<usize, &'static str> {
    let number = object.get(key).and_then(Value::as_u64).ok_or("config")?;
    usize::try_from(number).map_err(|_| "config")
}

fn u64_field(object: &JsonMap<String, Value>, key: &str) -> Result<u64, &'static str> {
    object.get(key).and_then(Value::as_u64).ok_or("config")
}

fn u16_field(object: &JsonMap<String, Value>, key: &str) -> Result<u16, &'static str> {
    let number = object.get(key).and_then(Value::as_u64).ok_or("endpoint")?;
    u16::try_from(number).map_err(|_| "endpoint")
}

#[derive(Clone, Debug)]
pub(crate) struct TlsPeer {
    workload: String,
}

struct TlsListener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
    handshake_timeout: Duration,
}

impl Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<TcpStream>;
    type Addr = TlsPeer;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (tcp, _remote) = match self.tcp.accept().await {
                Ok(pair) => pair,
                Err(error) => {
                    if !is_connection_error(&error) {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    continue;
                }
            };
            let handshake = self.acceptor.accept(tcp);
            let Ok(Ok(stream)) = tokio::time::timeout(self.handshake_timeout, handshake).await
            else {
                continue;
            };
            let workload = match workload_from_stream(&stream) {
                Some(workload) => workload,
                None if stream.get_ref().1.peer_certificates().is_none() => String::new(),
                None => continue,
            };
            return (stream, TlsPeer { workload });
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp.local_addr()?;
        Ok(TlsPeer {
            workload: String::new(),
        })
    }
}

impl Connected<IncomingStream<'_, TlsListener>> for TlsPeer {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        stream.remote_addr().clone()
    }
}

fn is_connection_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::ConnectionRefused | ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset
    )
}

fn transport_name(object: &JsonMap<String, Value>) -> Result<&str, &'static str> {
    match object.get("transport") {
        None => Ok("loopback"),
        Some(Value::String(value)) if value == "loopback" || value == "tls" => Ok(value.as_str()),
        Some(Value::String(_)) => Err("transport"),
        Some(_) => Err("config"),
    }
}

fn allowed_config_key(tls: bool, key: &str) -> bool {
    matches!(
        key,
        "listen"
            | "audience"
            | "runtime_user"
            | "password_file"
            | "ca_file"
            | "endpoint_file"
            | "pack_dir"
            | "pack_ceilings_file"
            | "body_limit"
            | "request_timeout_ms"
            | "global_concurrency"
            | "transport"
            | "deployment_mode"
            | "data_query_concurrency"
            | "data_query_timeout_ms"
            | "token_admin_password_file"
            | "token_admin_workload"
    ) || crate::public_surface::data_config_key(key)
        || (tls
            && matches!(
                key,
                "tls_key_file" | "tls_chain_file" | "client_ca_file" | "handshake_timeout_ms"
            ))
}

fn load_tls_acceptor(
    dir: &Path,
    object: &JsonMap<String, Value>,
) -> Result<(TlsAcceptor, Duration), &'static str> {
    let key_name = tls_filename(object, "tls_key_file", "tls_key")?;
    let chain_name = tls_filename(object, "tls_chain_file", "tls_chain")?;
    let ca_name = tls_filename(object, "client_ca_file", "client_ca")?;
    let timeout_ms = match object.get("handshake_timeout_ms") {
        Some(Value::Number(number)) => number.as_u64().ok_or("handshake_timeout")?,
        _ => return Err("handshake_timeout"),
    };
    if !(1..=30_000).contains(&timeout_ms) {
        return Err("handshake_timeout");
    }
    let mut key_bytes = fs::read(dir.join(key_name)).map_err(|_| "tls_key")?;
    let chain_bytes = fs::read(dir.join(chain_name)).map_err(|_| "tls_chain")?;
    let ca_bytes = fs::read(dir.join(ca_name)).map_err(|_| "client_ca")?;
    let key = match parse_private_key(&key_bytes) {
        Ok(key) => {
            key_bytes.zeroize();
            key
        }
        Err(reason) => {
            key_bytes.zeroize();
            return Err(reason);
        }
    };
    let chain = parse_chain(&chain_bytes)?;
    let ca = parse_client_ca(&ca_bytes)?;
    let mut roots = RootCertStore::empty();
    roots.add(ca).map_err(|_| "client_ca")?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone());
    let builder = if object.contains_key("token_admin_password_file") {
        builder.allow_unauthenticated()
    } else {
        builder
    };
    let verifier = builder.build().map_err(|_| "tls")?;
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| "tls")?
        .with_client_cert_verifier(verifier)
        .with_single_cert(chain, key)
        .map_err(|_| "tls")?;
    Ok((
        TlsAcceptor::from(Arc::new(config)),
        Duration::from_millis(timeout_ms),
    ))
}

fn tls_filename<'a>(
    object: &'a JsonMap<String, Value>,
    key: &str,
    missing: &'static str,
) -> Result<&'a str, &'static str> {
    let Some(Value::String(value)) = object.get(key) else {
        return Err(missing);
    };
    if value.is_empty() {
        return Err(missing);
    }
    one_segment(value)
}

fn parse_private_key(bytes: &[u8]) -> Result<PrivateKeyDer<'static>, &'static str> {
    if bytes.is_empty()
        || !contains_ascii(bytes, b"PRIVATE KEY")
        || contains_ascii(bytes, b"ENCRYPTED PRIVATE KEY")
    {
        return Err("tls_key");
    }
    PrivateKeyDer::from_pem_slice(bytes).map_err(|_| "tls_key")
}

fn parse_chain(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, &'static str> {
    if bytes.is_empty() || contains_ascii(bytes, b"PRIVATE KEY") {
        return Err("tls_chain");
    }
    let certs = pem_certs(bytes).map_err(|_| "tls_chain")?;
    if certs.is_empty() {
        Err("tls_chain")
    } else {
        Ok(certs)
    }
}

fn parse_client_ca(bytes: &[u8]) -> Result<CertificateDer<'static>, &'static str> {
    if bytes.is_empty() || contains_ascii(bytes, b"PRIVATE KEY") {
        return Err("client_ca");
    }
    let mut certs = pem_certs(bytes).map_err(|_| "client_ca")?;
    if certs.len() != 1 {
        return Err("client_ca");
    }
    Ok(certs.remove(0))
}

fn pem_certs(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, ()> {
    CertificateDer::pem_slice_iter(bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())
}

fn contains_ascii(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

fn workload_from_stream(stream: &tokio_rustls::server::TlsStream<TcpStream>) -> Option<String> {
    let (_, connection) = stream.get_ref();
    let leaf = connection.peer_certificates()?.first()?;
    workload_uri(leaf.as_ref()).ok()
}

fn workload_uri(der: &[u8]) -> Result<String, ()> {
    let mut outer = Der::new(der);
    let body = outer.expect(0x30)?;
    if !outer.rest.is_empty() {
        return Err(());
    }
    let mut certificate = Der::new(body);
    let tbs = certificate.expect(0x30)?;
    let _signature_algorithm = certificate.expect(0x30)?;
    let _signature = certificate.expect(0x03)?;
    if !certificate.rest.is_empty() {
        return Err(());
    }
    let mut fields = Der::new(tbs);
    if fields.rest.first() == Some(&0xA0) {
        let _version = fields.expect(0xA0)?;
    }
    let _serial = fields.expect(0x02)?;
    let _signature = fields.expect(0x30)?;
    let _issuer = fields.expect(0x30)?;
    let _validity = fields.expect(0x30)?;
    let _subject = fields.expect(0x30)?;
    let _subject_public_key = fields.expect(0x30)?;
    let mut san = None;
    while !fields.rest.is_empty() {
        let (tag, value) = fields.read_tlv()?;
        match tag {
            0x81 | 0x82 => {}
            0xA3 => {
                let mut wrapped = Der::new(value);
                let extensions = wrapped.expect(0x30)?;
                if !wrapped.rest.is_empty() {
                    return Err(());
                }
                san = one_san(san, san_extension(extensions)?)?;
            }
            _ => return Err(()),
        }
    }
    parse_san_uri(san.ok_or(())?)
}

fn one_san<'a>(found: Option<&'a [u8]>, next: Option<&'a [u8]>) -> Result<Option<&'a [u8]>, ()> {
    match (found, next) {
        (Some(_), Some(_)) => Err(()),
        (None, next) => Ok(next),
        (found, None) => Ok(found),
    }
}

fn san_extension(extensions: &[u8]) -> Result<Option<&[u8]>, ()> {
    let mut cursor = Der::new(extensions);
    let mut found = None;
    while !cursor.rest.is_empty() {
        let extension = cursor.expect(0x30)?;
        let mut inner = Der::new(extension);
        let oid = inner.expect(0x06)?;
        if inner.rest.first() == Some(&0x01) {
            let boolean = inner.expect(0x01)?;
            if boolean != [0x00] && boolean != [0xFF] {
                return Err(());
            }
        }
        let value = inner.expect(0x04)?;
        if !inner.rest.is_empty() {
            return Err(());
        }
        if oid == SAN_OID {
            if found.is_some() {
                return Err(());
            }
            found = Some(value);
        }
    }
    Ok(found)
}

fn parse_san_uri(value: &[u8]) -> Result<String, ()> {
    let mut cursor = Der::new(value);
    let names = cursor.expect(0x30)?;
    if !cursor.rest.is_empty() {
        return Err(());
    }
    let mut names = Der::new(names);
    let (tag, body) = names.read_tlv()?;
    if tag != 0x86 || !names.rest.is_empty() {
        return Err(());
    }
    let text = std::str::from_utf8(body).map_err(|_| ())?;
    let remainder = text.strip_prefix(WORKLOAD_PREFIX).ok_or(())?;
    if remainder.is_empty() || remainder.len() > 128 {
        return Err(());
    }
    if !remainder
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
    {
        return Err(());
    }
    Ok(text.to_owned())
}

struct Der<'a> {
    rest: &'a [u8],
}

impl<'a> Der<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { rest: input }
    }

    fn expect(&mut self, tag: u8) -> Result<&'a [u8], ()> {
        let (got, body) = self.read_tlv()?;
        if got == tag { Ok(body) } else { Err(()) }
    }

    fn read_tlv(&mut self) -> Result<(u8, &'a [u8]), ()> {
        let tag = *self.rest.first().ok_or(())?;
        if tag & 0x1F == 0x1F {
            return Err(());
        }
        self.rest = &self.rest[1..];
        let length = self.read_len()?;
        if self.rest.len() < length {
            return Err(());
        }
        let (body, rest) = self.rest.split_at(length);
        self.rest = rest;
        Ok((tag, body))
    }

    fn read_len(&mut self) -> Result<usize, ()> {
        let first = *self.rest.first().ok_or(())?;
        self.rest = &self.rest[1..];
        if first < 0x80 {
            return Ok(usize::from(first));
        }
        let count = usize::from(first & 0x7F);
        if count == 0 || count > 3 || self.rest.len() < count {
            return Err(());
        }
        let (bytes, rest) = self.rest.split_at(count);
        self.rest = rest;
        if bytes[0] == 0 {
            return Err(());
        }
        let mut value = 0usize;
        for byte in bytes {
            value = value
                .checked_shl(8)
                .ok_or(())?
                .checked_add(usize::from(*byte))
                .ok_or(())?;
        }
        if value < 0x80 { Err(()) } else { Ok(value) }
    }
}

pub async fn harness_tls_exchange(
    addr: SocketAddr,
    ca_pem: &[u8],
    client_cert_pem: Option<&[u8]>,
    client_key_pem: Option<&[u8]>,
    request: &[u8],
) -> Result<Vec<u8>, String> {
    let mut roots = RootCertStore::empty();
    let mut anchors = pem_certs(ca_pem).map_err(|_| "ca".to_owned())?;
    if anchors.len() != 1 {
        return Err("ca".to_owned());
    }
    roots.add(anchors.remove(0)).map_err(|_| "ca".to_owned())?;
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| "tls".to_owned())?
    .with_root_certificates(roots);
    let config = match (client_cert_pem, client_key_pem) {
        (None, None) => builder.with_no_client_auth(),
        (Some(cert_pem), Some(key_pem)) => {
            let chain = pem_certs(cert_pem).map_err(|_| "client_cert".to_owned())?;
            if chain.is_empty() {
                return Err("client_cert".to_owned());
            }
            let key =
                PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| "client_key".to_owned())?;
            builder
                .with_client_auth_cert(chain, key)
                .map_err(|_| "client_cert".to_owned())?
        }
        _ => return Err("client_cert".to_owned()),
    };
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = TcpStream::connect(addr)
        .await
        .map_err(|_| "connect".to_owned())?;
    let server_name = ServerName::<'static>::from(addr.ip());
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|_| "handshake".to_owned())?;
    let (mut reader, mut writer) = tokio::io::split(tls);
    writer
        .write_all(request)
        .await
        .map_err(|_| "write".to_owned())?;
    writer.flush().await.map_err(|_| "write".to_owned())?;
    let mut buffer = Vec::new();
    // The gateway closes the socket after Connection: close and does not send a TLS close_notify.
    // read_to_end then returns an error after the HTTP bytes are already in the buffer.
    let read = reader.read_to_end(&mut buffer).await;
    drop(writer);
    match read {
        Ok(_) => Ok(buffer),
        Err(_) if http_message_complete(&buffer) => Ok(buffer),
        Err(_) => Err("read".to_owned()),
    }
}

fn http_message_complete(bytes: &[u8]) -> bool {
    let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = &bytes[..split];
    let body = &bytes[split + 4..];
    let mut length = None;
    for line in head.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        let name = &line[..colon];
        let value = &line[colon + 1..];
        if !name.eq_ignore_ascii_case(b"content-length") {
            continue;
        }
        let Ok(text) = std::str::from_utf8(value) else {
            return false;
        };
        let Ok(parsed) = text.trim().parse::<usize>() else {
            return false;
        };
        length = Some(parsed);
    }
    length.is_some_and(|length| body.len() >= length)
}

pub async fn harness_install(
    endpoint: Endpoint,
    password: &str,
    ca_pem: &[u8],
) -> Result<(), String> {
    token_lifecycle::Control::new(endpoint, password.into(), ca_pem.into())
        .install()
        .await
        .map_err(str::to_owned)
}

pub async fn harness_issue(
    endpoint: Endpoint,
    password: &str,
    ca_pem: &[u8],
    grant: IssueGrant,
) -> Result<(String, String), String> {
    let g = token_lifecycle::Grant::from_legacy(grant, String::new()).map_err(str::to_owned)?;
    let result = token_lifecycle::Control::new(endpoint, password.into(), ca_pem.into())
        .execute(token_lifecycle::Operation::Issue(g), ADMIN_USER)
        .await
        .map_err(str::to_owned)?;
    Ok((
        result.secret.ok_or("secret")?.to_string(),
        result.metadata["token_sha256"]
            .as_str()
            .ok_or("hash")?
            .into(),
    ))
}

pub async fn harness_revoke_token(
    endpoint: Endpoint,
    password: &str,
    ca_pem: &[u8],
    project_id: String,
    token_sha256: String,
) -> Result<(), String> {
    token_lifecycle::Control::new(endpoint, password.into(), ca_pem.into())
        .execute(
            token_lifecycle::Operation::Revoke {
                project: project_id,
                hash: token_sha256,
            },
            ADMIN_USER,
        )
        .await
        .map(|_| ())
        .map_err(str::to_owned)
}

#[cfg(test)]
mod http_timeout_tests {
    use super::*;
    use oo_core::api::{ErrorEnvelope, parse_api_json};
    use tower::ServiceExt;

    #[tokio::test]
    async fn concurrent_requests_keep_their_own_pack_snapshot_across_awaits() {
        let state = AppState::for_limits(PackCatalog::empty(), 1024);
        let first = Arc::new(PackCatalog::empty());
        let second = Arc::new(PackCatalog::empty());
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let request = |pack: Arc<PackCatalog>| {
            let state = state.clone();
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                let expected = Arc::clone(&pack);
                REQUEST_PACKS
                    .scope(pack, async move {
                        assert!(Arc::ptr_eq(&state.pack_catalog(), &expected));
                        barrier.wait().await;
                        tokio::task::yield_now().await;
                        assert!(Arc::ptr_eq(&state.pack_catalog(), &expected));
                    })
                    .await;
            })
        };
        let a = request(Arc::clone(&first));
        let b = request(Arc::clone(&second));
        a.await.unwrap();
        b.await.unwrap();
        assert!(!Arc::ptr_eq(&state.pack_catalog(), &first));
        assert!(!Arc::ptr_eq(&state.pack_catalog(), &second));
    }

    fn installed_state(http_ms: u64, sql_ms: u64) -> AppState {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("oo-http-budget-{}-{nonce}", std::process::id()));
        fs::create_dir_all(root.join("packs")).unwrap();
        fs::write(root.join("runtime.pass"), "synthetic-test-only").unwrap();
        fs::write(root.join("ca.pem"), "synthetic-test-only").unwrap();
        fs::write(
            root.join("endpoint.json"),
            r#"{"host":"127.0.0.1","port":9,"database":"unused"}"#,
        )
        .unwrap();
        let config = serde_json::json!({
            "deployment_mode": "all_in_one", "listen": "127.0.0.1:0",
            "audience": "https://gateway.loopback.invalid/v1", "runtime_user": RUNTIME_USER,
            "password_file": "runtime.pass", "ca_file": "ca.pem",
            "endpoint_file": "endpoint.json", "pack_dir": "packs",
            "body_limit": 1024, "request_timeout_ms": http_ms, "global_concurrency": 64,
            "data_query_concurrency": 2, "data_query_timeout_ms": sql_ms,
        });
        fs::write(root.join("config.json"), config.to_string()).unwrap();
        assert_eq!(
            oo_core::config::DeploymentMode::AllInOne
                .query_limits(&config)
                .unwrap(),
            (2, sql_ms),
        );
        let mut loaded = load_operator_dir(&root).expect("synthetic installed config");
        // This fixture tests HTTP timing after removing its temporary operator directory.
        Arc::get_mut(&mut loaded.state.inner)
            .unwrap()
            .reloading_packs = None;
        fs::remove_dir_all(root).unwrap();
        loaded.state
    }

    #[test]
    fn http_budget_is_independent_of_sql_budget_and_concurrency_cap_stays() {
        let state = installed_state(120_000, 20_000);
        assert_eq!(state.inner.request_timeout, Duration::from_secs(120));
        assert_eq!(state.inner.global_concurrency, 2);
        let shorter = installed_state(10, 20_000);
        assert_eq!(shorter.inner.request_timeout, Duration::from_millis(10));
        assert_eq!(shorter.inner.global_concurrency, 2);
    }

    // Synthetic handlers exercise the HTTP layer only, without credentials or PostgreSQL.
    async fn delayed_ok() -> Response {
        tokio::time::sleep(Duration::from_millis(60)).await;
        json_value(200, serde_json::json!({"status":"completed"}))
    }

    fn slow_router(state: AppState) -> Router {
        Router::new()
            .route("/v1/freshness", get(delayed_ok))
            .route("/v1/query", post(delayed_ok))
            .route_layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                state.inner.request_timeout,
            ))
            .layer(from_fn_with_state(state.clone(), access_log))
            .with_state(state)
    }

    #[tokio::test]
    async fn metadata_may_outlast_sql_budget_and_http_timeout_is_core_json() {
        let response = slow_router(installed_state(500, 20))
            .oneshot(
                Request::builder()
                    .uri("/v1/freshness")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "metadata was capped by the SQL budget"
        );

        for version in [axum::http::Version::HTTP_11, axum::http::Version::HTTP_2] {
            for (method, path) in [("GET", "/v1/freshness"), ("POST", "/v1/query")] {
                let response = slow_router(installed_state(10, 20))
                    .oneshot(
                        Request::builder()
                            .method(method)
                            .uri(path)
                            .version(version)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
                assert_eq!(response.headers()[header::CONTENT_TYPE], JSON_MEDIA_TYPE);
                let bytes = axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap();
                let envelope: ErrorEnvelope =
                    parse_api_json(std::str::from_utf8(&bytes).unwrap(), |_| Vec::new())
                        .expect("SDK/CLI core error contract");
                assert_eq!(envelope.error.code(), ErrorCode::Timeout);
                assert!(!envelope.error.retryable());
                assert!(envelope.error.query_id().is_none());
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                assert!(value["error"].get("query_id").is_none());
            }
        }
    }

    #[tokio::test]
    async fn existing_query_timeout_receipt_and_non_timeout_bodies_are_preserved() {
        let response = crate::public_surface::error_response(
            ErrorCode::Timeout,
            "SQL execution deadline exceeded",
            None,
            Some("q_abc123"),
        );
        let response = normalize_layer_timeout(response);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let envelope: ErrorEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.error.query_id().unwrap().as_str(), "q_abc123");
        assert_eq!(
            envelope.error.message().as_str(),
            "SQL execution deadline exceeded"
        );
        for (status, body) in [
            (StatusCode::REQUEST_TIMEOUT, "existing body"),
            (StatusCode::PAYLOAD_TOO_LARGE, ""),
        ] {
            let response = Response::builder()
                .status(status)
                .body(Body::from(body))
                .unwrap();
            let response = normalize_layer_timeout(response);
            assert_eq!(response.status(), status);
            assert!(!response.headers().contains_key(header::CONTENT_TYPE));
            assert_eq!(
                axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap(),
                body
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires OO_HTTP_TIMEOUT_CLI pointing to an already built CLI; no rebuild"]
    async fn existing_cli_reports_gateway_timeout_for_slow_metadata_and_query() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let cli = std::env::var("OO_HTTP_TIMEOUT_CLI").expect("explicit existing CLI path");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = slow_router(installed_state(10, 20));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for command in ["freshness", "query"] {
            let cli = cli.clone();
            let origin = origin.clone();
            let output = tokio::task::spawn_blocking(move || {
                let mut child = Command::new(cli);
                child.args([
                    command,
                    "--json",
                    "--gateway",
                    &origin,
                    "--transport",
                    "local-development",
                    "--auth",
                    "external",
                    "--token-stdin",
                ]);
                if command == "query" {
                    child.arg("SELECT 1");
                }
                let mut child = child
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(b"synthetic-timeout-probe\n")
                    .unwrap();
                child.wait_with_output().unwrap()
            })
            .await
            .unwrap();
            assert_eq!(output.status.code(), Some(2), "{output:?}");
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["class"], "gateway");
            assert_eq!(value["code"], "TIMEOUT");
            assert_eq!(value["retryable"], false);
            assert!(value.get("query_id").is_none());
        }
        server.abort();
    }
}

#[cfg(test)]
mod deployment_config_tests {
    use super::load_operator_dir;
    #[test]
    fn unknown_mode_and_all_in_one_replica_fail_before_binding() {
        let root =
            std::env::temp_dir().join(format!("oo-wf-allinone-config-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for (document, expected) in [
            (r#"{"deployment_mode":"unknown"}"#, "deployment_mode"),
            (
                r#"{"deployment_mode":"all_in_one","replica_pg":{}}"#,
                "all_in_one_replica_conflict",
            ),
        ] {
            std::fs::write(root.join("config.json"), document).unwrap();
            assert_eq!(load_operator_dir(&root).err(), Some(expected));
        }
        std::fs::remove_file(root.join("config.json")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
