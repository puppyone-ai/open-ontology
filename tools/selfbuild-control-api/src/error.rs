//! Frozen error codes. A failure is never HTTP 200.

use serde_json::{json, Value};

use crate::CONTRACT;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub message: &'static str,
    pub retryable: bool,
}

impl ApiError {
    pub const fn new(status: u16, code: &'static str, message: &'static str, retryable: bool) -> Self {
        Self {
            status,
            code,
            message,
            retryable,
        }
    }

    pub const fn malformed() -> Self {
        Self::new(400, "malformed", "the request is malformed", false)
    }

    pub const fn unauthenticated() -> Self {
        Self::new(401, "unauthenticated", "the session is not authenticated", false)
    }

    pub const fn forbidden() -> Self {
        Self::new(403, "forbidden", "the object is not in scope", false)
    }

    pub const fn origin() -> Self {
        Self::new(403, "origin.rejected", "the origin is not allowed", false)
    }

    pub const fn csrf() -> Self {
        Self::new(403, "csrf.rejected", "the session token was rejected", false)
    }

    pub const fn revision() -> Self {
        Self::new(409, "revision.conflict", "the revision does not match", false)
    }

    pub const fn idempotency() -> Self {
        Self::new(409, "idempotency.conflict", "the idempotency key was reused", false)
    }

    pub const fn not_found() -> Self {
        Self::new(404, "not_found", "the route is not found", false)
    }

    pub const fn method() -> Self {
        Self::new(405, "method.not_allowed", "the method is not allowed", false)
    }

    pub const fn not_ready() -> Self {
        Self::new(503, "dependency.not_ready", "a required dependency is not ready", true)
    }

    pub const fn membership() -> Self {
        Self::new(409, "membership.refused", "the object is not a member of this build", false)
    }

    pub const fn classification_incomplete() -> Self {
        Self::new(409, "classification.incomplete", "a visible column is not classified", false)
    }

    pub const fn pii_unknown() -> Self {
        Self::new(409, "pii.unknown", "unknown PII stays hidden", false)
    }

    pub const fn accept_all() -> Self {
        Self::new(409, "batch.accept_all", "accept-all is refused", false)
    }

    pub const fn commit_unknown() -> Self {
        Self::new(503, "commit.unknown", "the commit reply was lost", false)
    }

    pub const fn resume_uncertain() -> Self {
        Self::new(409, "resume.uncertain", "the model stage cannot be resumed", false)
    }

    pub fn body(&self, management_verified: bool) -> Value {
        json!({
            "contract": CONTRACT,
            "error": {
                "code": self.code,
                "message": self.message,
                "retryable": self.retryable
            },
            "management_identity": if management_verified { "verified" } else { "not_verified" },
            "published": false,
            "active": false,
            "data_grants": "none",
            "compiled_pack": "not_published"
        })
    }
}
