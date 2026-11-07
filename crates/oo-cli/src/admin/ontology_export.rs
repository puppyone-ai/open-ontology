//! Export only the supplied Pack, including all graph/summary continuations.
use oo_core::api::payload_sha256;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
type Result<T> = std::result::Result<T, &'static str>;

pub struct Model {
    pub role: String,
    pub entities: Vec<Value>,
    pub relationships: Vec<Value>,
    pub tables: Vec<Value>,
    pub columns: Vec<Value>,
    pub metrics: Vec<Value>,
}

fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().ok_or("ONTOLOGY_PACK_SHAPE")
}

fn string(value: &Value) -> Result<&str> {
    value.as_str().ok_or("ONTOLOGY_PACK_SHAPE")
}

fn document(files: &BTreeMap<String, String>, path: &str) -> Result<Value> {
    serde_json::from_str(files.get(path).ok_or("ONTOLOGY_PACK_MISSING_PAGE")?)
        .map_err(|_| "ONTOLOGY_PACK_JSON")
}

fn pages(files: &BTreeMap<String, String>, start: &str) -> Result<Vec<Value>> {
    let mut path = start.to_owned();
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    loop {
        if !path.starts_with("summaries/") || !seen.insert(path.clone()) {
            return Err("ONTOLOGY_PACK_PAGE_CYCLE");
        }
        let page = document(files, &path)?;
        if page["protocol"] != "oo.knowledge.summary/v1" {
            return Err("ONTOLOGY_PACK_PROTOCOL");
        }
        rows.extend(array(&page["items"])?.iter().cloned());
        match page.get("next") {
            Some(Value::Null) => return Ok(rows),
            Some(Value::String(next)) => path = next.clone(),
            _ => return Err("ONTOLOGY_PACK_SHAPE"),
        }
    }
}

impl Model {
    pub fn parse(text: &str, role: Option<&str>) -> Result<Self> {
        if text.len() as u64 > MAX_BYTES {
            return Err("ONTOLOGY_PACK_BUDGET");
        }
        let artifact: Value = serde_json::from_str(text).map_err(|_| "ONTOLOGY_PACK_JSON")?;
        if artifact["protocol"] != "oo.builder.role-pack/v1" {
            return Err("ONTOLOGY_PACK_PROTOCOL");
        }
        let mut files = BTreeMap::new();
        let mut records = BTreeMap::new();
        let mut total = 0;
        for file in array(&artifact["files"])? {
            let path = string(&file["path"])?;
            let body = string(&file["text"])?;
            if path.starts_with('/')
                || path.contains('\\')
                || path
                    .split('/')
                    .any(|c| c.is_empty() || c == "." || c == "..")
                || files.insert(path.to_owned(), body.to_owned()).is_some()
            {
                return Err("ONTOLOGY_PACK_PATH");
            }
            let digest = payload_sha256(body.as_bytes());
            if file["bytes"].as_u64() != Some(body.len() as u64) || file["sha256"] != digest {
                return Err("ONTOLOGY_PACK_DIGEST");
            }
            records.insert(
                path,
                json!({"path":path,"bytes":body.len(),"sha256":digest}),
            );
            total += body.len() as u64;
        }
        let records: Vec<_> = records.into_values().collect();
        if artifact["entry_count"].as_u64() != Some(files.len() as u64)
            || artifact["total_bytes"].as_u64() != Some(total)
            || artifact["pack_digest"]
                != payload_sha256(
                    serde_json::to_string(&records)
                        .map_err(|_| "ONTOLOGY_PACK_JSON")?
                        .as_bytes(),
                )
            || artifact["manifest_sha256"]
                != payload_sha256(
                    files
                        .get("manifest.json")
                        .ok_or("ONTOLOGY_PACK_MISSING_PAGE")?
                        .as_bytes(),
                )
        {
            return Err("ONTOLOGY_PACK_DIGEST");
        }
        let manifest = document(&files, "manifest.json")?;
        if manifest["protocol"] != "oo.builder.role-pack-manifest/v1" {
            return Err("ONTOLOGY_PACK_PROTOCOL");
        }
        let actual_role = string(&manifest["role"])?;
        if role.is_some_and(|r| r != actual_role) {
            return Err("ONTOLOGY_ROLE_MISMATCH");
        }
        let graph = document(&files, "relationships/graph.json")?;
        if graph["protocol"] != "oo.knowledge.relationship-graph/v1" {
            return Err("ONTOLOGY_PACK_PROTOCOL");
        }
        let mut entities = Vec::new();
        let mut relationships = Vec::new();
        if let Some(index) = graph.get("pages") {
            if graph.get("entities").is_some() || graph.get("relationships").is_some() {
                return Err("ONTOLOGY_PACK_SHAPE");
            }
            let mut seen = BTreeSet::new();
            for path in array(index)? {
                let path = string(path)?;
                if !path.starts_with("relationships/graph-") || !seen.insert(path) {
                    return Err("ONTOLOGY_PACK_PATH");
                }
                let page = document(&files, path)?;
                match (page.get("entities"), page.get("relationships")) {
                    (Some(rows), None) => entities.extend(array(rows)?.iter().cloned()),
                    (None, Some(rows)) => relationships.extend(array(rows)?.iter().cloned()),
                    _ => return Err("ONTOLOGY_PACK_SHAPE"),
                }
            }
        } else {
            entities.extend(array(&graph["entities"])?.iter().cloned());
            relationships.extend(array(&graph["relationships"])?.iter().cloned());
        }
        let tables = pages(&files, "summaries/tables.json")?;
        let columns = pages(&files, "summaries/columns.json")?;
        let mut table_map = BTreeMap::new();
        for table in &tables {
            if table_map
                .insert(string(&table["dataset"])?, table)
                .is_some()
            {
                return Err("ONTOLOGY_PACK_DUPLICATE");
            }
            string(&table["community"])?;
        }
        let mut ids = BTreeSet::new();
        for entity in &mut entities {
            if !ids.insert(string(&entity["id"])?.to_owned()) {
                return Err("ONTOLOGY_PACK_DUPLICATE");
            }
            let main = string(&entity["main_table"])?;
            let table = table_map.get(main).ok_or("ONTOLOGY_PACK_REFERENCE")?;
            let domain = main
                .split_once('.')
                .ok_or("ONTOLOGY_PACK_SHAPE")?
                .0
                .to_owned();
            for key in ["related_tables", "members"] {
                for name in array(&entity[key])? {
                    if !table_map.contains_key(string(name)?) {
                        return Err("ONTOLOGY_PACK_REFERENCE");
                    }
                }
            }
            string(&entity["canonical_name"])?;
            entity["domain"] = json!(domain);
            entity["community"] = table["community"].clone();
        }
        let mut column_names = BTreeSet::new();
        for column in &columns {
            let dataset = string(&column["dataset"])?;
            let name = string(&column["name"])?;
            if !table_map.contains_key(dataset) || !column_names.insert((dataset, name)) {
                return Err("ONTOLOGY_PACK_REFERENCE");
            }
        }
        for relation in &relationships {
            string(&relation["id"])?;
            for end in ["from", "to"] {
                let dataset = string(&relation[end]["dataset"])?;
                if !table_map.contains_key(dataset) {
                    return Err("ONTOLOGY_PACK_REFERENCE");
                }
                for column in array(&relation[end]["columns"])? {
                    if !column_names.contains(&(dataset, string(column)?)) {
                        return Err("ONTOLOGY_PACK_REFERENCE");
                    }
                }
            }
        }
        // Keep metric definitions as text, never render their Markdown/SQL as HTML.
        let metrics = files
            .iter()
            .filter(|(path, _)| {
                path.starts_with("metrics/")
                    && path.ends_with(".md")
                    && !path.starts_with("metrics/_index/")
            })
            .map(|(path, text)| {
                let mut body = text.clone();
                let prefix = format!("{}/", path.trim_end_matches(".md"));
                for (part_path, part) in files.range(prefix.clone()..) {
                    if !part_path.starts_with(&prefix) {
                        break;
                    }
                    if let Ok(value) = serde_json::from_str::<Value>(part)
                        && value["protocol"] == "oo.knowledge.metric-fragment/v1"
                        && let Some(fragment) = value["fragment"].as_str()
                    {
                        body.push_str(fragment);
                    }
                }
                json!({"path":path,"text":body})
            })
            .collect();
        Ok(Self {
            role: actual_role.into(),
            entities,
            relationships,
            tables,
            columns,
            metrics,
        })
    }

    pub fn html(&self) -> Result<String> {
        let data = json!({"role":self.role,"entities":self.entities,
            "relationships":self.relationships,"tables":self.tables,
            "columns":self.columns,"metrics":self.metrics});
        // Escape script terminators and URL punctuation, even inside inert JSON.
        let encoded = serde_json::to_string(&data)
            .map_err(|_| "ONTOLOGY_PACK_JSON")?
            .replace('&', "\\u0026")
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('/', "\\u002f");
        Ok(include_str!("ontology.html").replace("/*PACK_DATA*/null", &encoded))
    }

    pub fn mermaid(&self) -> Result<String> {
        let mut groups: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
        let mut names = BTreeMap::new();
        for (index, table) in self.tables.iter().enumerate() {
            names.insert(string(&table["dataset"])?, format!("T{index}"));
            groups
                .entry(string(&table["community"])?)
                .or_default()
                .push(table);
        }
        let mut output = String::from(
            "# Role-visible ontology\n\nNo publication or runtime verification implied.\n",
        );
        for (community, tables) in groups.values().enumerate() {
            for (page, chunk) in tables.chunks(50).enumerate() {
                output.push_str(&format!(
                    "\n## Community {} / page {}\n\n```mermaid\nerDiagram\n",
                    community + 1,
                    page + 1
                ));
                for table in chunk {
                    let dataset = string(&table["dataset"])?;
                    output.push_str(&format!("    {}[\"{}\"]\n", names[dataset], label(dataset)));
                }
                output.push_str("```\n");
            }
        }
        // Every edge is retained once, including cross-community edges. Separate
        // bounded edge pages avoid a high-degree node defeating the table limit.
        for (page, edges) in self.relationships.chunks(50).enumerate() {
            output.push_str(&format!(
                "\n## Relations / page {}\n\n```mermaid\nerDiagram\n",
                page + 1
            ));
            let mut endpoints = BTreeSet::new();
            for edge in edges {
                endpoints.insert(string(&edge["from"]["dataset"])?);
                endpoints.insert(string(&edge["to"]["dataset"])?);
            }
            for dataset in endpoints {
                output.push_str(&format!("    {}[\"{}\"]\n", names[dataset], label(dataset)));
            }
            for edge in edges {
                let cardinality = match edge["cardinality"].as_str() {
                    Some("one_to_one") => "||--||",
                    Some("one_to_many") => "||--o{",
                    Some("many_to_one") => "}o--||",
                    Some("many_to_many") => "}o--o{",
                    _ => return Err("ONTOLOGY_PACK_CARDINALITY"),
                };
                output.push_str(&format!(
                    "    {} {} {} : \"{}\"\n",
                    names[string(&edge["from"]["dataset"])?],
                    cardinality,
                    names[string(&edge["to"]["dataset"])?],
                    label(string(&edge["id"])?)
                ));
            }
            output.push_str("```\n");
        }
        Ok(output)
    }
}

fn label(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
