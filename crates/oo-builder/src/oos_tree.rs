//! Offline OOS source-tree compiler. No authentication, grants or publication.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{RolePack, RolePackCeilings, compile_role_pack_with_ceilings};
use oo_core::api::payload_sha256;
use oo_core::oos::{self, FactPin, merge_effective};
use oo_core::yaml::parse_yaml_value;
use serde::Deserialize;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, String>;
const CATEGORIES: [&str; 7] = [
    "datasets",
    "relationships",
    "enums",
    "glossary",
    "entities",
    "metrics",
    "examples",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    project: String,
    ontology_version: String,
    catalog_version: String,
    policy: Value,
    knowledge: Value,
    generated_sha256: String,
    curated_sha256: String,
    ontology_sha256: String,
}

fn read(path: &Path, max: u64) -> Result<String> {
    let meta = fs::symlink_metadata(path).map_err(|_| format!("io.read: {}", path.display()))?;
    if !meta.is_file() || meta.len() > max {
        return Err(format!("io.file_or_budget: {}", path.display()));
    }
    fs::read_to_string(path).map_err(|_| format!("io.utf8: {}", path.display()))
}

fn walk(
    root: &Path,
    depth: usize,
    files: &mut BTreeMap<PathBuf, String>,
    total: &mut u64,
) -> Result<()> {
    if depth > 8 || !fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return Err("tree.directory_or_depth".into());
    }
    for entry in fs::read_dir(root).map_err(|_| "tree.read")? {
        let path = entry.map_err(|_| "tree.entry")?.path();
        let kind = fs::symlink_metadata(&path).map_err(|_| "tree.metadata")?;
        // Finder metadata is binary and not an OOS document. Never follow a symlink.
        if path.file_name().is_some_and(|n| n == ".DS_Store")
            && kind.is_file()
            && kind.len() <= oo_core::limits::DOCUMENT_MAX_BYTES as u64
        {
            continue;
        }
        if kind.is_dir() {
            walk(&path, depth + 1, files, total)?;
        } else {
            if files.len() >= 16384 {
                return Err("tree.file_budget".into());
            }
            *total = total.checked_add(kind.len()).ok_or("tree.total_budget")?;
            if *total > 64 * 1024 * 1024 {
                return Err("tree.total_budget".into());
            }
            files.insert(
                path.clone(),
                read(
                    &path,
                    if depth == 0 && path.file_name().is_some_and(|n| n == "explorer.html") {
                        8 * 1024 * 1024
                    } else {
                        oo_core::limits::DOCUMENT_MAX_BYTES as u64
                    },
                )?,
            );
        }
    }
    Ok(())
}

fn validate(category: &str, text: &str) -> Result<Value> {
    macro_rules! check {
        ($parser:ident) => {
            oos::$parser(text).map(|_| ()).map_err(|e| {
                e.items
                    .first()
                    .map(|d| d.code.clone())
                    .unwrap_or_else(|| "oos.invalid".into())
            })?
        };
    }
    match category {
        "ontology" => check!(parse_ontology_yaml),
        "datasets" => check!(parse_dataset_yaml),
        "relationships" => check!(parse_relationships_yaml),
        "enums" => check!(parse_enum_yaml),
        "glossary" => check!(parse_glossary_yaml),
        "entities" => check!(parse_entity_yaml),
        "metrics" => check!(parse_metrics_yaml),
        "examples" => check!(parse_examples_yaml),
        "decisions" => check!(parse_decisions_yaml),
        _ => return Err("oos.unsupported_category".into()),
    }
    // Preserve omitted fields and number lexemes; typed reserialization adds defaults.
    parse_yaml_value(text, "oos/v1").map_err(|_| "oos.yaml".into())
}

fn identity(category: &str) -> &str {
    match category {
        "datasets" => "dataset",
        "enums" => "enum",
        "glossary" => "term",
        "entities" => "entity",
        "metrics" => "metric",
        "examples" => "id",
        _ => "name",
    }
}

fn assemble(root: &Path) -> Result<(Value, Value, Value, usize)> {
    let mut files = BTreeMap::new();
    walk(root, 0, &mut files, &mut 0)?;
    let mut generated = json!({});
    for category in CATEGORIES {
        generated[category] = json!([]);
    }
    let mut curated = json!({});
    let mut ontology = None;
    let mut decisions = false;
    let mut seen = BTreeSet::new();
    let mut yaml_count = 0;
    for (path, text) in files {
        let relative = path.strip_prefix(root).map_err(|_| "tree.path")?;
        let name = relative.to_str().ok_or("tree.utf8")?;
        // Exact, non-semantic companions only; never ignore arbitrary extensions.
        if matches!(
            name,
            "README.md"
                | "curated/README.md"
                | "reviews/README.md"
                | "explorer.html"
                | "preview.puppyoneapp"
                | "ontology-preview.puppyoneapp"
                | ".agent"
                | "AGENTS.md"
                | ".cli/oo"
        ) {
            continue;
        }
        yaml_count += 1;
        if name == "ontology.yaml" {
            ontology = Some(validate("ontology", &text)?);
            continue;
        }
        if name == "reviews/decisions.yaml" {
            let doc = validate("decisions", &text)?;
            if doc["decisions"].as_array().is_none_or(|v| !v.is_empty()) {
                return Err("review.decisions_require_ledger_application".into());
            }
            decisions = true;
            continue;
        }
        let parts: Vec<_> = name.split('/').collect();
        if parts.len() < 3
            || !matches!(parts[0], "generated" | "curated")
            || !CATEGORIES.contains(&parts[1])
            || !name.ends_with(".yaml")
        {
            return Err(format!("tree.unsupported_path: {name}"));
        }
        let category = parts[1];
        let doc = validate(category, &text).map_err(|e| format!("{name}: {e}"))?;
        if category == "datasets" {
            let dataset = doc["dataset"].as_str().ok_or("dataset.identity")?;
            let (schema, table) = dataset.split_once('.').ok_or("dataset.identity")?;
            if name != format!("{}/datasets/{schema}/{table}.yaml", parts[0]) {
                return Err("dataset.path_identity".into());
            }
        }
        let wrapper = match category {
            "relationships" | "metrics" | "examples" => Some(category),
            "glossary" => Some("terms"),
            _ => None,
        };
        let items = match wrapper {
            Some(key) => doc[key].as_array().ok_or("oos.items")?.clone(),
            None => vec![doc],
        };
        let layer = if parts[0] == "generated" {
            &mut generated
        } else {
            &mut curated
        };
        if layer.get(category).is_none() {
            layer[category] = json!([]);
        }
        for item in items {
            let id = item[identity(category)].as_str().ok_or("oos.identity")?;
            if !seen.insert((parts[0].to_owned(), category.to_owned(), id.to_owned())) {
                return Err("oos.duplicate_identity".into());
            }
            layer[category]
                .as_array_mut()
                .ok_or("oos.items")?
                .push(item);
        }
    }
    if !decisions {
        return Err("review.decisions_missing".into());
    }
    Ok((
        ontology.ok_or("ontology.missing")?,
        generated,
        curated,
        yaml_count,
    ))
}

pub struct OosTreeCompilation {
    pub effective_oos: Value,
    pub pack: RolePack,
    pub yaml_files: usize,
}

/// Compile schema-checked YAML layers with an explicit review binding.
/// This local input binding does not authenticate a reviewer or publish a pack.
pub fn compile_oos_tree(
    root: &Path,
    binding_json: &str,
    visibility: &str,
    ceilings: &RolePackCeilings,
) -> Result<OosTreeCompilation> {
    let binding: Binding = serde_json::from_str(binding_json).map_err(|_| "binding.schema")?;
    let (ontology, generated, curated, count) = assemble(root)?;
    let generated_hash = payload_sha256(generated.to_string().as_bytes());
    let curated_hash = payload_sha256(curated.to_string().as_bytes());
    if generated_hash != binding.generated_sha256
        || curated_hash != binding.curated_sha256
        || payload_sha256(ontology.to_string().as_bytes()) != binding.ontology_sha256
    {
        return Err("review.stale_or_unbound_overlay".into());
    }
    let pin = FactPin {
        path: String::new(),
        generated_fingerprint: serde_json::from_value(json!(generated_hash))
            .map_err(|_| "review.hash")?,
        recorded_fingerprint: serde_json::from_value(json!(binding.generated_sha256))
            .map_err(|_| "review.hash")?,
    };
    let merged = merge_effective(&generated, &curated, &[pin]).map_err(|_| "overlay.merge")?;
    if !merged.stale.is_empty() || !merged.missing_pins.is_empty() {
        return Err("review.stale_override".into());
    }
    let mut effective = merged.effective;
    effective["protocol"] = json!("oo.builder.effective-oos/v1");
    effective["ontology"] = ontology;
    effective["project"] = json!(binding.project);
    effective["ontology_version"] = json!(binding.ontology_version);
    effective["catalog_version"] = json!(binding.catalog_version);
    effective["policy"] = binding.policy;
    effective["knowledge"] = binding.knowledge;
    let pack = compile_role_pack_with_ceilings(&effective.to_string(), visibility, ceilings)
        .map_err(|e| e.code)?;
    Ok(OosTreeCompilation {
        effective_oos: effective,
        pack,
        yaml_files: count,
    })
}
