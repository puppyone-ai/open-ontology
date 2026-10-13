//! Local PostgreSQL mirror of a read-only PostgreSQL snapshot.
//! `customer_complete` stays false. This crate does not copy or test an enterprise database.

#![forbid(unsafe_code)]

mod codec;
mod copy;
mod discover;
mod error;
mod ident;
mod jsonutil;
mod load;
mod manifest;
mod pg;
mod plan;
mod read_local;
mod session;
mod spool;

pub const SOURCE_COPY_BINARY: &str = "local-pg-mirror";
pub const SOURCE_COPY_CONFIG_PROTOCOL: &str = "oo.pipeline.local-pg-mirror/v1";

pub use copy::{copy, CopyOutcome, CopyRequest, CopyStop};
pub use discover::{discover, DiscoverRequest};
pub use error::MirrorError;
pub use manifest::{sealed_import, Manifest, SealedImport};
pub use plan::{parse_config_text, seal_plan, store_plan, ColumnShape, InputConfig, Plan, TableShape};
pub use read_local::{read_local, ColumnView, LocalRead, LocalTable, ReadRequest};
