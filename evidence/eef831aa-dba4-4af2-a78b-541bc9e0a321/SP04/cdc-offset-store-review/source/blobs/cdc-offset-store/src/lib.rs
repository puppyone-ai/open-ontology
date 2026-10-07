//! Control-database mirror of one existing target apply receipt.
//!
//! The target connection is a least-privileged reader. The control
//! connection records that row and may compare-and-swap one contiguous
//! watermark. Neither connection acknowledges a stream.

#![forbid(unsafe_code)]

mod model;
mod rules;
mod sha256;
mod sql;
mod store;
mod tls;

pub use model::{
    validate_mirror, CheckpointRef, DeclaredNext, DeclaredTxn, Endpoint, InstallOutput,
    InstallRequest, OrderBinding, Output, Request, CONTRACT, PROVENANCE,
};
pub use rules::{
    checkpoint, cmp_pos, pos_ok, receipt_document, receipt_hash, same_batch, Checkpoint, Receipt,
    Watermark, BOOTSTRAP_SHA256, FENCE_SHA256, HISTORICAL_V2_SHA256, PREFLIGHT_SHA256,
};
pub use sha256::{hex, sha256};
pub use store::{install, migration_sha256, mirror};
