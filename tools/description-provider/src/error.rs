//! Static provider failures. The message does not include secrets or response bodies.

use std::fmt;

/// One local or compiler failure. `message` is a fixed sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    /// Stable code. Compiler failures keep the compiler code.
    pub code: String,
    /// Static text. It does not echo a secret, a catalog name, or a response body.
    pub message: &'static str,
}

impl ProviderError {
    pub(crate) fn new(name: &'static str, message: &'static str) -> Self {
        Self {
            code: format!("oo.builder.description-provider/v1/{name}"),
            message,
        }
    }

    pub(crate) fn from_compiler(code: String, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.code, self.message)
    }
}

impl std::error::Error for ProviderError {}
