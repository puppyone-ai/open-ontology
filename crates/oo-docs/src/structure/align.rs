//! Field alignment across languages and spellings (D93). The built-in dictionary (`observe::CONCEPTS`) folds the common field names of
//! business records; a label it does not know is aligned by the model once, and the answer is cached (`structure_alignment`), so the
//! same label never costs a second call. The aligned key replaces the label's own key before clustering, so the zh and en files of one
//! kind of record form one class; every original label stays as an alias of the property.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use sqlx::{PgConnection, Row};

use super::observe::{CONCEPTS, FileObs, slug_of};
use crate::extract::llm::egress::LlmPort;
use crate::extract::llm::prompt::ChatRequest;
use crate::store::StoreError;

/// Labels sent in one call, and files a label must recur in before it is asked about.
const MAX_LABELS: usize = 60;
const MIN_FILES: usize = 2;

fn concept(key: &str) -> bool {
    CONCEPTS.iter().any(|(s, _)| *s == key)
}

fn valid_target(t: &str) -> bool {
    (2..=40).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// The cached alignment: field key to the key it is read as.
pub async fn load(conn: &mut PgConnection) -> Result<BTreeMap<String, String>, sqlx::Error> {
    let rows = sqlx::query("SELECT key, target FROM _oo_docs.structure_alignment")
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("key"), r.get("target")))
        .collect())
}

/// Read every field of every file under its aligned key.
pub fn apply(files: &mut [(String, String, FileObs)], map: &BTreeMap<String, String>) {
    if map.is_empty() {
        return;
    }
    for (_, _, o) in files {
        for f in &mut o.fields {
            if let Some(t) = map.get(&f.key) {
                f.key = t.clone();
            }
        }
    }
}

/// Read the fields of one file under their aligned keys.
pub fn apply_obs(o: &mut FileObs, map: &BTreeMap<String, String>) {
    for f in &mut o.fields {
        if let Some(t) = map.get(&f.key) {
            f.key = t.clone();
        }
    }
}

/// Keys the dictionary does not know, in at least two files, with their labels and a few values: what the model is asked about.
pub fn unknown(
    files: &[(String, String, FileObs)],
    known: &BTreeMap<String, String>,
) -> Vec<(String, String, Vec<String>)> {
    let mut by: BTreeMap<String, (String, BTreeSet<usize>, Vec<String>)> = BTreeMap::new();
    for (i, (_, _, o)) in files.iter().enumerate() {
        for f in &o.fields {
            if concept(&f.key)
                || known.contains_key(&f.key)
                || f.source == super::FieldSource::Directory
            {
                continue;
            }
            let e = by
                .entry(f.key.clone())
                .or_insert_with(|| (f.label.clone(), BTreeSet::new(), Vec::new()));
            e.1.insert(i);
            if e.2.len() < 3
                && let Some(v) = f.values.first()
            {
                e.2.push(v.chars().take(30).collect());
            }
        }
    }
    let mut out: Vec<(String, String, Vec<String>)> = by
        .into_iter()
        .filter(|(_, (_, n, _))| n.len() >= MIN_FILES)
        .map(|(k, (l, _, v))| (k, l, v))
        .collect();
    out.truncate(MAX_LABELS);
    out
}

/// Ask the model to align `items` (key, label, example values) to a concept of the dictionary or to a shared short English name.
pub async fn ask(
    port: &dyn LlmPort,
    items: &[(String, String, Vec<String>)],
) -> Option<BTreeMap<String, String>> {
    if items.is_empty() {
        return Some(BTreeMap::new());
    }
    let concepts: Vec<&str> = CONCEPTS.iter().map(|(s, _)| *s).collect();
    let mut user = String::new();
    for (_, label, ex) in items {
        user.push_str(&format!("{label} | {}\n", ex.join(" ; ")));
    }
    let req = ChatRequest {
        model: port.model().to_owned(),
        system: format!(
            "You align the field names of business records across languages and spellings. For every line `label | example values` answer a concept: one of the known concepts when one fits ({}), otherwise a short English snake_case name (2 to 30 letters or underscores). Labels that mean the same thing, in any language, must get the same concept. Answer one JSON object {{\"label\": \"concept\"}} and nothing else.",
            concepts.join(", ")
        ),
        user,
    };
    let reply = port.complete(&req).await.ok()?;
    let text = reply.content.trim();
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let v: Value = serde_json::from_str(&text[start..=end]).ok()?;
    let obj = v.as_object()?;
    let mut out = BTreeMap::new();
    for (key, label, _) in items {
        let t = obj
            .get(label)
            .and_then(Value::as_str)
            .map(|s| s.trim().to_lowercase().replace([' ', '-'], "_"));
        // An answer that is not a valid name leaves the label as it was (its own slug), and is not asked again.
        out.insert(
            key.clone(),
            t.filter(|t| valid_target(t))
                .unwrap_or_else(|| slug_of(key)),
        );
    }
    Some(out)
}

pub async fn store(
    conn: &mut PgConnection,
    map: &BTreeMap<String, String>,
) -> Result<(), StoreError> {
    for (k, t) in map {
        sqlx::query("INSERT INTO _oo_docs.structure_alignment (key, target, source) VALUES ($1, $2, 'model') ON CONFLICT (key) DO NOTHING")
            .bind(k)
            .bind(t)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
