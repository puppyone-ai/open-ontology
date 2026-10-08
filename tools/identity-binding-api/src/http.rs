//! Preview forwards. Approve writes the Git ref only after a planned preview.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::config::{change_value, parse_change, Change, Config};
use crate::error::{error_body, Fault, CONTRACT};
use crate::git::{cas_commit, sha256_hex};
use crate::tls::{server_config, TlsListener};

const SESSION_COOKIE: &str = "__Host-oo-session";

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    client: reqwest::Client,
}

pub async fn listen(config: &Config) -> Result<TcpListener, Fault> {
    TcpListener::bind((config.listen_host, config.listen_port)).await.map_err(|_| Fault::unavailable())
}

pub async fn serve(config: Arc<Config>, listener: TcpListener) -> Result<(), Fault> {
    let certs = reqwest::Certificate::from_pem_bundle(&config.identity_ca_pem).map_err(|_| Fault::malformed())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .referer(false)
        .connection_verbose(false)
        .tls_certs_only(certs)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| Fault::unavailable())?;
    let tls = server_config(&config.tls_cert_pem, &config.tls_key_pem)?;
    let app = App { config, client };
    let incoming = TlsListener { tcp: listener, acceptor: TlsAcceptor::from(Arc::new(tls)) };
    axum::serve(incoming, router(app)).await.map_err(|_| Fault::unavailable())
}

fn router(app: App) -> Router {
    Router::new()
        .route("/binding/v1/preview", post(preview).fallback(method_not_allowed))
        .route("/binding/v1/approve", post(approve).fallback(method_not_allowed))
        .fallback(not_found)
        .with_state(app)
}

async fn preview(State(app): State<App>, request: Request) -> Response {
    let headers = request.headers().clone();
    let bytes = match json_bytes(request).await {
        Ok(bytes) => bytes,
        Err(fault) => return fault_response(fault),
    };
    let change = match parse_change(&bytes) {
        Ok(change) => change,
        Err(fault) => return fault_response(fault),
    };
    if let Err(fault) = caller_guards(&app, &headers) {
        return fault_response(fault);
    }
    match app.post_identity("/control/v1/identity/bindings/preview", &headers, &change, None).await {
        Ok((status, body)) => pass_through(status, body),
        Err(fault) => fault_response(fault),
    }
}

async fn approve(State(app): State<App>, request: Request) -> Response {
    let headers = request.headers().clone();
    let bytes = match json_bytes(request).await {
        Ok(bytes) => bytes,
        Err(fault) => return fault_response(fault),
    };
    let change = match parse_change(&bytes) {
        Ok(change) => change,
        Err(fault) => return fault_response(fault),
    };
    if let Err(fault) = caller_guards(&app, &headers) {
        return fault_response(fault);
    }
    app.approve_change(&headers, &change).await
}

impl App {
    async fn approve_change(&self, headers: &HeaderMap, change: &Change) -> Response {
        let preview = match self.post_identity("/control/v1/identity/bindings/preview", headers, change, None).await {
            Ok(preview) => preview,
            Err(fault) => return fault_response(fault),
        };
        if preview.0 != 200 {
            if error_code(&preview.1) == "revision.conflict" {
                return self.finish_approve(headers, change, None).await;
            }
            return pass_through(preview.0, preview.1);
        }
        match preview.1.get("outcome").and_then(Value::as_str) {
            Some("unchanged") => self.finish_approve(headers, change, None).await,
            Some("planned") => {
                let Some(document) = preview.1.get("document").and_then(Value::as_str) else {
                    return fault_response(Fault::unavailable());
                };
                let Some(document_sha256) = preview.1.get("document_sha256").and_then(Value::as_str) else {
                    return fault_response(Fault::unavailable());
                };
                if sha256_hex(document.as_bytes()) != document_sha256 {
                    return fault_response(Fault::git_rejected());
                }
                let commit = match cas_commit(&self.config.git, &change.expected_git_head, document.as_bytes(), &change.idempotency_key, document_sha256) {
                    Ok(commit) => commit,
                    Err(fault) => return fault_response(fault),
                };
                self.finish_approve(headers, change, Some(commit.as_str())).await
            }
            _ => fault_response(Fault::unavailable()),
        }
    }

    async fn finish_approve(&self, headers: &HeaderMap, change: &Change, git_commit: Option<&str>) -> Response {
        match self.post_identity("/control/v1/identity/bindings/approve", headers, change, git_commit).await {
            Ok((status, body)) => pass_through(status, body),
            Err(fault) => fault_response(fault),
        }
    }

    async fn post_identity(&self, path: &str, headers: &HeaderMap, change: &Change, git_commit: Option<&str>) -> Result<(u16, Value), Fault> {
        let cookie = session_cookie(headers)?;
        let csrf = csrf_header(headers)?;
        let url = format!("{}{path}", self.config.identity_origin);
        let response = self
            .client
            .post(url)
            .header("cookie", cookie)
            .header("origin", &self.config.identity_origin)
            .header("x-oo-csrf", csrf)
            .header("content-type", "application/json")
            .body(change_value(change, git_commit).to_string())
            .send()
            .await
            .map_err(|_| Fault::unavailable())?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|_| Fault::unavailable())?;
        if bytes.len() > 65_536 {
            return Err(Fault::unavailable());
        }
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok((status, body))
    }
}

fn caller_guards(app: &App, headers: &HeaderMap) -> Result<(), Fault> {
    let origin = one_header(headers, "origin")?.ok_or_else(Fault::origin)?;
    if origin != app.config.https_origin {
        return Err(Fault::origin());
    }
    let _ = session_cookie(headers)?;
    let _ = csrf_header(headers)?;
    Ok(())
}

fn pass_through(status: u16, mut body: Value) -> Response {
    if let Some(object) = body.as_object_mut() {
        object.insert("contract".to_string(), json!(CONTRACT));
    }
    json_response(status, body)
}

fn error_code(body: &Value) -> &str {
    body.get("error").and_then(|error| error.get("code")).and_then(Value::as_str).unwrap_or("")
}

fn session_cookie(headers: &HeaderMap) -> Result<String, Fault> {
    let header = one_header(headers, "cookie")?.ok_or_else(Fault::unauthenticated)?;
    let mut found = None;
    for part in header.split(';') {
        let part = part.trim();
        let Some((key, value)) = part.split_once('=') else {
            return Err(Fault::malformed());
        };
        if key.trim() != SESSION_COOKIE {
            continue;
        }
        if found.is_some() || !token_shape(value) {
            return Err(Fault::unauthenticated());
        }
        found = Some(value.to_string());
    }
    found.map(|value| format!("{SESSION_COOKIE}={value}")).ok_or_else(Fault::unauthenticated)
}

fn csrf_header(headers: &HeaderMap) -> Result<String, Fault> {
    match one_header(headers, "x-oo-csrf")? {
        Some(value) if token_shape(value) => Ok(value.to_string()),
        _ => Err(Fault::csrf()),
    }
}

fn token_shape(value: &str) -> bool {
    value.len() == 43 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
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

async fn json_bytes(request: Request) -> Result<Vec<u8>, Fault> {
    let content_type = one_header(request.headers(), "content-type")?;
    let Some(content_type) = content_type else {
        return Err(Fault::malformed());
    };
    if content_type.split(';').next().unwrap_or("").trim() != "application/json" {
        return Err(Fault::malformed());
    }
    let bytes = axum::body::to_bytes(request.into_body(), 4096).await.map_err(|_| Fault::malformed())?;
    Ok(bytes.to_vec())
}

async fn method_not_allowed() -> Response {
    fault_response(Fault::method())
}

async fn not_found() -> Response {
    fault_response(Fault::not_found())
}

fn fault_response(fault: Fault) -> Response {
    json_response(fault.http, error_body(&fault))
}

fn json_response(status: u16, body: Value) -> Response {
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header("x-content-type-options", "nosniff")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
