use oo_core::docir::Anchor;
use serde_json::json;

use super::gates::*;

fn fact(pred: &str, s: &str, o: Option<&str>, v: Option<serde_json::Value>) -> FactRow {
    FactRow {
        id: "00000000-0000-0000-0000-000000000001".into(),
        fact_key: "0".repeat(64),
        predicate: pred.into(),
        subject_type: s.into(),
        object_type: o.map(str::to_owned),
        value: v,
    }
}

#[test]
fn g1_accepts_the_core_shapes() {
    for (p, s, o) in [
        ("owns", "Person", "Project"),
        ("member_of", "Person", "Project"),
        ("member_of", "Person", "Organization"),
        ("decides", "Person", "Decision"),
        ("supersedes", "Decision", "Decision"),
        ("rejects", "Decision", "Term"),
        ("tried_and_failed", "Project", "Term"),
        ("mentions", "Document", "Person"),
        ("about", "Decision", "Project"),
        ("about", "Document", "Project"),
    ] {
        assert_eq!(g1_fact(&fact(p, s, Some(o), None)), Ok(()), "{p} {s} {o}");
    }
    let v = Some(json!({"text": "active"}));
    assert_eq!(g1_fact(&fact("attr.status", "Project", None, v)), Ok(()));
}

#[test]
fn g1_rejects_each_kind_of_violation() {
    assert_eq!(
        g1_fact(&fact("likes", "Person", Some("Project"), None)),
        Err("schema_predicate")
    );
    assert_eq!(
        g1_fact(&fact(
            "attr.Bad",
            "Project",
            None,
            Some(json!({"text":"x"}))
        )),
        Err("schema_predicate")
    );
    assert_eq!(
        g1_fact(&fact("owns", "Project", Some("Person"), None)),
        Err("schema_domain")
    );
    assert_eq!(
        g1_fact(&fact("owns", "Person", Some("Person"), None)),
        Err("schema_range")
    );
    assert_eq!(
        g1_fact(&fact("decides", "Person", Some("Project"), None)),
        Err("schema_range")
    );
    assert_eq!(
        g1_fact(&fact("supersedes", "Decision", Some("Document"), None)),
        Err("schema_range")
    );
    assert_eq!(
        g1_fact(&fact("mentions", "Person", Some("Person"), None)),
        Err("schema_domain")
    );
    assert_eq!(
        g1_fact(&fact("owns", "Alien", Some("Project"), None)),
        Err("schema_type")
    );
    assert_eq!(
        g1_fact(&fact("owns", "Person", None, None)),
        Err("schema_shape")
    );
    assert_eq!(
        g1_fact(&fact(
            "owns",
            "Person",
            Some("Project"),
            Some(json!({"text":"x"}))
        )),
        Err("schema_shape")
    );
    assert_eq!(
        g1_fact(&fact("attr.status", "Project", Some("Project"), None)),
        Err("schema_shape")
    );
    assert_eq!(
        g1_fact(&fact(
            "attr.status",
            "Project",
            None,
            Some(json!({"text":"  "}))
        )),
        Err("schema_value")
    );
    assert_eq!(
        g1_fact(&fact(
            "attr.status",
            "Project",
            None,
            Some(json!({"other":"x"}))
        )),
        Err("schema_value")
    );
    assert_eq!(
        g1_fact(&fact(
            "attr.status",
            "Project",
            None,
            Some(json!({"text":"a","enum":"b"}))
        )),
        Err("schema_value")
    );
    let long = "字".repeat(MAX_VALUE_CHARS + 1);
    assert_eq!(
        g1_fact(&fact(
            "attr.status",
            "Project",
            None,
            Some(json!({"text": long}))
        )),
        Err("schema_value")
    );
}

#[test]
fn g1_alias_rules() {
    let a = |alias: &str, t: &str| AliasRow {
        id: "x".into(),
        entity_key: "person:aa".into(),
        norm_alias: alias.to_lowercase(),
        alias: alias.into(),
        entity_type: t.into(),
    };
    assert_eq!(g1_alias(&a("小陈", "Person")), Ok(()));
    assert_eq!(g1_alias(&a("", "Person")), Err("schema_alias"));
    assert_eq!(g1_alias(&a("  ", "Person")), Err("schema_alias"));
    assert_eq!(g1_alias(&a("a\u{0}b", "Person")), Err("schema_alias"));
    assert_eq!(
        g1_alias(&a(&"x".repeat(257), "Person")),
        Err("schema_alias")
    );
    assert_eq!(g1_alias(&a("x", "Alien")), Err("schema_type"));
}

fn anchor(l: u32) -> Anchor {
    Anchor::Text {
        line_start: l,
        line_end: l,
        char_start: 0,
        char_end: 5,
    }
}

fn index() -> IrIndex {
    let mut m = std::collections::BTreeMap::new();
    m.insert("blk_a".to_owned(), anchor(1));
    let mut i = IrIndex::default();
    i.0.insert("b3x".to_owned(), m);
    i
}

fn ev(block: &str, b3: &str, a: &Anchor) -> EvidenceRow {
    EvidenceRow {
        block_id: block.into(),
        b3: b3.into(),
        anchor: serde_json::to_value(a).unwrap(),
    }
}

#[test]
fn g2_checks_presence_block_and_anchor() {
    let ir = index();
    assert_eq!(g2(&[], &ir), Err("no_evidence"));
    assert_eq!(g2(&[ev("blk_a", "b3x", &anchor(1))], &ir), Ok(()));
    assert_eq!(
        g2(&[ev("blk_a", "other", &anchor(1))], &ir),
        Err("ir_missing")
    );
    assert_eq!(
        g2(&[ev("blk_zz", "b3x", &anchor(1))], &ir),
        Err("block_missing")
    );
    assert_eq!(
        g2(&[ev("blk_a", "b3x", &anchor(2))], &ir),
        Err("anchor_mismatch")
    );
    let bad = Anchor::Text {
        line_start: 0,
        line_end: 1,
        char_start: 0,
        char_end: 1,
    };
    assert_eq!(g2(&[ev("blk_a", "b3x", &bad)], &ir), Err("anchor_invalid"));
    let junk = EvidenceRow {
        block_id: "blk_a".into(),
        b3: "b3x".into(),
        anchor: json!({"kind":"nope"}),
    };
    assert_eq!(g2(&[junk], &ir), Err("anchor_invalid"));
    // One bad evidence fails the item even if another is fine.
    assert_eq!(
        g2(
            &[
                ev("blk_a", "b3x", &anchor(1)),
                ev("blk_zz", "b3x", &anchor(1))
            ],
            &ir
        ),
        Err("block_missing")
    );
}

#[test]
fn tally_disposition() {
    let mut t = Tally::default();
    t.pass();
    assert_eq!(t.disposition(), "pass");
    t.block("schema_domain", false);
    t.block("no_evidence", true);
    assert_eq!(t.disposition(), "warn");
    let d = t.detail();
    assert_eq!(d["checked"], 3);
    assert_eq!(d["blocked_facts"], 1);
    assert_eq!(d["blocked_aliases"], 1);
    assert_eq!(d["reasons"]["schema_domain"], 1);
}
