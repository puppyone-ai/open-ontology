//! External source-driver supervisor.
//!
//! The parent pins one child executable and one customer driver artifact,
//! then pulls framed Arrow IPC. It is not an Oracle client. Kernel network
//! control is [`KernelNetwork::Pending`] and is not a containment certificate.

#![forbid(unsafe_code)]

pub mod c5;
pub mod c5_arrow;
mod p2;
mod p2_contract;
mod p2_docker;
mod parent;
mod protocol;

pub use parent::{
    CallerRequest, DriverReceipt, Failure, FailureKind, GroupSignal, KernelNetwork, ParentConfig,
    ParentError, ParentPull, Teardown, VerifiedBatch, KERNEL_NETWORK,
};
pub use p2::{
    execute, write_batch_receipt, ExecuteResult, OracleLaunch, OracleReceipt, P2Supervisor,
};
pub use p2_contract::{
    accept_schema, expected_schema, gate_container_inspect, map_source_secret, P2_PORT,
    P2_PROTOCOL, P2_SCHEMA_ID, P2_SERVICE,
};
pub use p2_contract::fingerprint_hex as p2_fingerprint_hex;
pub use protocol::{
    completion_text, encode_ipc, file_digest, fingerprint_hex, lineage_text, parse_child,
    rows_batch, write_frame, ChildBody, ARTIFACT_NAME, DRIVER_MAX_BYTES, PROTOCOL,
    REQUEST_MAX_BYTES, SCHEMA_ID, TAG_COMPLETE, TAG_IPC, TAG_LINEAGE,
};
