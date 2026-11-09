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
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    protocol: String,
    input_sha256: String,
    thresholds: BTreeMap<String, f64>,
    items: Vec<Item>,
    graph: Graph,
    summary_sha256: String,
}

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
        let original_item = doc
            .items
            .iter()
            .find(|i| i.id == id)
            .ok_or("adjudication_override_unknown")?;
        if !allowed(&original_item.class, &change.decision) || change.reason.trim().is_empty() {
            return Err("adjudication_override_decision".into());
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
    let draft_edges = input["draft"]["graph"]["edges"]
        .as_array()
        .ok_or("adjudication_draft")?;
    if draft_edges.len() != doc.graph.edges.len() {
        return Err("adjudication_edge_coverage".into());
    }
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
        if edge.adopted {
            relationships.push(json!({"name":id("r",&edge.relation),
                "from":{"dataset":from.table,"columns":source["columns"]["from"]},
                "to":{"dataset":to.table,"columns":source["columns"]["to"]},
                "cardinality":edge.cardinality,"join_hint":"left",
                "provenance":if source["provenance"]=="declared_fk" {"declared_fk"} else {"inferred"},
                "confidence":item.scores["confidence"],"status":"active",
                "review":{"state":"approved","by":"owner-confirmation","at":date}}));
        }
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
