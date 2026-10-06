//! Durable bounded quarantine for normalized schema-change and TRUNCATE
//! control events.
//!
//! The wire contract is `CONTRACT.md`. This crate does not route Debezium,
//! apply a target schema, truncate a target, verify an upstream commit, or ACK.

#![forbid(unsafe_code)]

mod jsonline;
mod model;
mod pathutil;
mod sha256;
mod store;

pub use model::{
    identity_preimage, limits_from_env, parse_control, FaultCode, Limits, Reason, Status,
    CONTRACT, MAX_DISK_BYTES, MAX_EVENTS, MAX_LINE_BYTES, MAX_PAYLOAD_BYTES,
};
pub use sha256::{hex, sha256};
pub use store::{put, recover, replay};
