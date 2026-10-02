//! Field-level overlay of generated and curated ontology JSON.
//!
//! `effective = merge(generated, curated)`. Curated wins field by field.
//! Object lists merge by a stable key (`name`, `id`, `code`, `term`, `metric`,
//! `dataset`, `entity`, `item`), never by array index. A curated item with
//! `status: rejected` leaves the effective list and is kept in `removed`.
//! An unkeyed list is replaced only when curated contains that list. An omitted
//! key keeps the generated value. An empty keyed list means the curator wrote
//! no item overrides.
//!
//! Fact fingerprints are supplied by the caller. This crate does not hash
//! upstream column types or code sets, and a successful merge is not AT-OOS-01.
//! A pin is stale only when it covers a path the curator actually overrode and
//! the fingerprints differ. A real override with no covering pin is listed in
//! `missing_pins` and still keeps the curated value. An unrelated pin is not
//! stale. Coverage is the pin itself or a descendant at a `/` boundary.
//! JSON Pointer tokens escape `~` as `~0` and `/` as `~1`.

use serde_json::{Map, Value};

use crate::diagnostic::{CONTRACT_OOS, Diagnostic, DiagnosticList};
use crate::text::HexDigest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactPin {
    pub path: String,
    pub generated_fingerprint: HexDigest,
    pub recorded_fingerprint: HexDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleOverride {
    pub path: String,
    pub generated_fingerprint: HexDigest,
    pub recorded_fingerprint: HexDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedItem {
    pub path: String,
    pub evidence: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub effective: Value,
    pub removed: Vec<RemovedItem>,
    pub stale: Vec<StaleOverride>,
    /// Override paths that no supplied pin covers. The curated value is still
    /// in `effective`. A later builder treats each entry as review-required.
    pub missing_pins: Vec<String>,
}

const LIST_KEYS: &[&str] = &[
    "name", "id", "code", "term", "metric", "dataset", "entity", "item",
];

pub fn merge_effective(
    generated: &Value,
    curated: &Value,
    facts: &[FactPin],
) -> Result<MergeOutcome, DiagnosticList> {
    let mut removed = Vec::new();
    let mut overrides = Vec::new();
    let effective = merge_node(generated, curated, "", &mut removed, &mut overrides)?;
    let mut covered = vec![false; overrides.len()];
    let mut stale = Vec::new();
    for pin in facts {
        let hits: Vec<usize> = overrides
            .iter()
            .enumerate()
            .filter(|(_, path)| pin_covers(&pin.path, path))
            .map(|(index, _)| index)
            .collect();
        if hits.is_empty() {
            continue;
        }
        for index in hits {
            covered[index] = true;
        }
        if pin.generated_fingerprint != pin.recorded_fingerprint {
            stale.push(StaleOverride {
                path: pin.path.clone(),
                generated_fingerprint: pin.generated_fingerprint.clone(),
                recorded_fingerprint: pin.recorded_fingerprint.clone(),
            });
        }
    }
    let missing_pins = overrides
        .iter()
        .enumerate()
        .filter(|(index, _)| !covered[*index])
        .map(|(_, path)| path.clone())
        .collect();
    Ok(MergeOutcome {
        effective,
        removed,
        stale,
        missing_pins,
    })
}

fn pin_covers(pin: &str, path: &str) -> bool {
    path == pin || path.starts_with(&format!("{pin}/"))
}

fn merge_node(
    generated: &Value,
    curated: &Value,
    path: &str,
    removed: &mut Vec<RemovedItem>,
    overrides: &mut Vec<String>,
) -> Result<Value, DiagnosticList> {
    match (generated, curated) {
        (Value::Object(generated_map), Value::Object(curated_map)) => {
            merge_object(generated_map, curated_map, path, removed, overrides)
        }
        (Value::Array(generated_items), Value::Array(curated_items)) => {
            merge_array(generated_items, curated_items, path, removed, overrides)
        }
        _ => {
            if generated != curated {
                record_override(curated, path, overrides);
            }
            Ok(curated.clone())
        }
    }
}

fn merge_object(
    generated: &Map<String, Value>,
    curated: &Map<String, Value>,
    path: &str,
    removed: &mut Vec<RemovedItem>,
    overrides: &mut Vec<String>,
) -> Result<Value, DiagnosticList> {
    let mut out = generated.clone();
    for (key, curated_value) in curated {
        let child = join(path, key);
        if let Some(generated_value) = generated.get(key) {
            out.insert(
                key.clone(),
                merge_node(generated_value, curated_value, &child, removed, overrides)?,
            );
        } else {
            record_override(curated_value, &child, overrides);
            out.insert(key.clone(), curated_value.clone());
        }
    }
    Ok(Value::Object(out))
}

fn merge_array(
    generated: &[Value],
    curated: &[Value],
    path: &str,
    removed: &mut Vec<RemovedItem>,
    overrides: &mut Vec<String>,
) -> Result<Value, DiagnosticList> {
    let Some(key) = list_key(generated, curated) else {
        if generated != curated {
            record_override(&Value::Array(curated.to_vec()), path, overrides);
        }
        return Ok(Value::Array(curated.to_vec()));
    };
    if curated.is_empty() {
        index_list(generated, key, path)?;
        return Ok(Value::Array(generated.to_vec()));
    }
    let generated_items = index_list(generated, key, path)?;
    let curated_items = index_list(curated, key, path)?;
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for (id, generated_item) in &generated_items {
        seen.push(id.clone());
        let item_path = join(path, id);
        if let Some(curated_item) = curated_items
            .iter()
            .find(|(curated_id, _)| curated_id == id)
        {
            if is_rejected(&curated_item.1) {
                overrides.push(item_path.clone());
                removed.push(RemovedItem {
                    path: item_path,
                    evidence: curated_item.1.clone(),
                });
                continue;
            }
            out.push(merge_node(
                generated_item,
                &curated_item.1,
                &item_path,
                removed,
                overrides,
            )?);
        } else {
            out.push(generated_item.clone());
        }
    }
    for (id, curated_item) in &curated_items {
        if seen.iter().any(|seen_id| seen_id == id) {
            continue;
        }
        let item_path = join(path, id);
        if is_rejected(curated_item) {
            overrides.push(item_path.clone());
            removed.push(RemovedItem {
                path: item_path,
                evidence: curated_item.clone(),
            });
            continue;
        }
        record_override(curated_item, &item_path, overrides);
        out.push(curated_item.clone());
    }
    Ok(Value::Array(out))
}

fn list_key(generated: &[Value], curated: &[Value]) -> Option<&'static str> {
    if curated.is_empty() {
        return shared_key(generated);
    }
    if generated.is_empty() {
        return shared_key(curated);
    }
    let generated_key = shared_key(generated)?;
    let curated_key = shared_key(curated)?;
    (generated_key == curated_key).then_some(generated_key)
}

fn shared_key(items: &[Value]) -> Option<&'static str> {
    if items.is_empty() || items.iter().any(|item| !item.is_object()) {
        return None;
    }
    LIST_KEYS.iter().copied().find(|key| {
        items
            .iter()
            .all(|item| item.get(*key).is_some_and(|value| value.as_str().is_some()))
    })
}

fn index_list(
    items: &[Value],
    key: &str,
    path: &str,
) -> Result<Vec<(String, Value)>, DiagnosticList> {
    let mut indexed = Vec::new();
    for item in items {
        let Some(id) = item.get(key).and_then(Value::as_str) else {
            return Err(DiagnosticList::one(Diagnostic::error(
                CONTRACT_OOS,
                "merge.key",
                path,
                format!("list item is missing string key `{key}`"),
            )));
        };
        if indexed.iter().any(|(seen, _)| seen == id) {
            return Err(DiagnosticList::one(Diagnostic::error(
                CONTRACT_OOS,
                "merge.duplicate_key",
                join(path, id),
                format!("duplicate list key `{id}`"),
            )));
        }
        indexed.push((id.to_owned(), item.clone()));
    }
    Ok(indexed)
}

fn is_rejected(value: &Value) -> bool {
    value.get("status").and_then(Value::as_str) == Some("rejected")
}

fn record_override(value: &Value, path: &str, overrides: &mut Vec<String>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                record_override(child, &join(path, key), overrides);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            if list_key(items, items).is_some() {
                overrides.push(path.to_owned());
            } else {
                for (index, child) in items.iter().enumerate() {
                    record_override(child, &join(path, &index.to_string()), overrides);
                }
            }
        }
        _ => overrides.push(path.to_owned()),
    }
}

fn join(path: &str, key: &str) -> String {
    format!("{path}/{}", escape_pointer(key))
}

fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
