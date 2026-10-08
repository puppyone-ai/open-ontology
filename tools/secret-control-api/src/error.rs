//! Stable error codes. Messages are static and contain no secret material.

use std::fmt;

use serde_json::{json, Value};

pub const CONTRACT: &str = "oo.secret-control/v1";

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

    pub const fn forbidden() -> Self {
        Self::new("forbidden", "the project capability does not allow this", 403, 2)
    }

    pub const fn origin() -> Self {
        Self::new("origin.rejected", "origin is not the configured https origin", 403, 2)
    }

    pub const fn csrf() -> Self {
        Self::new("csrf.rejected", "csrf token was rejected", 403, 2)
    }

    pub const fn namespace() -> Self {
        Self::new("namespace.rejected", "the namespace does not match the reference", 403, 2)
    }

    pub const fn workload() -> Self {
        Self::new("workload.rejected", "the workload is not granted", 403, 2)
    }

    pub const fn aead() -> Self {
        Self::new("aead.rejected", "the ciphertext was rejected", 403, 2)
    }

    pub const fn not_found() -> Self {
        Self::new("not_found", "the secret was not found", 404, 2)
    }

    pub const fn method() -> Self {
        Self::new("method.not_allowed", "the method is not served", 405, 2)
    }

    pub const fn revision() -> Self {
        Self::new("revision.conflict", "the session revision does not match", 409, 2)
    }

    pub const fn version_conflict() -> Self {
        Self::new("version.conflict", "the version was not activated", 409, 2)
    }

    pub const fn retired() -> Self {
        Self::new("version.retired", "the requested version is not current", 409, 2)
    }

    pub const fn idempotency() -> Self {
        Self::new("idempotency.conflict", "the idempotency key was reused", 409, 2)
    }

    pub const fn revoked() -> Self {
        Self::new("revoked", "the secret is revoked", 409, 2)
    }

    pub const fn not_ready() -> Self {
        Self::new("dependency.not_ready", "a dependency is not ready", 503, 2)
    }

    pub const fn commit_unknown() -> Self {
        Self::new("commit.unknown", "the commit outcome is unknown", 503, 3)
    }

    pub const fn kms() -> Self {
        Self::new("kms.external", "customer kms is an external gate", 0, 2)
    }

    pub const fn drift() -> Self {
        Self::new("malformed", "install drift", 400, 1)
    }
}

pub fn error_body(fault: &Fault) -> Value {
    json!({
        "contract": CONTRACT,
        "error": {
            "code": fault.code,
            "message": fault.message,
            "retryable": fault.code == "dependency.not_ready" || fault.code == "commit.unknown"
        },
        "customer_kms": "external_gate",
        "mtls_ca": "not_issued"
    })
}

pub fn metadata_body(
    operation: &str,
    secret_ref: &str,
    version: i32,
    namespace: &str,
    project_id: &str,
    workload: &str,
    state: &str,
) -> Value {
    json!({
        "contract": CONTRACT,
        "operation": operation,
        "secret_ref": secret_ref,
        "version": version,
        "namespace": namespace,
        "project_id": project_id,
        "workload": workload,
        "state": state,
        "customer_kms": "external_gate",
        "mtls_ca": "not_issued"
    })
}

impl fmt::Display for Fault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for Fault {}
