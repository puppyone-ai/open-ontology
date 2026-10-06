//! Static pack-bridge failures. Messages do not echo names, hashes, or prose.

use std::fmt;

pub const COMMAND_PROTOCOL: &str = "oo.builder.description-pack-command/v1";
pub const PACK_PROTOCOL: &str = "oo.builder.description-pack/v1";
pub const PACK_COMPILER: &str = "oo-builder/description-pack/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionPackError {
    pub code: String,
    pub message: &'static str,
}

impl DescriptionPackError {
    pub fn named(name: &'static str, message: &'static str) -> Self {
        Self {
            code: format!("{COMMAND_PROTOCOL}/{name}"),
            message,
        }
    }

    pub fn upstream(code: String, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl fmt::Display for DescriptionPackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.code, self.message)
    }
}

impl std::error::Error for DescriptionPackError {}
