//! Store failures. Display text is a reason code, never a password or a driver string.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Input,
    Tls,
    Connect,
    LoginRole,
    PrincipalNotAuthentication,
    ProjectKeyMismatch,
    NotFound,
    Consistency,
    BootstrapConflict,
    InstallDrift { check: &'static str },
    ReducerDisagreement,
    StateTooLarge,
    CommitUnknown,
    Query { sqlstate: Option<String> },
}

impl StoreError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Tls => "tls",
            Self::Connect => "connect",
            Self::LoginRole => "login_role",
            Self::PrincipalNotAuthentication => "principal_not_authentication",
            Self::ProjectKeyMismatch => "project_key_mismatch",
            Self::NotFound => "not_found",
            Self::Consistency => "consistency",
            Self::BootstrapConflict => "bootstrap_conflict",
            Self::InstallDrift { .. } => "install_drift",
            Self::ReducerDisagreement => "reducer_disagreement",
            Self::StateTooLarge => "state_too_large",
            Self::CommitUnknown => "commit_unknown",
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

/// A server ErrorResponse is a definite answer. A missing SQLSTATE is transport uncertainty.
pub fn commit_error(err: &tokio_postgres::Error) -> StoreError {
    if err.code().is_some() {
        query_error(err)
    } else {
        StoreError::CommitUnknown
    }
}
