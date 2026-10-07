//! Closed request document. Parse does not stat a file.

use std::path::{Path, PathBuf};

use oo_connectors::{
    BomPolicy, DelimitedColumn, DelimitedDialect, DelimitedProfile, DelimitedType, HeaderPolicy,
    JsonlColumn, JsonlProfile, JsonlType, NullPolicy, PG17_RESERVED, PullBudget, SourceProfile,
};
use oo_core::parse_json_value;
use oo_pipeline::physical_mapping::{plan_physical_mapping, Level, MappingRequest};
use serde_json::{Map, Value};

use crate::digest::{config_preimage, digest_text};
use crate::{decode_sha256, ImportError, REQUEST_PROTOCOL};

const PATH_MAX: usize = 4096;
const BANNED: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "authorization",
    "credential",
    "private_key",
    "ca_pem",
    "ca_body",
    "api_key",
];

/// One approved column. `precision` and `scale` are set only for decimal.
#[derive(Clone, PartialEq, Eq)]
pub struct ColumnSpec {
    name: String,
    parse: ParseKind,
    pg: PgKind,
    nullable: bool,
    primary_key: bool,
    precision: Option<u8>,
    scale: Option<i8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParseKind {
    Int64,
    Bool,
    Utf8,
    Decimal,
    JsonText,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PgKind {
    Int8,
    Bool,
    Text,
    Json,
    Numeric,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FormatKind {
    Csv,
    Tsv,
    Jsonl,
}

impl ColumnSpec {
    /// Admit one parse/pg pair. A primary key is not nullable.
    pub fn try_new(
        name: &str,
        parse: &str,
        pg: &str,
        nullable: bool,
        primary_key: bool,
        precision: Option<u8>,
        scale: Option<i8>,
    ) -> Result<Self, ImportError> {
        if !pgident_ok(name) {
            return Err(ImportError::new("closed_input", "column"));
        }
        if primary_key && nullable {
            return Err(ImportError::new("closed_input", "primary_key"));
        }
        let (parse_kind, pg_kind) = match (parse, pg) {
            ("int64", "int8") => (ParseKind::Int64, PgKind::Int8),
            ("bool", "bool") => (ParseKind::Bool, PgKind::Bool),
            ("utf8", "text") => (ParseKind::Utf8, PgKind::Text),
            ("utf8", "json") => (ParseKind::Utf8, PgKind::Json),
            ("decimal", "numeric") => (ParseKind::Decimal, PgKind::Numeric),
            ("json_text", "json") => (ParseKind::JsonText, PgKind::Json),
            _ => return Err(ImportError::new("closed_input", "type_pair")),
        };
        match parse_kind {
            ParseKind::Decimal => {
                let precision = precision.ok_or(ImportError::new("closed_input", "precision"))?;
                let scale = scale.ok_or(ImportError::new("closed_input", "scale"))?;
                if !(1..=38).contains(&precision) || scale < 0 || u8::try_from(scale).unwrap_or(255) > precision {
                    return Err(ImportError::new("unsupported_scale", "scale"));
                }
            }
            _ => {
                if precision.is_some() || scale.is_some() {
                    return Err(ImportError::new("closed_input", "precision"));
                }
            }
        }
        Ok(Self {
            name: name.to_string(),
            parse: parse_kind,
            pg: pg_kind,
            nullable,
            primary_key,
            precision,
            scale,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn nullable(&self) -> bool {
        self.nullable
    }

    pub fn primary_key(&self) -> bool {
        self.primary_key
    }

    pub fn parse_name(&self) -> &'static str {
        match self.parse {
            ParseKind::Int64 => "int64",
            ParseKind::Bool => "bool",
            ParseKind::Utf8 => "utf8",
            ParseKind::Decimal => "decimal",
            ParseKind::JsonText => "json_text",
        }
    }

    pub fn pg_name(&self) -> &'static str {
        match self.pg {
            PgKind::Int8 => "int8",
            PgKind::Bool => "bool",
            PgKind::Text => "text",
            PgKind::Json => "json",
            PgKind::Numeric => "numeric",
        }
    }

    pub fn oid(&self) -> i64 {
        match self.pg {
            PgKind::Bool => 16,
            PgKind::Int8 => 20,
            PgKind::Text => 25,
            PgKind::Json => 114,
            PgKind::Numeric => 1700,
        }
    }

    /// Numeric typmod is `((precision << 16) | scale) + 4`. Other oids use `-1`.
    pub fn typmod(&self) -> i32 {
        match (self.precision, self.scale) {
            (Some(precision), Some(scale)) if self.pg == PgKind::Numeric => {
                ((i32::from(precision) << 16) | i32::from(scale)) + 4
            }
            _ => -1,
        }
    }

    pub fn precision_word(&self) -> String {
        match self.precision {
            Some(value) => value.to_string(),
            None => "-".to_string(),
        }
    }

    pub fn scale_word(&self) -> String {
        match self.scale {
            Some(value) => value.to_string(),
            None => "-".to_string(),
        }
    }

    pub(crate) fn precision(&self) -> Option<u8> {
        self.precision
    }

    pub(crate) fn scale(&self) -> Option<i8> {
        self.scale
    }

    pub(crate) fn is_json(&self) -> bool {
        self.pg == PgKind::Json
    }

    pub(crate) fn is_utf8(&self) -> bool {
        matches!(self.parse, ParseKind::Utf8 | ParseKind::JsonText)
    }

    pub(crate) fn parse_kind_json_text(&self) -> bool {
        self.parse == ParseKind::JsonText
    }

    pub(crate) fn parse_kind_decimal(&self) -> bool {
        self.parse == ParseKind::Decimal
    }
}

/// Admitted request. The password is not a field.
#[derive(Clone)]
pub struct ImportRequest {
    object_path: PathBuf,
    object_sha256: [u8; 32],
    object_sha_hex: String,
    worker_path: PathBuf,
    worker_sha256: [u8; 32],
    profile: SourceProfile,
    budget: PullBudget,
    columns: Vec<ColumnSpec>,
    format: FormatKind,
    locale: String,
    null_line: String,
    bom_line: String,
    header_line: String,
    profile_id: String,
    source: String,
    namespace: String,
    table: String,
    shard: String,
    host: String,
    port: u16,
    database: String,
    user: String,
    ca_path: PathBuf,
    parent_deadline_ms: u64,
    max_total_bytes: u64,
}

/// Names `plan_physical_mapping` derives at source generation 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalNames {
    /// Physical namespace.
    pub namespace: String,
    /// Physical shadow table.
    pub table: String,
    /// Physical columns in profile order.
    pub columns: Vec<String>,
}

impl ImportRequest {
    pub fn locale(&self) -> &str {
        &self.locale
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn format_name(&self) -> &'static str {
        match self.format {
            FormatKind::Csv => "csv",
            FormatKind::Tsv => "tsv",
            FormatKind::Jsonl => "jsonl",
        }
    }

    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    pub fn bom_line(&self) -> &str {
        &self.bom_line
    }

    pub fn header_line(&self) -> &str {
        &self.header_line
    }

    pub fn null_line(&self) -> &str {
        &self.null_line
    }

    pub fn columns(&self) -> &[ColumnSpec] {
        &self.columns
    }

    pub(crate) fn object_path(&self) -> &Path {
        &self.object_path
    }

    pub(crate) fn object_sha256(&self) -> [u8; 32] {
        self.object_sha256
    }

    pub(crate) fn object_sha_hex(&self) -> &str {
        &self.object_sha_hex
    }

    pub(crate) fn worker_path(&self) -> &Path {
        &self.worker_path
    }

    pub(crate) fn worker_sha256(&self) -> [u8; 32] {
        self.worker_sha256
    }

    pub(crate) fn profile(&self) -> &SourceProfile {
        &self.profile
    }

    pub(crate) fn budget(&self) -> &PullBudget {
        &self.budget
    }

    pub(crate) fn shard(&self) -> &str {
        &self.shard
    }

    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn database(&self) -> &str {
        &self.database
    }

    pub(crate) fn user(&self) -> &str {
        &self.user
    }

    pub(crate) fn ca_path(&self) -> &Path {
        &self.ca_path
    }

    pub(crate) fn parent_deadline_ms(&self) -> u64 {
        self.parent_deadline_ms
    }

    pub(crate) fn max_total_bytes(&self) -> u64 {
        self.max_total_bytes
    }
}

/// Parse one request document. This does not read the object or the worker.
pub fn parse_request(text: &str) -> Result<ImportRequest, ImportError> {
    let value = parse_json_value(text, REQUEST_PROTOCOL).map_err(|_| ImportError::new("closed_input", "json"))?;
    walk_credentials(&value, "")?;
    let root = object(
        &value,
        &[
            "protocol",
            "strategy",
            "parsing_mode",
            "locale",
            "type_policy",
            "object",
            "worker",
            "profile",
            "budget",
            "target",
            "fence",
            "mapping",
            "source",
        ],
    )?;
    let protocol = require_str(root, "protocol")?;
    if protocol != REQUEST_PROTOCOL {
        return Err(ImportError::new("closed_input", "protocol"));
    }
    match require_str(root, "strategy")? {
        "replace" => {}
        "append" | "upsert" => return Err(ImportError::new("unsupported_strategy", "strategy")),
        _ => return Err(ImportError::new("closed_input", "strategy")),
    }
    match require_str(root, "parsing_mode")? {
        "trusted_public_fixture" => {}
        "production_untrusted" | "runsc" => return Err(ImportError::new("unsupported_mode", "mode")),
        _ => return Err(ImportError::new("closed_input", "mode")),
    }
    let locale = require_str(root, "locale")?;
    if locale != "C" && locale != "C.UTF-8" {
        return Err(ImportError::new("closed_input", "locale"));
    }
    if require_str(root, "type_policy")? != "preserve_row" {
        return Err(ImportError::new("closed_input", "type_policy"));
    }
    let source = require_str(root, "source")?;
    if !source_ok(source) {
        return Err(ImportError::new("closed_input", "source"));
    }
    let object = object(required(root, "object")?, &["sha256", "path"])?;
    let object_sha_hex = require_str(object, "sha256")?;
    let object_sha256 = decode_sha256(object_sha_hex)?;
    let object_path = absolute_path(require_str(object, "path")?)?;
    let worker = object(required(root, "worker")?, &["sha256", "path"])?;
    let worker_sha256 = decode_sha256(require_str(worker, "sha256")?)?;
    let worker_path = absolute_path(require_str(worker, "path")?)?;
    let (format, profile, columns, null_line, bom_line, header_line, profile_id) =
        parse_profile(required(root, "profile")?)?;
    let (budget, parent_deadline_ms, max_total_bytes) = parse_budget(required(root, "budget")?, format)?;
    let target = object(
        required(root, "target")?,
        &["host", "port", "database", "user", "server_name", "ca_path"],
    )?;
    let host = require_str(target, "host")?;
    let server_name = require_str(target, "server_name")?;
    if host != server_name || host.is_empty() || host.contains('\0') {
        return Err(ImportError::new("closed_input", "host"));
    }
    let port = parse_port(required(target, "port")?)?;
    let database = require_str(target, "database")?;
    let user = require_str(target, "user")?;
    if !pgident_ok(database) || !pgident_ok(user) {
        return Err(ImportError::new("closed_input", "principal"));
    }
    let ca_path = absolute_path(require_str(target, "ca_path")?)?;
    let fence = object(required(root, "fence")?, &["shard"])?;
    let shard = require_str(fence, "shard")?;
    if !shard_ok(shard) {
        return Err(ImportError::new("closed_input", "shard"));
    }
    let mapping = object(required(root, "mapping")?, &["namespace", "table"])?;
    let namespace = require_str(mapping, "namespace")?;
    let table = require_str(mapping, "table")?;
    if !pgident_ok(namespace) || !pgident_ok(table) {
        return Err(ImportError::new("closed_input", "mapping"));
    }
    Ok(ImportRequest {
        object_path,
        object_sha256,
        object_sha_hex: object_sha_hex.to_string(),
        worker_path,
        worker_sha256,
        profile,
        budget,
        columns,
        format,
        locale: locale.to_string(),
        null_line,
        bom_line,
        header_line,
        profile_id,
        source: source.to_string(),
        namespace: namespace.to_string(),
        table: table.to_string(),
        shard: shard.to_string(),
        host: host.to_string(),
        port,
        database: database.to_string(),
        user: user.to_string(),
        ca_path,
        parent_deadline_ms,
        max_total_bytes,
    })
}

/// Config digest. Object bytes and paths are not inputs.
pub fn config_digest(request: &ImportRequest) -> String {
    digest_text(&config_preimage(request))
}

/// Physical names for this config. The object hash is not an input.
pub fn physical_plan(request: &ImportRequest) -> Result<PhysicalNames, ImportError> {
    let digest = config_digest(request);
    let namespace = plan_one(
        request.source(),
        &digest,
        Level::Namespace,
        request.namespace(),
        "",
        "",
    )?;
    let table = plan_one(
        request.source(),
        &digest,
        Level::Table,
        request.namespace(),
        request.table(),
        "",
    )?;
    let mut columns = Vec::with_capacity(request.columns().len());
    for column in request.columns() {
        let planned = plan_one(
            request.source(),
            &digest,
            Level::Column,
            request.namespace(),
            request.table(),
            column.name(),
        )?;
        columns.push(planned);
    }
    Ok(PhysicalNames {
        namespace,
        table,
        columns,
    })
}

fn plan_one(
    source: &str,
    digest: &str,
    level: Level,
    namespace: &str,
    table: &str,
    column: &str,
) -> Result<String, ImportError> {
    let request = MappingRequest {
        source: source.to_string(),
        config_digest: digest.to_string(),
        run_generation: 1,
        level,
        logical_namespace: namespace.to_string(),
        logical_table: table.to_string(),
        logical_column: column.to_string(),
    };
    plan_physical_mapping(&request)
        .map(|plan| plan.physical_name)
        .map_err(|_| ImportError::new("closed_input", "mapping"))
}

fn parse_profile(
    value: &Value,
) -> Result<(FormatKind, SourceProfile, Vec<ColumnSpec>, String, String, String, String), ImportError> {
    let map = value.as_object().ok_or(ImportError::new("closed_input", "json"))?;
    let format_text = require_str(map, "format")?;
    let format = match format_text {
        "csv" => FormatKind::Csv,
        "tsv" => FormatKind::Tsv,
        "jsonl" => FormatKind::Jsonl,
        "parquet" | "excel" | "xlsx" | "sqlite" | "sql_dump" | "dump" => {
            return Err(ImportError::new("unsupported_format", "format"));
        }
        _ => return Err(ImportError::new("closed_input", "format")),
    };
    let allowed: &[&str] = match format {
        FormatKind::Jsonl => &["format", "id", "null", "columns"],
        FormatKind::Csv | FormatKind::Tsv => &["format", "id", "bom", "header", "null", "columns"],
    };
    let map = object(value, allowed)?;
    let profile_id = require_str(map, "id")?.to_string();
    let columns = parse_columns(required(map, "columns")?, format)?;
    if !columns.iter().any(ColumnSpec::primary_key) {
        return Err(ImportError::new("closed_input", "primary_key"));
    }
    let (null_policy, null_line) = parse_null(required(map, "null")?, format)?;
    match format {
        FormatKind::Jsonl => {
            let profile = JsonlProfile::try_new(&profile_id, jsonl_columns(&columns)?)
                .map_err(|error| ImportError::new("closed_input", error.fact()))?;
            Ok((
                format,
                SourceProfile::Jsonl(profile),
                columns,
                null_line,
                "none".to_string(),
                "none".to_string(),
                profile_id,
            ))
        }
        FormatKind::Csv | FormatKind::Tsv => {
            let bom = match require_str(map, "bom")? {
                "refuse" => BomPolicy::Refuse,
                "strip" => BomPolicy::Strip,
                "require" => BomPolicy::Require,
                _ => return Err(ImportError::new("closed_input", "bom")),
            };
            let header = match require_str(map, "header")? {
                "absent" => HeaderPolicy::Absent,
                "exact" => HeaderPolicy::Exact,
                _ => return Err(ImportError::new("closed_input", "header")),
            };
            let dialect = if format == FormatKind::Tsv {
                DelimitedDialect::Tsv
            } else {
                DelimitedDialect::Csv
            };
            let profile = DelimitedProfile::try_new(
                &profile_id,
                dialect,
                bom,
                header,
                null_policy,
                delimited_columns(&columns)?,
            )
            .map_err(|error| ImportError::new("closed_input", error.fact()))?;
            Ok((
                format,
                SourceProfile::Delimited(profile),
                columns,
                null_line,
                bom.as_str().to_string(),
                header.as_str().to_string(),
                profile_id,
            ))
        }
    }
}

fn parse_columns(value: &Value, format: FormatKind) -> Result<Vec<ColumnSpec>, ImportError> {
    let items = value.as_array().ok_or(ImportError::new("closed_input", "columns"))?;
    if items.is_empty() {
        return Err(ImportError::new("closed_input", "columns"));
    }
    let mut columns = Vec::with_capacity(items.len());
    for item in items {
        let map = object(
            item,
            &["name", "parse", "pg", "nullable", "primary_key", "precision", "scale"],
        )?;
        let precision = optional_u8(required(map, "precision")?)?;
        let scale = optional_i8(required(map, "scale")?)?;
        let column = ColumnSpec::try_new(
            require_str(map, "name")?,
            require_str(map, "parse")?,
            require_str(map, "pg")?,
            require_bool(map, "nullable")?,
            require_bool(map, "primary_key")?,
            precision,
            scale,
        )?;
        if column.parse_kind_decimal() && format == FormatKind::Jsonl {
            return Err(ImportError::new("closed_input", "type_pair"));
        }
        if column.parse_kind_json_text() && format != FormatKind::Jsonl {
            return Err(ImportError::new("closed_input", "type_pair"));
        }
        if columns.iter().any(|other| other.name() == column.name()) {
            return Err(ImportError::new("closed_input", "column"));
        }
        columns.push(column);
    }
    Ok(columns)
}

fn delimited_columns(columns: &[ColumnSpec]) -> Result<Vec<DelimitedColumn>, ImportError> {
    let mut out = Vec::with_capacity(columns.len());
    for column in columns {
        let ty = match column.parse {
            ParseKind::Utf8 => DelimitedType::Utf8,
            ParseKind::Bool => DelimitedType::Bool,
            ParseKind::Int64 => DelimitedType::Int64,
            ParseKind::Decimal => DelimitedType::Decimal {
                precision: column.precision.ok_or(ImportError::new("closed_input", "precision"))?,
                scale: column.scale.ok_or(ImportError::new("closed_input", "scale"))?,
            },
            ParseKind::JsonText => return Err(ImportError::new("closed_input", "type_pair")),
        };
        let built = DelimitedColumn::try_new(column.name(), ty, column.nullable())
            .map_err(|error| ImportError::new("closed_input", error.fact()))?;
        out.push(built);
    }
    Ok(out)
}

fn jsonl_columns(columns: &[ColumnSpec]) -> Result<Vec<JsonlColumn>, ImportError> {
    let mut out = Vec::with_capacity(columns.len());
    for column in columns {
        let ty = match column.parse {
            ParseKind::Utf8 => JsonlType::Utf8,
            ParseKind::Bool => JsonlType::Bool,
            ParseKind::Int64 => JsonlType::Int64,
            ParseKind::JsonText => JsonlType::JsonText,
            ParseKind::Decimal => return Err(ImportError::new("closed_input", "type_pair")),
        };
        let built = JsonlColumn::try_new(column.name(), ty, column.nullable())
            .map_err(|error| ImportError::new("closed_input", error.fact()))?;
        out.push(built);
    }
    Ok(out)
}

fn parse_null(value: &Value, format: FormatKind) -> Result<(NullPolicy, String), ImportError> {
    let map = value.as_object().ok_or(ImportError::new("closed_input", "null"))?;
    if map.keys().any(|key| key != "policy" && key != "token") {
        return Err(ImportError::new("closed_input", "unknown_field"));
    }
    let policy = require_str(map, "policy")?;
    match (format, policy) {
        (FormatKind::Jsonl, "json") => {
            if map.contains_key("token") {
                return Err(ImportError::new("closed_input", "unknown_field"));
            }
            Ok((NullPolicy::Empty, "json".to_string()))
        }
        (FormatKind::Csv | FormatKind::Tsv, "empty") => {
            if map.contains_key("token") {
                return Err(ImportError::new("closed_input", "unknown_field"));
            }
            Ok((NullPolicy::Empty, "empty".to_string()))
        }
        (FormatKind::Csv | FormatKind::Tsv, "token") => {
            let token = require_str(map, "token")?.to_string();
            if token.is_empty() {
                return Err(ImportError::new("closed_input", "null_token"));
            }
            Ok((NullPolicy::Token(token.clone()), format!("token:{token}")))
        }
        (FormatKind::Jsonl, "empty" | "token") | (_, "json") => {
            Err(ImportError::new("closed_input", "null"))
        }
        _ => Err(ImportError::new("closed_input", "null")),
    }
}

fn parse_budget(value: &Value, format: FormatKind) -> Result<(PullBudget, u64, u64), ImportError> {
    let map = object(
        value,
        &[
            "batch_rows",
            "batch_bytes",
            "max_record_bytes",
            "max_field_bytes",
            "max_total_bytes",
            "max_rows",
            "max_columns",
            "max_fields",
            "worker_deadline_ms",
            "parent_deadline_ms",
        ],
    )?;
    let batch_rows = require_u32(map, "batch_rows")?;
    let batch_bytes = require_u64(map, "batch_bytes")?;
    let max_record_bytes = require_u32(map, "max_record_bytes")?;
    let max_field_bytes = require_u32(map, "max_field_bytes")?;
    let max_total_bytes = require_u64(map, "max_total_bytes")?;
    let max_rows = require_u64(map, "max_rows")?;
    let max_columns = require_u32(map, "max_columns")?;
    let max_fields = require_u32(map, "max_fields")?;
    let worker_deadline_ms = require_u64(map, "worker_deadline_ms")?;
    let parent_deadline_ms = require_u64(map, "parent_deadline_ms")?;
    let budget = match format {
        FormatKind::Jsonl => PullBudget::jsonl(
            batch_rows,
            batch_bytes,
            max_record_bytes,
            max_total_bytes,
            max_rows,
            max_columns,
            max_fields,
            worker_deadline_ms,
        ),
        FormatKind::Csv | FormatKind::Tsv => PullBudget::delimited(
            batch_rows,
            batch_bytes,
            max_record_bytes,
            max_field_bytes,
            max_total_bytes,
            max_rows,
            max_columns,
            max_fields,
            worker_deadline_ms,
        ),
    }
    .map_err(|error| match error {
        oo_connectors::ParentError::Config(token) => ImportError::new("closed_input", token),
        oo_connectors::ParentError::Failed(_) => ImportError::new("closed_input", "budget"),
    })?;
    if !(1..=120_000).contains(&parent_deadline_ms) {
        return Err(ImportError::new("closed_input", "deadline"));
    }
    Ok((budget, parent_deadline_ms, max_total_bytes))
}

fn parse_port(value: &Value) -> Result<u16, ImportError> {
    let number = value.as_u64().ok_or(ImportError::new("closed_input", "number"))?;
    let port = u16::try_from(number).map_err(|_| ImportError::new("closed_input", "port"))?;
    if port == 0 {
        return Err(ImportError::new("closed_input", "port"));
    }
    Ok(port)
}

fn walk_credentials(value: &Value, path: &str) -> Result<(), ImportError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if BANNED.contains(&key.as_str()) || (key == "token" && path != "profile.null") {
                    return Err(ImportError::new("credential", "credential"));
                }
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                walk_credentials(child, &child_path)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                walk_credentials(item, path)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, ImportError> {
    let map = value.as_object().ok_or(ImportError::new("closed_input", "json"))?;
    for key in map.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(ImportError::new("closed_input", "unknown_field"));
        }
    }
    for key in keys {
        if !map.contains_key(*key) {
            return Err(ImportError::new("closed_input", "missing_key"));
        }
    }
    Ok(map)
}

fn required<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, ImportError> {
    map.get(key).ok_or(ImportError::new("closed_input", "missing_key"))
}

fn require_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, ImportError> {
    match required(map, key)? {
        Value::String(text) => Ok(text.as_str()),
        Value::Number(_) => Err(ImportError::new("closed_input", "number")),
        _ => Err(ImportError::new("closed_input", "json")),
    }
}

fn require_bool(map: &Map<String, Value>, key: &str) -> Result<bool, ImportError> {
    match required(map, key)? {
        Value::Bool(value) => Ok(*value),
        _ => Err(ImportError::new("closed_input", "json")),
    }
}

fn require_u64(map: &Map<String, Value>, key: &str) -> Result<u64, ImportError> {
    match required(map, key)? {
        Value::Number(number) => number.as_u64().ok_or(ImportError::new("closed_input", "number")),
        _ => Err(ImportError::new("closed_input", "number")),
    }
}

fn require_u32(map: &Map<String, Value>, key: &str) -> Result<u32, ImportError> {
    u32::try_from(require_u64(map, key)?).map_err(|_| ImportError::new("closed_input", "number"))
}

fn optional_u8(value: &Value) -> Result<Option<u8>, ImportError> {
    match value {
        Value::Null => Ok(None),
        Value::Number(number) => {
            let wide = number.as_u64().ok_or(ImportError::new("closed_input", "number"))?;
            u8::try_from(wide)
                .map(Some)
                .map_err(|_| ImportError::new("closed_input", "precision"))
        }
        _ => Err(ImportError::new("closed_input", "precision")),
    }
}

fn optional_i8(value: &Value) -> Result<Option<i8>, ImportError> {
    match value {
        Value::Null => Ok(None),
        Value::Number(number) => {
            let wide = number.as_i64().ok_or(ImportError::new("closed_input", "number"))?;
            i8::try_from(wide)
                .map(Some)
                .map_err(|_| ImportError::new("closed_input", "scale"))
        }
        _ => Err(ImportError::new("closed_input", "scale")),
    }
}

fn absolute_path(text: &str) -> Result<PathBuf, ImportError> {
    if !valid_absolute(text) {
        return Err(ImportError::new("closed_input", "path_form"));
    }
    Ok(PathBuf::from(text))
}

fn valid_absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > PATH_MAX || bytes.contains(&0) || !bytes.starts_with(b"/") || bytes.ends_with(b"/") {
        return false;
    }
    text.split('/').skip(1).all(|part| !part.is_empty() && part != "." && part != "..")
}

fn source_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn shard_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'_' | b'.' | b':' | b'-')
    })
}

fn pgident_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 || bytes.starts_with(b"q_") {
        return false;
    }
    let first = bytes[0];
    if first != b'_' && !first.is_ascii_lowercase() {
        return false;
    }
    if !bytes[1..]
        .iter()
        .all(|byte| *byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return false;
    }
    PG17_RESERVED.binary_search(&name).is_err()
}
