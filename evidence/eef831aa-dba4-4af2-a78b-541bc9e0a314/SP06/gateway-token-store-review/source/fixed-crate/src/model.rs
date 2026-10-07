//! Issue and lookup inputs. A principal, a role, and claims are stored data.
//! They are not a login, an IdP result, or an mTLS decision.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Barrier;

use crate::canon::{canonicalize_audience, canonicalize_claims, ident, one_role};
use crate::error::StoreError;
use crate::sql;

pub const SESSION_IDLE_MS: i64 = 1_800_000;
pub const SESSION_ABSOLUTE_MS: i64 = 28_800_000;
pub const CLIENT_DEFAULT_MS: i64 = 3_600_000;
pub const CLIENT_MAX_MS: i64 = 7_776_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Session,
    Client,
}

impl TokenKind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Session => "oost_",
            Self::Client => "ooct_",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Client => "client",
        }
    }

    pub fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "session" => Ok(Self::Session),
            "client" => Ok(Self::Client),
            _ => Err(StoreError::Consistency),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallSource {
    System,
    Injected,
}

impl WallSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Injected => "injected",
        }
    }
}

pub trait WallClock: Send + Sync {
    fn now_ms(&self) -> Result<i64, StoreError>;
    fn source(&self) -> WallSource;
}

pub struct SystemWall;

impl WallClock for SystemWall {
    fn now_ms(&self) -> Result<i64, StoreError> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?;
        i64::try_from(elapsed.as_millis()).map_err(|_| StoreError::Unavailable)
    }

    fn source(&self) -> WallSource {
        WallSource::System
    }
}

#[derive(Clone, Copy)]
pub struct FixedWall(pub i64);

impl WallClock for FixedWall {
    fn now_ms(&self) -> Result<i64, StoreError> {
        Ok(self.0)
    }

    fn source(&self) -> WallSource {
        WallSource::Injected
    }
}

#[derive(Clone, Default)]
pub struct WaitGate {
    pub paused: Option<Arc<Barrier>>,
    pub release: Option<Arc<Barrier>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentTuple {
    pub project_id: String,
    pub principal_id: String,
    pub role: String,
    pub claims: Vec<(String, String)>,
    pub policy_version: String,
    pub knowledge_version: String,
    pub audience_url: String,
    pub sandbox_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueGrant {
    pub kind: TokenKind,
    pub current: CurrentTuple,
    pub client_ttl_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedGrant {
    pub kind: TokenKind,
    pub project_id: String,
    pub principal_id: String,
    pub role: String,
    pub claims_canonical: String,
    pub policy_version: String,
    pub knowledge_version: String,
    pub audience_url: String,
    pub sandbox_id: Option<String>,
    pub session_id: Option<String>,
    pub client_ttl_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredTransport {
    EgressProxy { sandbox_id: String, session_id: String },
    ExternalClient,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevokeScope {
    Token { project_id: String, token_sha256: String },
    Session { project_id: String, session_id: String },
    Principal { project_id: String, principal_id: String },
    Project { project_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevokeKey {
    pub scope: &'static str,
    pub project_id: String,
    pub principal_id: String,
    pub session_id: String,
    pub token_sha256: String,
}

pub fn check_grant(grant: &IssueGrant) -> Result<CheckedGrant, StoreError> {
    let current = &grant.current;
    ident(&current.project_id, 128)?;
    ident(&current.principal_id, 128)?;
    if is_login_name(&current.principal_id) {
        return Err(StoreError::PrincipalNotAuthentication);
    }
    one_role(&current.role)?;
    ident(&current.policy_version, 64)?;
    ident(&current.knowledge_version, 64)?;
    let claims_canonical = canonicalize_claims(&current.claims)?;
    let audience_url = canonicalize_audience(&current.audience_url)?;
    let (sandbox_id, session_id) = match grant.kind {
        TokenKind::Session => {
            let sandbox = required_id(current.sandbox_id.as_deref())?;
            let session = required_id(current.session_id.as_deref())?;
            if grant.client_ttl_ms.is_some() {
                return Err(StoreError::Input);
            }
            (Some(sandbox), Some(session))
        }
        TokenKind::Client => {
            if current.sandbox_id.is_some() || current.session_id.is_some() {
                return Err(StoreError::Binding);
            }
            if let Some(ttl) = grant.client_ttl_ms
                && !(1..=CLIENT_MAX_MS).contains(&ttl)
            {
                return Err(StoreError::Input);
            }
            (None, None)
        }
    };
    Ok(CheckedGrant {
        kind: grant.kind,
        project_id: current.project_id.clone(),
        principal_id: current.principal_id.clone(),
        role: current.role.clone(),
        claims_canonical,
        policy_version: current.policy_version.clone(),
        knowledge_version: current.knowledge_version.clone(),
        audience_url,
        sandbox_id,
        session_id,
        client_ttl_ms: grant.client_ttl_ms,
    })
}

pub fn check_revoke(scope: &RevokeScope) -> Result<RevokeKey, StoreError> {
    let (name, project, principal, session, hash) = match scope {
        RevokeScope::Token { project_id, token_sha256 } => {
            ("token", project_id, "", "", token_sha256.as_str())
        }
        RevokeScope::Session { project_id, session_id } => {
            ("session", project_id, "", session_id.as_str(), "")
        }
        RevokeScope::Principal { project_id, principal_id } => {
            ("principal", project_id, principal_id.as_str(), "", "")
        }
        RevokeScope::Project { project_id } => ("project", project_id, "", "", ""),
    };
    ident(project, 128)?;
    if !principal.is_empty() {
        ident(principal, 128)?;
    }
    if !session.is_empty() {
        ident(session, 128)?;
    }
    if name == "token" && !is_sha256_hex(hash) {
        return Err(StoreError::Input);
    }
    Ok(RevokeKey {
        scope: name,
        project_id: project.to_owned(),
        principal_id: principal.to_owned(),
        session_id: session.to_owned(),
        token_sha256: hash.to_owned(),
    })
}

pub fn is_login_name(value: &str) -> bool {
    value == sql::ADMIN_ROLE || value == sql::RUNTIME_ROLE || value == sql::FIXTURE_ROLE
}

pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn required_id(value: Option<&str>) -> Result<String, StoreError> {
    let Some(value) = value else {
        return Err(StoreError::Input);
    };
    ident(value, 128)?;
    Ok(value.to_owned())
}

pub fn deadlines(kind: TokenKind, now: i64, client_ttl_ms: Option<i64>) -> Result<(Option<i64>, i64), StoreError> {
    if now < 0 {
        return Err(StoreError::Input);
    }
    match kind {
        TokenKind::Session => {
            let idle = now.checked_add(SESSION_IDLE_MS).ok_or(StoreError::Input)?;
            let absolute = now.checked_add(SESSION_ABSOLUTE_MS).ok_or(StoreError::Input)?;
            Ok((Some(idle), absolute))
        }
        TokenKind::Client => {
            let ttl = client_ttl_ms.unwrap_or(CLIENT_DEFAULT_MS);
            if !(1..=CLIENT_MAX_MS).contains(&ttl) {
                return Err(StoreError::Input);
            }
            Ok((None, now.checked_add(ttl).ok_or(StoreError::Input)?))
        }
    }
}

pub fn slide_idle(now: i64, absolute: i64) -> Result<i64, StoreError> {
    let idle = now.checked_add(SESSION_IDLE_MS).ok_or(StoreError::Input)?;
    Ok(idle.min(absolute))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenView {
    pub project_id: String,
    pub principal_id: String,
    pub role: String,
    pub claims_canonical: String,
    pub policy_version: String,
    pub knowledge_version: String,
    pub audience_url: String,
    pub sandbox_id: Option<String>,
    pub session_id: Option<String>,
}

pub fn check_for_kind(kind: TokenKind, current: &CurrentTuple) -> Result<CheckedGrant, StoreError> {
    check_grant(&IssueGrant {
        kind,
        current: current.clone(),
        client_ttl_ms: None,
    })
}

pub fn wall_status(now: i64, issued: i64, idle: Option<i64>, absolute: i64) -> Result<(), StoreError> {
    if now < issued || now >= absolute {
        return Err(StoreError::Expired);
    }
    if let Some(idle) = idle
        && now >= idle
    {
        return Err(StoreError::Expired);
    }
    Ok(())
}

pub fn classify(
    frozen: &FrozenView,
    presented: &CheckedGrant,
    gateway_audience: &str,
    transport: &DeclaredTransport,
) -> Result<(), StoreError> {
    if frozen.project_id != presented.project_id
        || frozen.principal_id != presented.principal_id
        || frozen.policy_version != presented.policy_version
        || frozen.knowledge_version != presented.knowledge_version
    {
        return Err(StoreError::Stale);
    }
    if frozen.audience_url != presented.audience_url || frozen.audience_url != gateway_audience {
        return Err(StoreError::Audience);
    }
    if frozen.role != presented.role {
        return Err(StoreError::Role);
    }
    if frozen.claims_canonical != presented.claims_canonical {
        return Err(StoreError::Claims);
    }
    match transport {
        DeclaredTransport::EgressProxy { sandbox_id, session_id } => {
            if presented.kind != TokenKind::Session
                || frozen.sandbox_id.as_deref() != Some(sandbox_id.as_str())
                || frozen.session_id.as_deref() != Some(session_id.as_str())
                || presented.sandbox_id.as_deref() != Some(sandbox_id.as_str())
                || presented.session_id.as_deref() != Some(session_id.as_str())
            {
                return Err(StoreError::Binding);
            }
        }
        DeclaredTransport::ExternalClient => {
            if presented.kind != TokenKind::Client || frozen.sandbox_id.is_some() || frozen.session_id.is_some() {
                return Err(StoreError::Binding);
            }
        }
    }
    Ok(())
}
