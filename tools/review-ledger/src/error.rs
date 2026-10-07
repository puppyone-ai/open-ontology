//! Static failures. Messages do not echo names, hashes, credentials, or SQL text.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerError {
    pub code: String,
    pub message: &'static str,
}

impl LedgerError {
    pub fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code: code.to_owned(),
            message,
        }
    }
}

impl fmt::Display for LedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.code, self.message)
    }
}

impl std::error::Error for LedgerError {}

pub fn err(code: &'static str, message: &'static str) -> LedgerError {
    LedgerError::new(code, message)
}
