//! oo.stack/v1. Diagnostics never contain configuration values.
//! Bounded YAML and JSON are accepted regardless of filename.

pub mod credentials;
pub mod secret;
pub mod stack;

pub use stack::{Stack, parse, validate};

/// Structural schema; run `validate` for the semantic security checks as well.
pub const JSON_SCHEMA: &str = include_str!("../stack.schema.json");
