use proptest::prelude::*;

use super::*;
use crate::extract::model::{EntityType, EvidenceRef};
use oo_core::docir::Anchor;

fn ev(tag: &str) -> EvidenceRef {
    EvidenceRef {
        block_id: format!("blk_{tag:0>20}"),
        anchor: Anchor::Text {
            line_start: 1,
            line_end: 1,
            char_start: 0,
            char_end: 1,
        },
        quote_b3: "0".repeat(64),
    }
}

fn person(n: &str) -> EntityRef {
    EntityRef::new(EntityType::Person, n)
}

fn cand(key: &str, via: Via) -> Candidate {
    Candidate {
        key: key.into(),
        via,
    }
}

#[test]
fn no_candidates_creates() {
    assert_eq!(
        resolve_one(&EmptyIndex, &person("陈子墨")),
        Resolution::Create
    );
}

#[test]
fn canonical_hit_binds_and_alias_ranking() {
    let m = person("陈子墨");
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:aaa", Via::CanonicalName),
    );
    assert_eq!(resolve_one(&idx, &m), Resolution::Bind("person:aaa".into()));

    // Published alias alone binds (9000 >= 8500); a draft alias alone does not.
    let m = person("小陈");
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:bbb", Via::AliasPublished),
    );
    assert_eq!(resolve_one(&idx, &m), Resolution::Bind("person:bbb".into()));
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:bbb", Via::AliasDraft),
    );
    assert_eq!(resolve_one(&idx, &m), Resolution::Create);
}

#[test]
fn canonical_leads_a_published_alias_by_exactly_the_gap() {
    let m = person("x");
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        "x",
        cand("person:c", Via::CanonicalName),
    );
    idx.insert(
        EntityType::Person,
        None,
        "x",
        cand("person:a", Via::AliasPublished),
    );
    assert_eq!(resolve_one(&idx, &m), Resolution::Bind("person:c".into()));
}

#[test]
fn ties_are_ambiguous_and_sorted() {
    let m = person("小陈");
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:zzz", Via::AliasPublished),
    );
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:aaa", Via::AliasPublished),
    );
    assert_eq!(
        resolve_one(&idx, &m),
        Resolution::Ambiguous(vec!["person:aaa".into(), "person:zzz".into()])
    );
}

#[test]
fn own_key_alone_is_not_a_conflict_with_itself() {
    let m = person("陈子墨");
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand(&m.key(), Via::CanonicalName),
    );
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand(&m.key(), Via::AliasPublished),
    );
    assert_eq!(resolve_one(&idx, &m), Resolution::Bind(m.key()));
}

#[test]
fn types_never_cross() {
    let m = EntityRef::new(EntityType::Project, "星图");
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:aaa", Via::CanonicalName),
    );
    assert_eq!(resolve_one(&idx, &m), Resolution::Create);
}

#[test]
fn apply_rekeys_facts_to_the_bound_entity() {
    use crate::extract::model::{EntityMention, FactMention, Predicate};
    let alias_mention = person("小陈");
    let proj = EntityRef::new(EntityType::Project, "星图看板");
    let x = Extraction {
        path: "Projects/星图看板.md".into(),
        entities: vec![
            EntityMention {
                entity: alias_mention.clone(),
                evidence: vec![ev("1")],
            },
            EntityMention {
                entity: proj.clone(),
                evidence: vec![ev("1")],
            },
        ],
        aliases: vec![],
        facts: vec![FactMention {
            subject: alias_mention.clone(),
            predicate: Predicate::Owns,
            object: Some(proj.clone()),
            value: None,
            valid_from: Some("2026-01-01".into()),
            valid_to: None,
            valid_precision: Some("day"),
            confidence_bp: 9000,
            rule_id: "md.kv",
            evidence: vec![ev("1")],
        }],
    };
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &alias_mention.norm(),
        cand("person:stored", Via::AliasPublished),
    );
    let r = resolve_extraction(&idx, &x);
    assert_eq!(r.facts.len(), 1);
    assert_eq!(r.facts[0].subject_key, "person:stored");
    assert_eq!(r.facts[0].object_key.as_deref(), Some(proj.key().as_str()));
    let direct = crate::resolve::normalize::fact_key(
        "person:stored",
        "owns",
        Some(&proj.key()),
        None,
        Some("2026-01-01"),
    );
    assert_eq!(r.facts[0].fact_key, direct);
    let bound: Vec<_> = r
        .entities
        .iter()
        .filter(|e| e.bound)
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(bound, ["person:stored"]);
    assert!(r.conflicts.is_empty());
}

#[test]
fn ambiguous_creates_under_own_key_and_reports_conflict() {
    use crate::extract::model::EntityMention;
    let m = person("小陈");
    let x = Extraction {
        path: "a.md".into(),
        entities: vec![EntityMention {
            entity: m.clone(),
            evidence: vec![ev("2")],
        }],
        ..Extraction::default()
    };
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:a", Via::AliasPublished),
    );
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:b", Via::AliasPublished),
    );
    let r = resolve_extraction(&idx, &x);
    assert_eq!(r.entities.len(), 1);
    assert_eq!(r.entities[0].key, m.key());
    assert!(!r.entities[0].bound);
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(r.conflicts[0].entity_keys, ["person:a", "person:b"]);
}

proptest! {
    #[test]
    fn candidate_order_does_not_matter(perm in Just((0..5usize).collect::<Vec<_>>()).prop_shuffle(), top in 0usize..3) {
        let vias = [Via::CanonicalName, Via::AliasPublished, Via::AliasDraft];
        let all: Vec<Candidate> = (0..5)
            .map(|i| cand(&format!("person:{i}"), vias[(i + top) % 3]))
            .collect();
        let m = person("同名");
        let mut a = MapIndex::default();
        let mut b = MapIndex::default();
        for c in &all { a.insert(EntityType::Person, None, &m.norm(), c.clone()); }
        for &i in &perm { b.insert(EntityType::Person, None, &m.norm(), all[i].clone()); }
        prop_assert_eq!(resolve_one(&a, &m), resolve_one(&b, &m));
    }
}
