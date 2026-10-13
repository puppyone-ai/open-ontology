//! On-wire adapter for getIdentityCurrent.
//!
//! Caller JSON is not authority. A verified value comes only from the typed
//! envelope fetched from the configured origin.

use std::time::Duration;

use serde_json::Value;

use crate::error::ApiError;
use crate::ids::{ct_eq, hex64, ident_ok, session_cookie_ok};
use crate::jsonutil::parse_exact;

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
    pub revision: i64,
    pub csrf_token: String,
    pub projects: Vec<VerifiedProject>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Mutate,
    ReadSource,
    ReadImport,
    ReadCatalog,
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
        .user_agent("oo-source-control-api/v1")
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

pub fn mutation_checks(
    origin_header: Option<&str>,
    csrf_header: Option<&str>,
    allow: &[String],
    identity: &VerifiedIdentity,
    revision: i64,
) -> Result<(), ApiError> {
    let Some(origin) = origin_header else {
        return Err(ApiError::origin());
    };
    if !allow.iter().any(|item| ct_eq(item, origin)) {
        return Err(ApiError::origin());
    }
    let Some(csrf) = csrf_header else {
        return Err(ApiError::csrf());
    };
    if !session_cookie_ok(csrf) {
        return Err(ApiError::csrf());
    }
    if revision != identity.revision {
        return Err(ApiError::revision());
    }
    Ok(())
}

/// `getIdentityCurrent` rotates the CSRF token, so the presented token is
/// checked against the stored hash before that read. An unknown bootstrap
/// token is rejected after the CSRF check and is not consumed.
pub async fn confirm_csrf(client: &reqwest::Client, origin: &str, cookie: &str, csrf: &str) -> Result<(), ApiError> {
    const UNKNOWN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let body = format!("{{\"token\":\"{UNKNOWN}\"}}");
    let response = client
        .post(format!("{origin}/control/v1/auth/bootstrap/consume"))
        .header("cookie", format!("__Host-oo-session={cookie}"))
        .header("origin", origin)
        .header("x-oo-csrf", csrf)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| ApiError::not_ready("identity"))?;
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|_| ApiError::not_ready("identity"))?;
    if bytes.len() > 65_536 {
        return Err(ApiError::not_ready("identity"));
    }
    let value = parse_exact(&bytes).unwrap_or(serde_json::Value::Null);
    let code = value.pointer("/error/code").and_then(|item| item.as_str()).unwrap_or("");
    match status {
        401 => Err(ApiError::unauthenticated()),
        403 if code == "csrf.rejected" => Err(ApiError::csrf()),
        403 if code == "origin.rejected" => Err(ApiError::origin()),
        403 | 409 if code.starts_with("bootstrap.") => Ok(()),
        300..=399 => Err(ApiError::not_ready("identity")),
        _ => Err(ApiError::not_ready("identity")),
    }
}

pub fn authorize(identity: &VerifiedIdentity, project_id: &str, scope: Scope) -> Result<(), ApiError> {
    let Some(project) = identity.project(project_id) else {
        return Err(ApiError::forbidden("scope"));
    };
    let allowed = match scope {
        Scope::Mutate => has(&project.capabilities, "project.admin"),
        Scope::ReadSource | Scope::ReadImport | Scope::ReadCatalog => {
            has(&project.capabilities, "project.admin") || has(&project.capabilities, "audit.read")
        }
    };
    if allowed {
        Ok(())
    } else {
        Err(ApiError::forbidden("capability"))
    }
}

pub fn authorize_connector(identity: &VerifiedIdentity) -> Result<(), ApiError> {
    let allowed = identity.projects.iter().any(|project| {
        has(&project.capabilities, "identity.current")
            || has(&project.capabilities, "project.admin")
            || has(&project.capabilities, "audit.read")
    });
    if allowed {
        Ok(())
    } else {
        Err(ApiError::forbidden("capability"))
    }
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
    Ok(VerifiedProject { project_id: project_id.to_owned(), role: role.to_owned(), capabilities: names })
}

fn require_const(object: &serde_json::Map<String, Value>, key: &str, expected: &str) -> Result<(), IdentityFault> {
    if object.get(key).and_then(Value::as_str) == Some(expected) {
        Ok(())
    } else {
        Err(IdentityFault::Untyped)
    }
}

fn bounded_string(object: &serde_json::Map<String, Value>, key: &str, min: usize, max: usize) -> Result<String, IdentityFault> {
    let text = object.get(key).and_then(Value::as_str).ok_or(IdentityFault::Untyped)?;
    if text.len() < min || text.len() > max {
        return Err(IdentityFault::Untyped);
    }
    Ok(text.to_string())
}
