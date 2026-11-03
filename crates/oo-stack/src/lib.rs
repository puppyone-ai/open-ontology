//! oo.stack/v1. Diagnostics never contain configuration values.
//! YAML admission is blocked; JSON is accepted regardless of filename.

pub mod secret;
pub mod stack;

pub use stack::{Stack, parse, validate};

/// Structural schema; run `validate` for the semantic security checks as well.
pub const JSON_SCHEMA: &str = include_str!("../stack.schema.json");
