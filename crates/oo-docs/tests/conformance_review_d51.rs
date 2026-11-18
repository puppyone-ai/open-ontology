//! FM5 conformance for D51 (80 section 4, card V-S5-01): which supersessions run by themselves and which wait
//! for a person. The model (`formal/quint/review_decide.qnt`, kind SUPER) derives `certain` from the origin and
//! the rule id of the fact; this test reads the committed and bulk ITF traces, takes every supersession slot the
//! model creates and compares its `certain` flag and its queueing with the real code:
//! `temporal::certain_supersedes` and `review::queue::classify` (a `supersedes` fact that is not certain is
//! always queued as `supersede_candidate`, whatever its confidence; a certain one follows the thresholds).
//!
//! No database. The merge half of D51 (kind MERGE: only a shared identity attribute merges by itself, an LLM
//! candidate is queued) is exercised by `resolve_live.rs`, not replayed here.
//!
//! ```text
//! cargo test -p oo-docs --locked --test conformance_review_d51
//! OO_FM5_TRACES_DIR=/tmp/fm5-traces ... (bulk; the stepFacts profile has no supersessions, so use the default step:
//!   formal/scripts/formal-traces review_decide review_decide 2000 40 0x464d /tmp/fm5-super)
//! ```

#[path = "itf_support/mod.rs"]
mod itf;

use itf::*;
use oo_docs::extract::confidence::Signals;
use oo_docs::review::queue::{Class, Thresholds, classify};
use oo_docs::temporal::{Origin, certain_supersedes};

fn origin(s: &str) -> Origin {
    Origin::parse(s).unwrap_or_else(|| panic!("origin {s}"))
}

#[test]
fn model_certainty_equals_certain_supersedes_for_every_origin_and_rule() {
    // the whole table, not only what a trace happens to reach
    for og in ["llm", "rule", "human"] {
        for rid in ["fm.fields", "md.headings", "name.version", "none"] {
            let model =
                og == "human" || (og == "rule" && (rid == "fm.fields" || rid == "md.headings"));
            let rule_id = (rid != "none").then_some(rid);
            assert_eq!(
                certain_supersedes(rule_id, origin(og)),
                model,
                "origin {og}, rule {rid}"
            );
        }
    }
}

#[test]
fn model_supersession_slots_are_queued_exactly_when_not_certain() {
    let traces = load_traces("review_decide_super", "OO_FM5_SUPER_TRACES_DIR");
    let (mut seen, mut certain_n, mut queued_n) = (0usize, 0usize, 0usize);
    for (name, t) in &traces {
        for st in t["states"].as_array().unwrap() {
            for (idx, s) in map_values(&st["slots"]) {
                if int(&s["kind"]) != 2 || text(&s["fst"]) == "none" {
                    continue;
                }
                let og = text(&s["origin"]);
                let rid = text(&s["rid"]);
                let rule_id = (rid != "none").then_some(rid);
                let model_certain = s["certain"].as_bool().unwrap();
                assert_eq!(
                    certain_supersedes(rule_id, origin(og)),
                    model_certain,
                    "{name} slot {idx}: origin {og} rule {rid}"
                );
                // classification of the supersedes fact at any confidence the model gives it
                let bp = match int(&s["conf"]) {
                    2 => 9500,
                    1 => 6000,
                    _ => 3000,
                };
                let (_, class) = classify(
                    origin(og),
                    rule_id,
                    "supersedes",
                    bp,
                    Signals::default(),
                    false,
                    false,
                    Thresholds::default(),
                );
                if !model_certain {
                    assert_eq!(
                        class,
                        Class::Queue("supersede_candidate"),
                        "{name} slot {idx}"
                    );
                    queued_n += 1;
                } else {
                    assert_ne!(
                        class,
                        Class::Queue("supersede_candidate"),
                        "{name} slot {idx}"
                    );
                    certain_n += 1;
                }
                // in the model a not-certain supersession only runs after a person accepted it
                if int(&s["kind"]) == 2 && s["executed"].as_bool().unwrap() && !model_certain {
                    assert_eq!(text(&s["ist"]), "accepted", "{name} slot {idx}");
                }
                seen += 1;
            }
        }
    }
    eprintln!(
        "FM5 D51: {seen} supersession slots, {certain_n} certain, {queued_n} queued as supersede_candidate"
    );
    assert!(
        seen > 0 && certain_n > 0 && queued_n > 0,
        "the traces do not reach supersessions"
    );
}
