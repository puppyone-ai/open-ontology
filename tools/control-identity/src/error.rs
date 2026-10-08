//! Stable error codes. Messages are static and contain no secret material.

use std::fmt;

use serde_json::{json, Value};

pub const CONTRACT: &str = "oo.control-identity/v1";

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

    pub const fn drift() -> Self {
        Self::new("malformed", "install drift", 400, 1)
    }

    pub const fn unauthenticated() -> Self {
        Self::new("unauthenticated", "authentication required", 401, 2)
    }

    pub const fn forbidden() -> Self {
        Self::new("forbidden", "the management role does not allow this", 403, 2)
    }

    pub const fn origin() -> Self {
        Self::new("origin.rejected", "origin is not the configured https origin", 403, 2)
    }

    pub const fn csrf() -> Self {
        Self::new("csrf.rejected", "csrf token was rejected", 403, 2)
    }

    pub const fn revision() -> Self {
        Self::new("revision.conflict", "the session revision does not match", 409, 2)
    }

    pub const fn state() -> Self {
        Self::new("state.rejected", "oidc state was rejected", 400, 2)
    }

    pub const fn nonce() -> Self {
        Self::new("nonce.rejected", "oidc nonce was rejected", 400, 2)
    }

    pub const fn pkce() -> Self {
        Self::new("pkce.rejected", "pkce verification failed", 400, 2)
    }

    pub const fn redirect() -> Self {
        Self::new("redirect.rejected", "the return path is not allowed", 400, 2)
    }

    pub const fn issuer() -> Self {
        Self::new("issuer.rejected", "issuer was rejected", 401, 2)
    }

    pub const fn audience() -> Self {
        Self::new("audience.rejected", "audience was rejected", 401, 2)
    }

    pub const fn expired() -> Self {
        Self::new("token.expired", "the token is expired", 401, 2)
    }

    pub const fn token() -> Self {
        Self::new("token.rejected", "the id token was rejected", 401, 2)
    }

    pub const fn kid_unknown() -> Self {
        Self::new("kid.unknown", "the signing key is not registered", 401, 2)
    }

    pub const fn kid_ambiguous() -> Self {
        Self::new("kid.ambiguous", "the signing key id is not unique", 401, 2)
    }

    pub const fn alg() -> Self {
        Self::new("alg.rejected", "the signature algorithm was rejected", 401, 2)
    }

    pub const fn jwks() -> Self {
        Self::new("jwks.rejected", "the jwks document was rejected", 401, 2)
    }

    pub const fn discovery() -> Self {
        Self::new("discovery.rejected", "oidc discovery was rejected", 401, 2)
    }

    pub const fn bootstrap_expired() -> Self {
        Self::new("bootstrap.expired", "the bootstrap token is expired", 403, 2)
    }

    pub const fn bootstrap_replay() -> Self {
        Self::new("bootstrap.replay", "the bootstrap token was already consumed", 409, 2)
    }

    pub const fn bootstrap_rejected() -> Self {
        Self::new("bootstrap.rejected", "the bootstrap token was rejected", 403, 2)
    }

    pub const fn bootstrap_subject() -> Self {
        Self::new("bootstrap.subject_mismatch", "the oidc subject does not match the frozen binding", 403, 2)
    }

    pub const fn bootstrap_unavailable() -> Self {
        Self::new("bootstrap.unavailable", "bootstrap is not available", 403, 2)
    }

    pub const fn commit_unknown() -> Self {
        Self::new("commit.unknown", "the commit outcome is unknown", 500, 3)
    }

    pub const fn unavailable() -> Self {
        Self::new("unavailable", "the identity store is unavailable", 503, 2)
    }

    pub const fn not_found() -> Self {
        Self::new("not_found", "the operation is not served", 404, 2)
    }

    pub const fn method() -> Self {
        Self::new("method.not_allowed", "the method is not served", 405, 2)
    }

    pub const fn binding_unavailable() -> Self {
        Self::new("binding.unavailable", "binding git is not configured", 403, 2)
    }

    pub const fn elevation() -> Self {
        Self::new("binding.elevation", "a subject cannot change their own management role", 403, 2)
    }

    pub const fn frozen() -> Self {
        Self::new("binding.frozen", "the bootstrap binding is frozen", 403, 2)
    }

    pub const fn idempotency() -> Self {
        Self::new("idempotency.conflict", "the idempotency key was reused for a different request", 409, 2)
    }

    pub const fn git_cas() -> Self {
        Self::new("git.cas", "the config git head did not match", 409, 2)
    }

    pub const fn git_rejected() -> Self {
        Self::new("git.rejected", "the config git commit was rejected", 403, 2)
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
