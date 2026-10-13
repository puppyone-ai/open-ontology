//! HTTPS routes. Caller JSON is not authority.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, RawQuery, Request, State};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Map, Value};

use crate::config::Config;
use crate::error::{ApiError, CONTRACT};
use crate::identity::{self, authorize, mutation_checks, Scope, VerifiedIdentity};
use crate::ids::{ct_eq, hex64, host_ok, ident_ok, idempotency_ok, new_operation_id, operation_id_ok, secret_ref_ok, session_cookie_ok};
use crate::jsonutil::{self, parse_exact};
use crate::sha256::{action_hash, contains_secret, sha256_hex, upload_hash};
use crate::signal::signal_tree;
use crate::store::{CancelApply, ImportRow, NewImport, NewObject, NewSource, ObjectRow, SourceRow, Store, StoreError};

const JSON_LIMIT: usize = 1_048_576;
const CLOSED: &[(&str, &str)] = &[
    ("strategy", "strategy"),
    ("paths", "paths"),
    ("worker", "worker"),
    ("target", "target"),
    ("password", "password"),
    ("password_file", "password_file"),
    ("binaries", "binaries"),
    ("argv", "argv"),
];

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Store,
    pub identity: reqwest::Client,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/control/v1/connectors", get(connectors).fallback(method_not_allowed))
        .route("/control/v1/sources", get(list_sources).post(register_source).fallback(method_not_allowed))
        .route("/control/v1/sources/{source_ref}", get(get_source).fallback(method_not_allowed))
        .route("/control/v1/sources/{source_ref}/catalog", get(get_catalog).fallback(method_not_allowed))
        .route("/control/v1/sources/{source_ref}/objects", post(upload_object).fallback(method_not_allowed))
        .route("/control/v1/imports", get(list_imports).post(create_import).fallback(method_not_allowed))
        .route("/control/v1/imports/{operation_id}", get(get_import).fallback(method_not_allowed))
        .route("/control/v1/imports/{operation_id}/cancel", post(cancel_import).fallback(method_not_allowed))
        .route("/control/v1/imports/{operation_id}/restart", post(restart_import).fallback(method_not_allowed))
        .fallback(not_found)
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
}

pub fn connector_document() -> Value {
    json!({
        "contract": CONTRACT,
        "operation": "getConnectorMatrix",
        "connectors": [
            {"id": "postgres", "mode": "local_private", "version": "17.10", "enterprise": false, "binding": "schema_frozen"},
            {"id": "file", "formats": ["csv", "tsv", "jsonl"], "strategy": "replace", "binding": "schema_frozen"}
        ],
        "not_claimed": [
            "mysql", "parquet", "excel", "sqlite", "dump", "pause", "resnapshot", "dba_apply",
            "semantic_publish", "build_review", "release", "role_grant", "enterprise_source", "customer_idp"
        ],
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    })
}

async fn connectors(State(state): State<AppState>, request: Request) -> Response {
    let identity = match authenticate(&state, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    if let Err(error) = identity::authorize_connector(&identity) {
        return error_response(error);
    }
    json_response(200, &connector_document())
}

async fn list_sources(State(state): State<AppState>, RawQuery(query): RawQuery, request: Request) -> Response {
    let identity = match authenticate(&state, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    let project_id = match project_query(query.as_deref()) {
        Ok(project_id) => project_id,
        Err(error) => return error_response(error),
    };
    if let Err(error) = authorize(&identity, project_id, Scope::ReadSource) {
        return error_response(error);
    }
    let rows = match state.store.list_sources(project_id).await {
        Ok(rows) => rows,
        Err(error) => return store_response(error),
    };
    let mut sources = Vec::new();
    for row in rows {
        match source_body("getSource", &row) {
            Ok(body) => sources.push(body),
            Err(error) => return error_response(error),
        }
    }
    json_response(200, &json!({
        "contract": CONTRACT,
        "operation": "listSources",
        "project_id": project_id,
        "sources": sources,
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    }))
}

async fn register_source(State(state): State<AppState>, request: Request) -> Response {
    let (headers, value, bytes) = match read_json(request, &state.config.secret_values).await {
        Ok(parsed) => parsed,
        Err(response) => return *response,
    };
    let parsed = match parse_register(&value) {
        Ok(parsed) => parsed,
        Err(error) => return error_response(error),
    };
    let identity = match mutate(&state, &headers, parsed.revision).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    if let Err(error) = authorize(&identity, &parsed.project_id, Scope::Mutate) {
        return error_response(error);
    }
    if let Err(error) = admit_register(&state, &parsed) {
        return error_response(error);
    }
    let source = NewSource {
        source_ref: parsed.source_ref,
        project_id: parsed.project_id,
        kind: parsed.kind,
        secret_ref: parsed.secret_ref,
        host: parsed.host,
        port: parsed.port,
        database_name: parsed.database,
        username: parsed.username,
        tables_json: parsed.tables_json,
        idempotency_key: parsed.idempotency_key,
        body_sha256: action_hash("registerSource", &bytes),
        subject: identity.subject,
    };
    match state.store.register(&source).await {
        Ok((row, _)) => match source_body("registerSource", &row) {
            Ok(body) => json_response(201, &body),
            Err(error) => error_response(error),
        },
        Err(error) => store_response(error),
    }
}

async fn get_source(State(state): State<AppState>, Path(source_ref): Path<String>, request: Request) -> Response {
    let identity = match authenticate(&state, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    if !ident_ok(&source_ref) {
        return error_response(ApiError::malformed("source_ref"));
    }
    let row = match visible_source(&state, &identity, &source_ref).await {
        Seen::Row(row) => row,
        Seen::Hidden => return error_response(ApiError::forbidden("scope")),
        Seen::Unavailable => return error_response(ApiError::not_ready("database")),
    };
    match source_body("getSource", &row) {
        Ok(body) => json_response(200, &body),
        Err(error) => error_response(error),
    }
}

async fn get_catalog(State(state): State<AppState>, Path(source_ref): Path<String>, request: Request) -> Response {
    let identity = match authenticate(&state, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    if !ident_ok(&source_ref) {
        return error_response(ApiError::malformed("source_ref"));
    }
    let row = match visible_source(&state, &identity, &source_ref).await {
        Seen::Row(row) => row,
        Seen::Hidden => return error_response(ApiError::forbidden("scope")),
        Seen::Unavailable => return error_response(ApiError::not_ready("database")),
    };
    let docs = match state.store.exchange(&row.project_id, &row.source_ref).await {
        Ok(Some(docs)) => docs,
        Ok(None) => return error_response(ApiError::missing("catalog")),
        Err(error) => return store_response(error),
    };
    if contains_secret(docs.catalog.as_bytes(), &state.config.secret_values) || contains_secret(docs.profile.as_bytes(), &state.config.secret_values) {
        return error_response(ApiError::not_ready("catalog"));
    }
    let catalog = match parse_exact(docs.catalog.as_bytes()) {
        Ok(catalog) => catalog,
        Err(_) => return error_response(ApiError::not_ready("catalog")),
    };
    let profile = match parse_exact(docs.profile.as_bytes()) {
        Ok(profile) => profile,
        Err(_) => return error_response(ApiError::not_ready("catalog")),
    };
    json_response(200, &json!({
        "contract": "oo.source-control.exchange/v1",
        "operation": "getSourceCatalog",
        "project_id": row.project_id,
        "source_ref": row.source_ref,
        "revision": row.revision,
        "kind": row.kind,
        "state": row.state,
        "catalog_sha256": docs.catalog_sha256,
        "profile_sha256": docs.profile_sha256,
        "catalog": catalog,
        "profile": profile,
        "published": false,
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    }))
}

async fn upload_object(State(state): State<AppState>, Path(source_ref): Path<String>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    if !ident_ok(&source_ref) {
        return error_response(ApiError::malformed("source_ref"));
    }
    if !media(&parts.headers, "application/octet-stream") {
        return error_response(ApiError::malformed("type"));
    }
    let revision = match header_revision(&parts.headers) {
        Ok(revision) => revision,
        Err(error) => return error_response(error),
    };
    let identity = match mutate(&state, &parts.headers, revision).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    let key = match header_key(&parts.headers) {
        Ok(key) => key,
        Err(error) => return error_response(error),
    };
    let declared = match header_sha(&parts.headers) {
        Ok(declared) => declared,
        Err(error) => return error_response(error),
    };
    let bytes = match axum::body::to_bytes(body, state.config.upload_max_bytes).await {
        Ok(bytes) => bytes,
        Err(_) => return error_response(ApiError::too_large()),
    };
    if sha256_hex(&bytes) != declared {
        return error_response(ApiError::malformed("sha256"));
    }
    if contains_secret(&bytes, &state.config.secret_values) {
        return error_response(ApiError::malformed("secret"));
    }
    let source = match state.store.source(&source_ref).await {
        Ok(Some(source)) => source,
        Ok(None) => return error_response(ApiError::forbidden("scope")),
        Err(error) => return store_response(error),
    };
    if let Err(error) = authorize(&identity, &source.project_id, Scope::Mutate) {
        return error_response(if error.detail == "capability" { error } else { ApiError::forbidden("scope") });
    }
    if source.kind != "file_local" {
        return error_response(ApiError::closed("kind"));
    }
    if write_object(&state.config.object_dir, &declared, &bytes).is_err() {
        return error_response(ApiError::not_ready("object"));
    }
    let object = NewObject {
        source_ref,
        project_id: source.project_id.clone(),
        sha256: declared,
        byte_len: i64::try_from(bytes.len()).unwrap_or(i64::MAX),
        stored_name: sha256_hex(&bytes),
        idempotency_key: key,
        body_sha256: upload_hash(&source.source_ref, &bytes),
        subject: identity.subject,
    };
    match state.store.upload(&object).await {
        Ok((row, _)) => json_response(201, &object_body(&source.project_id, &row)),
        Err(error) => store_response(error),
    }
}

async fn list_imports(State(state): State<AppState>, RawQuery(query): RawQuery, request: Request) -> Response {
    let identity = match authenticate(&state, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    let project_id = match project_query(query.as_deref()) {
        Ok(project_id) => project_id,
        Err(error) => return error_response(error),
    };
    if let Err(error) = authorize(&identity, project_id, Scope::ReadImport) {
        return error_response(error);
    }
    let rows = match state.store.list_operations(project_id).await {
        Ok(rows) => rows,
        Err(error) => return store_response(error),
    };
    let imports: Vec<Value> = rows.iter().map(|row| import_body("getImport", row)).collect();
    json_response(200, &json!({
        "contract": CONTRACT,
        "operation": "listImports",
        "project_id": project_id,
        "imports": imports,
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    }))
}

async fn create_import(State(state): State<AppState>, request: Request) -> Response {
    let (headers, value, bytes) = match read_json(request, &state.config.secret_values).await {
        Ok(parsed) => parsed,
        Err(response) => return *response,
    };
    let parsed = match parse_create(&value, u64::try_from(state.config.upload_max_bytes).unwrap_or(0)) {
        Ok(parsed) => parsed,
        Err(error) => return error_response(error),
    };
    let identity = match mutate(&state, &headers, parsed.revision).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    if let Err(error) = authorize(&identity, &parsed.project_id, Scope::Mutate) {
        return error_response(error);
    }
    let input_json = match import_input(&state, &parsed).await {
        Ok(input) => input,
        Err(error) => return error_response(error),
    };
    let Some(operation_id) = new_operation_id() else {
        return error_response(ApiError::not_ready("urandom"));
    };
    let import = NewImport {
        operation_id,
        project_id: parsed.project_id,
        source_ref: parsed.source_ref,
        kind: parsed.kind,
        idempotency_key: parsed.idempotency_key,
        body_sha256: action_hash("createImport", &bytes),
        identity_revision: parsed.revision,
        subject: identity.subject,
        expected_revision: parsed.expected_revision,
        input_json,
    };
    match state.store.create_import(&import).await {
        Ok((row, _)) => json_response(202, &import_body("createImport", &row)),
        Err(error) => store_response(error),
    }
}

async fn get_import(State(state): State<AppState>, Path(operation_id): Path<String>, request: Request) -> Response {
    let identity = match authenticate(&state, request.headers()).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    if !operation_id_ok(&operation_id) {
        return error_response(ApiError::malformed("operation_id"));
    }
    let row = match visible_import(&state, &identity, &operation_id).await {
        Seen::Row(row) => row,
        Seen::Hidden => return error_response(ApiError::forbidden("scope")),
        Seen::Unavailable => return error_response(ApiError::not_ready("database")),
    };
    json_response(200, &import_body("getImport", &row))
}

async fn cancel_import(State(state): State<AppState>, Path(operation_id): Path<String>, request: Request) -> Response {
    action_import(&state, operation_id, request, "cancelImport", true).await
}

async fn restart_import(State(state): State<AppState>, Path(operation_id): Path<String>, request: Request) -> Response {
    action_import(&state, operation_id, request, "restartImport", false).await
}

async fn action_import(state: &AppState, operation_id: String, request: Request, action: &'static str, cancel: bool) -> Response {
    let (headers, value, bytes) = match read_json(request, &state.config.secret_values).await {
        Ok(parsed) => parsed,
        Err(response) => return *response,
    };
    if !operation_id_ok(&operation_id) {
        return error_response(ApiError::malformed("operation_id"));
    }
    let parsed = match parse_action(&value) {
        Ok(parsed) => parsed,
        Err(error) => return error_response(error),
    };
    let identity = match mutate(state, &headers, parsed.0).await {
        Ok(identity) => identity,
        Err(response) => return *response,
    };
    if let Err(error) = authorize(&identity, &parsed.1, Scope::Mutate) {
        return error_response(error);
    }
    let hash = action_hash(action, &bytes);
    if cancel {
        match state.store.cancel(&parsed.1, &operation_id, &parsed.2, &hash, &identity.subject).await {
            Ok((row, _apply, signal)) => {
                if let CancelApply::Signal { pid } = signal {
                    let _ = write_cancel(&state.config.run_dir, &operation_id);
                    if let Some(pid) = pid {
                        signal_tree(pid);
                    }
                }
                json_response(202, &import_body("cancelImport", &row))
            }
            Err(error) => store_response(error),
        }
    } else {
        match state.store.restart(&parsed.1, &operation_id, &parsed.2, &hash, &identity.subject).await {
            Ok((row, _apply)) => json_response(202, &import_body("restartImport", &row)),
            Err(error) => store_response(error),
        }
    }
}

async fn not_found() -> Response {
    error_response(ApiError::not_found())
}

async fn method_not_allowed() -> Response {
    error_response(ApiError::method())
}

fn boxed(error: ApiError) -> Box<Response> {
    Box::new(error_response(error))
}

async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<VerifiedIdentity, Box<Response>> {
    let cookie = session_cookie(headers).map_err(boxed)?;
    identity::fetch_current(&state.identity, &state.config.identity_origin, cookie).await.map_err(|fault| boxed(fault.api()))
}

async fn mutate(state: &AppState, headers: &HeaderMap, revision: i64) -> Result<VerifiedIdentity, Box<Response>> {
    let origin = header_str(headers, header::ORIGIN);
    let csrf = header_str(headers, "x-oo-csrf");
    let Some(token) = csrf.filter(|value| session_cookie_ok(value)) else {
        return Err(boxed(ApiError::csrf()));
    };
    let Some(origin_value) = origin else {
        return Err(boxed(ApiError::origin()));
    };
    if !state.config.deployment_origins.iter().any(|item| ct_eq(item, origin_value)) {
        return Err(boxed(ApiError::origin()));
    }
    let cookie = session_cookie(headers).map_err(boxed)?;
    identity::confirm_csrf(&state.identity, &state.config.identity_origin, cookie, token).await.map_err(boxed)?;
    let identity = authenticate(state, headers).await?;
    mutation_checks(origin, csrf, &state.config.deployment_origins, &identity, revision).map_err(boxed)?;
    Ok(identity)
}

async fn read_json(request: Request, secrets: &[Vec<u8>]) -> Result<(HeaderMap, Value, Vec<u8>), Box<Response>> {
    let (parts, body) = request.into_parts();
    if !media(&parts.headers, "application/json") {
        return Err(boxed(ApiError::malformed("type")));
    }
    let bytes = match axum::body::to_bytes(body, JSON_LIMIT).await {
        Ok(bytes) => bytes.to_vec(),
        Err(_) => return Err(boxed(ApiError::too_large())),
    };
    if contains_secret(&bytes, secrets) {
        return Err(boxed(ApiError::malformed("secret")));
    }
    let value = parse_exact(&bytes).map_err(boxed)?;
    Ok((parts.headers, value, bytes))
}

enum Seen<T> {
    Row(T),
    Hidden,
    Unavailable,
}

async fn visible_source(state: &AppState, identity: &VerifiedIdentity, source_ref: &str) -> Seen<SourceRow> {
    match state.store.source(source_ref).await {
        Ok(Some(source)) if readable(identity, &source.project_id) => Seen::Row(source),
        Ok(_) => Seen::Hidden,
        Err(_) => Seen::Unavailable,
    }
}

async fn visible_import(state: &AppState, identity: &VerifiedIdentity, operation_id: &str) -> Seen<ImportRow> {
    match state.store.operation(operation_id).await {
        Ok(Some(row)) if readable(identity, &row.project_id) => Seen::Row(row),
        Ok(_) => Seen::Hidden,
        Err(_) => Seen::Unavailable,
    }
}

fn readable(identity: &VerifiedIdentity, project_id: &str) -> bool {
    identity.project(project_id).is_some_and(|project| {
        project.capabilities.iter().any(|item| item == "project.admin" || item == "audit.read")
    })
}

struct ParsedSource {
    project_id: String,
    revision: i64,
    source_ref: String,
    kind: String,
    idempotency_key: String,
    secret_ref: Option<String>,
    host: Option<String>,
    port: Option<i32>,
    database: Option<String>,
    username: Option<String>,
    tables_json: Option<String>,
}

fn parse_register(value: &Value) -> Result<ParsedSource, ApiError> {
    reject_closed(value)?;
    jsonutil::allow_keys(value, &["project_id", "identity_revision", "source_ref", "kind", "idempotency_key", "secret_ref", "connection", "tables"])?;
    let map = value.as_object().ok_or(ApiError::malformed("json"))?;
    let project_id = require_ident(map, "project_id")?;
    let source_ref = require_ident(map, "source_ref")?;
    let kind = jsonutil::require_str(map, "kind")?;
    let idempotency_key = require_key(map)?;
    let revision = jsonutil::positive_i64(map.get("identity_revision").ok_or(ApiError::malformed("json"))?)?;
    if kind == "file_local" {
        if map.contains_key("connection") || map.contains_key("secret_ref") || map.contains_key("tables") {
            return Err(ApiError::closed("kind"));
        }
        return Ok(ParsedSource {
            project_id,
            revision,
            source_ref,
            kind: kind.to_string(),
            idempotency_key,
            secret_ref: None,
            host: None,
            port: None,
            database: None,
            username: None,
            tables_json: None,
        });
    }
    if kind != "pg_local" {
        return Err(ApiError::malformed("kind"));
    }
    let secret_ref = jsonutil::require_str(map, "secret_ref").map_err(|_| ApiError::malformed("secret_ref"))?;
    if !secret_ref_ok(secret_ref) {
        return Err(ApiError::malformed("secret_ref"));
    }
    let connection = map.get("connection").ok_or(ApiError::malformed("connection"))?;
    let (host, port, database, username) = parse_connection(connection)?;
    let tables_json = parse_tables(map.get("tables").ok_or(ApiError::malformed("tables"))?)?;
    Ok(ParsedSource {
        project_id,
        revision,
        source_ref,
        kind: kind.to_string(),
        idempotency_key,
        secret_ref: Some(secret_ref.to_string()),
        host: Some(host),
        port: Some(port),
        database: Some(database),
        username: Some(username),
        tables_json: Some(tables_json),
    })
}

fn admit_register(state: &AppState, parsed: &ParsedSource) -> Result<(), ApiError> {
    if parsed.kind != "pg_local" {
        return Ok(());
    }
    let host = parsed.host.as_deref().ok_or(ApiError::malformed("host"))?;
    if !state.config.admitted_hosts.iter().any(|item| item == host) {
        return Err(ApiError::not_admitted());
    }
    let secret_ref = parsed.secret_ref.as_deref().ok_or(ApiError::malformed("secret_ref"))?;
    if state.config.secrets.contains_key(secret_ref) {
        Ok(())
    } else {
        Err(ApiError::secret_unknown())
    }
}

struct ParsedImport {
    project_id: String,
    revision: i64,
    source_ref: String,
    kind: String,
    idempotency_key: String,
    expected_revision: i64,
    object_sha256: Option<String>,
    profile: Option<Value>,
    budget: Option<Value>,
}

fn parse_create(value: &Value, upload_max: u64) -> Result<ParsedImport, ApiError> {
    reject_closed(value)?;
    jsonutil::allow_keys(
        value,
        &[
            "project_id",
            "identity_revision",
            "source_ref",
            "kind",
            "idempotency_key",
            "expected_source_revision",
            "object_sha256",
            "profile",
            "budget",
        ],
    )?;
    let map = value.as_object().ok_or(ApiError::malformed("json"))?;
    let kind = jsonutil::require_str(map, "kind")?;
    let object_sha256 = map.get("object_sha256").cloned();
    let profile = map.get("profile").cloned();
    let budget = map.get("budget").cloned();
    if kind == "pg_inspect" {
        if object_sha256.is_some() || profile.is_some() || budget.is_some() {
            return Err(ApiError::closed("kind"));
        }
    } else if kind == "file_replace" {
        let sha = object_sha256.as_ref().and_then(Value::as_str).ok_or(ApiError::malformed("sha256"))?;
        if !hex64(sha) {
            return Err(ApiError::malformed("sha256"));
        }
        if !profile.as_ref().is_some_and(Value::is_object) {
            return Err(ApiError::malformed("profile"));
        }
        admit_budget(budget.as_ref().ok_or(ApiError::malformed("budget"))?, upload_max)?;
    } else {
        return Err(ApiError::malformed("kind"));
    }
    Ok(ParsedImport {
        project_id: require_ident(map, "project_id")?,
        revision: jsonutil::positive_i64(map.get("identity_revision").ok_or(ApiError::malformed("json"))?)?,
        source_ref: require_ident(map, "source_ref")?,
        kind: kind.to_string(),
        idempotency_key: require_key(map)?,
        expected_revision: jsonutil::positive_i64(map.get("expected_source_revision").ok_or(ApiError::malformed("json"))?)?,
        object_sha256: object_sha256.and_then(|item| item.as_str().map(str::to_string)),
        profile,
        budget,
    })
}

async fn import_input(state: &AppState, parsed: &ParsedImport) -> Result<String, ApiError> {
    if parsed.kind == "pg_inspect" {
        let source = state.store.source(&parsed.source_ref).await.map_err(|_| ApiError::not_ready("database"))?;
        let Some(source) = source else {
            return Err(ApiError::forbidden("scope"));
        };
        if source.project_id != parsed.project_id {
            return Err(ApiError::forbidden("scope"));
        }
        let tables = parse_exact(source.tables_json.as_deref().unwrap_or("[]").as_bytes()).map_err(|_| ApiError::not_ready("source"))?;
        return serde_json::to_string(&json!({"tables": tables})).map_err(|_| ApiError::malformed("json"));
    }
    let sha = parsed.object_sha256.as_deref().ok_or(ApiError::malformed("sha256"))?;
    let found = state.store.object(&parsed.source_ref, sha).await.map_err(|_| ApiError::not_ready("database"))?;
    if found.is_none() {
        return Err(ApiError::conflict("object"));
    }
    let input = json!({
        "object_sha256": sha,
        "profile": parsed.profile,
        "budget": parsed.budget
    });
    let text = serde_json::to_string(&input).map_err(|_| ApiError::malformed("json"))?;
    if contains_secret(text.as_bytes(), &state.config.secret_values) {
        return Err(ApiError::malformed("secret"));
    }
    Ok(text)
}

fn admit_budget(value: &Value, upload_max: u64) -> Result<(), ApiError> {
    let map = jsonutil::object(
        value,
        &[
            "batch_rows",
            "batch_bytes",
            "max_record_bytes",
            "max_field_bytes",
            "max_total_bytes",
            "max_rows",
            "max_columns",
            "max_fields",
            "worker_deadline_ms",
            "parent_deadline_ms",
        ],
    )?;
    let _batch_rows = jsonutil::bounded_u64(field(map, "batch_rows")?, 1, 65_536)?;
    let _batch_bytes = jsonutil::bounded_u64(field(map, "batch_bytes")?, 1, 16_777_216)?;
    let max_record = jsonutil::bounded_u64(field(map, "max_record_bytes")?, 3, 1_048_576)?;
    let max_field = jsonutil::bounded_u64(field(map, "max_field_bytes")?, 1, 1_048_576)?;
    let total_cap = upload_max.min(5 * 1024 * 1024 * 1024);
    let max_total = jsonutil::bounded_u64(field(map, "max_total_bytes")?, 1, total_cap)?;
    let _max_rows = jsonutil::bounded_u64(field(map, "max_rows")?, 1, 100_000)?;
    let _max_columns = jsonutil::bounded_u64(field(map, "max_columns")?, 1, 256)?;
    let _max_fields = jsonutil::bounded_u64(field(map, "max_fields")?, 1, 256)?;
    let _worker = jsonutil::bounded_u64(field(map, "worker_deadline_ms")?, 1, 120_000)?;
    let _parent = jsonutil::bounded_u64(field(map, "parent_deadline_ms")?, 1, 120_000)?;
    if max_field > max_record || max_total < max_record {
        return Err(ApiError::closed("budget"));
    }
    Ok(())
}

fn parse_action(value: &Value) -> Result<(i64, String, String), ApiError> {
    reject_closed(value)?;
    let map = jsonutil::object(value, &["project_id", "identity_revision", "idempotency_key"])?;
    let revision = jsonutil::positive_i64(field(map, "identity_revision")?)?;
    Ok((revision, require_ident(map, "project_id")?, require_key(map)?))
}

fn parse_connection(value: &Value) -> Result<(String, i32, String, String), ApiError> {
    let map = jsonutil::object(value, &["host", "port", "database", "username"])?;
    let host = jsonutil::require_str(map, "host")?;
    if !host_ok(host) {
        return Err(ApiError::malformed("host"));
    }
    let port = field(map, "port")?.as_u64().ok_or(ApiError::malformed("port"))?;
    if !(1..=65535).contains(&port) {
        return Err(ApiError::malformed("port"));
    }
    let database = jsonutil::require_str(map, "database")?;
    let username = jsonutil::require_str(map, "username")?;
    if !ident_ok(database) || !ident_ok(username) {
        return Err(ApiError::malformed("connection"));
    }
    Ok((host.to_string(), i32::try_from(port).map_err(|_| ApiError::malformed("port"))?, database.to_string(), username.to_string()))
}

fn parse_tables(value: &Value) -> Result<String, ApiError> {
    let items = value.as_array().ok_or(ApiError::malformed("tables"))?;
    if items.is_empty() || items.len() > 32 {
        return Err(ApiError::malformed("tables"));
    }
    for item in items {
        let map = jsonutil::object(item, &["schema", "table"])?;
        if !ident_ok(jsonutil::require_str(map, "schema")?) || !ident_ok(jsonutil::require_str(map, "table")?) {
            return Err(ApiError::malformed("tables"));
        }
    }
    serde_json::to_string(value).map_err(|_| ApiError::malformed("tables"))
}

fn reject_closed(value: &Value) -> Result<(), ApiError> {
    let map = value.as_object().ok_or(ApiError::malformed("json"))?;
    for (key, detail) in CLOSED {
        if map.contains_key(*key) {
            return Err(ApiError::closed(detail));
        }
    }
    Ok(())
}

fn require_ident(map: &Map<String, Value>, key: &str) -> Result<String, ApiError> {
    let text = jsonutil::require_str(map, key)?;
    if ident_ok(text) {
        Ok(text.to_string())
    } else {
        Err(ApiError::malformed("json"))
    }
}

fn require_key(map: &Map<String, Value>) -> Result<String, ApiError> {
    let text = jsonutil::require_str(map, "idempotency_key")?;
    if idempotency_ok(text) {
        Ok(text.to_string())
    } else {
        Err(ApiError::malformed("idempotency"))
    }
}

fn field<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, ApiError> {
    map.get(key).ok_or(ApiError::malformed("json"))
}

fn source_body(operation: &str, row: &SourceRow) -> Result<Value, ApiError> {
    let mut body = json!({
        "contract": CONTRACT,
        "operation": operation,
        "project_id": row.project_id,
        "source_ref": row.source_ref,
        "kind": row.kind,
        "revision": row.revision,
        "state": row.state,
        "published": false,
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    });
    if row.kind == "pg_local" {
        let object = body.as_object_mut().ok_or(ApiError::not_ready("source"))?;
        object.insert("host".to_string(), Value::String(row.host.clone().ok_or(ApiError::not_ready("source"))?));
        object.insert("database".to_string(), Value::String(row.database_name.clone().ok_or(ApiError::not_ready("source"))?));
        object.insert("username".to_string(), Value::String(row.username.clone().ok_or(ApiError::not_ready("source"))?));
        object.insert(
            "tables".to_string(),
            parse_exact(row.tables_json.as_deref().unwrap_or("[]").as_bytes()).map_err(|_| ApiError::not_ready("source"))?,
        );
    }
    Ok(body)
}

fn import_body(operation: &str, row: &ImportRow) -> Value {
    json!({
        "contract": CONTRACT,
        "operation": operation,
        "operation_id": row.operation_id,
        "project_id": row.project_id,
        "source_ref": row.source_ref,
        "kind": row.kind,
        "status": row.status,
        "attempt": row.attempt,
        "rows": row.rows_copied,
        "producer_code": row.producer_code,
        "producer_detail": row.producer_detail,
        "child_started": row.child_started,
        "catalog_sha256": row.catalog_sha256,
        "profile_sha256": row.profile_sha256,
        "published": false,
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    })
}

fn object_body(project_id: &str, row: &ObjectRow) -> Value {
    json!({
        "contract": CONTRACT,
        "operation": "uploadSourceObject",
        "project_id": project_id,
        "source_ref": row.source_ref,
        "sha256": row.sha256,
        "byte_len": row.byte_len,
        "published": false,
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    })
}

fn write_object(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Result<(), ()> {
    if !hex64(name) {
        return Err(());
    }
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(dir.join(name)).map_err(|_| ())?;
    file.write_all(bytes).map_err(|_| ())
}

fn write_cancel(dir: &std::path::Path, operation_id: &str) -> Result<(), ()> {
    let parent = dir.join(operation_id);
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&parent).map_err(|_| ())?;
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(parent.join("cancel")).map_err(|_| ())?;
    file.write_all(b"cancel\n").map_err(|_| ())
}

fn session_cookie(headers: &HeaderMap) -> Result<&str, ApiError> {
    let raw = header_str(headers, header::COOKIE).ok_or(ApiError::unauthenticated())?;
    for part in raw.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("__Host-oo-session=") {
            if session_cookie_ok(value) {
                return Ok(value);
            }
            return Err(ApiError::unauthenticated());
        }
    }
    Err(ApiError::unauthenticated())
}

fn header_revision(headers: &HeaderMap) -> Result<i64, ApiError> {
    let text = header_str(headers, "x-oo-identity-revision").ok_or(ApiError::malformed("revision"))?;
    let revision = text.parse::<i64>().map_err(|_| ApiError::malformed("revision"))?;
    if revision >= 1 { Ok(revision) } else { Err(ApiError::malformed("revision")) }
}

fn header_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let text = header_str(headers, "idempotency-key").ok_or(ApiError::malformed("idempotency"))?;
    if idempotency_ok(text) { Ok(text.to_string()) } else { Err(ApiError::malformed("idempotency")) }
}

fn header_sha(headers: &HeaderMap) -> Result<String, ApiError> {
    let text = header_str(headers, "x-oo-sha256").ok_or(ApiError::malformed("sha256"))?;
    if hex64(text) { Ok(text.to_string()) } else { Err(ApiError::malformed("sha256")) }
}

fn header_str<K: header::AsHeaderName>(headers: &HeaderMap, name: K) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn media(headers: &HeaderMap, expected: &str) -> bool {
    header_str(headers, header::CONTENT_TYPE).is_some_and(|value| value.split(';').next().is_some_and(|item| item.trim().eq_ignore_ascii_case(expected)))
}

fn project_query(query: Option<&str>) -> Result<&str, ApiError> {
    let query = query.ok_or(ApiError::malformed("query"))?;
    let mut found = None;
    if query.is_empty() {
        return Err(ApiError::malformed("query"));
    }
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or(ApiError::malformed("query"))?;
        if key != "project_id" || found.is_some() {
            return Err(ApiError::malformed("query"));
        }
        found = Some(value);
    }
    let project_id = found.ok_or(ApiError::malformed("query"))?;
    if ident_ok(project_id) { Ok(project_id) } else { Err(ApiError::malformed("query")) }
}

fn store_response(error: StoreError) -> Response {
    error_response(match error {
        StoreError::Unavailable => ApiError::not_ready("database"),
        StoreError::Conflict(detail) => ApiError::conflict(detail),
        StoreError::Idempotency => ApiError::idempotency(),
        StoreError::Revision => ApiError::revision(),
        StoreError::NotVisible => ApiError::forbidden("scope"),
    })
}

fn error_response(error: ApiError) -> Response {
    json_response(error.status, &error.body())
}

fn json_response(status: u16, value: &Value) -> Response {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| {
        br#"{"contract":"oo.source-control-api/v1","code":"malformed","detail":"json","build_review":"not_claimed","release":"not_claimed","customer_integration":"not_claimed"}"#.to_vec()
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
