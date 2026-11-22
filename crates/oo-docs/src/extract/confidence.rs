//! Confidence in basis points and the review thresholds (10 §5.3). Pure.
//!
//! Base score: a rule's fixed value, an LLM `high`/`medium`/`low` is
//! 8500/7000/5000, a person is 10000. Adjustments, in this order and clamped
//! to 0..=10000:
//!
//! 1. the same `fact_key` is also stated by a rule fact: +1000;
//! 2. every further independent file version with evidence: +500, at most
//!    +1500;
//! 3. subject or object is an entity created by this extraction that appears
//!    only once: -1000;
//! 4. the object is a `Thing`: -500.
//!
//! Thresholds (per Drive, global defaults in `docs.json.review`): at or above
//! `auto_publish_bp` a fact publishes by itself; from `queue_min_bp` up to
//! that it goes to the review queue; below it stays a draft and is not queued
//! (it still counts in the evaluation's recall denominator).

/// `auto_publish_bp` default (D24).
pub const AUTO_PUBLISH_BP: i32 = 8000;
/// Lowest score that is queued for review.
pub const QUEUE_MIN_BP: i32 = 4000;

/// An LLM confidence word to its base score.
pub fn llm_base_bp(word: &str) -> Option<i32> {
    match word {
        "high" => Some(8500),
        "medium" => Some(7000),
        "low" => Some(5000),
        _ => None,
    }
}

/// What the adjustments look at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    /// A rule fact with the same `fact_key` exists.
    pub confirmed_by_rule: bool,
    /// Independent file versions with evidence, beyond the first.
    pub extra_versions: u32,
    /// Subject or object is new in this extraction and appears once.
    pub new_single_entity: bool,
    /// The object is a `Thing`.
    pub object_is_thing: bool,
    /// The model's statement, checked by the program: the quote names both ends and the model did not say `low`
    /// (rule id `llm.verified`). D91: such a statement needs no person unless another claim contradicts it.
    pub verified: bool,
    /// Another live fact claims the same thing for someone else (a second owner or decider of the same object
    /// over an overlapping time).
    pub conflicted: bool,
    /// A supersession whose old object has facts a live Share uses (D97).
    pub share_used: bool,
    /// The object is a Project or an Organization: a proper name, so a verified `about` link to it is a statement.
    pub object_proper: bool,
}

/// Base score with the adjustments applied.
pub fn effective_bp(base: i32, s: Signals) -> i32 {
    let mut bp = base;
    if s.confirmed_by_rule {
        bp += 1000;
    }
    bp += i32::try_from(s.extra_versions.min(3)).unwrap_or(3) * 500;
    if s.new_single_entity {
        bp -= 1000;
    }
    if s.object_is_thing {
        bp -= 500;
    }
    bp.clamp(0, 10_000)
}

/// Which way a draft fact goes (5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Publishes without a person.
    Auto,
    /// Waits in the review queue.
    Queue,
    /// Stays a draft, not queued.
    Hold,
}

/// Disposition of a fact with score `bp`.
pub fn disposition(bp: i32, auto_publish_bp: i32, queue_min_bp: i32) -> Disposition {
    if bp >= auto_publish_bp {
        Disposition::Auto
    } else if bp >= queue_min_bp {
        Disposition::Queue
    } else {
        Disposition::Hold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_words() {
        assert_eq!(llm_base_bp("high"), Some(8500));
        assert_eq!(llm_base_bp("medium"), Some(7000));
        assert_eq!(llm_base_bp("low"), Some(5000));
        assert_eq!(llm_base_bp("certain"), None);
    }

    #[test]
    fn adjustments_apply_in_order_and_clamp() {
        let none = Signals::default();
        assert_eq!(effective_bp(7000, none), 7000);
        assert_eq!(
            effective_bp(
                7000,
                Signals {
                    confirmed_by_rule: true,
                    ..none
                }
            ),
            8000
        );
        // Independent evidence is capped at +1500.
        for (n, want) in [(1, 7500), (2, 8000), (3, 8500), (9, 8500)] {
            assert_eq!(
                effective_bp(
                    7000,
                    Signals {
                        extra_versions: n,
                        ..none
                    }
                ),
                want
            );
        }
        let down = Signals {
            new_single_entity: true,
            object_is_thing: true,
            ..none
        };
        assert_eq!(effective_bp(7000, down), 5500);
        assert_eq!(effective_bp(100, down), 0);
        assert_eq!(
            effective_bp(
                10_000,
                Signals {
                    confirmed_by_rule: true,
                    extra_versions: 3,
                    ..none
                }
            ),
            10_000
        );
    }

    #[test]
    fn thresholds() {
        assert_eq!(disposition(8000, 8000, 4000), Disposition::Auto);
        assert_eq!(disposition(7999, 8000, 4000), Disposition::Queue);
        assert_eq!(disposition(4000, 8000, 4000), Disposition::Queue);
        assert_eq!(disposition(3999, 8000, 4000), Disposition::Hold);
    }
}
