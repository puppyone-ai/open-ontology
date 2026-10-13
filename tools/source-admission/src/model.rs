//! Structured capability model. Closed enums. Unknown strings do not coerce.

use serde::{Deserialize, Serialize};

pub const REPORT_PROTOCOL: &str = "oo.source-admission.report/v1";
pub const CONFIG_PROTOCOL: &str = "oo.source-admission.config/v1";
pub const DECISION_PROTOCOL: &str = "oo.source-admission.decision/v1";
pub const WITNESS_PROTOCOL: &str = "oo.source-admission.capture-witness/v1";
pub const CAPTURE_RECEIPT_PROTOCOL: &str = "oo.source-admission.capture-receipt/v1";
pub const BINDING_REGISTRY_PROTOCOL: &str = "oo.source-admission.binding-registry/v1";
pub const CAPTURE_BINDING_PROTOCOL: &str = "oo.source-admission.capture-binding/v1";
pub const SEALED_CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";
pub const AUDIT_PROTOCOL: &str = "oo.source-admission.audit/v1";
pub const OBSERVE_PROTOCOL: &str = "oo.source-admission.observe/v1";
pub const PG_REPORT_PROTOCOL: &str = "oo.source.pgsql/v1";
pub const PG_BUDGET_PROTOCOL: &str = "oo.source.pgsql.budget/v1";
pub const PG_BUDGET_CONFIG_PROTOCOL: &str = "oo.source-admission.pg-budget/v1";
pub const CORE_CONTRACT: &str = "oo.config/v1";

pub const BUDGET_UNITS: &[&str] = &["Rows", "Bytes", "Credits", "Dbu", "ReadUnits", "WallClock"];
pub const COMPAT_MODES: &[&str] = &[
    "postgres-native",
    "mysql-native",
    "mariadb-native",
    "oracle",
    "sqlserver",
    "db2",
    "mysql-mode",
    "oracle-mode",
    "postgres-mode",
    "sqlserver-mode",
];
pub const PERMISSIONS: &[&str] = &[
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "TRUNCATE",
    "REFERENCES",
    "TRIGGER",
    "CREATE",
    "USAGE",
    "CONNECT",
    "TEMP",
    "EXECUTE",
    "REPLICATION",
    "BYPASSRLS",
    "SUPERUSER",
    "CREATEDB",
    "CREATEROLE",
];
pub const READ_ONLY_PERMISSIONS: &[&str] = &["SELECT", "USAGE", "CONNECT"];
pub const WRITE_CLASSES: &[&str] = &["R0", "R1", "R1w", "R2", "R3"];
pub const LEVELS: &[&str] = &[
    "candidate",
    "designed",
    "experimental",
    "verified",
    "certified",
    "excluded",
];
pub const CONSISTENCY: &[&str] = &["T", "S1", "S0", "B"];
pub const TOKEN_ORDERS: &[&str] = &["Total", "PerPartition", "Opaque", "None"];
pub const CURSOR_KINDS: &[&str] = &["Timestamp", "RowVersion", "Sequence", "PartitionModified"];
pub const FEED_KINDS: &[&str] = &[
    "ChangeTracking",
    "ChangesClause",
    "ChangeDataFeed",
    "SnapshotDiff",
];
pub const CDC_KINDS: &[&str] = &[
    "LogicalDecoding",
    "Binlog",
    "LogMiner",
    "SqlServerCdcTables",
    "ChangeStream",
    "ExternalBuffer",
];
pub const CAPABILITY_NAMES: &[&str] = &[
    "Catalog",
    "Logic",
    "QueryLog",
    "BatchFull",
    "BatchIncremental",
    "ChangeFeed",
    "Cdc",
    "DeleteNative",
    "DeleteReconcile",
    "SnapshotToken",
    "ReplicaRead",
    "Budget",
];
pub const CAPTURE_METHODS: &[&str] = &["C1", "C2", "C3", "C4", "C5"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Driver {
    pub kind: String,
    pub name: String,
    pub version: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub name: String,
    pub status: String,
    pub granted: bool,
    /// Engine-native scope. Absent on SQL, MongoDB, StarRocks, and Doris reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<GrantScope>,
}

/// Scope observed with one grant. Fields that do not belong to the engine deny.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantScope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    /// OpenSearch only. `true` means a live call on this index returned success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<bool>,
}

/// ClickHouse `readonly` profile, `max_execution_time`, and quota interval.
/// The `readonly` value is a setting, not a permission name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClickHouseNative {
    pub readonly: String,
    pub max_execution_time: String,
    pub quota: ClickHouseQuota,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClickHouseQuota {
    pub duration_seconds: u64,
    pub queries: u64,
    pub errors: u64,
    pub result_rows: u64,
    pub read_rows: u64,
    pub execution_time_seconds: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clickhouse: Option<ClickHouseNative>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredCapability {
    pub name: String,
    pub cursor: Option<String>,
    pub feed: Option<String>,
    pub cdc: Option<String>,
    pub token_order: Option<String>,
    pub budget_unit: Option<String>,
    pub level: String,
    pub write_class: String,
    pub consistency: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureMethod {
    pub method: String,
    pub write_class: String,
    pub consistency: String,
    pub token_order: String,
    pub retention_window: Option<String>,
    pub cursor: Option<String>,
    pub feed: Option<String>,
    pub cdc: Option<String>,
    /// Delete observation taken from the capturer. Absent on a legacy report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_detection: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostModel {
    pub unit: String,
    pub confidence: String,
    pub estimate: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct R1wApproval {
    pub approved: bool,
    pub adr: String,
    pub audit_ref: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetReport {
    pub protocol: String,
    pub target_id: String,
    pub source_id: String,
    pub secret_ref: String,
    pub driver: Driver,
    pub compat_mode: String,
    pub license_flags: Vec<String>,
    pub permissions: Vec<Permission>,
    pub capabilities: Vec<DeclaredCapability>,
    pub capture_methods: Vec<CaptureMethod>,
    pub cost_model: CostModel,
    pub r1w: Option<R1wApproval>,
    /// ClickHouse profile. Absent on every other engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeFacts>,
    /// Absolute path of an `oo.source-admission.capture-witness/v1` file.
    /// Absent on a legacy report. The path is not a hash and not an admission bit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_witness: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteOptions {
    pub signal_table: bool,
    pub heartbeat_write: bool,
    pub create_stream: bool,
    pub create_trigger: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebeziumOptions {
    pub signal_data_collection: Option<String>,
    pub heartbeat_action_query: Option<String>,
    pub snapshot_mode: String,
    pub create_stream: bool,
    pub create_trigger: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub unit: String,
    pub hard_limit: u64,
    pub confidence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub protocol: String,
    pub target_id: String,
    pub source_id: String,
    pub secret_ref: String,
    pub compat_mode: String,
    pub permissions: Vec<Permission>,
    pub budget: Budget,
    pub write_options: WriteOptions,
    pub debezium: DebeziumOptions,
    pub requested_write_class: String,
    pub r1w: Option<R1wApproval>,
    /// ClickHouse profile. Must equal the report when either side sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserveConfig {
    pub protocol: String,
    pub source_id: String,
    pub secret_ref: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub code: String,
    pub fact: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecklistItem {
    pub id: String,
    pub level: String,
    pub write_class: String,
    pub risk: String,
    pub rollback: String,
    pub verification: String,
    pub statement: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub target_id: String,
    pub source_id: String,
    /// Present on a legacy decision. A capture-witness decision leaves this empty,
    /// and the empty string is omitted so the public document has no `secret_ref` key.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret_ref: String,
    pub source_report_protocol: Option<String>,
    pub core_contract: Option<String>,
    pub source_report_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZeroWrite {
    pub proven: bool,
    pub basis: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub capabilities: Vec<DeclaredCapability>,
    pub capture_methods: Vec<CaptureMethod>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub protocol: String,
    pub admitted: bool,
    pub matrix_mutated: bool,
    pub blocks: Vec<Block>,
    pub model: Model,
    pub identity: Identity,
    pub dba_checklist: Vec<ChecklistItem>,
    pub zero_write: ZeroWrite,
}

pub fn known(set: &[&str], value: &str) -> bool {
    set.contains(&value)
}
