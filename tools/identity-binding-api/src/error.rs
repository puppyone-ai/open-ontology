//! Stable error codes. Messages are static and contain no secret material.

use std::fmt;

use serde_json::{json, Value};

pub const CONTRACT: &str = "oo.identity-binding/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub code: &'static str,
    pub message: &'static str,
    pub http: u16,
    pub exit_code: u8,
}

impl Fault {
    const fn new(code: &'static str, message: &'static str, http: u16, exit_code: u8) -> Self {
        Self { code, message, http, exit_code }
    }

    pub const fn malformed() -> Self {
        Self::new("malformed", "the request is malformed", 400, 1)
    }

    pub const fn unauthenticated() -> Self {
        Self::new("unauthenticated", "authentication required", 401, 2)
    }

    pub const fn origin() -> Self {
        Self::new("origin.rejected", "origin is not the configured https origin", 403, 2)
    }

    pub const fn csrf() -> Self {
        Self::new("csrf.rejected", "csrf token was rejected", 403, 2)
    }

    pub const fn git_cas() -> Self {
        Self::new("git.cas", "the config git head did not match", 409, 2)
    }

    pub const fn git_rejected() -> Self {
        Self::new("git.rejected", "the config git commit was rejected", 403, 2)
    }

    pub const fn unavailable() -> Self {
        Self::new("unavailable", "the binding service is unavailable", 503, 2)
    }

    pub const fn not_found() -> Self {
        Self::new("not_found", "the operation is not served", 404, 2)
    }

    pub const fn method() -> Self {
        Self::new("method.not_allowed", "the method is not served", 405, 2)
    }
}

pub fn error_body(fault: &Fault) -> Value {
    json!({
        "contract": CONTRACT,
        "error": {
            "code": fault.code,
            "message": fault.message,
            "retryable": fault.code == "unavailable"
        },
        "authority": "management_session",
        "data_grants": "none",
        "owner_certification": "not_issued",
        "agent_session": "not_issued",
        "harness_session": "not_issued",
        "gateway_token": "not_issued",
        "customer_integration": "not_claimed"
    })
}

impl fmt::Display for Fault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for Fault {}
