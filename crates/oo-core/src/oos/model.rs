//! OOS structures from docs/06 §§3.2–3.12. Unknown fields are rejected.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::canonical::validate_date;
use crate::config::de::{self, deprecation_releases};
use crate::config::model::{PiiLevel, SampleValues};
use crate::config::version::OosVersion;
use crate::decimal::FlexDecimal;
use crate::pattern_map::{PatternKey, PatternMap};
use crate::schema_util::schema_object;
use crate::text::{
    BoundedText, ColumnPath, HexDigest, Label, PluginName, QualifiedTable, SourceName, TableName,
    TimezoneName, VersionToken,
};

crate::text::string_newtype_export!(ClassName, 1, 64, r"^[a-z][a-z0-9_]{0,63}$", class_name_ok);
crate::text::string_newtype_export!(ExampleId, 2, 64, r"^[a-z][a-z0-9-]{1,63}$", example_id_ok);
/// Gregorian `YYYY-MM-DD`. JSON Schema `format: date` is necessary and is not
/// the whole rule: jsonschema treats year 0 as a leap year, and Rust rejects
/// year 0 along with every impossible day. The accepted year range for this
/// 4-digit spelling is 0001 through 9999.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct CivilDate(String);

impl CivilDate {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if civil_date_ok(&value) {
            Ok(Self(value))
        } else {
            Err("invalid CivilDate".to_owned())
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CivilDate {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CivilDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CivilDate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for CivilDate {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("CivilDate")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schema_object(serde_json::json!({
            "type": "string",
            "format": "date",
            "pattern": r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$",
            "minLength": 10,
            "maxLength": 10,
            "description": "format date checks a Gregorian shape. Rust also rejects year 0000, including 0000-02-29, which a u16 leap-year check can accept."
        }))
    }
}

impl PartialSchema for CivilDate {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .pattern(Some(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$"))
            .min_length(Some(10))
            .max_length(Some(10))
            .format(Some(utoipa::openapi::schema::SchemaFormat::KnownFormat(
                utoipa::openapi::schema::KnownFormat::Date,
            )))
            .into()
    }
}

impl ToSchema for CivilDate {}
crate::text::string_newtype_export!(
    AnchorExpr,
    1,
    200,
    r"^[A-Za-z0-9_+() ,-]{1,200}$",
    anchor_expr_ok
);
crate::text::string_newtype_export!(
    PhraseLabel,
    1,
    64,
    r"^[^\u{0}-\u{1F}\u{7F}]{1,64}$",
    phrase_ok
);
crate::text::string_newtype_export!(
    Placeholder,
    0,
    64,
    r"^[^\u{0}-\u{1F}\u{7F}]{0,64}$",
    placeholder_ok
);

fn class_name_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

fn example_id_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (2..=64).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn civil_date_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let year = std::str::from_utf8(&bytes[0..4])
        .ok()
        .and_then(|text| text.parse::<i32>().ok());
    let month = std::str::from_utf8(&bytes[5..7])
        .ok()
        .and_then(|text| text.parse::<u8>().ok());
    let day = std::str::from_utf8(&bytes[8..10])
        .ok()
        .and_then(|text| text.parse::<u8>().ok());
    match (year, month, day) {
        (Some(year), Some(month), Some(day)) => validate_date(year, month, day).is_ok(),
        _ => false,
    }
}

fn anchor_expr_ok(value: &str) -> bool {
    (1..=200).contains(&value.chars().count())
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '(' | ')' | ' ' | ',' | '-')
        })
}

fn phrase_ok(value: &str) -> bool {
    let count = value.chars().count();
    (1..=64).contains(&count)
        && value
            .chars()
            .all(|ch| !matches!(u32::from(ch), 0x00..=0x1F | 0x7F))
}

fn placeholder_ok(value: &str) -> bool {
    value.chars().count() <= 64
        && value
            .chars()
            .all(|ch| !matches!(u32::from(ch), 0x00..=0x1F | 0x7F))
}

impl PatternKey for ClassName {
    const PATTERN: &'static str = r"^[a-z][a-z0-9_]{0,63}$";
}
impl PatternKey for PhraseLabel {
    const PATTERN: &'static str = r"^[^\u{0}-\u{1F}\u{7F}]{1,64}$";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Raw,
    Scene,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimeRole {
    CreatedAt,
    UpdatedAt,
    EventTime,
    DeletedAt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SemanticType {
    Identifier,
    ForeignKey,
    EntityName,
    Code,
    StatusCode,
    Category,
    BooleanFlag,
    Money,
    Quantity,
    Ratio,
    EventTime,
    Date,
    AuditTime,
    Geo,
    Address,
    Phone,
    Email,
    PersonName,
    FreeText,
    Json,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Draft,
    Active,
    Deprecated,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishPolicy {
    #[serde(default)]
    pub include_unreviewed: Vec<PluginName>,
    #[serde(default)]
    pub require_eval: Vec<PluginName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassificationPolicy {
    pub auto_accept: PluginName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecyclePolicy {
    #[serde(deserialize_with = "deprecation_releases")]
    pub deprecation_min_releases: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CompileBudgets {
    #[serde(deserialize_with = "de::token_budget")]
    pub readme_tokens: u32,
    #[serde(deserialize_with = "de::token_budget")]
    pub table_file_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SamplePolicy {
    pub default: SampleValues,
    #[serde(deserialize_with = "de::sample_cap")]
    pub max_per_column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CompileOptions {
    pub per_role: bool,
    pub budgets: CompileBudgets,
    pub sample_values: SamplePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OntologyFile {
    pub spec_version: OosVersion,
    pub name: PluginName,
    pub locale: crate::text::LocaleCode,
    pub include_schemas: Vec<SourceName>,
    pub default_tier_preference: Vec<Tier>,
    pub publish: PublishPolicy,
    pub classification: ClassificationPolicy,
    pub lifecycle: LifecyclePolicy,
    pub compile: CompileOptions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeColumn {
    pub name: TableName,
    pub role: TimeRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub state: ReviewState,
    #[serde(default)]
    pub by: Option<Label>,
    #[serde(default)]
    pub at: Option<CivilDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColumnProvenance {
    Human,
    Generated,
    Declared,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClassState {
    Confirmed,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassificationRecord {
    pub state: ClassState,
    #[serde(default)]
    pub by: Option<Label>,
    #[serde(default)]
    pub at: Option<CivilDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DatasetColumn {
    pub name: TableName,
    #[serde(default)]
    pub label: Option<Label>,
    #[serde(default)]
    pub data_type: Option<Label>,
    #[serde(default)]
    pub semantic_type: Option<SemanticType>,
    #[serde(default)]
    pub searchable: Option<bool>,
    #[serde(default)]
    pub pii: Option<PiiLevel>,
    #[serde(default)]
    pub classification: Option<ClassificationRecord>,
    #[serde(default, rename = "enum")]
    pub enum_ref: Option<ColumnPath>,
    #[serde(default)]
    pub status: Option<ItemStatus>,
    #[serde(default)]
    pub replaced_by: Option<ColumnPath>,
    #[serde(default)]
    pub deprecated_at: Option<Label>,
    #[serde(default)]
    pub unit: Option<Label>,
    #[serde(default)]
    pub description: Option<BoundedText<4000>>,
    #[serde(default)]
    pub provenance: Option<ColumnProvenance>,
    #[serde(default)]
    pub review: Option<Review>,
}

/// Source consistency. These four tokens are not an A–F profile quality grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
pub enum SourceConsistencyGrade {
    #[serde(rename = "T")]
    T,
    #[serde(rename = "S1")]
    S1,
    #[serde(rename = "S0")]
    S0,
    #[serde(rename = "B")]
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceConstraintKind {
    PrimaryKey,
    Unique,
    ForeignKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceConstraintTrustLevel {
    Enforced,
    DeclaredOnly,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceConstraintDisposition {
    Enforced,
    Candidate,
    Blocked,
    Recorded,
    Stripped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceInclusion {
    pub rows_checked: u64,
    #[serde(deserialize_with = "ratio_thousandths")]
    #[schemars(range(min = 0, max = 1000))]
    pub ratio_thousandths: u32,
}

/// Column-scoped source caveat. The note is a token, not SQL text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceColumnSemantics {
    pub column: TableName,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<PluginName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_note: Option<PluginName>,
}

/// One constraint-trust decision carried on the dataset that owns the columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceConstraintTrust {
    pub name: SourceName,
    pub kind: SourceConstraintKind,
    pub columns: Vec<TableName>,
    pub trust: SourceConstraintTrustLevel,
    pub disposition: SourceConstraintDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inclusion: Option<SourceInclusion>,
}

/// Optional additive `oos/v1` source record. Absence matches a legacy dataset.
///
/// Fields are identifiers and tokens. There is no endpoint, credential, or SQL text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceSemantics {
    pub family: SourceName,
    pub product: SourceName,
    pub version: VersionToken,
    pub capture: SourceName,
    /// Absent when the source observation did not record a consistency grade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<SourceConsistencyGrade>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<PluginName>,
    pub source_kind: SourceName,
    /// Absent when the source observation did not record delete detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_detection: Option<SourceName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<SourceColumnSemantics>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraint_trust: Vec<SourceConstraintTrust>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DatasetFile {
    pub schema_version: OosVersion,
    pub dataset: QualifiedTable,
    #[serde(default)]
    pub source_name: Option<Label>,
    pub label: Label,
    #[serde(default)]
    pub description: Option<BoundedText<4000>>,
    pub tier: Tier,
    #[serde(default)]
    pub recommended: Option<bool>,
    #[serde(default)]
    pub superseded_by: Option<QualifiedTable>,
    #[serde(default)]
    pub grain: Option<Label>,
    #[serde(default)]
    pub primary_key: Vec<TableName>,
    #[serde(default)]
    pub entity: Option<SourceName>,
    #[serde(default)]
    pub row_count_estimate: Option<u64>,
    #[serde(default)]
    pub time_columns: Vec<TimeColumn>,
    #[serde(default)]
    pub columns: Vec<DatasetColumn>,
    #[serde(default)]
    pub caveats: Vec<BoundedText<2000>>,
    /// Missing on legacy documents. Serialization omits it so those bytes stay stable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_semantics: Option<SourceSemantics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityMember {
    pub dataset: QualifiedTable,
    pub key: Vec<TableName>,
    #[serde(default)]
    pub name: Option<TableName>,
    #[serde(default)]
    pub strong_key: Option<TableName>,
    #[serde(default)]
    pub phone: Option<TableName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityFile {
    pub schema_version: OosVersion,
    pub entity: SourceName,
    pub label: Label,
    #[serde(default)]
    pub synonyms: Vec<Label>,
    #[serde(default)]
    pub description: Option<BoundedText<4000>>,
    pub primary_dataset: QualifiedTable,
    pub key: TableName,
    pub display: TableName,
    #[serde(default)]
    pub members: Vec<EntityMember>,
    #[serde(default)]
    pub resolution: Option<SourceName>,
    #[serde(default)]
    pub datasets: Vec<QualifiedTable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JoinHint {
    Left,
    Inner,
    Right,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelationProvenance {
    DeclaredFk,
    Inferred,
    ViewDefinition,
    QueryLog,
    Human,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NameMatch {
    Exact,
    Partial,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationEvidence {
    #[serde(default)]
    pub containment: Option<FlexDecimal>,
    #[serde(default)]
    pub name_match: Option<NameMatch>,
    #[serde(default)]
    pub type_compatible: Option<bool>,
    #[serde(default)]
    pub seen_in_query_log: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationEnd {
    pub dataset: QualifiedTable,
    pub columns: Vec<TableName>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Relationship {
    pub name: SourceName,
    pub from: RelationEnd,
    pub to: RelationEnd,
    pub cardinality: Cardinality,
    pub join_hint: JoinHint,
    pub provenance: RelationProvenance,
    #[serde(default)]
    pub confidence: Option<FlexDecimal>,
    #[serde(default)]
    pub evidence: Option<RelationEvidence>,
    #[serde(default)]
    pub review: Option<Review>,
    #[serde(default)]
    pub status: Option<ItemStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationshipsFile {
    pub schema_version: OosVersion,
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnumValueStatus {
    Approved,
    Undetermined,
    Unexplained,
    Superseded,
    Deprecated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnumSource {
    DictionaryTable,
    Inferred,
    Human,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumEvidence {
    #[serde(default)]
    pub signature: Option<BoundedText<2000>>,
    #[serde(default)]
    pub rows: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumValue {
    pub code: Label,
    #[serde(default)]
    pub label: Option<Label>,
    #[serde(default)]
    pub class: Option<ClassName>,
    pub status: EnumValueStatus,
    #[serde(default)]
    pub source: Option<EnumSource>,
    #[serde(default)]
    pub evidence: Option<EnumEvidence>,
    #[serde(default)]
    pub valid_from: Option<CivilDate>,
    #[serde(default)]
    pub valid_to: Option<CivilDate>,
    #[serde(default)]
    pub review: Option<Review>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumFile {
    pub schema_version: OosVersion,
    #[serde(rename = "enum")]
    pub enum_name: ColumnPath,
    #[serde(default)]
    pub owner: Option<Label>,
    #[serde(default)]
    pub classes: PatternMap<ClassName, BoundedText<500>>,
    #[serde(default)]
    pub values: Vec<EnumValue>,
    #[serde(default)]
    pub coverage: Option<FlexDecimal>,
    #[serde(default)]
    pub unexplained_values: Vec<Label>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetricStatus {
    Draft,
    Active,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumFilter {
    pub column: ColumnPath,
    #[serde(rename = "enum")]
    pub enum_name: ColumnPath,
    pub classes: Vec<ClassName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestExpect {
    #[serde(default)]
    pub rows: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MetricTest {
    pub params: PatternMap<TableName, Label>,
    pub expect: TestExpect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Tolerance {
    #[serde(default)]
    pub abs: Option<FlexDecimal>,
    #[serde(default)]
    pub rel: Option<FlexDecimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlTotal {
    pub as_of: AnchorName,
    pub source: SourceName,
    pub sql: BoundedText<16000>,
    pub tolerance: Tolerance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub metric: SourceName,
    pub label: Label,
    #[serde(default)]
    pub synonyms: Vec<Label>,
    #[serde(default)]
    pub description: Option<BoundedText<4000>>,
    #[serde(default)]
    pub owner: Option<Label>,
    #[serde(default)]
    pub certified: bool,
    #[serde(default)]
    pub unit: Option<Label>,
    pub status: MetricStatus,
    #[serde(default)]
    pub grain: Vec<Label>,
    #[serde(default)]
    pub time_dimension: Option<ColumnPath>,
    #[serde(default)]
    pub default_anchor: Option<AnchorName>,
    #[serde(default)]
    pub enum_filters: Vec<EnumFilter>,
    pub sql: BoundedText<16000>,
    #[serde(default)]
    pub dimensions: Vec<SourceName>,
    #[serde(default)]
    pub caveats: Vec<BoundedText<2000>>,
    #[serde(default)]
    pub tests: Vec<MetricTest>,
    #[serde(default)]
    pub control_total: Option<ControlTotal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MetricsFile {
    pub schema_version: OosVersion,
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TermStatus {
    Draft,
    Approved,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TermMapping {
    #[serde(default)]
    pub dataset: Option<QualifiedTable>,
    #[serde(default)]
    pub filter: Option<BoundedText<2000>>,
    #[serde(default)]
    pub metric: Option<SourceName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Term {
    pub term: Label,
    pub definition: BoundedText<2000>,
    #[serde(default)]
    pub maps_to: Option<TermMapping>,
    #[serde(default)]
    pub owner: Option<Label>,
    #[serde(default)]
    pub status: Option<TermStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GlossaryFile {
    pub schema_version: OosVersion,
    pub terms: Vec<Term>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExampleCheck {
    pub at: CivilDate,
    pub status: CheckStatus,
    pub result_hash: HexDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifiedExample {
    pub id: ExampleId,
    pub question: BoundedText<2000>,
    pub sql: BoundedText<16000>,
    #[serde(default)]
    pub uses: Vec<Label>,
    pub verified_by: Label,
    pub verified_at: CivilDate,
    #[serde(default)]
    pub last_checked: Option<ExampleCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExamplesFile {
    pub schema_version: OosVersion,
    pub examples: Vec<VerifiedExample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnchorName {
    Today,
    MonthStart,
    PrevMonthStart,
    PrevMonthEnd,
    QuarterStart,
    PrevQuarterStart,
    PrevQuarterEnd,
    FiscalYearStart,
    PrevFiscalYearEnd,
    LastClosedPeriodEnd,
}

impl AnchorName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::MonthStart => "month_start",
            Self::PrevMonthStart => "prev_month_start",
            Self::PrevMonthEnd => "prev_month_end",
            Self::QuarterStart => "quarter_start",
            Self::PrevQuarterStart => "prev_quarter_start",
            Self::PrevQuarterEnd => "prev_quarter_end",
            Self::FiscalYearStart => "fiscal_year_start",
            Self::PrevFiscalYearEnd => "prev_fiscal_year_end",
            Self::LastClosedPeriodEnd => "last_closed_period_end",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AnchorRange {
    pub start: AnchorExpr,
    pub end: AnchorExpr,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PhraseComparison {
    pub current: AnchorRange,
    pub previous: AnchorRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(untagged)]
pub enum Phrase {
    Range(AnchorRange),
    Comparison(PhraseComparison),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds;

impl Serialize for Bounds {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("[)")
    }
}

impl<'de> Deserialize<'de> for Bounds {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == "[)" {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom("bounds must be `[)`"))
        }
    }
}

impl JsonSchema for Bounds {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Bounds")
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        crate::schema_util::schema_object(serde_json::json!({"type": "string", "const": "[)"}))
    }
}

impl utoipa::PartialSchema for Bounds {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .enum_values(Some(["[)"]))
            .into()
    }
}

impl ToSchema for Bounds {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarFile {
    pub schema_version: OosVersion,
    pub business_tz: TimezoneName,
    #[serde(deserialize_with = "fiscal_month")]
    #[schemars(range(min = 1, max = 12))]
    pub fiscal_year_start_month: u8,
    pub week_start: Weekday,
    pub anchors: Vec<AnchorName>,
    #[serde(default)]
    pub phrases: PatternMap<PhraseLabel, Phrase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyScheme {
    pub strong: BoundedText<200>,
    pub singleton: BoundedText<200>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StrongKey {
    pub validator: PluginName,
    pub placeholders: Vec<Placeholder>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MinLength {
    #[serde(deserialize_with = "small_count")]
    #[schemars(range(min = 1, max = 32))]
    pub han: u32,
    #[serde(deserialize_with = "small_count")]
    #[schemars(range(min = 1, max = 32))]
    pub latin: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Normalization {
    pub fullwidth_to_halfwidth: bool,
    pub casefold: bool,
    pub collapse_whitespace: bool,
    #[serde(default)]
    pub strip_suffixes: Vec<Label>,
    pub min_length: MinLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    ListAll,
    PreferFirst,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SurvivorshipPolicy {
    pub order: Vec<SourceName>,
    pub on_conflict: ConflictPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(untagged)]
pub enum Survivorship {
    Order(Vec<PluginName>),
    Policy(SurvivorshipPolicy),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolutionFile {
    pub schema_version: OosVersion,
    pub resolution: SourceName,
    pub entity: SourceName,
    pub key_scheme: KeyScheme,
    pub strong_key: StrongKey,
    #[serde(default)]
    pub auto_merge: Vec<PluginName>,
    #[serde(default)]
    pub candidates: Vec<PluginName>,
    pub normalization: Normalization,
    #[serde(default)]
    pub survivorship: PatternMap<TableName, Survivorship>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Approved,
    Rejected,
    Merge,
    Split,
    NotSame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub item: Label,
    pub input_hash: HexDigest,
    pub decision: DecisionKind,
    #[serde(default)]
    pub operator: Option<Label>,
    pub at: CivilDate,
    #[serde(default)]
    pub reason: Option<BoundedText<2000>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionsFile {
    pub schema_version: OosVersion,
    pub decisions: Vec<Decision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Pass,
    Fail,
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GateReport {
    pub gate_id: PluginName,
    pub status: GateStatus,
    pub blocking: bool,
    #[serde(default)]
    pub inputs: Vec<Label>,
    #[serde(default)]
    pub evidence: Vec<Label>,
    #[serde(default)]
    pub affected_objects: Vec<Label>,
    #[serde(default)]
    pub reviewer: Option<Label>,
}

fn fiscal_month<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let value = u8::deserialize(deserializer)?;
    if (1..=12).contains(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("month must be 1..=12"))
    }
}

fn small_count<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    if (1..=32).contains(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("count must be 1..=32"))
    }
}

fn ratio_thousandths<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    if value <= 1000 {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "ratio_thousandths is above 1000",
        ))
    }
}

const MAX_SOURCE_CAVEATS: usize = 32;
const MAX_SOURCE_COLUMNS: usize = 256;
const MAX_SOURCE_CONSTRAINTS: usize = 256;

pub fn validate_dataset(file: &DatasetFile) -> Vec<crate::diagnostic::Diagnostic> {
    let mut out = Vec::new();
    for (index, column) in file.columns.iter().enumerate() {
        if column.semantic_type == Some(SemanticType::Money) && column.unit.is_none() {
            out.push(oos(
                &format!("/columns/{index}/unit"),
                "semantic type money requires unit",
            ));
        }
    }
    if let Some(semantics) = &file.source_semantics {
        validate_source_semantics(semantics, file, &mut out);
    }
    out
}

fn validate_source_semantics(
    semantics: &SourceSemantics,
    file: &DatasetFile,
    out: &mut Vec<crate::diagnostic::Diagnostic>,
) {
    if semantics.caveats.len() > MAX_SOURCE_CAVEATS
        || semantics.columns.len() > MAX_SOURCE_COLUMNS
        || semantics.constraint_trust.len() > MAX_SOURCE_CONSTRAINTS
    {
        out.push(oos(
            "/source_semantics",
            "source semantics exceeds its list budget",
        ));
        return;
    }
    let mut seen_columns = Vec::new();
    for (index, column) in semantics.columns.iter().enumerate() {
        if column.caveats.len() > MAX_SOURCE_CAVEATS {
            out.push(oos(
                &format!("/source_semantics/columns/{index}/caveats"),
                "source semantics exceeds its list budget",
            ));
        }
        if !file
            .columns
            .iter()
            .any(|item| item.name.as_str() == column.column.as_str())
        {
            out.push(oos(
                &format!("/source_semantics/columns/{index}/column"),
                "source semantics names a column that is not on the dataset",
            ));
        }
        if seen_columns.iter().any(|name| name == &column.column) {
            out.push(oos(
                &format!("/source_semantics/columns/{index}/column"),
                "source semantics repeats a column",
            ));
        }
        seen_columns.push(column.column.clone());
    }
    let mut seen_constraints = Vec::new();
    for (index, constraint) in semantics.constraint_trust.iter().enumerate() {
        if constraint.columns.is_empty() {
            out.push(oos(
                &format!("/source_semantics/constraint_trust/{index}/columns"),
                "a source constraint names no column",
            ));
        }
        for name in &constraint.columns {
            if !file
                .columns
                .iter()
                .any(|item| item.name.as_str() == name.as_str())
            {
                out.push(oos(
                    &format!("/source_semantics/constraint_trust/{index}/columns"),
                    "source semantics names a column that is not on the dataset",
                ));
            }
        }
        if constraint
            .inclusion
            .as_ref()
            .is_some_and(|inclusion| inclusion.ratio_thousandths > 1000)
        {
            out.push(oos(
                &format!("/source_semantics/constraint_trust/{index}/inclusion/ratio_thousandths"),
                "ratio_thousandths is above 1000",
            ));
        }
        if seen_constraints.iter().any(|name| name == &constraint.name) {
            out.push(oos(
                &format!("/source_semantics/constraint_trust/{index}/name"),
                "source semantics repeats a constraint",
            ));
        }
        seen_constraints.push(constraint.name.clone());
    }
}

pub fn validate_relationships(file: &RelationshipsFile) -> Vec<crate::diagnostic::Diagnostic> {
    let mut out = Vec::new();
    for (index, item) in file.relationships.iter().enumerate() {
        if item.from.columns.is_empty() || item.from.columns.len() != item.to.columns.len() {
            out.push(oos(
                &format!("/relationships/{index}/from/columns"),
                "relationship ends need the same non-empty column count",
            ));
        }
    }
    out
}

pub fn validate_enum(file: &EnumFile) -> Vec<crate::diagnostic::Diagnostic> {
    let mut out = Vec::new();
    for (index, value) in file.values.iter().enumerate() {
        let forced = match value.status {
            EnumValueStatus::Undetermined => Some("undetermined"),
            EnumValueStatus::Unexplained => Some("unexplained"),
            _ => None,
        };
        if let Some(name) = forced {
            if value
                .class
                .as_ref()
                .is_none_or(|class| class.as_str() != name)
            {
                out.push(oos(
                    &format!("/values/{index}/class"),
                    format!("status {name} forces class {name}"),
                ));
            }
        } else if value.status == EnumValueStatus::Approved {
            match &value.class {
                Some(class) if file.classes.get(class).is_some() => {}
                _ => out.push(oos(
                    &format!("/values/{index}/class"),
                    "an approved code's class must exist on this enum",
                )),
            }
        }
    }
    out
}

pub fn validate_metrics(file: &MetricsFile) -> Vec<crate::diagnostic::Diagnostic> {
    let mut out = Vec::new();
    for (index, metric) in file.metrics.iter().enumerate() {
        if metric.certified
            && (metric.owner.is_none()
                || metric.default_anchor.is_none()
                || metric.control_total.is_none()
                || metric.tests.is_empty())
        {
            out.push(oos(
                &format!("/metrics/{index}"),
                "a certified metric requires owner, default_anchor, control_total, and at least one test",
            ));
        }
    }
    out
}

pub fn validate_glossary(file: &GlossaryFile) -> Vec<crate::diagnostic::Diagnostic> {
    let mut out = Vec::new();
    for (index, term) in file.terms.iter().enumerate() {
        if let Some(mapping) = &term.maps_to {
            let has_dataset = mapping.dataset.is_some();
            let has_metric = mapping.metric.is_some();
            if has_dataset == has_metric {
                out.push(oos(
                    &format!("/terms/{index}/maps_to"),
                    "maps_to names either a dataset or a metric",
                ));
            }
        }
    }
    out
}

pub fn validate_calendar(file: &CalendarFile) -> Vec<crate::diagnostic::Diagnostic> {
    let mut out = Vec::new();
    for (label, phrase) in file.phrases.iter() {
        match phrase {
            Phrase::Range(range) => {
                check_range(&format!("/phrases/{label}"), range, &file.anchors, &mut out)
            }
            Phrase::Comparison(item) => {
                check_range(
                    &format!("/phrases/{label}/current"),
                    &item.current,
                    &file.anchors,
                    &mut out,
                );
                check_range(
                    &format!("/phrases/{label}/previous"),
                    &item.previous,
                    &file.anchors,
                    &mut out,
                );
            }
        }
    }
    out
}

pub fn validate_decisions(file: &DecisionsFile) -> Vec<crate::diagnostic::Diagnostic> {
    let mut out = Vec::new();
    for (index, decision) in file.decisions.iter().enumerate() {
        if decision.decision == DecisionKind::Approved && decision.operator.is_none() {
            out.push(oos(
                &format!("/decisions/{index}/operator"),
                "an approved decision names an operator",
            ));
        }
    }
    out
}

fn check_range(
    path: &str,
    range: &AnchorRange,
    anchors: &[AnchorName],
    out: &mut Vec<crate::diagnostic::Diagnostic>,
) {
    if let Err(message) = parse_anchor_expr(range.start.as_str(), anchors) {
        out.push(oos(&format!("{path}/start"), message));
    }
    if let Err(message) = parse_anchor_expr(range.end.as_str(), anchors) {
        out.push(oos(&format!("{path}/end"), message));
    }
}

pub fn parse_anchor_expr(input: &str, anchors: &[AnchorName]) -> Result<(), String> {
    let mut parser = ExprParser {
        bytes: input.as_bytes(),
        index: 0,
        anchors,
    };
    parser.expr()?;
    parser.skip();
    if parser.index == parser.bytes.len() {
        Ok(())
    } else {
        Err("trailing anchor expression".to_owned())
    }
}

struct ExprParser<'a> {
    bytes: &'a [u8],
    index: usize,
    anchors: &'a [AnchorName],
}

impl<'a> ExprParser<'a> {
    fn expr(&mut self) -> Result<(), String> {
        self.term()?;
        loop {
            self.skip();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
                self.term()?;
            } else {
                return Ok(());
            }
        }
    }

    fn term(&mut self) -> Result<(), String> {
        self.skip();
        if self.peek() == Some(b'(') {
            self.bump();
            self.expr()?;
            self.skip();
            if self.peek() != Some(b')') {
                return Err("unclosed '('".to_owned());
            }
            self.bump();
            return Ok(());
        }
        if self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.bump();
            }
            return Ok(());
        }
        let start = self.index;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.bump();
        }
        if start == self.index {
            return Err("expected an anchor, number, or call".to_owned());
        }
        let name = std::str::from_utf8(&self.bytes[start..self.index])
            .map_err(|_| "invalid anchor".to_owned())?;
        self.skip();
        if self.peek() == Some(b'(') {
            if name != "min" && name != "max" {
                return Err(format!("unknown function `{name}`"));
            }
            self.bump();
            self.expr()?;
            self.skip();
            if self.peek() != Some(b',') {
                return Err(format!("{name} takes two arguments"));
            }
            self.bump();
            self.expr()?;
            self.skip();
            if self.peek() != Some(b')') {
                return Err(format!("unclosed {name}("));
            }
            self.bump();
            return Ok(());
        }
        if self.anchors.iter().any(|anchor| anchor.as_str() == name) {
            Ok(())
        } else {
            Err(format!("unknown anchor `{name}`"))
        }
    }

    fn skip(&mut self) {
        while self.peek() == Some(b' ') {
            self.bump();
        }
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }
    fn bump(&mut self) {
        self.index += 1;
    }
}

fn oos(path: &str, message: impl Into<String>) -> crate::diagnostic::Diagnostic {
    crate::diagnostic::Diagnostic::error(
        crate::diagnostic::CONTRACT_OOS,
        "cross_field",
        path,
        message,
    )
}

#[cfg(test)]
mod source_semantics_contract {
    use super::{validate_dataset, DatasetFile};
    use serde_json::json;

    fn legacy() -> serde_json::Value {
        json!({
            "schema_version": "oos/v1",
            "dataset": "public.customer",
            "label": "customer",
            "tier": "raw",
            "columns": [
                {"name": "id", "semantic_type": "identifier"},
                {"name": "amount", "semantic_type": "money", "unit": "USD"}
            ]
        })
    }

    #[test]
    fn legacy_dataset_omits_source_semantics_and_keeps_money_unit() {
        let file: DatasetFile = serde_json::from_value(legacy()).expect("legacy dataset");
        assert!(file.source_semantics.is_none());
        let encoded = serde_json::to_value(&file).expect("encode");
        assert!(encoded.get("source_semantics").is_none());
        assert!(validate_dataset(&file).is_empty());
        let mut money = legacy();
        money["columns"][1].as_object_mut().unwrap().remove("unit");
        let bare: DatasetFile = serde_json::from_value(money).expect("money without unit");
        assert!(
            validate_dataset(&bare)
                .iter()
                .any(|item| item.message.contains("money requires unit"))
        );
    }

    #[test]
    fn source_semantics_accepts_consistency_grades_and_rejects_profile_grade_and_authority() {
        let mut document = legacy();
        document["source_semantics"] = json!({
            "family": "oracle",
            "product": "oracle",
            "version": "19c",
            "capture": "batch_full",
            "grade": "B",
            "source_kind": "oracle_table",
            "delete_detection": "reconcile",
            "columns": [{
                "column": "amount",
                "caveats": ["precision_loss"],
                "native_note": "unconstrained_number"
            }],
            "constraint_trust": [{
                "name": "customer_party_fk",
                "kind": "foreign_key",
                "columns": ["id"],
                "trust": "declared_only",
                "disposition": "candidate",
                "inclusion": {"rows_checked": 1000, "ratio_thousandths": 980}
            }]
        });
        let file: DatasetFile = serde_json::from_value(document.clone()).expect("grade B");
        assert!(validate_dataset(&file).is_empty());
        assert_eq!(
            file.source_semantics.as_ref().unwrap().grade,
            Some(super::SourceConsistencyGrade::B)
        );
        let mut unobserved = document.clone();
        unobserved["source_semantics"]
            .as_object_mut()
            .unwrap()
            .remove("grade");
        unobserved["source_semantics"]
            .as_object_mut()
            .unwrap()
            .remove("delete_detection");
        let bare: DatasetFile = serde_json::from_value(unobserved).expect("grade omitted");
        let bare_semantics = bare.source_semantics.as_ref().unwrap();
        assert!(bare_semantics.grade.is_none());
        assert!(bare_semantics.delete_detection.is_none());
        let encoded = serde_json::to_value(bare_semantics).expect("encode");
        assert!(encoded.get("grade").is_none());
        assert!(encoded.get("delete_detection").is_none());

        for grade in ["A", "C", "D", "E", "F"] {
            let mut rejected = document.clone();
            rejected["source_semantics"]["grade"] = json!(grade);
            assert!(
                serde_json::from_value::<DatasetFile>(rejected).is_err(),
                "{grade} is a profile grade"
            );
        }

        let mut endpoint = document.clone();
        endpoint["source_semantics"]["endpoint"] = json!("secret://shop");
        assert!(serde_json::from_value::<DatasetFile>(endpoint).is_err());

        let mut sql = document.clone();
        sql["source_semantics"]["caveats"] = json!(["select nvl(name, id) from customer"]);
        assert!(serde_json::from_value::<DatasetFile>(sql).is_err());

        let mut ratio = document.clone();
        ratio["source_semantics"]["constraint_trust"][0]["inclusion"]["ratio_thousandths"] =
            json!(1001);
        assert!(serde_json::from_value::<DatasetFile>(ratio).is_err());

        let mut missing = document.clone();
        missing["source_semantics"]["columns"][0]["column"] = json!("not_a_column");
        let orphan: DatasetFile = serde_json::from_value(missing).expect("parses");
        assert!(
            validate_dataset(&orphan)
                .iter()
                .any(|item| item.path.contains("/source_semantics/columns/"))
        );
    }
}
