//! Failures. Display text is a reason code, never a token, password, or driver string.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Input,
    Tls,
    Connect,
    Unavailable,
    LoginRole,
    PrincipalNotAuthentication,
    ForgedAuthClaim,
    RoleMerge,
    Audience,
    Binding,
    Stale,
    Claims,
    Role,
    Expired,
    Revoked,
    NotFound,
    ScopeClosed,
    InstallDrift { check: &'static str },
    Randomness,
    CommitUnknown,
    Consistency,
    Query { sqlstate: Option<String> },
}

impl StoreError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Tls => "tls",
            Self::Connect => "connect",
            Self::Unavailable => "unavailable",
            Self::LoginRole => "login_role",
            Self::PrincipalNotAuthentication => "principal_not_authentication",
            Self::ForgedAuthClaim => "forged_auth_claim",
            Self::RoleMerge => "role_merge",
            Self::Audience => "audience",
            Self::Binding => "binding",
            Self::Stale => "stale",
            Self::Claims => "claims",
            Self::Role => "role",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
            Self::NotFound => "not_found",
            Self::ScopeClosed => "scope_closed",
            Self::InstallDrift { .. } => "install_drift",
            Self::Randomness => "randomness",
            Self::CommitUnknown => "commit_unknown",
            Self::Consistency => "consistency",
            Self::Query { .. } => "query",
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query { sqlstate: Some(sqlstate) } => write!(formatter, "query {sqlstate}"),
            Self::InstallDrift { check } => write!(formatter, "install_drift {check}"),
            other => formatter.write_str(other.reason()),
        }
    }
}

pub fn query_error(err: &tokio_postgres::Error) -> StoreError {
    StoreError::Query {
        sqlstate: err.code().map(|code| code.code().to_owned()),
    }
}

pub fn commit_error(err: &tokio_postgres::Error) -> StoreError {
    if err.code().is_some() {
        query_error(err)
    } else {
        StoreError::CommitUnknown
    }
}

pub fn drift(check: &'static str) -> StoreError {
    StoreError::InstallDrift { check }
}
