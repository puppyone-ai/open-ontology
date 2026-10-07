//! Durable candidate, gate, and active-version coordinator.
//!
//! See `CONTRACT.md`. This crate does not accept SP08 or emit a production certificate.

#![forbid(unsafe_code)]

mod canon;
mod cli;
#[cfg(test)]
mod live;
mod decide;
mod error;
mod seal;
mod sha256;
mod sql;
mod store;
mod tls;

pub use cli::{argv_malformed, dispatch, CommandExit};
pub use decide::{
    production_admissible, summarize_gates, test_admissible, GateFact, Summary, GATE_IDS, PRODUCTION_EXTERNAL,
};
pub use error::StoreError;
pub use seal::{authority_statement, compile_seal, Seal};
pub use sha256::sha256_hex;
pub use store::{
    activate, checkpoint, commit_activation, enter_applying, gate_record, install, migration_sha256, plan, recover,
    stage, status, CheckpointRequest, Endpoint, GateRequest, InstallRequest, PlanRequest, Receipt, RecoverRequest,
    StageRequest,
};
pub use tls::rustls_connector;

pub const CONTRACT: &str = "oo.release-coordinator/v1";
pub const PRODUCTION_READY_CERTIFICATE: bool = false;
pub const TEST_MODE_CANNOT_PRODUCE_PRODUCTION_CERTIFICATE: bool = true;
pub const FULL_SP08_ACCEPTED: bool = false;
