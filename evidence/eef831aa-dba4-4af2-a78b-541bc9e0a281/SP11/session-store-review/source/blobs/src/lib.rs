#![forbid(unsafe_code)]

//! PostgreSQL durability adapter for the fixed session-transition reducer.
//! See `CONTRACT.md`. This crate is not an authenticator.

mod error;
mod sha256;
mod sql;
mod store;
mod tls;

pub use error::StoreError;
pub use session_transition::{
    Event, FailureCode, IdentityTuple, Outcome, Phase, Reason, ReleaseRecord, SessionState,
    TransitionOutput,
    CONTRACT as REDUCER_CONTRACT,
};
pub use store::{
    bootstrap, get, install, migration_sha256, reconcile, transition, transition_with_fault, BootstrapDisposition,
    BootstrapReceipt, Disposition, Endpoint, GetReceipt, InstallDisposition, InstallReceipt, ReconcileDisposition,
    ReconcileReceipt, TransitionReceipt, TransitionRequest,
};
pub use tls::{rustls_connector, CommitFault};

pub const CONTRACT: &str = "session-store-v1";
pub const PROBE_CONTRACT: &str = "session-store-probe";
pub const STDIN_MAX_BYTES: usize = 1_048_576;
