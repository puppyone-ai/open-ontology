//! The structure layer on the synthetic record corpus (D93), without a database: every file is parsed by the local
//! parsers, observed, and the classes are induced. With `OO_STRUCTURE_OUT=<dir>` the result is written as
//! `structure.json` for `tools/super-ontology/structure/score.py`, which scores it against the frozen gold.

use std::path::PathBuf;

use oo_docs::parse::csv::parse_csv;
use oo_docs::parse::markdown::parse_markdown;
use oo_docs::structure::induce::{FileRow, induce};
use oo_docs::structure::observe::{FileObs, observe};
use oo_docs::structure::vtype::Lexicon;
use oo_docs::structure::yaml;
use oo_docs::structure::{ClassStructure, Status};
use serde_json::json;

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testing/super-ontology/structure/project")
}

fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, base, out);
        } else {
            out.push(p.strip_prefix(base).unwrap().to_string_lossy().into_owned());
        }
    }
}

fn observed() -> (Vec<(String, FileObs)>, Vec<String>) {
    let base = corpus();
    let mut paths = Vec::new();
    walk(&base, &base, &mut paths);
    let mut out = Vec::new();
    let mut bare = Vec::new();
    for p in paths {
        let bytes = std::fs::read(base.join(&p)).unwrap();
        let ir = if p.ends_with(".csv") {
            parse_csv(&bytes).unwrap().ir
        } else {
            parse_markdown(&bytes).unwrap().ir
        };
        match observe(&ir) {
            Some(o) => out.push((p, o)),
            None => bare.push(p),
        }
    }
    (out, bare)
}

fn induced() -> (Vec<ClassStructure>, Vec<String>, usize) {
    let (obs, bare) = observed();
    let rows: Vec<FileRow<'_>> = obs.iter().map(|(p, o)| (p.as_str(), o)).collect();
    let r = induce(&rows, &Lexicon::new());
    let mut unclustered = r.unclustered;
    unclustered.extend(bare.iter().cloned());
    unclustered.sort();
    (r.classes, unclustered, obs.len() + bare.len())
}

#[test]
fn the_record_corpus_gives_six_classes() {
    let (classes, unclustered, observed) = induced();
    let kinds: Vec<&str> = classes.iter().map(|c| c.kind.as_str()).collect();
    for k in [
        "meeting_minutes",
        "decision_log",
        "weekly_report",
        "people_table",
        "project_table",
        "contract",
    ] {
        assert!(kinds.contains(&k), "class {k} missing in {kinds:?}");
    }
    assert!(observed >= 60);
    assert!(!unclustered.is_empty(), "the free-form notes fit no class");
    if let Ok(out) = std::env::var("OO_STRUCTURE_OUT") {
        let mut classes = classes.clone();
        for c in &mut classes {
            c.status = Status::Confirmed;
        }
        let y = yaml::render(1, &classes, None);
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            PathBuf::from(&out).join("structure.json"),
            serde_json::to_string_pretty(
                &json!({"classes": classes, "unclustered": unclustered, "observed": observed}),
            )
            .unwrap(),
        )
        .unwrap();
        std::fs::write(PathBuf::from(&out).join("structure.yaml"), y).unwrap();
    }
}
