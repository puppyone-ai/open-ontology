//! Static errors. Detail values are tokens.

use serde_json::{json, Value};

pub const CONTRACT: &str = "oo.release-control-api/v1";

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

    pub const fn revision() -> Self {
        Self::new(409, "revision.conflict", "revision")
    }

    pub const fn idempotency() -> Self {
        Self::new(409, "idempotency.conflict", "idempotency")
    }

    pub const fn pack(detail: &'static str) -> Self {
        Self::new(409, "pack.rejected", detail)
    }

    pub const fn unreviewed() -> Self {
        Self::new(409, "pack.unreviewed", "review")
    }

    pub const fn compat() -> Self {
        Self::new(409, "compat.refused", "permission")
    }

    pub const fn conflict(detail: &'static str) -> Self {
        Self::new(409, "conflict", detail)
    }

    pub const fn not_ready(detail: &'static str) -> Self {
        Self::new(503, "dependency.not_ready", detail)
    }

    pub const fn not_found() -> Self {
        Self::new(404, "not_found", "route")
    }

    pub const fn method() -> Self {
        Self::new(405, "method.not_allowed", "method")
    }

    pub const fn commit_unknown() -> Self {
        Self::new(503, "commit.unknown", "commit")
    }

    pub fn body(self) -> Value {
        json!({
            "contract": CONTRACT,
            "code": self.code,
            "detail": self.detail,
            "production_ready": false,
            "full_sp08_accepted": false
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub exit_code: u8,
    pub code: &'static str,
}

impl Fault {
    pub const fn malformed() -> Self {
        Self { exit_code: 1, code: "malformed" }
    }

    pub const fn refused(code: &'static str) -> Self {
        Self { exit_code: 2, code }
    }

    pub const fn commit_unknown() -> Self {
        Self { exit_code: 3, code: "commit.unknown" }
    }

    pub fn from_api(error: ApiError) -> Self {
        let exit_code = if error.code == "commit.unknown" { 3 } else if error.status == 400 { 1 } else { 2 };
        Self { exit_code, code: error.code }
    }
}

pub fn error_body(fault: &Fault) -> Value {
    json!({
        "contract": CONTRACT,
        "code": fault.code,
        "production_ready": false,
        "full_sp08_accepted": false
    })
}
