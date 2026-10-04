//! Bounded role Knowledge Pack compiler.
//!
//! `compile_role_pack` accepts one `oo.builder.effective-oos/v1` document and
//! one `oo.builder.role-visibility/v1` snapshot. The snapshot is an input from
//! a future policy adapter. This module does not authenticate the caller, does
//! not grant privileges, does not mount a filesystem, and does not publish.
//!
//! Visibility is the snapshot allowlist. Review state, declared provenance,
//! and classification never add a table or column to that allowlist. A column
//! the snapshot names must already be classified. Unknown PII stays out, and
//! naming it in the allowlist refuses the pack instead of inventing a class.
//! User-authored text is copied only when it passes the visibility closure.
//! That includes data types, units, labels, and calendar phrases and anchors.
//! Hidden names and hidden labels are matched after NFC. Quoted identifiers
//! use PostgreSQL doubled quotes. Verified SQL must parse as the supported
//! subset; this module does not execute SQL and is not a gateway.
//! `classification.auto_accept` is not applied.
//!
//! Samples are never emitted. The OOS model has no sample or most-common
//! field; a document that tries to carry one is rejected. RLS datasets omit
//! global statistics unless the snapshot itself says those statistics are
//! proven. This compiler does not re-prove that claim.
//!
//! Token counts are unsupported. Byte and entry ceilings are enforced and are
//! not token budgets. Domain files, file splitting, search, mount, and a
//! publish gate are unsupported.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;
use serde_json::Value;

use oo_core::config::{PiiLevel, SampleValues};
use oo_core::diagnostic::DiagnosticList;
use oo_core::json_scan::parse_json_value;
use oo_core::oos::{
    CalendarFile, ClassName, ClassState, DatasetColumn, DatasetFile, EntityFile, EnumFile,
    EnumValueStatus, GlossaryFile, ItemStatus, Metric, MetricStatus, MetricsFile, OntologyFile,
    RelationProvenance, Relationship, RelationshipsFile, ReviewState, Term, TermStatus,
    VerifiedExample, validate_calendar, validate_dataset, validate_enum, validate_glossary,
    validate_metrics, validate_relationships,
};
use oo_core::text::{BoundedText, Label, TableName};

use crate::canonical::{canonical_json, sha256_hex};
use crate::nfc::nfc;

pub const ROLE_PACK_PROTOCOL: &str = "oo.builder.role-pack/v1";
pub const ROLE_PACK_COMPILER: &str = "oo-builder/role-pack/v1";
pub const EFFECTIVE_PROTOCOL: &str = "oo.builder.effective-oos/v1";
pub const VISIBILITY_PROTOCOL: &str = "oo.builder.role-visibility/v1";
pub const MANIFEST_PROTOCOL: &str = "oo.builder.role-pack-manifest/v1";
pub const CACHE_PROTOCOL: &str = "oo.builder.role-pack-cache/v1";

const MAX_FILE_BYTES: u64 = 262_144;
const MAX_TOTAL_BYTES: u64 = 1_048_576;
const MAX_ENTRIES: u64 = 128;
const MAX_DATASETS: usize = 64;
const MAX_COLUMNS: usize = 256;
const MAX_RELATIONSHIPS: usize = 256;
const MAX_METRICS: usize = 128;
const MAX_TERMS: usize = 256;
const MAX_EXAMPLES: usize = 128;
const MAX_ENUMS: usize = 128;
const MAX_ENUM_VALUES: usize = 256;
const MAX_ENTITIES: usize = 64;
const MAX_PATH_BYTES: usize = 256;

const UNREVIEWED_TOKENS: &[&str] = &[
    "column_descriptions",
    "dataset_descriptions",
    "caveats",
    "relationships",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePackError {
    pub code: String,
    pub path: String,
    pub message: &'static str,
}

impl RolePackError {
    fn new(name: &'static str, path: impl Into<String>, message: &'static str) -> Self {
        Self {
            code: format!("{ROLE_PACK_PROTOCOL}/{name}"),
            path: path.into(),
            message,
        }
    }
}

impl fmt::Display for RolePackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}: {}",
            self.code, self.path, self.message
        )
    }
}

impl std::error::Error for RolePackError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePack {
    /// Mountable file set, manifest hashes, and pack digest. No principal.
    pub artifact: String,
    pub pack_digest: String,
    pub cache_key: String,
    pub cache_key_sha256: String,
    pub manifest_sha256: String,
    pub total_bytes: u64,
    pub entry_count: u64,
    files: BTreeMap<String, String>,
}

impl RolePack {
    /// Exact file lookup after path normalization. This is not a search and
    /// it does not touch a filesystem.
    pub fn file(&self, path: &str) -> Result<&str, RolePackError> {
        let key = normalize_pack_path(path)?;
        self.files.get(&key).map(String::as_str).ok_or_else(|| {
            RolePackError::new("path.absent", key, "that relative path is not in this pack")
        })
    }
}

/// Normalize one pack-relative path.
///
/// The result is Unicode NFC, uses `/`, and has no empty, `.`, or `..`
/// segment. Absolute paths, backslashes, and NUL are refused.
pub fn normalize_pack_path(path: &str) -> Result<String, RolePackError> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES {
        return Err(RolePackError::new(
            "path.invalid",
            "/",
            "a pack path must be a non-empty relative path inside the path budget",
        ));
    }
    if path.as_bytes().contains(&0)
        || path.contains('\\')
        || path.contains(':')
        || path.starts_with('/')
        || path.starts_with("./")
        || path.ends_with('/')
        || path.contains("//")
    {
        return Err(RolePackError::new(
            "path.invalid",
            "/",
            "a pack path must be a non-empty relative path inside the path budget",
        ));
    }
    let canonical = nfc(path);
    if canonical.chars().any(|ch| {
        let code = u32::from(ch);
        matches!(code, 0x00..=0x1F | 0x7F)
    }) {
        return Err(RolePackError::new(
            "path.invalid",
            "/",
            "a pack path must be a non-empty relative path inside the path budget",
        ));
    }
    let mut out = String::new();
    for (index, segment) in canonical.split('/').enumerate() {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(RolePackError::new(
                "path.invalid",
                "/",
                "a pack path must be a non-empty relative path inside the path budget",
            ));
        }
        if index > 0 {
            out.push('/');
        }
        out.push_str(segment);
    }
    if out.is_empty() || out.len() > MAX_PATH_BYTES {
        return Err(RolePackError::new(
            "path.invalid",
            "/",
            "a pack path must be a non-empty relative path inside the path budget",
        ));
    }
    Ok(out)
}

pub fn compile_role_pack(
    effective_json: &str,
    visibility_json: &str,
) -> Result<RolePack, RolePackError> {
    let effective_value = parse_json_value(effective_json, EFFECTIVE_PROTOCOL)
        .map_err(|list| reject_json(&list))?;
    let visibility_value = parse_json_value(visibility_json, VISIBILITY_PROTOCOL)
        .map_err(|list| reject_json(&list))?;
    refuse_unsupported(&effective_value, true)?;
    refuse_unsupported(&visibility_value, false)?;
    let effective: EffectiveDoc = serde_json::from_value(effective_value).map_err(|_| {
        RolePackError::new(
            "schema.rejected",
            "/",
            "the effective document does not match the role-pack contract",
        )
    })?;
    let visibility: VisibilityDoc = serde_json::from_value(visibility_value).map_err(|_| {
        RolePackError::new(
            "schema.rejected",
            "/",
            "the visibility snapshot does not match the role-pack contract",
        )
    })?;
    let model = bind(effective, visibility)?;
    let selected = select(&model)?;
    let files = render(&model, &selected)?;
    seal(&model, files)
}

fn reject_json(list: &DiagnosticList) -> RolePackError {
    match list.items.first() {
        Some(item) => RolePackError {
            code: item.code.clone(),
            path: item.path.clone(),
            message: "the JSON document was rejected",
        },
        None => RolePackError::new("json.rejected", "/", "the JSON document was rejected"),
    }
}

fn refuse_unsupported(value: &Value, effective: bool) -> Result<(), RolePackError> {
    let Some(object) = value.as_object() else {
        return Err(RolePackError::new(
            "schema.rejected",
            "/",
            "the document must be a JSON object",
        ));
    };
    let names = if effective {
        [
            "decisions",
            "resolution",
            "guides",
            "samples",
            "profiles",
            "domains",
            "candidates",
            "credentials",
            "privileges",
        ]
        .as_slice()
    } else {
        ["samples", "most_common_vals", "credentials", "privileges", "grants"].as_slice()
    };
    for name in names {
        if object.contains_key(*name) {
            return Err(RolePackError::new(
                "unsupported.field",
                format!("/{name}"),
                "that field is not a role-pack input and was not interpreted as authority",
            ));
        }
    }
    if !effective && let Some(datasets) = object.get("datasets").and_then(Value::as_array) {
        for (index, dataset) in datasets.iter().enumerate() {
            let Some(row) = dataset.as_object() else {
                continue;
            };
            for name in ["sample_values", "most_common_vals", "samples"] {
                if row.contains_key(name) {
                    return Err(RolePackError::new(
                        "unsupported.samples",
                        format!("/datasets/{index}/{name}"),
                        "samples are not accepted from a visibility snapshot",
                    ));
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectiveDoc {
    protocol: String,
    project: String,
    ontology_version: String,
    catalog_version: String,
    policy: VersionedId,
    knowledge: VersionedId,
    ontology: OntologyFile,
    datasets: Vec<DatasetFile>,
    #[serde(default)]
    relationships: Vec<Relationship>,
    #[serde(default)]
    metrics: Vec<Metric>,
    #[serde(default)]
    glossary: Vec<Term>,
    #[serde(default)]
    examples: Vec<VerifiedExample>,
    #[serde(default)]
    enums: Vec<EnumFile>,
    #[serde(default)]
    entities: Vec<EntityFile>,
    #[serde(default)]
    calendar: Option<CalendarFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionedId {
    id: String,
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibilityDoc {
    protocol: String,
    project: String,
    principal: String,
    role: String,
    ontology_version: String,
    catalog_version: String,
    policy: VersionedId,
    knowledge: VersionedId,
    issued_for_compiler: String,
    datasets: Vec<VisibleDataset>,
    limits: LimitsIn,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisibleDataset {
    dataset: String,
    row_security: String,
    global_stats: String,
    #[serde(default)]
    global_stats_proof: Option<StatsProof>,
    columns: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatsProof {
    #[serde(rename = "ref")]
    reference: String,
    proven: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitsIn {
    max_file_bytes: u64,
    max_total_bytes: u64,
    max_entries: u64,
}

struct Grant {
    rls: bool,
    global_stats: bool,
    proof_ref: Option<String>,
    columns: BTreeSet<String>,
}

struct Model {
    project: String,
    principal: String,
    role: String,
    ontology_version: String,
    catalog_version: String,
    policy_id: String,
    policy_version: String,
    knowledge_id: String,
    knowledge_version: String,
    ontology: OntologyFile,
    datasets: BTreeMap<String, DatasetFile>,
    relationships: Vec<Relationship>,
    metrics: Vec<Metric>,
    glossary: Vec<Term>,
    examples: Vec<VerifiedExample>,
    enums: Vec<EnumFile>,
    entities: Vec<EntityFile>,
    calendar: Option<CalendarFile>,
    grants: BTreeMap<String, Grant>,
    limits: LimitsIn,
    unreviewed: BTreeSet<String>,
}

struct Selected {
    relationships: Vec<Relationship>,
    metrics: Vec<Metric>,
    glossary: Vec<Term>,
    examples: Vec<VerifiedExample>,
    enums: Vec<EnumFile>,
    entities: Vec<EntityFile>,
}

fn bind(effective: EffectiveDoc, visibility: VisibilityDoc) -> Result<Model, RolePackError> {
    if effective.protocol != EFFECTIVE_PROTOCOL || visibility.protocol != VISIBILITY_PROTOCOL {
        return Err(RolePackError::new(
            "protocol.mismatch",
            "/protocol",
            "the document protocol is not the role-pack input this compiler accepts",
        ));
    }
    if visibility.issued_for_compiler != ROLE_PACK_COMPILER {
        return Err(RolePackError::new(
            "binding.compiler",
            "/issued_for_compiler",
            "the visibility snapshot is not bound to this compiler version",
        ));
    }
    check_slug(&effective.project, "/project")?;
    check_slug(&visibility.project, "/project")?;
    check_slug(&visibility.principal, "/principal")?;
    check_role(&visibility.role)?;
    check_version(&effective.ontology_version, "/ontology_version")?;
    check_version(&visibility.ontology_version, "/ontology_version")?;
    check_version(&effective.catalog_version, "/catalog_version")?;
    check_version(&visibility.catalog_version, "/catalog_version")?;
    check_slug(&effective.policy.id, "/policy/id")?;
    check_version(&effective.policy.version, "/policy/version")?;
    check_slug(&visibility.policy.id, "/policy/id")?;
    check_version(&visibility.policy.version, "/policy/version")?;
    check_slug(&effective.knowledge.id, "/knowledge/id")?;
    check_version(&effective.knowledge.version, "/knowledge/version")?;
    check_slug(&visibility.knowledge.id, "/knowledge/id")?;
    check_version(&visibility.knowledge.version, "/knowledge/version")?;
    same(
        &effective.project,
        &visibility.project,
        "binding.project",
        "/project",
        "the snapshot project does not match the effective document",
    )?;
    same(
        &effective.ontology_version,
        &visibility.ontology_version,
        "binding.ontology_version",
        "/ontology_version",
        "the snapshot ontology version does not match the effective document",
    )?;
    same(
        &effective.catalog_version,
        &visibility.catalog_version,
        "binding.catalog_version",
        "/catalog_version",
        "the snapshot catalog version does not match the effective document",
    )?;
    same(
        &effective.policy.id,
        &visibility.policy.id,
        "binding.policy",
        "/policy/id",
        "the snapshot policy identity does not match the effective document",
    )?;
    same(
        &effective.policy.version,
        &visibility.policy.version,
        "binding.policy",
        "/policy/version",
        "the snapshot policy version does not match the effective document",
    )?;
    same(
        &effective.knowledge.id,
        &visibility.knowledge.id,
        "binding.knowledge",
        "/knowledge/id",
        "the snapshot knowledge identity does not match the effective document",
    )?;
    same(
        &effective.knowledge.version,
        &visibility.knowledge.version,
        "binding.knowledge",
        "/knowledge/version",
        "the snapshot knowledge version does not match the effective document",
    )?;
    check_limits(&visibility.limits)?;
    if !effective.ontology.compile.per_role {
        return Err(RolePackError::new(
            "compile.per_role",
            "/ontology/compile/per_role",
            "this compiler only builds a pack for one role",
        ));
    }
    if effective.ontology.compile.sample_values.default == SampleValues::Raw {
        return Err(RolePackError::new(
            "sample.raw_refused",
            "/ontology/compile/sample_values/default",
            "raw samples are refused and are not downgraded into this pack",
        ));
    }
    if effective.ontology.include_schemas.is_empty() {
        return Err(RolePackError::new(
            "ontology.schemas",
            "/ontology/include_schemas",
            "the ontology names no schema",
        ));
    }
    let mut schemas = BTreeSet::new();
    for (index, schema) in effective.ontology.include_schemas.iter().enumerate() {
        if !schemas.insert(schema.as_str().to_owned()) {
            return Err(RolePackError::new(
                "ontology.schemas",
                format!("/ontology/include_schemas/{index}"),
                "a schema name is repeated",
            ));
        }
    }
    let mut unreviewed = BTreeSet::new();
    for (index, token) in effective.ontology.publish.include_unreviewed.iter().enumerate() {
        let name = token.as_str();
        if !UNREVIEWED_TOKENS.contains(&name) {
            return Err(RolePackError::new(
                "include_unreviewed.unsupported",
                format!("/ontology/publish/include_unreviewed/{index}"),
                "that include_unreviewed token is not implemented and was not treated as a grant",
            ));
        }
        if !unreviewed.insert(name.to_owned()) {
            return Err(RolePackError::new(
                "include_unreviewed.duplicate",
                format!("/ontology/publish/include_unreviewed/{index}"),
                "include_unreviewed repeats a token",
            ));
        }
    }
    if effective.datasets.len() > MAX_DATASETS {
        return Err(object_budget("/datasets"));
    }
    let mut datasets = BTreeMap::new();
    for dataset in &effective.datasets {
        let diagnostics = validate_dataset(dataset);
        if !diagnostics.is_empty() {
            return Err(RolePackError::new(
                "oos.invalid",
                diagnostics[0].path.clone(),
                "a dataset failed OOS validation",
            ));
        }
        let name = dataset.dataset.as_str();
        let Some((schema, _)) = name.split_once('.') else {
            return Err(RolePackError::new(
                "oos.invalid",
                "/datasets",
                "a dataset name is not schema.table",
            ));
        };
        if !schemas.contains(schema) {
            return Err(RolePackError::new(
                "dataset.schema",
                format!("/datasets/{name}"),
                "a dataset schema is not in include_schemas",
            ));
        }
        if dataset.columns.len() > MAX_COLUMNS {
            return Err(object_budget("/datasets"));
        }
        let mut columns = BTreeSet::new();
        for column in &dataset.columns {
            if !columns.insert(column.name.as_str().to_owned()) {
                return Err(RolePackError::new(
                    "column.duplicate",
                    format!("/datasets/{name}/columns/{}", column.name.as_str()),
                    "a column name is repeated",
                ));
            }
            reject_secret(column.description.as_ref().map(BoundedText::<4000>::as_str).unwrap_or(""))?;
        }
        reject_secret(dataset.description.as_ref().map(BoundedText::<4000>::as_str).unwrap_or(""))?;
        if let Some(source) = &dataset.source_name {
            reject_secret(source.as_str())?;
        }
        for key in &dataset.primary_key {
            if !columns.contains(key.as_str()) {
                return Err(RolePackError::new(
                    "dataset.primary_key",
                    format!("/datasets/{name}/primary_key"),
                    "a primary-key column is not on the dataset",
                ));
            }
        }
        for time in &dataset.time_columns {
            if !columns.contains(time.name.as_str()) {
                return Err(RolePackError::new(
                    "dataset.time_column",
                    format!("/datasets/{name}/time_columns"),
                    "a time column is not on the dataset",
                ));
            }
        }
        if datasets.insert(name.to_owned(), dataset.clone()).is_some() {
            return Err(RolePackError::new(
                "dataset.duplicate",
                format!("/datasets/{name}"),
                "a dataset name is repeated",
            ));
        }
    }
    if effective.relationships.len() > MAX_RELATIONSHIPS {
        return Err(object_budget("/relationships"));
    }
    if !effective.relationships.is_empty() {
        let diagnostics = validate_relationships(&RelationshipsFile {
            schema_version: effective.ontology.spec_version,
            relationships: effective.relationships.clone(),
        });
        if !diagnostics.is_empty() {
            return Err(RolePackError::new(
                "oos.invalid",
                diagnostics[0].path.clone(),
                "a relationship failed OOS validation",
            ));
        }
    }
    let mut relationship_names = BTreeSet::new();
    for relationship in &effective.relationships {
        if !relationship_names.insert(relationship.name.as_str().to_owned()) {
            return Err(RolePackError::new(
                "relationship.duplicate",
                format!("/relationships/{}", relationship.name.as_str()),
                "a relationship name is repeated",
            ));
        }
        require_end(
            &datasets,
            relationship.from.dataset.as_str(),
            &relationship.from.columns,
            "/relationships",
        )?;
        require_end(
            &datasets,
            relationship.to.dataset.as_str(),
            &relationship.to.columns,
            "/relationships",
        )?;
    }
    if effective.metrics.len() > MAX_METRICS {
        return Err(object_budget("/metrics"));
    }
    if !effective.metrics.is_empty() {
        let diagnostics = validate_metrics(&MetricsFile {
            schema_version: effective.ontology.spec_version,
            metrics: effective.metrics.clone(),
        });
        if !diagnostics.is_empty() {
            return Err(RolePackError::new(
                "oos.invalid",
                diagnostics[0].path.clone(),
                "a metric failed OOS validation",
            ));
        }
    }
    let mut metric_names = BTreeSet::new();
    for metric in &effective.metrics {
        if !metric_names.insert(metric.metric.as_str().to_owned()) {
            return Err(RolePackError::new(
                "metric.duplicate",
                format!("/metrics/{}", metric.metric.as_str()),
                "a metric name is repeated",
            ));
        }
        reject_secret(metric.sql.as_str())?;
        if let Some(control) = &metric.control_total {
            reject_secret(control.sql.as_str())?;
        }
    }
    if effective.glossary.len() > MAX_TERMS {
        return Err(object_budget("/glossary"));
    }
    if !effective.glossary.is_empty() {
        let diagnostics = validate_glossary(&GlossaryFile {
            schema_version: effective.ontology.spec_version,
            terms: effective.glossary.clone(),
        });
        if !diagnostics.is_empty() {
            return Err(RolePackError::new(
                "oos.invalid",
                diagnostics[0].path.clone(),
                "a glossary term failed OOS validation",
            ));
        }
    }
    let mut terms = BTreeSet::new();
    for term in &effective.glossary {
        if !terms.insert(term.term.as_str().to_owned()) {
            return Err(RolePackError::new(
                "glossary.duplicate",
                "/glossary",
                "a glossary term is repeated",
            ));
        }
        reject_secret(term.definition.as_str())?;
    }
    if effective.examples.len() > MAX_EXAMPLES {
        return Err(object_budget("/examples"));
    }
    let mut example_ids = BTreeSet::new();
    for example in &effective.examples {
        if !example_ids.insert(example.id.as_str().to_owned()) {
            return Err(RolePackError::new(
                "example.duplicate",
                format!("/examples/{}", example.id.as_str()),
                "an example id is repeated",
            ));
        }
        reject_secret(example.sql.as_str())?;
        reject_secret(example.question.as_str())?;
    }
    if effective.enums.len() > MAX_ENUMS {
        return Err(object_budget("/enums"));
    }
    let mut enum_names = BTreeSet::new();
    for enum_file in &effective.enums {
        let diagnostics = validate_enum(enum_file);
        if !diagnostics.is_empty() {
            return Err(RolePackError::new(
                "oos.invalid",
                diagnostics[0].path.clone(),
                "an enum failed OOS validation",
            ));
        }
        if enum_file.values.len() > MAX_ENUM_VALUES {
            return Err(object_budget("/enums"));
        }
        let name = enum_file.enum_name.as_str();
        if !enum_names.insert(name.to_owned()) {
            return Err(RolePackError::new(
                "enum.duplicate",
                format!("/enums/{name}"),
                "an enum name is repeated",
            ));
        }
        let Some((schema, table, column)) = split_column(name) else {
            return Err(RolePackError::new(
                "enum.unknown_column",
                format!("/enums/{name}"),
                "an enum path is not a known column",
            ));
        };
        require_known_column(&datasets, schema, table, column, &format!("/enums/{name}"))?;
    }
    if effective.entities.len() > MAX_ENTITIES {
        return Err(object_budget("/entities"));
    }
    let mut entity_names = BTreeSet::new();
    for entity in &effective.entities {
        let name = entity.entity.as_str();
        if !entity_names.insert(name.to_owned()) {
            return Err(RolePackError::new(
                "entity.duplicate",
                format!("/entities/{name}"),
                "an entity name is repeated",
            ));
        }
        require_known_column(
            &datasets,
            schema_of(entity.primary_dataset.as_str())?,
            table_of(entity.primary_dataset.as_str())?,
            entity.key.as_str(),
            &format!("/entities/{name}/key"),
        )?;
        require_known_column(
            &datasets,
            schema_of(entity.primary_dataset.as_str())?,
            table_of(entity.primary_dataset.as_str())?,
            entity.display.as_str(),
            &format!("/entities/{name}/display"),
        )?;
        for member in &entity.members {
            if !datasets.contains_key(member.dataset.as_str()) {
                return Err(RolePackError::new(
                    "entity.unknown_dataset",
                    format!("/entities/{name}"),
                    "an entity member names a dataset that is not in the effective document",
                ));
            }
            for column in member
                .key
                .iter()
                .map(TableName::as_str)
                .chain(member.name.as_ref().map(TableName::as_str))
                .chain(member.strong_key.as_ref().map(TableName::as_str))
                .chain(member.phone.as_ref().map(TableName::as_str))
            {
                require_known_column(
                    &datasets,
                    schema_of(member.dataset.as_str())?,
                    table_of(member.dataset.as_str())?,
                    column,
                    &format!("/entities/{name}"),
                )?;
            }
        }
        for dataset in &entity.datasets {
            if !datasets.contains_key(dataset.as_str()) {
                return Err(RolePackError::new(
                    "entity.unknown_dataset",
                    format!("/entities/{name}"),
                    "an entity dataset is not in the effective document",
                ));
            }
        }
    }
    if let Some(calendar) = &effective.calendar {
        let diagnostics = validate_calendar(calendar);
        if !diagnostics.is_empty() {
            return Err(RolePackError::new(
                "oos.invalid",
                diagnostics[0].path.clone(),
                "the calendar failed OOS validation",
            ));
        }
    }
    let grants = bind_grants(&datasets, &visibility.datasets)?;
    Ok(Model {
        project: effective.project,
        principal: visibility.principal,
        role: visibility.role,
        ontology_version: effective.ontology_version,
        catalog_version: effective.catalog_version,
        policy_id: effective.policy.id,
        policy_version: effective.policy.version,
        knowledge_id: effective.knowledge.id,
        knowledge_version: effective.knowledge.version,
        ontology: effective.ontology,
        datasets,
        relationships: effective.relationships,
        metrics: effective.metrics,
        glossary: effective.glossary,
        examples: effective.examples,
        enums: effective.enums,
        entities: effective.entities,
        calendar: effective.calendar,
        grants,
        limits: visibility.limits,
        unreviewed,
    })
}

fn bind_grants(
    datasets: &BTreeMap<String, DatasetFile>,
    rows: &[VisibleDataset],
) -> Result<BTreeMap<String, Grant>, RolePackError> {
    if rows.len() > MAX_DATASETS {
        return Err(object_budget("/datasets"));
    }
    let mut grants = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let path = format!("/datasets/{index}");
        let Some(dataset) = datasets.get(&row.dataset) else {
            return Err(RolePackError::new(
                "visibility.unknown_dataset",
                path,
                "the snapshot names a dataset that is not in the effective document",
            ));
        };
        if grants.contains_key(&row.dataset) {
            return Err(RolePackError::new(
                "visibility.duplicate_dataset",
                path,
                "the snapshot repeats a dataset",
            ));
        }
        if row.columns.is_empty() || row.columns.len() > MAX_COLUMNS {
            return Err(RolePackError::new(
                "visibility.columns",
                format!("{path}/columns"),
                "a visible dataset needs a non-empty column allowlist inside the column budget",
            ));
        }
        let rls = match row.row_security.as_str() {
            "none" => false,
            "rls" => true,
            _ => {
                return Err(RolePackError::new(
                    "row_security.unknown",
                    format!("{path}/row_security"),
                    "row security must be none or rls",
                ));
            }
        };
        let permitted = match row.global_stats.as_str() {
            "denied" => false,
            "permitted" => true,
            _ => {
                return Err(RolePackError::new(
                    "evidence.unknown",
                    format!("{path}/global_stats"),
                    "global statistics permission must be permitted or denied",
                ));
            }
        };
        let proof_ref = match &row.global_stats_proof {
            Some(proof) => {
                check_version(&proof.reference, &format!("{path}/global_stats_proof/ref"))?;
                if !proof.proven || !rls || !permitted {
                    return Err(RolePackError::new(
                        "evidence.unproven",
                        format!("{path}/global_stats_proof"),
                        "a global-statistics proof is accepted only for an rls dataset whose snapshot says it is proven",
                    ));
                }
                Some(proof.reference.clone())
            }
            None if rls && permitted => {
                return Err(RolePackError::new(
                    "evidence.unproven",
                    format!("{path}/global_stats"),
                    "rls global statistics require an explicit proven proof on the snapshot",
                ));
            }
            None => None,
        };
        let known: BTreeSet<&str> = dataset
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect();
        let mut columns = BTreeSet::new();
        for (column_index, column_name) in row.columns.iter().enumerate() {
            if !known.contains(column_name.as_str()) {
                return Err(RolePackError::new(
                    "visibility.unknown_column",
                    format!("{path}/columns/{column_index}"),
                    "the snapshot names a column that is not in the effective document",
                ));
            }
            if !columns.insert(column_name.clone()) {
                return Err(RolePackError::new(
                    "visibility.duplicate_column",
                    format!("{path}/columns/{column_index}"),
                    "the snapshot repeats a column",
                ));
            }
            let Some(column) = dataset
                .columns
                .iter()
                .find(|column| column.name.as_str() == column_name)
            else {
                return Err(RolePackError::new(
                    "visibility.unknown_column",
                    format!("{path}/columns/{column_index}"),
                    "the snapshot names a column that is not in the effective document",
                ));
            };
            if column.status == Some(ItemStatus::Rejected) {
                return Err(RolePackError::new(
                    "column.rejected",
                    format!("{path}/columns/{column_index}"),
                    "a rejected column cannot be compiled",
                ));
            }
            if column.pii.is_none() {
                return Err(RolePackError::new(
                    "classification.unknown_pii",
                    format!("{path}/columns/{column_index}"),
                    "unknown PII stays hidden and is not classified by this compiler",
                ));
            }
            match column.classification.as_ref().map(|record| &record.state) {
                Some(ClassState::Confirmed) => {}
                _ => {
                    return Err(RolePackError::new(
                        "classification.incomplete",
                        format!("{path}/columns/{column_index}"),
                        "an allowlisted column must already be classification confirmed",
                    ));
                }
            }
        }
        grants.insert(
            row.dataset.clone(),
            Grant {
                rls,
                global_stats: permitted,
                proof_ref,
                columns,
            },
        );
    }
    Ok(grants)
}

fn select(model: &Model) -> Result<Selected, RolePackError> {
    let mut relationships = Vec::new();
    for relationship in &model.relationships {
        if relationship.status == Some(ItemStatus::Rejected) {
            continue;
        }
        if !end_visible(
            model,
            relationship.from.dataset.as_str(),
            &relationship.from.columns,
        ) || !end_visible(
            model,
            relationship.to.dataset.as_str(),
            &relationship.to.columns,
        ) {
            continue;
        }
        let approved = relationship
            .review
            .as_ref()
            .is_some_and(|review| review.state == ReviewState::Approved);
        let declared = relationship.provenance == RelationProvenance::DeclaredFk;
        let unreviewed = model.unreviewed.contains("relationships");
        if approved || declared || unreviewed {
            relationships.push(relationship.clone());
        }
    }
    relationships.sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));

    let mut enums = Vec::new();
    for enum_file in &model.enums {
        let name = enum_file.enum_name.as_str();
        let Some((schema, table, column)) = split_column(name) else {
            continue;
        };
        let dataset = format!("{schema}.{table}");
        if column_visible(model, &dataset, column) {
            enums.push(enum_file.clone());
        }
    }
    enums.sort_by(|left, right| left.enum_name.as_str().cmp(right.enum_name.as_str()));
    let enum_names: BTreeSet<&str> = enums.iter().map(|item| item.enum_name.as_str()).collect();

    let mut metrics = Vec::new();
    for metric in &model.metrics {
        if !metric.certified || metric.status == MetricStatus::Draft {
            continue;
        }
        if metric_kept(model, metric, &enum_names) {
            metrics.push(metric.clone());
        }
    }
    metrics.sort_by(|left, right| left.metric.as_str().cmp(right.metric.as_str()));
    let metric_names: BTreeSet<&str> = metrics.iter().map(|item| item.metric.as_str()).collect();
    let relationship_names: BTreeSet<&str> = relationships
        .iter()
        .map(|item| item.name.as_str())
        .collect();

    let mut glossary = Vec::new();
    for term in &model.glossary {
        if term.status != Some(TermStatus::Approved) {
            continue;
        }
        if term_kept(model, term, &metric_names) {
            glossary.push(term.clone());
        }
    }
    glossary.sort_by(|left, right| left.term.as_str().cmp(right.term.as_str()));

    let mut examples = Vec::new();
    for example in &model.examples {
        if example_kept(model, example, &metric_names, &relationship_names) {
            examples.push(example.clone());
        }
    }
    examples.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));

    let mut entities = Vec::new();
    for entity in &model.entities {
        if entity_fully_visible(model, entity) {
            entities.push(entity.clone());
        }
    }
    entities.sort_by(|left, right| left.entity.as_str().cmp(right.entity.as_str()));
    Ok(Selected {
        relationships,
        metrics,
        glossary,
        examples,
        enums,
        entities,
    })
}

fn metric_kept(model: &Model, metric: &Metric, enums: &BTreeSet<&str>) -> bool {
    if !verified_sql(metric.sql.as_str()) {
        return false;
    }
    let mut texts = vec![metric.metric.as_str(), metric.label.as_str(), metric.sql.as_str()];
    if let Some(description) = &metric.description {
        texts.push(description.as_str());
    }
    for caveat in &metric.caveats {
        texts.push(caveat.as_str());
    }
    for synonym in &metric.synonyms {
        texts.push(synonym.as_str());
    }
    if let Some(unit) = &metric.unit {
        texts.push(unit.as_str());
    }
    if let Some(anchor) = metric.default_anchor {
        texts.push(anchor.as_str());
    }
    if let Some(control) = &metric.control_total {
        if !verified_sql(control.sql.as_str()) {
            return false;
        }
        texts.push(control.sql.as_str());
    }
    let mut forced = BTreeSet::new();
    if let Some(path) = &metric.time_dimension {
        if !path_visible(model, path.as_str()) {
            return false;
        }
        if let Some(dataset) = dataset_of_path(path.as_str()) {
            forced.insert(dataset);
        }
    }
    for filter in &metric.enum_filters {
        if !path_visible(model, filter.column.as_str()) || !path_visible(model, filter.enum_name.as_str())
        {
            return false;
        }
        if !enums.contains(filter.enum_name.as_str()) {
            return false;
        }
        if let Some(dataset) = dataset_of_path(filter.column.as_str()) {
            forced.insert(dataset);
        }
    }
    matches!(
        scan_texts(model, &texts, &forced, true),
        Scan::Ok { .. }
    )
}

fn term_kept(model: &Model, term: &Term, metrics: &BTreeSet<&str>) -> bool {
    let mut texts = vec![term.term.as_str(), term.definition.as_str()];
    let mut forced = BTreeSet::new();
    if let Some(mapping) = &term.maps_to {
        if let Some(dataset) = &mapping.dataset {
            if !model.grants.contains_key(dataset.as_str()) {
                return false;
            }
            forced.insert(dataset.as_str().to_owned());
        }
        if let Some(metric) = &mapping.metric
            && !metrics.contains(metric.as_str())
        {
            return false;
        }
        if mapping.dataset.is_none() && mapping.metric.is_none() {
            return false;
        }
        if let Some(filter) = &mapping.filter {
            texts.push(filter.as_str());
        }
    }
    matches!(
        scan_texts(model, &texts, &forced, mapping_requires_dataset(term)),
        Scan::Ok { .. }
    )
}

fn mapping_requires_dataset(term: &Term) -> bool {
    term.maps_to
        .as_ref()
        .is_some_and(|mapping| mapping.dataset.is_some())
        || term.maps_to.is_none()
}

fn example_kept(
    model: &Model,
    example: &VerifiedExample,
    metrics: &BTreeSet<&str>,
    relationships: &BTreeSet<&str>,
) -> bool {
    let mut forced = BTreeSet::new();
    for item in &example.uses {
        let text = item.as_str();
        if let Some(name) = text.strip_prefix("metric:") {
            if !metrics.contains(name) {
                return false;
            }
        } else if let Some(name) = text.strip_prefix("rel:") {
            if !relationships.contains(name) {
                return false;
            }
        } else if let Some(name) = text.strip_prefix("dataset:") {
            if !model.grants.contains_key(name) {
                return false;
            }
            forced.insert(name.to_owned());
        } else if let Some(path) = text.strip_prefix("column:") {
            if !path_visible(model, path) {
                return false;
            }
            if let Some(dataset) = dataset_of_path(path) {
                forced.insert(dataset);
            }
        } else {
            return false;
        }
    }
    if !verified_sql(example.sql.as_str()) {
        return false;
    }
    matches!(
        scan_texts(
            model,
            &[
                example.id.as_str(),
                example.sql.as_str(),
                example.question.as_str(),
            ],
            &forced,
            true
        ),
        Scan::Ok { .. }
    )
}

fn entity_fully_visible(model: &Model, entity: &EntityFile) -> bool {
    let primary = entity.primary_dataset.as_str();
    if !column_visible(model, primary, entity.key.as_str())
        || !column_visible(model, primary, entity.display.as_str())
    {
        return false;
    }
    for dataset in &entity.datasets {
        if !model.grants.contains_key(dataset.as_str()) {
            return false;
        }
    }
    for member in &entity.members {
        let dataset = member.dataset.as_str();
        if !model.grants.contains_key(dataset) {
            return false;
        }
        for column in member.key.iter() {
            if !column_visible(model, dataset, column.as_str()) {
                return false;
            }
        }
        for column in member
            .name
            .as_ref()
            .into_iter()
            .chain(member.strong_key.as_ref())
            .chain(member.phone.as_ref())
        {
            if !column_visible(model, dataset, column.as_str()) {
                return false;
            }
        }
    }
    true
}

enum Scan {
    Ok { datasets: BTreeSet<String> },
    Affected,
    Unbound,
}

/// Hidden names and labels match user text after NFC.
///
/// Qualifiers and aliases are included. A hidden column named `id` drops any
/// text that uses that token. Quoted identifiers use PostgreSQL doubled
/// quotes. This closure does not authorize SQL. Verified SQL has to parse
/// before it can be kept.
fn scan_texts(
    model: &Model,
    texts: &[&str],
    forced: &BTreeSet<String>,
    require_dataset: bool,
) -> Scan {
    let mut datasets = forced.clone();
    let mut bare = BTreeSet::new();
    for text in texts {
        let text = nfc(text);
        let idents = match all_idents(&text) {
            Ok(idents) => idents,
            Err(()) => return Scan::Affected,
        };
        if copies_hidden_label(model, &text, &idents) {
            return Scan::Affected;
        }
        match scan_one(model, &text) {
            Scan::Affected => return Scan::Affected,
            Scan::Unbound => return Scan::Unbound,
            Scan::Ok {
                datasets: found,
            } => {
                datasets.extend(found);
                bare.extend(idents);
            }
        }
    }
    for dataset in model.datasets.values() {
        let grant = model.grants.get(dataset.dataset.as_str());
        if grant.is_none() && ident_hit(&bare, dataset.label.as_str()) {
            return Scan::Affected;
        }
        for column in &dataset.columns {
            let visible = grant.is_some_and(|grant| grant.columns.contains(column.name.as_str()));
            if visible {
                continue;
            }
            if ident_hit(&bare, column.name.as_str()) {
                return Scan::Affected;
            }
            if let Some(label) = &column.label
                && ident_hit(&bare, label.as_str())
            {
                return Scan::Affected;
            }
        }
    }
    if require_dataset && datasets.is_empty() {
        return Scan::Unbound;
    }
    Scan::Ok { datasets }
}

fn ident_hit(bare: &BTreeSet<String>, candidate: &str) -> bool {
    let candidate = nfc(candidate);
    if !is_plain_ident(&candidate) {
        return false;
    }
    bare.iter()
        .any(|ident| ident.eq_ignore_ascii_case(&candidate))
}

fn copies_hidden_label(model: &Model, text: &str, idents: &BTreeSet<String>) -> bool {
    hidden_substrings(model).into_iter().any(|label| {
        label_in_text(text, &label) || idents.iter().any(|ident| label_eq(ident, &label))
    })
}

/// Equality on a decoded identifier. Non-ASCII labels stay case-sensitive.
fn label_eq(ident: &str, label: &str) -> bool {
    if !label.is_ascii() {
        ident == label
    } else {
        ident.eq_ignore_ascii_case(label)
    }
}

fn hidden_substrings(model: &Model) -> Vec<String> {
    let mut labels = Vec::new();
    for dataset in model.datasets.values() {
        let grant = model.grants.get(dataset.dataset.as_str());
        if grant.is_none() {
            push_substring(&mut labels, dataset.label.as_str());
        }
        for column in &dataset.columns {
            let visible = grant.is_some_and(|grant| grant.columns.contains(column.name.as_str()));
            if visible {
                continue;
            }
            if let Some(label) = &column.label {
                push_substring(&mut labels, label.as_str());
            }
            push_substring(&mut labels, column.name.as_str());
        }
    }
    labels
}

fn push_substring(labels: &mut Vec<String>, value: &str) {
    let value = nfc(value);
    if value.is_empty() || is_plain_ident(&value) {
        return;
    }
    labels.push(value);
}

fn label_in_text(text: &str, label: &str) -> bool {
    if !label.is_ascii() {
        text.contains(label)
    } else {
        text.to_ascii_lowercase()
            .contains(&label.to_ascii_lowercase())
    }
}

fn is_plain_ident(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(ch) if is_ident_start(ch) => chars.all(is_ident_cont),
        _ => false,
    }
}

fn scan_one(model: &Model, text: &str) -> Scan {
    let mut datasets = BTreeSet::new();
    let parts = match qualified_parts(text) {
        Ok(parts) => parts,
        Err(()) => return Scan::Affected,
    };
    for parts in parts {
        if parts.len() < 2 || parts.len() > 3 {
            continue;
        }
        let schema = parts[0].as_str();
        let table = parts[1].as_str();
        if is_system_schema(schema) {
            return Scan::Affected;
        }
        if is_platform_schema(schema) {
            continue;
        }
        match find_dataset(model, schema, table) {
            Some(dataset) => {
                let name = dataset.dataset.as_str().to_owned();
                if !model.grants.contains_key(&name) {
                    return Scan::Affected;
                }
                if parts.len() == 3 {
                    let column = parts[2].as_str();
                    let known = dataset
                        .columns
                        .iter()
                        .any(|item| item.name.as_str().eq_ignore_ascii_case(column));
                    if !known || !column_visible_ci(model, &name, column) {
                        return Scan::Affected;
                    }
                }
                datasets.insert(name);
            }
            None if schema_included(model, schema) => return Scan::Unbound,
            None => {}
        }
    }
    Scan::Ok { datasets }
}

struct Walk<'a> {
    text: &'a str,
    index: usize,
}

impl<'a> Walk<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, index: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.text[self.index..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.index += ch.len_utf8();
        Some(ch)
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }
}

/// PostgreSQL quoted identifiers, with `""` meaning one embedded quote.
/// An unclosed quote is an error so the caller can fail closed.
fn take_ident(walk: &mut Walk<'_>) -> Result<Option<String>, ()> {
    let Some(ch) = walk.peek() else {
        return Ok(None);
    };
    if ch == '"' {
        walk.bump();
        let mut out = String::new();
        loop {
            match walk.bump() {
                None => return Err(()),
                Some('"') => {
                    if walk.peek() == Some('"') {
                        walk.bump();
                        out.push('"');
                    } else {
                        break;
                    }
                }
                Some(ch) => out.push(ch),
            }
        }
        return Ok(Some(nfc(&out)));
    }
    if is_ident_start(ch) {
        let mut out = String::new();
        out.push(ch);
        walk.bump();
        while walk.peek().is_some_and(is_ident_cont) {
            out.push(walk.bump().expect("peeked ident byte"));
        }
        return Ok(Some(out));
    }
    Ok(None)
}

fn qualified_parts(text: &str) -> Result<Vec<Vec<String>>, ()> {
    let mut walk = Walk::new(text);
    let mut found = Vec::new();
    while walk.peek().is_some() {
        match take_ident(&mut walk)? {
            Some(first) => {
                let mut parts = vec![first];
                loop {
                    let mark = walk.index;
                    walk.skip_ws();
                    if walk.peek() != Some('.') {
                        walk.index = mark;
                        break;
                    }
                    walk.bump();
                    walk.skip_ws();
                    match take_ident(&mut walk)? {
                        Some(next) => parts.push(next),
                        None => {
                            walk.index = mark;
                            break;
                        }
                    }
                }
                if parts.len() >= 2 {
                    found.push(parts);
                }
            }
            None => {
                walk.bump();
            }
        }
    }
    Ok(found)
}

fn all_idents(text: &str) -> Result<BTreeSet<String>, ()> {
    let mut walk = Walk::new(text);
    let mut found = BTreeSet::new();
    while walk.peek().is_some() {
        match take_ident(&mut walk)? {
            Some(ident) => {
                found.insert(ident);
            }
            None => {
                walk.bump();
            }
        }
    }
    Ok(found)
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_cont(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

const SQL_KEYWORDS: &[&str] = &[
    "select", "from", "where", "union", "all", "as", "and", "is", "not", "null",
];

fn keyword_matches(walk: &Walk<'_>, keyword: &str) -> bool {
    let mut chars = walk.text[walk.index..].chars();
    for expected in keyword.chars() {
        match chars.next() {
            Some(ch) if ch.eq_ignore_ascii_case(&expected) => {}
            _ => return false,
        }
    }
    !chars.next().is_some_and(is_ident_cont)
}

fn keyword_here(walk: &Walk<'_>) -> bool {
    SQL_KEYWORDS.iter().any(|keyword| keyword_matches(walk, keyword))
}

fn eat_keyword(walk: &mut Walk<'_>, keyword: &str) -> bool {
    walk.skip_ws();
    if !keyword_matches(walk, keyword) {
        return false;
    }
    walk.index += keyword.len();
    true
}

fn eat_op(walk: &mut Walk<'_>) -> bool {
    walk.skip_ws();
    let rest = &walk.text[walk.index..];
    for op in ["<>", "!=", "<=", ">=", "=", "<", ">"] {
        if rest.starts_with(op) {
            walk.index += op.len();
            return true;
        }
    }
    false
}

/// Supported verified SQL. Rejection means the object is omitted.
/// Acceptance is not execution and not a grant.
fn verified_sql(text: &str) -> bool {
    let mut walk = Walk::new(text);
    if parse_query(&mut walk).is_err() {
        return false;
    }
    walk.skip_ws();
    walk.index == walk.text.len()
}

fn parse_query(walk: &mut Walk<'_>) -> Result<(), ()> {
    parse_select(walk)?;
    loop {
        let mark = walk.index;
        if eat_keyword(walk, "union") && eat_keyword(walk, "all") {
            parse_select(walk)?;
            continue;
        }
        walk.index = mark;
        break;
    }
    Ok(())
}

fn parse_select(walk: &mut Walk<'_>) -> Result<(), ()> {
    if !eat_keyword(walk, "select") {
        return Err(());
    }
    parse_item(walk)?;
    loop {
        walk.skip_ws();
        if walk.peek() != Some(',') {
            break;
        }
        walk.bump();
        parse_item(walk)?;
    }
    if !eat_keyword(walk, "from") {
        return Err(());
    }
    parse_relation(walk)?;
    if eat_keyword(walk, "where") {
        parse_predicate(walk)?;
    }
    Ok(())
}

fn parse_item(walk: &mut Walk<'_>) -> Result<(), ()> {
    parse_expr(walk)?;
    if eat_keyword(walk, "as") {
        parse_name(walk)?;
    }
    Ok(())
}

fn parse_relation(walk: &mut Walk<'_>) -> Result<(), ()> {
    parse_name(walk)?;
    walk.skip_ws();
    if walk.peek() == Some('.') {
        walk.bump();
        parse_name(walk)?;
    }
    Ok(())
}

fn parse_predicate(walk: &mut Walk<'_>) -> Result<(), ()> {
    parse_comparison(walk)?;
    while eat_keyword(walk, "and") {
        parse_comparison(walk)?;
    }
    Ok(())
}

fn parse_comparison(walk: &mut Walk<'_>) -> Result<(), ()> {
    parse_expr(walk)?;
    if eat_keyword(walk, "is") {
        if eat_keyword(walk, "not") {
            if eat_keyword(walk, "null") {
                return Ok(());
            }
            return Err(());
        }
        if eat_keyword(walk, "null") {
            return Ok(());
        }
        return Err(());
    }
    if !eat_op(walk) {
        return Err(());
    }
    parse_expr(walk)
}

fn parse_expr(walk: &mut Walk<'_>) -> Result<(), ()> {
    walk.skip_ws();
    if walk.peek() == Some('\'') {
        return parse_string(walk);
    }
    if walk.peek().is_some_and(|ch| ch.is_ascii_digit()) {
        return parse_number(walk);
    }
    parse_name(walk)?;
    walk.skip_ws();
    if walk.peek() == Some('(') {
        walk.bump();
        walk.skip_ws();
        if walk.peek() == Some('*') {
            walk.bump();
            walk.skip_ws();
            if walk.peek() != Some(')') {
                return Err(());
            }
            walk.bump();
            return Ok(());
        }
        parse_expr(walk)?;
        loop {
            walk.skip_ws();
            if walk.peek() != Some(',') {
                break;
            }
            walk.bump();
            parse_expr(walk)?;
        }
        walk.skip_ws();
        if walk.peek() != Some(')') {
            return Err(());
        }
        walk.bump();
        return Ok(());
    }
    loop {
        walk.skip_ws();
        if walk.peek() != Some('.') {
            break;
        }
        walk.bump();
        parse_name(walk)?;
    }
    Ok(())
}

fn parse_name(walk: &mut Walk<'_>) -> Result<(), ()> {
    walk.skip_ws();
    if keyword_here(walk) {
        return Err(());
    }
    match take_ident(walk)? {
        Some(_) => Ok(()),
        None => Err(()),
    }
}

fn parse_number(walk: &mut Walk<'_>) -> Result<(), ()> {
    if !walk.peek().is_some_and(|ch| ch.is_ascii_digit()) {
        return Err(());
    }
    while walk.peek().is_some_and(|ch| ch.is_ascii_digit()) {
        walk.bump();
    }
    if walk.peek() == Some('.') {
        let fraction = walk.text[walk.index + 1..].chars().next();
        if fraction.is_some_and(|ch| ch.is_ascii_digit()) {
            walk.bump();
            while walk.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                walk.bump();
            }
        }
    }
    Ok(())
}

fn parse_string(walk: &mut Walk<'_>) -> Result<(), ()> {
    if walk.bump() != Some('\'') {
        return Err(());
    }
    loop {
        match walk.bump() {
            None => return Err(()),
            Some('\'') => {
                if walk.peek() == Some('\'') {
                    walk.bump();
                } else {
                    return Ok(());
                }
            }
            Some(_) => {}
        }
    }
}

fn is_system_schema(schema: &str) -> bool {
    let folded = schema.to_ascii_lowercase();
    folded == "pg_catalog" || folded == "information_schema" || folded.starts_with("_oo")
}

fn is_platform_schema(schema: &str) -> bool {
    let folded = schema.to_ascii_lowercase();
    folded == "oo_dict" || folded == "oo_entity"
}

fn schema_included(model: &Model, schema: &str) -> bool {
    model
        .ontology
        .include_schemas
        .iter()
        .any(|item| item.as_str().eq_ignore_ascii_case(schema))
}

fn find_dataset<'a>(model: &'a Model, schema: &str, table: &str) -> Option<&'a DatasetFile> {
    model.datasets.values().find(|dataset| {
        let Some((found_schema, found_table)) = dataset.dataset.as_str().split_once('.') else {
            return false;
        };
        found_schema.eq_ignore_ascii_case(schema) && found_table.eq_ignore_ascii_case(table)
    })
}

fn column_visible(model: &Model, dataset: &str, column: &str) -> bool {
    model
        .grants
        .get(dataset)
        .is_some_and(|grant| grant.columns.contains(column))
}

fn column_visible_ci(model: &Model, dataset: &str, column: &str) -> bool {
    model.grants.get(dataset).is_some_and(|grant| {
        grant
            .columns
            .iter()
            .any(|item| item.eq_ignore_ascii_case(column))
    })
}

fn path_visible(model: &Model, path: &str) -> bool {
    let Some((schema, table, column)) = split_column(path) else {
        return false;
    };
    let Some(dataset) = find_dataset(model, schema, table) else {
        return false;
    };
    column_visible_ci(model, dataset.dataset.as_str(), column)
}

fn dataset_of_path(path: &str) -> Option<String> {
    let (schema, table, _) = split_column(path)?;
    Some(format!("{schema}.{table}"))
}

fn end_visible<T: AsRef<str>>(model: &Model, dataset: &str, columns: &[T]) -> bool {
    columns
        .iter()
        .all(|column| column_visible(model, dataset, column.as_ref()))
}

fn render(model: &Model, selected: &Selected) -> Result<BTreeMap<String, String>, RolePackError> {
    let mut files = BTreeMap::new();
    add_file(&mut files, "README.md", render_readme(model)?)?;
    add_file(&mut files, "INDEX.md", render_index(model)?)?;
    for (name, dataset) in &model.datasets {
        let Some(grant) = model.grants.get(name) else {
            continue;
        };
        let path = normalize_pack_path(&format!("tables/{name}.md"))?;
        add_file(&mut files, &path, render_table(model, selected, dataset, grant)?)?;
    }
    add_file(&mut files, "metrics.md", render_metrics(model, selected)?)?;
    add_file(&mut files, "calendar.md", render_calendar(model)?)?;
    add_file(&mut files, "glossary.md", render_glossary(model, selected)?)?;
    add_file(&mut files, "relationships.md", render_relationships(model, selected)?)?;
    add_file(&mut files, "examples/verified.sql", render_examples(model, selected)?)?;
    for enum_file in &selected.enums {
        let path = normalize_pack_path(&format!("enums/{}.md", enum_file.enum_name.as_str()))?;
        add_file(&mut files, &path, render_enum(model, enum_file)?)?;
    }
    for entity in &selected.entities {
        let path = normalize_pack_path(&format!("entities/{}.md", entity.entity.as_str()))?;
        add_file(&mut files, &path, render_entity(model, entity)?)?;
    }
    Ok(files)
}

fn add_file(
    files: &mut BTreeMap<String, String>,
    path: &str,
    text: String,
) -> Result<(), RolePackError> {
    let key = normalize_pack_path(path)?;
    reject_secret(&text)?;
    if files.insert(key, text).is_some() {
        return Err(RolePackError::new(
            "path.duplicate",
            path,
            "two pack files normalized to the same relative path",
        ));
    }
    Ok(())
}

fn render_readme(model: &Model) -> Result<String, RolePackError> {
    let mut schemas = BTreeSet::new();
    for name in model.grants.keys() {
        if let Some((schema, _)) = name.split_once('.') {
            schemas.insert(schema.to_owned());
        }
    }
    let mut text = String::new();
    text.push_str("# ");
    text.push_str(model.ontology.name.as_str());
    text.push_str("\n\n");
    line(
        &mut text,
        &format!("ontology_version: `{}`", model.ontology_version.as_str()),
    );
    line(&mut text, &format!("compiler: `{ROLE_PACK_COMPILER}`"));
    line(&mut text, &format!("locale: `{}`", model.ontology.locale.as_str()));
    line(&mut text, &format!("project: `{}`", model.project.as_str()));
    line(&mut text, &format!("role: `{}`", model.role.as_str()));
    text.push('\n');
    text.push_str("This file is a read-only role pack. It does not authenticate the caller and it does not grant privileges. Visibility came from a supplied policy snapshot.\n\n");
    text.push_str("Read README.md, then INDEX.md, then tables, metrics.md, and examples/verified.sql.\n\n");
    text.push_str("Visible schemas: ");
    text.push_str(&schemas.into_iter().collect::<Vec<_>>().join(", "));
    text.push_str("\n\nSamples are never included. This compiler has no fixed tokenizer, does not treat byte counts as tokens, and does not claim a publish gate.\n");
    finish(text)
}

fn render_index(model: &Model) -> Result<String, RolePackError> {
    let mut text = String::from("# INDEX\n\n");
    let mut schemas: BTreeMap<String, Vec<&DatasetFile>> = BTreeMap::new();
    for (name, dataset) in &model.datasets {
        if !model.grants.contains_key(name) {
            continue;
        }
        let Some((schema, _)) = name.split_once('.') else {
            continue;
        };
        schemas
            .entry(schema.to_owned())
            .or_default()
            .push(dataset);
    }
    if schemas.is_empty() {
        text.push_str("No dataset is visible.\n");
        return finish(text);
    }
    for (schema, datasets) in schemas {
        text.push_str("## ");
        text.push_str(&schema);
        text.push_str("\n\n");
        for dataset in datasets {
            text.push_str("- `");
            text.push_str(dataset.dataset.as_str());
            text.push('`');
            if visible_prose(model, dataset.label.as_str()) {
                text.push_str(" — ");
                text.push_str(dataset.label.as_str());
            }
            if let Some(grain) = &dataset.grain
                && visible_prose(model, grain.as_str())
            {
                text.push_str(" — ");
                text.push_str(grain.as_str());
            }
            text.push('\n');
        }
        text.push('\n');
    }
    finish(text)
}

fn render_table(
    model: &Model,
    selected: &Selected,
    dataset: &DatasetFile,
    grant: &Grant,
) -> Result<String, RolePackError> {
    let name = dataset.dataset.as_str();
    let mut text = String::new();
    text.push_str("# ");
    text.push_str(name);
    if visible_prose(model, dataset.label.as_str()) {
        text.push_str(" — ");
        text.push_str(dataset.label.as_str());
    }
    text.push_str("\n\n");
    line(&mut text, &format!("tier: `{}`", enum_token(&dataset.tier)?));
    if let Some(grain) = &dataset.grain
        && visible_prose(model, grain.as_str())
    {
        line(&mut text, &format!("grain: {}", grain.as_str()));
    }
    if let Some(source) = &dataset.source_name
        && visible_prose(model, source.as_str())
    {
        line(&mut text, &format!("source label: {}", source.as_str()));
    }
    let keys: Vec<&str> = dataset
        .primary_key
        .iter()
        .filter(|column| grant.columns.contains(column.as_str()))
        .map(TableName::as_str)
        .collect();
    if keys.is_empty() {
        line(&mut text, "primary key: not visible");
    } else {
        line(&mut text, &format!("primary key: `{}`", keys.join("`, `")));
    }
    text.push('\n');
    if grant.rls {
        text.push_str("Row security: rls. Cross-row samples and most_common values are omitted. ");
        if let Some(proof) = &grant.proof_ref {
            text.push_str("The snapshot marks global statistics as proven (");
            text.push_str(proof);
            text.push_str("). This compiler did not re-prove that claim.\n");
        } else {
            text.push_str("Global statistics are omitted. This compiler did not re-prove row security.\n");
        }
    } else {
        text.push_str("Row security: none. This compiler did not query the database.\n");
    }
    if grant.global_stats {
        if let Some(rows) = dataset.row_count_estimate {
            line(&mut text, &format!("estimated rows: {rows}"));
        }
    } else {
        text.push_str("Estimated row count is omitted.\n");
    }
    text.push('\n');
    if model.unreviewed.contains("dataset_descriptions")
        && let Some(description) = &dataset.description
        && visible_prose(model, description.as_str())
    {
        text.push_str("> UNREVIEWED: not an approved review, and not a grant.\n\n");
        text.push_str(&fence(description.as_str()));
        text.push('\n');
    }
    if model.unreviewed.contains("caveats") {
        for caveat in &dataset.caveats {
            if visible_prose(model, caveat.as_str()) {
                text.push_str("> UNREVIEWED: not an approved review, and not a grant.\n\n");
                text.push_str(&fence(caveat.as_str()));
                text.push('\n');
            }
        }
    }
    text.push_str("| column | label | type | semantic | pii | notes |\n");
    text.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for column in &dataset.columns {
        if !grant.columns.contains(column.name.as_str()) {
            continue;
        }
        text.push_str("| `");
        text.push_str(column.name.as_str());
        text.push_str("` | ");
        let label = column
            .label
            .as_ref()
            .map(Label::as_str)
            .filter(|label| visible_prose(model, label))
            .unwrap_or("");
        text.push_str(&cell(label));
        text.push_str(" | ");
        let data_type = column
            .data_type
            .as_ref()
            .map(Label::as_str)
            .filter(|text| visible_prose(model, text))
            .unwrap_or("");
        text.push_str(&cell(data_type));
        text.push_str(" | ");
        let semantic = match &column.semantic_type {
            Some(value) => enum_token(value)?,
            None => String::new(),
        };
        text.push_str(&cell(&semantic));
        text.push_str(" | ");
        let pii = column.pii.map(pii_token).unwrap_or("unknown");
        text.push_str(pii);
        text.push_str(" | ");
        text.push_str(&cell(&column_notes(model, column)?));
        text.push_str(" |\n");
    }
    text.push('\n');
    text.push_str("## relationships\n\n");
    let mut any = false;
    for relationship in &selected.relationships {
        if relationship.from.dataset.as_str() == name || relationship.to.dataset.as_str() == name {
            any = true;
            text.push_str("- `");
            text.push_str(relationship.name.as_str());
            text.push_str("` ");
            text.push_str(&end_text(relationship.from.dataset.as_str(), &relationship.from.columns));
            text.push_str(" -> ");
            text.push_str(&end_text(relationship.to.dataset.as_str(), &relationship.to.columns));
            text.push('\n');
        }
    }
    if !any {
        text.push_str("None.\n");
    }
    finish(text)
}

fn column_notes(model: &Model, column: &DatasetColumn) -> Result<String, RolePackError> {
    let mut notes = Vec::new();
    if let Some(pii) = column.pii
        && pii != PiiLevel::None
    {
        notes.push(format!(
            "Samples are omitted because pii is {}.",
            pii_token(pii)
        ));
    }
    if let Some(unit) = &column.unit
        && visible_prose(model, unit.as_str())
    {
        notes.push(format!("unit {}", unit.as_str()));
    }
    if let Some(status) = column.status {
        if status == ItemStatus::Deprecated {
            let replacement = match &column.replaced_by {
                Some(path) if path_visible(model, path.as_str()) => {
                    format!("Deprecated. Replacement: `{}`.", path.as_str())
                }
                Some(_) => "Deprecated. The replacement is not visible to this role.".to_owned(),
                None => "Deprecated.".to_owned(),
            };
            notes.push(replacement);
        } else if status != ItemStatus::Active {
            notes.push(format!("status {}", enum_token(&status)?));
        }
    }
    if let Some(path) = &column.enum_ref
        && path_visible(model, path.as_str())
    {
        notes.push(format!("enum `{}`", path.as_str()));
    }
    match description_text(
        model,
        column,
        model.unreviewed.contains("column_descriptions"),
    ) {
        Some((true, prose)) => notes.push(format!("UNREVIEWED: {prose}")),
        Some((false, prose)) => notes.push(prose),
        None => {}
    }
    Ok(notes.join(" "))
}

fn description_text(
    model: &Model,
    column: &DatasetColumn,
    unreviewed: bool,
) -> Option<(bool, String)> {
    let prose = column.description.as_ref()?.as_str();
    if !visible_prose(model, prose) {
        return None;
    }
    let approved = column
        .review
        .as_ref()
        .is_some_and(|review| review.state == ReviewState::Approved);
    if approved {
        Some((false, prose.to_owned()))
    } else if unreviewed {
        Some((true, prose.to_owned()))
    } else {
        None
    }
}

fn render_metrics(model: &Model, selected: &Selected) -> Result<String, RolePackError> {
    let mut text = String::from("# metrics\n\n");
    text.push_str("Only certified metrics with a visible dependency set are included. Reconciliation is not executed and is not claimed.\n\n");
    if selected.metrics.is_empty() {
        text.push_str("None.\n");
        return finish(text);
    }
    for metric in &selected.metrics {
        text.push_str("## ");
        text.push_str(metric.metric.as_str());
        if visible_prose(model, metric.label.as_str()) {
            text.push_str(" — ");
            text.push_str(metric.label.as_str());
        }
        text.push_str("\n\n");
        line(&mut text, &format!("status: `{}`", enum_token(&metric.status)?));
        if metric.status == MetricStatus::Deprecated {
            text.push_str("Deprecated. A replacement is not named unless it is a visible column path on the metric itself.\n");
        }
        if let Some(unit) = &metric.unit
            && visible_prose(model, unit.as_str())
        {
            line(&mut text, &format!("unit: {}", unit.as_str()));
        }
        if let Some(anchor) = metric.default_anchor {
            let token = enum_token(&anchor)?;
            if visible_prose(model, &token) {
                line(&mut text, &format!("default anchor: `{token}`"));
            }
        }
        let mut warned = BTreeSet::new();
        for filter in &metric.enum_filters {
            if enum_has_unconfirmed(selected, filter.enum_name.as_str()) {
                warned.insert(filter.enum_name.as_str().to_owned());
            }
        }
        for name in warned {
            text.push_str("> Unconfirmed codes: ");
            text.push_str(&name);
            text.push_str(" has undetermined or unexplained values. Those codes count in no class.\n\n");
        }
        text.push_str("> Reconciliation status is not in this pack. control_total was not executed and is not claimed to match.\n\n");
        if let Some(description) = &metric.description
            && visible_prose(model, description.as_str())
        {
            text.push_str(&fence(description.as_str()));
            text.push('\n');
        }
        if verified_sql(metric.sql.as_str()) && visible_prose(model, metric.sql.as_str()) {
            text.push_str("```sql\n");
            text.push_str(metric.sql.as_str().trim_end());
            text.push_str("\n```\n\n");
        }
    }
    finish(text)
}

fn enum_has_unconfirmed(selected: &Selected, name: &str) -> bool {
    selected
        .enums
        .iter()
        .find(|item| item.enum_name.as_str() == name)
        .is_some_and(|item| {
            item.values.iter().any(|value| {
                matches!(
                    value.status,
                    EnumValueStatus::Undetermined | EnumValueStatus::Unexplained
                )
            }) || !item.unexplained_values.is_empty()
        })
}

fn render_calendar(model: &Model) -> Result<String, RolePackError> {
    let mut text = String::from("# calendar\n\n");
    let Some(calendar) = &model.calendar else {
        text.push_str("The effective document has no calendar. This compiler does not substitute a civil month.\n");
        return finish(text);
    };
    line(&mut text, &format!("business time zone: `{}`", calendar.business_tz.as_str()));
    let month = calendar.fiscal_year_start_month;
    line(&mut text, &format!("fiscal year start month: {month}"));
    line(&mut text, &format!("week start: `{}`", enum_token(&calendar.week_start)?));
    text.push_str("\nAnchors:\n\n");
    let mut any_anchor = false;
    for anchor in &calendar.anchors {
        let token = enum_token(anchor)?;
        if !visible_prose(model, &token) {
            continue;
        }
        any_anchor = true;
        text.push_str("- `");
        text.push_str(&token);
        text.push_str("`\n");
    }
    if !any_anchor {
        text.push_str("None.\n");
    }
    text.push_str("\nPhrases:\n\n");
    let mut any_phrase = false;
    for (label, phrase) in calendar.phrases.iter() {
        if !phrase_visible(model, label.as_str(), phrase) {
            continue;
        }
        any_phrase = true;
        text.push_str("- ");
        text.push_str(label.as_str());
        text.push_str(": ");
        match phrase {
            oo_core::oos::Phrase::Range(range) => {
                text.push_str(range.start.as_str());
                text.push_str(" .. ");
                text.push_str(range.end.as_str());
            }
            oo_core::oos::Phrase::Comparison(item) => {
                text.push_str("current ");
                text.push_str(item.current.start.as_str());
                text.push_str(" .. ");
                text.push_str(item.current.end.as_str());
                text.push_str("; previous ");
                text.push_str(item.previous.start.as_str());
                text.push_str(" .. ");
                text.push_str(item.previous.end.as_str());
            }
        }
        text.push('\n');
    }
    if !any_phrase {
        text.push_str("None.\n");
    }
    finish(text)
}

fn phrase_visible(model: &Model, label: &str, phrase: &oo_core::oos::Phrase) -> bool {
    if !visible_prose(model, label) {
        return false;
    }
    match phrase {
        oo_core::oos::Phrase::Range(range) => {
            visible_prose(model, range.start.as_str()) && visible_prose(model, range.end.as_str())
        }
        oo_core::oos::Phrase::Comparison(item) => {
            visible_prose(model, item.current.start.as_str())
                && visible_prose(model, item.current.end.as_str())
                && visible_prose(model, item.previous.start.as_str())
                && visible_prose(model, item.previous.end.as_str())
        }
    }
}

fn render_glossary(model: &Model, selected: &Selected) -> Result<String, RolePackError> {
    let mut text = String::from("# glossary\n\n");
    text.push_str("Only terms with status approved and a visible binding are included.\n\n");
    if selected.glossary.is_empty() {
        text.push_str("None.\n");
        return finish(text);
    }
    for term in &selected.glossary {
        text.push_str("## ");
        if visible_prose(model, term.term.as_str()) {
            text.push_str(term.term.as_str());
        }
        text.push_str("\n\n");
        if visible_prose(model, term.definition.as_str()) {
            text.push_str(term.definition.as_str());
            text.push_str("\n\n");
        }
        if let Some(mapping) = &term.maps_to {
            if let Some(dataset) = &mapping.dataset {
                line(&mut text, &format!("maps to dataset `{}`", dataset.as_str()));
            }
            if let Some(metric) = &mapping.metric {
                line(&mut text, &format!("maps to metric `{}`", metric.as_str()));
            }
            if let Some(filter) = &mapping.filter
                && visible_prose(model, filter.as_str())
            {
                text.push_str(&fence(filter.as_str()));
            }
        }
        text.push('\n');
    }
    finish(text)
}

fn render_relationships(model: &Model, selected: &Selected) -> Result<String, RolePackError> {
    let mut text = String::from("# relationships\n\n");
    text.push_str("A relationship is included only when every endpoint column is on the allowlist and the claim is approved, source-declared, or explicitly include_unreviewed. Containment and query-log counts are never copied. Name match and type compatibility are included only when both ends allow global statistics.\n\n");
    if selected.relationships.is_empty() {
        text.push_str("None.\n");
        return finish(text);
    }
    for relationship in &selected.relationships {
        text.push_str("## ");
        text.push_str(relationship.name.as_str());
        text.push_str("\n\n");
        line(
            &mut text,
            &format!(
                "`{}` -> `{}`",
                end_text(relationship.from.dataset.as_str(), &relationship.from.columns),
                end_text(relationship.to.dataset.as_str(), &relationship.to.columns)
            ),
        );
        line(&mut text, &format!("cardinality: `{}`", enum_token(&relationship.cardinality)?));
        line(&mut text, &format!("join: `{}`", enum_token(&relationship.join_hint)?));
        let approved = relationship
            .review
            .as_ref()
            .is_some_and(|review| review.state == ReviewState::Approved);
        if relationship.provenance == RelationProvenance::DeclaredFk && !approved {
            text.push_str("Provenance: declared_fk (source-declared, not a grant).\n");
        } else if !approved {
            text.push_str("> UNREVIEWED: not an approved review, and not a grant.\n");
        } else {
            line(
                &mut text,
                &format!("provenance: `{}`", enum_token(&relationship.provenance)?),
            );
        }
        if stats_allowed(model, relationship.from.dataset.as_str())
            && stats_allowed(model, relationship.to.dataset.as_str())
            && let Some(evidence) = &relationship.evidence
        {
            if let Some(name_match) = evidence.name_match {
                line(&mut text, &format!("name match: `{}`", enum_token(&name_match)?));
            }
            if let Some(compatible) = evidence.type_compatible {
                line(&mut text, &format!("type compatible: {compatible}"));
            }
        }
        text.push('\n');
    }
    finish(text)
}

fn stats_allowed(model: &Model, dataset: &str) -> bool {
    model.grants.get(dataset).is_some_and(|grant| {
        grant.global_stats && (!grant.rls || grant.proof_ref.is_some())
    })
}

fn render_examples(model: &Model, selected: &Selected) -> Result<String, RolePackError> {
    let mut text = String::new();
    let mut wrote = false;
    for example in &selected.examples {
        if !verified_sql(example.sql.as_str())
            || !visible_prose(model, example.sql.as_str())
            || !visible_prose(model, example.question.as_str())
            || !visible_prose(model, example.id.as_str())
        {
            continue;
        }
        wrote = true;
        text.push_str("-- ");
        text.push_str(example.id.as_str());
        text.push('\n');
        text.push_str("-- verified_at: ");
        text.push_str(example.verified_at.as_str());
        text.push('\n');
        for line_text in example.question.as_str().lines() {
            text.push_str("-- ");
            text.push_str(line_text);
            text.push('\n');
        }
        text.push_str(example.sql.as_str().trim_end());
        text.push_str("\n\n");
    }
    if !wrote {
        text.push_str("-- No verified example is visible for this role.\n");
    }
    finish(text)
}

fn render_enum(model: &Model, enum_file: &EnumFile) -> Result<String, RolePackError> {
    let name = enum_file.enum_name.as_str();
    let mut text = String::new();
    text.push_str("# ");
    text.push_str(name);
    text.push_str("\n\n");
    text.push_str("Filter with `oo_dict.enum_values` and class. Do not copy codes into a negative predicate.\n\n");
    let dataset = dataset_of_path(name).unwrap_or_default();
    let stats = stats_allowed(model, &dataset);
    if stats && let Some(coverage) = &enum_file.coverage {
        line(&mut text, &format!("coverage: {}", enum_token(coverage)?));
        text.push('\n');
    } else if !stats {
        text.push_str("Coverage and evidence row counts are omitted.\n\n");
    }
    text.push_str("| code | label | class | status |\n| --- | --- | --- | --- |\n");
    for value in &enum_file.values {
        if !visible_prose(model, value.code.as_str()) {
            continue;
        }
        let value_label = value
            .label
            .as_ref()
            .map(Label::as_str)
            .filter(|label| visible_prose(model, label))
            .unwrap_or("");
        let class_name = value
            .class
            .as_ref()
            .map(ClassName::as_str)
            .filter(|class| visible_prose(model, class))
            .unwrap_or("");
        text.push_str("| `");
        text.push_str(&cell(value.code.as_str()));
        text.push_str("` | ");
        text.push_str(&cell(value_label));
        text.push_str(" | ");
        text.push_str(&cell(class_name));
        text.push_str(" | ");
        text.push_str(&enum_token(&value.status)?);
        text.push_str(" |\n");
        if stats && let Some(rows) = value.evidence.as_ref().and_then(|item| item.rows) {
            line(&mut text, &format!("Evidence rows for this code: {rows}."));
        }
        if matches!(
            value.status,
            EnumValueStatus::Undetermined | EnumValueStatus::Unexplained
        ) {
            text.push_str("\nMeaning is unconfirmed. It counts in no class. Filter by joining `oo_dict.enum_values` on class. Do not copy codes into a predicate.\n\n");
        }
    }
    if !enum_file.unexplained_values.is_empty() {
        text.push_str("\nUnexplained codes present in the enum document:\n\n");
        for code in &enum_file.unexplained_values {
            if !visible_prose(model, code.as_str()) {
                continue;
            }
            text.push_str("- `");
            text.push_str(&cell(code.as_str()));
            text.push_str("`\n");
        }
    }
    finish(text)
}

fn render_entity(model: &Model, entity: &EntityFile) -> Result<String, RolePackError> {
    let mut text = String::new();
    text.push_str("# ");
    text.push_str(entity.entity.as_str());
    if visible_prose(model, entity.label.as_str()) {
        text.push_str(" — ");
        text.push_str(entity.label.as_str());
    }
    text.push_str("\n\n");
    line(&mut text, &format!("primary dataset: `{}`", entity.primary_dataset.as_str()));
    line(&mut text, &format!("key: `{}`", entity.key.as_str()));
    line(&mut text, &format!("display: `{}`", entity.display.as_str()));
    text.push_str("\nThe member list is every member in the effective document. This compiler does not emit an entity when any member is hidden.\n\n");
    if entity.members.is_empty() {
        text.push_str("Members: none.\n");
    } else {
        text.push_str("Members:\n\n");
        for member in &entity.members {
            text.push_str("- `");
            text.push_str(member.dataset.as_str());
            text.push_str("` key `");
            text.push_str(
                &member
                    .key
                    .iter()
                    .map(TableName::as_str)
                    .collect::<Vec<_>>()
                    .join("`, `"),
            );
            text.push('`');
            if let Some(name) = &member.name {
                text.push_str(" name `");
                text.push_str(name.as_str());
                text.push('`');
            }
            text.push('\n');
        }
    }
    finish(text)
}

fn seal(model: &Model, mut files: BTreeMap<String, String>) -> Result<RolePack, RolePackError> {
    for (path, text) in &files {
        check_file_budget(path, text, &model.limits)?;
    }
    let listed = file_records(&files);
    let manifest = serde_json::json!({
        "authentication": "not_claimed",
        "authorization": "not_claimed",
        "catalog_version": model.catalog_version,
        "compiler": ROLE_PACK_COMPILER,
        "domains": "unsupported",
        "file_splitting": "unsupported",
        "files": listed,
        "knowledge": {"id": model.knowledge_id, "version": model.knowledge_version},
        "mount": "not_performed",
        "ontology_version": model.ontology_version,
        "policy": {"id": model.policy_id, "version": model.policy_version},
        "project": model.project,
        "protocol": MANIFEST_PROTOCOL,
        "publication": "not_claimed",
        "role": model.role,
        "row_security_reproven": false,
        "samples": "omitted",
        "search": "unsupported",
        "spec_version": "oos/v1",
        "tokens": {
            "estimator": "none",
            "readme_token_budget": model.ontology.compile.budgets.readme_tokens,
            "status": "unsupported",
            "table_file_token_budget": model.ontology.compile.budgets.table_file_tokens
        }
    });
    let manifest_text = canonical_json(&manifest);
    check_file_budget("manifest.json", &manifest_text, &model.limits)?;
    add_file(&mut files, "manifest.json", manifest_text.clone())?;
    let entry_count = files.len() as u64;
    if entry_count > model.limits.max_entries {
        return Err(RolePackError::new(
            "budget.entries",
            "/",
            "the pack has more files than the entry budget",
        ));
    }
    let total_bytes = files.values().map(|text| text.len() as u64).sum::<u64>();
    if total_bytes > model.limits.max_total_bytes {
        return Err(RolePackError::new(
            "budget.total",
            "/",
            "the pack is larger than the total byte budget",
        ));
    }
    let pack_digest = sha256_hex(canonical_json(&Value::Array(file_records(&files))).as_bytes());
    let project = model.project.as_str();
    let principal = model.principal.as_str();
    let role = model.role.as_str();
    let policy_id = model.policy_id.as_str();
    let policy_version = model.policy_version.as_str();
    let knowledge_id = model.knowledge_id.as_str();
    let knowledge_version = model.knowledge_version.as_str();
    let ontology_version = model.ontology_version.as_str();
    let catalog_version = model.catalog_version.as_str();
    let cache_key = format!(
        "{CACHE_PROTOCOL}|project={project}|principal={principal}|role={role}|policy={policy_id}@{policy_version}|knowledge={knowledge_id}@{knowledge_version}|ontology={ontology_version}|catalog={catalog_version}|compiler={ROLE_PACK_COMPILER}|pack={pack_digest}"
    );
    let cache_key_sha256 = sha256_hex(cache_key.as_bytes());
    let manifest_sha256 = sha256_hex(manifest_text.as_bytes());
    let mut artifact_files = Vec::new();
    for (path, text) in &files {
        artifact_files.push(serde_json::json!({
            "bytes": text.len(),
            "path": path,
            "sha256": sha256_hex(text.as_bytes()),
            "text": text,
        }));
    }
    let artifact = canonical_json(&serde_json::json!({
        "compiler": ROLE_PACK_COMPILER,
        "entry_count": entry_count,
        "files": artifact_files,
        "manifest_sha256": manifest_sha256,
        "pack_digest": pack_digest,
        "protocol": ROLE_PACK_PROTOCOL,
        "total_bytes": total_bytes,
    }));
    Ok(RolePack {
        artifact,
        pack_digest,
        cache_key,
        cache_key_sha256,
        manifest_sha256,
        total_bytes,
        entry_count,
        files,
    })
}

fn file_records(files: &BTreeMap<String, String>) -> Vec<Value> {
    files
        .iter()
        .map(|(path, text)| {
            serde_json::json!({
                "bytes": text.len(),
                "path": path,
                "sha256": sha256_hex(text.as_bytes()),
            })
        })
        .collect()
}

fn check_file_budget(path: &str, text: &str, limits: &LimitsIn) -> Result<(), RolePackError> {
    if text.len() as u64 > limits.max_file_bytes {
        return Err(RolePackError::new(
            "budget.file",
            path,
            "a pack file is larger than the file byte budget",
        ));
    }
    Ok(())
}

fn finish(text: String) -> Result<String, RolePackError> {
    let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
    while text.ends_with('\n') {
        text.pop();
    }
    text.push('\n');
    Ok(text)
}

fn visible_prose(model: &Model, text: &str) -> bool {
    matches!(
        scan_texts(model, &[text], &BTreeSet::new(), false),
        Scan::Ok { .. }
    )
}

fn line(text: &mut String, value: &str) {
    text.push_str(value);
    text.push('\n');
}

fn fence(text: &str) -> String {
    format!("```text\n{}\n```\n", text.replace("```", "'''"))
}

fn cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn end_text<T: AsRef<str>>(dataset: &str, columns: &[T]) -> String {
    format!(
        "{dataset}.{}",
        columns
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn enum_token<T: serde::Serialize>(value: &T) -> Result<String, RolePackError> {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => Ok(text),
        _ => Err(RolePackError::new(
            "render.enum",
            "/",
            "an enumeration did not render as a token",
        )),
    }
}

fn pii_token(level: PiiLevel) -> &'static str {
    match level {
        PiiLevel::None => "none",
        PiiLevel::Personal => "personal",
        PiiLevel::Sensitive => "sensitive",
        PiiLevel::Restricted => "restricted",
    }
}

fn object_budget(path: &str) -> RolePackError {
    RolePackError::new(
        "budget.objects",
        path,
        "the effective document exceeds an object budget",
    )
}

fn same(
    left: &str,
    right: &str,
    name: &'static str,
    path: &str,
    message: &'static str,
) -> Result<(), RolePackError> {
    if left == right {
        Ok(())
    } else {
        Err(RolePackError::new(name, path, message))
    }
}

fn check_slug(value: &str, path: &str) -> Result<(), RolePackError> {
    let bytes = value.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-');
    if ok {
        Ok(())
    } else {
        Err(RolePackError::new(
            "identity.invalid",
            path,
            "an identity token is outside the accepted pattern",
        ))
    }
}

fn check_role(value: &str) -> Result<(), RolePackError> {
    let Some(rest) = value.strip_prefix("oo_r_") else {
        return Err(RolePackError::new(
            "identity.invalid",
            "/role",
            "a role must use the oo_r_ name",
        ));
    };
    let bytes = rest.as_bytes();
    let ok = !bytes.is_empty()
        && value.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_');
    if ok {
        Ok(())
    } else {
        Err(RolePackError::new(
            "identity.invalid",
            "/role",
            "a role must use the oo_r_ name",
        ))
    }
}

fn check_version(value: &str, path: &str) -> Result<(), RolePackError> {
    let bytes = value.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'+' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(RolePackError::new(
            "identity.invalid",
            path,
            "a version token is outside the accepted pattern",
        ))
    }
}

fn check_limits(limits: &LimitsIn) -> Result<(), RolePackError> {
    if limits.max_file_bytes == 0
        || limits.max_file_bytes > MAX_FILE_BYTES
        || limits.max_total_bytes == 0
        || limits.max_total_bytes > MAX_TOTAL_BYTES
        || limits.max_entries == 0
        || limits.max_entries > MAX_ENTRIES
        || limits.max_total_bytes < limits.max_file_bytes
    {
        return Err(RolePackError::new(
            "limit.invalid",
            "/limits",
            "a limit is outside the compiler ceiling or cannot hold one file",
        ));
    }
    Ok(())
}

fn reject_secret(text: &str) -> Result<(), RolePackError> {
    let folded = text.to_ascii_lowercase();
    if folded.contains("secret://")
        || folded.contains("postgres://")
        || folded.contains("mysql://")
        || folded.contains("password=")
    {
        return Err(RolePackError::new(
            "source.credential",
            "/",
            "a source credential is not copied into a role pack",
        ));
    }
    Ok(())
}

fn require_end<T: AsRef<str>>(
    datasets: &BTreeMap<String, DatasetFile>,
    dataset: &str,
    columns: &[T],
    path: &str,
) -> Result<(), RolePackError> {
    for column in columns {
        let Some((schema, table)) = dataset.split_once('.') else {
            return Err(RolePackError::new(
                "relationship.unknown_column",
                path,
                "a relationship endpoint is not a known column",
            ));
        };
        require_known_column(datasets, schema, table, column.as_ref(), path)?;
    }
    Ok(())
}

fn require_known_column(
    datasets: &BTreeMap<String, DatasetFile>,
    schema: &str,
    table: &str,
    column: &str,
    path: &str,
) -> Result<(), RolePackError> {
    let name = format!("{schema}.{table}");
    let Some(dataset) = datasets.get(&name) else {
        return Err(RolePackError::new(
            "oos.unknown_column",
            path,
            "a referenced column is not in the effective document",
        ));
    };
    if dataset
        .columns
        .iter()
        .any(|item| item.name.as_str() == column)
    {
        Ok(())
    } else {
        Err(RolePackError::new(
            "oos.unknown_column",
            path,
            "a referenced column is not in the effective document",
        ))
    }
}

fn schema_of(dataset: &str) -> Result<&str, RolePackError> {
    dataset.split_once('.').map(|(schema, _)| schema).ok_or_else(|| {
        RolePackError::new(
            "oos.invalid",
            dataset,
            "a dataset name is not schema.table",
        )
    })
}

fn table_of(dataset: &str) -> Result<&str, RolePackError> {
    dataset.split_once('.').map(|(_, table)| table).ok_or_else(|| {
        RolePackError::new(
            "oos.invalid",
            dataset,
            "a dataset name is not schema.table",
        )
    })
}

fn split_column(path: &str) -> Option<(&str, &str, &str)> {
    let (schema, rest) = path.split_once('.')?;
    let (table, column) = rest.split_once('.')?;
    if column.contains('.') {
        None
    } else {
        Some((schema, table, column))
    }
}

