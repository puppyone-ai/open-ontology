//! Failures of catalog-evidence parsing and draft compilation.
//!
//! Messages name the rule. They do not echo catalog comments, SQL, or secrets.

use std::fmt;

pub const EVIDENCE_PROTOCOL: &str = "oo.builder.catalog-evidence/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftError {
    pub code: String,
    pub path: String,
    pub message: &'static str,
}

impl DraftError {
    pub fn new(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{EVIDENCE_PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }

    pub fn from_full(code: String, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code,
            path: path.into(),
            message,
        }
    }
}

impl fmt::Display for DraftError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}: {}",
            self.code, self.path, self.message
        )
    }
}

impl std::error::Error for DraftError {}
