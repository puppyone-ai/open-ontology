//! Failures. Display text is a reason code, never a password or a driver string.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryFailure {
    Closed,
    Io(std::io::ErrorKind),
    WrongType,
    WasNull,
    Unclassified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    ReadPointer,
    ReadCandidate,
    PreviousCandidate,
    PreviousBinding,
    RetirePrevious,
    PrepareTarget,
    KnownMappingQuery,
    KnownMappingDecode,
    NativeConnect,
    NativePending,
    NativeNamespace,
    NativeTable,
    NativeRelation,
    NativeColumns,
    NativeCommit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Input,
    Tls,
    Connect,
    LoginRole,
    Query { sqlstate: Option<String> },
    QueryFailure(QueryFailure),
    Phase { phase: Phase, error: Box<StoreError> },
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
            Self::QueryFailure(_) => "query",
            Self::Phase { error, .. } => error.reason(),
            Self::CommitUnknown => "commit_unknown",
        }
    }

    pub(crate) fn at(self, phase: Phase) -> Self {
        // Keep the innermost fixed phase, with no unbounded context chain.
        if matches!(self, Self::Phase { .. }) { self } else {
            Self::Phase { phase, error: Box::new(self) }
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query {
                sqlstate: Some(sqlstate),
            } => {
                let safe = sqlstate.len() == 5
                    && sqlstate.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
                write!(formatter, "query {}", if safe {sqlstate} else {"unknown_sqlstate"})
            }
            Self::QueryFailure(kind) => match kind {
                QueryFailure::Closed => formatter.write_str("query class=closed"),
                QueryFailure::Io(kind) => write!(formatter, "query class=io kind={kind:?}"),
                QueryFailure::WrongType => formatter.write_str("query class=wrong_type"),
                QueryFailure::WasNull => formatter.write_str("query class=was_null"),
                QueryFailure::Unclassified => formatter.write_str("query class=unclassified"),
            },
            Self::Phase { phase, error } => write!(formatter, "{error} phase={phase:?}"),
            other => formatter.write_str(other.reason()),
        }
    }
}

pub fn query_error(err: tokio_postgres::Error) -> StoreError {
    if let Some(code) = err.code() {
        return StoreError::Query { sqlstate: Some(code.code().to_owned()) };
    }
    if err.is_closed() { return StoreError::QueryFailure(QueryFailure::Closed); }
    StoreError::QueryFailure(classify_source(&err))
}

fn classify_source(error: &(dyn std::error::Error + 'static)) -> QueryFailure {
    let mut source = Some(error);
    for _ in 0..8 {
        let Some(error) = source else { break; };
        if error.is::<postgres_types::WrongType>() { return QueryFailure::WrongType; }
        if error.is::<postgres_types::WasNull>() { return QueryFailure::WasNull; }
        if let Some(error) = error.downcast_ref::<std::io::Error>() {
            return QueryFailure::Io(error.kind());
        }
        source = error.source();
    }
    QueryFailure::Unclassified
}

#[cfg(test)]
#[path = "../tests/observations/error_diagnostics.rs"]
mod tests;
