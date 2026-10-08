//! On-wire adapter for getIdentityCurrent.
//!
//! `try_from_caller_json` never succeeds. A verified value is built only from
//! a successful typed envelope fetched from the pinned origin.

use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::pki_types::pem::PemObject;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

use crate::error::ApiError;
use crate::ids::{ct_eq, hex64, ident_ok, session_cookie_ok};
use crate::jsonutil::parse_exact;
use crate::tlsutil::client_config;

const PATH: &str = "/control/v1/identity/current";
const CURRENT_KEYS: &[&str] = &[
    "agent_session",
    "authority",
    "config_commit",
    "contract",
    "csrf_token",
    "customer_integration",
    "data_grants",
    "gateway_token",
    "harness_session",
    "install_state",
    "issuer",
    "operation",
    "owner_certification",
    "projects",
    "revision",
    "session",
    "session_class",
    "subject",
];
const ROLES: &[&str] = &["admin", "auditor", "business_owner", "reviewer", "steward"];
const CAPABILITIES: &[&str] = &[
    "audit.read",
    "binding.read",
    "business.record",
    "identity.current",
    "ontology.draft",
    "project.admin",
    "review.decide",
    "session.revoke",
];

#[derive(Clone, Debug)]
pub struct VerifiedProject {
    pub project_id: String,
    pub role: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct VerifiedIdentity {
    pub subject: String,
    pub issuer: String,
    pub config_commit: String,
    pub revision: i64,
    pub csrf_token: String,
    pub projects: Vec<VerifiedProject>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityFault {
    Unauthenticated,
    Redirect,
    Untyped,
    Unavailable,
}

impl IdentityFault {
    pub fn api(self) -> ApiError {
        match self {
            Self::Unauthenticated => ApiError::unauthenticated(),
            Self::Redirect | Self::Untyped | Self::Unavailable => ApiError::not_ready(),
        }
    }
}

impl VerifiedIdentity {
    pub fn project(&self, project_id: &str) -> Option<&VerifiedProject> {
        self.projects.iter().find(|project| project.project_id == project_id)
    }
}

/// Caller JSON is never authority, including a well-formed envelope.
pub fn try_from_caller_json(_bytes: &[u8]) -> Result<VerifiedIdentity, IdentityFault> {
    Err(IdentityFault::Untyped)
}

pub fn parse_current_envelope(bytes: &[u8]) -> Result<VerifiedIdentity, IdentityFault> {
    let value = parse_exact(bytes).map_err(|_| IdentityFault::Untyped)?;
    let object = value.as_object().ok_or(IdentityFault::Untyped)?;
    if object.len() != CURRENT_KEYS.len() || CURRENT_KEYS.iter().any(|key| !object.contains_key(*key)) {
        return Err(IdentityFault::Untyped);
    }
    require_const(object, "contract", "oo.control-identity/v1")?;
    require_const(object, "operation", "getIdentityCurrent")?;
    require_const(object, "authority", "management_session")?;
    require_const(object, "session_class", "management_cookie")?;
    require_const(object, "data_grants", "none")?;
    require_const(object, "owner_certification", "not_issued")?;
    require_const(object, "agent_session", "not_issued")?;
    require_const(object, "harness_session", "not_issued")?;
    require_const(object, "gateway_token", "not_issued")?;
    require_const(object, "customer_integration", "not_claimed")?;
    if object.get("install_state").and_then(Value::as_str) != Some("initialized") {
        return Err(IdentityFault::Untyped);
    }
    let subject = bounded_string(object, "subject", 1, 256)?;
    let issuer = bounded_string(object, "issuer", 8, 512)?;
    let config_commit = bounded_string(object, "config_commit", 64, 64)?;
    if !hex64(&config_commit) {
        return Err(IdentityFault::Untyped);
    }
    let revision = object.get("revision").and_then(Value::as_i64).ok_or(IdentityFault::Untyped)?;
    if revision < 1 || object.get("revision").and_then(Value::as_u64).is_none() {
        return Err(IdentityFault::Untyped);
    }
    let csrf_token = bounded_string(object, "csrf_token", 43, 43)?;
    if !session_cookie_ok(&csrf_token) {
        return Err(IdentityFault::Untyped);
    }
    let session = object.get("session").and_then(Value::as_object).ok_or(IdentityFault::Untyped)?;
    if session.len() != 3
        || !session.contains_key("idle_expires_at")
        || !session.contains_key("absolute_expires_at")
        || session.get("revoked") != Some(&Value::Bool(false))
    {
        return Err(IdentityFault::Untyped);
    }
    if session.get("idle_expires_at").and_then(Value::as_str).is_none()
        || session.get("absolute_expires_at").and_then(Value::as_str).is_none()
    {
        return Err(IdentityFault::Untyped);
    }
    let projects_value = object.get("projects").and_then(Value::as_array).ok_or(IdentityFault::Untyped)?;
    let mut projects = Vec::new();
    for project in projects_value {
        projects.push(parse_project(project)?);
    }
    Ok(VerifiedIdentity {
        subject,
        issuer,
        config_commit,
        revision,
        csrf_token,
        projects,
    })
}

pub fn classify_status(status: u16) -> Result<(), IdentityFault> {
    match status {
        200 => Ok(()),
        401 => Err(IdentityFault::Unauthenticated),
        300..=399 => Err(IdentityFault::Redirect),
        _ => Err(IdentityFault::Unavailable),
    }
}

/// Unknown bootstrap material. `consumeBootstrapToken` checks the stored CSRF
/// hash before it classifies the token. `getIdentityCurrent` rotates that hash,
/// so the presented token cannot be compared with the post-rotation value.
const UNKNOWN_BOOTSTRAP: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const _: () = assert!(UNKNOWN_BOOTSTRAP.len() == 43);
const CONSUME_PATH: &str = "/control/v1/auth/bootstrap/consume";

pub async fn fetch_current(origin: &str, ca_pem: &[u8], cookie: &str) -> Result<VerifiedIdentity, IdentityFault> {
    if !session_cookie_ok(cookie) || header_injection(cookie) {
        return Err(IdentityFault::Unauthenticated);
    }
    let host_header = split_origin(origin).map_err(|_| IdentityFault::Unavailable)?.0;
    let request = format!(
        "GET {PATH} HTTP/1.1\r\nHost: {host_header}\r\nCookie: __Host-oo-session={cookie}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    let (status, body) = https_exchange(origin, ca_pem, request).await?;
    classify_status(status)?;
    parse_current_envelope(&body)
}

pub fn presented_mutation<'a>(
    origin_header: Option<&'a str>,
    csrf_header: Option<&'a str>,
    allow: &[String],
) -> Result<(&'a str, &'a str), ApiError> {
    let Some(origin) = origin_header else {
        return Err(ApiError::origin());
    };
    if header_injection(origin) || !allow.iter().any(|item| ct_eq(item, origin)) {
        return Err(ApiError::origin());
    }
    let Some(csrf) = csrf_header else {
        return Err(ApiError::csrf());
    };
    if header_injection(csrf) || !session_cookie_ok(csrf) {
        return Err(ApiError::csrf());
    }
    Ok((origin, csrf))
}

pub async fn authorize_mutation(
    service_origin: &str,
    ca_pem: &[u8],
    cookie: &str,
    origin_header: Option<&str>,
    csrf_header: Option<&str>,
    allow: &[String],
    revision: i64,
) -> Result<VerifiedIdentity, ApiError> {
    let (origin, csrf) = presented_mutation(origin_header, csrf_header, allow)?;
    confirm_presented_csrf(service_origin, ca_pem, cookie, origin, csrf).await?;
    let identity = fetch_current(service_origin, ca_pem, cookie)
        .await
        .map_err(IdentityFault::api)?;
    if revision != identity.revision {
        return Err(ApiError::revision());
    }
    Ok(identity)
}

async fn confirm_presented_csrf(
    service_origin: &str,
    ca_pem: &[u8],
    cookie: &str,
    caller_origin: &str,
    csrf: &str,
) -> Result<(), ApiError> {
    if !session_cookie_ok(cookie) || header_injection(cookie) || header_injection(service_origin) {
        return Err(ApiError::unauthenticated());
    }
    let host_header = split_origin(service_origin).map_err(|_| ApiError::not_ready())?.0;
    let body = format!("{{\"token\":\"{UNKNOWN_BOOTSTRAP}\"}}");
    let request = format!(
        "POST {CONSUME_PATH} HTTP/1.1\r\nHost: {host_header}\r\nCookie: __Host-oo-session={cookie}\r\nOrigin: {caller_origin}\r\nX-OO-CSRF: {csrf}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let (status, bytes) = match https_exchange(service_origin, ca_pem, request).await {
        Ok(pair) => pair,
        Err(IdentityFault::Unauthenticated) => return Err(ApiError::unauthenticated()),
        Err(_) => return Err(ApiError::not_ready()),
    };
    let code = error_code(&bytes);
    match status {
        300..=399 => Err(ApiError::not_ready()),
        401 if code.as_deref() == Some("unauthenticated") => Err(ApiError::unauthenticated()),
        403 if code.as_deref() == Some("csrf.rejected") => Err(ApiError::csrf()),
        403 if code.as_deref() == Some("origin.rejected") => Err(ApiError::origin()),
        403 | 409 if code.as_deref().is_some_and(|item| item.starts_with("bootstrap.")) => Ok(()),
        _ => Err(ApiError::not_ready()),
    }
}

async fn https_exchange(origin: &str, ca_pem: &[u8], request: String) -> Result<(u16, Vec<u8>), IdentityFault> {
    let (host, port) = split_origin(origin).map_err(|_| IdentityFault::Unavailable)?;
    let config = client_config(ca_pem).map_err(|_| IdentityFault::Unavailable)?;
    let connector = TlsConnector::from(std::sync::Arc::new(config));
    let connect = async move {
        let server_name = ServerName::try_from(host.clone()).map_err(|_| IdentityFault::Unavailable)?;
        let tcp = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|_| IdentityFault::Unavailable)?;
        connector
            .connect(server_name, tcp)
            .await
            .map_err(|_| IdentityFault::Unavailable)
    };
    let mut stream = timeout(Duration::from_secs(5), connect)
        .await
        .map_err(|_| IdentityFault::Unavailable)?
        .map_err(|_| IdentityFault::Unavailable)?;
    timeout(Duration::from_secs(5), stream.write_all(request.as_bytes()))
        .await
        .map_err(|_| IdentityFault::Unavailable)?
        .map_err(|_| IdentityFault::Unavailable)?;
    let mut buf = Vec::new();
    let read = timeout(Duration::from_secs(5), async {
        let mut chunk = [0u8; 4096];
        loop {
            if buf.len() > 65_536 + 8192 {
                return Err(IdentityFault::Untyped);
            }
            let count = stream.read(&mut chunk).await.map_err(|_| IdentityFault::Unavailable)?;
            if count == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..count]);
            if header_end(&buf).is_some() && body_complete(&buf) {
                break;
            }
        }
        Ok(())
    });
    read.await.map_err(|_| IdentityFault::Unavailable)?.map_err(|_| IdentityFault::Unavailable)?;
    let head_end = header_end(&buf).ok_or(IdentityFault::Untyped)?;
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| IdentityFault::Untyped)?;
    let status = status_code(head).ok_or(IdentityFault::Untyped)?;
    let body = decode_body(head, &buf[head_end + 4..])?;
    Ok((status, body))
}

fn header_injection(value: &str) -> bool {
    value.bytes().any(|byte| byte == b'\r' || byte == b'\n')
}

fn error_code(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    value.get("error")?.get("code")?.as_str().map(str::to_owned)
}

fn parse_project(value: &Value) -> Result<VerifiedProject, IdentityFault> {
    let object = value.as_object().ok_or(IdentityFault::Untyped)?;
    if object.len() != 3 {
        return Err(IdentityFault::Untyped);
    }
    let project_id = object.get("project_id").and_then(Value::as_str).ok_or(IdentityFault::Untyped)?;
    if !ident_ok(project_id) {
        return Err(IdentityFault::Untyped);
    }
    let role = object.get("role").and_then(Value::as_str).ok_or(IdentityFault::Untyped)?;
    if !ROLES.contains(&role) {
        return Err(IdentityFault::Untyped);
    }
    let capabilities = object.get("capabilities").and_then(Value::as_array).ok_or(IdentityFault::Untyped)?;
    let mut names = Vec::new();
    for capability in capabilities {
        let name = capability.as_str().ok_or(IdentityFault::Untyped)?;
        if !CAPABILITIES.contains(&name) {
            return Err(IdentityFault::Untyped);
        }
        names.push(name.to_owned());
    }
    Ok(VerifiedProject {
        project_id: project_id.to_owned(),
        role: role.to_owned(),
        capabilities: names,
    })
}

fn require_const(object: &serde_json::Map<String, Value>, key: &str, expected: &str) -> Result<(), IdentityFault> {
    if object.get(key).and_then(Value::as_str) == Some(expected) {
        Ok(())
    } else {
        Err(IdentityFault::Untyped)
    }
}

fn bounded_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    min: usize,
    max: usize,
) -> Result<String, IdentityFault> {
    let text = object.get(key).and_then(Value::as_str).ok_or(IdentityFault::Untyped)?;
    if text.len() < min || text.len() > max {
        return Err(IdentityFault::Untyped);
    }
    Ok(text.to_owned())
}

fn split_origin(origin: &str) -> Result<(String, u16), ()> {
    let rest = origin.strip_prefix("https://").ok_or(())?;
    if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ']) {
        return Err(());
    }
    if let Some((host, port)) = rest.rsplit_once(':') {
        if host.is_empty() || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(());
        }
        let port: u16 = port.parse().map_err(|_| ())?;
        if port == 0 {
            return Err(());
        }
        Ok((host.to_owned(), port))
    } else {
        Ok((rest.to_owned(), 443))
    }
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn status_code(head: &str) -> Option<u16> {
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    let version = parts.next()?;
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return None;
    }
    parts.next()?.parse().ok()
}

fn body_complete(buf: &[u8]) -> bool {
    let Some(end) = header_end(buf) else {
        return false;
    };
    let Ok(head) = std::str::from_utf8(&buf[..end]) else {
        return false;
    };
    let headers = head.to_ascii_lowercase();
    if headers.contains("transfer-encoding: chunked") {
        return buf[end + 4..].windows(5).any(|window| window == b"0\r\n\r\n");
    }
    if let Some(length) = content_length(&headers) {
        return buf.len() >= end + 4 + length;
    }
    false
}

fn decode_body(head: &str, body: &[u8]) -> Result<Vec<u8>, IdentityFault> {
    let headers = head.to_ascii_lowercase();
    if headers.contains("transfer-encoding: chunked") {
        return decode_chunks(body);
    }
    let length = content_length(&headers).ok_or(IdentityFault::Untyped)?;
    if length > 65_536 || body.len() < length {
        return Err(IdentityFault::Untyped);
    }
    Ok(body[..length].to_vec())
}

fn content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "content-length" {
            return value.trim().parse().ok();
        }
    }
    None
}

fn decode_chunks(mut body: &[u8]) -> Result<Vec<u8>, IdentityFault> {
    let mut out = Vec::new();
    loop {
        let line_end = body.windows(2).position(|window| window == b"\r\n").ok_or(IdentityFault::Untyped)?;
        let line = std::str::from_utf8(&body[..line_end]).map_err(|_| IdentityFault::Untyped)?;
        let size = usize::from_str_radix(line.trim(), 16).map_err(|_| IdentityFault::Untyped)?;
        body = &body[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if size > 65_536 || out.len() + size > 65_536 || body.len() < size + 2 {
            return Err(IdentityFault::Untyped);
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size..];
        if !body.starts_with(b"\r\n") {
            return Err(IdentityFault::Untyped);
        }
        body = &body[2..];
    }
}

pub fn load_ca(path: &str) -> Result<Vec<u8>, ApiError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ApiError::not_ready())?;
    if !meta.file_type().is_file() || meta.len() > 1_048_576 {
        return Err(ApiError::not_ready());
    }
    let bytes = std::fs::read(path).map_err(|_| ApiError::not_ready())?;
    let _ = CertificateDer::from_pem_slice(&bytes).map_err(|_| ApiError::not_ready())?;
    Ok(bytes)
}
