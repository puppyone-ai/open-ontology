//! On-wire adapter for getIdentityCurrent.
//!
//! Caller JSON is not authority. A verified value comes only from the typed
//! envelope fetched from the configured origin.

use std::time::Duration;

use serde_json::{Map, Value};

use crate::error::ApiError;
use crate::ids::{ct_eq, hex64, ident_ok, session_cookie_ok};
use crate::jsonutil::parse_request;

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
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct VerifiedIdentity {
    pub subject: String,
    pub revision: i64,
    pub csrf_token: String,
    pub projects: Vec<VerifiedProject>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Create,
    Read,
    Rollback,
    Pack,
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
            Self::Redirect | Self::Untyped | Self::Unavailable => ApiError::not_ready("identity"),
        }
    }
}

impl VerifiedIdentity {
    pub fn project(&self, project_id: &str) -> Option<&VerifiedProject> {
        self.projects.iter().find(|project| project.project_id == project_id)
    }
}

pub fn identity_client(ca_pem: &[u8]) -> Result<reqwest::Client, ApiError> {
    let certs = reqwest::Certificate::from_pem_bundle(ca_pem).map_err(|_| ApiError::not_ready("identity"))?;
    if certs.is_empty() {
        return Err(ApiError::not_ready("identity"));
    }
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .https_only(true)
        .http1_only()
        .referer(false)
        .connection_verbose(false)
        .user_agent("oo-release-control-api/v1")
        .tls_certs_only(certs)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| ApiError::not_ready("identity"))
}

pub async fn fetch_current(client: &reqwest::Client, origin: &str, cookie: &str) -> Result<VerifiedIdentity, IdentityFault> {
    if !session_cookie_ok(cookie) {
        return Err(IdentityFault::Unauthenticated);
    }
    let response = client
        .get(format!("{origin}{PATH}"))
        .header("cookie", format!("__Host-oo-session={cookie}"))
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|_| IdentityFault::Unavailable)?;
    classify_status(response.status().as_u16())?;
    let bytes = response.bytes().await.map_err(|_| IdentityFault::Unavailable)?;
    if bytes.len() > 65_536 {
        return Err(IdentityFault::Untyped);
    }
    parse_current_envelope(&bytes)
}

pub fn parse_current_envelope(bytes: &[u8]) -> Result<VerifiedIdentity, IdentityFault> {
    let value = parse_request(bytes).map_err(|_| IdentityFault::Untyped)?;
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
    if subject.bytes().any(|byte| byte.is_ascii_whitespace() || !byte.is_ascii()) {
        return Err(IdentityFault::Untyped);
    }
    let config_commit = bounded_string(object, "config_commit", 64, 64)?;
    if !hex64(&config_commit) {
        return Err(IdentityFault::Untyped);
    }
    let _issuer = bounded_string(object, "issuer", 8, 512)?;
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
        || session.get("revoked") != Some(&Value::Bool(false))
        || session.get("idle_expires_at").and_then(Value::as_str).is_none()
        || session.get("absolute_expires_at").and_then(Value::as_str).is_none()
    {
        return Err(IdentityFault::Untyped);
    }
    let projects_value = object.get("projects").and_then(Value::as_array).ok_or(IdentityFault::Untyped)?;
    let mut projects = Vec::new();
    for project in projects_value {
        projects.push(parse_project(project)?);
    }
    Ok(VerifiedIdentity { subject, revision, csrf_token, projects })
}

pub fn classify_status(status: u16) -> Result<(), IdentityFault> {
    match status {
        200 => Ok(()),
        401 => Err(IdentityFault::Unauthenticated),
        300..=399 => Err(IdentityFault::Redirect),
        _ => Err(IdentityFault::Unavailable),
    }
}

/// Confirm a mutation against a6f92.
///
/// `getIdentityCurrent` stores a new CSRF hash before it returns the new
/// token, so the presented token cannot equal that response. Identity's
/// consume route compares the header with the stored hash first. An unknown
/// bootstrap token then fails `bootstrap.*` and leaves the hash in place.
/// That route accepts only the identity origin, so the probe uses the
/// configured identity origin after the caller origin has matched a
/// deployment origin. `current` runs after the probe and supplies the
/// session, revision, and capabilities.
pub async fn authorize_mutation(
    client: &reqwest::Client,
    identity_origin: &str,
    cookie: &str,
    origin_header: Option<&str>,
    csrf_header: Option<&str>,
    allow: &[String],
    revision: i64,
) -> Result<VerifiedIdentity, ApiError> {
    let Some(origin) = origin_header else {
        return Err(ApiError::origin());
    };
    if injected(origin) || !allow.iter().any(|item| ct_eq(item, origin)) {
        return Err(ApiError::origin());
    }
    let Some(csrf) = csrf_header else {
        return Err(ApiError::csrf());
    };
    if injected(csrf) || !session_cookie_ok(csrf) || !session_cookie_ok(cookie) || injected(cookie) {
        return Err(ApiError::csrf());
    }
    confirm_presented_csrf(client, identity_origin, cookie, csrf).await?;
    let identity = fetch_current(client, identity_origin, cookie).await.map_err(IdentityFault::api)?;
    if revision != identity.revision {
        return Err(ApiError::revision());
    }
    Ok(identity)
}

const UNKNOWN_BOOTSTRAP: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const CONSUME_PATH: &str = "/control/v1/auth/bootstrap/consume";

async fn confirm_presented_csrf(client: &reqwest::Client, identity_origin: &str, cookie: &str, csrf: &str) -> Result<(), ApiError> {
    let body = format!("{{\"token\":\"{UNKNOWN_BOOTSTRAP}\"}}");
    let response = client
        .post(format!("{identity_origin}{CONSUME_PATH}"))
        .header("cookie", format!("__Host-oo-session={cookie}"))
        .header("origin", identity_origin)
        .header("x-oo-csrf", csrf)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| ApiError::not_ready("identity"))?;
    let status = response.status().as_u16();
    if (300..400).contains(&status) {
        return Err(ApiError::not_ready("identity"));
    }
    let bytes = response.bytes().await.map_err(|_| ApiError::not_ready("identity"))?;
    if bytes.len() > 65_536 {
        return Err(ApiError::not_ready("identity"));
    }
    let code = error_code(&bytes);
    match status {
        401 if code.as_deref() == Some("unauthenticated") => Err(ApiError::unauthenticated()),
        403 if code.as_deref() == Some("csrf.rejected") => Err(ApiError::csrf()),
        403 if code.as_deref() == Some("origin.rejected") => Err(ApiError::origin()),
        403 | 409 if code.as_deref().is_some_and(|item| item.starts_with("bootstrap.")) => Ok(()),
        _ => Err(ApiError::not_ready("identity")),
    }
}

fn injected(value: &str) -> bool {
    value.bytes().any(|byte| byte == b'\r' || byte == b'\n' || byte == 0)
}

fn error_code(bytes: &[u8]) -> Option<String> {
    let value: Value = parse_request(bytes).ok()?;
    let code = value.get("error")?.get("code")?.as_str()?;
    Some(code.to_owned())
}

pub fn authorize(identity: &VerifiedIdentity, project_id: &str, deployment: &str, scope: Scope) -> Result<(), ApiError> {
    if project_id != deployment {
        return Err(ApiError::forbidden("scope"));
    }
    let Some(project) = identity.project(project_id) else {
        return Err(ApiError::forbidden("scope"));
    };
    let allowed = match scope {
        Scope::Create | Scope::Rollback => has(&project.capabilities, "project.admin"),
        Scope::Read => {
            has(&project.capabilities, "project.admin")
                || has(&project.capabilities, "audit.read")
                || has(&project.capabilities, "review.decide")
                || has(&project.capabilities, "ontology.draft")
        }
        Scope::Pack => has(&project.capabilities, "project.admin") || has(&project.capabilities, "audit.read"),
    };
    if allowed { Ok(()) } else { Err(ApiError::forbidden("capability")) }
}

fn has(capabilities: &[String], name: &str) -> bool {
    capabilities.iter().any(|item| item == name)
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
    Ok(VerifiedProject { project_id: project_id.to_owned(), capabilities: names })
}

fn require_const(object: &Map<String, Value>, key: &str, expected: &str) -> Result<(), IdentityFault> {
    if object.get(key).and_then(Value::as_str) == Some(expected) { Ok(()) } else { Err(IdentityFault::Untyped) }
}

fn bounded_string(object: &Map<String, Value>, key: &str, min: usize, max: usize) -> Result<String, IdentityFault> {
    let text = object.get(key).and_then(Value::as_str).ok_or(IdentityFault::Untyped)?;
    if text.len() < min || text.len() > max { Err(IdentityFault::Untyped) } else { Ok(text.to_string()) }
}
