//! The four impact rules (30 §L5, 10 §11.1). Pure: the facts about the
//! world are gathered by `store`, the verdict is made here.
//!
//! | rule | verdict |
//! |---|---|
//! | new fact: no such fact, no functional conflict, evidence inside the share's scope | auto accept, listed in the day's digest, can be undone |
//! | supersede a fact no Share ever used | silent supersession, listed in the digest |
//! | supersede a fact a Share used | a person decides; the affected Shares are listed |
//! | preference or type change | always a person |

use serde::Serialize;

/// What `store` found out about one suggestion.
#[derive(Clone, Debug, Default)]
pub struct Findings {
    /// The record is a fact (`new_fact`, `supersede`, `correction`).
    pub fact_kind: bool,
    /// Subject (and object) resolved to entities.
    pub entities_resolved: bool,
    /// A live fact with the same `fact_key` exists.
    pub already_known: bool,
    /// A live fact of the same functional group has another value.
    pub functional_conflict: bool,
    /// At least one evidence reference resolved to a block inside the share's scope.
    pub evidence_in_scope: bool,
    /// For supersede and correction: the old fact was found.
    pub old_found: bool,
    /// The old fact is used by a live Share (`share_fact_use`).
    pub old_in_use: bool,
}

/// The rule that decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    NewFact,
    SupersedeUnused,
    SupersedeInUse,
    Manual,
}

impl Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NewFact => "new_fact",
            Self::SupersedeUnused => "supersede_unused",
            Self::SupersedeInUse => "supersede_in_use",
            Self::Manual => "manual",
        }
    }
}

/// The verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Applied now: `status` of the suggestion afterwards.
    Apply { rule: Rule, status: &'static str },
    /// Nothing to do: the fact is already known.
    Duplicate,
    /// Waits for a person; `reason` is the review item's `reason_code`.
    Review { rule: Rule, reason: &'static str },
}

/// Decide one suggestion.
pub fn decide(kind: &str, f: &Findings) -> Outcome {
    if matches!(kind, "preference" | "type_change") || !f.fact_kind {
        return Outcome::Review {
            rule: Rule::Manual,
            reason: "suggestion_manual_kind",
        };
    }
    if !f.entities_resolved {
        return Outcome::Review {
            rule: Rule::Manual,
            reason: "suggestion_unresolved_entity",
        };
    }
    match kind {
        "new_fact" => {
            if f.already_known {
                Outcome::Duplicate
            } else if f.functional_conflict {
                Outcome::Review {
                    rule: Rule::NewFact,
                    reason: "suggestion_functional_conflict",
                }
            } else if !f.evidence_in_scope {
                Outcome::Review {
                    rule: Rule::NewFact,
                    reason: "suggestion_no_scope_evidence",
                }
            } else {
                Outcome::Apply {
                    rule: Rule::NewFact,
                    status: "auto_accepted",
                }
            }
        }
        _ => {
            // supersede, correction
            if !f.old_found {
                Outcome::Review {
                    rule: Rule::Manual,
                    reason: "suggestion_supersedes_unknown",
                }
            } else if f.old_in_use {
                Outcome::Review {
                    rule: Rule::SupersedeInUse,
                    reason: "suggestion_supersede_in_use",
                }
            } else if f.already_known {
                Outcome::Duplicate
            } else if !f.evidence_in_scope {
                Outcome::Review {
                    rule: Rule::SupersedeUnused,
                    reason: "suggestion_no_scope_evidence",
                }
            } else {
                Outcome::Apply {
                    rule: Rule::SupersedeUnused,
                    status: "superseded_silently",
                }
            }
        }
    }
}

/// Functional groups the suggestion path guards: an `attr.*` value is one per
/// subject; `owns` is one per object (a Project has one owner, 10 §16.7).
pub fn functional_shape(predicate: &str, has_object: bool) -> bool {
    predicate.starts_with("attr.") || (predicate == "owns" && has_object)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Findings {
        Findings {
            fact_kind: true,
            entities_resolved: true,
            evidence_in_scope: true,
            old_found: true,
            ..Findings::default()
        }
    }

    #[test]
    fn rule_new_fact() {
        assert_eq!(
            decide("new_fact", &base()),
            Outcome::Apply {
                rule: Rule::NewFact,
                status: "auto_accepted"
            }
        );
        let mut f = base();
        f.already_known = true;
        assert_eq!(decide("new_fact", &f), Outcome::Duplicate);
        let mut f = base();
        f.functional_conflict = true;
        assert!(matches!(
            decide("new_fact", &f),
            Outcome::Review {
                reason: "suggestion_functional_conflict",
                ..
            }
        ));
        let mut f = base();
        f.evidence_in_scope = false;
        assert!(matches!(
            decide("new_fact", &f),
            Outcome::Review {
                reason: "suggestion_no_scope_evidence",
                ..
            }
        ));
    }

    #[test]
    fn rule_supersede_unused_and_in_use() {
        assert_eq!(
            decide("supersede", &base()),
            Outcome::Apply {
                rule: Rule::SupersedeUnused,
                status: "superseded_silently"
            }
        );
        let mut f = base();
        f.old_in_use = true;
        assert_eq!(
            decide("supersede", &f),
            Outcome::Review {
                rule: Rule::SupersedeInUse,
                reason: "suggestion_supersede_in_use"
            }
        );
        assert_eq!(decide("correction", &f), decide("supersede", &f));
        let mut f = base();
        f.old_found = false;
        assert!(matches!(
            decide("supersede", &f),
            Outcome::Review {
                reason: "suggestion_supersedes_unknown",
                ..
            }
        ));
    }

    #[test]
    fn rule_preference_and_type_change_are_always_human() {
        for k in ["preference", "type_change"] {
            let mut f = base();
            f.fact_kind = false;
            assert_eq!(
                decide(k, &f),
                Outcome::Review {
                    rule: Rule::Manual,
                    reason: "suggestion_manual_kind"
                }
            );
            // even with every condition met
            assert_eq!(
                decide(k, &base()),
                Outcome::Review {
                    rule: Rule::Manual,
                    reason: "suggestion_manual_kind"
                }
            );
        }
    }

    #[test]
    fn unresolved_entities_never_apply() {
        let mut f = base();
        f.entities_resolved = false;
        assert!(matches!(
            decide("new_fact", &f),
            Outcome::Review {
                reason: "suggestion_unresolved_entity",
                ..
            }
        ));
    }

    #[test]
    fn functional_shapes() {
        assert!(functional_shape("attr.status", false));
        assert!(functional_shape("owns", true));
        assert!(!functional_shape("member_of", true));
        assert!(!functional_shape("mentions", true));
    }
}
