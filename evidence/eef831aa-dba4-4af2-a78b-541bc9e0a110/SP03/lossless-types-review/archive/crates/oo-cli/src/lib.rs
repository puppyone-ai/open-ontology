//! `oo query` over the frozen `oo-sdk` HTTP client.
//!
//! This slice sends one `POST /v1/query` and renders the typed response.
//! It does not open a database, cache rows, or search the environment for a
//! token. Sandbox proxy authentication is rejected before any credential read
//! because `oo_sdk::from_sandbox_env` is not implemented.

#![forbid(unsafe_code)]

/// Public `max_rows` ceiling. Flags may only select inside `1..=QUERY_MAX_ROWS`.
pub(crate) const QUERY_MAX_ROWS: u32 = 2_000;
/// Public server `timeout_ms` ceiling. The client wait is never raised above
/// the SDK production timeouts.
pub(crate) const QUERY_TIMEOUT_MAX_MS: u32 = 120_000;

mod args;
mod load;
mod outcome;
mod render;
mod run;

pub fn run() -> u8 {
    run::run()
}
