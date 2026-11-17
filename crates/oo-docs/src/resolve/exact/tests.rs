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
    Candidate::new(key, via)
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
        ..Extraction::default()
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

// ---- B-S5-04: identity, honorifics, context, permutation invariance ----

use std::collections::BTreeMap;

fn ident(pairs: &[(&str, &str)]) -> Identity {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn cand_with(key: &str, via: Via, id: &[(&str, &str)], neigh: &[&str], dirs: &[&str]) -> Candidate {
    Candidate {
        identity: ident(id),
        neighbors: neigh.iter().map(|s| (*s).to_owned()).collect(),
        dirs: dirs.iter().map(|s| (*s).to_owned()).collect(),
        ..Candidate::new(key, via)
    }
}

#[test]
fn step2_identity_binds_before_any_name_match() {
    let mut idx = MapIndex::default();
    idx.insert_identity(
        EntityType::Person,
        "email",
        "ethan@x.io",
        cand_with(
            "person:eth",
            Via::Identity,
            &[("email", "ethan@x.io")],
            &[],
            &[],
        ),
    );
    // The name is unknown, the email is not.
    let m = person("陈子墨的同事");
    let got = resolve_scored(
        &idx,
        &m,
        &ident(&[("email", "ethan@x.io")]),
        &Context::default(),
    );
    assert_eq!(got, Resolution::Bind("person:eth".into()));
    // A different type never binds on the identity.
    let org = EntityRef::new(EntityType::Organization, "X");
    assert_eq!(
        resolve_scored(
            &idx,
            &org,
            &ident(&[("email", "ethan@x.io")]),
            &Context::default()
        ),
        Resolution::Create
    );
}

#[test]
fn step3_same_name_other_email_is_a_different_person() {
    let m = person("张三");
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand_with(
            "person:old",
            Via::CanonicalName,
            &[("email", "a@x.io")],
            &[],
            &[],
        ),
    );
    let id = ident(&[("email", "b@x.io")]);
    let got = resolve_scored(&idx, &m, &id, &Context::default());
    let key = identity_key(&m, &id);
    assert_ne!(key, m.key(), "the identity is part of the key");
    assert_eq!(
        got,
        Resolution::Distinct {
            key: key.clone(),
            against: vec!["person:old".into()]
        }
    );
    // Same email: it is the same person.
    let same = ident(&[("email", "a@x.io")]);
    assert_eq!(
        resolve_scored(&idx, &m, &same, &Context::default()),
        Resolution::Bind("person:old".into())
    );
    // The stored entity has no identity yet: no contradiction.
    let mut idx2 = MapIndex::default();
    idx2.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:bare", Via::CanonicalName),
    );
    assert_eq!(
        resolve_scored(&idx2, &m, &id, &Context::default()),
        Resolution::Bind("person:bare".into())
    );
    // apply: both entities are kept and the conflict names both.
    use crate::extract::model::{EntityMention, IdentityMention};
    let x = Extraction {
        path: "a.md".into(),
        entities: vec![EntityMention {
            entity: m.clone(),
            evidence: vec![ev("3")],
        }],
        identities: vec![IdentityMention {
            entity: m.clone(),
            kind: "email",
            value: "b@x.io".into(),
            confidence_bp: 9000,
            rule_id: "text.email",
            evidence: vec![ev("3")],
        }],
        ..Extraction::default()
    };
    let r = resolve_extraction(&idx, &x);
    assert_eq!(r.entities.len(), 1);
    assert_eq!(r.entities[0].key, key);
    assert_eq!(r.entities[0].identity, id);
    assert!(!r.entities[0].bound);
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(r.conflicts[0].reason, "identity");
    let mut both = vec!["person:old".to_owned(), key];
    both.sort();
    assert_eq!(r.conflicts[0].entity_keys, both);
}

#[test]
fn step1_honorific_forms_rank_low_and_never_bind() {
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &norm("陈子墨"),
        cand("person:czm", Via::CanonicalName),
    );
    idx.insert_surname("陈", cand("person:czm", Via::CanonicalName));
    let m = person("陈总");
    let r = rank(&idx, &m, &Identity::new(), &Context::default());
    assert_eq!(r.ranked.len(), 1);
    assert_eq!((r.ranked[0].0, r.ranked[0].1.via), (5000, Via::Honorific));
    assert_eq!(resolve_one(&idx, &m), Resolution::Create);
    // Even with all the context there is (500 * 3 + 500), 7000 is below 8500.
    let ctx = Context {
        keys: ["a", "b", "c", "d"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        dir: "Projects".into(),
    };
    let mut idx2 = MapIndex::default();
    idx2.insert_surname(
        "陈",
        cand_with(
            "person:czm",
            Via::CanonicalName,
            &[],
            &["a", "b", "c", "d"],
            &["Projects"],
        ),
    );
    let r = rank(&idx2, &m, &Identity::new(), &ctx);
    assert_eq!(r.ranked[0].0, 7000);
    assert_eq!(
        resolve_scored(&idx2, &m, &Identity::new(), &ctx),
        Resolution::Create
    );
    // Only people get title-free forms.
    let proj = EntityRef::new(EntityType::Project, "陈总");
    assert!(
        rank(&idx, &proj, &Identity::new(), &Context::default())
            .ranked
            .is_empty()
    );
}

#[test]
fn step4_context_raises_scores_up_to_the_caps() {
    let m = person("小陈");
    let neigh: Vec<String> = (0..5).map(|i| format!("n{i}")).collect();
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        Candidate {
            neighbors: neigh.iter().cloned().collect(),
            dirs: ["Projects/x".to_owned()].into(),
            ..Candidate::new("person:a", Via::AliasDraft)
        },
    );
    // No context: a draft alias alone is 7500, a new entity is created.
    assert_eq!(resolve_one(&idx, &m), Resolution::Create);
    let mk = |n: usize, dir: &str| Context {
        keys: neigh.iter().take(n).cloned().collect(),
        dir: dir.into(),
    };
    let score = |ctx: &Context| rank(&idx, &m, &Identity::new(), ctx).ranked[0].0;
    assert_eq!(score(&mk(1, "")), 8000);
    assert_eq!(score(&mk(2, "")), 8500);
    assert_eq!(score(&mk(5, "")), 9000, "capped at +1500");
    assert_eq!(score(&mk(0, "Projects/x")), 8000);
    assert_eq!(score(&mk(5, "Projects/x")), 9500);
    assert_eq!(score(&mk(0, "Projects/y")), 7500);
    assert_eq!(
        resolve_scored(&idx, &m, &Identity::new(), &mk(2, "")),
        Resolution::Bind("person:a".into())
    );
}

#[test]
fn step5_context_breaks_a_tie() {
    let m = person("小陈");
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand_with("person:a", Via::AliasPublished, &[], &["p1", "p2"], &[]),
    );
    idx.insert(
        EntityType::Person,
        None,
        &m.norm(),
        cand("person:b", Via::AliasPublished),
    );
    // Without context: tie, ambiguous.
    assert_eq!(
        resolve_one(&idx, &m),
        Resolution::Ambiguous(vec!["person:a".into(), "person:b".into()])
    );
    // Two shared neighbours: +1000 is exactly the lead that binds.
    let ctx = Context {
        keys: ["p1", "p2"].iter().map(|s| (*s).to_owned()).collect(),
        dir: String::new(),
    };
    assert_eq!(
        resolve_scored(&idx, &m, &Identity::new(), &ctx),
        Resolution::Bind("person:a".into())
    );
}

#[test]
fn bind_hints_only_bind_never_create() {
    use crate::extract::model::{BindHint, EntityMention, Predicate};
    let doc = EntityRef::new(EntityType::Document, "W22.pptx");
    let x = Extraction {
        path: "Weekly/W22.pptx".into(),
        entities: vec![EntityMention {
            entity: doc.clone(),
            evidence: vec![ev("5")],
        }],
        hints: vec![
            BindHint {
                subject: doc.clone(),
                predicate: Predicate::About,
                target_ty: EntityType::Project,
                target_name: "星图看板".into(),
                confidence_bp: 7000,
                rule_id: "pptx.title",
                evidence: vec![ev("5")],
            },
            BindHint {
                subject: doc.clone(),
                predicate: Predicate::About,
                target_ty: EntityType::Project,
                target_name: "不存在".into(),
                confidence_bp: 7000,
                rule_id: "pptx.title",
                evidence: vec![ev("5")],
            },
        ],
        ..Extraction::default()
    };
    let mut idx = MapIndex::default();
    idx.insert(
        EntityType::Project,
        None,
        &norm("星图看板"),
        cand("project:xtkb", Via::CanonicalName),
    );
    let r = resolve_extraction(&idx, &x);
    assert_eq!(r.facts.len(), 1);
    assert_eq!(r.facts[0].object_key.as_deref(), Some("project:xtkb"));
    assert_eq!(r.facts[0].rule_id, "pptx.title");
    assert!(
        r.entities
            .iter()
            .any(|e| e.key == "project:xtkb" && e.bound && !e.evidence.is_empty())
    );
    assert!(r.entities.iter().all(|e| e.name != "不存在"));
}

#[test]
fn new_people_get_title_free_alias_candidates() {
    use crate::extract::model::EntityMention;
    let m = person("陈子墨老师");
    let x = Extraction {
        path: "a.md".into(),
        entities: vec![EntityMention {
            entity: m,
            evidence: vec![ev("6")],
        }],
        ..Extraction::default()
    };
    let r = resolve_extraction(&MapIndex::default(), &x);
    assert_eq!(r.aliases.len(), 1);
    assert_eq!(r.aliases[0].norm, "陈子墨");
    assert_eq!(
        (r.aliases[0].confidence_bp, r.aliases[0].rule_id),
        (5000, "resolve.honorific")
    );
    assert!(!r.aliases[0].evidence.is_empty());
}

fn permuted(x: &Extraction, seed: &[usize]) -> Extraction {
    fn rot<T: Clone>(v: &[T], k: usize) -> Vec<T> {
        if v.is_empty() {
            return Vec::new();
        }
        let k = k % v.len();
        v[k..].iter().chain(v[..k].iter()).cloned().collect()
    }
    let mut y = x.clone();
    y.entities = rot(&y.entities, seed[0]);
    y.entities.reverse();
    y.aliases = rot(&y.aliases, seed[1]);
    y.facts = rot(&y.facts, seed[2]);
    y.facts.reverse();
    y.identities = rot(&y.identities, seed[3]);
    y
}

proptest! {
    // 5.4: "same input in any order gives the same result".
    #[test]
    fn resolve_is_invariant_under_permutation(
        names in proptest::collection::vec(0usize..6, 1..8),
        ids in proptest::collection::vec(proptest::option::of(0usize..3), 8),
        stored in proptest::collection::vec((0usize..6, 0usize..5, 0usize..3), 0..8),
        seed in proptest::collection::vec(0usize..16, 4),
        cand_perm in Just((0..8usize).collect::<Vec<_>>()).prop_shuffle(),
    ) {
        use crate::extract::model::{EntityMention, FactMention, IdentityMention, Predicate};
        let pool = ["陈总", "陈子墨", "老陈", "小陈", "周若溪", "Ethan", "Evan"];
        let emails = ["a@x.io", "b@x.io", "c@x.io"];
        let mut entities = Vec::new();
        let mut identities = Vec::new();
        let mut facts = Vec::new();
        for (i, n) in names.iter().enumerate() {
            let m = person(pool[*n]);
            entities.push(EntityMention { entity: m.clone(), evidence: vec![ev(&format!("e{i}"))] });
            if let Some(e) = ids[i % ids.len()] {
                identities.push(IdentityMention {
                    entity: m.clone(), kind: "email", value: emails[e].into(),
                    confidence_bp: 9000, rule_id: "text.email", evidence: vec![ev(&format!("i{i}"))],
                });
            }
            facts.push(FactMention {
                subject: m, predicate: Predicate::MemberOf,
                object: Some(EntityRef::new(EntityType::Organization, "研发")),
                value: None, valid_from: None, valid_to: None, valid_precision: None,
                confidence_bp: 9500, rule_id: "xlsx.people", evidence: vec![ev(&format!("f{i}"))],
            });
        }
        let x = Extraction { path: "People/a.md".into(), entities, facts, identities, ..Extraction::default() };
        let vias = [Via::CanonicalName, Via::AliasPublished, Via::AliasDraft, Via::Honorific];
        let all: Vec<(usize, Candidate)> = stored.iter().enumerate().map(|(i, (n, v, e))| {
            let id: &[(&str, &str)] = if *e == 0 { &[] } else { &[("email", emails[*e - 1 + (*v % 2)])] };
            (*n, cand_with(&format!("person:s{i}"), vias[*v % 4], id, &["research"], &["People"]))
        }).collect();
        let build = |order: &[usize]| {
            let mut idx = MapIndex::default();
            for &o in order {
                if let Some((n, c)) = all.get(o) {
                    idx.insert(EntityType::Person, None, &norm(pool[*n]), c.clone());
                    for (k, v) in &c.identity {
                        idx.insert_identity(EntityType::Person, k, v, c.clone());
                    }
                    if *n == 1 { idx.insert_surname("陈", c.clone()); }
                }
            }
            idx
        };
        let natural: Vec<usize> = (0..8).collect();
        let a = resolve_extraction(&build(&natural), &x);
        let b = resolve_extraction(&build(&cand_perm), &permuted(&x, &seed));
        prop_assert_eq!(a, b);
    }
}

#[test]
fn unused_import_guard() {
    let _: BTreeMap<u8, u8> = BTreeMap::new();
}
