//! B01 routes. The original five operations stay. Binding preview and approve are additive.
//! No review, publish, data-grant, or secret-read route.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::binding::{hex40, Change};
use crate::config::{Config, Role};
use crate::cookie::{
    clear_cookie, cookie_value, percent_decode, return_path, session_hash, set_cookie, LoginCookie, LOGIN_COOKIE, SESSION_COOKIE,
};
use crate::crypto::{opaque_token, pkce_challenge, random_32, token_shape};
use crate::error::{error_body, Fault, CONTRACT};
use crate::oidc::{authorization_url, Oidc};
use crate::store::{ApproveReport, CurrentRecord, PreviewReport, Store};
use crate::tls::{server_config, TlsListener};

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    store: Arc<Store>,
    oidc: Arc<Oidc>,
}

struct LoginQuery {
    return_to: String,
    bootstrap_token: Option<String>,
}

struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsumeBody {
    token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeBody {
    effect: String,
    project_id: String,
    subject: String,
    #[serde(default)]
    role: Option<String>,
    expected_revision: i64,
    expected_config_commit: String,
    expected_git_head: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApproveBody {
    effect: String,
    project_id: String,
    subject: String,
    #[serde(default)]
    role: Option<String>,
    expected_revision: i64,
    expected_config_commit: String,
    expected_git_head: String,
    idempotency_key: String,
    #[serde(default)]
    git_commit: Option<String>,
}

pub async fn listen(config: &Config) -> Result<TcpListener, Fault> {
    let listener = TcpListener::bind((config.listen_host, config.listen_port)).await.map_err(|_| Fault::unavailable())?;
    check_bound(config, &listener)?;
    Ok(listener)
}

pub async fn serve(config: Arc<Config>, listener: TcpListener) -> Result<(), Fault> {
    check_bound(&config, &listener)?;
    let store = Arc::new(Store::connect(&config).await?);
    store.require_installed().await?;
    let oidc = Arc::new(Oidc::new(&config)?);
    let tls = server_config(&config.tls_cert_pem, &config.tls_key_pem)?;
    let app = App { config, store, oidc };
    let incoming = TlsListener { tcp: listener, acceptor: TlsAcceptor::from(Arc::new(tls)) };
    axum::serve(incoming, router(app)).await.map_err(|_| Fault::unavailable())
}

fn check_bound(config: &Config, listener: &TcpListener) -> Result<(), Fault> {
    let bound = listener.local_addr().map_err(|_| Fault::unavailable())?;
    if bound.ip() != config.listen_host || (config.listen_port != 0 && bound.port() != config.listen_port) {
        Err(Fault::malformed())
    } else {
        Ok(())
    }
}

fn router(app: App) -> Router {
    Router::new()
        .route("/control/v1/auth/login", get(login).fallback(method_not_allowed))
        .route("/control/v1/auth/callback", get(callback).fallback(method_not_allowed))
        .route("/control/v1/identity/current", get(current).fallback(method_not_allowed))
        .route("/control/v1/auth/logout", post(logout).fallback(method_not_allowed))
        .route("/control/v1/auth/bootstrap/consume", post(consume).fallback(method_not_allowed))
        .route("/control/v1/identity/bindings/preview", post(preview_binding).fallback(method_not_allowed))
        .route("/control/v1/identity/bindings/approve", post(approve_binding).fallback(method_not_allowed))
        .fallback(not_found)
        .with_state(app)
}

async fn login(State(app): State<App>, request: Request) -> Response {
    let query = request.uri().query().map(str::to_string);
    if let Err(fault) = discard_body(request).await {
        return fault_response(fault, &[]);
    }
    app.start_login(query.as_deref()).await
}

async fn callback(State(app): State<App>, request: Request) -> Response {
    let headers = request.headers().clone();
    let query = request.uri().query().map(str::to_string);
    if let Err(fault) = discard_body(request).await {
        return fault_response(fault, &[clear_cookie(LOGIN_COOKIE)]);
    }
    app.finish_callback(&headers, query.as_deref()).await
}

async fn current(State(app): State<App>, request: Request) -> Response {
    let headers = request.headers().clone();
    if let Err(fault) = discard_body(request).await {
        return fault_response(fault, &[]);
    }
    app.read_current(&headers).await
}

async fn logout(State(app): State<App>, request: Request) -> Response {
    let headers = request.headers().clone();
    if let Err(fault) = empty_body(request).await {
        return fault_response(fault, &[]);
    }
    app.revoke(&headers).await
}

async fn consume(State(app): State<App>, request: Request) -> Response {
    let headers = request.headers().clone();
    let body = match json_body(request).await {
        Ok(body) => body,
        Err(fault) => return fault_response(fault, &[]),
    };
    app.consume(&headers, body).await
}

async fn preview_binding(State(app): State<App>, request: Request) -> Response {
    let headers = request.headers().clone();
    let body = match json_body_limit(request, 4096).await {
        Ok(body) => body,
        Err(fault) => return fault_response(fault, &[]),
    };
    app.preview(&headers, body).await
}

async fn approve_binding(State(app): State<App>, request: Request) -> Response {
    let headers = request.headers().clone();
    let body = match json_body_limit(request, 4096).await {
        Ok(body) => body,
        Err(fault) => return fault_response(fault, &[]),
    };
    app.approve(&headers, body).await
}

async fn method_not_allowed() -> Response {
    fault_response(Fault::method(), &[])
}

async fn not_found() -> Response {
    fault_response(Fault::not_found(), &[])
}

impl App {
    async fn start_login(&self, query: Option<&str>) -> Response {
        let parsed = match parse_login_query(query) {
            Ok(parsed) => parsed,
            Err(fault) => return fault_response(fault, &[]),
        };
        let endpoint = match self.oidc.authorization_endpoint().await {
            Ok(endpoint) => endpoint,
            Err(fault) => return fault_response(fault, &[]),
        };
        let handle = match random_32() {
            Ok(handle) => handle,
            Err(fault) => return fault_response(fault, &[]),
        };
        let verifier = match random_32() {
            Ok(verifier) => verifier,
            Err(fault) => return fault_response(fault, &[]),
        };
        let state = match opaque_token() {
            Ok(state) => state,
            Err(fault) => return fault_response(fault, &[]),
        };
        let nonce = match opaque_token() {
            Ok(nonce) => nonce,
            Err(fault) => return fault_response(fault, &[]),
        };
        let cookie = LoginCookie { handle, verifier };
        if let Err(fault) = self
            .store
            .begin_login(&self.config, &parsed.return_to, parsed.bootstrap_token.as_deref(), &cookie, &state, &nonce,
            )
            .await
        {
            return fault_response(fault, &[]);
        }
        let location = authorization_url(
            &endpoint,
            &self.config.client_id,
            &self.config.callback_uri,
            &state,
            &nonce,
            &pkce_challenge(&cookie.verifier_string()),
        );
        redirect(&location, &[set_cookie(LOGIN_COOKIE, &cookie.encode())])
    }

    async fn finish_callback(&self, headers: &HeaderMap, query: Option<&str>) -> Response {
        let clear = clear_cookie(LOGIN_COOKIE);
        let login = match login_cookie(headers) {
            Ok(login) => login,
            Err(fault) => return fault_response(fault, &[clear]),
        };
        let parsed = match parse_callback(query) {
            Ok(parsed) => parsed,
            Err(fault) => return fault_response(fault, &[clear]),
        };
        let burned = match self.store.burn_login(&login.handle_hash(), parsed.state.as_deref(), &login.verifier_string(), &self.config.callback_uri).await {
            Ok(burned) => burned,
            Err(fault) => return fault_response(fault, &[clear]),
        };
        if parsed.error.is_some() {
            return self.reject(Fault::token(), &clear).await;
        }
        let Some(code) = parsed.code.as_deref() else {
            return self.reject(Fault::token(), &clear).await;
        };
        let claims = match self.oidc.verify_code(code, &burned.redirect_uri, &login.verifier_string(), &burned.nonce_hash).await {
            Ok(claims) => claims,
            Err(fault) => return self.reject(fault, &clear).await,
        };
        let return_to = match return_path(Some(burned.return_to.as_str())) {
            Ok(return_to) => return_to,
            Err(fault) => return fault_response(fault, &[clear]),
        };
        match self.store.finish_login(&self.config, &claims.subject, burned.bootstrap_hash.as_deref()).await {
            Ok(session) => {
                let location = format!("{}{return_to}", self.config.https_origin);
                redirect(&location, &[clear, set_cookie(SESSION_COOKIE, &session)])
            }
            Err(fault) => fault_response(fault, &[clear]),
        }
    }

    async fn reject(&self, fault: Fault, clear: &str) -> Response {
        if let Err(audit) = self.store.reject_login(&fault).await {
            return fault_response(audit, &[clear.to_string()]);
        }
        fault_response(fault, &[clear.to_string()])
    }

    async fn read_current(&self, headers: &HeaderMap) -> Response {
        let cookie_hash = match session_cookie_hash(headers) {
            Ok(hash) => hash,
            Err(fault) => return fault_response(fault, &[]),
        };
        let record = match self.store.current(&self.config, &cookie_hash).await {
            Ok(record) => record,
            Err(fault) => return fault_response(fault, &[]),
        };
        json_response(200, current_body(&record), &[])
    }

    async fn preview(&self, headers: &HeaderMap, body: Value) -> Response {
        let change = match change_from(body) {
            Ok(change) => change,
            Err(fault) => return fault_response(fault, &[]),
        };
        let cookie_hash = match session_cookie_hash(headers) {
            Ok(hash) => hash,
            Err(fault) => return fault_response(fault, &[]),
        };
        let origin_ok = match one_header(headers, "origin") {
            Ok(value) => value == Some(self.config.https_origin.as_str()),
            Err(fault) => return fault_response(fault, &[]),
        };
        let csrf = match csrf_header(headers) {
            Ok(csrf) => csrf,
            Err(fault) => return fault_response(fault, &[]),
        };
        match self.store.preview_binding(&self.config, &cookie_hash, origin_ok, &csrf, &change).await {
            Ok(report) => json_response(200, preview_body(&report), &[]),
            Err(fault) => fault_response(fault, &[]),
        }
    }

    async fn approve(&self, headers: &HeaderMap, body: Value) -> Response {
        let parsed: ApproveBody = match serde_json::from_value(body) {
            Ok(parsed) => parsed,
            Err(_) => return fault_response(Fault::malformed(), &[]),
        };
        if parsed.git_commit.as_ref().is_some_and(|value| !hex40(value)) {
            return fault_response(Fault::malformed(), &[]);
        }
        let git_commit = parsed.git_commit;
        let change = match change_from_parts(ChangeBody {
            effect: parsed.effect,
            project_id: parsed.project_id,
            subject: parsed.subject,
            role: parsed.role,
            expected_revision: parsed.expected_revision,
            expected_config_commit: parsed.expected_config_commit,
            expected_git_head: parsed.expected_git_head,
            idempotency_key: parsed.idempotency_key,
        }) {
            Ok(change) => change,
            Err(fault) => return fault_response(fault, &[]),
        };
        let cookie_hash = match session_cookie_hash(headers) {
            Ok(hash) => hash,
            Err(fault) => return fault_response(fault, &[]),
        };
        let origin_ok = match one_header(headers, "origin") {
            Ok(value) => value == Some(self.config.https_origin.as_str()),
            Err(fault) => return fault_response(fault, &[]),
        };
        let csrf = match csrf_header(headers) {
            Ok(csrf) => csrf,
            Err(fault) => return fault_response(fault, &[]),
        };
        match self.store.approve_binding(&self.config, &cookie_hash, origin_ok, &csrf, &change, git_commit.as_deref()).await {
            Ok(report) => json_response(200, approve_body(&report), &[]),
            Err(fault) => fault_response(fault, &[]),
        }
    }

    async fn revoke(&self, headers: &HeaderMap) -> Response {
        let cookie_hash = match session_cookie_hash(headers) {
            Ok(hash) => hash,
            Err(fault) => return fault_response(fault, &[]),
        };
        let origin_ok = match one_header(headers, "origin") {
            Ok(value) => value == Some(self.config.https_origin.as_str()),
            Err(fault) => return fault_response(fault, &[]),
        };
        let csrf = match csrf_header(headers) {
            Ok(csrf) => csrf,
            Err(fault) => return fault_response(fault, &[]),
        };
        match self.store.logout(&cookie_hash, origin_ok, &csrf).await {
            Ok(()) => json_response(200, logout_body(), &[clear_cookie(SESSION_COOKIE)]),
            Err(fault) => fault_response(fault, &[]),
        }
    }

    async fn consume(&self, headers: &HeaderMap, body: Value) -> Response {
        let parsed: ConsumeBody = match serde_json::from_value(body) {
            Ok(parsed) => parsed,
            Err(_) => return fault_response(Fault::malformed(), &[]),
        };
        if !token_shape(&parsed.token) {
            return fault_response(Fault::malformed(), &[]);
        }
        let cookie_hash = match session_cookie_hash(headers) {
            Ok(hash) => hash,
            Err(fault) => return fault_response(fault, &[]),
        };
        let origin_ok = match one_header(headers, "origin") {
            Ok(value) => value == Some(self.config.https_origin.as_str()),
            Err(fault) => return fault_response(fault, &[]),
        };
        let csrf = match csrf_header(headers) {
            Ok(csrf) => csrf,
            Err(fault) => return fault_response(fault, &[]),
        };
        match self.store.consume_bootstrap(&self.config, &cookie_hash, origin_ok, &csrf, &parsed.token).await {
            Ok(()) => json_response(200, bootstrap_body(), &[]),
            Err(fault) => fault_response(fault, &[]),
        }
    }
}

fn current_body(record: &CurrentRecord) -> Value {
    let projects = &record.projects;
    json!({
        "contract": CONTRACT,
        "operation": "getIdentityCurrent",
        "subject": record.subject,
        "issuer": record.issuer,
        "config_commit": record.config_commit,
        "revision": record.revision,
        "install_state": if record.initialized { "initialized" } else { "uninitialized" },
        "projects": projects.iter().map(|item| json!({
            "project_id": item.project_id,
            "role": item.role.as_str(),
            "capabilities": item.capabilities
        })).collect::<Vec<_>>(),
        "csrf_token": record.csrf,
        "session": {
            "idle_expires_at": record.idle_expires_at,
            "absolute_expires_at": record.absolute_expires_at,
            "revoked": false
        },
        "authority": "management_session",
        "session_class": "management_cookie",
        "data_grants": "none",
        "owner_certification": "not_issued",
        "agent_session": "not_issued",
        "harness_session": "not_issued",
        "gateway_token": "not_issued",
        "customer_integration": "not_claimed"
    })
}

fn preview_body(report: &PreviewReport) -> Value {
    denial(json!({
        "contract": CONTRACT,
        "operation": "previewManagementBinding",
        "outcome": report.outcome,
        "effect": report.effect,
        "project_id": report.project_id,
        "subject": report.subject,
        "role": role_json(report.role),
        "revision": report.revision,
        "config_commit": report.config_commit,
        "document": report.document,
        "document_sha256": report.document_sha256
    }))
}

fn approve_body(report: &ApproveReport) -> Value {
    denial(json!({
        "contract": CONTRACT,
        "operation": "approveManagementBinding",
        "disposition": report.disposition,
        "outcome": report.outcome,
        "effect": report.effect,
        "project_id": report.project_id,
        "subject": report.subject,
        "role": role_json(report.role),
        "revision": report.revision,
        "config_commit": report.config_commit,
        "git_commit": report.git_commit,
        "document_sha256": report.document_sha256,
        "receipt_sha256": report.receipt_sha256
    }))
}

fn role_json(role: Option<Role>) -> Value {
    role.map(|role| Value::String(role.as_str().to_string())).unwrap_or(Value::Null)
}

fn denial(mut body: Value) -> Value {
    if let Some(object) = body.as_object_mut() {
        object.insert("authority".to_string(), json!("management_session"));
        object.insert("data_grants".to_string(), json!("none"));
        object.insert("owner_certification".to_string(), json!("not_issued"));
        object.insert("agent_session".to_string(), json!("not_issued"));
        object.insert("harness_session".to_string(), json!("not_issued"));
        object.insert("gateway_token".to_string(), json!("not_issued"));
        object.insert("customer_integration".to_string(), json!("not_claimed"));
    }
    body
}

fn change_from(body: Value) -> Result<Change, Fault> {
    let parsed: ChangeBody = serde_json::from_value(body).map_err(|_| Fault::malformed())?;
    change_from_parts(parsed)
}

fn change_from_parts(parsed: ChangeBody) -> Result<Change, Fault> {
    let role = match parsed.role.as_deref() {
        None => None,
        Some(value) => Some(Role::parse(value)?),
    };
    Ok(Change {
        effect: parsed.effect,
        project_id: parsed.project_id,
        subject: parsed.subject,
        role,
        expected_revision: parsed.expected_revision,
        expected_config_commit: parsed.expected_config_commit,
        expected_git_head: parsed.expected_git_head,
        idempotency_key: parsed.idempotency_key,
    })
}

fn logout_body() -> Value {
    json!({
        "contract": CONTRACT,
        "operation": "logoutManagementSession",
        "outcome": "revoked",
        "authority": "management_session",
        "data_grants": "none",
        "owner_certification": "not_issued",
        "agent_session": "not_issued",
        "harness_session": "not_issued",
        "gateway_token": "not_issued",
        "customer_integration": "not_claimed"
    })
}

fn bootstrap_body() -> Value {
    json!({
        "contract": CONTRACT,
        "operation": "consumeBootstrapToken",
        "outcome": "consumed",
        "install_state": "initialized",
        "fixture_class": "explicit_test_fixture",
        "customer_owner": false,
        "owner_certification": "not_issued",
        "data_grants": "none",
        "agent_session": "not_issued",
        "harness_session": "not_issued",
        "gateway_token": "not_issued",
        "customer_integration": "not_claimed"
    })
}

fn login_cookie(headers: &HeaderMap) -> Result<LoginCookie, Fault> {
    let header = one_header(headers, "cookie")?.ok_or_else(Fault::unauthenticated)?;
    let value = cookie_value(header, LOGIN_COOKIE)?.ok_or_else(Fault::unauthenticated)?;
    LoginCookie::decode(value)
}

fn session_cookie_hash(headers: &HeaderMap) -> Result<String, Fault> {
    let header = one_header(headers, "cookie")?.ok_or_else(Fault::unauthenticated)?;
    let value = cookie_value(header, SESSION_COOKIE)?.ok_or_else(Fault::unauthenticated)?;
    session_hash(value)
}

fn csrf_header(headers: &HeaderMap) -> Result<String, Fault> {
    match one_header(headers, "x-oo-csrf")? {
        Some(value) if token_shape(value) => Ok(value.to_string()),
        _ => Err(Fault::csrf()),
    }
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

fn parse_login_query(query: Option<&str>) -> Result<LoginQuery, Fault> {
    let mut return_to = None;
    let mut bootstrap = None;
    for (key, value) in parse_query(query)? {
        match key.as_str() {
            "return_to" if return_to.is_none() => return_to = Some(value),
            "bootstrap_token" if bootstrap.is_none() => bootstrap = Some(value),
            _ => return Err(Fault::malformed()),
        }
    }
    if bootstrap.as_ref().is_some_and(|token| !token_shape(token)) {
        return Err(Fault::malformed());
    }
    Ok(LoginQuery { return_to: return_path(return_to.as_deref())?, bootstrap_token: bootstrap })
}

fn parse_callback(query: Option<&str>) -> Result<CallbackQuery, Fault> {
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = false;
    for (key, value) in parse_query(query)? {
        if value.len() > 512 {
            return Err(Fault::malformed());
        }
        match key.as_str() {
            "code" if code.is_none() => code = Some(value),
            "state" if state.is_none() => state = Some(value),
            "error" if error.is_none() => error = Some(value),
            "error_description" if !description => description = true,
            _ => return Err(Fault::malformed()),
        }
    }
    Ok(CallbackQuery { code, state, error })
}

fn parse_query(query: Option<&str>) -> Result<Vec<(String, String)>, Fault> {
    let Some(query) = query.filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };
    if query.len() > 4096 {
        return Err(Fault::malformed());
    }
    let mut pairs = Vec::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(Fault::malformed)?;
        if key.is_empty() {
            return Err(Fault::malformed());
        }
        pairs.push((percent_decode(key)?, percent_decode(value)?));
    }
    Ok(pairs)
}

async fn discard_body(request: Request) -> Result<(), Fault> {
    axum::body::to_bytes(request.into_body(), 1024).await.map(|_| ()).map_err(|_| Fault::malformed())
}

async fn empty_body(request: Request) -> Result<(), Fault> {
    let bytes = axum::body::to_bytes(request.into_body(), 1).await.map_err(|_| Fault::malformed())?;
    if bytes.is_empty() { Ok(()) } else { Err(Fault::malformed()) }
}

async fn json_body(request: Request) -> Result<Value, Fault> {
    json_body_limit(request, 1024).await
}

async fn json_body_limit(request: Request, limit: usize) -> Result<Value, Fault> {
    let content_type = one_header(request.headers(), "content-type")?;
    let Some(content_type) = content_type else {
        return Err(Fault::malformed());
    };
    if content_type.split(';').next().unwrap_or("").trim() != "application/json" {
        return Err(Fault::malformed());
    }
    let bytes = axum::body::to_bytes(request.into_body(), limit).await.map_err(|_| Fault::malformed())?;
    serde_json::from_slice(&bytes).map_err(|_| Fault::malformed())
}

fn fault_response(fault: Fault, cookies: &[String]) -> Response {
    json_response(fault.http, error_body(&fault), cookies)
}

fn json_response(status: u16, body: Value, cookies: &[String]) -> Response {
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    build(status, Body::from(bytes), cookies, true)
}

fn redirect(location: &str, cookies: &[String]) -> Response {
    let mut response = build(302, Body::empty(), cookies, false);
    if let Ok(value) = header::HeaderValue::from_str(location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    response
}

fn build(status: u16, body: Body, cookies: &[String], json: bool) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = Response::builder()
        .status(code)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header("x-content-type-options", "nosniff");
    if json {
        response = response.header(header::CONTENT_TYPE, "application/json");
    }
    for cookie in cookies {
        response = response.header(header::SET_COOKIE, cookie.as_str());
    }
    response.body(body).unwrap_or_else(|_| Response::new(Body::empty()))
}

#[cfg(test)]
mod tests {
    use super::parse_login_query;
    use crate::error::Fault;

    #[test]
    fn login_query_rejects_a_foreign_redirect() {
        assert_eq!(parse_login_query(Some("redirect_uri=https://evil.example")).err(), Some(Fault::malformed()));
        assert_eq!(parse_login_query(Some("return_to=//evil")).err(), Some(Fault::redirect()));
        assert_eq!(parse_login_query(Some("return_to=%2Fapp")).expect("path").return_to, "/app");
    }
}
