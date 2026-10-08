//! Secret write, rotate, revoke, and workload resolve.
//!
//! This crate is not a customer KMS and it does not issue an mTLS CA.

#![forbid(unsafe_code)]

mod aad;
mod config;
mod crypto;
mod error;
mod http;
mod identity;
mod ids;
mod jsonutil;
mod sql;
mod store;
mod tlsutil;
mod workload;

pub use aad::{canonical, Scope};
pub use config::{load_config, Config};
pub use crypto::{open, seal, sha256_hex, NonceSource, OsNonce, ScriptedNonce, CIPHER};
pub use error::{error_body, Fault, CONTRACT};
pub use http::{bound_addr, listen, serve, spawn_server, ServerHandle};
pub use identity::try_from_caller_json;
pub use sql::{migration_sha256, MIGRATION};
pub use store::{InstallReport, Store};
pub use tlsutil::{pg_tls, server_config, TlsListener};
