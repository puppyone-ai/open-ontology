//! D28 confirmation is a local compilation gate, not publisher authentication.
use std::collections::{BTreeMap, BTreeSet};

use oo_core::api::payload_sha256;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::canonical::canonical_sha256;
use crate::{RolePackCeilings, compile_role_pack_with_ceilings};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    class: String,
    subject: BTreeMap<String, String>,
    decision: String,
    evidence: Value,
    scores: BTreeMap<String, f64>,
    rule_id: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entity {
    id: String,
    label: String,
    table: String,
    key: Vec<String>,
    display: Option<String>,
    rows: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edge {
    relation: String,
    from: String,
    to: String,
    cardinality: String,
    adopted: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Graph {
    entities: Vec<Entity>,
    edges: Vec<Edge>,
    #[serde(default)]
    dictionary_bindings_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Unchecked {
    id: String,
    kind: String,
    from: String,
    to: String,
    from_columns: Vec<String>,
    to_columns: Vec<String>,
}

/// Recall coverage. It is not hashed: the builder re-derives the unchecked set
/// from the input's draft edges and the hashed graph, so it cannot be forged.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discovery {
    status: String,
    budget: u64,
    checked: u64,
    remaining: u64,
    unchecked: Vec<Unchecked>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    protocol: String,
    input_sha256: String,
    thresholds: BTreeMap<String, f64>,
    items: Vec<Item>,
    #[serde(default)]
    discovery: Option<Discovery>,
    graph: Graph,
    summary_sha256: String,
}

/// The one override key that is not an item: the owner's explicit acceptance
/// of an incomplete relationship discovery.
pub const DISCOVERY_OVERRIDE: &str = "discovery/relationships";
const ACCEPT_INCOMPLETE: &str = "accept_incomplete";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    decision: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Overrides {
    protocol: String,
    summary_sha256: String,
    items: BTreeMap<String, Override>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirmation {
    protocol: String,
    summary_sha256: String,
    overrides_sha256: Option<String>,
    reason: String,
    confirmed_at: String,
}

fn allowed(class: &str, decision: &str) -> bool {
    matches!(
        (class, decision),
        ("relation" | "entity_key", "adopted" | "rejected")
            | ("sensitive", "hidden" | "visible")
            | ("metric", "certified" | "demoted")
            | ("code_table", "complete" | "incomplete")
    )
}

pub fn adjudication_summary_sha256(document: &Value) -> String {
    let decisions: BTreeMap<_, _> = document["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| Some((item["id"].as_str()?, item["decision"].as_str()?)))
        .collect();
    canonical_sha256(&json!({"graph":document["graph"],"decisions":decisions}))
}

fn document(value: &Value) -> Result<Document, String> {
    let doc: Document = serde_json::from_value(value.clone()).map_err(|_| "adjudication_shape")?;
    if doc.protocol != "oo.adjudication/v1"
        || doc.summary_sha256 != adjudication_summary_sha256(value)
        || doc.input_sha256.len() != 64
        || doc.thresholds.values().any(|v| !v.is_finite() || *v < 0.0)
        || ![
            "relation_containment_strict",
            "relation_containment_with_signal",
            "code_coverage",
        ]
        .iter()
        .all(|k| doc.thresholds.get(*k).is_some_and(|v| *v <= 1.0))
    {
        return Err("adjudication_binding".into());
    }
    let mut ids = BTreeSet::new();
    for item in &doc.items {
        if !ids.insert(&item.id)
            || !item.id.starts_with(&format!("{}/", item.class))
            || !allowed(&item.class, &item.decision)
            || item.reason.trim().is_empty()
            || !item.rule_id.starts_with(&format!("{}.", item.class))
            || !item.evidence.is_object()
            || item.subject.is_empty()
            || item
                .scores
                .get("confidence")
                .is_none_or(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("adjudication_item".into());
        }
        // Aggregate-only output: neither an extension nor an override can carry
        // examples of sensitive values into the graph/pack.
        if item.class == "sensitive" {
            const FIELDS: &[&str] = &[
                "rows",
                "non_null",
                "distinct",
                "max_length",
                "readable",
                "phone",
                "id_card",
                "email",
                "bank_card",
                "person_name",
                "street_address",
                "contact_text",
                "birth_date",
                "pii_marker",
                "entropy",
                "name_suspicion",
                "format_hit_rate",
            ];
            if item.evidence.as_object().is_none_or(|e| {
                e.iter().any(|(k, v)| {
                    !FIELDS.contains(&k.as_str()) || !(v.is_number() || v.is_boolean())
                })
            }) {
                return Err("adjudication_sensitive_values".into());
            }
        }
    }
    let entities: BTreeMap<_, _> = doc.graph.entities.iter().map(|e| (&e.id, e)).collect();
    if entities.len() != doc.graph.entities.len() {
        return Err("adjudication_graph".into());
    }
    let mut relations = BTreeSet::new();
    for edge in &doc.graph.edges {
        let item = doc
            .items
            .iter()
            .find(|i| i.id == edge.relation)
            .ok_or("adjudication_graph")?;
        if item.class != "relation"
            || !relations.insert(&edge.relation)
            || !entities.contains_key(&edge.from)
            || !entities.contains_key(&edge.to)
            || edge.adopted != (item.decision == "adopted")
            || item.evidence["cardinality"] != edge.cardinality
            || !matches!(
                edge.cardinality.as_str(),
                "one_to_one" | "one_to_many" | "many_to_one" | "many_to_many"
            )
        {
            return Err("adjudication_graph".into());
        }
    }
    if relations.len() != doc.items.iter().filter(|i| i.class == "relation").count() {
        return Err("adjudication_graph".into());
    }
    if let Some(discovery) = &doc.discovery {
        let mut unchecked = BTreeSet::new();
        if !matches!(discovery.status.as_str(), "complete" | "incomplete")
            || (discovery.status == "complete") != discovery.unchecked.is_empty()
            || discovery.remaining != discovery.unchecked.len() as u64
            || discovery.checked != relations.len() as u64
            || discovery.unchecked.iter().any(|u| {
                !unchecked.insert(&u.id)
                    || relations.contains(&u.id)
                    || !u.id.starts_with("relation/")
                    || !matches!(
                        u.kind.as_str(),
                        "declared_fk" | "inferred_id" | "snapshot_name_mapping"
                    )
                    || u.from.is_empty()
                    || u.to.is_empty()
                    || u.from_columns.is_empty()
                    || u.from_columns.len() != u.to_columns.len()
            })
        {
            return Err("adjudication_discovery".into());
        }
        let _ = discovery.budget;
    }
    Ok(doc)
}

/// Applies only explicitly keyed overrides and recomputes the graph-bound hash.
/// The original artifact remains unchanged for audit.
pub fn apply_adjudication_overrides(original: &Value, overrides: &Value) -> Result<Value, String> {
    let doc = document(original)?;
    let changes: Overrides =
        serde_json::from_value(overrides.clone()).map_err(|_| "adjudication_override_shape")?;
    if changes.protocol != "oo.adjudication-override/v1"
        || changes.summary_sha256 != doc.summary_sha256
    {
        return Err("adjudication_override_binding".into());
    }
    let mut output = original.clone();
    for (id, change) in changes.items {
        if id == DISCOVERY_OVERRIDE {
            if doc
                .discovery
                .as_ref()
                .is_none_or(|d| d.status != "incomplete")
                || change.decision != ACCEPT_INCOMPLETE
                || change.reason.trim().is_empty()
            {
                return Err("adjudication_override_decision".into());
            }
            continue;
        }
        let original_item = doc
            .items
            .iter()
            .find(|i| i.id == id)
            .ok_or("adjudication_override_unknown")?;
        if !allowed(&original_item.class, &change.decision) || change.reason.trim().is_empty() {
            return Err("adjudication_override_decision".into());
        }
        // Owner review cannot turn a fan-out JOIN into a many-to-one relationship.
        if original_item.class == "relation"
            && change.decision == "adopted"
            && (original_item.evidence["parent_unique"] == json!(false)
                || original_item.evidence["max_fanout"]
                    .as_u64()
                    .is_some_and(|n| n > 1))
        {
            return Err("adjudication_override_fanout".into());
        }
        let item = output["items"]
            .as_array_mut()
            .ok_or("adjudication_shape")?
            .iter_mut()
            .find(|i| i["id"] == id)
            .ok_or("adjudication_shape")?;
        item["decision"] = json!(change.decision);
        item["reason"] = json!(change.reason);
        item["rule_id"] = json!(format!("{}.owner_override", original_item.class));
        if original_item.class == "relation" {
            for edge in output["graph"]["edges"]
                .as_array_mut()
                .ok_or("adjudication_shape")?
            {
                if edge["relation"] == id {
                    edge["adopted"] = json!(change.decision == "adopted");
                }
            }
        } else if original_item.class == "entity_key" {
            for entity in output["graph"]["entities"]
                .as_array_mut()
                .ok_or("adjudication_shape")?
            {
                if original_item
                    .subject
                    .get("table")
                    .is_some_and(|t| entity["table"] == *t)
                {
                    entity["key"] = if change.decision == "adopted" {
                        original_item.evidence["key"].clone()
                    } else {
                        json!([])
                    };
                    entity["display"] = if change.decision == "adopted" {
                        original_item.evidence["display"].clone()
                    } else {
                        Value::Null
                    };
                }
            }
        }
    }
    let complete: BTreeSet<String> = output["items"]
        .as_array()
        .ok_or("adjudication_shape")?
        .iter()
        .filter(|i| i["class"] == "code_table" && i["decision"] == "complete")
        .filter_map(|i| i["id"].as_str().map(str::to_owned))
        .collect();
    for item in output["items"].as_array_mut().ok_or("adjudication_shape")? {
        if item["class"] == "metric"
            && item["decision"] == "certified"
            && item["evidence"]["code_dependencies"]
                .as_array()
                .is_some_and(|deps| {
                    deps.iter()
                        .any(|d| d.as_str().is_none_or(|id| !complete.contains(id)))
                })
        {
            item["decision"] = json!("demoted");
            item["rule_id"] = json!("metric.code_dependency");
            item["reason"] = json!("A required code table is incomplete.");
            item["evidence"]["code_dependencies_complete"] = json!(false);
        }
    }
    output["summary_sha256"] = json!(adjudication_summary_sha256(&output));
    document(&output)?;
    Ok(output)
}

/// `<child>_<role>_<parent>` when an edge has a role, a stable hash otherwise.
/// Both satisfy OOS `SourceName` (`^[a-z][a-z0-9_]{0,62}$`); roles are never merged.
fn relationship_name(
    child: &str,
    role: Option<&str>,
    parent: &str,
    relation: &str,
    taken: &BTreeSet<String>,
) -> String {
    let hashed = format!("r_{}", &payload_sha256(relation.as_bytes())[..60]);
    let Some(role) = role else {
        return hashed;
    };
    let slug = |s: &str| -> String {
        let table = s.rsplit('.').next().unwrap_or(s).to_ascii_lowercase();
        let mut out = String::new();
        for c in table.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else if !out.ends_with('_') {
                out.push('_');
            }
        }
        out.trim_matches('_').to_owned()
    };
    let name = format!("{}_{}_{}", slug(child), slug(role), slug(parent));
    let readable = name.len() <= 63
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.contains("__")
        && !taken.contains(&name);
    if readable {
        name
    } else {
        // Keep the role readable and the name unique within the bound.
        let suffix = &payload_sha256(relation.as_bytes())[..8];
        let stem: String = format!("r_{}", slug(role)).chars().take(63 - 9).collect();
        format!("{}_{suffix}", stem.trim_end_matches('_'))
    }
}

/// `adjudication_input` contains the exact reviewed-community file bytes as a
/// string. This avoids silently hashing a reserialized or augmented command.
pub fn compile_confirmed_community(envelope: &Value) -> Result<Value, String> {
    let raw = envelope["adjudication_input"]
        .as_str()
        .ok_or("adjudication_input")?;
    let input: Value = serde_json::from_str(raw).map_err(|_| "adjudication_input")?;
    if input["protocol"] != "oo.builder.reviewed-community/v1" {
        return Err("adjudication_input_protocol".into());
    }
    let original = document(&envelope["adjudication"])?;
    if original.input_sha256 != payload_sha256(raw.as_bytes()) {
        return Err("adjudication_input_binding".into());
    }
    let applied = apply_adjudication_overrides(&envelope["adjudication"], &envelope["overrides"])?;
    let doc = document(&applied)?;
    let dictionary_specs = &input["adjudication_specs"]["dictionary_bindings"];
    let binding_hash = dictionary_specs
        .as_array()
        .filter(|s| !s.is_empty())
        .map(|_| canonical_sha256(dictionary_specs));
    if doc.graph.dictionary_bindings_sha256 != binding_hash {
        return Err("adjudication_dictionary_confirmation".into());
    }
    let confirmation: Confirmation = serde_json::from_value(envelope["confirmation"].clone())
        .map_err(|_| "adjudication_confirmation_shape")?;
    if confirmation.protocol != "oo.publish-confirm/v1"
        || confirmation.summary_sha256 != doc.summary_sha256
        || match &confirmation.overrides_sha256 {
            Some(hash) => *hash != canonical_sha256(&envelope["overrides"]),
            None => !envelope["overrides"]["items"]
                .as_object()
                .is_some_and(|items| items.is_empty()),
        }
        || confirmation.reason.trim().is_empty()
    {
        return Err("adjudication_confirmation_binding".into());
    }
    // CivilDate checks calendar validity; confirmation also accepts an RFC3339
    // timestamp, whose complete grammar is checked below without a new dependency.
    let date = confirmation
        .confirmed_at
        .get(..10)
        .ok_or("adjudication_confirmation_date")?;
    serde_json::from_value::<oo_core::oos::CivilDate>(json!(date))
        .map_err(|_| "adjudication_confirmation_date")?;
    if !confirmation.confirmed_at.is_ascii()
        || confirmation.confirmed_at.len() != 20
        || &confirmation.confirmed_at[10..11] != "T"
        || &confirmation.confirmed_at[13..14] != ":"
        || &confirmation.confirmed_at[16..17] != ":"
        || !confirmation.confirmed_at.ends_with('Z')
        || ![(11..13, 23), (14..16, 59), (17..19, 59)]
            .iter()
            .all(|(r, max)| {
                confirmation
                    .confirmed_at
                    .get(r.clone())
                    .and_then(|s| s.parse::<u8>().ok())
                    .is_some_and(|n| n <= *max)
            })
    {
        return Err("adjudication_confirmation_date".into());
    }
    let mut effective = input["effective"].clone();
    let mut visibility = input["visibility"].clone();
    let datasets = effective["datasets"]
        .as_array()
        .ok_or("adjudication_datasets")?;
    let inventory: BTreeMap<_, BTreeSet<_>> = datasets
        .iter()
        .map(|d| {
            Ok((
                d["dataset"]
                    .as_str()
                    .ok_or("adjudication_dataset")?
                    .to_owned(),
                d["columns"]
                    .as_array()
                    .ok_or("adjudication_columns")?
                    .iter()
                    .map(|c| {
                        c["name"]
                            .as_str()
                            .map(str::to_owned)
                            .ok_or("adjudication_column")
                    })
                    .collect::<Result<_, _>>()?,
            ))
        })
        .collect::<Result<_, &str>>()?;
    let graph_tables: BTreeSet<_> = doc.graph.entities.iter().map(|e| e.table.clone()).collect();
    if inventory.len() != datasets.len()
        || graph_tables != inventory.keys().cloned().collect()
        || graph_tables.len() != doc.graph.entities.len()
    {
        return Err("adjudication_inventory".into());
    }
    let mut covered = BTreeSet::new();
    for item in doc.items.iter().filter(|i| i.class == "sensitive") {
        let path = item.subject.get("column").ok_or("adjudication_column")?;
        let (table, column) = path.rsplit_once('.').ok_or("adjudication_column")?;
        if !inventory
            .get(table)
            .is_some_and(|cols| cols.contains(column))
            || !covered.insert(path.clone())
        {
            return Err("adjudication_column".into());
        }
        for dataset in effective["datasets"]
            .as_array_mut()
            .ok_or("adjudication_datasets")?
        {
            if dataset["dataset"] == table {
                for col in dataset["columns"]
                    .as_array_mut()
                    .ok_or("adjudication_columns")?
                {
                    if col["name"] == column {
                        col["pii"] = json!(if item.decision == "hidden" {
                            "personal"
                        } else {
                            "none"
                        });
                        col["classification"] = json!({"state":"confirmed",
                            "by":"owner-confirmation","at":date});
                    }
                }
            }
        }
        if item.decision == "hidden" {
            for dataset in visibility["datasets"]
                .as_array_mut()
                .ok_or("adjudication_visibility")?
            {
                if dataset["dataset"] == table {
                    dataset["columns"]
                        .as_array_mut()
                        .ok_or("adjudication_visibility")?
                        .retain(|c| c != column);
                }
            }
        }
    }
    if covered.len() != inventory.values().map(BTreeSet::len).sum::<usize>() {
        return Err("adjudication_column_coverage".into());
    }
    let id = |prefix: &str, s: &str| format!("{prefix}_{}", &payload_sha256(s.as_bytes())[..60]);
    let mut entities = Vec::new();
    for entity in &doc.graph.entities {
        let decisions: Vec<_> = doc
            .items
            .iter()
            .filter(|i| i.class == "entity_key" && i.subject.get("table") == Some(&entity.table))
            .collect();
        if decisions.len() != 1
            || (decisions[0].decision == "adopted") != !entity.key.is_empty()
            || entity
                .key
                .iter()
                .any(|k| !inventory[&entity.table].contains(k))
            || entity
                .display
                .as_ref()
                .is_some_and(|d| !inventory[&entity.table].contains(d))
        {
            return Err("adjudication_entity_binding".into());
        }
        if !entity.key.is_empty() {
            let key = if entity.key.len() == 1 {
                json!(entity.key[0])
            } else {
                json!(entity.key)
            };
            let display = entity
                .display
                .as_ref()
                .map_or_else(|| key.clone(), |d| json!(d));
            entities.push(
                json!({"schema_version":"oos/v1","entity":id("e",&entity.id),
                "label":entity.label,"primary_dataset":entity.table,"datasets":[entity.table],
                "key":key,"display":display,"members":[]}),
            );
        }
        let _ = entity.rows;
    }
    let mut relationships = Vec::new();
    let mut evidence_records = Vec::new();
    let mut withheld = Vec::new();
    let draft_edges = input["draft"]["graph"]["edges"]
        .as_array()
        .ok_or("adjudication_draft")?;
    // Every draft edge is either measured (in the hashed graph) or listed as
    // unchecked; a forged discovery object cannot hide a candidate.
    let measured: BTreeSet<String> = doc.graph.edges.iter().map(|e| e.relation.clone()).collect();
    let mut unchecked = BTreeSet::new();
    for edge in draft_edges {
        let id = format!(
            "relation/{}",
            edge["id"].as_str().ok_or("adjudication_draft")?
        );
        if !measured.contains(&id) && !unchecked.insert(id) {
            return Err("adjudication_edge_coverage".into());
        }
    }
    let listed: BTreeSet<String> = doc
        .discovery
        .iter()
        .flat_map(|d| d.unchecked.iter().map(|u| u.id.clone()))
        .collect();
    if draft_edges.len() != measured.len() + unchecked.len() || listed != unchecked {
        return Err("adjudication_edge_coverage".into());
    }
    if !unchecked.is_empty()
        && envelope["overrides"]["items"][DISCOVERY_OVERRIDE]["decision"] != ACCEPT_INCOMPLETE
    {
        return Err("adjudication_discovery_incomplete".into());
    }
    let grain = |table: &str| -> Value {
        doc.graph
            .entities
            .iter()
            .find(|e| e.table == table)
            .map_or_else(|| json!([]), |e| json!(e.key))
    };
    let mut names = BTreeSet::new();
    for edge in &doc.graph.edges {
        let item = doc
            .items
            .iter()
            .find(|i| i.id == edge.relation)
            .ok_or("adjudication_edge")?;
        let source = draft_edges
            .iter()
            .find(|e| item.id == format!("relation/{}", e["id"].as_str().unwrap_or("")))
            .ok_or("adjudication_edge_binding")?;
        let from = doc
            .graph
            .entities
            .iter()
            .find(|e| e.id == edge.from)
            .ok_or("adjudication_edge")?;
        let to = doc
            .graph
            .entities
            .iter()
            .find(|e| e.id == edge.to)
            .ok_or("adjudication_edge")?;
        if source["from"] != from.table
            || source["to"] != to.table
            || item.evidence["from_columns"] != source["columns"]["from"]
            || item.evidence["to_columns"] != source["columns"]["to"]
        {
            return Err("adjudication_edge_binding".into());
        }
        let ev = &item.evidence;
        let role = ev["role"].as_str().filter(|r| !r.is_empty());
        let kind = ev["kind"]
            .as_str()
            .unwrap_or(if source["provenance"] == "declared_fk" {
                "declared_fk"
            } else {
                "inferred_id"
            });
        if !edge.adopted {
            if matches!(
                item.rule_id.as_str(),
                "relation.scope_required"
                    | "relation.parent_not_unique"
                    | "relation.normalized_collision"
            ) {
                withheld.push(json!({
                    "from":{"dataset":from.table,"columns":source["columns"]["from"]},
                    "to":{"dataset":to.table,"columns":source["columns"]["to"]},
                    "kind":kind,"role":role,"rule":item.rule_id,
                    "scope_columns":ev.get("scope_columns").cloned().unwrap_or_else(|| json!([])),
                    "max_fanout":ev["max_fanout"],"parent_duplicate_keys":ev["parent_duplicate_keys"],
                    "normalized_collisions":ev["normalized_collisions"]}));
            }
            continue;
        }
        // A many-to-many or fan-out edge would multiply rows; never publish one.
        if !matches!(edge.cardinality.as_str(), "many_to_one" | "one_to_one")
            || ev["parent_unique"] == json!(false)
            || ev["max_fanout"].as_u64().is_some_and(|n| n > 1)
        {
            return Err("adjudication_relation_fanout".into());
        }
        let rows = ev["containment"].as_f64();
        let distinct = ev["distinct_containment"].as_f64();
        let complete = rows == Some(1.0) && distinct == Some(1.0);
        let join_hint = if complete { "inner" } else { "left" };
        let name = relationship_name(&from.table, role, &to.table, &edge.relation, &names);
        names.insert(name.clone());
        let mut oos_evidence = serde_json::Map::new();
        if let Some(r) = ev.get("containment").filter(|v| v.is_number()) {
            oos_evidence.insert("containment".into(), r.clone());
        }
        if let Some(t) = ev.get("type_compatible").filter(|v| v.is_boolean()) {
            oos_evidence.insert("type_compatible".into(), t.clone());
        }
        if let Some(j) = ev.get("join_cooccurrence").and_then(Value::as_u64) {
            oos_evidence.insert("seen_in_query_log".into(), json!(j));
        }
        if let Some(n) = ev["name_similarity"].as_f64() {
            let level = if n >= 1.0 {
                "exact"
            } else if n > 0.0 {
                "partial"
            } else {
                "none"
            };
            oos_evidence.insert("name_match".into(), json!(level));
        }
        relationships.push(json!({"name":name,
            "from":{"dataset":from.table,"columns":source["columns"]["from"]},
            "to":{"dataset":to.table,"columns":source["columns"]["to"]},
            "cardinality":edge.cardinality,"join_hint":join_hint,
            "provenance":if kind == "declared_fk" {"declared_fk"} else {"inferred"},
            "confidence":item.scores["confidence"],"status":"active",
            "evidence":oos_evidence,
            "review":{"state":"approved","by":"owner-confirmation","at":date}}));
        let count = |k: &str| ev[k].as_u64().unwrap_or(0);
        evidence_records.push(json!({"name":name,"kind":kind,"role":role,
            "row_containment":rows.unwrap_or(0.0),
            "distinct_containment":distinct.or(rows).unwrap_or(0.0),
            "rows_checked":count("rows_checked"),"matched_rows":count("matched_rows"),
            "unmatched_rows":count("unmatched_rows"),"empty_rows":count("empty_rows"),
            "max_fanout":ev["max_fanout"].as_u64().unwrap_or(1),
            "normalized_collisions":count("normalized_collisions"),
            "join_hint":join_hint,"from_grain":grain(&from.table),"to_grain":grain(&to.table)}));
    }
    if !evidence_records.is_empty() || !withheld.is_empty() {
        effective["relationship_evidence"] = json!({
            "protocol":crate::RELATIONSHIP_EVIDENCE_PROTOCOL,
            "adjudication_summary_sha256":doc.summary_sha256,
            "relationships":evidence_records,"withheld":withheld});
    }
    effective["entities"] = json!(entities);
    effective["relationships"] = json!(relationships);
    let mut metrics = Vec::new();
    let mut example_queries = Vec::new();
    for metric in effective["metrics"].as_array().into_iter().flatten() {
        let decisions: Vec<_> = doc
            .items
            .iter()
            .filter(|i| {
                i.class == "metric"
                    && i.subject
                        .get("metric")
                        .is_some_and(|n| metric["metric"] == *n)
            })
            .collect();
        if decisions.len() != 1 {
            return Err("adjudication_metric_coverage".into());
        }
        if decisions[0].decision == "certified" {
            let mut metric = metric.clone();
            metric["certified"] = json!(true);
            metric["status"] = json!("active");
            metrics.push(metric);
        } else {
            // Failed reconciliation cannot manufacture OOS execution verification.
            example_queries.push(json!({"question":metric["label"],"sql":metric["sql"],
                "status":"unverified","source_metric":metric["metric"]}));
        }
    }
    effective["metrics"] = json!(metrics);
    // Meanings come from the pinned reviewed input, never from coverage alone.
    // Evidence can reject an ambiguous lookup, but cannot invent its semantics.
    let mut bindings = Vec::new();
    for spec in input["adjudication_specs"]["dictionary_bindings"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let id = spec["id"].as_str().ok_or("adjudication_dictionary_id")?;
        let item = doc
            .items
            .iter()
            .find(|i| i.id == format!("code_table/dictionary-{id}"))
            .ok_or("adjudication_dictionary_coverage")?;
        if item.evidence["binding_sha256"] != canonical_sha256(spec)
            || item.rule_id != "code_table.reviewed_dictionary"
        {
            return Err("adjudication_dictionary_binding".into());
        }
        let mut evidence = serde_json::Map::new();
        for key in [
            "rows_checked",
            "matched_rows",
            "ambiguous_rows",
            "invalid_intervals",
            "overlap_pairs",
            "gap_pairs",
        ] {
            let count = item.evidence[key]
                .as_u64()
                .ok_or("adjudication_dictionary_evidence")?;
            evidence.insert(key.to_owned(), json!(count));
        }
        if ["ambiguous_rows", "invalid_intervals", "overlap_pairs"]
            .iter()
            .any(|k| evidence[*k] != 0)
        {
            continue;
        }
        let mut binding = spec.clone();
        binding["evidence"] = Value::Object(evidence);
        bindings.push(binding);
    }
    if !bindings.is_empty() || effective.get("dictionary_bindings").is_some() {
        // No unadjudicated extension from the model/effective input can survive.
        effective["dictionary_bindings"] = json!(bindings);
    }
    let ceilings: RolePackCeilings =
        serde_json::from_value(input["ceilings"].clone()).map_err(|_| "adjudication_ceilings")?;
    let pack =
        compile_role_pack_with_ceilings(&effective.to_string(), &visibility.to_string(), &ceilings)
            .map_err(|e| e.code)?;
    let artifact: Value = serde_json::from_str(&pack.artifact).map_err(|_| "adjudication_pack")?;
    Ok(json!({"protocol":"oo.builder.reviewed-community-result/v1",
        "effective":effective,"visibility":visibility,
        "summary":applied,"summary_sha256":doc.summary_sha256,
        "example_queries":example_queries,
        "producer":{"protocol":"oo.selfbuild.library-pack/v1","artifact":artifact,
            "pack_digest":pack.pack_digest,"effective_sha256":canonical_sha256(&effective),
            "visibility_sha256":canonical_sha256(&visibility),
            "review_sha256":canonical_sha256(&envelope["confirmation"]),
            "community_review_sha256":canonical_sha256(&envelope["confirmation"]),
            "community_review_required":true,"published":false,"business_owner_signed":false},
        "published":false,"authentication":"not_claimed"}))
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_adjudication_hash_vector() {
        let vector: serde_json::Value = serde_json::from_str(include_str!(
            "../../../execution/v0.3/contracts/hash-vector.json"
        ))
        .unwrap();
        assert_eq!(
            super::adjudication_summary_sha256(&vector["document"]),
            vector["expected_sha256"].as_str().unwrap()
        );
    }

    use super::*;

    fn fixture() -> Value {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tools/release-coordinator/tests/fixtures/retained_reviewed.json"
        ))
        .unwrap();
        let mut input = fixture["command"].clone();
        input.as_object_mut().unwrap().remove("review");
        input
            .as_object_mut()
            .unwrap()
            .remove("classification_review");
        input["effective"]["datasets"][0]["columns"][0]
            .as_object_mut()
            .unwrap()
            .remove("classification");
        let raw = input.to_string();
        let mut adjudication = json!({
            "protocol":"oo.adjudication/v1", "input_sha256":payload_sha256(raw.as_bytes()),
            "thresholds":{"relation_containment_strict":0.98,
                "relation_containment_with_signal":0.9,"code_coverage":0.99},
            "items":[
                {"id":"entity_key/app.items","class":"entity_key","subject":{"table":"app.items"},
                 "decision":"adopted","evidence":{"key":["amount"],"display":null},
                 "scores":{"confidence":1.0},"rule_id":"entity_key.full_unique","reason":"Full scan."},
                {"id":"sensitive/app.items.amount","class":"sensitive","subject":{"column":"app.items.amount"},
                 "decision":"visible","evidence":{"rows":2,"non_null":2,"distinct":2,"entropy":1.0},
                 "scores":{"confidence":1.0},"rule_id":"sensitive.negative_profile","reason":"No format hits."}],
            "graph":{"entities":[{"id":"entity/app.items","label":"Items","table":"app.items",
                "key":["amount"],"display":null,"rows":2}],"edges":[]}
        });
        adjudication["summary_sha256"] = json!(adjudication_summary_sha256(&adjudication));
        let overrides = json!({"protocol":"oo.adjudication-override/v1",
            "summary_sha256":adjudication["summary_sha256"],"items":{}});
        json!({"adjudication_input":raw, "adjudication":adjudication,"overrides":overrides,
            "confirmation":{"protocol":"oo.publish-confirm/v1",
                "summary_sha256":adjudication["summary_sha256"],
                "overrides_sha256":canonical_sha256(&overrides),
                "reason":"Confirm graph","confirmed_at":"2026-11-09T12:00:00Z"}})
    }

    fn confirm(input: &mut Value) {
        let applied =
            apply_adjudication_overrides(&input["adjudication"], &input["overrides"]).unwrap();
        input["confirmation"]["summary_sha256"] = applied["summary_sha256"].clone();
        input["confirmation"]["overrides_sha256"] = json!(canonical_sha256(&input["overrides"]));
    }

    #[test]
    fn dictionary_semantics_are_pinned_and_ambiguous_lookups_are_withheld() {
        let mut envelope = fixture();
        let mut command: Value =
            serde_json::from_str(envelope["adjudication_input"].as_str().unwrap()).unwrap();
        let spec = json!({
            "id":"fixture", "concept":"Reviewed fixture",
            "from":{"dataset":"app.items","code":"amount","at":"amount"},
            "dictionary":{"dataset":"app.items","code":"amount","label":"amount",
                "discriminator":"amount","value":"fixture","valid_from":"amount","valid_to":"amount"},
            "interval":"closed_open_null_end_unbounded",
            "provenance":{"origin":"operator_reviewed","source_sha256":"a".repeat(64),
                "reviewer":"fixture-owner","reviewed_at":"2026-11-12"}
        });
        command["adjudication_specs"]["dictionary_bindings"] = json!([spec]);
        let raw = command.to_string();
        envelope["adjudication_input"] = json!(raw);
        envelope["adjudication"]["input_sha256"] = json!(payload_sha256(raw.as_bytes()));
        envelope["adjudication"]["items"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id":"code_table/dictionary-fixture","class":"code_table",
                "subject":{"from":"app.items.amount","to":"app.items.amount"},
                "decision":"incomplete","rule_id":"code_table.reviewed_dictionary",
                "reason":"Reviewed meaning; measured replica coverage.",
                "scores":{"confidence":0.0},
                "evidence":{"binding_sha256":canonical_sha256(&spec),"rows_checked":2,
                    "matched_rows":1,"ambiguous_rows":0,"invalid_intervals":0,
                    "overlap_pairs":0,"gap_pairs":1}
            }));
        envelope["adjudication"]["graph"]["dictionary_bindings_sha256"] = json!(canonical_sha256(
            &command["adjudication_specs"]["dictionary_bindings"]
        ));
        envelope["adjudication"]["summary_sha256"] =
            json!(adjudication_summary_sha256(&envelope["adjudication"]));
        envelope["overrides"]["summary_sha256"] =
            envelope["adjudication"]["summary_sha256"].clone();
        confirm(&mut envelope);
        let output = compile_confirmed_community(&envelope).unwrap();
        assert_eq!(
            output["effective"]["dictionary_bindings"][0]["dictionary"]["value"],
            "fixture"
        );
        let mut drift = envelope.clone();
        drift["adjudication"]["items"][2]["evidence"]["binding_sha256"] = json!("b".repeat(64));
        assert_eq!(
            compile_confirmed_community(&drift).unwrap_err(),
            "adjudication_dictionary_binding"
        );
        let mut changed = envelope.clone();
        command["adjudication_specs"]["dictionary_bindings"][0]["dictionary"]["value"] =
            json!("changed-meaning");
        let raw = command.to_string();
        changed["adjudication_input"] = json!(raw);
        changed["adjudication"]["input_sha256"] = json!(payload_sha256(raw.as_bytes()));
        assert_eq!(
            compile_confirmed_community(&changed).unwrap_err(),
            "adjudication_dictionary_confirmation"
        );
        envelope["adjudication"]["items"][2]["evidence"]["overlap_pairs"] = json!(1);
        let output = compile_confirmed_community(&envelope).unwrap();
        assert!(output["effective"].get("dictionary_bindings").is_none());
    }

    fn relations(variant: &str) -> Value {
        let all: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/adjudication_relations.json"
        ))
        .unwrap();
        let mut input = all[variant].clone();
        let summary = input["adjudication"]["summary_sha256"].clone();
        let overrides = json!({"protocol":"oo.adjudication-override/v1",
            "summary_sha256":summary,"items":{}});
        input["confirmation"] = json!({"protocol":"oo.publish-confirm/v1",
            "summary_sha256":summary,"overrides_sha256":canonical_sha256(&overrides),
            "reason":"Confirm graph","confirmed_at":"2026-11-11T12:00:00Z"});
        input["overrides"] = overrides;
        input
    }

    fn pack_file(output: &Value, path: &str) -> Value {
        let text = output["producer"]["artifact"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == path)
            .unwrap_or_else(|| panic!("missing {path}"))["text"]
            .as_str()
            .unwrap()
            .to_owned();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn relations_carry_evidence_roles_and_never_fan_out() {
        let output = compile_confirmed_community(&relations("complete")).unwrap();
        let relationships = output["effective"]["relationships"].as_array().unwrap();
        let mut names: Vec<_> = relationships
            .iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "shipments_origin_region_regions",
                "shipments_region_regions"
            ]
        );
        let by = |name: &str| relationships.iter().find(|r| r["name"] == name).unwrap();
        let records = output["effective"]["relationship_evidence"]["relationships"]
            .as_array()
            .unwrap();
        let record = |name: &str| records.iter().find(|r| r["name"] == name).unwrap();
        for r in relationships {
            assert_ne!(r["cardinality"], "many_to_many");
            assert!(r["evidence"]["containment"].is_number());
            assert_eq!(r["evidence"]["type_compatible"], true);
            assert!(r["evidence"]["seen_in_query_log"].is_u64());
        }
        // Inner only when row and distinct coverage are both complete.
        assert_eq!(by("shipments_origin_region_regions")["join_hint"], "inner");
        assert_eq!(by("shipments_region_regions")["join_hint"], "left");
        let extension = &output["effective"]["relationship_evidence"];
        assert_eq!(extension["protocol"], crate::RELATIONSHIP_EVIDENCE_PROTOCOL);
        assert_eq!(extension["withheld"][0]["rule"], "relation.scope_required");
        assert_eq!(
            extension["withheld"][0]["scope_columns"],
            json!(["valid_from"])
        );
        assert_eq!(
            record("shipments_region_regions")["kind"],
            "snapshot_name_mapping"
        );
        assert_eq!(record("shipments_region_regions")["unmatched_rows"], 5);
        let paths = pack_file(&output, "relationships/query-paths.json");
        let evidence_text = output["producer"]["artifact"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "relationships/evidence.json")
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            paths["evidence_sha256"],
            crate::canonical::sha256_hex(evidence_text.as_bytes())
        );
        let first = &paths["paths"][0];
        assert_eq!(first["start"], "app.shipments");
        assert_eq!(first["start_grain"], json!(["id"]));
        assert_eq!(first["role"], "origin_region");
        assert_eq!(first["join"]["type"], "INNER JOIN");
        let sql = first["sql"].as_str().unwrap();
        assert!(
            sql.contains("GROUP BY join_key_1") && sql.contains("LEFT JOIN \"app\".\"regions\"")
        );
        assert!(
            paths["withheld"][0]["reason"]
                .as_str()
                .unwrap()
                .contains("valid_from")
        );
    }

    #[test]
    fn hidden_columns_trim_relationships_and_evidence() {
        let mut input = relations("complete");
        let mut command: Value =
            serde_json::from_str(input["adjudication_input"].as_str().unwrap()).unwrap();
        for dataset in command["visibility"]["datasets"].as_array_mut().unwrap() {
            if dataset["dataset"] == "app.shipments" {
                dataset["columns"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|c| c != "region_name");
            }
            if dataset["dataset"] == "app.regions" {
                dataset["columns"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|c| c != "valid_from");
            }
        }
        let raw = command.to_string();
        input["adjudication_input"] = json!(raw);
        input["adjudication"]["input_sha256"] = json!(payload_sha256(raw.as_bytes()));
        let output = compile_confirmed_community(&input).unwrap();
        let files = output["producer"]["artifact"]["files"].as_array().unwrap();
        for file in files.iter().filter(|f| {
            f["path"]
                .as_str()
                .is_some_and(|p| p.starts_with("relationships/"))
        }) {
            let text = file["text"].as_str().unwrap();
            assert!(
                !text.contains("region_name") && !text.contains("valid_from"),
                "{}",
                file["path"]
            );
        }
        assert_eq!(
            pack_file(&output, "relationships/query-paths.json")["withheld"],
            json!([])
        );
    }

    #[test]
    fn incomplete_discovery_needs_the_owner_acceptance_override() {
        let mut input = relations("incomplete");
        assert_eq!(
            compile_confirmed_community(&input).unwrap_err(),
            "adjudication_discovery_incomplete"
        );
        input["overrides"]["items"][DISCOVERY_OVERRIDE] = json!({"decision":"accept_incomplete","reason":"Budget reached; publish checked edges."});
        confirm(&mut input);
        assert!(compile_confirmed_community(&input).is_ok());
        // The unchecked list is re-derived from the bound input; hiding one fails.
        let mut forged = input.clone();
        forged["adjudication"]["discovery"]["unchecked"]
            .as_array_mut()
            .unwrap()
            .pop();
        let remaining = forged["adjudication"]["discovery"]["unchecked"]
            .as_array()
            .unwrap()
            .len();
        forged["adjudication"]["discovery"]["remaining"] = json!(remaining);
        if remaining == 0 {
            forged["adjudication"]["discovery"]["status"] = json!("complete");
        }
        assert!(compile_confirmed_community(&forged).is_err());
        // Accepting a complete discovery is not a valid override.
        let mut complete = relations("complete");
        complete["overrides"]["items"][DISCOVERY_OVERRIDE] =
            json!({"decision":"accept_incomplete","reason":"No."});
        assert_eq!(
            apply_adjudication_overrides(&complete["adjudication"], &complete["overrides"])
                .unwrap_err(),
            "adjudication_override_decision"
        );
    }

    #[test]
    fn owner_cannot_adopt_a_fan_out_join() {
        let mut input = relations("complete");
        let scoped = input["adjudication"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["rule_id"] == "relation.scope_required")
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        input["overrides"]["items"][scoped] = json!({"decision":"adopted","reason":"Looks fine."});
        assert_eq!(
            apply_adjudication_overrides(&input["adjudication"], &input["overrides"]).unwrap_err(),
            "adjudication_override_fanout"
        );
    }

    #[test]
    fn relationship_names_are_source_names_and_roles_stay_apart() {
        let taken = BTreeSet::new();
        assert_eq!(
            relationship_name(
                "app.orders",
                Some("ship_wh"),
                "app.warehouses",
                "relation/a",
                &taken
            ),
            "orders_ship_wh_warehouses"
        );
        assert!(
            relationship_name("app.orders", None, "app.w", "relation/a", &taken).starts_with("r_")
        );
        let long = relationship_name(&"x".repeat(60), Some("role"), "app.w", "relation/a", &taken);
        assert!(long.len() <= 63 && long.starts_with("r_role_"));
        let mut taken = BTreeSet::new();
        taken.insert("orders_ship_wh_warehouses".to_owned());
        let other = relationship_name(
            "app.orders",
            Some("ship_wh"),
            "app.warehouses",
            "relation/b",
            &taken,
        );
        assert_ne!(other, "orders_ship_wh_warehouses");
    }

    #[test]
    fn one_confirmation_compiles_without_per_item_review() {
        let output = crate::compile_reviewed_community(&fixture()).unwrap();
        assert_eq!(output["published"], false);
        assert_eq!(output["authentication"], "not_claimed");
        assert_eq!(output["effective"]["entities"][0]["key"], "amount");
        assert_eq!(
            output["effective"]["datasets"][0]["columns"][0]["pii"],
            "none"
        );
    }

    #[test]
    fn tui_null_override_digest_is_only_valid_for_empty_overrides() {
        let mut input = fixture();
        input["confirmation"]["overrides_sha256"] = Value::Null;
        assert!(compile_confirmed_community(&input).is_ok());
        input["overrides"]["items"]["entity_key/app.items"] =
            json!({"decision":"rejected","reason":"Plain dataset"});
        confirm(&mut input);
        input["confirmation"]["overrides_sha256"] = Value::Null;
        assert_eq!(
            compile_confirmed_community(&input).unwrap_err(),
            "adjudication_confirmation_binding"
        );
    }

    #[test]
    fn new_retained_input_cannot_fall_back_to_legacy_per_item_review() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tools/release-coordinator/tests/fixtures/retained_reviewed.json"
        ))
        .unwrap();
        let mut legacy = fixture["command"].clone();
        legacy["adjudication_replica"] = json!({"generation":"synthetic-cut"});
        assert_eq!(
            crate::compile_reviewed_community(&legacy).unwrap_err(),
            "adjudication_input"
        );
    }

    #[test]
    fn overrides_require_post_override_summary_and_bind_the_exact_override() {
        let mut input = fixture();
        input["overrides"]["items"]["entity_key/app.items"] =
            json!({"decision":"rejected","reason":"Keep a plain dataset."});
        assert!(compile_confirmed_community(&input).is_err());
        confirm(&mut input);
        let output = compile_confirmed_community(&input).unwrap();
        assert_eq!(output["effective"]["entities"], json!([]));
        assert_ne!(
            output["summary_sha256"],
            input["adjudication"]["summary_sha256"]
        );
        input["overrides"]["items"]["entity_key/app.items"]["reason"] = json!("Changed reason");
        assert!(compile_confirmed_community(&input).is_err());
    }

    #[test]
    fn incomplete_code_override_demotes_dependent_metric_before_confirmation() {
        let mut input = fixture();
        input["adjudication"]["items"].as_array_mut().unwrap().extend([
            json!({"id":"code_table/status","class":"code_table","subject":{"table":"app.items"},
                "decision":"complete","evidence":{"coverage":1.0},"scores":{"confidence":1.0},
                "rule_id":"code_table.coverage","reason":"Covered."}),
            json!({"id":"metric/count","class":"metric","subject":{"metric":"count"},
                "decision":"certified","evidence":{"code_dependencies":["code_table/status"],
                    "code_dependencies_complete":true},"scores":{"confidence":1.0},
                "rule_id":"metric.reconcile","reason":"Reconciled."}),
        ]);
        let hash = adjudication_summary_sha256(&input["adjudication"]);
        input["adjudication"]["summary_sha256"] = json!(hash);
        input["overrides"]["summary_sha256"] = json!(hash);
        let same =
            apply_adjudication_overrides(&input["adjudication"], &input["overrides"]).unwrap();
        assert_eq!(same["summary_sha256"], hash);
        input["overrides"]["items"]["code_table/status"] =
            json!({"decision":"incomplete","reason":"Missing a code."});
        let applied =
            apply_adjudication_overrides(&input["adjudication"], &input["overrides"]).unwrap();
        assert_eq!(applied["items"][3]["decision"], "demoted");
        assert_ne!(applied["summary_sha256"], hash);
    }

    #[test]
    fn reject_stale_input_graph_confirmation_and_unrecognized_overrides() {
        for mutate in [
            |v: &mut Value| {
                v["adjudication_input"] =
                    json!(format!("{} ", v["adjudication_input"].as_str().unwrap()))
            },
            |v: &mut Value| v["adjudication"]["graph"]["entities"][0]["rows"] = json!(3),
            |v: &mut Value| v["confirmation"]["summary_sha256"] = json!("0".repeat(64)),
            |v: &mut Value| v["confirmation"]["confirmed_at"] = json!("2026-11-09T99:00:00Z"),
            |v: &mut Value| v["confirmation"]["confirmed_at"] = json!("2026-11-09T12:00:00x"),
            |v: &mut Value| {
                v["overrides"]["items"]["unknown"] = json!({"decision":"visible","reason":"No."})
            },
            |v: &mut Value| {
                v["overrides"]["items"]["entity_key/app.items"] =
                    json!({"decision":"hidden","reason":"No."})
            },
        ] {
            let mut input = fixture();
            mutate(&mut input);
            assert!(compile_confirmed_community(&input).is_err());
        }
    }

    #[test]
    fn missing_column_decisions_and_sensitive_values_are_rejected() {
        for leak in [false, true] {
            let mut input = fixture();
            if leak {
                input["adjudication"]["items"][1]["evidence"]["sample"] = json!("synthetic-value");
            } else {
                input["adjudication"]["items"].as_array_mut().unwrap().pop();
            }
            let hash = adjudication_summary_sha256(&input["adjudication"]);
            input["adjudication"]["summary_sha256"] = json!(hash);
            input["overrides"]["summary_sha256"] = json!(hash);
            input["confirmation"]["summary_sha256"] = json!(hash);
            input["confirmation"]["overrides_sha256"] =
                json!(canonical_sha256(&input["overrides"]));
            assert!(compile_confirmed_community(&input).is_err());
        }
    }
}
