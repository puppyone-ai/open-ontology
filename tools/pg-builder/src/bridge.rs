//! Dataset review and role-pack bridge.
//!
//! A validated generation is rebound onto description prompt v2. Column text
//! is written only by an explicit caller decision. See CONTRACT.md.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use description_pack::apply_description_pack;
use description_review::{apply_description_review, curated_dataset_sha256, descriptive_field_sha256};
use oo_builder::{
    compile_captured_pg_catalog, prepare_description_request, DESCRIPTION_PROMPT_VERSION,
    DESCRIPTION_REQUEST_COMMAND_PROTOCOL, DESCRIPTION_REQUEST_PROTOCOL,
};
use oo_core::oos::DatasetFile;

use crate::canonical::{canonical_json, canonical_sha256, object};
use crate::error::PgBuilderError;
use crate::rebind::{rebind_generation, Rebound};
use crate::{field_seam, sha256_hex};

pub struct PackIn<'a> {
    pub generation: &'a Value,
    pub curated: &'a Value,
    pub operator: &'a Value,
    pub settings: &'a Value,
    pub classifications: &'a Value,
    pub roles: &'a Value,
    pub hidden_columns: &'a [&'a str],
}

pub fn seal_request(catalog_json: &str) -> Result<Value, PgBuilderError> {
    let compiled = compile_captured_pg_catalog(catalog_json, None, None).map_err(|_| {
        PgBuilderError::new("compiler.seal", "the captured catalog was refused")
    })?;
    let catalog: Value = serde_json::from_str(catalog_json)
        .map_err(|_| PgBuilderError::new("catalog.schema", "the catalog is not JSON"))?;
    let tables = catalog
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog has no tables"))?;
    let mut selection = Vec::new();
    for table in tables {
        let schema = text_field(table, "schema")?;
        let schema_ident = text_field(table, "schema_ident")?;
        let name = text_field(table, "name")?;
        let name_ident = text_field(table, "name_ident")?;
        selection.push(json!({
            "dataset": format!("{schema_ident}.{name_ident}"),
            "evidence_hash": compiled.evidence_hash,
            "name": name,
            "name_ident": name_ident,
            "schema": schema,
            "schema_ident": schema_ident,
            "schema_hash": compiled.schema_hash,
            "visibility": "visible"
        }));
    }
    selection.sort_by(|left, right| {
        (left["schema"].as_str(), left["name"].as_str()).cmp(&(right["schema"].as_str(), right["name"].as_str()))
    });
    let artifact = json!({
        "draft": parse_compiled(&compiled.draft)?,
        "evidence_envelope": parse_compiled(&compiled.evidence_envelope)?,
        "protocol": "oo.builder.captured-pg-draft-command/v1",
        "wrapper": parse_compiled(&compiled.wrapper)?
    });
    let command = json!({
        "admitted": true,
        "artifact": artifact,
        "binding": {
            "model_id": "grok-4.7",
            "prompt_version": DESCRIPTION_PROMPT_VERSION,
            "source_id": compiled.source_id
        },
        "budget": {
            "max_caveat_chars": 2000,
            "max_caveats": 8,
            "max_columns_per_table": 64,
            "max_description_chars": 4000,
            "max_grain_chars": 256,
            "max_label_chars": 256,
            "max_open_questions": 8,
            "max_question_chars": 2000,
            "max_request_bytes": 65536,
            "max_response_bytes": 65536,
            "max_tables": 16
        },
        "protocol": DESCRIPTION_REQUEST_COMMAND_PROTOCOL,
        "selection": {
            "evidence_hash": compiled.evidence_hash,
            "schema_hash": compiled.schema_hash,
            "tables": selection
        }
    });
    let command_text = canonical_json(&command);
    let prepared = prepare_description_request(&command_text).map_err(|_| {
        PgBuilderError::new("compiler.request", "the description request was refused")
    })?;
    let prepared_value: Value = serde_json::from_str(&prepared)
        .map_err(|_| PgBuilderError::new("compiler.request", "the description request was refused"))?;
    let prepare_status = if prepared_value.get("protocol").and_then(Value::as_str) == Some(DESCRIPTION_REQUEST_PROTOCOL)
    {
        "request"
    } else {
        "budget_failure"
    };
    let prepared_request = if prepare_status == "request" {
        prepared_value
    } else {
        Value::Null
    };
    Ok(json!({
        "protocol": "oo.pg-builder.seal/v1",
        "schema_hash": compiled.schema_hash,
        "evidence_hash": compiled.evidence_hash,
        "source_id": compiled.source_id,
        "prepare_status": prepare_status,
        "prepared_request": prepared_request,
        "prepared_bytes": prepared.len(),
        "artifact": artifact,
        "command": command_text,
        "command_bytes": command_text.len()
    }))
}

pub fn scaffold_curated(catalog: &Value) -> Result<Value, PgBuilderError> {
    let tables = catalog
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog has no tables"))?;
    let mut map = Map::new();
    for table in tables {
        let schema = text_field(table, "schema")?;
        let name = text_field(table, "name")?;
        let dataset = format!("{schema}.{name}");
        let mut primary_key = Vec::new();
        if let Some(keys) = table.get("primary_key").and_then(Value::as_array) {
            for key in keys {
                primary_key.push(Value::String(text_field(key, "name")?));
            }
        }
        let columns = table
            .get("columns")
            .and_then(Value::as_array)
            .ok_or_else(|| PgBuilderError::new("catalog.schema", "a table has no columns"))?;
        let mut column_files = Vec::new();
        for column in columns {
            column_files.push(json!({"name": text_field(column, "name")?}));
        }
        let file = json!({
            "schema_version": "oos/v1",
            "dataset": dataset,
            "label": name,
            "tier": "raw",
            "primary_key": primary_key,
            "columns": column_files
        });
        let parsed: DatasetFile = serde_json::from_value(file).map_err(|_| {
            PgBuilderError::new("curated.schema", "the catalog row is not a dataset file")
        })?;
        let value = serde_json::to_value(&parsed).map_err(|_| {
            PgBuilderError::new("curated.schema", "the catalog row is not a dataset file")
        })?;
        map.insert(dataset, value);
    }
    Ok(Value::Object(map))
}

pub fn review_only(generation: &Value, curated: &Value, operator: &Value) -> Result<Value, PgBuilderError> {
    if is_aggregate(generation) {
        return review_aggregate(generation, curated, operator);
    }
    review_one(generation, curated, operator)
}

pub fn pack_pair(input: &PackIn<'_>) -> Result<Value, PgBuilderError> {
    if is_aggregate(input.generation) {
        return pack_aggregate(input);
    }
    pack_one(input)
}

fn review_one(generation: &Value, curated: &Value, operator: &Value) -> Result<Value, PgBuilderError> {
    let rebound = rebind_generation(generation)?;
    if !rebound.bound {
        return Ok(json!({
            "protocol": "oo.pg-builder.review-bridge/v1",
            "authenticated": false,
            "parent_auth": "required",
            "outcome": "rebind_refused",
            "rebind": rebound.provenance,
            "field_publication_seam": field_seam(),
            "statements": {
                "field_candidates_applied": false,
                "publication": "unavailable",
                "authenticated": false,
                "parent_auth": "required"
            }
        }));
    }
    let command = build_review_command(&rebound, curated, operator, false)?;
    let bytes = apply_description_review(&command).map_err(forward_review)?;
    let review: Value = serde_json::from_str(&bytes).map_err(|_| {
        PgBuilderError::new("review.shape", "the description review document was refused")
    })?;
    let mut document = bridge_document("oo.pg-builder.review-bridge/v1", &bytes, command, review);
    if let Some(map) = document.as_object_mut() {
        map.insert("rebind".to_owned(), rebound.provenance);
    }
    Ok(document)
}

fn review_aggregate(generation: &Value, curated: &Value, operator: &Value) -> Result<Value, PgBuilderError> {
    let calls = generation
        .get("calls")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("review.blocked", "the aggregate has no calls"))?;
    let any_validated = calls.iter().any(|call| {
        call.get("outcome").and_then(Value::as_str) == Some("validated")
    });
    if !any_validated {
        return Err(PgBuilderError::new(
            "review.blocked",
            "review requires a validated generation",
        ));
    }
    let mut tables = Vec::new();
    for call in calls {
        if call.get("outcome").and_then(Value::as_str) == Some("validated") {
            tables.push(review_one(call, curated, operator)?);
        } else {
            tables.push(json!({
                "protocol": "oo.pg-builder.review-bridge/v1",
                "dataset": call.get("dataset").cloned().unwrap_or(Value::Null),
                "outcome": "blocked",
                "authenticated": false,
                "parent_auth": "required"
            }));
        }
    }
    let reviewed = tables.iter().all(|table| table.get("outcome").and_then(Value::as_str) == Some("reviewed"));
    Ok(json!({
        "protocol": "oo.pg-builder.review-bridge/v1",
        "authenticated": false,
        "parent_auth": "required",
        "outcome": if reviewed { "reviewed" } else { "partial" },
        "tables": tables
    }))
}

fn pack_one(input: &PackIn<'_>) -> Result<Value, PgBuilderError> {
    require_validated(input.generation)?;
    let artifact = command_artifact(input.generation)?;
    let draft = artifact
        .get("draft")
        .ok_or_else(|| PgBuilderError::new("compiler.seal", "the sealed draft is missing"))?;
    let classes = fill_classes(draft, input.classifications)?;
    require_role_coverage(input.roles, &classes)?;
    let rebound = rebind_generation(input.generation)?;
    if !rebound.bound {
        return Err(PgBuilderError::new("review.rebind", "the rebound body was refused"));
    }
    let after_command = build_review_command(&rebound, input.curated, input.operator, false)?;
    let before_command = build_review_command(&rebound, input.curated, input.operator, true)?;
    let after_review = apply_description_review(&after_command).map_err(forward_review)?;
    let before_review = apply_description_review(&before_command).map_err(forward_review)?;
    let after_pack = apply_pack(&after_command, &after_review, &artifact, input.settings, &classes, input.roles)?;
    let before_pack = apply_pack(&before_command, &before_review, &artifact, input.settings, &classes, input.roles)?;
    let before_text = canonical_json(&before_pack);
    let after_text = canonical_json(&after_pack);
    let field_text_absent = field_text_absent(input.generation, &before_text, &after_text);
    let applied = column_text_applied(input.generation, &before_text, &after_text);
    let hidden = hidden_report(&before_text, &after_text, input.hidden_columns);
    Ok(json!({
        "protocol": "oo.pg-builder.pack-bridge/v1",
        "authenticated": false,
        "parent_auth": "required",
        "field_publication_seam": field_seam(),
        "rebind": rebound.provenance,
        "before": before_pack,
        "after": after_pack,
        "diff": {
            "before_artifact_sha256": before_pack.get("artifact_sha256").cloned().unwrap_or(Value::Null),
            "after_artifact_sha256": after_pack.get("artifact_sha256").cloned().unwrap_or(Value::Null),
            "before_packs": pack_hashes(&before_pack),
            "after_packs": pack_hashes(&after_pack),
            "changed": before_text != after_text
        },
        "field_text_absent": field_text_absent,
        "hidden_columns": hidden,
        "statements": {
            "field_candidates_applied": applied,
            "model_publication": false,
            "authenticated": false,
            "parent_auth": "required",
            "pii_classification": "caller_fixture",
            "semantic_classification": "caller_fixture"
        }
    }))
}

fn pack_aggregate(input: &PackIn<'_>) -> Result<Value, PgBuilderError> {
    let calls = input
        .generation
        .get("calls")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("review.blocked", "the aggregate has no calls"))?;
    let mut tables = Vec::new();
    let mut packed = 0u32;
    for call in calls {
        if call.get("outcome").and_then(Value::as_str) != Some("validated") {
            tables.push(json!({
                "dataset": call.get("dataset").cloned().unwrap_or(Value::Null),
                "outcome": "blocked",
                "authenticated": false,
                "parent_auth": "required"
            }));
            continue;
        }
        let one = PackIn {
            generation: call,
            curated: input.curated,
            operator: input.operator,
            settings: input.settings,
            classifications: input.classifications,
            roles: input.roles,
            hidden_columns: input.hidden_columns,
        };
        match pack_one(&one) {
            Ok(document) => {
                packed = packed.saturating_add(1);
                tables.push(document);
            }
            Err(err) if err.code.ends_with("review.rebind") => {
                tables.push(json!({
                    "dataset": call.get("dataset").cloned().unwrap_or(Value::Null),
                    "outcome": "rebind_refused",
                    "authenticated": false,
                    "parent_auth": "required"
                }));
            }
            Err(err) => return Err(err),
        }
    }
    if packed == 0 {
        return Err(PgBuilderError::new("review.blocked", "review requires a validated generation"));
    }
    Ok(json!({
        "protocol": "oo.pg-builder.pack-bridge/v1",
        "authenticated": false,
        "parent_auth": "required",
        "tables": tables,
        "statements": {
            "model_publication": false,
            "authenticated": false,
            "parent_auth": "required"
        }
    }))
}

fn is_aggregate(generation: &Value) -> bool {
    generation.get("calls").and_then(Value::as_array).is_some()
        && generation.get("complete_command").and_then(Value::as_str).is_none()
}

fn apply_pack(
    review_command: &str,
    review_bytes: &str,
    artifact: &Value,
    settings: &Value,
    classes: &[Value],
    roles: &Value,
) -> Result<Value, PgBuilderError> {
    let command = json!({
        "protocol": description_pack::COMMAND_PROTOCOL,
        "review": review_command,
        "expected_review_sha256": sha256_hex(review_bytes.as_bytes()),
        "artifact": artifact,
        "settings": settings,
        "decisions": classes,
        "roles": roles
    });
    let text = serde_json::to_string(&command).map_err(|_| {
        PgBuilderError::new("pack.schema", "the pack command could not be encoded")
    })?;
    let packed = apply_description_pack(&text).map_err(forward_pack)?;
    serde_json::from_str(&packed).map_err(|_| PgBuilderError::new("pack.shape", "the pack document was refused"))
}

fn build_review_command(
    rebound: &Rebound,
    curated: &Value,
    operator: &Value,
    force_defer: bool,
) -> Result<String, PgBuilderError> {
    let outcome = &rebound.outcome;
    if outcome.get("outcome").and_then(Value::as_str) != Some("candidates") {
        return Err(PgBuilderError::new("review.blocked", "review requires description candidates"));
    }
    let operator = filter_operator(operator, outcome)?;
    let decisions = build_decisions(outcome, curated, &operator, force_defer)?;
    let column_decisions = build_column_decisions(outcome, curated, &operator, force_defer)?;
    let schema = outcome
        .pointer("/structural_draft/schema_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("review.anchor", "the compiler draft has no schema hash"))?;
    let evidence = outcome
        .pointer("/structural_draft/evidence_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("review.anchor", "the compiler draft has no evidence hash"))?;
    let command = json!({
        "protocol": description_review::COMMAND_PROTOCOL,
        "command": rebound.command,
        "expected_outcome_sha256": sha256_hex(rebound.outcome_bytes.as_bytes()),
        "current": {"schema_hash": schema, "evidence_hash": evidence},
        "curated": curated,
        "decisions": decisions,
        "column_decisions": column_decisions
    });
    serde_json::to_string(&command).map_err(|_| PgBuilderError::new("review.schema", "the review command could not be encoded"))
}

fn build_decisions(
    outcome: &Value,
    curated: &Value,
    operator: &Value,
    force_defer: bool,
) -> Result<Vec<Value>, PgBuilderError> {
    let map = require_declared(operator, "review.schema")?;
    if !operator_root_keys(map) {
        return Err(PgBuilderError::new(
            "review.schema",
            "the object has an unknown or missing field",
        ));
    }
    if map.get("protocol").and_then(Value::as_str) != Some("oo.pg-builder.operator-review/v1") {
        return Err(PgBuilderError::new("review.schema", "the operator review protocol is wrong"));
    }
    let rows = map
        .get("decisions")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("review.schema", "the operator review has no decisions"))?;
    let mut decisions = Vec::new();
    for row in rows {
        let item = object(row, "review.schema")?;
        if !decision_keys(item) {
            return Err(PgBuilderError::new("review.schema", "the operator decision shape is wrong"));
        }
        let dataset = required_text(item, "dataset", "review.schema")?;
        let mut kind = required_text(item, "decision", "review.schema")?;
        let reviewer_label = required_text(item, "reviewer_label", "review.schema")?;
        let reviewer_at = required_text(item, "reviewer_at", "review.schema")?;
        let mut fields = item.get("fields").cloned();
        if force_defer {
            kind = "defer".to_owned();
            fields = None;
        }
        let approve = kind == "approve";
        if approve {
            if fields.is_none() {
                return Err(PgBuilderError::new("review.schema", "an approval requires fields"));
            }
        } else if kind == "reject" || kind == "defer" {
            if item.get("fields").is_some() && !force_defer {
                return Err(PgBuilderError::new("review.schema", "reject and defer do not take fields"));
            }
            fields = None;
        } else {
            return Err(PgBuilderError::new("review.schema", "the decision is not approve, reject, or defer"));
        }
        let candidate = outcome_candidate(outcome, &dataset)?;
        let curated_file = curated.get(&dataset).ok_or_else(|| {
            PgBuilderError::new("curated.missing", "a decision dataset has no curated file")
        })?;
        let parsed: DatasetFile = serde_json::from_value(curated_file.clone()).map_err(|_| {
            PgBuilderError::new("curated.schema", "a curated dataset is not a core dataset file")
        })?;
        let mut decision = json!({
            "candidate_sha256": canonical_sha256(candidate),
            "card_sha256": candidate.get("card_sha256").cloned().unwrap_or(Value::Null),
            "curated_sha256": curated_dataset_sha256(&parsed),
            "dataset": dataset,
            "decision": kind,
            "evidence_hash": outcome.pointer("/structural_draft/evidence_hash").cloned().unwrap_or(Value::Null),
            "request_sha256": outcome.get("request_sha256").cloned().unwrap_or(Value::Null),
            "reviewer_at": reviewer_at,
            "reviewer_label": reviewer_label,
            "schema_hash": outcome.pointer("/structural_draft/schema_hash").cloned().unwrap_or(Value::Null)
        });
        if approve {
            decision["fields"] = approve_fields(fields.as_ref().unwrap_or(&Value::Null), curated_file)?;
        }
        decisions.push(decision);
    }
    Ok(decisions)
}

fn decision_keys(map: &Map<String, Value>) -> bool {
    let required = ["dataset", "decision", "reviewer_at", "reviewer_label"];
    required.iter().all(|key| map.contains_key(*key))
        && map.keys().all(|key| required.contains(&key.as_str()) || key == "fields")
}

fn operator_root_keys(map: &Map<String, Value>) -> bool {
    let required = ["authenticated", "decisions", "parent_auth", "protocol"];
    required.iter().all(|key| map.contains_key(*key))
        && map.keys().all(|key| required.contains(&key.as_str()) || key == "column_decisions")
}

fn filter_operator(operator: &Value, outcome: &Value) -> Result<Value, PgBuilderError> {
    let mut selected = BTreeSet::new();
    if let Some(rows) = outcome.get("selected_datasets").and_then(Value::as_array) {
        for row in rows {
            if let Some(name) = row.as_str() {
                selected.insert(name.to_owned());
            }
        }
    }
    let mut copy = operator.clone();
    let map = copy
        .as_object_mut()
        .ok_or_else(|| PgBuilderError::new("review.schema", "the operator review is not an object"))?;
    if let Some(rows) = map.get_mut("decisions").and_then(Value::as_array_mut) {
        rows.retain(|row| {
            row.get("dataset")
                .and_then(Value::as_str)
                .is_some_and(|dataset| selected.contains(dataset))
        });
    }
    if let Some(rows) = map.get_mut("column_decisions").and_then(Value::as_array_mut) {
        rows.retain(|row| {
            row.get("dataset")
                .and_then(Value::as_str)
                .is_some_and(|dataset| selected.contains(dataset))
        });
    }
    Ok(copy)
}

fn build_column_decisions(
    outcome: &Value,
    curated: &Value,
    operator: &Value,
    force_defer: bool,
) -> Result<Vec<Value>, PgBuilderError> {
    let map = object(operator, "review.schema")?;
    let Some(rows) = map.get("column_decisions") else {
        return Ok(Vec::new());
    };
    let rows = rows
        .as_array()
        .ok_or_else(|| PgBuilderError::new("review.schema", "column decisions are not an array"))?;
    let outcome_columns = outcome.get("columns").and_then(Value::as_array).ok_or_else(|| {
        PgBuilderError::new("review.anchor", "the compiler outcome has no columns")
    })?;
    let mut by_key: BTreeMap<(String, String), &Value> = BTreeMap::new();
    for row in rows {
        let item = object(row, "review.schema")?;
        if !column_decision_keys(item) {
            return Err(PgBuilderError::new(
                "review.schema",
                "the operator column decision shape is wrong",
            ));
        }
        let dataset = required_text(item, "dataset", "review.schema")?;
        let column = required_text(item, "column", "review.schema")?;
        if by_key.insert((dataset, column), row).is_some() {
            return Err(PgBuilderError::new("review.column", "a column has more than one decision"));
        }
    }
    if by_key.len() != outcome_columns.len() {
        return Err(PgBuilderError::new(
            "review.column",
            "column decisions do not cover the compiler columns",
        ));
    }
    let mut decisions = Vec::new();
    for column in outcome_columns {
        let dataset = column
            .get("dataset")
            .and_then(Value::as_str)
            .ok_or_else(|| PgBuilderError::new("review.anchor", "a compiler column has no dataset"))?;
        let name = column
            .get("column")
            .and_then(Value::as_str)
            .ok_or_else(|| PgBuilderError::new("review.anchor", "a compiler column has no name"))?;
        let row = by_key.get(&(dataset.to_owned(), name.to_owned())).ok_or_else(|| {
            PgBuilderError::new("review.column", "column decisions do not cover the compiler columns")
        })?;
        let item = object(row, "review.schema")?;
        let mut kind = required_text(item, "decision", "review.schema")?;
        let reviewer_label = required_text(item, "reviewer_label", "review.schema")?;
        let reviewer_at = required_text(item, "reviewer_at", "review.schema")?;
        let mut fields = item.get("fields").cloned();
        if force_defer {
            kind = "defer".to_owned();
            fields = None;
        }
        let approve = kind == "approve";
        if approve {
            if fields.is_none() {
                return Err(PgBuilderError::new("review.schema", "an approval requires fields"));
            }
        } else if kind == "reject" || kind == "defer" {
            if item.get("fields").is_some() && !force_defer {
                return Err(PgBuilderError::new("review.schema", "reject and defer do not take fields"));
            }
            fields = None;
        } else {
            return Err(PgBuilderError::new(
                "review.schema",
                "the decision is not approve, reject, or defer",
            ));
        }
        let ident = column
            .get("ident")
            .and_then(Value::as_str)
            .ok_or_else(|| PgBuilderError::new("review.anchor", "a compiler column has no ident"))?;
        let curated_file = curated.get(dataset).ok_or_else(|| {
            PgBuilderError::new("curated.missing", "a decision dataset has no curated file")
        })?;
        let parsed: DatasetFile = serde_json::from_value(curated_file.clone()).map_err(|_| {
            PgBuilderError::new("curated.schema", "a curated dataset is not a core dataset file")
        })?;
        let attnum = column
            .get("attnum")
            .and_then(Value::as_i64)
            .ok_or_else(|| PgBuilderError::new("review.anchor", "a compiler column has no attnum"))?;
        let mut decision = json!({
            "attnum": attnum,
            "candidate_sha256": canonical_sha256(column),
            "card_sha256": column.get("card_sha256").cloned().unwrap_or(Value::Null),
            "column": name,
            "curated_sha256": curated_dataset_sha256(&parsed),
            "dataset": dataset,
            "decision": kind,
            "evidence_hash": outcome.pointer("/structural_draft/evidence_hash").cloned().unwrap_or(Value::Null),
            "request_sha256": outcome.get("request_sha256").cloned().unwrap_or(Value::Null),
            "reviewer_at": reviewer_at,
            "reviewer_label": reviewer_label,
            "schema_hash": outcome.pointer("/structural_draft/schema_hash").cloned().unwrap_or(Value::Null)
        });
        if approve {
            decision["fields"] = column_approve_fields(fields.as_ref().unwrap_or(&Value::Null), curated_file, ident)?;
        }
        decisions.push(decision);
    }
    Ok(decisions)
}

fn column_decision_keys(map: &Map<String, Value>) -> bool {
    let required = ["column", "dataset", "decision", "reviewer_at", "reviewer_label"];
    required.iter().all(|key| map.contains_key(*key))
        && map.keys().all(|key| required.contains(&key.as_str()) || key == "fields")
}

fn column_approve_fields(fields: &Value, curated_file: &Value, ident: &str) -> Result<Value, PgBuilderError> {
    let map = object(fields, "review.schema")?;
    require_exact(map, &["description", "label"], "review.schema")?;
    Ok(json!({
        "label": column_field_mode(map.get("label").unwrap_or(&Value::Null), curated_file, ident, "label")?,
        "description": column_field_mode(map.get("description").unwrap_or(&Value::Null), curated_file, ident, "description")?
    }))
}

fn column_field_mode(
    value: &Value,
    curated_file: &Value,
    ident: &str,
    field: &str,
) -> Result<Value, PgBuilderError> {
    let map = object(value, "review.schema")?;
    require_exact(map, &["overwrite"], "review.schema")?;
    let overwrite = map
        .get("overwrite")
        .and_then(Value::as_bool)
        .ok_or_else(|| PgBuilderError::new("review.schema", "overwrite is not a boolean"))?;
    if overwrite {
        Ok(json!({
            "overwrite": true,
            "current_sha256": descriptive_field_sha256(curated_column_text(curated_file, ident, field))
        }))
    } else {
        Ok(json!({"overwrite": false}))
    }
}

fn curated_column_text<'a>(file: &'a Value, ident: &str, field: &str) -> Option<&'a str> {
    let columns = file.get("columns")?.as_array()?;
    let column = columns
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(ident))?;
    column.get(field).and_then(Value::as_str)
}

fn approve_fields(fields: &Value, curated_file: &Value) -> Result<Value, PgBuilderError> {
    let map = object(fields, "review.schema")?;
    require_exact(map, &["description", "grain", "label"], "review.schema")?;
    Ok(json!({
        "label": field_mode(map.get("label").unwrap_or(&Value::Null), curated_file, "label")?,
        "description": field_mode(map.get("description").unwrap_or(&Value::Null), curated_file, "description")?,
        "grain": field_mode(map.get("grain").unwrap_or(&Value::Null), curated_file, "grain")?
    }))
}

fn field_mode(value: &Value, curated_file: &Value, name: &str) -> Result<Value, PgBuilderError> {
    let map = object(value, "review.schema")?;
    require_exact(map, &["overwrite"], "review.schema")?;
    let overwrite = map
        .get("overwrite")
        .and_then(Value::as_bool)
        .ok_or_else(|| PgBuilderError::new("review.schema", "overwrite is not a boolean"))?;
    if overwrite {
        let current = curated_file.get(name).and_then(Value::as_str);
        Ok(json!({"overwrite": true, "current_sha256": descriptive_field_sha256(current)}))
    } else {
        Ok(json!({"overwrite": false}))
    }
}

fn fill_classes(draft: &Value, operator: &Value) -> Result<Vec<Value>, PgBuilderError> {
    let map = require_declared(operator, "classification.schema")?;
    require_exact(
        map,
        &["authenticated", "decisions", "parent_auth", "protocol"],
        "classification.schema",
    )?;
    if map.get("protocol").and_then(Value::as_str) != Some("oo.pg-builder.operator-classification/v1") {
        return Err(PgBuilderError::new(
            "classification.schema",
            "the classification protocol is wrong",
        ));
    }
    let schema_hash = draft
        .pointer("/provenance/schema_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("classification.fingerprint", "the draft has no schema hash"))?;
    let rows = map
        .get("decisions")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("classification.schema", "the classification list is empty"))?;
    let mut out = Vec::new();
    for row in rows {
        let item = object(row, "classification.schema")?;
        require_exact(
            item,
            &["column", "dataset", "pii", "reviewer", "semantic_type"],
            "classification.schema",
        )?;
        let dataset = required_text(item, "dataset", "classification.schema")?;
        let column = required_text(item, "column", "classification.schema")?;
        let reviewer = object(item.get("reviewer").unwrap_or(&Value::Null), "classification.schema")?;
        require_exact(reviewer, &["at", "by"], "classification.schema")?;
        out.push(json!({
            "dataset": dataset,
            "column": column,
            "schema_hash": schema_hash,
            "fact_fingerprint": fingerprint(draft, &dataset, &column)?,
            "pii": required_text(item, "pii", "classification.schema")?,
            "semantic_type": required_text(item, "semantic_type", "classification.schema")?,
            "reviewer": {
                "by": required_text(reviewer, "by", "classification.schema")?,
                "at": required_text(reviewer, "at", "classification.schema")?
            }
        }));
    }
    Ok(out)
}

fn require_role_coverage(roles: &Value, classes: &[Value]) -> Result<(), PgBuilderError> {
    let rows = roles
        .as_array()
        .ok_or_else(|| PgBuilderError::new("pack.schema", "roles is not an array"))?;
    let mut covered = BTreeSet::new();
    for class in classes {
        let dataset = class.get("dataset").and_then(Value::as_str).unwrap_or("");
        let column = class.get("column").and_then(Value::as_str).unwrap_or("");
        covered.insert((dataset.to_owned(), column.to_owned()));
    }
    for role in rows {
        let datasets = role
            .get("datasets")
            .and_then(Value::as_array)
            .ok_or_else(|| PgBuilderError::new("pack.schema", "a role has no datasets"))?;
        for dataset in datasets {
            let name = dataset
                .get("dataset")
                .and_then(Value::as_str)
                .ok_or_else(|| PgBuilderError::new("pack.schema", "a role dataset is missing"))?;
            let columns = dataset
                .get("columns")
                .and_then(Value::as_array)
                .ok_or_else(|| PgBuilderError::new("pack.schema", "a role dataset has no columns"))?;
            for column in columns {
                let column = column
                    .as_str()
                    .ok_or_else(|| PgBuilderError::new("pack.schema", "a role column is not a name"))?;
                if !covered.contains(&(name.to_owned(), column.to_owned())) {
                    return Err(PgBuilderError::new(
                        "classification.missing",
                        "a role column has no classification decision",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn fingerprint(draft: &Value, dataset: &str, column: &str) -> Result<String, PgBuilderError> {
    let item_id = format!("column/{dataset}/{column}");
    let queue = draft
        .get("review_queue")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("classification.fingerprint", "the draft has no review queue"))?;
    for item in queue {
        if item.get("item_id").and_then(Value::as_str) == Some(item_id.as_str())
            && item.get("kind").and_then(Value::as_str) == Some("structural_column")
        {
            return item
                .get("fact_fingerprint")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| PgBuilderError::new("classification.fingerprint", "a column has no fact fingerprint"));
        }
    }
    Err(PgBuilderError::new(
        "classification.fingerprint",
        "a classified column is not in the draft queue",
    ))
}

fn outcome_candidate<'a>(outcome: &'a Value, dataset: &str) -> Result<&'a Value, PgBuilderError> {
    outcome
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| rows.iter().find(|row| row.get("dataset").and_then(Value::as_str) == Some(dataset)))
        .ok_or_else(|| PgBuilderError::new("review.anchor", "a decision dataset is outside the candidate set"))
}

fn command_artifact(generation: &Value) -> Result<Value, PgBuilderError> {
    let text = generation
        .get("complete_command")
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("review.blocked", "the generation has no complete command"))?;
    let command: Value = serde_json::from_str(text).map_err(|_| {
        PgBuilderError::new("compiler.complete", "the complete command is not JSON")
    })?;
    command
        .get("artifact")
        .cloned()
        .ok_or_else(|| PgBuilderError::new("compiler.seal", "the sealed draft is missing"))
}

fn require_validated(generation: &Value) -> Result<(), PgBuilderError> {
    if generation.get("outcome").and_then(Value::as_str) == Some("validated") {
        Ok(())
    } else {
        Err(PgBuilderError::new("review.blocked", "review requires a validated generation"))
    }
}

fn require_declared<'a>(value: &'a Value, code: &'static str) -> Result<&'a Map<String, Value>, PgBuilderError> {
    let map = object(value, code)?;
    if map.get("authenticated") != Some(&Value::Bool(false))
        || map.get("parent_auth").and_then(Value::as_str) != Some("required")
    {
        return Err(PgBuilderError::new(
            "approval.authentication",
            "local operator review is not authenticated RBAC",
        ));
    }
    Ok(map)
}

fn bridge_document(protocol: &str, bytes: &str, command: String, review: Value) -> Value {
    json!({
        "protocol": protocol,
        "authenticated": false,
        "parent_auth": "required",
        "outcome": review.get("outcome").cloned().unwrap_or(Value::Null),
        "review_sha256": sha256_hex(bytes.as_bytes()),
        "review_command": command,
        "review": review,
        "field_publication_seam": field_seam(),
        "statements": {
            "field_candidates_applied": false,
            "publication": "unavailable",
            "authenticated": false,
            "parent_auth": "required"
        }
    })
}

fn pack_hashes(packed: &Value) -> Value {
    let Some(packs) = packed.get("packs").and_then(Value::as_array) else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        packs
            .iter()
            .map(|pack| {
                json!({
                    "role": pack.get("role").cloned().unwrap_or(Value::Null),
                    "artifact_sha256": pack.get("artifact_sha256").cloned().unwrap_or(Value::Null)
                })
            })
            .collect(),
    )
}

fn column_text_applied(generation: &Value, before: &str, after: &str) -> bool {
    let Some(fields) = generation.pointer("/candidates/field").and_then(Value::as_array) else {
        return false;
    };
    fields.iter().any(|field| {
        let Some(description) = field.get("description").and_then(Value::as_str) else {
            return false;
        };
        description.chars().count() >= 12 && after.contains(description) && !before.contains(description)
    })
}

fn field_text_absent(generation: &Value, before: &str, after: &str) -> bool {
    let Some(fields) = generation.pointer("/candidates/field").and_then(Value::as_array) else {
        return true;
    };
    for field in fields {
        let Some(description) = field.get("description").and_then(Value::as_str) else {
            continue;
        };
        if description.chars().count() >= 12 && (before.contains(description) || after.contains(description)) {
            return false;
        }
    }
    true
}

fn hidden_report(before: &str, after: &str, names: &[&str]) -> Value {
    let mut rows = Vec::new();
    for name in names {
        rows.push(json!({
            "column": name,
            "absent": !before.contains(name) && !after.contains(name)
        }));
    }
    Value::Array(rows)
}

fn parse_compiled(text: &str) -> Result<Value, PgBuilderError> {
    serde_json::from_str(text).map_err(|_| PgBuilderError::new("compiler.seal", "the captured catalog was refused"))
}

fn text_field(value: &Value, key: &str) -> Result<String, PgBuilderError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "a catalog field is missing"))
}

fn required_text(map: &Map<String, Value>, key: &str, code: &'static str) -> Result<String, PgBuilderError> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| PgBuilderError::new(code, "a required field is missing"))
}

fn require_exact(map: &Map<String, Value>, keys: &[&str], code: &'static str) -> Result<(), PgBuilderError> {
    if map.len() == keys.len() && keys.iter().all(|key| map.contains_key(*key)) {
        Ok(())
    } else {
        Err(PgBuilderError::new(code, "the object has an unknown or missing field"))
    }
}

fn forward_review(err: description_review::DescriptionReviewError) -> PgBuilderError {
    PgBuilderError::forwarded(err.code, err.message)
}

fn forward_pack(err: description_pack::DescriptionPackError) -> PgBuilderError {
    PgBuilderError::forwarded(err.code, err.message)
}
