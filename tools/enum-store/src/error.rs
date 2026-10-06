//! Failures. Display text is a reason code, never a password or a driver string.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Input,
    Tls,
    Connect,
    LoginRole,
    ProjectBinding,
    Allowlist,
    ReportMismatch,
    SnapshotDrift,
    SnapshotRefused {
        reason: &'static str,
        detail: String,
    },
    HelperDisagreement,
    Flag,
    InstallDrift {
        check: &'static str,
    },
    Collision {
        check: &'static str,
    },
    CasConflict,
    Activation,
    NotInstalled,
    CommitUnknown,
    Query {
        sqlstate: Option<String>,
    },
}

impl StoreError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Tls => "tls",
            Self::Connect => "connect",
            Self::LoginRole => "login_role",
            Self::ProjectBinding => "project_binding",
            Self::Allowlist => "allowlist",
            Self::ReportMismatch => "report_mismatch",
            Self::SnapshotDrift => "snapshot_drift",
            Self::SnapshotRefused { .. } => "snapshot_refused",
            Self::HelperDisagreement => "helper_disagreement",
            Self::Flag => "flag",
            Self::InstallDrift { .. } => "install_drift",
            Self::Collision { .. } => "collision",
            Self::CasConflict => "cas_conflict",
            Self::Activation => "activation",
            Self::NotInstalled => "not_installed",
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
            Self::Collision { check } => write!(formatter, "collision {check}"),
            Self::SnapshotRefused { reason, detail } => {
                write!(formatter, "snapshot_refused {reason} {detail}")
            }
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
