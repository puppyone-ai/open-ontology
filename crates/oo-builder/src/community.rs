//! Deterministic graph planning and review inputs; model output never approves G14.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use oo_core::api::payload_sha256;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::budget::{Batch, Budgeter, Item, Kind, stable_id, table_items};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub columns: Value,
    pub provenance: String,
    pub confidence: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Community {
    pub id: String,
    pub tables: Vec<String>,
    pub hub_signatures: Vec<Value>,
    pub batches: Vec<Batch>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GraphPlan {
    pub families: BTreeMap<String, Vec<String>>,
    pub edges: Vec<Edge>,
    pub hubs: Vec<String>,
    pub communities: Vec<Community>,
    pub cut_edges: Vec<Edge>,
}

fn hash(value: &Value) -> String {
    payload_sha256(value.to_string().as_bytes())
}

fn structure(table: &Value) -> Value {
    let mut columns: Vec<_> = table["columns"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            json!({"name":c["name"],"data_type":c["data_type"],"type_name":c["type_name"],
            "source_type":c["source_type"],"nullable":c["nullable"],"comment":c["comment"]})
        })
        .collect();
    columns.sort_by_key(Value::to_string);
    json!({"columns":columns,"primary_key":table["primary_key"]})
}
fn family_name(name: &str) -> String {
    let (schema, table) = name.rsplit_once('.').unwrap_or(("", name));
    let mut parts: Vec<_> = table.split('_').collect();
    while parts.last().is_some_and(|p| {
        matches!(*p, "audit" | "backup" | "bak" | "tmp" | "temp")
            || (p.len() >= 4 && p.bytes().all(|b| b.is_ascii_digit()))
    }) {
        parts.pop();
    }
    if let Some(start) = parts.iter().enumerate().find_map(|(i, p)| {
        (i > 0
            && p.len() >= 4
            && parts[i..]
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
        .then_some(i)
    }) {
        parts.truncate(start);
    }
    // A name consisting only of a suffix is not a family.
    if parts.is_empty() {
        name.to_owned()
    } else {
        format!("{schema}.{}", parts.join("_"))
    }
}

pub fn signature(table: &Value) -> Value {
    let pk: BTreeSet<_> = table["primary_key"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut columns: Vec<_> = structure(table)["columns"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    columns.sort_by_key(|c| {
        (
            !pk.contains(c["name"].as_str().unwrap_or("")),
            c["name"].to_string(),
        )
    });
    columns.truncate(10);
    json!({"dataset":table["dataset"],"primary_key":table["primary_key"],"columns":columns,
        "entity":table["entity"]})
}

pub fn build_graph(
    tables: &[Value],
    candidates: &[Value],
    budget: &Budgeter,
) -> Result<GraphPlan, String> {
    let mut inventory = BTreeMap::new();
    for table in tables {
        let name = table["dataset"].as_str().ok_or("community_dataset")?;
        if inventory.insert(name.to_owned(), table).is_some() {
            return Err("community_duplicate_table".into());
        }
        table_items(std::slice::from_ref(table))?;
    }
    let mut groups: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (name, table) in &inventory {
        groups
            .entry((family_name(name), hash(&structure(table))))
            .or_default()
            .push(name.clone());
    }
    let mut representative = BTreeMap::new();
    let mut families = BTreeMap::new();
    for ((base, _), mut members) in groups {
        members.sort_by_key(|name| (name != &base, name.clone()));
        let first = members[0].clone();
        for name in &members {
            representative.insert(name.clone(), first.clone());
        }
        families.insert(first, members);
    }
    let mut relations = candidates.to_vec();
    for (name, table) in &inventory {
        for fk in table["foreign_keys"].as_array().into_iter().flatten() {
            let to = fk["to"]["dataset"]
                .as_str()
                .or_else(|| fk["referenced_dataset"].as_str());
            if let Some(to) = to {
                relations.push(json!({"from":{"dataset":name,"columns":fk["columns"]},
                    "to":{"dataset":to,"columns":fk["referenced_columns"]},"provenance":"declared_fk","confidence":1.0}));
            } else {
                return Err("community_fk_endpoint_required".into());
            }
        }
        for column in table["columns"].as_array().into_iter().flatten() {
            let column_name = column["name"].as_str().unwrap_or("");
            let target = column_name
                .strip_suffix("_id")
                .or_else(|| column_name.strip_suffix("_code"));
            if let Some(target) = target {
                let suffix = if column_name.ends_with("_id") {
                    "id"
                } else {
                    "code"
                };
                let matches: Vec<_> = inventory
                    .iter()
                    .filter(|(other, t)| {
                        let leaf = other.rsplit('.').next().unwrap_or(other);
                        (*other != name)
                            && (leaf == target || leaf == format!("{target}s"))
                            && t["columns"]
                                .as_array()
                                .is_some_and(|cols| cols.iter().any(|c| c["name"] == suffix))
                    })
                    .collect();
                if matches.len() == 1 {
                    relations.push(json!({"from":{"dataset":name,"columns":[column_name]},
                        "to":{"dataset":matches[0].0,"columns":[suffix]},"provenance":"naming","confidence":0.25}));
                }
            }
        }
    }
    let mut edges = BTreeMap::new();
    for relation in relations {
        let from = relation["from"]["dataset"]
            .as_str()
            .ok_or("community_edge_from")?;
        let to = relation["to"]["dataset"]
            .as_str()
            .ok_or("community_edge_to")?;
        if !inventory.contains_key(from) || !inventory.contains_key(to) {
            return Err("community_edge_outside_inventory".into());
        }
        let provenance = relation["provenance"].as_str().unwrap_or("containment");
        let confidence = relation["confidence"]
            .as_f64()
            .or_else(|| relation["confidence"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0.5);
        if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
            return Err("community_edge_confidence".into());
        }
        let columns = json!({"from":relation["from"]["columns"],"to":relation["to"]["columns"]});
        let id = stable_id(
            Kind::Relation,
            &json!([from, to, &columns, provenance]).to_string(),
            None,
        );
        edges.insert(
            id.clone(),
            Edge {
                id,
                from: from.into(),
                to: to.into(),
                columns,
                provenance: provenance.into(),
                confidence,
            },
        );
    }
    let edges: Vec<_> = edges.into_values().collect();
    let nodes: Vec<_> = families.keys().cloned().collect();
    let positions: BTreeMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut adjacency = vec![BTreeMap::<usize, f64>::new(); nodes.len()];
    for edge in &edges {
        let a = positions[&representative[&edge.from]];
        let b = positions[&representative[&edge.to]];
        if a != b {
            let weight = if edge.provenance == "declared_fk" {
                2.0
            } else {
                edge.confidence.max(0.01)
            };
            *adjacency[a].entry(b).or_default() += weight;
            *adjacency[b].entry(a).or_default() += weight;
        }
    }
    let threshold = 8.max((nodes.len() as f64).sqrt().ceil() as usize);
    let hub_indices: BTreeSet<_> = adjacency
        .iter()
        .enumerate()
        .filter(|(_, n)| n.len() >= threshold)
        .map(|(i, _)| i)
        .collect();
    let hubs = hub_indices
        .iter()
        .map(|i| nodes[*i].clone())
        .collect::<Vec<_>>();
    let mut local = adjacency.clone();
    for (i, row) in local.iter_mut().enumerate() {
        if hub_indices.contains(&i) {
            row.clear();
        } else {
            row.retain(|j, _| !hub_indices.contains(j));
        }
    }
    let assignments = louvain(&local);
    let mut partitions: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, c) in assignments.iter().enumerate() {
        partitions.entry(*c).or_default().push(i);
    }
    let mut communities = Vec::new();
    let mut pending: VecDeque<_> = partitions.into_values().collect();
    while let Some(members) = pending.pop_front() {
        // Budget subdivision is deterministic and follows strongest adjacent edges.
        // Each resulting batch still uses the shared two-sided planner.
        let members = connected_order(&members, &local);
        let names: Vec<_> = members.iter().map(|i| nodes[*i].clone()).collect();
        let referenced_hubs: BTreeSet<_> = members
            .iter()
            .flat_map(|i| adjacency[*i].keys())
            .filter(|j| hub_indices.contains(j))
            .copied()
            .collect();
        let signatures: Vec<_> = referenced_hubs
            .iter()
            .map(|i| signature(inventory[&nodes[*i]]))
            .collect();
        let cards: Vec<_> = names.iter().map(|n| (*inventory[n]).clone()).collect();
        let items = table_items(&cards)?;
        let batches = budget.plan(&items, &json!({"hubs":signatures}))?;
        if batches.len() > 1 && members.len() > 1 {
            let cut = budget_cut(&members, &local);
            pending.push_back(members[..cut].to_vec());
            pending.push_back(members[cut..].to_vec());
            continue;
        }
        communities.push(Community {
            id: hash(&json!(names)),
            tables: names,
            hub_signatures: signatures,
            batches,
        });
    }
    communities.sort_by(|a, b| a.id.cmp(&b.id));
    let owner: BTreeMap<_, _> = communities
        .iter()
        .flat_map(|c| c.tables.iter().map(move |t| (t.clone(), c.id.clone())))
        .collect();
    let cut_edges = edges
        .iter()
        .filter(|e| owner[&representative[&e.from]] != owner[&representative[&e.to]])
        .cloned()
        .collect();
    Ok(GraphPlan {
        families,
        edges,
        hubs,
        communities,
        cut_edges,
    })
}

fn budget_cut(members: &[usize], graph: &[BTreeMap<usize, f64>]) -> usize {
    let positions: BTreeMap<_, _> = members
        .iter()
        .enumerate()
        .map(|(i, node)| (*node, i))
        .collect();
    let mut crossing = 0.0;
    let mut best = (f64::INFINITY, usize::MAX, 1);
    let low = (members.len() / 4).max(1);
    let high = members.len() - low;
    for (i, node) in members.iter().enumerate().take(members.len() - 1) {
        for (neighbor, weight) in &graph[*node] {
            if let Some(position) = positions.get(neighbor) {
                if *position < i {
                    crossing -= weight;
                } else if *position > i {
                    crossing += weight;
                }
            }
        }
        let cut = i + 1;
        if cut >= low && cut <= high {
            let balance = (cut * 2).abs_diff(members.len());
            if crossing < best.0 || (crossing == best.0 && balance < best.1) {
                best = (crossing, balance, cut);
            }
        }
    }
    best.2
}

// Louvain: deterministic modularity local moves followed by graph contraction.
fn louvain(graph: &[BTreeMap<usize, f64>]) -> Vec<usize> {
    let mut graph = graph.to_vec();
    let mut original: Vec<_> = (0..graph.len()).collect();
    loop {
        let degree: Vec<f64> = graph.iter().map(|r| r.values().sum()).collect();
        let mass: f64 = degree.iter().sum();
        if mass == 0.0 {
            return original;
        }
        let mut labels: Vec<_> = (0..graph.len()).collect();
        let mut totals = degree.clone();
        for _ in 0..32 {
            let mut changed = false;
            for i in 0..graph.len() {
                let old = labels[i];
                totals[old] -= degree[i];
                let mut weights = BTreeMap::<usize, f64>::new();
                for (j, w) in &graph[i] {
                    if i != *j {
                        *weights.entry(labels[*j]).or_default() += w;
                    }
                }
                let score = |c: usize| {
                    weights.get(&c).copied().unwrap_or(0.0) - degree[i] * totals[c] / mass
                };
                let mut best = old;
                let mut gain = score(old);
                for c in weights.keys() {
                    let proposed = score(*c);
                    if proposed > gain + 1e-10 {
                        best = *c;
                        gain = proposed;
                    }
                }
                totals[best] += degree[i];
                labels[i] = best;
                changed |= best != old;
            }
            if !changed {
                break;
            }
        }
        let unique: BTreeSet<_> = labels.iter().copied().collect();
        let index: BTreeMap<_, _> = unique.iter().enumerate().map(|(i, c)| (*c, i)).collect();
        for label in &mut original {
            *label = index[&labels[*label]];
        }
        if unique.len() == graph.len() {
            return original;
        }
        let mut next = vec![BTreeMap::<usize, f64>::new(); unique.len()];
        for (i, row) in graph.iter().enumerate() {
            for (j, w) in row {
                *next[index[&labels[i]]]
                    .entry(index[&labels[*j]])
                    .or_default() += w;
            }
        }
        graph = next;
    }
}
fn connected_order(members: &[usize], graph: &[BTreeMap<usize, f64>]) -> Vec<usize> {
    let mut remaining: BTreeSet<_> = members.iter().copied().collect();
    let mut result: Vec<usize> = Vec::new();
    while !remaining.is_empty() {
        let next = result
            .last()
            .and_then(|i| {
                graph[*i]
                    .iter()
                    .filter(|(j, _)| remaining.contains(j))
                    .max_by(|(a, x), (b, y)| x.total_cmp(y).then_with(|| b.cmp(a)))
                    .map(|(j, _)| *j)
            })
            .unwrap_or_else(|| *remaining.first().unwrap());
        remaining.remove(&next);
        result.push(next);
    }
    result
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entity {
    pub id: String,
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub main_table: String,
    pub related_tables: Vec<String>,
    pub primary_key: Value,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Registry {
    pub entities: BTreeMap<String, Entity>,
}
impl Registry {
    pub fn insert(&mut self, entity: Entity, inventory: &BTreeSet<String>) -> Result<(), String> {
        if entity.id != stable_id(Kind::Entity, &entity.main_table, None)
            || entity.canonical_name.trim().is_empty()
            || !inventory.contains(&entity.main_table)
            || entity.related_tables.iter().any(|t| !inventory.contains(t))
        {
            return Err("registry_entity_binding".into());
        }
        self.entities.insert(entity.id.clone(), entity);
        Ok(())
    }
    pub fn compact(&self) -> Value {
        json!(
            self.entities
                .values()
                .map(|e| json!({"id":e.id,"canonical_name":e.canonical_name,
            "primary_key":e.primary_key}))
                .collect::<Vec<_>>()
        )
    }
    pub fn conflict_groups(&self) -> Vec<Value> {
        let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for entity in self.entities.values() {
            for name in entity
                .aliases
                .iter()
                .chain(std::iter::once(&entity.canonical_name))
            {
                groups
                    .entry(format!("name:{}", name.trim().to_lowercase()))
                    .or_default()
                    .insert(entity.id.clone());
            }
            for table in entity
                .related_tables
                .iter()
                .chain(std::iter::once(&entity.main_table))
            {
                groups
                    .entry(format!("table:{table}"))
                    .or_default()
                    .insert(entity.id.clone());
            }
        }
        // Group conflicts avoid an unbounded quadratic list of pairwise clashes.
        groups
            .into_iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(key, ids)| json!({"id":hash(&json!([key,ids])),"key":key,"entities":ids}))
            .collect()
    }
    pub fn naming_conflicts(&self) -> Vec<String> {
        self.conflict_groups()
            .iter()
            .map(|v| v["id"].as_str().unwrap().to_owned())
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReviewDecision {
    pub reviewer: String,
    pub decision: String,
    pub summary_sha256: String,
}

pub fn connectivity(
    graph: &GraphPlan,
    registry: &Registry,
    mapped: &BTreeSet<String>,
    rejections: &BTreeMap<String, String>,
    core: &BTreeSet<String>,
    orphan_threshold: f64,
) -> Result<Value, String> {
    if !orphan_threshold.is_finite() || !(0.0..=1.0).contains(&orphan_threshold) {
        return Err("connectivity_threshold".into());
    }
    let mut owners: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut adjacency: BTreeMap<String, BTreeSet<String>> = registry
        .entities
        .keys()
        .map(|k| (k.clone(), BTreeSet::new()))
        .collect();
    for entity in registry.entities.values() {
        for table in entity
            .related_tables
            .iter()
            .chain(std::iter::once(&entity.main_table))
        {
            owners
                .entry(table.clone())
                .or_default()
                .insert(entity.id.clone());
        }
    }
    if !core.iter().all(|id| adjacency.contains_key(id)) {
        return Err("connectivity_core_entity".into());
    }
    let known: BTreeSet<_> = graph.edges.iter().map(|e| e.id.clone()).collect();
    if !mapped.is_subset(&known)
        || rejections
            .keys()
            .any(|k| !known.contains(k) || mapped.contains(k))
    {
        return Err("connectivity_relationship_binding".into());
    }
    for edge in graph.edges.iter().filter(|e| mapped.contains(&e.id)) {
        if let (Some(from), Some(to)) = (owners.get(&edge.from), owners.get(&edge.to)) {
            for a in from {
                for b in to {
                    if a != b {
                        adjacency.get_mut(a).unwrap().insert(b.clone());
                        adjacency.get_mut(b).unwrap().insert(a.clone());
                    }
                }
            }
        }
    }
    let declared: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.provenance == "declared_fk")
        .collect();
    let missing: Vec<_> = declared
        .iter()
        .filter(|e| {
            !mapped.contains(&e.id) && !rejections.get(&e.id).is_some_and(|s| !s.trim().is_empty())
        })
        .map(|e| e.id.clone())
        .collect();
    let orphan_count = adjacency.values().filter(|v| v.is_empty()).count();
    let ratio = if adjacency.is_empty() {
        0.0
    } else {
        orphan_count as f64 / adjacency.len() as f64
    };
    let mut visited = BTreeSet::new();
    let mut components = 0;
    for start in adjacency.keys() {
        if !visited.contains(start) {
            components += 1;
            let mut queue = VecDeque::from([start.clone()]);
            while let Some(node) = queue.pop_front() {
                if visited.insert(node.clone()) {
                    queue.extend(adjacency[&node].iter().cloned());
                }
            }
        }
    }
    let mut reachable = core.clone();
    let mut frontier = core.clone();
    for _ in 0..3 {
        let next: BTreeSet<_> = frontier
            .iter()
            .flat_map(|n| adjacency[n].iter().cloned())
            .filter(|n| !reachable.contains(n))
            .collect();
        reachable.extend(next.iter().cloned());
        frontier = next;
    }
    let tables: BTreeSet<_> = graph.families.values().flatten().cloned().collect();
    let reached = tables
        .iter()
        .filter(|t| {
            owners
                .get(*t)
                .is_some_and(|ids| ids.iter().any(|id| reachable.contains(id)))
        })
        .count();
    let conflicts = registry.naming_conflicts();
    let mapped_fk = declared.iter().filter(|e| mapped.contains(&e.id)).count();
    let summary = json!({"declared_fk_count":declared.len(),"mapped_fk_count":mapped_fk,
        "fk_mapping_rate":if declared.is_empty() { 1.0 } else { mapped_fk as f64 / declared.len() as f64 },
        "unmapped_without_reason":missing,"rejection_reasons":rejections,"orphan_entity_ratio":ratio,
        "orphan_review_required":ratio>orphan_threshold,"connected_components":components,
        "three_hop_table_ratio":if tables.is_empty() { 0.0 } else { reached as f64 / tables.len() as f64 },
        "core_entities":core,"naming_conflicts":conflicts,"naming_conflict_count":conflicts.len(),
        "unassigned_tables":tables.iter().filter(|t|!owners.contains_key(*t)).collect::<Vec<_>>(),
        "model_certified":false});
    Ok(summary)
}

pub fn gate_g14(
    summary: Option<&Value>,
    decisions: &BTreeMap<String, ReviewDecision>,
) -> Result<(), String> {
    let summary = summary.ok_or("g14_missing_summary")?;
    let missing = summary["unmapped_without_reason"]
        .as_array()
        .ok_or("g14_summary_shape")?;
    let conflicts = summary["naming_conflicts"]
        .as_array()
        .ok_or("g14_summary_shape")?;
    let orphans = summary["orphan_review_required"]
        .as_bool()
        .ok_or("g14_summary_shape")?;
    if !missing.is_empty() {
        return Err("g14_unmapped_fk".into());
    }
    let mut required: Vec<_> = conflicts
        .iter()
        .map(|v| v.as_str().ok_or("g14_summary_shape"))
        .collect::<Result<_, _>>()?;
    if orphans {
        required.push("orphan_entities");
    }
    let digest = hash(summary);
    for id in required {
        if !decisions.get(id).is_some_and(|d| {
            !d.reviewer.trim().is_empty()
                && d.reviewer != "model"
                && d.decision == "approved"
                && d.summary_sha256 == digest
        }) {
            return Err("g14_review_required".into());
        }
    }
    Ok(())
}

pub fn relation_items(edges: &[Edge]) -> Vec<Item> {
    edges
        .iter()
        .map(|edge| Item {
            id: edge.id.clone(),
            kind: Kind::Relation,
            input: json!(edge),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table(name: &str) -> Value {
        json!({"dataset":name,"primary_key":["id"],"columns":[{"name":"id","data_type":"bigint"}]})
    }
    #[test]
    fn fold_only_identical_shapes_and_preserve_members() {
        let mut changed = table("app.order_backup");
        changed["columns"][0]["data_type"] = json!("text");
        let graph = build_graph(
            &[table("app.order"), table("app.order_2023"), changed],
            &[],
            &Budgeter::default(),
        )
        .unwrap();
        assert_eq!(graph.families["app.order"].len(), 2);
        assert_eq!(graph.families.len(), 2);
    }
    #[test]
    fn thousand_tables_have_bounded_batches_and_hub_signatures() {
        let tables: Vec<_> = (0..1000).map(|i| table(&format!("app.t{i}"))).collect();
        let edges: Vec<_> = (1..1000)
            .map(|i| {
                json!({"from":{"dataset":format!("app.t{i}")},
            "to":{"dataset":"app.t0"},"provenance":"declared_fk","confidence":1})
            })
            .collect();
        let graph = build_graph(&tables, &edges, &Budgeter::default()).unwrap();
        assert_eq!(graph.families.values().map(Vec::len).sum::<usize>(), 1000);
        assert_eq!(graph.hubs, vec!["app.t0"]);
        assert_eq!(graph.cut_edges.len(), 999);
        for community in &graph.communities {
            assert!(
                community
                    .batches
                    .iter()
                    .all(|b| b.input_tokens <= b.input_available && b.output_reserve <= 4000)
            );
            if community.tables != vec!["app.t0"] {
                assert_eq!(community.hub_signatures.len(), 1);
            }
        }
    }
    #[test]
    fn g14_requires_hash_bound_operator_decisions_not_good_numbers() {
        let graph = build_graph(&[table("a.t")], &[], &Budgeter::default()).unwrap();
        let mut registry = Registry::default();
        registry
            .insert(
                Entity {
                    id: stable_id(Kind::Entity, "a.t", None),
                    canonical_name: "T".into(),
                    aliases: vec![],
                    main_table: "a.t".into(),
                    related_tables: vec![],
                    primary_key: json!(["id"]),
                },
                &BTreeSet::from(["a.t".into()]),
            )
            .unwrap();
        let summary = connectivity(
            &graph,
            &registry,
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
            0.2,
        )
        .unwrap();
        assert!(gate_g14(Some(&summary), &BTreeMap::new()).is_err());
        let mut decisions = BTreeMap::from([(
            "orphan_entities".into(),
            ReviewDecision {
                reviewer: "operator".into(),
                decision: "approved".into(),
                summary_sha256: hash(&summary),
            },
        )]);
        assert!(gate_g14(Some(&summary), &decisions).is_ok());
        decisions.get_mut("orphan_entities").unwrap().reviewer = "model".into();
        assert!(gate_g14(Some(&summary), &decisions).is_err());
        assert!(gate_g14(None, &BTreeMap::new()).is_err());
    }

    #[test]
    fn connectivity_reports_entity_components_and_three_hop_table_coverage() {
        let tables: Vec<_> = (0..5).map(|i| table(&format!("app.t{i}"))).collect();
        let edges: Vec<_> = (1..5)
            .map(|i| {
                json!({
                    "from":{"dataset":format!("app.t{i}")},
                    "to":{"dataset":format!("app.t{}",i-1)},
                    "provenance":"declared_fk","confidence":1
                })
            })
            .collect();
        let graph = build_graph(&tables, &edges, &Budgeter::default()).unwrap();
        let names: BTreeSet<_> = tables
            .iter()
            .map(|t| t["dataset"].as_str().unwrap().to_owned())
            .collect();
        let mut registry = Registry::default();
        for name in &names {
            registry
                .insert(
                    Entity {
                        id: stable_id(Kind::Entity, name, None),
                        canonical_name: name.clone(),
                        aliases: vec![],
                        main_table: name.clone(),
                        related_tables: vec![],
                        primary_key: json!(["id"]),
                    },
                    &names,
                )
                .unwrap();
        }
        let mapped = graph.edges.iter().map(|e| e.id.clone()).collect();
        let core = BTreeSet::from([stable_id(Kind::Entity, "app.t0", None)]);
        let summary =
            connectivity(&graph, &registry, &mapped, &BTreeMap::new(), &core, 0.2).unwrap();
        assert_eq!(summary["fk_mapping_rate"], 1.0);
        assert_eq!(summary["connected_components"], 1);
        assert_eq!(summary["orphan_entity_ratio"], 0.0);
        assert_eq!(summary["three_hop_table_ratio"], 0.8);
        assert!(gate_g14(Some(&summary), &BTreeMap::new()).is_ok());
    }
}
