//! Operator request and the public receipt. Missing production actions stay unsupported.

use serde::{Deserialize, Serialize};
use session_transition::{CancelTrigger, IdentityTuple};

pub const CONTRACT: &str = "session-release-v1";
pub const RUNTIME_LOGIN: &str = "oo_session_runtime";
pub const CANCEL_LOGIN: &str = "oo_session_cancel";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRequest {
    pub contract: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub runtime_user: String,
    pub runtime_password: String,
    pub cancel_user: String,
    pub cancel_password: String,
    pub ca_pem: String,
    pub project_id: String,
    pub session_id: String,
    pub identity: IdentityTuple,
    pub expected_revision: u64,
    pub expected_owner_generation: u64,
    pub now_ms: u64,
    pub cancel_trigger: CancelTrigger,
    pub event_prefix: String,
    pub owner_uuid: String,
    pub workspace_root: String,
    pub workspace_path: String,
    pub checkpoint_dir: String,
    pub transcript_path: String,
    pub outputs_path: String,
    pub backend_pid: i32,
    pub application_name: String,
    pub query_role: String,
    pub harness_argv: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Unsupported {
    pub action: &'static str,
    pub status: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StepView {
    pub id: &'static str,
    pub orchestrator: String,
    pub reducer: String,
    pub evidence_sha256: Option<String>,
    pub evidence_json: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ReleaseReceipt {
    pub contract: &'static str,
    pub disposition: &'static str,
    pub reason: String,
    pub phase: Option<String>,
    pub revision: Option<u64>,
    pub physical_release_certified: bool,
    pub database_used: bool,
    pub auth_provider_used: bool,
    pub runtime_used: bool,
    pub containment: &'static str,
    pub production_sandbox: bool,
    pub local_fixture: bool,
    pub complete_release_called: bool,
    pub cancel_return_was_sufficient: bool,
    pub slo_miss: bool,
    pub resumed: bool,
    pub deadline_basis: &'static str,
    pub t0_wall_epoch_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub backend_elapsed_ms: Option<u64>,
    pub harness_elapsed_ms: Option<u64>,
    pub harness_pid: Option<u32>,
    pub backend_basis: Option<String>,
    pub supported_adapters: Vec<&'static str>,
    pub unsupported: Vec<Unsupported>,
    pub steps: Vec<StepView>,
}

pub fn unsupported_matrix() -> Vec<Unsupported> {
    [
        "s5_snapshot",
        "s6_session_token",
        "s7_proxy_mapping",
        "s9_warmup_pool",
        "s10_permits",
        "s11_close",
        "authentication",
        "token_revoke",
        "mapping",
        "permits",
        "network",
        "runsc",
        "sandbox",
        "reaper_daemon",
    ]
    .into_iter()
    .map(|action| Unsupported { action, status: "unsupported" })
    .collect()
}

pub fn supported_adapters() -> Vec<&'static str> {
    vec![
        "fixture_pg_backend_cancel",
        "direct_child_harness",
        "fixture_bounded_collect",
        "fixture_directory_destroy",
    ]
}

pub fn blank(disposition: &'static str, reason: impl Into<String>) -> ReleaseReceipt {
    ReleaseReceipt {
        contract: CONTRACT,
        disposition,
        reason: reason.into(),
        phase: None,
        revision: None,
        physical_release_certified: false,
        database_used: false,
        auth_provider_used: false,
        runtime_used: false,
        containment: "local_fixture",
        production_sandbox: false,
        local_fixture: true,
        complete_release_called: false,
        cancel_return_was_sufficient: false,
        slo_miss: false,
        resumed: false,
        deadline_basis: "not_started",
        t0_wall_epoch_ms: None,
        elapsed_ms: None,
        backend_elapsed_ms: None,
        harness_elapsed_ms: None,
        harness_pid: None,
        backend_basis: None,
        supported_adapters: supported_adapters(),
        unsupported: unsupported_matrix(),
        steps: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{blank, unsupported_matrix};

    #[test]
    fn receipt_defaults_do_not_certify_or_close() {
        let receipt = blank("incomplete", "probe");
        assert!(!receipt.physical_release_certified);
        assert!(!receipt.production_sandbox);
        assert!(!receipt.complete_release_called);
        assert!(!receipt.cancel_return_was_sufficient);
        assert!(!receipt.auth_provider_used);
        assert!(!receipt.runtime_used);
        assert!(receipt.local_fixture);
        assert!(unsupported_matrix().iter().any(|item| item.action == "runsc" && item.status == "unsupported"));
        assert!(unsupported_matrix().iter().all(|item| item.status != "confirmed"));
    }
}
