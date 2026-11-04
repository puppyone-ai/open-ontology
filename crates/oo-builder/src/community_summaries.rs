//! Role-scoped, extractive navigation. Each level consumes only child summaries.
use super::*;
use crate::budget::Budgeter;
use crate::community::build_graph;
use serde_json::{Value, json};

fn rollup(title: &str, children: &[Value], path: &str) -> Value {
    let preview = children
        .iter()
        .take(3)
        .filter_map(|c| c["title"].as_str())
        .collect::<Vec<_>>()
        .join("; ");
    json!({"title":title, "preview":preview, "preview_items":children.len().min(3),
        "children":children.len(), "path":path,
        "columns":children.iter().map(|c| c["columns"].as_u64().unwrap_or(1)).sum::<u64>()})
}

fn write_pages(
    files: &mut BTreeMap<String, String>,
    path: &str,
    level: &str,
    rows: &[Value],
    limit: usize,
) -> Result<(), RolePackError> {
    let page_path = |n| {
        if n == 0 {
            path.to_owned()
        } else {
            format!("{}-{n:04}.json", path.trim_end_matches(".json"))
        }
    };
    let encode = |rows: &[Value], next: Option<String>| {
        canonical_json(&json!({
            "protocol":"oo.knowledge.summary/v1", "level":level, "items":rows, "next":next,
            "basis":"role-visible extractive summaries; not certification"
        }))
    };
    let mut page = Vec::new();
    let mut number = 0;
    for row in rows {
        page.push(row.clone());
        if encode(&page, Some(page_path(number + 1))).len() > limit {
            page.pop();
            if page.is_empty() {
                return Err(object_budget("/summaries"));
            }
            add_file(
                files,
                &page_path(number),
                encode(&page, Some(page_path(number + 1))),
            )?;
            number += 1;
            page = vec![row.clone()];
            if encode(&page, Some(page_path(number + 1))).len() > limit {
                return Err(object_budget("/summaries"));
            }
        }
    }
    let text = encode(&page, None);
    if text.len() > limit {
        return Err(object_budget("/summaries"));
    }
    add_file(files, &page_path(number), text)
}

pub(super) fn render(
    files: &mut BTreeMap<String, String>,
    model: &Model,
    selected: &Selected,
) -> Result<(), RolePackError> {
    let limit = model.limits.max_file_bytes.min(u64::from(
        model
            .ontology
            .compile
            .budgets
            .readme_tokens
            .min(model.ontology.compile.budgets.table_file_tokens),
    )) as usize;
    let mut cards = BTreeMap::new();
    let mut tables = Vec::new();
    let mut column_rows = Vec::new();
    for (name, grant) in &model.grants {
        let dataset = &model.datasets[name];
        let columns: Vec<_> = dataset
            .columns
            .iter()
            .filter(|c| grant.columns.contains(c.name.as_str()))
            .map(|c| {
                let title = c
                    .label
                    .as_ref()
                    .map(Label::as_str)
                    .filter(|s| visible_prose(model, s))
                    .unwrap_or(c.name.as_str());
                let description =
                    description_text(model, c, model.unreviewed.contains("column_descriptions"));
                json!({"name":c.name.as_str(), "title":title, "columns":1,
                    "description":description.as_ref().map(|(pending,text)|
                        if *pending { format!("UNREVIEWED: {text}") } else { text.clone() }),
                    "unreviewed":description.as_ref().is_some_and(|(pending,_)|*pending)})
            })
            .collect();
        let path = "summaries/columns.json";
        column_rows.extend(columns.iter().cloned().map(|mut c| {
            c["dataset"] = json!(name);
            c
        }));
        let title = if visible_prose(model, dataset.label.as_str()) {
            dataset.label.as_str()
        } else {
            name
        };
        let mut card = rollup(title, &columns, path);
        card["dataset"] = json!(name);
        card["select"] = json!({"dataset":name});
        card["table_path"] = json!(format!("tables/{name}.md"));
        cards.insert(name.clone(), card);
        tables.push(json!({"dataset":name, "columns":columns.iter()
            .map(|c|json!({"name":c["name"],"data_type":"unknown"})).collect::<Vec<_>>()}));
    }
    // Repartition the visible graph, never leak hidden membership or source counts.
    let relations: Vec<_> = selected
        .relationships
        .iter()
        .filter(|r| {
            r.provenance == RelationProvenance::DeclaredFk
                || r.review
                    .as_ref()
                    .is_some_and(|v| v.state == ReviewState::Approved)
        })
        .map(|r| {
            json!({"from":r.from,"to":r.to,"confidence":r.confidence,
            "provenance":if r.provenance == RelationProvenance::DeclaredFk {
                "declared_fk" } else { "containment" }})
        })
        .collect();
    let graph = build_graph(&tables, &relations, &Budgeter::default())
        .map_err(|_| object_budget("/summaries/communities"))?;
    let mut domains: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut table_rows = Vec::new();
    let mut community_rows = Vec::new();
    for community in graph.communities {
        let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for representative in community.tables {
            for name in &graph.families[&representative] {
                let domain = name.split_once('.').expect("bound dataset").0;
                groups
                    .entry(domain.to_owned())
                    .or_default()
                    .push(cards[name].clone());
            }
        }
        for (domain, mut children) in groups {
            children.sort_by_key(|c| c["dataset"].as_str().unwrap().to_owned());
            let id = sha256_hex(format!("{}:{domain}", community.id).as_bytes());
            for child in &mut children {
                child["community"] = json!(id);
            }
            let title = children
                .iter()
                .take(3)
                .filter_map(|c| c["title"].as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let mut summary = rollup(&title, &children, "summaries/tables.json");
            summary["select"] = json!({"community":id});
            summary["domain"] = json!(domain);
            table_rows.extend(children);
            domains.entry(domain).or_default().push(summary);
        }
    }
    let mut roots = Vec::new();
    for (domain, children) in domains {
        let mut root = rollup(&domain, &children, "summaries/communities.json");
        root["select"] = json!({"domain":domain});
        roots.push(root);
        community_rows.extend(children);
    }
    write_pages(
        files,
        "summaries/columns.json",
        "columns",
        &column_rows,
        limit,
    )?;
    write_pages(
        files,
        "summaries/tables.json",
        "table_cards",
        &table_rows,
        limit,
    )?;
    write_pages(
        files,
        "summaries/communities.json",
        "communities",
        &community_rows,
        limit,
    )?;
    write_pages(files, "summaries/index.json", "domains", &roots, limit)?;
    let mut navigation = String::from(
        "\n## Hierarchical summaries\n\n\
        [Domain summaries](summaries/index.json) -> communities -> table cards -> columns. \
        Follow every `next` link and match the parent `select` fields for complete coverage. These are role-visible extractive \
        summaries, not inferred business domains or certification; previews show at most \
        three child titles. Column descriptions retain their review status.\n\n",
    );
    for root in roots.iter().take(3) {
        navigation.push_str(&format!(
            "- `{}`: {} communities, {} visible columns; [{}]({}).\n",
            root["title"].as_str().unwrap(),
            root["children"],
            root["columns"],
            root["title"].as_str().unwrap(),
            root["path"].as_str().unwrap()
        ));
    }
    for path in ["README.md", "INDEX.md"] {
        files
            .get_mut(path)
            .expect("entry point")
            .push_str(&navigation);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_pack_entry_points_follow_visible_summary_hierarchy() {
        let mut effective: Value = serde_json::from_str(include_str!(
            "../../../testing/real-db/gateway-compiled-pack/fixtures/effective.json"
        ))
        .unwrap();
        let visibility = include_str!(
            "../../../testing/real-db/gateway-compiled-pack/fixtures/visibility-picker.json"
        );
        effective["datasets"][0]["columns"][0]["description"] = json!("Order identity.");
        effective["datasets"][0]["columns"][1]["description"] = json!("Unreviewed note meaning.");
        effective["datasets"][0]["columns"][1]
            .as_object_mut()
            .unwrap()
            .remove("review");
        let pack = compile_role_pack(&effective.to_string(), visibility).unwrap();
        let artifact: Value = serde_json::from_str(&pack.artifact).unwrap();
        let files: BTreeMap<_, _> = artifact["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["path"].as_str().unwrap(), f["text"].as_str().unwrap()))
            .collect();
        for entry in ["README.md", "INDEX.md"] {
            assert!(files[entry].contains("summaries/index.json"));
            assert!(files[entry].contains("2 visible columns"));
        }
        let mut path = "summaries/index.json".to_owned();
        for level in ["domains", "communities", "table_cards", "columns"] {
            let page: Value = serde_json::from_str(files[path.as_str()]).unwrap();
            assert_eq!(page["level"], level);
            if level == "columns" {
                assert_eq!(page["items"].as_array().unwrap().len(), 2);
                assert_eq!(page["items"][0]["description"], "Order identity.");
                assert!(page["items"][1]["description"].is_null());
            } else {
                assert_eq!(page["items"][0]["columns"], 2);
                path = page["items"][0]["path"].as_str().unwrap().to_owned();
            }
        }
        for (path, text) in files {
            if path.starts_with("summaries/") {
                assert!(!text.contains("auditor_phone"));
                assert!(!text.contains("unclassified_pii"));
                assert!(!text.contains("Unreviewed note meaning"));
            }
        }
        effective["ontology"]["publish"]["include_unreviewed"] = json!(["column_descriptions"]);
        let pack = compile_role_pack(&effective.to_string(), visibility).unwrap();
        let artifact: Value = serde_json::from_str(&pack.artifact).unwrap();
        let text = artifact["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let warning = "UNREVIEWED: Unreviewed note meaning.";
        assert!(text.contains(warning));
        assert!(
            !text
                .replace(warning, "")
                .contains("Unreviewed note meaning.")
        );
    }

    #[test]
    fn summary_pages_keep_all_children_and_fail_on_oversized_items() {
        let rows: Vec<_> = (0..30)
            .map(|n| json!({"title":format!("Column {n}")}))
            .collect();
        let mut files = BTreeMap::new();
        write_pages(&mut files, "summaries/test.json", "columns", &rows, 400).unwrap();
        assert!(files.len() > 1);
        let mut path = Some("summaries/test.json".to_owned());
        let mut restored = Vec::new();
        while let Some(current) = path {
            assert!(files[&current].len() <= 400);
            let value: Value = serde_json::from_str(&files[&current]).unwrap();
            restored.extend(value["items"].as_array().unwrap().clone());
            path = value["next"].as_str().map(str::to_owned);
        }
        assert_eq!(restored, rows);
        assert!(
            write_pages(
                &mut files,
                "bad.json",
                "columns",
                &[json!({"title":"x".repeat(500)})],
                400
            )
            .is_err()
        );
    }
}
