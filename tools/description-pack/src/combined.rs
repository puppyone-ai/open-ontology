//! One reseal for several per-table description reviews that share a catalog.
//!
//! Approvals are merged, then `reseal` runs once. This module does not set
//! `review.state`, grant, authenticate, or write the caller's files.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Map, Value};

use description_review::apply_description_review;
use oo_core::json_scan::parse_json_value;

use crate::apply::{
    collect_approvals, collect_column_approvals, command_text_of, compile_downstream,
    embedded_artifact, finish, occupied_replacement_values, queue_projection, recompile_original,
    reject_stale, reseal, statements, Approval, ColumnApproval, ReplaceAuth, Reseal, Stop,
};
use crate::canonical::{canonical_json, sha256_bytes};
use crate::error::{
    COMBINED_COMMAND_PROTOCOL, COMBINED_PROTOCOL, COMMAND_PROTOCOL, PACK_COMPILER, PACK_PROTOCOL,
    DescriptionPackError,
};

const MAX_REVIEWS: usize = 16;

/// Read one combined command and return the canonical document.
///
/// The returned text has no trailing newline. A refusal returns `Err` and
/// no document. The captured compiler is called through `reseal` at most once.
pub fn apply_description_pack_combined(input: &str) -> Result<String, DescriptionPackError> {
    let value = parse_json_value(input, COMBINED_COMMAND_PROTOCOL).map_err(map_scan)?;
    let command: CombinedIn = serde_json::from_value(value).map_err(map_schema)?;
    if command.protocol != COMBINED_COMMAND_PROTOCOL {
        return Err(DescriptionPackError::combined(
            "protocol.mismatch",
            "the command protocol is not the combined description pack command",
        ));
    }
    require_sha(&command.expected_schema_hash)?;
    require_sha(&command.expected_evidence_hash)?;
    require_sha(&command.expected_curated_sha256)?;
    require_sha(&command.expected_artifact_sha256)?;
    require_sha(&command.expected_prior_review_sha256)?;
    if !command.settings.is_object() {
        return Err(DescriptionPackError::combined(
            "schema.invalid",
            "settings is not an object",
        ));
    }
    let Some(decisions) = command.decisions.as_array() else {
        return Err(DescriptionPackError::combined(
            "schema.invalid",
            "decisions is not an array",
        ));
    };
    let Some(roles) = command.roles.as_array() else {
        return Err(DescriptionPackError::combined(
            "schema.invalid",
            "roles is not an array",
        ));
    };
    if roles.len() != 2 {
        return Err(DescriptionPackError::combined(
            "schema.invalid",
            "the combined command requires two roles",
        ));
    }
    if command.reviews.is_empty() || command.reviews.len() > MAX_REVIEWS {
        return Err(DescriptionPackError::combined(
            "schema.invalid",
            "reviews must contain one to sixteen documents",
        ));
    }
    let mut seen_roles = BTreeSet::new();
    for role in roles {
        if !role.is_object() {
            return Err(DescriptionPackError::combined(
                "schema.invalid",
                "a role visibility value is not an object",
            ));
        }
        let name = role.get("role").and_then(Value::as_str).ok_or_else(|| {
            DescriptionPackError::combined("schema.invalid", "a role visibility value has no role")
        })?;
        if !seen_roles.insert(name.to_owned()) {
            return Err(DescriptionPackError::combined(
                "role.duplicate",
                "two visibility snapshots use the same role",
            ));
        }
    }
    let mut members = Vec::with_capacity(command.reviews.len());
    for pin in &command.reviews {
        require_sha(&pin.expected_review_sha256)?;
        members.push(load_member(pin)?);
    }
    let Some(shared) = members.first().map(scope_pins) else {
        return Err(DescriptionPackError::combined(
            "schema.invalid",
            "reviews must contain one to sixteen documents",
        ));
    };
    if members.iter().any(|item| scope_pins(item) != shared) {
        return Err(DescriptionPackError::combined(
            "review.mixed_scope",
            "the reviews do not share one sealed catalog",
        ));
    }
    if shared.wrapper_schema != shared.schema
        || shared.wrapper_evidence != shared.evidence
        || shared.schema != command.expected_schema_hash
        || shared.evidence != command.expected_evidence_hash
        || shared.curated_sha != command.expected_curated_sha256
        || shared.artifact_sha != command.expected_artifact_sha256
        || shared.prior_sha != command.expected_prior_review_sha256
    {
        return Err(DescriptionPackError::combined(
            "review.stale",
            "a review pin or queue is stale",
        ));
    }
    for member in &members {
        if let Err(err) = reject_stale(&member.review) {
            return Err(stale_or_lift(err));
        }
    }
    let mut datasets = BTreeSet::new();
    for member in &members {
        if !datasets.insert(member.dataset.clone()) {
            return Err(DescriptionPackError::combined(
                "review.double",
                "the same dataset is selected twice",
            ));
        }
    }
    members.sort_by(|left, right| left.dataset.cmp(&right.dataset));
    let catalog = {
        let Some(first) = members.first() else {
            return Err(DescriptionPackError::combined(
                "schema.invalid",
                "reviews must contain one to sixteen documents",
            ));
        };
        catalog_datasets(&first.artifact)?
    };
    reject_classes(decisions, &catalog)?;
    reject_role_scope(roles, &catalog)?;
    let mut approvals = Vec::new();
    let mut column_approvals = Vec::new();
    for member in &members {
        approvals.extend(take_approvals(&member.review)?);
        column_approvals.extend(take_columns(&member.review)?);
    }
    approvals.sort_by(|left, right| left.dataset.cmp(&right.dataset));
    column_approvals.sort_by(|left, right| {
        (&left.dataset, &left.ident).cmp(&(&right.dataset, &right.ident))
    });
    reject_approval_clash(&approvals)?;
    reject_column_clash(&column_approvals)?;
    let Some(first) = members.first() else {
        return Err(DescriptionPackError::combined(
            "schema.invalid",
            "reviews must contain one to sixteen documents",
        ));
    };
    let original = recompile_original(&first.artifact).map_err(lift)?;
    let sealed = if approvals.is_empty() && column_approvals.is_empty() {
        None
    } else {
        match reseal(
            &first.artifact,
            &original,
            &approvals,
            &column_approvals,
            ReplaceAuth { enabled: false, expected: None },
            ReplaceAuth { enabled: false, expected: None },
        ) {
            Ok(item) => Some(item),
            Err(Stop::Unsupported(seam)) => {
                return Err(DescriptionPackError::combined(
                    seam,
                    "the combined command does not return a seal",
                ));
            }
            Err(Stop::Refuse(err)) => return Err(lift(err)),
        }
    };
    let artifact_text = match &sealed {
        Some(item) => item.text.clone(),
        None => command_text_of(&first.artifact).map_err(lift)?,
    };
    let downstream = compile_downstream(&artifact_text, &command.settings, &command.decisions, roles)
        .map_err(lift)?;
    finish(document(
        &command,
        &members,
        &original.wrapper_hash,
        &original.draft_hash,
        sealed.as_ref(),
        &artifact_text,
        &downstream,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CombinedIn {
    protocol: String,
    reviews: Vec<ReviewPin>,
    expected_schema_hash: String,
    expected_evidence_hash: String,
    expected_curated_sha256: String,
    expected_artifact_sha256: String,
    expected_prior_review_sha256: String,
    settings: Value,
    decisions: Value,
    roles: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewPin {
    review: String,
    expected_review_sha256: String,
}

struct Member {
    dataset: String,
    review_sha: String,
    review: Value,
    artifact: Value,
    schema: String,
    evidence: String,
    wrapper_schema: String,
    wrapper_evidence: String,
    curated_sha: String,
    artifact_sha: String,
    prior_sha: String,
}

fn load_member(pin: &ReviewPin) -> Result<Member, DescriptionPackError> {
    let bytes = apply_description_review(&pin.review).map_err(|err| {
        DescriptionPackError::upstream(err.code, err.message)
    })?;
    let review_sha = sha256_bytes(bytes.as_bytes());
    if review_sha != pin.expected_review_sha256 {
        return Err(DescriptionPackError::combined(
            "review.hash",
            "the description review hash does not match",
        ));
    }
    let review: Value = serde_json::from_str(&bytes).map_err(|_| {
        DescriptionPackError::combined(
            "review.shape",
            "the description review document is not an object",
        )
    })?;
    if review.get("outcome").and_then(Value::as_str) != Some("reviewed") {
        return Err(DescriptionPackError::combined(
            "review.blocked",
            "a review is not a reviewed description",
        ));
    }
    let command: Value = serde_json::from_str(&pin.review).map_err(|_| {
        DescriptionPackError::combined("schema.invalid", "a review command is not an object")
    })?;
    let artifact = embedded_artifact(&pin.review).map_err(lift)?;
    let schema = required_text(command.pointer("/current/schema_hash"))?;
    let evidence = required_text(command.pointer("/current/evidence_hash"))?;
    let wrapper_schema = required_text(artifact.pointer("/wrapper/schema_hash"))?;
    let wrapper_evidence = required_text(artifact.pointer("/wrapper/evidence_hash"))?;
    let curated = command.get("curated").ok_or_else(|| {
        DescriptionPackError::combined("schema.invalid", "a review command has no curated object")
    })?;
    if !curated.is_object() {
        return Err(DescriptionPackError::combined(
            "schema.invalid",
            "a review command has no curated object",
        ));
    }
    let dataset = selected_dataset(&command)?;
    Ok(Member {
        dataset,
        review_sha,
        review,
        artifact_sha: sha256_bytes(canonical_json(&artifact).as_bytes()),
        prior_sha: prior_hash(&artifact)?,
        curated_sha: sha256_bytes(canonical_json(curated).as_bytes()),
        artifact,
        schema,
        evidence,
        wrapper_schema,
        wrapper_evidence,
    })
}

fn selected_dataset(command: &Value) -> Result<String, DescriptionPackError> {
    let embedded = command.get("command").and_then(Value::as_str).ok_or_else(|| {
        DescriptionPackError::combined("schema.invalid", "a review command has no captured command")
    })?;
    let request: Value = serde_json::from_str(embedded).map_err(|_| {
        DescriptionPackError::combined("schema.invalid", "a review command has no captured command")
    })?;
    let tables = request.pointer("/selection/tables").and_then(Value::as_array).ok_or_else(|| {
        DescriptionPackError::combined("review.mixed_scope", "a review selection is not one dataset")
    })?;
    let mut names = BTreeSet::new();
    for table in tables {
        let dataset = table.get("dataset").and_then(Value::as_str).ok_or_else(|| {
            DescriptionPackError::combined("review.mixed_scope", "a review selection is not one dataset")
        })?;
        names.insert(dataset.to_owned());
    }
    let mut listed = names.into_iter();
    let Some(dataset) = listed.next() else {
        return Err(DescriptionPackError::combined(
            "review.mixed_scope",
            "a review selection is not one dataset",
        ));
    };
    if listed.next().is_some() {
        return Err(DescriptionPackError::combined(
            "review.mixed_scope",
            "a review selection is not one dataset",
        ));
    }
    for key in ["decisions", "column_decisions"] {
        let Some(rows) = command.get(key) else {
            continue;
        };
        let rows = rows.as_array().ok_or_else(|| {
            DescriptionPackError::combined("schema.invalid", "a review decision list is not an array")
        })?;
        for row in rows {
            let named = row.get("dataset").and_then(Value::as_str).ok_or_else(|| {
                DescriptionPackError::combined("schema.invalid", "a review decision has no dataset")
            })?;
            if named != dataset {
                return Err(DescriptionPackError::combined(
                    "review.mixed_scope",
                    "a decision names a dataset outside its selection",
                ));
            }
        }
    }
    Ok(dataset)
}

#[derive(Clone, PartialEq, Eq)]
struct ScopePins {
    schema: String,
    evidence: String,
    wrapper_schema: String,
    wrapper_evidence: String,
    curated_sha: String,
    artifact_sha: String,
    prior_sha: String,
}

fn scope_pins(member: &Member) -> ScopePins {
    ScopePins {
        schema: member.schema.clone(),
        evidence: member.evidence.clone(),
        wrapper_schema: member.wrapper_schema.clone(),
        wrapper_evidence: member.wrapper_evidence.clone(),
        curated_sha: member.curated_sha.clone(),
        artifact_sha: member.artifact_sha.clone(),
        prior_sha: member.prior_sha.clone(),
    }
}

fn prior_hash(artifact: &Value) -> Result<String, DescriptionPackError> {
    let reviews = artifact.pointer("/evidence_envelope/reviews");
    let text = match reviews {
        None | Some(Value::Null) => "[]".to_owned(),
        Some(value) if value.is_array() => canonical_json(value),
        Some(_) => {
            return Err(DescriptionPackError::combined(
                "review.mixed_scope",
                "the sealed review list is not an array",
            ));
        }
    };
    Ok(sha256_bytes(text.as_bytes()))
}

fn catalog_datasets(artifact: &Value) -> Result<BTreeSet<String>, DescriptionPackError> {
    let tables = artifact
        .pointer("/evidence_envelope/catalog/tables")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            DescriptionPackError::combined("schema.invalid", "the sealed catalog has no tables")
        })?;
    let mut names = BTreeSet::new();
    for table in tables {
        let schema = table
            .get("schema_ident")
            .and_then(Value::as_str)
            .or_else(|| table.get("schema").and_then(Value::as_str))
            .ok_or_else(|| {
                DescriptionPackError::combined("schema.invalid", "the sealed catalog has no tables")
            })?;
        let name = table
            .get("name_ident")
            .and_then(Value::as_str)
            .or_else(|| table.get("name").and_then(Value::as_str))
            .ok_or_else(|| {
                DescriptionPackError::combined("schema.invalid", "the sealed catalog has no tables")
            })?;
        names.insert(format!("{schema}.{name}"));
    }
    Ok(names)
}

fn reject_classes(rows: &[Value], catalog: &BTreeSet<String>) -> Result<(), DescriptionPackError> {
    let mut seen: BTreeMap<(String, String), String> = BTreeMap::new();
    for row in rows {
        if !row.is_object() {
            return Err(DescriptionPackError::combined(
                "schema.invalid",
                "a classification decision is not an object",
            ));
        }
        let dataset = row.get("dataset").and_then(Value::as_str).ok_or_else(|| {
            DescriptionPackError::combined("schema.invalid", "a classification decision has no dataset")
        })?;
        let column = row.get("column").and_then(Value::as_str).ok_or_else(|| {
            DescriptionPackError::combined("schema.invalid", "a classification decision has no column")
        })?;
        if !catalog.contains(dataset) {
            return Err(DescriptionPackError::combined(
                "review.mixed_scope",
                "a decision names a dataset outside the shared catalog",
            ));
        }
        let canonical = canonical_json(row);
        if let Some(prior) = seen.get(&(dataset.to_owned(), column.to_owned())) {
            if prior == &canonical {
                return Err(DescriptionPackError::combined(
                    "review.double",
                    "the same classification column is repeated",
                ));
            }
            return Err(DescriptionPackError::combined(
                "review.conflict",
                "the same column is classified twice",
            ));
        }
        seen.insert((dataset.to_owned(), column.to_owned()), canonical);
    }
    Ok(())
}

fn reject_role_scope(roles: &[Value], catalog: &BTreeSet<String>) -> Result<(), DescriptionPackError> {
    for role in roles {
        let datasets = role.get("datasets").and_then(Value::as_array).ok_or_else(|| {
            DescriptionPackError::combined("schema.invalid", "a role has no datasets")
        })?;
        for dataset in datasets {
            let name = dataset.get("dataset").and_then(Value::as_str).ok_or_else(|| {
                DescriptionPackError::combined("schema.invalid", "a role dataset is missing")
            })?;
            if !catalog.contains(name) {
                return Err(DescriptionPackError::combined(
                    "review.mixed_scope",
                    "a role names a dataset outside the shared catalog",
                ));
            }
        }
    }
    Ok(())
}

fn take_approvals(review: &Value) -> Result<Vec<Approval>, DescriptionPackError> {
    match collect_approvals(review) {
        Ok(items) => Ok(items),
        Err(Stop::Unsupported(seam)) => Err(DescriptionPackError::combined(
            seam,
            "the combined command does not return a seal",
        )),
        Err(Stop::Refuse(err)) => Err(lift(err)),
    }
}

fn take_columns(review: &Value) -> Result<Vec<ColumnApproval>, DescriptionPackError> {
    match collect_column_approvals(review) {
        Ok(items) => Ok(items),
        Err(Stop::Unsupported(seam)) => Err(DescriptionPackError::combined(
            seam,
            "the combined command does not return a seal",
        )),
        Err(Stop::Refuse(err)) => Err(lift(err)),
    }
}

fn reject_approval_clash(items: &[Approval]) -> Result<(), DescriptionPackError> {
    let mut previous: Option<&Approval> = None;
    for item in items {
        if let Some(prior) = previous.filter(|prior| prior.dataset == item.dataset) {
            if prior.label != item.label || prior.description != item.description || prior.grain != item.grain {
                return Err(DescriptionPackError::combined(
                    "review.conflict",
                    "the same field would receive two descriptions",
                ));
            }
            return Err(DescriptionPackError::combined(
                "review.double",
                "the same dataset is selected twice",
            ));
        }
        previous = Some(item);
    }
    Ok(())
}

fn reject_column_clash(items: &[ColumnApproval]) -> Result<(), DescriptionPackError> {
    let mut previous: Option<&ColumnApproval> = None;
    for item in items {
        if let Some(prior) = previous.filter(|prior| prior.dataset == item.dataset && prior.ident == item.ident) {
            if prior.label != item.label || prior.description != item.description {
                return Err(DescriptionPackError::combined(
                    "review.conflict",
                    "the same field would receive two descriptions",
                ));
            }
            return Err(DescriptionPackError::combined(
                "review.double",
                "the same dataset is selected twice",
            ));
        }
        previous = Some(item);
    }
    Ok(())
}

fn document(
    command: &CombinedIn,
    members: &[Member],
    original_wrapper: &str,
    original_draft: &str,
    sealed: Option<&Reseal>,
    artifact_text: &str,
    downstream: &crate::apply::Downstream,
) -> Map<String, Value> {
    let reseal_count = u64::from(sealed.is_some());
    let outcome = if sealed.is_some() { "packed" } else { "reviewed_unchanged" };
    let mut product = Map::new();
    product.insert(
        "artifact_sha256".to_owned(),
        Value::String(sha256_bytes(artifact_text.as_bytes())),
    );
    product.insert("compiler".to_owned(), Value::String(PACK_COMPILER.to_owned()));
    product.insert("curated_documents".to_owned(), merged_curated(members));
    product.insert("effective".to_owned(), downstream.effective.clone());
    product.insert(
        "effective_sha256".to_owned(),
        Value::String(sha256_bytes(downstream.effective_text.as_bytes())),
    );
    product.insert(
        "original_draft_hash".to_owned(),
        Value::String(original_draft.to_owned()),
    );
    product.insert(
        "original_wrapper_hash".to_owned(),
        Value::String(original_wrapper.to_owned()),
    );
    product.insert("outcome".to_owned(), Value::String(outcome.to_owned()));
    product.insert("packs".to_owned(), Value::Array(downstream.packs.clone()));
    product.insert("protocol".to_owned(), Value::String(PACK_PROTOCOL.to_owned()));
    product.insert(
        "resealed_artifact".to_owned(),
        sealed.map(|item| item.artifact.clone()).unwrap_or(Value::Null),
    );
    product.insert(
        "resealed_draft_hash".to_owned(),
        sealed
            .map(|item| Value::String(item.draft_hash.clone()))
            .unwrap_or(Value::Null),
    );
    product.insert(
        "resealed_evidence_envelope_hash".to_owned(),
        sealed
            .map(|item| Value::String(item.evidence_envelope_hash.clone()))
            .unwrap_or(Value::Null),
    );
    product.insert(
        "resealed_wrapper_hash".to_owned(),
        sealed
            .map(|item| Value::String(item.wrapper_hash.clone()))
            .unwrap_or(Value::Null),
    );
    if let Some(item) = sealed.filter(|item| !occupied_replacement_values(item).is_empty()) {
        product.insert(
            "occupied_replacements".to_owned(),
            Value::Array(occupied_replacement_values(item)),
        );
    }
    product.insert(
        "statements".to_owned(),
        statements(if sealed.is_some() { "compiler" } else { "not_required" }),
    );
    let mut map = Map::new();
    map.insert(
        "artifact_sha256".to_owned(),
        Value::String(command.expected_artifact_sha256.clone()),
    );
    map.insert("compiler".to_owned(), Value::String(PACK_COMPILER.to_owned()));
    map.insert(
        "curated_sha256".to_owned(),
        Value::String(command.expected_curated_sha256.clone()),
    );
    map.insert(
        "evidence_hash".to_owned(),
        Value::String(command.expected_evidence_hash.clone()),
    );
    map.insert("outcome".to_owned(), Value::String(outcome.to_owned()));
    map.insert(
        "prior_review_sha256".to_owned(),
        Value::String(command.expected_prior_review_sha256.clone()),
    );
    map.insert("product".to_owned(), Value::Object(product));
    map.insert("protocol".to_owned(), Value::String(COMBINED_PROTOCOL.to_owned()));
    map.insert("reseal_count".to_owned(), Value::from(reseal_count));
    map.insert(
        "schema_hash".to_owned(),
        Value::String(command.expected_schema_hash.clone()),
    );
    map.insert(
        "statements".to_owned(),
        statements(if sealed.is_some() { "compiler" } else { "not_required" }),
    );
    map.insert("tables".to_owned(), Value::Array(members.iter().map(table_row).collect()));
    map
}

fn table_row(member: &Member) -> Value {
    serde_json::json!({
        "authenticated": false,
        "column_decisions": rows_for(&member.review, "column_decisions", &member.dataset),
        "dataset": member.dataset,
        "decisions": rows_for(&member.review, "decisions", &member.dataset),
        "prior_reviews": prior_for(&member.artifact, &member.dataset),
        "queue": queue_for(&member.review, &member.dataset),
        "review_sha256": member.review_sha,
    })
}

fn rows_for(review: &Value, key: &str, dataset: &str) -> Value {
    let Some(items) = review.get(key).and_then(Value::as_array) else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        items
            .iter()
            .filter(|item| item.get("dataset").and_then(Value::as_str) == Some(dataset))
            .cloned()
            .collect(),
    )
}

fn queue_for(review: &Value, dataset: &str) -> Value {
    let Value::Array(items) = queue_projection(review) else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        items
            .into_iter()
            .filter(|item| item.get("dataset").and_then(Value::as_str) == Some(dataset))
            .collect(),
    )
}

fn prior_for(artifact: &Value, dataset: &str) -> Value {
    let Some(items) = artifact.pointer("/evidence_envelope/reviews").and_then(Value::as_array) else {
        return Value::Array(Vec::new());
    };
    let dataset_item = format!("dataset/{dataset}");
    let column_prefix = format!("column/{dataset}/");
    Value::Array(
        items
            .iter()
            .filter(|item| {
                let id = item.get("item_id").and_then(Value::as_str).unwrap_or("");
                id == dataset_item || id.starts_with(&column_prefix)
            })
            .cloned()
            .collect(),
    )
}

fn merged_curated(members: &[Member]) -> Value {
    let mut rows: BTreeMap<String, Value> = BTreeMap::new();
    if let Some(first) = members.first().and_then(|member| member.review.get("datasets")).and_then(Value::as_array) {
        for item in first {
            if let Some(name) = item.get("dataset").and_then(Value::as_str) {
                rows.insert(name.to_owned(), item.clone());
            }
        }
    }
    for member in members.iter().skip(1) {
        let Some(items) = member.review.get("datasets").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if item.get("changed").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            if let Some(name) = item.get("dataset").and_then(Value::as_str) {
                rows.insert(name.to_owned(), item.clone());
            }
        }
    }
    Value::Array(rows.into_values().collect())
}

fn required_text(value: Option<&Value>) -> Result<String, DescriptionPackError> {
    value.and_then(Value::as_str).map(str::to_owned).ok_or_else(|| {
        DescriptionPackError::combined("schema.invalid", "a review pin is missing")
    })
}

fn require_sha(value: &str) -> Result<(), DescriptionPackError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        Ok(())
    } else {
        Err(DescriptionPackError::combined(
            "schema.invalid",
            "a hash is not 64 lowercase hexadecimal characters",
        ))
    }
}

fn stale_or_lift(err: DescriptionPackError) -> DescriptionPackError {
    match err.code.rsplit('/').next().unwrap_or("") {
        "review.schema" | "review.evidence" | "review.cas" | "review.anchor" => {
            DescriptionPackError::combined("review.stale", "a review pin or queue is stale")
        }
        _ => lift(err),
    }
}

fn lift(err: DescriptionPackError) -> DescriptionPackError {
    let Some(rest) = err.code.strip_prefix(COMMAND_PROTOCOL) else {
        return err;
    };
    let name = rest.trim_start_matches('/');
    if let Some(kept) = known_suffix(name) {
        return DescriptionPackError::combined(kept, err.message);
    }
    DescriptionPackError::combined("review.refused", "the combined command was refused")
}

fn known_suffix(name: &str) -> Option<&'static str> {
    Some(match name {
        "review.hash" => "review.hash",
        "review.shape" => "review.shape",
        "review.blocked" => "review.blocked",
        "review.occupied_cas" => "review.occupied_cas",
        "review.occupied_stale" => "review.occupied_stale",
        "review.stale" => "review.stale",
        "seal.artifact" => "seal.artifact",
        "seal.rejected" => "seal.rejected",
        "schema.invalid" => "schema.invalid",
        "role.duplicate" => "role.duplicate",
        "budget.bytes" => "budget.bytes",
        "pack.shape" => "pack.shape",
        "effective.shape" => "effective.shape",
        "protocol.mismatch" => "protocol.mismatch",
        "curated.unknown_item" => "curated.unknown_item",
        "curated.overlay_rejected" => "curated.overlay_rejected",
        "curated.document_absent" => "curated.document_absent",
        "curated.field_absent" => "curated.field_absent",
        "review.dataset_decision_occupied" => "review.dataset_decision_occupied",
        "review.column_decision_occupied" => "review.column_decision_occupied",
        "review.dataset_action_unsupported" => "review.dataset_action_unsupported",
        "review.column_action_unsupported" => "review.column_action_unsupported",
        "review.entry_not_preserved" => "review.entry_not_preserved",
        "review.dataset_fingerprint_absent" => "review.dataset_fingerprint_absent",
        "review.column_fingerprint_absent" => "review.column_fingerprint_absent",
        "effective.field_not_applied" => "effective.field_not_applied",
        _ => return None,
    })
}

fn map_scan(err: oo_core::diagnostic::DiagnosticList) -> DescriptionPackError {
    let code = err
        .items
        .first()
        .map(|item| item.code.clone())
        .unwrap_or_else(|| format!("{COMBINED_COMMAND_PROTOCOL}/json.syntax"));
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
        DescriptionPackError::combined("schema.invalid", "the pack command has an unknown field")
    } else if text.contains("missing field") {
        DescriptionPackError::combined("schema.invalid", "the pack command is missing a required field")
    } else {
        DescriptionPackError::combined("schema.invalid", "the pack command is not the pack shape")
    }
}
