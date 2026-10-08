//! On-wire adapter for getIdentityCurrent.
//!
//! `try_from_caller_json` never succeeds. A verified value is built only from
//! a successful typed envelope fetched from the pinned origin.

use std::time::Duration;

use rustls::pki_types::ServerName;
use serde_json::{Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

use crate::error::Fault;
use crate::ids::{ct_eq, hex64, ident_ok, session_cookie_ok, subject_ok};
use crate::jsonutil::parse_exact;
use crate::tlsutil::client_config;

const PATH: &str = "/control/v1/identity/current";
const CONSUME_PATH: &str = "/control/v1/auth/bootstrap/consume";
const UNKNOWN_BOOTSTRAP: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const _: () = assert!(UNKNOWN_BOOTSTRAP.len() == 43);
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

impl VerifiedIdentity {
    pub fn project(&self, project_id: &str) -> Option<&VerifiedProject> {
        self.projects.iter().find(|project| project.project_id == project_id)
    }

    pub fn require_admin(&self, project_id: &str) -> Result<(), Fault> {
        let Some(project) = self.project(project_id) else {
            return Err(Fault::forbidden());
        };
        if project.capabilities.iter().any(|item| item == "project.admin") {
            Ok(())
        } else {
            Err(Fault::forbidden())
        }
    }
}

/// Caller JSON is never authority, including a well-formed envelope.
pub fn try_from_caller_json(_bytes: &[u8]) -> Result<VerifiedIdentity, Fault> {
    Err(Fault::unauthenticated())
}

pub fn parse_current_envelope(bytes: &[u8]) -> Result<VerifiedIdentity, Fault> {
    let value = parse_exact(bytes).map_err(|_| Fault::not_ready())?;
    let object = value.as_object().ok_or_else(Fault::not_ready)?;
    if object.len() != CURRENT_KEYS.len() || CURRENT_KEYS.iter().any(|key| !object.contains_key(*key)) {
        return Err(Fault::not_ready());
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
        return Err(Fault::not_ready());
    }
    let subject = bounded_string(object, "subject", 1, 256)?;
    if !subject_ok(&subject) {
        return Err(Fault::not_ready());
    }
    let issuer = bounded_string(object, "issuer", 8, 512)?;
    let config_commit = bounded_string(object, "config_commit", 64, 64)?;
    if !hex64(&config_commit) {
        return Err(Fault::not_ready());
    }
    let revision = object.get("revision").and_then(Value::as_i64).ok_or_else(Fault::not_ready)?;
    if revision < 1 || object.get("revision").and_then(Value::as_u64).is_none() {
        return Err(Fault::not_ready());
    }
    let csrf_token = bounded_string(object, "csrf_token", 43, 43)?;
    if !session_cookie_ok(&csrf_token) {
        return Err(Fault::not_ready());
    }
    let session = object.get("session").and_then(Value::as_object).ok_or_else(Fault::not_ready)?;
    if session.len() != 3
        || !session.contains_key("idle_expires_at")
        || !session.contains_key("absolute_expires_at")
        || session.get("revoked") != Some(&Value::Bool(false))
    {
        return Err(Fault::not_ready());
    }
    if session.get("idle_expires_at").and_then(Value::as_str).is_none()
        || session.get("absolute_expires_at").and_then(Value::as_str).is_none()
    {
        return Err(Fault::not_ready());
    }
    let projects_value = object.get("projects").and_then(Value::as_array).ok_or_else(Fault::not_ready)?;
    let mut projects = Vec::new();
    for project in projects_value {
        projects.push(parse_project(project)?);
    }
    Ok(VerifiedIdentity { subject, issuer, config_commit, revision, csrf_token, projects })
}

pub async fn fetch_current(origin: &str, ca_pem: &[u8], cookie: &str) -> Result<VerifiedIdentity, Fault> {
    if !session_cookie_ok(cookie) || injected(cookie) || injected(origin) {
        return Err(Fault::unauthenticated());
    }
    let (host_header, _) = split_origin(origin).map_err(|_| Fault::not_ready())?;
    let request = format!(
        "GET {PATH} HTTP/1.1\r\nHost: {host_header}\r\nCookie: __Host-oo-session={cookie}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    let (status, body) = exchange(origin, ca_pem, request).await?;
    if status == 401 {
        return Err(Fault::unauthenticated());
    }
    if status != 200 {
        return Err(Fault::not_ready());
    }
    parse_current_envelope(&body)
}

async fn exchange(origin: &str, ca_pem: &[u8], request: String) -> Result<(u16, Vec<u8>), Fault> {
    let (host, port) = split_origin(origin).map_err(|_| Fault::not_ready())?;
    let config = client_config(ca_pem).map_err(|_| Fault::not_ready())?;
    let connector = TlsConnector::from(std::sync::Arc::new(config));
    let connect = async move {
        let server_name = ServerName::try_from(host.clone()).map_err(|_| Fault::not_ready())?;
        let tcp = TcpStream::connect((host.as_str(), port)).await.map_err(|_| Fault::not_ready())?;
        connector.connect(server_name, tcp).await.map_err(|_| Fault::not_ready())
    };
    let mut stream = timeout(Duration::from_secs(5), connect).await.map_err(|_| Fault::not_ready())?.map_err(|_| Fault::not_ready())?;
    timeout(Duration::from_secs(5), stream.write_all(request.as_bytes()))
        .await
        .map_err(|_| Fault::not_ready())?
        .map_err(|_| Fault::not_ready())?;
    let mut buf = Vec::new();
    timeout(Duration::from_secs(5), async {
        let mut chunk = [0_u8; 4096];
        loop {
            if buf.len() > 65_536 + 8192 {
                return Err(Fault::not_ready());
            }
            let count = stream.read(&mut chunk).await.map_err(|_| Fault::not_ready())?;
            if count == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..count]);
            if header_end(&buf).is_some() && body_complete(&buf) {
                break;
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| Fault::not_ready())?
    .map_err(|_| Fault::not_ready())?;
    let head_end = header_end(&buf).ok_or_else(Fault::not_ready)?;
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| Fault::not_ready())?;
    let status = status_code(head).ok_or_else(Fault::not_ready)?;
    let body = decode_body(head, &buf[head_end + 4..])?;
    Ok((status, body))
}

fn injected(value: &str) -> bool {
    value.bytes().any(|byte| byte == b'\r' || byte == b'\n')
}

fn error_code(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    value.get("error")?.get("code")?.as_str().map(str::to_owned)
}

pub fn presented<'a>(origin_header: Option<&'a str>, csrf_header: Option<&'a str>, allow: &str) -> Result<(&'a str, &'a str), Fault> {
    let Some(origin) = origin_header else {
        return Err(Fault::origin());
    };
    if injected(origin) || !ct_eq(allow, origin) {
        return Err(Fault::origin());
    }
    let Some(csrf) = csrf_header else {
        return Err(Fault::csrf());
    };
    if injected(csrf) || !session_cookie_ok(csrf) {
        return Err(Fault::csrf());
    }
    Ok((origin, csrf))
}

/// Identity `current` rotates the CSRF token. Confirm the presented token
/// against the stored hash first. `bootstrap.*` means origin and CSRF matched
/// and the unknown token was refused.
pub async fn confirm_presented_csrf(origin: &str, ca_pem: &[u8], cookie: &str, caller_origin: &str, csrf: &str) -> Result<(), Fault> {
    if !session_cookie_ok(cookie) || injected(cookie) || injected(origin) || injected(caller_origin) || injected(csrf) {
        return Err(Fault::unauthenticated());
    }
    let (host_header, _) = split_origin(origin).map_err(|_| Fault::not_ready())?;
    let body = format!("{{\"token\":\"{UNKNOWN_BOOTSTRAP}\"}}");
    let length = body.len();
    let request = format!(
        "POST {CONSUME_PATH} HTTP/1.1\r\nHost: {host_header}\r\nCookie: __Host-oo-session={cookie}\r\nOrigin: {caller_origin}\r\nX-OO-CSRF: {csrf}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}"
    );
    let (status, bytes) = exchange(origin, ca_pem, request).await?;
    let code = error_code(&bytes);
    match status {
        401 if code.as_deref() == Some("unauthenticated") => Err(Fault::unauthenticated()),
        403 if code.as_deref() == Some("csrf.rejected") => Err(Fault::csrf()),
        403 if code.as_deref() == Some("origin.rejected") => Err(Fault::origin()),
        403 | 409 if code.as_deref().is_some_and(|item| item.starts_with("bootstrap.")) => Ok(()),
        _ => Err(Fault::not_ready()),
    }
}

fn parse_project(value: &Value) -> Result<VerifiedProject, Fault> {
    let object = value.as_object().ok_or_else(Fault::not_ready)?;
    if object.len() != 3 {
        return Err(Fault::not_ready());
    }
    let project_id = object.get("project_id").and_then(Value::as_str).ok_or_else(Fault::not_ready)?;
    if !ident_ok(project_id) {
        return Err(Fault::not_ready());
    }
    let role = object.get("role").and_then(Value::as_str).ok_or_else(Fault::not_ready)?;
    if !ROLES.contains(&role) {
        return Err(Fault::not_ready());
    }
    let capabilities = object.get("capabilities").and_then(Value::as_array).ok_or_else(Fault::not_ready)?;
    let mut names = Vec::new();
    for capability in capabilities {
        let name = capability.as_str().ok_or_else(Fault::not_ready)?;
        if !CAPABILITIES.contains(&name) {
            return Err(Fault::not_ready());
        }
        names.push(name.to_owned());
    }
    Ok(VerifiedProject { project_id: project_id.to_owned(), role: role.to_owned(), capabilities: names })
}

fn require_const(object: &Map<String, Value>, key: &str, expected: &str) -> Result<(), Fault> {
    if object.get(key).and_then(Value::as_str) == Some(expected) { Ok(()) } else { Err(Fault::not_ready()) }
}

fn bounded_string(object: &Map<String, Value>, key: &str, min: usize, max: usize) -> Result<String, Fault> {
    let text = object.get(key).and_then(Value::as_str).ok_or_else(Fault::not_ready)?;
    if text.len() < min || text.len() > max {
        return Err(Fault::not_ready());
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
        return Ok((host.to_owned(), port));
    }
    Ok((rest.to_owned(), 443))
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn status_code(head: &str) -> Option<u16> {
    let mut parts = head.lines().next()?.split_whitespace();
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
        return false;
    }
    content_length(&headers).is_some_and(|length| buf.len() >= end + 4 + length)
}

fn decode_body(head: &str, body: &[u8]) -> Result<Vec<u8>, Fault> {
    let headers = head.to_ascii_lowercase();
    if headers.contains("transfer-encoding: chunked") {
        return Err(Fault::not_ready());
    }
    let length = content_length(&headers).ok_or_else(Fault::not_ready)?;
    if length > 65_536 || body.len() < length {
        return Err(Fault::not_ready());
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
