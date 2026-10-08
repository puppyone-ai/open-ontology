//! Authenticated publish and rollback. The producer owns the pointer.
//!
//! See `CONTRACT.md`. This crate does not accept SP08 or emit a production certificate.

#![forbid(unsafe_code)]

mod config;
mod error;
mod http;
mod identity;
mod ids;
mod jsonutil;
mod pack;
mod sha256;
mod store;
mod tls;

pub use config::{load_config, Config};
pub use error::{error_body, ApiError, Fault, CONTRACT};
pub use http::{bound_addr, listen, serve, Runtime};
pub use pack::{permission_compatible, Lineage, LineageFields};
pub use sha256::sha256_hex;
pub use store::{install, migration_sha256};

pub const PRODUCER_COMMIT: &str = "9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3";
pub const IDENTITY_COMMIT: &str = "a6f92da8a0bf872f8a30efb116c2c7b14463505b";
pub const PRODUCER_CONTRACT: &str = "oo.release-coordinator/v1";
