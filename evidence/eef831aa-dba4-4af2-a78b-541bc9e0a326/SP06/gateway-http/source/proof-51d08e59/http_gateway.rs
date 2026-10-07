//! Loopback HTTP router. Caller headers are not authority.
//!
//! The process derives project, principal, role, and scope from the hashed
//! bearer token, then calls the accepted `lookup`. Session tokens fail that
//! binding check because this process is not an egress proxy.

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{RawQuery, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{Map as JsonMap, Value};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_postgres::config::SslMode;
use tower::limit::GlobalConcurrencyLimitLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use zeroize::Zeroize;

pub use crate::accepted_token_store::{CurrentTuple, Endpoint, IssueGrant, TokenKind};
use crate::accepted_token_store::{
    lookup, rustls_connector, token_hash, DeclaredTransport, LookupCache, LookupRequest, RevokeScope,
    TrustedControlAdapter,
};
use crate::http_error::{ApiCode, ApiError, JSON_MEDIA_TYPE};
use crate::pack_catalog::{PackCatalog, PackMetadata};

const READ_TOKEN: &str = "SELECT token_kind, project_id, principal_id, role_name, claims_canonical, policy_version, knowledge_version, audience_url, sandbox_id, session_id, issued_at_ms, idle_deadline_ms, absolute_deadline_ms, revocation_floor FROM oo_token_control.token_record WHERE token_sha256 = $1";
const TLS_FACT: &str = "SELECT ssl, version FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid()";
const TUPLE_CACHE_TTL: Duration = Duration::from_millis(5_000);
const RUNTIME_USER: &str = "oo_token_runtime";
const ADMIN_USER: &str = "oo_token_admin";
pub const PRODUCTION_BODY_LIMIT: usize = 1024 * 1024;

struct TupleEntry {
    tuple: CurrentTuple,
    endpoint: Endpoint,
    expires: Instant,
}

struct Inner {
    audience: String,
    endpoint_path: PathBuf,
    password: String,
    ca_pem: Vec<u8>,
    catalog: PackCatalog,
    cache: LookupCache,
    tuples: Mutex<HashMap<String, TupleEntry>>,
    body_limit: usize,
    request_timeout: Duration,
    global_concurrency: usize,
    probe: Option<Arc<ProbeHooks>>,
    log_probe_paths: bool,
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
}

pub struct OperatorLoad {
    pub state: AppState,
    pub listen: SocketAddr,
}

pub fn router(state: AppState) -> Router {
    assemble(state)
}

impl AppState {
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
                endpoint_path: PathBuf::from("endpoint.json"),
                password: String::new(),
                ca_pem: Vec::new(),
                catalog,
                cache: LookupCache::new(),
                tuples: Mutex::new(HashMap::new()),
                body_limit,
                request_timeout: Duration::from_secs(5),
                global_concurrency: 1,
                probe,
                log_probe_paths,
            }),
        }
    }
}

pub fn load_operator_dir(dir: &Path) -> Result<OperatorLoad, &'static str> {
    let text = fs::read_to_string(dir.join("config.json")).map_err(|_| "config")?;
    let value: Value = serde_json::from_str(&text).map_err(|_| "config")?;
    let object = value.as_object().ok_or("config")?;
    let allowed = [
        "listen",
        "audience",
        "runtime_user",
        "password_file",
        "ca_file",
        "endpoint_file",
        "pack_dir",
        "body_limit",
        "request_timeout_ms",
        "global_concurrency",
    ];
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err("config");
        }
    }
    let listen = required_text(object, "listen")?.parse::<SocketAddr>().map_err(|_| "listen")?;
    if listen.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
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
    let timeout_ms = u64_field(object, "request_timeout_ms")?;
    let global_concurrency = usize_field(object, "global_concurrency")?;
    if body_limit == 0 || timeout_ms == 0 || global_concurrency == 0 {
        return Err("limits");
    }
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
    let catalog = match PackCatalog::load_dir(&dir.join(pack_dir)) {
        Ok(catalog) => catalog,
        Err(_) => {
            password.zeroize();
            return Err("packs");
        }
    };
    if let Err(reason) = read_endpoint(&dir.join(endpoint_file)) {
        password.zeroize();
        return Err(reason);
    }
    Ok(OperatorLoad {
        state: AppState {
            inner: Arc::new(Inner {
                audience,
                endpoint_path: dir.join(endpoint_file),
                password,
                ca_pem,
                catalog,
                cache: LookupCache::new(),
                tuples: Mutex::new(HashMap::new()),
                body_limit,
                request_timeout: Duration::from_millis(timeout_ms),
                global_concurrency,
                probe: None,
                log_probe_paths: false,
            }),
        },
        listen,
    })
}

pub async fn serve(state: AppState, listen: SocketAddr, addr_path: &Path) -> Result<(), &'static str> {
    if listen.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err("listen");
    }
    let listener = TcpListener::bind(listen).await.map_err(|_| "bind")?;
    let bound = listener.local_addr().map_err(|_| "bind")?;
    if bound.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err("listen");
    }
    fs::write(addr_path, format!("{bound}\n")).map_err(|_| "addr")?;
    axum::serve(listener, router(state)).await.map_err(|_| "serve")
}

fn assemble(state: AppState) -> Router {
    let timeout = state.inner.request_timeout;
    let body_limit = state.inner.body_limit;
    let concurrency = state.inner.global_concurrency;
    let probe = state.inner.probe.is_some();
    let mut router = Router::new();
    if probe {
        // The short deadline applies to `/slow` only. Routes added after this layer are outside it.
        router = router.route("/slow", get(slow)).route_layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_millis(150),
        ));
    }
    router = router
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/v1/session/current", get(session_current))
        .route("/v1/pack/metadata", get(pack_metadata))
        .route("/v1/sql", post(sql_unsupported))
        .route_layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            timeout,
        ));
    if probe {
        router = router.route("/hold", get(hold));
    }
    router
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(RequestBodyLimitLayer::new(body_limit))
        .layer(GlobalConcurrencyLimitLayer::new(concurrency))
        .layer(from_fn_with_state(state.clone(), access_log))
        .with_state(state)
}

async fn access_log(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = logged_path(request.uri().path(), state.inner.log_probe_paths);
    let response = next.run(request).await;
    eprintln!("method={method} path={path} status={}", response.status().as_u16());
    response
}

fn logged_path(path: &str, probe: bool) -> &'static str {
    match path {
        "/health" => "/health",
        "/ready" => "/ready",
        "/v1/session/current" => "/v1/session/current",
        "/v1/pack/metadata" => "/v1/pack/metadata",
        "/v1/sql" => "/v1/sql",
        "/slow" if probe => "/slow",
        "/hold" if probe => "/hold",
        _ => "unknown",
    }
}

async fn health(State(state): State<AppState>, RawQuery(query): RawQuery) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
    if let Some(probe) = &state.inner.probe {
        probe.health_started.fetch_add(1, Ordering::SeqCst);
    }
    json_value(200, boundary_body("up", &[("sql_execution", Value::String("unsupported".to_owned()))]))
}

async fn ready(RawQuery(query): RawQuery) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
    json_value(
        200,
        boundary_body(
            "ready",
            &[
                ("database", Value::String("not_probed".to_owned())),
                ("pack_catalog", Value::String("loaded".to_owned())),
            ],
        ),
    )
}

async fn session_current(State(state): State<AppState>, RawQuery(query): RawQuery, headers: HeaderMap) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
    match authenticate(&state, &headers).await {
        Ok(identity) => json_value(200, session_body(&identity)),
        Err(error) => json_error(error),
    }
}

async fn pack_metadata(State(state): State<AppState>, RawQuery(query): RawQuery, headers: HeaderMap) -> Response {
    if let Some(response) = reject_query(&query) {
        return response;
    }
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
    let Some(pack) = state.inner.catalog.find(
        &identity.tuple.project_id,
        &identity.tuple.role,
        &identity.tuple.policy_version,
        &identity.tuple.knowledge_version,
    ) else {
        return json_error(ApiError::new(ApiCode::Forbidden));
    };
    json_value(200, pack_body(&identity, &pack))
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

async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<Identity, ApiError> {
    let mut token = bearer(headers).map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    let result = authenticate_token(state, &token).await;
    token.zeroize();
    result
}

async fn authenticate_token(state: &AppState, token: &str) -> Result<Identity, ApiError> {
    let (_kind, hash) = token_hash(token).map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    let (tuple, endpoint, from_cache) = load_tuple(state, &hash).await?;
    let transport = DeclaredTransport::ExternalClient;
    let request = LookupRequest {
        endpoint: &endpoint,
        user: RUNTIME_USER,
        password: &state.inner.password,
        ca_pem: &state.inner.ca_pem,
        cache: &state.inner.cache,
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
            Ok(Identity { tuple, scope })
        }
        Err(err) => {
            eprintln!("auth_reason={}", err.reason());
            forget_tuple(state, &hash);
            Err(ApiError::new(ApiCode::Unauthenticated))
        }
    }
}

async fn load_tuple(state: &AppState, hash: &str) -> Result<(CurrentTuple, Endpoint, bool), ApiError> {
    if let Some(hit) = fresh_tuple(state, hash) {
        return Ok((hit.0, hit.1, true));
    }
    let endpoint = read_endpoint(&state.inner.endpoint_path).map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
    let client = connect(state, &endpoint).await.map_err(|_| ApiError::new(ApiCode::Unauthenticated))?;
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
    let expired = guard.get(hash).is_some_and(|entry| Instant::now() >= entry.expires);
    if expired {
        guard.remove(hash);
        return None;
    }
    guard.get(hash).map(|entry| (entry.tuple.clone(), entry.endpoint.clone()))
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
    if endpoint.host.is_empty() || endpoint.database.is_empty() || endpoint.port == 0 || state.inner.password.is_empty() {
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
    Ok(Endpoint { host, port, database })
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

fn boundary_body(status: &str, extra: &[(&str, Value)]) -> Value {
    let mut body = JsonMap::new();
    body.insert("status".to_owned(), Value::String(status.to_owned()));
    body.insert("authentication".to_owned(), Value::String("loopback-http".to_owned()));
    body.insert("external_mtls".to_owned(), Value::Bool(false));
    body.insert("oauth".to_owned(), Value::Bool(false));
    body.insert("idp".to_owned(), Value::Bool(false));
    for (key, value) in extra {
        body.insert((*key).to_owned(), value.clone());
    }
    Value::Object(body)
}

fn session_body(identity: &Identity) -> Value {
    Value::Object(session_map(identity))
}

fn session_map(identity: &Identity) -> JsonMap<String, Value> {
    let mut body = JsonMap::new();
    body.insert("authentication".to_owned(), Value::String("loopback-http".to_owned()));
    body.insert("external_mtls".to_owned(), Value::Bool(false));
    body.insert("oauth".to_owned(), Value::Bool(false));
    body.insert("idp".to_owned(), Value::Bool(false));
    body.insert("project_id".to_owned(), Value::String(identity.tuple.project_id.clone()));
    body.insert("principal_id".to_owned(), Value::String(identity.tuple.principal_id.clone()));
    body.insert("role".to_owned(), Value::String(identity.tuple.role.clone()));
    body.insert("policy_version".to_owned(), Value::String(identity.tuple.policy_version.clone()));
    body.insert("knowledge_version".to_owned(), Value::String(identity.tuple.knowledge_version.clone()));
    body.insert("audience_url".to_owned(), Value::String(identity.tuple.audience_url.clone()));
    body.insert("scope".to_owned(), Value::String(identity.scope.to_owned()));
    body.insert(
        "sandbox_id".to_owned(),
        identity.tuple.sandbox_id.clone().map(Value::String).unwrap_or(Value::Null),
    );
    body.insert(
        "session_id".to_owned(),
        identity.tuple.session_id.clone().map(Value::String).unwrap_or(Value::Null),
    );
    body
}

fn pack_body(identity: &Identity, pack: &PackMetadata) -> Value {
    let mut object = session_map(identity);
    object.insert("pack_digest".to_owned(), Value::String(pack.pack_digest.clone()));
    object.insert("protocol".to_owned(), Value::String(crate::pack_catalog::ROLE_PACK_PROTOCOL.to_owned()));
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
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(JSON_MEDIA_TYPE));
    response
}

fn required_text<'a>(object: &'a JsonMap<String, Value>, key: &str) -> Result<&'a str, &'static str> {
    object.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).ok_or("config")
}

fn one_segment(value: &str) -> Result<&str, &'static str> {
    if value.is_empty() || value.contains('/') || value.contains('\\') || value == "." || value == ".." {
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

pub async fn harness_install(endpoint: Endpoint, password: &str, ca_pem: &[u8]) -> Result<(), String> {
    let adapter = TrustedControlAdapter::new(endpoint, ADMIN_USER, password, ca_pem.to_vec())
        .map_err(|err| err.reason().to_owned())?;
    adapter.install().await.map(|_| ()).map_err(|err| err.reason().to_owned())
}

pub async fn harness_issue(endpoint: Endpoint, password: &str, ca_pem: &[u8], grant: IssueGrant) -> Result<(String, String), String> {
    let adapter = TrustedControlAdapter::new(endpoint, ADMIN_USER, password, ca_pem.to_vec())
        .map_err(|err| err.reason().to_owned())?;
    let receipt = adapter.issue(grant).await.map_err(|err| err.reason().to_owned())?;
    let token = receipt.token.reveal_once();
    Ok((token, receipt.hash))
}

pub async fn harness_revoke_token(
    endpoint: Endpoint,
    password: &str,
    ca_pem: &[u8],
    project_id: String,
    token_sha256: String,
) -> Result<(), String> {
    let adapter = TrustedControlAdapter::new(endpoint, ADMIN_USER, password, ca_pem.to_vec())
        .map_err(|err| err.reason().to_owned())?;
    adapter
        .revoke(RevokeScope::Token {
            project_id,
            token_sha256,
        })
        .await
        .map(|_| ())
        .map_err(|err| err.reason().to_owned())
}
