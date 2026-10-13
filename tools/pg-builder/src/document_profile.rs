//! Document profile for one sealed jsonb column.
//!
//! This calls the fixed archive compilers. It does not open a database, does
//! not alter a type, and does not call a model.

use serde_json::{Value, json};

use oo_builder::{
    REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL, compile_jsonb_document_profile,
    compile_reviewed_sealed_import, compile_role_pack, compile_sealed_import_catalog,
};

use crate::source_contract::project_sealed_source_contract;
use crate::PgBuilderError;

/// Profile command protocol. Task 41 reads this object.
pub const PROTOCOL: &str = "oo.pg-builder.document-profile/v1";

/// Inputs for one profile. Pack fields are all present or all absent.
pub struct DocumentProfileIn<'a> {
    pub catalog_json: &'a str,
    pub document_json: &'a str,
    pub settings_json: Option<&'a str>,
    pub visibility_json: Option<&'a str>,
    pub reviewer_by: Option<&'a str>,
    pub reviewer_at: Option<&'a str>,
    pub source_contract_json: Option<&'a str>,
    /// Optional `{column, semantic_type}` rows, or an object with a `decisions` array.
    /// Every sealed column must appear once. Absence uses the storage type.
    pub decisions_json: Option<&'a str>,
}

/// Compile the sealed catalog and the local document profile.
///
/// A pack is the archive role pack for this catalog. Its type cell is jsonb.
/// `document_storage` lines are appended later by the source-semantics packer
/// from `profile.document`. This command does not compile that tool.
pub fn compile_document_profile(input: DocumentProfileIn<'_>) -> Result<Value, PgBuilderError> {
    let profile = compile_jsonb_document_profile(input.catalog_json, input.document_json)
        .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?;
    let draft = compile_sealed_import_catalog(input.catalog_json)
        .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?;
    let dataset = profile.dataset.clone();
    let document_column = profile.column.clone();
    let mut body = json!({
        "protocol": PROTOCOL,
        "outcome": "compiled",
        "ready": false,
        "partial_ready": false,
        "pg_source_passed": false,
        "model": "not_called",
        "compiler": profile.compiler,
        "profile_protocol": profile.protocol,
        "document": profile.document,
        "document_identity_hash": profile.document_identity_hash,
        "schema_digest": profile.schema_digest,
        "catalog_identity_hash": draft.identity_hash,
        "source_engine": profile.source_engine,
        "dataset": dataset,
        "column": document_column.clone(),
        "raw_bson_sha256": profile.raw_bson_sha256,
        "pg_jsonb_sha256": profile.pg_jsonb_sha256,
        "normalized_representation": profile.normalized_representation,
        "pack": Value::Null,
    });
    if input.settings_json.is_none()
        && input.visibility_json.is_none()
        && input.reviewer_by.is_none()
        && input.reviewer_at.is_none()
        && input.source_contract_json.is_none()
    {
        return Ok(body);
    }
    let (Some(settings_json), Some(visibility_json), Some(reviewer_by), Some(reviewer_at)) = (
        input.settings_json,
        input.visibility_json,
        input.reviewer_by,
        input.reviewer_at,
    ) else {
        return Err(PgBuilderError::new(
            "usage",
            "document profile pack arguments are incomplete",
        ));
    };
    if !closed_reviewer(reviewer_by) || !closed_date(reviewer_at) {
        return Err(PgBuilderError::new(
            "schema.invalid",
            "document profile reviewer is closed",
        ));
    }
    let supplied = supplied_semantics(input.decisions_json)?;
    let mut decisions = Vec::new();
    for item in &draft.columns {
        let semantic = if let Some(rows) = &supplied {
            rows.get(&item.name).copied().ok_or_else(|| {
                PgBuilderError::new(
                    "schema.invalid",
                    "document pack decision does not cover a catalog column",
                )
            })?
        } else {
            semantic_for_storage(item, &profile.dataset, &document_column)?
        };
        decisions.push(json!({
            "column": item.name,
            "dataset": item.dataset,
            "fact_fingerprint": item.fact_fingerprint,
            "pii": "none",
            "reviewer": {"at": reviewer_at, "by": reviewer_by},
            "schema_hash": draft.schema_digest,
            "semantic_type": semantic,
        }));
    }
    if supplied.is_some_and(|rows| rows.len() != draft.columns.len()) {
        return Err(PgBuilderError::new(
            "schema.invalid",
            "document pack decisions do not match the catalog columns",
        ));
    }
    let settings: Value = serde_json::from_str(settings_json).map_err(|_| {
        PgBuilderError::new("input.schema", "document profile settings are not JSON")
    })?;
    let catalog: Value = serde_json::from_str(input.catalog_json).map_err(|_| {
        PgBuilderError::new("input.schema", "document profile catalog is not JSON")
    })?;
    let command = json!({
        "protocol": REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL,
        "catalog": catalog.clone(),
        "settings": settings,
        "decisions": decisions,
    });
    let command_text = serde_json::to_string(&command).map_err(|_| {
        PgBuilderError::new("input.schema", "document profile command is not JSON")
    })?;
    let reviewed = compile_reviewed_sealed_import(&command_text)
        .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?;
    let reviewed_value: Value = serde_json::from_str(&reviewed).map_err(|_| {
        PgBuilderError::new("compiler.contract", "reviewed sealed import is not JSON")
    })?;
    let effective = reviewed_value.get("effective_oos").ok_or_else(|| {
        PgBuilderError::new("compiler.contract", "reviewed sealed import has no effective document")
    })?;
    let mut effective_value = effective.clone();
    let contract = match input.source_contract_json {
        Some(text) => Some(
            serde_json::from_str::<Value>(text)
                .map_err(|_| PgBuilderError::new("input.schema", "source contract is not JSON"))?,
        ),
        None => None,
    };
    project_sealed_source_contract(&mut effective_value, &catalog, contract.as_ref())?;
    let effective_text = serde_json::to_string(&effective_value).map_err(|_| {
        PgBuilderError::new("compiler.contract", "effective document is not JSON")
    })?;
    let pack = compile_role_pack(&effective_text, visibility_json)
        .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?;
    let page_path = format!("tables/{}.md", profile.dataset);
    let page = pack
        .file(&page_path)
        .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?
        .to_string();
    let type_cell = format!("| `{document_column}` |  | jsonb | json | none |  |");
    if !page.contains(&type_cell) {
        return Err(PgBuilderError::new(
            "compiler.contract",
            "document role page does not name jsonb",
        ));
    }
    body["pack"] = json!({
        "pack_digest": pack.pack_digest,
        "page_path": page_path,
        "page": page,
        "ready": false,
    });
    Ok(body)
}

fn supplied_semantics(
    text: Option<&str>,
) -> Result<Option<std::collections::BTreeMap<String, &'static str>>, PgBuilderError> {
    let Some(text) = text else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(text)
        .map_err(|_| PgBuilderError::new("input.schema", "document profile decisions are not JSON"))?;
    let rows = if let Some(array) = value.as_array() {
        array
    } else if let Some(array) = value.get("decisions").and_then(Value::as_array) {
        array
    } else if let Some(array) = value.pointer("/binding/decisions").and_then(Value::as_array) {
        array
    } else {
        return Err(PgBuilderError::new(
            "schema.invalid",
            "document pack decisions are not a list",
        ));
    };
    let mut out = std::collections::BTreeMap::new();
    for row in rows {
        let column = row.get("column").and_then(Value::as_str).ok_or_else(|| {
            PgBuilderError::new("schema.invalid", "document pack decision omits a column")
        })?;
        let semantic = row.get("semantic_type").and_then(Value::as_str).ok_or_else(|| {
            PgBuilderError::new("schema.invalid", "document pack decision omits a semantic type")
        })?;
        let token = known_semantic(semantic).ok_or_else(|| {
            PgBuilderError::new("schema.invalid", "document pack semantic type is not closed")
        })?;
        if out.insert(column.to_string(), token).is_some() {
            return Err(PgBuilderError::new(
                "schema.invalid",
                "document pack decisions repeat a column",
            ));
        }
    }
    Ok(Some(out))
}

fn semantic_for_storage(
    item: &oo_builder::SealedImportColumnDraft,
    dataset: &str,
    document_column: &str,
) -> Result<&'static str, PgBuilderError> {
    if item.dataset == dataset && item.name == document_column && item.target_type == "jsonb" {
        return Ok("json");
    }
    if item.key && item.target_type == "int4" {
        return Ok("identifier");
    }
    match item.target_type.as_str() {
        "int2" | "int4" | "int8" | "numeric" | "float4" | "float8" => Ok("quantity"),
        "bool" => Ok("boolean_flag"),
        "text" | "varchar" | "bpchar" => Ok("free_text"),
        "json" | "jsonb" => Ok("json"),
        "bytea" => Ok("system"),
        "date" | "timestamp" | "timestamptz" => Ok("event_time"),
        "uuid" => Ok("identifier"),
        _ => Err(PgBuilderError::new(
            "schema.invalid",
            "document pack column has no legal semantic type",
        )),
    }
}

fn known_semantic(value: &str) -> Option<&'static str> {
    Some(match value {
        "identifier" => "identifier",
        "foreign_key" => "foreign_key",
        "entity_name" => "entity_name",
        "code" => "code",
        "status_code" => "status_code",
        "category" => "category",
        "boolean_flag" => "boolean_flag",
        "money" => "money",
        "quantity" => "quantity",
        "ratio" => "ratio",
        "event_time" => "event_time",
        "date" => "date",
        "audit_time" => "audit_time",
        "geo" => "geo",
        "address" => "address",
        "phone" => "phone",
        "email" => "email",
        "person_name" => "person_name",
        "free_text" => "free_text",
        "json" => "json",
        "system" => "system",
        _ => return None,
    })
}

fn closed_reviewer(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
}

fn closed_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_core::api::payload_sha256;
    use oo_core::sealed_import::{push_schema_column, push_schema_table};
    use serde_json::{Value, json};

    const RAW: &[u8] = include_bytes!("../../../crates/oo-core/tests/fixtures/jsonb3802/canonical-bson.bin");
    const PG: &[u8] = include_bytes!("../../../crates/oo-core/tests/fixtures/jsonb3802/canonical.bin");

    #[test]
    fn profile_and_pack_keep_jsonb_3802_and_refuse_a_dropped_document() {
        let catalog = catalog_json();
        let document = document_json();
        let profile = compile_document_profile(DocumentProfileIn {
            catalog_json: &catalog,
            document_json: &document,
            settings_json: None,
            visibility_json: None,
            reviewer_by: None,
            reviewer_at: None,
            source_contract_json: None,
            decisions_json: None,
        })
        .expect("profile");
        assert_eq!(profile["outcome"], "compiled");
        assert_eq!(profile["ready"], false);
        assert_eq!(profile["partial_ready"], false);
        assert_eq!(profile["pg_source_passed"], false);
        assert_eq!(profile["model"], "not_called");
        assert_eq!(profile["profile_protocol"], "oo.builder.document-jsonb-profile/v1");
        assert_eq!(profile["compiler"], "compile_jsonb_document_profile");
        assert_eq!(profile["raw_bson_sha256"], payload_sha256(RAW));
        assert_eq!(profile["pg_jsonb_sha256"], payload_sha256(PG));
        let local = profile["document"].as_str().expect("document");
        assert!(local.contains("9223372036854775807"));
        assert!(local.contains("1.2300"));
        assert!(local.contains("absent_not_null") || local.contains("\"oo.document.missing\":\"absent_not_null\"") || local.contains("absent_not_null"));
        assert!(!local.contains(&hex_encode(RAW)));
        assert!(profile["pack"].is_null());
        let identity = profile["catalog_identity_hash"].as_str().expect("identity");
        let settings = settings_json(identity);
        let visibility = visibility_json(identity);
        let packed = compile_document_profile(DocumentProfileIn {
            catalog_json: &catalog,
            document_json: &document,
            settings_json: Some(&settings),
            visibility_json: Some(&visibility),
            reviewer_by: Some("ada.owner"),
            reviewer_at: Some("2026-10-11"),
            source_contract_json: None,
            decisions_json: None,
        })
        .expect("pack");
        let page = packed["pack"]["page"].as_str().expect("page");
        assert!(page.contains("| `doc` |  | jsonb | json | none |  |"));
        assert!(page.contains("| `id` |  | int4 | identifier | none |  |"));
        assert!(!page.contains("json_not_jsonb"));
        assert!(!page.contains("## 来源与语义"));
        assert_eq!(packed["pack"]["ready"], false);
        let mut dropped: Value = serde_json::from_str(&document).unwrap();
        dropped["raw_bson_dropped"] = json!(true);
        dropped["raw_bson_hex"] = json!("");
        let err = compile_document_profile(DocumentProfileIn {
            catalog_json: &catalog,
            document_json: &dropped.to_string(),
            settings_json: None,
            visibility_json: None,
            reviewer_by: None,
            reviewer_at: None,
            source_contract_json: None,
            decisions_json: None,
        })
        .expect_err("dropped");
        assert!(err.code.ends_with("/raw_bson_dropped"), "{}", err.code);
    }

    #[test]
    fn shipment_catalog_keeps_weight_and_the_nineteen_approved_columns() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../evidence/1e7798c0-f799-4d4b-ab47-5e15599f60d9/DL01/semantics/bson-double/seal",
        );
        let command: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("command.json")).expect("command"),
        )
        .expect("command json");
        let catalog = command["catalog"].to_string();
        let document = std::fs::read_to_string(root.join("customer.envelope.json")).expect("envelope");
        let profile = compile_document_profile(DocumentProfileIn {
            catalog_json: &catalog,
            document_json: &document,
            settings_json: None,
            visibility_json: None,
            reviewer_by: None,
            reviewer_at: None,
            source_contract_json: None,
            decisions_json: None,
        })
        .expect("profile");
        let local = profile["document"].as_str().expect("document");
        assert!(local.contains("\"$numberDouble\":\"41.25\"") || local.contains("\"$numberDouble\": \"41.25\""));
        assert!(local.contains("weight"));
        assert_eq!(profile["compiler"], "compile_jsonb_document_profile");
        assert_eq!(profile["ready"], false);
        let identity = profile["catalog_identity_hash"].as_str().expect("identity");
        let names: Vec<String> = command["catalog"]["tables"][0]["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .map(|column| column["logical_name"].as_str().expect("name").to_string())
            .collect();
        assert_eq!(names.len(), 20);
        assert!(names.iter().any(|name| name == "weight"));
        assert!(names.iter().any(|name| name == "doc"));
        let settings = json!({
            "project": "shipments",
            "ontology_version": "ont-1",
            "catalog_version": identity,
            "policy": {"id": "pol-ship", "version": "1"},
            "knowledge": {"id": "know-ship", "version": "1"},
            "ontology": {
                "spec_version": "oos/v1",
                "name": "shipments",
                "locale": "en-US",
                "include_schemas": ["oo_w141"],
                "default_tier_preference": ["raw"],
                "publish": {"include_unreviewed": []},
                "classification": {"auto_accept": "rule_high_confidence"},
                "lifecycle": {"deprecation_min_releases": 2},
                "compile": {
                    "per_role": true,
                    "budgets": {"readme_tokens": 4000, "table_file_tokens": 8000},
                    "sample_values": {"default": "masked", "max_per_column": 0}
                }
            }
        })
        .to_string();
        let visibility = json!({
            "protocol": "oo.builder.role-visibility/v1",
            "project": "shipments",
            "principal": "ada",
            "role": "oo_r_clerk",
            "ontology_version": "ont-1",
            "catalog_version": identity,
            "policy": {"id": "pol-ship", "version": "1"},
            "knowledge": {"id": "know-ship", "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": [{
                "dataset": "oo_w141.shipments",
                "row_security": "none",
                "global_stats": "denied",
                "columns": names
            }],
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        })
        .to_string();
        let decisions = command["binding"].to_string();
        let packed = compile_document_profile(DocumentProfileIn {
            catalog_json: &catalog,
            document_json: &document,
            settings_json: Some(&settings),
            visibility_json: Some(&visibility),
            reviewer_by: Some("dl01-w1-41"),
            reviewer_at: Some("2026-10-12"),
            source_contract_json: None,
            decisions_json: Some(&decisions),
        })
        .expect("pack");
        let page = packed["pack"]["page"].as_str().expect("page");
        for name in &names {
            assert!(page.contains(&format!("`{name}`")), "{name} missing from {page}");
        }
        assert!(page.contains("| `doc` |  | jsonb | json | none |  |"));
        assert!(page.contains("| `weight` |"));
        assert!(!page.contains("json_not_jsonb"));
        assert_eq!(packed["pack"]["ready"], false);
        assert_eq!(packed["ready"], false);
    }

    fn document_json() -> String {
        let text = std::str::from_utf8(&PG[1..]).expect("text");
        json!({
            "protocol": "oo.document.bson-jsonb/v1",
            "dataset": "public.docs",
            "column": "doc",
            "oid": 3802,
            "typmod": -1,
            "source_type": "jsonb",
            "raw_bson_dropped": false,
            "raw_bson_hex": hex_encode(RAW),
            "raw_bson_sha256": payload_sha256(RAW),
            "pg_jsonb_hex": hex_encode(PG),
            "pg_jsonb_text": text,
            "loss_concealed": false,
            "missing_is_null": false
        })
        .to_string()
    }

    fn catalog_json() -> String {
        let mut preimage = String::new();
        push_schema_table(&mut preimage, "public", "docs");
        push_schema_column(&mut preimage, "id", 23, -1, None);
        push_schema_column(&mut preimage, "doc", 3802, -1, None);
        let schema_digest = payload_sha256(preimage.as_bytes());
        let digest = "a".repeat(64);
        let shadow = "ab".repeat(30);
        json!({
            "capture": "sealed_import",
            "config_digest": digest,
            "engine": "mongodb",
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
            "source_id": "mongo_docs",
            "source_introspection": "not_used",
            "state": "awaiting_publish",
            "tables": [{
                "accepted_rows": 1,
                "canonical_digest": digest,
                "columns": [
                    {
                        "caveat": null,
                        "key": true,
                        "logical_name": "id",
                        "oid": 23,
                        "source_type": null,
                        "stored_physical": format!("pc_{shadow}"),
                        "typmod": -1
                    },
                    {
                        "caveat": null,
                        "key": false,
                        "logical_name": "doc",
                        "oid": 3802,
                        "source_type": "jsonb",
                        "stored_physical": format!("pc_{}", "cd".repeat(30)),
                        "typmod": -1
                    }
                ],
                "logical_schema": "public",
                "logical_table": "docs",
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

    fn settings_json(identity: &str) -> String {
        json!({
            "project": "docs",
            "ontology_version": "ont-1",
            "catalog_version": identity,
            "policy": {"id": "pol-docs", "version": "1"},
            "knowledge": {"id": "know-docs", "version": "1"},
            "ontology": {
                "spec_version": "oos/v1",
                "name": "docs",
                "locale": "en-US",
                "include_schemas": ["public"],
                "default_tier_preference": ["raw"],
                "publish": {"include_unreviewed": []},
                "classification": {"auto_accept": "rule_high_confidence"},
                "lifecycle": {"deprecation_min_releases": 2},
                "compile": {
                    "per_role": true,
                    "budgets": {"readme_tokens": 2000, "table_file_tokens": 4000},
                    "sample_values": {"default": "masked", "max_per_column": 0}
                }
            }
        })
        .to_string()
    }

    fn visibility_json(identity: &str) -> String {
        json!({
            "protocol": "oo.builder.role-visibility/v1",
            "project": "docs",
            "principal": "ada",
            "role": "oo_r_clerk",
            "ontology_version": "ont-1",
            "catalog_version": identity,
            "policy": {"id": "pol-docs", "version": "1"},
            "knowledge": {"id": "know-docs", "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": [{
                "dataset": "public.docs",
                "row_security": "none",
                "global_stats": "denied",
                "columns": ["id", "doc"]
            }],
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        })
        .to_string()
    }

    fn hex_encode(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }
}
