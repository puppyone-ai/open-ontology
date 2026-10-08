//! Re-executed review. Stale, deferred, and unapproved fields do not become a commit.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use description_review::{REVIEW_COMPILER, REVIEW_PROTOCOL, REVIEW_PROTOCOL_V2, apply_description_review};
use oo_core::oos::{DatasetFile, validate_dataset};
use oo_core::yaml::parse_yaml_typed;

use crate::canonical::{canonical_json, canonical_sha256};
use crate::error::{CuratedGitError, PROTOCOL};
use crate::request::{Binding, Request, is_sha256};

const STALE: &[&str] = &[
    "schema",
    "evidence",
    "request",
    "card",
    "candidate",
    "curated",
    "cas",
];
const DATASET_TEXT: &[&str] = &["label", "description", "grain"];
const COLUMN_TEXT: &[&str] = &["label", "description"];

pub struct Plan {
    pub files: Vec<PlannedFile>,
    pub decisions: Vec<Value>,
    pub review_sha256: String,
    pub outcome_sha256: String,
}

pub struct PlannedFile {
    pub dataset: String,
    pub document: Value,
}

pub fn review_plan(request: &Request) -> Result<Plan, CuratedGitError> {
    let text = apply_description_review(&request.review_command).map_err(|error| {
        CuratedGitError::refused(error.code)
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|_| refused())?;
    check_review_hash(&value)?;
    let protocol = text_field(&value, "protocol")?;
    if protocol != REVIEW_PROTOCOL && protocol != REVIEW_PROTOCOL_V2 {
        return Err(refused());
    }
    if text_field(&value, "compiler")? != REVIEW_COMPILER {
        return Err(refused());
    }
    let outcome = text_field(&value, "outcome")?;
    let outcome_sha256 = require_hash(text_field(&value, "outcome_sha256")?)?;
    let review_sha256 = require_hash(text_field(&value, "review_sha256")?)?;
    check_seals(request, &outcome_sha256)?;
    if outcome == "not_candidates" {
        return Err(CuratedGitError::new(
            "review.declined",
            "the compiler did not return candidates",
        ));
    }
    if outcome != "reviewed" {
        return Err(refused());
    }
    let queue = array_field(&value, "queue")?;
    if queue_stale(queue) {
        return Err(CuratedGitError::new(
            "review.stale",
            "the review facts do not match the sealed artifact",
        ));
    }
    if queue_kind(queue, "core_rejected") {
        return Err(CuratedGitError::new(
            "review.core",
            "the compiler rejected a descriptive value",
        ));
    }
    let decisions = receipt_decisions(&value)?;
    let command: Value = serde_json::from_str(&request.review_command).map_err(|_| refused())?;
    let curated = command
        .get("curated")
        .and_then(Value::as_object)
        .ok_or_else(refused)?;
    let bindings = index_bindings(request, curated)?;
    if bound_column_missing(queue, &bindings) {
        return Err(CuratedGitError::new(
            "review.column",
            "a bound column is not in the curated dataset",
        ));
    }
    let files = changed_files(&value, &bindings)?;
    Ok(Plan {
        files,
        decisions,
        review_sha256,
        outcome_sha256,
    })
}

fn check_seals(request: &Request, outcome_sha256: &str) -> Result<(), CuratedGitError> {
    let command: Value = serde_json::from_str(&request.review_command).map_err(|_| refused())?;
    let current = command.get("current").and_then(Value::as_object).ok_or_else(refused)?;
    let schema = current
        .get("schema_hash")
        .and_then(Value::as_str)
        .ok_or_else(refused)?;
    let evidence = current
        .get("evidence_hash")
        .and_then(Value::as_str)
        .ok_or_else(refused)?;
    let expected = command
        .get("expected_outcome_sha256")
        .and_then(Value::as_str)
        .ok_or_else(refused)?;
    if schema != request.expected_schema_hash {
        return Err(CuratedGitError::new(
            "seal.schema",
            "the schema hash does not match the sealed artifact",
        ));
    }
    if evidence != request.expected_evidence_hash {
        return Err(CuratedGitError::new(
            "seal.evidence",
            "the evidence hash does not match the sealed artifact",
        ));
    }
    if expected != outcome_sha256 {
        return Err(CuratedGitError::new(
            "review.hash",
            "the review document hash does not match",
        ));
    }
    Ok(())
}

fn index_bindings<'a>(
    request: &'a Request,
    curated: &Map<String, Value>,
) -> Result<BTreeMap<&'a str, &'a Binding>, CuratedGitError> {
    let mut out = BTreeMap::new();
    for binding in &request.bindings {
        if !curated.contains_key(&binding.dataset) {
            return Err(CuratedGitError::new(
                "binding.unknown",
                "a bound dataset is not in the review command",
            ));
        }
        out.insert(binding.dataset.as_str(), binding);
    }
    Ok(out)
}

fn changed_files(
    review: &Value,
    bindings: &BTreeMap<&str, &Binding>,
) -> Result<Vec<PlannedFile>, CuratedGitError> {
    let mut files = Vec::new();
    for item in array_field(review, "datasets")? {
        let dataset = item
            .get("dataset")
            .and_then(Value::as_str)
            .ok_or_else(refused)?;
        let changed = item
            .get("changed")
            .and_then(Value::as_bool)
            .ok_or_else(refused)?;
        if !changed {
            continue;
        }
        if !bindings.contains_key(dataset) {
            return Err(CuratedGitError::new(
                "scope.dataset",
                "a change is outside the bound datasets",
            ));
        }
        let document = item.get("document").ok_or_else(refused)?;
        let after: DatasetFile = serde_json::from_value(document.clone()).map_err(|_| {
            CuratedGitError::new(
                "field.unapproved",
                "a change is outside compiler-approved descriptive fields",
            )
        })?;
        if !validate_dataset(&after).is_empty() {
            return Err(CuratedGitError::new(
                "field.unapproved",
                "a change is outside compiler-approved descriptive fields",
            ));
        }
        files.push(PlannedFile {
            dataset: dataset.to_owned(),
            document: document.clone(),
        });
        if files.len() > 16 {
            return Err(CuratedGitError::new(
                "limit.files",
                "the change set exceeds the file bound",
            ));
        }
    }
    Ok(files)
}

pub fn load_before(bytes: &[u8], binding: &Binding, curated: &Value) -> Result<DatasetFile, CuratedGitError> {
    if bytes.len() > 1_048_576 {
        return Err(CuratedGitError::new(
            "limit.bytes",
            "the document exceeds the byte bound",
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| yaml_parse())?;
    let git_file: DatasetFile = parse_yaml_typed(text, PROTOCOL).map_err(|_| yaml_parse())?;
    if !validate_dataset(&git_file).is_empty() {
        return Err(yaml_parse());
    }
    let command_file: DatasetFile = serde_json::from_value(curated.clone()).map_err(|_| {
        CuratedGitError::new(
            "curated.drift",
            "the git dataset does not match the review command",
        )
    })?;
    if git_file != command_file {
        return Err(CuratedGitError::new(
            "curated.drift",
            "the git dataset does not match the review command",
        ));
    }
    for column in &binding.columns {
        if !git_file.columns.iter().any(|item| item.name.as_str() == column) {
            return Err(CuratedGitError::new(
                "review.column",
                "a bound column is not in the curated dataset",
            ));
        }
    }
    Ok(git_file)
}

pub fn assert_delta(
    before: &DatasetFile,
    document: &Value,
    columns: &BTreeSet<String>,
) -> Result<(), CuratedGitError> {
    let after: DatasetFile = serde_json::from_value(document.clone()).map_err(|_| unapproved())?;
    let left = serde_json::to_value(before).map_err(|_| unapproved())?;
    let right = serde_json::to_value(&after).map_err(|_| unapproved())?;
    let left_map = left.as_object().ok_or_else(unapproved)?;
    let right_map = right.as_object().ok_or_else(unapproved)?;
    if left_map.len() != right_map.len() {
        return Err(unapproved());
    }
    for (key, left_value) in left_map {
        let Some(right_value) = right_map.get(key) else {
            return Err(unapproved());
        };
        if key == "columns" {
            assert_columns(left_value, right_value, columns)?;
            continue;
        }
        if left_value == right_value {
            continue;
        }
        if !DATASET_TEXT.contains(&key.as_str()) {
            return Err(unapproved());
        }
    }
    Ok(())
}

fn assert_columns(
    left: &Value,
    right: &Value,
    allowed: &BTreeSet<String>,
) -> Result<(), CuratedGitError> {
    let left_items = left.as_array().ok_or_else(unapproved)?;
    let right_items = right.as_array().ok_or_else(unapproved)?;
    if left_items.len() != right_items.len() {
        return Err(unapproved());
    }
    for (left_item, right_item) in left_items.iter().zip(right_items) {
        let left_map = left_item.as_object().ok_or_else(unapproved)?;
        let right_map = right_item.as_object().ok_or_else(unapproved)?;
        if left_map.get("name") != right_map.get("name") || left_map.len() != right_map.len() {
            return Err(unapproved());
        }
        let name = left_map
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(unapproved)?;
        for (key, left_value) in left_map {
            let Some(right_value) = right_map.get(key) else {
                return Err(unapproved());
            };
            if left_value == right_value {
                continue;
            }
            if !COLUMN_TEXT.contains(&key.as_str()) {
                return Err(unapproved());
            }
            if !allowed.contains(name) {
                return Err(CuratedGitError::new(
                    "scope.column",
                    "a change is outside the bound columns",
                ));
            }
        }
    }
    Ok(())
}

pub fn encode_dataset(after: &DatasetFile) -> Result<Vec<u8>, CuratedGitError> {
    let value = serde_json::to_value(after).map_err(|_| roundtrip())?;
    let mut text = canonical_json(&value);
    text.push('\n');
    if text.len() > 1_048_576 {
        return Err(CuratedGitError::new(
            "limit.bytes",
            "the document exceeds the byte bound",
        ));
    }
    let parsed: DatasetFile = parse_yaml_typed(&text, PROTOCOL).map_err(|_| roundtrip())?;
    if &parsed != after {
        return Err(roundtrip());
    }
    Ok(text.into_bytes())
}

pub fn dataset_path(dataset: &str) -> Result<String, CuratedGitError> {
    let Some((schema, table)) = dataset.split_once('.') else {
        return Err(refused());
    };
    if schema.is_empty()
        || table.is_empty()
        || schema.contains('/')
        || table.contains('/')
        || schema.contains('\\')
        || table.contains('\\')
        || schema.contains("..")
        || table.contains("..")
    {
        return Err(CuratedGitError::new(
            "root.escape",
            "a path escapes the checkout",
        ));
    }
    Ok(format!("ontology/curated/datasets/{schema}/{table}.yaml"))
}

fn receipt_decisions(review: &Value) -> Result<Vec<Value>, CuratedGitError> {
    let mut out = Vec::new();
    for item in array_field(review, "decisions")? {
        out.push(one_decision(item, None, None)?);
    }
    if let Some(columns) = review.get("column_decisions") {
        for item in columns.as_array().ok_or_else(refused)? {
            let column = item.get("column").and_then(Value::as_str).ok_or_else(refused)?;
            let attnum = item.get("attnum").and_then(Value::as_i64).ok_or_else(refused)?;
            out.push(one_decision(item, Some(column), Some(attnum))?);
        }
    }
    Ok(out)
}

fn one_decision(
    item: &Value,
    column: Option<&str>,
    attnum: Option<i64>,
) -> Result<Value, CuratedGitError> {
    if item.get("authenticated").and_then(Value::as_bool) != Some(false) {
        return Err(CuratedGitError::new(
            "review.identity",
            "the review claims an authenticated reviewer",
        ));
    }
    let decision = item.get("decision").and_then(Value::as_str).ok_or_else(refused)?;
    if !matches!(decision, "approve" | "reject" | "defer") {
        return Err(refused());
    }
    Ok(serde_json::json!({
        "applied": item.get("applied").and_then(Value::as_bool).ok_or_else(refused)?,
        "attnum": attnum,
        "authenticated": false,
        "column": column,
        "dataset": item.get("dataset").and_then(Value::as_str).ok_or_else(refused)?,
        "decision": decision,
        "reviewer_at": item.get("reviewer_at").and_then(Value::as_str).ok_or_else(refused)?,
        "reviewer_label": item.get("reviewer_label").and_then(Value::as_str).ok_or_else(refused)?,
    }))
}

fn queue_stale(queue: &[Value]) -> bool {
    queue.iter().any(|item| {
        item.get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| STALE.contains(&reason))
    })
}

fn queue_kind(queue: &[Value], kind: &str) -> bool {
    queue.iter().any(|item| item.get("kind").and_then(Value::as_str) == Some(kind))
}

fn bound_column_missing(queue: &[Value], bindings: &BTreeMap<&str, &Binding>) -> bool {
    queue.iter().any(|item| {
        if item.get("reason").and_then(Value::as_str) != Some("column_missing") {
            return false;
        }
        let Some(dataset) = item.get("dataset").and_then(Value::as_str) else {
            return false;
        };
        let Some(column) = item.get("column").and_then(Value::as_str) else {
            return false;
        };
        bindings
            .get(dataset)
            .is_some_and(|binding| binding.columns.contains(column))
    })
}

fn check_review_hash(value: &Value) -> Result<(), CuratedGitError> {
    let published = value
        .get("review_sha256")
        .and_then(Value::as_str)
        .ok_or_else(refused)?;
    let mut stripped = value.clone();
    stripped
        .as_object_mut()
        .ok_or_else(refused)?
        .remove("review_sha256");
    if canonical_sha256(&stripped) != published {
        return Err(CuratedGitError::new(
            "review.hash",
            "the review document hash does not match",
        ));
    }
    Ok(())
}

fn array_field<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], CuratedGitError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(refused)
}

fn text_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, CuratedGitError> {
    value.get(key).and_then(Value::as_str).ok_or_else(refused)
}

fn require_hash(value: &str) -> Result<String, CuratedGitError> {
    if is_sha256(value) {
        Ok(value.to_owned())
    } else {
        Err(refused())
    }
}

fn refused() -> CuratedGitError {
    CuratedGitError::new(
        "review.refused",
        "the description review compiler refused the command",
    )
}

fn unapproved() -> CuratedGitError {
    CuratedGitError::new(
        "field.unapproved",
        "a change is outside compiler-approved descriptive fields",
    )
}

fn yaml_parse() -> CuratedGitError {
    CuratedGitError::new(
        "yaml.parse",
        "the curated document is not a core dataset file",
    )
}

fn roundtrip() -> CuratedGitError {
    CuratedGitError::new(
        "yaml.roundtrip",
        "the written document does not parse back to the same dataset",
    )
}
