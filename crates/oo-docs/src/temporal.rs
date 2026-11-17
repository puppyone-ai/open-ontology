//! Bitemporal facts and supersession (10 §5.5, D17, D51). Board WS-BE3.
//!
//! Every fact has a real-world interval `valid_from..valid_to` and a system
//! interval (`recorded_at`, `retracted_at`). This module is the pure part:
//!
//! * [`is_live`] and [`starts_after`]: the validity of a fact against a version's
//!   `as_of`. A `valid_to` in the future does not end a fact (it ends when
//!   `as_of` passes it); validity is only ever compared with `as_of`, never
//!   with the wall clock.
//! * [`plan_group`]: functional predicates (`attr.status`, `attr.role`,
//!   `owns` of a Project) allow one value at a time. Over the live facts of
//!   one group, a fact with a different value that starts later ends the
//!   earlier one (`valid_to = later.valid_from`, `superseded_by = later`);
//!   equal starts with different values are a conflict, where the lower
//!   confidence goes to review and the higher one proceeds. A superseder that
//!   is not rule-certain (LLM, suggestion) only proposes: D51 sends it to
//!   review. The result depends on the set of facts, not on their order.
//! * [`plan_decision`]: a rule-certain `supersedes` between two Decisions ends
//!   the old decision's `decides` and `about` facts at the new decision's date.
//!
//! The database side is `temporal::pg`.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

pub mod pg;

/// Who made a fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    Rule,
    Llm,
    Human,
    Suggest,
}

impl Origin {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "rule" => Self::Rule,
            "llm" => Self::Llm,
            "human" => Self::Human,
            "suggest" => Self::Suggest,
            _ => return None,
        })
    }

    /// Rule and human facts may change other facts by themselves (D51);
    /// LLM and suggestion facts only propose.
    pub const fn certain(self) -> bool {
        matches!(self, Self::Rule | Self::Human)
    }
}

/// The columns of a fact that time cares about. Dates are `YYYY-MM-DD`, so
/// string order is date order.
#[derive(Clone, Debug, PartialEq)]
pub struct TFact {
    pub id: String,
    pub fact_key: String,
    pub subject_key: String,
    pub predicate: String,
    pub object_key: Option<String>,
    /// Type of the object entity (`owns` is functional on a Project).
    pub object_type: Option<String>,
    pub value: Option<Value>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub confidence_bp: i32,
    pub origin: Origin,
    /// `rule_id` of a rule fact (`fm.fields`, `md.headings`, ...).
    pub rule_id: Option<String>,
    /// Already in a published version (a published fact is never unpublished
    /// by a newcomer).
    pub published: bool,
}

/// Group of facts that may hold only one value at a time, or `None` when the
/// predicate is multi valued (`member_of`, `mentions`, `about`, ...).
pub fn functional_group(f: &TFact) -> Option<String> {
    match f.predicate.as_str() {
        "attr.status" | "attr.role" => Some(format!("{}\0{}", f.subject_key, f.predicate)),
        "owns" if f.object_type.as_deref() == Some("Project") => {
            Some(format!("{}\0owns", f.object_key.as_deref()?))
        }
        _ => None,
    }
}

/// Same object or same value. `owns` is functional on the Project, so its
/// "value" is the owner: two `owns` of one Project agree when the subject is
/// the same.
pub fn same_value(a: &TFact, b: &TFact) -> bool {
    if a.predicate == "owns" && b.predicate == "owns" {
        return a.subject_key == b.subject_key;
    }
    match (&a.object_key, &b.object_key) {
        (Some(x), Some(y)) => x == y,
        (None, None) => match (&a.value, &b.value) {
            (Some(x), Some(y)) => oo_builder::canonical_json(x) == oo_builder::canonical_json(y),
            _ => false,
        },
        _ => false,
    }
}

/// A fact is live at `as_of` when it has not ended: `valid_to` is empty or
/// after `as_of`. A `valid_to` in the future never ends a fact early.
pub fn is_live(valid_to: Option<&str>, as_of: &str) -> bool {
    valid_to.is_none_or(|to| to > as_of)
}

/// The fact starts after `as_of` (it is announced, not yet in force).
pub fn starts_after(valid_from: Option<&str>, as_of: &str) -> bool {
    valid_from.is_some_and(|from| from > as_of)
}

/// Why something goes to the review queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReviewReason {
    /// Two values start the same day; this is the lower one.
    FunctionalConflict,
    /// The superseder is an LLM or suggestion candidate (D18, D51).
    SupersedeCandidate,
    /// The old fact is used by a live Share (D51).
    SupersedeInUse,
    /// The superseding date is unknown or before the old fact started.
    SupersedeUndated,
    /// A later extraction disagrees with a person's correction, which is
    /// never overridden by itself (D22, 16.8).
    ConflictsWithHumanOverlay,
}

impl ReviewReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::FunctionalConflict => "functional_conflict",
            Self::SupersedeCandidate => "supersede_candidate",
            Self::SupersedeInUse => "supersede_in_use",
            Self::SupersedeUndated => "supersede_undated",
            Self::ConflictsWithHumanOverlay => "conflicts_with_human_overlay",
        }
    }
}

/// End `fact` at `valid_to`, because `by` starts then.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EndFact {
    pub fact_id: String,
    pub valid_to: String,
    pub superseded_by: String,
}

/// A fact that waits for a person.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Held {
    /// The fact that goes to review (the loser, or the proposer).
    pub fact_id: String,
    /// The other fact of the pair.
    pub other_id: String,
    pub reason: ReviewReason,
}

/// What a group or a decision supersession does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub end: Vec<EndFact>,
    pub held: Vec<Held>,
}

impl Plan {
    fn sort(mut self) -> Self {
        self.end.sort();
        self.end.dedup();
        self.held.sort();
        self.held.dedup();
        self
    }
}

fn better(a: &TFact, b: &TFact) -> Ordering {
    // Published first, then higher confidence; ties go to the smaller fact
    // key, then id.
    b.published
        .cmp(&a.published)
        .then_with(|| b.confidence_bp.cmp(&a.confidence_bp))
        .then_with(|| a.fact_key.cmp(&b.fact_key))
        .then_with(|| a.id.cmp(&b.id))
}

/// Plan the live facts of one functional group (published facts and the
/// eligible drafts). Facts without `valid_from` cannot be ordered and are
/// left alone.
pub fn plan_group(group: &[TFact]) -> Plan {
    let mut plan = Plan::default();
    // Facts by start date.
    let mut by_start: BTreeMap<&str, Vec<&TFact>> = BTreeMap::new();
    for f in group {
        if let Some(from) = f.valid_from.as_deref() {
            by_start.entry(from).or_default().push(f);
        }
    }
    // Same start, different values: the best stays, the others wait. A
    // published fact always beats a draft: it is not unpublished here.
    let mut losers: BTreeSet<&str> = BTreeSet::new();
    for facts in by_start.values() {
        let mut sorted: Vec<&TFact> = facts.clone();
        sorted.sort_by(|a, b| better(a, b));
        let Some(top) = sorted.first() else { continue };
        for other in sorted.iter().skip(1) {
            if !same_value(top, other) {
                losers.insert(other.id.as_str());
                plan.held.push(Held {
                    fact_id: other.id.clone(),
                    other_id: top.id.clone(),
                    reason: if top.origin == Origin::Human && other.origin != Origin::Human {
                        ReviewReason::ConflictsWithHumanOverlay
                    } else {
                        ReviewReason::FunctionalConflict
                    },
                });
            }
        }
    }
    // Timeline: each surviving fact ends where the next different value begins.
    let starts: Vec<&str> = by_start.keys().copied().collect();
    for f in group {
        let Some(from) = f.valid_from.as_deref() else {
            continue;
        };
        if losers.contains(f.id.as_str()) {
            continue;
        }
        // First later start with a different value among the survivors.
        let next = starts.iter().filter(|s| **s > from).find_map(|s| {
            by_start[*s]
                .iter()
                .filter(|g| !losers.contains(g.id.as_str()) && !same_value(f, g))
                .min_by(|a, b| better(a, b))
                .copied()
        });
        let Some(g) = next else { continue };
        let to = g.valid_from.as_deref().unwrap_or_default();
        if f.valid_to.as_deref().is_some_and(|have| have <= to) {
            continue;
        }
        if f.origin == Origin::Human && g.origin != Origin::Human {
            // A person's value is not ended by an extraction (16.8).
            plan.held.push(Held {
                fact_id: g.id.clone(),
                other_id: f.id.clone(),
                reason: ReviewReason::ConflictsWithHumanOverlay,
            });
        } else if g.origin.certain() {
            plan.end.push(EndFact {
                fact_id: f.id.clone(),
                valid_to: to.to_owned(),
                superseded_by: g.id.clone(),
            });
        } else {
            // A candidate does not end anything by itself (D51).
            plan.held.push(Held {
                fact_id: g.id.clone(),
                other_id: f.id.clone(),
                reason: ReviewReason::SupersedeCandidate,
            });
        }
    }
    plan.sort()
}

/// Rules whose `supersedes` is certain: an explicit statement in frontmatter or
/// under a heading. `name.version` only proposes.
pub fn certain_supersedes(rule_id: Option<&str>, origin: Origin) -> bool {
    match origin {
        Origin::Human => true,
        Origin::Rule => matches!(rule_id, Some("fm.fields" | "md.headings")),
        Origin::Llm | Origin::Suggest => false,
    }
}

/// Plan a `supersedes` fact between two Decisions: the old decision's
/// `decides` (object = old) and `about` (subject = old) facts end at `date`.
/// `related` are the facts around the old decision; `date` is the new
/// decision's start. Facts that already end by then are left alone; a fact
/// that starts after `date` cannot end then and is held.
pub fn plan_decision(
    supersedes_id: &str,
    old_key: &str,
    date: Option<&str>,
    related: &[TFact],
) -> Plan {
    let mut plan = Plan::default();
    for f in related {
        let hit = (f.predicate == "decides" && f.object_key.as_deref() == Some(old_key))
            || (f.predicate == "about" && f.subject_key == old_key);
        if !hit || f.id == supersedes_id {
            continue;
        }
        let Some(date) = date else {
            plan.held.push(Held {
                fact_id: supersedes_id.to_owned(),
                other_id: f.id.clone(),
                reason: ReviewReason::SupersedeUndated,
            });
            continue;
        };
        if f.valid_to.as_deref().is_some_and(|to| to <= date) {
            continue;
        }
        if f.valid_from.as_deref().is_some_and(|from| from > date) {
            plan.held.push(Held {
                fact_id: supersedes_id.to_owned(),
                other_id: f.id.clone(),
                reason: ReviewReason::SupersedeUndated,
            });
            continue;
        }
        plan.end.push(EndFact {
            fact_id: f.id.clone(),
            valid_to: date.to_owned(),
            superseded_by: supersedes_id.to_owned(),
        });
    }
    plan.sort()
}

/// DOC-G3 for one group: pairs of live facts with different values whose
/// intervals overlap (`valid_to` empty is open ended). Both ends use the
/// plan's result, so a planned supersession is not an overlap.
pub fn overlaps(group: &[TFact]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (i, a) in group.iter().enumerate() {
        for b in group.iter().skip(i + 1) {
            if same_value(a, b) {
                continue;
            }
            let (Some(af), Some(bf)) = (a.valid_from.as_deref(), b.valid_from.as_deref()) else {
                continue;
            };
            let a_to = a.valid_to.as_deref();
            let b_to = b.valid_to.as_deref();
            let a_before_b = a_to.is_some_and(|t| t <= bf);
            let b_before_a = b_to.is_some_and(|t| t <= af);
            if !a_before_b && !b_before_a {
                let (x, y) = if a.id <= b.id {
                    (a.id.clone(), b.id.clone())
                } else {
                    (b.id.clone(), a.id.clone())
                };
                out.push((x, y));
            }
        }
    }
    out.sort();
    out
}

/// Apply a plan to a group in memory (tests and the publisher's G3 check).
pub fn apply_plan(group: &[TFact], plan: &Plan) -> Vec<TFact> {
    let ends: BTreeMap<&str, &str> = plan
        .end
        .iter()
        .map(|e| (e.fact_id.as_str(), e.valid_to.as_str()))
        .collect();
    group
        .iter()
        .map(|f| {
            let mut f = f.clone();
            if let Some(to) = ends.get(f.id.as_str())
                && f.valid_to.as_deref().is_none_or(|have| *to < have)
            {
                f.valid_to = Some((*to).to_owned());
            }
            f
        })
        .collect()
}

#[cfg(test)]
mod tests;
