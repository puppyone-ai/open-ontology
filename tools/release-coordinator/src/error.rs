//! Failures. Display text is a reason code, never a password or a driver string.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Input,
    Tls,
    Connect,
    LoginRole,
    Query { sqlstate: Option<String> },
    CommitUnknown,
}

impl StoreError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Tls => "tls",
            Self::Connect => "connect",
            Self::LoginRole => "login_role",
            Self::Query { .. } => "query",
            Self::CommitUnknown => "commit_unknown",
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query { sqlstate: Some(sqlstate) } => write!(formatter, "query {sqlstate}"),
            other => formatter.write_str(other.reason()),
        }
    }
}

pub fn query_error(err: tokio_postgres::Error) -> StoreError {
    StoreError::Query { sqlstate: err.code().map(|code| code.code().to_owned()) }
}
