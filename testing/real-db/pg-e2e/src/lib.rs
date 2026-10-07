//! Public fixture driver. It writes source rows, binds review fingerprints, and
//! checks CLI outputs. It does not insert target business rows.

#![forbid(unsafe_code)]

pub mod gold;
pub mod json;
pub mod run;
pub mod sha;

pub type Fail = String;

pub fn fail(detail: impl Into<String>) -> Fail {
    detail.into()
}
