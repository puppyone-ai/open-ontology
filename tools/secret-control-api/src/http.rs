//! Management metadata routes and the component resolve route.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::extract::connect_info::Connected;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{post, put};
use axum::serve::IncomingStream;
use axum::Router;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot, watch};
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use zeroize::Zeroizing;

use crate::aad::Scope;
use crate::config::Config;
use crate::crypto::{sha256_hex, NonceSource, OsNonce};
use crate::error::{error_body, Fault};
use crate::identity::{confirm_presented_csrf, fetch_current, presented};
use crate::ids::{ident_ok, idempotency_ok, session_cookie_ok, workload_ok};
use crate::jsonutil::parse_exact;
use crate::store::{RotateRequest, Store, WriteRequest};
use crate::tlsutil::{component_config, server_config, ComponentListener, TlsListener, WorkloadPeer};

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    store: Arc<Store>,
}

#[derive(Deserialize)]
struct SecretPath {
    project_id: String,
    namespace: String,
    secret_id: String,
}

pub struct ServerHandle {
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<Result<(), Fault>>,
    ready: Option<oneshot::Receiver<Result<(), Fault>>>,
}

impl ServerHandle {
    pub async fn ready(&mut self) -> Result<(), Fault> {
        let Some(receiver) = self.ready.take() else {
            return Err(Fault::not_ready());
        };
        receiver.await.map_err(|_| Fault::not_ready())?
    }

    pub async fn stop(self) -> Result<(), Fault> {
        let _ = self.stop.send(true);
        self.task.await.map_err(|_| Fault::not_ready())?
    }
}

pub async fn listen(config: &Config) -> Result<(TcpListener, TcpListener), Fault> {
    let management = TcpListener::bind((config.listen_host, config.management_port)).await.map_err(|_| Fault::not_ready())?;
    let component = TcpListener::bind((config.listen_host, config.component_port)).await.map_err(|_| Fault::not_ready())?;
    check_bound(config.listen_host, config.management_port, &management)?;
    check_bound(config.listen_host, config.component_port, &component)?;
    Ok((management, component))
}

pub async fn serve(config: Arc<Config>, management: TcpListener, component: TcpListener) -> Result<(), Fault> {
    // Hold the sender. Dropping it closes the watch and both listeners stop.
    let (_stop, stop_rx) = watch::channel(false);
    serve_with(config, management, component, Arc::new(OsNonce), stop_rx).await
}

pub fn spawn_server(config: Arc<Config>, management: TcpListener, component: TcpListener, nonces: Arc<dyn NonceSource>) -> ServerHandle {
    let (stop, stop_rx) = watch::channel(false);
    let (ready_tx, ready_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let prepared = prepare(config, nonces).await;
        let (app, management_tls, component_tls) = match prepared {
            Ok(value) => value,
            Err(fault) => {
                let _ = ready_tx.send(Err(fault));
                return Err(fault);
            }
        };
        let _ = ready_tx.send(Ok(()));
        run(app, management, component, management_tls, component_tls, stop_rx).await
    });
    ServerHandle { stop, task, ready: Some(ready_rx) }
}

async fn serve_with(
    config: Arc<Config>,
    management: TcpListener,
    component: TcpListener,
    nonces: Arc<dyn NonceSource>,
    stop_rx: watch::Receiver<bool>,
) -> Result<(), Fault> {
    let (app, management_tls, component_tls) = prepare(config, nonces).await?;
    run(app, management, component, management_tls, component_tls, stop_rx).await
}

async fn prepare(config: Arc<Config>, nonces: Arc<dyn NonceSource>) -> Result<(App, ServerConfigOwned, ServerConfigOwned), Fault> {
    let store = Arc::new(Store::connect(&config, nonces).await?);
    store.require_installed().await?;
    let management_tls = server_config(&config.tls_cert_pem, &config.tls_key_pem)?;
    let component_tls = component_config(&config.tls_cert_pem, &config.tls_key_pem, &config.client_ca_pem)?;
    Ok((App { config, store }, management_tls, component_tls))
}

type ServerConfigOwned = rustls::ServerConfig;

async fn run(
    app: App,
    management: TcpListener,
    component: TcpListener,
    management_tls: rustls::ServerConfig,
    component_tls: rustls::ServerConfig,
    stop_rx: watch::Receiver<bool>,
) -> Result<(), Fault> {
    let management_listener = TlsListener { tcp: management, acceptor: TlsAcceptor::from(Arc::new(management_tls)) };
    let component_listener = ComponentListener { tcp: component, acceptor: TlsAcceptor::from(Arc::new(component_tls)) };
    let management_stop = stop_rx.clone();
    let component_router = component_router(app.clone()).into_make_service_with_connect_info::<WorkloadPeer>();
    let (left, right) = tokio::join!(
        axum::serve(management_listener, management_router(app)).with_graceful_shutdown(watch_stop(management_stop)),
        axum::serve(component_listener, component_router).with_graceful_shutdown(watch_stop(stop_rx)),
    );
    left.map_err(|_| Fault::not_ready())?;
    right.map_err(|_| Fault::not_ready())?;
    Ok(())
}

async fn watch_stop(mut receiver: watch::Receiver<bool>) {
    loop {
        if *receiver.borrow() {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

fn check_bound(host: std::net::IpAddr, port: u16, listener: &TcpListener) -> Result<(), Fault> {
    let bound = listener.local_addr().map_err(|_| Fault::not_ready())?;
    if bound.ip() != host || (port != 0 && bound.port() != port) { Err(Fault::malformed()) } else { Ok(()) }
}

fn management_router(app: App) -> Router {
    Router::new()
        .route("/control/v1/projects/{project_id}/namespaces/{namespace}/secrets/{secret_id}", put(write).get(status).fallback(method_not_allowed))
        .route("/control/v1/projects/{project_id}/namespaces/{namespace}/secrets/{secret_id}/rotate", post(rotate).fallback(method_not_allowed))
        .route("/control/v1/projects/{project_id}/namespaces/{namespace}/secrets/{secret_id}/revoke", post(revoke).fallback(method_not_allowed))
        .fallback(not_found)
        .with_state(app)
}

fn component_router(app: App) -> Router {
    Router::new().route("/component/v1/resolve", post(resolve).fallback(method_not_allowed)).fallback(not_found).with_state(app)
}

async fn write(State(app): State<App>, Path(path): Path<SecretPath>, request: Request) -> Response {
    timed(write_in(&app, path, request)).await
}

async fn status(State(app): State<App>, Path(path): Path<SecretPath>, request: Request) -> Response {
    timed(status_in(&app, path, request)).await
}

async fn rotate(State(app): State<App>, Path(path): Path<SecretPath>, request: Request) -> Response {
    timed(rotate_in(&app, path, request)).await
}

async fn revoke(State(app): State<App>, Path(path): Path<SecretPath>, request: Request) -> Response {
    timed(revoke_in(&app, path, request)).await
}

async fn resolve(State(app): State<App>, ConnectInfo(peer): ConnectInfo<WorkloadPeer>, request: Request) -> Response {
    timed(resolve_in(&app, peer, request)).await
}

async fn method_not_allowed() -> Response {
    fault_response(Fault::method())
}

async fn not_found() -> Response {
    fault_response(Fault::not_found())
}

async fn write_in(app: &App, path: SecretPath, request: Request) -> Response {
    let headers = request.headers().clone();
    let body = match take_body(request, false).await {
        Ok(body) => body,
        Err(fault) => return fault_response(fault),
    };
    let hash = sha256_hex(&body);
    let parsed = match parse_mutation(&body, MutationKind::Write) {
        Ok(parsed) => parsed,
        Err(fault) => return fault_response(fault),
    };
    let scope = match scope_from(app, &path) {
        Ok(scope) => scope,
        Err(fault) => return fault_response(fault),
    };
    let identity = match authorize(app, &headers, &parsed, &scope.project).await {
        Ok(identity) => identity,
        Err(fault) => return fault_response(fault),
    };
    let Some(value) = parsed.value else {
        return fault_response(Fault::malformed());
    };
    match app.store.create(WriteRequest {
        scope: &scope,
        workload: &parsed.workload,
        subject: &identity.subject,
        value: &value,
        body_hash: &hash,
        idempotency_key: &parsed.idempotency_key,
        key: &app.config.master_key,
    }).await {
        Ok(mutation) => json_response(if mutation.replayed { 200 } else { 201 }, mutation.body),
        Err(fault) => fault_response(fault),
    }
}

async fn rotate_in(app: &App, path: SecretPath, request: Request) -> Response {
    let headers = request.headers().clone();
    let body = match take_body(request, false).await {
        Ok(body) => body,
        Err(fault) => return fault_response(fault),
    };
    let hash = sha256_hex(&body);
    let parsed = match parse_mutation(&body, MutationKind::Rotate) {
        Ok(parsed) => parsed,
        Err(fault) => return fault_response(fault),
    };
    let scope = match scope_from(app, &path) {
        Ok(scope) => scope,
        Err(fault) => return fault_response(fault),
    };
    let identity = match authorize(app, &headers, &parsed, &scope.project).await {
        Ok(identity) => identity,
        Err(fault) => return fault_response(fault),
    };
    let (Some(value), Some(expected)) = (parsed.value, parsed.expected_version) else {
        return fault_response(Fault::malformed());
    };
    match app.store.rotate(RotateRequest {
        scope: &scope,
        workload: &parsed.workload,
        subject: &identity.subject,
        value: &value,
        expected_version: expected,
        body_hash: &hash,
        idempotency_key: &parsed.idempotency_key,
        key: &app.config.master_key,
    }).await {
        Ok(mutation) => json_response(200, mutation.body),
        Err(fault) => fault_response(fault),
    }
}

async fn revoke_in(app: &App, path: SecretPath, request: Request) -> Response {
    let headers = request.headers().clone();
    let body = match take_body(request, false).await {
        Ok(body) => body,
        Err(fault) => return fault_response(fault),
    };
    let hash = sha256_hex(&body);
    let parsed = match parse_mutation(&body, MutationKind::Revoke) {
        Ok(parsed) => parsed,
        Err(fault) => return fault_response(fault),
    };
    let scope = match scope_from(app, &path) {
        Ok(scope) => scope,
        Err(fault) => return fault_response(fault),
    };
    let identity = match authorize(app, &headers, &parsed, &scope.project).await {
        Ok(identity) => identity,
        Err(fault) => return fault_response(fault),
    };
    match app.store.revoke(&scope, &identity.subject, &hash, &parsed.idempotency_key).await {
        Ok(mutation) => json_response(200, mutation.body),
        Err(fault) => fault_response(fault),
    }
}

async fn status_in(app: &App, path: SecretPath, request: Request) -> Response {
    let headers = request.headers().clone();
    if let Err(fault) = take_body(request, true).await {
        return fault_response(fault);
    }
    let scope = match scope_from(app, &path) {
        Ok(scope) => scope,
        Err(fault) => return fault_response(fault),
    };
    let cookie = match session_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(fault) => return fault_response(fault),
    };
    let _guard = auth_gate().lock().await;
    let identity = match fetch_current(&app.config.identity_origin, &app.config.identity_ca_pem, &cookie).await {
        Ok(identity) => identity,
        Err(fault) => return fault_response(fault),
    };
    if let Err(fault) = identity.require_admin(&scope.project) {
        return fault_response(fault);
    }
    match app.store.status(&scope).await {
        Ok(body) => json_response(200, body),
        Err(fault) => fault_response(fault),
    }
}

async fn resolve_in(app: &App, peer: WorkloadPeer, request: Request) -> Response {
    let body = match take_body(request, false).await {
        Ok(body) => body,
        Err(fault) => return fault_response(fault),
    };
    let parsed = match parse_resolve(&body) {
        Ok(parsed) => parsed,
        Err(fault) => return fault_response(fault),
    };
    if parsed.scope.deployment != app.config.deployment {
        return fault_response(Fault::not_found());
    }
    if parsed.project_id != parsed.scope.project {
        return fault_response(Fault::forbidden());
    }
    if parsed.namespace != parsed.scope.namespace {
        return fault_response(Fault::namespace());
    }
    if !workload_ok(&peer.workload) {
        return fault_response(Fault::workload());
    }
    match app.store.resolve(&parsed.scope, &peer.workload, parsed.version, &app.config.master_key).await {
        Ok(resolved) => {
            let mut meta = resolved.meta;
            meta["value"] = Value::String(resolved.value.to_string());
            json_response(200, meta)
        }
        Err(fault) => fault_response(fault),
    }
}

fn auth_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

async fn authorize(app: &App, headers: &HeaderMap, parsed: &ParsedMutation, project_id: &str) -> Result<crate::identity::VerifiedIdentity, Fault> {
    let cookie = session_cookie(headers)?;
    let (origin, csrf) = presented(one_header(headers, "origin")?, one_header(headers, "x-oo-csrf")?, &app.config.https_origin)?;
    let _guard = auth_gate().lock().await;
    confirm_presented_csrf(&app.config.identity_origin, &app.config.identity_ca_pem, &cookie, origin, csrf).await?;
    let identity = fetch_current(&app.config.identity_origin, &app.config.identity_ca_pem, &cookie).await?;
    if parsed.revision != identity.revision {
        return Err(Fault::revision());
    }
    identity.require_admin(project_id)?;
    Ok(identity)
}

fn scope_from(app: &App, path: &SecretPath) -> Result<Scope, Fault> {
    if !ident_ok(&path.project_id) || !ident_ok(&path.namespace) || !ident_ok(&path.secret_id) {
        return Err(Fault::malformed());
    }
    Ok(Scope {
        deployment: app.config.deployment.clone(),
        project: path.project_id.clone(),
        namespace: path.namespace.clone(),
        secret_id: path.secret_id.clone(),
    })
}

#[derive(Clone, Copy)]
enum MutationKind {
    Write,
    Rotate,
    Revoke,
}

struct ParsedMutation {
    revision: i64,
    idempotency_key: String,
    workload: String,
    expected_version: Option<i32>,
    value: Option<Zeroizing<String>>,
}

struct ParsedResolve {
    scope: Scope,
    project_id: String,
    namespace: String,
    version: Option<i32>,
}

fn parse_mutation(bytes: &Zeroizing<Vec<u8>>, kind: MutationKind) -> Result<ParsedMutation, Fault> {
    let value = parse_exact(bytes)?;
    let mut object = match value {
        Value::Object(map) => map,
        _ => return Err(Fault::malformed()),
    };
    let keys: &[&str] = match kind {
        MutationKind::Write => &["identity_revision", "idempotency_key", "workload", "value"],
        MutationKind::Rotate => &["identity_revision", "idempotency_key", "workload", "expected_version", "value"],
        MutationKind::Revoke => &["identity_revision", "idempotency_key"],
    };
    exact_keys(&object, keys)?;
    let revision = positive_i64(object.get("identity_revision"))?;
    let idempotency_key = required_string(&object, "idempotency_key")?;
    if !idempotency_ok(&idempotency_key) {
        return Err(Fault::malformed());
    }
    let (workload, expected_version, value) = match kind {
        MutationKind::Revoke => (String::new(), None, None),
        MutationKind::Write | MutationKind::Rotate => {
            let workload = required_string(&object, "workload")?;
            if !workload_ok(&workload) {
                return Err(Fault::malformed());
            }
            let expected_version = if matches!(kind, MutationKind::Rotate) { Some(version_number(object.get("expected_version"))?) } else { None };
            let Some(Value::String(text)) = object.remove("value") else {
                return Err(Fault::malformed());
            };
            if text.is_empty() || text.len() > 4096 {
                return Err(Fault::malformed());
            }
            (workload, expected_version, Some(Zeroizing::new(text)))
        }
    };
    Ok(ParsedMutation { revision, idempotency_key, workload, expected_version, value })
}

fn parse_resolve(bytes: &Zeroizing<Vec<u8>>) -> Result<ParsedResolve, Fault> {
    let value = parse_exact(bytes)?;
    let object = value.as_object().ok_or_else(Fault::malformed)?;
    if object.len() != 3 && object.len() != 4 {
        return Err(Fault::malformed());
    }
    if !object.contains_key("secret_ref") || !object.contains_key("project_id") || !object.contains_key("namespace") {
        return Err(Fault::malformed());
    }
    if object.len() == 4 && !object.contains_key("version") {
        return Err(Fault::malformed());
    }
    let secret_ref = required_string(object, "secret_ref")?;
    let scope = Scope::parse_ref(&secret_ref)?;
    let project_id = required_string(object, "project_id")?;
    let namespace = required_string(object, "namespace")?;
    if !ident_ok(&project_id) || !ident_ok(&namespace) {
        return Err(Fault::malformed());
    }
    let version = if object.contains_key("version") { Some(version_number(object.get("version"))?) } else { None };
    Ok(ParsedResolve { scope, project_id, namespace, version })
}

fn exact_keys(object: &Map<String, Value>, keys: &[&str]) -> Result<(), Fault> {
    if object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)) { Ok(()) } else { Err(Fault::malformed()) }
}

fn required_string(object: &Map<String, Value>, key: &str) -> Result<String, Fault> {
    object.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned).ok_or_else(Fault::malformed)
}

fn positive_i64(value: Option<&Value>) -> Result<i64, Fault> {
    let number = value.and_then(Value::as_i64).ok_or_else(Fault::malformed)?;
    if number < 1 || value.and_then(Value::as_u64).is_none() { Err(Fault::malformed()) } else { Ok(number) }
}

fn version_number(value: Option<&Value>) -> Result<i32, Fault> {
    let number = positive_i64(value)?;
    if number > 1_000_000 { Err(Fault::malformed()) } else { i32::try_from(number).map_err(|_| Fault::malformed()) }
}

async fn take_body(request: Request, empty: bool) -> Result<Zeroizing<Vec<u8>>, Fault> {
    if request.uri().query().is_some() {
        return Err(Fault::malformed());
    }
    if !empty {
        let content_type = request.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        if content_type != Some("application/json") {
            return Err(Fault::malformed());
        }
    }
    let bytes = axum::body::to_bytes(request.into_body(), 16_384).await.map_err(|_| Fault::malformed())?;
    if empty && !bytes.is_empty() || !empty && bytes.is_empty() {
        return Err(Fault::malformed());
    }
    Ok(Zeroizing::new(bytes.to_vec()))
}

fn session_cookie(headers: &HeaderMap) -> Result<String, Fault> {
    let Some(value) = headers.get(header::COOKIE) else {
        return Err(Fault::unauthenticated());
    };
    let text = value.to_str().map_err(|_| Fault::unauthenticated())?;
    let mut found = None;
    for part in text.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((name, cookie)) = part.split_once('=') else {
            return Err(Fault::malformed());
        };
        if name.trim() != "__Host-oo-session" {
            continue;
        }
        if found.is_some() {
            return Err(Fault::unauthenticated());
        }
        found = Some(cookie.trim().to_owned());
    }
    let cookie = found.ok_or_else(Fault::unauthenticated)?;
    if session_cookie_ok(&cookie) { Ok(cookie) } else { Err(Fault::unauthenticated()) }
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, Fault> {
    let mut values = headers.get_all(name).iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(Fault::malformed());
    }
    first.to_str().map(Some).map_err(|_| Fault::malformed())
}

async fn timed<F>(future: F) -> Response
where
    F: Future<Output = Response>,
{
    match timeout(Duration::from_secs(5), future).await {
        Ok(response) => response,
        Err(_elapsed) => fault_response(Fault::not_ready()),
    }
}

fn fault_response(fault: Fault) -> Response {
    json_response(fault.http, error_body(&fault))
}

fn json_response(status: u16, body: Value) -> Response {
    let mut response = Response::new(Body::from(serde_json::to_vec(&body).unwrap_or_default()));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

impl Connected<IncomingStream<'_, ComponentListener>> for WorkloadPeer {
    fn connect_info(stream: IncomingStream<'_, ComponentListener>) -> Self {
        stream.remote_addr().clone()
    }
}

pub fn bound_addr(listener: &TcpListener) -> Result<SocketAddr, Fault> {
    listener.local_addr().map_err(|_| Fault::not_ready())
}
