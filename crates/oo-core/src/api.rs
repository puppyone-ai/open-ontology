//! API envelopes and query documents.
//!
//! These types describe JSON bodies. They do not speak HTTP. A schema match
//! is not evidence that a server returned the document.
//!
//! Trust boundary: JSON Schema and [`Deserialize`] accept a document.
//! [`ErrorBody::new`], [`QueryColumn::new`], and [`crate::decimal::Position::new`]
//! apply the same rules to in-memory values. [`Serialize`] checks those rules
//! again and refuses to emit a body the door would reject. It is not a second,
//! unchecked door. [`parse_api_json`] re-checks an error envelope and the
//! nested columns and source positions of a query response after the typed parse.

use std::any::Any;
use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::openapi::schema::{ObjectBuilder, OneOfBuilder, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::config::model::ConsistencyMode;
use crate::decimal::{ExactDecimal, Position};
use crate::limits::QUERY_CELL_MAX_CHARS;
use crate::rfc3339::Rfc3339Timestamp;
use crate::schema_util::{compile_schema, schema_object, scrub_endpoint_text};
use crate::text::{
    ErrorText, HexDigest, Label, OperationId, QualifiedTable, QueryId, QuerySql, SourceName,
    TableName, TimezoneName,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
pub enum ErrorCode {
    #[serde(rename = "UNAUTHENTICATED")]
    Unauthenticated,
    #[serde(rename = "SQL_PARSE_ERROR")]
    SqlParseError,
    #[serde(rename = "STATEMENT_NOT_ALLOWED")]
    StatementNotAllowed,
    #[serde(rename = "FUNCTION_NOT_ALLOWED")]
    FunctionNotAllowed,
    #[serde(rename = "RELATION_NOT_FOUND")]
    RelationNotFound,
    #[serde(rename = "COLUMN_NOT_ALLOWED")]
    ColumnNotAllowed,
    #[serde(rename = "UNQUALIFIED_RELATION")]
    UnqualifiedRelation,
    #[serde(rename = "COST_TOO_HIGH")]
    CostTooHigh,
    #[serde(rename = "TIMEOUT")]
    Timeout,
    #[serde(rename = "TABLE_PAUSED")]
    TablePaused,
    #[serde(rename = "QUERY_TOO_SHORT")]
    QueryTooShort,
    #[serde(rename = "ENUMERATION_REFUSED")]
    EnumerationRefused,
    #[serde(rename = "RATE_LIMITED")]
    RateLimited,
    #[serde(rename = "OVERLOADED")]
    Overloaded,
    #[serde(rename = "INTERNAL")]
    Internal,
    #[serde(rename = "AUDIT_UNAVAILABLE")]
    AuditUnavailable,
    #[serde(rename = "IDEMPOTENCY_CONFLICT")]
    IdempotencyConflict,
    #[serde(rename = "CONFLICT")]
    Conflict,
    #[serde(rename = "VERSION_UNSUPPORTED")]
    VersionUnsupported,
    #[serde(rename = "SCHEMA_INVALID")]
    SchemaInvalid,
}

impl ErrorCode {
    pub fn http_status(self) -> u16 {
        match self {
            Self::Unauthenticated => 401,
            Self::SqlParseError
            | Self::StatementNotAllowed
            | Self::FunctionNotAllowed
            | Self::RelationNotFound
            | Self::ColumnNotAllowed
            | Self::UnqualifiedRelation
            | Self::QueryTooShort
            | Self::EnumerationRefused
            | Self::SchemaInvalid => 400,
            Self::CostTooHigh => 422,
            Self::Timeout => 408,
            Self::TablePaused | Self::IdempotencyConflict | Self::Conflict => 409,
            Self::RateLimited => 429,
            Self::Overloaded | Self::AuditUnavailable => 503,
            Self::Internal => 500,
            Self::VersionUnsupported => 400,
        }
    }

    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Overloaded | Self::Internal | Self::AuditUnavailable
        )
    }
}

/// `retryable` is fixed by [`ErrorCode::retryable`]. It is not an independent
/// field a caller can flip after a valid parse. Deserialization rejects a
/// body that disagrees. Serialization writes the code's value and refuses a
/// stored flag that does not match. HTTP `Retry-After` is not a field of this
/// object; a later HTTP implementation may set that header and is not invented
/// here.
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    code: ErrorCode,
    message: ErrorText,
    #[serde(default)]
    hint: Option<ErrorText>,
    retryable: bool,
    #[serde(default)]
    query_id: Option<QueryId>,
}

impl ErrorBody {
    pub fn new(
        code: ErrorCode,
        message: ErrorText,
        hint: Option<ErrorText>,
        query_id: Option<QueryId>,
    ) -> Self {
        Self {
            code,
            message,
            hint,
            retryable: code.retryable(),
            query_id,
        }
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn message(&self) -> &ErrorText {
        &self.message
    }

    pub fn hint(&self) -> Option<&ErrorText> {
        self.hint.as_ref()
    }

    /// The flag implied by [`ErrorCode::retryable`], not a separately stored choice.
    pub fn retryable(&self) -> bool {
        self.code.retryable()
    }

    pub fn query_id(&self) -> Option<&QueryId> {
        self.query_id.as_ref()
    }
}

impl Serialize for ErrorBody {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.retryable != self.code.retryable() {
            return Err(serde::ser::Error::custom(
                "retryable does not match the error code",
            ));
        }
        let mut state = serializer.serialize_struct("ErrorBody", 5)?;
        state.serialize_field("code", &self.code)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("hint", &self.hint)?;
        state.serialize_field("retryable", &self.code.retryable())?;
        state.serialize_field("query_id", &self.query_id)?;
        state.end()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorBodyDe {
    code: ErrorCode,
    message: ErrorText,
    #[serde(default)]
    hint: Option<ErrorText>,
    retryable: bool,
    #[serde(default)]
    query_id: Option<QueryId>,
}

impl TryFrom<ErrorBodyDe> for ErrorBody {
    type Error = String;

    fn try_from(raw: ErrorBodyDe) -> Result<Self, Self::Error> {
        if raw.retryable != raw.code.retryable() {
            return Err("retryable does not match the error code".to_owned());
        }
        Ok(Self {
            code: raw.code,
            message: raw.message,
            hint: raw.hint,
            retryable: raw.retryable,
            query_id: raw.query_id,
        })
    }
}

impl<'de> Deserialize<'de> for ErrorBody {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = ErrorBodyDe::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

impl ErrorEnvelope {
    pub fn consistent(&self) -> bool {
        self.error.retryable == self.error.code.retryable()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryFormat {
    Json,
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClientPurpose {
    Answer,
    Export,
    Resolve,
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClientContext {
    pub purpose: ClientPurpose,
    pub turn: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    pub sql: QuerySql,
    #[serde(default, deserialize_with = "option_query_rows")]
    #[schemars(range(min = 1, max = 2000))]
    pub max_rows: Option<u32>,
    #[serde(default, deserialize_with = "option_timeout")]
    #[schemars(range(min = 1, max = 120000))]
    pub timeout_ms: Option<u32>,
    pub format: QueryFormat,
    #[serde(default)]
    pub client_context: Option<ClientContext>,
}

/// Column metadata. Parameters are private so a struct literal cannot bypass
/// [`Self::new`]. Serialization runs the same check and returns an error
/// instead of emitting a type the JSON door rejects.
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryColumn {
    name: TableName,
    #[serde(rename = "type")]
    type_name: Label,
    #[serde(default)]
    precision: Option<u32>,
    #[serde(default)]
    scale: Option<i32>,
    #[serde(default)]
    timezone: Option<TimezoneName>,
}

impl QueryColumn {
    pub fn new(
        name: TableName,
        type_name: Label,
        precision: Option<u32>,
        scale: Option<i32>,
        timezone: Option<TimezoneName>,
    ) -> Result<Self, String> {
        validate_query_column(type_name.as_str(), precision, scale)?;
        Ok(Self {
            name,
            type_name,
            precision,
            scale,
            timezone,
        })
    }

    pub fn name(&self) -> &TableName {
        &self.name
    }

    pub fn type_name(&self) -> &Label {
        &self.type_name
    }

    pub fn precision(&self) -> Option<u32> {
        self.precision
    }

    pub fn scale(&self) -> Option<i32> {
        self.scale
    }

    pub fn timezone(&self) -> Option<&TimezoneName> {
        self.timezone.as_ref()
    }

    pub fn check(&self) -> Result<(), String> {
        validate_query_column(self.type_name.as_str(), self.precision, self.scale)
    }
}

impl Serialize for QueryColumn {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.check().map_err(serde::ser::Error::custom)?;
        let mut state = serializer.serialize_struct("QueryColumn", 5)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("type", &self.type_name)?;
        state.serialize_field("precision", &self.precision)?;
        state.serialize_field("scale", &self.scale)?;
        state.serialize_field("timezone", &self.timezone)?;
        state.end()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryColumnDe {
    name: TableName,
    #[serde(rename = "type")]
    type_name: Label,
    #[serde(default)]
    precision: Option<u32>,
    #[serde(default)]
    scale: Option<i32>,
    #[serde(default)]
    timezone: Option<TimezoneName>,
}

impl TryFrom<QueryColumnDe> for QueryColumn {
    type Error = String;

    fn try_from(raw: QueryColumnDe) -> Result<Self, Self::Error> {
        validate_query_column(raw.type_name.as_str(), raw.precision, raw.scale)?;
        Ok(Self {
            name: raw.name,
            type_name: raw.type_name,
            precision: raw.precision,
            scale: raw.scale,
            timezone: raw.timezone,
        })
    }
}

impl<'de> Deserialize<'de> for QueryColumn {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = QueryColumnDe::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

fn validate_query_column(
    type_name: &str,
    precision: Option<u32>,
    scale: Option<i32>,
) -> Result<(), String> {
    match type_name {
        "decimal128" => decimal_physical(precision, scale, 38, "decimal128"),
        "decimal256" => decimal_physical(precision, scale, 76, "decimal256"),
        "numeric" | "decimal" => pg_numeric(precision, scale),
        "real" | "float" | "double" | "float4" | "float8" | "float32" | "float64" => {
            if precision.is_some() || scale.is_some() {
                Err("floating type rejects precision and scale".to_owned())
            } else {
                Ok(())
            }
        }
        "uint8" | "uint16" | "uint32" | "uint64" | "unsigned" => {
            if precision.is_some() {
                return Err("unsigned integer rejects precision".to_owned());
            }
            match scale {
                None | Some(0) => Ok(()),
                Some(_) => Err("unsigned integer scale must be 0".to_owned()),
            }
        }
        "string" | "utf8" | "text" | "decimal_string" => overflow_metadata(precision, scale),
        _ => {
            if precision.is_some() || scale.is_some() {
                Err("this type does not accept precision or scale".to_owned())
            } else {
                Ok(())
            }
        }
    }
}

fn decimal_physical(
    precision: Option<u32>,
    scale: Option<i32>,
    max_precision: u32,
    name: &str,
) -> Result<(), String> {
    let Some(precision) = precision else {
        return Err(format!("{name} requires precision"));
    };
    let Some(scale) = scale else {
        return Err(format!("{name} requires scale"));
    };
    if !(1..=max_precision).contains(&precision) {
        return Err(format!("{name} precision must be 1..={max_precision}"));
    }
    let Ok(scale_i8) = i8::try_from(scale) else {
        return Err(format!(
            "{name} scale does not fit the physical i8 parameter"
        ));
    };
    // arrow-schema 60.0.0 `validate_decimal`: only a positive scale is compared
    // with precision. Scale -100 with precision 38 is valid. This is not
    // `ExactDecimal::fits_decimal128`, which bounds stored scale to -38..=38.
    if scale_i8 > 0 && scale_i8 as u8 > precision as u8 {
        return Err(format!(
            "{name} positive scale must be less than or equal to precision; a negative scale is valid"
        ));
    }
    Ok(())
}

fn pg_numeric(precision: Option<u32>, scale: Option<i32>) -> Result<(), String> {
    if let Some(precision) = precision
        && !(1..=1000).contains(&precision)
    {
        return Err("numeric precision must be 1..=1000 when present".to_owned());
    }
    if let Some(scale) = scale
        && !(-1000..=1000).contains(&scale)
    {
        return Err("numeric scale must be -1000..=1000 when present".to_owned());
    }
    Ok(())
}

fn overflow_metadata(precision: Option<u32>, scale: Option<i32>) -> Result<(), String> {
    if let Some(precision) = precision
        && !(1..=1000).contains(&precision)
    {
        return Err("overflow precision must be 1..=1000 when present".to_owned());
    }
    if let Some(scale) = scale
        && !(-1000..=1000).contains(&scale)
    {
        return Err("overflow scale must be -1000..=1000 when present".to_owned());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryCell {
    Null,
    Bool(bool),
    Text(String),
}

impl Serialize for QueryCell {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Text(value) => serializer.serialize_str(value),
        }
    }
}

impl<'de> Deserialize<'de> for QueryCell {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Null => Ok(Self::Null),
            serde_json::Value::Bool(item) => Ok(Self::Bool(item)),
            serde_json::Value::String(item)
                if !item.contains('\0') && item.chars().count() <= QUERY_CELL_MAX_CHARS =>
            {
                Ok(Self::Text(item))
            }
            serde_json::Value::String(_) => Err(serde::de::Error::custom(
                "query cell text exceeds its budget or contains NUL",
            )),
            serde_json::Value::Number(_) => Err(serde::de::Error::custom(
                "query cells reject JSON numbers; use a decimal string",
            )),
            _ => Err(serde::de::Error::custom(
                "query cell must be null, bool, or string",
            )),
        }
    }
}

impl JsonSchema for QueryCell {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("QueryCell")
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schema_object(serde_json::json!({
            "oneOf": [
                {"type": "null"},
                {"type": "boolean"},
                {"type": "string", "maxLength": QUERY_CELL_MAX_CHARS}
            ]
        }))
    }
}

impl PartialSchema for QueryCell {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        OneOfBuilder::new()
            .item(ObjectBuilder::new().schema_type(Type::Null))
            .item(ObjectBuilder::new().schema_type(Type::Boolean))
            .item(
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .max_length(Some(QUERY_CELL_MAX_CHARS)),
            )
            .into()
    }
}

impl ToSchema for QueryCell {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TruncationReason {
    Rows,
    Bytes,
    Cell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState {
    Live,
    Stale,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Freshness {
    pub table: QualifiedTable,
    pub as_of: Rfc3339Timestamp,
    pub lag_ms: u64,
    pub state: FreshnessState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceWatermark {
    pub source: SourceName,
    pub consistency: ConsistencyMode,
    pub cross_table_consistent: bool,
    pub applied_pos: Position,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
pub enum WarningCode {
    #[serde(rename = "RECONCILE_MISMATCH")]
    ReconcileMismatch,
    #[serde(rename = "RECONCILE_STALE")]
    ReconcileStale,
    #[serde(rename = "VALUES_REJECTED")]
    ValuesRejected,
    #[serde(rename = "UNEXPLAINED_CODES")]
    UnexplainedCodes,
    #[serde(rename = "PARTIAL_TRANSACTION")]
    PartialTransaction,
    #[serde(rename = "CROSS_TABLE_SKEW")]
    CrossTableSkew,
    #[serde(rename = "ENTITY_CHECK_FAILED")]
    EntityCheckFailed,
    #[serde(rename = "STALE_DATA")]
    StaleData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Warning {
    pub code: WarningCode,
    #[serde(default)]
    pub table: Option<QualifiedTable>,
    #[serde(default)]
    pub column: Option<TableName>,
    #[serde(default)]
    pub impact: Option<Label>,
    pub message: ErrorText,
    /// Row count from the CLI example. It is not an amount.
    #[serde(default)]
    pub rows: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryResponse {
    pub query_id: QueryId,
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Vec<QueryCell>>,
    pub returned_rows: u64,
    pub truncated: bool,
    #[serde(default)]
    pub truncation_reasons: Vec<TruncationReason>,
    pub duration_ms: u64,
    pub freshness: Vec<Freshness>,
    pub sources: Vec<SourceWatermark>,
    pub warnings: Vec<Warning>,
    #[serde(default)]
    pub star_expanded: Option<Vec<QualifiedTable>>,
    pub knowledge_version: Label,
    pub policy_version: Label,
    #[serde(default)]
    pub hints: Vec<ErrorText>,
    #[serde(default)]
    pub estimated_total_rows: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Accepted,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptedOperation {
    pub operation_id: OperationId,
    pub status: OperationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdempotencyDecision {
    FirstAccept,
    Replay,
    Conflict,
}

pub fn idempotency_decision(
    stored_sha256: Option<&str>,
    incoming_sha256: &str,
) -> IdempotencyDecision {
    match stored_sha256 {
        None => IdempotencyDecision::FirstAccept,
        Some(stored) if stored == incoming_sha256 => IdempotencyDecision::Replay,
        Some(_) => IdempotencyDecision::Conflict,
    }
}

pub fn payload_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = crate::canonical::hash_bytes(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    #[serde(default)]
    pub cursor: Option<Label>,
    #[serde(deserialize_with = "page_limit")]
    #[schemars(range(min = 1, max = 200))]
    pub limit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportStatus {
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportCompletion {
    pub status: ExportStatus,
    pub rows: ExactDecimal,
    pub bytes: ExactDecimal,
    pub truncated: bool,
    pub result_sha256: HexDigest,
    pub freshness: Vec<Freshness>,
    pub warnings: Vec<Warning>,
}

fn option_query_rows<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    bounded_option(deserializer, 1, 2000)
}

fn option_timeout<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    bounded_option(deserializer, 1, 120_000)
}

fn page_limit<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    if (1..=200).contains(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("limit must be in 1..=200"))
    }
}

pub fn parse_api_json<T>(
    input: &str,
    validate: fn(&T) -> Vec<crate::diagnostic::Diagnostic>,
) -> Result<T, crate::diagnostic::DiagnosticList>
where
    T: serde::de::DeserializeOwned + JsonSchema + 'static,
{
    use crate::diagnostic::{CONTRACT_API, Diagnostic, DiagnosticList};
    use crate::json_scan::parse_json_value;
    let value = parse_json_value(input, CONTRACT_API)?;
    let mut errors = Vec::new();
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
        .to_value();
    match compile_schema(&schema) {
        Ok(validator) => {
            for error in validator.iter_errors(&value) {
                let path = error.instance_path().to_string();
                let path = if path.is_empty() {
                    "/".to_owned()
                } else {
                    path
                };
                errors.push(Diagnostic::error(
                    CONTRACT_API,
                    "schema",
                    path,
                    scrub_endpoint_text(&error.to_string()),
                ));
            }
        }
        Err(error) => {
            errors.push(Diagnostic::error(
                CONTRACT_API,
                "schema",
                "/",
                format!(
                    "generated schema could not be compiled: {}",
                    scrub_endpoint_text(&error.to_string())
                ),
            ));
        }
    }
    if !errors.is_empty() {
        return Err(DiagnosticList::new(errors));
    }
    let typed = serde_json::from_value::<T>(value).map_err(|error| {
        DiagnosticList::one(Diagnostic::error(
            CONTRACT_API,
            "schema",
            "/",
            scrub_endpoint_text(&format!(
                "schema accepted a document serde rejected: {error}"
            )),
        ))
    })?;
    audit_api_value(&typed, &mut errors);
    errors.extend(validate(&typed));
    if errors.is_empty() {
        Ok(typed)
    } else {
        Err(DiagnosticList::new(errors))
    }
}

fn audit_api_value<T: Any>(typed: &T, errors: &mut Vec<crate::diagnostic::Diagnostic>) {
    use crate::diagnostic::{CONTRACT_API, Diagnostic};
    if let Some(envelope) = (typed as &dyn Any).downcast_ref::<ErrorEnvelope>()
        && !envelope.consistent()
    {
        errors.push(Diagnostic::error(
            CONTRACT_API,
            "cross_field",
            "/error/retryable",
            "retryable does not match the error code",
        ));
    }
    if let Some(response) = (typed as &dyn Any).downcast_ref::<QueryResponse>() {
        for (index, column) in response.columns.iter().enumerate() {
            if let Err(message) = column.check() {
                errors.push(Diagnostic::error(
                    CONTRACT_API,
                    "cross_field",
                    format!("/columns/{index}"),
                    message,
                ));
            }
        }
        for (index, source) in response.sources.iter().enumerate() {
            if source.applied_pos.check().is_err() {
                errors.push(Diagnostic::error(
                    CONTRACT_API,
                    "cross_field",
                    format!("/sources/{index}/applied_pos"),
                    "applied_pos must be a non-negative scale-0 decimal of at most 38 digits",
                ));
            }
        }
    }
}

fn bounded_option<'de, D: Deserializer<'de>>(
    deserializer: D,
    min: u32,
    max: u32,
) -> Result<Option<u32>, D::Error> {
    match Option::<u32>::deserialize(deserializer)? {
        None => Ok(None),
        Some(value) if (min..=max).contains(&value) => Ok(Some(value)),
        Some(_) => Err(serde::de::Error::custom(
            "integer is outside its accepted range",
        )),
    }
}
