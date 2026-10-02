//! Evidence closure for the five containment-supervisor registry crates.
//!
//! The governance checker can accept a locator and still skip detail while
//! `audited` is false. This crate reads the retained scan, the lock, and the
//! license bytes.

#![forbid(unsafe_code)]

mod check;
mod fail;
mod json;
mod sha;
mod timeutil;

pub use check::run;
pub use fail::{Fail, USAGE};
pub use timeutil::{completed_days, parse_rfc3339};

#[derive(Debug)]
pub struct Paths {
    pub inventory: std::path::PathBuf,
    pub records: std::path::PathBuf,
    pub licenses: std::path::PathBuf,
    pub retained_licenses: std::path::PathBuf,
    pub helper_lock: std::path::PathBuf,
    pub helper_manifest: std::path::PathBuf,
    pub deny_config: std::path::PathBuf,
    pub metadata: std::path::PathBuf,
    pub age: std::path::PathBuf,
    pub audit_json: std::path::PathBuf,
    pub deny_advisories: std::path::PathBuf,
    pub advisory_rev: std::path::PathBuf,
    pub version_meta: std::path::PathBuf,
    pub repo_root: std::path::PathBuf,
    pub as_of: String,
}
