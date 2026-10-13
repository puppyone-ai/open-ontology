//! Sealed import catalog to a normalized target draft.
//!
//! The input is `oo.pipeline.sealed-import-catalog/v1`. The source engine and
//! the native caveats stay on the draft. Column storage is labeled
//! `postgresql_target_storage`. This module does not forge
//! `oo.source.pgsql/v1`, open a database, or copy shadow names into the draft.

use std::fmt;

use serde_json::Value;

use oo_core::api::payload_sha256;
use oo_core::sealed_import::{
    self, SealedImportCatalog, SealedImportColumn, SEALED_IMPORT_CATALOG_PROTOCOL,
    SEALED_IMPORT_DRAFT_PROTOCOL,
};

use crate::canonical::canonical_json;
use crate::pgsql;

pub const SEALED_IMPORT_COMPILER: &str = "oo-builder/sealed-import-draft/v1";
pub const REVIEWED_SEALED_IMPORT_PROTOCOL: &str = "oo.builder.reviewed-sealed-import/v1";
pub const REVIEWED_SEALED_IMPORT_COMPILER: &str = "oo-builder/reviewed-sealed-import/v1";
pub const REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL: &str =
    "oo.builder.reviewed-sealed-import-command/v1";
pub const SEALED_IMPORT_COLLECTION_PROTOCOL: &str = "oo.pipeline.sealed-import-collection/v1";

/// Static failure of the sealed-import compiler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportError {
    pub code: String,
    pub path: String,
    pub message: &'static str,
}

impl SealedImportError {
    pub fn draft(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{SEALED_IMPORT_DRAFT_PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }

    pub fn command(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }

    pub fn from_full(code: String, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code,
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

/// One logical table after normalization. Shadow names are absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportTableDraft {
    pub dataset: String,
    pub accepted_rows: u64,
    pub canonical_digest: String,
}

/// One logical column after normalization. Shadow names are absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportColumnDraft {
    pub dataset: String,
    pub name: String,
    pub fact_fingerprint: String,
    pub target_type: String,
    pub target_oid: i64,
    pub target_typmod: i32,
    pub caveat: Option<String>,
    pub source_type: Option<String>,
    pub key: bool,
    pub precision_loss: bool,
}

/// Compiled sealed import. `document` is canonical JSON and has no shadow names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedImportDraft {
    pub protocol: &'static str,
    pub compiler: &'static str,
    pub document: String,
    pub schema_digest: String,
    pub identity_hash: String,
    pub input_digest: String,
    pub config_digest: String,
    pub type_digest: String,
    pub source_engine: String,
    pub source_id: String,
    pub source_generation: i64,
    pub target_generation: i64,
    pub normalized_representation: &'static str,
    pub tables: Vec<SealedImportTableDraft>,
    pub columns: Vec<SealedImportColumnDraft>,
    /// Exact producer binding vector for a multi-catalog collection.
    pub binding_vector: Option<Value>,
    pub partition_vector: Option<Value>,
}

impl SealedImportDraft {
    /// Fingerprint of one logical column, recomputed from this draft.
    pub fn fact_fingerprint(&self, dataset: &str, column: &str) -> Option<&str> {
        self.columns
            .iter()
            .find(|item| item.dataset == dataset && item.name == column)
            .map(|item| item.fact_fingerprint.as_str())
    }
}

/// Compile one sealed-import catalog. The same path accepts every closed engine.
pub fn compile_sealed_import_catalog(
    catalog_json: &str,
) -> Result<SealedImportDraft, SealedImportError> {
    let catalog = sealed_import::parse_sealed_import_catalog(catalog_json).map_err(lift_core)?;
    let mut columns = Vec::new();
    for table in &catalog.tables {
        let dataset = format!("{}.{}", table.logical_schema, table.logical_table);
        for column in &table.columns {
            let precision_loss = precision_loss(column.oid, column.typmod);
            if column.key && precision_loss {
                return Err(SealedImportError::draft(
                    "precision_loss_key",
                    format!("/tables/{dataset}/{}", column.logical_name),
                    "a precision-loss column is a key",
                ));
            }
            columns.push(column_draft(&dataset, column, precision_loss));
        }
    }
    let identity_hash = payload_sha256(identity_preimage(&catalog).as_bytes());
    let partition_vector = catalog.partition_vector.as_ref().map(|vector| serde_json::to_value(vector).expect("validated partition vector is serializable"));
    let tables = catalog
        .tables
        .iter()
        .map(|table| SealedImportTableDraft {
            dataset: format!("{}.{}", table.logical_schema, table.logical_table),
            accepted_rows: table.accepted_rows,
            canonical_digest: table.canonical_digest.clone(),
        })
        .collect::<Vec<_>>();
    let body = draft_body(&catalog, &identity_hash, &columns);
    let document = canonical_json(&body);
    Ok(SealedImportDraft {
        protocol: SEALED_IMPORT_DRAFT_PROTOCOL,
        compiler: SEALED_IMPORT_COMPILER,
        document,
        schema_digest: catalog.schema_digest,
        identity_hash,
        input_digest: catalog.input_digest,
        config_digest: catalog.config_digest,
        type_digest: catalog.type_digest,
        source_engine: catalog.engine,
        source_id: catalog.source_id,
        source_generation: catalog.source_generation,
        target_generation: catalog.target_generation,
        normalized_representation: sealed_import::normalized_representation(),
        tables,
        columns,
        binding_vector: None,
        partition_vector,
    })
}

/// Compile a producer collection while retaining each independently sealed
/// catalog identity. The collection identity is bound to the exact vector;
/// it is never presented as a single schema or source-run digest.
pub fn compile_sealed_import_collection(input: &str) -> Result<SealedImportDraft, SealedImportError> {
    use std::collections::BTreeSet;
    let value = oo_core::json_scan::parse_json_value(input, SEALED_IMPORT_COLLECTION_PROTOCOL)
        .map_err(|_| SealedImportError::draft("json.syntax", "/", "collection JSON is invalid or exceeds limits"))?;
    let object = value.as_object().ok_or_else(|| SealedImportError::draft("schema.invalid", "/", "collection is not an object"))?;
    if object.len() != 4 || value.get("protocol").and_then(Value::as_str) != Some(SEALED_IMPORT_COLLECTION_PROTOCOL) {
        return Err(SealedImportError::draft("schema.invalid", "/protocol", "collection protocol or fields are invalid"));
    }
    let entries = value.get("entries").and_then(Value::as_array).ok_or_else(|| SealedImportError::draft("schema.invalid", "/entries", "collection entries are missing"))?;
    let vector = value.get("binding_vector").and_then(Value::as_array).ok_or_else(|| SealedImportError::draft("schema.invalid", "/binding_vector", "collection binding vector is missing"))?;
    if entries.is_empty() || entries.len() > 32 || entries.len() != vector.len() {
        return Err(SealedImportError::draft("schema.invalid", "/entries", "collection entry count is invalid"));
    }
    let mut preimage = value.clone();
    preimage.as_object_mut().expect("object checked").remove("collection_sha256");
    let actual = payload_sha256(canonical_json(&preimage).as_bytes());
    if value.get("collection_sha256").and_then(Value::as_str) != Some(actual.as_str()) {
        return Err(SealedImportError::draft("binding.collection_sha256", "/collection_sha256", "collection digest does not match its entries"));
    }
    let mut seen = BTreeSet::new();
    let mut drafts = Vec::with_capacity(entries.len());
    for (index, (entry, binding)) in entries.iter().zip(vector).enumerate() {
        let draft = compile_sealed_import_catalog(&canonical_json(entry))?;
        if draft.tables.len() != 1 || !seen.insert(draft.tables[0].dataset.clone()) {
            return Err(SealedImportError::draft("binding.dataset", format!("/entries/{index}"), "collection entry must contain one unique dataset"));
        }
        let table = &entry["tables"][0];
        let expected = serde_json::json!({
            "dataset": draft.tables[0].dataset,
            "source_id": entry["source_id"],
            "run_id": entry["run_id"],
            "source_generation": entry["source_generation"],
            "target_generation": entry["target_generation"],
            "config_digest": entry["config_digest"],
            "schema_digest": entry["schema_digest"],
            "type_digest": entry["type_digest"],
            "input_digest": entry["input_digest"],
            "stored_namespace": table["stored_namespace"],
            "stored_physical": table["stored_physical"]
        });
        if *binding != expected {
            return Err(SealedImportError::draft("binding.vector", format!("/binding_vector/{index}"), "collection binding differs from the sealed entry"));
        }
        if drafts.first().is_some_and(|first: &SealedImportDraft| first.source_engine != draft.source_engine) {
            return Err(SealedImportError::draft("binding.source", format!("/entries/{index}"), "collection source engine differs"));
        }
        drafts.push(draft);
    }
    let partition_vectors = drafts.iter().filter_map(|draft| draft.partition_vector.as_ref().map(|vector| serde_json::json!({"dataset": draft.tables[0].dataset, "partition_vector": vector}))).collect::<Vec<_>>();
    let source_ids = drafts.iter().map(|draft| draft.source_id.clone()).collect::<BTreeSet<_>>();
    let multiple_sources = source_ids.len() > 1;
    let mut combined = drafts.remove(0);
    if multiple_sources {
        combined.source_id.clear();
    }
    for draft in drafts {
        combined.source_generation = combined.source_generation.max(draft.source_generation);
        combined.target_generation = combined.target_generation.max(draft.target_generation);
        combined.tables.extend(draft.tables);
        combined.columns.extend(draft.columns);
    }
    combined.identity_hash = actual.clone();
    combined.schema_digest = String::new();
    combined.config_digest = String::new();
    combined.type_digest = String::new();
    combined.input_digest = String::new();
    combined.binding_vector = Some(Value::Array(vector.clone()));
    combined.partition_vector = if partition_vectors.is_empty() { None } else { Some(Value::Array(partition_vectors)) };
    combined.document = canonical_json(&serde_json::json!({"protocol": SEALED_IMPORT_COLLECTION_PROTOCOL, "collection_sha256": actual, "binding_vector": vector, "tables": combined.tables.iter().map(|table| &table.dataset).collect::<Vec<_>>() }));
    Ok(combined)
}

/// Identity preimage. A changed digest changes the hash that pack cache uses.
pub fn identity_preimage(catalog: &SealedImportCatalog) -> String {
    let mut text = format!(
        "sealed-import-identity/v1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        catalog.engine,
        catalog.source_id,
        catalog.config_digest,
        catalog.source_generation,
        catalog.target_generation,
        catalog.schema_digest,
        catalog.type_digest,
        catalog.input_digest,
        catalog.run_id
    );
    if let Some(vector) = &catalog.partition_vector {
        let value = serde_json::to_value(vector).expect("validated partition vector is serializable");
        text.push_str("partition-vector/v1\n");
        text.push_str(&payload_sha256(canonical_json(&value).as_bytes()));
        text.push('\n');
    }
    text
}

fn column_draft(
    dataset: &str,
    column: &SealedImportColumn,
    precision_loss: bool,
) -> SealedImportColumnDraft {
    let fact_fingerprint = payload_sha256(column_preimage(dataset, column).as_bytes());
    SealedImportColumnDraft {
        dataset: dataset.to_string(),
        name: column.logical_name.clone(),
        fact_fingerprint,
        target_type: column.target_type.to_string(),
        target_oid: column.oid,
        target_typmod: column.typmod,
        caveat: column.caveat.clone(),
        source_type: column.source_type.clone(),
        key: column.key,
        precision_loss,
    }
}

fn column_preimage(dataset: &str, column: &SealedImportColumn) -> String {
    format!(
        "sealed-import-column/v1\n{dataset}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        column.logical_name,
        column.oid,
        column.typmod,
        column.caveat.as_deref().unwrap_or(""),
        if column.key { "true" } else { "false" },
        column.source_type.as_deref().unwrap_or("")
    )
}

fn draft_body(
    catalog: &SealedImportCatalog,
    identity_hash: &str,
    columns: &[SealedImportColumnDraft],
) -> Value {
    let mut tables = Vec::new();
    for table in &catalog.tables {
        let dataset = format!("{}.{}", table.logical_schema, table.logical_table);
        let table_columns: Vec<Value> = columns
            .iter()
            .filter(|column| column.dataset == dataset)
            .map(column_json)
            .collect();
        let rejects: Vec<Value> = table
            .rejects
            .iter()
            .map(|reject| {
                serde_json::json!({
                    "column": reject.column,
                    "dataset": dataset,
                    "reason": reject.reason,
                    "row_index": reject.row_index,
                })
            })
            .collect();
        tables.push(serde_json::json!({
            "accepted_rows": table.accepted_rows,
            "canonical_digest": table.canonical_digest,
            "columns": table_columns,
            "dataset": dataset,
            "rejects": rejects,
        }));
    }
    let mut body = serde_json::json!({
        "capture": "sealed_import",
        "columns": columns.iter().map(column_json).collect::<Vec<_>>(),
        "compiler": SEALED_IMPORT_COMPILER,
        "config_digest": catalog.config_digest,
        "identity_hash": identity_hash,
        "input_digest": catalog.input_digest,
        "live_database": "not_connected",
        "normalized_representation": sealed_import::normalized_representation(),
        "protocol": SEALED_IMPORT_DRAFT_PROTOCOL,
        "run_id": catalog.run_id,
        "schema_digest": catalog.schema_digest,
        "source_connection": "not_used",
        "source_engine": catalog.engine,
        "source_generation": catalog.source_generation,
        "source_id": catalog.source_id,
        "source_introspection": "not_used",
        "source_protocol": SEALED_IMPORT_CATALOG_PROTOCOL,
        "tables": tables,
        "target_generation": catalog.target_generation,
        "type_digest": catalog.type_digest,
    });
    if let Some(vector) = &catalog.partition_vector {
        body["partition_vector"] = serde_json::to_value(vector).expect("validated partition vector is serializable");
    }
    body
}

fn column_json(column: &SealedImportColumnDraft) -> Value {
    serde_json::json!({
        "caveat": column.caveat,
        "dataset": column.dataset,
        "fact_fingerprint": column.fact_fingerprint,
        "key": column.key,
        "name": column.name,
        "precision_loss": column.precision_loss,
        "source_type": column.source_type,
        "target_oid": column.target_oid,
        "target_type": column.target_type,
        "target_typmod": column.target_typmod,
    })
}

fn precision_loss(oid: i64, typmod: i32) -> bool {
    if matches!(oid, 700 | 701) {
        return true;
    }
    if oid != 1700 {
        return false;
    }
    match pgsql::numeric_parts(typmod) {
        None => true,
        Some((precision, _)) if precision > 38 => true,
        Some(_) => false,
    }
}

fn lift_core(err: sealed_import::SealedImportError) -> SealedImportError {
    SealedImportError::from_full(err.code, err.path, err.message)
}
