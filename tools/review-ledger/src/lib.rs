//! Durable private review queue. A stored decision is not a curated write or a publication.
//!
//! Caller capability comes from the control file and the credential file.
//! Request JSON cannot grant a role, claim authentication, or publish.

#![forbid(unsafe_code)]

mod canonical;
mod compiler;
mod control;
mod error;
mod fields;
mod model;
mod sha256;
mod sql;
mod store;
mod tls;

use serde::de::DeserializeOwned;
use serde_json::Value;

pub use canonical::{canonical_json, canonical_sha, sha256_hex};
pub use control::{
    civil_date_ok, ident_ok, in_scope, object_id_ok, parse_control, Control, Principal,
    CONTROL_PROTOCOL,
};
pub use error::{err, LedgerError};
pub use fields::{prepare, Prepared};
pub use model::{
    reject_request_keys, BatchItem, BatchRequest, DecideRequest, Endpoint, EnqueueRequest,
    HistoryRequest, InstallRequest, ItemOut, Output, ReceiptRequest, RESULT_PROTOCOL,
};
pub use store::{
    batch, decide, enqueue, history, install, migration_sha256, read_request_file, receipt,
};

pub fn screen_request(bytes: &[u8]) -> Result<Value, LedgerError> {
    if bytes.len() > 262_144 {
        return Err(err("io.budget", "the request exceeds the byte budget"));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| err("schema.invalid", "the request is not JSON"))?;
    reject_request_keys(&value)?;
    Ok(value)
}

pub fn parse_request<T: DeserializeOwned>(value: Value) -> Result<T, LedgerError> {
    serde_json::from_value(value).map_err(|_| err("schema.invalid", "the request is not this command"))
}
