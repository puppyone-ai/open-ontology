//! Local-operator PostgreSQL 17 logical backup and fresh-database restore.
//!
//! This crate is not an all-state certificate. SP14-W04 stays open.

pub mod cut;
pub mod decide;
pub mod fixture;
pub mod gold;
pub mod json;
pub mod manifest;
pub mod ops;
pub mod pg;
pub mod protocol;
pub mod sha256;
pub mod snapshot;

pub const EXIT_TOOL: u8 = 1;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_REFUSED: u8 = 4;

#[derive(Debug)]
pub struct Fail {
    pub exit: u8,
    pub detail: String,
}

pub fn fail(exit: u8, detail: impl Into<String>) -> Fail {
    Fail { exit, detail: detail.into() }
}
