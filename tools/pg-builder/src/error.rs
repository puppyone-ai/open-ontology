//! Static failures. Messages do not echo comments, names, tokens, or response text.

use std::fmt;

pub const PROTOCOL: &str = "oo.pg-builder/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgBuilderError {
    pub code: String,
    pub message: &'static str,
}

impl PgBuilderError {
    pub fn new(name: &'static str, message: &'static str) -> Self {
        Self {
            code: format!("{PROTOCOL}/{name}"),
            message,
        }
    }

    pub fn forwarded(code: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: code.into(),
            message,
        }
    }
}

impl fmt::Display for PgBuilderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.code, self.message)
    }
}

impl std::error::Error for PgBuilderError {}
