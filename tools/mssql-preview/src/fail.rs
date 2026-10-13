use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fail {
    pub code: &'static str,
    pub detail: String,
}

impl Fail {
    pub fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for Fail {}

pub fn exit_class(code: &str) -> u8 {
    match code {
        "native_engine_unrecognized" => 4,
        "gates_pending" => 5,
        "deadline_exceeded" | "connect_failed" | "login_mismatch" | "permission_not_readonly"
        | "select_not_granted" | "permission_unobserved" | "catalog_cap" => 3,
        _ => 2,
    }
}
