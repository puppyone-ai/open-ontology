//! Bounded assembler for an explicit normalized PostgreSQL CDC feed.
//!
//! The wire contract is `CONTRACT.md`. This crate does not normalize Debezium,
//! verify an upstream commit LSN, apply a target, or ACK.

#![forbid(unsafe_code)]

mod assemble;
mod jsonline;
mod lsn;
mod model;
mod sha256;

pub use assemble::{ApplyOut, Assembler, Effect};
pub use lsn::{encode_position, parse_commit_lsn, position_value};
pub use model::{
    canonical_preimage, event_hash, Attestations, CanonicalData, Cell, CompletedTxn,
    DecodedProgress, EmittedRow, Fault, FaultCode, Op, PendingStatus, PendingTxn, Progress,
    ReadProgress, SeenProgress, CONTRACT, MAX_ACTIVE, MAX_LINE_BYTES, MAX_PAYLOAD_BYTES,
    MAX_TXN_EVENTS,
};
