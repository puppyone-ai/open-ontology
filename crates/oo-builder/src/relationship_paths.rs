//! Relationship evidence and query paths ("how to join") for role Packs.
//!
//! The effective document may carry `relationship_evidence`
//! (`oo.builder.relationship-evidence/v1`), produced by the confirmed
//! adjudication. It extends the OOS relationships by name; it never adds a
//! relationship. The Pack receives two files, both closed over the role's
//! visibility: `relationships/evidence.json` and
//! `relationships/query-paths.json`, which binds the evidence file by digest.
//! SQL examples aggregate the child at its own grain before the join, so a
//! parent-grain value is never summed over child rows.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    Model, RolePackError, Selected, add_file, column_visible, end_visible, object_budget,
    visible_prose,
};
use crate::canonical::{canonical_json, sha256_hex};

pub const RELATIONSHIP_EVIDENCE_PROTOCOL: &str = "oo.builder.relationship-evidence/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RelationshipEvidence {
    protocol: String,
    adjudication_summary_sha256: String,
    relationships: Vec<EvidenceRecord>,
    withheld: Vec<Withheld>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceRecord {
    name: String,
    kind: String,
    role: Option<String>,
    row_containment: f64,
    distinct_containment: f64,
    rows_checked: u64,
    matched_rows: u64,
    unmatched_rows: u64,
    empty_rows: u64,
    max_fanout: u64,
    normalized_collisions: u64,
    join_hint: String,
    from_grain: Vec<String>,
    to_grain: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct End {
    dataset: String,
    columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Withheld {
    from: End,
    to: End,
    kind: String,
    role: Option<String>,
    rule: String,
    scope_columns: Vec<String>,
    max_fanout: Option<u64>,
    parent_duplicate_keys: Option<u64>,
    normalized_collisions: Option<u64>,
}

fn invalid(path: impl Into<String>) -> RolePackError {
    RolePackError::new(
        "relationship_evidence.invalid",
        path,
        "relationship evidence does not match the effective relationships",
    )
}

fn has_columns(
    datasets: &BTreeMap<String, oo_core::oos::DatasetFile>,
    dataset: &str,
    columns: &[String],
) -> bool {
    datasets.get(dataset).is_some_and(|d| {
        columns
            .iter()
            .all(|c| d.columns.iter().any(|col| col.name.as_str() == c))
    })
}

/// Bind the extension to the effective relationships before any rendering.
pub(super) fn validate(
    evidence: &RelationshipEvidence,
    relationships: &[oo_core::oos::Relationship],
    datasets: &BTreeMap<String, oo_core::oos::DatasetFile>,
) -> Result<(), RolePackError> {
    let path = "/relationship_evidence";
    if evidence.protocol != RELATIONSHIP_EVIDENCE_PROTOCOL
        || evidence.adjudication_summary_sha256.len() != 64
        || !evidence
            .adjudication_summary_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid(path));
    }
    let by_name: BTreeMap<_, _> = relationships.iter().map(|r| (r.name.as_str(), r)).collect();
    let mut seen = BTreeSet::new();
    for record in &evidence.relationships {
        let Some(relationship) = by_name.get(record.name.as_str()) else {
            return Err(invalid(format!("{path}/relationships/{}", record.name)));
        };
        let hint = serde_json::to_value(relationship.join_hint).unwrap_or(Value::Null);
        let ratio = |r: f64| r.is_finite() && (0.0..=1.0).contains(&r);
        if !seen.insert(record.name.as_str())
            || !matches!(
                record.kind.as_str(),
                "declared_fk" | "inferred_id" | "snapshot_name_mapping"
            )
            || !ratio(record.row_containment)
            || !ratio(record.distinct_containment)
            || record.max_fanout > 1
            || record.normalized_collisions > 0
            || record.matched_rows + record.unmatched_rows != record.rows_checked
            || hint != json!(record.join_hint)
            || (record.join_hint == "inner"
                && (record.row_containment < 1.0 || record.distinct_containment < 1.0))
            || !has_columns(
                datasets,
                relationship.from.dataset.as_str(),
                &record.from_grain,
            )
            || !has_columns(datasets, relationship.to.dataset.as_str(), &record.to_grain)
            || record.role.as_deref().is_some_and(|r| r.is_empty())
        {
            return Err(invalid(format!("{path}/relationships/{}", record.name)));
        }
    }
    for (index, item) in evidence.withheld.iter().enumerate() {
        if !matches!(
            item.rule.as_str(),
            "relation.scope_required"
                | "relation.parent_not_unique"
                | "relation.normalized_collision"
        ) || !has_columns(datasets, &item.from.dataset, &item.from.columns)
            || !has_columns(datasets, &item.to.dataset, &item.to.columns)
            || !has_columns(datasets, &item.to.dataset, &item.scope_columns)
            || item.from.columns.is_empty()
            || item.from.columns.len() != item.to.columns.len()
        {
            return Err(invalid(format!("{path}/withheld/{index}")));
        }
    }
    Ok(())
}

fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn table(dataset: &str) -> String {
    dataset.split('.').map(ident).collect::<Vec<_>>().join(".")
}

fn data_type(model: &Model, dataset: &str, column: &str) -> Option<String> {
    model.datasets.get(dataset).and_then(|d| {
        d.columns
            .iter()
            .find(|c| c.name.as_str() == column)
            .and_then(|c| {
                c.data_type
                    .as_ref()
                    .map(|t| t.as_str().to_ascii_lowercase())
            })
    })
}

/// Grain-correct example: count child rows per parent key. The child is
/// aggregated at its own grain first; summing the result reproduces the child
/// total exactly, including rows whose key has no parent.
fn example_sql(model: &Model, relationship: &oo_core::oos::Relationship) -> String {
    let from = relationship.from.dataset.as_str();
    let to = relationship.to.dataset.as_str();
    let pairs: Vec<_> = relationship
        .from
        .columns
        .iter()
        .zip(&relationship.to.columns)
        .enumerate()
        .map(|(i, (c, k))| (i, c.as_str(), k.as_str()))
        .collect();
    let keys = pairs
        .iter()
        .map(|(i, c, _)| format!("c.{} AS join_key_{}", ident(c), i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let group = pairs
        .iter()
        .map(|(i, _, _)| format!("join_key_{}", i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let on = pairs
        .iter()
        .map(|(i, c, k)| {
            // Compare as text only when the stored types differ, as adjudication measured.
            let cast = if data_type(model, from, c) == data_type(model, to, k) {
                ""
            } else {
                "::text"
            };
            format!("p.{}{cast} = s.join_key_{}{cast}", ident(k), i + 1)
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let parent_keys = pairs
        .iter()
        .map(|(i, _, k)| format!("p.{} AS parent_key_{}", ident(k), i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {parent_keys}, s.child_rows FROM (SELECT {keys}, COUNT(*) AS child_rows FROM {} AS c GROUP BY {group}) AS s LEFT JOIN {} AS p ON {on} ORDER BY {group_order}",
        table(from),
        table(to),
        group_order = pairs
            .iter()
            .map(|(i, _, _)| format!("s.join_key_{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn grain(model: &Model, dataset: &str, columns: &[String]) -> Value {
    if !columns.is_empty() && end_visible(model, dataset, columns) {
        json!(columns)
    } else {
        json!("unknown")
    }
}

fn withheld_reason(item: &Withheld) -> String {
    match item.rule.as_str() {
        "relation.scope_required" => format!(
            "Parent key repeats; it is unique only per {}. Filter the parent to one value of that column before joining.",
            item.scope_columns.join(", ")
        ),
        "relation.normalized_collision" => {
            "Parent keys differ only by case or surrounding spaces; the match is ambiguous. Do not join on this key.".into()
        }
        _ => "Parent key repeats; joining would multiply child rows. Do not join on this key.".into(),
    }
}

fn visible_role(model: &Model, role: Option<&String>) -> Value {
    role.filter(|r| visible_prose(model, r))
        .map_or(Value::Null, |r| json!(r))
}

/// Adds the evidence asset and query paths. Absent evidence emits nothing,
/// so legacy effective documents compile to the same Pack bytes.
pub(super) fn render(
    files: &mut BTreeMap<String, String>,
    model: &Model,
    selected: &Selected,
) -> Result<(), RolePackError> {
    let Some(evidence) = &model.relationship_evidence else {
        return Ok(());
    };
    let records: BTreeMap<_, _> = evidence
        .relationships
        .iter()
        .map(|r| (r.name.as_str(), r))
        .collect();
    let mut visible_records = Vec::new();
    let mut paths = Vec::new();
    for relationship in &selected.relationships {
        let Some(record) = records.get(relationship.name.as_str()) else {
            continue;
        };
        let from = relationship.from.dataset.as_str();
        let to = relationship.to.dataset.as_str();
        let role = visible_role(model, record.role.as_ref());
        visible_records.push(json!({
            "name":relationship.name.as_str(),"kind":record.kind,"role":role,
            "row_containment":record.row_containment,
            "distinct_containment":record.distinct_containment,
            "rows_checked":record.rows_checked,"matched_rows":record.matched_rows,
            "unmatched_rows":record.unmatched_rows,"empty_rows":record.empty_rows,
            "max_fanout":record.max_fanout,"normalized_collisions":record.normalized_collisions,
            "join_hint":record.join_hint,
            "from_grain":grain(model, from, &record.from_grain),
            "to_grain":grain(model, to, &record.to_grain)}));
        let inner = record.join_hint == "inner";
        paths.push(json!({
            "relationship":relationship.name.as_str(),
            "start":from,"start_grain":grain(model, from, &record.from_grain),
            "target":to,"target_grain":grain(model, to, &record.to_grain),
            "role":role,"kind":record.kind,
            "join":{"type":if inner {"INNER JOIN"} else {"LEFT JOIN"},
                "keys":relationship.from.columns.iter().zip(&relationship.to.columns)
                    .map(|(c, k)| json!({"from":format!("{from}.{}", c.as_str()),
                        "to":format!("{to}.{}", k.as_str())}))
                    .collect::<Vec<_>>()},
            "coverage":{"rows":record.row_containment,"distinct":record.distinct_containment,
                "unmatched_rows":record.unmatched_rows,"empty_rows":record.empty_rows},
            "unmatched_handling":if inner {
                "Every non-blank child key has exactly one parent row; INNER JOIN keeps every row.".to_owned()
            } else {
                format!("{} child rows have no parent and {} have a blank key; LEFT JOIN keeps them as unknown parent.",
                    record.unmatched_rows, record.empty_rows)
            },
            "aggregation":"Aggregate child measures at the child grain (GROUP BY the join key) before joining. Never SUM a parent-grain value after joining it to child rows.",
            "sql":example_sql(model, relationship)}));
    }
    let mut withheld = Vec::new();
    let mut withheld_paths = Vec::new();
    for item in &evidence.withheld {
        // Hidden datasets, columns or scope columns never leave the compiler.
        if !end_visible(model, &item.from.dataset, &item.from.columns)
            || !end_visible(model, &item.to.dataset, &item.to.columns)
            || !item
                .scope_columns
                .iter()
                .all(|c| column_visible(model, &item.to.dataset, c))
        {
            continue;
        }
        let role = visible_role(model, item.role.as_ref());
        let entry = json!({"from":item.from,"to":item.to,"kind":item.kind,"role":role,
            "rule":item.rule,"scope_columns":item.scope_columns});
        withheld.push(entry.clone());
        let mut path = entry;
        path["reason"] = json!(withheld_reason(item));
        withheld_paths.push(path);
    }
    if visible_records.is_empty() && withheld.is_empty() {
        return Ok(());
    }
    let evidence_text = canonical_json(&json!({
        "protocol":"oo.knowledge.relationship-evidence/v1",
        "adjudication_summary_sha256":evidence.adjudication_summary_sha256,
        "relationships":visible_records,"withheld":withheld}));
    let budget = model.limits.max_file_bytes as usize;
    if evidence_text.len() > budget {
        return Err(object_budget("/relationship_evidence"));
    }
    let digest = sha256_hex(evidence_text.as_bytes());
    add_file(files, "relationships/evidence.json", evidence_text)?;
    let header = |pages: Value| {
        json!({"protocol":"oo.knowledge.query-paths/v1",
            "evidence":"relationships/evidence.json","evidence_sha256":digest,
            "dialect":"postgresql","pages":pages})
    };
    let whole = canonical_json(&json!({"protocol":"oo.knowledge.query-paths/v1",
        "evidence":"relationships/evidence.json","evidence_sha256":digest,
        "dialect":"postgresql","paths":paths,"withheld":withheld_paths}));
    if whole.len() <= budget {
        return add_file(files, "relationships/query-paths.json", whole);
    }
    let mut pages = Vec::new();
    for (category, rows) in [("paths", paths), ("withheld", withheld_paths)] {
        let mut page: Vec<Value> = Vec::new();
        let mut flush = |page: &mut Vec<Value>, files: &mut BTreeMap<String, String>| {
            let path = format!(
                "relationships/query-paths-{category}-{:04}.json",
                pages.len()
            );
            let text = canonical_json(&json!({ category: page }));
            pages.push(path.clone());
            page.clear();
            add_file(files, &path, text)
        };
        for row in rows {
            page.push(row);
            if canonical_json(&json!({ category: &page })).len() > budget {
                let last = page.pop().expect("row just pushed");
                if page.is_empty() {
                    return Err(object_budget("/relationships/query-paths"));
                }
                flush(&mut page, files)?;
                page.push(last);
            }
        }
        if !page.is_empty() {
            flush(&mut page, files)?;
        }
    }
    add_file(
        files,
        "relationships/query-paths.json",
        canonical_json(&header(json!(pages))),
    )
}
