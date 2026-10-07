//! Hand-truth scorer. It does not call the producer.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::canonical::canonical_json;
use crate::scan::parse_json;
use crate::{Fail, CARDS_COMMIT, CATALOG_COMMIT, REVIEW_PROTOCOL, SCORE_PROTOCOL, TRUTH_PROTOCOL};

const REVIEW_KEYS: &[&str] = &[
    "authenticated",
    "auto_accept",
    "certification",
    "columns",
    "comment_disclosure",
    "conflicts",
    "dictionaries",
    "effect",
    "entity_merge",
    "enums",
    "expert_cards",
    "grants",
    "identity",
    "knowledge_graph",
    "metric_certification",
    "parent_auth",
    "pii_leak_count",
    "protocol",
    "publication",
    "relations",
    "sample_policy",
    "schema_version",
    "source_pins",
];

pub fn score(truth: &[u8], actual: &[u8]) -> Result<Value, Fail> {
    let truth = parse_json(truth)?;
    let actual = parse_json(actual)?;
    let truth_map = truth.as_object().ok_or(Fail::new("schema"))?;
    if truth_map.get("protocol").and_then(Value::as_str) != Some(TRUTH_PROTOCOL) {
        return Err(Fail::new("schema"));
    }
    for key in [
        "columns",
        "relations",
        "enums",
        "dictionaries",
        "conflicts",
        "personal_ids",
        "sensitive_ids",
        "unknown_ids",
        "forbidden_substrings",
    ] {
        if !truth_map.contains_key(key) {
            return Err(Fail::new("schema"));
        }
    }
    let mut mismatches = Vec::new();
    check_literals(actual.as_object(), &mut mismatches);
    let actual_map = actual.as_object();
    let columns = compare_columns(truth_map, actual_map, &mut mismatches)?;
    compare_relations(truth_map, actual_map, &mut mismatches)?;
    compare_enums(truth_map, actual_map, &mut mismatches)?;
    compare_dictionaries(truth_map, actual_map, &mut mismatches)?;
    compare_conflicts(truth_map, actual_map, &mut mismatches)?;
    let personal = string_list(truth_map, "personal_ids")?;
    let sensitive = string_list(truth_map, "sensitive_ids")?;
    let unknown = string_list(truth_map, "unknown_ids")?;
    let forbidden = string_list(truth_map, "forbidden_substrings")?;
    check_unknown(&unknown, &columns, &mut mismatches);
    let leak = leak_count(&personal, &sensitive, &columns);
    if let Some(map) = actual_map
        && map.get("pii_leak_count").and_then(Value::as_i64) != Some(leak)
    {
        mismatches.push("leak.field".to_owned());
    }
    let text = canonical_json(&actual);
    if forbidden.iter().any(|item| text.contains(item)) {
        mismatches.push("forbidden".to_owned());
    }
    if let Some(map) = actual_map
        && let Some(Value::Array(cards)) = map.get("expert_cards")
    {
        for card in cards {
            check_expert(card, &mut mismatches);
        }
    }
    mismatches.sort();
    mismatches.dedup();
    let matched = mismatches.is_empty() && leak == 0;
    let shown: Vec<Value> = mismatches.into_iter().take(32).map(Value::String).collect();
    Ok(serde_json::json!({
        "protocol": SCORE_PROTOCOL,
        "matched": matched,
        "pii_leak_count": leak,
        "mismatches": shown
    }))
}

fn check_literals(actual: Option<&Map<String, Value>>, mismatches: &mut Vec<String>) {
    let Some(map) = actual else {
        mismatches.push("top.type".to_owned());
        return;
    };
    let mut keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
    for key in REVIEW_KEYS {
        if !keys.remove(key) {
            mismatches.push(format!("top.missing:{key}"));
        }
    }
    for key in keys {
        mismatches.push(format!("top.extra:{key}"));
    }
    expect_str(map, "protocol", REVIEW_PROTOCOL, mismatches);
    expect_str(map, "schema_version", REVIEW_PROTOCOL, mismatches);
    expect_str(map, "effect", "review_only", mismatches);
    expect_str(map, "parent_auth", "required", mismatches);
    expect_str(map, "publication", "unavailable", mismatches);
    expect_str(map, "grants", "not_authorized", mismatches);
    expect_str(map, "certification", "none", mismatches);
    expect_str(map, "entity_merge", "not_performed", mismatches);
    expect_str(map, "metric_certification", "not_performed", mismatches);
    expect_str(map, "knowledge_graph", "not_emitted", mismatches);
    expect_str(map, "sample_policy", "none", mismatches);
    expect_str(map, "comment_disclosure", "withheld", mismatches);
    if map.get("authenticated") != Some(&Value::Bool(false)) {
        mismatches.push("top.authenticated".to_owned());
    }
    if !matches!(map.get("auto_accept").and_then(Value::as_str), Some("rule_high_confidence" | "none")) {
        mismatches.push("top.auto_accept".to_owned());
    }
    let pins = map.get("source_pins").and_then(Value::as_object);
    if pins.and_then(|p| p.get("catalog_commit")).and_then(Value::as_str) != Some(CATALOG_COMMIT)
        || pins.and_then(|p| p.get("cards_commit")).and_then(Value::as_str) != Some(CARDS_COMMIT)
    {
        mismatches.push("top.pins".to_owned());
    }
    if !map.get("identity").is_some_and(Value::is_object) {
        mismatches.push("top.identity".to_owned());
    }
}

fn expect_str(map: &Map<String, Value>, key: &str, want: &str, mismatches: &mut Vec<String>) {
    if map.get(key).and_then(Value::as_str) != Some(want) {
        mismatches.push(format!("top.{key}"));
    }
}

fn compare_columns<'a>(
    truth: &'a Map<String, Value>,
    actual: Option<&'a Map<String, Value>>,
    mismatches: &mut Vec<String>,
) -> Result<BTreeMap<&'a str, &'a Map<String, Value>>, Fail> {
    let truth_rows = truth_rows(truth, "columns")?;
    let actual_rows = actual_rows(actual, "columns", mismatches);
    let truth_idx = index_rows(&truth_rows, "column", true, mismatches)?;
    let actual_idx = index_rows(&actual_rows, "column", false, mismatches)?;
    for (id, row) in &truth_idx {
        let Some(got) = actual_idx.get(id) else {
            mismatches.push(format!("column.missing:{id}"));
            continue;
        };
        field_str(row, got, "semantic_type", &format!("column.semantic:{id}"), mismatches);
        field_str(row, got, "pii", &format!("column.pii:{id}"), mismatches);
        field_str(row, got, "state", &format!("column.state:{id}"), mismatches);
    }
    for id in actual_idx.keys() {
        if !truth_idx.contains_key(id) {
            mismatches.push(format!("column.extra:{id}"));
        }
    }
    Ok(actual_idx)
}

fn compare_relations(
    truth: &Map<String, Value>,
    actual: Option<&Map<String, Value>>,
    mismatches: &mut Vec<String>,
) -> Result<(), Fail> {
    let truth_rows = truth_rows(truth, "relations")?;
    let actual_rows = actual_rows(actual, "relations", mismatches);
    let truth_idx = index_rows(&truth_rows, "relation", true, mismatches)?;
    let actual_idx = index_rows(&actual_rows, "relation", false, mismatches)?;
    for (id, row) in &truth_idx {
        let Some(got) = actual_idx.get(id) else {
            mismatches.push(format!("relation.missing:{id}"));
            continue;
        };
        field_str(row, got, "provenance", &format!("relation.provenance:{id}"), mismatches);
        field_str(row, got, "cardinality", &format!("relation.cardinality:{id}"), mismatches);
        field_bool(row, got, "conflict", &format!("relation.conflict:{id}"), mismatches);
    }
    for id in actual_idx.keys() {
        if !truth_idx.contains_key(id) {
            mismatches.push(format!("relation.extra:{id}"));
        }
    }
    Ok(())
}

fn compare_enums(
    truth: &Map<String, Value>,
    actual: Option<&Map<String, Value>>,
    mismatches: &mut Vec<String>,
) -> Result<(), Fail> {
    let truth_rows = truth_rows(truth, "enums")?;
    let actual_rows = actual_rows(actual, "enums", mismatches);
    let truth_idx = index_rows(&truth_rows, "enum", true, mismatches)?;
    let actual_idx = index_rows(&actual_rows, "enum", false, mismatches)?;
    for (id, row) in &truth_idx {
        let Some(got) = actual_idx.get(id) else {
            mismatches.push(format!("enum.missing:{id}"));
            continue;
        };
        field_str(row, got, "status", &format!("enum.status:{id}"), mismatches);
        field_bool(row, got, "approved", &format!("enum.approved:{id}"), mismatches);
    }
    for id in actual_idx.keys() {
        if !truth_idx.contains_key(id) {
            mismatches.push(format!("enum.extra:{id}"));
        }
    }
    Ok(())
}

fn compare_dictionaries(
    truth: &Map<String, Value>,
    actual: Option<&Map<String, Value>>,
    mismatches: &mut Vec<String>,
) -> Result<(), Fail> {
    let truth_rows = truth_rows(truth, "dictionaries")?;
    let actual_rows = actual_rows(actual, "dictionaries", mismatches);
    let truth_idx = index_rows(&truth_rows, "dictionary", true, mismatches)?;
    let actual_idx = index_rows(&actual_rows, "dictionary", false, mismatches)?;
    for (id, row) in &truth_idx {
        let Some(got) = actual_idx.get(id) else {
            mismatches.push(format!("dictionary.missing:{id}"));
            continue;
        };
        field_str(row, got, "status", &format!("dictionary.status:{id}"), mismatches);
        field_bool(row, got, "approved", &format!("dictionary.approved:{id}"), mismatches);
        field_str(row, got, "match", &format!("dictionary.match:{id}"), mismatches);
    }
    for id in actual_idx.keys() {
        if !truth_idx.contains_key(id) {
            mismatches.push(format!("dictionary.extra:{id}"));
        }
    }
    Ok(())
}

fn compare_conflicts(
    truth: &Map<String, Value>,
    actual: Option<&Map<String, Value>>,
    mismatches: &mut Vec<String>,
) -> Result<(), Fail> {
    let truth_rows = truth_rows(truth, "conflicts")?;
    let actual_rows = actual_rows(actual, "conflicts", mismatches);
    let truth_idx = index_rows(&truth_rows, "conflict", true, mismatches)?;
    let actual_idx = index_rows(&actual_rows, "conflict", false, mismatches)?;
    for id in truth_idx.keys() {
        let Some(got) = actual_idx.get(id) else {
            mismatches.push(format!("conflict.missing:{id}"));
            continue;
        };
        if !got.get("selected").is_some_and(Value::is_null) {
            mismatches.push(format!("conflict.selected:{id}"));
        }
    }
    for id in actual_idx.keys() {
        if !truth_idx.contains_key(id) {
            mismatches.push(format!("conflict.extra:{id}"));
        }
    }
    Ok(())
}

fn check_unknown(ids: &[&str], columns: &BTreeMap<&str, &Map<String, Value>>, mismatches: &mut Vec<String>) {
    for id in ids {
        let Some(row) = columns.get(id) else {
            mismatches.push(format!("unknown:{id}"));
            continue;
        };
        if row.get("semantic_type").and_then(Value::as_str) != Some("unknown")
            || row.get("pii").and_then(Value::as_str) != Some("unknown")
            || row.get("state").and_then(Value::as_str) != Some("pending")
        {
            mismatches.push(format!("unknown:{id}"));
        }
    }
}

fn leak_count(personal: &[&str], sensitive: &[&str], columns: &BTreeMap<&str, &Map<String, Value>>) -> i64 {
    let mut leak = 0i64;
    for id in personal.iter().chain(sensitive.iter()) {
        match columns.get(id) {
            Some(row) => {
                let pii = row.get("pii").and_then(Value::as_str).unwrap_or("");
                let state = row.get("state").and_then(Value::as_str).unwrap_or("");
                if pii == "none" || state == "auto_accepted" {
                    leak += 1;
                }
            }
            None => leak += 1,
        }
    }
    for row in columns.values() {
        let pii = row.get("pii").and_then(Value::as_str).unwrap_or("");
        let state = row.get("state").and_then(Value::as_str).unwrap_or("");
        if (pii == "personal" || pii == "sensitive") && state == "auto_accepted" {
            leak += 1;
        }
    }
    leak
}

fn check_expert(card: &Value, mismatches: &mut Vec<String>) {
    let Some(map) = card.as_object() else {
        mismatches.push("expert:".to_owned());
        return;
    };
    let id = map.get("stable_id").and_then(Value::as_str).unwrap_or("");
    let question_ok = map.get("question").and_then(Value::as_str).is_some_and(|q| !q.is_empty());
    let evidence_ok = map.get("evidence").is_some_and(Value::is_object);
    let uncertainty_ok = matches!(map.get("uncertainty").and_then(Value::as_str), Some("0" | "0.5" | "1"));
    let anchors_ok = map.get("anchors").and_then(Value::as_array).is_some_and(|items| {
        !items.is_empty() && items.iter().all(|item| item.as_str().is_some())
    });
    let provenance_ok = map.get("provenance").and_then(Value::as_str) == Some("rule");
    if id.is_empty() || !question_ok || !evidence_ok || !uncertainty_ok || !anchors_ok || !provenance_ok {
        mismatches.push(format!("expert:{id}"));
    }
}

fn truth_rows<'a>(truth: &'a Map<String, Value>, key: &str) -> Result<Vec<&'a Map<String, Value>>, Fail> {
    let items = truth.get(key).and_then(Value::as_array).ok_or(Fail::new("schema"))?;
    let mut rows = Vec::new();
    for item in items {
        rows.push(item.as_object().ok_or(Fail::new("schema"))?);
    }
    Ok(rows)
}

fn actual_rows<'a>(
    actual: Option<&'a Map<String, Value>>,
    key: &str,
    mismatches: &mut Vec<String>,
) -> Vec<&'a Map<String, Value>> {
    let Some(map) = actual else {
        return Vec::new();
    };
    match map.get(key) {
        Some(Value::Array(items)) => {
            let mut rows = Vec::new();
            for (index, item) in items.iter().enumerate() {
                if let Some(row) = item.as_object() {
                    rows.push(row);
                } else {
                    mismatches.push(format!("{key}.type:{index}"));
                }
            }
            rows
        }
        Some(_) => {
            mismatches.push(format!("top.type:{key}"));
            Vec::new()
        }
        None => Vec::new(),
    }
}

fn index_rows<'a>(
    rows: &[&'a Map<String, Value>],
    kind: &str,
    required: bool,
    mismatches: &mut Vec<String>,
) -> Result<BTreeMap<&'a str, &'a Map<String, Value>>, Fail> {
    let mut out = BTreeMap::new();
    for row in rows {
        let Some(id) = row.get("stable_id").and_then(Value::as_str) else {
            if required {
                return Err(Fail::new("schema"));
            }
            mismatches.push(format!("{kind}.id"));
            continue;
        };
        if out.insert(id, *row).is_some() {
            if required {
                return Err(Fail::new("schema"));
            }
            mismatches.push(format!("{kind}.duplicate:{id}"));
        }
    }
    Ok(out)
}

fn field_str(truth: &Map<String, Value>, actual: &Map<String, Value>, key: &str, label: &str, mismatches: &mut Vec<String>) {
    if truth.get(key) != actual.get(key) {
        mismatches.push(label.to_owned());
    }
}

fn field_bool(truth: &Map<String, Value>, actual: &Map<String, Value>, key: &str, label: &str, mismatches: &mut Vec<String>) {
    if truth.get(key) != actual.get(key) {
        mismatches.push(label.to_owned());
    }
}

fn string_list<'a>(truth: &'a Map<String, Value>, key: &str) -> Result<Vec<&'a str>, Fail> {
    let items = truth.get(key).and_then(Value::as_array).ok_or(Fail::new("schema"))?;
    let mut out = Vec::new();
    for item in items {
        out.push(item.as_str().ok_or(Fail::new("schema"))?);
    }
    Ok(out)
}
