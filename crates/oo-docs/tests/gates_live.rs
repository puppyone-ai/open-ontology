//! L4 cross-cutting gates on a live database (B-S6-07, 30 section 4): red team
//! (DOC-G6), deterministic replay (DOC-G11) and provenance (DOC-G7).
//!
//! The files are the real golden files, read from `testing/super-ontology/golden`
//! and indexed through the engine's own parsers (md, txt, csv), the rules, the
//! resolver and the publisher; packs come out of `compile_from_store`, the
//! function behind the API. docx, pptx, pdf and xlsx need the sidecar and are
//! not part of this run: the red-team `.docx` is excluded, see the L4 report.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test gates_live -- --ignored --test-threads=1`

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::fx::{DRIVE, Fx, golden};
use oo_docs::compile::{CompileRequest, compile_from_store, source};
use oo_docs::normalize::Normalized;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Index every md, txt and csv file under `root` with `prefix` stripped.
async fn index_tree(fx: &mut Fx, root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut paths = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&f).unwrap();
        let n: Normalized = match f.extension().and_then(|e| e.to_str()) {
            Some("md") => oo_docs::parse::markdown::parse_markdown(&bytes).unwrap(),
            Some("txt") => oo_docs::parse::text::parse_text(&bytes).unwrap(),
            Some("csv") => oo_docs::parse::csv::parse_csv(&bytes).unwrap(),
            _ => continue,
        };
        let p = fx.plant_normalized(&rel, n, bytes.len()).await;
        fx.index_planted(&rel, &p).await;
        paths.push(rel);
    }
    paths
}

async fn world(db: &str, with_redteam: bool) -> (Fx, Vec<String>, Vec<String>) {
    let mut fx = Fx::new(db).await;
    let main = index_tree(&mut fx, &golden().join("docs")).await;
    let red = if with_redteam {
        index_tree(&mut fx, &golden().join("redteam/docs")).await
    } else {
        Vec::new()
    };
    // The rules read little of the red-team prose, so the labelled red-team
    // entities and the canaries are added as model-origin facts whose only
    // evidence is a red-team file (as the model would extract them).
    for path in &red {
        let mut names: Vec<String> = Vec::new();
        for line in std::fs::read_to_string(golden().join("redteam/redteam-entities.jsonl"))
            .unwrap()
            .lines()
        {
            let v: Value = serde_json::from_str(line).unwrap();
            let in_file = v["anchors"].as_array().into_iter().flatten().any(|a| {
                a["file"]
                    .as_str()
                    .is_some_and(|f| f.ends_with(&format!("docs/{path}")))
            });
            if v["redteam_only"] == true && in_file {
                names.extend(v["name"].as_str().map(str::to_owned));
            }
        }
        names.extend(
            std::fs::read_to_string(golden().join("canaries.txt"))
                .unwrap()
                .lines()
                .take(1)
                .map(str::to_owned),
        );
        for n in names {
            fx.llm_facts_named(path, &n, 1, 9500).await;
        }
    }
    let out = fx.publish().await;
    assert!(
        matches!(out, oo_docs::publish::run::PublishOutcome::Published(_)),
        "{out:?}"
    );
    (fx, main, red)
}

fn req(scope: &[String], task: &str) -> CompileRequest {
    CompileRequest {
        scope: scope.to_vec(),
        task: task.to_owned(),
        budget: 1500,
        ..CompileRequest::default()
    }
}

fn scan(s: &str) -> String {
    s.nfkc().collect::<String>().to_lowercase()
}

/// Words that must never appear in a pack of a scope without the red-team files.
fn forbidden() -> Vec<String> {
    let root = golden();
    let mut out: Vec<String> = std::fs::read_to_string(root.join("canaries.txt"))
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    for line in std::fs::read_to_string(root.join("redteam/redteam-entities.jsonl"))
        .unwrap()
        .lines()
    {
        let v: Value = serde_json::from_str(line).unwrap();
        if v["redteam_only"] == true {
            if let Some(n) = v["name"].as_str() {
                out.push(n.to_owned());
            }
            for a in v["aliases"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if a.chars().count() >= 2 {
                    out.push(a.to_owned());
                }
            }
        }
    }
    out
}

/// Scope prefixes of `scopes.json` that do not contain the red-team files,
/// limited to paths that exist in the Drive.
fn scopes(main: &[String]) -> Vec<(String, Vec<String>)> {
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(golden().join("scopes.json")).unwrap())
            .unwrap();
    let mut out = Vec::new();
    for s in doc["scopes"].as_array().unwrap() {
        if s["contains_redteam"] != false {
            continue;
        }
        let mut prefixes: BTreeSet<String> = BTreeSet::new();
        for inc in s["include"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
        {
            let inc = inc.strip_prefix("docs/").unwrap_or(inc);
            match inc.strip_suffix("/**") {
                Some("") | None if inc == "**" => {
                    for p in main {
                        prefixes.insert(p.split('/').next().unwrap().to_owned());
                    }
                }
                Some(dir) => {
                    prefixes.insert(dir.to_owned());
                }
                None => {
                    prefixes.insert(inc.to_owned());
                }
            }
        }
        if s["include"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i == "docs/**")
        {
            prefixes.clear();
            for p in main {
                prefixes.insert(p.split('/').next().unwrap().to_owned());
            }
        }
        let prefixes: Vec<String> = prefixes
            .into_iter()
            .filter(|p| {
                main.iter()
                    .any(|f| f == p || f.starts_with(&format!("{p}/")))
            })
            .collect();
        if !prefixes.is_empty() {
            out.push((s["id"].as_str().unwrap().to_owned(), prefixes));
        }
    }
    out
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn red_team_files_never_reach_a_pack_of_another_scope() {
    let (mut fx, main, red) = world("gate_redteam", true).await;
    assert!(
        red.len() >= 3,
        "md, txt and csv red-team files indexed: {red:?}"
    );
    let words = forbidden();
    assert!(words.len() >= 8, "{} forbidden words", words.len());
    let all_scopes = scopes(&main);
    assert!(all_scopes.len() >= 3, "{all_scopes:?}");
    let mut packs = 0;
    for (id, prefixes) in &all_scopes {
        // Several tasks per scope: none, a name of the scope, an empty-looking one.
        for task in ["", "写周报", "星图看板的决定和负责人"] {
            let pack =
                match compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &req(prefixes, task)).await {
                    Ok(p) => p,
                    Err(oo_docs::compile::PackError::NothingVisible) => continue,
                    Err(e) => panic!("{id}: {e}"),
                };
            packs += 1;
            for f in &pack.files {
                let text = scan(f.as_str());
                for w in &words {
                    assert!(
                        !text.contains(&scan(w)),
                        "scope {id}: {} contains a red-team word",
                        f.path
                    );
                }
            }
            // No red-team path, either.
            assert!(
                pack.files
                    .iter()
                    .all(|f| !f.as_str().contains("Confidential/")),
                "{id}"
            );
        }
    }
    assert!(packs >= 6, "{packs} packs checked");
    // The test has teeth: a scope that includes the files shows them.
    let with = compile_from_store(
        &mut fx.rt,
        &fx.cas,
        DRIVE,
        &req(&["Confidential".into()], ""),
    )
    .await
    .unwrap();
    let joined: String = with.files.iter().map(|f| scan(f.as_str())).collect();
    assert!(
        words.iter().filter(|w| joined.contains(&scan(w))).count() >= 2,
        "a Confidential scope must show red-team names"
    );
    // The blocked counts are numbers only.
    let one = compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &req(&all_scopes[0].1, ""))
        .await
        .unwrap();
    assert!(one.blocked.entities >= 1);
    fx.finish("gate_redteam").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_same_files_in_two_databases_give_byte_identical_packs() {
    let (mut a, main, _) = world("gate_replay_a", true).await;
    let (mut b, _, _) = world("gate_replay_b", true).await;
    let scope: Vec<String> = vec!["Decisions".into(), "Projects".into(), "People".into()];
    for task in ["", "星图看板的决定", "林远舟负责什么"] {
        let r = req(&scope, task);
        let pa = compile_from_store(&mut a.rt, &a.cas, DRIVE, &r)
            .await
            .unwrap();
        let pb = compile_from_store(&mut b.rt, &b.cas, DRIVE, &r)
            .await
            .unwrap();
        assert_eq!(pa.pack_digest, pb.pack_digest, "task {task:?}");
        assert_eq!(pa.files.len(), pb.files.len());
        for (x, y) in pa.files.iter().zip(&pb.files) {
            assert_eq!((&x.path, &x.bytes), (&y.path, &y.bytes), "{}", x.path);
        }
        // And again in the same database.
        let pc = compile_from_store(&mut a.rt, &a.cas, DRIVE, &r)
            .await
            .unwrap();
        assert_eq!(pa.pack_digest, pc.pack_digest);
        // SHARE.json differs only in the caller's fields.
        let f = |id: &str| oo_docs::compile::pack::ShareFields {
            share_id: id.into(),
            grantor: "owner".into(),
            created_at: "2026-11-17T00:00:00Z".into(),
            expires_at: "2026-11-24T00:00:00Z".into(),
            refs_endpoint: "http://127.0.0.1:1/v1/docs/refs".into(),
            refs_credential: format!("oodr_{id}"),
            refs_expires_at: "2026-11-24T00:00:00Z".into(),
        };
        let (s1, s2) = (pa.share_json(&f("a")), pb.share_json(&f("a")));
        assert_eq!(s1.bytes, s2.bytes);
    }
    // A scope listed in another order is the same pack.
    let rev: Vec<String> = scope.iter().rev().cloned().collect();
    let p1 = compile_from_store(&mut a.rt, &a.cas, DRIVE, &req(&scope, ""))
        .await
        .unwrap();
    let p2 = compile_from_store(&mut a.rt, &a.cas, DRIVE, &req(&rev, ""))
        .await
        .unwrap();
    assert_eq!(p1.pack_digest, p2.pack_digest);
    assert!(!main.is_empty());
    a.finish("gate_replay_a").await;
    b.finish("gate_replay_b").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn every_brief_line_has_a_source_that_resolves_to_an_ir_anchor() {
    let (mut fx, main, _) = world("gate_prov", true).await;
    let mut lines = 0;
    let mut tags = 0;
    for (id, prefixes) in scopes(&main)
        .iter()
        .chain(&[("whole".to_owned(), Vec::<String>::new())])
    {
        let pack =
            match compile_from_store(&mut fx.rt, &fx.cas, DRIVE, &req(prefixes, "星图看板的决定"))
                .await
            {
                Ok(p) => p,
                Err(_) => continue,
            };
        for f in pack.files.iter().filter(|f| {
            f.path == "BRIEF.md"
                || f.path.starts_with("people/")
                || f.path.starts_with("projects/")
                || f.path.starts_with("decisions")
                || f.path.starts_with("terms")
        }) {
            for line in f.as_str().lines().filter(|l| l.starts_with("- ")) {
                lines += 1;
                let found = source::tags_in(line);
                assert!(
                    !found.is_empty(),
                    "scope {id} {}: no [src: in {line}",
                    f.path
                );
                for t in found {
                    if t.starts_with('+') {
                        continue; // a refs pointer, checked by the pack itself
                    }
                    tags += 1;
                    let (path, fragment) =
                        source::split_tag(t).unwrap_or_else(|| panic!("bad tag {t}"));
                    let plant = fx.known_plant(&path).await;
                    let ok = plant
                        .ir
                        .blocks
                        .iter()
                        .any(|b| source::frag(&b.anchor) == fragment);
                    assert!(ok, "scope {id}: {t} resolves to no block of {path}");
                    // The file has to be inside the scope.
                    assert!(
                        prefixes.is_empty()
                            || prefixes
                                .iter()
                                .any(|p| path == *p || path.starts_with(&format!("{p}/"))),
                        "scope {id}: {path} is out of scope"
                    );
                }
            }
        }
    }
    assert!(lines > 20 && tags >= lines, "{lines} lines, {tags} tags");
    eprintln!("provenance gate: {lines} content lines, {tags} tags, all resolved");
    fx.finish("gate_prov").await;
}
