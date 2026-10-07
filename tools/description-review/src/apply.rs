//! Apply one explicit description review in memory.
//!
//! The compiler outcome is re-executed from the original command bytes.
//! A supplied candidate object is not an input.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Map, Value};

use oo_builder::{
    DESCRIPTION_CANDIDATE_COMPILER, DESCRIPTION_CANDIDATE_PROTOCOL, DRAFT_PROTOCOL,
    DescriptionCandidateError, complete_description_candidates,
};
use oo_core::diagnostic::DiagnosticList;
use oo_core::json_scan::parse_json_value;
use oo_core::oos::{CivilDate, DatasetFile};
use oo_core::text::{Label, contains_forbidden_interpolation};

use crate::canonical::{canonical_json, canonical_sha256, push_string, sha256_bytes};
use crate::error::{
    COMMAND_PROTOCOL, REVIEW_COMPILER, REVIEW_PROTOCOL, REVIEW_PROTOCOL_V2, DescriptionReviewError,
};

const OUTCOME_KEYS: &[&str] = &[
    "budget_reason",
    "candidates",
    "compiler",
    "outcome",
    "protocol",
    "refusal",
    "request_sha256",
    "selected_datasets",
    "statements",
    "structural_draft",
];
const OUTCOME_KEYS_V2: &[&str] = &[
    "budget_reason",
    "candidates",
    "columns",
    "compiler",
    "outcome",
    "protocol",
    "refusal",
    "request_sha256",
    "selected_datasets",
    "statements",
    "structural_draft",
];
const COLUMN_KEYS: &[&str] = &[
    "attnum",
    "card_sha256",
    "caveats",
    "column",
    "confidence",
    "dataset",
    "description",
    "effect",
    "ident",
    "label",
    "open_questions",
];
const CANDIDATE_PROTOCOL_V2: &str = "oo.builder.description-candidate/v2";
const DRAFT_KEYS: &[&str] = &[
    "config_hash",
    "draft_hash",
    "evidence_hash",
    "manifest_hash",
    "protocol",
    "schema_hash",
    "source_id",
    "type_hash",
    "wrapper_hash",
];
const CANDIDATE_KEYS: &[&str] = &[
    "card_sha256",
    "caveats",
    "confidence",
    "dataset",
    "description",
    "effect",
    "grain",
    "label",
    "open_questions",
];
const STATEMENT_KEYS: &[&str] = &[
    "auto_accept",
    "certification",
    "confidence_effect",
    "cost",
    "curated",
    "effective",
    "grants",
    "llm_transport",
    "pii_classification",
    "publication",
    "residency",
    "samples",
];
const DECISION_KEYS: &[&str] = &[
    "candidate_sha256",
    "card_sha256",
    "curated_sha256",
    "dataset",
    "decision",
    "evidence_hash",
    "request_sha256",
    "reviewer_at",
    "reviewer_label",
    "schema_hash",
];
const COLUMN_DECISION_KEYS: &[&str] = &[
    "attnum",
    "candidate_sha256",
    "card_sha256",
    "column",
    "curated_sha256",
    "dataset",
    "decision",
    "evidence_hash",
    "request_sha256",
    "reviewer_at",
    "reviewer_label",
    "schema_hash",
];

/// SHA-256 of the canonical JSON literal for one descriptive field.
///
/// Absent text hashes the four bytes `null`. Present text hashes the canonical
/// JSON string, including quotes and short escapes.
pub fn descriptive_field_sha256(value: Option<&str>) -> String {
    let literal = match value {
        None => "null".to_owned(),
        Some(text) => {
            let mut out = String::new();
            push_string(&mut out, text);
            out
        }
    };
    sha256_bytes(literal.as_bytes())
}

/// SHA-256 of the canonical JSON for one core dataset file.
///
/// A value that cannot be serialized hashes the empty byte string, which does
/// not match a 64-character field hash, so nothing is overwritten.
pub fn curated_dataset_sha256(file: &DatasetFile) -> String {
    match serde_json::to_value(file) {
        Ok(value) => canonical_sha256(&value),
        Err(_) => sha256_bytes(b""),
    }
}

/// Read one review command and return the canonical review document.
pub fn apply_description_review(input: &str) -> Result<String, DescriptionReviewError> {
    let value = parse_json_value(input, COMMAND_PROTOCOL).map_err(map_scan)?;
    let command: CommandIn = serde_json::from_value(value).map_err(map_schema)?;
    let review = validate_command(&command)?;
    let curated = load_curated(&command.curated)?;
    let outcome_bytes =
        complete_description_candidates(&command.command).map_err(map_compiler)?;
    let outcome_sha = sha256_bytes(outcome_bytes.as_bytes());
    if outcome_sha != command.expected_outcome_sha256 {
        return Err(error(
            "outcome.hash",
            "the compiler outcome hash does not match",
        ));
    }
    let outcome = parse_outcome(&outcome_bytes)?;
    if !outcome.column_mode && !review.column_decisions.is_empty() {
        return Err(error(
            "decision.unknown",
            "a column decision does not match a column candidate",
        ));
    }
    if outcome.is_candidates {
        check_effects(&outcome)?;
        check_known_decisions(&review.decisions, &outcome)?;
        check_known_columns(&review.column_decisions, &outcome)?;
    }
    let document = build_review(&review, &curated, &outcome, &outcome_sha);
    Ok(finish_review(document))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandIn {
    protocol: String,
    command: String,
    expected_outcome_sha256: String,
    current: CurrentIn,
    curated: Map<String, Value>,
    decisions: Vec<Value>,
    #[serde(default)]
    column_decisions: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CurrentIn {
    schema_hash: String,
    evidence_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldsIn {
    #[serde(default)]
    label: Option<FieldIn>,
    #[serde(default)]
    description: Option<FieldIn>,
    #[serde(default)]
    grain: Option<FieldIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnFieldsIn {
    #[serde(default)]
    label: Option<FieldIn>,
    #[serde(default)]
    description: Option<FieldIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldIn {
    overwrite: bool,
    #[serde(default)]
    current_sha256: Option<String>,
}

struct ReviewCommand {
    current_schema: String,
    current_evidence: String,
    decisions: Vec<Decision>,
    column_decisions: Vec<ColumnDecision>,
}

struct Decision {
    dataset: String,
    kind: Kind,
    reviewer_label: String,
    reviewer_at: String,
    request_sha256: String,
    card_sha256: String,
    schema_hash: String,
    evidence_hash: String,
    candidate_sha256: String,
    curated_sha256: String,
    label: FieldMode,
    description: FieldMode,
    grain: FieldMode,
}

struct ColumnDecision {
    dataset: String,
    column: String,
    attnum: i64,
    kind: Kind,
    reviewer_label: String,
    reviewer_at: String,
    request_sha256: String,
    card_sha256: String,
    schema_hash: String,
    evidence_hash: String,
    candidate_sha256: String,
    curated_sha256: String,
    label: FieldMode,
    description: FieldMode,
}

enum Kind {
    Approve,
    Reject,
    Defer,
}

enum FieldMode {
    Omit,
    KeepFill,
    Overwrite(String),
}

struct Loaded {
    file: DatasetFile,
    document: Value,
    preimage: String,
}

struct Outcome {
    name: String,
    refusal: Value,
    budget_reason: Value,
    request_sha256: Option<String>,
    statements: Value,
    selected: Vec<String>,
    candidates: BTreeMap<String, Candidate>,
    columns: BTreeMap<(String, String), ColumnCandidate>,
    schema_hash: String,
    evidence_hash: String,
    is_candidates: bool,
    column_mode: bool,
}

struct ColumnCandidate {
    dataset: String,
    column: String,
    attnum: i64,
    ident: String,
    card_sha256: String,
    label: String,
    description: String,
    confidence: String,
    caveats: Vec<String>,
    open_questions: Vec<String>,
    effect: String,
    object_sha: String,
}

struct Candidate {
    dataset: String,
    card_sha256: String,
    label: String,
    description: String,
    grain: String,
    caveats: Vec<String>,
    open_questions: Vec<String>,
    confidence: String,
    effect: String,
    object_sha: String,
}

struct QueueItem {
    dataset: String,
    column: Option<String>,
    kind: &'static str,
    reason: String,
}

fn validate_command(command: &CommandIn) -> Result<ReviewCommand, DescriptionReviewError> {
    if command.protocol != COMMAND_PROTOCOL {
        return Err(error(
            "schema.invalid",
            "the command protocol is not the description review command",
        ));
    }
    require_sha(&command.expected_outcome_sha256)?;
    require_sha(&command.current.schema_hash)?;
    require_sha(&command.current.evidence_hash)?;
    let mut seen = BTreeSet::new();
    let mut decisions = Vec::with_capacity(command.decisions.len());
    for item in &command.decisions {
        let decision = parse_decision(item)?;
        if !seen.insert(decision.dataset.clone()) {
            return Err(error(
                "decision.duplicate",
                "a dataset has more than one decision",
            ));
        }
        decisions.push(decision);
    }
    let mut column_seen = BTreeSet::new();
    let mut column_decisions = Vec::with_capacity(command.column_decisions.len());
    for item in &command.column_decisions {
        let decision = parse_column_decision(item)?;
        if !column_seen.insert((decision.dataset.clone(), decision.column.clone())) {
            return Err(error(
                "decision.duplicate",
                "a column has more than one decision",
            ));
        }
        column_decisions.push(decision);
    }
    Ok(ReviewCommand {
        current_schema: command.current.schema_hash.clone(),
        current_evidence: command.current.evidence_hash.clone(),
        decisions,
        column_decisions,
    })
}

fn parse_column_decision(value: &Value) -> Result<ColumnDecision, DescriptionReviewError> {
    let map = value.as_object().ok_or_else(shape)?;
    if !map
        .keys()
        .all(|key| key == "fields" || COLUMN_DECISION_KEYS.contains(&key.as_str()))
        || COLUMN_DECISION_KEYS.iter().any(|key| !map.contains_key(*key))
    {
        return Err(if COLUMN_DECISION_KEYS.iter().any(|key| !map.contains_key(*key)) {
            error(
                "schema.invalid",
                "the review command is missing a required field",
            )
        } else {
            error("schema.invalid", "the review command has an unknown field")
        });
    }
    let dataset = require_string(map, "dataset")?;
    if !is_qualified_table(dataset) {
        return Err(error(
            "schema.invalid",
            "a decision dataset is not a qualified table",
        ));
    }
    let column = require_string(map, "column")?;
    if column.is_empty() || column.contains('.') || column.contains('\0') {
        return Err(error(
            "schema.invalid",
            "a decision column is not a column name",
        ));
    }
    let attnum = map.get("attnum").and_then(Value::as_i64).ok_or_else(shape)?;
    let kind = match require_string(map, "decision")? {
        "approve" => Kind::Approve,
        "reject" => Kind::Reject,
        "defer" => Kind::Defer,
        _ => {
            return Err(error(
                "schema.invalid",
                "the decision is not approve, reject, or defer",
            ));
        }
    };
    let reviewer_label = require_string(map, "reviewer_label")?;
    check_reviewer(reviewer_label)?;
    let reviewer_at = require_string(map, "reviewer_at")?;
    CivilDate::new(reviewer_at).map_err(|_| {
        error("reviewer.date", "the reviewer date is not a civil date")
    })?;
    let (label, description) = parse_column_choice(&kind, map.get("fields"))?;
    Ok(ColumnDecision {
        dataset: dataset.to_owned(),
        column: column.to_owned(),
        attnum,
        kind,
        reviewer_label: reviewer_label.to_owned(),
        reviewer_at: reviewer_at.to_owned(),
        request_sha256: require_sha_field(map, "request_sha256")?,
        card_sha256: require_sha_field(map, "card_sha256")?,
        schema_hash: require_sha_field(map, "schema_hash")?,
        evidence_hash: require_sha_field(map, "evidence_hash")?,
        candidate_sha256: require_sha_field(map, "candidate_sha256")?,
        curated_sha256: require_sha_field(map, "curated_sha256")?,
        label,
        description,
    })
}

fn parse_column_choice(
    kind: &Kind,
    fields: Option<&Value>,
) -> Result<(FieldMode, FieldMode), DescriptionReviewError> {
    let present = fields.is_some();
    match kind {
        Kind::Approve if !present => {
            return Err(error("schema.invalid", "an approval requires fields"));
        }
        Kind::Reject | Kind::Defer if present => {
            return Err(error(
                "schema.invalid",
                "reject and defer do not take fields",
            ));
        }
        Kind::Reject | Kind::Defer => return Ok((FieldMode::Omit, FieldMode::Omit)),
        Kind::Approve => {}
    }
    let Some(value) = fields else {
        return Err(error("schema.invalid", "an approval requires fields"));
    };
    let parsed: ColumnFieldsIn = serde_json::from_value(value.clone()).map_err(|_| {
        error(
            "schema.invalid",
            "an approval field is not label or description",
        )
    })?;
    Ok((field_mode(parsed.label)?, field_mode(parsed.description)?))
}

fn parse_decision(value: &Value) -> Result<Decision, DescriptionReviewError> {
    let map = value.as_object().ok_or_else(shape)?;
    if !decision_keys(map) {
        return Err(if missing_decision_key(map) {
            error(
                "schema.invalid",
                "the review command is missing a required field",
            )
        } else {
            error("schema.invalid", "the review command has an unknown field")
        });
    }
    let dataset = require_string(map, "dataset")?;
    if !is_qualified_table(dataset) {
        return Err(error(
            "schema.invalid",
            "a decision dataset is not a qualified table",
        ));
    }
    let kind = match require_string(map, "decision")? {
        "approve" => Kind::Approve,
        "reject" => Kind::Reject,
        "defer" => Kind::Defer,
        _ => {
            return Err(error(
                "schema.invalid",
                "the decision is not approve, reject, or defer",
            ));
        }
    };
    let reviewer_label = require_string(map, "reviewer_label")?;
    check_reviewer(reviewer_label)?;
    let reviewer_at = require_string(map, "reviewer_at")?;
    CivilDate::new(reviewer_at).map_err(|_| {
        error("reviewer.date", "the reviewer date is not a civil date")
    })?;
    let request_sha256 = require_sha_field(map, "request_sha256")?;
    let card_sha256 = require_sha_field(map, "card_sha256")?;
    let schema_hash = require_sha_field(map, "schema_hash")?;
    let evidence_hash = require_sha_field(map, "evidence_hash")?;
    let candidate_sha256 = require_sha_field(map, "candidate_sha256")?;
    let curated_sha256 = require_sha_field(map, "curated_sha256")?;
    let (label, description, grain) = parse_choice(&kind, map.get("fields"))?;
    Ok(Decision {
        dataset: dataset.to_owned(),
        kind,
        reviewer_label: reviewer_label.to_owned(),
        reviewer_at: reviewer_at.to_owned(),
        request_sha256,
        card_sha256,
        schema_hash,
        evidence_hash,
        candidate_sha256,
        curated_sha256,
        label,
        description,
        grain,
    })
}

fn decision_keys(map: &Map<String, Value>) -> bool {
    map.keys().all(|key| key == "fields" || DECISION_KEYS.contains(&key.as_str()))
        && DECISION_KEYS.iter().all(|key| map.contains_key(*key))
}

fn missing_decision_key(map: &Map<String, Value>) -> bool {
    DECISION_KEYS.iter().any(|key| !map.contains_key(*key))
}

fn parse_choice(
    kind: &Kind,
    fields: Option<&Value>,
) -> Result<(FieldMode, FieldMode, FieldMode), DescriptionReviewError> {
    let present = fields.is_some();
    match kind {
        Kind::Approve if !present => {
            return Err(error("schema.invalid", "an approval requires fields"));
        }
        Kind::Reject | Kind::Defer if present => {
            return Err(error(
                "schema.invalid",
                "reject and defer do not take fields",
            ));
        }
        Kind::Reject | Kind::Defer => {
            return Ok((FieldMode::Omit, FieldMode::Omit, FieldMode::Omit));
        }
        Kind::Approve => {}
    }
    let Some(value) = fields else {
        return Err(error("schema.invalid", "an approval requires fields"));
    };
    let parsed: FieldsIn = serde_json::from_value(value.clone()).map_err(|_| {
        error(
            "schema.invalid",
            "an approval field is not label, description, or grain",
        )
    })?;
    Ok((
        field_mode(parsed.label)?,
        field_mode(parsed.description)?,
        field_mode(parsed.grain)?,
    ))
}

fn field_mode(field: Option<FieldIn>) -> Result<FieldMode, DescriptionReviewError> {
    let Some(field) = field else {
        return Ok(FieldMode::Omit);
    };
    match (field.overwrite, field.current_sha256) {
        (false, None) => Ok(FieldMode::KeepFill),
        (true, Some(hash)) if is_sha256(&hash) => Ok(FieldMode::Overwrite(hash)),
        (true, Some(_)) => Err(error(
            "schema.invalid",
            "a hash is not 64 lowercase hexadecimal characters",
        )),
        (true, None) | (false, Some(_)) => Err(error(
            "field.cas",
            "overwrite and the current value hash do not match",
        )),
    }
}

fn check_reviewer(label: &str) -> Result<(), DescriptionReviewError> {
    let count = label.chars().count();
    if !(1..=256).contains(&count) {
        return Err(error(
            "reviewer.length",
            "the reviewer label length is outside 1 to 256",
        ));
    }
    if Label::new(label).is_err() {
        return Err(error(
            "reviewer.controls",
            "the reviewer label contains a forbidden control",
        ));
    }
    Ok(())
}

fn load_curated(
    curated: &Map<String, Value>,
) -> Result<BTreeMap<String, Loaded>, DescriptionReviewError> {
    let mut out = BTreeMap::new();
    for (key, value) in curated {
        if !is_qualified_table(key) {
            return Err(error(
                "curated.identity",
                "a curated key is not that dataset",
            ));
        }
        let file: DatasetFile = serde_json::from_value(value.clone()).map_err(|_| {
            error(
                "schema.invalid",
                "a curated dataset is not a core dataset file",
            )
        })?;
        if file.dataset.as_str() != key {
            return Err(error(
                "curated.identity",
                "a curated key is not that dataset",
            ));
        }
        let document = serde_json::to_value(&file).map_err(|_| {
            error(
                "schema.invalid",
                "a curated dataset is not a core dataset file",
            )
        })?;
        let preimage = canonical_sha256(&document);
        out.insert(
            key.clone(),
            Loaded {
                file,
                document,
                preimage,
            },
        );
    }
    Ok(out)
}

fn parse_outcome(bytes: &str) -> Result<Outcome, DescriptionReviewError> {
    let value = parse_json_value(bytes, DESCRIPTION_CANDIDATE_PROTOCOL).map_err(|_| {
        error(
            "compiler.schema",
            "the compiler outcome is not the candidate document",
        )
    })?;
    let map = value.as_object().ok_or_else(compiler_shape)?;
    let protocol = map.get("protocol").and_then(Value::as_str).unwrap_or("");
    let column_mode = match protocol {
        DESCRIPTION_CANDIDATE_PROTOCOL => false,
        CANDIDATE_PROTOCOL_V2 => true,
        _ => return Err(compiler_shape()),
    };
    let keys = if column_mode { OUTCOME_KEYS_V2 } else { OUTCOME_KEYS };
    if !exact_keys(map, keys)
        || map.get("compiler").and_then(Value::as_str) != Some(DESCRIPTION_CANDIDATE_COMPILER)
    {
        return Err(compiler_shape());
    }
    let name = map
        .get("outcome")
        .and_then(Value::as_str)
        .ok_or_else(compiler_shape)?
        .to_owned();
    let is_candidates = name == "candidates";
    if !is_candidates
        && !matches!(
            name.as_str(),
            "budget_failure" | "truncated" | "incomplete" | "provider_failure" | "format_rejected"
        )
    {
        return Err(compiler_shape());
    }
    let refusal = json_text_or_null(map.get("refusal"))?;
    let budget_reason = json_text_or_null(map.get("budget_reason"))?;
    let request_sha256 = match map.get("request_sha256") {
        Some(Value::Null) => None,
        Some(Value::String(text)) if is_sha256(text) => Some(text.clone()),
        _ => return Err(compiler_shape()),
    };
    if is_candidates && request_sha256.is_none() {
        return Err(compiler_shape());
    }
    let statements = map.get("statements").ok_or_else(compiler_shape)?;
    let statement_map = statements.as_object().ok_or_else(compiler_shape)?;
    if !exact_keys(statement_map, STATEMENT_KEYS) {
        return Err(compiler_shape());
    }
    let draft = map
        .get("structural_draft")
        .and_then(Value::as_object)
        .ok_or_else(compiler_shape)?;
    if !exact_keys(draft, DRAFT_KEYS) {
        return Err(compiler_shape());
    }
    if draft.get("protocol").and_then(Value::as_str) != Some(DRAFT_PROTOCOL) {
        return Err(compiler_shape());
    }
    let schema_hash = require_draft_string(draft, "schema_hash")?;
    let evidence_hash = require_draft_string(draft, "evidence_hash")?;
    let selected = string_list(map.get("selected_datasets"))?;
    let candidates = parse_candidates(map.get("candidates"), is_candidates)?;
    if is_candidates {
        if !same_dataset_set(&selected, &candidates) {
            return Err(compiler_shape());
        }
    } else if !candidates.is_empty() {
        return Err(compiler_shape());
    }
    let columns = if column_mode {
        parse_columns(map.get("columns"), is_candidates)?
    } else {
        BTreeMap::new()
    };
    Ok(Outcome {
        name,
        refusal,
        budget_reason,
        request_sha256,
        statements: statements.clone(),
        selected,
        candidates,
        columns,
        schema_hash,
        evidence_hash,
        is_candidates,
        column_mode,
    })
}

fn parse_columns(
    value: Option<&Value>,
    is_candidates: bool,
) -> Result<BTreeMap<(String, String), ColumnCandidate>, DescriptionReviewError> {
    let items = value.and_then(Value::as_array).ok_or_else(compiler_shape)?;
    if !is_candidates && !items.is_empty() {
        return Err(compiler_shape());
    }
    let mut out = BTreeMap::new();
    for item in items {
        let candidate = parse_column_candidate(item)?;
        let key = (candidate.dataset.clone(), candidate.column.clone());
        if out.insert(key, candidate).is_some() {
            return Err(compiler_shape());
        }
    }
    Ok(out)
}

fn parse_column_candidate(value: &Value) -> Result<ColumnCandidate, DescriptionReviewError> {
    let map = value.as_object().ok_or_else(compiler_shape)?;
    if !exact_keys(map, COLUMN_KEYS) {
        return Err(compiler_shape());
    }
    let attnum = map.get("attnum").and_then(Value::as_i64).ok_or_else(compiler_shape)?;
    Ok(ColumnCandidate {
        dataset: require_string(map, "dataset")?.to_owned(),
        column: require_string(map, "column")?.to_owned(),
        attnum,
        ident: require_string(map, "ident")?.to_owned(),
        card_sha256: require_string(map, "card_sha256")?.to_owned(),
        label: require_string(map, "label")?.to_owned(),
        description: require_string(map, "description")?.to_owned(),
        confidence: require_string(map, "confidence")?.to_owned(),
        caveats: string_list(map.get("caveats"))?,
        open_questions: string_list(map.get("open_questions"))?,
        effect: require_string(map, "effect")?.to_owned(),
        object_sha: canonical_sha256(value),
    })
}

fn parse_candidates(
    value: Option<&Value>,
    is_candidates: bool,
) -> Result<BTreeMap<String, Candidate>, DescriptionReviewError> {
    let items = value.and_then(Value::as_array).ok_or_else(compiler_shape)?;
    if !is_candidates && !items.is_empty() {
        return Err(compiler_shape());
    }
    let mut out = BTreeMap::new();
    for item in items {
        let candidate = parse_candidate(item)?;
        if out.insert(candidate.dataset.clone(), candidate).is_some() {
            return Err(compiler_shape());
        }
    }
    Ok(out)
}

fn parse_candidate(value: &Value) -> Result<Candidate, DescriptionReviewError> {
    let map = value.as_object().ok_or_else(compiler_shape)?;
    if !exact_keys(map, CANDIDATE_KEYS) {
        return Err(compiler_shape());
    }
    let dataset = require_string(map, "dataset")?.to_owned();
    let card_sha256 = require_string(map, "card_sha256")?.to_owned();
    let label = require_string(map, "label")?.to_owned();
    let description = require_string(map, "description")?.to_owned();
    let grain = require_string(map, "grain")?.to_owned();
    let confidence = require_string(map, "confidence")?.to_owned();
    let effect = require_string(map, "effect")?.to_owned();
    let caveats = string_list(map.get("caveats"))?;
    let open_questions = string_list(map.get("open_questions"))?;
    Ok(Candidate {
        dataset,
        card_sha256,
        label,
        description,
        grain,
        caveats,
        open_questions,
        confidence,
        effect,
        object_sha: canonical_sha256(value),
    })
}

fn check_effects(outcome: &Outcome) -> Result<(), DescriptionReviewError> {
    if outcome
        .candidates
        .values()
        .any(|candidate| candidate.effect != "review_only")
        || outcome
            .columns
            .values()
            .any(|candidate| candidate.effect != "review_only")
    {
        return Err(error(
            "candidate.effect",
            "a candidate effect is not review_only",
        ));
    }
    Ok(())
}

fn check_known_columns(
    decisions: &[ColumnDecision],
    outcome: &Outcome,
) -> Result<(), DescriptionReviewError> {
    for decision in decisions {
        let key = (decision.dataset.clone(), decision.column.clone());
        match outcome.columns.get(&key) {
            Some(candidate) if candidate.attnum == decision.attnum => {}
            _ => {
                return Err(error(
                    "decision.unknown",
                    "a column decision does not match a column candidate",
                ));
            }
        }
    }
    Ok(())
}

fn check_known_decisions(
    decisions: &[Decision],
    outcome: &Outcome,
) -> Result<(), DescriptionReviewError> {
    for decision in decisions {
        if !outcome.candidates.contains_key(&decision.dataset) {
            return Err(error(
                "decision.unknown",
                "a decision names a dataset outside the candidate set",
            ));
        }
    }
    Ok(())
}

fn build_review(
    command: &ReviewCommand,
    curated: &BTreeMap<String, Loaded>,
    outcome: &Outcome,
    outcome_sha: &str,
) -> Map<String, Value> {
    let mut docs: BTreeMap<String, Loaded> = curated
        .iter()
        .map(|(key, loaded)| {
            (
                key.clone(),
                Loaded {
                    file: loaded.file.clone(),
                    document: loaded.document.clone(),
                    preimage: loaded.preimage.clone(),
                },
            )
        })
        .collect();
    let mut records = Vec::new();
    let mut queue = Vec::new();
    if outcome.is_candidates {
        review_candidates(command, &mut docs, outcome, &mut records, &mut queue);
    } else {
        for decision in &command.decisions {
            records.push(decision_record(decision, None, false, UNCHANGED));
        }
        queue.push(QueueItem {
            dataset: String::new(),
            column: None,
            kind: "compiler",
            reason: outcome.name.clone(),
        });
    }
    let mut column_records = Vec::new();
    if outcome.column_mode {
        review_columns(command, &mut docs, outcome, &mut column_records, &mut queue);
    }
    records.sort_by(|left, right| json_dataset(left).cmp(json_dataset(right)));
    column_records.sort_by(|left, right| {
        (
            json_dataset(left),
            json_text(left, "column"),
            json_attnum(left),
        )
            .cmp(&(
                json_dataset(right),
                json_text(right, "column"),
                json_attnum(right),
            ))
    });
    queue.sort_by(|left, right| {
        (
            left.dataset.as_str(),
            left.column.as_deref().unwrap_or(""),
            left.kind,
            left.reason.as_str(),
        )
            .cmp(&(
                right.dataset.as_str(),
                right.column.as_deref().unwrap_or(""),
                right.kind,
                right.reason.as_str(),
            ))
    });
    let mut datasets = Vec::new();
    for (name, loaded) in &docs {
        let hash = canonical_sha256(&loaded.document);
        datasets.push(serde_json::json!({
            "changed": hash != loaded.preimage,
            "dataset": name,
            "document": loaded.document,
            "document_sha256": hash,
        }));
    }
    let mut map = Map::new();
    map.insert("compiler".to_owned(), Value::String(REVIEW_COMPILER.to_owned()));
    map.insert(
        "compiler_budget_reason".to_owned(),
        outcome.budget_reason.clone(),
    );
    map.insert("compiler_outcome".to_owned(), Value::String(outcome.name.clone()));
    map.insert("compiler_refusal".to_owned(), outcome.refusal.clone());
    map.insert("compiler_statements".to_owned(), outcome.statements.clone());
    map.insert(
        "datasets".to_owned(),
        Value::Array(datasets),
    );
    if outcome.column_mode {
        map.insert(
            "column_decisions".to_owned(),
            Value::Array(column_records),
        );
    }
    map.insert("decisions".to_owned(), Value::Array(records));
    map.insert(
        "outcome".to_owned(),
        Value::String(if outcome.is_candidates {
            "reviewed".to_owned()
        } else {
            "not_candidates".to_owned()
        }),
    );
    map.insert(
        "outcome_sha256".to_owned(),
        Value::String(outcome_sha.to_owned()),
    );
    map.insert(
        "protocol".to_owned(),
        Value::String(
            if outcome.column_mode {
                REVIEW_PROTOCOL_V2
            } else {
                REVIEW_PROTOCOL
            }
            .to_owned(),
        ),
    );
    map.insert("queue".to_owned(), Value::Array(queue.into_iter().map(queue_value).collect()));
    map.insert(
        "request_sha256".to_owned(),
        match &outcome.request_sha256 {
            Some(hash) => Value::String(hash.clone()),
            None => Value::Null,
        },
    );
    map.insert("statements".to_owned(), review_statements());
    map
}

struct ColumnDispositions {
    label: &'static str,
    description: &'static str,
}

const UNCHANGED_COLUMN: ColumnDispositions = ColumnDispositions {
    label: "unchanged",
    description: "unchanged",
};

fn review_columns(
    command: &ReviewCommand,
    docs: &mut BTreeMap<String, Loaded>,
    outcome: &Outcome,
    records: &mut Vec<Value>,
    queue: &mut Vec<QueueItem>,
) {
    if !outcome.is_candidates {
        for decision in &command.column_decisions {
            records.push(column_record(decision, None, false, UNCHANGED_COLUMN));
        }
        return;
    }
    let keys: Vec<(String, String)> = outcome.columns.keys().cloned().collect();
    for key in keys {
        consider_column(command, docs, outcome, &key, records, queue);
    }
}

fn consider_column(
    command: &ReviewCommand,
    docs: &mut BTreeMap<String, Loaded>,
    outcome: &Outcome,
    key: &(String, String),
    records: &mut Vec<Value>,
    queue: &mut Vec<QueueItem>,
) {
    let Some(candidate) = outcome.columns.get(key) else {
        return;
    };
    let decision_index = command.column_decisions.iter().position(|item| {
        item.dataset == candidate.dataset && item.column == candidate.column
    });
    let dataset = candidate.dataset.clone();
    let column_name = candidate.column.clone();
    let ident = candidate.ident.clone();
    let missing = docs
        .get(&dataset)
        .and_then(|loaded| column_object(&loaded.document, &ident))
        .is_none();
    if missing {
        queue.push(column_item(&dataset, &column_name, "pending", "column_missing"));
        push_column_context(queue, candidate);
        if let Some(index) = decision_index {
            records.push(column_record(
                &command.column_decisions[index],
                Some(candidate),
                false,
                UNCHANGED_COLUMN,
            ));
        }
        return;
    }
    let label_now = docs
        .get(&dataset)
        .and_then(|loaded| column_text(&loaded.document, &ident, "label"));
    let description_now = docs
        .get(&dataset)
        .and_then(|loaded| column_text(&loaded.document, &ident, "description"));
    let preimage = docs
        .get(&dataset)
        .map(|loaded| loaded.preimage.clone())
        .unwrap_or_default();
    let decision = decision_index.map(|index| &command.column_decisions[index]);
    if let Some(reason) = column_stale(
        command,
        outcome,
        decision,
        candidate,
        &preimage,
        label_now.as_deref(),
        description_now.as_deref(),
    ) {
        queue.push(column_item(&dataset, &column_name, "stale", reason));
        push_column_context(queue, candidate);
        if let Some(decision) = decision {
            records.push(column_record(decision, Some(candidate), false, UNCHANGED_COLUMN));
        }
        return;
    }
    let Some(decision) = decision else {
        queue.push(column_item(&dataset, &column_name, "pending", "decision_missing"));
        push_column_context(queue, candidate);
        return;
    };
    match decision.kind {
        Kind::Reject => {
            push_column_context(queue, candidate);
            records.push(column_record(decision, Some(candidate), false, UNCHANGED_COLUMN));
        }
        Kind::Defer => {
            queue.push(column_item(&dataset, &column_name, "pending", "deferred"));
            push_column_context(queue, candidate);
            records.push(column_record(decision, Some(candidate), false, UNCHANGED_COLUMN));
        }
        Kind::Approve => approve_column(
            docs,
            decision,
            candidate,
            label_now.as_deref(),
            description_now.as_deref(),
            records,
            queue,
        ),
    }
}

fn column_stale(
    command: &ReviewCommand,
    outcome: &Outcome,
    decision: Option<&ColumnDecision>,
    candidate: &ColumnCandidate,
    preimage: &str,
    label_now: Option<&str>,
    description_now: Option<&str>,
) -> Option<&'static str> {
    if command.current_schema != outcome.schema_hash {
        return Some("schema");
    }
    if command.current_evidence != outcome.evidence_hash {
        return Some("evidence");
    }
    let decision = decision?;
    if decision.schema_hash != command.current_schema {
        return Some("schema");
    }
    if decision.evidence_hash != command.current_evidence {
        return Some("evidence");
    }
    if decision.request_sha256 != outcome.request_sha256.as_deref().unwrap_or("") {
        return Some("request");
    }
    if decision.card_sha256 != candidate.card_sha256 {
        return Some("card");
    }
    if decision.candidate_sha256 != candidate.object_sha {
        return Some("candidate");
    }
    if decision.curated_sha256 != preimage {
        return Some("curated");
    }
    if mismatch(&decision.label, label_now) || mismatch(&decision.description, description_now) {
        return Some("cas");
    }
    None
}

fn approve_column(
    docs: &mut BTreeMap<String, Loaded>,
    decision: &ColumnDecision,
    candidate: &ColumnCandidate,
    label_now: Option<&str>,
    description_now: Option<&str>,
    records: &mut Vec<Value>,
    queue: &mut Vec<QueueItem>,
) {
    let label = match plan_field(&decision.label, label_now, &candidate.label, FieldKind::Label) {
        Ok(plan) => plan,
        Err(reason) => {
            queue.push(column_item(
                &candidate.dataset,
                &candidate.column,
                "core_rejected",
                reason,
            ));
            push_column_context(queue, candidate);
            records.push(column_record(decision, Some(candidate), false, UNCHANGED_COLUMN));
            return;
        }
    };
    let description = match plan_field(
        &decision.description,
        description_now,
        &candidate.description,
        FieldKind::Description,
    ) {
        Ok(plan) => plan,
        Err(reason) => {
            queue.push(column_item(
                &candidate.dataset,
                &candidate.column,
                "core_rejected",
                reason,
            ));
            push_column_context(queue, candidate);
            records.push(column_record(decision, Some(candidate), false, UNCHANGED_COLUMN));
            return;
        }
    };
    let dispositions = ColumnDispositions {
        label: label.disposition,
        description: description.disposition,
    };
    if label.write.is_none() && description.write.is_none() {
        push_column_context(queue, candidate);
        records.push(column_record(decision, Some(candidate), false, dispositions));
        return;
    }
    let Some(loaded) = docs.get_mut(&candidate.dataset) else {
        return;
    };
    match commit_column(
        &loaded.document,
        &candidate.ident,
        label.write.as_deref(),
        description.write.as_deref(),
    ) {
        Ok((file, document)) => {
            loaded.file = file;
            loaded.document = document;
            push_column_context(queue, candidate);
            records.push(column_record(decision, Some(candidate), true, dispositions));
        }
        Err(reason) => {
            queue.push(column_item(
                &candidate.dataset,
                &candidate.column,
                "core_rejected",
                reason,
            ));
            push_column_context(queue, candidate);
            records.push(column_record(decision, Some(candidate), false, UNCHANGED_COLUMN));
        }
    }
}

fn commit_column(
    document: &Value,
    ident: &str,
    label: Option<&str>,
    description: Option<&str>,
) -> Result<(DatasetFile, Value), &'static str> {
    let mut next = document.clone();
    let columns = next
        .as_object_mut()
        .and_then(|object| object.get_mut("columns"))
        .and_then(Value::as_array_mut)
        .ok_or("column_missing")?;
    let column = columns
        .iter_mut()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(ident))
        .and_then(Value::as_object_mut)
        .ok_or("column_missing")?;
    if let Some(text) = label {
        column.insert("label".to_owned(), Value::String(text.to_owned()));
    }
    if let Some(text) = description {
        column.insert("description".to_owned(), Value::String(text.to_owned()));
    }
    let parsed: DatasetFile = serde_json::from_value(next).map_err(|_| "core_schema")?;
    let document = serde_json::to_value(&parsed).map_err(|_| "core_schema")?;
    Ok((parsed, document))
}

fn column_object<'a>(document: &'a Value, ident: &str) -> Option<&'a Value> {
    document
        .get("columns")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some(ident))
        })
}

fn column_text(document: &Value, ident: &str, field: &str) -> Option<String> {
    match column_object(document, ident)?.get(field) {
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn push_column_context(queue: &mut Vec<QueueItem>, candidate: &ColumnCandidate) {
    if !candidate.caveats.is_empty() {
        queue.push(column_item(
            &candidate.dataset,
            &candidate.column,
            "pending",
            "caveats",
        ));
    }
    if !candidate.open_questions.is_empty() {
        queue.push(column_item(
            &candidate.dataset,
            &candidate.column,
            "pending",
            "open_questions",
        ));
    }
}

fn column_record(
    decision: &ColumnDecision,
    candidate: Option<&ColumnCandidate>,
    applied: bool,
    dispositions: ColumnDispositions,
) -> Value {
    let kind = match decision.kind {
        Kind::Approve => "approve",
        Kind::Reject => "reject",
        Kind::Defer => "defer",
    };
    serde_json::json!({
        "applied": applied,
        "attnum": candidate.map(|item| item.attnum).unwrap_or(decision.attnum),
        "authenticated": false,
        "candidate_caveats": candidate.map(|item| string_values(&item.caveats)).unwrap_or_else(|| Value::Array(Vec::new())),
        "candidate_confidence": candidate.map(|item| Value::String(item.confidence.clone())).unwrap_or(Value::Null),
        "candidate_description": candidate.map(|item| Value::String(item.description.clone())).unwrap_or(Value::Null),
        "candidate_label": candidate.map(|item| Value::String(item.label.clone())).unwrap_or(Value::Null),
        "candidate_open_questions": candidate.map(|item| string_values(&item.open_questions)).unwrap_or_else(|| Value::Array(Vec::new())),
        "candidate_sha256": decision.candidate_sha256,
        "card_sha256": decision.card_sha256,
        "column": decision.column,
        "confidence_effect": "recorded_only",
        "curated_sha256": decision.curated_sha256,
        "dataset": decision.dataset,
        "decision": kind,
        "description_disposition": dispositions.description,
        "effect": candidate.map(|_| Value::String("review_only".to_owned())).unwrap_or(Value::Null),
        "evidence_hash": decision.evidence_hash,
        "ident": candidate.map(|item| Value::String(item.ident.clone())).unwrap_or(Value::Null),
        "label_disposition": dispositions.label,
        "request_sha256": decision.request_sha256,
        "reviewer_at": decision.reviewer_at,
        "reviewer_label": decision.reviewer_label,
        "schema_hash": decision.schema_hash,
    })
}

struct Facts<'a> {
    command: &'a ReviewCommand,
    outcome: &'a Outcome,
    request: &'a str,
    candidate: &'a Candidate,
    decision: Option<&'a Decision>,
    preimage: &'a str,
    label: Option<&'a str>,
    description: Option<&'a str>,
    grain: Option<&'a str>,
}

fn review_candidates(
    command: &ReviewCommand,
    docs: &mut BTreeMap<String, Loaded>,
    outcome: &Outcome,
    records: &mut Vec<Value>,
    queue: &mut Vec<QueueItem>,
) {
    let selected = outcome.selected.clone();
    for dataset in &selected {
        if !outcome.candidates.contains_key(dataset) {
            continue;
        }
        let decision_index = command
            .decisions
            .iter()
            .position(|item| &item.dataset == dataset);
        consider(
            command,
            docs,
            outcome,
            dataset,
            decision_index,
            records,
            queue,
        );
    }
}

fn consider(
    command: &ReviewCommand,
    docs: &mut BTreeMap<String, Loaded>,
    outcome: &Outcome,
    dataset: &str,
    decision_index: Option<usize>,
    records: &mut Vec<Value>,
    queue: &mut Vec<QueueItem>,
) {
    let request_owned = outcome.request_sha256.clone().unwrap_or_default();
    if !docs.contains_key(dataset) {
        let Some(candidate) = outcome.candidates.get(dataset) else {
            return;
        };
        queue.push(item(dataset, "pending", "curated_missing"));
        push_context(queue, candidate);
        if let Some(index) = decision_index {
            let decision = &command.decisions[index];
            records.push(decision_record(decision, Some(candidate), false, UNCHANGED));
        }
        return;
    }
    let label_owned = docs[dataset].file.label.as_str().to_owned();
    let description_owned = docs[dataset]
        .file
        .description
        .as_ref()
        .map(|text| text.as_str().to_owned());
    let grain_owned = docs[dataset]
        .file
        .grain
        .as_ref()
        .map(|text| text.as_str().to_owned());
    let preimage = docs[dataset].preimage.clone();
    let facts = Facts {
        command,
        outcome,
        request: &request_owned,
        candidate: outcome.candidates.get(dataset).unwrap(),
        decision: decision_index.and_then(|index| command.decisions.get(index)),
        preimage: &preimage,
        label: Some(label_owned.as_str()),
        description: description_owned.as_deref(),
        grain: grain_owned.as_deref(),
    };
    if let Some(reason) = first_stale(&facts) {
        let candidate = facts.candidate;
        queue.push(item(&candidate.dataset, "stale", reason));
        push_context(queue, candidate);
        if let Some(decision) = facts.decision {
            records.push(decision_record(decision, Some(candidate), false, UNCHANGED));
        }
        return;
    }
    let Some(decision) = facts.decision else {
        queue.push(item(dataset, "pending", "decision_missing"));
        push_context(queue, facts.candidate);
        return;
    };
    match decision.kind {
        Kind::Reject => {
            push_context(queue, facts.candidate);
            records.push(decision_record(decision, Some(facts.candidate), false, UNCHANGED));
        }
        Kind::Defer => {
            queue.push(item(dataset, "pending", "deferred"));
            push_context(queue, facts.candidate);
            records.push(decision_record(decision, Some(facts.candidate), false, UNCHANGED));
        }
        Kind::Approve => approve_one(docs, &facts, records, queue),
    }
}

fn approve_one(
    docs: &mut BTreeMap<String, Loaded>,
    facts: &Facts<'_>,
    records: &mut Vec<Value>,
    queue: &mut Vec<QueueItem>,
) {
    let Some(decision) = facts.decision else {
        return;
    };
    let planned = match plan_writes(
        decision,
        facts.candidate,
        facts.label,
        facts.description,
        facts.grain,
    ) {
        Ok(planned) => planned,
        Err(reason) => {
            queue.push(item(&facts.candidate.dataset, "core_rejected", reason));
            push_context(queue, facts.candidate);
            records.push(decision_record(decision, Some(facts.candidate), false, UNCHANGED));
            return;
        }
    };
    if !planned.any_write {
        push_context(queue, facts.candidate);
        records.push(decision_record(
            decision,
            Some(facts.candidate),
            false,
            planned.dispositions,
        ));
        return;
    }
    let Some(loaded) = docs.get_mut(&facts.candidate.dataset) else {
        return;
    };
    match commit_writes(&loaded.document, &planned) {
        Ok(document) => {
            loaded.document = document;
            push_context(queue, facts.candidate);
            records.push(decision_record(
                decision,
                Some(facts.candidate),
                true,
                planned.dispositions,
            ));
        }
        Err(reason) => {
            queue.push(item(&facts.candidate.dataset, "core_rejected", reason));
            push_context(queue, facts.candidate);
            records.push(decision_record(decision, Some(facts.candidate), false, UNCHANGED));
        }
    }
}

struct Planned {
    label: Option<String>,
    description: Option<String>,
    grain: Option<String>,
    dispositions: Dispositions,
    any_write: bool,
}

struct Dispositions {
    label: &'static str,
    description: &'static str,
    grain: &'static str,
}

const UNCHANGED: Dispositions = Dispositions {
    label: "unchanged",
    description: "unchanged",
    grain: "unchanged",
};

struct FieldPlan {
    write: Option<String>,
    disposition: &'static str,
}

fn plan_writes(
    decision: &Decision,
    candidate: &Candidate,
    label_now: Option<&str>,
    description_now: Option<&str>,
    grain_now: Option<&str>,
) -> Result<Planned, &'static str> {
    let label = plan_field(&decision.label, label_now, &candidate.label, FieldKind::Label)?;
    let description = plan_field(
        &decision.description,
        description_now,
        &candidate.description,
        FieldKind::Description,
    )?;
    let grain = plan_field(&decision.grain, grain_now, &candidate.grain, FieldKind::Grain)?;
    let any_write =
        label.write.is_some() || description.write.is_some() || grain.write.is_some();
    Ok(Planned {
        label: label.write,
        description: description.write,
        grain: grain.write,
        dispositions: Dispositions {
            label: label.disposition,
            description: description.disposition,
            grain: grain.disposition,
        },
        any_write,
    })
}

enum FieldKind {
    Label,
    Grain,
    Description,
}

fn plan_field(
    mode: &FieldMode,
    current: Option<&str>,
    next: &str,
    kind: FieldKind,
) -> Result<FieldPlan, &'static str> {
    match mode {
        FieldMode::Omit => Ok(FieldPlan {
            write: None,
            disposition: "not_selected",
        }),
        FieldMode::KeepFill if current.is_some() => Ok(FieldPlan {
            write: None,
            disposition: "kept",
        }),
        FieldMode::KeepFill => {
            write_reason(kind, next)?;
            Ok(FieldPlan {
                write: Some(next.to_owned()),
                disposition: "filled",
            })
        }
        FieldMode::Overwrite(_) => {
            write_reason(kind, next)?;
            Ok(FieldPlan {
                write: Some(next.to_owned()),
                disposition: "replaced",
            })
        }
    }
}

fn write_reason(kind: FieldKind, text: &str) -> Result<(), &'static str> {
    let max = match kind {
        FieldKind::Label | FieldKind::Grain => 256,
        FieldKind::Description => 4_000,
    };
    let count = text.chars().count();
    if count == 0 || count > max {
        return Err("core_length");
    }
    let bad_control = text.chars().any(|ch| match kind {
        FieldKind::Description => ch == '\0',
        FieldKind::Label | FieldKind::Grain => {
            let code = u32::from(ch);
            matches!(code, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F)
        }
    });
    if bad_control {
        return Err("core_type");
    }
    if contains_forbidden_interpolation(text) {
        return Err("core_schema");
    }
    Ok(())
}

fn commit_writes(document: &Value, planned: &Planned) -> Result<Value, &'static str> {
    let mut next = document.clone();
    let obj = next.as_object_mut().ok_or("core_schema")?;
    if let Some(text) = &planned.label {
        obj.insert("label".to_owned(), Value::String(text.clone()));
    }
    if let Some(text) = &planned.description {
        obj.insert("description".to_owned(), Value::String(text.clone()));
    }
    if let Some(text) = &planned.grain {
        obj.insert("grain".to_owned(), Value::String(text.clone()));
    }
    let parsed: DatasetFile = serde_json::from_value(next).map_err(|_| "core_schema")?;
    serde_json::to_value(&parsed).map_err(|_| "core_schema")
}

fn first_stale(facts: &Facts<'_>) -> Option<&'static str> {
    if facts.command.current_schema != facts.outcome.schema_hash {
        return Some("schema");
    }
    if facts.command.current_evidence != facts.outcome.evidence_hash {
        return Some("evidence");
    }
    let decision = facts.decision?;
    if decision.schema_hash != facts.command.current_schema {
        return Some("schema");
    }
    if decision.evidence_hash != facts.command.current_evidence {
        return Some("evidence");
    }
    if decision.request_sha256 != facts.request {
        return Some("request");
    }
    if decision.card_sha256 != facts.candidate.card_sha256 {
        return Some("card");
    }
    if decision.candidate_sha256 != facts.candidate.object_sha {
        return Some("candidate");
    }
    if decision.curated_sha256 != facts.preimage {
        return Some("curated");
    }
    if cas_mismatch(decision, facts.label, facts.description, facts.grain) {
        return Some("cas");
    }
    None
}

fn cas_mismatch(
    decision: &Decision,
    label_now: Option<&str>,
    description_now: Option<&str>,
    grain_now: Option<&str>,
) -> bool {
    mismatch(&decision.label, label_now)
        || mismatch(&decision.description, description_now)
        || mismatch(&decision.grain, grain_now)
}

fn mismatch(mode: &FieldMode, current: Option<&str>) -> bool {
    match mode {
        FieldMode::Overwrite(expected) => expected != &descriptive_field_sha256(current),
        FieldMode::Omit | FieldMode::KeepFill => false,
    }
}

fn push_context(queue: &mut Vec<QueueItem>, candidate: &Candidate) {
    if !candidate.caveats.is_empty() {
        queue.push(item(&candidate.dataset, "pending", "caveats"));
    }
    if !candidate.open_questions.is_empty() {
        queue.push(item(&candidate.dataset, "pending", "open_questions"));
    }
}

fn decision_record(
    decision: &Decision,
    candidate: Option<&Candidate>,
    applied: bool,
    dispositions: Dispositions,
) -> Value {
    let kind = match decision.kind {
        Kind::Approve => "approve",
        Kind::Reject => "reject",
        Kind::Defer => "defer",
    };
    let effect = match candidate {
        Some(_) => Value::String("review_only".to_owned()),
        None => Value::Null,
    };
    serde_json::json!({
        "applied": applied,
        "authenticated": false,
        "candidate_caveats": candidate.map(|item| string_values(&item.caveats)).unwrap_or_else(|| Value::Array(Vec::new())),
        "candidate_confidence": candidate.map(|item| Value::String(item.confidence.clone())).unwrap_or(Value::Null),
        "candidate_description": candidate.map(|item| Value::String(item.description.clone())).unwrap_or(Value::Null),
        "candidate_grain": candidate.map(|item| Value::String(item.grain.clone())).unwrap_or(Value::Null),
        "candidate_label": candidate.map(|item| Value::String(item.label.clone())).unwrap_or(Value::Null),
        "candidate_open_questions": candidate.map(|item| string_values(&item.open_questions)).unwrap_or_else(|| Value::Array(Vec::new())),
        "candidate_sha256": decision.candidate_sha256,
        "card_sha256": decision.card_sha256,
        "confidence_effect": "recorded_only",
        "curated_sha256": decision.curated_sha256,
        "dataset": decision.dataset,
        "decision": kind,
        "description_disposition": dispositions.description,
        "effect": effect,
        "evidence_hash": decision.evidence_hash,
        "grain_disposition": dispositions.grain,
        "label_disposition": dispositions.label,
        "request_sha256": decision.request_sha256,
        "reviewer_at": decision.reviewer_at,
        "reviewer_label": decision.reviewer_label,
        "schema_hash": decision.schema_hash,
    })
}

fn review_statements() -> Value {
    serde_json::json!({
        "authentication": "not_claimed",
        "auto_accept": "not_authorized",
        "column_acl": "not_changed",
        "confidence_effect": "recorded_only",
        "database": "not_used",
        "effective": "not_written",
        "entity": "not_changed",
        "files": "not_written",
        "grants": "not_authorized",
        "metric": "not_changed",
        "owner_ui": "not_claimed",
        "pii_classification": "not_performed",
        "publication": "unavailable",
        "reviewer": "declared_only",
        "semantic_classification": "not_performed",
        "source_artifact": "not_mutated",
        "status": "not_changed",
    })
}

fn finish_review(map: Map<String, Value>) -> String {
    let hash = canonical_sha256(&Value::Object(map.clone()));
    let mut sealed = map;
    sealed.insert("review_sha256".to_owned(), Value::String(hash));
    canonical_json(&Value::Object(sealed))
}

fn queue_value(item: QueueItem) -> Value {
    let mut map = Map::new();
    if let Some(column) = item.column {
        map.insert("column".to_owned(), Value::String(column));
    }
    map.insert("dataset".to_owned(), Value::String(item.dataset));
    map.insert("kind".to_owned(), Value::String(item.kind.to_owned()));
    map.insert("reason".to_owned(), Value::String(item.reason));
    Value::Object(map)
}

fn item(dataset: &str, kind: &'static str, reason: &str) -> QueueItem {
    QueueItem {
        dataset: dataset.to_owned(),
        column: None,
        kind,
        reason: reason.to_owned(),
    }
}

fn column_item(dataset: &str, column: &str, kind: &'static str, reason: &str) -> QueueItem {
    QueueItem {
        dataset: dataset.to_owned(),
        column: Some(column.to_owned()),
        kind,
        reason: reason.to_owned(),
    }
}

fn json_text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn json_attnum(value: &Value) -> i64 {
    value.get("attnum").and_then(Value::as_i64).unwrap_or(0)
}

fn string_values(items: &[String]) -> Value {
    Value::Array(items.iter().cloned().map(Value::String).collect())
}

fn json_dataset(value: &Value) -> &str {
    value.get("dataset").and_then(Value::as_str).unwrap_or("")
}

fn map_scan(err: DiagnosticList) -> DescriptionReviewError {
    let code = err.items.first().map(|item| item.code.clone()).unwrap_or_else(|| {
        format!("{COMMAND_PROTOCOL}/json.syntax")
    });
    let name = code.rsplit('/').next().unwrap_or("json.syntax");
    let message = match name {
        "json.budget.bytes" => "document exceeds the byte budget",
        "json.budget.depth" => "JSON nesting exceeds the depth budget",
        "json.budget.nodes" => "JSON node count exceeds the budget",
        "json.duplicate_key" => "duplicate JSON object key",
        _ => "review command JSON is not valid",
    };
    DescriptionReviewError::from_compiler(code, message)
}

fn map_schema(err: serde_json::Error) -> DescriptionReviewError {
    let text = err.to_string();
    if text.contains("unknown field") {
        error("schema.invalid", "the review command has an unknown field")
    } else if text.contains("missing field") {
        error(
            "schema.invalid",
            "the review command is missing a required field",
        )
    } else {
        error("schema.invalid", "the review command is not the review shape")
    }
}

fn map_compiler(err: DescriptionCandidateError) -> DescriptionReviewError {
    DescriptionReviewError::from_compiler(err.code, err.message)
}

fn error(name: &'static str, message: &'static str) -> DescriptionReviewError {
    DescriptionReviewError::new(name, message)
}

fn shape() -> DescriptionReviewError {
    error("schema.invalid", "the review command is not the review shape")
}

fn compiler_shape() -> DescriptionReviewError {
    error(
        "compiler.schema",
        "the compiler outcome is not the candidate document",
    )
}

fn require_sha(value: &str) -> Result<(), DescriptionReviewError> {
    if is_sha256(value) {
        Ok(())
    } else {
        Err(error(
            "schema.invalid",
            "a hash is not 64 lowercase hexadecimal characters",
        ))
    }
}

fn require_sha_field(map: &Map<String, Value>, key: &str) -> Result<String, DescriptionReviewError> {
    let text = require_string(map, key)?;
    require_sha(text)?;
    Ok(text.to_owned())
}

fn require_string<'a>(
    map: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, DescriptionReviewError> {
    map.get(key).and_then(Value::as_str).ok_or_else(shape)
}

fn require_draft_string(map: &Map<String, Value>, key: &str) -> Result<String, DescriptionReviewError> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(compiler_shape)
}

fn json_text_or_null(value: Option<&Value>) -> Result<Value, DescriptionReviewError> {
    match value {
        Some(Value::Null) => Ok(Value::Null),
        Some(Value::String(text)) => Ok(Value::String(text.clone())),
        _ => Err(compiler_shape()),
    }
}

fn string_list(value: Option<&Value>) -> Result<Vec<String>, DescriptionReviewError> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Err(compiler_shape());
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(text) = item.as_str() else {
            return Err(compiler_shape());
        };
        out.push(text.to_owned());
    }
    Ok(out)
}

fn exact_keys(map: &Map<String, Value>, keys: &[&str]) -> bool {
    map.len() == keys.len() && keys.iter().all(|key| map.contains_key(*key))
}

fn same_dataset_set(selected: &[String], candidates: &BTreeMap<String, Candidate>) -> bool {
    if selected.len() != candidates.len() {
        return false;
    }
    let mut names = selected.to_vec();
    names.sort();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return false;
    }
    names.iter().all(|name| candidates.contains_key(name))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_qualified_table(value: &str) -> bool {
    let Some((schema, table)) = value.split_once('.') else {
        return false;
    };
    if schema.contains('.') || table.contains('.') {
        return false;
    }
    is_schema(schema) && is_table(table)
}

fn is_schema(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn is_table(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}
