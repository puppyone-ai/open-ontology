//! Configuration documents from docs/12.
//!
//! Field names, defaults, and closed sets follow that reference. `schema_version`
//! is required even though the narrative samples omit it. `tls: disable` is not
//! a variant: production NoTls is rejected. `consistency_acknowledged` is the
//! pure stand-in for the spec's administrator confirmation, which the field
//! table does not name. Agent placeholders such as `<model-id>` are not valid
//! model identifiers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::decimal::FlexDecimal;
use crate::duration::{ByteSize, Duration};
use crate::pattern_map::PatternMap;
use crate::text::{
    ByteBoundedText, CharsetName, CharsetTarget, ClaimName, ColumnPath, ConnectorName,
    DatabaseName, DayWindow, Delimiter, DeploymentName, FilesSchemaName, GlobPattern, HostName,
    HttpsUrl, Label, LocaleCode, ModelId, PluginName, QualifiedTable, RelativePath, RowFilterExpr,
    SecretRef, SourceName, TableName, TimezoneName, VersionToken,
};

use super::de;
use super::version::ConfigVersion;

fn duration(text: &str) -> Duration {
    Duration::parse(text).expect("built-in duration")
}

fn flex(text: &str) -> FlexDecimal {
    FlexDecimal(crate::decimal::ExactDecimal::parse_sql(text).expect("built-in decimal"))
}

fn default_tls() -> TlsMode {
    TlsMode::VerifyFull
}
fn default_read_from() -> ReadFrom {
    ReadFrom::Replica
}
fn default_sync() -> SyncMode {
    SyncMode::Auto
}
fn default_snapshot() -> SnapshotMode {
    SnapshotMode::OoChunked
}
fn default_consistency() -> ConsistencyMode {
    ConsistencyMode::Transactional
}
fn default_txn_events() -> u32 {
    200_000
}
fn default_snapshot_seconds() -> Duration {
    duration("300s")
}
fn default_include() -> Vec<GlobPattern> {
    vec![GlobPattern::new("*").expect("star glob")]
}
fn default_connections() -> u32 {
    4
}
fn default_rows_per_sec() -> u32 {
    50_000
}
fn default_cursor() -> TableName {
    TableName::new("updated_at").expect("updated_at")
}
fn default_every() -> Duration {
    duration("1h")
}
fn default_overlap() -> Duration {
    duration("5m")
}
fn default_sample() -> SampleValues {
    SampleValues::Masked
}
fn default_zero_date() -> ZeroDatePolicy {
    ZeroDatePolicy::Null
}
fn default_tinyint1() -> Tinyint1Policy {
    Tinyint1Policy::Smallint
}
fn default_strip_nul() -> bool {
    true
}
fn default_reconcile() -> Duration {
    duration("3d")
}
fn default_reject_count() -> u32 {
    1000
}
fn default_reject_ratio() -> FlexDecimal {
    flex("0.001")
}
fn default_audit_days() -> u32 {
    180
}
fn default_output_days() -> u32 {
    30
}
fn default_max_rows() -> u32 {
    200
}
fn default_max_rows_cap() -> u32 {
    2000
}
fn default_export_rows() -> u32 {
    500_000
}
fn default_export_bytes() -> ByteSize {
    ByteSize::parse("1GB").expect("1GB")
}
fn default_concurrency() -> u32 {
    2
}
fn default_statement_timeout() -> Duration {
    duration("30s")
}
fn default_export_timeout() -> Duration {
    duration("120s")
}
fn default_session_tokens() -> u32 {
    2_000_000
}
fn default_tool_calls() -> u32 {
    60
}
fn default_turn_seconds() -> u32 {
    600
}
fn default_idle() -> Duration {
    duration("30m")
}
fn default_lifetime() -> Duration {
    duration("8h")
}
fn default_cpu() -> u32 {
    2
}
fn default_memory() -> ByteSize {
    ByteSize::parse("4Gi").expect("4Gi")
}
fn default_disk() -> ByteSize {
    ByteSize::parse("5Gi").expect("5Gi")
}
fn default_pids() -> u32 {
    512
}
fn default_min_idle() -> u32 {
    2
}
fn default_max_pool() -> u32 {
    20
}
fn default_pool_idle() -> Duration {
    duration("1h")
}
fn default_group_policy() -> GroupPolicy {
    GroupPolicy::DmDetails
}
fn default_runs() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SecretBackend {
    Builtin,
    Openbao,
    Vault,
    AwsKms,
    GcpKms,
    AzureKv,
    Infisical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ObjectBackend {
    Local,
    S3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BufferBackend {
    ValkeyStreams,
    Kafka,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    Require,
    VerifyCa,
    VerifyFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadFrom {
    Primary,
    Replica,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    Auto,
    Cdc,
    Incremental,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    #[serde(rename = "oo_chunked")]
    OoChunked,
    #[serde(rename = "debezium_incremental")]
    DebeziumIncremental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConsistencyMode {
    Transactional,
    PerTable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SampleValues {
    None,
    Masked,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ZeroDatePolicy {
    Null,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tinyint1Policy {
    Smallint,
    Boolean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileMode {
    Replace,
    Append,
    Upsert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
pub enum DateSystem {
    #[serde(rename = "1900")]
    Excel1900,
    #[serde(rename = "1904")]
    Excel1904,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookType {
    OnBatch,
    AfterLoad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupPolicy {
    DmDetails,
    FullInGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PiiLevel {
    None,
    Personal,
    Sensitive,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub name: DeploymentName,
    pub timezone: TimezoneName,
    pub locale: LocaleCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Secrets {
    pub backend: SecretBackend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct S3Store {
    pub endpoint: HttpsUrl,
    pub bucket: Label,
    pub region: Label,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectStore {
    pub backend: ObjectBackend,
    #[serde(default)]
    pub s3: Option<S3Store>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Buffer {
    pub backend: BufferBackend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DbEndpoint {
    pub host: HostName,
    #[serde(deserialize_with = "de::port")]
    #[schemars(range(min = 1, max = 65535))]
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentDb {
    pub primary: DbEndpoint,
    #[serde(default)]
    pub replicas: Vec<DbEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RejectsAlert {
    #[serde(
        default = "default_reject_count",
        deserialize_with = "de::reject_count"
    )]
    #[schemars(range(min = 0, max = 1_000_000_000))]
    pub per_table_daily: u32,
    #[serde(default = "default_reject_ratio")]
    pub ratio: FlexDecimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    #[serde(default = "default_reconcile")]
    pub reconcile_max_age: Duration,
    #[serde(default)]
    pub rejects_alert: RejectsAlert,
}

impl Default for RejectsAlert {
    fn default() -> Self {
        Self {
            per_table_daily: default_reject_count(),
            ratio: default_reject_ratio(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Retention {
    #[serde(
        default = "default_audit_days",
        deserialize_with = "de::retention_days"
    )]
    #[schemars(range(min = 1, max = 36500))]
    pub audit_days: u32,
    #[serde(
        default = "default_audit_days",
        deserialize_with = "de::retention_days"
    )]
    #[schemars(range(min = 1, max = 36500))]
    pub transcripts_days: u32,
    #[serde(
        default = "default_output_days",
        deserialize_with = "de::retention_days"
    )]
    #[schemars(range(min = 1, max = 36500))]
    pub outputs_days: u32,
    #[serde(
        default = "default_audit_days",
        deserialize_with = "de::retention_days"
    )]
    #[schemars(range(min = 1, max = 36500))]
    pub uploads_days: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LlmForBuilder {
    pub provider: PluginName,
    pub base_url: HttpsUrl,
    pub model: ModelId,
    pub credential: SecretRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OoConfig {
    pub schema_version: ConfigVersion,
    pub deployment: Deployment,
    pub secrets: Secrets,
    pub object_store: ObjectStore,
    pub buffer: Buffer,
    pub agentdb: AgentDb,
    #[serde(default)]
    pub quality: Quality,
    #[serde(default)]
    pub retention: Retention,
    pub llm_for_builder: LlmForBuilder,
}

impl Default for Quality {
    fn default() -> Self {
        Self {
            reconcile_max_age: default_reconcile(),
            rejects_alert: RejectsAlert::default(),
        }
    }
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            audit_days: default_audit_days(),
            transcripts_days: default_audit_days(),
            outputs_days: default_output_days(),
            uploads_days: default_audit_days(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub host: HostName,
    #[serde(deserialize_with = "de::port")]
    #[schemars(range(min = 1, max = 65535))]
    pub port: u16,
    pub database: DatabaseName,
    #[serde(default = "default_tls")]
    pub tls: TlsMode,
    #[serde(default)]
    pub username: Option<Label>,
    #[serde(default)]
    pub application_name: Option<Label>,
    #[serde(default)]
    pub connect_timeout: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TableSelect {
    #[serde(default = "default_include")]
    pub include: Vec<GlobPattern>,
    #[serde(default)]
    pub exclude: Vec<GlobPattern>,
}

impl Default for TableSelect {
    fn default() -> Self {
        Self {
            include: default_include(),
            exclude: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Incremental {
    #[serde(default = "default_cursor")]
    pub cursor: TableName,
    #[serde(default = "default_every")]
    pub every: Duration,
    #[serde(default = "default_overlap")]
    pub overlap: Duration,
}

impl Default for Incremental {
    fn default() -> Self {
        Self {
            cursor: default_cursor(),
            every: default_every(),
            overlap: default_overlap(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceLimits {
    #[serde(default = "default_connections", deserialize_with = "de::connections")]
    #[schemars(range(min = 1, max = 128))]
    pub max_connections: u32,
    #[serde(
        default = "default_rows_per_sec",
        deserialize_with = "de::rows_per_sec"
    )]
    #[schemars(range(min = 1, max = 1_000_000_000))]
    pub max_rows_per_sec: u32,
    #[serde(default)]
    pub window: Option<DayWindow>,
}

impl Default for SourceLimits {
    fn default() -> Self {
        Self {
            max_connections: default_connections(),
            max_rows_per_sec: default_rows_per_sec(),
            window: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TypePolicy {
    #[serde(default = "default_zero_date")]
    pub zero_date: ZeroDatePolicy,
    #[serde(default = "default_tinyint1")]
    pub tinyint1: Tinyint1Policy,
    #[serde(default = "default_strip_nul")]
    pub strip_nul: bool,
    #[serde(default)]
    pub charset_override: PatternMap<CharsetTarget, CharsetName>,
}

impl Default for TypePolicy {
    fn default() -> Self {
        Self {
            zero_date: default_zero_date(),
            tinyint1: default_tinyint1(),
            strip_nul: default_strip_nul(),
            charset_override: PatternMap::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ColumnExclude {
    #[serde(default)]
    pub exclude: Vec<TableName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TableOverride {
    #[serde(default)]
    pub sync: Option<SyncMode>,
    #[serde(default)]
    pub every: Option<Duration>,
    #[serde(default)]
    pub columns: Option<ColumnExclude>,
    #[serde(default)]
    pub key: Option<Vec<TableName>>,
    #[serde(default)]
    pub partition: Option<TableName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub connector: ConnectorName,
    pub connection: Connection,
    #[serde(default = "default_read_from")]
    pub read_from: ReadFrom,
    pub credential: SecretRef,
    #[serde(default)]
    pub timezone: Option<TimezoneName>,
    #[serde(default)]
    pub tables: TableSelect,
    #[serde(default = "default_sync")]
    pub sync: SyncMode,
    #[serde(default = "default_snapshot")]
    pub snapshot: SnapshotMode,
    #[serde(default = "default_consistency")]
    pub consistency: ConsistencyMode,
    #[serde(default)]
    pub consistency_acknowledged: bool,
    #[serde(default = "default_txn_events", deserialize_with = "de::txn_events")]
    #[schemars(range(min = 1, max = 10_000_000))]
    pub max_txn_events: u32,
    #[serde(default = "default_snapshot_seconds")]
    pub max_snapshot_txn_seconds: Duration,
    #[serde(default)]
    pub freshness_expectation: Option<Duration>,
    #[serde(default)]
    pub incremental: Incremental,
    #[serde(default)]
    pub limits: SourceLimits,
    #[serde(default)]
    pub type_policy: TypePolicy,
    #[serde(default)]
    pub overrides: PatternMap<TableName, TableOverride>,
    #[serde(default = "default_sample")]
    pub sample_values: SampleValues,
    #[serde(default)]
    pub hooks: Vec<PluginName>,
    #[serde(default)]
    pub allow_write_capable_credential: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourcesFile {
    pub schema_version: ConfigVersion,
    pub sources: PatternMap<SourceName, Source>,
}

/// CDC defaults to 10 minutes. Batch paths default to 2 hours.
/// `auto` uses the CDC expectation because auto prefers CDC when the connector
/// can do it. Callers that already know the resolved mode should pass that mode.
pub fn effective_freshness(sync: SyncMode, configured: Option<&Duration>) -> Duration {
    if let Some(duration) = configured {
        return duration.clone();
    }
    match sync {
        SyncMode::Incremental | SyncMode::Full => duration("2h"),
        SyncMode::Auto | SyncMode::Cdc => duration("10m"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ParseProfileInline {
    #[serde(default, deserialize_with = "de::header_row")]
    #[schemars(range(min = 0, max = 1_000_000))]
    pub header_row: u32,
    #[serde(default)]
    pub columns: Vec<Label>,
    #[serde(default)]
    pub ignore_rows: Vec<u32>,
    #[serde(default)]
    pub encoding: Option<CharsetName>,
    #[serde(default)]
    pub delimiter: Option<Delimiter>,
    #[serde(default)]
    pub date_system: Option<DateSystem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(untagged)]
pub enum ParseProfile {
    Named(PluginName),
    Inline(ParseProfileInline),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetMapping {
    pub sheet: Label,
    pub table: TableName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FileSet {
    pub schema: FilesSchemaName,
    pub mode: FileMode,
    #[serde(default)]
    pub key: Option<Vec<TableName>>,
    pub parse_profile: ParseProfile,
    #[serde(default)]
    pub sheets: Vec<SheetMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FilesFile {
    pub schema_version: ConfigVersion,
    pub files: PatternMap<SourceName, FileSet>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RoleLimits {
    #[serde(default = "default_max_rows", deserialize_with = "de::max_rows")]
    #[schemars(range(min = 1, max = 100000))]
    pub max_rows: u32,
    #[serde(
        default = "default_max_rows_cap",
        deserialize_with = "de::max_rows_cap"
    )]
    #[schemars(range(min = 1, max = 100000))]
    pub max_rows_cap: u32,
    #[serde(default = "default_export_rows", deserialize_with = "de::export_rows")]
    #[schemars(range(min = 1, max = 100000000))]
    pub export_rows: u32,
    #[serde(default = "default_export_bytes")]
    pub export_bytes: ByteSize,
    #[serde(default = "default_concurrency", deserialize_with = "de::concurrency")]
    #[schemars(range(min = 1, max = 256))]
    pub concurrency: u32,
    #[serde(default = "default_statement_timeout")]
    pub statement_timeout: Duration,
    #[serde(default = "default_export_timeout")]
    pub export_timeout: Duration,
}

impl Default for RoleLimits {
    fn default() -> Self {
        Self {
            max_rows: default_max_rows(),
            max_rows_cap: default_max_rows_cap(),
            export_rows: default_export_rows(),
            export_bytes: default_export_bytes(),
            concurrency: default_concurrency(),
            statement_timeout: default_statement_timeout(),
            export_timeout: default_export_timeout(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RoleLimitOverride {
    #[serde(default, deserialize_with = "de_option_max_rows")]
    #[schemars(range(min = 1, max = 100000))]
    pub max_rows: Option<u32>,
    #[serde(default, deserialize_with = "de_option_max_rows_cap")]
    #[schemars(range(min = 1, max = 100000))]
    pub max_rows_cap: Option<u32>,
    #[serde(default, deserialize_with = "de_option_export_rows")]
    #[schemars(range(min = 1, max = 100000000))]
    pub export_rows: Option<u32>,
    #[serde(default)]
    pub export_bytes: Option<ByteSize>,
    #[serde(default, deserialize_with = "de_option_concurrency")]
    #[schemars(range(min = 1, max = 256))]
    pub concurrency: Option<u32>,
    #[serde(default)]
    pub statement_timeout: Option<Duration>,
    #[serde(default)]
    pub export_timeout: Option<Duration>,
}

fn de_option_max_rows<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    Option::<u32>::deserialize(deserializer)?
        .map(|value| {
            if (1..=100_000).contains(&value) {
                Ok(value)
            } else {
                Err(serde::de::Error::custom("max_rows out of range"))
            }
        })
        .transpose()
}

fn de_option_max_rows_cap<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    Option::<u32>::deserialize(deserializer)?
        .map(|value| {
            if (1..=100_000).contains(&value) {
                Ok(value)
            } else {
                Err(serde::de::Error::custom("max_rows_cap out of range"))
            }
        })
        .transpose()
}

fn de_option_export_rows<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    Option::<u32>::deserialize(deserializer)?
        .map(|value| {
            if (1..=100_000_000).contains(&value) {
                Ok(value)
            } else {
                Err(serde::de::Error::custom("export_rows out of range"))
            }
        })
        .transpose()
}

fn de_option_concurrency<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    Option::<u32>::deserialize(deserializer)?
        .map(|value| {
            if (1..=256).contains(&value) {
                Ok(value)
            } else {
                Err(serde::de::Error::custom("concurrency out of range"))
            }
        })
        .transpose()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SchemaGrant {
    pub schema: SourceName,
    pub tables: Vec<GlobPattern>,
    #[serde(default)]
    pub exclude: Vec<GlobPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TableGrant {
    pub table: QualifiedTable,
    pub columns: ColumnExclude,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(untagged)]
pub enum Grant {
    Schema(SchemaGrant),
    Table(TableGrant),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BindingMatch {
    #[serde(default)]
    pub idp_group: Option<Label>,
    #[serde(default)]
    pub any: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(untagged)]
pub enum ClaimValue {
    Text(String),
    List(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    #[serde(rename = "match")]
    pub match_rule: BindingMatch,
    pub role: Option<SourceName>,
    #[serde(default)]
    pub claims: PatternMap<ClaimName, ClaimValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub label: Label,
    #[serde(default)]
    pub grants: Vec<Grant>,
    #[serde(default)]
    pub column_masks: PatternMap<ColumnPath, PluginName>,
    #[serde(default)]
    pub pii_access: Vec<PiiLevel>,
    #[serde(default)]
    pub row_filters: PatternMap<QualifiedTable, RowFilterExpr>,
    #[serde(default)]
    pub limits: Option<RoleLimitOverride>,
    #[serde(default)]
    pub allow_external_clients: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct RoleDefaults {
    #[serde(default)]
    pub limits: RoleLimits,
    #[serde(default)]
    pub allow_external_clients: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RolesFile {
    pub schema_version: ConfigVersion,
    #[serde(default)]
    pub defaults: RoleDefaults,
    pub roles: PatternMap<SourceName, Role>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Harness {
    pub default: PluginName,
    #[serde(default)]
    pub versions: PatternMap<PluginName, VersionToken>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub provider: PluginName,
    pub model: ModelId,
    pub endpoint_alias: PluginName,
    #[serde(default)]
    pub credential: Option<SecretRef>,
    #[serde(default)]
    pub fallback: Option<Box<ModelConfig>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    #[serde(default = "default_session_tokens", deserialize_with = "de::tokens")]
    #[schemars(range(min = 1, max = 100000000))]
    pub per_session_tokens: u32,
    #[serde(default = "default_tool_calls", deserialize_with = "de::tool_calls")]
    #[schemars(range(min = 1, max = 10000))]
    pub per_turn_tool_calls: u32,
    #[serde(
        default = "default_turn_seconds",
        deserialize_with = "de::turn_seconds"
    )]
    #[schemars(range(min = 1, max = 86400))]
    pub per_turn_seconds: u32,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            per_session_tokens: default_session_tokens(),
            per_turn_tool_calls: default_tool_calls(),
            per_turn_seconds: default_turn_seconds(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Session {
    #[serde(default = "default_idle")]
    pub idle_timeout: Duration,
    #[serde(default = "default_lifetime")]
    pub max_lifetime: Duration,
    #[serde(default)]
    pub workspace_snapshot: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            idle_timeout: default_idle(),
            max_lifetime: default_lifetime(),
            workspace_snapshot: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxDriver {
    DockerGvisor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SandboxResources {
    #[serde(default = "default_cpu", deserialize_with = "de::cpu")]
    #[schemars(range(min = 1, max = 64))]
    pub cpu: u32,
    #[serde(default = "default_memory")]
    pub memory: ByteSize,
    #[serde(default = "default_disk")]
    pub disk: ByteSize,
    #[serde(default = "default_pids", deserialize_with = "de::pids")]
    #[schemars(range(min = 1, max = 65535))]
    pub pids: u32,
}

impl Default for SandboxResources {
    fn default() -> Self {
        Self {
            cpu: default_cpu(),
            memory: default_memory(),
            disk: default_disk(),
            pids: default_pids(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SandboxPool {
    #[serde(default = "default_min_idle", deserialize_with = "de::pool_size")]
    #[schemars(range(min = 0, max = 10000))]
    pub min_idle: u32,
    #[serde(default = "default_max_pool", deserialize_with = "de::pool_size")]
    #[schemars(range(min = 0, max = 10000))]
    pub max_pool: u32,
    #[serde(default = "default_pool_idle")]
    pub max_idle_age: Duration,
}

impl Default for SandboxPool {
    fn default() -> Self {
        Self {
            min_idle: default_min_idle(),
            max_pool: default_max_pool(),
            max_idle_age: default_pool_idle(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Sandbox {
    pub driver: SandboxDriver,
    pub image: Label,
    #[serde(default)]
    pub resources: SandboxResources,
    #[serde(default)]
    pub pool: SandboxPool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct WebChannel {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FeishuChannel {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub app_id: Option<Label>,
    #[serde(default)]
    pub credential: Option<SecretRef>,
    #[serde(default = "default_group_policy")]
    pub group_policy: GroupPolicy,
}

impl Default for FeishuChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            app_id: None,
            credential: None,
            group_policy: default_group_policy(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct SlackChannel {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub credential: Option<SecretRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct Channels {
    #[serde(default)]
    pub web: WebChannel,
    #[serde(default)]
    pub feishu: FeishuChannel,
    #[serde(default)]
    pub slack: SlackChannel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub schema_version: ConfigVersion,
    pub harness: Harness,
    pub model: ModelConfig,
    #[serde(default)]
    pub budgets: Budgets,
    #[serde(default)]
    pub session: Session,
    pub sandbox: Sandbox,
    #[serde(default)]
    pub instructions_append: Option<ByteBoundedText<{ crate::limits::INSTRUCTIONS_MAX_BYTES }>>,
    #[serde(default)]
    pub channels: Channels,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HookParams {
    #[serde(default)]
    pub columns: Vec<GlobPattern>,
    #[serde(default)]
    pub pattern: Option<PluginName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    #[serde(rename = "type")]
    pub kind: HookType,
    #[serde(default)]
    pub builtin: Option<PluginName>,
    #[serde(default)]
    pub params: Option<HookParams>,
    #[serde(default)]
    pub triggers: Vec<QualifiedTable>,
    #[serde(default)]
    pub debounce: Option<Duration>,
    #[serde(default)]
    pub sql_dir: Option<RelativePath>,
    #[serde(default)]
    pub materialize: Vec<TableName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct HooksFile {
    pub schema_version: ConfigVersion,
    pub hooks: PatternMap<PluginName, Hook>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Scoring {
    pub numeric_rel_tol: FlexDecimal,
    pub pass_rule: crate::text::PassRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EvalSuite {
    pub schema_version: ConfigVersion,
    pub suite: PluginName,
    pub visibility: Visibility,
    pub snapshot: Label,
    #[serde(default = "default_runs", deserialize_with = "de::runs_per_item")]
    #[schemars(range(min = 1, max = 100))]
    pub runs_per_item: u32,
    pub scoring: Scoring,
    pub items_file: RelativePath,
}
