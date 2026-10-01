#![forbid(unsafe_code)]

//! Dependency-direction gate for the Cargo workspace.
//!
//! Reads `cargo metadata` and reports forbidden edges. This is not a product API.

mod check;
mod policy;

pub use check::{
    Demonstration, Report, Violation, check, default_workspace_manifest,
    demonstrate_forbidden_gateway_edge, load_workspace_metadata,
};
