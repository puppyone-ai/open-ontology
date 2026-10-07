//! Re-execute a description review and compile the accepted text into packs.
//!
//! The captured-draft compiler is the only sealer. This module does not hash
//! an envelope of its own. A probe compile asks that compiler for the dataset
//! fact fingerprint, and a second compile applies an override review. An
//! occupied dataset review is replaced only when the caller sets
//! `replace_existing_dataset_review` and the canonical hash of that review
//! object. The compiler review object does not gain a reviewer.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Map, Value};

use description_review::apply_description_review;
use oo_builder::{
    CapturedPgDraft, compile_captured_pg_catalog, compile_reviewed_effective, compile_role_pack,
};
use oo_core::diagnostic::DiagnosticList;
use oo_core::json_scan::parse_json_value;
use oo_core::limits::DOCUMENT_MAX_BYTES;

use crate::canonical::{canonical_json, sha256_bytes};
use crate::error::{COMMAND_PROTOCOL, PACK_COMPILER, PACK_PROTOCOL, DescriptionPackError};

const CAPTURED_COMMAND: &str = "oo.builder.captured-pg-draft-command/v1";
const REVIEWED_COMMAND: &str = "oo.builder.reviewed-effective-oos-command/v1";
const MAX_ROLES: usize = 8;

/// Read one pack command and return the canonical bridge document.
///
/// The returned text has no trailing newline. A refusal returns `Err` and
/// no document. `unsupported` and `not_candidates` are documents: they do
/// not carry a new seal.
pub fn apply_description_pack(input: &str) -> Result<String, DescriptionPackError> {
    let value = parse_json_value(input, COMMAND_PROTOCOL).map_err(map_scan)?;
    let command: CommandIn = serde_json::from_value(value).map_err(map_schema)?;
    if command.protocol != COMMAND_PROTOCOL {
        return Err(DescriptionPackError::named(
            "protocol.mismatch",
            "the command protocol is not the description pack command",
        ));
    }
    require_sha(&command.expected_review_sha256)?;
    require_object(&command.artifact, "artifact is not an object")?;
    require_object(&command.settings, "settings is not an object")?;
    if !command.decisions.is_array() {
        return Err(DescriptionPackError::named(
            "schema.invalid",
            "decisions is not an array",
        ));
    }
    if !command.roles.is_array() {
        return Err(DescriptionPackError::named(
            "schema.invalid",
            "roles is not an array",
        ));
    }
    let roles = command.roles.as_array().expect("roles array");
    if roles.is_empty() || roles.len() > MAX_ROLES {
        return Err(DescriptionPackError::named(
            "schema.invalid",
            "roles must contain one to eight visibility objects",
        ));
    }
    if !command.replace_existing_dataset_review && command.expected_existing_review_sha256.is_some()
    {
        return Err(DescriptionPackError::named(
            "schema.invalid",
            "a replacement hash requires authorization",
        ));
    }
    if !command.replace_existing_column_review
        && command.expected_existing_column_review_sha256.is_some()
    {
        return Err(DescriptionPackError::named(
            "schema.invalid",
            "a replacement hash requires authorization",
        ));
    }
    if let Some(map) = &command.expected_existing_review_sha256 {
        for hash in map.values() {
            require_sha(hash)?;
        }
    }
    if let Some(map) = &command.expected_existing_column_review_sha256 {
        for hash in map.values() {
            require_sha(hash)?;
        }
    }
    let review_bytes = apply_description_review(&command.review)?;
    let review_sha = sha256_bytes(review_bytes.as_bytes());
    if review_sha != command.expected_review_sha256 {
        return Err(DescriptionPackError::named(
            "review.hash",
            "the description review hash does not match",
        ));
    }
    let review: Value = serde_json::from_str(&review_bytes).map_err(|_| {
        DescriptionPackError::named(
            "review.shape",
            "the description review document is not an object",
        )
    })?;
    let outcome_name = text_field(&review, "outcome").ok_or_else(|| {
        DescriptionPackError::named(
            "review.shape",
            "the description review document is not an object",
        )
    })?;
    if outcome_name != "reviewed" {
        return finish(not_candidates_document(&review, &review_sha));
    }
    reject_stale(&review)?;
    let supplied = &command.artifact;
    let embedded = embedded_artifact(&command.review)?;
    if &embedded != supplied {
        return Err(DescriptionPackError::named(
            "seal.artifact",
            "the sealed captured draft does not match the review command",
        ));
    }
    let original = recompile_original(supplied)?;
    let approvals = match collect_approvals(&review) {
        Ok(items) => items,
        Err(Stop::Unsupported(seam)) => return unsupported_outcome(seam, &review_sha),
        Err(Stop::Refuse(err)) => return Err(err),
    };
    let column_approvals = match collect_column_approvals(&review) {
        Ok(items) => items,
        Err(Stop::Unsupported(seam)) => return unsupported_outcome(seam, &review_sha),
        Err(Stop::Refuse(err)) => return Err(err),
    };
    if approvals.is_empty()
        && command
            .expected_existing_review_sha256
            .as_ref()
            .is_some_and(|map| !map.is_empty())
    {
        return Err(occupied_cas());
    }
    if column_approvals.is_empty()
        && command
            .expected_existing_column_review_sha256
            .as_ref()
            .is_some_and(|map| !map.is_empty())
    {
        return Err(occupied_cas());
    }
    let sealed = if approvals.is_empty() && column_approvals.is_empty() {
        None
    } else {
        match reseal(
            supplied,
            &original,
            &approvals,
            &column_approvals,
            ReplaceAuth {
                enabled: command.replace_existing_dataset_review,
                expected: command.expected_existing_review_sha256.as_ref(),
            },
            ReplaceAuth {
                enabled: command.replace_existing_column_review,
                expected: command.expected_existing_column_review_sha256.as_ref(),
            },
        ) {
            Ok(item) => Some(item),
            Err(Stop::Unsupported(seam)) => return unsupported_outcome(seam, &review_sha),
            Err(Stop::Refuse(err)) => return Err(err),
        }
    };
    let artifact_text = match &sealed {
        Some(item) => item.text.clone(),
        None => command_text_of(supplied)?,
    };
    let downstream =
        compile_downstream(&artifact_text, &command.settings, &command.decisions, roles)?;
    finish(packed_document(
        &review,
        &review_sha,
        &original,
        sealed.as_ref(),
        &artifact_text,
        &downstream,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandIn {
    protocol: String,
    review: String,
    expected_review_sha256: String,
    artifact: Value,
    settings: Value,
    decisions: Value,
    roles: Value,
    #[serde(default)]
    replace_existing_dataset_review: bool,
    #[serde(default)]
    expected_existing_review_sha256: Option<BTreeMap<String, String>>,
    #[serde(default)]
    replace_existing_column_review: bool,
    #[serde(default)]
    expected_existing_column_review_sha256: Option<BTreeMap<String, String>>,
}

struct ReplaceAuth<'a> {
    enabled: bool,
    expected: Option<&'a BTreeMap<String, String>>,
}

struct OriginalSeal {
    catalog: String,
    curated: Option<String>,
    reviews: Option<String>,
    wrapper_hash: String,
    draft_hash: String,
    schema_hash: String,
}

struct Reseal {
    text: String,
    artifact: Value,
    wrapper_hash: String,
    draft_hash: String,
    evidence_envelope_hash: String,
    replacements: Vec<Replacement>,
}

struct Replacement {
    item_id: String,
    prior_review: Value,
    prior_sha: String,
    compiler_sha: String,
    reviewer_label: String,
    reviewer_at: String,
    authenticated: bool,
    fields: Vec<String>,
}

struct Approval {
    dataset: String,
    label: Option<String>,
    description: Option<String>,
    grain: Option<String>,
    reviewer_label: String,
    reviewer_at: String,
    authenticated: bool,
}

struct ColumnApproval {
    dataset: String,
    ident: String,
    label: Option<String>,
    description: Option<String>,
    reviewer_label: String,
    reviewer_at: String,
    authenticated: bool,
}

struct Downstream {
    effective_text: String,
    effective: Value,
    packs: Vec<Value>,
}

enum Stop {
    Refuse(DescriptionPackError),
    Unsupported(&'static str),
}

fn not_candidates_document(review: &Value, review_sha: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("compiler".to_owned(), Value::String(PACK_COMPILER.to_owned()));
    map.insert(
        "compiler_budget_reason".to_owned(),
        review.get("compiler_budget_reason").cloned().unwrap_or(Value::Null),
    );
    map.insert(
        "compiler_outcome".to_owned(),
        json_string(text_field(review, "compiler_outcome").unwrap_or("")),
    );
    map.insert(
        "compiler_refusal".to_owned(),
        review.get("compiler_refusal").cloned().unwrap_or(Value::Null),
    );
    map.insert("outcome".to_owned(), Value::String("not_candidates".to_owned()));
    map.insert("protocol".to_owned(), Value::String(PACK_PROTOCOL.to_owned()));
    map.insert("queue".to_owned(), queue_projection(review));
    map.insert(
        "review_sha256".to_owned(),
        Value::String(review_sha.to_owned()),
    );
    map.insert("statements".to_owned(), statements("not_required"));
    map
}

fn packed_document(
    review: &Value,
    review_sha: &str,
    original: &OriginalSeal,
    reseal: Option<&Reseal>,
    artifact_text: &str,
    downstream: &Downstream,
) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        "artifact_sha256".to_owned(),
        Value::String(sha256_bytes(artifact_text.as_bytes())),
    );
    map.insert("compiler".to_owned(), Value::String(PACK_COMPILER.to_owned()));
    map.insert(
        "curated_documents".to_owned(),
        review.get("datasets").cloned().unwrap_or(Value::Array(Vec::new())),
    );
    map.insert("effective".to_owned(), downstream.effective.clone());
    map.insert(
        "effective_sha256".to_owned(),
        Value::String(sha256_bytes(downstream.effective_text.as_bytes())),
    );
    map.insert(
        "original_draft_hash".to_owned(),
        Value::String(original.draft_hash.clone()),
    );
    map.insert(
        "original_wrapper_hash".to_owned(),
        Value::String(original.wrapper_hash.clone()),
    );
    map.insert("outcome".to_owned(), Value::String(outcome_of(reseal).to_owned()));
    map.insert("packs".to_owned(), Value::Array(downstream.packs.clone()));
    map.insert("protocol".to_owned(), Value::String(PACK_PROTOCOL.to_owned()));
    map.insert(
        "resealed_artifact".to_owned(),
        reseal.map(|item| item.artifact.clone()).unwrap_or(Value::Null),
    );
    map.insert(
        "resealed_draft_hash".to_owned(),
        reseal
            .map(|item| Value::String(item.draft_hash.clone()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "resealed_evidence_envelope_hash".to_owned(),
        reseal
            .map(|item| Value::String(item.evidence_envelope_hash.clone()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "resealed_wrapper_hash".to_owned(),
        reseal
            .map(|item| Value::String(item.wrapper_hash.clone()))
            .unwrap_or(Value::Null),
    );
    map.insert(
        "review_sha256".to_owned(),
        Value::String(review_sha.to_owned()),
    );
    if let Some(item) = reseal.filter(|item| !item.replacements.is_empty()) {
        map.insert(
            "occupied_replacements".to_owned(),
            Value::Array(item.replacements.iter().map(replacement_value).collect()),
        );
    }
    map.insert(
        "statements".to_owned(),
        statements(if reseal.is_some() {
            "compiler"
        } else {
            "not_required"
        }),
    );
    map
}

fn replacement_value(item: &Replacement) -> Value {
    serde_json::json!({
        "attribution": "bridge_provenance",
        "compiler_review_sha256": item.compiler_sha,
        "descriptive_provenance": {
            "authenticated": item.authenticated,
            "fields": item.fields,
            "reviewer_at": item.reviewer_at,
            "reviewer_label": item.reviewer_label,
        },
        "item_id": item.item_id,
        "prior_review": item.prior_review,
        "prior_review_sha256": item.prior_sha,
    })
}

fn outcome_of(reseal: Option<&Reseal>) -> &'static str {
    if reseal.is_some() {
        "packed"
    } else {
        "reviewed_unchanged"
    }
}

fn statements(reseal: &str) -> Value {
    serde_json::json!({
        "authentication": "not_claimed",
        "authorization": "not_claimed",
        "database": "not_used",
        "grants": "not_authorized",
        "llm": "not_used",
        "publication": "unavailable",
        "reseal": reseal,
        "semantic_publication": "not_claimed",
        "source_artifact": "not_mutated",
    })
}

fn reject_stale(review: &Value) -> Result<(), DescriptionPackError> {
    let Some(items) = review.get("queue").and_then(Value::as_array) else {
        return Err(DescriptionPackError::named(
            "review.shape",
            "the description review document is not an object",
        ));
    };
    for item in items {
        if item.get("kind").and_then(Value::as_str) != Some("stale") {
            continue;
        }
        let reason = item.get("reason").and_then(Value::as_str).unwrap_or("");
        return Err(stale_error(reason));
    }
    Ok(())
}

fn stale_error(reason: &str) -> DescriptionPackError {
    let (name, message) = match reason {
        "schema" => (
            "review.schema",
            "schema hash mismatch leaves the curated documents unchanged",
        ),
        "evidence" => (
            "review.evidence",
            "evidence hash mismatch leaves the curated documents unchanged",
        ),
        "cas" => (
            "review.cas",
            "CAS mismatch leaves the curated field unchanged",
        ),
        _ => (
            "review.anchor",
            "a review anchor does not match the recomputed candidate",
        ),
    };
    DescriptionPackError::named(name, message)
}

fn embedded_artifact(review_command: &str) -> Result<Value, DescriptionPackError> {
    let review: Value = serde_json::from_str(review_command).map_err(|_| {
        DescriptionPackError::named(
            "seal.artifact",
            "the sealed captured draft does not match the review command",
        )
    })?;
    let command = review
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DescriptionPackError::named(
                "seal.artifact",
                "the sealed captured draft does not match the review command",
            )
        })?;
    let candidate: Value = serde_json::from_str(command).map_err(|_| {
        DescriptionPackError::named(
            "seal.artifact",
            "the sealed captured draft does not match the review command",
        )
    })?;
    candidate.get("artifact").cloned().ok_or_else(|| {
        DescriptionPackError::named(
            "seal.artifact",
            "the sealed captured draft does not match the review command",
        )
    })
}

fn recompile_original(artifact: &Value) -> Result<OriginalSeal, DescriptionPackError> {
    let envelope = artifact.get("evidence_envelope").ok_or_else(seal_rejected)?;
    let catalog = envelope.get("catalog").ok_or_else(seal_rejected)?;
    let curated = optional_json(envelope.get("curated"))?;
    let reviews = optional_json(envelope.get("reviews"))?;
    let compiled = compile_captured_pg_catalog(
        &canonical_json(catalog),
        curated.as_deref(),
        reviews.as_deref(),
    )
    .map_err(|_| seal_rejected())?;
    let rebuilt = command_document(&compiled);
    let rebuilt_value: Value = serde_json::from_str(&rebuilt).map_err(|_| seal_rejected())?;
    if &rebuilt_value != artifact {
        return Err(seal_rejected());
    }
    Ok(OriginalSeal {
        catalog: canonical_json(catalog),
        curated,
        reviews,
        wrapper_hash: compiled.wrapper_hash,
        draft_hash: compiled.draft_hash,
        schema_hash: compiled.schema_hash,
    })
}

fn reseal(
    artifact: &Value,
    original: &OriginalSeal,
    approvals: &[Approval],
    column_approvals: &[ColumnApproval],
    auth: ReplaceAuth<'_>,
    column_auth: ReplaceAuth<'_>,
) -> Result<Reseal, Stop> {
    let mut overlay = original
        .curated
        .as_deref()
        .map(|text| serde_json::from_str(text).unwrap_or(Value::Null))
        .unwrap_or_else(|| serde_json::json!({"datasets": [], "relationships": []}));
    if overlay.is_null() {
        overlay = serde_json::json!({"datasets": [], "relationships": []});
    }
    write_approvals(&mut overlay, approvals).map_err(Stop::Unsupported)?;
    write_column_approvals(&mut overlay, column_approvals).map_err(Stop::Unsupported)?;
    let overlay_text = canonical_json(&overlay);
    let reviews = review_list(artifact)?;
    let mut occupied: Vec<(&Approval, &Value)> = Vec::new();
    let mut fresh: Vec<&Approval> = Vec::new();
    for approval in approvals {
        let item_id = format!("dataset/{}", approval.dataset);
        match find_review(&reviews, &item_id)? {
            Some(existing) => occupied.push((approval, existing)),
            None => fresh.push(approval),
        }
    }
    let mut column_occupied: Vec<(&ColumnApproval, &Value)> = Vec::new();
    let mut column_fresh: Vec<&ColumnApproval> = Vec::new();
    for approval in column_approvals {
        let item_id = column_item_id(&approval.dataset, &approval.ident);
        match find_review(&reviews, &item_id)? {
            Some(existing) => column_occupied.push((approval, existing)),
            None => column_fresh.push(approval),
        }
    }
    if !occupied.is_empty() && !auth.enabled {
        return Err(Stop::Unsupported("review.dataset_decision_occupied"));
    }
    if !column_occupied.is_empty() && !column_auth.enabled {
        return Err(Stop::Unsupported("review.column_decision_occupied"));
    }
    if !occupied.is_empty() {
        authorize_occupied(&occupied, auth)?;
    } else if auth.expected.is_some_and(|map| !map.is_empty()) {
        return Err(Stop::Refuse(occupied_cas()));
    }
    if !column_occupied.is_empty() {
        authorize_columns(&column_occupied, column_auth)?;
    } else if column_auth.expected.is_some_and(|map| !map.is_empty()) {
        return Err(Stop::Refuse(occupied_cas()));
    }
    let probe = compile_captured_pg_catalog(
        &original.catalog,
        Some(&overlay_text),
        original.reviews.as_deref(),
    )
    .map_err(|err| Stop::Unsupported(seam_code(&err.code)))?;
    if occupied_is_stale(&probe, &occupied) || column_occupied_is_stale(&probe, &column_occupied) {
        return Err(Stop::Refuse(occupied_stale()));
    }
    if occupied.is_empty()
        && column_occupied.is_empty()
        && fields_applied(&probe, approvals)
        && column_fields_applied(&probe, column_approvals)
    {
        return reseal_from(probe, Vec::new());
    }
    if fresh.is_empty() && column_fresh.is_empty() {
        if !fields_applied(&probe, approvals) || !column_fields_applied(&probe, column_approvals) {
            return Err(Stop::Unsupported("effective.field_not_applied"));
        }
        if !reviews_match(&probe, original.reviews.as_deref()) {
            return Err(Stop::Unsupported("review.entry_not_preserved"));
        }
        let mut replacements = replacement_rows(&probe, &occupied)?;
        replacements.extend(column_replacement_rows(&probe, &column_occupied)?);
        replacements.sort_by(|left, right| left.item_id.cmp(&right.item_id));
        return reseal_from(probe, replacements);
    }
    let mut next = reviews.clone();
    for approval in &fresh {
        let item_id = format!("dataset/{}", approval.dataset);
        let fingerprint = dataset_fingerprint(&probe, &item_id)
            .ok_or(Stop::Unsupported("review.dataset_fingerprint_absent"))?;
        next.push(serde_json::json!({
            "action": "override",
            "fact_fingerprint": fingerprint,
            "item_id": item_id,
            "schema_hash": original.schema_hash,
        }));
    }
    for approval in &column_fresh {
        let item_id = column_item_id(&approval.dataset, &approval.ident);
        let fingerprint = dataset_fingerprint(&probe, &item_id)
            .ok_or(Stop::Unsupported("review.column_fingerprint_absent"))?;
        next.push(serde_json::json!({
            "action": "override",
            "fact_fingerprint": fingerprint,
            "item_id": item_id,
            "schema_hash": original.schema_hash,
        }));
    }
    let reviews_text = canonical_json(&Value::Array(next));
    let compiled = compile_captured_pg_catalog(
        &original.catalog,
        Some(&overlay_text),
        Some(&reviews_text),
    )
    .map_err(|err| Stop::Unsupported(seam_code(&err.code)))?;
    if occupied_is_stale(&compiled, &occupied) || column_occupied_is_stale(&compiled, &column_occupied)
    {
        return Err(Stop::Refuse(occupied_stale()));
    }
    if !fields_applied(&compiled, approvals) || !column_fields_applied(&compiled, column_approvals) {
        return Err(Stop::Unsupported("effective.field_not_applied"));
    }
    if !original_reviews_kept(&compiled, &reviews) {
        return Err(Stop::Unsupported("review.entry_not_preserved"));
    }
    let mut replacements = replacement_rows(&compiled, &occupied)?;
    replacements.extend(column_replacement_rows(&compiled, &column_occupied)?);
    replacements.sort_by(|left, right| left.item_id.cmp(&right.item_id));
    reseal_from(compiled, replacements)
}

fn column_item_id(dataset: &str, ident: &str) -> String {
    format!("column/{dataset}/{ident}")
}

fn authorize_columns(
    occupied: &[(&ColumnApproval, &Value)],
    auth: ReplaceAuth<'_>,
) -> Result<(), Stop> {
    let Some(expected) = auth.expected else {
        return Err(Stop::Refuse(occupied_cas()));
    };
    let needed: BTreeSet<String> = occupied
        .iter()
        .map(|(approval, _)| column_item_id(&approval.dataset, &approval.ident))
        .collect();
    let supplied: BTreeSet<String> = expected.keys().cloned().collect();
    if supplied != needed {
        return Err(Stop::Refuse(occupied_cas()));
    }
    for (approval, review) in occupied {
        let item_id = column_item_id(&approval.dataset, &approval.ident);
        let Some(supplied_hash) = expected.get(&item_id) else {
            return Err(Stop::Refuse(occupied_cas()));
        };
        if supplied_hash != &review_sha(review) {
            return Err(Stop::Refuse(occupied_cas()));
        }
        if review.get("action").and_then(Value::as_str) != Some("override") {
            return Err(Stop::Unsupported("review.column_action_unsupported"));
        }
    }
    Ok(())
}

fn column_occupied_is_stale(
    compiled: &CapturedPgDraft,
    occupied: &[(&ColumnApproval, &Value)],
) -> bool {
    occupied.iter().any(|(_, review)| {
        let item_id = review.get("item_id").and_then(Value::as_str).unwrap_or("");
        has_stale(compiled, item_id)
    })
}

fn column_replacement_rows(
    compiled: &CapturedPgDraft,
    occupied: &[(&ColumnApproval, &Value)],
) -> Result<Vec<Replacement>, Stop> {
    let envelope: Value = serde_json::from_str(&compiled.evidence_envelope).map_err(|_| {
        Stop::Refuse(DescriptionPackError::named(
            "seal.rejected",
            "the resealed draft is not an object",
        ))
    })?;
    let Some(reviews) = envelope.get("reviews").and_then(Value::as_array) else {
        return Err(Stop::Unsupported("review.entry_not_preserved"));
    };
    let mut rows = Vec::with_capacity(occupied.len());
    for (approval, prior) in occupied {
        let item_id = column_item_id(&approval.dataset, &approval.ident);
        let Some(current) = reviews.iter().find(|item| {
            item.get("item_id").and_then(Value::as_str) == Some(item_id.as_str())
        }) else {
            return Err(Stop::Unsupported("review.entry_not_preserved"));
        };
        let prior_sha = review_sha(prior);
        let compiler_sha = review_sha(current);
        if prior_sha != compiler_sha {
            return Err(Stop::Unsupported("review.entry_not_preserved"));
        }
        rows.push(Replacement {
            item_id,
            prior_review: (*prior).clone(),
            prior_sha,
            compiler_sha,
            reviewer_label: approval.reviewer_label.clone(),
            reviewer_at: approval.reviewer_at.clone(),
            authenticated: approval.authenticated,
            fields: column_approval_fields(approval),
        });
    }
    Ok(rows)
}

fn column_approval_fields(approval: &ColumnApproval) -> Vec<String> {
    let mut fields = Vec::new();
    if approval.description.is_some() {
        fields.push("description".to_owned());
    }
    if approval.label.is_some() {
        fields.push("label".to_owned());
    }
    fields
}

fn authorize_occupied(
    occupied: &[(&Approval, &Value)],
    auth: ReplaceAuth<'_>,
) -> Result<(), Stop> {
    let Some(expected) = auth.expected else {
        return Err(Stop::Refuse(occupied_cas()));
    };
    let needed: BTreeSet<String> = occupied
        .iter()
        .map(|(approval, _)| format!("dataset/{}", approval.dataset))
        .collect();
    let supplied: BTreeSet<String> = expected.keys().cloned().collect();
    if supplied != needed {
        return Err(Stop::Refuse(occupied_cas()));
    }
    for (approval, review) in occupied {
        let item_id = format!("dataset/{}", approval.dataset);
        let Some(supplied_hash) = expected.get(&item_id) else {
            return Err(Stop::Refuse(occupied_cas()));
        };
        if supplied_hash != &review_sha(review) {
            return Err(Stop::Refuse(occupied_cas()));
        }
        if review.get("action").and_then(Value::as_str) != Some("override") {
            return Err(Stop::Unsupported("review.dataset_action_unsupported"));
        }
    }
    Ok(())
}

fn occupied_is_stale(compiled: &CapturedPgDraft, occupied: &[(&Approval, &Value)]) -> bool {
    occupied.iter().any(|(_, review)| {
        let item_id = review.get("item_id").and_then(Value::as_str).unwrap_or("");
        has_stale(compiled, item_id)
    })
}

fn has_stale(compiled: &CapturedPgDraft, item_id: &str) -> bool {
    let Ok(draft) = serde_json::from_str::<Value>(&compiled.draft) else {
        return true;
    };
    let Some(items) = draft.get("review_queue").and_then(Value::as_array) else {
        return true;
    };
    items.iter().any(|item| {
        item.get("item_id").and_then(Value::as_str) == Some(item_id)
            && item.get("kind").and_then(Value::as_str) == Some("stale_decision")
    })
}

fn reviews_match(compiled: &CapturedPgDraft, original: Option<&str>) -> bool {
    let Some(original) = original else {
        return false;
    };
    let Ok(envelope) = serde_json::from_str::<Value>(&compiled.evidence_envelope) else {
        return false;
    };
    let Some(reviews) = envelope.get("reviews") else {
        return false;
    };
    canonical_json(reviews) == original
}

fn original_reviews_kept(compiled: &CapturedPgDraft, original: &[Value]) -> bool {
    let Ok(envelope) = serde_json::from_str::<Value>(&compiled.evidence_envelope) else {
        return false;
    };
    let Some(reviews) = envelope.get("reviews").and_then(Value::as_array) else {
        return false;
    };
    original.iter().all(|prior| {
        let prior_text = canonical_json(prior);
        reviews
            .iter()
            .any(|item| canonical_json(item) == prior_text)
    })
}

fn replacement_rows(
    compiled: &CapturedPgDraft,
    occupied: &[(&Approval, &Value)],
) -> Result<Vec<Replacement>, Stop> {
    let envelope: Value = serde_json::from_str(&compiled.evidence_envelope).map_err(|_| {
        Stop::Refuse(DescriptionPackError::named(
            "seal.rejected",
            "the resealed draft is not an object",
        ))
    })?;
    let Some(reviews) = envelope.get("reviews").and_then(Value::as_array) else {
        return Err(Stop::Unsupported("review.entry_not_preserved"));
    };
    let mut rows = Vec::with_capacity(occupied.len());
    for (approval, prior) in occupied {
        let item_id = format!("dataset/{}", approval.dataset);
        let Some(current) = reviews.iter().find(|item| {
            item.get("item_id").and_then(Value::as_str) == Some(item_id.as_str())
        }) else {
            return Err(Stop::Unsupported("review.entry_not_preserved"));
        };
        let prior_sha = review_sha(prior);
        let compiler_sha = review_sha(current);
        if prior_sha != compiler_sha {
            return Err(Stop::Unsupported("review.entry_not_preserved"));
        }
        rows.push(Replacement {
            item_id,
            prior_review: (*prior).clone(),
            prior_sha,
            compiler_sha,
            reviewer_label: approval.reviewer_label.clone(),
            reviewer_at: approval.reviewer_at.clone(),
            authenticated: approval.authenticated,
            fields: approval_fields(approval),
        });
    }
    rows.sort_by(|left, right| left.item_id.cmp(&right.item_id));
    Ok(rows)
}

fn approval_fields(approval: &Approval) -> Vec<String> {
    let mut fields = Vec::new();
    if approval.description.is_some() {
        fields.push("description".to_owned());
    }
    if approval.grain.is_some() {
        fields.push("grain".to_owned());
    }
    if approval.label.is_some() {
        fields.push("label".to_owned());
    }
    fields
}

fn review_sha(review: &Value) -> String {
    sha256_bytes(canonical_json(review).as_bytes())
}

fn find_review<'a>(reviews: &'a [Value], item_id: &str) -> Result<Option<&'a Value>, Stop> {
    let mut found = None;
    for item in reviews {
        if item.get("item_id").and_then(Value::as_str) == Some(item_id) {
            if found.is_some() {
                return Err(Stop::Unsupported("curated.overlay_rejected"));
            }
            found = Some(item);
        }
    }
    Ok(found)
}

fn occupied_cas() -> DescriptionPackError {
    DescriptionPackError::named(
        "review.occupied_cas",
        "the occupied review hash does not match",
    )
}

fn occupied_stale() -> DescriptionPackError {
    DescriptionPackError::named("review.occupied_stale", "the occupied review is stale")
}

fn reseal_from(compiled: CapturedPgDraft, replacements: Vec<Replacement>) -> Result<Reseal, Stop> {
    let text = command_document(&compiled);
    let artifact: Value = serde_json::from_str(&text).map_err(|_| {
        Stop::Refuse(DescriptionPackError::named(
            "seal.rejected",
            "the resealed draft is not an object",
        ))
    })?;
    Ok(Reseal {
        evidence_envelope_hash: compiled.evidence_envelope_hash,
        wrapper_hash: compiled.wrapper_hash,
        draft_hash: compiled.draft_hash,
        text,
        artifact,
        replacements,
    })
}

fn write_approvals(overlay: &mut Value, approvals: &[Approval]) -> Result<(), &'static str> {
    let Some(object) = overlay.as_object_mut() else {
        return Err("curated.overlay_rejected");
    };
    if !object.contains_key("datasets") {
        object.insert("datasets".to_owned(), Value::Array(Vec::new()));
    }
    if !object.contains_key("relationships") {
        object.insert("relationships".to_owned(), Value::Array(Vec::new()));
    }
    let Some(datasets) = object.get_mut("datasets").and_then(Value::as_array_mut) else {
        return Err("curated.overlay_rejected");
    };
    for approval in approvals {
        let mut index = datasets.iter().position(|item| {
            item.get("dataset").and_then(Value::as_str) == Some(approval.dataset.as_str())
        });
        if index.is_none() {
            datasets.push(serde_json::json!({"dataset": approval.dataset}));
            index = Some(datasets.len().saturating_sub(1));
        }
        let Some(index) = index else {
            return Err("curated.overlay_rejected");
        };
        let Some(entry) = datasets.get_mut(index).and_then(Value::as_object_mut) else {
            return Err("curated.overlay_rejected");
        };
        if let Some(label) = &approval.label {
            entry.insert("label".to_owned(), Value::String(label.clone()));
        }
        if let Some(description) = &approval.description {
            entry.insert("description".to_owned(), Value::String(description.clone()));
        }
        if let Some(grain) = &approval.grain {
            entry.insert("grain".to_owned(), Value::String(grain.clone()));
        }
    }
    Ok(())
}

fn write_column_approvals(overlay: &mut Value, approvals: &[ColumnApproval]) -> Result<(), &'static str> {
    if approvals.is_empty() {
        return Ok(());
    }
    let Some(object) = overlay.as_object_mut() else {
        return Err("curated.overlay_rejected");
    };
    if !object.contains_key("datasets") {
        object.insert("datasets".to_owned(), Value::Array(Vec::new()));
    }
    if !object.contains_key("relationships") {
        object.insert("relationships".to_owned(), Value::Array(Vec::new()));
    }
    let Some(datasets) = object.get_mut("datasets").and_then(Value::as_array_mut) else {
        return Err("curated.overlay_rejected");
    };
    for approval in approvals {
        let mut index = datasets.iter().position(|item| {
            item.get("dataset").and_then(Value::as_str) == Some(approval.dataset.as_str())
        });
        if index.is_none() {
            datasets.push(serde_json::json!({
                "columns": [],
                "dataset": approval.dataset,
            }));
            index = Some(datasets.len().saturating_sub(1));
        }
        let Some(index) = index else {
            return Err("curated.overlay_rejected");
        };
        let Some(entry) = datasets.get_mut(index).and_then(Value::as_object_mut) else {
            return Err("curated.overlay_rejected");
        };
        if !entry.contains_key("columns") {
            entry.insert("columns".to_owned(), Value::Array(Vec::new()));
        }
        let Some(columns) = entry.get_mut("columns").and_then(Value::as_array_mut) else {
            return Err("curated.overlay_rejected");
        };
        let mut column_index = columns.iter().position(|item| {
            item.get("name").and_then(Value::as_str) == Some(approval.ident.as_str())
        });
        if column_index.is_none() {
            columns.push(serde_json::json!({"name": approval.ident}));
            column_index = Some(columns.len().saturating_sub(1));
        }
        let Some(column_index) = column_index else {
            return Err("curated.overlay_rejected");
        };
        let Some(column) = columns.get_mut(column_index).and_then(Value::as_object_mut) else {
            return Err("curated.overlay_rejected");
        };
        if let Some(label) = &approval.label {
            column.insert("label".to_owned(), Value::String(label.clone()));
        }
        if let Some(description) = &approval.description {
            column.insert("description".to_owned(), Value::String(description.clone()));
        }
    }
    Ok(())
}

fn fields_applied(compiled: &CapturedPgDraft, approvals: &[Approval]) -> bool {
    let Ok(draft) = serde_json::from_str::<Value>(&compiled.draft) else {
        return false;
    };
    let Some(datasets) = draft
        .get("effective")
        .and_then(|value| value.get("datasets"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    approvals.iter().all(|approval| {
        let Some(dataset) = datasets.iter().find(|item| {
            item.get("dataset").and_then(Value::as_str) == Some(approval.dataset.as_str())
        }) else {
            return false;
        };
        field_eq(dataset, "label", &approval.label)
            && field_eq(dataset, "description", &approval.description)
            && field_eq(dataset, "grain", &approval.grain)
    })
}

fn column_fields_applied(compiled: &CapturedPgDraft, approvals: &[ColumnApproval]) -> bool {
    if approvals.is_empty() {
        return true;
    }
    let Ok(draft) = serde_json::from_str::<Value>(&compiled.draft) else {
        return false;
    };
    let Some(datasets) = draft
        .get("effective")
        .and_then(|value| value.get("datasets"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    approvals.iter().all(|approval| {
        let Some(dataset) = datasets.iter().find(|item| {
            item.get("dataset").and_then(Value::as_str) == Some(approval.dataset.as_str())
        }) else {
            return false;
        };
        let Some(column) = dataset.get("columns").and_then(Value::as_array).and_then(|items| {
            items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some(approval.ident.as_str()))
        }) else {
            return false;
        };
        field_eq(column, "label", &approval.label)
            && field_eq(column, "description", &approval.description)
    })
}

fn field_eq(dataset: &Value, key: &str, expected: &Option<String>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    dataset.get(key).and_then(Value::as_str) == Some(expected.as_str())
}

fn dataset_fingerprint(compiled: &CapturedPgDraft, item_id: &str) -> Option<String> {
    let draft: Value = serde_json::from_str(&compiled.draft).ok()?;
    let items = draft.get("review_queue")?.as_array()?;
    let mut found: Option<String> = None;
    for item in items {
        if item.get("item_id").and_then(Value::as_str) != Some(item_id) {
            continue;
        }
        if item.get("detail").and_then(Value::as_str) != Some("curated_not_applied") {
            continue;
        }
        let Some(hash) = item.get("fact_fingerprint").and_then(Value::as_str) else {
            continue;
        };
        if found.as_deref().is_some_and(|prior| prior != hash) {
            return None;
        }
        found = Some(hash.to_owned());
    }
    found
}

fn review_list(artifact: &Value) -> Result<Vec<Value>, Stop> {
    let reviews = artifact
        .get("evidence_envelope")
        .and_then(|envelope| envelope.get("reviews"));
    match reviews {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(_) => Err(Stop::Unsupported("curated.overlay_rejected")),
    }
}

fn collect_approvals(review: &Value) -> Result<Vec<Approval>, Stop> {
    let decisions = review.get("decisions").and_then(Value::as_array).ok_or_else(|| {
        Stop::Refuse(DescriptionPackError::named(
            "review.shape",
            "the description review document is not an object",
        ))
    })?;
    let documents = review.get("datasets").and_then(Value::as_array).ok_or_else(|| {
        Stop::Refuse(DescriptionPackError::named(
            "review.shape",
            "the description review document is not an object",
        ))
    })?;
    let mut approvals = Vec::new();
    for decision in decisions {
        if decision.get("applied").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let dataset = decision
            .get("dataset")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Stop::Refuse(DescriptionPackError::named(
                    "review.shape",
                    "the description review document is not an object",
                ))
            })?;
        let document = documents
            .iter()
            .find(|item| item.get("dataset").and_then(Value::as_str) == Some(dataset))
            .and_then(|item| item.get("document"))
            .ok_or(Stop::Unsupported("curated.document_absent"))?;
        let label = taken_field(decision, document, "label", "label_disposition")?;
        let description = taken_field(decision, document, "description", "description_disposition")?;
        let grain = taken_field(decision, document, "grain", "grain_disposition")?;
        if label.is_none() && description.is_none() && grain.is_none() {
            continue;
        }
        let reviewer_label = decision
            .get("reviewer_label")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Stop::Refuse(DescriptionPackError::named(
                    "review.shape",
                    "the description review document is not an object",
                ))
            })?;
        let reviewer_at = decision.get("reviewer_at").and_then(Value::as_str).ok_or_else(|| {
            Stop::Refuse(DescriptionPackError::named(
                "review.shape",
                "the description review document is not an object",
            ))
        })?;
        let authenticated = decision
            .get("authenticated")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                Stop::Refuse(DescriptionPackError::named(
                    "review.shape",
                    "the description review document is not an object",
                ))
            })?;
        approvals.push(Approval {
            dataset: dataset.to_owned(),
            label,
            description,
            grain,
            reviewer_label: reviewer_label.to_owned(),
            reviewer_at: reviewer_at.to_owned(),
            authenticated,
        });
    }
    approvals.sort_by(|left, right| left.dataset.cmp(&right.dataset));
    Ok(approvals)
}

fn collect_column_approvals(review: &Value) -> Result<Vec<ColumnApproval>, Stop> {
    let Some(decisions) = review.get("column_decisions") else {
        return Ok(Vec::new());
    };
    let decisions = decisions.as_array().ok_or_else(|| {
        Stop::Refuse(DescriptionPackError::named(
            "review.shape",
            "the description review document is not an object",
        ))
    })?;
    let documents = review.get("datasets").and_then(Value::as_array).ok_or_else(|| {
        Stop::Refuse(DescriptionPackError::named(
            "review.shape",
            "the description review document is not an object",
        ))
    })?;
    let mut approvals = Vec::new();
    for decision in decisions {
        if decision.get("applied").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let dataset = decision.get("dataset").and_then(Value::as_str).ok_or_else(|| {
            Stop::Refuse(DescriptionPackError::named(
                "review.shape",
                "the description review document is not an object",
            ))
        })?;
        let ident = decision.get("ident").and_then(Value::as_str).ok_or_else(|| {
            Stop::Refuse(DescriptionPackError::named(
                "review.shape",
                "the description review document is not an object",
            ))
        })?;
        let document = documents
            .iter()
            .find(|item| item.get("dataset").and_then(Value::as_str) == Some(dataset))
            .and_then(|item| item.get("document"))
            .ok_or(Stop::Unsupported("curated.document_absent"))?;
        let column = document
            .get("columns")
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item.get("name").and_then(Value::as_str) == Some(ident))
            })
            .ok_or(Stop::Unsupported("curated.field_absent"))?;
        let label = taken_field(decision, column, "label", "label_disposition")?;
        let description = taken_field(decision, column, "description", "description_disposition")?;
        if label.is_none() && description.is_none() {
            continue;
        }
        let reviewer_label = decision.get("reviewer_label").and_then(Value::as_str).ok_or_else(|| {
            Stop::Refuse(DescriptionPackError::named(
                "review.shape",
                "the description review document is not an object",
            ))
        })?;
        let reviewer_at = decision.get("reviewer_at").and_then(Value::as_str).ok_or_else(|| {
            Stop::Refuse(DescriptionPackError::named(
                "review.shape",
                "the description review document is not an object",
            ))
        })?;
        let authenticated = decision.get("authenticated").and_then(Value::as_bool).ok_or_else(|| {
            Stop::Refuse(DescriptionPackError::named(
                "review.shape",
                "the description review document is not an object",
            ))
        })?;
        approvals.push(ColumnApproval {
            dataset: dataset.to_owned(),
            ident: ident.to_owned(),
            label,
            description,
            reviewer_label: reviewer_label.to_owned(),
            reviewer_at: reviewer_at.to_owned(),
            authenticated,
        });
    }
    approvals.sort_by(|left, right| (&left.dataset, &left.ident).cmp(&(&right.dataset, &right.ident)));
    Ok(approvals)
}

fn taken_field(
    decision: &Value,
    document: &Value,
    field: &str,
    disposition: &str,
) -> Result<Option<String>, Stop> {
    let kind = decision.get(disposition).and_then(Value::as_str).unwrap_or("");
    if kind != "filled" && kind != "replaced" {
        return Ok(None);
    }
    match document.get(field).and_then(Value::as_str) {
        Some(text) => Ok(Some(text.to_owned())),
        None => Err(Stop::Unsupported("curated.field_absent")),
    }
}

fn compile_downstream(
    artifact_text: &str,
    settings: &Value,
    decisions: &Value,
    roles: &[Value],
) -> Result<Downstream, DescriptionPackError> {
    let artifact: Value = serde_json::from_str(artifact_text).map_err(|_| {
        DescriptionPackError::named("seal.rejected", "the sealed captured draft is not an object")
    })?;
    let command = serde_json::json!({
        "artifact": artifact,
        "decisions": decisions,
        "protocol": REVIEWED_COMMAND,
        "settings": settings,
    });
    let effective_text = compile_reviewed_effective(&canonical_json(&command)).map_err(|err| {
        DescriptionPackError::upstream(err.code, err.message)
    })?;
    let effective: Value = serde_json::from_str(&effective_text).map_err(|_| {
        DescriptionPackError::named(
            "effective.shape",
            "the reviewed effective document is not an object",
        )
    })?;
    let effective_oos = effective.get("effective_oos").ok_or_else(|| {
        DescriptionPackError::named(
            "effective.shape",
            "the reviewed effective document is not an object",
        )
    })?;
    let effective_oos_text = canonical_json(effective_oos);
    let mut packs = Vec::with_capacity(roles.len());
    let mut seen = BTreeSet::new();
    for role in roles {
        if !role.is_object() {
            return Err(DescriptionPackError::named(
                "schema.invalid",
                "a role visibility value is not an object",
            ));
        }
        let name = role.get("role").and_then(Value::as_str).ok_or_else(|| {
            DescriptionPackError::named("schema.invalid", "a role visibility value has no role")
        })?;
        if !seen.insert(name.to_owned()) {
            return Err(DescriptionPackError::named(
                "role.duplicate",
                "two visibility snapshots use the same role",
            ));
        }
        let pack = compile_role_pack(&effective_oos_text, &canonical_json(role)).map_err(|err| {
            DescriptionPackError::upstream(err.code, err.message)
        })?;
        let artifact_value: Value = serde_json::from_str(&pack.artifact).map_err(|_| {
            DescriptionPackError::named("pack.shape", "a compiled role pack is not an object")
        })?;
        packs.push(serde_json::json!({
            "artifact": artifact_value,
            "artifact_sha256": sha256_bytes(pack.artifact.as_bytes()),
            "cache_key_sha256": pack.cache_key_sha256,
            "manifest_sha256": pack.manifest_sha256,
            "pack_digest": pack.pack_digest,
            "role": name,
        }));
    }
    packs.sort_by(|left, right| {
        left.get("role")
            .and_then(Value::as_str)
            .cmp(&right.get("role").and_then(Value::as_str))
    });
    Ok(Downstream {
        effective_text,
        effective,
        packs,
    })
}

fn command_document(compiled: &CapturedPgDraft) -> String {
    format!(
        "{{\"draft\":{},\"evidence_envelope\":{},\"protocol\":\"{CAPTURED_COMMAND}\",\"wrapper\":{}}}",
        compiled.draft, compiled.evidence_envelope, compiled.wrapper
    )
}

fn command_text_of(artifact: &Value) -> Result<String, DescriptionPackError> {
    let draft = artifact.get("draft").ok_or_else(seal_rejected)?;
    let envelope = artifact.get("evidence_envelope").ok_or_else(seal_rejected)?;
    let wrapper = artifact.get("wrapper").ok_or_else(seal_rejected)?;
    Ok(format!(
        "{{\"draft\":{},\"evidence_envelope\":{},\"protocol\":\"{CAPTURED_COMMAND}\",\"wrapper\":{}}}",
        canonical_json(draft),
        canonical_json(envelope),
        canonical_json(wrapper)
    ))
}

fn optional_json(value: Option<&Value>) -> Result<Option<String>, DescriptionPackError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(inner) => Ok(Some(canonical_json(inner))),
    }
}

fn queue_projection(review: &Value) -> Value {
    let Some(items) = review.get("queue").and_then(Value::as_array) else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        items
            .iter()
            .map(|item| {
                let mut projected = serde_json::json!({
                    "dataset": item.get("dataset").and_then(Value::as_str).unwrap_or(""),
                    "kind": item.get("kind").and_then(Value::as_str).unwrap_or(""),
                    "reason": item.get("reason").and_then(Value::as_str).unwrap_or(""),
                });
                if let Some(column) = item.get("column").and_then(Value::as_str)
                    && let Some(object) = projected.as_object_mut()
                {
                    object.insert("column".to_owned(), Value::String(column.to_owned()));
                }
                projected
            })
            .collect(),
    )
}

fn finish(map: Map<String, Value>) -> Result<String, DescriptionPackError> {
    let unsigned = canonical_json(&Value::Object(map.clone()));
    if unsigned.len() > DOCUMENT_MAX_BYTES {
        return Err(DescriptionPackError::named(
            "budget.bytes",
            "document exceeds the byte budget",
        ));
    }
    let hash = sha256_bytes(unsigned.as_bytes());
    let mut sealed = map;
    sealed.insert("pack_sha256".to_owned(), Value::String(hash));
    let text = canonical_json(&Value::Object(sealed));
    if text.len() > DOCUMENT_MAX_BYTES {
        return Err(DescriptionPackError::named(
            "budget.bytes",
            "document exceeds the byte budget",
        ));
    }
    Ok(text)
}

fn unsupported_outcome(seam: &str, review_sha: &str) -> Result<String, DescriptionPackError> {
    let mut map = Map::new();
    map.insert("compiler".to_owned(), Value::String(PACK_COMPILER.to_owned()));
    map.insert("outcome".to_owned(), Value::String("unsupported".to_owned()));
    map.insert("protocol".to_owned(), Value::String(PACK_PROTOCOL.to_owned()));
    map.insert(
        "review_sha256".to_owned(),
        Value::String(review_sha.to_owned()),
    );
    map.insert("seam".to_owned(), Value::String(seam.to_owned()));
    map.insert("statements".to_owned(), statements("unsupported"));
    finish(map)
}

fn seam_code(code: &str) -> &'static str {
    match code.rsplit('/').next().unwrap_or(code) {
        "curated.unknown_item" => "curated.unknown_item",
        "schema.invalid" => "schema.invalid",
        _ => "curated.overlay_rejected",
    }
}

fn seal_rejected() -> DescriptionPackError {
    DescriptionPackError::named(
        "seal.rejected",
        "the sealed captured draft does not recompile to its wrapper",
    )
}

fn require_sha(value: &str) -> Result<(), DescriptionPackError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(DescriptionPackError::named(
            "schema.invalid",
            "a hash is not 64 lowercase hexadecimal characters",
        ))
    }
}

fn require_object(value: &Value, message: &'static str) -> Result<(), DescriptionPackError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(DescriptionPackError::named("schema.invalid", message))
    }
}

fn text_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn json_string(value: &str) -> Value {
    Value::String(value.to_owned())
}

fn map_scan(err: DiagnosticList) -> DescriptionPackError {
    let code = err
        .items
        .first()
        .map(|item| item.code.clone())
        .unwrap_or_else(|| format!("{COMMAND_PROTOCOL}/json.syntax"));
    let name = code.rsplit('/').next().unwrap_or("json.syntax");
    let message = match name {
        "json.budget.bytes" => "document exceeds the byte budget",
        "json.budget.depth" => "JSON nesting exceeds the depth budget",
        "json.budget.nodes" => "JSON node count exceeds the budget",
        "json.duplicate_key" => "duplicate JSON object key",
        _ => "pack command JSON is not valid",
    };
    DescriptionPackError::upstream(code, message)
}

fn map_schema(err: serde_json::Error) -> DescriptionPackError {
    let text = err.to_string();
    if text.contains("unknown field") {
        DescriptionPackError::named(
            "schema.invalid",
            "the pack command has an unknown field",
        )
    } else if text.contains("missing field") {
        DescriptionPackError::named(
            "schema.invalid",
            "the pack command is missing a required field",
        )
    } else {
        DescriptionPackError::named("schema.invalid", "the pack command is not the pack shape")
    }
}

impl From<description_review::DescriptionReviewError> for DescriptionPackError {
    fn from(err: description_review::DescriptionReviewError) -> Self {
        DescriptionPackError::upstream(err.code, err.message)
    }
}
