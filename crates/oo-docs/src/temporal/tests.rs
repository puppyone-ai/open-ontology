use proptest::prelude::*;
use serde_json::json;

use super::*;

fn t(id: &str, pred: &str, value: &str, from: Option<&str>, conf: i32, origin: Origin) -> TFact {
    TFact {
        id: id.into(),
        fact_key: format!("k{id}"),
        subject_key: "project:p".into(),
        predicate: pred.into(),
        object_key: None,
        object_type: None,
        value: Some(json!({"text": value})),
        valid_from: from.map(str::to_owned),
        valid_to: None,
        confidence_bp: conf,
        origin,
        rule_id: Some("fm.fields".into()),
        published: false,
    }
}

fn status(id: &str, v: &str, from: &str) -> TFact {
    t(id, "attr.status", v, Some(from), 9800, Origin::Rule)
}

fn owner(id: &str, person: &str, from: Option<&str>) -> TFact {
    TFact {
        id: id.into(),
        fact_key: format!("k{id}"),
        subject_key: format!("person:{person}"),
        predicate: "owns".into(),
        object_key: Some("project:p".into()),
        object_type: Some("Project".into()),
        value: None,
        valid_from: from.map(str::to_owned),
        valid_to: None,
        confidence_bp: 9000,
        origin: Origin::Rule,
        rule_id: Some("md.kv".into()),
        published: false,
    }
}

#[test]
fn newer_value_ends_the_older_one() {
    let g = [
        status("1", "进行中", "2026-01-01"),
        status("2", "完成", "2026-06-01"),
    ];
    let p = plan_group(&g);
    assert_eq!(
        p.end,
        [EndFact {
            fact_id: "1".into(),
            valid_to: "2026-06-01".into(),
            superseded_by: "2".into()
        }]
    );
    assert!(p.held.is_empty());
    // After applying there is no overlap.
    assert!(overlaps(&apply_plan(&g, &p)).is_empty());
    assert_eq!(overlaps(&g), [("1".to_owned(), "2".to_owned())]);
}

#[test]
fn the_same_value_again_is_continuity_not_supersession() {
    let g = [
        status("1", "进行中", "2026-01-01"),
        status("2", "进行中", "2026-06-01"),
    ];
    let p = plan_group(&g);
    assert!(p.end.is_empty() && p.held.is_empty());
    assert!(overlaps(&g).is_empty());
}

#[test]
fn late_arrival_is_bounded_by_the_later_fact() {
    // The newer fact was extracted first; an older one arrives afterwards.
    let g = [
        status("2", "完成", "2026-06-01"),
        status("1", "进行中", "2026-01-01"),
    ];
    let p = plan_group(&g);
    assert_eq!(p.end.len(), 1);
    assert_eq!(p.end[0].fact_id, "1");
}

#[test]
fn chain_of_three_ends_each_at_the_next_start() {
    let g = [
        status("1", "计划中", "2026-01-01"),
        status("2", "进行中", "2026-03-01"),
        status("3", "完成", "2026-06-01"),
    ];
    let p = plan_group(&g);
    let ends: Vec<(&str, &str)> = p
        .end
        .iter()
        .map(|e| (e.fact_id.as_str(), e.valid_to.as_str()))
        .collect();
    assert_eq!(ends, [("1", "2026-03-01"), ("2", "2026-06-01")]);
}

#[test]
fn equal_start_different_value_is_a_conflict_the_lower_confidence_waits() {
    let mut a = status("1", "进行中", "2026-01-01");
    a.confidence_bp = 9000;
    let mut b = status("2", "完成", "2026-01-01");
    b.confidence_bp = 9800;
    let p = plan_group(&[a, b]);
    assert!(p.end.is_empty());
    assert_eq!(
        p.held,
        [Held {
            fact_id: "1".into(),
            other_id: "2".into(),
            reason: ReviewReason::FunctionalConflict
        }]
    );
    // Equal confidence: the smaller fact key wins, deterministically.
    let a = status("1", "进行中", "2026-01-01");
    let b = status("2", "完成", "2026-01-01");
    for g in [[a.clone(), b.clone()], [b, a]] {
        let p = plan_group(&g);
        assert_eq!(p.held.len(), 1);
        assert_eq!(p.held[0].fact_id, "2");
    }
}

#[test]
fn a_newcomer_never_unpublishes_a_published_fact() {
    let mut old = status("1", "进行中", "2026-01-01");
    old.published = true;
    old.confidence_bp = 9000;
    let new = status("2", "完成", "2026-01-01"); // higher confidence, same day
    let p = plan_group(&[old, new]);
    assert_eq!(p.held.len(), 1);
    assert_eq!(p.held[0].fact_id, "2", "the newcomer waits");
}

#[test]
fn llm_superseder_only_proposes() {
    let old = status("1", "进行中", "2026-01-01");
    let new = t(
        "2",
        "attr.status",
        "完成",
        Some("2026-06-01"),
        9000,
        Origin::Llm,
    );
    let p = plan_group(&[old, new]);
    assert!(
        p.end.is_empty(),
        "D51: the LLM does not end a fact by itself"
    );
    assert_eq!(
        p.held,
        [Held {
            fact_id: "2".into(),
            other_id: "1".into(),
            reason: ReviewReason::SupersedeCandidate
        }]
    );
    // A human superseder is certain.
    let new = t(
        "3",
        "attr.status",
        "完成",
        Some("2026-06-01"),
        10000,
        Origin::Human,
    );
    let p = plan_group(&[status("1", "进行中", "2026-01-01"), new]);
    assert_eq!(p.end.len(), 1);
}

#[test]
fn owner_is_functional_on_the_project_not_the_person() {
    let a = owner("1", "chen", Some("2026-01-01"));
    let b = owner("2", "zhou", Some("2026-06-01"));
    assert_eq!(functional_group(&a), functional_group(&b));
    assert!(functional_group(&a).is_some());
    let p = plan_group(&[a, b]);
    assert_eq!(p.end[0].fact_id, "1");
    // owns of a non-Project is multi valued; member_of and mentions too.
    let mut doc = owner("3", "chen", Some("2026-01-01"));
    doc.object_type = Some("Document".into());
    assert_eq!(functional_group(&doc), None);
    let mut m = owner("4", "chen", Some("2026-01-01"));
    m.predicate = "member_of".into();
    assert_eq!(functional_group(&m), None);
}

#[test]
fn undated_facts_are_left_alone() {
    let p = plan_group(&[
        owner("1", "chen", None),
        owner("2", "zhou", Some("2026-06-01")),
    ]);
    assert!(p.end.is_empty() && p.held.is_empty());
}

#[test]
fn an_explicit_earlier_end_is_kept() {
    let mut a = owner("1", "chen", Some("2026-01-01"));
    a.valid_to = Some("2026-05-31".into());
    let b = owner("2", "zhou", Some("2026-06-01"));
    // The end stays at 05-31; the next owner is still recorded as the replacement.
    let plan = plan_group(&[a, b]);
    assert_eq!(
        plan.end,
        vec![EndFact {
            fact_id: "1".into(),
            valid_to: "2026-05-31".into(),
            superseded_by: "2".into()
        }]
    );
    assert!(plan.held.is_empty());
}

#[test]
fn future_valid_to_does_not_end_a_fact() {
    assert!(is_live(Some("2099-01-01"), "2026-11-17"));
    assert!(is_live(None, "2026-11-17"));
    assert!(
        !is_live(Some("2026-11-17"), "2026-11-17"),
        "ends on the day"
    );
    assert!(!is_live(Some("2026-01-01"), "2026-11-17"));
    assert!(starts_after(Some("2026-12-01"), "2026-11-17"));
    assert!(!starts_after(Some("2026-11-17"), "2026-11-17"));
    // Planning never ends a fact because its date is in the future relative
    // to anything but another fact.
    let mut a = owner("1", "chen", Some("2026-01-01"));
    a.valid_to = Some("2099-12-31".into());
    assert!(plan_group(&[a]).end.is_empty());
}

fn dec(id: &str, pred: &str, subject: &str, object: &str, from: Option<&str>) -> TFact {
    TFact {
        id: id.into(),
        fact_key: format!("k{id}"),
        subject_key: subject.into(),
        predicate: pred.into(),
        object_key: Some(object.into()),
        object_type: None,
        value: None,
        valid_from: from.map(str::to_owned),
        valid_to: None,
        confidence_bp: 8500,
        origin: Origin::Rule,
        rule_id: Some("md.headings".into()),
        published: true,
    }
}

#[test]
fn rule_certain_supersedes_ends_decides_and_about_of_the_old_decision() {
    let related = [
        dec(
            "a",
            "decides",
            "person:p",
            "decision:old",
            Some("2026-01-01"),
        ),
        dec(
            "b",
            "about",
            "decision:old",
            "project:x",
            Some("2026-01-01"),
        ),
        dec(
            "c",
            "decides",
            "person:p",
            "decision:other",
            Some("2026-01-01"),
        ),
        dec(
            "d",
            "decides",
            "person:q",
            "decision:old",
            Some("2026-02-01"),
        ),
    ];
    let p = plan_decision("s", "decision:old", Some("2026-05-01"), &related);
    let ids: Vec<&str> = p.end.iter().map(|e| e.fact_id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "d"]);
    assert!(
        p.end
            .iter()
            .all(|e| e.valid_to == "2026-05-01" && e.superseded_by == "s")
    );
    // Already ended earlier: left alone. Starts after the date: held.
    let mut ended = dec(
        "e",
        "decides",
        "person:p",
        "decision:old",
        Some("2026-01-01"),
    );
    ended.valid_to = Some("2026-03-01".into());
    let later = dec(
        "f",
        "decides",
        "person:p",
        "decision:old",
        Some("2026-09-01"),
    );
    let p = plan_decision("s", "decision:old", Some("2026-05-01"), &[ended, later]);
    assert!(p.end.is_empty());
    assert_eq!(p.held.len(), 1);
    assert_eq!(p.held[0].reason, ReviewReason::SupersedeUndated);
    // No date: nothing can end.
    let p = plan_decision("s", "decision:old", None, &related);
    assert!(p.end.is_empty() && !p.held.is_empty());
}

#[test]
fn which_supersedes_are_certain() {
    assert!(certain_supersedes(Some("fm.fields"), Origin::Rule));
    assert!(certain_supersedes(Some("md.headings"), Origin::Rule));
    assert!(!certain_supersedes(Some("name.version"), Origin::Rule));
    assert!(!certain_supersedes(None, Origin::Llm));
    assert!(!certain_supersedes(Some("fm.fields"), Origin::Suggest));
    assert!(certain_supersedes(None, Origin::Human));
}

proptest! {
    // The plan is a function of the set of facts.
    #[test]
    fn plan_does_not_depend_on_order(
        picks in proptest::collection::vec((0usize..4, 0usize..4, 0i32..3, any::<bool>()), 1..9),
        perm in Just((0..9usize).collect::<Vec<_>>()).prop_shuffle(),
    ) {
        let days = ["2026-01-01", "2026-03-01", "2026-06-01", "2026-09-01"];
        let vals = ["甲", "乙", "丙", "丁"];
        let g: Vec<TFact> = picks.iter().enumerate().map(|(i, (d, v, c, llm))| {
            let mut f = t(&format!("{i}"), "attr.status", vals[*v], Some(days[*d]), 9000 + c * 100,
                if *llm { Origin::Llm } else { Origin::Rule });
            f.published = i % 3 == 0;
            f
        }).collect();
        let mut shuffled: Vec<TFact> = perm.iter().filter_map(|i| g.get(*i).cloned()).collect();
        shuffled.reverse();
        prop_assert_eq!(plan_group(&g), plan_group(&shuffled));
    }

    // With only certain facts, everything that is not held ends up without overlap.
    #[test]
    fn certain_facts_leave_no_overlap(
        picks in proptest::collection::vec((0usize..4, 0usize..3, 0i32..3), 1..9),
    ) {
        let days = ["2026-01-01", "2026-03-01", "2026-06-01", "2026-09-01"];
        let vals = ["甲", "乙", "丙"];
        let g: Vec<TFact> = picks.iter().enumerate().map(|(i, (d, v, c))|
            t(&format!("{i}"), "attr.status", vals[*v], Some(days[*d]), 9000 + c * 100, Origin::Rule)
        ).collect();
        let plan = plan_group(&g);
        let held: std::collections::BTreeSet<&str> =
            plan.held.iter().map(|h| h.fact_id.as_str()).collect();
        let live: Vec<TFact> = apply_plan(&g, &plan)
            .into_iter().filter(|f| !held.contains(f.id.as_str())).collect();
        prop_assert!(overlaps(&live).is_empty(), "{:?}", overlaps(&live));
    }
}

#[test]
fn an_extraction_never_overrides_a_person() {
    let human = t(
        "1",
        "attr.status",
        "已完成",
        Some("2026-01-01"),
        10000,
        Origin::Human,
    );
    // A later extracted value would normally end the person's: it waits instead.
    let later = status("2", "暂停", "2026-06-01");
    let p = plan_group(&[human.clone(), later]);
    assert!(p.end.is_empty());
    assert_eq!(
        p.held,
        [Held {
            fact_id: "2".into(),
            other_id: "1".into(),
            reason: ReviewReason::ConflictsWithHumanOverlay
        }]
    );
    // The same day: the person's value stays, the extraction waits with the same reason.
    let same_day = status("3", "暂停", "2026-01-01");
    let p = plan_group(&[human.clone(), same_day]);
    assert_eq!(p.held.len(), 1);
    assert_eq!(p.held[0].reason, ReviewReason::ConflictsWithHumanOverlay);
    assert_eq!(p.held[0].reason.code(), "conflicts_with_human_overlay");
    // A person's later value ends an extracted earlier one as usual.
    let newer = t(
        "4",
        "attr.status",
        "完成",
        Some("2026-09-01"),
        10000,
        Origin::Human,
    );
    let p = plan_group(&[status("5", "进行中", "2026-01-01"), newer]);
    assert_eq!(p.end.len(), 1);
}
