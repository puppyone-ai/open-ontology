//! Add reviewed raw fields to a sealed library without regenerating old semantics.
//! New metric/relationship/code proposals remain pending; this is not certification.
use std::collections::{BTreeMap, BTreeSet};

use oo_core::api::payload_sha256;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{RolePackCeilings, compile_role_pack_with_ceilings};

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SemanticSnapshot {
    pub tables: BTreeMap<String, String>,
    pub communities: BTreeMap<String, String>,
    pub cut_edges: BTreeMap<String, String>,
    pub merge_items: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SemanticWork {
    pub communities: Vec<String>,
    pub cut_edges: Vec<String>,
    pub merge_items: Vec<String>,
    pub removed_communities: Vec<String>,
    pub removed_edges: Vec<String>,
    pub removed_merge_items: Vec<String>,
}

/// Only semantic inputs belong here. Observation counts and generations do not.
pub fn semantic_snapshot(
    tables: &[Value],
    graph: &crate::community::GraphPlan,
) -> Result<SemanticSnapshot> {
    let mut snapshot = SemanticSnapshot::default();
    for table in tables {
        let name = table["dataset"].as_str().ok_or("semantic_dataset")?;
        let mut columns: Vec<_> = table["columns"].as_array().ok_or("semantic_columns")?
            .iter().map(|c| json!({
                "name":c["name"], "data_type":c["data_type"], "type_name":c["type_name"],
                "source_type":c["source_type"], "nullable":c["nullable"], "comment":c["comment"],
                "classification":c["classification"], "enum":c["enum"]
            })).collect();
        columns.sort_by_key(Value::to_string);
        let relationships: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| e.from == name || e.to == name)
            .collect();
        let value = json!({"columns":columns,"primary_key":table["primary_key"],
            "comment":table["comment"],"relationships":relationships});
        if snapshot
            .tables
            .insert(name.into(), digest(&value))
            .is_some()
        {
            return Err("semantic_duplicate_table".into());
        }
        snapshot.merge_items.insert(
            crate::budget::stable_id(crate::budget::Kind::Entity, name, None),
            digest(&value),
        );
    }
    for community in &graph.communities {
        let members: BTreeMap<_, _> = community
            .tables
            .iter()
            .flat_map(|t| &graph.families[t])
            .map(|t| {
                Ok((
                    t.clone(),
                    snapshot
                        .tables
                        .get(t)
                        .ok_or("semantic_family_binding")?
                        .clone(),
                ))
            })
            .collect::<Result<_>>()?;
        snapshot.communities.insert(
            community.id.clone(),
            digest(&json!({"tables":members,"hubs":community.hub_signatures})),
        );
    }
    for edge in &graph.cut_edges {
        snapshot.cut_edges.insert(
            edge.id.clone(),
            digest(&json!({
                "edge":edge, "from":snapshot.tables.get(&edge.from).ok_or("semantic_edge_binding")?,
                "to":snapshot.tables.get(&edge.to).ok_or("semantic_edge_binding")?
            })),
        );
    }
    Ok(snapshot)
}

pub fn semantic_work(old: &SemanticSnapshot, new: &SemanticSnapshot) -> SemanticWork {
    fn changed(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> Vec<String> {
        new.iter()
            .filter(|(key, value)| old.get(*key) != Some(*value))
            .map(|(key, _)| key.clone())
            .collect()
    }
    fn removed(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> Vec<String> {
        old.keys()
            .filter(|key| !new.contains_key(*key))
            .cloned()
            .collect()
    }
    SemanticWork {
        communities: changed(&old.communities, &new.communities),
        cut_edges: changed(&old.cut_edges, &new.cut_edges),
        merge_items: changed(&old.merge_items, &new.merge_items),
        removed_communities: removed(&old.communities, &new.communities),
        removed_edges: removed(&old.cut_edges, &new.cut_edges),
        removed_merge_items: removed(&old.merge_items, &new.merge_items),
    }
}
const COLLECTIONS: [&str; 6] = [
    "relationships",
    "enums",
    "entities",
    "glossary",
    "metrics",
    "examples",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: String,
    base_bundle: Value,
    delta_effective: Value,
    delta_visibility: Value,
    delta_review: Value,
    additions: Vec<String>,
    binding: Value,
    release_versions: Value,
}

fn ensure(ok: bool, code: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(code.into()) }
}

fn digest(value: &Value) -> String {
    payload_sha256(value.to_string().as_bytes())
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("delta_{key}"))
}

fn index<'a>(value: &'a Value, key: &str) -> Result<BTreeMap<String, &'a Value>> {
    let mut result = BTreeMap::new();
    for item in value.as_array().ok_or("delta_array")? {
        let name = text(item, key)?.to_owned();
        ensure(
            result.insert(name, item).is_none(),
            "delta_duplicate_identity",
        )?;
    }
    Ok(result)
}

fn names(value: &Value) -> Result<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    for item in value.as_array().ok_or("delta_array")? {
        ensure(
            result.insert(item.as_str().ok_or("delta_column")?.to_owned()),
            "delta_duplicate_column",
        )?;
    }
    Ok(result)
}

fn review_column(name: &str, column: &Value, review: &Value) -> Result<()> {
    let decision = &review["columns"][name];
    let pii = text(column, "pii")?;
    let explicit = match decision.get("explicit_allow") {
        None => false,
        Some(value) => value.as_bool().ok_or("delta_column_review")?,
    };
    ensure(
        ["none", "personal", "sensitive", "restricted"].contains(&pii)
            && decision["pii"] == column["pii"]
            && decision["semantic_type"] == column["semantic_type"]
            && explicit == (pii != "none")
            && column["classification"]["state"] == "confirmed"
            && column["classification"]["by"] == decision["reviewer"]
            && decision["reviewer"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty())
            && !names(&review["hidden_columns"])?.contains(name),
        "delta_column_review",
    )?;
    if !column["review"].is_null() {
        let fields = decision["approved_fields"]
            .as_object()
            .ok_or("delta_semantic_review")?;
        ensure(
            decision["label_approved"] == true
                && column["review"]["state"] == "approved"
                && column["review"]["by"] == decision["reviewer"]
                && fields
                    .keys()
                    .all(|k| ["label", "description"].contains(&k.as_str()))
                && fields.values().all(Value::is_boolean)
                && fields.values().any(|v| v == true)
                && (column["label"].is_null() || fields.get("label") == Some(&json!(true)))
                && (column["description"].is_null()
                    || fields.get("description") == Some(&json!(true))),
            "delta_semantic_review",
        )?;
    }
    Ok(())
}

fn review_shape(review: &Value) -> Result<()> {
    ensure(
        review.is_object()
            && review["columns"].is_object()
            && review["columns"]
                .as_object()
                .unwrap()
                .values()
                .all(Value::is_object)
            && review["hidden_columns"].is_array(),
        "delta_review_shape",
    )?;
    names(&review["hidden_columns"])?;
    Ok(())
}

fn add_fields(
    base: &Value,
    base_visibility: &Value,
    delta: &Value,
    delta_visibility: &Value,
    review: &Value,
    additions: &BTreeSet<String>,
) -> Result<(Value, Value, Value)> {
    let old = index(&base["datasets"], "dataset")?;
    let selected = index(&delta["datasets"], "dataset")?;
    let grants = index(&delta_visibility["datasets"], "dataset")?;
    let old_grants = index(&base_visibility["datasets"], "dataset")?;
    let affected = additions
        .iter()
        .map(|name| {
            name.rsplit_once('.')
                .map(|(table, _)| table.to_owned())
                .ok_or("delta_column_identity")
        })
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    ensure(
        !affected.is_empty() && selected.keys().cloned().collect::<BTreeSet<_>>() == affected,
        "delta_dataset_scope",
    )?;
    let mut effective = base.clone();
    let mut visibility = base_visibility.clone();
    let mut seen = BTreeSet::new();
    let mut ignored = Vec::new();
    let mut dispositions = Vec::new();
    let mut unchanged = serde_json::Map::new();
    for table in effective["datasets"]
        .as_array_mut()
        .ok_or("delta_datasets")?
    {
        let name = text(table, "dataset")?.to_owned();
        let Some(proposed) = selected.get(&name) else {
            unchanged.insert(name, json!(digest(table)));
            continue;
        };
        let prior = old.get(&name).ok_or("delta_new_dataset")?;
        let old_columns = index(&prior["columns"], "name")?;
        let columns = index(&proposed["columns"], "name")?;
        ensure(
            old_columns.keys().all(|key| columns.contains_key(key)),
            "delta_removed_column",
        )?;
        let grant = grants.get(&name).ok_or("delta_missing_grant")?;
        let old_grant = old_grants.get(&name).ok_or("delta_old_grant")?;
        for field in ["row_security", "global_stats", "global_stats_proof"] {
            ensure(grant[field] == old_grant[field], "delta_grant_scope")?;
        }
        let visible = names(&grant["columns"])?;
        for (column_name, column) in columns {
            let identity = format!("{name}.{column_name}");
            if let Some(previous) = old_columns.get(&column_name) {
                ensure(
                    column["data_type"] == previous["data_type"],
                    "delta_type_changed",
                )?;
                if column != *previous {
                    ignored.push(
                        json!({"column":identity,"reason":"existing_definition_preserved",
                                        "ignored_sha256":digest(column)}),
                    );
                }
                continue;
            }
            ensure(
                additions.contains(&identity) && seen.insert(identity.clone()),
                "delta_unlisted_addition",
            )?;
            review_column(&identity, column, review)?;
            ensure(visible.contains(&column_name), "delta_column_not_visible")?;
            // Code definitions require their own evidence path, not an implicit
            // attachment from a partial model draft.
            ensure(
                column["enum"].is_null(),
                "delta_enum_requires_separate_review",
            )?;
            table["columns"]
                .as_array_mut()
                .ok_or("delta_columns")?
                .push(column.clone());
            let target = visibility["datasets"]
                .as_array_mut()
                .ok_or("delta_grants")?
                .iter_mut()
                .find(|t| t["dataset"] == name)
                .ok_or("delta_old_grant")?;
            target["columns"]
                .as_array_mut()
                .ok_or("delta_grants")?
                .push(json!(column_name));
            dispositions.push(json!({"column":identity,"status":"reviewed_raw_field",
                                     "business_codes_approved":false,"join_approved":false}));
        }
    }
    ensure(seen == *additions, "delta_additions_coverage")?;
    Ok((
        effective,
        visibility,
        json!({
            "affected_datasets":affected, "unaffected_dataset_count":unchanged.len(),
            "verified_unaffected_dataset_hashes":unchanged,
            "ignored_delta_existing_columns":ignored, "column_dispositions":dispositions,
        }),
    ))
}

/// Compile a bounded additions-only command using the existing role-pack compiler.
/// The supplied review is an operator artifact, not an authenticated business signature.
pub fn compile_library_incremental(input: &str, ceilings: &RolePackCeilings) -> Result<String> {
    let parsed = oo_core::json_scan::parse_json_value_with_limits(
        input,
        "oo.builder.library-incremental/v1",
        &ceilings.json_scan,
    )
    .map_err(|_| "delta_json_or_budget")?;
    let request: Request = serde_json::from_value(parsed).map_err(|_| "delta_request")?;
    ensure(
        request.protocol == "oo.builder.library-incremental/v1",
        "delta_protocol",
    )?;
    let base = &request.base_bundle;
    ensure(
        base["protocol"] == "oo.gateway.library-reviewed/v1",
        "delta_baseline_protocol",
    )?;
    review_shape(&base["envelope"]["review"])?;
    review_shape(&request.delta_review)?;
    for (key, value) in [
        ("base_bundle_sha256", base),
        ("delta_effective_sha256", &request.delta_effective),
        ("delta_visibility_sha256", &request.delta_visibility),
        ("delta_review_sha256", &request.delta_review),
    ] {
        ensure(request.binding[key] == digest(value), "delta_input_digest")?;
    }
    let producer = &base["envelope"]["producer"];
    ensure(
        producer["protocol"] == "oo.selfbuild.library-pack/v1"
            && producer["published"] == false
            && producer["business_owner_signed"] == false
            && base["envelope"]["output_sha256"] == digest(producer),
        "delta_baseline_envelope",
    )?;
    for (key, value) in [
        ("effective_sha256", &base["effective"]),
        ("visibility_sha256", &base["visibility"]),
        ("review_sha256", &base["envelope"]["review"]),
    ] {
        ensure(producer[key] == digest(value), "delta_baseline_digest")?;
    }
    let baseline = compile_role_pack_with_ceilings(
        &base["effective"].to_string(),
        &base["visibility"].to_string(),
        ceilings,
    )
    .map_err(|e| e.code)?;
    let artifact: Value = serde_json::from_str(&baseline.artifact).map_err(|_| "delta_artifact")?;
    ensure(
        artifact == producer["artifact"] && producer["pack_digest"] == baseline.pack_digest,
        "delta_baseline_recompile",
    )?;
    compile_role_pack_with_ceilings(
        &request.delta_effective.to_string(),
        &request.delta_visibility.to_string(),
        ceilings,
    )
    .map_err(|e| e.code)?;
    for key in ["project", "role", "principal"] {
        ensure(
            base["visibility"][key] == request.binding[key]
                && request.delta_visibility[key] == request.binding[key]
                && request.delta_review[key] == request.binding[key],
            "delta_identity",
        )?;
    }
    ensure(
        request.delta_review["review_origin"] == "operator_pinned_file"
            && base["effective"]["ontology"] == request.delta_effective["ontology"]
            && base["visibility"]["limits"] == request.delta_visibility["limits"],
        "delta_policy_or_review",
    )?;
    let sources = index(&base["bindings"], "dataset")?;
    let delta_tables = index(&request.delta_effective["datasets"], "dataset")?;
    let observed = request.binding["source_columns"]
        .as_object()
        .ok_or("delta_source_columns")?;
    let source_ids = request.binding["source_ids"]
        .as_object()
        .ok_or("delta_source_ids")?;
    ensure(
        observed.len() == delta_tables.len() && source_ids.len() == delta_tables.len(),
        "delta_source_coverage",
    )?;
    for (name, table) in &delta_tables {
        ensure(
            source_ids.get(name) == sources.get(name).map(|v| &v["source"]),
            "delta_source_identity",
        )?;
        let actual: serde_json::Map<String, Value> = index(&table["columns"], "name")?
            .into_iter()
            .map(|(name, c)| (name, c["data_type"].clone()))
            .collect();
        ensure(
            observed.get(name) == Some(&Value::Object(actual)),
            "delta_observation_binding",
        )?;
    }
    let additions = request.additions.iter().cloned().collect::<BTreeSet<_>>();
    ensure(
        !additions.is_empty() && additions.len() == request.additions.len(),
        "delta_additions",
    )?;
    let (mut effective, mut visibility, mut audit) = add_fields(
        &base["effective"],
        &base["visibility"],
        &request.delta_effective,
        &request.delta_visibility,
        &request.delta_review,
        &additions,
    )?;
    let mut merged_review = base["envelope"]["review"].clone();
    for column in &additions {
        merged_review["columns"][column] = request.delta_review["columns"][column].clone();
    }
    for (key, old) in [
        ("ontology", &base["effective"]["ontology_version"]),
        ("catalog", &base["effective"]["catalog_version"]),
        ("policy", &base["effective"]["policy"]["version"]),
        ("knowledge", &base["effective"]["knowledge"]["version"]),
    ] {
        let new = text(&request.release_versions, key)?;
        ensure(old != new, "delta_version_reuse")?;
        match key {
            "ontology" | "catalog" => {
                let field = format!("{key}_version");
                effective[&field] = json!(new);
                visibility[&field] = json!(new);
            }
            _ => {
                ensure(
                    base["effective"][key]["id"] == request.delta_effective[key]["id"],
                    "delta_asset_identity",
                )?;
                effective[key]["version"] = json!(new);
                visibility[key]["version"] = json!(new);
            }
        }
    }
    let pending = COLLECTIONS
        .iter()
        .flat_map(|category| {
            request.delta_effective[*category]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |item| {
                    json!({"category":category,"definition":item,"sha256":digest(item),
                   "status":"pending","reason":"dependent_asset_requires_separate_review"})
                })
        })
        .collect::<Vec<_>>();
    audit["pending_candidates"] = json!(pending);
    audit["baseline_pack_digest"] = json!(baseline.pack_digest);
    audit["input_binding"] = request.binding;
    audit["implementation"] = json!("rust/oo-builder/library-incremental/v1");
    audit["published"] = json!(false);
    audit["business_owner_signed"] = json!(false);
    let pack =
        compile_role_pack_with_ceilings(&effective.to_string(), &visibility.to_string(), ceilings)
            .map_err(|e| e.code)?;
    let artifact: Value = serde_json::from_str(&pack.artifact).map_err(|_| "delta_artifact")?;
    Ok(json!({
        "protocol":"oo.builder.library-incremental-result/v1",
        "effective_oos":effective,"visibility":visibility,"review":merged_review,"audit":audit,
        "artifact":artifact,"pack_digest":pack.pack_digest,"manifest_sha256":pack.manifest_sha256,
        "published":false,"business_owner_signed":false,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_cycles_ignore_data_and_scope_schema_changes() {
        let mut tables = vec![
            json!({"dataset":"app.a","columns":[{"name":"id","data_type":"text","profile":{"rows":10}}]}),
            json!({"dataset":"app.b","columns":[{"name":"id","data_type":"text"}]}),
        ];
        let graph = crate::community::build_graph(&tables, &[], &Default::default()).unwrap();
        let old = semantic_snapshot(&tables, &graph).unwrap();
        tables[0]["columns"][0]["profile"]["rows"] = json!(100);
        tables[0]["source_generation"] = json!(9);
        let data = semantic_snapshot(&tables, &graph).unwrap();
        assert!(semantic_work(&old, &data).communities.is_empty());
        assert!(semantic_work(&old, &data).cut_edges.is_empty());
        assert!(semantic_work(&old, &data).merge_items.is_empty());
        tables[0]["columns"][0]["data_type"] = json!("bigint");
        let schema = semantic_snapshot(&tables, &graph).unwrap();
        let work = semantic_work(&old, &schema);
        assert_eq!(work.communities.len(), 1);
        assert_eq!(work.merge_items.len(), 1);
    }

    #[test]
    fn growing_profiles_do_not_repartition_semantic_communities() {
        let mut tables: Vec<_> = (0..6)
            .map(|t| {
                json!({
                    "dataset":format!("app.t{t}"),"columns":(0..20).map(|c|json!({
                        "name":format!("c{c}"),"data_type":"text","profile":{"rows":1}
                    })).collect::<Vec<_>>()
                })
            })
            .collect();
        let edges: Vec<_> = (1..6)
            .map(|t| {
                json!({
                    "from":{"dataset":format!("app.t{t}"),"columns":["c0"]},
                    "to":{"dataset":format!("app.t{}",t-1),"columns":["c0"]},
                    "provenance":"declared_fk","confidence":1
                })
            })
            .collect();
        let mut budget = crate::budget::Budgeter::default();
        budget.capabilities.max_output = 128000;
        let graph = crate::community::build_graph(&tables, &edges, &budget).unwrap();
        let before = semantic_snapshot(&tables, &graph).unwrap();
        for table in &mut tables {
            for column in table["columns"].as_array_mut().unwrap() {
                column["profile"] = json!({"rows":100000,"distribution":"x".repeat(5000)});
            }
        }
        let graph = crate::community::build_graph(&tables, &edges, &budget).unwrap();
        let after = semantic_snapshot(&tables, &graph).unwrap();
        let work = semantic_work(&before, &after);
        assert!(work.communities.is_empty());
        assert!(work.cut_edges.is_empty());
        assert!(work.merge_items.is_empty());
        assert!(
            graph
                .communities
                .iter()
                .flat_map(|c| &c.batches)
                .all(|b| b.input_tokens <= b.input_available)
        );
    }

    fn fixture() -> (Value, Value, Value, Value, Value, BTreeSet<String>) {
        let col = json!({"name":"id","data_type":"text","pii":"none","semantic_type":"identifier"});
        let new = json!({"name":"status","data_type":"text","pii":"none","semantic_type":"status_code",
            "classification":{"state":"confirmed","by":"operator"}});
        let base = json!({"datasets":[{"dataset":"app.items","columns":[col.clone()]},
            {"dataset":"app.other","columns":[col.clone()]}]});
        let vis = json!({"datasets":[{"dataset":"app.items","columns":["id"],
            "row_security":"none","global_stats":"denied"}]});
        let delta = json!({"datasets":[{"dataset":"app.items","columns":[col,new]}]});
        let dvis = json!({"datasets":[{"dataset":"app.items","columns":["id","status"],
            "row_security":"none","global_stats":"denied"}]});
        let review = json!({"hidden_columns":[],"columns":{"app.items.status":{
            "pii":"none","semantic_type":"status_code","explicit_allow":false,"reviewer":"operator"}}});
        (
            base,
            vis,
            delta,
            dvis,
            review,
            BTreeSet::from(["app.items.status".into()]),
        )
    }

    #[test]
    fn preserves_other_tables_and_existing_columns() {
        let (base, vis, mut delta, dvis, review, names) = fixture();
        delta["datasets"][0]["columns"][0]["description"] = json!("unapproved rewrite");
        let (result, _, audit) = add_fields(&base, &vis, &delta, &dvis, &review, &names).unwrap();
        assert_eq!(result["datasets"][1], base["datasets"][1]);
        assert_eq!(
            result["datasets"][0]["columns"][0],
            base["datasets"][0]["columns"][0]
        );
        assert_eq!(audit["unaffected_dataset_count"], 1);
        assert_eq!(
            audit["ignored_delta_existing_columns"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn rejects_type_changes_and_unapproved_access() {
        let (base, vis, mut delta, dvis, mut review, names) = fixture();
        delta["datasets"][0]["columns"][0]["data_type"] = json!("bigint");
        assert!(add_fields(&base, &vis, &delta, &dvis, &review, &names).is_err());
        delta["datasets"][0]["columns"][0]["data_type"] = json!("text");
        review["columns"]["app.items.status"]["reviewer"] = json!("");
        assert!(add_fields(&base, &vis, &delta, &dvis, &review, &names).is_err());
    }

    #[test]
    fn rejects_missing_added_column_and_implicit_enum() {
        let (base, vis, mut delta, dvis, review, mut names) = fixture();
        names.insert("app.items.missing".into());
        assert!(add_fields(&base, &vis, &delta, &dvis, &review, &names).is_err());
        names.remove("app.items.missing");
        delta["datasets"][0]["columns"][1]["enum"] = json!("app.items.status");
        assert!(add_fields(&base, &vis, &delta, &dvis, &review, &names).is_err());
    }

    fn request_fixture() -> Value {
        let (raw_base, raw_vis, raw_delta, raw_dvis, mut review, additions) = fixture();
        let mut base = json!({
            "protocol":"oo.builder.effective-oos/v1","project":"alpha",
            "ontology_version":"one","catalog_version":"one",
            "policy":{"id":"policy","version":"one"},"knowledge":{"id":"knowledge","version":"one"},
            "ontology":{"spec_version":"oos/v1","name":"example","locale":"en-US",
                "include_schemas":["app"],"default_tier_preference":["raw"],
                "publish":{"include_unreviewed":[]},"classification":{"auto_accept":"none"},
                "lifecycle":{"deprecation_min_releases":1},
                "compile":{"per_role":true,"budgets":{"readme_tokens":2000,"table_file_tokens":3000},
                    "sample_values":{"default":"none","max_per_column":0}}},
            "datasets":raw_base["datasets"],
            "relationships":[],"enums":[],"entities":[],"glossary":[],"metrics":[],"examples":[]
        });
        for t in base["datasets"].as_array_mut().unwrap() {
            t["schema_version"] = json!("oos/v1");
            t["tier"] = json!("raw");
            t["label"] = t["dataset"].clone();
            for c in t["columns"].as_array_mut().unwrap() {
                c["classification"] = json!({"state":"confirmed","by":"operator"});
            }
        }
        let mut vis = json!({"protocol":"oo.builder.role-visibility/v1","project":"alpha",
            "role":"oo_r_reader","principal":"reader","ontology_version":"one","catalog_version":"one",
            "policy":base["policy"],"knowledge":base["knowledge"],
            "issued_for_compiler":"oo-builder/role-pack/v1","datasets":raw_vis["datasets"],
            "limits":{"max_file_bytes":262144,"max_entries":128,"max_total_bytes":1048576}});
        let mut delta = base.clone();
        delta["datasets"] = json!([base["datasets"][0].clone()]);
        delta["datasets"][0]["columns"] = raw_delta["datasets"][0]["columns"].clone();
        delta["datasets"][0]["columns"][0]["classification"] =
            json!({"state":"confirmed","by":"operator"});
        review["project"] = json!("alpha");
        review["role"] = json!("oo_r_reader");
        review["principal"] = json!("reader");
        review["review_origin"] = json!("operator_pinned_file");
        let mut old_review = review.clone();
        old_review["columns"] = json!({});
        let pack = crate::compile_role_pack(&base.to_string(), &vis.to_string()).unwrap();
        let producer = json!({"protocol":"oo.selfbuild.library-pack/v1",
            "artifact":serde_json::from_str::<Value>(&pack.artifact).unwrap(),
            "pack_digest":pack.pack_digest,"published":false,"business_owner_signed":false,
            "effective_sha256":digest(&base),"visibility_sha256":digest(&vis),
            "review_sha256":digest(&old_review)});
        let bundle = json!({"protocol":"oo.gateway.library-reviewed/v1",
            "effective":base,"visibility":vis,"envelope":{
                "producer":producer,"review":old_review,"output_sha256":digest(&producer)},
            "bindings":[{"dataset":"app.items","source":"source_a"}]});
        vis["datasets"] = raw_dvis["datasets"].clone();
        let binding = json!({"project":"alpha","role":"oo_r_reader","principal":"reader",
            "base_bundle_sha256":digest(&bundle),"delta_effective_sha256":digest(&delta),
            "delta_visibility_sha256":digest(&vis),"delta_review_sha256":digest(&review),
            "source_ids":{"app.items":"source_a"},
            "source_columns":{"app.items":{"id":"text","status":"text"}}});
        json!({"protocol":"oo.builder.library-incremental/v1","base_bundle":bundle,
            "delta_effective":delta,"delta_visibility":vis,"delta_review":review,
            "additions":additions,"binding":binding,
            "release_versions":{"ontology":"two","catalog":"two","policy":"two","knowledge":"two"}})
    }

    #[test]
    fn entrypoint_rejects_unbound_semantic_approval() {
        let mut req = request_fixture();
        assert!(compile_library_incremental(&req.to_string(), &Default::default()).is_ok());
        req["delta_effective"]["datasets"][0]["columns"][1]["review"] =
            json!({"state":"approved","by":"unapproved_reviewer"});
        req["binding"]["delta_effective_sha256"] = json!(digest(&req["delta_effective"]));
        assert!(compile_library_incremental(&req.to_string(), &Default::default()).is_err());
    }

    #[test]
    fn entrypoint_malformed_baseline_review_is_error_not_panic() {
        let mut req = request_fixture();
        req["base_bundle"]["envelope"]["review"]["columns"] = json!([]);
        req["binding"]["base_bundle_sha256"] = json!(digest(&req["base_bundle"]));
        let result = std::panic::catch_unwind(|| {
            compile_library_incremental(&req.to_string(), &Default::default())
        });
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn entrypoint_default_none_grant_and_sensitive_negative() {
        let mut req = request_fixture();
        req["delta_review"]["columns"]["app.items.status"]
            .as_object_mut()
            .unwrap()
            .remove("explicit_allow");
        req["binding"]["delta_review_sha256"] = json!(digest(&req["delta_review"]));
        assert!(compile_library_incremental(&req.to_string(), &Default::default()).is_ok());
        req["delta_review"]["columns"]["app.items.status"]["pii"] = json!("sensitive");
        req["delta_effective"]["datasets"][0]["columns"][1]["pii"] = json!("sensitive");
        req["binding"]["delta_review_sha256"] = json!(digest(&req["delta_review"]));
        req["binding"]["delta_effective_sha256"] = json!(digest(&req["delta_effective"]));
        assert!(compile_library_incremental(&req.to_string(), &Default::default()).is_err());
    }
}
