//! Local product-stack decisions. Safe Rust only. No service is mocked here.

#![forbid(unsafe_code)]

pub mod decide;
pub mod json;
pub mod sha256;

pub use decide::{health, identity_commit, journey, preflight, sbom, Decision, Malformed};
pub use json::{parse, Val};
pub use sha256::{sha256, sha256_hex};
