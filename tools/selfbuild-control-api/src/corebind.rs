//! Calls into fixed `oo-core` at `983577c9b89453b08ab4e1a02cab88df78c3e005`.
//!
//! `derive_source_bound` runs only for an Oracle sealed-import catalog plus an
//! Oracle source-bound dictionary. A PostgreSQL capture is refused here and is
//! not relabeled as Oracle. PostgreSQL storage names stay in `adapt`; this
//! module only answers whether a PII token or a semantic token is one of the
//! core enums. `unknown` is not a core PII level.

use serde_json::{json, Value};

pub const CORE_COMMIT: &str = "983577c9b89453b08ab4e1a02cab88df78c3e005";
pub const SEALED_CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";
pub const SOURCE_BOUND_PROTOCOL: &str = "oo.source-semantics.source-bound/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreCallError {
    pub code: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleReceipt {
    pub called: bool,
    pub identity_hash: String,
    pub engine: String,
    pub source_id: String,
    pub version: String,
    pub consistency_grade: Option<String>,
    pub delete_detection: Option<String>,
    pub column_count: usize,
    pub bytea_columns: Vec<String>,
    pub target_types: Vec<(String, String)>,
}

pub fn core_pii_level(value: &str) -> bool {
    serde_json::from_value::<oo_core::config::model::PiiLevel>(Value::String(value.to_owned())).is_ok()
}

pub fn core_semantic(value: &str) -> bool {
    serde_json::from_value::<oo_core::oos::SemanticType>(Value::String(value.to_owned())).is_ok()
}

/// Derive one Oracle observation.
///
/// `catalog_json` and `bound_json` are the sealed catalog object and the
/// source-bound object. A postgres capture, or any catalog whose engine is
/// not `oracle`, returns `engine_not_oracle` without calling
/// `derive_source_bound`.
pub fn derive_oracle(catalog_json: &str, bound_json: &str) -> Result<OracleReceipt, CoreCallError> {
    let catalog = serde_json::from_str::<Value>(catalog_json).map_err(|_| CoreCallError { code: "catalog.malformed" })?;
    let bound = serde_json::from_str::<Value>(bound_json).map_err(|_| CoreCallError { code: "bound.malformed" })?;
    if catalog.get("protocol").and_then(Value::as_str) == Some(crate::adapt::CAPTURE_CATALOG) {
        return Err(CoreCallError { code: "engine_not_oracle" });
    }
    if catalog.get("engine").and_then(Value::as_str) != Some("oracle") || bound.get("engine").and_then(Value::as_str) != Some("oracle") {
        return Err(CoreCallError { code: "engine_not_oracle" });
    }
    if catalog.get("protocol").and_then(Value::as_str) != Some(SEALED_CATALOG_PROTOCOL) {
        return Err(CoreCallError { code: "protocol.mismatch" });
    }
    let derived = oo_core::source_bound::derive_source_bound(catalog_json, bound_json).map_err(|err| CoreCallError {
        code: static_code(&err.code),
    })?;
    let parsed = oo_core::sealed_import::parse_sealed_import_catalog(catalog_json).map_err(|err| CoreCallError { code: static_code(&err.code) })?;
    let mut bytea_columns = Vec::new();
    let mut target_types = Vec::new();
    for table in &parsed.tables {
        for column in &table.columns {
            target_types.push((column.logical_name.clone(), column.target_type.to_owned()));
            if column.target_type == "bytea" {
                bytea_columns.push(column.logical_name.clone());
            }
        }
    }
    target_types.sort();
    bytea_columns.sort();
    Ok(OracleReceipt {
        called: true,
        identity_hash: derived.identity_hash,
        engine: derived.engine,
        source_id: derived.source_id,
        version: derived.version,
        consistency_grade: derived.consistency_grade,
        delete_detection: derived.delete_detection,
        column_count: derived.columns.len(),
        bytea_columns,
        target_types,
    })
}

pub fn oracle_receipt_json(receipt: &OracleReceipt, profile_identity: &str) -> Value {
    json!({
        "bytea_columns": receipt.bytea_columns,
        "called": receipt.called,
        "column_count": receipt.column_count,
        "consistency_grade": receipt.consistency_grade,
        "core_commit": CORE_COMMIT,
        "delete_detection": receipt.delete_detection,
        "engine": receipt.engine,
        "identity_hash": receipt.identity_hash,
        "identity_match": receipt.identity_hash == profile_identity,
        "profile_identity_hash": profile_identity,
        "source_id": receipt.source_id,
        "target_types": receipt.target_types.iter().map(|(name, target)| json!({"column": name, "target_type": target})).collect::<Vec<_>>(),
        "version": receipt.version
    })
}

fn static_code(code: &str) -> &'static str {
    match code.rsplit('/').next().unwrap_or("") {
        "protocol.mismatch" => "protocol.mismatch",
        "token.rejected" => "token.rejected",
        "schema.invalid" => "schema.invalid",
        "observation.missing" => "observation.missing",
        "json.syntax" => "json.syntax",
        "json.duplicate_key" => "json.duplicate_key",
        "json.budget.bytes" => "json.budget.bytes",
        "json.budget.depth" => "json.budget.depth",
        "json.budget.nodes" => "json.budget.nodes",
        "field.unknown" => "field.unknown",
        "field.missing" => "field.missing",
        "digest.mismatch" => "digest.mismatch",
        "column.mismatch" => "column.mismatch",
        "cross_source" => "cross_source",
        "cross_run" => "cross_run",
        "fidelity.contradiction" => "fidelity.contradiction",
        "secret.rejected" => "secret.rejected",
        "seal.unsealed" => "seal.unsealed",
        "seal.stale" => "seal.stale",
        "quality.rejected" => "quality.rejected",
        _ => "source_bound_rejected",
    }
}
