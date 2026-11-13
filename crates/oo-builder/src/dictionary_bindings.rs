//! Reviewed lookup semantics, separate from statistical code containment.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Model, RolePackError, add_file, column_visible, visible_prose};
use crate::canonical::canonical_json;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    id: String,
    concept: String,
    from: Option<Fact>,
    dictionary: Dictionary,
    interval: String,
    provenance: Provenance,
    evidence: Evidence,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fact {
    dataset: String,
    code: String,
    at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Dictionary {
    dataset: String,
    discriminator: String,
    value: String,
    code: String,
    label: String,
    valid_from: String,
    valid_to: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    origin: String,
    source_sha256: String,
    reviewer: String,
    reviewed_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    rows_checked: u64,
    matched_rows: u64,
    ambiguous_rows: u64,
    invalid_intervals: u64,
    overlap_pairs: u64,
    gap_pairs: u64,
}

impl Binding {
    fn columns(&self) -> Vec<(&str, &str)> {
        let d = &self.dictionary;
        let mut columns: Vec<(&str, &str)> = vec![
            (&d.dataset, &d.discriminator),
            (&d.dataset, &d.code),
            (&d.dataset, &d.label),
            (&d.dataset, &d.valid_from),
            (&d.dataset, &d.valid_to),
        ];
        if let Some(fact) = &self.from {
            columns.extend([(&*fact.dataset, &*fact.code), (&*fact.dataset, &*fact.at)]);
        }
        columns
    }
}

pub(super) fn validate(
    bindings: &[Binding],
    datasets: &BTreeMap<String, oo_core::oos::DatasetFile>,
) -> Result<(), RolePackError> {
    let mut ids = BTreeSet::new();
    let text = |s: &str| !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control);
    for b in bindings {
        let p = &b.provenance;
        if bindings.len() > 128
            || !ids.insert(&b.id)
            || !text(&b.id)
            || !text(&b.concept)
            || !text(&b.dictionary.value)
            || !text(&p.reviewer)
            || p.reviewer.eq_ignore_ascii_case("model")
            || p.origin != "operator_reviewed"
            || p.source_sha256.len() != 64
            || !p
                .source_sha256
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            || serde_json::from_value::<oo_core::oos::CivilDate>(json!(p.reviewed_at)).is_err()
            || !matches!(
                b.interval.as_str(),
                "closed_open_null_end_unbounded" | "closed_closed_date_null_end_unbounded"
            )
            || b.evidence.matched_rows > b.evidence.rows_checked
            || b.evidence.ambiguous_rows > b.evidence.rows_checked
            || b.evidence.invalid_intervals != 0
            || b.evidence.overlap_pairs != 0
            || b.evidence.ambiguous_rows != 0
            || (b.interval == "closed_closed_date_null_end_unbounded"
                && [&b.dictionary.valid_from, &b.dictionary.valid_to]
                    .iter()
                    .any(|name| {
                        datasets
                            .get(&b.dictionary.dataset)
                            .and_then(|d| {
                                d.columns.iter().find(|c| c.name.as_str() == name.as_str())
                            })
                            .and_then(|c| c.data_type.as_ref())
                            .is_none_or(|t| t.as_str() != "date")
                    }))
            || !b.columns().iter().all(|(table, col)| {
                datasets
                    .get(*table)
                    .is_some_and(|d| d.columns.iter().any(|c| c.name.as_str() == *col))
            })
        {
            return Err(RolePackError::new(
                "dictionary.binding",
                "/dictionary_bindings",
                "a reviewed dictionary binding or its replica evidence is invalid",
            ));
        }
    }
    Ok(())
}

pub(super) fn render(
    files: &mut BTreeMap<String, String>,
    model: &Model,
) -> Result<(), RolePackError> {
    let bindings: Vec<_> = model
        .dictionary_bindings
        .iter()
        .filter(|b| {
            b.columns()
                .iter()
                .all(|(table, col)| column_visible(model, table, col))
                && [
                    &b.id,
                    &b.concept,
                    &b.dictionary.value,
                    &b.provenance.reviewer,
                ]
                .iter()
                .all(|s| visible_prose(model, s))
        })
        .collect();
    if bindings.is_empty() {
        return Ok(());
    }
    let mut text = String::from(
        "# Reviewed dictionary bindings\n\nThese operator-reviewed definitions are not inferred from containment. Use the discriminator and requested effective time together; codes may repeat in other dictionaries and historical labels may differ. Follow each binding's interval: closed_open excludes the end; closed_closed_date includes both dates. NULL end is unbounded. A null from means lookup-only: reference coverage was not measured. Application role codes are data, never authorization grants. Missing matches remain unknown, not a fabricated label. Gaps and coverage below are measured on the retained replica, not a freshness guarantee.\n\n",
    );
    for b in &bindings {
        // JSON keeps source literals escaped, rather than interpolating them as SQL.
        let mut rendered = json!(b);
        if b.columns().iter().any(|(name, _)| {
            model
                .grants
                .get(*name)
                .is_none_or(|g| g.rls || !g.global_stats)
        }) {
            rendered.as_object_mut().unwrap().remove("evidence");
        }
        text.push_str("```json\n");
        text.push_str(&canonical_json(&rendered));
        text.push_str("\n```\n\n");
    }
    add_file(files, "dictionaries.md", text)
}
