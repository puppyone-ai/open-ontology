#![forbid(unsafe_code)]

//! Single-project PostgreSQL calendar snapshots and `oo_dict.calendar`.
//! See `CONTRACT.md`. This crate is not an authenticator and it does not
//! approve a financial close.

mod error;
mod publish;
mod role;
mod sql;
mod store;
mod tls;

pub use error::StoreError;
pub use store::{
    activate, install, migration_sha256, put_snapshot, read_view, ActivateReceipt, ActivateRequest, AnchorColumn,
    Endpoint, InstallDisposition, InstallReceipt, InstallRequest, PutDisposition, PutReceipt, PutRequest, ReadReceipt,
    ReadRequest,
};
pub use tls::rustls_connector;

pub const CONTRACT: &str = "calendar-store-v1";
