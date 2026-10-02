//! Compile a catalog-evidence envelope into a reviewable structural draft.
//!
//! Generated, curated, and effective layers stay separate. Primary keys and
//! foreign keys are structural facts and tentative candidates. Semantic type
//! and PII stay unknown. Nothing in this module connects, classifies, certifies,
//! or publishes.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Map, Value};

use oo_core::config::version::OosVersion;
use oo_core::json_scan::parse_json_value;
use oo_core::oos::{
    Cardinality, ColumnProvenance, DatasetColumn, DatasetFile, FactPin, ItemStatus, JoinHint,
    RelationEnd, RelationProvenance, Relationship, RelationshipsFile, Tier, merge_effective,
    validate_dataset, validate_relationships,
};
use oo_core::text::{HexDigest, Label, QualifiedTable, SourceName, TableName};

use crate::canonical::{canonical_json, canonical_sha256};
use crate::error::{DraftError, EVIDENCE_PROTOCOL};
use crate::limits::{MAX_CURATED_ITEMS, MAX_ITEM_ID_BYTES, MAX_REVIEWS, MAX_TABLES};
use crate::pgsql::{self, Catalog, Column, ForeignKey, Table, TypeReason};

pub const DRAFT_PROTOCOL: &str = "oo.builder.structure-draft/v1";
pub const COMPILER: &str = "oo-builder/structure-draft/v1";
const PG_CONTRACT_COMMIT: &str = "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74";

/// Compile one evidence envelope into canonical structure-draft JSON.
///
/// The returned bytes have no trailing newline. The same bytes are produced
/// for identical inputs and for inputs that differ only by list order.
pub fn compile_structure_draft(input: &str) -> Result<String, DraftError> {
    let value = parse_json_value(input, EVIDENCE_PROTOCOL).map_err(map_json)?;
    let envelope: EnvelopeIn = serde_json::from_value(value).map_err(map_envelope)?;
    check_versions(&envelope)?;
    if envelope.adapter != "pgsql" {
        return Err(DraftError::new(
            "adapter.unsupported",
            "/adapter",
            "catalog adapter is not supported",
        ));
    }
    let catalog = pgsql::load(&envelope.catalog)?;
    if envelope.source.source_id != catalog.source_id
        || envelope.source.database != catalog.database
    {
        return Err(DraftError::new(
            "source.mismatch",
            "/source",
            "envelope source does not match the catalog",
        ));
    }
    check_selection(&envelope.selection, &catalog)?;
    check_bindings(&envelope, &catalog)?;
    pgsql::reject_repeated_key_members(&catalog)?;
    let curated = parse_curated(envelope.curated)?;
    let reviews = parse_reviews(envelope.reviews)?;
    let plans = plan_tables(&catalog)?;
    let evidence_hash = envelope.bindings.evidence_hash.clone();
    let generated = generated_value(&plans)?;
    let (merge_curated, mut queue, states) = apply_reviews(&plans, &curated, &reviews, &catalog)?;
    let mut effective = merge_layers(&generated, &merge_curated, &catalog.schema_hash)?;
    strip_dangling_keys(&mut effective, &plans, &mut queue);
    strip_dangling_relationships(&mut effective, &plans, &mut queue);
    normalize_effective(&mut effective)?;
    for item in &mut queue {
        if let Some(state) = states.get(&(item.item_id.clone(), item.kind)) {
            item.state = (*state).to_string();
        }
    }
    queue.sort_by(|left, right| (&left.item_id, left.kind).cmp(&(&right.item_id, right.kind)));
    queue.dedup_by(|later, earlier| later.item_id == earlier.item_id && later.kind == earlier.kind);
    let document = document_value(
        &catalog,
        &generated,
        &curated.echo,
        &effective,
        &reviews,
        &queue,
        &evidence_hash,
    );
    Ok(seal(document))
}

fn map_json(err: oo_core::diagnostic::DiagnosticList) -> DraftError {
    let item = err.items.first();
    let code = item
        .map(|diag| diag.code.clone())
        .unwrap_or_else(|| format!("{EVIDENCE_PROTOCOL}/json.syntax"));
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
        "catalog evidence JSON is not valid"
    };
    DraftError::from_full(code, path, message)
}

fn map_envelope(err: serde_json::Error) -> DraftError {
    let text = err.to_string();
    let name = if text.contains("unknown field") {
        "field.unknown"
    } else if text.contains("missing field") {
        "field.missing"
    } else if text.contains("duplicate field") {
        "field.duplicate"
    } else {
        "schema.invalid"
    };
    DraftError::new(
        name,
        "/",
        "catalog evidence envelope is not oo.builder.catalog-evidence/v1",
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeIn {
    protocol: String,
    adapter: String,
    source: SourceIn,
    versions: VersionsIn,
    bindings: BindingsIn,
    selection: SelectionIn,
    catalog: Value,
    #[serde(default)]
    curated: Option<Value>,
    #[serde(default)]
    reviews: Vec<ReviewIn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceIn {
    source_id: String,
    database: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionsIn {
    schema: String,
    config: String,
    r#type: String,
    evidence: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingsIn {
    schema_hash: String,
    config_hash: String,
    type_hash: String,
    evidence_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionIn {
    tables: Vec<SelectedTable>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedTable {
    schema: String,
    name: String,
    columns: Vec<String>,
    keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewIn {
    item_id: String,
    action: String,
    schema_hash: String,
    fact_fingerprint: String,
}

#[derive(Debug)]
struct CuratedDoc {
    echo: Value,
    datasets: Vec<CuratedDataset>,
    relationships: Vec<CuratedRelationship>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratedIn {
    #[serde(default)]
    datasets: Vec<CuratedDataset>,
    #[serde(default)]
    relationships: Vec<CuratedRelationship>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratedDataset {
    dataset: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    grain: Option<String>,
    #[serde(default)]
    source_name: Option<String>,
    #[serde(default)]
    entity: Option<Value>,
    #[serde(default)]
    primary_key: Option<Vec<String>>,
    #[serde(default)]
    columns: Vec<CuratedColumn>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratedColumn {
    name: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    pii: Option<Value>,
    #[serde(default)]
    semantic_type: Option<Value>,
    #[serde(default)]
    classification: Option<Value>,
    #[serde(default)]
    review: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratedRelationship {
    name: String,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Accept,
    Reject,
    Override,
}

impl Action {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "accept" => Some(Self::Accept),
            "reject" => Some(Self::Reject),
            "override" => Some(Self::Override),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
            Self::Override => "override",
        }
    }
}

struct Review {
    item_id: String,
    action: Action,
    schema_hash: String,
    fact_fingerprint: String,
}

struct ColumnPlan {
    item_id: String,
    fingerprint: String,
    ident: String,
    reason: TypeReason,
    data_type: Option<String>,
    unsupported: bool,
}

struct FkPlan {
    item_id: String,
    fingerprint: String,
    ident: String,
    oos_name: Option<String>,
    parent_selected: bool,
    parent_qualified: Option<String>,
    key_verified: bool,
    ambiguous: bool,
    one_to_one: bool,
    local_idents: Vec<String>,
    ref_idents: Vec<String>,
}

struct TablePlan {
    schema_ident: String,
    name_ident: String,
    qualified: Option<String>,
    dataset_item: String,
    dataset_fingerprint: String,
    label: String,
    source_name: Option<String>,
    pk_item: String,
    pk_fingerprint: String,
    pk_idents: Vec<String>,
    columns: Vec<ColumnPlan>,
    fks: Vec<FkPlan>,
    emit: bool,
}

struct QueueItem {
    item_id: String,
    kind: &'static str,
    state: String,
    publishable: bool,
    fact_fingerprint: Option<String>,
    detail: &'static str,
}

fn check_versions(envelope: &EnvelopeIn) -> Result<(), DraftError> {
    if envelope.protocol != EVIDENCE_PROTOCOL {
        return Err(DraftError::new(
            "version.unsupported",
            "/protocol",
            "evidence protocol is not oo.builder.catalog-evidence/v1",
        ));
    }
    if envelope.versions.schema != pgsql::HASH_ALGORITHM
        || envelope.versions.config != pgsql::CORE_CONTRACT
        || envelope.versions.r#type != pgsql::PROTOCOL
        || envelope.versions.evidence != EVIDENCE_PROTOCOL
    {
        return Err(DraftError::new(
            "version.unsupported",
            "/versions",
            "schema, config, type, or evidence version is not supported",
        ));
    }
    Ok(())
}

fn check_selection(selection: &SelectionIn, catalog: &Catalog) -> Result<(), DraftError> {
    if selection.tables.len() > MAX_TABLES {
        return Err(DraftError::new(
            "budget.tables",
            "/selection/tables",
            "table count exceeds the draft budget",
        ));
    }
    let mut seen = BTreeSet::new();
    for table in &selection.tables {
        if !seen.insert((table.schema.clone(), table.name.clone())) {
            return Err(DraftError::new(
                "duplicate.selection",
                "/selection/tables",
                "duplicate selected table",
            ));
        }
        if has_duplicate(&table.columns) || has_duplicate(&table.keys) {
            return Err(DraftError::new(
                "duplicate.selection",
                "/selection/tables",
                "duplicate selected column or key",
            ));
        }
    }
    if selection.tables.len() != catalog.tables.len() {
        return Err(DraftError::new(
            "selection.mismatch",
            "/selection",
            "selection does not match the catalog",
        ));
    }
    for table in &catalog.tables {
        let Some(selected) = selection
            .tables
            .iter()
            .find(|item| item.schema == table.schema && item.name == table.name)
        else {
            return Err(DraftError::new(
                "selection.mismatch",
                "/selection",
                "selection does not match the catalog",
            ));
        };
        let columns: BTreeSet<&str> = table
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect();
        let selected_columns: BTreeSet<&str> =
            selected.columns.iter().map(String::as_str).collect();
        if columns.len() != table.columns.len() || columns != selected_columns {
            return Err(DraftError::new(
                "selection.mismatch",
                "/selection",
                "selection does not match the catalog",
            ));
        }
        let mut keys = BTreeSet::new();
        if !table.primary_key.is_empty() {
            keys.insert("primary_key");
        }
        for key in &table.unique_keys {
            keys.insert(key.name.as_str());
        }
        for key in &table.foreign_keys {
            keys.insert(key.name.as_str());
        }
        let selected_keys: BTreeSet<&str> = selected.keys.iter().map(String::as_str).collect();
        if keys != selected_keys {
            return Err(DraftError::new(
                "selection.mismatch",
                "/selection",
                "selection does not match the catalog",
            ));
        }
    }
    Ok(())
}

fn has_duplicate(items: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    items.iter().any(|item| !seen.insert(item))
}

fn check_bindings(envelope: &EnvelopeIn, catalog: &Catalog) -> Result<(), DraftError> {
    if envelope.bindings.schema_hash != catalog.schema_hash {
        return Err(DraftError::new(
            "hash.schema",
            "/bindings/schema_hash",
            "schema_hash does not match pg-catalog-hash/v1",
        ));
    }
    if envelope.bindings.config_hash != config_hash(catalog) {
        return Err(DraftError::new(
            "hash.config",
            "/bindings/config_hash",
            "config_hash does not match the source binding",
        ));
    }
    if envelope.bindings.type_hash != type_hash(catalog) {
        return Err(DraftError::new(
            "hash.type",
            "/bindings/type_hash",
            "type_hash does not match the physical types",
        ));
    }
    if envelope.bindings.evidence_hash != evidence_hash(catalog, &envelope.selection) {
        return Err(DraftError::new(
            "hash.evidence",
            "/bindings/evidence_hash",
            "evidence_hash does not match the selection and comments",
        ));
    }
    Ok(())
}

fn config_hash(catalog: &Catalog) -> String {
    canonical_sha256(&serde_json::json!({
        "adapter": "pgsql",
        "config_version": pgsql::CORE_CONTRACT,
        "database": catalog.database,
        "source_id": catalog.source_id,
    }))
}

fn type_hash(catalog: &Catalog) -> String {
    let mut columns = Vec::new();
    for table in &catalog.tables {
        for column in &table.columns {
            columns.push(serde_json::json!({
                "attnum": column.attnum,
                "base_type_oid": column.base_type_oid,
                "numeric_precision": column.numeric_precision,
                "numeric_scale": column.numeric_scale,
                "reason": column.reason.as_str(),
                "schema": table.schema,
                "table": table.name,
                "type_name": column.type_name,
                "type_oid": column.type_oid,
                "type_schema": column.type_schema,
                "typmod": column.typmod,
                "typtype": column.typtype,
            }));
        }
    }
    canonical_sha256(&serde_json::json!({
        "columns": columns,
        "type_conversion": "unsupported",
        "type_version": pgsql::PROTOCOL,
    }))
}

fn evidence_hash(catalog: &Catalog, selection: &SelectionIn) -> String {
    canonical_sha256(&serde_json::json!({
        "comments": comment_facts(catalog),
        "database": catalog.database,
        "evidence_version": EVIDENCE_PROTOCOL,
        "schema_hash": catalog.schema_hash,
        "selection": selection_facts(selection),
        "source_id": catalog.source_id,
        "type_hash": type_hash(catalog),
    }))
}

fn comment_facts(catalog: &Catalog) -> Vec<Value> {
    let mut out = Vec::new();
    for table in &catalog.tables {
        if let Some(comment) = &table.comment {
            out.push(serde_json::json!({
                "byte_len": comment.byte_len,
                "path": format!("table/{}.{}", table.schema_ident, table.name_ident),
                "sha256": comment.sha256,
            }));
        }
        for column in &table.columns {
            if let Some(comment) = &column.comment {
                out.push(serde_json::json!({
                    "byte_len": comment.byte_len,
                    "path": format!(
                        "column/{}.{}/{}",
                        table.schema_ident, table.name_ident, column.ident
                    ),
                    "sha256": comment.sha256,
                }));
            }
        }
    }
    out.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    out
}

fn selection_facts(selection: &SelectionIn) -> Vec<Value> {
    let mut tables = selection.tables.clone();
    tables.sort_by(|left, right| (&left.schema, &left.name).cmp(&(&right.schema, &right.name)));
    tables
        .into_iter()
        .map(|table| {
            let mut columns = table.columns;
            columns.sort();
            let mut keys = table.keys;
            keys.sort();
            serde_json::json!({
                "columns": columns,
                "keys": keys,
                "name": table.name,
                "schema": table.schema,
            })
        })
        .collect()
}

fn parse_curated(value: Option<Value>) -> Result<CuratedDoc, DraftError> {
    let Some(value) = value.filter(|item| !item.is_null()) else {
        return Ok(CuratedDoc {
            echo: serde_json::json!({"datasets": [], "relationships": []}),
            datasets: Vec::new(),
            relationships: Vec::new(),
        });
    };
    let parsed: CuratedIn = serde_json::from_value(value.clone()).map_err(|_| {
        DraftError::new(
            "schema.invalid",
            "/curated",
            "curated overlay is not a structure-draft overlay",
        )
    })?;
    let count = parsed.datasets.len()
        + parsed.relationships.len()
        + parsed
            .datasets
            .iter()
            .map(|dataset| dataset.columns.len())
            .sum::<usize>();
    if count > MAX_CURATED_ITEMS {
        return Err(DraftError::new(
            "budget.curated",
            "/curated",
            "curated overlay exceeds the draft budget",
        ));
    }
    let mut datasets_seen = BTreeSet::new();
    for dataset in &parsed.datasets {
        if !datasets_seen.insert(dataset.dataset.clone()) {
            return Err(DraftError::new(
                "curated.duplicate",
                "/curated/datasets",
                "duplicate curated dataset",
            ));
        }
        let mut columns = BTreeSet::new();
        for column in &dataset.columns {
            if !columns.insert(column.name.clone()) {
                return Err(DraftError::new(
                    "curated.duplicate",
                    "/curated/datasets/columns",
                    "duplicate curated column",
                ));
            }
        }
    }
    let mut relationships = BTreeSet::new();
    for relationship in &parsed.relationships {
        if !relationships.insert(relationship.name.clone()) {
            return Err(DraftError::new(
                "curated.duplicate",
                "/curated/relationships",
                "duplicate curated relationship",
            ));
        }
    }
    let mut echo = value;
    sort_echo(&mut echo);
    Ok(CuratedDoc {
        echo,
        datasets: parsed.datasets,
        relationships: parsed.relationships,
    })
}

fn sort_echo(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if let Some(Value::Array(datasets)) = object.get_mut("datasets") {
        datasets.sort_by(|left, right| field_str(left, "dataset").cmp(field_str(right, "dataset")));
        for dataset in datasets {
            if let Some(Value::Array(columns)) = dataset.get_mut("columns") {
                columns
                    .sort_by(|left, right| field_str(left, "name").cmp(field_str(right, "name")));
            }
        }
    }
    if let Some(Value::Array(relationships)) = object.get_mut("relationships") {
        relationships.sort_by(|left, right| field_str(left, "name").cmp(field_str(right, "name")));
    }
}

fn field_str<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn parse_reviews(reviews: Vec<ReviewIn>) -> Result<Vec<Review>, DraftError> {
    if reviews.len() > MAX_REVIEWS {
        return Err(DraftError::new(
            "budget.reviews",
            "/reviews",
            "review count exceeds the draft budget",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(reviews.len());
    for review in reviews {
        if review.item_id.is_empty()
            || review.item_id.len() > MAX_ITEM_ID_BYTES
            || review
                .item_id
                .chars()
                .any(|ch| (ch as u32) < 0x20 || ch == '\u{7f}')
        {
            return Err(DraftError::new(
                "field.invalid",
                "/reviews/item_id",
                "review item id is empty, too long, or contains a control",
            ));
        }
        if !seen.insert(review.item_id.clone()) {
            return Err(DraftError::new(
                "duplicate.decision",
                "/reviews",
                "duplicate review decision",
            ));
        }
        if !is_hex(&review.schema_hash) || !is_hex(&review.fact_fingerprint) {
            return Err(DraftError::new(
                "field.invalid",
                "/reviews",
                "review decision hash is not lowercase sha256",
            ));
        }
        let Some(action) = Action::parse(&review.action) else {
            return Err(DraftError::new(
                "review.action",
                "/reviews/action",
                "review action is not accept, reject, or override",
            ));
        };
        if is_out_of_scope(&review.item_id)
            || (review.item_id.starts_with("dataset/") && action == Action::Reject)
        {
            return Err(DraftError::new(
                "review.not_in_scope",
                "/reviews",
                "semantic, PII, type, and whole-dataset decisions are not in this slice",
            ));
        }
        out.push(Review {
            item_id: review.item_id,
            action,
            schema_hash: review.schema_hash,
            fact_fingerprint: review.fact_fingerprint,
        });
    }
    out.sort_by(|left, right| left.item_id.cmp(&right.item_id));
    Ok(out)
}

fn is_out_of_scope(item_id: &str) -> bool {
    [
        "pii/",
        "semantic/",
        "type/",
        "classification/",
        "review/",
        "entity/",
    ]
    .iter()
    .any(|prefix| item_id.starts_with(prefix))
}

fn is_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn plan_tables(catalog: &Catalog) -> Result<Vec<TablePlan>, DraftError> {
    let mut plans = Vec::with_capacity(catalog.tables.len());
    for table in &catalog.tables {
        plans.push(plan_table(catalog, table)?);
    }
    let mut used_names = BTreeSet::new();
    for plan in &mut plans {
        if !plan.emit {
            continue;
        }
        for fk in &mut plan.fks {
            if fk.parent_selected && fk.parent_qualified.is_some() {
                fk.oos_name = relationship_name(&plan.schema_ident, &fk.ident, &mut used_names);
            }
        }
    }
    let ambiguous = ambiguous_pairs(&plans);
    mark_ambiguous(&mut plans, &ambiguous);
    Ok(plans)
}

fn plan_table(catalog: &Catalog, table: &Table) -> Result<TablePlan, DraftError> {
    let qualified_text = format!("{}.{}", table.schema_ident, table.name_ident);
    let qualified = QualifiedTable::new(qualified_text.clone())
        .ok()
        .map(|item| item.to_string());
    let columns_fit = table
        .columns
        .iter()
        .all(|column| TableName::new(column.ident.clone()).is_ok());
    let emit = qualified.is_some() && columns_fit;
    let mut columns = Vec::with_capacity(table.columns.len());
    for column in &table.columns {
        let item_id = format!("column/{qualified_text}/{}", column.ident);
        check_item_id(&item_id)?;
        let data_type = match column.reason {
            TypeReason::KnownBuiltinNotConverted => Label::new(column.type_name.clone())
                .ok()
                .map(|_| column.type_name.clone()),
            _ => None,
        };
        columns.push(ColumnPlan {
            item_id,
            fingerprint: column_fingerprint(table, column),
            ident: column.ident.clone(),
            reason: column.reason,
            data_type,
            unsupported: column.reason != TypeReason::KnownBuiltinNotConverted,
        });
    }
    let pk_idents: Vec<String> = table
        .primary_key
        .iter()
        .map(|column| column.ident.clone())
        .collect();
    let pk_item = format!("primary_key/{qualified_text}");
    check_item_id(&pk_item)?;
    let dataset_item = format!("dataset/{qualified_text}");
    check_item_id(&dataset_item)?;
    let mut fks = Vec::with_capacity(table.foreign_keys.len());
    for key in &table.foreign_keys {
        fks.push(plan_fk(catalog, table, key, &qualified_text)?);
    }
    let label = table.name_ident.clone();
    let source_text = format!("{}.{}", table.schema, table.name);
    let source_name = Label::new(source_text.clone()).ok().map(|_| source_text);
    Ok(TablePlan {
        schema_ident: table.schema_ident.clone(),
        name_ident: table.name_ident.clone(),
        qualified,
        dataset_item,
        dataset_fingerprint: canonical_sha256(&serde_json::json!({
            "kind": "dataset",
            "name": table.name,
            "schema": table.schema,
        })),
        label,
        source_name,
        pk_item,
        pk_fingerprint: canonical_sha256(&serde_json::json!({
            "columns": table.primary_key.iter().map(|column| &column.name).collect::<Vec<_>>(),
            "kind": "primary_key",
            "schema": table.schema,
            "table": table.name,
        })),
        pk_idents,
        columns,
        fks,
        emit,
    })
}

fn plan_fk(
    catalog: &Catalog,
    table: &Table,
    key: &ForeignKey,
    qualified_text: &str,
) -> Result<FkPlan, DraftError> {
    let item_id = format!("relationship/{qualified_text}/{}", key.ident);
    check_item_id(&item_id)?;
    let parent = key.columns.first();
    let parent_selected = parent.is_some_and(|column| {
        catalog
            .tables
            .iter()
            .any(|item| item.schema == column.ref_schema && item.name == column.ref_table)
    });
    let parent_table = parent.and_then(|column| {
        catalog
            .tables
            .iter()
            .find(|item| item.schema == column.ref_schema && item.name == column.ref_table)
    });
    let parent_qualified = parent_table.and_then(|item| {
        QualifiedTable::new(format!("{}.{}", item.schema_ident, item.name_ident))
            .ok()
            .map(|value| value.to_string())
    });
    let ref_idents: Vec<String> = key
        .columns
        .iter()
        .map(|column| column.ref_ident.clone())
        .collect();
    let local_idents: Vec<String> = key
        .columns
        .iter()
        .map(|column| column.ident.clone())
        .collect();
    let key_verified = parent_table.is_some_and(|item| parent_key_matches(item, &ref_idents));
    let child_keys = std::iter::once(
        table
            .primary_key
            .iter()
            .map(|column| column.ident.clone())
            .collect::<Vec<_>>(),
    )
    .chain(table.unique_keys.iter().map(|unique| {
        unique
            .columns
            .iter()
            .map(|column| column.ident.clone())
            .collect()
    }));
    let one_to_one = child_keys.into_iter().any(|idents| idents == local_idents);
    Ok(FkPlan {
        item_id,
        fingerprint: fk_fingerprint(table, key),
        ident: key.ident.clone(),
        oos_name: None,
        parent_selected,
        parent_qualified,
        key_verified,
        ambiguous: false,
        one_to_one,
        local_idents,
        ref_idents,
    })
}

fn parent_key_matches(parent: &Table, ref_idents: &[String]) -> bool {
    let pk: Vec<&str> = parent
        .primary_key
        .iter()
        .map(|column| column.ident.as_str())
        .collect();
    if pk == ref_idents.iter().map(String::as_str).collect::<Vec<_>>() {
        return !pk.is_empty();
    }
    parent.unique_keys.iter().any(|key| {
        let idents: Vec<&str> = key
            .columns
            .iter()
            .map(|column| column.ident.as_str())
            .collect();
        idents == ref_idents.iter().map(String::as_str).collect::<Vec<_>>()
    })
}

fn column_fingerprint(table: &Table, column: &Column) -> String {
    canonical_sha256(&serde_json::json!({
        "attnum": column.attnum,
        "collation_oid": column.collation_oid,
        "generated": column.generated,
        "identity": column.identity,
        "name": column.name,
        "not_null": column.not_null,
        "schema": table.schema,
        "table": table.name,
        "type_oid": column.type_oid,
        "typmod": column.typmod,
        "typtype": column.typtype,
    }))
}

fn fk_fingerprint(table: &Table, key: &ForeignKey) -> String {
    let columns: Vec<Value> = key
        .columns
        .iter()
        .map(|column| {
            serde_json::json!({
                "name": column.name,
                "ref_name": column.ref_name,
                "ref_schema": column.ref_schema,
                "ref_table": column.ref_table,
            })
        })
        .collect();
    canonical_sha256(&serde_json::json!({
        "columns": columns,
        "constraint": key.name,
        "kind": "foreign_key",
        "schema": table.schema,
        "table": table.name,
    }))
}

fn relationship_name(
    schema_ident: &str,
    fk_ident: &str,
    used: &mut BTreeSet<String>,
) -> Option<String> {
    if let Ok(name) = SourceName::new(fk_ident.to_string()) {
        let text = name.to_string();
        if used.insert(text.clone()) {
            return Some(text);
        }
    }
    let qualified = format!("{schema_ident}__{fk_ident}");
    if let Ok(name) = SourceName::new(qualified) {
        let text = name.to_string();
        if used.insert(text.clone()) {
            return Some(text);
        }
    }
    None
}

fn ambiguous_pairs(plans: &[TablePlan]) -> BTreeSet<(String, String)> {
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for plan in plans {
        let Some(child) = &plan.qualified else {
            continue;
        };
        for fk in &plan.fks {
            let Some(parent) = &fk.parent_qualified else {
                continue;
            };
            *counts.entry((child.clone(), parent.clone())).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(pair, _)| pair)
        .collect()
}

fn mark_ambiguous(plans: &mut [TablePlan], pairs: &BTreeSet<(String, String)>) {
    for plan in plans {
        let Some(child) = plan.qualified.clone() else {
            continue;
        };
        for fk in &mut plan.fks {
            if let Some(parent) = &fk.parent_qualified {
                fk.ambiguous = pairs.contains(&(child.clone(), parent.clone()));
            }
        }
    }
}

fn check_item_id(item_id: &str) -> Result<(), DraftError> {
    if item_id.len() > MAX_ITEM_ID_BYTES {
        return Err(DraftError::new(
            "budget.item_id",
            "/catalog",
            "review item id exceeds the draft budget",
        ));
    }
    Ok(())
}

fn generated_value(plans: &[TablePlan]) -> Result<Value, DraftError> {
    let mut datasets = Vec::new();
    let mut relationships = Vec::new();
    for plan in plans {
        if !plan.emit {
            continue;
        }
        let Some(qualified) = qualified_of(plan) else {
            continue;
        };
        let mut columns = Vec::new();
        for column in &plan.columns {
            let Some(name) = TableName::new(column.ident.clone()).ok() else {
                continue;
            };
            columns.push(DatasetColumn {
                name,
                label: Label::new(column.ident.clone()).ok(),
                data_type: column
                    .data_type
                    .as_deref()
                    .and_then(|value| Label::new(value.to_string()).ok()),
                semantic_type: None,
                searchable: None,
                pii: None,
                classification: None,
                enum_ref: None,
                status: Some(ItemStatus::Draft),
                replaced_by: None,
                deprecated_at: None,
                unit: None,
                description: None,
                provenance: Some(ColumnProvenance::Declared),
                review: None,
            });
        }
        let mut primary_key = Vec::new();
        for ident in &plan.pk_idents {
            if let Ok(name) = TableName::new(ident.clone()) {
                primary_key.push(name);
            }
        }
        let dataset = DatasetFile {
            schema_version: OosVersion,
            dataset: qualified,
            source_name: plan
                .source_name
                .as_deref()
                .and_then(|value| Label::new(value.to_string()).ok()),
            label: Label::new(plan.label.clone()).map_err(|_| {
                DraftError::new(
                    "oos.invalid",
                    "/generated",
                    "generated dataset is not oos/v1",
                )
            })?,
            description: None,
            tier: Tier::Raw,
            recommended: Some(false),
            superseded_by: None,
            grain: None,
            primary_key,
            entity: None,
            row_count_estimate: None,
            time_columns: Vec::new(),
            columns,
            caveats: Vec::new(),
        };
        if !validate_dataset(&dataset).is_empty() {
            return Err(DraftError::new(
                "oos.invalid",
                "/generated",
                "generated dataset is not oos/v1",
            ));
        }
        datasets.push(serde_json::to_value(&dataset).map_err(|_| {
            DraftError::new(
                "oos.invalid",
                "/generated",
                "generated dataset is not oos/v1",
            )
        })?);
        for fk in &plan.fks {
            let Some(name) = fk
                .oos_name
                .as_deref()
                .and_then(|value| SourceName::new(value.to_string()).ok())
            else {
                continue;
            };
            let Some(parent) = fk
                .parent_qualified
                .as_deref()
                .and_then(|value| QualifiedTable::new(value.to_string()).ok())
            else {
                continue;
            };
            let child = qualified_of(plan).ok_or_else(|| {
                DraftError::new(
                    "oos.invalid",
                    "/generated",
                    "generated relationship is not oos/v1",
                )
            })?;
            let relationship = Relationship {
                name,
                from: RelationEnd {
                    dataset: child,
                    columns: names(&fk.local_idents)?,
                },
                to: RelationEnd {
                    dataset: parent,
                    columns: names(&fk.ref_idents)?,
                },
                cardinality: if fk.one_to_one {
                    Cardinality::OneToOne
                } else {
                    Cardinality::ManyToOne
                },
                join_hint: JoinHint::Left,
                provenance: RelationProvenance::DeclaredFk,
                confidence: None,
                evidence: None,
                review: None,
                status: Some(ItemStatus::Draft),
            };
            let file = RelationshipsFile {
                schema_version: OosVersion,
                relationships: vec![relationship.clone()],
            };
            if !validate_relationships(&file).is_empty() {
                return Err(DraftError::new(
                    "oos.invalid",
                    "/generated",
                    "generated relationship is not oos/v1",
                ));
            }
            relationships.push(serde_json::to_value(&relationship).map_err(|_| {
                DraftError::new(
                    "oos.invalid",
                    "/generated",
                    "generated relationship is not oos/v1",
                )
            })?);
        }
    }
    datasets.sort_by(|left, right| field_str(left, "dataset").cmp(field_str(right, "dataset")));
    relationships.sort_by(|left, right| field_str(left, "name").cmp(field_str(right, "name")));
    Ok(serde_json::json!({
        "datasets": datasets,
        "relationships": relationships,
    }))
}

fn qualified_of(plan: &TablePlan) -> Option<QualifiedTable> {
    plan.qualified
        .as_deref()
        .and_then(|value| QualifiedTable::new(value.to_string()).ok())
}

fn names(idents: &[String]) -> Result<Vec<TableName>, DraftError> {
    idents
        .iter()
        .map(|ident| {
            TableName::new(ident.clone()).map_err(|_| {
                DraftError::new(
                    "oos.invalid",
                    "/generated",
                    "generated relationship is not oos/v1",
                )
            })
        })
        .collect()
}

type ReviewStates = BTreeMap<(String, &'static str), &'static str>;

struct Consumption {
    datasets: BTreeSet<String>,
    columns: BTreeSet<(String, String)>,
    keys: BTreeSet<String>,
    relationships: BTreeSet<String>,
}

fn apply_reviews(
    plans: &[TablePlan],
    curated: &CuratedDoc,
    reviews: &[Review],
    catalog: &Catalog,
) -> Result<(Value, Vec<QueueItem>, ReviewStates), DraftError> {
    let mut queue = base_queue(plans);
    queue.extend(semantic_queue(curated, plans)?);
    let mut states: ReviewStates = BTreeMap::new();
    let mut datasets: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let mut relationships: BTreeMap<String, Value> = BTreeMap::new();
    let mut used = Consumption {
        datasets: BTreeSet::new(),
        columns: BTreeSet::new(),
        keys: BTreeSet::new(),
        relationships: BTreeSet::new(),
    };
    for review in reviews {
        let fresh = review.schema_hash == catalog.schema_hash
            && fingerprint_of(plans, &review.item_id).as_deref()
                == Some(review.fact_fingerprint.as_str());
        let known = fingerprint_of(plans, &review.item_id).is_some();
        if !known && review.schema_hash == catalog.schema_hash {
            return Err(DraftError::new(
                "review.unknown_item",
                "/reviews",
                "review item is not in this catalog",
            ));
        }
        if !fresh {
            queue.push(QueueItem {
                item_id: review.item_id.clone(),
                kind: "stale_decision",
                state: "pending".to_string(),
                publishable: false,
                fact_fingerprint: fingerprint_of(plans, &review.item_id),
                detail: if review.schema_hash != catalog.schema_hash {
                    "stale_schema_hash"
                } else {
                    "stale_fact_fingerprint"
                },
            });
            continue;
        }
        match review.action {
            Action::Accept => {
                if let Some(kind) = structural_kind(plans, &review.item_id) {
                    states.insert((review.item_id.clone(), kind), "accepted");
                }
            }
            Action::Reject => apply_reject(
                plans,
                &review.item_id,
                &mut datasets,
                &mut relationships,
                &mut states,
                &mut used,
            )?,
            Action::Override => apply_override(
                plans,
                curated,
                &review.item_id,
                &mut datasets,
                &mut relationships,
                &mut states,
                &mut used,
            )?,
        }
    }
    queue.extend(unbound_queue(curated, plans, &used)?);
    let merge = serde_json::json!({
        "datasets": datasets.into_values().map(Value::Object).collect::<Vec<_>>(),
        "relationships": relationships.into_values().collect::<Vec<_>>(),
    });
    Ok((merge, queue, states))
}

fn fingerprint_of(plans: &[TablePlan], item_id: &str) -> Option<String> {
    for plan in plans {
        if plan.dataset_item == item_id {
            return Some(plan.dataset_fingerprint.clone());
        }
        if plan.pk_item == item_id {
            return Some(plan.pk_fingerprint.clone());
        }
        for column in &plan.columns {
            if column.item_id == item_id {
                return Some(column.fingerprint.clone());
            }
        }
        for fk in &plan.fks {
            if fk.item_id == item_id {
                return Some(fk.fingerprint.clone());
            }
        }
    }
    None
}

fn structural_kind(plans: &[TablePlan], item_id: &str) -> Option<&'static str> {
    for plan in plans {
        if plan.pk_item == item_id {
            return Some("structural_primary_key");
        }
        if plan.columns.iter().any(|column| column.item_id == item_id) {
            return Some("structural_column");
        }
        if let Some(fk) = plan.fks.iter().find(|fk| fk.item_id == item_id) {
            if fk.oos_name.is_some() {
                return Some("candidate_relationship");
            }
            if !fk.parent_selected {
                return Some("fk_target_not_selected");
            }
            return Some("identifier_not_in_oos");
        }
    }
    None
}

fn dataset_slot<'a>(
    datasets: &'a mut BTreeMap<String, Map<String, Value>>,
    qualified: &str,
) -> &'a mut Map<String, Value> {
    datasets.entry(qualified.to_string()).or_insert_with(|| {
        let mut map = Map::new();
        map.insert("dataset".to_string(), Value::String(qualified.to_string()));
        map
    })
}

fn apply_reject(
    plans: &[TablePlan],
    item_id: &str,
    datasets: &mut BTreeMap<String, Map<String, Value>>,
    relationships: &mut BTreeMap<String, Value>,
    states: &mut ReviewStates,
    used: &mut Consumption,
) -> Result<(), DraftError> {
    for plan in plans {
        let Some(qualified) = &plan.qualified else {
            continue;
        };
        if plan.pk_item == item_id {
            dataset_slot(datasets, qualified)
                .insert("primary_key".to_string(), Value::Array(Vec::new()));
            states.insert((item_id.to_string(), "structural_primary_key"), "rejected");
            used.keys.insert(qualified.clone());
            return Ok(());
        }
        if let Some(column) = plan.columns.iter().find(|column| column.item_id == item_id) {
            push_column(datasets, qualified, column_reject(&column.ident));
            states.insert((item_id.to_string(), "structural_column"), "rejected");
            used.columns
                .insert((qualified.clone(), column.ident.clone()));
            return Ok(());
        }
        if let Some(fk) = plan.fks.iter().find(|fk| fk.item_id == item_id) {
            if let Some(name) = &fk.oos_name {
                relationships.insert(
                    name.clone(),
                    serde_json::json!({"name": name, "status": "rejected"}),
                );
                states.insert((item_id.to_string(), "candidate_relationship"), "rejected");
                used.relationships.insert(name.clone());
            } else if let Some(kind) = structural_kind(plans, item_id) {
                states.insert((item_id.to_string(), kind), "rejected");
            }
            return Ok(());
        }
    }
    if item_id.starts_with("dataset/") {
        return Err(DraftError::new(
            "review.not_in_scope",
            "/reviews",
            "semantic, PII, type, and whole-dataset decisions are not in this slice",
        ));
    }
    Err(DraftError::new(
        "review.unknown_item",
        "/reviews",
        "review item is not in this catalog",
    ))
}

fn apply_override(
    plans: &[TablePlan],
    curated: &CuratedDoc,
    item_id: &str,
    datasets: &mut BTreeMap<String, Map<String, Value>>,
    relationships: &mut BTreeMap<String, Value>,
    states: &mut ReviewStates,
    used: &mut Consumption,
) -> Result<(), DraftError> {
    for plan in plans {
        let Some(qualified) = &plan.qualified else {
            if plan.dataset_item == item_id
                || plan.columns.iter().any(|column| column.item_id == item_id)
            {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no emitted structural item",
                ));
            }
            continue;
        };
        if plan.dataset_item == item_id {
            let Some(dataset) = curated
                .datasets
                .iter()
                .find(|item| &item.dataset == qualified)
            else {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            };
            let slot = dataset_slot(datasets, qualified);
            let mut applied = false;
            if let Some(label) = &dataset.label {
                slot.insert("label".to_string(), Value::String(require_label(label)?));
                applied = true;
            }
            if let Some(description) = &dataset.description {
                slot.insert(
                    "description".to_string(),
                    Value::String(require_text(description)?),
                );
                applied = true;
            }
            if let Some(grain) = &dataset.grain {
                slot.insert("grain".to_string(), Value::String(require_label(grain)?));
                applied = true;
            }
            if let Some(source_name) = &dataset.source_name {
                slot.insert(
                    "source_name".to_string(),
                    Value::String(require_label(source_name)?),
                );
                applied = true;
            }
            if !applied {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            }
            used.datasets.insert(qualified.clone());
            return Ok(());
        }
        if plan.pk_item == item_id {
            let Some(dataset) = curated
                .datasets
                .iter()
                .find(|item| &item.dataset == qualified)
            else {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            };
            let Some(keys) = &dataset.primary_key else {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            };
            if has_duplicate(keys) {
                return Err(DraftError::new(
                    "curated.duplicate",
                    "/curated/datasets/primary_key",
                    "duplicate curated primary key column",
                ));
            }
            let known: BTreeSet<&str> = plan
                .columns
                .iter()
                .map(|column| column.ident.as_str())
                .collect();
            if keys.iter().any(|key| !known.contains(key.as_str())) {
                return Err(DraftError::new(
                    "curated.unknown_item",
                    "/curated/datasets/primary_key",
                    "curated primary key column is not in the table",
                ));
            }
            dataset_slot(datasets, qualified).insert(
                "primary_key".to_string(),
                Value::Array(keys.iter().cloned().map(Value::String).collect()),
            );
            used.keys.insert(qualified.clone());
            return Ok(());
        }
        if let Some(column) = plan.columns.iter().find(|column| column.item_id == item_id) {
            let Some(dataset) = curated
                .datasets
                .iter()
                .find(|item| &item.dataset == qualified)
            else {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            };
            let Some(curated_column) = dataset
                .columns
                .iter()
                .find(|item| item.name == column.ident)
            else {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            };
            let mut object = Map::new();
            object.insert("name".to_string(), Value::String(column.ident.clone()));
            let mut applied = false;
            if let Some(label) = &curated_column.label {
                object.insert("label".to_string(), Value::String(require_label(label)?));
                applied = true;
            }
            if let Some(description) = &curated_column.description {
                object.insert(
                    "description".to_string(),
                    Value::String(require_text(description)?),
                );
                applied = true;
            }
            if let Some(unit) = &curated_column.unit {
                object.insert("unit".to_string(), Value::String(require_label(unit)?));
                applied = true;
            }
            if let Some(status) = &curated_column.status {
                object.insert(
                    "status".to_string(),
                    Value::String(require_status(status)?.to_string()),
                );
                applied = true;
                if status == "rejected" {
                    states.insert((item_id.to_string(), "structural_column"), "rejected");
                }
            }
            if !applied {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            }
            push_column(datasets, qualified, Value::Object(object));
            used.columns
                .insert((qualified.clone(), column.ident.clone()));
            return Ok(());
        }
        if let Some(fk) = plan.fks.iter().find(|fk| fk.item_id == item_id) {
            let Some(name) = &fk.oos_name else {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no emitted structural item",
                ));
            };
            let Some(curated_rel) = curated.relationships.iter().find(|item| &item.name == name)
            else {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            };
            let Some(status) = &curated_rel.status else {
                return Err(DraftError::new(
                    "review.override_missing",
                    "/reviews",
                    "override has no curated structural fields",
                ));
            };
            let status = require_status(status)?;
            relationships.insert(
                name.clone(),
                serde_json::json!({"name": name, "status": status}),
            );
            if status == "rejected" {
                states.insert((item_id.to_string(), "candidate_relationship"), "rejected");
            }
            used.relationships.insert(name.clone());
            return Ok(());
        }
    }
    Err(DraftError::new(
        "review.override_missing",
        "/reviews",
        "override has no curated structural fields",
    ))
}

fn column_reject(ident: &str) -> Value {
    serde_json::json!({"name": ident, "status": "rejected"})
}

fn push_column(
    datasets: &mut BTreeMap<String, Map<String, Value>>,
    qualified: &str,
    column: Value,
) {
    let slot = dataset_slot(datasets, qualified);
    let columns = slot
        .entry("columns".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(items) = columns {
        items.push(column);
    }
}

fn require_label(value: &str) -> Result<String, DraftError> {
    Label::new(value.to_string())
        .map(|_| value.to_string())
        .map_err(|_| DraftError::new("curated.invalid", "/curated", "curated label is not valid"))
}

fn require_text(value: &str) -> Result<String, DraftError> {
    if value.chars().any(|ch| ch == '\0') || value.chars().count() > 4000 {
        return Err(DraftError::new(
            "curated.invalid",
            "/curated",
            "curated text is empty, too long, or contains NUL",
        ));
    }
    Ok(value.to_string())
}

fn require_status(value: &str) -> Result<&'static str, DraftError> {
    match value {
        "draft" => Ok("draft"),
        "rejected" => Ok("rejected"),
        _ => Err(DraftError::new(
            "curated.invalid",
            "/curated",
            "curated status must stay draft or rejected",
        )),
    }
}

fn base_queue(plans: &[TablePlan]) -> Vec<QueueItem> {
    let mut queue = Vec::new();
    for plan in plans {
        if !plan.emit {
            queue.push(pending(
                plan.dataset_item.clone(),
                "identifier_not_in_oos",
                None,
                "identifier_not_in_oos",
            ));
        }
        if plan.emit && !plan.pk_idents.is_empty() {
            queue.push(pending(
                plan.pk_item.clone(),
                "structural_primary_key",
                Some(plan.pk_fingerprint.clone()),
                "structural_fact",
            ));
        }
        for column in &plan.columns {
            queue.push(pending(
                column.item_id.clone(),
                "structural_column",
                Some(column.fingerprint.clone()),
                "structural_fact",
            ));
            let qualified = plan
                .qualified
                .clone()
                .unwrap_or_else(|| format!("{}.{}", plan.schema_ident, plan.name_ident));
            queue.push(pending(
                format!("pii/{qualified}/{}", column.ident),
                "pii_unknown",
                Some(column.fingerprint.clone()),
                "pii_unreviewed",
            ));
            queue.push(pending(
                format!("semantic/{qualified}/{}", column.ident),
                "semantic_unknown",
                Some(column.fingerprint.clone()),
                "semantic_unreviewed",
            ));
            if column.unsupported {
                queue.push(pending(
                    format!("type/{qualified}/{}", column.ident),
                    "type_unsupported",
                    Some(column.fingerprint.clone()),
                    column.reason.as_str(),
                ));
            }
        }
        for fk in &plan.fks {
            if !fk.parent_selected {
                queue.push(pending(
                    fk.item_id.clone(),
                    "fk_target_not_selected",
                    Some(fk.fingerprint.clone()),
                    "fk_target_not_selected",
                ));
                continue;
            }
            if fk.oos_name.is_some() {
                queue.push(pending(
                    fk.item_id.clone(),
                    "candidate_relationship",
                    Some(fk.fingerprint.clone()),
                    "join_hint_left_unverified",
                ));
            } else {
                queue.push(pending(
                    fk.item_id.clone(),
                    "identifier_not_in_oos",
                    Some(fk.fingerprint.clone()),
                    "relationship_name_exceeds_oos",
                ));
            }
            if fk.ambiguous {
                queue.push(pending(
                    fk.item_id.clone(),
                    "fk_ambiguous",
                    Some(fk.fingerprint.clone()),
                    "fk_ambiguous",
                ));
            }
            if !fk.key_verified {
                queue.push(pending(
                    fk.item_id.clone(),
                    "fk_target_key_unverified",
                    Some(fk.fingerprint.clone()),
                    "fk_target_key_unverified",
                ));
            }
        }
    }
    queue
}

fn pending(
    item_id: String,
    kind: &'static str,
    fact_fingerprint: Option<String>,
    detail: &'static str,
) -> QueueItem {
    QueueItem {
        item_id,
        kind,
        state: "pending".to_string(),
        publishable: false,
        fact_fingerprint,
        detail,
    }
}

fn semantic_queue(curated: &CuratedDoc, plans: &[TablePlan]) -> Result<Vec<QueueItem>, DraftError> {
    let mut queue = Vec::new();
    for dataset in &curated.datasets {
        let Some(plan) = plans
            .iter()
            .find(|plan| plan.qualified.as_deref() == Some(dataset.dataset.as_str()))
        else {
            return Err(DraftError::new(
                "curated.unknown_item",
                "/curated/datasets",
                "curated dataset is not in the generated draft",
            ));
        };
        if dataset.entity.is_some() {
            queue.push(pending(
                format!("entity/{}", dataset.dataset),
                "semantic_field_not_applied",
                Some(plan.dataset_fingerprint.clone()),
                "entity",
            ));
        }
        for column in &dataset.columns {
            if !plan.columns.iter().any(|item| item.ident == column.name) {
                return Err(DraftError::new(
                    "curated.unknown_item",
                    "/curated/datasets/columns",
                    "curated column is not in the generated draft",
                ));
            }
            for (present, id_prefix, detail) in [
                (column.pii.is_some(), "pii", "pii"),
                (column.semantic_type.is_some(), "semantic", "semantic_type"),
                (
                    column.classification.is_some(),
                    "classification",
                    "classification",
                ),
                (column.review.is_some(), "review", "review"),
            ] {
                if present {
                    queue.push(pending(
                        format!("{id_prefix}/{}/{}", dataset.dataset, column.name),
                        "semantic_field_not_applied",
                        fingerprint_of(
                            plans,
                            &format!("column/{}/{}", dataset.dataset, column.name),
                        ),
                        detail,
                    ));
                }
            }
        }
    }
    for relationship in &curated.relationships {
        let known = plans.iter().any(|plan| {
            plan.fks
                .iter()
                .any(|fk| fk.oos_name.as_deref() == Some(relationship.name.as_str()))
        });
        if !known {
            return Err(DraftError::new(
                "curated.unknown_item",
                "/curated/relationships",
                "curated relationship is not in the generated draft",
            ));
        }
    }
    Ok(queue)
}

fn unbound_queue(
    curated: &CuratedDoc,
    plans: &[TablePlan],
    used: &Consumption,
) -> Result<Vec<QueueItem>, DraftError> {
    let mut queue = Vec::new();
    for dataset in &curated.datasets {
        let Some(plan) = plans
            .iter()
            .find(|plan| plan.qualified.as_deref() == Some(dataset.dataset.as_str()))
        else {
            continue;
        };
        let structural = dataset.label.is_some()
            || dataset.description.is_some()
            || dataset.grain.is_some()
            || dataset.source_name.is_some();
        if structural && !used.datasets.contains(&dataset.dataset) {
            queue.push(pending(
                plan.dataset_item.clone(),
                "curated_unbound",
                Some(plan.dataset_fingerprint.clone()),
                "curated_not_applied",
            ));
        }
        if dataset.primary_key.is_some() && !used.keys.contains(&dataset.dataset) {
            queue.push(pending(
                plan.pk_item.clone(),
                "curated_unbound",
                Some(plan.pk_fingerprint.clone()),
                "curated_not_applied",
            ));
        }
        for column in &dataset.columns {
            let structural = column.label.is_some()
                || column.description.is_some()
                || column.unit.is_some()
                || column.status.is_some();
            if structural
                && !used
                    .columns
                    .contains(&(dataset.dataset.clone(), column.name.clone()))
            {
                queue.push(pending(
                    format!("column/{}/{}", dataset.dataset, column.name),
                    "curated_unbound",
                    fingerprint_of(
                        plans,
                        &format!("column/{}/{}", dataset.dataset, column.name),
                    ),
                    "curated_not_applied",
                ));
            }
        }
    }
    for relationship in &curated.relationships {
        if relationship.status.is_some() && !used.relationships.contains(&relationship.name) {
            let item = plans.iter().find_map(|plan| {
                plan.fks
                    .iter()
                    .find(|fk| fk.oos_name.as_deref() == Some(relationship.name.as_str()))
            });
            if let Some(fk) = item {
                queue.push(pending(
                    fk.item_id.clone(),
                    "curated_unbound",
                    Some(fk.fingerprint.clone()),
                    "curated_not_applied",
                ));
            }
        }
    }
    Ok(queue)
}

fn merge_layers(
    generated: &Value,
    curated: &Value,
    schema_hash: &str,
) -> Result<Value, DraftError> {
    let mut facts = Vec::new();
    if let Some(datasets) = curated.get("datasets").and_then(Value::as_array) {
        for dataset in datasets {
            if let Some(id) = dataset.get("dataset").and_then(Value::as_str) {
                let digest = HexDigest::new(schema_hash.to_string()).map_err(|_| {
                    DraftError::new(
                        "hash.schema",
                        "/catalog/schema_hash",
                        "schema_hash is not lowercase sha256",
                    )
                })?;
                facts.push(FactPin {
                    path: format!("/datasets/{id}"),
                    generated_fingerprint: digest.clone(),
                    recorded_fingerprint: digest,
                });
            }
        }
    }
    if let Some(relationships) = curated.get("relationships").and_then(Value::as_array) {
        for relationship in relationships {
            if let Some(name) = relationship.get("name").and_then(Value::as_str) {
                let digest = HexDigest::new(schema_hash.to_string()).map_err(|_| {
                    DraftError::new(
                        "hash.schema",
                        "/catalog/schema_hash",
                        "schema_hash is not lowercase sha256",
                    )
                })?;
                facts.push(FactPin {
                    path: format!("/relationships/{name}"),
                    generated_fingerprint: digest.clone(),
                    recorded_fingerprint: digest,
                });
            }
        }
    }
    let outcome = merge_effective(generated, curated, &facts).map_err(|_| {
        DraftError::new(
            "oos.invalid",
            "/curated",
            "curated overlay could not be merged",
        )
    })?;
    if !outcome.stale.is_empty() || !outcome.missing_pins.is_empty() {
        return Err(DraftError::new(
            "oos.invalid",
            "/curated",
            "curated overlay is not bound to fresh fact fingerprints",
        ));
    }
    Ok(outcome.effective)
}

fn strip_dangling_keys(effective: &mut Value, plans: &[TablePlan], queue: &mut Vec<QueueItem>) {
    let Some(datasets) = effective.get_mut("datasets").and_then(Value::as_array_mut) else {
        return;
    };
    for dataset in datasets {
        let Some(qualified) = dataset
            .get("dataset")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Some(columns) = dataset.get("columns").and_then(Value::as_array) else {
            continue;
        };
        let names: BTreeSet<String> = columns
            .iter()
            .filter_map(|column| {
                column
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        let Some(keys) = dataset.get_mut("primary_key").and_then(Value::as_array_mut) else {
            continue;
        };
        let before = keys.len();
        keys.retain(|key| key.as_str().is_some_and(|name| names.contains(name)));
        if keys.len() != before
            && let Some(plan) = plans
                .iter()
                .find(|plan| plan.qualified.as_deref() == Some(qualified.as_str()))
        {
            queue.push(pending(
                plan.pk_item.clone(),
                "primary_key_column_not_effective",
                Some(plan.pk_fingerprint.clone()),
                "primary_key_column_not_effective",
            ));
        }
    }
}

enum EndpointPresence {
    Present,
    MissingColumn,
    MissingDataset,
}

fn effective_column_index(effective: &Value) -> BTreeMap<String, BTreeSet<String>> {
    let mut index = BTreeMap::new();
    let Some(datasets) = effective.get("datasets").and_then(Value::as_array) else {
        return index;
    };
    for dataset in datasets {
        let Some(name) = dataset.get("dataset").and_then(Value::as_str) else {
            continue;
        };
        let mut columns = BTreeSet::new();
        if let Some(items) = dataset.get("columns").and_then(Value::as_array) {
            for column in items {
                if let Some(column_name) = column.get("name").and_then(Value::as_str) {
                    columns.insert(column_name.to_string());
                }
            }
        }
        index.insert(name.to_string(), columns);
    }
    index
}

fn endpoint_presence(
    relationship: &Value,
    side: &str,
    index: &BTreeMap<String, BTreeSet<String>>,
) -> EndpointPresence {
    let Some(end) = relationship.get(side) else {
        return EndpointPresence::MissingDataset;
    };
    let Some(dataset) = end.get("dataset").and_then(Value::as_str) else {
        return EndpointPresence::MissingDataset;
    };
    let Some(present) = index.get(dataset) else {
        return EndpointPresence::MissingDataset;
    };
    let Some(columns) = end.get("columns").and_then(Value::as_array) else {
        return EndpointPresence::MissingColumn;
    };
    if columns.iter().any(|column| {
        column
            .as_str()
            .is_none_or(|name| !present.contains(name))
    }) {
        EndpointPresence::MissingColumn
    } else {
        EndpointPresence::Present
    }
}

fn relationship_gap(
    relationship: &Value,
    index: &BTreeMap<String, BTreeSet<String>>,
) -> Option<&'static str> {
    let from = endpoint_presence(relationship, "from", index);
    let to = endpoint_presence(relationship, "to", index);
    let dataset_missing = matches!(
        (&from, &to),
        (EndpointPresence::MissingDataset, _) | (_, EndpointPresence::MissingDataset)
    );
    if dataset_missing {
        return Some("endpoint_dataset_not_effective");
    }
    match (from, to) {
        (EndpointPresence::MissingColumn, EndpointPresence::MissingColumn) => {
            Some("from_and_to_column_not_effective")
        }
        (EndpointPresence::MissingColumn, _) => Some("from_column_not_effective"),
        (_, EndpointPresence::MissingColumn) => Some("to_column_not_effective"),
        _ => None,
    }
}

fn strip_dangling_relationships(
    effective: &mut Value,
    plans: &[TablePlan],
    queue: &mut Vec<QueueItem>,
) {
    let index = effective_column_index(effective);
    let Some(relationships) = effective
        .get_mut("relationships")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    let mut closed = Vec::new();
    relationships.retain(|relationship| {
        match relationship_gap(relationship, &index) {
            None => true,
            Some(detail) => {
                if let Some(name) = relationship.get("name").and_then(Value::as_str) {
                    closed.push((name.to_string(), detail));
                }
                false
            }
        }
    });
    closed.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, detail) in closed {
        let Some(fk) = plans.iter().find_map(|plan| {
            plan.fks
                .iter()
                .find(|fk| fk.oos_name.as_deref() == Some(name.as_str()))
        }) else {
            continue;
        };
        queue.push(pending(
            fk.item_id.clone(),
            "relationship_endpoint_not_effective",
            Some(fk.fingerprint.clone()),
            detail,
        ));
    }
}

fn normalize_effective(effective: &mut Value) -> Result<(), DraftError> {
    let datasets = effective
        .get("datasets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut normalized_datasets = Vec::with_capacity(datasets.len());
    for dataset in datasets {
        let parsed: DatasetFile = serde_json::from_value(dataset).map_err(|_| {
            DraftError::new(
                "curated.invalid",
                "/curated",
                "curated overlay is not valid oos/v1",
            )
        })?;
        if !validate_dataset(&parsed).is_empty() {
            return Err(DraftError::new(
                "curated.invalid",
                "/curated",
                "curated overlay is not valid oos/v1",
            ));
        }
        normalized_datasets.push(serde_json::to_value(&parsed).map_err(|_| {
            DraftError::new(
                "oos.invalid",
                "/effective",
                "effective dataset is not oos/v1",
            )
        })?);
    }
    let relationships = effective
        .get("relationships")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut normalized_relationships = Vec::with_capacity(relationships.len());
    for relationship in relationships {
        let parsed: Relationship = serde_json::from_value(relationship).map_err(|_| {
            DraftError::new(
                "curated.invalid",
                "/curated",
                "curated overlay is not valid oos/v1",
            )
        })?;
        let file = RelationshipsFile {
            schema_version: OosVersion,
            relationships: vec![parsed.clone()],
        };
        if !validate_relationships(&file).is_empty() {
            return Err(DraftError::new(
                "curated.invalid",
                "/curated",
                "curated overlay is not valid oos/v1",
            ));
        }
        normalized_relationships.push(serde_json::to_value(&parsed).map_err(|_| {
            DraftError::new(
                "oos.invalid",
                "/effective",
                "effective relationship is not oos/v1",
            )
        })?);
    }
    normalized_datasets
        .sort_by(|left, right| field_str(left, "dataset").cmp(field_str(right, "dataset")));
    normalized_relationships
        .sort_by(|left, right| field_str(left, "name").cmp(field_str(right, "name")));
    effective["datasets"] = Value::Array(normalized_datasets);
    effective["relationships"] = Value::Array(normalized_relationships);
    Ok(())
}

fn document_value(
    catalog: &Catalog,
    generated: &Value,
    curated: &Value,
    effective: &Value,
    reviews: &[Review],
    queue: &[QueueItem],
    evidence_hash: &str,
) -> Value {
    let reviews: Vec<Value> = reviews
        .iter()
        .map(|review| {
            serde_json::json!({
                "action": review.action.as_str(),
                "fact_fingerprint": review.fact_fingerprint,
                "item_id": review.item_id,
                "schema_hash": review.schema_hash,
            })
        })
        .collect();
    let queue: Vec<Value> = queue
        .iter()
        .map(|item| {
            serde_json::json!({
                "detail": item.detail,
                "fact_fingerprint": item.fact_fingerprint,
                "item_id": item.item_id,
                "kind": item.kind,
                "publishable": item.publishable,
                "state": item.state,
            })
        })
        .collect();
    serde_json::json!({
        "compiler": COMPILER,
        "curated": curated,
        "effective": effective,
        "generated": generated,
        "physical": physical_value(catalog),
        "protocol": DRAFT_PROTOCOL,
        "provenance": provenance_value(catalog, evidence_hash),
        "review_queue": queue,
        "reviews": reviews,
    })
}

fn physical_value(catalog: &Catalog) -> Value {
    let tables: Vec<Value> = catalog.tables.iter().map(physical_table).collect();
    serde_json::json!({"tables": tables})
}

fn physical_table(table: &Table) -> Value {
    serde_json::json!({
        "columns": table.columns.iter().map(physical_column).collect::<Vec<_>>(),
        "comment": evidence_json(table.comment.as_ref()),
        "foreign_keys": table.foreign_keys.iter().map(physical_fk).collect::<Vec<_>>(),
        "name": table.name,
        "name_ident": table.name_ident,
        "policies": table.policies.iter().map(|policy| serde_json::json!({
            "check_expr": evidence_json(policy.check_expr.as_ref()),
            "command": policy.command,
            "name": policy.name,
            "permissive": policy.permissive,
            "using_expr": evidence_json(policy.using_expr.as_ref()),
        })).collect::<Vec<_>>(),
        "primary_key": table.primary_key.iter().map(|column| serde_json::json!({
            "attnum": column.attnum,
            "ident": column.ident,
            "name": column.name,
        })).collect::<Vec<_>>(),
        "primary_key_nulls_not_distinct": table.primary_key_nulls_not_distinct,
        "relkind": table.relkind,
        "replica_identity": table.replica_identity,
        "rls_enabled": table.rls_enabled,
        "rls_forced": table.rls_forced,
        "schema": table.schema,
        "schema_ident": table.schema_ident,
        "unique_keys": table.unique_keys.iter().map(|key| serde_json::json!({
            "columns": key.columns.iter().map(|column| serde_json::json!({
                "attnum": column.attnum,
                "ident": column.ident,
                "name": column.name,
            })).collect::<Vec<_>>(),
            "ident": key.ident,
            "name": key.name,
            "nulls_not_distinct": key.nulls_not_distinct,
        })).collect::<Vec<_>>(),
    })
}

fn physical_column(column: &Column) -> Value {
    serde_json::json!({
        "attnum": column.attnum,
        "base_type_oid": column.base_type_oid,
        "collation_name": column.collation_name,
        "collation_oid": column.collation_oid,
        "comment": evidence_json(column.comment.as_ref()),
        "default_expr": evidence_json(column.default_expr.as_ref()),
        "domain_checks": column.domain_checks.iter().map(|check| serde_json::json!({
            "byte_len": check.byte_len,
            "sha256": check.sha256,
        })).collect::<Vec<_>>(),
        "enum_label_count": column.enum_labels.len(),
        "enum_labels_sha256": canonical_sha256(&Value::Array(column.enum_labels.iter().cloned().map(Value::String).collect())),
        "generated": column.generated,
        "ident": column.ident,
        "identity": column.identity,
        "name": column.name,
        "not_null": column.not_null,
        "numeric_precision": column.numeric_precision,
        "numeric_scale": column.numeric_scale,
        "type_name": column.type_name,
        "type_oid": column.type_oid,
        "type_schema": column.type_schema,
        "type_support": {"converted": false, "reason": column.reason.as_str()},
        "typmod": column.typmod,
        "typtype": column.typtype,
    })
}

fn physical_fk(key: &ForeignKey) -> Value {
    serde_json::json!({
        "columns": key.columns.iter().map(|column| serde_json::json!({
            "attnum": column.attnum,
            "ident": column.ident,
            "name": column.name,
            "ref_attnum": column.ref_attnum,
            "ref_ident": column.ref_ident,
            "ref_name": column.ref_name,
            "ref_schema": column.ref_schema,
            "ref_schema_ident": column.ref_schema_ident,
            "ref_table": column.ref_table,
            "ref_table_ident": column.ref_table_ident,
        })).collect::<Vec<_>>(),
        "ident": key.ident,
        "name": key.name,
    })
}

fn evidence_json(value: Option<&pgsql::EvidenceHash>) -> Value {
    match value {
        None => Value::Null,
        Some(evidence) => serde_json::json!({
            "byte_len": evidence.byte_len,
            "sha256": evidence.sha256,
        }),
    }
}

fn provenance_value(catalog: &Catalog, evidence_hash: &str) -> Value {
    serde_json::json!({
        "adapter": "pgsql",
        "captured_at": catalog.captured_at,
        "certification": "none",
        "config_hash": config_hash(catalog),
        "config_version": pgsql::CORE_CONTRACT,
        "database": catalog.database,
        "evidence_hash": evidence_hash,
        "evidence_version": EVIDENCE_PROTOCOL,
        "ident_algorithm": pgsql::IDENT_ALGORITHM,
        "llm": "not_used",
        "oos_version": OosVersion::VALUE,
        "pg_catalog_contract": pgsql::PROTOCOL,
        "pg_catalog_contract_commit": PG_CONTRACT_COMMIT,
        "pg_catalog_integration": "not_accepted",
        "publication": "unavailable",
        "schema_hash": catalog.schema_hash,
        "schema_version": pgsql::HASH_ALGORITHM,
        "source_connection": "not_used",
        "source_id": catalog.source_id,
        "type_hash": type_hash(catalog),
        "type_version": pgsql::PROTOCOL,
    })
}

fn seal(mut document: Value) -> String {
    let manifest = canonical_sha256(&document);
    if let Some(object) = document
        .get_mut("provenance")
        .and_then(Value::as_object_mut)
    {
        object.insert("manifest_hash".to_string(), Value::String(manifest));
    }
    canonical_json(&document)
}

#[cfg(test)]
#[path = "draft_tests.rs"]
mod tests;
