//! Sanitized client errors.
//!
//! `Debug` and `Display` carry the class, the gateway code, retry metadata,
//! and `query_id` when the envelope parsed. They do not carry the token, the
//! request SQL, the response body, the base URL, or proxy credentials.

use std::fmt;
use std::time::Duration;

use oo_core::api::ErrorCode;
use oo_core::text::QueryId;

use crate::endpoint::EndpointError;

#[derive(Clone, PartialEq, Eq)]
pub enum QueryError {
    Local(LocalError),
    Transport(TransportError),
    Protocol(ProtocolError),
    Gateway(GatewayError),
}

#[derive(Clone, PartialEq, Eq)]
pub enum LocalError {
    Endpoint(EndpointError),
    Token,
    Config(&'static str),
    RequestRejected,
    RequestTooLarge { len: usize, cap: usize },
    Trust,
    Client,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    Connect,
    Timeout,
    Dns,
    Incomplete,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    ContentType,
    Redirect,
    DeclaredTooLarge,
    BodyTooLarge,
    ConflictingFraming,
    Rejected,
    Status { status: u16 },
}

#[derive(Clone, PartialEq, Eq)]
pub struct GatewayError {
    status: u16,
    code: ErrorCode,
    retryable: bool,
    query_id: Option<QueryId>,
    retry_after: Option<Duration>,
    message: String,
    hint: Option<String>,
}

impl GatewayError {
    pub(crate) fn new(
        status: u16,
        code: ErrorCode,
        retryable: bool,
        query_id: Option<QueryId>,
        retry_after: Option<Duration>,
        message: String,
        hint: Option<String>,
    ) -> Self {
        Self {
            status,
            code,
            retryable,
            query_id,
            retry_after,
            message,
            hint,
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn retryable(&self) -> bool {
        self.retryable
    }

    pub fn query_id(&self) -> Option<&QueryId> {
        self.query_id.as_ref()
    }

    pub fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }

    /// Agent-facing text after the request token and request SQL were removed.
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }
}

impl QueryError {
    pub fn query_id(&self) -> Option<&QueryId> {
        match self {
            Self::Gateway(error) => error.query_id(),
            _ => None,
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Gateway(error) => error.retryable(),
            _ => false,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Gateway(error) => error.retry_after(),
            _ => None,
        }
    }

    pub fn gateway(&self) -> Option<&GatewayError> {
        match self {
            Self::Gateway(error) => Some(error),
            _ => None,
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(error) => write!(formatter, "local {error}"),
            Self::Transport(error) => write!(formatter, "transport {error}"),
            Self::Protocol(error) => write!(formatter, "protocol {error}"),
            Self::Gateway(error) => write!(formatter, "gateway {error}"),
        }
    }
}

impl fmt::Debug for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for QueryError {}

impl fmt::Display for LocalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Endpoint(error) => write!(formatter, "endpoint {error}"),
            Self::Token => formatter.write_str("token rejected"),
            Self::Config(reason) => write!(formatter, "config {reason}"),
            Self::RequestRejected => formatter.write_str("request rejected by the API contract"),
            Self::RequestTooLarge { len, cap } => {
                write!(formatter, "request has {len} bytes and the cap is {cap}")
            }
            Self::Trust => formatter.write_str("trust material rejected"),
            Self::Client => formatter.write_str("HTTP client rejected the configuration"),
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Connect => "connect failed; the client stopped",
            Self::Timeout => "the client stopped waiting before a complete response",
            Self::Dns => "name resolution failed; the client stopped",
            Self::Incomplete => "the response ended before a complete body; the client stopped",
            Self::Other => "transport failed; the client stopped",
        })
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContentType => formatter.write_str("content type is not application/json"),
            Self::Redirect => formatter.write_str("redirect refused"),
            Self::DeclaredTooLarge => {
                formatter.write_str("declared body length exceeds the response cap")
            }
            Self::BodyTooLarge => formatter.write_str("response body exceeds the cap"),
            Self::ConflictingFraming => formatter.write_str("response framing conflicts"),
            Self::Rejected => formatter.write_str("response rejected by the API contract"),
            Self::Status { status } => write!(formatter, "HTTP status {status}"),
        }
    }
}

impl fmt::Display for GatewayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} status {} retryable {}",
            wire_code(self.code),
            self.status,
            self.retryable
        )?;
        if let Some(query_id) = &self.query_id {
            write!(formatter, " query_id {query_id}")?;
        }
        if let Some(retry_after) = self.retry_after {
            write!(formatter, " retry_after_ms {}", retry_after.as_millis())?;
        }
        Ok(())
    }
}

pub(crate) fn wire_code(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Unauthenticated => "UNAUTHENTICATED",
        ErrorCode::SqlParseError => "SQL_PARSE_ERROR",
        ErrorCode::StatementNotAllowed => "STATEMENT_NOT_ALLOWED",
        ErrorCode::FunctionNotAllowed => "FUNCTION_NOT_ALLOWED",
        ErrorCode::RelationNotFound => "RELATION_NOT_FOUND",
        ErrorCode::ColumnNotAllowed => "COLUMN_NOT_ALLOWED",
        ErrorCode::UnqualifiedRelation => "UNQUALIFIED_RELATION",
        ErrorCode::CostTooHigh => "COST_TOO_HIGH",
        ErrorCode::Timeout => "TIMEOUT",
        ErrorCode::TablePaused => "TABLE_PAUSED",
        ErrorCode::QueryTooShort => "QUERY_TOO_SHORT",
        ErrorCode::EnumerationRefused => "ENUMERATION_REFUSED",
        ErrorCode::RateLimited => "RATE_LIMITED",
        ErrorCode::Overloaded => "OVERLOADED",
        ErrorCode::Internal => "INTERNAL",
        ErrorCode::AuditUnavailable => "AUDIT_UNAVAILABLE",
        ErrorCode::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
        ErrorCode::Conflict => "CONFLICT",
        ErrorCode::VersionUnsupported => "VERSION_UNSUPPORTED",
        ErrorCode::SchemaInvalid => "SCHEMA_INVALID",
    }
}

pub(crate) fn classify_transport(error: &reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else if error.is_connect() {
        TransportError::Connect
    } else if error.is_dns() {
        TransportError::Dns
    } else if error.is_body() || error.is_request() || error.is_decode() {
        TransportError::Incomplete
    } else {
        TransportError::Other
    }
}
