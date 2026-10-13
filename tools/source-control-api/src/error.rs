//! Static API errors and process exits. Detail values are tokens.

use serde_json::{json, Value};

pub const CONTRACT: &str = "oo.source-control-api/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub detail: &'static str,
}

impl ApiError {
    const fn new(status: u16, code: &'static str, detail: &'static str) -> Self {
        Self { status, code, detail }
    }

    pub const fn unauthenticated() -> Self {
        Self::new(401, "unauthenticated", "session")
    }

    pub const fn forbidden(detail: &'static str) -> Self {
        Self::new(403, "forbidden", detail)
    }

    pub const fn origin() -> Self {
        Self::new(403, "origin.rejected", "origin")
    }

    pub const fn csrf() -> Self {
        Self::new(403, "csrf.rejected", "csrf")
    }

    pub const fn malformed(detail: &'static str) -> Self {
        Self::new(400, "malformed", detail)
    }

    pub const fn closed(detail: &'static str) -> Self {
        Self::new(400, "closed_input", detail)
    }

    pub const fn revision() -> Self {
        Self::new(409, "revision.conflict", "revision")
    }

    pub const fn idempotency() -> Self {
        Self::new(409, "idempotency.conflict", "idempotency")
    }

    pub const fn conflict(detail: &'static str) -> Self {
        Self::new(409, "conflict", detail)
    }

    pub const fn not_admitted() -> Self {
        Self::new(403, "source.not_admitted", "host")
    }

    pub const fn secret_unknown() -> Self {
        Self::new(403, "secret.unknown", "secret_ref")
    }

    pub const fn too_large() -> Self {
        Self::new(413, "payload.too_large", "bytes")
    }

    pub const fn not_ready(detail: &'static str) -> Self {
        Self::new(503, "dependency.not_ready", detail)
    }

    pub const fn not_found() -> Self {
        Self::new(404, "not_found", "route")
    }

    pub const fn missing(detail: &'static str) -> Self {
        Self::new(404, "not_found", detail)
    }

    pub const fn method() -> Self {
        Self::new(405, "method.not_allowed", "method")
    }

    pub fn body(self) -> Value {
        json!({
            "contract": CONTRACT,
            "code": self.code,
            "detail": self.detail,
            "build_review": "not_claimed",
            "release": "not_claimed",
            "customer_integration": "not_claimed"
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Malformed,
    Refused,
    Commit,
}

impl Fault {
    pub fn code(self) -> i32 {
        match self {
            Self::Malformed => 1,
            Self::Refused => 2,
            Self::Commit => 3,
        }
    }
}
