//! Strict OOS overlay documents.
//!
//! Builder output, human curated text, and review records stay separate.
//! `effective` is a field merge: curated wins, lists join by name, a rejected
//! name stays out, and a curated field bound to an older fact becomes stale
//! instead of being reused. This module does not publish and does not sign
//! for a business owner.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::canonical::{canonical_sha256, sha256_hex};
use crate::error::ApiError;

pub const COMPILER: &str = "oo.selfbuild-overlay/1";
const SAMPLE_MARK: &str = "sample";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Y {
    Scalar(String),
    Map(BTreeMap<String, Y>),
    Seq(Vec<Y>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub sealed_data_type: String,
    pub semantic_type: String,
    pub pii: String,
    pub unit: String,
    pub confidence: String,
    pub provenance: String,
    pub classification_state: String,
    pub fact_sha256: String,
    pub label: String,
    pub label_from: String,
    pub binds_fact: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dataset {
    pub name: String,
    pub columns: BTreeMap<String, Column>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    pub name: String,
    pub from_column: String,
    pub to_column: String,
    pub cardinality: String,
    pub provenance: String,
    pub confidence: String,
    pub status: String,
    pub fact_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub datasets: BTreeMap<String, Dataset>,
    pub relationships: BTreeMap<String, Relationship>,
    pub rejections: BTreeMap<String, String>,
    pub stale: Vec<Stale>,
    pub candidates: Vec<Candidate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stale {
    pub name: String,
    pub kind: String,
    pub bound_fact: String,
    pub current_fact: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub kind: String,
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct PackFile {
    pub path: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct RolePack {
    pub files: Vec<PackFile>,
    pub pack_sha256: String,
    pub manifest_sha256: String,
    pub manifest: String,
}

pub fn column_fact(dataset: &str, column: &str, data_type: &str) -> String {
    canonical_sha256(&json!({
        "column": column,
        "data_type": data_type,
        "dataset": dataset
    }))
}

pub fn relationship_fact(name: &str, from_column: &str, to_column: &str, cardinality: &str) -> String {
    canonical_sha256(&json!({
        "cardinality": cardinality,
        "from_column": from_column,
        "name": name,
        "to_column": to_column
    }))
}

pub fn parse_yaml(text: &str) -> Result<Y, ApiError> {
    if text.is_empty() || text.len() > 1_000_000 || text.contains('\t') || text.contains('\r') || text.contains('#') {
        return Err(ApiError::malformed());
    }
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.is_empty() {
        return Err(ApiError::malformed());
    }
    let mut index = 0;
    let value = parse_map(&lines, &mut index, 0)?;
    if index != lines.len() {
        return Err(ApiError::malformed());
    }
    Ok(value)
}

pub fn emit_yaml(value: &Y) -> String {
    let mut out = String::new();
    emit_map(&mut out, value, 0);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn canonical_doc(value: &Y) -> Result<String, ApiError> {
    let sorted = sort_doc(value)?;
    Ok(emit_yaml(&sorted))
}

pub fn doc_sha256(value: &Y) -> Result<String, ApiError> {
    Ok(sha256_hex(canonical_doc(value)?.as_bytes()))
}

pub fn generated_from_inputs(catalog: &Value, rules: &Value, auto_accept: &str) -> Result<(Model, String), ApiError> {
    let model = model_from_inputs(catalog, rules, auto_accept)?;
    let yaml = canonical_doc(&model_to_y(&model, "generated"))?;
    Ok((model, yaml))
}

pub fn parse_model(text: &str, kind: &str) -> Result<Model, ApiError> {
    let value = parse_yaml(text)?;
    model_from_y(&value, kind)
}

pub fn merge(generated: &Model, curated: &Model, reviews: &[Review]) -> Model {
    let mut out = generated.clone();
    out.rejections.clear();
    out.stale.clear();
    out.candidates = generated.candidates.clone();
    for (name, relation) in &curated.relationships {
        if relation.status == "rejected" {
            out.relationships.remove(name);
            out.rejections.insert(name.clone(), "rejected".to_owned());
            if let Some(current) = generated.relationships.get(name)
                && current.fact_sha256 != relation.fact_sha256
            {
                out.stale.push(Stale {
                    name: name.clone(),
                    kind: "relationship".to_owned(),
                    bound_fact: relation.fact_sha256.clone(),
                    current_fact: current.fact_sha256.clone(),
                });
            }
        }
    }
    for (dataset_name, curated_dataset) in &curated.datasets {
        let Some(dataset) = out.datasets.get_mut(dataset_name) else {
            continue;
        };
        for (column_name, curated_column) in &curated_dataset.columns {
            if curated_column.classification_state == "rejected" {
                dataset.columns.remove(column_name);
                out.rejections.insert(format!("{dataset_name}.{column_name}"), "rejected".to_owned());
                continue;
            }
            let Some(column) = dataset.columns.get_mut(column_name) else {
                continue;
            };
            if !curated_column.label.is_empty() {
                if curated_column.binds_fact == column.fact_sha256 {
                    column.label = curated_column.label.clone();
                    column.label_from = "curated".to_owned();
                    column.binds_fact = curated_column.binds_fact.clone();
                } else {
                    column.label.clear();
                    column.label_from.clear();
                    out.stale.push(Stale {
                        name: format!("{dataset_name}.{column_name}"),
                        kind: "column".to_owned(),
                        bound_fact: curated_column.binds_fact.clone(),
                        current_fact: column.fact_sha256.clone(),
                    });
                }
            }
        }
    }
    for review in reviews {
        if review.decision != "confirmed" {
            continue;
        }
        let Some((dataset_name, column_name)) = review.name.rsplit_once('.') else {
            continue;
        };
        let Some(column) = out.datasets.get_mut(dataset_name).and_then(|dataset| dataset.columns.get_mut(column_name)) else {
            continue;
        };
        if review.binds_fact == column.fact_sha256 && column.pii == "none" && column.classification_state == "pending" {
            column.classification_state = "confirmed".to_owned();
        }
    }
    out.stale.sort_by(|left, right| left.name.cmp(&right.name));
    out
}

#[derive(Clone, Debug)]
pub struct Review {
    pub name: String,
    pub decision: String,
    pub binds_fact: String,
}

pub fn apply_fact_edit(model: &mut Model, dataset: &str, column: &str, data_type: &str) -> Result<(), ApiError> {
    if !data_type_ok(data_type) {
        return Err(ApiError::malformed());
    }
    let Some(item) = model.datasets.get_mut(dataset).and_then(|item| item.columns.get_mut(column)) else {
        return Err(ApiError::membership());
    };
    if item.data_type == data_type {
        return Err(ApiError::malformed());
    }
    item.data_type = data_type.to_owned();
    item.fact_sha256 = column_fact(dataset, column, data_type);
    if item.semantic_type == "money" && !matches!(data_type, "numeric" | "bigint" | "integer") {
        item.confidence = "low".to_owned();
        item.classification_state = "pending".to_owned();
    }
    Ok(())
}

pub fn render_pack(model: &Model, role: &str, columns: &[String], policy_version: &str) -> Result<RolePack, ApiError> {
    if model.stale.iter().any(|item| columns.iter().any(|column| column == &item.name)) {
        return Err(ApiError::pack_refused());
    }
    let mut tables: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for column_id in columns {
        let Some((dataset, column_name)) = split_column(column_id) else {
            return Err(ApiError::malformed());
        };
        let Some(column) = model.datasets.get(dataset).and_then(|item| item.columns.get(column_name)) else {
            return Err(ApiError::membership());
        };
        if column.pii == "unknown" {
            return Err(ApiError::pii_unknown());
        }
        if column.pii != "none" {
            return Err(ApiError::pii_withheld());
        }
        if column.classification_state != "auto_accepted" && column.classification_state != "confirmed" {
            return Err(ApiError::classification_incomplete());
        }
        let line = format!("| {column_name} | {} | {} |", column.data_type, column.semantic_type);
        tables.entry(dataset.to_owned()).or_default().push(line);
    }
    let mut files = Vec::new();
    let readme = format!(
        "# {role}\n\npublished: false\ncompiler: {COMPILER}\npolicy: {policy_version}\nLabels that lack a business-owner signature are withheld.\n"
    );
    files.push(PackFile { path: "README.md".to_owned(), body: readme });
    for (dataset, lines) in &tables {
        let mut body = format!("# {dataset}\n\n| column | data_type | semantic_type |\n|---|---|---|\n");
        for line in lines {
            body.push_str(line);
            body.push('\n');
        }
        files.push(PackFile {
            path: format!("tables/{dataset}.md"),
            body,
        });
    }
    let mut relationships = String::from("# relationships\n\n");
    for relation in model.relationships.values() {
        if !columns.contains(&relation.from_column) || !columns.contains(&relation.to_column) { continue; }
        if relation.status != "declared" && relation.confidence != "high" {
            continue;
        }
        relationships.push_str(&format!("- {} {} {}\n", relation.name, relation.cardinality, relation.from_column));
    }
    files.push(PackFile { path: "relationships.md".to_owned(), body: relationships });
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut listed = Vec::new();
    for file in &files {
        if file.body.contains(SAMPLE_MARK) {
            return Err(ApiError::pii_withheld());
        }
        listed.push(json!({
            "bytes": file.body.len(),
            "path": file.path,
            "sha256": sha256_hex(file.body.as_bytes())
        }));
    }
    let manifest_value = json!({
        "active": false,
        "compiled_pack": "not_published",
        "compiler": COMPILER,
        "files": listed,
        "policy_version": policy_version,
        "published": false,
        "role": role
    });
    let manifest = canonical_json_pretty(&manifest_value);
    let manifest_sha256 = sha256_hex(manifest.as_bytes());
    let mut packed = manifest_sha256.clone();
    for file in &files {
        packed.push('\n');
        packed.push_str(&file.path);
        packed.push('\n');
        packed.push_str(&sha256_hex(file.body.as_bytes()));
    }
    let pack_sha256 = sha256_hex(packed.as_bytes());
    Ok(RolePack { files, pack_sha256, manifest_sha256, manifest })
}

pub fn gate_report(model: &Model, sealed_types: &BTreeMap<String, String>) -> Value {
    let mut current_match = true;
    let mut sealed_match = true;
    for (dataset_name, dataset) in &model.datasets {
        for (column_name, column) in &dataset.columns {
            let id = format!("{dataset_name}.{column_name}");
            if let Some(sealed) = sealed_types.get(&id)
                && sealed != &column.data_type
            {
                sealed_match = false;
            }
            if column.semantic_type == "money" && column.unit.is_empty() {
                current_match = false;
            }
        }
    }
    let g2 = if current_match && sealed_match { "pass" } else { "block" };
    json!({
        "contract": "oo.selfbuild-control-api/gate-report/v1",
        "published": false,
        "publication": "refused",
        "active": false,
        "compiled_pack": "not_published",
        "business_owner_signed": false,
        "owner_certification": "not_issued",
        "cas": "not_supported",
        "fence": "not_supported",
        "replica_barrier": "not_supported",
        "gates": [
            gate("G1", if current_match { "pass" } else { "block" }, true, "schema and money unit"),
            gate("G2", g2, true, if sealed_match { "effective types match the sealed catalog" } else { "effective types diverged from the sealed catalog" }),
            gate("G3", "warning", false, "inferred relationships stay unapproved"),
            gate("G4", "not_run", true, "enum coverage is an external sweeper"),
            gate("G5a", "not_run", true, "metric parser and EXPLAIN are not in this control api"),
            gate("G5b", "not_run", true, "independent control_total is not in this control api"),
            gate("G6", "not_run", true, "verified examples need the gateway"),
            gate("G7", "not_run", true, "role fence, ACL, and replica barrier are not in this control api"),
            gate("G8", "pass", true, "role pack files have no sample field"),
            gate("G9", "not_run", true, "tokenizer is not pinned; byte length is not a token count"),
            gate("G10", "not_run", true, "customer eval is not claimed"),
            gate("G11", "not_run", true, "metric semantic diff is not in this control api"),
            gate("G12", "not_run", true, "name lifecycle references are not in this control api"),
            gate("G13", if model.datasets.values().all(|dataset| dataset.columns.values().all(classified_or_withheld)) { "pass" } else { "block" }, true, "role columns are classified or withheld")
        ]
    })
}

fn classified_or_withheld(column: &Column) -> bool {
    if column.pii == "none" {
        matches!(column.classification_state.as_str(), "auto_accepted" | "confirmed")
    } else {
        !matches!(column.classification_state.as_str(), "auto_accepted" | "confirmed")
    }
}

fn gate(id: &str, status: &str, blocking: bool, evidence: &str) -> Value {
    json!({
        "gate_id": id,
        "status": status,
        "blocking": blocking,
        "evidence": evidence,
        "reviewer": "not_a_business_owner_signature",
        "force": "refused"
    })
}

pub fn model_to_y(model: &Model, kind: &str) -> Y {
    let mut datasets = Vec::new();
    for dataset in model.datasets.values() {
        let mut columns = Vec::new();
        for column in dataset.columns.values() {
            let mut map = BTreeMap::new();
            map.insert("name".to_owned(), Y::Scalar(column.name.clone()));
            map.insert("data_type".to_owned(), Y::Scalar(column.data_type.clone()));
            map.insert("sealed_data_type".to_owned(), Y::Scalar(column.sealed_data_type.clone()));
            map.insert("semantic_type".to_owned(), Y::Scalar(column.semantic_type.clone()));
            map.insert("pii".to_owned(), Y::Scalar(column.pii.clone()));
            if !column.unit.is_empty() {
                map.insert("unit".to_owned(), Y::Scalar(column.unit.clone()));
            }
            map.insert("confidence".to_owned(), Y::Scalar(column.confidence.clone()));
            map.insert("provenance".to_owned(), Y::Scalar(column.provenance.clone()));
            map.insert("classification_state".to_owned(), Y::Scalar(column.classification_state.clone()));
            map.insert("fact_sha256".to_owned(), Y::Scalar(column.fact_sha256.clone()));
            if !column.label.is_empty() {
                map.insert("label".to_owned(), Y::Scalar(column.label.clone()));
                map.insert("label_from".to_owned(), Y::Scalar(column.label_from.clone()));
                map.insert("binds_fact".to_owned(), Y::Scalar(column.binds_fact.clone()));
            }
            columns.push(Y::Map(map));
        }
        let mut dataset_map = BTreeMap::new();
        dataset_map.insert("name".to_owned(), Y::Scalar(dataset.name.clone()));
        dataset_map.insert("columns".to_owned(), Y::Seq(columns));
        datasets.push(Y::Map(dataset_map));
    }
    let mut relationships = Vec::new();
    for relation in model.relationships.values() {
        let mut map = BTreeMap::new();
        map.insert("name".to_owned(), Y::Scalar(relation.name.clone()));
        map.insert("from_column".to_owned(), Y::Scalar(relation.from_column.clone()));
        map.insert("to_column".to_owned(), Y::Scalar(relation.to_column.clone()));
        map.insert("cardinality".to_owned(), Y::Scalar(relation.cardinality.clone()));
        map.insert("provenance".to_owned(), Y::Scalar(relation.provenance.clone()));
        map.insert("confidence".to_owned(), Y::Scalar(relation.confidence.clone()));
        map.insert("status".to_owned(), Y::Scalar(relation.status.clone()));
        map.insert("fact_sha256".to_owned(), Y::Scalar(relation.fact_sha256.clone()));
        relationships.push(Y::Map(map));
    }
    let mut root = BTreeMap::new();
    root.insert("spec_version".to_owned(), Y::Scalar("oos/v1".to_owned()));
    root.insert("kind".to_owned(), Y::Scalar(kind.to_owned()));
    root.insert("datasets".to_owned(), Y::Seq(datasets));
    root.insert("relationships".to_owned(), Y::Seq(relationships));
    Y::Map(root)
}

fn model_from_inputs(catalog: &Value, rules: &Value, auto_accept: &str) -> Result<Model, ApiError> {
    let tables = catalog.get("tables").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    let mut datasets = BTreeMap::new();
    for table in tables {
        let schema = text(table, "schema")?;
        let name = text(table, "table")?;
        if !ident(schema) || !ident(name) {
            return Err(ApiError::malformed());
        }
        let dataset_name = format!("{schema}.{name}");
        let mut columns = BTreeMap::new();
        let Some(listed) = table.get("columns").and_then(Value::as_array) else {
            return Err(ApiError::malformed());
        };
        for column in listed {
            let column_name = text(column, "name")?;
            let data_type = text(column, "data_type")?;
            if !ident(column_name) || !data_type_ok(data_type) {
                return Err(ApiError::malformed());
            }
            let id = format!("{dataset_name}.{column_name}");
            let rule = find_rule(rules, &id);
            let semantic = rule.and_then(|item| item.get("semantic_type")).and_then(Value::as_str).unwrap_or("free_text");
            let pii = rule.and_then(|item| item.get("pii")).and_then(Value::as_str).unwrap_or("unknown");
            let confidence = rule.and_then(|item| item.get("confidence")).and_then(Value::as_str).unwrap_or("unknown");
            let unit = rule.and_then(|item| item.get("unit")).and_then(Value::as_str).unwrap_or("").to_owned();
            if !semantic_ok(semantic) || !pii_ok(pii) || !confidence_ok(confidence) {
                return Err(ApiError::malformed());
            }
            if semantic == "money" && unit.is_empty() {
                return Err(ApiError::malformed());
            }
            if rule.and_then(|item| item.get(SAMPLE_MARK)).is_some() {
                return Err(ApiError::malformed());
            }
            let classification = classify(pii, semantic, confidence, auto_accept, &unit);
            columns.insert(
                column_name.to_owned(),
                Column {
                    name: column_name.to_owned(),
                    data_type: data_type.to_owned(),
                    sealed_data_type: data_type.to_owned(),
                    semantic_type: semantic.to_owned(),
                    pii: pii.to_owned(),
                    unit,
                    confidence: confidence.to_owned(),
                    provenance: "declared".to_owned(),
                    classification_state: classification,
                    fact_sha256: column_fact(&dataset_name, column_name, data_type),
                    label: String::new(),
                    label_from: String::new(),
                    binds_fact: String::new(),
                },
            );
        }
        datasets.insert(dataset_name.clone(), Dataset { name: dataset_name, columns });
    }
    let mut relationships = BTreeMap::new();
    if let Some(items) = rules.get("relationships").and_then(Value::as_array) {
        for item in items {
            let name = text(item, "name")?;
            let from_column = text(item, "from_column")?;
            let to_column = text(item, "to_column")?;
            let cardinality = text(item, "cardinality")?;
            let confidence = text(item, "confidence")?;
            if !ident(name) || !column_id_ok(from_column) || !column_id_ok(to_column) || !cardinality_ok(cardinality) || !confidence_ok(confidence) {
                return Err(ApiError::malformed());
            }
            let fact = relationship_fact(name, from_column, to_column, cardinality);
            relationships.insert(
                name.to_owned(),
                Relationship {
                    name: name.to_owned(),
                    from_column: from_column.to_owned(),
                    to_column: to_column.to_owned(),
                    cardinality: cardinality.to_owned(),
                    provenance: "inferred".to_owned(),
                    confidence: confidence.to_owned(),
                    status: "pending".to_owned(),
                    fact_sha256: fact,
                },
            );
        }
    }
    let mut candidates = Vec::new();
    for dataset in datasets.values() {
        for column in dataset.columns.values() {
            if column.classification_state == "pending" {
                candidates.push(Candidate {
                    name: format!("{}.{}", dataset.name, column.name),
                    kind: "classification".to_owned(),
                    note: "pending review".to_owned(),
                });
            }
        }
    }
    Ok(Model {
        datasets,
        relationships,
        rejections: BTreeMap::new(),
        stale: Vec::new(),
        candidates,
    })
}

fn model_from_y(value: &Y, expected_kind: &str) -> Result<Model, ApiError> {
    let root = value.as_map().ok_or_else(ApiError::malformed)?;
    if scalar(root, "spec_version")? != "oos/v1" || scalar(root, "kind")? != expected_kind {
        return Err(ApiError::malformed());
    }
    let mut datasets = BTreeMap::new();
    for item in seq(root, "datasets")? {
        let map = item.as_map().ok_or_else(ApiError::malformed)?;
        let name = scalar(map, "name")?;
        if !dataset_ok(&name) {
            return Err(ApiError::malformed());
        }
        let mut columns = BTreeMap::new();
        for column in seq(map, "columns")? {
            let fields = column.as_map().ok_or_else(ApiError::malformed)?;
            reject_sample(fields)?;
            let column_name = scalar(fields, "name")?;
            if expected_kind == "curated" {
                let status = optional(fields, "status");
                if status == "rejected" {
                    columns.insert(
                        column_name.clone(),
                        blank_column(&column_name, "rejected"),
                    );
                    continue;
                }
                let label = optional(fields, "label");
                let binds = optional(fields, "binds_fact");
                if !label.is_empty() && !hex64(&binds) {
                    return Err(ApiError::malformed());
                }
                if fields.contains_key("data_type")
                    || fields.contains_key("pii")
                    || fields.contains_key("classification_state")
                    || fields.contains_key("review_state")
                    || fields.contains_key("published")
                    || fields.contains_key("active")
                    || fields.contains_key("business_owner_signed")
                {
                    return Err(ApiError::malformed());
                }
                let mut column = blank_column(&column_name, "active");
                column.label = label;
                column.label_from = "curated".to_owned();
                column.binds_fact = binds;
                columns.insert(column_name, column);
                continue;
            }
            let data_type = scalar(fields, "data_type")?;
            let semantic = scalar(fields, "semantic_type")?;
            let pii = scalar(fields, "pii")?;
            let fact = scalar(fields, "fact_sha256")?;
            if !data_type_ok(&data_type) || !semantic_ok(&semantic) || !pii_ok(&pii) || !hex64(&fact) {
                return Err(ApiError::malformed());
            }
            let expected = column_fact(&name, &column_name, &data_type);
            if fact != expected {
                return Err(ApiError::malformed());
            }
            if semantic == "money" && optional(fields, "unit").is_empty() {
                return Err(ApiError::malformed());
            }
            columns.insert(
                column_name.clone(),
                Column {
                    name: column_name,
                    data_type: data_type.clone(),
                    sealed_data_type: optional(fields, "sealed_data_type"),
                    semantic_type: semantic,
                    pii,
                    unit: optional(fields, "unit"),
                    confidence: scalar(fields, "confidence")?,
                    provenance: scalar(fields, "provenance")?,
                    classification_state: scalar(fields, "classification_state")?,
                    fact_sha256: fact,
                    label: optional(fields, "label"),
                    label_from: optional(fields, "label_from"),
                    binds_fact: optional(fields, "binds_fact"),
                },
            );
        }
        datasets.insert(name.clone(), Dataset { name, columns });
    }
    let mut relationships = BTreeMap::new();
    for item in seq(root, "relationships")? {
        let map = item.as_map().ok_or_else(ApiError::malformed)?;
        let name = scalar(map, "name")?;
        if expected_kind == "curated" {
            if scalar(map, "status")? != "rejected" || !hex64(&scalar(map, "fact_sha256")?) {
                return Err(ApiError::malformed());
            }
            relationships.insert(
                name.clone(),
                Relationship {
                    name,
                    from_column: String::new(),
                    to_column: String::new(),
                    cardinality: String::new(),
                    provenance: "human".to_owned(),
                    confidence: "high".to_owned(),
                    status: "rejected".to_owned(),
                    fact_sha256: scalar(map, "fact_sha256")?,
                },
            );
            continue;
        }
        let from_column = scalar(map, "from_column")?;
        let to_column = scalar(map, "to_column")?;
        let cardinality = scalar(map, "cardinality")?;
        let fact = scalar(map, "fact_sha256")?;
        if fact != relationship_fact(&name, &from_column, &to_column, &cardinality) {
            return Err(ApiError::malformed());
        }
        relationships.insert(
            name.clone(),
            Relationship {
                name,
                from_column,
                to_column,
                cardinality,
                provenance: scalar(map, "provenance")?,
                confidence: scalar(map, "confidence")?,
                status: scalar(map, "status")?,
                fact_sha256: fact,
            },
        );
    }
    Ok(Model {
        datasets,
        relationships,
        rejections: BTreeMap::new(),
        stale: Vec::new(),
        candidates: Vec::new(),
    })
}

fn blank_column(name: &str, state: &str) -> Column {
    Column {
        name: name.to_owned(),
        data_type: String::new(),
        sealed_data_type: String::new(),
        semantic_type: String::new(),
        pii: String::new(),
        unit: String::new(),
        confidence: String::new(),
        provenance: "human".to_owned(),
        classification_state: state.to_owned(),
        fact_sha256: String::new(),
        label: String::new(),
        label_from: String::new(),
        binds_fact: String::new(),
    }
}

fn classify(pii: &str, semantic: &str, confidence: &str, auto_accept: &str, unit: &str) -> String {
    let safe = matches!(
        semantic,
        "identifier" | "foreign_key" | "money" | "quantity" | "ratio" | "event_time" | "date" | "audit_time" | "code" | "status_code" | "boolean_flag" | "system"
    );
    if auto_accept == "rule_high_confidence" && pii == "none" && confidence == "high" && safe && (semantic != "money" || !unit.is_empty()) {
        "auto_accepted".to_owned()
    } else {
        "pending".to_owned()
    }
}

fn find_rule<'a>(rules: &'a Value, id: &str) -> Option<&'a Value> {
    rules.get("columns")?.as_array()?.iter().find(|item| item.get("column").and_then(Value::as_str) == Some(id))
}

fn split_column(id: &str) -> Option<(&str, &str)> {
    let (schema, rest) = id.split_once('.')?;
    let (table, column) = rest.split_once('.')?;
    if ident(schema) && ident(table) && ident(column) {
        Some((&id[..schema.len() + 1 + table.len()], column))
    } else {
        None
    }
}

fn parse_map(lines: &[&str], index: &mut usize, indent: usize) -> Result<Y, ApiError> {
    let mut map = BTreeMap::new();
    while *index < lines.len() {
        let line = lines[*index];
        let current = indent_of(line)?;
        if current < indent {
            break;
        }
        if current != indent || line.trim_start().starts_with("- ") {
            return Err(ApiError::malformed());
        }
        let (key, rest) = split_key(line.trim_start())?;
        if map.contains_key(&key) {
            return Err(ApiError::malformed());
        }
        *index += 1;
        let value = if rest.is_empty() {
            if *index >= lines.len() {
                return Err(ApiError::malformed());
            }
            let next = lines[*index];
            let next_indent = indent_of(next)?;
            if next_indent == indent + 2 && next.trim_start().starts_with("- ") {
                parse_seq(lines, index, indent + 2)?
            } else if next_indent == indent + 2 {
                parse_map(lines, index, indent + 2)?
            } else {
                return Err(ApiError::malformed());
            }
        } else if rest == "[]" {
            Y::Seq(Vec::new())
        } else {
            Y::Scalar(parse_scalar(rest)?)
        };
        map.insert(key, value);
    }
    if map.is_empty() {
        return Err(ApiError::malformed());
    }
    Ok(Y::Map(map))
}

fn parse_seq(lines: &[&str], index: &mut usize, indent: usize) -> Result<Y, ApiError> {
    let mut items = Vec::new();
    while *index < lines.len() {
        let line = lines[*index];
        let current = indent_of(line)?;
        if current < indent {
            break;
        }
        if current != indent {
            return Err(ApiError::malformed());
        }
        let trimmed = line.trim_start();
        let Some(body) = trimmed.strip_prefix("- ") else {
            return Err(ApiError::malformed());
        };
        if body.is_empty() {
            return Err(ApiError::malformed());
        }
        *index += 1;
        if body.contains(':') {
            let (key, rest) = split_key(body)?;
            let mut map = BTreeMap::new();
            map.insert(key, read_value(lines, index, indent + 4, rest)?);
            while *index < lines.len() {
                let nested = lines[*index];
                let nested_indent = indent_of(nested)?;
                if nested_indent < indent + 2 {
                    break;
                }
                if nested_indent != indent + 2 || nested.trim_start().starts_with("- ") {
                    return Err(ApiError::malformed());
                }
                let (child, child_rest) = split_key(nested.trim_start())?;
                if map.contains_key(&child) {
                    return Err(ApiError::malformed());
                }
                *index += 1;
                map.insert(child, read_value(lines, index, nested_indent + 2, child_rest)?);
            }
            items.push(Y::Map(map));
        } else {
            items.push(Y::Scalar(parse_scalar(body)?));
        }
    }
    Ok(Y::Seq(items))
}

fn emit_map(out: &mut String, value: &Y, indent: usize) {
    let Y::Map(map) = value else {
        return;
    };
    for (key, child) in map {
        pad(out, indent);
        out.push_str(key);
        out.push(':');
        match child {
            Y::Scalar(text) => {
                out.push(' ');
                out.push_str(&quote_scalar(text));
                out.push('\n');
            }
            Y::Seq(items) if items.is_empty() => out.push_str(" []\n"),
            Y::Seq(items) => {
                out.push('\n');
                for item in items {
                    pad(out, indent + 2);
                    out.push_str("- ");
                    match item {
                        Y::Scalar(text) => {
                            out.push_str(&quote_scalar(text));
                            out.push('\n');
                        }
                        Y::Map(fields) => {
                            let mut first = true;
                            for (field, field_value) in fields {
                                if first {
                                    first = false;
                                } else {
                                    pad(out, indent + 4);
                                }
                                out.push_str(field);
                                out.push(':');
                                match field_value {
                                    Y::Scalar(text) => {
                                        out.push(' ');
                                        out.push_str(&quote_scalar(text));
                                        out.push('\n');
                                    }
                                    Y::Seq(nested) if nested.is_empty() => out.push_str(" []\n"),
                                    other => {
                                        out.push('\n');
                                        emit_nested(out, other, indent + 6);
                                    }
                                }
                            }
                        }
                        Y::Seq(_) => out.push_str("[]\n"),
                    }
                }
            }
            other => {
                out.push('\n');
                emit_map(out, other, indent + 2);
            }
        }
    }
}

fn emit_nested(out: &mut String, value: &Y, indent: usize) {
    match value {
        Y::Map(_) => emit_map(out, value, indent),
        Y::Seq(items) => {
            for item in items {
                pad(out, indent);
                out.push_str("- ");
                if let Y::Map(fields) = item {
                    let mut first = true;
                    for (field, field_value) in fields {
                        if !first {
                            pad(out, indent + 2);
                        }
                        first = false;
                        out.push_str(field);
                        out.push_str(": ");
                        if let Y::Scalar(text) = field_value {
                            out.push_str(&quote_scalar(text));
                            out.push('\n');
                        }
                    }
                }
            }
        }
        Y::Scalar(text) => {
            pad(out, indent);
            out.push_str(&quote_scalar(text));
            out.push('\n');
        }
    }
}

fn sort_doc(value: &Y) -> Result<Y, ApiError> {
    match value {
        Y::Scalar(text) => Ok(Y::Scalar(text.clone())),
        Y::Map(map) => {
            let mut out = BTreeMap::new();
            for (key, child) in map {
                out.insert(key.clone(), sort_doc(child)?);
            }
            Ok(Y::Map(out))
        }
        Y::Seq(items) => {
            let mut out = Vec::new();
            for item in items {
                out.push(sort_doc(item)?);
            }
            out.sort_by_key(seq_key);
            Ok(Y::Seq(out))
        }
    }
}

fn seq_key(value: &Y) -> String {
    value
        .as_map()
        .and_then(|map| map.get("name"))
        .and_then(Y::as_scalar)
        .unwrap_or("")
        .to_owned()
}

fn pad(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push(' ');
    }
}

fn quote_scalar(text: &str) -> String {
    if text.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'/' | b'-' | b':')) && !text.is_empty() {
        text.to_owned()
    } else {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn read_value(lines: &[&str], index: &mut usize, child_indent: usize, rest: &str) -> Result<Y, ApiError> {
    if rest.is_empty() {
        if *index >= lines.len() {
            return Err(ApiError::malformed());
        }
        let next = lines[*index];
        let next_indent = indent_of(next)?;
        if next_indent == child_indent && next.trim_start().starts_with("- ") {
            parse_seq(lines, index, child_indent)
        } else if next_indent == child_indent {
            parse_map(lines, index, child_indent)
        } else {
            Err(ApiError::malformed())
        }
    } else if rest == "[]" {
        Ok(Y::Seq(Vec::new()))
    } else {
        Ok(Y::Scalar(parse_scalar(rest)?))
    }
}

fn parse_scalar(text: &str) -> Result<String, ApiError> {
    if let Some(inner) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return Ok(inner.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    if text.is_empty() || text.bytes().any(|byte| byte.is_ascii_whitespace() || byte == b'"' || byte == b'#') {
        return Err(ApiError::malformed());
    }
    Ok(text.to_owned())
}

fn split_key(text: &str) -> Result<(String, &str), ApiError> {
    let Some((key, rest)) = text.split_once(':') else {
        return Err(ApiError::malformed());
    };
    if !key_ok(key) {
        return Err(ApiError::malformed());
    }
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    if rest.starts_with(' ') {
        return Err(ApiError::malformed());
    }
    Ok((key.to_owned(), rest))
}

fn indent_of(line: &str) -> Result<usize, ApiError> {
    let count = line.bytes().take_while(|byte| *byte == b' ').count();
    if count % 2 != 0 || line[count..].is_empty() {
        return Err(ApiError::malformed());
    }
    Ok(count)
}

fn key_ok(key: &str) -> bool {
    let bytes = key.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn ident(value: &str) -> bool {
    crate::ids::source_ident_ok(value)
}

fn hex64(value: &str) -> bool {
    crate::ids::hex64(value)
}

fn dataset_ok(value: &str) -> bool {
    value.split_once('.').is_some_and(|(schema, table)| ident(schema) && ident(table) && !table.contains('.'))
}

fn column_id_ok(value: &str) -> bool {
    let mut parts = value.split('.');
    ident(parts.next().unwrap_or("")) && ident(parts.next().unwrap_or("")) && ident(parts.next().unwrap_or("")) && parts.next().is_none()
}

fn data_type_ok(value: &str) -> bool {
    matches!(value, "bigint" | "integer" | "numeric" | "text" | "boolean" | "date" | "timestamp" | "timestamptz" | "interval" | "jsonb" | "uuid" | "bytea" | "int2" | "float4" | "float8" | "varchar" | "bpchar" | "time" | "json")
}

#[cfg(test)]
mod interval_tests {
    use super::{generated_from_inputs, parse_model};
    use serde_json::json;

    #[test]
    fn sealed_interval_survives_generated_yaml_roundtrip() {
        let catalog = json!({"tables":[{"schema":"native_mysql_20261012","table":"value_probe",
            "columns":[{"name":"clock_at","data_type":"interval"}]}]});
        let rules = json!({"columns":[],"relationships":[]});
        let (_, yaml) = generated_from_inputs(&catalog, &rules, "none").expect("interval target type");
        let parsed = parse_model(&yaml, "generated").expect("generated YAML");
        assert_eq!(parsed.datasets["native_mysql_20261012.value_probe"].columns["clock_at"].sealed_data_type, "interval");
    }
}

fn semantic_ok(value: &str) -> bool {
    matches!(
        value,
        "identifier"
            | "foreign_key"
            | "entity_name"
            | "code"
            | "status_code"
            | "category"
            | "boolean_flag"
            | "money"
            | "quantity"
            | "ratio"
            | "event_time"
            | "date"
            | "audit_time"
            | "geo"
            | "address"
            | "phone"
            | "email"
            | "person_name"
            | "free_text"
            | "json"
            | "system"
    )
}

fn pii_ok(value: &str) -> bool {
    matches!(value, "none" | "personal" | "sensitive" | "restricted" | "unknown")
}

fn confidence_ok(value: &str) -> bool {
    matches!(value, "high" | "low" | "unknown")
}

fn cardinality_ok(value: &str) -> bool {
    matches!(value, "many_to_one" | "one_to_one" | "one_to_many" | "many_to_many")
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    value.get(key).and_then(Value::as_str).filter(|item| !item.is_empty()).ok_or_else(ApiError::malformed)
}

fn scalar(map: &BTreeMap<String, Y>, key: &str) -> Result<String, ApiError> {
    map.get(key).and_then(Y::as_scalar).map(str::to_owned).ok_or_else(ApiError::malformed)
}

fn optional(map: &BTreeMap<String, Y>, key: &str) -> String {
    map.get(key).and_then(Y::as_scalar).unwrap_or("").to_owned()
}

fn seq<'a>(map: &'a BTreeMap<String, Y>, key: &str) -> Result<&'a [Y], ApiError> {
    match map.get(key) {
        Some(Y::Seq(items)) => Ok(items),
        _ => Err(ApiError::malformed()),
    }
}

fn reject_sample(map: &BTreeMap<String, Y>) -> Result<(), ApiError> {
    if map.contains_key(SAMPLE_MARK) {
        Err(ApiError::malformed())
    } else {
        Ok(())
    }
}

fn canonical_json_pretty(value: &Value) -> String {
    crate::canonical::canonical_json(value)
}

impl Y {
    fn as_map(&self) -> Option<&BTreeMap<String, Y>> {
        match self {
            Y::Map(map) => Some(map),
            _ => None,
        }
    }

    fn as_scalar(&self) -> Option<&str> {
        match self {
            Y::Scalar(text) => Some(text),
            _ => None,
        }
    }
}

pub fn sealed_types(model: &Model) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (dataset_name, dataset) in &model.datasets {
        for (column_name, column) in &dataset.columns {
            out.insert(format!("{dataset_name}.{column_name}"), column.sealed_data_type.clone());
        }
    }
    out
}
