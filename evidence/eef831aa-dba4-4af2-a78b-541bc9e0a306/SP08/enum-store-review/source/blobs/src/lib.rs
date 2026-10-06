#![forbid(unsafe_code)]

//! Single-project PostgreSQL materialization of one `enum-history/v1` report.
//! See `CONTRACT.md`. This crate does not certify finance, semantics, or an owner.

mod error;
mod publish;
mod role;
mod sha256;
mod sql;
mod store;
mod tls;

pub use error::StoreError;
pub use publish::{report_sha256, threshold_spelling};
pub use sha256::sha256_hex;
pub use store::{
    activate, install, migration_sha256, put_snapshot, read_view, ActivateReceipt, ActivateRequest, Endpoint,
    EnumColumn, InstallDisposition, InstallReceipt, InstallRequest, PutDisposition, PutReceipt, PutRequest,
    ReadReceipt, ReadRequest,
};
pub use tls::rustls_connector;

pub const CONTRACT: &str = "enum-store-v1";
