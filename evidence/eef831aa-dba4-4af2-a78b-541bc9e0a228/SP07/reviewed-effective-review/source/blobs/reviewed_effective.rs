//! Explicit classification bridge into `oo.builder.effective-oos/v1`.
//!
//! A sealed `oo.builder.captured-pg-draft-command/v1` artifact is recompiled.
//! A column becomes classified only when a decision names that effective
//! column, repeats its schema hash and structural fingerprint, and carries a
//! PII level, a semantic type, and a reviewer. Curated echoes and
//! `classification.auto_accept` are not decisions. This module does not
//! connect, grant, authenticate, or publish.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;
use serde_json::Value;

use oo_core::config::{PiiLevel, SampleValues};
use oo_core::diagnostic::DiagnosticList;
use oo_core::json_scan::parse_json_value;
use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_core::oos::{
    CivilDate, ClassState, ClassificationRecord, DatasetFile, ItemStatus, OntologyFile,
    Relationship, RelationshipsFile, SemanticType, validate_dataset, validate_relationships,
};
use oo_core::text::{Label, TableName};

use crate::canonical::{canonical_json, canonical_sha256, sha256_hex};
use crate::captured_pg::{CAPTURED_COMPILER, CAPTURED_PROTOCOL};
use crate::draft::{self, DRAFT_PROTOCOL, compile_structure_draft, manifest_preimage_hash};
use crate::role_pack::EFFECTIVE_PROTOCOL;

pub const REVIEWED_EFFECTIVE_PROTOCOL: &str = "oo.builder.reviewed-effective-oos/v1";
pub const REVIEWED_EFFECTIVE_COMPILER: &str = "oo-builder/reviewed-effective-oos/v1";
pub const REVIEWED_EFFECTIVE_COMMAND_PROTOCOL: &str =
    "oo.builder.reviewed-effective-oos-command/v1";

const CAPTURED_COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const MAX_DECISIONS: usize = 4096;
const BINDING_MESSAGE: &str = "settings are outside the explicit version binding";
const SCHEMA_MESSAGE: &str = "include_schemas does not cover the effective datasets";
const DECISION_MESSAGE: &str = "classification decision is not a bound PII and semantic input";
const OOS_MESSAGE: &str = "effective OOS validation failed";

/// Static failure. The message does not echo the request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewedEffectiveError {
    pub code: String,
    pub path: String,
    pub message: &'static str,
}

impl ReviewedEffectiveError {
    /// Compiler failure. The code is `{reviewed-effective protocol}/{name}`.
    pub fn new(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{REVIEWED_EFFECTIVE_PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }

    /// Command-envelope failure. The code uses the command protocol.
    pub fn command(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{REVIEWED_EFFECTIVE_COMMAND_PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }

    pub fn from_full(code: String, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code,
            path: path.into(),
            message,
        }
    }
}

impl fmt::Display for ReviewedEffectiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}: {}",
            self.code, self.path, self.message
        )
    }
}

impl std::error::Error for ReviewedEffectiveError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandIn {
    protocol: String,
    artifact: Value,
    settings: Value,
    decisions: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsIn {
    project: String,
    ontology_version: String,
    catalog_version: String,
    policy: IdVersion,
    knowledge: IdVersion,
    ontology: OntologyFile,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdVersion {
    id: String,
    version: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionIn {
    dataset: String,
    column: String,
    schema_hash: String,
    fact_fingerprint: String,
    pii: PiiLevel,
    semantic_type: SemanticType,
    reviewer: ReviewerIn,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewerIn {
    by: Label,
    at: CivilDate,
}

struct SealedDraft {
    datasets: Vec<DatasetFile>,
    relationships: Vec<Value>,
    fingerprints: BTreeMap<(String, String), String>,
    schema_hash: String,
    draft_hash: String,
    wrapper_hash: String,
    generated_sha256: String,
    curated_sha256: String,
    sealed_source_sha256: String,
}

/// Compile one reviewed captured draft into a role-pack effective document.
///
/// `input` is one `oo.builder.reviewed-effective-oos-command/v1` document.
/// The returned text is canonical JSON with no trailing newline. A column
/// with no applied decision stays on its dataset and is listed in `hidden`.
pub fn compile_reviewed_effective(input: &str) -> Result<String, ReviewedEffectiveError> {
    let value = parse_json_value(input, REVIEWED_EFFECTIVE_COMMAND_PROTOCOL).map_err(map_scan)?;
    let command: CommandIn = serde_json::from_value(value).map_err(map_command)?;
    if command.protocol != REVIEWED_EFFECTIVE_COMMAND_PROTOCOL {
        return Err(ReviewedEffectiveError::command(
            "protocol.mismatch",
            "/protocol",
            "the command protocol is not the reviewed effective command this entry accepts",
        ));
    }
    if !command.artifact.is_object() || !command.settings.is_object() || !command.decisions.is_array()
    {
        return Err(ReviewedEffectiveError::command(
            "schema.invalid",
            "/",
            "artifact, settings, or decisions has the wrong JSON shape",
        ));
    }
    let sealed = open_sealed(&command.artifact)?;
    let settings: SettingsIn =
        serde_json::from_value(command.settings.clone()).map_err(map_settings)?;
    check_settings(&settings)?;
    let decisions = bind_decisions(&command.decisions)?;
    let (datasets, hidden) = apply_decisions(&sealed, &decisions)?;
    check_schemas(&settings, &datasets)?;
    validate_datasets(&datasets)?;
    let (relationships, omitted) =
        partition_relationships(&sealed.relationships, &column_index(&datasets))?;
    validate_relationship_file(&settings, &relationships)?;
    let effective = effective_document(&settings, &datasets, &relationships)?;
    let effective_text = canonical_json(&effective);
    if effective_text.len() > DOCUMENT_MAX_BYTES {
        return Err(ReviewedEffectiveError::new(
            "budget.bytes",
            "/effective_oos",
            "effective document exceeds the byte budget",
        ));
    }
    parse_json_value(&effective_text, EFFECTIVE_PROTOCOL).map_err(|_| {
        ReviewedEffectiveError::new(
            "oos.invalid",
            "/effective_oos",
            "effective document is not oo.builder.effective-oos/v1",
        )
    })?;
    let result = command_result(&sealed, &command.decisions, &effective, &hidden, &omitted);
    finish(canonical_json(&result))
}

fn map_scan(err: DiagnosticList) -> ReviewedEffectiveError {
    let item = err.items.first();
    let code = item.map(|diag| diag.code.clone()).unwrap_or_else(|| {
        format!("{REVIEWED_EFFECTIVE_COMMAND_PROTOCOL}/json.syntax")
    });
    let path = item
        .map(|diag| diag.path.clone())
        .unwrap_or_else(|| "/".to_string());
    let message = if code.ends_with("/json.budget.bytes") {
        "document exceeds the byte budget"
    } else if code.ends_with("/json.budget.depth") {
        "JSON nesting exceeds the depth budget"
    } else if code.ends_with("/json.budget.nodes") {
        "JSON node count exceeds the budget"
    } else if code.ends_with("/json.duplicate_key") {
        "duplicate JSON object key"
    } else {
        "reviewed effective command JSON is not valid"
    };
    ReviewedEffectiveError::from_full(code, path, message)
}

fn map_command(err: serde_json::Error) -> ReviewedEffectiveError {
    map_shape(
        err,
        "/",
        "reviewed effective command is not protocol, artifact, settings, and decisions",
    )
}

fn map_settings(err: serde_json::Error) -> ReviewedEffectiveError {
    let text = err.to_string();
    if text.contains("unknown field") {
        ReviewedEffectiveError::command(
            "field.unknown",
            "/settings",
            "settings has an unknown field",
        )
    } else if text.contains("missing field") {
        ReviewedEffectiveError::command(
            "field.missing",
            "/settings",
            "settings is missing a required field",
        )
    } else {
        ReviewedEffectiveError::new("settings.invalid", "/settings", BINDING_MESSAGE)
    }
}

fn map_decision(err: serde_json::Error) -> ReviewedEffectiveError {
    let text = err.to_string();
    if text.contains("unknown field") {
        ReviewedEffectiveError::command(
            "field.unknown",
            "/decisions",
            "classification decision has an unknown field",
        )
    } else if text.contains("missing field") {
        ReviewedEffectiveError::command(
            "field.missing",
            "/decisions",
            "classification decision is missing a required field",
        )
    } else {
        ReviewedEffectiveError::new("decision.invalid", "/decisions", DECISION_MESSAGE)
    }
}

fn map_shape(
    err: serde_json::Error,
    path: &str,
    schema_message: &'static str,
) -> ReviewedEffectiveError {
    let text = err.to_string();
    if text.contains("unknown field") {
        ReviewedEffectiveError::command("field.unknown", path, "reviewed effective command has an unknown field")
    } else if text.contains("missing field") {
        ReviewedEffectiveError::command("field.missing", path, "reviewed effective command is missing a required field")
    } else {
        ReviewedEffectiveError::command("schema.invalid", path, schema_message)
    }
}

fn seal_error() -> ReviewedEffectiveError {
    ReviewedEffectiveError::new(
        "seal.rejected",
        "/artifact",
        "the captured draft artifact did not recompile to its sealed hashes",
    )
}

fn open_sealed(artifact: &Value) -> Result<SealedDraft, ReviewedEffectiveError> {
    let object = artifact.as_object().ok_or_else(seal_error)?;
    let keys = ["draft", "evidence_envelope", "protocol", "wrapper"];
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(seal_error());
    }
    if object.get("protocol").and_then(Value::as_str) != Some(CAPTURED_COMMAND_PROTOCOL) {
        return Err(seal_error());
    }
    let wrapper = object.get("wrapper").ok_or_else(seal_error)?;
    if !wrapper_statements(wrapper) {
        return Err(seal_error());
    }
    let wrapper_hash = wrapper
        .get("wrapper_hash")
        .and_then(Value::as_str)
        .ok_or_else(seal_error)?;
    let mut unsigned = wrapper.clone();
    unsigned
        .as_object_mut()
        .ok_or_else(seal_error)?
        .remove("wrapper_hash");
    if canonical_sha256(&unsigned) != wrapper_hash {
        return Err(seal_error());
    }
    let envelope = object.get("evidence_envelope").ok_or_else(seal_error)?;
    if !envelope.is_object()
        || wrapper.get("evidence_envelope_hash").and_then(Value::as_str)
            != Some(canonical_sha256(envelope).as_str())
    {
        return Err(seal_error());
    }
    let recomputed = compile_structure_draft(&canonical_json(envelope)).map_err(|_| seal_error())?;
    let embedded = object.get("draft").ok_or_else(seal_error)?;
    if !embedded.is_object() || canonical_json(embedded) != recomputed {
        return Err(seal_error());
    }
    let draft_hash = sha256_hex(recomputed.as_bytes());
    if wrapper.get("draft_hash").and_then(Value::as_str) != Some(draft_hash.as_str()) {
        return Err(seal_error());
    }
    let draft: Value = serde_json::from_str(&recomputed).map_err(|_| seal_error())?;
    if !draft_statements(&draft, wrapper) {
        return Err(seal_error());
    }
    let manifest = manifest_preimage_hash(&draft).ok_or_else(seal_error)?;
    if draft
        .get("provenance")
        .and_then(|item| item.get("manifest_hash"))
        .and_then(Value::as_str)
        != Some(manifest.as_str())
        || wrapper.get("manifest_hash").and_then(Value::as_str) != Some(manifest.as_str())
    {
        return Err(seal_error());
    }
    let generated = draft.get("generated").ok_or_else(seal_error)?;
    let curated = draft.get("curated").ok_or_else(seal_error)?;
    let effective = draft.get("effective").ok_or_else(seal_error)?;
    refuse_classified(generated)?;
    refuse_classified(effective)?;
    let schema_hash = draft
        .get("provenance")
        .and_then(|item| item.get("schema_hash"))
        .and_then(Value::as_str)
        .ok_or_else(seal_error)?
        .to_string();
    Ok(SealedDraft {
        datasets: parse_datasets(effective)?,
        relationships: relationship_values(effective)?,
        fingerprints: structural_fingerprints(&draft)?,
        schema_hash,
        draft_hash,
        wrapper_hash: wrapper_hash.to_string(),
        generated_sha256: canonical_sha256(generated),
        curated_sha256: canonical_sha256(curated),
        sealed_source_sha256: canonical_sha256(artifact),
    })
}

fn wrapper_statements(wrapper: &Value) -> bool {
    let historical = &wrapper["historical"];
    same(wrapper, "protocol", CAPTURED_PROTOCOL)
        && same(wrapper, "compiler", CAPTURED_COMPILER)
        && same(wrapper, "grants", "not_authorized")
        && same(wrapper, "publication", "unavailable")
        && same(wrapper, "certification", "none")
        && same(wrapper, "llm", "not_used")
        && same(wrapper, "source_connection", "not_used")
        && same(wrapper, "row_import", "not_proven")
        && same(wrapper, "live_catalog_review", "pending")
        && same(historical, "pg_catalog_integration", "not_accepted")
        && same(
            historical,
            "pg_catalog_contract_commit",
            draft::PG_CONTRACT_COMMIT,
        )
}

fn draft_statements(draft: &Value, wrapper: &Value) -> bool {
    let provenance = &draft["provenance"];
    draft.get("protocol").and_then(Value::as_str) == Some(DRAFT_PROTOCOL)
        && same(provenance, "publication", "unavailable")
        && same(provenance, "pg_catalog_integration", "not_accepted")
        && same(provenance, "certification", "none")
        && same(provenance, "llm", "not_used")
        && same(provenance, "source_connection", "not_used")
        && provenance.get("schema_hash").and_then(Value::as_str).is_some()
        && provenance.get("schema_hash") == wrapper.get("schema_hash")
}

fn same(value: &Value, key: &str, expected: &str) -> bool {
    value.get(key).and_then(Value::as_str) == Some(expected)
}

fn classified_column(column: &Value) -> bool {
    ["pii", "semantic_type", "classification", "review"]
        .iter()
        .any(|key| match column.get(*key) {
            None | Some(Value::Null) => false,
            Some(_) => true,
        })
}

fn refuse_classified(layer: &Value) -> Result<(), ReviewedEffectiveError> {
    let Some(datasets) = layer.get("datasets").and_then(Value::as_array) else {
        return Err(seal_error());
    };
    for dataset in datasets {
        let Some(columns) = dataset.get("columns").and_then(Value::as_array) else {
            return Err(seal_error());
        };
        for column in columns {
            if !column.is_object() {
                return Err(seal_error());
            }
            if classified_column(column) {
                return Err(ReviewedEffectiveError::new(
                    "source.classified",
                    "/artifact/draft",
                    "generated or effective column already carries a class",
                ));
            }
        }
    }
    Ok(())
}

fn parse_datasets(effective: &Value) -> Result<Vec<DatasetFile>, ReviewedEffectiveError> {
    let Some(datasets) = effective.get("datasets").and_then(Value::as_array) else {
        return Err(seal_error());
    };
    let mut parsed = Vec::with_capacity(datasets.len());
    for dataset in datasets {
        let file: DatasetFile = serde_json::from_value(dataset.clone()).map_err(|_| seal_error())?;
        parsed.push(file);
    }
    parsed.sort_by(|left, right| left.dataset.as_str().cmp(right.dataset.as_str()));
    Ok(parsed)
}

fn relationship_values(effective: &Value) -> Result<Vec<Value>, ReviewedEffectiveError> {
    effective
        .get("relationships")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(seal_error)
}

fn structural_fingerprints(
    draft: &Value,
) -> Result<BTreeMap<(String, String), String>, ReviewedEffectiveError> {
    let Some(queue) = draft.get("review_queue").and_then(Value::as_array) else {
        return Err(seal_error());
    };
    let mut fingerprints = BTreeMap::new();
    for item in queue {
        if item.get("kind").and_then(Value::as_str) != Some("structural_column") {
            continue;
        }
        let Some(item_id) = item.get("item_id").and_then(Value::as_str) else {
            return Err(seal_error());
        };
        let Some((dataset, column)) = column_item_id(item_id) else {
            continue;
        };
        let Some(fingerprint) = item.get("fact_fingerprint").and_then(Value::as_str) else {
            continue;
        };
        if fingerprints
            .insert((dataset, column), fingerprint.to_string())
            .is_some()
        {
            return Err(seal_error());
        }
    }
    Ok(fingerprints)
}

fn column_item_id(item_id: &str) -> Option<(String, String)> {
    let rest = item_id.strip_prefix("column/")?;
    let (dataset, column) = rest.rsplit_once('/')?;
    if dataset.is_empty() || column.is_empty() {
        None
    } else {
        Some((dataset.to_string(), column.to_string()))
    }
}

fn check_settings(settings: &SettingsIn) -> Result<(), ReviewedEffectiveError> {
    check_slug(&settings.project, "/settings/project")?;
    check_version(&settings.ontology_version, "/settings/ontology_version")?;
    check_version(&settings.catalog_version, "/settings/catalog_version")?;
    check_slug(&settings.policy.id, "/settings/policy/id")?;
    check_version(&settings.policy.version, "/settings/policy/version")?;
    check_slug(&settings.knowledge.id, "/settings/knowledge/id")?;
    check_version(&settings.knowledge.version, "/settings/knowledge/version")?;
    if !settings.ontology.compile.per_role {
        return Err(ReviewedEffectiveError::new(
            "settings.invalid",
            "/settings/ontology/compile/per_role",
            BINDING_MESSAGE,
        ));
    }
    if settings.ontology.compile.sample_values.default == SampleValues::Raw {
        return Err(ReviewedEffectiveError::new(
            "settings.invalid",
            "/settings/ontology/compile/sample_values/default",
            BINDING_MESSAGE,
        ));
    }
    Ok(())
}

fn check_slug(value: &str, path: &str) -> Result<(), ReviewedEffectiveError> {
    let bytes = value.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        });
    if ok {
        Ok(())
    } else {
        Err(ReviewedEffectiveError::new(
            "settings.invalid",
            path,
            BINDING_MESSAGE,
        ))
    }
}

fn check_version(value: &str, path: &str) -> Result<(), ReviewedEffectiveError> {
    let bytes = value.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 64
        && bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'+' | b'-')
        });
    if ok {
        Ok(())
    } else {
        Err(ReviewedEffectiveError::new(
            "settings.invalid",
            path,
            BINDING_MESSAGE,
        ))
    }
}

fn check_schemas(
    settings: &SettingsIn,
    datasets: &[DatasetFile],
) -> Result<(), ReviewedEffectiveError> {
    if settings.ontology.include_schemas.is_empty() {
        return Err(ReviewedEffectiveError::new(
            "settings.schema",
            "/settings/ontology/include_schemas",
            SCHEMA_MESSAGE,
        ));
    }
    let mut seen = BTreeSet::new();
    for (index, schema) in settings.ontology.include_schemas.iter().enumerate() {
        if !seen.insert(schema.as_str().to_string()) {
            return Err(ReviewedEffectiveError::new(
                "settings.schema",
                format!("/settings/ontology/include_schemas/{index}"),
                SCHEMA_MESSAGE,
            ));
        }
    }
    for dataset in datasets {
        let Some((schema, _)) = dataset.dataset.as_str().split_once('.') else {
            return Err(ReviewedEffectiveError::new(
                "oos.invalid",
                "/effective_oos/datasets",
                "a dataset name is not schema.table",
            ));
        };
        if !seen.contains(schema) {
            return Err(ReviewedEffectiveError::new(
                "settings.schema",
                format!("/effective_oos/datasets/{}", dataset.dataset.as_str()),
                SCHEMA_MESSAGE,
            ));
        }
    }
    Ok(())
}

fn bind_decisions(value: &Value) -> Result<Vec<DecisionIn>, ReviewedEffectiveError> {
    let Some(items) = value.as_array() else {
        return Err(ReviewedEffectiveError::command(
            "schema.invalid",
            "/decisions",
            "artifact, settings, or decisions has the wrong JSON shape",
        ));
    };
    if items.len() > MAX_DECISIONS {
        return Err(ReviewedEffectiveError::new(
            "budget.decisions",
            "/decisions",
            "the decision list exceeds 4096",
        ));
    }
    let mut decisions = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        if !item.is_object() {
            return Err(ReviewedEffectiveError::new(
                "decision.invalid",
                "/decisions",
                DECISION_MESSAGE,
            ));
        }
        let decision: DecisionIn = serde_json::from_value(item.clone()).map_err(map_decision)?;
        if !is_sha256(&decision.schema_hash) || !is_sha256(&decision.fact_fingerprint) {
            return Err(ReviewedEffectiveError::new(
                "decision.invalid",
                "/decisions",
                DECISION_MESSAGE,
            ));
        }
        if !seen.insert((decision.dataset.clone(), decision.column.clone())) {
            return Err(ReviewedEffectiveError::new(
                "decision.duplicate",
                "/decisions",
                "a dataset and column pair is repeated",
            ));
        }
        decisions.push(decision);
    }
    Ok(decisions)
}

fn is_sha256(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (*byte >= b'a' && *byte <= b'f'))
}

fn apply_decisions(
    sealed: &SealedDraft,
    decisions: &[DecisionIn],
) -> Result<(Vec<DatasetFile>, Vec<Value>), ReviewedEffectiveError> {
    let mut datasets = sealed.datasets.clone();
    let mut reasons = BTreeMap::new();
    for dataset in &datasets {
        for column in &dataset.columns {
            let reason = if column.status == Some(ItemStatus::Rejected) {
                "column.rejected"
            } else {
                "classification.absent"
            };
            reasons.insert(
                (
                    dataset.dataset.as_str().to_string(),
                    column.name.as_str().to_string(),
                ),
                reason,
            );
        }
    }
    for decision in decisions {
        let key = (decision.dataset.clone(), decision.column.clone());
        let Some(rejected) = column_rejected(&datasets, &decision.dataset, &decision.column) else {
            return Err(ReviewedEffectiveError::new(
                "decision.unknown",
                "/decisions",
                "the decision names a column that is not on the effective draft",
            ));
        };
        let Some(fingerprint) = sealed.fingerprints.get(&key) else {
            return Err(ReviewedEffectiveError::new(
                "decision.unbound",
                "/decisions",
                "the decision has no structural column fingerprint",
            ));
        };
        if decision.schema_hash != sealed.schema_hash {
            reasons.insert(key, "decision.stale_schema");
            continue;
        }
        if decision.fact_fingerprint != *fingerprint {
            reasons.insert(key, "decision.stale_fact");
            continue;
        }
        if rejected {
            continue;
        }
        write_classification(&mut datasets, decision)?;
        reasons.remove(&key);
    }
    let hidden = reasons
        .into_iter()
        .map(|((dataset, column), reason)| {
            serde_json::json!({
                "column": column,
                "dataset": dataset,
                "reason": reason,
            })
        })
        .collect();
    Ok((datasets, hidden))
}

fn column_rejected(datasets: &[DatasetFile], dataset: &str, column: &str) -> Option<bool> {
    let file = datasets.iter().find(|item| item.dataset.as_str() == dataset)?;
    let column = file.columns.iter().find(|item| item.name.as_str() == column)?;
    Some(column.status == Some(ItemStatus::Rejected))
}

fn write_classification(
    datasets: &mut [DatasetFile],
    decision: &DecisionIn,
) -> Result<(), ReviewedEffectiveError> {
    let Some(file) = datasets
        .iter_mut()
        .find(|item| item.dataset.as_str() == decision.dataset)
    else {
        return Err(ReviewedEffectiveError::new(
            "decision.unknown",
            "/decisions",
            "the decision names a column that is not on the effective draft",
        ));
    };
    let Some(column) = file
        .columns
        .iter_mut()
        .find(|item| item.name.as_str() == decision.column)
    else {
        return Err(ReviewedEffectiveError::new(
            "decision.unknown",
            "/decisions",
            "the decision names a column that is not on the effective draft",
        ));
    };
    column.pii = Some(decision.pii);
    column.semantic_type = Some(decision.semantic_type);
    column.classification = Some(ClassificationRecord {
        state: ClassState::Confirmed,
        by: Some(decision.reviewer.by.clone()),
        at: Some(decision.reviewer.at.clone()),
    });
    Ok(())
}

fn column_index(datasets: &[DatasetFile]) -> BTreeMap<String, BTreeSet<String>> {
    let mut index = BTreeMap::new();
    for dataset in datasets {
        let columns = dataset
            .columns
            .iter()
            .map(|column| column.name.as_str().to_string())
            .collect();
        index.insert(dataset.dataset.as_str().to_string(), columns);
    }
    index
}

fn partition_relationships(
    relationships: &[Value],
    columns: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(Vec<Relationship>, Vec<Value>), ReviewedEffectiveError> {
    let mut kept = Vec::new();
    let mut omitted = Vec::new();
    for relationship in relationships {
        let parsed: Relationship = serde_json::from_value(relationship.clone()).map_err(|_| {
            ReviewedEffectiveError::new("oos.invalid", "/effective_oos/relationships", OOS_MESSAGE)
        })?;
        if endpoint_present(parsed.from.dataset.as_str(), &parsed.from.columns, columns)
            && endpoint_present(parsed.to.dataset.as_str(), &parsed.to.columns, columns)
        {
            kept.push(parsed);
        } else {
            omitted.push(serde_json::json!({
                "name": parsed.name.as_str(),
                "reason": "endpoint_not_effective",
            }));
        }
    }
    kept.sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));
    omitted.sort_by(|left, right| {
        left.get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(right.get("name").and_then(Value::as_str).unwrap_or(""))
    });
    Ok((kept, omitted))
}

fn endpoint_present(
    dataset: &str,
    end_columns: &[TableName],
    columns: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let Some(present) = columns.get(dataset) else {
        return false;
    };
    end_columns
        .iter()
        .all(|column| present.contains(column.as_str()))
}

fn validate_datasets(datasets: &[DatasetFile]) -> Result<(), ReviewedEffectiveError> {
    let mut names = BTreeSet::new();
    for dataset in datasets {
        if !names.insert(dataset.dataset.as_str().to_string()) {
            return Err(ReviewedEffectiveError::new(
                "oos.invalid",
                "/effective_oos/datasets",
                "a dataset name is repeated",
            ));
        }
        let mut columns = BTreeSet::new();
        for column in &dataset.columns {
            if !columns.insert(column.name.as_str().to_string()) {
                return Err(ReviewedEffectiveError::new(
                    "oos.invalid",
                    format!(
                        "/effective_oos/datasets/{}/columns",
                        dataset.dataset.as_str()
                    ),
                    "a column name is repeated",
                ));
            }
        }
        if let Some(diagnostic) = validate_dataset(dataset).into_iter().next() {
            return Err(ReviewedEffectiveError::new(
                "oos.invalid",
                diagnostic.path,
                OOS_MESSAGE,
            ));
        }
    }
    Ok(())
}

fn validate_relationship_file(
    settings: &SettingsIn,
    relationships: &[Relationship],
) -> Result<(), ReviewedEffectiveError> {
    let file = RelationshipsFile {
        schema_version: settings.ontology.spec_version,
        relationships: relationships.to_vec(),
    };
    if let Some(diagnostic) = validate_relationships(&file).into_iter().next() {
        return Err(ReviewedEffectiveError::new(
            "oos.invalid",
            diagnostic.path,
            OOS_MESSAGE,
        ));
    }
    Ok(())
}

fn effective_document(
    settings: &SettingsIn,
    datasets: &[DatasetFile],
    relationships: &[Relationship],
) -> Result<Value, ReviewedEffectiveError> {
    let datasets = datasets
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            ReviewedEffectiveError::new("oos.invalid", "/effective_oos/datasets", OOS_MESSAGE)
        })?;
    let relationships = relationships
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            ReviewedEffectiveError::new(
                "oos.invalid",
                "/effective_oos/relationships",
                OOS_MESSAGE,
            )
        })?;
    let ontology = serde_json::to_value(&settings.ontology).map_err(|_| {
        ReviewedEffectiveError::new("oos.invalid", "/effective_oos/ontology", OOS_MESSAGE)
    })?;
    Ok(serde_json::json!({
        "catalog_version": settings.catalog_version,
        "datasets": datasets,
        "knowledge": {"id": settings.knowledge.id, "version": settings.knowledge.version},
        "ontology": ontology,
        "ontology_version": settings.ontology_version,
        "policy": {"id": settings.policy.id, "version": settings.policy.version},
        "project": settings.project,
        "protocol": EFFECTIVE_PROTOCOL,
        "relationships": relationships,
    }))
}

fn command_result(
    sealed: &SealedDraft,
    decisions: &Value,
    effective: &Value,
    hidden: &[Value],
    omitted: &[Value],
) -> Value {
    serde_json::json!({
        "compiler": REVIEWED_EFFECTIVE_COMPILER,
        "effective_oos": effective,
        "hidden": hidden,
        "omitted_relationships": omitted,
        "protocol": REVIEWED_EFFECTIVE_COMMAND_PROTOCOL,
        "provenance": {
            "authentication": "not_claimed",
            "authorization": "not_claimed",
            "curated_sha256": sealed.curated_sha256,
            "decisions_sha256": canonical_sha256(decisions),
            "draft_hash": sealed.draft_hash,
            "generated_sha256": sealed.generated_sha256,
            "grants": "not_claimed",
            "publication": "not_claimed",
            "schema_hash": sealed.schema_hash,
            "sealed_source_sha256": sealed.sealed_source_sha256,
            "semantic_publication": "not_claimed",
            "wrapper_hash": sealed.wrapper_hash,
        },
    })
}

fn finish(text: String) -> Result<String, ReviewedEffectiveError> {
    if text.len() > DOCUMENT_MAX_BYTES {
        return Err(ReviewedEffectiveError::new(
            "budget.bytes",
            "/",
            "effective document exceeds the byte budget",
        ));
    }
    let parsed = parse_json_value(&text, REVIEWED_EFFECTIVE_COMMAND_PROTOCOL).map_err(|_| {
        ReviewedEffectiveError::command(
            "schema.invalid",
            "/",
            "command result is not an object",
        )
    })?;
    if !parsed.is_object() {
        return Err(ReviewedEffectiveError::command(
            "schema.invalid",
            "/",
            "command result is not an object",
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    use crate::canonical::canonical_sha256;
    use crate::compile_captured_pg_catalog;
    use crate::compile_role_pack;
    use crate::pgsql::{parse_catalog, schema_hash};
    use crate::CapturedPgDraft;

    const OWNER: &str = "owner_role_do_not_leak";
    const CAPTURED_AT: &str = "2026-10-04T12:00:00.000000Z";

    fn column(attnum: i32, name: &str, type_oid: i64, type_name: &str) -> Value {
        json!({
            "attnum": attnum,
            "name": name,
            "name_ident": name,
            "type_oid": type_oid,
            "type_schema": "pg_catalog",
            "type_name": type_name,
            "typtype": "b",
            "typmod": -1,
            "base_type_oid": Value::Null,
            "numeric_precision": Value::Null,
            "numeric_scale": Value::Null,
            "not_null": true,
            "collation_oid": 0,
            "collation_name": Value::Null,
            "identity": "",
            "generated": "",
            "default_expr": Value::Null,
            "comment": Value::Null,
            "enum_labels": [],
            "domain_checks": [],
            "type_support": {"converted": false, "reason": "known_builtin_not_converted"},
            "current_privileges": {"select": true, "insert": false, "update": false}
        })
    }

    fn catalog(email_name: &str) -> Value {
        let mut value = json!({
            "protocol": "oo.source.pgsql/v1",
            "core_contract": "oo.config/v1",
            "source_id": "shop_src",
            "database": "shop",
            "captured_at": CAPTURED_AT,
            "schema_hash": "0".repeat(64),
            "hash_algorithm": "pg-catalog-hash/v1",
            "ident_algorithm": "pgident/v1",
            "tables": [{
                "schema": "public",
                "schema_ident": "public",
                "name": "customer",
                "name_ident": "customer",
                "relkind": "r",
                "replica_identity": "d",
                "rls_enabled": false,
                "rls_forced": false,
                "owner": OWNER,
                "comment": Value::Null,
                "columns": [
                    column(1, "id", 23, "int4"),
                    column(2, email_name, 25, "text")
                ],
                "primary_key": [{"attnum": 1, "name": "id"}],
                "primary_key_nulls_not_distinct": false,
                "unique_keys": [],
                "foreign_keys": [],
                "policies": [],
                "privileges": [{"grantee": OWNER, "privilege": "SELECT", "grantable": false}]
            }],
            "type_conversion": "unsupported"
        });
        let parsed = parse_catalog(&value).expect("catalog shape");
        value["schema_hash"] = json!(schema_hash(&parsed.tables));
        value
    }

    fn artifact_of(compiled: &CapturedPgDraft) -> Value {
        json!({
            "draft": serde_json::from_str::<Value>(&compiled.draft).unwrap(),
            "evidence_envelope": serde_json::from_str::<Value>(&compiled.evidence_envelope).unwrap(),
            "protocol": "oo.builder.captured-pg-draft-command/v1",
            "wrapper": serde_json::from_str::<Value>(&compiled.wrapper).unwrap(),
        })
    }

    fn settings() -> Value {
        json!({
            "project": "shop",
            "ontology_version": "ont-1",
            "catalog_version": "cat-1",
            "policy": {"id": "pol-shop", "version": "1"},
            "knowledge": {"id": "know-shop", "version": "1"},
            "ontology": {
                "spec_version": "oos/v1",
                "name": "shop",
                "locale": "en-US",
                "include_schemas": ["public"],
                "default_tier_preference": ["raw"],
                "publish": {"include_unreviewed": []},
                "classification": {"auto_accept": "rule_high_confidence"},
                "lifecycle": {"deprecation_min_releases": 2},
                "compile": {
                    "per_role": true,
                    "budgets": {"readme_tokens": 2000, "table_file_tokens": 3000},
                    "sample_values": {"default": "masked", "max_per_column": 0}
                }
            }
        })
    }

    fn decision(dataset: &str, column: &str, schema: &str, fingerprint: &str) -> Value {
        json!({
            "dataset": dataset,
            "column": column,
            "schema_hash": schema,
            "fact_fingerprint": fingerprint,
            "pii": "none",
            "semantic_type": "identifier",
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        })
    }

    fn fingerprint(draft: &Value, column: &str) -> String {
        let item_id = format!("column/public.customer/{column}");
        draft["review_queue"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["item_id"] == item_id && item["kind"] == "structural_column")
            .unwrap()["fact_fingerprint"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn compile_named(email_name: &str) -> (Value, Value) {
        let text = serde_json::to_string(&catalog(email_name)).unwrap();
        let compiled = compile_captured_pg_catalog(&text, None, None).expect("sealed catalog");
        let artifact = artifact_of(&compiled);
        let draft = artifact["draft"].clone();
        (artifact, draft)
    }

    fn run(artifact: &Value, decisions: Value) -> Result<Value, ReviewedEffectiveError> {
        let command = json!({
            "protocol": REVIEWED_EFFECTIVE_COMMAND_PROTOCOL,
            "artifact": artifact,
            "settings": settings(),
            "decisions": decisions,
        });
        let text = compile_reviewed_effective(&serde_json::to_string(&command).unwrap())?;
        Ok(serde_json::from_str(&text).unwrap())
    }

    fn column_of<'a>(effective: &'a Value, name: &str) -> &'a Value {
        effective["datasets"][0]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|column| column["name"] == name)
            .unwrap()
    }

    #[test]
    fn renamed_column_refuses_the_old_name_and_classifies_only_the_new_fingerprint() {
        let (email_artifact, email_draft) = compile_named("email");
        let email_fingerprint = fingerprint(&email_draft, "email");
        let (contact_artifact, contact_draft) = compile_named("contact");
        let contact_fingerprint = fingerprint(&contact_draft, "contact");
        let schema = contact_draft["provenance"]["schema_hash"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(email_fingerprint, contact_fingerprint);
        assert!(email_artifact["draft"]["effective"].to_string().contains("email"));
        assert!(!contact_artifact["draft"]["effective"]
            .to_string()
            .contains("\"email\""));

        let unknown = run(
            &contact_artifact,
            json!([decision("public.customer", "email", &schema, &contact_fingerprint)]),
        );
        assert!(unknown.unwrap_err().code.ends_with("/decision.unknown"));

        let stale_schema = run(
            &contact_artifact,
            json!([decision(
                "public.customer",
                "contact",
                &"ab".repeat(32),
                &email_fingerprint
            )]),
        )
        .expect("stale schema stays hidden");
        assert_eq!(stale_schema["hidden"][0]["reason"], "decision.stale_schema");
        assert!(column_of(&stale_schema["effective_oos"], "contact")["pii"].is_null());

        let stale_fact = run(
            &contact_artifact,
            json!([decision(
                "public.customer",
                "contact",
                &schema,
                &email_fingerprint
            )]),
        )
        .expect("stale fact stays hidden");
        assert_eq!(stale_fact["hidden"][0]["column"], "contact");
        assert_eq!(stale_fact["hidden"][0]["reason"], "decision.stale_fact");
        assert_eq!(stale_fact["hidden"][1]["column"], "id");
        assert_eq!(stale_fact["hidden"][1]["reason"], "classification.absent");
        assert!(column_of(&stale_fact["effective_oos"], "contact")["pii"].is_null());
        assert_eq!(
            stale_fact["effective_oos"]["ontology"]["classification"]["auto_accept"],
            "rule_high_confidence"
        );
        assert!(column_of(&stale_fact["effective_oos"], "id")["pii"].is_null());
        assert!(column_of(&stale_fact["effective_oos"], "id")["classification"].is_null());

        let decisions = json!([decision(
            "public.customer",
            "contact",
            &schema,
            &contact_fingerprint
        )]);
        let fresh = run(&contact_artifact, decisions.clone()).expect("fresh decision");
        let contact = column_of(&fresh["effective_oos"], "contact");
        assert_eq!(contact["pii"], "none");
        assert_eq!(contact["semantic_type"], "identifier");
        assert_eq!(contact["classification"]["state"], "confirmed");
        assert_eq!(contact["classification"]["by"], "ada.owner");
        assert_eq!(contact["classification"]["at"], "2026-10-06");
        assert!(contact["review"].is_null());
        assert!(column_of(&fresh["effective_oos"], "id")["pii"].is_null());
        assert_eq!(fresh["hidden"][0]["column"], "id");
        assert_eq!(fresh["hidden"][0]["reason"], "classification.absent");
        assert!(fresh["hidden"].as_array().unwrap().len() == 1);
        assert_eq!(
            fresh["provenance"]["decisions_sha256"],
            canonical_sha256(&decisions)
        );
        assert_ne!(
            fresh["provenance"]["generated_sha256"],
            fresh["provenance"]["curated_sha256"]
        );
        for key in [
            "authentication",
            "authorization",
            "grants",
            "publication",
            "semantic_publication",
        ] {
            assert_eq!(fresh["provenance"][key], "not_claimed");
        }
        let rendered = serde_json::to_string(&fresh).unwrap();
        assert!(!rendered.contains(OWNER));
        assert!(!fresh["effective_oos"].to_string().contains("email"));

        let effective = canonical_json(&fresh["effective_oos"]);
        let pack = compile_role_pack(&effective, &visibility(&["contact"])).expect("role pack");
        let table = pack.file("tables/public.customer.md").unwrap();
        assert!(table.contains("contact"));
        assert!(!table.contains("email"));
        let hidden = compile_role_pack(&effective, &visibility(&["id"])).expect_err("unclassified");
        assert_eq!(
            hidden.code,
            "oo.builder.role-pack/v1/classification.unknown_pii"
        );
    }

    fn visibility(columns: &[&str]) -> String {
        serde_json::to_string(&json!({
            "protocol": "oo.builder.role-visibility/v1",
            "project": "shop",
            "principal": "ada",
            "role": "oo_r_clerk",
            "ontology_version": "ont-1",
            "catalog_version": "cat-1",
            "policy": {"id": "pol-shop", "version": "1"},
            "knowledge": {"id": "know-shop", "version": "1"},
            "issued_for_compiler": "oo-builder/role-pack/v1",
            "datasets": [{
                "dataset": "public.customer",
                "row_security": "none",
                "global_stats": "denied",
                "columns": columns
            }],
            "limits": {"max_file_bytes": 262144, "max_total_bytes": 1048576, "max_entries": 128}
        }))
        .unwrap()
    }

    #[test]
    fn duplicate_decision_and_bad_pii_refuse_the_document() {
        let (artifact, draft) = compile_named("email");
        let schema = draft["provenance"]["schema_hash"].as_str().unwrap();
        let id_fingerprint = fingerprint(&draft, "id");
        let mut first = decision("public.customer", "id", schema, &id_fingerprint);
        let duplicate = run(&artifact, json!([first.clone(), first.clone()]));
        assert!(duplicate.unwrap_err().code.ends_with("/decision.duplicate"));
        first["pii"] = json!("secret");
        let invalid = run(&artifact, json!([first]));
        assert!(invalid.unwrap_err().code.ends_with("/decision.invalid"));
    }

    #[test]
    fn tampered_class_fails_the_seal_before_it_can_apply() {
        let (mut artifact, _) = compile_named("email");
        artifact["draft"]["effective"]["datasets"][0]["columns"][0]["pii"] = json!("none");
        let err = run(&artifact, json!([])).expect_err("tamper");
        assert!(err.code.ends_with("/seal.rejected"));
    }

    #[test]
    fn omitted_endpoint_is_explicit_and_a_carried_class_is_detected() {
        let relationship = json!({
            "name": "gone_fk",
            "from": {"dataset": "public.purchase", "columns": ["missing"]},
            "to": {"dataset": "public.customer", "columns": ["id"]},
            "cardinality": "many_to_one",
            "join_hint": "left",
            "provenance": "declared_fk",
            "status": "draft"
        });
        let mut index = BTreeMap::new();
        index.insert(
            "public.customer".to_string(),
            BTreeSet::from(["id".to_string()]),
        );
        let (kept, omitted) = partition_relationships(&[relationship], &index).unwrap();
        assert!(kept.is_empty());
        assert_eq!(omitted[0]["name"], "gone_fk");
        assert_eq!(omitted[0]["reason"], "endpoint_not_effective");
        assert!(!classified_column(&json!({"pii": null, "semantic_type": null})));
        assert!(classified_column(&json!({"pii": "personal"})));
    }

    #[test]
    fn a_rejected_column_that_is_still_effective_is_not_classified() {
        let dataset: DatasetFile = serde_json::from_value(json!({
            "schema_version": "oos/v1",
            "dataset": "public.customer",
            "label": "customer",
            "tier": "raw",
            "columns": [
                {"name": "email", "status": "rejected"},
                {"name": "id", "status": "draft"}
            ]
        }))
        .unwrap();
        let schema = "cd".repeat(32);
        let email_fingerprint = "ab".repeat(32);
        let sealed = SealedDraft {
            datasets: vec![dataset],
            relationships: Vec::new(),
            fingerprints: BTreeMap::from([(
                ("public.customer".to_string(), "email".to_string()),
                email_fingerprint.clone(),
            )]),
            schema_hash: schema.clone(),
            draft_hash: "11".repeat(32),
            wrapper_hash: "22".repeat(32),
            generated_sha256: "33".repeat(32),
            curated_sha256: "44".repeat(32),
            sealed_source_sha256: "55".repeat(32),
        };
        let fresh: DecisionIn = serde_json::from_value(json!({
            "dataset": "public.customer",
            "column": "email",
            "schema_hash": schema.clone(),
            "fact_fingerprint": email_fingerprint.clone(),
            "pii": "none",
            "semantic_type": "identifier",
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        }))
        .unwrap();
        let (datasets, hidden) = apply_decisions(&sealed, &[fresh]).unwrap();
        let email = datasets[0]
            .columns
            .iter()
            .find(|column| column.name.as_str() == "email")
            .unwrap();
        assert!(email.pii.is_none());
        assert_eq!(hidden[0]["column"], "email");
        assert_eq!(hidden[0]["reason"], "column.rejected");
        assert_eq!(hidden[1]["column"], "id");
        assert_eq!(hidden[1]["reason"], "classification.absent");

        let mut stale = serde_json::from_value::<DecisionIn>(json!({
            "dataset": "public.customer",
            "column": "email",
            "schema_hash": "00".repeat(32),
            "fact_fingerprint": email_fingerprint.clone(),
            "pii": "personal",
            "semantic_type": "email",
            "reviewer": {"by": "ada.owner", "at": "2026-10-06"}
        }))
        .unwrap();
        let (_, stale_hidden) = apply_decisions(&sealed, &[stale.clone()]).unwrap();
        assert_eq!(stale_hidden[0]["reason"], "decision.stale_schema");
        stale.schema_hash = schema;
        stale.fact_fingerprint = "ef".repeat(32);
        let (_, fact_hidden) = apply_decisions(&sealed, &[stale]).unwrap();
        assert_eq!(fact_hidden[0]["reason"], "decision.stale_fact");
    }
}
