//! HTTPS routes. Caller JSON is not authority.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, RawQuery, Request, State};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use crate::config::Config;
use crate::error::{ApiError, Fault, CONTRACT};
use crate::identity::{self, authorize, authorize_mutation, Scope, VerifiedIdentity};
use crate::ids::{ct_eq, hex64, ident_ok, idempotency_ok, operation_id_ok, role_ok, token_ok};
use crate::jsonutil::{canonical_sha, parse_request};
use crate::pack::{self, permission_compatible, Lineage, LineageFields};
use crate::store::{self, Operation, PublishDraft, RollbackDraft};
use crate::tls::{acceptor, TlsListener};

pub use crate::store::Runtime;

const CREATE_KEYS: &[&str] = &[
    "base_version",
    "candidate_id",
    "config_commit",
    "expected_version",
    "idempotency_key",
    "identity_revision",
    "project_id",
    "role",
    "scope",
    "version",
];
const ROLLBACK_KEYS: &[&str] = &["expected_version", "idempotency_key", "identity_revision", "project_id"];

pub async fn listen(config: &Config) -> Result<TcpListener, Fault> {
    let listener = TcpListener::bind((config.listen_host, config.listen_port)).await.map_err(|_| Fault::refused("dependency.not_ready"))?;
    let bound = listener.local_addr().map_err(|_| Fault::malformed())?;
    if bound.ip() != config.listen_host || (config.listen_port != 0 && bound.port() != config.listen_port) {
        return Err(Fault::malformed());
    }
    Ok(listener)
}

pub fn bound_addr(listener: &TcpListener) -> Result<SocketAddr, Fault> {
    listener.local_addr().map_err(|_| Fault::malformed())
}

pub async fn serve(config: Arc<Config>, listener: TcpListener) -> Result<(), Fault> {
    bound_addr(&listener)?;
    let runtime = Runtime::connect(Arc::clone(&config)).await?;
    runtime.boot().await?;
    print_line(&json!({
        "contract": CONTRACT,
        "outcome": "listening",
        "production_ready": false,
        "full_sp08_accepted": false
    }))?;
    let worker = runtime.clone();
    tokio::spawn(async move { worker.run_worker().await });
    let tls = acceptor(&config.tls_cert_pem, &config.tls_key_pem)?;
    axum::serve(TlsListener { tcp: listener, acceptor: tls }, router(runtime)).await.map_err(|_| Fault::refused("dependency.not_ready"))
}

fn router(runtime: Runtime) -> Router {
    Router::new()
        .route("/control/v1/releases/active/pack", get(active_pack).fallback(method_not_allowed))
        .route("/control/v1/releases/active", get(active).fallback(method_not_allowed))
        .route("/control/v1/releases/{operation_id}/rollback", post(rollback).fallback(method_not_allowed))
        .route("/control/v1/releases/{operation_id}", get(get_release).fallback(method_not_allowed))
        .route("/control/v1/releases", get(list_releases).post(create_release).fallback(method_not_allowed))
        .fallback(not_found)
        .layer(DefaultBodyLimit::disable())
        .with_state(runtime)
}

async fn create_release(State(runtime): State<Runtime>, request: Request) -> Response {
    let headers = request.headers().clone();
    let cookie = match session_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(error) => return error_response(error),
    };
    let value = match read_json(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let parsed = match parse_create(&value) {
        Ok(parsed) => parsed,
        Err(error) => return error_response(error),
    };
    let identity = match authorize_mutation(
        runtime.identity_client(),
        runtime.identity_origin(),
        &cookie,
        header_str(&headers, header::ORIGIN),
        header_str(&headers, "x-oo-csrf"),
        runtime.origins(),
        parsed.revision,
    )
    .await
    {
        Ok(identity) => identity,
        Err(error) => return error_response(error),
    };
    if let Err(error) = authorize(&identity, &parsed.project_id, runtime.project_id(), Scope::Create) {
        return error_response(error);
    }
    let body_sha = canonical_sha(&value);
    match replay_of(&runtime, &parsed.idempotency_key, &body_sha, "createRelease").await {
        Replay::Response(response) => response,
        Replay::Fresh => match queue_publish(&runtime, &identity, parsed, body_sha).await {
            Ok(response) => response,
            Err(error) => error_response(error),
        },
    }
}

async fn list_releases(State(runtime): State<Runtime>, RawQuery(query): RawQuery, request: Request) -> Response {
    let identity = match authenticate(&runtime, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let project_id = match one_query(query.as_deref(), &["project_id"]) {
        Ok(pairs) => pairs[0].1.clone(),
        Err(error) => return error_response(error),
    };
    if !ident_ok(&project_id) {
        return error_response(ApiError::malformed("query"));
    }
    if let Err(error) = authorize(&identity, &project_id, runtime.project_id(), Scope::Read) {
        return error_response(error);
    }
    let rows = match runtime.list_project().await {
        Ok(rows) => rows,
        Err(error) => return error_response(error),
    };
    let releases = rows.iter().map(|row| release_body(kind_name(&row.kind), row, false)).collect::<Vec<_>>();
    json_response(200, &json!({
        "contract": CONTRACT,
        "operation": "listReleases",
        "project_id": runtime.project_id(),
        "releases": releases,
        "production_ready": false,
        "full_sp08_accepted": false
    }))
}

async fn active(State(runtime): State<Runtime>, RawQuery(query): RawQuery, request: Request) -> Response {
    let identity = match authenticate(&runtime, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let project_id = match project_only(query.as_deref()) {
        Ok(project_id) => project_id,
        Err(error) => return error_response(error),
    };
    if let Err(error) = authorize(&identity, &project_id, runtime.project_id(), Scope::Read) {
        return error_response(error);
    }
    let pointer = match runtime.pointer().await {
        Ok(pointer) => pointer,
        Err(error) => return error_response(error),
    };
    let class = match certificate_of(&runtime, pointer.version.as_deref()).await {
        Ok(class) => class,
        Err(error) => return error_response(error),
    };
    json_response(200, &json!({
        "contract": CONTRACT,
        "operation": "getActiveRelease",
        "project_id": runtime.project_id(),
        "version": pointer.version,
        "candidate_id": pointer.candidate_id,
        "source_hash": pointer.source_hash,
        "pack_digest": pointer.pack_digest,
        "manifest_sha256": pointer.manifest_sha256,
        "certificate_class": class,
        "production_ready": false,
        "customer_gate_verified": false,
        "owner_authenticated": false,
        "runsc_verified": false,
        "full_sp08_accepted": false,
        "public_deployment_claimed": false,
        "customer_ownership_claimed": false,
        "standby_configured": false,
        "half_published": false,
        "reader": "oo_release_reader"
    }))
}

async fn active_pack(State(runtime): State<Runtime>, RawQuery(query): RawQuery, request: Request) -> Response {
    let identity = match authenticate(&runtime, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let pairs = match one_query(query.as_deref(), &["project_id", "role"]) {
        Ok(pairs) => pairs,
        Err(error) => return error_response(error),
    };
    let project_id = &pairs[0].1;
    let role = &pairs[1].1;
    if !ident_ok(project_id) || !role_ok(role) {
        return error_response(ApiError::malformed("query"));
    }
    if let Err(error) = authorize(&identity, project_id, runtime.project_id(), Scope::Pack) {
        return error_response(error);
    }
    let pointer = match runtime.pointer().await {
        Ok(pointer) => pointer,
        Err(error) => return error_response(error),
    };
    let Some(version) = pointer.version.clone() else {
        return error_response(ApiError::conflict("inactive"));
    };
    let Some(operation) = (match runtime.succeeded_publish(&version).await {
        Ok(operation) => operation,
        Err(error) => return error_response(error),
    }) else {
        return error_response(ApiError::conflict("inactive"));
    };
    if operation.role_name.as_deref() != Some(role.as_str()) {
        return error_response(ApiError::forbidden("role"));
    }
    let receipt = match runtime.role_status(&operation).await {
        Ok(receipt) => receipt,
        Err(error) => return error_response(error),
    };
    if receipt.code != 0 {
        return error_response(ApiError::not_ready("producer"));
    }
    if receipt.body.get("pack_digest").and_then(Value::as_str) != pointer.pack_digest.as_deref() {
        return error_response(ApiError::not_ready("producer"));
    }
    let files = match pack_files(&receipt.body) {
        Ok(files) => files,
        Err(error) => return error_response(error),
    };
    json_response(200, &json!({
        "contract": CONTRACT,
        "operation": "getActiveRolePack",
        "project_id": runtime.project_id(),
        "role": role,
        "version": version,
        "candidate_id": pointer.candidate_id,
        "pack_digest": pointer.pack_digest,
        "manifest_sha256": pointer.manifest_sha256,
        "files": files,
        "certificate_class": class_name(&operation.certificate_class),
        "production_ready": false,
        "customer_gate_verified": false,
        "owner_authenticated": false,
        "runsc_verified": false,
        "replica_production_admissible": false,
        "full_sp08_accepted": false,
        "half_published": false
    }))
}

async fn get_release(State(runtime): State<Runtime>, Path(operation_id): Path<String>, request: Request) -> Response {
    let identity = match authenticate(&runtime, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    if let Err(error) = authorize(&identity, runtime.project_id(), runtime.project_id(), Scope::Read) {
        return error_response(error);
    }
    match visible_operation(&runtime, &operation_id).await {
        Ok(operation) => json_response(200, &release_body("getRelease", &operation, false)),
        Err(error) => error_response(error),
    }
}

async fn rollback(State(runtime): State<Runtime>, Path(operation_id): Path<String>, request: Request) -> Response {
    let headers = request.headers().clone();
    let cookie = match session_cookie(&headers) {
        Ok(cookie) => cookie,
        Err(error) => return error_response(error),
    };
    let value = match read_json(request).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let parsed = match parse_rollback(&value) {
        Ok(parsed) => parsed,
        Err(error) => return error_response(error),
    };
    let identity = match authorize_mutation(
        runtime.identity_client(),
        runtime.identity_origin(),
        &cookie,
        header_str(&headers, header::ORIGIN),
        header_str(&headers, "x-oo-csrf"),
        runtime.origins(),
        parsed.revision,
    )
    .await
    {
        Ok(identity) => identity,
        Err(error) => return error_response(error),
    };
    if let Err(error) = authorize(&identity, &parsed.project_id, runtime.project_id(), Scope::Rollback) {
        return error_response(error);
    }
    let body_sha = canonical_sha(&value);
    match replay_of(&runtime, &parsed.idempotency_key, &body_sha, "rollbackRelease").await {
        Replay::Response(response) => response,
        Replay::Fresh => match queue_rollback(&runtime, &identity, &operation_id, parsed, body_sha).await {
            Ok(response) => response,
            Err(error) => error_response(error),
        },
    }
}

async fn method_not_allowed() -> Response {
    error_response(ApiError::method())
}

async fn not_found() -> Response {
    error_response(ApiError::not_found())
}

enum Replay {
    Response(Response),
    Fresh,
}

async fn replay_of(runtime: &Runtime, key: &str, body_sha: &str, operation_name: &str) -> Replay {
    let id = store::operation_id(runtime.project_id(), key);
    match runtime.operation(&id).await {
        Ok(Some(existing)) if ct_eq(&existing.body_sha256, body_sha) => Replay::Response(json_response(202, &release_body(operation_name, &existing, true))),
        Ok(Some(_)) => Replay::Response(error_response(ApiError::idempotency())),
        Ok(None) => Replay::Fresh,
        Err(error) => Replay::Response(error_response(error)),
    }
}

async fn queue_publish(runtime: &Runtime, identity: &VerifiedIdentity, parsed: CreateBody, body_sha: String) -> Result<Response, ApiError> {
    let pointer = runtime.pointer().await?;
    if pointer.version.as_deref() != parsed.base_version.as_deref() {
        return Err(ApiError::conflict("pointer"));
    }
    let prepared = pack::load_pack(runtime.pack_dir(), &parsed.config_commit, &parsed.role)?;
    if prepared.lineage.role != parsed.role {
        return Err(ApiError::pack("role"));
    }
    pack::load_intake(runtime.pack_dir(), &parsed.config_commit)?;
    let queued = runtime
        .enqueue_publish(PublishDraft {
            identity: identity.clone(),
            body_sha256: body_sha,
            idempotency_key: parsed.idempotency_key,
            config_commit: parsed.config_commit,
            candidate_id: parsed.candidate_id,
            version: parsed.version,
            base_version: parsed.base_version,
            expected_version: parsed.expected_version,
            scope: parsed.scope,
            prepared,
        })
        .await?;
    Ok(json_response(202, &release_body("createRelease", &queued.operation, queued.replay)))
}

async fn queue_rollback(runtime: &Runtime, identity: &VerifiedIdentity, operation_id: &str, parsed: RollbackBody, body_sha: String) -> Result<Response, ApiError> {
    let target = visible_operation(runtime, operation_id).await?;
    if target.kind != "publish" || target.status != "succeeded" {
        return Err(ApiError::conflict("pointer"));
    }
    let pointer = runtime.pointer().await?;
    if pointer.version.as_deref() != Some(parsed.expected_version.as_str()) || target.version.as_deref() != Some(parsed.expected_version.as_str()) {
        return Err(ApiError::conflict("pointer"));
    }
    let active = lineage_of(&target);
    let restored = match target.base_version.as_deref() {
        None => None,
        Some(version) => {
            let row = runtime.succeeded_publish(version).await?.ok_or(ApiError::compat())?;
            Some(lineage_of(&row))
        }
    };
    if !permission_compatible(restored.as_ref(), &active) {
        return Err(ApiError::compat());
    }
    let queued = runtime
        .enqueue_rollback(RollbackDraft {
            identity: identity.clone(),
            body_sha256: body_sha,
            idempotency_key: parsed.idempotency_key,
            expected_version: parsed.expected_version,
            target,
            restored: None,
        })
        .await?;
    Ok(json_response(202, &release_body("rollbackRelease", &queued.operation, queued.replay)))
}

async fn visible_operation(runtime: &Runtime, operation_id: &str) -> Result<Operation, ApiError> {
    if !operation_id_ok(operation_id) {
        return Err(ApiError::forbidden("scope"));
    }
    match runtime.operation(operation_id).await? {
        Some(operation) if operation.project_id == runtime.project_id() => Ok(operation),
        _ => Err(ApiError::forbidden("scope")),
    }
}

async fn certificate_of(runtime: &Runtime, version: Option<&str>) -> Result<&'static str, ApiError> {
    let Some(version) = version else {
        return Ok("not_issued");
    };
    let class = match runtime.succeeded_publish(version).await? {
        Some(operation) => class_name(&operation.certificate_class),
        None => "not_issued",
    };
    Ok(class)
}

async fn authenticate(runtime: &Runtime, headers: &HeaderMap) -> Result<VerifiedIdentity, Response> {
    let cookie = session_cookie(headers).map_err(error_response)?;
    identity::fetch_current(runtime.identity_client(), runtime.identity_origin(), &cookie).await.map_err(|fault| error_response(fault.api()))
}

async fn read_json(request: Request) -> Result<Value, Response> {
    let bytes = axum::body::to_bytes(request.into_body(), 65_536).await.map_err(|_| error_response(ApiError::malformed("json")))?;
    parse_request(&bytes).map_err(error_response)
}

struct CreateBody {
    project_id: String,
    revision: i64,
    idempotency_key: String,
    config_commit: String,
    candidate_id: String,
    version: String,
    base_version: Option<String>,
    role: String,
    scope: String,
    expected_version: Option<String>,
}

struct RollbackBody {
    project_id: String,
    revision: i64,
    idempotency_key: String,
    expected_version: String,
}

fn parse_create(value: &Value) -> Result<CreateBody, ApiError> {
    let object = exact_object(value, CREATE_KEYS)?;
    let base_version = optional_token(object.get("base_version").unwrap_or(&Value::Null))?;
    let expected_version = optional_token(object.get("expected_version").unwrap_or(&Value::Null))?;
    if base_version != expected_version {
        return Err(ApiError::malformed("version"));
    }
    let scope = required_str(object, "scope")?;
    if scope != "test_scope" && scope != "production" {
        return Err(ApiError::malformed("scope"));
    }
    Ok(CreateBody {
        project_id: required_ident(object, "project_id")?,
        revision: required_revision(object)?,
        idempotency_key: required_key(object)?,
        config_commit: required_hex(object, "config_commit")?,
        candidate_id: required_token(object, "candidate_id")?,
        version: required_token(object, "version")?,
        base_version,
        role: required_role(object)?,
        scope,
        expected_version,
    })
}

fn parse_rollback(value: &Value) -> Result<RollbackBody, ApiError> {
    let object = exact_object(value, ROLLBACK_KEYS)?;
    Ok(RollbackBody {
        project_id: required_ident(object, "project_id")?,
        revision: required_revision(object)?,
        idempotency_key: required_key(object)?,
        expected_version: required_token(object, "expected_version")?,
    })
}

fn exact_object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a serde_json::Map<String, Value>, ApiError> {
    let object = value.as_object().ok_or(ApiError::malformed("json"))?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(ApiError::malformed("json"));
    }
    Ok(object)
}

fn required_str(object: &serde_json::Map<String, Value>, key: &'static str) -> Result<String, ApiError> {
    object.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_owned).ok_or(ApiError::malformed(key))
}

fn required_ident(object: &serde_json::Map<String, Value>, key: &'static str) -> Result<String, ApiError> {
    let text = required_str(object, key)?;
    if ident_ok(&text) { Ok(text) } else { Err(ApiError::malformed(key)) }
}

fn required_token(object: &serde_json::Map<String, Value>, key: &'static str) -> Result<String, ApiError> {
    let text = required_str(object, key)?;
    if token_ok(&text) { Ok(text) } else { Err(ApiError::malformed(key)) }
}

fn required_hex(object: &serde_json::Map<String, Value>, key: &'static str) -> Result<String, ApiError> {
    let text = required_str(object, key)?;
    if hex64(&text) { Ok(text) } else { Err(ApiError::malformed(key)) }
}

fn required_role(object: &serde_json::Map<String, Value>) -> Result<String, ApiError> {
    let text = required_str(object, "role")?;
    if role_ok(&text) { Ok(text) } else { Err(ApiError::malformed("role")) }
}

fn required_key(object: &serde_json::Map<String, Value>) -> Result<String, ApiError> {
    let text = required_str(object, "idempotency_key")?;
    if idempotency_ok(&text) { Ok(text) } else { Err(ApiError::malformed("idempotency")) }
}

fn required_revision(object: &serde_json::Map<String, Value>) -> Result<i64, ApiError> {
    let value = object.get("identity_revision").ok_or(ApiError::malformed("revision"))?;
    let revision = value.as_i64().ok_or(ApiError::malformed("revision"))?;
    if revision >= 1 && value.as_u64().is_some() { Ok(revision) } else { Err(ApiError::malformed("revision")) }
}

fn optional_token(value: &Value) -> Result<Option<String>, ApiError> {
    if value.is_null() {
        return Ok(None);
    }
    let text = value.as_str().ok_or(ApiError::malformed("version"))?;
    if token_ok(text) { Ok(Some(text.to_owned())) } else { Err(ApiError::malformed("version")) }
}

fn project_only(query: Option<&str>) -> Result<String, ApiError> {
    let pairs = one_query(query, &["project_id"])?;
    if ident_ok(&pairs[0].1) { Ok(pairs[0].1.clone()) } else { Err(ApiError::malformed("query")) }
}

fn one_query(query: Option<&str>, keys: &[&str]) -> Result<Vec<(String, String)>, ApiError> {
    let Some(query) = query.filter(|text| !text.is_empty()) else {
        return Err(ApiError::malformed("query"));
    };
    if query.contains(['%', '+', '#', '?', ' ']) {
        return Err(ApiError::malformed("query"));
    }
    let mut pairs = Vec::new();
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(ApiError::malformed("query"));
        };
        if key.is_empty() || pairs.iter().any(|(found, _)| found == key) {
            return Err(ApiError::malformed("query"));
        }
        pairs.push((key.to_owned(), value.to_owned()));
    }
    if pairs.len() != keys.len() || keys.iter().any(|key| pairs.iter().all(|(found, _)| found.as_str() != *key)) {
        return Err(ApiError::malformed("query"));
    }
    let mut ordered = Vec::new();
    for key in keys {
        let value = pairs.iter().find(|(found, _)| found == *key).map(|(_, value)| value.clone()).ok_or(ApiError::malformed("query"))?;
        ordered.push(((*key).to_owned(), value));
    }
    Ok(ordered)
}

fn session_cookie(headers: &HeaderMap) -> Result<String, ApiError> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        let raw = value.to_str().map_err(|_| ApiError::unauthenticated())?;
        for part in raw.split(';') {
            let Some(cookie) = part.trim().strip_prefix("__Host-oo-session=") else {
                continue;
            };
            if found.is_some() || !crate::ids::session_cookie_ok(cookie) {
                return Err(ApiError::unauthenticated());
            }
            found = Some(cookie.to_owned());
        }
    }
    found.ok_or(ApiError::unauthenticated())
}

fn header_str<K: header::AsHeaderName>(headers: &HeaderMap, name: K) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn release_body(operation_name: &str, operation: &Operation, replay: bool) -> Value {
    json!({
        "contract": CONTRACT,
        "operation": operation_name,
        "operation_id": operation.operation_id,
        "project_id": operation.project_id,
        "status": operation.status,
        "kind": operation.kind,
        "scope": operation.scope_name,
        "version": operation.version,
        "candidate_id": operation.candidate_id,
        "role": operation.role_name,
        "config_commit": operation.config_commit,
        "source_hash": operation.source_hash,
        "pack_digest": operation.pack_digest,
        "producer_outcome": operation.producer_outcome,
        "producer_reason": operation.producer_reason,
        "certificate_class": class_name(&operation.certificate_class),
        "production_ready": false,
        "customer_gate_verified": false,
        "owner_authenticated": false,
        "runsc_verified": false,
        "full_sp08_accepted": false,
        "half_published": false,
        "replay": replay,
        "gates": gates_value(&operation.gates_json)
    })
}

fn gates_value(text: &str) -> Value {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(items)) => Value::Array(items),
        _ => Value::Array(Vec::new()),
    }
}

fn pack_files(body: &Value) -> Result<Vec<Value>, ApiError> {
    let listed = body.get("files").and_then(Value::as_array).ok_or(ApiError::not_ready("producer"))?;
    let mut files = Vec::new();
    for file in listed {
        let path = file.get("path").and_then(Value::as_str).ok_or(ApiError::not_ready("producer"))?;
        let sha = file.get("sha256").and_then(Value::as_str).ok_or(ApiError::not_ready("producer"))?;
        let bytes = file.get("bytes").and_then(Value::as_i64).ok_or(ApiError::not_ready("producer"))?;
        if path.is_empty() || path.contains("..") || path.starts_with('/') || path.contains('\\') || !hex64(sha) || bytes < 0 {
            return Err(ApiError::not_ready("producer"));
        }
        files.push(json!({"path": path, "sha256": sha, "bytes": bytes}));
    }
    Ok(files)
}

fn lineage_of(operation: &Operation) -> Lineage {
    Lineage::from_columns(&operation.columns_text, lineage_fields(operation))
}

fn lineage_fields(operation: &Operation) -> LineageFields<'_> {
    LineageFields {
        role: operation.role_name.as_deref().unwrap_or(""),
        policy_id: operation.policy_id.as_deref().unwrap_or(""),
        policy_version: operation.policy_version.as_deref().unwrap_or(""),
        knowledge_id: operation.knowledge_id.as_deref().unwrap_or(""),
        knowledge_version: operation.knowledge_version.as_deref().unwrap_or(""),
        catalog_version: operation.catalog_version.as_deref().unwrap_or(""),
        review_admitted: operation.review_admitted,
    }
}

fn kind_name(kind: &str) -> &'static str {
    if kind == "rollback" { "rollbackRelease" } else { "getRelease" }
}

fn class_name(value: &str) -> &'static str {
    if value == "test_scope_not_production" { "test_scope_not_production" } else { "not_issued" }
}

fn error_response(error: ApiError) -> Response {
    json_response(error.status, &error.body())
}

fn json_response(status: u16, value: &Value) -> Response {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| {
        br#"{"contract":"oo.release-control-api/v1","code":"malformed","detail":"json","production_ready":false,"full_sp08_accepted":false}"#.to_vec()
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn print_line(value: &Value) -> Result<(), Fault> {
    let text = serde_json::to_string(value).map_err(|_| Fault::malformed())?;
    let mut stdout = std::io::stdout().lock();
    use std::io::Write;
    writeln!(stdout, "{text}").map_err(|_| Fault::refused("dependency.not_ready"))?;
    stdout.flush().map_err(|_| Fault::refused("dependency.not_ready"))?;
    Ok(())
}
