//! FM8 conformance (80 section 4, card V-S5-02/V-S6-03): the visibility model
//! `formal/quint/visibility.qnt` against `compile/visible.rs` and the pack
//! compiler.
//!
//! Every ITF trace of the model carries a graph (4 files, 6 entities, 10 facts,
//! 6 aliases, withdrawn evidence, human facts), a scope and the model's pack
//! (`out`). Per state this test builds the same graph in a [`MemoryReader`],
//! loads it through [`VisibleGraph::load`] (the single exit of D26), expands the
//! seeds one hop with the accessors BRIEF uses, and compares with the model:
//! the facts, the evidence files of each fact, the entities, the aliases and the
//! two blocked counts. It then compiles a whole pack for the visible seeds and
//! scans every output byte for the names that exist only out of scope.
//!
//! No database is needed:
//!
//! ```text
//! cargo test -p oo-docs --locked --test conformance_visibility
//! OO_FM8_TRACES_DIR=/tmp/fm8-traces cargo test ... (bulk: formal/scripts/formal-traces visibility visibility 2000 6 0x464d /tmp/fm8-traces)
//! ```
//!
//! Not covered: the leak guard's word scan on real text (the red-team gate
//! `gates_live.rs` does that), and what the SQL reader does with the same graph
//! (`pg.rs` is checked by `gates_live.rs` and `share_live.rs`; the model graph is
//! small enough to be planted there, which this test does not do).

#[path = "itf_support/mod.rs"]
mod itf;

use std::collections::{BTreeMap, BTreeSet};

use itf::*;
use oo_core::docir::Anchor;
use oo_docs::compile::reader::{
    BlockText, MemAlias, MemEvidence, MemFact, MemFile, MemoryReader, OntologyReader, RawAlias,
    RawEntity, RawEvidence, RawFact, SnapshotFile, VersionInfo,
};
use oo_docs::compile::visible::VisibleGraph;
use oo_docs::compile::{CompileRequest, compile};
use oo_docs::extract::model::{EntityType, Predicate};
use serde_json::{Value, json};

const FILES: i64 = 4;

fn vid(f: i64) -> String {
    format!("00000000-0000-4000-8000-{f:012x}")
}
fn path_of(f: i64) -> String {
    if f == FILES {
        "d/zz.md".to_owned()
    } else {
        format!("d/f{f}.md")
    }
}
fn ent_name(e: i64) -> String {
    format!("Ent{e}Qz")
}
fn alias_name(a: i64) -> String {
    format!("Alias{a}Qz")
}

struct Graph {
    subj: BTreeMap<i64, i64>,
    obj: BTreeMap<i64, i64>,
    ev: BTreeMap<i64, BTreeSet<i64>>,
    wd: BTreeMap<i64, BTreeSet<i64>>,
    human: BTreeMap<i64, bool>,
    aowner: BTreeMap<i64, i64>,
    aev: BTreeMap<i64, BTreeSet<i64>>,
    awd: BTreeMap<i64, BTreeSet<i64>>,
}

fn graph(g: &Value) -> Graph {
    Graph {
        subj: map_ints(&g["subj"]),
        obj: map_ints(&g["obj"]),
        ev: map_sets(&g["ev"]),
        wd: map_sets(&g["wd"]),
        human: map_bools(&g["human"]),
        aowner: map_ints(&g["aowner"]),
        aev: map_sets(&g["aev"]),
        awd: map_sets(&g["awd"]),
    }
}

fn anchor() -> Anchor {
    Anchor::Text {
        line_start: 1,
        line_end: 1,
        char_start: 0,
        char_end: 4,
    }
}

fn evidence(files: &BTreeSet<i64>, wd: &BTreeSet<i64>, block: &str) -> Vec<MemEvidence> {
    files
        .iter()
        .map(|f| MemEvidence {
            evidence: RawEvidence {
                version_id: vid(*f),
                block_id: block.to_owned(),
                anchor: anchor(),
            },
            withdrawn: wd.contains(f),
        })
        .collect()
}

fn reader(g: &Graph) -> MemoryReader {
    let mut r = MemoryReader::default();
    r.versions.push(VersionInfo {
        n: 1,
        as_of: "2026-11-17".into(),
    });
    for f in 0..=FILES {
        r.files.push(MemFile {
            file: SnapshotFile {
                path: path_of(f),
                b3: format!("b3-f{f}"),
                version_id: vid(f),
            },
            since: 1,
            until: None,
        });
        // a block for every fact and alias, so every evidence has a quote
        let mut blocks = Vec::new();
        for i in 0..10 {
            blocks.push(BlockText {
                block_id: format!("bf{i}"),
                ordinal: u32::try_from(i).unwrap(),
                text: "Quoted text without names.".into(),
            });
        }
        for i in 0..6 {
            blocks.push(BlockText {
                block_id: format!("ba{i}"),
                ordinal: 10 + u32::try_from(i).unwrap(),
                text: "Quoted text without names.".into(),
            });
        }
        r.blocks.insert(vid(f), blocks);
    }
    for e in 0..6 {
        r.entities.push(RawEntity {
            key: format!("e{e}"),
            ty: EntityType::Person,
            subkind: None,
            thing_label: None,
            name: ent_name(e),
        });
    }
    for (f, s) in &g.subj {
        let o = g.obj[f];
        let (predicate, object_key, value) = if o < 0 {
            (Predicate::Attr("note".into()), None, Some(json!("v")))
        } else {
            (Predicate::Mentions, Some(format!("e{o}")), None)
        };
        r.facts.push(MemFact {
            fact: RawFact {
                fact_key: format!("fact{f}"),
                subject_key: format!("e{s}"),
                predicate,
                object_key,
                value,
                valid_from: None,
                valid_to: None,
                origin: if g.human[f] { "human" } else { "llm" }.into(),
                evidence: Vec::new(),
            },
            published_in: 1,
            retracted_in: None,
            evidence: evidence(&g.ev[f], &g.wd[f], &format!("bf{f}")),
        });
    }
    for (a, owner) in &g.aowner {
        r.aliases.push(MemAlias {
            alias: RawAlias {
                entity_key: format!("e{owner}"),
                alias: alias_name(*a),
                evidence: Vec::new(),
            },
            published_in: 1,
            retracted_in: None,
            evidence: evidence(&g.aev[a], &g.awd[a], &format!("ba{a}")),
        });
    }
    r
}

fn file_of(path: &str) -> i64 {
    path.trim_start_matches("d/f")
        .trim_end_matches(".md")
        .parse()
        .expect("file index")
}
fn index(key: &str, prefix: &str) -> i64 {
    key.trim_start_matches(prefix)
        .trim_end_matches("Qz")
        .parse()
        .expect("index")
}

/// Compare the model's state with the implementation. Returns the number of
/// facts of the model pack that were checked.
fn check_state(g: &Graph, st: &Value, trace: &str, i: usize) -> usize {
    let scope = set_ints(&st["scope"]);
    let seeds = set_ints(&st["seeds"]);
    let out = &st["out"];
    let who = format!("{trace} state {i}");
    let r = reader(g);
    let version = r.latest_version().unwrap();
    // an empty model scope is "nothing": the always-empty file stands for it, because the
    // engine reads an empty prefix list as the whole Drive
    let prefixes: Vec<String> = if scope.is_empty() {
        vec![path_of(FILES)]
    } else {
        scope.iter().map(|f| path_of(*f)).collect()
    };
    let (vg, _terms) = VisibleGraph::load(&r, &version, &prefixes).expect("load");

    // the pack of BRIEF: visible seeds and their one hop of visible facts
    let seeds_vis: BTreeSet<i64> = seeds
        .iter()
        .copied()
        .filter(|e| vg.entity(&format!("e{e}")).is_some())
        .collect();
    let mut facts: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    let mut ents: BTreeSet<i64> = seeds_vis.clone();
    for s in &seeds_vis {
        for f in vg.facts_of(&format!("e{s}")) {
            let fi = index(&f.key, "fact");
            let files: BTreeSet<i64> = f.evidence.iter().map(|e| file_of(&e.path)).collect();
            facts.insert(fi, files);
            ents.insert(index(&f.subject, "e"));
            if let Some(o) = &f.object {
                ents.insert(index(o, "e"));
            }
        }
    }
    let m_facts = set_ints(&out["facts"]);
    assert_eq!(
        facts.keys().copied().collect::<BTreeSet<_>>(),
        m_facts,
        "{who}: pack facts"
    );
    let m_ev = map_sets(&out["ev"]);
    for (f, files) in &facts {
        assert_eq!(files, &m_ev[f], "{who}: evidence files of fact {f}");
    }
    assert_eq!(ents, set_ints(&out["ents"]), "{who}: pack entities");
    let aliases: BTreeSet<i64> = ents
        .iter()
        .flat_map(|e| vg.aliases(&format!("e{e}")))
        .map(|a| index(&a.alias, "Alias"))
        .collect();
    assert_eq!(aliases, set_ints(&out["aliases"]), "{who}: pack aliases");
    let b = vg.blocked();
    assert_eq!(
        (b.facts, b.entities),
        (
            u64::try_from(int(&out["blockedF"])).unwrap(),
            u64::try_from(int(&out["blockedE"])).unwrap()
        ),
        "{who}: blocked counts"
    );
    // human overlay facts get no back door: a human fact with no in-scope live evidence is not visible
    for (f, h) in &g.human {
        let in_scope = g.ev[f].difference(&g.wd[f]).any(|x| scope.contains(x));
        if *h && !in_scope {
            assert!(
                vg.facts().all(|x| x.key != format!("fact{f}")),
                "{who}: human fact {f} visible without evidence in scope"
            );
        }
    }

    // a whole pack for the visible seeds: nothing that exists only out of scope is in any byte
    if !scope.is_empty() {
        let visible_ents: BTreeSet<i64> = (0..6)
            .filter(|e| vg.entity(&format!("e{e}")).is_some())
            .collect();
        let visible_aliases: BTreeSet<i64> = (0..6)
            .filter(|e| vg.entity(&format!("e{e}")).is_some())
            .flat_map(|e| vg.aliases(&format!("e{e}")))
            .map(|a| index(&a.alias, "Alias"))
            .collect();
        let req = CompileRequest {
            scope: prefixes.clone(),
            seeds: seeds_vis.iter().map(|e| format!("e{e}")).collect(),
            budget: 20_000,
            ..CompileRequest::default()
        };
        let pack = match compile(&r, &req) {
            Ok(p) => p,
            // nothing in scope is visible: the compiler refuses to write an empty pack
            Err(oo_docs::compile::PackError::NothingVisible) => {
                assert!(
                    vg.facts().next().is_none() && vg.entities().next().is_none(),
                    "{who}: NothingVisible but the graph shows something"
                );
                return facts.len();
            }
            Err(e) => panic!("{who}: compile: {e:?}"),
        };
        assert_eq!(
            (pack.blocked.facts, pack.blocked.entities),
            (b.facts, b.entities),
            "{who}: pack blocked counts"
        );
        for f in &pack.files {
            let text = String::from_utf8_lossy(&f.bytes);
            for e in 0..6 {
                if !visible_ents.contains(&e) {
                    assert!(
                        !text.contains(&ent_name(e)),
                        "{who}: out-of-scope entity {e} in {}",
                        f.path
                    );
                }
                if !visible_aliases.contains(&e) {
                    assert!(
                        !text.contains(&alias_name(e)),
                        "{who}: out-of-scope alias {e} in {}",
                        f.path
                    );
                }
            }
        }
    }
    facts.len()
}

#[test]
fn fm8_itf_traces_conform_to_the_visible_graph() {
    let traces = load_traces("visibility", "OO_FM8_TRACES_DIR");
    let (mut states, mut packed, mut with_blocked) = (0usize, 0usize, 0usize);
    for (name, t) in &traces {
        let sts = t["states"].as_array().expect("states");
        let g = graph(&sts[0]["g"]);
        for (i, st) in sts.iter().enumerate() {
            packed += check_state(&g, st, name, i);
            if int(&st["out"]["blockedF"]) > 0 {
                with_blocked += 1;
            }
            states += 1;
        }
    }
    eprintln!(
        "FM8 conformance: {} traces, {states} states, {packed} pack facts compared, {with_blocked} states with blocked facts",
        traces.len()
    );
    assert!(
        packed > 0 && with_blocked > 0,
        "the traces exercise nothing"
    );
}

#[test]
fn fm8_same_scope_different_read_order_is_the_same_graph() {
    // determinism (D27) on the model's own graphs: the graph is read from a shuffled reader
    let traces = load_traces("visibility", "OO_FM8_TRACES_DIR");
    for (name, t) in traces.iter().take(8) {
        let sts = t["states"].as_array().unwrap();
        let g = graph(&sts[0]["g"]);
        let scope = set_ints(&sts[1]["scope"]);
        if scope.is_empty() {
            continue;
        }
        let prefixes: Vec<String> = scope.iter().map(|f| path_of(*f)).collect();
        let r1 = reader(&g);
        let mut r2 = r1.clone();
        r2.facts.reverse();
        r2.aliases.reverse();
        r2.entities.reverse();
        let v = r1.latest_version().unwrap();
        let (a, _) = VisibleGraph::load(&r1, &v, &prefixes).unwrap();
        let (b, _) = VisibleGraph::load(&r2, &v, &prefixes).unwrap();
        let ka: Vec<&str> = a.facts().map(|f| f.key.as_str()).collect();
        let kb: Vec<&str> = b.facts().map(|f| f.key.as_str()).collect();
        assert_eq!(ka, kb, "{name}");
    }
}
