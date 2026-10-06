//! Static review failures. Messages do not echo names or response text.

use std::fmt;

pub const COMMAND_PROTOCOL: &str = "oo.builder.description-review-command/v1";
pub const REVIEW_PROTOCOL: &str = "oo.builder.description-review/v1";
pub const REVIEW_COMPILER: &str = "oo-builder/description-review/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionReviewError {
    pub code: String,
    pub message: &'static str,
}

impl DescriptionReviewError {
    pub fn new(name: &'static str, message: &'static str) -> Self {
        Self {
            code: format!("{COMMAND_PROTOCOL}/{name}"),
            message,
        }
    }

    pub fn from_compiler(code: String, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl fmt::Display for DescriptionReviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.code, self.message)
    }
}

impl std::error::Error for DescriptionReviewError {}
