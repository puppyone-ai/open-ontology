//! D90 guard: the entity quality gate never drops an entity the golden and held-out labels name.
//!
//! The text formats of both labelled sets are read in process (md, txt, csv) and the rules run with and without
//! the gate; every entity of a core type the labels name (by type and canonical name) must survive. Gold and
//! labels are read only. No database, no container.
//!
//! `cargo test -p oo-docs --locked --test quality_gold`

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use oo_docs::extract::model::EntityType;
use oo_docs::extract::rules::{Siblings, extract_ungated, extract_with_quality};
use oo_docs::resolve::normalize::norm;

fn root(set: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testing/super-ontology")
        .join(set)
}

fn labelled(set: &str) -> BTreeSet<(String, String)> {
    let text = std::fs::read_to_string(root(set).join("labels/entities.jsonl")).unwrap();
    let mut out = BTreeSet::new();
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        if v["kind"] == "entity" {
            let mut names = vec![v["name"].as_str().unwrap().to_owned()];
            names.extend(
                v["aliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a.as_str().map(str::to_owned)),
            );
            for n in names {
                out.insert((v["type"].as_str().unwrap().to_owned(), norm(&n)));
            }
        }
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn ir_of(path: &Path) -> Option<oo_core::docir::DocIr> {
    let bytes = std::fs::read(path).ok()?;
    let n = match path.extension()?.to_str()? {
        "md" => oo_docs::parse::markdown::parse_markdown(&bytes),
        "txt" => oo_docs::parse::text::parse_text(&bytes),
        "csv" => oo_docs::parse::csv::parse_csv(&bytes),
        _ => return None,
    };
    Some(n.ok()?.ir)
}

fn check(set: &str) {
    let gold = labelled(set);
    let docs = root(set).join("docs");
    let mut files = Vec::new();
    walk(&docs, &mut files);
    files.sort();
    let (mut seen, mut kept_total, mut dropped_total) = (0, 0usize, 0usize);
    let mut lost: Vec<String> = Vec::new();
    for f in files {
        let Some(ir) = ir_of(&f) else { continue };
        seen += 1;
        let rel = f
            .strip_prefix(&docs)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let before = extract_ungated(&rel, &ir, &Siblings::default());
        let (after, stats) = extract_with_quality(&rel, &ir, &Siblings::default());
        kept_total += after.entities.len();
        dropped_total += stats.entities_dropped() as usize;
        for m in &before.entities {
            let named = (
                match m.entity.ty {
                    EntityType::Decision => "Decision",
                    EntityType::Person => "Person",
                    EntityType::Event => "Event",
                    EntityType::Term => "Term",
                    EntityType::Topic => "Topic",
                    _ => continue,
                }
                .to_owned(),
                m.entity.norm(),
            );
            if gold.contains(&named) && !after.entities.iter().any(|a| a.entity == m.entity) {
                lost.push(format!("{rel}: {} {}", named.0, m.entity.name));
            }
        }
    }
    eprintln!(
        "{set}: {seen} text files, {kept_total} entities kept, {dropped_total} dropped by the gate"
    );
    assert!(seen > 10, "{seen} text files");
    assert!(
        lost.is_empty(),
        "the gate dropped labelled entities: {lost:#?}"
    );
}

#[test]
fn the_golden_set_loses_no_labelled_entity() {
    check("golden");
}

#[test]
fn the_held_out_set_loses_no_labelled_entity() {
    check("holdout");
}
