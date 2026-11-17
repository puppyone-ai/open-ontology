//! B-S6-01, layer 1 and the structure of layers 2 to 4 of 10 §7.2: a plain
//! text scan of `src/compile`, so a change that lets a renderer reach the
//! store, or drops a guard, fails in CI without a database.

use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/compile")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Strip `//` comments and string literals' content is kept: the banned words
/// must not appear even in strings of the pure modules.
fn code(name: &str) -> String {
    read(name)
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

const PURE: [&str; 11] = [
    "brief.rs",
    "pack.rs",
    "render.rs",
    "sections.rs",
    "coverage.rs",
    "leakguard.rs",
    "source.rs",
    "tokens.rs",
    "visible.rs",
    "reader.rs",
    "persist_free_marker.rs",
];

#[test]
fn renderers_cannot_reach_the_store() {
    for name in PURE.iter().filter(|n| **n != "persist_free_marker.rs") {
        let text = code(name);
        for banned in [
            "sqlx",
            "PgConnection",
            "PgPool",
            "Store",
            "store::",
            "crate::cas",
            "Cas::",
            "_oo_docs",
        ] {
            assert!(!text.contains(banned), "{name} mentions {banned}");
        }
    }
}

#[test]
fn only_visible_calls_the_reader() {
    for name in [
        "brief.rs",
        "pack.rs",
        "render.rs",
        "sections.rs",
        "coverage.rs",
        "leakguard.rs",
    ] {
        let text = code(name);
        for banned in ["OntologyReader", "MemoryReader", ".scoped("] {
            assert!(!text.contains(banned), "{name} mentions {banned}");
        }
    }
}

#[test]
fn the_database_reader_cuts_evidence_in_sql_and_never_reads_blocked_evidence() {
    let pg = read("pg.rs");
    // Visible evidence is restricted by the in-scope version id list.
    assert!(pg.contains("e.version_id = ANY($3::text[]::uuid[])"));
    // The held-back queries only test existence (no evidence columns).
    let blocked = pg
        .split("What was held back: existence tests only.")
        .nth(1)
        .expect("blocked section");
    let blocked = blocked
        .split("Open constraint violations")
        .next()
        .unwrap_or("");
    assert!(blocked.contains("NOT EXISTS"));
    assert!(
        !blocked.contains("e.anchor"),
        "blocked queries must not read anchors"
    );
    assert!(!blocked.contains("e.block_id"));
    // Conflict sides read only in-scope evidence.
    assert!(pg.contains("e.version_id = ANY($2::text[]::uuid[])"));
}

#[test]
fn the_guards_run_before_anything_is_returned() {
    let c = code("../compile.rs");
    let lg = c.find("leakguard::leak_guard").expect("leak guard call");
    let pv = c
        .find("leakguard::provenance_gaps")
        .expect("provenance call");
    let ret = c.rfind("Ok(CompiledPack").expect("return");
    assert!(pv < ret && lg < ret);
    assert!(
        !c.contains("persist::"),
        "persisting belongs to the API layer"
    );
}

#[test]
fn refs_snippets_come_from_the_visible_graph_only() {
    let p = code("pack.rs");
    assert!(p.contains(".quote("));
    // Nothing else produces quote text.
    for name in ["brief.rs", "render.rs", "sections.rs"] {
        assert!(!code(name).contains(".quote("), "{name}");
    }
}

#[test]
fn every_markdown_writer_goes_through_md_file() {
    // D66: the label and the front matter come from one function; no module
    // builds a Markdown file by hand.
    for name in ["brief.rs", "pack.rs"] {
        let t = code(name);
        assert!(
            !t.contains("AIGC_LABEL_LINE"),
            "{name} writes the label itself"
        );
        assert!(t.contains("md_file("), "{name}");
    }
    let r = code("render.rs");
    assert!(r.contains("pub fn md_file"));
}
