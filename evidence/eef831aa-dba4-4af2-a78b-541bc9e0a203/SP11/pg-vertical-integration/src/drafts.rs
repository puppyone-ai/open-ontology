//! Captured-catalog draft, review decisions, and the role-pack seam.
//!
//! Datasets placed in the effective-oos envelope are the compiler's own
//! `effective.datasets` values. PII and classification are not injected.

use std::collections::BTreeSet;

use oo_builder::{compile_captured_pg_catalog, compile_role_pack, CapturedPgDraft};
use serde_json::{json, Value};

use crate::stop::Stop;

const ACCEPT_KINDS: &[&str] = &[
    "structural_primary_key",
    "structural_column",
    "candidate_relationship",
];

pub struct DraftPass {
    pub unreviewed: CapturedPgDraft,
    pub curated_input: String,
    pub curated: CapturedPgDraft,
    pub reviews_input: String,
    pub reviewed: CapturedPgDraft,
    pub review_note: String,
    pub label_attempt: String,
}

pub fn compile_passes(catalog_json: &str) -> Result<DraftPass, Stop> {
    let unreviewed = compile_one(catalog_json, None, None, "unreviewed")?;
    let draft_value = parse_draft(&unreviewed.draft, "unreviewed")?;
    let (curated_zh, curated_ascii) = curated_documents(&draft_value)?;
    let (curated_input, curated, label_attempt) =
        match compile_one(catalog_json, Some(&curated_zh), None, "curated_zh") {
            Ok(compiled) => (curated_zh, compiled, "chinese_labels_accepted".to_string()),
            Err(error) => {
                let compiled = compile_one(catalog_json, Some(&curated_ascii), None, "curated_ascii")?;
                (
                    curated_ascii,
                    compiled,
                    format!("chinese_labels_refused; ascii_fallback; {error}"),
                )
            }
        };
    let curated_value = parse_draft(&curated.draft, "curated")?;
    let (reviews_input, review_note) = review_document(&curated_value)?;
    let reviewed = compile_one(
        catalog_json,
        Some(&curated_input),
        Some(&reviews_input),
        "reviewed",
    )?;
    Ok(DraftPass {
        unreviewed,
        curated_input,
        curated,
        reviews_input,
        reviewed,
        review_note,
        label_attempt,
    })
}

fn compile_one(
    catalog_json: &str,
    curated: Option<&str>,
    reviews: Option<&str>,
    step: &'static str,
) -> Result<CapturedPgDraft, Stop> {
    compile_captured_pg_catalog(catalog_json, curated, reviews).map_err(|error| {
        let curated_present = curated.is_some();
        let reviews_present = reviews.is_some();
        Stop::new(
            step,
            format!(
                "compile_captured_pg_catalog curated={curated_present} reviews={reviews_present}"
            ),
            "oo.builder.captured-pg-draft/v1",
            error.to_string(),
            "evidence harness only; production compiler is unchanged",
        )
    })
}

fn parse_draft(draft: &str, step: &'static str) -> Result<Value, Stop> {
    serde_json::from_str(draft).map_err(|error| {
        Stop::new(
            step,
            "structure draft",
            "json object",
            error.to_string(),
            "none",
        )
    })
}

fn curated_documents(draft: &Value) -> Result<(String, String), Stop> {
    let datasets = draft
        .pointer("/effective/datasets")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Stop::new(
                "curated_names",
                "unreviewed draft",
                "effective.datasets",
                "missing",
                "none",
            )
        })?;
    let mut zh_sets = Vec::new();
    let mut ascii_sets = Vec::new();
    for dataset in datasets {
        let name = dataset
            .get("dataset")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.is_empty() {
            return Err(Stop::new(
                "curated_names",
                "effective.datasets",
                "dataset name",
                "empty",
                "none",
            ));
        }
        let (label, description) = if name.ends_with(".party") {
            ("交易方", "企业主体名录")
        } else if name.ends_with(".invoice") {
            ("发票", "发票单据名录")
        } else {
            ("实体", "未命名数据集")
        };
        zh_sets.push(json!({
            "dataset": name,
            "label": label,
            "description": description,
        }));
        ascii_sets.push(json!({
            "dataset": name,
            "label": name.replace('.', "_"),
            "description": "enterprise fixture dataset",
        }));
    }
    let relationships = draft
        .pointer("/effective/relationships")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rels = Vec::new();
    for relationship in relationships {
        if let Some(name) = relationship.get("name").and_then(Value::as_str) {
            rels.push(json!({"name": name, "status": "draft"}));
        }
    }
    Ok((
        json!({"datasets": zh_sets, "relationships": rels}).to_string(),
        json!({"datasets": ascii_sets, "relationships": rels}).to_string(),
    ))
}

fn review_document(draft: &Value) -> Result<(String, String), Stop> {
    let schema_hash = draft
        .pointer("/provenance/schema_hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if schema_hash.len() != 64 {
        return Err(Stop::new(
            "reviews",
            "provenance.schema_hash",
            "64 lowercase hex",
            schema_hash,
            "none",
        ));
    }
    let queue = draft
        .get("review_queue")
        .and_then(Value::as_array)
        .ok_or_else(|| Stop::new("reviews", "review_queue", "array", "missing", "none"))?;
    let mut accept_ids = BTreeSet::new();
    let mut reviews = Vec::new();
    let mut skipped = Vec::new();
    for item in queue {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let item_id = item.get("item_id").and_then(Value::as_str).unwrap_or("");
        let fingerprint = item.get("fact_fingerprint").and_then(Value::as_str);
        if !ACCEPT_KINDS.contains(&kind) {
            continue;
        }
        let Some(fingerprint) = fingerprint else {
            skipped.push(format!("accept_without_fingerprint:{item_id}:{kind}"));
            continue;
        };
        if item_id.is_empty() || !accept_ids.insert(item_id.to_string()) {
            skipped.push(format!("duplicate_item:{item_id}:{kind}"));
            continue;
        }
        reviews.push(json!({
            "item_id": item_id,
            "action": "accept",
            "schema_hash": schema_hash,
            "fact_fingerprint": fingerprint,
        }));
    }
    for item in queue {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        if kind != "curated_unbound" {
            continue;
        }
        let item_id = item.get("item_id").and_then(Value::as_str).unwrap_or("");
        let fingerprint = item.get("fact_fingerprint").and_then(Value::as_str);
        let Some(fingerprint) = fingerprint else {
            skipped.push(format!("override_without_fingerprint:{item_id}"));
            continue;
        };
        if item_id.is_empty() {
            continue;
        }
        if accept_ids.contains(item_id) {
            skipped.push(format!(
                "one_review_per_item_id blocks override beside accept:{item_id}"
            ));
            continue;
        }
        if !accept_ids.insert(item_id.to_string()) {
            skipped.push(format!("duplicate_override:{item_id}"));
            continue;
        }
        reviews.push(json!({
            "item_id": item_id,
            "action": "override",
            "schema_hash": schema_hash,
            "fact_fingerprint": fingerprint,
        }));
    }
    let note = if skipped.is_empty() {
        "no review collisions".to_string()
    } else {
        skipped.join("\n")
    };
    Ok((Value::Array(reviews).to_string(), note))
}

pub fn queue_snapshot(draft: &str) -> Result<Value, Stop> {
    let value = parse_draft(draft, "queue_snapshot")?;
    let queue = value
        .get("review_queue")
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    let publication = value
        .pointer("/provenance/publication")
        .cloned()
        .unwrap_or(Value::Null);
    let integration = value
        .pointer("/provenance/pg_catalog_integration")
        .cloned()
        .unwrap_or(Value::Null);
    let grants = value
        .pointer("/provenance/grants")
        .cloned()
        .unwrap_or(Value::Null);
    Ok(json!({
        "publication": publication,
        "pg_catalog_integration": integration,
        "grants": grants,
        "review_queue": queue,
    }))
}

pub struct PackAttempt {
    pub input_kind: &'static str,
    pub error_code: String,
    pub error_path: String,
    pub error_message: String,
    pub compiled: bool,
}

pub fn role_pack_attempts(reviewed_draft: &str) -> Result<(Vec<PackAttempt>, String, String), Stop> {
    let visibility = visibility_for_draft(reviewed_draft)?;
    let raw = pack_once("structure_draft_json", reviewed_draft, &visibility);
    let envelope = effective_envelope(reviewed_draft, true)?;
    let embedded = pack_once("compiler_datasets_and_relationships", &envelope, &visibility);
    let mut attempts = vec![raw, embedded];
    let datasets_only = if attempts[1].compiled {
        String::new()
    } else {
        let bare = effective_envelope(reviewed_draft, false)?;
        let attempt = pack_once("compiler_datasets_only", &bare, &visibility);
        let text = bare;
        attempts.push(attempt);
        text
    };
    Ok((attempts, envelope, datasets_only))
}

fn pack_once(kind: &'static str, effective: &str, visibility: &str) -> PackAttempt {
    match compile_role_pack(effective, visibility) {
        Ok(_) => PackAttempt {
            input_kind: kind,
            error_code: String::new(),
            error_path: String::new(),
            error_message: String::new(),
            compiled: true,
        },
        Err(error) => PackAttempt {
            input_kind: kind,
            error_code: error.code,
            error_path: error.path,
            error_message: error.message.to_string(),
            compiled: false,
        },
    }
}

fn effective_envelope(draft: &str, include_relationships: bool) -> Result<String, Stop> {
    let value = parse_draft(draft, "effective_envelope")?;
    let datasets = value
        .pointer("/effective/datasets")
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    let relationships = if include_relationships {
        value
            .pointer("/effective/relationships")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()))
    } else {
        Value::Array(Vec::new())
    };
    Ok(json!({
        "protocol": "oo.builder.effective-oos/v1",
        "project": "a203ent",
        "ontology_version": "a203.1",
        "catalog_version": "a203.1",
        "policy": {"id": "pol_a203", "version": "1"},
        "knowledge": {"id": "know_a203", "version": "1"},
        "ontology": {
            "spec_version": "oos/v1",
            "name": "a203ent",
            "locale": "zh-CN",
            "include_schemas": ["ent"],
            "default_tier_preference": ["raw"],
            "publish": {"include_unreviewed": []},
            "classification": {"auto_accept": "rule_high_confidence"},
            "lifecycle": {"deprecation_min_releases": 2},
            "compile": {
                "per_role": true,
                "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                "sample_values": {"default": "masked", "max_per_column": 0}
            }
        },
        "datasets": datasets,
        "relationships": relationships
    })
    .to_string())
}

pub fn visibility_for_draft(draft: &str) -> Result<String, Stop> {
    let value = parse_draft(draft, "visibility")?;
    let datasets = value
        .pointer("/effective/datasets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rows = Vec::new();
    for dataset in datasets {
        let name = dataset
            .get("dataset")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let columns = dataset
            .get("columns")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("name").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if name.is_empty() || columns.is_empty() {
            continue;
        }
        rows.push(json!({
            "dataset": name,
            "row_security": "none",
            "global_stats": "denied",
            "columns": columns,
        }));
    }
    if rows.is_empty() {
        return Err(Stop::new(
            "visibility",
            "effective.datasets",
            "at least one dataset with columns",
            "none",
            "none",
        ));
    }
    Ok(json!({
        "protocol": "oo.builder.role-visibility/v1",
        "project": "a203ent",
        "principal": "a203reader",
        "role": "oo_r_party_read",
        "ontology_version": "a203.1",
        "catalog_version": "a203.1",
        "policy": {"id": "pol_a203", "version": "1"},
        "knowledge": {"id": "know_a203", "version": "1"},
        "issued_for_compiler": "oo-builder/role-pack/v1",
        "datasets": rows,
        "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
    })
    .to_string())
}

