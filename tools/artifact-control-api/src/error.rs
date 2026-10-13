//! Static faults. Display text is the code and nothing else.

use serde_json::{json, Value};

pub const CONTRACT: &str = "oo.artifact-control/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub code: &'static str,
    pub http: u16,
    pub exit_code: u8,
}

impl Fault {
    const fn new(code: &'static str, http: u16, exit_code: u8) -> Self {
        Self { code, http, exit_code }
    }

    pub const fn malformed() -> Self { Self::new("malformed", 400, 1) }
    pub const fn self_asserted() -> Self { Self::new("self_asserted", 400, 1) }
    pub const fn path() -> Self { Self::new("path", 400, 1) }
    pub const fn budget() -> Self { Self::new("budget", 400, 1) }
    pub const fn unauthenticated() -> Self { Self::new("unauthenticated", 401, 2) }
    pub const fn forbidden() -> Self { Self::new("forbidden", 403, 2) }
    pub const fn scope_rejected() -> Self { Self::new("scope.rejected", 403, 2) }
    pub const fn origin_rejected() -> Self { Self::new("origin.rejected", 403, 2) }
    pub const fn csrf_rejected() -> Self { Self::new("csrf.rejected", 403, 2) }
    pub const fn workspace_disabled() -> Self { Self::new("workspace.disabled", 403, 2) }
    pub const fn aead() -> Self { Self::new("aead.rejected", 403, 2) }
    pub const fn revoked() -> Self { Self::new("revoked", 403, 2) }
    pub const fn expired() -> Self { Self::new("expired", 403, 2) }
    pub const fn not_found() -> Self { Self::new("not_found", 404, 2) }
    pub const fn method_not_allowed() -> Self { Self::new("method.not_allowed", 405, 2) }
    pub const fn session_extended() -> Self { Self::new("session.extended", 409, 2) }
    pub const fn idempotency_conflict() -> Self { Self::new("idempotency.conflict", 409, 2) }
    pub const fn drift() -> Self { Self::new("drift", 409, 2) }
    pub const fn not_ready() -> Self { Self::new("dependency.not_ready", 503, 3) }
    pub const fn commit_unknown() -> Self { Self::new("commit.unknown", 503, 3) }
}

impl std::fmt::Display for Fault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code)
    }
}

pub fn error_body(fault: &Fault) -> Value {
    json!({
        "contract": CONTRACT,
        "code": fault.code,
        "retryable": fault.code == "dependency.not_ready" || fault.code == "commit.unknown",
        "signed_url": null,
        "workspace": "disabled",
        "production_signed": false,
        "runsc": "not_claimed",
        "customer_integration": "not_claimed",
        "parent_complete": false
    })
}

pub fn card_flags() -> Value {
    json!({
        "signed_url": null,
        "workspace": "disabled",
        "production_signed": false,
        "runsc": "not_claimed",
        "customer_integration": "not_claimed",
        "parent_complete": false
    })
}
