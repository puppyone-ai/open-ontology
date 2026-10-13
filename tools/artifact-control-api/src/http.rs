//! HTTPS routes. Every call re-reads the scope file. A query string is not a grant.

use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Map, Value};
use tokio::net::TcpListener;

use crate::config::Config;
use crate::crypto::{self, ct_eq, sha256_hex};
use crate::error::{card_flags, error_body, Fault, CONTRACT};
use crate::export_run::{intake, Intake};
use crate::paths::{column_ok, idempotency_ok, intake_name_ok};
use crate::scope::{grant_key, read_scope_files, Scope};
use crate::store::{now_unix, Outcome, Store};
use crate::tlsutil::{self, TlsListener};

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    store: Arc<Store>,
}

pub async fn listen(config: &Config) -> Result<TcpListener, Fault> {
    let listener = TcpListener::bind((config.listen_host, config.listen_port)).await.map_err(|_| Fault::not_ready())?;
    let bound = listener.local_addr().map_err(|_| Fault::not_ready())?;
    if bound.ip() != config.listen_host || bound.port() != config.listen_port {
        return Err(Fault::not_ready());
    }
    Ok(listener)
}

pub fn bound_addr(listener: &TcpListener) -> Result<SocketAddr, Fault> {
    listener.local_addr().map_err(|_| Fault::not_ready())
}

pub async fn serve(config: Arc<Config>, tcp: TcpListener) -> Result<(), Fault> {
    let store = Arc::new(Store::connect(&config).await?);
    store.require_installed().await?;
    store.reconcile(&config.object_dir).await?;
    let tls = tlsutil::server_config(&config.tls_cert_pem, &config.tls_key_pem)?;
    let listener = TlsListener { tcp, acceptor: tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(tls)) };
    let app = App { config, store };
    axum::serve(listener, router(app)).await.map_err(|_| Fault::not_ready())?;
    Ok(())
}

fn router(app: App) -> Router {
    Router::new()
        .route("/control/v1/session/consume", post(consume).fallback(method_not_allowed))
        .route("/control/v1/artifacts", get(list).post(accept).fallback(method_not_allowed))
        .route("/control/v1/artifacts/{object_id}/content", get(download).fallback(method_not_allowed))
        .route("/control/v1/artifacts/{object_id}/revoke", post(revoke).fallback(method_not_allowed))
        .route("/control/v1/artifacts/{object_id}", get(detail).fallback(method_not_allowed))
        .route("/control/v1/cleanup", post(cleanup).fallback(method_not_allowed))
        .fallback(not_found)
        .with_state(app)
}

async fn consume(State(app): State<App>, request: Request) -> Response {
    match consume_in(&app, request).await {
        Ok(response) => response,
        Err(fault) => fault_response(fault),
    }
}

async fn list(State(app): State<App>, request: Request) -> Response {
    match list_in(&app, request).await {
        Ok(body) => json_response(200, body),
        Err(fault) => fault_response(fault),
    }
}

async fn accept(State(app): State<App>, request: Request) -> Response {
    match accept_in(&app, request).await {
        Ok(response) => response,
        Err(fault) => fault_response(fault),
    }
}

async fn detail(State(app): State<App>, Path(object_id): Path<String>, request: Request) -> Response {
    match detail_in(&app, &object_id, request).await {
        Ok(body) => json_response(200, body),
        Err(fault) => fault_response(fault),
    }
}

async fn download(State(app): State<App>, Path(object_id): Path<String>, request: Request) -> Response {
    note("download");
    match download_in(&app, &object_id, request).await {
        Ok(bytes) => {
            note("download-ok");
            csv_response(bytes)
        }
        Err(fault) => {
            note("download-fault");
            fault_response(fault)
        }
    }
}

async fn revoke(State(app): State<App>, Path(object_id): Path<String>, request: Request) -> Response {
    match revoke_in(&app, &object_id, request).await {
        Ok(response) => response,
        Err(fault) => fault_response(fault),
    }
}

async fn cleanup(State(app): State<App>, request: Request) -> Response {
    match cleanup_in(&app, request).await {
        Ok(body) => json_response(200, body),
        Err(fault) => fault_response(fault),
    }
}

async fn consume_in(app: &App, request: Request) -> Result<Response, Fault> {
    let headers = request.headers().clone();
    let bytes = take_body(request, true).await?;
    origin_matches(&app.config.https_origin, &headers)?;
    let scope = read_scope(&app.config)?;
    session_live(app, &scope).await?;
    let object = json_object(&bytes)?;
    reject_grants(&object)?;
    reject_urls(&object)?;
    if object.contains_key("workspace") {
        workspace_value(&object)?;
    }
    let (cookie, csrf) = app.store.consume(&scope).await?;
    let mut body = json!({
        "contract": CONTRACT,
        "csrf": csrf,
        "principal_id": scope.principal_id,
        "project_id": scope.project_id,
        "session_id": scope.session_id,
        "role": scope.role,
        "policy_id": scope.policy_id,
        "knowledge_id": scope.knowledge_id,
        "knowledge_digest": scope.knowledge_digest
    });
    merge_flags(&mut body);
    let mut response = json_response(200, body);
    let cookie_header = format!("oo-artifact={cookie}; HttpOnly; Secure; SameSite=Strict; Path=/control");
    let value = HeaderValue::from_str(&cookie_header).map_err(|_| Fault::not_ready())?;
    response.headers_mut().insert(header::SET_COOKIE, value);
    Ok(response)
}

async fn list_in(app: &App, request: Request) -> Result<Value, Fault> {
    let headers = request.headers().clone();
    let _bytes = take_body(request, true).await?;
    let scope = authorized(app, &headers, false).await?;
    let rows = app.store.list_active(&scope).await?;
    let mut body = json!({ "contract": CONTRACT, "artifacts": rows });
    merge_flags(&mut body);
    Ok(body)
}

async fn accept_in(app: &App, request: Request) -> Result<Response, Fault> {
    let headers = request.headers().clone();
    let bytes = take_body(request, false).await?;
    let scope = authorized(app, &headers, true).await?;
    session_live(app, &scope).await?;
    let object = json_object(&bytes)?;
    reject_grants(&object)?;
    reject_urls(&object)?;
    let accept = parse_accept(&object)?;
    let body_hash = sha256_hex(&bytes);
    let config = Arc::clone(&app.config);
    let source_name = accept.source_file.clone();
    let columns = accept.columns.clone();
    let produced = tokio::task::spawn_blocking(move || intake(&config, &source_name, &columns)).await.map_err(|_| Fault::not_ready())??;
    let outcome = match produced {
        Intake::Ready(ready) => app.store.store_ready(&app.config, &scope, &ready, &accept.idempotency_key, &body_hash).await?,
        Intake::Loss { reason, source_sha256 } => app.store.record_loss(&scope, reason, &source_sha256, &accept.idempotency_key, &body_hash).await?,
    };
    Ok(outcome_response(outcome, 201))
}

async fn detail_in(app: &App, object_id: &str, request: Request) -> Result<Value, Fault> {
    let headers = request.headers().clone();
    let _bytes = take_body(request, true).await?;
    let scope = authorized(app, &headers, false).await?;
    app.store.detail(&scope, object_id).await
}

async fn download_in(app: &App, object_id: &str, request: Request) -> Result<Vec<u8>, Fault> {
    let headers = request.headers().clone();
    let _bytes = take_body(request, true).await?;
    let scope = authorized(app, &headers, false).await?;
    let plain = app.store.download(&app.config, &scope, object_id).await?;
    Ok(plain.to_vec())
}

async fn revoke_in(app: &App, object_id: &str, request: Request) -> Result<Response, Fault> {
    let headers = request.headers().clone();
    let bytes = take_body(request, false).await?;
    let scope = authorized(app, &headers, true).await?;
    let object = json_object(&bytes)?;
    reject_grants(&object)?;
    reject_urls(&object)?;
    let key = parse_mutation_key(&object)?;
    let outcome = app.store.revoke(&scope, object_id, &key, &sha256_hex(&bytes)).await?;
    Ok(outcome_response(outcome, 200))
}

async fn cleanup_in(app: &App, request: Request) -> Result<Value, Fault> {
    let headers = request.headers().clone();
    let bytes = take_body(request, true).await?;
    let _scope = authorized(app, &headers, true).await?;
    if !bytes.is_empty() {
        let object = json_object(&bytes)?;
        reject_grants(&object)?;
        reject_urls(&object)?;
        if object.contains_key("workspace") {
            workspace_value(&object)?;
        }
        for key in object.keys() {
            if key != "version" && key != "workspace" {
                return Err(Fault::malformed());
            }
        }
        if let Some(version) = object.get("version")
            && version.as_str() != Some(CONTRACT)
        {
            return Err(Fault::malformed());
        }
    }
    let report = app.store.cleanup_before(&app.config.object_dir, crate::store::now_ms()?).await?;
    let mut body = json!({
        "contract": CONTRACT,
        "expired": report.expired,
        "idle": report.idle
    });
    merge_flags(&mut body);
    Ok(body)
}

async fn authorized(app: &App, headers: &HeaderMap, mutation: bool) -> Result<Scope, Fault> {
    let scope = read_scope(&app.config)?;
    let cookie = cookie_token(headers)?;
    let (csrf_sha, session_id, principal_id, project_id) = app.store.session_for_cookie(&cookie).await?;
    if session_id != scope.session_id || principal_id != scope.principal_id || project_id != scope.project_id {
        return Err(Fault::forbidden());
    }
    if mutation {
        origin_matches(&app.config.https_origin, headers)?;
        csrf_matches(headers, &csrf_sha)?;
    }
    Ok(scope)
}

async fn session_live(app: &App, scope: &Scope) -> Result<(), Fault> {
    let now = now_unix()?;
    if now >= scope.absolute_end_unix {
        return Err(Fault::expired());
    }
    match app.store.anchor_end(&scope.session_id).await {
        Ok(end) if now >= end => Err(Fault::expired()),
        Ok(_) => Ok(()),
        Err(fault) if fault == Fault::not_found() => Ok(()),
        Err(fault) => Err(fault),
    }
}

fn read_scope(config: &Config) -> Result<Scope, Fault> {
    read_scope_files(&config.scope_file, &config.scope_mac_file, &config.aead_key)
}

fn origin_matches(expected: &str, headers: &HeaderMap) -> Result<(), Fault> {
    let presented = headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()).unwrap_or("");
    if ct_eq(presented.as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(Fault::origin_rejected())
    }
}

fn csrf_matches(headers: &HeaderMap, stored_sha: &str) -> Result<(), Fault> {
    let presented = headers.get("x-oo-csrf").and_then(|value| value.to_str().ok()).unwrap_or("");
    if !crypto::token_ok(presented) {
        return Err(Fault::csrf_rejected());
    }
    let hash = sha256_hex(presented.as_bytes());
    if ct_eq(hash.as_bytes(), stored_sha.as_bytes()) {
        Ok(())
    } else {
        Err(Fault::csrf_rejected())
    }
}

fn cookie_token(headers: &HeaderMap) -> Result<String, Fault> {
    let Some(value) = headers.get(header::COOKIE) else {
        return Err(Fault::unauthenticated());
    };
    let text = value.to_str().map_err(|_| Fault::unauthenticated())?;
    let mut found = None;
    for part in text.split(';') {
        let part = part.trim();
        let Some(token) = part.strip_prefix("oo-artifact=") else {
            continue;
        };
        if found.is_some() || !crypto::token_ok(token) {
            return Err(Fault::unauthenticated());
        }
        found = Some(token.to_owned());
    }
    found.ok_or_else(Fault::unauthenticated)
}

async fn take_body(request: Request, allow_empty: bool) -> Result<Vec<u8>, Fault> {
    if request.uri().query().is_some() {
        return Err(Fault::malformed());
    }
    let content_type = request.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(str::to_owned);
    let length = request.headers().get(header::CONTENT_LENGTH).and_then(|value| value.to_str().ok()).and_then(|text| text.parse::<u64>().ok());
    if length.is_some_and(|value| value > 16_384) {
        return Err(Fault::budget());
    }
    let bytes = axum::body::to_bytes(request.into_body(), 16_384).await.map_err(|_| Fault::budget())?;
    if bytes.is_empty() && allow_empty {
        return Ok(Vec::new());
    }
    if bytes.is_empty() {
        return Err(Fault::malformed());
    }
    if content_type.as_deref() != Some("application/json") {
        return Err(Fault::malformed());
    }
    Ok(bytes.to_vec())
}

fn json_object(bytes: &[u8]) -> Result<Map<String, Value>, Fault> {
    if bytes.is_empty() {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Fault::malformed())?;
    value.as_object().cloned().ok_or_else(Fault::malformed)
}

fn reject_grants(object: &Map<String, Value>) -> Result<(), Fault> {
    if object.keys().any(|key| grant_key(key)) {
        Err(Fault::self_asserted())
    } else {
        Ok(())
    }
}

fn reject_urls(object: &Map<String, Value>) -> Result<(), Fault> {
    for value in object.values() {
        if value.as_str().is_some_and(|text| text.contains("://")) {
            return Err(Fault::path());
        }
        if let Some(items) = value.as_array() {
            for item in items {
                if item.as_str().is_some_and(|text| text.contains("://")) {
                    return Err(Fault::path());
                }
            }
        }
    }
    Ok(())
}

struct AcceptRequest {
    source_file: String,
    columns: Vec<String>,
    idempotency_key: String,
}

fn parse_accept(object: &Map<String, Value>) -> Result<AcceptRequest, Fault> {
    if object.contains_key("workspace") {
        workspace_value(object)?;
    }
    for key in object.keys() {
        if !matches!(key.as_str(), "version" | "source_file" | "columns" | "idempotency_key" | "workspace") {
            return Err(Fault::malformed());
        }
    }
    if object.get("version").and_then(Value::as_str) != Some(CONTRACT) {
        return Err(Fault::malformed());
    }
    let source_file = object.get("source_file").and_then(Value::as_str).ok_or_else(Fault::malformed)?;
    if !intake_name_ok(source_file) {
        return Err(Fault::path());
    }
    let columns_value = object.get("columns").and_then(Value::as_array).ok_or_else(Fault::malformed)?;
    if columns_value.is_empty() || columns_value.len() > 32 {
        return Err(Fault::malformed());
    }
    let mut columns = Vec::new();
    for item in columns_value {
        let name = item.as_str().ok_or_else(Fault::malformed)?;
        if !column_ok(name) || columns.iter().any(|other: &String| other == name) {
            return Err(Fault::malformed());
        }
        columns.push(name.to_owned());
    }
    let idempotency_key = object.get("idempotency_key").and_then(Value::as_str).ok_or_else(Fault::malformed)?;
    if !idempotency_ok(idempotency_key) {
        return Err(Fault::malformed());
    }
    Ok(AcceptRequest { source_file: source_file.to_owned(), columns, idempotency_key: idempotency_key.to_owned() })
}

fn parse_mutation_key(object: &Map<String, Value>) -> Result<String, Fault> {
    if object.contains_key("workspace") {
        workspace_value(object)?;
    }
    for key in object.keys() {
        if !matches!(key.as_str(), "version" | "idempotency_key" | "workspace") {
            return Err(Fault::malformed());
        }
    }
    if object.get("version").and_then(Value::as_str) != Some(CONTRACT) {
        return Err(Fault::malformed());
    }
    let key = object.get("idempotency_key").and_then(Value::as_str).ok_or_else(Fault::malformed)?;
    if !idempotency_ok(key) {
        return Err(Fault::malformed());
    }
    Ok(key.to_owned())
}

fn workspace_value(object: &Map<String, Value>) -> Result<(), Fault> {
    match object.get("workspace") {
        Some(Value::Bool(true)) => Err(Fault::workspace_disabled()),
        Some(Value::Bool(false)) => Ok(()),
        Some(_) => Err(Fault::malformed()),
        None => Ok(()),
    }
}

fn outcome_response(outcome: Outcome, created: u16) -> Response {
    match outcome {
        Outcome::Created(body) => json_response(created, body),
        Outcome::Replay(body) => json_response(200, body),
        Outcome::Loss(body) => json_response(409, body),
    }
}

fn merge_flags(body: &mut Value) {
    if let (Some(object), Some(flags)) = (body.as_object_mut(), card_flags().as_object()) {
        for (key, value) in flags {
            object.insert(key.clone(), value.clone());
        }
    }
}

async fn method_not_allowed() -> Response {
    fault_response(Fault::method_not_allowed())
}

async fn not_found() -> Response {
    fault_response(Fault::not_found())
}

fn fault_response(fault: Fault) -> Response {
    json_response(fault.http, error_body(&fault))
}

fn json_response(status: u16, body: Value) -> Response {
    bytes_response(status, "application/json", serde_json::to_vec(&body).unwrap_or_default())
}

fn csv_response(bytes: Vec<u8>) -> Response {
    bytes_response(200, "text/csv; charset=utf-8", bytes)
}

fn bytes_response(status: u16, content_type: &'static str, bytes: Vec<u8>) -> Response {
    let length = bytes.len().to_string();
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(value) = HeaderValue::from_str(&length) {
        headers.insert(header::CONTENT_LENGTH, value);
    }
    headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    response
}

fn note(message: &str) {
    let _ = writeln!(io::stderr(), "artifact-control {message}");
    let _ = io::stderr().flush();
}
