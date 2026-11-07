//! Local operator-reviewed registry conversion. Authentication stays with the publisher.
use std::collections::{BTreeMap, BTreeSet};

use oo_core::api::payload_sha256;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::budget::{Kind, stable_id};
use crate::community::{GraphPlan, Registry, ReviewDecision, connectivity, gate_g14};
use crate::{RolePackCeilings, compile_role_pack_with_ceilings};

fn hash(value: &Value) -> String {
    payload_sha256(value.to_string().as_bytes())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntitySelection {
    /// One column (string) or the ordered composite the catalog declares (array).
    key: oo_core::oos::EntityKey,
    display: oo_core::oos::EntityKey,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    reviewer: String,
    reviewed_at: String,
    mode: String,
    review_origin: String,
    draft_sha256: String,
    effective_sha256: String,
    summary_sha256: String,
    entities: BTreeMap<String, EntitySelection>,
    decisions: BTreeMap<String, ReviewDecision>,
}

/// Converts a complete candidate registry only after independently supplied review.
/// Existing classifications and grants are preserved, not inferred by this adapter.
pub fn compile_reviewed_community(input: &Value) -> Result<Value, String> {
    if input["protocol"] != "oo.builder.reviewed-community/v1" {
        return Err("community_review_protocol".into());
    }
    let draft = &input["draft"];
    let classification = &input["classification_review"];
    if classification["review_origin"] != "operator_pinned_file"
        || classification["project"] != input["effective"]["project"]
        || classification["role"] != input["visibility"]["role"]
        || classification["principal"] != input["visibility"]["principal"]
    {
        return Err("community_review_classification_binding".into());
    }
    let review: Review =
        serde_json::from_value(input["review"].clone()).map_err(|_| "community_review_shape")?;
    serde_json::from_value::<oo_core::oos::CivilDate>(json!(review.reviewed_at))
        .map_err(|_| "community_review_date")?;
    if draft["protocol"] != "oo.selfbuild.community-draft/v1"
        || draft["unresolved"].as_array().is_none_or(|a| !a.is_empty())
        || review.reviewer.trim().is_empty()
        || review.reviewer.eq_ignore_ascii_case("model")
        || !matches!(review.mode.as_str(), "self" | "independent")
        || review.review_origin != "operator_pinned_file"
        || review.draft_sha256 != hash(draft)
        || review.effective_sha256 != hash(&input["effective"])
    {
        return Err("community_review_binding".into());
    }
    let graph: GraphPlan =
        serde_json::from_value(draft["graph"].clone()).map_err(|_| "community_review_graph")?;
    let registry: Registry = serde_json::from_value(draft["registry"].clone())
        .map_err(|_| "community_review_registry")?;
    let inventory: BTreeSet<_> = graph.families.values().flatten().cloned().collect();
    let mut checked = Registry::default();
    for (id, entity) in &registry.entities {
        if id != &entity.id {
            return Err("community_review_registry".into());
        }
        checked.insert(entity.clone(), &inventory)?;
    }
    if checked.entities.keys().collect::<Vec<_>>() != review.entities.keys().collect::<Vec<_>>() {
        return Err("community_review_entity_selection".into());
    }
    let accepted = draft["accepted"]
        .as_object()
        .ok_or("community_review_items")?;
    let mut mapped = BTreeSet::new();
    let mut rejections = BTreeMap::new();
    for edge in &graph.edges {
        let value = accepted
            .get(&edge.id)
            .ok_or("community_review_missing_edge")?;
        if value["kind"] != "relation" || value["id"] != edge.id {
            return Err("community_review_edge_binding".into());
        }
        match value["decision"].as_str() {
            Some("proposed") => {
                mapped.insert(edge.id.clone());
            }
            Some("rejected") => {
                let reason = value["reason"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or("community_review_rejection")?;
                rejections.insert(edge.id.clone(), reason.to_owned());
            }
            _ => return Err("community_review_edge_decision".into()),
        }
    }
    let core = serde_json::from_value(draft["review"]["summary"]["core_entities"].clone())
        .map_err(|_| "community_review_core")?;
    let threshold = input["orphan_threshold"]
        .as_f64()
        .ok_or("community_review_threshold")?;
    let summary = connectivity(&graph, &checked, &mapped, &rejections, &core, threshold)?;
    if hash(&summary) != review.summary_sha256
        || summary["unassigned_tables"]
            .as_array()
            .is_none_or(|v| !v.is_empty())
    {
        return Err("community_review_summary_binding".into());
    }
    gate_g14(Some(&summary), &review.decisions)?;
    let mut effective = input["effective"].clone();
    let datasets = effective["datasets"]
        .as_array_mut()
        .ok_or("community_review_datasets")?;
    let names: BTreeSet<_> = datasets
        .iter()
        .filter_map(|d| d["dataset"].as_str().map(str::to_owned))
        .collect();
    if names != inventory || names.len() != datasets.len() {
        return Err("community_review_inventory_binding".into());
    }
    for dataset in datasets {
        let name = dataset["dataset"]
            .as_str()
            .ok_or("community_review_dataset")?
            .to_owned();
        let table = accepted
            .get(&stable_id(Kind::Table, &name, None))
            .ok_or("community_review_missing_table")?;
        dataset["label"] = table["label"].clone();
        dataset["description"] = table["description"].clone();
        for column in dataset["columns"]
            .as_array_mut()
            .ok_or("community_review_columns")?
        {
            let name = stable_id(Kind::Column, &name, column["name"].as_str());
            let candidate = accepted
                .get(&name)
                .ok_or("community_review_missing_column")?;
            column["label"] = candidate["label"].clone();
            column["description"] = candidate["description"].clone();
        }
    }
    // A replacement needs explicit reconciliation with existing curated semantics.
    for key in ["entities", "relationships"] {
        if effective[key].as_array().is_some_and(|a| !a.is_empty()) {
            return Err("community_review_existing_semantics".into());
        }
    }
    let oos_id =
        |prefix: &str, id: &str| format!("{prefix}_{}", &payload_sha256(id.as_bytes())[..60]);
    // A composite identity is accepted only as the declared primary key, in order;
    // an invented column tuple has no uniqueness evidence.
    for entity in checked.entities.values() {
        let key = &review.entities[&entity.id].key;
        if key.is_composite() {
            let declared = input["effective"]["datasets"]
                .as_array()
                .and_then(|d| {
                    d.iter()
                        .find(|d| d["dataset"] == entity.main_table.as_str())
                })
                .map(|d| &d["primary_key"]);
            if declared != Some(&json!(key)) {
                return Err("community_review_composite_key".into());
            }
        }
    }
    let entities: Vec<_> = checked
        .entities
        .values()
        .map(|entity| {
            let selection = &review.entities[&entity.id];
            json!({"schema_version":"oos/v1","entity":oos_id("e",&entity.id),
            "label":entity.canonical_name,"synonyms":entity.aliases,
            "primary_dataset":entity.main_table,"datasets":entity.related_tables,
            "key":selection.key,"display":selection.display,"members":[]})
        })
        .collect();
    let relationships: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| mapped.contains(&e.id))
        .map(|edge| {
            let value = &accepted[&edge.id];
            json!({"name":oos_id("r",&edge.id),
            "from":{"dataset":edge.from,"columns":edge.columns["from"]},
            "to":{"dataset":edge.to,"columns":edge.columns["to"]},
            "cardinality":value["cardinality"],"join_hint":"left",
            "provenance":if edge.provenance=="declared_fk" {"declared_fk"} else {"inferred"},
            "confidence":edge.confidence,"status":"active",
            "review":{"state":"approved","by":review.reviewer,"at":review.reviewed_at}})
        })
        .collect();
    effective["entities"] = json!(entities);
    effective["relationships"] = json!(relationships);
    let ceilings: RolePackCeilings = serde_json::from_value(input["ceilings"].clone())
        .map_err(|_| "community_review_ceilings")?;
    let pack = compile_role_pack_with_ceilings(
        &effective.to_string(),
        &input["visibility"].to_string(),
        &ceilings,
    )
    .map_err(|e| e.code)?;
    let artifact: Value =
        serde_json::from_str(&pack.artifact).map_err(|_| "community_review_pack")?;
    Ok(json!({"protocol":"oo.builder.reviewed-community-result/v1",
        "effective":effective,"summary":summary,"summary_sha256":hash(&summary),
        "producer":{"protocol":"oo.selfbuild.library-pack/v1","artifact":artifact,
            "pack_digest":pack.pack_digest,"effective_sha256":hash(&effective),
            "visibility_sha256":hash(&input["visibility"]),"review_sha256":hash(classification),
            "community_review_sha256":hash(&input["review"]),
            "community_review_required":true,"published":false,"business_owner_signed":false},
        "published":false,"authentication":"not_claimed"}))
}

#[cfg(test)]
mod composite_key_tests {
    use super::*;

    // Real `oo review` outputs over the same replay model; see retained_fixture.py.
    const SCALAR: &str =
        include_str!("../../../tools/release-coordinator/tests/fixtures/retained_reviewed.json");
    const COMPOSITE: &str = include_str!(
        "../../../tools/release-coordinator/tests/fixtures/retained_reviewed_composite.json"
    );

    fn command(text: &str) -> Value {
        serde_json::from_str::<Value>(text).unwrap()["command"].clone()
    }

    fn selection(command: &mut Value) -> &mut Value {
        command["review"]["entities"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap()
    }

    fn pack_text(compiled: &Value) -> String {
        compiled["producer"]["artifact"]["files"].to_string()
    }

    #[test]
    fn scalar_and_composite_reviews_compile_to_their_declared_keys() {
        let scalar = compile_reviewed_community(&command(SCALAR)).unwrap();
        let fixture: Value = serde_json::from_str(SCALAR).unwrap();
        assert_eq!(scalar, fixture["compiled"], "scalar output is unchanged");
        assert_eq!(scalar["effective"]["entities"][0]["key"], json!("amount"));
        assert!(pack_text(&scalar).contains("key: `amount`"));

        let composite = compile_reviewed_community(&command(COMPOSITE)).unwrap();
        let fixture: Value = serde_json::from_str(COMPOSITE).unwrap();
        assert_eq!(composite, fixture["compiled"]);
        let entity = &composite["effective"]["entities"][0];
        assert_eq!(entity["key"], json!(["order_id", "line_no"]));
        assert_eq!(entity["display"], json!(["order_id", "line_no"]));
        assert!(pack_text(&composite).contains("key: `order_id`, `line_no`"));
    }

    #[test]
    fn composite_selection_must_be_the_declared_primary_key_in_order() {
        for (key, code) in [
            (
                json!(["line_no", "order_id"]),
                "community_review_composite_key",
            ),
            (json!(["order_id", "order_id"]), "community_review_shape"),
            (json!(["order_id"]), "community_review_shape"),
            (json!([]), "community_review_shape"),
            (json!(null), "community_review_shape"),
        ] {
            let mut bad = command(COMPOSITE);
            selection(&mut bad)["key"] = key.clone();
            assert_eq!(
                compile_reviewed_community(&bad).err().as_deref(),
                Some(code),
                "{key}"
            );
        }
        // An owner may still pick one declared column as a scalar key and display.
        let mut scalar = command(COMPOSITE);
        selection(&mut scalar)["key"] = json!("order_id");
        selection(&mut scalar)["display"] = json!("line_no");
        let compiled = compile_reviewed_community(&scalar).unwrap();
        assert_eq!(
            compiled["effective"]["entities"][0]["key"],
            json!("order_id")
        );
    }

    #[test]
    fn composite_key_columns_are_checked_and_hidden_columns_hide_the_entity() {
        let compiled = compile_reviewed_community(&command(COMPOSITE)).unwrap();
        let input = command(COMPOSITE);
        let ceilings: RolePackCeilings = serde_json::from_value(input["ceilings"].clone()).unwrap();
        let mut unknown = compiled["effective"].clone();
        unknown["entities"][0]["display"] = json!(["order_id", "missing"]);
        let error = compile_role_pack_with_ceilings(
            &unknown.to_string(),
            &input["visibility"].to_string(),
            &ceilings,
        )
        .err()
        .unwrap();
        assert!(error.code.ends_with("oos.unknown_column"), "{}", error.code);
        let mut visibility = input["visibility"].clone();
        visibility["datasets"][0]["columns"] = json!(["order_id"]);
        let pack = compile_role_pack_with_ceilings(
            &compiled["effective"].to_string(),
            &visibility.to_string(),
            &ceilings,
        )
        .unwrap();
        assert!(!pack.artifact.contains("entities/"), "{}", pack.artifact);
    }
}
