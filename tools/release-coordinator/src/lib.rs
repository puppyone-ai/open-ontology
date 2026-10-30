//! Durable candidate, gate, and active-version coordinator.
//!
//! See `CONTRACT.md`. This crate does not accept SP08 or emit a production certificate.

#![forbid(unsafe_code)]

mod apply;
mod business_evidence;
mod business_positive;
mod business_native;
#[cfg(test)]
#[path = "../tests/observations/business_guard_pg.rs"]
mod business_guard_pg;
#[cfg(test)]
#[path = "../tests/observations/business_positive_pg.rs"]
mod business_positive_pg;
#[cfg(test)]
#[path = "../tests/observations/business_native_pg.rs"]
mod business_native_pg;
mod canon;
mod schema_check;
mod cli;
#[cfg(test)]
mod live;
#[cfg(test)]
mod w73;
mod decide;
mod error;
mod maintain;
mod native_publication;
mod native_library;
mod seal;
mod sha256;
mod sql;
mod store;
mod target;
mod tls;

pub use cli::{argv_malformed, dispatch, stdin_limit, CommandExit};
pub use business_evidence::{check_business_evidence, BusinessEvidenceContext, BUSINESS_EVIDENCE_SCHEMA};
pub use business_positive::{business_execution_capability, execute_business_checks, install_business_policy, review_business_checks, revoke_business_policy};
pub use decide::{
    production_admissible, summarize_gates, test_admissible, GateFact, Summary, GATE_IDS, PRODUCTION_EXTERNAL,
};
pub use error::StoreError;
pub use seal::{authority_statement, compile_seal, verify_native_artifact, Seal};
pub use sha256::sha256_hex;
pub use maintain::{maintain_activate, maintain_install, maintain_recover, maintenance_sha256, ControlConn, MaintainRequest};
pub use maintain::{maintain_data_only, DataOnlyParent};
pub use store::{
    activate, checkpoint, commit_activation, enter_applying, gate_record, install, migration_sha256, plan, recover,
    stage, status, CheckpointRequest, Endpoint, GateRequest, InstallRequest, PlanRequest, Receipt, RecoverRequest,
    StageRequest,
};
pub use target::{column_types, close_target, grant_target, install_target, observe_target, open_native_target, open_target, revoke_target, status_target, FenceRef, TargetConn};
pub use tls::rustls_connector;
pub use native_publication::{Publication, from_context as native_publication_context};

pub const CONTRACT: &str = "oo.release-coordinator/v1";
pub const PRODUCTION_READY_CERTIFICATE: bool = false;
pub const TEST_MODE_CANNOT_PRODUCE_PRODUCTION_CERTIFICATE: bool = true;
pub const FULL_SP08_ACCEPTED: bool = false;

pub use oo_core::config::DeploymentMode;

pub use target::{open_native_collection, prepare_library_target, library_state, primary_visibility_barrier, replica_visibility_barrier, install_visibility_reader, VisibilityRef};

pub use target::verify_library_visibility;

pub use target::validate_native_context;

pub use target::replica_barrier;

pub use oo_builder::RolePackCeilings;
pub use seal::{compile_seal_with_ceilings, verify_native_artifact_with_ceilings};
