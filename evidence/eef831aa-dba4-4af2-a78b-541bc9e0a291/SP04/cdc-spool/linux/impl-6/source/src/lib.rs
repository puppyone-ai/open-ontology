//! Durable bounded spool for one normalized CDC transaction.
//! Not a target writer, offset store, or ACK.

#![forbid(unsafe_code)]

mod jsonline;
mod lsn;
mod model;
mod recover;
mod sha256;
mod spool;

pub use lsn::{encode_position, parse_commit_lsn};
pub use model::{
    canonical_preimage, chunk_name, event_hash, event_hash_hex, is_chunk_name, is_hex64,
    raw_value_bytes, validate_bind, validate_key, validate_payload, validate_table,
    validate_timestamp, validate_txn_id, Bind, CanonicalData, Cell, Fault, FaultCode, Limits, Op,
    Ordinal, Wire, CONTRACT, INDEX_ALLOWANCE, INPUT_CONTRACT, MAX_ACTIVE, MAX_CELLS,
    MAX_CHUNK_BYTES, MAX_CHUNK_EVENTS, MAX_COLLECTIONS, MAX_DISK_BYTES, MAX_EVENTS, MAX_FILES,
    MAX_LINE_BYTES, MAX_ORDINAL, PROGRESS_ALLOWANCE, PROVENANCE,
};
pub use recover::{
    recover, verify_replay, ChunkIter, ChunkRec, Committed, Event, Manifest, ManifestLimits,
    Pending, Recovery, RolledBack,
};
pub use spool::{Checkpoint, Outcome, OutcomeKind, ProgressView, SealSummary, Spool};
