//! Capture receipt and native-update request for `oo_w141.shipments`.
//!
//! Digests are sha256 of bytes this module writes or of the audit file it
//! reads. The receipt file hash is computed after those bytes exist. The
//! catalog table is `oo_w141.shipments`. A `scope_refused` row is not loaded.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use crate::arrow_utf8::{self, Utf8Cell, Utf8Column};
use crate::docseal::{self, APPROVED_FIELDS};
use oo_core::api::payload_sha256;
use oo_core::jsonb_doc;
use oo_core::sealed_import::{self, push_schema_column, push_schema_table};

pub const RECEIPT_SCHEMA: &str = "oo.source-admission.capture-receipt/v1";
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.native-update/v1";
pub const DATASET: &str = "oo_w141.shipments";
pub const SOURCE_ID: &str = "oo_w141";
pub const DELETED_KEY: &str = "c141d04141eef0410000000f";
pub const TEMPORARY_KEY: &str = "c141d04141eef04100000011";

#[derive(Debug)]
pub struct BindFault {
    pub code: &'static str,
    pub message: &'static str,
}

pub struct PackInput<'a> {
    pub seal: &'a Value,
    pub seal_bytes: &'a [u8],
    pub audit_bytes: &'a [u8],
    pub unclassified: Option<&'a Value>,
    pub unclassified_bytes: Option<&'a [u8]>,
    pub driver_bytes: &'a [u8],
    pub generation: i64,
    pub parent: Option<i64>,
    pub full_snapshot: bool,
    pub arrow_path: &'a str,
    pub receipt_path: &'a str,
    pub registry_path: &'a str,
    pub catalog_path: &'a str,
    pub audit_path: &'a str,
    pub driver_path: &'a str,
    pub password_file: &'a str,
    pub database: &'a str,
    pub port: u16,
    pub shard: &'a str,
    pub owner: &'a str,
}

pub struct PackFiles {
    pub arrow: Vec<u8>,
    pub config: Vec<u8>,
    pub hook: Vec<u8>,
    pub catalog: Vec<u8>,
    pub registry: Vec<u8>,
    pub receipt: Vec<u8>,
    pub request: Vec<u8>,
    pub census: Vec<u8>,
    pub summary: Value,
}

struct Row {
    key: String,
    doc_text: String,
    truth_text: String,
    weight: String,
}

pub fn build_pack(input: &PackInput<'_>) -> Result<PackFiles, BindFault> {
    if input.generation < 1 || (input.parent.is_none() && input.generation != 1) {
        return Err(fault(
            "stale_run",
            "source generation does not follow the parent rule",
        ));
    }
    if input
        .parent
        .is_some_and(|parent| parent.checked_add(1) != Some(input.generation))
    {
        return Err(fault(
            "stale_run",
            "parent generation is not the previous generation",
        ));
    }
    if input.full_snapshot && input.parent.is_none() {
        return Err(fault(
            "stale_run",
            "a full snapshot replace needs a parent generation",
        ));
    }
    absolute_path(input.arrow_path)?;
    absolute_path(input.receipt_path)?;
    absolute_path(input.registry_path)?;
    absolute_path(input.catalog_path)?;
    absolute_path(input.audit_path)?;
    absolute_path(input.driver_path)?;
    if input.driver_bytes.is_empty() {
        return Err(fault("digest_mismatch", "driver file is empty"));
    }
    if input.unclassified.is_some() != input.unclassified_bytes.is_some() {
        return Err(fault(
            "field.missing",
            "unclassified document and bytes must be paired",
        ));
    }
    let audit: Value = serde_json::from_slice(input.audit_bytes)
        .map_err(|_| fault("no_audit", "audit file is not JSON"))?;
    if audit.get("reader_dml").and_then(Value::as_i64) != Some(0) {
        return Err(fault("no_audit", "reader DML is not zero"));
    }
    let rows = sealed_rows(input.seal)?;
    let mut refused = refused_rows(input.seal);
    if let Some(extra) = input.unclassified {
        for item in refused_rows(extra) {
            let key = item.get("key").cloned().unwrap_or(Value::Null);
            if !refused.iter().any(|row| row.get("key") == Some(&key)) {
                refused.push(item);
            }
        }
    }
    if rows.len() + refused.len() > 16 {
        return Err(fault(
            "read_budget",
            "sealed plus unclassified rows are above the read budget",
        ));
    }
    if refused.iter().any(|item| {
        item.get("key")
            .and_then(Value::as_str)
            .is_some_and(|key| rows.iter().any(|row| row.key == key))
    }) {
        return Err(fault("field.scope", "an unclassified key is also sealed"));
    }
    let arrow_rows: Vec<Vec<Utf8Cell>> = rows
        .iter()
        .map(|row| {
            vec![
                Utf8Cell::Text(row.key.clone()),
                Utf8Cell::Text(row.doc_text.clone()),
            ]
        })
        .collect();
    let arrow = arrow_utf8::write_utf8_file(
        &[
            Utf8Column {
                name: "_id".to_string(),
                nullable: false,
            },
            Utf8Column {
                name: "doc".to_string(),
                nullable: false,
            },
        ],
        &arrow_rows,
    )
    .map_err(|_| fault("bad_type", "utf8 arrow file could not be written"))?;
    let audit_digest = payload_sha256(input.audit_bytes);
    let driver_sha = payload_sha256(input.driver_bytes);
    let unclassified_sha = input.unclassified_bytes.map(payload_sha256);
    let schema_digest = schema_preimage_digest();
    let type_digest = payload_sha256(TYPE_PREIMAGE.as_bytes());
    let config_value = config_document(
        input.generation,
        unclassified_sha.as_deref(),
        rows.len(),
        refused.len(),
    );
    let config = file_json(&config_value);
    let config_digest = payload_sha256(&config);
    let input_digest = arrow.file_sha256.clone();
    let hook_value = json!({
        "hook_checked": false,
        "reason": "tool hook is not part of this capture"
    });
    let hook = file_json(&hook_value);
    let tool_hook_digest = payload_sha256(&hook);
    let run_id = derived_run_id(input.generation, &arrow.file_sha256);
    let batch_id = format!("w141-g{}", input.generation);
    let snapshot_value = payload_sha256(input.seal_bytes);
    let catalog_value = catalog_document(
        input.generation,
        &run_id,
        rows.len(),
        &CatalogDigests {
            schema: &schema_digest,
            type_digest: &type_digest,
            config: &config_digest,
            input: &input_digest,
            tool_hook: &tool_hook_digest,
            canonical: &arrow.file_sha256,
        },
    )?;
    let catalog = file_json(&catalog_value);
    sealed_import::parse_sealed_import_catalog(
        &String::from_utf8(catalog.clone())
            .map_err(|_| fault("schema.invalid", "catalog bytes are not utf-8"))?,
    )
    .map_err(|_| fault("schema.invalid", "shipments catalog did not parse"))?;
    let catalog_sha256 = payload_sha256(&catalog);
    if arrow.schema_sha256 == schema_digest {
        return Err(fault(
            "digest_mismatch",
            "arrow schema message hash equals the catalog preimage",
        ));
    }
    let registry_preimage = registry_preimage_bytes(&RegistryParts {
        generation: input.generation,
        run_id: &run_id,
        snapshot: &snapshot_value,
        schema_digest: &schema_digest,
        type_digest: &type_digest,
        config_digest: &config_digest,
        input_digest: &input_digest,
        arrow_schema: &arrow.schema_sha256,
        arrow_file: &arrow.file_sha256,
        catalog_sha256: &catalog_sha256,
        catalog_path: input.catalog_path,
        driver_sha: &driver_sha,
        driver_path: input.driver_path,
        audit_path: input.audit_path,
        audit_digest: &audit_digest,
        receipt_path: input.receipt_path,
        row_count: rows.len(),
    })?;
    let registry_preimage_sha256 = payload_sha256(&registry_preimage);
    let receipt_value = json!({
        "schema": RECEIPT_SCHEMA,
        "source_engine": "mongodb",
        "source_id": SOURCE_ID,
        "source_generation": input.generation,
        "snapshot": {"kind": "seal", "value": snapshot_value},
        "run_id": run_id,
        "consistency": "B",
        "schema_digest": schema_digest,
        "type_digest": type_digest,
        "config_digest": config_digest,
        "input_digest": input_digest,
        "audit_digest": audit_digest,
        "catalog_sha256": catalog_sha256,
        "registry_sha256": registry_preimage_sha256,
        "driver_digest": driver_sha,
        "arrow_sha256": [arrow.file_sha256],
        "arrow_schema_sha256": arrow.schema_sha256
    });
    let receipt = file_json(&receipt_value);
    let receipt_sha256 = payload_sha256(&receipt);
    if receipt_sha256 == schema_digest
        || receipt_sha256 == type_digest
        || receipt_sha256 == config_digest
        || receipt_sha256 == input_digest
        || receipt_sha256 == audit_digest
        || receipt_sha256 == registry_preimage_sha256
        || receipt_sha256 == STAMP_BLANK
    {
        return Err(fault(
            "digest_mismatch",
            "receipt file hash collided with an identity digest",
        ));
    }
    let registry = seal_registry(&registry_preimage, &receipt_sha256)?;
    let registry_file_sha256 = payload_sha256(&registry);
    if registry_file_sha256 == registry_preimage_sha256 {
        return Err(fault(
            "digest_mismatch",
            "registry file hash equals its preimage",
        ));
    }
    let truth: Vec<Value> = rows
        .iter()
        .map(|row| json!([row.key, row.truth_text]))
        .collect();
    let mut request = Map::new();
    request.insert("protocol".to_string(), json!(REQUEST_PROTOCOL));
    request.insert("source_engine".to_string(), json!("mongodb"));
    request.insert("source_id".to_string(), json!(SOURCE_ID));
    request.insert("source_generation".to_string(), json!(input.generation));
    if let Some(parent) = input.parent {
        request.insert("parent_source_generation".to_string(), json!(parent));
    }
    if input.full_snapshot {
        request.insert("full_snapshot".to_string(), json!(true));
    }
    request.insert("logical_schema".to_string(), json!("oo_w141"));
    request.insert("logical_table".to_string(), json!("shipments"));
    request.insert("batch_id".to_string(), json!(batch_id));
    request.insert("capture_method".to_string(), json!("c5_diff"));
    request.insert("delete_detection".to_string(), json!("reconcile"));
    request.insert("consistency_grade".to_string(), json!("B"));
    request.insert("cross_table_consistent".to_string(), json!(false));
    request.insert("position_codec".to_string(), json!("snapshot-zero/v1"));
    request.insert("position_lineage".to_string(), json!("native-update.batch"));
    request.insert("applied_pos".to_string(), json!("0"));
    request.insert("config_digest".to_string(), json!(config_digest));
    request.insert("schema_digest".to_string(), json!(schema_digest));
    request.insert("type_digest".to_string(), json!(type_digest));
    request.insert("input_digest".to_string(), json!(input_digest));
    request.insert("audit_digest".to_string(), json!(audit_digest));
    request.insert(
        "snapshot".to_string(),
        json!({"kind": "seal", "value": snapshot_value}),
    );
    request.insert(
        "columns".to_string(),
        json!([
            {"name": "_id", "logical_name": "_id", "arrow_type": "utf8", "nullable": false},
            {"name": "doc", "logical_name": "doc", "arrow_type": "utf8", "nullable": false, "document": true}
        ]),
    );
    request.insert("arrow_path".to_string(), json!(input.arrow_path));
    request.insert("tombstones".to_string(), json!([]));
    request.insert("truth".to_string(), Value::Array(truth));
    request.insert("row_identity".to_string(), json!("unique"));
    request.insert("key_column".to_string(), json!("_id"));
    request.insert("mirror".to_string(), json!(false));
    request.insert(
        "target".to_string(),
        json!({
            "host": "127.0.0.1",
            "port": input.port,
            "database": input.database,
            "runtime_principal": "postgres",
            "password_file": input.password_file,
            "ssl": "disable",
            "shard": input.shard,
            "owner": input.owner
        }),
    );
    request.insert("registry_path".to_string(), json!(input.registry_path));
    request.insert("admission_path".to_string(), json!(input.receipt_path));
    request.insert(
        "admission_expect".to_string(),
        json!({"declared_receipt_sha256": receipt_sha256}),
    );
    let request_bytes = file_json(&Value::Object(request));
    let keys: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
    let census = file_json(&json!({
        "protocol": "oo.mongodb-preview.bind-census/v1",
        "dataset": DATASET,
        "source_acceptance": false,
        "fulldoc_complete": false,
        "ready": false,
        "model_called": false,
        "id_renamed": false,
        "key_column": "_id",
        "approved_fields": APPROVED_FIELDS,
        "approved_field_count": APPROVED_FIELDS.len(),
        "budget_rows": 16,
        "generation": input.generation,
        "parent": input.parent,
        "full_snapshot": input.full_snapshot,
        "sealed_rows": rows.len(),
        "refused_rows": refused.len(),
        "source_rows": rows.len() + refused.len(),
        "sealed_keys": keys,
        "refused": refused,
        "hidden_note": "unclassified",
        "raw_doc_authorized": false,
        "deleted_key": DELETED_KEY,
        "deleted_in_sealed": keys.contains(&DELETED_KEY),
        "temporary_key": TEMPORARY_KEY,
        "temporary_in_sealed": keys.contains(&TEMPORARY_KEY),
        "weights": rows.iter().map(|row| json!({"key": row.key, "weight": row.weight})).collect::<Vec<_>>()
    }));
    let summary = json!({
        "protocol": "oo.mongodb-preview.bind-pack/v1",
        "source_acceptance": false,
        "generation_created": false,
        "import_invoked": false,
        "model_called": false,
        "ready": false,
        "fulldoc_complete": false,
        "dataset": DATASET,
        "source_id": SOURCE_ID,
        "source_generation": input.generation,
        "run_id": run_id,
        "batch_id": batch_id,
        "sealed_rows": rows.len(),
        "refused_rows": refused.len(),
        "consistency": "B",
        "trusted_written": false,
        "one_way": true,
        "receipt_sha256": receipt_sha256,
        "arrow_sha256": arrow.file_sha256,
        "arrow_schema_sha256": arrow.schema_sha256,
        "catalog_sha256": catalog_sha256,
        "registry_sha256": registry_preimage_sha256,
        "registry_file_sha256": registry_file_sha256,
        "schema_digest": schema_digest,
        "driver_digest": driver_sha,
        "catalog_table": DATASET
    });
    Ok(PackFiles {
        arrow: arrow.bytes,
        config,
        hook,
        catalog,
        registry,
        receipt,
        request: request_bytes,
        census,
        summary,
    })
}

const TYPE_PREIMAGE: &str = "_id\tutf8\tfalse\t1043\t-1\ndoc\tutf8\tdocument\t3802\t-1\n";

fn schema_preimage_digest() -> String {
    let mut preimage = String::new();
    push_schema_table(&mut preimage, "oo_w141", "shipments");
    push_schema_column(&mut preimage, "_id", 1043, -1, None);
    push_schema_column(
        &mut preimage,
        "doc",
        jsonb_doc::JSONB_OID,
        jsonb_doc::JSONB_TYPMOD,
        None,
    );
    payload_sha256(preimage.as_bytes())
}

fn config_document(
    generation: i64,
    unclassified_sha: Option<&str>,
    sealed_rows: usize,
    refused_rows: usize,
) -> Value {
    let mut document = json!({
        "protocol": "oo.mongodb-preview.capture-config/v1",
        "source_engine": "mongodb",
        "source_id": SOURCE_ID,
        "dataset": DATASET,
        "role": "read",
        "budget_rows": 16,
        "approved_fields": APPROVED_FIELDS,
        "approved_field_count": APPROVED_FIELDS.len(),
        "sealed_rows": sealed_rows,
        "unclassified_rows": refused_rows,
        "hidden_note": "unclassified",
        "raw_doc_authorized": false,
        "key_column": "_id",
        "capture_method": "c5_diff",
        "delete_detection": "reconcile",
        "consistency_grade": "B",
        "position_codec": "snapshot-zero/v1",
        "source_generation": generation,
        "credential_material": "not_in_this_file"
    });
    if let Some(hash) = unclassified_sha {
        document["unclassified_sha256"] = json!(hash);
    }
    document
}

const STAMP_BLANK: &str = "0000000000000000000000000000000000000000000000000000000000000000";

struct RegistryParts<'a> {
    generation: i64,
    run_id: &'a str,
    snapshot: &'a str,
    schema_digest: &'a str,
    type_digest: &'a str,
    config_digest: &'a str,
    input_digest: &'a str,
    arrow_schema: &'a str,
    arrow_file: &'a str,
    catalog_sha256: &'a str,
    catalog_path: &'a str,
    driver_sha: &'a str,
    driver_path: &'a str,
    audit_path: &'a str,
    audit_digest: &'a str,
    receipt_path: &'a str,
    row_count: usize,
}

fn registry_preimage_bytes(parts: &RegistryParts<'_>) -> Result<Vec<u8>, BindFault> {
    let row_count = u64::try_from(parts.row_count)
        .map_err(|_| fault("schema.invalid", "row count is outside u64"))?;
    let binding = json!({
        "protocol": "oo.source-admission.capture-binding/v1",
        "source_engine": "mongodb",
        "source_id": SOURCE_ID,
        "source_generation": parts.generation,
        "snapshot": {"kind": "seal", "value": parts.snapshot},
        "run_id": parts.run_id,
        "schema_digest": parts.schema_digest,
        "type_digest": parts.type_digest,
        "config_digest": parts.config_digest,
        "input_digest": parts.input_digest,
        "arrow_schema_sha256": parts.arrow_schema,
        "catalog_sha256": parts.catalog_sha256,
        "catalog_path": parts.catalog_path,
        "driver_digest": parts.driver_sha,
        "driver_sha256": parts.driver_sha,
        "driver_kind": "native",
        "driver_name": "mongodb",
        "driver_version": "3.3.0",
        "driver_path": parts.driver_path,
        "capture_receipt_path": parts.audit_path,
        "capture_receipt_sha256": parts.audit_digest,
        "admission_receipt_path": parts.receipt_path,
        "admission_receipt_sha256": STAMP_BLANK,
        "objects": [{
            "logical_schema": "oo_w141",
            "logical_table": "shipments",
            "row_count": row_count,
            "arrow_sha256": parts.arrow_file
        }],
        "fields": [
            {"name": "_id", "arrow_type": "utf8", "nullable": false, "key": true},
            {"name": "doc", "arrow_type": "utf8", "nullable": false, "document": true}
        ]
    });
    let registry = json!({
        "protocol": "oo.source-admission.binding-registry/v1",
        "bindings": [binding]
    });
    let bytes = file_json(&registry);
    if count_text(&bytes, STAMP_BLANK) != 1 {
        return Err(fault(
            "digest_mismatch",
            "registry preimage stamp is not unique",
        ));
    }
    Ok(bytes)
}

fn seal_registry(preimage: &[u8], receipt_sha: &str) -> Result<Vec<u8>, BindFault> {
    if receipt_sha == STAMP_BLANK || count_text(preimage, receipt_sha) != 0 {
        return Err(fault(
            "digest_mismatch",
            "receipt hash collides with the registry preimage",
        ));
    }
    let sealed = replace_once(preimage, STAMP_BLANK.as_bytes(), receipt_sha.as_bytes())
        .ok_or_else(|| fault("digest_mismatch", "registry stamp was not replaced"))?;
    if count_text(&sealed, receipt_sha) != 1 || count_text(&sealed, STAMP_BLANK) != 0 {
        return Err(fault(
            "digest_mismatch",
            "sealed registry stamp is not one way",
        ));
    }
    Ok(sealed)
}

fn count_text(haystack: &[u8], needle: &str) -> usize {
    let needle = needle.as_bytes();
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut count = 0;
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        if &haystack[index..index + needle.len()] == needle {
            count += 1;
            index += needle.len();
        } else {
            index += 1;
        }
    }
    count
}

fn replace_once(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Option<Vec<u8>> {
    let position = haystack
        .windows(needle.len())
        .position(|window| window == needle)?;
    let mut out = Vec::with_capacity(haystack.len() - needle.len() + replacement.len());
    out.extend_from_slice(&haystack[..position]);
    out.extend_from_slice(replacement);
    out.extend_from_slice(&haystack[position + needle.len()..]);
    Some(out)
}

fn absolute_path(path: &str) -> Result<(), BindFault> {
    if path.starts_with('/') && !path.contains('\0') && path.len() <= 512 {
        Ok(())
    } else {
        Err(fault("nonadmitted", "a witness path is not absolute"))
    }
}

struct CatalogDigests<'a> {
    schema: &'a str,
    type_digest: &'a str,
    config: &'a str,
    input: &'a str,
    tool_hook: &'a str,
    canonical: &'a str,
}

fn catalog_document(
    generation: i64,
    run_id: &str,
    accepted_rows: usize,
    digests: &CatalogDigests<'_>,
) -> Result<Value, BindFault> {
    let accepted = u64::try_from(accepted_rows)
        .map_err(|_| fault("schema.invalid", "row count is outside u64"))?;
    let mark = format!("g{generation}");
    Ok(json!({
        "capture": "sealed_import",
        "config_digest": digests.config,
        "engine": "mongodb",
        "hook_checked": false,
        "input_digest": digests.input,
        "protocol": "oo.pipeline.sealed-import-catalog/v1",
        "quality": {
            "loss_concealed": false,
            "manifest_reject_count": 0,
            "payload_includes_reject_lines": false,
            "stored_omits_manifest_rejects": false,
            "stored_rejected_rows": 0,
            "stored_rows": [{
                "accepted_rows": accepted,
                "attnum": null,
                "batch_id": format!("t0.b{generation}"),
                "key_digest": null,
                "kind": "batch",
                "physical_name": shadow("pt_", &mark, 1),
                "reason": null,
                "rejected_rows": 0,
                "row_index": null,
                "value_len": null
            }]
        },
        "run_id": run_id,
        "schema_digest": digests.schema,
        "shadows_queried": false,
        "source_generation": generation,
        "source_id": SOURCE_ID,
        "source_introspection": "not_used",
        "state": "awaiting_publish",
        "tables": [{
            "accepted_rows": accepted,
            "canonical_digest": digests.canonical,
            "columns": [
                {
                    "caveat": null,
                    "key": true,
                    "logical_name": "_id",
                    "oid": 1043,
                    "source_type": "varchar",
                    "stored_physical": shadow("pc_", &mark, 2),
                    "typmod": -1
                },
                {
                    "caveat": null,
                    "key": false,
                    "logical_name": "doc",
                    "oid": jsonb_doc::JSONB_OID,
                    "source_type": "jsonb",
                    "stored_physical": shadow("pc_", &mark, 3),
                    "typmod": jsonb_doc::JSONB_TYPMOD
                }
            ],
            "logical_schema": "oo_w141",
            "logical_table": "shipments",
            "rejects": [],
            "stored_namespace": shadow("pn_", &mark, 0),
            "stored_physical": shadow("pt_", &mark, 1)
        }],
        "target_generation": generation,
        "tool_hook_digest": digests.tool_hook,
        "type_digest": digests.type_digest
    }))
}

fn shadow(prefix: &str, mark: &str, salt: u8) -> String {
    let hex = payload_sha256(format!("{mark}:{salt}").as_bytes());
    format!("{prefix}{}", &hex[..60])
}

fn sealed_rows(seal: &Value) -> Result<Vec<Row>, BindFault> {
    let items = seal
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| fault("schema.invalid", "seal rows are absent"))?;
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for item in items {
        if item.get("outcome").and_then(Value::as_str) != Some("sealed") {
            continue;
        }
        let document = item
            .get("extended_json")
            .cloned()
            .ok_or_else(|| fault("field.missing", "sealed row has no extended JSON"))?;
        refuse_scope(&document)?;
        let weight = docseal::weight_token(&document)
            .map_err(|_| fault("float.rejected", "sealed weight is not $numberDouble"))?;
        docseal::weight_bits_match(&weight)
            .map_err(|_| fault("float.rejected", "sealed weight does not round-trip"))?;
        if json_has_bare_double(&document) {
            return Err(fault(
                "jsonb_double_unencoded",
                "sealed document has a bare double",
            ));
        }
        let key = document
            .pointer("/_id/$oid")
            .and_then(Value::as_str)
            .filter(|text| text.len() == 24 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| fault("bad_type", "sealed _id is not an ObjectId"))?
            .to_string();
        if !seen.insert(key.clone()) {
            return Err(fault("conflict", "sealed ObjectId is repeated"));
        }
        let doc_text = serde_json::to_string(&document)
            .map_err(|_| fault("schema.invalid", "extended JSON could not be written"))?;
        rows.push(Row {
            truth_text: pg_jsonb_text(&document),
            key,
            doc_text,
            weight,
        });
    }
    if rows.is_empty() {
        return Err(fault("empty", "seal has no approved rows"));
    }
    Ok(rows)
}

fn refused_rows(seal: &Value) -> Vec<Value> {
    seal.get("rows")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| is_refusal(item))
                .map(classify_refused)
                .collect()
        })
        .unwrap_or_default()
}

fn is_refusal(item: &Value) -> bool {
    let outcome = item.get("outcome").and_then(Value::as_str).unwrap_or("");
    let code = item.get("code").and_then(Value::as_str).unwrap_or("");
    outcome == "scope_refused" || outcome == "refused" || code.contains("field.scope")
}

fn classify_refused(item: &Value) -> Value {
    let hidden = item.pointer("/extended_json/hiddenNote");
    let value_type = match hidden {
        None => "absent",
        Some(Value::String(_)) => "string",
        Some(Value::Number(_)) => "number",
        Some(Value::Bool(_)) => "bool",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
        Some(Value::Null) => "null",
    };
    json!({
        "outcome": item.get("outcome").cloned().unwrap_or(Value::Null),
        "code": item.get("code").cloned().unwrap_or(Value::Null),
        "key": item.pointer("/extended_json/_id/$oid").cloned().unwrap_or(Value::Null),
        "weight": item.get("weight").cloned().unwrap_or(Value::Null),
        "field": if hidden.is_some() { "hiddenNote" } else { "unapproved" },
        "value_type": value_type,
        "classification": "unclassified",
        "visible": false,
        "projected": false,
        "raw_doc_authorized": false,
        "loaded": false
    })
}

fn refuse_scope(document: &Value) -> Result<(), BindFault> {
    let keys = document
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if keys
        .iter()
        .any(|key| !APPROVED_FIELDS.contains(&key.as_str()))
    {
        return Err(fault(
            "field.scope",
            "a sealed row contains an unapproved root key",
        ));
    }
    Ok(())
}

fn json_has_bare_double(value: &Value) -> bool {
    match value {
        Value::Number(number) => number.as_i64().is_none() && number.as_u64().is_none(),
        Value::Array(items) => items.iter().any(json_has_bare_double),
        Value::Object(map) => map.values().any(json_has_bare_double),
        _ => false,
    }
}

pub fn pg_jsonb_text(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(bit) => bit.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Array(items) => {
            let body = items
                .iter()
                .map(pg_jsonb_text)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{body}]")
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|left, right| {
                left.len()
                    .cmp(&right.len())
                    .then(left.as_bytes().cmp(right.as_bytes()))
            });
            let body = keys
                .into_iter()
                .map(|key| {
                    let rendered =
                        serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
                    format!("{rendered}: {}", pg_jsonb_text(&map[key]))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{body}}}")
        }
    }
}

fn derived_run_id(generation: i64, arrow_sha: &str) -> String {
    let hex = payload_sha256(format!("oo_w141:{generation}:{arrow_sha}").as_bytes());
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn file_json(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

fn fault(code: &'static str, message: &'static str) -> BindFault {
    BindFault { code, message }
}
