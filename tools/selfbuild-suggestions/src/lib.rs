#![forbid(unsafe_code)]

mod gateway;
mod json;
mod produce;

pub use gateway::ProcessRunner;
pub use json::{DOCUMENT_MAX_BYTES, candidate_id, canonical_sha256, parse_document};
pub use produce::{Error, Gateway, SelectRunner, Spawned, produce, produce_with};
