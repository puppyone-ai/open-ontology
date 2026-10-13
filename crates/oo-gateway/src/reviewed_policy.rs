//! Column ACL set from one reviewed effective command.
//!
//! The caller supplies the command output, the sealed artifact, the decision
//! list, and an administrative binding. This module checks those seals and
//! compiles `ColumnAclPolicy` values. It does not open a database or grant.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;
use serde_json::{Map, Value};

use oo_core::api::payload_sha256;
use oo_core::config::{PiiLevel, SampleValues};
use oo_core::json_scan::parse_json_value;
use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_core::oos::{
    CivilDate, ClassState, DatasetColumn, DatasetFile, OntologyFile, Relationship,
    RelationshipsFile, SemanticType, validate_dataset, validate_relationships,
};
use oo_core::text::{Label, QualifiedTable, TableName};

use crate::policy::{
    ClassStatus, ColumnAclPolicy, ColumnSpec, CompiledPolicySet, LogicalTable, PiiClass,
    PolicyError, compile_column_acl_set,
};

const CONTRACT: &str = "oo.gateway.reviewed-policy/v1";
const COMMAND_PROTOCOL: &str = "oo.builder.reviewed-effective-oos-command/v1";
const COMMAND_COMPILER: &str = "oo-builder/reviewed-effective-oos/v1";
const EFFECTIVE_PROTOCOL: &str = "oo.builder.effective-oos/v1";
const ARTIFACT_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";
const WRAPPER_PROTOCOL: &str = "oo.builder.captured-pg-draft/v1";
const WRAPPER_COMPILER: &str = "oo-builder/captured-pg-draft/v1";
const DRAFT_PROTOCOL: &str = "oo.builder.structure-draft/v1";
const ENVELOPE_PROTOCOL: &str = "oo.builder.catalog-evidence/v1";
const PG_CONTRACT_COMMIT: &str = "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74";
const MAX_DECISIONS: usize = 4096;
const FORBIDDEN_KEYS: [&str; 6] = [
    "samples",
    "credentials",
    "privileges",
    "password",
    "secret",
    "row_filter",
];

/// Project and version names the effective document must repeat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedIdentity {
    /// Effective project.
    pub project: String,
    /// Effective ontology version.
    pub ontology_version: String,
    /// Effective catalog version.
    pub catalog_version: String,
    /// Effective policy id.
    pub policy_id: String,
    /// Effective policy version.
    pub policy_version: String,
    /// Effective knowledge id.
    pub knowledge_id: String,
    /// Effective knowledge version.
    pub knowledge_version: String,
}

/// Trust root recorded from the compiler run the caller accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedSourceSeal {
    /// `evidence_envelope.source.source_id`.
    pub source_id: String,
    /// Schema the allowlist names, as `schema.table`.
    pub schema: String,
    /// Catalog schema hash from that run.
    pub schema_hash: String,
    /// Canonical SHA-256 of the captured-draft artifact.
    pub artifact_sha256: String,
}

/// One administrative column allow. It carries no classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedAllow {
    /// `schema.table` on the effective source.
    pub dataset: String,
    /// Column on that dataset.
    pub column: String,
    /// Administrative token required when the decision declares PII.
    pub explicit_allow: bool,
}

/// One role mapping. This is not an authentication grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedBinding {
    /// Names that must equal the effective document.
    pub identity: ReviewedIdentity,
    /// Compiler trust root.
    pub seal: ReviewedSourceSeal,
    /// Mapping config digest stamped onto every policy.
    pub config_digest: String,
    /// Mapping generation stamped onto every policy. At least 1.
    pub generation: i64,
    /// Role slug. The compiled role is `oo_r_` plus this slug.
    pub role: String,
    /// Login stamped onto every policy. Not an authentication token.
    pub login: String,
    /// Columns this call may compile. Empty is refused by the existing set compiler.
    pub allow: Vec<ReviewedAllow>,
    /// Exact sealed collection vector for independently versioned native tables.
    pub binding_vector: Option<Value>,
}

/// Policies passed to `compile_column_acl_set`, plus that function's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewedColumnPolicies {
    /// One policy per allowlisted table, in first-seen order.
    pub policies: Vec<ColumnAclPolicy>,
    /// Result of the existing set compiler.
    pub compiled: CompiledPolicySet,
}

/// Refusal. Display text does not echo a document body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewedPolicyError {
    /// An input exceeds 1048576 bytes, or the decision list exceeds 4096.
    Budget,
    /// JSON scan rejected the input.
    Scan,
    /// Command bytes or the command protocol do not match.
    Protocol,
    /// A required field, type, or OOS check failed.
    Shape,
    /// A captured-draft or provenance seal failed.
    Seal,
    /// Effective identity does not match the binding.
    Identity,
    /// A column decision is missing, stale, rejected, or inconsistent.
    Decision {
        /// `unknown`, `missing`, `stale_schema`, `stale_fact`, `rejected`, `unbound`, or `mismatch`.
        reason: &'static str,
    },
    /// Physical type and semantic type do not agree.
    Semantic,
    /// Declared PII has no explicit allow, or `none` was given one.
    PiiAllow,
    /// A dataset and column pair is repeated.
    Duplicate,
    /// The command carries a forbidden key, or the sample default is raw.
    Forbidden,
    /// An allowlisted table has row security or a policy.
    Rls,
    /// The existing set compiler refused the policies.
    Policy(PolicyError),
}

impl ReviewedPolicyError {
    /// Stable code `oo.gateway.reviewed-policy/v1/` plus the variant suffix.
    pub fn code(&self) -> String {
        let suffix = match self {
            Self::Budget => "budget",
            Self::Scan => "scan",
            Self::Protocol => "protocol",
            Self::Shape => "shape",
            Self::Seal => "seal",
            Self::Identity => "identity",
            Self::Decision { .. } => "decision",
            Self::Semantic => "semantic",
            Self::PiiAllow => "pii_allow",
            Self::Duplicate => "duplicate",
            Self::Forbidden => "forbidden",
            Self::Rls => "rls",
            Self::Policy(_) => "policy",
        };
        format!("{CONTRACT}/{suffix}")
    }
}

impl fmt::Display for ReviewedPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Budget => formatter.write_str("reviewed policy input exceeds its budget"),
            Self::Scan => formatter.write_str("reviewed policy input is not accepted JSON"),
            Self::Protocol => {
                formatter.write_str("reviewed policy command protocol does not match")
            }
            Self::Shape => formatter.write_str("reviewed policy document shape is not accepted"),
            Self::Seal => formatter.write_str("reviewed policy seal does not match"),
            Self::Identity => formatter
                .write_str("reviewed document identity does not match the administrative binding"),
            Self::Decision { reason } => {
                write!(formatter, "reviewed column decision is {reason}")
            }
            Self::Semantic => formatter.write_str("reviewed column semantic type is not accepted"),
            Self::PiiAllow => {
                formatter.write_str("reviewed column pii requires an explicit allow token")
            }
            Self::Duplicate => formatter.write_str("reviewed column is duplicated"),
            Self::Forbidden => {
                formatter.write_str("reviewed policy document contains a forbidden field")
            }
            Self::Rls => formatter.write_str("reviewed table row security is not accepted"),
            Self::Policy(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ReviewedPolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionIn {
    dataset: String,
    column: String,
    schema_hash: String,
    fact_fingerprint: String,
    pii: PiiLevel,
    semantic_type: SemanticType,
    #[serde(default)]
    unit: Option<Label>,
    reviewer: ReviewerIn,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewerIn {
    by: Label,
    at: CivilDate,
}

struct Model {
    datasets: Vec<DatasetFile>,
}

struct Admitted {
    namespace: String,
    table: String,
    column: ColumnSpec,
}

/// Compile one role's column policies from a reviewed effective command.
///
/// `command_output` is the canonical `reviewed-effective-oos` stdout.
/// `artifact_json` is the captured-draft command that produced it.
/// `decisions_json` is the decision array, which need not be canonical.
/// `binding` is administrative mapping input, not an authorization decision.
pub fn compile_reviewed_policies(
    command_output: &str,
    artifact_json: &str,
    decisions_json: &str,
    binding: &ReviewedBinding,
) -> Result<ReviewedColumnPolicies, ReviewedPolicyError> {
    let (command, artifact, decisions) =
        scan_documents(command_output, artifact_json, decisions_json)?;
    accept_command(&command)?;
    accept_artifact(&artifact)?;
    verify_seals(&command, &artifact, binding)?;
    let parsed = parse_decision_list(&decisions, &command, binding)?;
    let effective = command
        .get("effective_oos")
        .ok_or(ReviewedPolicyError::Shape)?;
    if !identity_ok(effective, &artifact, binding) {
        return Err(ReviewedPolicyError::Identity);
    }
    let model = parse_model(effective, Some(&binding.seal.schema))?;
    unclassified_layer(&artifact, "generated")?;
    unclassified_layer(&artifact, "effective")?;
    let admitted = admit_all(&model, &parsed, &command, &artifact, binding)?;
    let policies = group_policies(admitted, binding);
    let compiled = compile_column_acl_set(&policies).map_err(ReviewedPolicyError::Policy)?;
    Ok(ReviewedColumnPolicies { policies, compiled })
}

fn scan_documents(
    command_output: &str,
    artifact_json: &str,
    decisions_json: &str,
) -> Result<(Value, Value, Value), ReviewedPolicyError> {
    check_budget(command_output)?;
    check_budget(artifact_json)?;
    check_budget(decisions_json)?;
    let command = scan_value(command_output)?;
    let artifact = scan_value(artifact_json)?;
    let decisions = scan_value(decisions_json)?;
    if canonical_json(&command) != command_output {
        return Err(ReviewedPolicyError::Protocol);
    }
    if forbidden_key(&command) {
        return Err(ReviewedPolicyError::Forbidden);
    }
    Ok((command, artifact, decisions))
}

fn check_budget(input: &str) -> Result<(), ReviewedPolicyError> {
    if input.len() > DOCUMENT_MAX_BYTES {
        Err(ReviewedPolicyError::Budget)
    } else {
        Ok(())
    }
}

fn scan_value(input: &str) -> Result<Value, ReviewedPolicyError> {
    parse_json_value(input, CONTRACT).map_err(|_| ReviewedPolicyError::Scan)
}

fn accept_command(value: &Value) -> Result<(), ReviewedPolicyError> {
    let object = value.as_object().ok_or(ReviewedPolicyError::Shape)?;
    const KEYS: [&str; 6] = [
        "compiler",
        "effective_oos",
        "hidden",
        "omitted_relationships",
        "protocol",
        "provenance",
    ];
    if object.len() != KEYS.len() || KEYS.iter().any(|key| !object.contains_key(*key)) {
        return Err(ReviewedPolicyError::Shape);
    }
    let protocol = object
        .get("protocol")
        .and_then(Value::as_str)
        .ok_or(ReviewedPolicyError::Shape)?;
    let compiler = object
        .get("compiler")
        .and_then(Value::as_str)
        .ok_or(ReviewedPolicyError::Shape)?;
    if protocol != COMMAND_PROTOCOL || compiler != COMMAND_COMPILER {
        return Err(ReviewedPolicyError::Protocol);
    }
    if !object.get("effective_oos").is_some_and(Value::is_object)
        || !object.get("hidden").is_some_and(Value::is_array)
        || !object
            .get("omitted_relationships")
            .is_some_and(Value::is_array)
        || !object.get("provenance").is_some_and(Value::is_object)
    {
        return Err(ReviewedPolicyError::Shape);
    }
    Ok(())
}

fn accept_artifact(value: &Value) -> Result<(), ReviewedPolicyError> {
    let object = value.as_object().ok_or(ReviewedPolicyError::Seal)?;
    const KEYS: [&str; 4] = ["draft", "evidence_envelope", "protocol", "wrapper"];
    if object.len() != KEYS.len() || KEYS.iter().any(|key| !object.contains_key(*key)) {
        return Err(ReviewedPolicyError::Seal);
    }
    if object.get("protocol").and_then(Value::as_str) != Some(ARTIFACT_PROTOCOL) {
        return Err(ReviewedPolicyError::Seal);
    }
    if !artifact_statements(value) {
        return Err(ReviewedPolicyError::Seal);
    }
    Ok(())
}

fn artifact_statements(artifact: &Value) -> bool {
    let wrapper = &artifact["wrapper"];
    let historical = &wrapper["historical"];
    let draft = &artifact["draft"];
    let provenance = &draft["provenance"];
    let envelope = &artifact["evidence_envelope"];
    same(wrapper, "protocol", WRAPPER_PROTOCOL)
        && same(wrapper, "compiler", WRAPPER_COMPILER)
        && same(wrapper, "grants", "not_authorized")
        && same(wrapper, "publication", "unavailable")
        && same(wrapper, "certification", "none")
        && same(wrapper, "llm", "not_used")
        && same(wrapper, "source_connection", "not_used")
        && same(wrapper, "row_import", "not_proven")
        && same(wrapper, "live_catalog_review", "pending")
        && same(historical, "pg_catalog_integration", "not_accepted")
        && same(historical, "pg_catalog_contract_commit", PG_CONTRACT_COMMIT)
        && same(draft, "protocol", DRAFT_PROTOCOL)
        && same(provenance, "publication", "unavailable")
        && same(provenance, "pg_catalog_integration", "not_accepted")
        && same(provenance, "certification", "none")
        && same(provenance, "llm", "not_used")
        && same(provenance, "source_connection", "not_used")
        && same(envelope, "protocol", ENVELOPE_PROTOCOL)
}

fn verify_seals(
    command: &Value,
    artifact: &Value,
    binding: &ReviewedBinding,
) -> Result<(), ReviewedPolicyError> {
    let wrapper = &artifact["wrapper"];
    let draft = &artifact["draft"];
    let envelope = &artifact["evidence_envelope"];
    let provenance = &command["provenance"];
    let draft_hash = canonical_sha256(draft);
    let wrapper_hash = unsigned_hash(wrapper).ok_or(ReviewedPolicyError::Seal)?;
    let envelope_hash = canonical_sha256(envelope);
    let manifest = manifest_preimage(draft).ok_or(ReviewedPolicyError::Seal)?;
    let generated = canonical_sha256(draft.get("generated").ok_or(ReviewedPolicyError::Seal)?);
    let curated = canonical_sha256(draft.get("curated").ok_or(ReviewedPolicyError::Seal)?);
    let schema = draft
        .pointer("/provenance/schema_hash")
        .and_then(Value::as_str)
        .ok_or(ReviewedPolicyError::Seal)?;
    let artifact_hash = canonical_sha256(artifact);
    if !hex64(schema) || !non_claims(provenance) {
        return Err(ReviewedPolicyError::Seal);
    }
    let matched = hash_eq(wrapper, "wrapper_hash", &wrapper_hash)
        && hash_eq(wrapper, "evidence_envelope_hash", &envelope_hash)
        && hash_eq(wrapper, "draft_hash", &draft_hash)
        && hash_eq(wrapper, "manifest_hash", &manifest)
        && hash_eq(wrapper, "schema_hash", schema)
        && hash_eq(&draft["provenance"], "manifest_hash", &manifest)
        && hash_eq(&draft["provenance"], "schema_hash", schema)
        && hash_eq(provenance, "draft_hash", &draft_hash)
        && hash_eq(provenance, "wrapper_hash", &wrapper_hash)
        && hash_eq(provenance, "generated_sha256", &generated)
        && hash_eq(provenance, "curated_sha256", &curated)
        && hash_eq(provenance, "schema_hash", schema)
        && hash_eq(provenance, "sealed_source_sha256", &artifact_hash)
        && binding.seal.schema_hash == schema
        && binding.seal.artifact_sha256 == artifact_hash;
    if matched {
        Ok(())
    } else {
        Err(ReviewedPolicyError::Seal)
    }
}

fn parse_decision_list(
    value: &Value,
    command: &Value,
    binding: &ReviewedBinding,
) -> Result<BTreeMap<(String, String), DecisionIn>, ReviewedPolicyError> {
    let items = value.as_array().ok_or(ReviewedPolicyError::Shape)?;
    if duplicated_values(items)? || duplicated_allow(&binding.allow) {
        return Err(ReviewedPolicyError::Duplicate);
    }
    if items.len() > MAX_DECISIONS {
        return Err(ReviewedPolicyError::Budget);
    }
    let expected = command
        .pointer("/provenance/decisions_sha256")
        .and_then(Value::as_str)
        .ok_or(ReviewedPolicyError::Seal)?;
    if !hash_text(expected, &canonical_sha256(value)) {
        return Err(ReviewedPolicyError::Seal);
    }
    let mut map = BTreeMap::new();
    for item in items {
        let parsed: DecisionIn =
            serde_json::from_value(item.clone()).map_err(|_| ReviewedPolicyError::Shape)?;
        map.insert((parsed.dataset.clone(), parsed.column.clone()), parsed);
    }
    Ok(map)
}

fn duplicated_values(items: &[Value]) -> Result<bool, ReviewedPolicyError> {
    let mut seen = BTreeSet::new();
    for item in items {
        let dataset = item
            .get("dataset")
            .and_then(Value::as_str)
            .ok_or(ReviewedPolicyError::Shape)?;
        let column = item
            .get("column")
            .and_then(Value::as_str)
            .ok_or(ReviewedPolicyError::Shape)?;
        if !seen.insert((dataset.to_owned(), column.to_owned())) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn duplicated_allow(allow: &[ReviewedAllow]) -> bool {
    let mut seen = BTreeSet::new();
    allow
        .iter()
        .any(|entry| !seen.insert((entry.dataset.as_str(), entry.column.as_str())))
}

fn identity_ok(effective: &Value, artifact: &Value, binding: &ReviewedBinding) -> bool {
    let id = &binding.identity;
    same(effective, "project", &id.project)
        && same(effective, "ontology_version", &id.ontology_version)
        && same(effective, "catalog_version", &id.catalog_version)
        && nested_same(effective, "policy", "id", &id.policy_id)
        && nested_same(effective, "policy", "version", &id.policy_version)
        && nested_same(effective, "knowledge", "id", &id.knowledge_id)
        && nested_same(effective, "knowledge", "version", &id.knowledge_version)
        && artifact
            .pointer("/evidence_envelope/source/source_id")
            .and_then(Value::as_str)
            == Some(binding.seal.source_id.as_str())
}

fn parse_model(effective: &Value, schema: Option<&str>) -> Result<Model, ReviewedPolicyError> {
    if effective.get("protocol").and_then(Value::as_str) != Some(EFFECTIVE_PROTOCOL) {
        return Err(ReviewedPolicyError::Shape);
    }
    let ontology: OntologyFile = serde_json::from_value(effective["ontology"].clone())
        .map_err(|_| ReviewedPolicyError::Shape)?;
    check_ontology(&ontology, schema)?;
    let datasets = parse_datasets(
        effective
            .get("datasets")
            .ok_or(ReviewedPolicyError::Shape)?,
    )?;
    parse_relationships(
        effective
            .get("relationships")
            .ok_or(ReviewedPolicyError::Shape)?,
        &ontology,
    )?;
    Ok(Model { datasets })
}

fn check_ontology(ontology: &OntologyFile, schema: Option<&str>) -> Result<(), ReviewedPolicyError> {
    if !ontology.compile.per_role {
        return Err(ReviewedPolicyError::Shape);
    }
    if ontology.compile.sample_values.default == SampleValues::Raw {
        return Err(ReviewedPolicyError::Forbidden);
    }
    if !schemas_cover(ontology, schema) {
        return Err(ReviewedPolicyError::Shape);
    }
    Ok(())
}

fn schemas_cover(ontology: &OntologyFile, schema: Option<&str>) -> bool {
    if ontology.include_schemas.is_empty() {
        return false;
    }
    let mut seen = BTreeSet::new();
    for name in &ontology.include_schemas {
        if !seen.insert(name.as_str()) {
            return false;
        }
    }
    schema.is_none_or(|name| seen.contains(name))
}

fn parse_datasets(value: &Value) -> Result<Vec<DatasetFile>, ReviewedPolicyError> {
    let items = value.as_array().ok_or(ReviewedPolicyError::Shape)?;
    let mut datasets = Vec::with_capacity(items.len());
    for item in items {
        let file: DatasetFile =
            serde_json::from_value(item.clone()).map_err(|_| ReviewedPolicyError::Shape)?;
        if !validate_dataset(&file).is_empty() {
            return Err(ReviewedPolicyError::Shape);
        }
        datasets.push(file);
    }
    Ok(datasets)
}

fn parse_relationships(value: &Value, ontology: &OntologyFile) -> Result<(), ReviewedPolicyError> {
    let items = value.as_array().ok_or(ReviewedPolicyError::Shape)?;
    let mut relationships = Vec::with_capacity(items.len());
    for item in items {
        let relationship: Relationship =
            serde_json::from_value(item.clone()).map_err(|_| ReviewedPolicyError::Shape)?;
        relationships.push(relationship);
    }
    let file = RelationshipsFile {
        schema_version: ontology.spec_version,
        relationships,
    };
    if validate_relationships(&file).is_empty() {
        Ok(())
    } else {
        Err(ReviewedPolicyError::Shape)
    }
}

fn unclassified_layer(artifact: &Value, layer: &str) -> Result<(), ReviewedPolicyError> {
    let path = format!("/draft/{layer}/datasets");
    let datasets = artifact
        .pointer(&path)
        .and_then(Value::as_array)
        .ok_or(ReviewedPolicyError::Seal)?;
    for dataset in datasets {
        let columns = dataset
            .get("columns")
            .and_then(Value::as_array)
            .ok_or(ReviewedPolicyError::Seal)?;
        for column in columns {
            if classified_column(column) {
                return Err(ReviewedPolicyError::Seal);
            }
        }
    }
    Ok(())
}

fn classified_column(column: &Value) -> bool {
    if !column.is_object() {
        return true;
    }
    ["pii", "semantic_type", "classification", "review"]
        .into_iter()
        .any(|key| match column.get(key) {
            None | Some(Value::Null) => false,
            Some(_) => true,
        })
}

fn admit_all(
    model: &Model,
    decisions: &BTreeMap<(String, String), DecisionIn>,
    command: &Value,
    artifact: &Value,
    binding: &ReviewedBinding,
) -> Result<Vec<Admitted>, ReviewedPolicyError> {
    let mut admitted = Vec::with_capacity(binding.allow.len());
    for entry in &binding.allow {
        admitted.push(admit_one(
            model, decisions, command, artifact, binding, entry,
        )?);
    }
    Ok(admitted)
}

fn admit_one(
    model: &Model,
    decisions: &BTreeMap<(String, String), DecisionIn>,
    command: &Value,
    artifact: &Value,
    binding: &ReviewedBinding,
    entry: &ReviewedAllow,
) -> Result<Admitted, ReviewedPolicyError> {
    let Some((namespace, table)) = split_logical(&entry.dataset, &binding.seal.schema) else {
        return Err(absent_reason(artifact, &entry.dataset, &entry.column)?);
    };
    let Some(column) = find_column(model, &entry.dataset, &entry.column) else {
        return Err(absent_reason(artifact, &entry.dataset, &entry.column)?);
    };
    let Some(decision) = decisions.get(&(entry.dataset.clone(), entry.column.clone())) else {
        return Err(ReviewedPolicyError::Decision { reason: "missing" });
    };
    check_decision(decision, artifact, binding, entry)?;
    check_output(column, decision, command, entry)?;
    let spec = column_spec(
        column,
        decision,
        entry.explicit_allow,
        artifact,
        namespace,
        table,
    )?;
    Ok(Admitted {
        namespace: namespace.to_owned(),
        table: table.to_owned(),
        column: spec,
    })
}

fn absent_reason(
    artifact: &Value,
    dataset: &str,
    column: &str,
) -> Result<ReviewedPolicyError, ReviewedPolicyError> {
    let rejected = queue_rejected(artifact, dataset, column)?
        || generated_rejected(artifact, dataset, column)?;
    let reason = if rejected { "rejected" } else { "unknown" };
    Ok(ReviewedPolicyError::Decision { reason })
}

fn check_decision(
    decision: &DecisionIn,
    artifact: &Value,
    binding: &ReviewedBinding,
    entry: &ReviewedAllow,
) -> Result<(), ReviewedPolicyError> {
    if decision.schema_hash != binding.seal.schema_hash {
        return Err(ReviewedPolicyError::Decision {
            reason: "stale_schema",
        });
    }
    let Some(fingerprint) = structural_fingerprint(artifact, &entry.dataset, &entry.column)? else {
        return Err(ReviewedPolicyError::Decision { reason: "unbound" });
    };
    if decision.fact_fingerprint != fingerprint {
        return Err(ReviewedPolicyError::Decision {
            reason: "stale_fact",
        });
    }
    if effective_rejected(artifact, &entry.dataset, &entry.column)? {
        return Err(ReviewedPolicyError::Decision { reason: "rejected" });
    }
    Ok(())
}

fn check_output(
    column: &DatasetColumn,
    decision: &DecisionIn,
    command: &Value,
    entry: &ReviewedAllow,
) -> Result<(), ReviewedPolicyError> {
    if !classification_matches(column, decision) {
        return Err(ReviewedPolicyError::Decision { reason: "mismatch" });
    }
    if hidden_has(command, &entry.dataset, &entry.column)? {
        return Err(ReviewedPolicyError::Seal);
    }
    Ok(())
}

fn column_spec(
    column: &DatasetColumn,
    decision: &DecisionIn,
    explicit_allow: bool,
    artifact: &Value,
    namespace: &str,
    table: &str,
) -> Result<ColumnSpec, ReviewedPolicyError> {
    let data_type = column
        .data_type
        .as_ref()
        .map(Label::as_str)
        .ok_or(ReviewedPolicyError::Semantic)?;
    let physical = physical_type(artifact, namespace, table, column.name.as_str(), data_type)?;
    let semantic = column.semantic_type.ok_or(ReviewedPolicyError::Semantic)?;
    if !compatible(&physical, semantic) {
        return Err(ReviewedPolicyError::Semantic);
    }
    Ok(ColumnSpec {
        name: column.name.as_str().to_owned(),
        status: ClassStatus::Confirmed,
        semantic_type: semantic_name(semantic).to_owned(),
        pii: map_pii(decision.pii, explicit_allow)?,
    })
}

fn split_logical<'a>(dataset: &'a str, schema: &'a str) -> Option<(&'a str, &'a str)> {
    if QualifiedTable::new(dataset.to_owned()).is_err() {
        return None;
    }
    let table = dataset.strip_prefix(schema)?.strip_prefix('.')?;
    if table.contains('.') || TableName::new(table.to_owned()).is_err() {
        None
    } else {
        Some((schema, table))
    }
}

fn split_native_logical<'a>(dataset: &'a str, effective: &Value, vector: bool, schema: &'a str) -> Option<(&'a str, &'a str)> {
    if !vector { return split_logical(dataset, schema); }
    let (actual_schema, _) = dataset.split_once('.')?;
    let included = effective["ontology"]["include_schemas"].as_array()?;
    if !included.iter().any(|name| name.as_str() == Some(actual_schema)) { return None; }
    split_logical(dataset, actual_schema)
}

fn find_column<'a>(model: &'a Model, dataset: &str, column: &str) -> Option<&'a DatasetColumn> {
    let file = model
        .datasets
        .iter()
        .find(|item| item.dataset.as_str() == dataset)?;
    file.columns
        .iter()
        .find(|item| item.name.as_str() == column)
}

fn classification_matches(column: &DatasetColumn, decision: &DecisionIn) -> bool {
    let Some(record) = &column.classification else {
        return false;
    };
    column.pii == Some(decision.pii)
        && column.semantic_type == Some(decision.semantic_type)
        && column.unit == decision.unit
        && column.review.is_none()
        && record.state == ClassState::Confirmed
        && record.by.as_ref() == Some(&decision.reviewer.by)
        && record.at.as_ref() == Some(&decision.reviewer.at)
}

fn queue_rejected(
    artifact: &Value,
    dataset: &str,
    column: &str,
) -> Result<bool, ReviewedPolicyError> {
    let queue = review_queue(artifact)?;
    let item_id = format!("column/{dataset}/{column}");
    Ok(queue.iter().any(|item| {
        structural_item(item, &item_id)
            && item.get("state").and_then(Value::as_str) == Some("rejected")
    }))
}

fn generated_rejected(
    artifact: &Value,
    dataset: &str,
    column: &str,
) -> Result<bool, ReviewedPolicyError> {
    column_status(artifact, "generated", dataset, column, "rejected")
}

fn effective_rejected(
    artifact: &Value,
    dataset: &str,
    column: &str,
) -> Result<bool, ReviewedPolicyError> {
    column_status(artifact, "effective", dataset, column, "rejected")
}

fn column_status(
    artifact: &Value,
    layer: &str,
    dataset: &str,
    column: &str,
    status: &str,
) -> Result<bool, ReviewedPolicyError> {
    let path = format!("/draft/{layer}/datasets");
    let datasets = artifact
        .pointer(&path)
        .and_then(Value::as_array)
        .ok_or(ReviewedPolicyError::Seal)?;
    for file in datasets {
        if file.get("dataset").and_then(Value::as_str) != Some(dataset) {
            continue;
        }
        let columns = file
            .get("columns")
            .and_then(Value::as_array)
            .ok_or(ReviewedPolicyError::Seal)?;
        for item in columns {
            if item.get("name").and_then(Value::as_str) == Some(column) {
                return Ok(item.get("status").and_then(Value::as_str) == Some(status));
            }
        }
    }
    Ok(false)
}

fn structural_fingerprint(
    artifact: &Value,
    dataset: &str,
    column: &str,
) -> Result<Option<String>, ReviewedPolicyError> {
    let queue = review_queue(artifact)?;
    let item_id = format!("column/{dataset}/{column}");
    let mut found = None;
    for item in queue {
        if !structural_item(item, &item_id) {
            continue;
        }
        let fingerprint = item
            .get("fact_fingerprint")
            .and_then(Value::as_str)
            .ok_or(ReviewedPolicyError::Seal)?;
        if found.is_some() {
            return Err(ReviewedPolicyError::Seal);
        }
        found = Some(fingerprint.to_owned());
    }
    Ok(found)
}

fn review_queue(artifact: &Value) -> Result<&Vec<Value>, ReviewedPolicyError> {
    artifact
        .pointer("/draft/review_queue")
        .and_then(Value::as_array)
        .ok_or(ReviewedPolicyError::Seal)
}

fn structural_item(item: &Value, item_id: &str) -> bool {
    item.get("kind").and_then(Value::as_str) == Some("structural_column")
        && item.get("item_id").and_then(Value::as_str) == Some(item_id)
}

fn hidden_has(command: &Value, dataset: &str, column: &str) -> Result<bool, ReviewedPolicyError> {
    let items = command
        .get("hidden")
        .and_then(Value::as_array)
        .ok_or(ReviewedPolicyError::Shape)?;
    for item in items {
        let object = item.as_object().ok_or(ReviewedPolicyError::Shape)?;
        let item_dataset = object
            .get("dataset")
            .and_then(Value::as_str)
            .ok_or(ReviewedPolicyError::Shape)?;
        let item_column = object
            .get("column")
            .and_then(Value::as_str)
            .ok_or(ReviewedPolicyError::Shape)?;
        if item_dataset == dataset && item_column == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn physical_type(
    artifact: &Value,
    schema: &str,
    table: &str,
    column: &str,
    data_type: &str,
) -> Result<String, ReviewedPolicyError> {
    let tables = artifact
        .pointer("/evidence_envelope/catalog/tables")
        .and_then(Value::as_array)
        .ok_or(ReviewedPolicyError::Semantic)?;
    let Some(table_value) = tables
        .iter()
        .find(|item| field_is(item, "schema_ident", schema) && field_is(item, "name_ident", table))
    else {
        return Err(ReviewedPolicyError::Semantic);
    };
    let columns = table_value
        .get("columns")
        .and_then(Value::as_array)
        .ok_or(ReviewedPolicyError::Semantic)?;
    let Some(column_value) = columns
        .iter()
        .find(|item| field_is(item, "name_ident", column))
    else {
        return Err(ReviewedPolicyError::Semantic);
    };
    let typtype = column_value
        .get("typtype")
        .and_then(Value::as_str)
        .ok_or(ReviewedPolicyError::Semantic)?;
    let type_name = column_value
        .get("type_name")
        .and_then(Value::as_str)
        .ok_or(ReviewedPolicyError::Semantic)?;
    if typtype != "b" || !base_type(type_name) || type_name != data_type {
        return Err(ReviewedPolicyError::Semantic);
    }
    if table_rls(table_value) {
        return Err(ReviewedPolicyError::Rls);
    }
    Ok(type_name.to_owned())
}

fn base_type(name: &str) -> bool {
    matches!(name, "bool" | "int2" | "int4" | "int8" | "text" | "numeric")
}

fn table_rls(table: &Value) -> bool {
    bool_field(table, "rls_enabled")
        || bool_field(table, "rls_forced")
        || table
            .get("policies")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
}

fn compatible(physical: &str, semantic: SemanticType) -> bool {
    match physical {
        "bool" => semantic == SemanticType::BooleanFlag,
        // Quantity is an integer count. Money, ratio, and event time stay refused.
        // admit_one checks the sealed current decision before this match.
        "int2" | "int4" | "int8" => matches!(
            semantic,
            SemanticType::Identifier
                | SemanticType::ForeignKey
                | SemanticType::Code
                | SemanticType::StatusCode
                | SemanticType::Quantity
        ),
        "text" => matches!(
            semantic,
            SemanticType::Identifier
                | SemanticType::ForeignKey
                | SemanticType::EntityName
                | SemanticType::Code
                | SemanticType::StatusCode
                | SemanticType::Category
                | SemanticType::Address
                | SemanticType::Phone
                | SemanticType::Email
                | SemanticType::PersonName
                | SemanticType::FreeText
        ),
        // Quantity is the documented semantic for a numeric amount.
        // Money stays refused. Measure and amount are not enum variants.
        "numeric" => semantic == SemanticType::Quantity,
        _ => false,
    }
}

fn map_pii(level: PiiLevel, explicit_allow: bool) -> Result<PiiClass, ReviewedPolicyError> {
    match (level, explicit_allow) {
        (PiiLevel::None, false) => Ok(PiiClass::None),
        (PiiLevel::Personal | PiiLevel::Sensitive | PiiLevel::Restricted, true) => {
            Ok(PiiClass::Declared {
                explicit_allow: true,
            })
        }
        _ => Err(ReviewedPolicyError::PiiAllow),
    }
}

fn semantic_name(value: SemanticType) -> &'static str {
    match value {
        SemanticType::Identifier => "identifier",
        SemanticType::ForeignKey => "foreign_key",
        SemanticType::EntityName => "entity_name",
        SemanticType::Code => "code",
        SemanticType::StatusCode => "status_code",
        SemanticType::Category => "category",
        SemanticType::BooleanFlag => "boolean_flag",
        SemanticType::Money => "money",
        SemanticType::Quantity => "quantity",
        SemanticType::Ratio => "ratio",
        SemanticType::EventTime => "event_time",
        SemanticType::Date => "date",
        SemanticType::AuditTime => "audit_time",
        SemanticType::Geo => "geo",
        SemanticType::Address => "address",
        SemanticType::Phone => "phone",
        SemanticType::Email => "email",
        SemanticType::PersonName => "person_name",
        SemanticType::FreeText => "free_text",
        SemanticType::Json => "json",
        SemanticType::System => "system",
    }
}

fn group_policies(admitted: Vec<Admitted>, binding: &ReviewedBinding) -> Vec<ColumnAclPolicy> {
    let mut policies: Vec<ColumnAclPolicy> = Vec::new();
    for item in admitted {
        if let Some(policy) = policies.iter_mut().find(|policy| {
            policy.table.namespace == item.namespace && policy.table.name == item.table
        }) {
            policy.table.columns.push(item.column);
            continue;
        }
        policies.push(policy_for(item, binding));
    }
    policies
}

fn policy_for(item: Admitted, binding: &ReviewedBinding) -> ColumnAclPolicy {
    let dataset = format!("{}.{}", item.namespace, item.table);
    let context = binding.binding_vector.as_ref().and_then(Value::as_array).and_then(|entries| {
        entries.iter().find(|entry| entry["dataset"] == dataset)
    });
    ColumnAclPolicy {
        project: binding.identity.project.clone(),
        config_digest: context.and_then(|entry| entry["config_digest"].as_str()).unwrap_or(&binding.config_digest).to_owned(),
        generation: context.and_then(|entry| entry["source_generation"].as_i64()).unwrap_or(binding.generation),
        role: binding.role.clone(),
        login: binding.login.clone(),
        table: LogicalTable {
            namespace: item.namespace,
            name: item.table,
            columns: vec![item.column],
        },
        row_filter: None,
    }
}

fn forbidden_key(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(forbidden_key),
        Value::Object(map) => {
            map.keys().any(|key| FORBIDDEN_KEYS.contains(&key.as_str()))
                || map.values().any(forbidden_key)
        }
        _ => false,
    }
}

fn same(value: &Value, key: &str, expected: &str) -> bool {
    value.get(key).and_then(Value::as_str) == Some(expected)
}

fn nested_same(value: &Value, parent: &str, key: &str, expected: &str) -> bool {
    value
        .get(parent)
        .is_some_and(|child| same(child, key, expected))
}

fn field_is(value: &Value, key: &str, expected: &str) -> bool {
    same(value, key, expected)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or_default()
}

fn non_claims(provenance: &Value) -> bool {
    [
        "authentication",
        "authorization",
        "grants",
        "publication",
        "semantic_publication",
    ]
    .into_iter()
    .all(|key| same(provenance, key, "not_claimed"))
}

fn hash_eq(value: &Value, key: &str, expected: &str) -> bool {
    value
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|stored| hash_text(stored, expected))
}

fn hash_text(stored: &str, expected: &str) -> bool {
    hex64(stored) && hex64(expected) && stored == expected
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn unsigned_hash(wrapper: &Value) -> Option<String> {
    let mut copy = wrapper.clone();
    copy.as_object_mut()?.remove("wrapper_hash")?;
    Some(canonical_sha256(&copy))
}

fn manifest_preimage(draft: &Value) -> Option<String> {
    let mut copy = draft.clone();
    copy.get_mut("provenance")?
        .as_object_mut()?
        .remove("manifest_hash")?;
    Some(canonical_sha256(&copy))
}

fn canonical_sha256(value: &Value) -> String {
    payload_sha256(canonical_json(value).as_bytes())
}

fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(other)));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(type_name: &str, typtype: &str) -> Value {
        serde_json::json!({
            "evidence_envelope": {
                "catalog": {
                    "tables": [{
                        "schema_ident": "shop",
                        "name_ident": "entry",
                        "rls_enabled": false,
                        "rls_forced": false,
                        "policies": [],
                        "columns": [{
                            "name_ident": "amount",
                            "typtype": typtype,
                            "type_name": type_name
                        }]
                    }]
                }
            }
        })
    }

    #[test]
    fn numeric_quantity_is_the_only_new_admission() {
        assert!(base_type("numeric"));
        assert!(base_type("int8"));
        assert!(!base_type("float4"));
        assert!(!base_type("float8"));
        assert!(!base_type("date"));
        assert!(!base_type("varchar"));
        assert!(!base_type("numeric(18,4)"));
        assert!(compatible("numeric", SemanticType::Quantity));
        assert!(!compatible("numeric", SemanticType::Money));
        assert!(!compatible("numeric", SemanticType::Identifier));
        assert!(!compatible("numeric", SemanticType::Ratio));
        assert!(compatible("int8", SemanticType::Quantity));
        assert!(compatible("int2", SemanticType::Quantity));
        assert!(compatible("int4", SemanticType::Quantity));
        assert!(!compatible("bool", SemanticType::Quantity));
        assert!(!compatible("text", SemanticType::Quantity));
        assert!(!compatible("float4", SemanticType::Quantity));
        assert!(!compatible("float8", SemanticType::Quantity));
        assert!(!compatible("date", SemanticType::Quantity));
        let admitted = physical_type(
            &catalog("numeric", "b"),
            "shop",
            "entry",
            "amount",
            "numeric",
        );
        assert_eq!(admitted, Ok("numeric".to_string()));
        assert!(
            physical_type(
                &catalog("numeric", "b"),
                "shop",
                "entry",
                "amount",
                "numeric(18,4)"
            )
            .is_err()
        );
        assert!(
            physical_type(
                &catalog("numeric", "d"),
                "shop",
                "entry",
                "amount",
                "numeric"
            )
            .is_err()
        );
        assert!(
            physical_type(&catalog("float8", "b"), "shop", "entry", "amount", "float8").is_err()
        );
        assert!(
            physical_type(&catalog("float4", "b"), "shop", "entry", "amount", "float4").is_err()
        );
        assert!(physical_type(&catalog("date", "b"), "shop", "entry", "amount", "date").is_err());
        assert!(physical_type(&catalog("bool", "b"), "shop", "entry", "amount", "bool").is_ok());
        assert!(physical_type(&catalog("text", "b"), "shop", "entry", "amount", "text").is_ok());
        assert!(physical_type(&catalog("int8", "b"), "shop", "entry", "amount", "int8").is_ok());
    }
}

/// Recompile a genuine native reviewed input, preserving its protocol and source semantics.
/// Binding is the administrator's accepted import/role identity, never inferred from the current files.
pub fn compile_native_reviewed_policies(
    input_json: &str,
    output_json: &str,
    binding: &ReviewedBinding,
) -> Result<ReviewedColumnPolicies, ReviewedPolicyError> {
    check_budget(input_json)?;
    check_budget(output_json)?;
    let input = scan_value(input_json)?;
    let output = scan_value(output_json)?;
    if input["protocol"] != "oo.builder.reviewed-sealed-import-command/v1"
        || output["protocol"] != "oo.builder.reviewed-sealed-import-command/v1"
    {
        return Err(ReviewedPolicyError::Shape);
    }
    let expected = oo_builder::compile_reviewed_sealed_import(input_json)
        .map_err(|_| ReviewedPolicyError::Seal)?;
    if expected != canonical_json(&output) {
        return Err(ReviewedPolicyError::Seal);
    }
    let catalog = &input["catalog"];
    let provenance = &output["provenance"];
    let collection = binding.binding_vector.as_ref();
    let legacy_seal = provenance["schema_digest"] == binding.seal.schema_hash
        && provenance["config_digest"] == binding.config_digest
        && provenance["source_generation"].as_i64() == Some(binding.generation);
    let vector_seal = collection.is_some_and(|vector| {
        let catalog_entries = catalog["entries"].as_array();
        catalog["protocol"] == "oo.pipeline.sealed-import-collection/v1"
            && catalog["binding_vector"] == *vector
            && provenance["binding_vector"] == *vector
            && provenance["collection_sha256"] == binding.seal.schema_hash
            && vector.as_array().is_some_and(|items| {
                let mut seen = BTreeSet::new();
                !items.is_empty() && catalog_entries.is_some_and(|entries| entries.len() == items.len())
                    && items.iter().all(|entry| {
                    let Some(dataset) = entry["dataset"].as_str() else { return false; };
                    let Some(config) = entry["config_digest"].as_str() else { return false; };
                    let Some(source_generation) = entry["source_generation"].as_i64() else { return false; };
                    let Some(target_generation) = entry["target_generation"].as_i64() else { return false; };
                    let Some(stored_namespace) = entry["stored_namespace"].as_str() else { return false; };
                    let Some(stored_physical) = entry["stored_physical"].as_str() else { return false; };
                    let Some(source) = entry["source_id"].as_str() else { return false; };
                    let matches_catalog = catalog_entries.is_some_and(|entries| entries.iter().filter(|item| {
                        item["source_id"] == source && item["run_id"] == entry["run_id"]
                            && item["tables"][0]["logical_schema"] == dataset.split_once('.').map(|(schema, _)| schema).unwrap_or("")
                            && item["tables"][0]["logical_table"] == dataset.split_once('.').map(|(_, table)| table).unwrap_or("")
                    }).count() == 1);
                    seen.insert(dataset.to_owned())
                        && split_native_logical(dataset, &output["effective_oos"], true, &binding.seal.schema).is_some()
                        && !source.is_empty() && matches_catalog
                        && config.len() == 64 && config.bytes().all(|b| b.is_ascii_hexdigit())
                        && source_generation >= 1 && target_generation >= 1
                        && stored_namespace.starts_with("pn_") && stored_namespace.len() == 63
                        && stored_physical.starts_with("pt_") && stored_physical.len() == 63
                })
            })
    });
    let vector_sources = collection.and_then(Value::as_array).map(|items| {
        items.iter().filter_map(|entry| entry["source_id"].as_str()).collect::<BTreeSet<_>>()
    });
    let provenance_source_ok = match vector_sources {
        Some(sources) if sources.len() > 1 => binding.seal.source_id.is_empty()
            && (provenance["source_id"] == "" || provenance["source_id"].is_null()),
        _ => provenance["source_id"] == binding.seal.source_id,
    };
    if canonical_sha256(catalog) != binding.seal.artifact_sha256
        || !provenance_source_ok
        || !(if collection.is_some() { vector_seal } else { legacy_seal })
    {
        return Err(ReviewedPolicyError::Identity);
    }
    let effective = &output["effective_oos"];
    let id = &binding.identity;
    if effective["project"] != id.project
        || effective["ontology_version"] != id.ontology_version
        || effective["catalog_version"] != id.catalog_version
        || effective["policy"]["id"] != id.policy_id
        || effective["policy"]["version"] != id.policy_version
        || effective["knowledge"]["id"] != id.knowledge_id
        || effective["knowledge"]["version"] != id.knowledge_version
    {
        return Err(ReviewedPolicyError::Identity);
    }
    let model = parse_model(effective, collection.is_none().then_some(binding.seal.schema.as_str()))?;
    if let Some(vector) = collection {
        let entries = vector.as_array().ok_or(ReviewedPolicyError::Identity)?;
        if entries.len() != model.datasets.len()
            || !model.datasets.iter().all(|dataset| entries.iter().any(|entry| entry["dataset"] == dataset.dataset.as_str()))
        {
            return Err(ReviewedPolicyError::Identity);
        }
    }
    let decisions: Vec<DecisionIn> = serde_json::from_value(input["decisions"].clone())
        .map_err(|_| ReviewedPolicyError::Shape)?;
    if decisions.len() > MAX_DECISIONS || binding.allow.is_empty() {
        return Err(ReviewedPolicyError::Shape);
    }
    let mut admitted = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in &binding.allow {
        if !seen.insert((entry.dataset.clone(), entry.column.clone())) {
            return Err(ReviewedPolicyError::Shape);
        }
        let (namespace, table) = split_native_logical(&entry.dataset, effective, collection.is_some(), &binding.seal.schema)
            .ok_or(ReviewedPolicyError::Identity)?;
        let column = find_column(&model, &entry.dataset, &entry.column)
            .ok_or(ReviewedPolicyError::Decision { reason: "missing" })?;
        let decision = decisions
            .iter()
            .find(|d| d.dataset == entry.dataset && d.column == entry.column)
            .ok_or(ReviewedPolicyError::Decision { reason: "missing" })?;
        if decision.schema_hash != binding.seal.schema_hash
            || !classification_matches(column, decision)
            || hidden_has(&output, &entry.dataset, &entry.column)?
        {
            return Err(ReviewedPolicyError::Decision { reason: "mismatch" });
        }
        let semantic = column.semantic_type.ok_or(ReviewedPolicyError::Semantic)?;
        admitted.push(Admitted {
            namespace: namespace.to_owned(),
            table: table.to_owned(),
            column: ColumnSpec {
                name: column.name.as_str().to_owned(),
                status: ClassStatus::Confirmed,
                semantic_type: semantic_name(semantic).to_owned(),
                pii: map_pii(decision.pii, entry.explicit_allow)?,
            },
        });
    }
    let policies = group_policies(admitted, binding);
    let compiled = if collection.is_some() {
        crate::policy::compile_column_acl_native_vector_set(&policies)
    } else {
        compile_column_acl_set(&policies)
    }.map_err(ReviewedPolicyError::Policy)?;
    Ok(ReviewedColumnPolicies { policies, compiled })
}

#[cfg(test)]
mod native_review_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn vector_namespaces_must_be_in_the_reviewed_ontology() {
        let effective = json!({"ontology":{"include_schemas":["logistics","cust_east","cust_west"]}});
        assert_eq!(split_native_logical("cust_east.k_note", &effective, true, ""), Some(("cust_east", "k_note")));
        assert_eq!(split_native_logical("logistics.hubs", &effective, true, ""), Some(("logistics", "hubs")));
        assert!(split_native_logical("other.k_note", &effective, true, "").is_none());
        assert!(split_native_logical("cust_east.k_note", &effective, false, "logistics").is_none());
    }
    #[test]
    fn actual_native_compiler_output_binds_two_tables_and_rejects_tampering() {
        let catalog: Value = serde_json::from_str(include_str!(
            "../../oo-builder/tests/fixtures/w06-sealed-import-catalog.json"
        ))
        .unwrap();
        let draft = oo_builder::compile_sealed_import_catalog(&canonical_json(&catalog)).unwrap();
        let schema = catalog["tables"][0]["logical_schema"].as_str().unwrap();
        let decisions=draft.columns.iter().map(|c|json!({"dataset":c.dataset,"column":c.name,"schema_hash":draft.schema_digest,"fact_fingerprint":c.fact_fingerprint,"pii":"none","semantic_type":"identifier","reviewer":{"by":"fixture.owner","at":"2026-10-12"}})).collect::<Vec<_>>();
        let input = json!({"protocol":"oo.builder.reviewed-sealed-import-command/v1","catalog":catalog,"decisions":decisions,"settings":{"project":"native_fixture","ontology_version":"ont-native","catalog_version":draft.identity_hash,"policy":{"id":"pol-native","version":"1"},"knowledge":{"id":"know-native","version":"1"},"ontology":{"spec_version":"oos/v1","name":"native_fixture","locale":"en-US","include_schemas":[schema],"default_tier_preference":["raw"],"publish":{"include_unreviewed":[]},"classification":{"auto_accept":"rule_high_confidence"},"lifecycle":{"deprecation_min_releases":2},"compile":{"per_role":true,"budgets":{"readme_tokens":2000,"table_file_tokens":3000},"sample_values":{"default":"masked","max_per_column":0}}}}});
        let input = canonical_json(&input);
        let output = oo_builder::compile_reviewed_sealed_import(&input).unwrap();
        let binding = ReviewedBinding {
            identity: ReviewedIdentity {
                project: "native_fixture".into(),
                ontology_version: "ont-native".into(),
                catalog_version: draft.identity_hash.clone(),
                policy_id: "pol-native".into(),
                policy_version: "1".into(),
                knowledge_id: "know-native".into(),
                knowledge_version: "1".into(),
            },
            seal: ReviewedSourceSeal {
                source_id: draft.source_id.clone(),
                schema: schema.to_owned(),
                schema_hash: draft.schema_digest.clone(),
                artifact_sha256: canonical_sha256(&catalog),
            },
            config_digest: draft.config_digest.clone(),
            generation: draft.source_generation,
            role: "native_fixture".into(),
            login: "oo_target_admin".into(),
            binding_vector: None,
            allow: draft
                .columns
                .iter()
                .map(|c| ReviewedAllow {
                    dataset: c.dataset.clone(),
                    column: c.name.clone(),
                    explicit_allow: false,
                })
                .collect(),
        };
        let result = compile_native_reviewed_policies(&input, &output, &binding).unwrap();
        assert_eq!(result.policies.len(), 2);
        assert_eq!(
            result
                .policies
                .iter()
                .map(|p| p.table.columns.len())
                .sum::<usize>(),
            19
        );
        let mut tampered: Value = serde_json::from_str(&output).unwrap();
        tampered["provenance"]["source_engine"] = json!("pgsql");
        assert!(
            compile_native_reviewed_policies(&input, &canonical_json(&tampered), &binding).is_err()
        );
        let mut changed = binding.clone();
        changed.generation += 1;
        assert!(compile_native_reviewed_policies(&input, &output, &changed).is_err());
        changed = binding.clone();
        changed.seal.artifact_sha256 = "00".repeat(32);
        assert!(compile_native_reviewed_policies(&input, &output, &changed).is_err());
        let pg_retag = input.replace(
            "oo.builder.reviewed-sealed-import-command/v1",
            "oo.builder.reviewed-effective-oos-command/v1",
        );
        assert!(compile_native_reviewed_policies(&pg_retag, &output, &binding).is_err());
    }
}
