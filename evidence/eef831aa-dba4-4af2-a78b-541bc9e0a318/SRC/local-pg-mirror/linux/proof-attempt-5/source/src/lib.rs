//! Local PostgreSQL mirror of a read-only PostgreSQL snapshot.
//! `customer_complete` stays false. This crate does not copy or test an enterprise database.

#![forbid(unsafe_code)]

mod copy;
mod discover;
mod error;
mod jsonutil;
mod manifest;
mod pg;
mod plan;
mod read_local;

pub use copy::{copy, CopyOutcome, CopyRequest, CopyStop};
pub use discover::{discover, DiscoverRequest};
pub use error::MirrorError;
pub use manifest::Manifest;
pub use plan::{parse_config_text, seal_plan, store_plan, ColumnShape, InputConfig, Plan, TableShape};
pub use read_local::{read_local, LocalRead, LocalTable, ReadRequest};
