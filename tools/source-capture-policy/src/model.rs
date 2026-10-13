//! Closed request and decision shapes. Unknown JSON fields are refused.
//! Fault strings are static. Request values are not copied into fault text.

use serde::{Deserialize, Serialize};

pub const CONTRACT: &str = "source-capture-policy/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Production,
    NonProduction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    Auto,
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedConsistency {
    Transactional,
    Snapshot,
    PerTable,
    BestEffort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureMethod {
    C1,
    C2,
    C3,
    C4,
    C5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    #[serde(rename = "T")]
    T,
    #[serde(rename = "S1")]
    S1,
    #[serde(rename = "S0")]
    S0,
    #[serde(rename = "B")]
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenOrder {
    Total,
    PerPartition,
    Opaque,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteClass {
    R0,
    R1,
    R1w,
    R2,
    R3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportGrade {
    Candidate,
    Designed,
    Experimental,
    Verified,
    Certified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteDetection {
    Native,
    PartitionReplace,
    Reconcile,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorSafety {
    CommitOrdered,
    StatementTime,
    AppMaintained,
    Coarse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionInput {
    pub window_seconds: u64,
    pub lag_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AsOfInput {
    pub snapshot_token: String,
    pub read_seconds: u64,
    pub token_retention_seconds: u64,
    pub same_token: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorInput {
    pub safety: CursorSafety,
    pub overlap_seconds: u64,
    pub max_transaction_age_seconds: Option<u64>,
    pub conservative_max_transaction_age_seconds: Option<u64>,
    pub upper_bound_is_commit_safe: bool,
    pub reconcile_configured: bool,
    pub equal_cursor_checkpoint: bool,
    pub admin_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodInput {
    pub method: CaptureMethod,
    pub support_grade: SupportGrade,
    pub write_class: WriteClass,
    pub r1w_adr: bool,
    pub r1w_audit: bool,
    pub token_order: TokenOrder,
    pub delete_detection: DeleteDetection,
    pub consistency_ceiling: Grade,
    pub transaction_boundary: bool,
    pub token_available: bool,
    pub partition_metadata_trusted: bool,
    pub full_partition_replay: bool,
    pub family: String,
    pub position_codec: String,
    pub position_lineage: String,
    pub source_epoch: i32,
    pub applied_pos: Option<String>,
    pub window_low: Option<String>,
    pub window_high: Option<String>,
    pub same_source_uuid: bool,
    pub same_instance: bool,
    pub failover_observed: bool,
    pub failover_position_retained: Option<bool>,
    pub soft_delete_only: bool,
    pub precision_truncated: bool,
    pub original_column_retained_as_key: bool,
    pub used_as_primary_key: bool,
    pub reconcile_period_seconds: Option<u64>,
    pub delete_visibility_sla_seconds: Option<u64>,
    pub cursor: Option<CursorInput>,
    pub retention: Option<RetentionInput>,
    pub as_of: Option<AsOfInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableInput {
    pub name: String,
    pub generation: String,
    pub stable_key: bool,
    pub declared_unique_key: bool,
    pub row_estimate: u64,
    pub source_changing_without_token: bool,
    pub budget_exhausted: bool,
    pub explicit_method: Option<CaptureMethod>,
    pub methods: Vec<MethodInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinInput {
    pub tables: Vec<String>,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetReceipt {
    pub source: String,
    pub table_generation: String,
    pub txn_or_batch_id: String,
    pub config_version: String,
    pub config_digest: String,
    pub type_policy_version: String,
    pub type_digest: String,
    pub hook_digest: String,
    pub schema_digest: String,
    pub payload_digest: String,
    pub protocol_version: String,
    pub position_codec: String,
    pub position_lineage: String,
    pub source_epoch: i32,
    pub applied_pos: String,
    pub partial_txn: bool,
    pub cross_table_consistent: bool,
    pub txn_complete: bool,
    pub segment_index: Option<i32>,
    pub segment_count: Option<i32>,
    pub segment_bitmap: Vec<u8>,
    pub fence_shard: String,
    pub fence_generation: i64,
    pub target_commit_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetWatermark {
    pub position_codec: String,
    pub position_lineage: String,
    pub source_epoch: i32,
    pub applied_pos: String,
    pub applied_txn_id: String,
    pub declared_next_txn: String,
    pub declared_next_pos: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetCheckpointRef {
    pub position_codec: String,
    pub position_lineage: String,
    pub source_epoch: i32,
    pub applied_pos: String,
    pub txn_or_batch_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetDeclared {
    pub txn_or_batch_id: String,
    pub applied_pos: String,
    pub required_table_generations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetNext {
    pub txn_or_batch_id: String,
    pub applied_pos: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetBinding {
    pub provenance: String,
    pub expected_previous: Option<OffsetCheckpointRef>,
    pub declared_transaction: OffsetDeclared,
    pub declared_next: OffsetNext,
    pub source_gap: bool,
    pub epoch_barrier: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetInput {
    pub receipts: Vec<OffsetReceipt>,
    pub selected_generation: String,
    pub watermark: Option<OffsetWatermark>,
    pub binding: OffsetBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub contract: String,
    pub profile: Profile,
    pub sync: SyncMode,
    pub requested_consistency: Option<RequestedConsistency>,
    pub source_id: String,
    pub volume_limit_rows: u64,
    pub repair_requested: bool,
    pub spool_dir: Option<String>,
    pub offset: Option<OffsetInput>,
    pub tables: Vec<TableInput>,
    pub joins: Vec<JoinInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Freshness {
    pub consistency_grade: Option<Grade>,
    pub capture_method: Option<CaptureMethod>,
    pub lag_seconds: Option<u64>,
    pub position: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableDecision {
    pub name: String,
    pub generation: String,
    pub next_generation: Option<String>,
    pub fault: Option<&'static str>,
    pub capture_method: Option<CaptureMethod>,
    pub consistency_grade: Option<Grade>,
    pub delete_detection: Option<DeleteDetection>,
    pub token_order: Option<TokenOrder>,
    pub statuses: Vec<&'static str>,
    pub operator_action: &'static str,
    pub initial_load: Option<&'static str>,
    pub advance_capture_position: bool,
    pub continue_from_latest: bool,
    pub resume_opaque: bool,
    pub repair_allowed: bool,
    pub consistency_claim_allowed: bool,
    pub transactional_allowed: bool,
    pub publish_new_generation: bool,
    pub old_generation_readable: bool,
    pub precision_loss: bool,
    pub token: Option<String>,
    pub freshness: Freshness,
    pub reasons: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JoinDecision {
    pub tables: Vec<String>,
    pub warning: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpoolReport {
    pub present: bool,
    pub state: &'static str,
    pub txn_id: Option<String>,
    pub commit_lsn: Option<String>,
    pub commit_position: Option<String>,
    pub ack_allowed: bool,
    pub target_applied: bool,
    pub cross_table_consistent: bool,
    pub applied: Option<bool>,
    pub authorizes_ack: bool,
    pub authorizes_publish: bool,
    pub fault: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OffsetReport {
    pub present: bool,
    pub receipt_closed: bool,
    pub receipt_hash: Option<String>,
    pub checkpoint: &'static str,
    pub watermark_hold: Option<&'static str>,
    pub checkpoint_advanced: bool,
    pub source_applied: bool,
    pub fence_proof: bool,
    pub ack_allowed: bool,
    pub durable_control_mirror: bool,
    pub live_mirror: &'static str,
    pub fault: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub contract: &'static str,
    pub kind: &'static str,
    pub fault: Option<&'static str>,
    pub downgraded: bool,
    pub configuration_accepted: bool,
    pub requested_consistency_result: &'static str,
    pub source_grade: Option<Grade>,
    pub transactional_allowed: bool,
    pub same_instant_claim_allowed: bool,
    pub repair_requested: bool,
    pub repair_allowed: bool,
    pub consistency_claim_allowed: bool,
    pub advance_capture_position: bool,
    pub continue_from_latest: bool,
    pub publish_new_generation: bool,
    pub operator_action: &'static str,
    pub loader_integration: &'static str,
    pub scheduler_integration: &'static str,
    pub live_offset_mirror: &'static str,
    pub tables: Vec<TableDecision>,
    pub joins: Vec<JoinDecision>,
    pub spool: SpoolReport,
    pub offset: OffsetReport,
    pub quality_statuses: Vec<&'static str>,
}

pub fn plan_bytes(bytes: &[u8]) -> Result<Decision, &'static str> {
    if bytes.len() > 1024 * 1024 {
        return Err("input");
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "input")?;
    refuse_secret_keys(&value)?;
    let request: PlanRequest = serde_json::from_value(value).map_err(|_| "input")?;
    crate::plan::plan_request(&request)
}

fn refuse_secret_keys(value: &serde_json::Value) -> Result<(), &'static str> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if secret_key(key) {
                    return Err("secret_field");
                }
                refuse_secret_keys(child)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for child in items {
                refuse_secret_keys(child)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    matches!(
        key.as_str(),
        "password"
            | "passwd"
            | "secret"
            | "credential"
            | "credentials"
            | "api_key"
            | "apikey"
            | "token"
            | "access_token"
            | "refresh_token"
    )
}

pub fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn qualified_table(value: &str) -> bool {
    let Some((schema, table)) = value.split_once('.') else {
        return false;
    };
    !table.contains('.')
        && ident_ok(schema)
        && !table.is_empty()
        && table.len() <= 128
        && table
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && (table.as_bytes()[0].is_ascii_alphabetic() || table.as_bytes()[0] == b'_')
}

pub fn rank(grade: Grade) -> u8 {
    match grade {
        Grade::T => 3,
        Grade::S1 => 2,
        Grade::S0 => 1,
        Grade::B => 0,
    }
}

pub fn next_generation(current: &str) -> Option<String> {
    let (head, tail) = current.rsplit_once("_g")?;
    if !ident_ok(head) || tail.is_empty() || !tail.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if tail.len() > 1 && tail.as_bytes()[0] == b'0' {
        return None;
    }
    let number: u64 = tail.parse().ok()?;
    let next = number.checked_add(1)?;
    let proposed = format!("{head}_g{next}");
    if ident_ok(&proposed) {
        Some(proposed)
    } else {
        None
    }
}
