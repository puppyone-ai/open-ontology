//! Catalog to role pack through the fixed builder APIs.
//!
//! `oo.source.pgsql/v1` uses `compile_captured_pg_catalog`,
//! `compile_reviewed_effective`, and `compile_role_pack`.
//! `oo.pipeline.sealed-import-catalog/v1` uses `compile_sealed_import_catalog`,
//! `compile_reviewed_sealed_import`, and the same `compile_role_pack`.
//! This module does not recompute a PostgreSQL `schema_hash`. After the reviewed
//! document returns, each
//! dataset receives native `source_semantics`. Foreign keys that are not
//! `enforced` are removed from relationships. The renderer emits the table
//! section and the README dialect sentence. Gateway freshness stays pending.

use serde::Deserialize;
use serde_json::{Value, json};

use oo_builder::{
    CapturedPgDraft, JsonbDocumentProfile, SealedImportDraft, compile_captured_pg_catalog,
    compile_jsonb_document_profile, compile_reviewed_effective, compile_reviewed_sealed_import,
    compile_role_pack, compile_sealed_import_catalog,
};

use crate::dialect::source_dialect_hit;
use crate::semantics::{
    BoundFacts, Normalized, Target, TrustRow, column_semantics, enforced_fk, is_table_token,
    source_semantics_for, trust_json,
};
use crate::SemanticsError;

const CAPTURED_COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const DIALECT_SENTENCE: &str = "所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub settings: Value,
    pub decisions: Vec<DecisionIn>,
    pub visibility: Value,
    #[serde(default)]
    pub examples: Vec<ExampleIn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionIn {
    dataset: String,
    column: String,
    pii: String,
    semantic_type: String,
    #[serde(default)]
    unit: Option<String>,
    reviewer: ReviewerIn,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewerIn {
    by: String,
    at: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExampleIn {
    id: String,
    question: String,
    sql: String,
    #[serde(default)]
    uses: Vec<String>,
    verified_by: String,
    verified_at: String,
}

pub struct Prepared {
    pub catalog_proof: Value,
    pub draft: Value,
    pub envelope: Value,
    pub wrapper: Value,
}

pub fn prove_catalog(catalog: &Value) -> Result<Prepared, SemanticsError> {
    if !catalog.is_object() {
        return Err(SemanticsError::new(
            "schema.invalid",
            "/catalog",
            "catalog is not an object",
        ));
    }
    let catalog_json = serde_json::to_string(catalog).map_err(|_| {
        SemanticsError::new("schema.invalid", "/catalog", "catalog is not JSON")
    })?;
    let compiled = compile_captured_pg_catalog(&catalog_json, None, None)
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let draft: Value = parse_text(&compiled.draft, "/catalog", "compiled draft is not JSON")?;
    let envelope = parse_text(
        &compiled.evidence_envelope,
        "/catalog",
        "evidence envelope is not JSON",
    )?;
    let wrapper = parse_text(&compiled.wrapper, "/catalog", "wrapper is not JSON")?;
    let proof = proof_of(&compiled, &draft, &wrapper);
    Ok(Prepared {
        catalog_proof: proof,
        draft,
        envelope,
        wrapper,
    })
}

pub fn pack_or_block(
    target: &Target,
    binding: Option<&Binding>,
    normalized: &Normalized,
    prepared: &Prepared,
) -> Result<Value, SemanticsError> {
    let mut result = json!({
        "blocks": normalized.blocks,
        "catalog_proof": prepared.catalog_proof,
        "chunk_key_order": normalized.chunk,
        "columns": normalized.columns,
        "constraint_trust": trust_json(&normalized.constraint_trust),
        "fixed_renderer_gaps": ["gateway_freshness_fields_absent"],
        "freshness_projection": {
            "capture_method": target.capture,
            "consistency_grade": target.source_consistency,
            "consistency_source": "compiler_normalized_not_gateway",
            "contract": "pending_gateway_fields",
            "delete_detection": target.delete_detection,
            "gateway_get_freshness": "not_called"
        },
        "live_database": "not_connected",
        "native_notes": normalized.native_notes,
        "protocol": crate::RESULT_PROTOCOL,
        "semantic_caveats": normalized.semantic_caveats,
        "source_semantics": normalized.source_semantics,
    });
    if !normalized.blocks.is_empty() {
        result["builder_calls"] = json!(["compile_captured_pg_catalog"]);
        result["pack"] = Value::Null;
        result["status"] = json!("blocked");
        return Ok(result);
    }
    let Some(binding) = binding else {
        return Err(SemanticsError::new(
            "binding.required",
            "/binding",
            "a packed result needs settings, decisions, and visibility",
        ));
    };
    build_pack(target, binding, normalized, prepared, result)
}

fn build_pack(
    target: &Target,
    binding: &Binding,
    normalized: &Normalized,
    prepared: &Prepared,
    mut result: Value,
) -> Result<Value, SemanticsError> {
    let schema_hash = prepared
        .draft
        .pointer("/provenance/schema_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            SemanticsError::new(
                "fingerprint.absent",
                "/draft/provenance/schema_hash",
                "draft did not carry schema_hash",
            )
        })?;
    let mut decisions = Vec::new();
    for decision in &binding.decisions {
        let fingerprint = fingerprint(&prepared.draft, &decision.dataset, &decision.column)?;
        let mut row = json!({
            "column": decision.column,
            "dataset": decision.dataset,
            "fact_fingerprint": fingerprint,
            "pii": decision.pii,
            "reviewer": {"at": decision.reviewer.at, "by": decision.reviewer.by},
            "schema_hash": schema_hash,
            "semantic_type": decision.semantic_type,
        });
        if let Some(unit) = &decision.unit {
            row["unit"] = json!(unit);
        }
        decisions.push(row);
    }
    let command = json!({
        "artifact": {
            "draft": prepared.draft,
            "evidence_envelope": prepared.envelope,
            "protocol": CAPTURED_COMMAND_PROTOCOL,
            "wrapper": prepared.wrapper,
        },
        "decisions": decisions,
        "protocol": oo_builder::REVIEWED_EFFECTIVE_COMMAND_PROTOCOL,
        "settings": binding.settings,
    });
    let command_text = serde_json::to_string(&command).map_err(|_| {
        SemanticsError::new("schema.invalid", "/binding", "reviewed command is not JSON")
    })?;
    let reviewed_text = compile_reviewed_effective(&command_text)
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let mut reviewed = parse_text(&reviewed_text, "/binding", "reviewed effective result is not JSON")?;
    let mut effective = reviewed.get_mut("effective_oos").map(Value::take).ok_or_else(|| {
        SemanticsError::new(
            "reviewed.rejected",
            "/effective_oos",
            "reviewed result has no effective_oos",
        )
    })?;
    let (rejected, kept_examples) = adjust_effective(&mut effective, target, normalized, binding)?;
    let visibility_text = serde_json::to_string(&binding.visibility).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            "/binding/visibility",
            "visibility is not JSON",
        )
    })?;
    let effective_text = serde_json::to_string(&effective).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            "/effective_oos",
            "effective document is not JSON",
        )
    })?;
    let pack = compile_role_pack(&effective_text, &visibility_text)
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let readme = pack
        .file("README.md")
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let verified = pack
        .file("examples/verified.sql")
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    if let Some(hit) = source_dialect_hit(verified) {
        return Err(SemanticsError::new(
            "dialect.leaked",
            "examples/verified.sql",
            format!("source dialect {hit} is in the executable example file"),
        ));
    }
    let mut pages = serde_json::Map::new();
    for dataset in visibility_datasets(&binding.visibility)? {
        let path = format!("tables/{dataset}.md");
        let text = pack
            .file(&path)
            .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
        pages.insert(path, json!(text));
    }
    let readme_has_sentence = readme.contains(DIALECT_SENTENCE);
    let fence_heading = pages.values().any(|page| {
        page.as_str()
            .is_some_and(caveat_fence_has_heading)
    });
    let dedicated = pages.values().any(|page| {
        page.as_str()
            .is_some_and(|text| text.lines().any(|line| line == "## 来源与语义"))
    });
    result["builder_calls"] = json!([
        "compile_captured_pg_catalog",
        "compile_reviewed_effective",
        "compile_role_pack"
    ]);
    result["examples_rejected"] = json!(rejected);
    result["pack"] = json!({
        "caveat_fence_contains_heading": fence_heading,
        "dedicated_heading_emitted": dedicated,
        "examples_in_verified_sql": example_ids(verified),
        "pages": Value::Object(pages),
        "readme": readme,
        "readme_contains_pg_dialect_sentence": readme_has_sentence,
        "verified_sql": verified,
    });
    result["role_pack_inputs"] = json!({
        "effective_json": effective_text,
        "examples_submitted": kept_examples,
        "visibility_json": visibility_text,
    });
    result["status"] = json!("packed");
    Ok(result)
}

fn adjust_effective(
    effective: &mut Value,
    target: &Target,
    normalized: &Normalized,
    binding: &Binding,
) -> Result<(Vec<Value>, Vec<String>), SemanticsError> {
    let datasets = effective.get_mut("datasets").and_then(Value::as_array_mut).ok_or_else(|| {
        SemanticsError::new(
            "pack.adjust",
            "/datasets",
            "effective datasets are not an array",
        )
    })?;
    for dataset in datasets.iter_mut() {
        let name = dataset
            .get("dataset")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        dataset["source_semantics"] = source_semantics_for(&name, target, normalized)?;
    }
    let relationships = effective
        .get_mut("relationships")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            SemanticsError::new(
                "pack.adjust",
                "/relationships",
                "effective relationships are not an array",
            )
        })?;
    let mut kept_relationships = Vec::new();
    for relationship in relationships.drain(..) {
        if keep_relationship(&relationship, &normalized.constraint_trust) {
            kept_relationships.push(relationship);
        }
    }
    *relationships = kept_relationships;
    let (rejected, kept) = split_examples(&binding.examples);
    let kept_ids = kept
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    effective
        .as_object_mut()
        .ok_or_else(|| {
            SemanticsError::new(
                "pack.adjust",
                "/effective_oos",
                "effective document is not an object",
            )
        })?
        .insert("examples".to_string(), Value::Array(kept));
    Ok((rejected, kept_ids))
}

fn split_examples(examples: &[ExampleIn]) -> (Vec<Value>, Vec<Value>) {
    let mut rejected = Vec::new();
    let mut kept = Vec::new();
    for example in examples {
        if let Some(hit) =
            source_dialect_hit(&example.sql).or_else(|| source_dialect_hit(&example.question))
        {
            rejected.push(json!({"id": example.id, "source_dialect": hit}));
            continue;
        }
        kept.push(json!({
            "id": example.id,
            "question": example.question,
            "sql": example.sql,
            "uses": example.uses,
            "verified_at": example.verified_at,
            "verified_by": example.verified_by,
        }));
    }
    (rejected, kept)
}

fn keep_relationship(relationship: &Value, rows: &[TrustRow]) -> bool {
    if relationship.get("provenance").and_then(Value::as_str) != Some("declared_fk") {
        return true;
    }
    let name = relationship.get("name").and_then(Value::as_str).unwrap_or("");
    let dataset = relationship
        .pointer("/from/dataset")
        .and_then(Value::as_str)
        .unwrap_or("");
    let columns = relationship
        .pointer("/from/columns")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    enforced_fk(rows, dataset, name, &columns)
}

fn proof_of(compiled: &CapturedPgDraft, draft: &Value, wrapper: &Value) -> Value {
    json!({
        "captured_catalog_hash": compiled.captured_catalog_hash,
        "captured_compiler": compiled.compiler,
        "captured_protocol": compiled.protocol,
        "draft_protocol": draft.get("protocol").and_then(Value::as_str).unwrap_or(""),
        "schema_hash": compiled.schema_hash,
        "source_connection": wrapper.get("source_connection").cloned().unwrap_or(Value::Null),
        "status": "accepted",
    })
}

fn fingerprint(draft: &Value, dataset: &str, column: &str) -> Result<String, SemanticsError> {
    let item_id = format!("column/{dataset}/{column}");
    let queue = draft.get("review_queue").and_then(Value::as_array).ok_or_else(|| {
        SemanticsError::new(
            "fingerprint.absent",
            "/draft/review_queue",
            "draft has no review queue",
        )
    })?;
    for item in queue {
        let same_id = item.get("item_id").and_then(Value::as_str) == Some(item_id.as_str());
        let structural = item.get("kind").and_then(Value::as_str) == Some("structural_column");
        if same_id && structural {
            return item
                .get("fact_fingerprint")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    SemanticsError::new(
                        "fingerprint.absent",
                        format!("/draft/review_queue/{item_id}"),
                        "structural column has no fact_fingerprint",
                    )
                });
        }
    }
    Err(SemanticsError::new(
        "fingerprint.absent",
        format!("/draft/review_queue/{item_id}"),
        "structural column fingerprint is not in the compiled draft",
    ))
}

fn visibility_datasets(visibility: &Value) -> Result<Vec<String>, SemanticsError> {
    let datasets = visibility.get("datasets").and_then(Value::as_array).ok_or_else(|| {
        SemanticsError::new(
            "schema.invalid",
            "/binding/visibility/datasets",
            "visibility datasets are not an array",
        )
    })?;
    let mut names = Vec::new();
    for dataset in datasets {
        let name = dataset.get("dataset").and_then(Value::as_str).ok_or_else(|| {
            SemanticsError::new(
                "schema.invalid",
                "/binding/visibility/datasets",
                "visibility dataset has no name",
            )
        })?;
        names.push(name.to_string());
    }
    Ok(names)
}

fn caveat_fence_has_heading(page: &str) -> bool {
    let mut inside = false;
    for line in page.lines() {
        if line.starts_with("```") {
            inside = !inside;
            continue;
        }
        if inside && line.contains("来源与语义") {
            return true;
        }
    }
    false
}

fn example_ids(verified: &str) -> Vec<String> {
    verified
        .lines()
        .filter_map(|line| line.strip_prefix("-- ").map(str::trim).map(str::to_string))
        .filter(|line| !line.starts_with("verified_at:") && !line.contains(' '))
        .collect()
}

fn parse_text(text: &str, path: &str, message: &'static str) -> Result<Value, SemanticsError> {
    serde_json::from_str(text).map_err(|_| SemanticsError::new("catalog.rejected", path, message))
}

struct SealedPrepared {
    catalog_proof: Value,
    draft: SealedImportDraft,
    catalog: Value,
}

/// Prove a sealed import, merge its logical columns, and compile the role pack.
///
/// A precision-loss key is `blocked`. Malformed, unsealed, and wrong-digest
/// catalogs stay errors. The PostgreSQL catalog compiler is not called.
pub fn compile_sealed(
    target: &Target,
    binding: Option<&Binding>,
    mut normalized: Normalized,
    catalog: &Value,
    document: Option<&Value>,
    source_bound: Option<&Value>,
    asserted: Option<&Value>,
) -> Result<Value, SemanticsError> {
    let result = match prove_sealed(catalog) {
        Ok(prepared) => {
            apply_sealed(&mut normalized, target, &prepared)?;
            if let Some(source_bound) = source_bound {
                install_source_bound(&mut normalized, target, &prepared, source_bound, asserted)?;
            }
            pack_sealed(target, binding, &normalized, &prepared)?
        }
        Err(err) if err.code.ends_with("/precision_loss_key") => {
            blocked_sealed(target, &normalized, &err)
        }
        Err(err) => return Err(err),
    };
    bind_document(result, catalog, document)
}

fn bind_document(
    mut result: Value,
    catalog: &Value,
    document: Option<&Value>,
) -> Result<Value, SemanticsError> {
    let Some(document) = document else {
        return Ok(result);
    };
    let accepted = result
        .pointer("/catalog_proof/status")
        .and_then(Value::as_str)
        == Some("accepted");
    if !accepted {
        return Err(SemanticsError::new(
            "document.refused",
            "/document",
            "a full document is not sealed when the catalog is refused",
        ));
    }
    attach_jsonb_document(&mut result, catalog, document)?;
    Ok(result)
}

fn attach_jsonb_document(
    result: &mut Value,
    catalog: &Value,
    document: &Value,
) -> Result<(), SemanticsError> {
    let catalog_json = serde_json::to_string(catalog).map_err(|_| {
        SemanticsError::new("schema.invalid", "/catalog", "catalog is not JSON")
    })?;
    let document_json = serde_json::to_string(document).map_err(|_| {
        SemanticsError::new("schema.invalid", "/document", "document envelope is not JSON")
    })?;
    let profile = compile_jsonb_document_profile(&catalog_json, &document_json)
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let profile_value = serde_json::from_str(&profile.document).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            "/document_profile",
            "document profile is not JSON",
        )
    })?;
    result
        .as_object_mut()
        .ok_or_else(|| SemanticsError::new("schema.invalid", "/", "result is not an object"))?
        .insert("document_profile".to_string(), profile_value);
    if let Some(calls) = result.get_mut("builder_calls").and_then(Value::as_array_mut) {
        calls.push(json!(oo_builder::JSONB_PROFILE_COMPILER));
    }
    if result.get("status").and_then(Value::as_str) != Some("packed") {
        return Ok(());
    }
    let path = format!("tables/{}.md", profile.dataset);
    let page = result
        .get_mut("pack")
        .and_then(|pack| pack.get_mut("pages"))
        .and_then(|pages| pages.get_mut(&path))
        .ok_or_else(|| {
            SemanticsError::new(
                "document.page",
                format!("/pack/pages/{path}"),
                "packed role page for the document dataset is absent",
            )
        })?;
    let text = page.as_str().ok_or_else(|| {
        SemanticsError::new(
            "document.page",
            format!("/pack/pages/{path}"),
            "role page is not text",
        )
    })?;
    let mut next = text.to_string();
    if !next.ends_with('\n') {
        next.push('\n');
    }
    next.push('\n');
    next.push_str(&document_lines(&profile)?);
    *page = json!(next);
    Ok(())
}

fn document_lines(profile: &JsonbDocumentProfile) -> Result<String, SemanticsError> {
    let parsed: Value = serde_json::from_str(&profile.document).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            "/document_profile",
            "document profile is not JSON",
        )
    })?;
    let metadata = parsed
        .pointer("/arrow/metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            SemanticsError::new(
                "schema.invalid",
                "/document_profile",
                "document profile metadata is absent",
            )
        })?;
    let token = |key: &str| -> Result<&str, SemanticsError> {
        metadata.get(key).and_then(Value::as_str).ok_or_else(|| {
            SemanticsError::new(
                "schema.invalid",
                "/document_profile",
                "document profile metadata is absent",
            )
        })
    };
    let kind = token("oo.document.kind")?;
    let missing = token("oo.document.missing")?;
    let uuid = token("oo.document.uuid")?;
    let date = token("oo.document.date")?;
    let bytes = token("oo.document.bytes")?;
    let decimal = token("oo.document.decimal")?;
    let double = token("oo.document.double")?;
    let negative_zero = token("oo.document.double.negative_zero")?;
    let non_finite = token("oo.document.double.non_finite")?;
    let nan_payload = token("oo.document.double.nan_payload")?;
    let field_scope = token("oo.document.field_scope")?;
    let float_precision = token("oo.precision.float")?;
    let oid = parsed.get("oid").and_then(Value::as_i64).ok_or_else(|| {
        SemanticsError::new(
            "schema.invalid",
            "/document_profile",
            "document profile metadata is absent",
        )
    })?;
    let typmod = parsed.get("typmod").and_then(Value::as_i64).ok_or_else(|| {
        SemanticsError::new(
            "schema.invalid",
            "/document_profile",
            "document profile metadata is absent",
        )
    })?;
    if parsed.pointer("/double/encoding").and_then(Value::as_str) != Some(double)
        || parsed.pointer("/double/negative_zero").and_then(Value::as_str) != Some(negative_zero)
        || parsed.pointer("/double/non_finite").and_then(Value::as_str) != Some(non_finite)
        || parsed.pointer("/double/nan_payload").and_then(Value::as_str) != Some(nan_payload)
        || parsed.pointer("/precision/float").and_then(Value::as_str) != Some(float_precision)
        || oid != 3802
        || typmod != -1
    {
        return Err(SemanticsError::new(
            "schema.invalid",
            "/document_profile",
            "document profile double declaration does not match its metadata",
        ));
    }
    let extended = parsed.get("extended_json").ok_or_else(|| {
        SemanticsError::new(
            "schema.invalid",
            "/document_profile",
            "document profile extended json is absent",
        )
    })?;
    let root = extended.as_object().ok_or_else(|| {
        SemanticsError::new(
            "schema.invalid",
            "/document_profile",
            "document profile extended json is not an object",
        )
    })?;
    let mut names: Vec<&str> = root.keys().map(String::as_str).collect();
    names.sort_unstable();
    let root_keys = names.join(",");
    let weight_line = match root.get("weight") {
        None => String::new(),
        Some(value) => {
            let rendered = serde_json::to_string(value).map_err(|_| {
                SemanticsError::new(
                    "schema.invalid",
                    "/document_profile",
                    "document weight is not JSON",
                )
            })?;
            format!("weight: {rendered}\n")
        }
    };
    Ok(format!(
        "\
document_storage: jsonb
document_oid: {oid}
document_typmod: {typmod}
document_kind: {kind}
missing: {missing}
uuid: {uuid}
date: {date}
bytes: {bytes}
decimal: {decimal}
double: {double}
double_negative_zero: {negative_zero}
double_non_finite: {non_finite}
double_nan_payload: {nan_payload}
field_scope: {field_scope}
float: {float_precision}
document_root_keys: {root_keys}
{weight_line}document_raw_bson_sha256: {raw}
document_pg_jsonb_sha256: {pg}
",
        raw = profile.raw_bson_sha256,
        pg = profile.pg_jsonb_sha256,
    ))
}

fn prove_sealed(catalog: &Value) -> Result<SealedPrepared, SemanticsError> {
    let catalog_json = serde_json::to_string(catalog).map_err(|_| {
        SemanticsError::new("schema.invalid", "/catalog", "catalog is not JSON")
    })?;
    let draft = compile_sealed_import_catalog(&catalog_json)
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let catalog_proof = json!({
        "compiler": draft.compiler,
        "config_digest": draft.config_digest,
        "identity_hash": draft.identity_hash,
        "input_digest": draft.input_digest,
        "live_database": "not_connected",
        "normalized_representation": draft.normalized_representation,
        "protocol": draft.protocol,
        "schema_digest": draft.schema_digest,
        "source_connection": "not_used",
        "source_engine": draft.source_engine,
        "source_generation": draft.source_generation,
        "source_id": draft.source_id,
        "source_introspection": "not_used",
        "source_protocol": oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL,
        "status": "accepted",
        "target_generation": draft.target_generation,
        "type_digest": draft.type_digest,
    });
    Ok(SealedPrepared {
        catalog_proof,
        draft,
        catalog: catalog.clone(),
    })
}

fn blocked_sealed(target: &Target, normalized: &Normalized, err: &SemanticsError) -> Value {
    let mut blocks = normalized.blocks.clone();
    blocks.push(json!({
        "code": "precision_loss_key",
        "message": err.message,
        "path": err.path,
    }));
    json!({
        "blocks": blocks,
        "builder_calls": ["compile_sealed_import_catalog"],
        "catalog_proof": {
            "code": err.code,
            "live_database": "not_connected",
            "normalized_representation": "postgresql_target_storage",
            "source_connection": "not_used",
            "source_protocol": oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL,
            "status": "refused",
        },
        "chunk_key_order": normalized.chunk,
        "columns": normalized.columns,
        "constraint_trust": trust_json(&normalized.constraint_trust),
        "fixed_renderer_gaps": ["gateway_freshness_fields_absent"],
        "freshness_projection": freshness(target),
        "live_database": "not_connected",
        "native_notes": normalized.native_notes,
        "pack": Value::Null,
        "protocol": crate::RESULT_PROTOCOL,
        "semantic_caveats": normalized.semantic_caveats,
        "source_semantics": normalized.source_semantics,
        "status": "blocked",
    })
}

fn pack_sealed(
    target: &Target,
    binding: Option<&Binding>,
    normalized: &Normalized,
    prepared: &SealedPrepared,
) -> Result<Value, SemanticsError> {
    let mut result = json!({
        "blocks": normalized.blocks,
        "catalog_proof": prepared.catalog_proof,
        "chunk_key_order": normalized.chunk,
        "columns": normalized.columns,
        "constraint_trust": trust_json(&normalized.constraint_trust),
        "fixed_renderer_gaps": ["gateway_freshness_fields_absent"],
        "freshness_projection": normalized
            .bound_freshness
            .clone()
            .unwrap_or_else(|| freshness(target)),
        "live_database": "not_connected",
        "native_notes": normalized.native_notes,
        "protocol": crate::RESULT_PROTOCOL,
        "semantic_caveats": normalized.semantic_caveats,
        "source_semantics": normalized.source_semantics,
    });
    if let Some(profile) = &normalized.source_profile {
        result["source_semantics_profile"] = profile.clone();
    }
    if !normalized.blocks.is_empty() {
        result["builder_calls"] = json!(sealed_calls(normalized.source_profile.is_some(), false));
        result["pack"] = Value::Null;
        result["status"] = json!("blocked");
        return Ok(result);
    }
    let Some(binding) = binding else {
        return Err(SemanticsError::new(
            "binding.required",
            "/binding",
            "a packed result needs settings, decisions, and visibility",
        ));
    };
    let visibility_version = binding
        .visibility
        .get("catalog_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    if binding.settings.get("catalog_version").and_then(Value::as_str) != Some(prepared.draft.identity_hash.as_str())
        || visibility_version != prepared.draft.identity_hash
    {
        return Err(SemanticsError::new(
            "binding.catalog_version",
            "/binding/settings/catalog_version",
            "catalog version is not the sealed identity hash",
        ));
    }
    let mut decisions = Vec::new();
    for decision in &binding.decisions {
        let fingerprint = prepared
            .draft
            .fact_fingerprint(&decision.dataset, &decision.column)
            .ok_or_else(|| {
                SemanticsError::new(
                    "fingerprint.absent",
                    format!("/draft/{}/{}", decision.dataset, decision.column),
                    "structural column fingerprint is not in the compiled draft",
                )
            })?;
        let mut row = json!({
            "column": decision.column,
            "dataset": decision.dataset,
            "fact_fingerprint": fingerprint,
            "pii": decision.pii,
            "reviewer": {"at": decision.reviewer.at, "by": decision.reviewer.by},
            "schema_hash": prepared.draft.schema_digest,
            "semantic_type": decision.semantic_type,
        });
        if let Some(unit) = &decision.unit {
            row["unit"] = json!(unit);
        }
        decisions.push(row);
    }
    let command = json!({
        "catalog": prepared.catalog,
        "decisions": decisions,
        "protocol": oo_builder::REVIEWED_SEALED_IMPORT_COMMAND_PROTOCOL,
        "settings": binding.settings,
    });
    let command_text = serde_json::to_string(&command).map_err(|_| {
        SemanticsError::new("schema.invalid", "/binding", "reviewed command is not JSON")
    })?;
    let reviewed_text = compile_reviewed_sealed_import(&command_text)
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let mut reviewed = parse_text(&reviewed_text, "/binding", "reviewed sealed result is not JSON")?;
    let mut effective = reviewed.get_mut("effective_oos").map(Value::take).ok_or_else(|| {
        SemanticsError::new(
            "reviewed.rejected",
            "/effective_oos",
            "reviewed result has no effective_oos",
        )
    })?;
    let (rejected, kept_examples) = adjust_effective(&mut effective, target, normalized, binding)?;
    let visibility_text = serde_json::to_string(&binding.visibility).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            "/binding/visibility",
            "visibility is not JSON",
        )
    })?;
    let effective_text = serde_json::to_string(&effective).map_err(|_| {
        SemanticsError::new(
            "schema.invalid",
            "/effective_oos",
            "effective document is not JSON",
        )
    })?;
    let pack = compile_role_pack(&effective_text, &visibility_text)
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let readme = pack
        .file("README.md")
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    let verified = pack
        .file("examples/verified.sql")
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    if let Some(hit) = source_dialect_hit(verified) {
        return Err(SemanticsError::new(
            "dialect.leaked",
            "examples/verified.sql",
            format!("source dialect {hit} is in the executable example file"),
        ));
    }
    let mut pages = serde_json::Map::new();
    for dataset in visibility_datasets(&binding.visibility)? {
        let path = format!("tables/{dataset}.md");
        let text = pack
            .file(&path)
            .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
        pages.insert(path, json!(text));
    }
    result["builder_calls"] = json!(sealed_calls(normalized.source_profile.is_some(), true));
    result["examples_rejected"] = json!(rejected);
    result["pack"] = json!({
        "caveat_fence_contains_heading": pages.values().any(|page| {
            page.as_str().is_some_and(caveat_fence_has_heading)
        }),
        "dedicated_heading_emitted": pages.values().any(|page| {
            page.as_str().is_some_and(|text| text.lines().any(|line| line == "## 来源与语义"))
        }),
        "examples_in_verified_sql": example_ids(verified),
        "pages": Value::Object(pages),
        "readme": readme,
        "readme_contains_pg_dialect_sentence": readme.contains(DIALECT_SENTENCE),
        "verified_sql": verified,
    });
    result["role_pack_inputs"] = json!({
        "effective_json": effective_text,
        "examples_submitted": kept_examples,
        "visibility_json": visibility_text,
    });
    result["status"] = json!("packed");
    Ok(result)
}

fn sealed_calls(source_bound: bool, packed: bool) -> Vec<&'static str> {
    let mut calls = vec!["compile_sealed_import_catalog"];
    if source_bound {
        calls.push("derive_source_bound");
    }
    if packed {
        calls.push("compile_reviewed_sealed_import");
        calls.push("compile_role_pack");
    }
    calls
}

fn install_source_bound(
    normalized: &mut Normalized,
    target: &Target,
    prepared: &SealedPrepared,
    source_bound: &Value,
    asserted: Option<&Value>,
) -> Result<(), SemanticsError> {
    let catalog_json = serde_json::to_string(&prepared.catalog).map_err(|_| {
        SemanticsError::new("schema.invalid", "/catalog", "catalog is not JSON")
    })?;
    let bound_json = serde_json::to_string(source_bound).map_err(|_| {
        SemanticsError::new("schema.invalid", "/source_bound", "source-bound observation is not JSON")
    })?;
    let derived = oo_core::source_bound::derive_source_bound(&catalog_json, &bound_json)
        .map_err(|err| SemanticsError::new(err.code, err.path, err.message))?;
    if target.version != derived.version {
        return Err(bound_error(
            "token.mismatch",
            "/target/version",
            "target version is not the release token in the banner",
        ));
    }
    if target.source_kind != derived.source_kind {
        return Err(bound_error(
            "token.mismatch",
            "/target/source_kind",
            "target source kind is not the derived source kind",
        ));
    }
    match (
        target.source_consistency.as_deref(),
        derived.consistency_grade.as_deref(),
    ) {
        (None, _) => {}
        (Some(_), None) => {
            return Err(bound_error(
                "fidelity.contradiction",
                "/target/source_consistency",
                "target consistency grade is not in the source observation",
            ));
        }
        (Some(claim), Some(observed)) if claim != observed => {
            return Err(bound_error(
                "token.mismatch",
                "/target/source_consistency",
                "target consistency grade does not match the source observation",
            ));
        }
        (Some(_), Some(_)) => {}
    }
    match (
        target.delete_detection.as_deref(),
        derived.delete_detection.as_deref(),
    ) {
        (None, _) => {}
        (Some(_), None) => {
            return Err(bound_error(
                "fidelity.contradiction",
                "/target/delete_detection",
                "target delete detection is not in the source observation",
            ));
        }
        (Some(claim), Some(observed)) if claim != observed => {
            return Err(bound_error(
                "token.mismatch",
                "/target/delete_detection",
                "target delete detection does not match the source observation",
            ));
        }
        (Some(_), Some(_)) => {}
    }
    for column in &prepared.draft.columns {
        if !column.precision_loss {
            continue;
        }
        let agrees = derived.columns.iter().any(|item| {
            item.dataset == column.dataset
                && item.logical_name == column.name
                && item.caveats.iter().any(|token| token == "precision_loss")
        });
        if !agrees {
            return Err(bound_error(
                "fidelity.contradiction",
                "/draft/precision_loss",
                "builder precision loss is not the derived source semantic",
            ));
        }
    }
    check_column_claims(target, &derived)?;
    if let Some(asserted) = asserted {
        check_asserted(asserted, &derived)?;
    }
    normalized
        .semantic_caveats
        .retain(|token| is_table_token(token));
    for column in &derived.columns {
        let qualified = format!("{}.{}", column.dataset, column.logical_name);
        set_derived_column(normalized, &qualified, &column.caveats, column.native_note.as_deref());
        for token in &column.caveats {
            push_unique(&mut normalized.semantic_caveats, token);
        }
    }
    normalized.semantic_caveats.sort();
    normalized.semantic_caveats.dedup();
    normalized.columns.sort_by(|left, right| {
        left["column"].as_str().cmp(&right["column"].as_str())
    });
    normalized.native_notes = normalized
        .columns
        .iter()
        .filter_map(|column| {
            column["native_note"].as_str().map(|note| {
                json!({"column": column["column"].as_str().unwrap_or(""), "note": note})
            })
        })
        .collect();
    normalized.bound_facts = Some(BoundFacts {
        family: derived.family.clone(),
        product: derived.product.clone(),
        version: derived.version.clone(),
        capture: derived.capture.clone(),
        grade: derived.consistency_grade.clone(),
        delete_detection: derived.delete_detection.clone(),
        source_kind: derived.source_kind.clone(),
    });
    normalized.source_profile = Some(json!({
        "protocol": "oo.builder.source-semantics-profile/v1",
        "compiler": "derive_source_bound",
        "identity_hash": derived.identity_hash,
        "schema_digest": derived.schema_digest,
        "type_digest": derived.type_digest,
        "config_digest": derived.config_digest,
        "input_digest": derived.input_digest,
        "source_id": derived.source_id,
        "run_id": derived.run_id,
        "engine": derived.engine,
        "capture": derived.capture,
        "version": derived.version,
        "source_kind": derived.source_kind,
        "consistency_grade": derived.consistency_grade,
        "consistency_grade_observed": derived.consistency_grade.is_some(),
        "delete_detection": derived.delete_detection,
        "delete_detection_observed": derived.delete_detection.is_some(),
        "scn": derived.scn,
        "registry_snapshot": derived.registry_snapshot,
        "source_generation": derived.source_generation,
        "target_generation": derived.target_generation,
        "columns": derived.columns.iter().map(|column| json!({
            "dataset": column.dataset,
            "column": column.logical_name,
            "caveats": column.caveats,
            "native_note": column.native_note,
        })).collect::<Vec<_>>(),
    }));
    normalized.bound_freshness = Some(json!({
        "capture_method": derived.capture,
        "consistency_grade": derived.consistency_grade,
        "consistency_grade_observed": derived.consistency_grade.is_some(),
        "consistency_source": "source_bound_observation",
        "contract": "pending_gateway_fields",
        "delete_detection": derived.delete_detection,
        "delete_detection_observed": derived.delete_detection.is_some(),
        "gateway_get_freshness": "not_called",
        "registry_snapshot": derived.registry_snapshot,
        "scn": derived.scn,
        "version": derived.version,
    }));
    let mut loose = json!({
        "capture": derived.capture,
        "caveats": normalized.semantic_caveats,
        "contract": "source_bound_observation",
        "family": derived.family,
        "identity_hash": derived.identity_hash,
        "native_type_semantics": normalized.native_notes,
        "product": derived.product,
        "source_kind": derived.source_kind,
        "version": derived.version,
    });
    if let Some(grade) = &derived.consistency_grade {
        loose["grade"] = json!(grade);
    }
    if let Some(delete_detection) = &derived.delete_detection {
        loose["delete_detection"] = json!(delete_detection);
    }
    normalized.source_semantics = loose;
    Ok(())
}

fn check_column_claims(
    target: &Target,
    derived: &oo_core::source_bound::DerivedSourceBound,
) -> Result<(), SemanticsError> {
    for column in &target.columns {
        let qualified = format!("{}.{}.{}", column.schema, column.table, column.name);
        let (tokens, note) = column_semantics(target, column)?;
        let Some(derived_column) = derived.columns.iter().find(|item| {
            format!("{}.{}", item.dataset, item.logical_name) == qualified
        }) else {
            return Err(bound_error(
                "column.mismatch",
                "/target/columns",
                "target column is not in the derived source",
            ));
        };
        for token in &tokens {
            let present = derived_column.caveats.iter().any(|item| item == token);
            if token == "precision_loss" && !present {
                return Err(bound_error(
                    "fidelity.contradiction",
                    "/target/columns",
                    "claimed precision loss is not the derived source semantic",
                ));
            }
            if !present {
                return Err(bound_error(
                    "token.mismatch",
                    "/target/columns",
                    "claimed column token is not the derived source semantic",
                ));
            }
        }
        if let Some(note) = note.as_deref()
            && derived_column.native_note.as_deref() != Some(note)
        {
            return Err(bound_error(
                "token.mismatch",
                "/target/columns",
                "claimed native note is not the derived source semantic",
            ));
        }
    }
    Ok(())
}

fn check_asserted(
    asserted: &Value,
    derived: &oo_core::source_bound::DerivedSourceBound,
) -> Result<(), SemanticsError> {
    let parsed: AssertedSemantics = serde_json::from_value(asserted.clone()).map_err(|err| {
        let text = err.to_string();
        if text.contains("unknown field") {
            bound_error(
                "field.unknown",
                "/asserted_semantics",
                "asserted semantics has an unknown field",
            )
        } else {
            bound_error(
                "schema.invalid",
                "/asserted_semantics",
                "asserted semantics is not the closed claim",
            )
        }
    })?;
    if parsed.protocol != "oo.source-semantics.asserted/v1" {
        return Err(bound_error(
            "protocol.mismatch",
            "/asserted_semantics/protocol",
            "asserted semantics protocol is not oo.source-semantics.asserted/v1",
        ));
    }
    let mut claimed = Vec::new();
    for column in parsed.columns {
        for token in &column.caveats {
            if !ASSERTED_CAVEATS.contains(&token.as_str()) {
                return Err(bound_error(
                    "token.rejected",
                    "/asserted_semantics/columns",
                    "asserted caveat is not a closed token",
                ));
            }
        }
        if let Some(note) = column.native_note.as_deref()
            && !ASSERTED_NOTES.contains(&note)
        {
            return Err(bound_error(
                "token.rejected",
                "/asserted_semantics/columns",
                "asserted native note is not a closed token",
            ));
        }
        let mut caveats = column.caveats;
        caveats.sort();
        caveats.dedup();
        claimed.push((column.column, caveats, column.native_note));
    }
    claimed.sort_by(|left, right| left.0.cmp(&right.0));
    if claimed.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(bound_error(
            "column.mismatch",
            "/asserted_semantics/columns",
            "asserted semantics repeats a column",
        ));
    }
    let mut expected = Vec::new();
    for column in &derived.columns {
        if column.caveats.is_empty() && column.native_note.is_none() {
            continue;
        }
        let mut caveats = column.caveats.clone();
        caveats.sort();
        expected.push((column.logical_name.clone(), caveats, column.native_note.clone()));
    }
    expected.sort_by(|left, right| left.0.cmp(&right.0));
    if claimed != expected {
        return Err(bound_error(
            "token.mismatch",
            "/asserted_semantics/columns",
            "asserted column flags do not match the derived source",
        ));
    }
    Ok(())
}

fn set_derived_column(
    normalized: &mut Normalized,
    qualified: &str,
    caveats: &[String],
    note: Option<&str>,
) {
    let found = normalized
        .columns
        .iter_mut()
        .find(|item| item["column"].as_str() == Some(qualified));
    let Some(entry) = found else {
        normalized.columns.push(json!({
            "caveats": caveats,
            "column": qualified,
            "native_note": note,
            "native_type": "absent",
        }));
        return;
    };
    entry["caveats"] = json!(caveats);
    entry["native_note"] = match note {
        Some(text) => json!(text),
        None => Value::Null,
    };
}

fn bound_error(name: &'static str, path: &str, message: &'static str) -> SemanticsError {
    SemanticsError::new(
        format!("oo.source-semantics.derived/v1/{name}"),
        path,
        message,
    )
}

const ASSERTED_CAVEATS: &[&str] = &[
    "empty_string_is_null",
    "lossy_type_fallback",
    "offset_lost",
    "precision_loss",
    "source_collation_ci",
    "time_ambiguous",
    "trailing_space_insensitive",
];

const ASSERTED_NOTES: &[&str] = &["oracle_date_includes_time", "unconstrained_number"];

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssertedSemantics {
    protocol: String,
    columns: Vec<AssertedColumn>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssertedColumn {
    column: String,
    #[serde(default)]
    caveats: Vec<String>,
    #[serde(default)]
    native_note: Option<String>,
}

fn apply_sealed(
    normalized: &mut Normalized,
    target: &Target,
    prepared: &SealedPrepared,
) -> Result<(), SemanticsError> {
    if target.family != prepared.draft.source_engine || target.product != prepared.draft.source_engine {
        return Err(SemanticsError::new(
            "provenance.conflict",
            "/target/family",
            "target family or product is not the sealed source engine",
        ));
    }
    if target.capture != "sealed_import" {
        return Err(SemanticsError::new(
            "provenance.conflict",
            "/target/capture",
            "target capture is not sealed_import",
        ));
    }
    let mut known = std::collections::BTreeSet::new();
    for column in &prepared.draft.columns {
        known.insert(format!("{}.{}", column.dataset, column.name));
    }
    for column in &target.columns {
        let qualified = format!("{}.{}.{}", column.schema, column.table, column.name);
        if !known.contains(&qualified) {
            return Err(SemanticsError::new(
                "provenance.conflict",
                format!("/target/columns/{qualified}"),
                "target column is not in the sealed import",
            ));
        }
        let source_type = prepared
            .draft
            .columns
            .iter()
            .find(|item| format!("{}.{}", item.dataset, item.name) == qualified)
            .and_then(|item| item.source_type.as_deref());
        if let Some(source_type) = source_type
            && column.native_type != source_type
        {
            return Err(SemanticsError::new(
                "type.ambiguous",
                format!("/target/columns/{qualified}"),
                "target native type does not match the sealed source type",
            ));
        }
    }
    for column in &prepared.draft.columns {
        let qualified = format!("{}.{}", column.dataset, column.name);
        upsert_sealed_column(normalized, &qualified, column);
        if let Some(caveat) = &column.caveat {
            push_unique(&mut normalized.semantic_caveats, caveat);
        }
        if column.precision_loss {
            push_unique(&mut normalized.semantic_caveats, "precision_loss");
        }
    }
    push_unique(&mut normalized.semantic_caveats, "postgresql_target_storage");
    normalized.semantic_caveats.sort();
    normalized.semantic_caveats.dedup();
    add_sealed_keys(normalized, &prepared.draft);
    normalized.constraint_trust.sort_by(|left, right| {
        (&left.schema, &left.table, &left.name).cmp(&(&right.schema, &right.table, &right.name))
    });
    normalized.columns.sort_by(|left, right| {
        left["column"].as_str().cmp(&right["column"].as_str())
    });
    normalized.native_notes = normalized
        .columns
        .iter()
        .filter_map(|column| {
            column["native_note"].as_str().map(|note| {
                json!({"column": column["column"].as_str().unwrap_or(""), "note": note})
            })
        })
        .collect();
    if let Some(object) = normalized.source_semantics.as_object_mut() {
        object.insert(
            "caveats".to_string(),
            json!(normalized.semantic_caveats),
        );
        object.insert(
            "native_type_semantics".to_string(),
            json!(normalized.native_notes),
        );
    }
    Ok(())
}

fn upsert_sealed_column(
    normalized: &mut Normalized,
    qualified: &str,
    column: &oo_builder::SealedImportColumnDraft,
) {
    let native = column.source_type.clone().unwrap_or_else(|| "absent".to_string());
    let found = normalized
        .columns
        .iter_mut()
        .find(|item| item["column"].as_str() == Some(qualified));
    let entry = if let Some(entry) = found {
        entry
    } else {
        normalized.columns.push(json!({
            "caveats": [],
            "column": qualified,
            "native_note": Value::Null,
            "native_type": native,
        }));
        normalized.columns.last_mut().expect("column pushed")
    };
    entry["native_type"] = json!(native);
    if column.source_type.is_none() {
        entry["native_note"] = Value::Null;
    }
    let mut tokens = std::collections::BTreeSet::new();
    if let Some(list) = entry.get("caveats").and_then(Value::as_array) {
        for token in list {
            if let Some(text) = token.as_str() {
                tokens.insert(text.to_string());
            }
        }
    }
    if let Some(caveat) = &column.caveat {
        tokens.insert(caveat.clone());
    }
    if column.precision_loss {
        tokens.insert("precision_loss".to_string());
    }
    entry["caveats"] = json!(tokens.into_iter().collect::<Vec<_>>());
}

fn add_sealed_keys(normalized: &mut Normalized, draft: &SealedImportDraft) {
    let mut tables: std::collections::BTreeMap<(String, String), Vec<String>> =
        std::collections::BTreeMap::new();
    for column in &draft.columns {
        if !column.key {
            continue;
        }
        let Some((schema, table)) = column.dataset.split_once('.') else {
            continue;
        };
        tables
            .entry((schema.to_string(), table.to_string()))
            .or_default()
            .push(column.name.clone());
    }
    for ((schema, table), columns) in tables {
        let exists = normalized.constraint_trust.iter().any(|row| {
            row.kind == "primary_key" && row.schema == schema && row.table == table
        });
        if exists {
            continue;
        }
        normalized.constraint_trust.push(TrustRow {
            schema,
            table: table.clone(),
            name: format!("{table}_key"),
            kind: "primary_key".to_string(),
            columns,
            claimed_trust: "declared_only".to_string(),
            trust: "declared_only".to_string(),
            disposition: "recorded".to_string(),
            inclusion: None,
        });
    }
}

fn push_unique(tokens: &mut Vec<String>, token: &str) {
    if !tokens.iter().any(|item| item == token) {
        tokens.push(token.to_string());
    }
}

fn freshness(target: &Target) -> Value {
    json!({
        "capture_method": target.capture,
        "consistency_grade": target.source_consistency,
        "consistency_source": "compiler_normalized_not_gateway",
        "contract": "pending_gateway_fields",
        "delete_detection": target.delete_detection,
        "gateway_get_freshness": "not_called"
    })
}
