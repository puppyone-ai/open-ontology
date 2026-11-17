//! Error envelope (10 §8.1, §8.4). Every code a route can return is a
//! [`Code`] constant declared in the one macro below; the HTTP status and the
//! `retryable` flag come from `contracts/docs-api/errors.v1.json`, so a code
//! that is not in the contract cannot be sent (and `tests/contract.rs` checks
//! the whole list against the file).

use std::sync::OnceLock;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Response, StatusCode, header};
use serde_json::{Value, json};

use crate::store::StoreError;

/// A stable machine code from `errors.v1.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code(&'static str);

impl Code {
    /// A state code (no HTTP status of its own in the table) that the content
    /// route also answers with, as a 409: the file changed after it was hashed.
    pub const SOURCE_CHANGED_DURING_READ: Code = Code("DOCS_SOURCE_CHANGED_DURING_READ");

    /// The `DOCS_*` text.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

macro_rules! codes {
    ($($id:ident = $text:literal,)*) => {
        /// Codes the control API can return.
        pub mod code {
            use super::Code;
            $(pub const $id: Code = Code($text);)*
        }
        /// Every code above, for the contract test.
        pub const ALL_CODES: &[&str] = &[$($text),*];
    };
}

codes! {
    BAD_HOST = "DOCS_ENGINE_BAD_HOST",
    BAD_REQUEST = "DOCS_ENGINE_BAD_REQUEST",
    FORBIDDEN_ORIGIN = "DOCS_ENGINE_FORBIDDEN_ORIGIN",
    UNAUTHENTICATED = "DOCS_ENGINE_UNAUTHENTICATED",
    NOT_FOUND = "DOCS_ENGINE_NOT_FOUND",
    METHOD_NOT_ALLOWED = "DOCS_ENGINE_METHOD_NOT_ALLOWED",
    PAYLOAD_TOO_LARGE = "DOCS_ENGINE_PAYLOAD_TOO_LARGE",
    VERSION_UNSUPPORTED = "DOCS_ENGINE_VERSION_UNSUPPORTED",
    IDEMPOTENCY_KEY_REQUIRED = "DOCS_ENGINE_IDEMPOTENCY_KEY_REQUIRED",
    IDEMPOTENCY_CONFLICT = "DOCS_ENGINE_IDEMPOTENCY_CONFLICT",
    PRECONDITION_REQUIRED = "DOCS_ENGINE_PRECONDITION_REQUIRED",
    PRECONDITION_FAILED = "DOCS_ENGINE_PRECONDITION_FAILED",
    RATE_LIMITED = "DOCS_ENGINE_RATE_LIMITED",
    CURSOR_INVALID = "DOCS_ENGINE_CURSOR_INVALID",
    CURSOR_EXPIRED = "DOCS_ENGINE_CURSOR_EXPIRED",
    DB_UNAVAILABLE = "DOCS_ENGINE_DB_UNAVAILABLE",
    INTERNAL = "DOCS_ENGINE_INTERNAL",
    DRIVE_NOT_FOUND = "DOCS_SOURCE_DRIVE_NOT_FOUND",
    DRIVE_EXISTS = "DOCS_SOURCE_DRIVE_EXISTS",
    DRIVE_HAS_ACTIVE_SHARES = "DOCS_SOURCE_DRIVE_HAS_ACTIVE_SHARES",
    ROOT_MISSING = "DOCS_SOURCE_ROOT_MISSING",
    INVALID_CONFIG = "DOCS_SOURCE_INVALID_CONFIG",
    SYNC_RUNNING = "DOCS_SOURCE_SYNC_RUNNING",
    MASS_DELETE_HELD = "DOCS_SOURCE_MASS_DELETE_HELD",
    PARSE_NOT_READY = "DOCS_PARSE_NOT_READY",
    PACK_SCOPE_INVALID = "DOCS_PACK_SCOPE_INVALID",
    PACK_NOTHING_VISIBLE = "DOCS_PACK_NOTHING_VISIBLE",
    PACK_SEED_NOT_VISIBLE = "DOCS_PACK_SEED_NOT_VISIBLE",
    PACK_TASK_TOO_LONG = "DOCS_PACK_TASK_TOO_LONG",
    PACK_BUDGET_TOO_SMALL = "DOCS_PACK_BUDGET_TOO_SMALL",
    PACK_VERSION_NOT_FOUND = "DOCS_PACK_VERSION_NOT_FOUND",
    PACK_LEAK_GUARD = "DOCS_PACK_LEAK_GUARD",
    PACK_PROVENANCE_GAP = "DOCS_PACK_PROVENANCE_GAP",
    PACK_TOO_LARGE = "DOCS_PACK_TOO_LARGE",
    PACK_NOT_FOUND = "DOCS_PACK_NOT_FOUND",
    SHARE_NOT_FOUND = "DOCS_SHARE_NOT_FOUND",
    SHARE_EXPIRY_INVALID = "DOCS_SHARE_EXPIRY_INVALID",
    SHARE_ROLE_UNAVAILABLE = "DOCS_SHARE_ROLE_UNAVAILABLE",
    SHARE_TARGET_INVALID = "DOCS_SHARE_TARGET_INVALID",
}

const ERRORS_JSON: &str = include_str!("../../../../contracts/docs-api/errors.v1.json");

/// `(http status, retryable)` per code, from the contract file.
fn table() -> &'static std::collections::HashMap<String, (u16, bool)> {
    static TABLE: OnceLock<std::collections::HashMap<String, (u16, bool)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let doc: Value = serde_json::from_str(ERRORS_JSON).unwrap_or(Value::Null);
        doc["codes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| {
                        Some((
                            c["code"].as_str()?.to_owned(),
                            (
                                c["http"].as_u64().unwrap_or(500) as u16,
                                c["retryable"].as_bool().unwrap_or(false),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// A response-shaped failure.
#[derive(Clone, Debug)]
pub struct ApiError {
    /// Machine code.
    pub code: Code,
    /// HTTP status.
    pub status: u16,
    /// English text for logs, never shown by the UI.
    pub message: String,
    /// Whether the same request may succeed later.
    pub retryable: bool,
    /// Code specific fields.
    pub details: Value,
    /// Extra response headers.
    pub headers: Vec<(&'static str, String)>,
}

impl ApiError {
    /// Status and retryable from the contract.
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        let (status, retryable) = table().get(code.0).copied().unwrap_or((500, false));
        Self {
            code,
            status,
            message: message.into(),
            retryable,
            details: json!({}),
            headers: Vec::new(),
        }
    }

    /// Set `details`.
    pub fn details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    /// Add a response header.
    pub fn header(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }

    /// 400 with the JSON pointer of the offending field.
    pub fn bad(pointer: &str, message: impl Into<String>) -> Self {
        Self::new(code::BAD_REQUEST, message).details(json!({ "pointer": pointer }))
    }

    /// 404 for an unknown resource kind.
    pub fn not_found(resource: &str) -> Self {
        Self::new(code::NOT_FOUND, "not found").details(json!({ "resource": resource }))
    }

    /// The JSON body.
    pub fn body(&self) -> Value {
        json!({
            "code": self.code.0,
            "message": self.message,
            "retryable": self.retryable,
            "details": self.details,
        })
    }

    /// Render.
    pub fn into_response(self) -> Response<Body> {
        let mut builder = Response::builder()
            .status(StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
            .header(header::CONTENT_TYPE, "application/json; charset=utf-8");
        for (name, value) in &self.headers {
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                builder = builder.header(n, v);
            }
        }
        builder
            .body(Body::from(self.body().to_string()))
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }
}

impl From<crate::compile::PackError> for ApiError {
    fn from(e: crate::compile::PackError) -> Self {
        use crate::compile::PackError as P;
        match &e {
            P::ScopeInvalid { paths } => {
                Self::new(code::PACK_SCOPE_INVALID, "scope matches no files")
                    .details(json!({ "paths": paths }))
            }
            P::NothingVisible => {
                Self::new(code::PACK_NOTHING_VISIBLE, "nothing is visible in scope")
            }
            P::SeedNotVisible => Self::new(code::PACK_SEED_NOT_VISIBLE, "a seed is not visible"),
            P::TaskTooLong { max_chars } => Self::new(code::PACK_TASK_TOO_LONG, "task too long")
                .details(json!({ "max_chars": max_chars })),
            P::BudgetTooSmall { min_budget } => {
                Self::new(code::PACK_BUDGET_TOO_SMALL, "budget too small")
                    .details(json!({ "min_budget": min_budget }))
            }
            P::VersionNotFound => {
                Self::new(code::PACK_VERSION_NOT_FOUND, "no such ontology version")
            }
            P::LeakGuard { hits } => {
                Self::new(code::PACK_LEAK_GUARD, "leak guard stopped the pack")
                    .details(json!({ "hits": hits }))
            }
            P::ProvenanceGap { lines } => {
                Self::new(code::PACK_PROVENANCE_GAP, "a line has no provenance")
                    .details(json!({ "lines": lines }))
            }
            P::TooLarge => Self::new(code::PACK_TOO_LARGE, "pack exceeds the size limit"),
            P::BadRequest(why) => Self::new(code::BAD_REQUEST, *why),
            P::Store(_) | P::Internal(_) => Self::new(code::INTERNAL, "pack compile failed"),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        db_error(&e)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        db_error(&StoreError::from(e))
    }
}

impl From<crate::ingest::IngestError> for ApiError {
    fn from(e: crate::ingest::IngestError) -> Self {
        match e {
            crate::ingest::IngestError::Store(s) => db_error(&s),
            other => Self::new(code::INTERNAL, other.to_string()),
        }
    }
}

/// Connection-level failures are 503, everything else a 500; the message
/// never carries SQL text or values.
fn db_error(e: &StoreError) -> ApiError {
    match e {
        StoreError::Database { sqlstate, .. }
            if sqlstate.is_empty()
                || sqlstate.starts_with("08")
                || sqlstate.starts_with("57")
                || sqlstate == "53300" =>
        {
            ApiError::new(code::DB_UNAVAILABLE, "database unavailable")
        }
        StoreError::Database { sqlstate, .. } => ApiError::new(
            code::INTERNAL,
            format!("database error sqlstate={sqlstate}"),
        ),
        StoreError::Protocol(m) => ApiError::new(code::INTERNAL, m.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_come_from_the_contract() {
        let e = ApiError::new(code::NOT_FOUND, "x");
        assert_eq!((e.status, e.retryable), (404, false));
        let e = ApiError::new(code::DB_UNAVAILABLE, "x");
        assert_eq!((e.status, e.retryable), (503, true));
        let e = ApiError::new(code::MASS_DELETE_HELD, "x");
        assert_eq!(e.status, 422);
        let body = ApiError::bad("/name", "too long").body();
        assert_eq!(body["details"]["pointer"], "/name");
        assert_eq!(body["retryable"], false);
    }

    #[test]
    fn the_state_code_served_as_an_error_is_in_the_contract() {
        assert!(table().contains_key(Code::SOURCE_CHANGED_DURING_READ.as_str()));
    }

    #[test]
    fn every_declared_code_is_in_the_contract() {
        for c in ALL_CODES {
            assert!(table().contains_key(*c), "{c} missing from errors.v1.json");
        }
    }
}
