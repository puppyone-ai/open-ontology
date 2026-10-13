//! JSON error envelope for the loopback HTTP gateway.
//!
//! This module is the API schema. It does not route, authenticate, open a
//! database connection, or execute SQL. Constructors take a code only, so a
//! caller cannot place a bearer token, query string, or driver message in
//! the body.

use serde_json::{Map, Value};

/// HTTP statuses produced by Tower layers, not by [`ApiError`].
///
/// `TimeoutLayer::with_status_code` answers [`LAYER_TIMEOUT_STATUS`].
/// `RequestBodyLimitLayer` answers [`LAYER_BODY_LIMIT_STATUS`]. Their bodies
/// are the layer bodies. This process does not emit [`RATE_LIMITED_STATUS`]:
/// a request that waits on the one global permit is cut by the same deadline
/// and receives 408. [`OVERLOADED_STATUS`] is the documented breaker status.
/// This process does not sample PostgreSQL health and does not emit it.
pub const LAYER_TIMEOUT_STATUS: u16 = 408;
pub const LAYER_BODY_LIMIT_STATUS: u16 = 413;
pub const RATE_LIMITED_STATUS: u16 = 429;
pub const OVERLOADED_STATUS: u16 = 503;

pub const JSON_MEDIA_TYPE: &str = "application/json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiCode {
    Unauthenticated,
    Forbidden,
    QueryNotAllowed,
    NotFound,
    MethodNotAllowed,
    SqlUnsupported,
    Internal,
}

impl ApiCode {
    pub const ALL: [ApiCode; 7] = [
        ApiCode::Unauthenticated,
        ApiCode::Forbidden,
        ApiCode::QueryNotAllowed,
        ApiCode::NotFound,
        ApiCode::MethodNotAllowed,
        ApiCode::SqlUnsupported,
        ApiCode::Internal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Forbidden => "FORBIDDEN",
            Self::QueryNotAllowed => "QUERY_NOT_ALLOWED",
            Self::NotFound => "NOT_FOUND",
            Self::MethodNotAllowed => "METHOD_NOT_ALLOWED",
            Self::SqlUnsupported => "SQL_UNSUPPORTED",
            Self::Internal => "INTERNAL",
        }
    }

    pub const fn status(self) -> u16 {
        match self {
            Self::Unauthenticated => 401,
            Self::Forbidden => 403,
            Self::QueryNotAllowed => 400,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::SqlUnsupported => 501,
            Self::Internal => 500,
        }
    }

    pub const fn retryable(self) -> bool {
        matches!(self, Self::Internal)
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::Unauthenticated => "The bearer token was not accepted.",
            Self::Forbidden => "The token does not allow this metadata.",
            Self::QueryNotAllowed => "Query strings are not accepted.",
            Self::NotFound => "That path is not served.",
            Self::MethodNotAllowed => "That method is not served on this path.",
            Self::SqlUnsupported => "SQL execution is not served by this process.",
            Self::Internal => "The request could not be completed.",
        }
    }

    pub const fn hint(self) -> &'static str {
        match self {
            Self::Unauthenticated => "Present a current client token for this gateway audience.",
            Self::Forbidden => "The project, role, and scope are taken from the token record.",
            Self::QueryNotAllowed => "Put the client token in the Authorization header.",
            Self::NotFound => "Use one of the fixed routes.",
            Self::MethodNotAllowed => "Use the method declared for that fixed route.",
            Self::SqlUnsupported => {
                "Untrusted SQL stays unavailable until a kernel sandbox admits it."
            }
            Self::Internal => "Retry later. This response has no driver detail.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiError {
    code: ApiCode,
}

impl ApiError {
    pub const fn new(code: ApiCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> ApiCode {
        self.code
    }

    pub const fn status(self) -> u16 {
        self.code.status()
    }

    /// Bytes of `{"error":{code,message,hint,retryable}}`. `query_id` is absent.
    pub fn body(self) -> Vec<u8> {
        let mut error = Map::new();
        error.insert(
            "code".to_owned(),
            Value::String(self.code.as_str().to_owned()),
        );
        error.insert(
            "message".to_owned(),
            Value::String(self.code.message().to_owned()),
        );
        error.insert(
            "hint".to_owned(),
            Value::String(self.code.hint().to_owned()),
        );
        error.insert("retryable".to_owned(), Value::Bool(self.code.retryable()));
        let mut root = Map::new();
        root.insert("error".to_owned(), Value::Object(error));
        serde_json::to_vec(&Value::Object(root)).unwrap_or_else(|_| internal_fallback())
    }
}

fn internal_fallback() -> Vec<u8> {
    br#"{"error":{"code":"INTERNAL","message":"The request could not be completed.","hint":"Retry later. This response has no driver detail.","retryable":true}}"#
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_a_closed_envelope_without_query_id() {
        for code in ApiCode::ALL {
            let raw = ApiError::new(code).body();
            let text = String::from_utf8(raw).expect("utf8");
            assert!(!text.contains("ooct_"), "{text}");
            assert!(!text.contains("oost_"), "{text}");
            assert!(!text.contains("Bearer"), "{text}");
            assert!(!text.contains("query_id"), "{text}");
            let value: Value = serde_json::from_str(&text).expect("json");
            let error = value.get("error").expect("error");
            assert!(error.get("query_id").is_none());
            assert_eq!(
                error.get("code").and_then(Value::as_str),
                Some(code.as_str())
            );
            assert_eq!(
                error.get("message").and_then(Value::as_str),
                Some(code.message())
            );
            assert_eq!(error.get("hint").and_then(Value::as_str), Some(code.hint()));
            assert_eq!(
                error.get("retryable").and_then(Value::as_bool),
                Some(code.retryable())
            );
            assert_eq!(value.as_object().map(Map::len), Some(1));
            assert_eq!(error.as_object().map(Map::len), Some(4));
        }
    }

    #[test]
    fn statuses_match_the_contract() {
        assert_eq!(ApiCode::Unauthenticated.status(), 401);
        assert_eq!(ApiCode::Forbidden.status(), 403);
        assert_eq!(ApiCode::QueryNotAllowed.status(), 400);
        assert_eq!(ApiCode::NotFound.status(), 404);
        assert_eq!(ApiCode::MethodNotAllowed.status(), 405);
        assert_eq!(ApiCode::SqlUnsupported.status(), 501);
        assert_eq!(ApiCode::Internal.status(), 500);
        assert!(!ApiCode::SqlUnsupported.retryable());
        assert!(ApiCode::Internal.retryable());
        assert_eq!(LAYER_TIMEOUT_STATUS, 408);
        assert_eq!(LAYER_BODY_LIMIT_STATUS, 413);
        assert_eq!(RATE_LIMITED_STATUS, 429);
        assert_eq!(OVERLOADED_STATUS, 503);
    }
}
