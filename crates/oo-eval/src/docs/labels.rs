//! Label records (`oo.eval.docs-labels/v1`, documented in testing/super-ontology/golden/labels/README.md).

use super::json::{self, Json};
use std::path::{Path, PathBuf};

pub const CORE_TYPES: [&str; 8] = [
    "Person",
    "Organization",
    "Project",
    "Decision",
    "Document",
    "Topic",
    "Event",
    "Term",
];
pub const PREDICATES: [&str; 8] = [
    "owns",
    "member_of",
    "decides",
    "supersedes",
    "rejects",
    "mentions",
    "about",
    "tried_and_failed",
];

#[derive(Clone, Debug, Default)]
pub struct Anchor {
    pub file: String,
    pub frag: String,
    pub bbox: Option<[f64; 4]>,
}

#[derive(Clone, Debug, Default)]
pub struct Entity {
    pub id: String,
    pub typ: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub anchors: Vec<Anchor>,
    pub confidence: f64,
    pub redteam_only: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Fact {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: Option<String>,
    pub value: Option<String>,
    pub anchors: Vec<Anchor>,
    pub confidence: f64,
}

#[derive(Clone, Debug, Default)]
pub struct LabelSet {
    pub entities: Vec<Entity>,
    pub facts: Vec<Fact>,
    /// (old fact id, new fact id)
    pub supersedes: Vec<(String, String)>,
    /// fact id sets
    pub conflicts: Vec<Vec<String>>,
}

fn s(v: &Json, k: &str) -> Option<String> {
    v.get(k).and_then(Json::as_str).map(str::to_string)
}

fn anchors(v: &Json) -> Vec<Anchor> {
    let mut out = Vec::new();
    if let Some(a) = v.get("anchors").and_then(Json::as_arr) {
        for x in a {
            let bbox = x.get("bbox").and_then(Json::as_arr).and_then(|b| {
                let n: Vec<f64> = b.iter().filter_map(Json::as_f64).collect();
                (n.len() == 4).then(|| [n[0], n[1], n[2], n[3]])
            });
            out.push(Anchor {
                file: s(x, "file").unwrap_or_default(),
                frag: s(x, "frag").unwrap_or_default(),
                bbox,
            });
        }
    }
    out
}

fn value_text(v: &Json) -> Option<String> {
    match v.get("value")? {
        Json::Str(t) => Some(t.clone()),
        Json::Num(n) => Some(n.to_string()),
        Json::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Parse JSON Lines text into `set`. Unknown `kind`s are ignored (forward compatible).
pub fn parse_lines(text: &str, origin: &str, set: &mut LabelSet) -> Result<(), String> {
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let at = || format!("{origin}:{}", n + 1);
        let v = json::parse(line).map_err(|e| format!("{}: {e}", at()))?;
        let need = |k: &str| s(&v, k).ok_or_else(|| format!("{}: missing string field {k}", at()));
        let conf = v.get("confidence").and_then(Json::as_f64).unwrap_or(1.0);
        match v.get("kind").and_then(Json::as_str) {
            Some("entity") => set.entities.push(Entity {
                id: need("id")?,
                typ: need("type")?,
                name: need("name")?,
                aliases: v
                    .get("aliases")
                    .and_then(Json::as_arr)
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                anchors: anchors(&v),
                confidence: conf,
                redteam_only: v
                    .get("redteam_only")
                    .and_then(Json::as_bool)
                    .unwrap_or(false),
            }),
            Some("fact") => set.facts.push(Fact {
                id: need("id")?,
                subject: need("subject")?,
                predicate: need("predicate")?,
                object: s(&v, "object"),
                value: value_text(&v),
                anchors: anchors(&v),
                confidence: conf,
            }),
            Some("supersede") => set.supersedes.push((need("old")?, need("new")?)),
            Some("conflict") => set.conflicts.push(
                v.get("facts")
                    .and_then(Json::as_arr)
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            _ => {}
        }
    }
    Ok(())
}

pub fn jsonl_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut v: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    v.sort();
    Ok(v)
}

/// Load every `*.jsonl` of a directory in file-name order.
pub fn load_dir(dir: &Path) -> Result<LabelSet, String> {
    let mut set = LabelSet::default();
    for f in jsonl_files(dir)? {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        parse_lines(&text, &name, &mut set)?;
    }
    Ok(set)
}
