#![forbid(unsafe_code)]

//! Opaque Gateway token hashes and a five-second positive cache.
//! See `CONTRACT.md`. This crate does not authenticate an IdP or terminate mTLS.

mod cache;
mod canon;
mod error;
mod model;
mod sha256;
mod sql;
mod store;
mod tls;
mod token;
mod trusted_control;

#[cfg(test)]
mod live;

pub use cache::LookupCache;
pub use error::StoreError;
pub use model::{
    CurrentTuple, DeclaredTransport, Endpoint, FixedWall, IssueGrant, RevokeScope, SystemWall, TokenKind, WaitGate,
    WallClock, WallSource, CLIENT_DEFAULT_MS, CLIENT_MAX_MS, SESSION_ABSOLUTE_MS, SESSION_IDLE_MS,
};
pub use store::{
    lookup, lookup_for_test, migration_sha256, ControlFlags, InstallDisposition, InstallReceipt, IssueReceipt,
    LookupReceipt, LookupRequest, RevokeDisposition, RevokeReceipt,
};
pub use tls::rustls_connector;
pub use token::{draw_32, scrub, token_hash, PlainToken};
pub use trusted_control::TrustedControlAdapter;

pub const CONTRACT: &str = "gateway-token-store-v1";
