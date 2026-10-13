//! `oo.pipeline.sealed-import-catalog/v1` as a compiler input.
//!
//! The document is the stdout of a sealed import. Its `engine` is the source
//! engine. Target shadow names are checked and then dropped. This module does
//! not open a database, accept source SQL, or relabel the document as
//! `oo.source.pgsql/v1`.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::api::payload_sha256;
use crate::diagnostic::DiagnosticList;
use crate::json_scan::parse_json_value;

/// Producer stdout protocol. This is not `oo.source.pgsql/v1`.
pub const SEALED_IMPORT_CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";

/// Compiler failure protocol for a sealed import document.
pub const SEALED_IMPORT_DRAFT_PROTOCOL: &str = "oo.builder.sealed-import-draft/v1";

const NORMALIZED_REPRESENTATION: &str = "postgresql_target_storage";

/// Static failure. Messages do not echo catalog text, endpoints, or secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportError {
    pub code: String,
    pub path: String,
    pub message: &'static str,
}

impl SealedImportError {
    pub fn new(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{SEALED_IMPORT_DRAFT_PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }
}

impl fmt::Display for SealedImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}: {}",
            self.code, self.path, self.message
        )
    }
}

impl std::error::Error for SealedImportError {}

/// One validated sealed import. Shadow names are not retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportCatalog {
    pub capture: String,
    pub engine: String,
    pub source_id: String,
    pub config_digest: String,
    pub input_digest: String,
    pub type_digest: String,
    pub schema_digest: String,
    pub tool_hook_digest: String,
    pub run_id: String,
    pub source_generation: i64,
    pub target_generation: i64,
    pub tables: Vec<SealedImportTable>,
    /// A producer-exported, full materialized C4 partition vector.
    /// Each member retains its own acknowledged run and generation.
    pub partition_vector: Option<Vec<SealedImportPartition>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedImportPartition {
    pub partition_id: String,
    pub table_epoch: String,
    pub schema_fingerprint: String,
    pub current_signature: Option<String>,
    pub previous_ack_exists: bool,
    pub previous_signature: Option<String>,
    pub partition_scope: SealedImportPartitionScope,
    pub audit_digest: String,
    pub consistency_grade: String,
    pub source_generation: i64,
    pub target_generation: i64,
    pub run_id: String,
    pub batch_id: String,
    pub payload_digest: String,
    pub row_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedImportPartitionScope {
    pub source_id: String,
    pub source_generation: i64,
    pub logical_schema: String,
    pub logical_table: String,
    pub partition_id: String,
    pub observed_partition_ids: Vec<String>,
    pub complete: bool,
    pub table_epoch: String,
    pub schema_fingerprint: String,
    pub schema_digest: String,
    pub type_digest: String,
    pub arrow_sha256: String,
    pub row_count: u64,
    pub current_signature: Option<String>,
    pub mutations_pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_range: Option<SealedImportPartitionRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_partition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_partition_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedImportPartitionRange {
    pub lower: String,
    pub upper: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportTable {
    pub logical_schema: String,
    pub logical_table: String,
    pub accepted_rows: u64,
    pub canonical_digest: String,
    pub columns: Vec<SealedImportColumn>,
    pub rejects: Vec<SealedImportReject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportColumn {
    pub logical_name: String,
    pub oid: i64,
    pub typmod: i32,
    pub caveat: Option<String>,
    pub source_type: Option<String>,
    pub key: bool,
    /// PostgreSQL type name of the normalized target storage. Not a source type.
    pub target_type: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportReject {
    pub column: String,
    pub reason: String,
    pub row_index: u64,
}

/// Label recorded on every compiled product. The source engine stays separate.
pub fn normalized_representation() -> &'static str {
    NORMALIZED_REPRESENTATION
}

/// Append one logical table header. The line is the producer schema preimage.
pub fn push_schema_table(out: &mut String, logical_schema: &str, logical_table: &str) {
    out.push_str(logical_schema);
    out.push('.');
    out.push_str(logical_table);
    out.push('\n');
}

/// Append one logical column. A missing caveat is an empty field.
pub fn push_schema_column(
    out: &mut String,
    logical_name: &str,
    oid: i64,
    typmod: i32,
    caveat: Option<&str>,
) {
    out.push_str(logical_name);
    out.push('\t');
    out.push_str(&oid.to_string());
    out.push('\t');
    out.push_str(&typmod.to_string());
    out.push('\t');
    out.push_str(caveat.unwrap_or(""));
    out.push('\n');
}

/// Parse one sealed-import catalog. `engine` is recorded for every closed token.
pub fn parse_sealed_import_catalog(input: &str) -> Result<SealedImportCatalog, SealedImportError> {
    let value = parse_json_value(input, SEALED_IMPORT_DRAFT_PROTOCOL).map_err(map_scan)?;
    refuse_secrets(&value)?;
    let document: Document = serde_json::from_value(value).map_err(map_shape)?;
    validate(document)
}

fn map_scan(err: DiagnosticList) -> SealedImportError {
    let item = err.items.first();
    let raw = item.map(|diag| diag.code.as_str()).unwrap_or("json.syntax");
    let name = raw.rsplit('/').next().unwrap_or("json.syntax");
    let name = match name {
        "json.budget.bytes" => "json.budget.bytes",
        "json.budget.depth" => "json.budget.depth",
        "json.budget.nodes" => "json.budget.nodes",
        "json.duplicate_key" => "json.duplicate_key",
        _ => "json.syntax",
    };
    let path = item
        .map(|diag| diag.path.clone())
        .unwrap_or_else(|| "/".to_string());
    let message = match name {
        "json.budget.bytes" => "document exceeds the byte budget",
        "json.budget.depth" => "JSON nesting exceeds the depth budget",
        "json.budget.nodes" => "JSON node count exceeds the budget",
        "json.duplicate_key" => "duplicate JSON object key",
        _ => "sealed import catalog JSON is not valid",
    };
    SealedImportError::new(name, path, message)
}

fn map_shape(err: serde_json::Error) -> SealedImportError {
    let text = err.to_string();
    if text.contains("unknown field") {
        SealedImportError::new(
            "field.unknown",
            "/catalog",
            "sealed import catalog has an unknown field",
        )
    } else if text.contains("missing field") {
        SealedImportError::new(
            "field.missing",
            "/catalog",
            "sealed import catalog is missing a required field",
        )
    } else {
        SealedImportError::new(
            "schema.invalid",
            "/catalog",
            "sealed import catalog JSON is not valid",
        )
    }
}

fn refuse_secrets(value: &Value) -> Result<(), SealedImportError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if secret_key(key) {
                    return Err(SealedImportError::new(
                        "secret.rejected",
                        "/catalog",
                        "sealed import catalog carries a connection field",
                    ));
                }
                refuse_secrets(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for child in items {
                refuse_secrets(child)?;
            }
            Ok(())
        }
        Value::String(text) => {
            let folded = text.to_ascii_lowercase();
            if folded.contains("://") || folded.contains("password") {
                Err(SealedImportError::new(
                    "secret.rejected",
                    "/catalog",
                    "sealed import catalog carries a connection field",
                ))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn secret_key(key: &str) -> bool {
    matches!(
        key,
        "password"
            | "password_file"
            | "host"
            | "endpoint"
            | "sql"
            | "secret"
            | "connection_string"
            | "username"
    )
}

fn validate(document: Document) -> Result<SealedImportCatalog, SealedImportError> {
    if document.protocol != SEALED_IMPORT_CATALOG_PROTOCOL {
        return Err(SealedImportError::new(
            "protocol.mismatch",
            "/protocol",
            "catalog protocol is not oo.pipeline.sealed-import-catalog/v1",
        ));
    }
    if document.state != "awaiting_publish" {
        return Err(SealedImportError::new(
            "seal.unsealed",
            "/state",
            "sealed import is not awaiting publish",
        ));
    }
    if document.capture != "sealed_import" {
        return Err(SealedImportError::new(
            "seal.unsealed",
            "/capture",
            "capture is not sealed_import",
        ));
    }
    if document.source_generation < 1 || document.target_generation < 1 {
        return Err(SealedImportError::new(
            "seal.stale",
            "/source_generation",
            "source or target generation is not current",
        ));
    }
    if document.hook_checked {
        return Err(SealedImportError::new(
            "quality.rejected",
            "/hook_checked",
            "hook proof is not in this catalog",
        ));
    }
    if document.shadows_queried || document.source_introspection != "not_used" {
        return Err(SealedImportError::new(
            "quality.rejected",
            "/source_introspection",
            "source introspection or shadow reads are not accepted",
        ));
    }
    require_source(&document.engine, "/engine")?;
    require_source(&document.source_id, "/source_id")?;
    require_digest(&document.config_digest, "/config_digest")?;
    require_digest(&document.input_digest, "/input_digest")?;
    require_digest(&document.type_digest, "/type_digest")?;
    require_digest(&document.schema_digest, "/schema_digest")?;
    require_digest(&document.tool_hook_digest, "/tool_hook_digest")?;
    require_run_id(&document.run_id)?;
    if document.tables.is_empty() || document.tables.len() > 16 {
        return Err(SealedImportError::new(
            "schema.invalid",
            "/tables",
            "sealed import table count is outside 1..=16",
        ));
    }
    let tables = validate_tables(&document.tables)?;
    let preimage = schema_preimage(&tables);
    if payload_sha256(preimage.as_bytes()) != document.schema_digest {
        return Err(SealedImportError::new(
            "digest.mismatch",
            "/schema_digest",
            "schema digest does not match the logical schema preimage",
        ));
    }
    validate_quality(&document.quality, &document.tables)?;
    let partition_vector = validate_partition_vector(&document, &tables)?;
    Ok(SealedImportCatalog {
        capture: document.capture,
        engine: document.engine,
        source_id: document.source_id,
        config_digest: document.config_digest,
        input_digest: document.input_digest,
        type_digest: document.type_digest,
        schema_digest: document.schema_digest,
        tool_hook_digest: document.tool_hook_digest,
        run_id: document.run_id,
        source_generation: document.source_generation,
        target_generation: document.target_generation,
        tables,
        partition_vector,
    })
}

fn validate_partition_vector(
    document: &Document,
    tables: &[SealedImportTable],
) -> Result<Option<Vec<SealedImportPartition>>, SealedImportError> {
    let Some(vector) = &document.partition_vector else {
        if document.engine == "clickhouse" {
            return Err(SealedImportError::new("partition.missing", "/partition_vector", "ClickHouse catalog lacks a full target partition vector"));
        }
        return Ok(None);
    };
    let olap = matches!(document.engine.as_str(), "doris" | "starrocks");
    if !(document.engine == "clickhouse" || olap) || tables.len() != 1 || vector.is_empty() || vector.len() > 4096 {
        return Err(SealedImportError::new("partition.invalid", "/partition_vector", "partition vector is outside the closed C4 single-table contract"));
    }
    let table = &tables[0];
    let mut ids = BTreeSet::new();
    let mut native_ids = BTreeSet::new();
    let mut ranges = Vec::new();
    let mut rows = 0_u64;
    let mut current = false;
    let first = &vector[0];
    for (index, entry) in vector.iter().enumerate() {
        let path = format!("/partition_vector/{index}");
        if !closed_partition_id(&entry.partition_id) || !ids.insert(entry.partition_id.clone())
            || (index > 0 && vector[index - 1].partition_id >= entry.partition_id) {
            return Err(SealedImportError::new("partition.order", path, "partition IDs are not closed, sorted, and unique"));
        }
        if olap { require_digest(&entry.table_epoch, "/partition_vector/table_epoch")?; }
        else { require_run_id(&entry.table_epoch)?; }
        require_run_id(&entry.run_id)?;
        if !closed_batch(&entry.batch_id) || entry.source_generation < 1 || entry.target_generation < 1
            || entry.source_generation > document.source_generation || entry.target_generation > document.target_generation
            || !matches!(entry.consistency_grade.as_str(), "T" | "S1" | "S0" | "B") {
            return Err(SealedImportError::new("partition.invalid", path, "partition generation, batch, or grade is invalid"));
        }
        require_digest(&entry.audit_digest, "/partition_vector/audit_digest")?;
        require_digest(&entry.payload_digest, "/partition_vector/payload_digest")?;
        require_digest(&entry.partition_scope.arrow_sha256, "/partition_vector/partition_scope/arrow_sha256")?;
        require_digest(&entry.partition_scope.schema_digest, "/partition_vector/partition_scope/schema_digest")?;
        require_digest(&entry.partition_scope.type_digest, "/partition_vector/partition_scope/type_digest")?;
        let scope = &entry.partition_scope;
        if olap {
            let Some(native_id) = scope.native_partition_id.as_deref() else {
                return Err(SealedImportError::new("partition.binding", path, "native partition ID is absent"));
            };
            let Some(range) = scope.partition_range.as_ref() else {
                return Err(SealedImportError::new("partition.binding", path, "native partition range is absent"));
            };
            if scope.source_engine.as_deref() != Some(document.engine.as_str())
                || !matches!(scope.key_model.as_deref(), Some("duplicate" | "unique" | "primary"))
                || scope.key_model != first.partition_scope.key_model
                || scope.routing_column != first.partition_scope.routing_column
                || scope.routing_column.as_deref().is_none_or(|name| name.is_empty() || !table.columns.iter().any(|column| column.logical_name == name && column.oid == 1082 && column.typmod == -1))
                || !closed_decimal(native_id) || !native_ids.insert(native_id.to_owned())
                || !scope.native_partition_version.as_deref().is_some_and(closed_decimal)
                || !closed_date(&range.lower) || !closed_date(&range.upper) || range.lower >= range.upper
                || !closed_digest(&entry.schema_fingerprint)
                || !entry.current_signature.as_deref().is_some_and(|signature| closed_native_signature(signature, native_id))
                || !entry.previous_signature.as_deref().is_none_or(|signature| closed_native_signature(signature, native_id)) {
                return Err(SealedImportError::new("partition.binding", path, "source partition identity or route is invalid"));
            }
            ranges.push((range.lower.as_str(), range.upper.as_str()));
        } else if scope.source_engine.is_some() || scope.key_model.is_some() || scope.routing_column.is_some()
            || scope.partition_range.is_some() || scope.native_partition_id.is_some()
            || scope.native_partition_version.is_some() {
            return Err(SealedImportError::new("partition.binding", path, "ClickHouse scope includes an OLAP extension"));
        }
        if scope.source_id != document.source_id || scope.source_generation != entry.source_generation
            || scope.logical_schema != table.logical_schema || scope.logical_table != table.logical_table
            || scope.partition_id != entry.partition_id || !scope.complete || scope.mutations_pending
            || scope.table_epoch != entry.table_epoch || scope.schema_fingerprint != entry.schema_fingerprint
            || scope.schema_digest != first.partition_scope.schema_digest || scope.type_digest != document.type_digest
            || scope.current_signature != entry.current_signature || scope.row_count != entry.row_count
            || entry.table_epoch != first.table_epoch || entry.schema_fingerprint != first.schema_fingerprint
            || !closed_fingerprint(&entry.schema_fingerprint) || !closed_signature(entry.current_signature.as_deref())
            || !closed_signature(entry.previous_signature.as_deref())
            || (!entry.previous_ack_exists && entry.previous_signature.is_some()) {
            return Err(SealedImportError::new("partition.binding", path, "partition scope or target sidecar differs from the catalog"));
        }
        if entry.source_generation == document.source_generation && entry.target_generation == document.target_generation && entry.run_id == document.run_id {
            current = true;
        }
        rows = rows.checked_add(entry.row_count).ok_or_else(|| SealedImportError::new("partition.rows", "/partition_vector", "partition row sum overflows"))?;
    }
    let observed: Vec<String> = ids.iter().cloned().collect();
    if olap {
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(SealedImportError::new("partition.binding", "/partition_vector", "source partition ranges overlap"));
        }
    }
    if !current || rows != table.accepted_rows || vector.iter().any(|entry| {
        let scoped = &entry.partition_scope.observed_partition_ids;
        scoped.is_empty()
            || scoped.binary_search(&entry.partition_id).is_err()
            || scoped.windows(2).any(|pair| pair[0] >= pair[1])
            || scoped.iter().any(|id| !ids.contains(id))
            || (entry.source_generation == document.source_generation
                && entry.target_generation == document.target_generation
                && entry.run_id == document.run_id
                && scoped != &observed)
    }) {
        return Err(SealedImportError::new("partition.coverage", "/partition_vector", "partition coverage or full materialized row count is incomplete"));
    }
    Ok(Some(vector.clone()))
}

fn closed_decimal(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|number| number > 0)
}

fn closed_date(value: &str) -> bool {
    value.len() == 10 && value.bytes().enumerate().all(|(index, byte)|
        if matches!(index, 4 | 7) { byte == b'-' } else { byte.is_ascii_digit() })
}

fn closed_native_signature(value: &str, native_id: &str) -> bool {
    value.strip_prefix(&format!("{native_id}|")).is_some_and(closed_digest)
}

fn closed_partition_id(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn closed_fingerprint(value: &str) -> bool {
    (1..=8192).contains(&value.len()) && !value.contains('\0')
}

fn closed_signature(value: Option<&str>) -> bool {
    value.is_none_or(|text| (1..=2048).contains(&text.len()) && !text.contains('\0'))
}

fn schema_preimage(tables: &[SealedImportTable]) -> String {
    let mut out = String::new();
    for table in tables {
        push_schema_table(&mut out, &table.logical_schema, &table.logical_table);
        for column in &table.columns {
            push_schema_column(
                &mut out,
                &column.logical_name,
                column.oid,
                column.typmod,
                column.caveat.as_deref(),
            );
        }
    }
    out
}

fn validate_tables(tables: &[TableIn]) -> Result<Vec<SealedImportTable>, SealedImportError> {
    let mut seen_tables = BTreeSet::new();
    let mut seen_physical = BTreeSet::new();
    let mut seen_columns = BTreeSet::new();
    let mut out = Vec::with_capacity(tables.len());
    for table in tables {
        require_source(&table.logical_schema, "/tables/logical_schema")?;
        require_table(&table.logical_table, "/tables/logical_table")?;
        require_digest(&table.canonical_digest, "/tables/canonical_digest")?;
        require_shadow(&table.stored_namespace, "pn_", "/tables/stored_namespace")?;
        require_shadow(&table.stored_physical, "pt_", "/tables/stored_physical")?;
        if table.columns.is_empty() || table.columns.len() > 64 {
            return Err(SealedImportError::new(
                "schema.invalid",
                "/tables/columns",
                "sealed import column count is outside 1..=64",
            ));
        }
        let dataset = format!("{}.{}", table.logical_schema, table.logical_table);
        if !seen_tables.insert(dataset.clone()) {
            return Err(SealedImportError::new(
                "duplicate",
                "/tables",
                "a logical table is repeated",
            ));
        }
        if !seen_physical.insert(table.stored_physical.clone()) {
            return Err(SealedImportError::new(
                "duplicate",
                "/tables/stored_physical",
                "a target shadow name is repeated",
            ));
        }
        let mut columns = Vec::with_capacity(table.columns.len());
        for column in &table.columns {
            columns.push(validate_column(column, &dataset, &mut seen_columns)?);
        }
        let mut rejects = Vec::with_capacity(table.rejects.len());
        for reject in &table.rejects {
            if !closed_reason(&reject.reason) || !columns.iter().any(|item| item.logical_name == reject.column)
            {
                return Err(SealedImportError::new(
                    "schema.invalid",
                    "/tables/rejects",
                    "a reject line does not name a logical column and a closed reason",
                ));
            }
            if let Some(digest) = &reject.key_digest {
                require_digest(digest, "/tables/rejects/key_digest")?;
            }
            rejects.push(SealedImportReject {
                column: reject.column.clone(),
                reason: reject.reason.clone(),
                row_index: reject.row_index,
            });
        }
        out.push(SealedImportTable {
            logical_schema: table.logical_schema.clone(),
            logical_table: table.logical_table.clone(),
            accepted_rows: table.accepted_rows,
            canonical_digest: table.canonical_digest.clone(),
            columns,
            rejects,
        });
    }
    Ok(out)
}

fn validate_column(
    column: &ColumnIn,
    dataset: &str,
    seen_columns: &mut BTreeSet<String>,
) -> Result<SealedImportColumn, SealedImportError> {
    require_table(&column.logical_name, "/tables/columns/logical_name")?;
    require_shadow(
        &column.stored_physical,
        "pc_",
        "/tables/columns/stored_physical",
    )?;
    let qualified = format!("{dataset}.{}", column.logical_name);
    if !seen_columns.insert(qualified) || !seen_columns.insert(column.stored_physical.clone()) {
        return Err(SealedImportError::new(
            "duplicate",
            "/tables/columns",
            "a logical column or target shadow name is repeated",
        ));
    }
    if let Some(source_type) = &column.source_type {
        if sql_authority(source_type) {
            return Err(SealedImportError::new(
                "sql.rejected",
                "/tables/columns/source_type",
                "source type text is not a type token",
            ));
        }
        if !plugin_token(source_type) {
            return Err(SealedImportError::new(
                "schema.invalid",
                "/tables/columns/source_type",
                "source type is not a closed token",
            ));
        }
    }
    if let Some(caveat) = &column.caveat
        && sql_authority(caveat)
    {
        return Err(SealedImportError::new(
            "sql.rejected",
            "/tables/columns/caveat",
            "caveat text is not a type token",
        ));
    }
    let target_type = target_type(column.oid, column.typmod).ok_or_else(|| {
        SealedImportError::new(
            "type.ambiguous",
            "/tables/columns/oid",
            "native type has no normalized target representation",
        )
    })?;
    if let Some(caveat) = &column.caveat
        && !caveat_agrees(caveat, column.oid, column.typmod)
    {
        return Err(SealedImportError::new(
            "type.ambiguous",
            "/tables/columns/caveat",
            "caveat does not agree with the normalized target type",
        ));
    }
    if let Some(source_type) = &column.source_type
        && let Some(named) = named_oid(source_type)
        && named != column.oid
    {
        return Err(SealedImportError::new(
            "type.ambiguous",
            "/tables/columns/source_type",
            "source type token names a different normalized type",
        ));
    }
    Ok(SealedImportColumn {
        logical_name: column.logical_name.clone(),
        oid: column.oid,
        typmod: column.typmod,
        caveat: column.caveat.clone(),
        source_type: column.source_type.clone(),
        key: column.key,
        target_type,
    })
}

fn validate_quality(quality: &QualityIn, tables: &[TableIn]) -> Result<(), SealedImportError> {
    if quality.loss_concealed {
        return Err(SealedImportError::new(
            "quality.rejected",
            "/quality/loss_concealed",
            "concealed loss is not accepted",
        ));
    }
    let manifest_count: usize = tables.iter().map(|table| table.rejects.len()).sum();
    if quality.manifest_reject_count != u64::try_from(manifest_count).unwrap_or(u64::MAX) {
        return Err(SealedImportError::new(
            "quality.rejected",
            "/quality/manifest_reject_count",
            "manifest reject count does not match the reject lines",
        ));
    }
    let mut batch_rows = 0_usize;
    let mut reject_rows = Vec::new();
    let mut stored_rejected = 0_i64;
    let mut seen_batch = BTreeSet::new();
    for row in &quality.stored_rows {
        require_shadow(&row.physical_name, "pt_", "/quality/stored_rows/physical_name")?;
        if !closed_batch(&row.batch_id) {
            return Err(SealedImportError::new(
                "schema.invalid",
                "/quality/stored_rows/batch_id",
                "batch id is not a closed token",
            ));
        }
        if row.kind == "batch" {
            batch_rows += 1;
            if !seen_batch.insert(row.physical_name.clone()) {
                return Err(SealedImportError::new(
                    "duplicate",
                    "/quality/stored_rows",
                    "a target shadow batch is repeated",
                ));
            }
            let table = tables
                .iter()
                .find(|table| table.stored_physical == row.physical_name)
                .ok_or_else(|| {
                    SealedImportError::new(
                        "quality.rejected",
                        "/quality/stored_rows",
                        "a stored batch does not name a sealed table",
                    )
                })?;
            let accepted = i64::try_from(table.accepted_rows).map_err(|_| {
                SealedImportError::new(
                    "schema.invalid",
                    "/tables/accepted_rows",
                    "accepted row count is outside the sealed range",
                )
            })?;
            if row.accepted_rows != Some(accepted)
                || row.reason.is_some()
                || row.attnum.is_some()
                || row.value_len.is_some()
                || row.row_index.is_some()
                || row.key_digest.is_some()
            {
                return Err(SealedImportError::new(
                    "quality.rejected",
                    "/quality/stored_rows",
                    "a stored batch does not match the sealed table",
                ));
            }
            stored_rejected = stored_rejected.saturating_add(row.rejected_rows.unwrap_or(0));
        } else if row.kind == "reject" {
            reject_rows.push(row);
        } else {
            return Err(SealedImportError::new(
                "quality.rejected",
                "/quality/stored_rows/kind",
                "stored quality kind is not batch or reject",
            ));
        }
    }
    if batch_rows != tables.len() || seen_batch.len() != tables.len() {
        return Err(SealedImportError::new(
            "quality.rejected",
            "/quality/stored_rows",
            "stored batches do not cover the sealed tables",
        ));
    }
    if quality.stored_rejected_rows != stored_rejected {
        return Err(SealedImportError::new(
            "quality.rejected",
            "/quality/stored_rejected_rows",
            "stored rejected row count does not match the batch rows",
        ));
    }
    let omits = !quality.payload_includes_reject_lines && manifest_count > 0;
    if quality.stored_omits_manifest_rejects != omits {
        return Err(SealedImportError::new(
            "quality.rejected",
            "/quality/stored_omits_manifest_rejects",
            "stored reject claim does not match the payload",
        ));
    }
    if quality.payload_includes_reject_lines {
        if stored_rejected != i64::try_from(manifest_count).unwrap_or(i64::MAX)
            || reject_rows.len() != manifest_count
        {
            return Err(SealedImportError::new(
                "quality.rejected",
                "/quality/payload_includes_reject_lines",
                "stored reject lines do not match the manifest",
            ));
        }
        for table in tables {
            for reject in &table.rejects {
                if reject.attnum.is_none() || reject.value_len.is_none() {
                    return Err(SealedImportError::new(
                        "quality.rejected",
                        "/tables/rejects",
                        "a payload reject line is missing its stored fields",
                    ));
                }
                let found = reject_rows.iter().any(|row| {
                    row.physical_name == table.stored_physical
                        && row.reason.as_deref() == Some(reject.reason.as_str())
                        && row.attnum == reject.attnum
                        && row.value_len == reject.value_len
                        && row.row_index == i64::try_from(reject.row_index).ok()
                        && key_same(row.key_digest.as_deref(), reject.key_digest.as_deref())
                });
                if !found {
                    return Err(SealedImportError::new(
                        "quality.rejected",
                        "/quality/stored_rows",
                        "a manifest reject has no stored row",
                    ));
                }
            }
        }
    } else if stored_rejected != 0 || !reject_rows.is_empty() {
        return Err(SealedImportError::new(
            "quality.rejected",
            "/quality/payload_includes_reject_lines",
            "stored reject lines are present while the payload omits them",
        ));
    }
    Ok(())
}

fn key_same(stored: Option<&str>, manifest: Option<&str>) -> bool {
    let stored = stored.filter(|value| !value.is_empty());
    let manifest = manifest.filter(|value| !value.is_empty());
    stored == manifest
}

fn target_type(oid: i64, typmod: i32) -> Option<&'static str> {
    let name = match oid {
        16 => "bool",
        17 => "bytea",
        20 => "int8",
        21 => "int2",
        23 => "int4",
        25 => "text",
        114 => "json",
        700 => "float4",
        701 => "float8",
        // PostgreSQL bpchar is CHAR(n). typmod is 4 plus the character length.
        // The name stays bpchar so fixed width is not rewritten as text or varchar.
        1042 => "bpchar",
        1043 => "varchar",
        1082 => "date",
        1083 => "time",
        1114 => "timestamp",
        1184 => "timestamptz",
        1186 => "interval",
        1700 => "numeric",
        // PostgreSQL uuid. Spec target name is uuid; text would drop the type.
        2950 => "uuid",
        // PostgreSQL jsonb. OID 114 stays json. A full document is a separate seal.
        3802 => "jsonb",
        _ => return None,
    };
    if typmod_ok(oid, typmod) {
        Some(name)
    } else {
        None
    }
}

fn typmod_ok(oid: i64, typmod: i32) -> bool {
    match oid {
        // CHAR(n) requires n >= 1, so typmod >= 5. Unconstrained text stays varchar.
        1042 => typmod >= 5,
        1043 => typmod == -1 || typmod >= 4,
        1700 => {
            if typmod < 0 {
                return true;
            }
            match numeric_parts(typmod) {
                Some((precision, scale)) => precision > 0 && scale >= 0 && scale <= precision,
                None => false,
            }
        }
        1083 | 1114 | 1184 => typmod == -1 || (0..=6).contains(&typmod),
        _ => typmod == -1,
    }
}

pub(crate) fn numeric_parts(typmod: i32) -> Option<(i32, i32)> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod - 4;
    let precision = (adjusted >> 16) & 0xFFFF;
    let scale = adjusted & 0xFFFF;
    Some((precision, scale))
}

fn caveat_agrees(caveat: &str, oid: i64, typmod: i32) -> bool {
    match caveat {
        "tinyint1_as_int2" => oid == 21 && typmod < 0,
        "bigint_unsigned_numeric" => {
            oid == 1700 && numeric_parts(typmod) == Some((20, 0))
        }
        "mysql_time_not_interval" => oid == 1083 && typmod < 0,
        "json_not_jsonb" => oid == 114 && typmod < 0,
        "enum_as_text" => oid == 25 && typmod < 0,
        _ => false,
    }
}

fn named_oid(name: &str) -> Option<i64> {
    Some(match name {
        "bool" => 16,
        "bytea" => 17,
        "int8" => 20,
        "int2" => 21,
        "int4" => 23,
        "text" => 25,
        "json" => 114,
        "float4" => 700,
        "float8" => 701,
        "bpchar" => 1042,
        "varchar" => 1043,
        "date" => 1082,
        "time" => 1083,
        "timestamp" => 1114,
        "timestamptz" => 1184,
        "interval" | "duration_micros" => 1186,
        "numeric" => 1700,
        "uuid" => 2950,
        "jsonb" => 3802,
        _ => return None,
    })
}

fn require_source(value: &str, path: &str) -> Result<(), SealedImportError> {
    if closed_source(value) {
        Ok(())
    } else {
        Err(SealedImportError::new(
            "schema.invalid",
            path,
            "identifier is not a closed source token",
        ))
    }
}

fn require_table(value: &str, path: &str) -> Result<(), SealedImportError> {
    if closed_table(value) {
        Ok(())
    } else {
        Err(SealedImportError::new(
            "schema.invalid",
            path,
            "identifier is not a closed table token",
        ))
    }
}

fn require_digest(value: &str, path: &str) -> Result<(), SealedImportError> {
    if closed_digest(value) {
        Ok(())
    } else {
        Err(SealedImportError::new(
            "digest.mismatch",
            path,
            "digest is not 64 lowercase hex characters",
        ))
    }
}

fn require_shadow(value: &str, prefix: &str, path: &str) -> Result<(), SealedImportError> {
    let rest = value.strip_prefix(prefix).unwrap_or("");
    let hex = rest.len() == 60
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    if value.starts_with(prefix) && hex {
        Ok(())
    } else {
        Err(SealedImportError::new(
            "schema.invalid",
            path,
            "target shadow name is not a closed storage token",
        ))
    }
}

fn require_run_id(value: &str) -> Result<(), SealedImportError> {
    let bytes = value.as_bytes();
    let ok = bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        });
    if ok {
        Ok(())
    } else {
        Err(SealedImportError::new(
            "schema.invalid",
            "/run_id",
            "run id is not a lowercase uuid",
        ))
    }
}

fn closed_source(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn closed_table(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn closed_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn closed_reason(value: &str) -> bool {
    (1..=80).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

fn closed_batch(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && !value.contains("..")
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

fn plugin_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-')
}

fn sql_authority(value: &str) -> bool {
    value.bytes().any(|byte| {
        matches!(
            byte,
            b' ' | b'(' | b')' | b'\'' | b'"' | b';' | b'\\' | b'`' | b'/' | b'*'
        )
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    protocol: String,
    capture: String,
    engine: String,
    source_id: String,
    config_digest: String,
    input_digest: String,
    type_digest: String,
    schema_digest: String,
    tool_hook_digest: String,
    run_id: String,
    source_generation: i64,
    target_generation: i64,
    state: String,
    hook_checked: bool,
    shadows_queried: bool,
    source_introspection: String,
    tables: Vec<TableIn>,
    quality: QualityIn,
    #[serde(default, deserialize_with = "non_null_partition_vector")]
    partition_vector: Option<Vec<SealedImportPartition>>,
}

fn non_null_partition_vector<'de, D>(deserializer: D) -> Result<Option<Vec<SealedImportPartition>>, D::Error>
where D: Deserializer<'de> {
    Vec::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableIn {
    logical_schema: String,
    logical_table: String,
    accepted_rows: u64,
    canonical_digest: String,
    stored_namespace: String,
    stored_physical: String,
    columns: Vec<ColumnIn>,
    rejects: Vec<RejectIn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnIn {
    logical_name: String,
    oid: i64,
    typmod: i32,
    caveat: Option<String>,
    source_type: Option<String>,
    key: bool,
    stored_physical: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectIn {
    column: String,
    reason: String,
    row_index: u64,
    #[serde(default)]
    attnum: Option<i32>,
    #[serde(default)]
    value_len: Option<i64>,
    #[serde(default)]
    key_digest: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityIn {
    loss_concealed: bool,
    manifest_reject_count: u64,
    payload_includes_reject_lines: bool,
    stored_omits_manifest_rejects: bool,
    stored_rejected_rows: i64,
    stored_rows: Vec<StoredRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRow {
    physical_name: String,
    batch_id: String,
    kind: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    value_len: Option<i64>,
    #[serde(default)]
    attnum: Option<i32>,
    #[serde(default)]
    row_index: Option<i64>,
    #[serde(default)]
    key_digest: Option<String>,
    #[serde(default)]
    accepted_rows: Option<i64>,
    #[serde(default)]
    rejected_rows: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_run_generation_batch_token_is_closed() {
        assert!(closed_batch("acd47cd5-daf0-4f0c-8da1-3b475a159778_4"));
        assert!(closed_batch("t0.b0"));
        assert!(closed_batch("oo-dl01w1-40-native-20261013-g5-part"));
        for refused in ["", "../part", "part..tail", "part/path", "part\\path", "part\nnext"] {
            assert!(!closed_batch(refused));
        }
        assert!(!closed_batch(&"x".repeat(129)));
    }

    #[test]
    fn schema_preimage_matches_the_producer_line_shape() {
        let mut out = String::new();
        push_schema_table(&mut out, "erp", "parcel");
        push_schema_column(&mut out, "day_on", 1082, -1, None);
        push_schema_column(&mut out, "clock_at", 1083, -1, Some("mysql_time_not_interval"));
        assert_eq!(
            out,
            "erp.parcel\nday_on\t1082\t-1\t\nclock_at\t1083\t-1\tmysql_time_not_interval\n"
        );
    }

    #[test]
    fn engine_is_not_a_success_switch() {
        let mysql = parse_sealed_import_catalog(&minimal("mysql")).expect("mysql");
        let oracle = parse_sealed_import_catalog(&minimal("oracle")).expect("oracle");
        assert_eq!(mysql.engine, "mysql");
        assert_eq!(oracle.engine, "oracle");
        assert_eq!(mysql.schema_digest, oracle.schema_digest);
        assert_eq!(mysql.tables[0].columns[0].target_type, "int4");
        let clickhouse = parse_sealed_import_catalog(&minimal("clickhouse")).expect_err("C4 vector required");
        assert!(clickhouse.code.ends_with("/partition.missing"));
        let mut null_vector: Value = serde_json::from_str(&minimal("mysql")).expect("fixture");
        null_vector["partition_vector"] = Value::Null;
        assert!(parse_sealed_import_catalog(&null_vector.to_string()).expect_err("null vector").code.ends_with("/schema.invalid"));
    }

    #[test]
    fn clickhouse_partition_vector_binds_full_materialized_table() {
        let mut catalog: Value = serde_json::from_str(&minimal("clickhouse")).expect("fixture");
        let digest = "a".repeat(64);
        let scope = serde_json::json!({
            "source_id":"src","source_generation":1,"logical_schema":"erp","logical_table":"parcel",
            "partition_id":"202401","observed_partition_ids":["202401"],"complete":true,
            "table_epoch":"de8276fc-8634-4543-994f-ce82a46a3c18","schema_fingerprint":"id\tUInt64",
            "schema_digest":digest,"type_digest":catalog["type_digest"],
            "arrow_sha256":digest,"row_count":1,"current_signature":"202401|1|1|sealed","mutations_pending":false
        });
        catalog["partition_vector"] = serde_json::json!([{
            "partition_id":"202401","table_epoch":scope["table_epoch"],"schema_fingerprint":scope["schema_fingerprint"],
            "current_signature":scope["current_signature"],"previous_ack_exists":false,"previous_signature":null,
            "partition_scope":scope,"audit_digest":digest,"consistency_grade":"S1",
            "source_generation":1,"target_generation":1,"run_id":catalog["run_id"],
            "batch_id":"t0.b0","payload_digest":digest,"row_count":1
        }]);
        let parsed = parse_sealed_import_catalog(&catalog.to_string()).expect("complete vector");
        assert_eq!(parsed.partition_vector.as_ref().expect("vector").len(), 1);
        assert_ne!(parsed.partition_vector.as_ref().expect("vector")[0].partition_scope.schema_digest, parsed.schema_digest);
        let mut history = catalog.clone();
        history["source_generation"] = serde_json::json!(2);
        history["target_generation"] = serde_json::json!(2);
        let mut current = history["partition_vector"][0].clone();
        current["partition_id"] = serde_json::json!("202403");
        current["partition_scope"]["partition_id"] = serde_json::json!("202403");
        current["partition_scope"]["source_generation"] = serde_json::json!(2);
        current["partition_scope"]["observed_partition_ids"] = serde_json::json!(["202401", "202403"]);
        current["current_signature"] = serde_json::json!("202403|1|1|sealed");
        current["partition_scope"]["current_signature"] = current["current_signature"].clone();
        current["source_generation"] = serde_json::json!(2);
        current["target_generation"] = serde_json::json!(2);
        current["row_count"] = serde_json::json!(0);
        current["partition_scope"]["row_count"] = serde_json::json!(0);
        history["partition_vector"].as_array_mut().unwrap().push(current);
        assert!(parse_sealed_import_catalog(&history.to_string()).is_ok());
        history["partition_vector"][1]["partition_scope"]["observed_partition_ids"] = serde_json::json!(["202403"]);
        assert!(parse_sealed_import_catalog(&history.to_string()).unwrap_err().code.ends_with("/partition.coverage"));
        history["partition_vector"][1]["partition_scope"]["observed_partition_ids"] = serde_json::json!(["202401", "202403"]);
        history["partition_vector"][0]["partition_scope"]["observed_partition_ids"] = serde_json::json!(["202401", "202405"]);
        assert!(parse_sealed_import_catalog(&history.to_string()).unwrap_err().code.ends_with("/partition.coverage"));
        catalog["partition_vector"][0]["row_count"] = serde_json::json!(0);
        let wrong = parse_sealed_import_catalog(&catalog.to_string()).expect_err("partial row count");
        assert!(wrong.code.ends_with("/partition.binding"));
        catalog["partition_vector"][0]["row_count"] = serde_json::json!(1);
        catalog["partition_vector"][0]["partition_scope"]["invented"] = serde_json::json!(true);
        let unknown = parse_sealed_import_catalog(&catalog.to_string()).expect_err("unknown scope field");
        assert!(unknown.code.ends_with("/field.unknown"));
    }

    #[test]
    fn uuid_oid_2950_is_a_normalized_target_for_every_engine() {
        for engine in ["mysql", "mongodb", "oracle", "postgresql"] {
            let parsed = parse_sealed_import_catalog(&one_column(engine, 2950, -1, None)).expect(engine);
            assert_eq!(parsed.engine, engine);
            assert_eq!(parsed.tables[0].columns[0].target_type, "uuid");
            assert_eq!(parsed.tables[0].columns[0].oid, 2950);
            assert!(parsed.tables[0].columns[0].source_type.is_none());
            let named = parse_sealed_import_catalog(&one_column(engine, 2950, -1, Some("uuid"))).expect(engine);
            assert_eq!(named.tables[0].columns[0].source_type.as_deref(), Some("uuid"));
            assert_eq!(named.tables[0].columns[0].target_type, "uuid");
        }
        let mysql = parse_sealed_import_catalog(&one_column("mysql", 2950, -1, None)).unwrap();
        let oracle = parse_sealed_import_catalog(&one_column("oracle", 2950, -1, None)).unwrap();
        assert_eq!(mysql.schema_digest, oracle.schema_digest);
        let text = parse_sealed_import_catalog(&one_column("postgresql", 2950, -1, Some("text"))).expect_err("text");
        assert!(text.code.ends_with("/type.ambiguous"));
        assert_eq!(text.message, "source type token names a different normalized type");
        let unknown = parse_sealed_import_catalog(&one_column("postgresql", 2951, -1, None)).expect_err("oid");
        assert!(unknown.code.ends_with("/type.ambiguous"));
        assert_eq!(unknown.message, "native type has no normalized target representation");
        let typmod = parse_sealed_import_catalog(&one_column("postgresql", 2950, 0, None)).expect_err("typmod");
        assert!(typmod.code.ends_with("/type.ambiguous"));
        assert_eq!(typmod.message, "native type has no normalized target representation");
    }

    #[test]
    fn mysql_duration_stays_interval_in_sealed_catalog() {
        let parsed = parse_sealed_import_catalog(&one_column("mysql", 1186, -1, Some("duration_micros"))).unwrap();
        assert_eq!(parsed.tables[0].columns[0].target_type, "interval");
        let err = parse_sealed_import_catalog(&one_column("mysql", 1083, -1, Some("duration_micros"))).unwrap_err();
        assert!(err.code.ends_with("/type.ambiguous"));
        let err = parse_sealed_import_catalog(&one_column("mysql", 1186, 0, Some("duration_micros"))).unwrap_err();
        assert!(err.code.ends_with("/type.ambiguous"));
    }

    #[test]
    fn bpchar_oid_1042_keeps_a_declared_width_for_every_engine() {
        for engine in ["mysql", "mongodb", "oracle", "postgresql"] {
            let parsed = parse_sealed_import_catalog(&one_column(engine, 1042, 10, None)).expect(engine);
            assert_eq!(parsed.engine, engine);
            let column = &parsed.tables[0].columns[0];
            assert_eq!(column.target_type, "bpchar");
            assert_eq!(column.oid, 1042);
            assert_eq!(column.typmod, 10);
            assert_eq!(column.typmod - 4, 6);
            assert!(column.source_type.is_none());
            let named = parse_sealed_import_catalog(&one_column(engine, 1042, 10, Some("bpchar"))).expect(engine);
            assert_eq!(named.tables[0].columns[0].source_type.as_deref(), Some("bpchar"));
            assert_eq!(named.tables[0].columns[0].target_type, "bpchar");
            assert_eq!(named.tables[0].columns[0].typmod, 10);
        }
        let mysql = parse_sealed_import_catalog(&one_column("mysql", 1042, 10, None)).unwrap();
        let oracle = parse_sealed_import_catalog(&one_column("oracle", 1042, 10, None)).unwrap();
        assert_eq!(mysql.schema_digest, oracle.schema_digest);
        let one = parse_sealed_import_catalog(&one_column("postgresql", 1042, 5, None)).expect("char1");
        assert_eq!(one.tables[0].columns[0].target_type, "bpchar");
        assert_eq!(one.tables[0].columns[0].typmod, 5);
        let wide = parse_sealed_import_catalog(&one_column("postgresql", 1042, 104, None)).expect("char100");
        assert_eq!(wide.tables[0].columns[0].target_type, "bpchar");
        assert_eq!(wide.tables[0].columns[0].typmod, 104);
        let token = parse_sealed_import_catalog(&one_column("oracle", 1042, 10, Some("char"))).expect("char");
        assert_eq!(token.tables[0].columns[0].source_type.as_deref(), Some("char"));
        assert_eq!(token.tables[0].columns[0].target_type, "bpchar");
        assert_eq!(token.tables[0].columns[0].oid, 1042);
        let varchar = parse_sealed_import_catalog(&one_column("oracle", 1043, -1, None)).expect("varchar");
        assert_eq!(varchar.tables[0].columns[0].target_type, "varchar");
        for (label, source) in [("text", Some("text")), ("varchar", Some("varchar"))] {
            let err = parse_sealed_import_catalog(&one_column("postgresql", 1042, 10, source)).expect_err(label);
            assert!(err.code.ends_with("/type.ambiguous"));
            assert_eq!(err.message, "source type token names a different normalized type");
        }
        for typmod in [-1, 0, 4] {
            let err = parse_sealed_import_catalog(&one_column("postgresql", 1042, typmod, None)).expect_err("typmod");
            assert!(err.code.ends_with("/type.ambiguous"));
            assert_eq!(err.message, "native type has no normalized target representation");
        }
        let unknown = parse_sealed_import_catalog(&one_column("postgresql", 1041, 10, None)).expect_err("oid");
        assert!(unknown.code.ends_with("/type.ambiguous"));
        assert_eq!(unknown.message, "native type has no normalized target representation");
    }

    #[test]
    fn jsonb_oid_3802_is_jsonb_and_json_114_stays_json() {
        let mut digests = Vec::new();
        for engine in ["mysql", "mongodb", "oracle", "postgresql"] {
            let parsed = parse_sealed_import_catalog(&one_column(engine, 3802, -1, None)).expect(engine);
            assert_eq!(parsed.engine, engine);
            let column = &parsed.tables[0].columns[0];
            assert_eq!(column.target_type, "jsonb");
            assert_eq!(column.oid, 3802);
            assert_eq!(column.typmod, -1);
            assert!(column.source_type.is_none());
            let named = parse_sealed_import_catalog(&one_column(engine, 3802, -1, Some("jsonb"))).expect(engine);
            assert_eq!(named.tables[0].columns[0].source_type.as_deref(), Some("jsonb"));
            assert_eq!(named.tables[0].columns[0].target_type, "jsonb");
            digests.push(parsed.schema_digest.clone());
        }
        assert!(digests.windows(2).all(|pair| pair[0] == pair[1]));
        let json = parse_sealed_import_catalog(&one_column("postgresql", 114, -1, None)).expect("json");
        assert_eq!(json.tables[0].columns[0].target_type, "json");
        assert_eq!(json.tables[0].columns[0].oid, 114);
        for source in ["json", "text"] {
            let err = parse_sealed_import_catalog(&one_column("mongodb", 3802, -1, Some(source))).expect_err(source);
            assert!(err.code.ends_with("/type.ambiguous"));
            assert_eq!(err.message, "source type token names a different normalized type");
        }
        let typmod = parse_sealed_import_catalog(&one_column("mongodb", 3802, 0, None)).expect_err("typmod");
        assert!(typmod.code.ends_with("/type.ambiguous"));
        assert_eq!(typmod.message, "native type has no normalized target representation");
        let unknown = parse_sealed_import_catalog(&one_column("mongodb", 3803, -1, None)).expect_err("oid");
        assert!(unknown.code.ends_with("/type.ambiguous"));
        assert_eq!(unknown.message, "native type has no normalized target representation");
        let mut bad = serde_json::from_str::<Value>(&one_column("postgresql", 3802, -1, None)).unwrap();
        bad["tables"][0]["columns"][0]["caveat"] = Value::String("json_not_jsonb".to_string());
        let mut preimage = String::new();
        push_schema_table(&mut preimage, "public", "hubs");
        push_schema_column(&mut preimage, "hub_token", 3802, -1, Some("json_not_jsonb"));
        bad["schema_digest"] = Value::String(payload_sha256(preimage.as_bytes()));
        let err = parse_sealed_import_catalog(&bad.to_string()).expect_err("caveat");
        assert!(err.code.ends_with("/type.ambiguous"));
        assert_eq!(err.message, "caveat does not agree with the normalized target type");
        let mut kept = serde_json::from_str::<Value>(&one_column("postgresql", 114, -1, None)).unwrap();
        kept["tables"][0]["columns"][0]["caveat"] = Value::String("json_not_jsonb".to_string());
        preimage.clear();
        push_schema_table(&mut preimage, "public", "hubs");
        push_schema_column(&mut preimage, "hub_token", 114, -1, Some("json_not_jsonb"));
        kept["schema_digest"] = Value::String(payload_sha256(preimage.as_bytes()));
        let parsed = parse_sealed_import_catalog(&kept.to_string()).expect("json caveat");
        assert_eq!(parsed.tables[0].columns[0].target_type, "json");
        assert_eq!(parsed.tables[0].columns[0].caveat.as_deref(), Some("json_not_jsonb"));
    }

    #[test]
    fn wrong_digest_unsealed_stale_and_secrets_fail() {
        let mut wrong = serde_json::from_str::<Value>(&minimal("mysql")).unwrap();
        wrong["schema_digest"] = Value::String("ab".repeat(32));
        let err = parse_sealed_import_catalog(&wrong.to_string()).expect_err("digest");
        assert!(err.code.ends_with("/digest.mismatch"));
        wrong = serde_json::from_str(&minimal("mysql")).unwrap();
        wrong["state"] = Value::String("retired".to_string());
        let err = parse_sealed_import_catalog(&wrong.to_string()).expect_err("state");
        assert!(err.code.ends_with("/seal.unsealed"));
        wrong = serde_json::from_str(&minimal("mysql")).unwrap();
        wrong["source_generation"] = Value::from(0);
        let err = parse_sealed_import_catalog(&wrong.to_string()).expect_err("stale");
        assert!(err.code.ends_with("/seal.stale"));
        wrong = serde_json::from_str(&minimal("mysql")).unwrap();
        wrong["password_file"] = Value::String("x".to_string());
        let err = parse_sealed_import_catalog(&wrong.to_string()).expect_err("secret");
        assert!(err.code.ends_with("/secret.rejected"));
    }

    fn one_column(engine: &str, oid: i64, typmod: i32, source_type: Option<&str>) -> String {
        let mut preimage = String::new();
        push_schema_table(&mut preimage, "public", "hubs");
        push_schema_column(&mut preimage, "hub_token", oid, typmod, None);
        let schema_digest = payload_sha256(preimage.as_bytes());
        let digest = "a".repeat(64);
        let shadow = "ab".repeat(30);
        serde_json::json!({
            "capture": "sealed_import",
            "config_digest": digest,
            "engine": engine,
            "hook_checked": false,
            "input_digest": digest,
            "protocol": "oo.pipeline.sealed-import-catalog/v1",
            "quality": {
                "loss_concealed": false,
                "manifest_reject_count": 0,
                "payload_includes_reject_lines": false,
                "stored_omits_manifest_rejects": false,
                "stored_rejected_rows": 0,
                "stored_rows": [{
                    "accepted_rows": 1,
                    "attnum": null,
                    "batch_id": "t0.b0",
                    "key_digest": null,
                    "kind": "batch",
                    "physical_name": format!("pt_{shadow}"),
                    "reason": null,
                    "rejected_rows": 0,
                    "row_index": null,
                    "value_len": null
                }]
            },
            "run_id": "beecbbc7-8dda-4779-81ba-6ff0383f72ec",
            "schema_digest": schema_digest,
            "shadows_queried": false,
            "source_generation": 1,
            "source_id": "src",
            "source_introspection": "not_used",
            "state": "awaiting_publish",
            "tables": [{
                "accepted_rows": 1,
                "canonical_digest": digest,
                "columns": [{
                    "caveat": null,
                    "key": false,
                    "logical_name": "hub_token",
                    "oid": oid,
                    "source_type": source_type,
                    "stored_physical": format!("pc_{shadow}"),
                    "typmod": typmod
                }],
                "logical_schema": "public",
                "logical_table": "hubs",
                "rejects": [],
                "stored_namespace": format!("pn_{shadow}"),
                "stored_physical": format!("pt_{shadow}")
            }],
            "target_generation": 1,
            "tool_hook_digest": digest,
            "type_digest": digest
        })
        .to_string()
    }

    fn minimal(engine: &str) -> String {
        let mut preimage = String::new();
        push_schema_table(&mut preimage, "erp", "parcel");
        push_schema_column(&mut preimage, "id", 23, -1, None);
        let schema_digest = payload_sha256(preimage.as_bytes());
        let digest = "a".repeat(64);
        format!(
            r#"{{"capture":"sealed_import","config_digest":"{digest}","engine":"{engine}","hook_checked":false,"input_digest":"{digest}","protocol":"oo.pipeline.sealed-import-catalog/v1","quality":{{"loss_concealed":false,"manifest_reject_count":0,"payload_includes_reject_lines":false,"stored_omits_manifest_rejects":false,"stored_rejected_rows":0,"stored_rows":[{{"accepted_rows":1,"attnum":null,"batch_id":"t0.b0","key_digest":null,"kind":"batch","physical_name":"pt_{shadow}","reason":null,"rejected_rows":0,"row_index":null,"value_len":null}}]}},"run_id":"de8276fc-8634-4543-994f-ce82a46a3c18","schema_digest":"{schema_digest}","shadows_queried":false,"source_generation":1,"source_id":"src","source_introspection":"not_used","state":"awaiting_publish","tables":[{{"accepted_rows":1,"canonical_digest":"{digest}","columns":[{{"caveat":null,"key":true,"logical_name":"id","oid":23,"source_type":null,"stored_physical":"pc_{shadow}","typmod":-1}}],"logical_schema":"erp","logical_table":"parcel","rejects":[],"stored_namespace":"pn_{shadow}","stored_physical":"pt_{shadow}"}}],"target_generation":1,"tool_hook_digest":"{digest}","type_digest":"{digest}"}}"#,
            shadow = "ab".repeat(30)
        )
    }
}
