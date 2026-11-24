//! DOC-G1 (schema) and DOC-G2 (evidence), 10 §6.1. Pure. DOC-G3 (bitemporal),
//! DOC-G4 (overlay) and DOC-G5 (merge integrity) are checked in `run.rs`
//! because they read the store; they share the [`Tally`].
//!
//! Reading of the spec where it is short:
//! * G1 checks per fact: the predicate is one of the eight core relations or
//!   `attr.*`; a relation has an object and no value, an attribute a value and
//!   no object; subject and object types fit the domain / range table below;
//!   the value is `{"text"|"date"|"enum": non-empty string}` of at most 2,000
//!   characters. Entity types are the nine of D15 by construction. Per alias:
//!   1 to 256 characters, no control characters.
//! * G2 checks per fact and alias: at least one non-withdrawn evidence; every
//!   evidence names a block that exists in the IR of its blob, with the
//!   anchor stored in the evidence equal to the block's anchor, and the anchor
//!   passes the structural checks of `oo.doc-ir/v1`.
//!
//! An item that fails a gate stays out of the version; the other items are
//! published (10 §4.9).

use std::collections::BTreeMap;

use oo_core::docir::{Anchor, DocIr};
use serde_json::Value;

use crate::extract::model::{EntityType, Predicate};

/// Gate ids as stored in `gate_report.gate_id`.
pub const DOC_G1: &str = "DOC-G1";
pub const DOC_G2: &str = "DOC-G2";
pub const DOC_G3: &str = "DOC-G3";
pub const DOC_G4: &str = "DOC-G4";
pub const DOC_G5: &str = "DOC-G5";
/// Reprocess diff (10 6.3, D23, B-S10-02): checked when a re-extraction of a file finishes.
pub const DOC_G10: &str = "DOC-G10";

/// DOC-G10: the share of the file's facts a reprocess may change before a person has to look (percent).
pub const G10_MAX_CHANGE_PERCENT: usize = 30;

/// New against old extraction of one file version, by `fact_key`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReprocessDiff {
    /// Facts the old extraction had.
    pub prior: usize,
    /// Keys only the new extraction has, sorted.
    pub added: Vec<String>,
    /// Keys only the old extraction had, sorted.
    pub removed: Vec<String>,
    /// Removed keys that a live Share's pack used, sorted.
    pub share_used_removed: Vec<String>,
}

impl ReprocessDiff {
    /// Diff of two key sets (any order); `share_used` are the keys a live Share uses.
    pub fn of(prior: &[String], now: &[String], share_used: &[String]) -> Self {
        use std::collections::BTreeSet;
        let (p, n): (BTreeSet<&String>, BTreeSet<&String>) =
            (prior.iter().collect(), now.iter().collect());
        let removed: Vec<String> = p.difference(&n).map(|k| (*k).clone()).collect();
        let added: Vec<String> = n.difference(&p).map(|k| (*k).clone()).collect();
        let used: BTreeSet<&String> = share_used.iter().collect();
        let share_used_removed = removed
            .iter()
            .filter(|k| used.contains(k))
            .cloned()
            .collect();
        Self {
            prior: p.len(),
            added,
            removed,
            share_used_removed,
        }
    }

    /// Nothing differs.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }

    /// DOC-G10: `Ok` when the new extraction may replace the old without a person; `Err` names why not:
    /// a removed fact is used by a live Share, or more than 30% of the file's facts changed.
    pub fn g10(&self) -> Result<(), &'static str> {
        if !self.share_used_removed.is_empty() {
            return Err("share_used_fact_removed");
        }
        if (self.added.len() + self.removed.len()) * 100 > self.prior * G10_MAX_CHANGE_PERCENT {
            return Err("diff_over_30_percent");
        }
        Ok(())
    }
}

/// Longest `attr.*` value text.
pub const MAX_VALUE_CHARS: usize = 2000;

/// A fact as the publisher reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct FactRow {
    pub id: String,
    pub fact_key: String,
    pub predicate: String,
    pub subject_type: String,
    pub object_type: Option<String>,
    pub value: Option<Value>,
}

/// An alias as the publisher reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AliasRow {
    pub id: String,
    pub entity_key: String,
    pub norm_alias: String,
    pub alias: String,
    pub entity_type: String,
}

/// One stored evidence row.
#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceRow {
    pub block_id: String,
    pub b3: String,
    pub anchor: Value,
}

use EntityType as T;

fn ty(text: &str) -> Option<EntityType> {
    EntityType::parse(text)
}

/// Domain (subject types) and range (object types) per relation.
fn domain_range(p: &Predicate) -> Option<(&'static [T], &'static [T])> {
    Some(match p {
        Predicate::Owns => (
            &[T::Person, T::Organization],
            &[
                T::Organization,
                T::Project,
                T::Document,
                T::Topic,
                T::Event,
                T::Term,
            ],
        ),
        Predicate::MemberOf => (
            &[T::Person, T::Organization, T::Project],
            &[T::Organization, T::Project, T::Event, T::Topic],
        ),
        Predicate::Decides => (&[T::Person, T::Organization], &[T::Decision]),
        Predicate::Supersedes => (
            &[T::Decision, T::Document, T::Project, T::Term, T::Topic],
            &[T::Decision, T::Document, T::Project, T::Term, T::Topic],
        ),
        Predicate::Rejects => (
            &[T::Decision, T::Person, T::Organization, T::Project],
            &[
                T::Term,
                T::Topic,
                T::Project,
                T::Decision,
                T::Document,
                T::Event,
                T::Thing,
            ],
        ),
        Predicate::TriedAndFailed => (
            &[
                T::Project,
                T::Person,
                T::Organization,
                T::Decision,
                T::Topic,
            ],
            &[T::Term, T::Topic, T::Decision, T::Project, T::Thing],
        ),
        Predicate::Mentions => (&[T::Document], &T::ALL),
        Predicate::About => (
            &[
                T::Document,
                T::Decision,
                T::Person,
                T::Project,
                T::Event,
                T::Topic,
            ],
            &[
                T::Project,
                T::Topic,
                T::Organization,
                T::Term,
                T::Event,
                T::Decision,
                T::Person,
            ],
        ),
        // D94: a reference cites a reference; a person wrote a reference. Labels are not visible to the gate.
        Predicate::Cites => (&[T::Thing], &[T::Thing]),
        Predicate::Authored => (&[T::Person], &[T::Thing]),
        Predicate::HasParticipant => (&[T::Meeting], &[T::Person]),
        Predicate::RecordedIn => (&[T::Meeting], &[T::Recording]),
        Predicate::Attr(_) => return None,
    })
}

/// DOC-G1 for one fact. `Err` carries the reason code.
pub fn g1_fact(f: &FactRow) -> Result<(), &'static str> {
    let Some(pred) = Predicate::parse(&f.predicate) else {
        return Err("schema_predicate");
    };
    let Some(subject) = ty(&f.subject_type) else {
        return Err("schema_type");
    };
    match (&pred, &f.object_type, &f.value) {
        (Predicate::Attr(_), None, Some(v)) => {
            let ok = v.as_object().is_some_and(|o| {
                o.len() == 1
                    && o.iter().all(|(k, v)| {
                        matches!(k.as_str(), "text" | "date" | "enum")
                            && v.as_str().is_some_and(|s| {
                                !s.trim().is_empty() && s.chars().count() <= MAX_VALUE_CHARS
                            })
                    })
            });
            if ok { Ok(()) } else { Err("schema_value") }
        }
        (Predicate::Attr(_), _, _) => Err("schema_shape"),
        (_, Some(object), None) => {
            let Some(object) = ty(object) else {
                return Err("schema_type");
            };
            let Some((domain, range)) = domain_range(&pred) else {
                return Err("schema_predicate");
            };
            if !domain.contains(&subject) {
                return Err("schema_domain");
            }
            if !range.contains(&object) {
                return Err("schema_range");
            }
            if pred == Predicate::Supersedes && subject != object {
                return Err("schema_range");
            }
            Ok(())
        }
        _ => Err("schema_shape"),
    }
}

/// DOC-G1 for one alias.
pub fn g1_alias(a: &AliasRow) -> Result<(), &'static str> {
    if ty(&a.entity_type).is_none() {
        return Err("schema_type");
    }
    let n = a.alias.chars().count();
    if !(1..=256).contains(&n) || a.alias.chars().any(char::is_control) {
        return Err("schema_alias");
    }
    if a.alias.trim().is_empty() {
        return Err("schema_alias");
    }
    Ok(())
}

/// Anchors of the blocks of every blob the gate needs, by blob `b3`.
#[derive(Clone, Debug, Default)]
pub struct IrIndex(pub BTreeMap<String, BTreeMap<String, Anchor>>);

impl IrIndex {
    pub fn insert(&mut self, b3: &str, ir: &DocIr) {
        let blocks = ir
            .blocks
            .iter()
            .map(|b| (b.id.clone(), b.anchor.clone()))
            .collect();
        self.0.insert(b3.to_owned(), blocks);
    }
}

/// DOC-G2 for one fact or alias: its evidence rows (non-withdrawn).
pub fn g2(evidence: &[EvidenceRow], ir: &IrIndex) -> Result<(), &'static str> {
    if evidence.is_empty() {
        return Err("no_evidence");
    }
    for ev in evidence {
        let Some(blocks) = ir.0.get(&ev.b3) else {
            return Err("ir_missing");
        };
        let Some(block_anchor) = blocks.get(&ev.block_id) else {
            return Err("block_missing");
        };
        let Ok(anchor) = serde_json::from_value::<Anchor>(ev.anchor.clone()) else {
            return Err("anchor_invalid");
        };
        if anchor.check().is_err() {
            return Err("anchor_invalid");
        }
        if &anchor != block_anchor {
            return Err("anchor_mismatch");
        }
    }
    Ok(())
}

/// Counts of blocked items by reason, for the gate detail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub checked: u64,
    pub blocked_facts: u64,
    pub blocked_aliases: u64,
    pub reasons: BTreeMap<&'static str, u64>,
}

impl Tally {
    pub fn pass(&mut self) {
        self.checked += 1;
    }

    pub fn block(&mut self, reason: &'static str, alias: bool) {
        self.checked += 1;
        if alias {
            self.blocked_aliases += 1;
        } else {
            self.blocked_facts += 1;
        }
        *self.reasons.entry(reason).or_default() += 1;
    }

    /// `pass` without blocked items, else `warn` (the version still
    /// publishes, minus the blocked items).
    pub fn disposition(&self) -> &'static str {
        if self.blocked_facts + self.blocked_aliases == 0 {
            "pass"
        } else {
            "warn"
        }
    }

    pub fn detail(&self) -> Value {
        let reasons: serde_json::Map<String, Value> = self
            .reasons
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Value::from(*v)))
            .collect();
        serde_json::json!({
            "checked": self.checked,
            "blocked_facts": self.blocked_facts,
            "blocked_aliases": self.blocked_aliases,
            "reasons": reasons,
        })
    }
}

#[cfg(test)]
mod g10_tests {
    use super::*;

    fn keys(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn a_small_diff_passes_and_is_sorted() {
        let prior = keys(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let now = keys(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "k"]);
        let d = ReprocessDiff::of(&prior, &now, &[]);
        assert_eq!(
            (d.added.as_slice(), d.removed.as_slice()),
            (&keys(&["k"])[..], &keys(&["j"])[..])
        );
        assert_eq!(d.g10(), Ok(()), "2 of 10 is 20%");
        assert!(ReprocessDiff::of(&prior, &prior, &[]).is_empty());
    }

    #[test]
    fn more_than_thirty_percent_or_a_share_used_fact_blocks() {
        let prior = keys(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let now = keys(&["a", "b", "c", "d", "e", "f", "g", "x", "y", "z"]);
        let d = ReprocessDiff::of(&prior, &now, &[]);
        assert_eq!(d.g10(), Err("diff_over_30_percent"), "6 of 10");
        // Exactly 30% (one removed, two added) is allowed.
        let now = keys(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "x", "y"]);
        assert_eq!(ReprocessDiff::of(&prior, &now, &[]).g10(), Ok(()));
        // One removed fact that a live share uses blocks whatever the size.
        let now = keys(&["a", "b", "c", "d", "e", "f", "g", "h", "i"]);
        let d = ReprocessDiff::of(&prior, &now, &keys(&["j", "zz"]));
        assert_eq!(d.share_used_removed, keys(&["j"]));
        assert_eq!(d.g10(), Err("share_used_fact_removed"));
    }
}
