//! State machine contract (10 §12.4, L3 cross-cutting gate).
//!
//! Four lists must be equal, name for name:
//! 1. `oo_docs::state::FileState::ALL` (the engine constants);
//! 2. `contracts/docs-api/docs-api.v1.schema.json#/$defs/FileState`;
//! 3. the state names of 00 section 5.1, hard coded below;
//! 4. the CHECK constraint on `_oo_docs.file.state` in migration 0138.
//!
//! And the engine's transition table (`state::is_legal`) must equal, pair for
//! pair, the table of 10 section 4.2, hard coded below with the engine's
//! readings of the loose rows marked.

use std::collections::BTreeSet;
use std::path::PathBuf;

use oo_docs::state::{FileState, is_legal};
use serde_json::Value;

/// 00 section 5.1, in the order the diagram and the prose name them.
const SPEC_STATES: [&str; 16] = [
    // main path (uploading is client side only and not a stored state)
    "stored",
    "queued",
    "parsing",
    "parsed",
    "extracting",
    "indexed",
    "published",
    // side branches
    "parse_failed",
    "extract_partial",
    "unsupported",
    "too_large",
    "encrypted",
    "duplicate",
    // lifecycle
    "superseded",
    "deleted",
    "outdated",
];

/// 10 section 4.2. `(from, to)`; `None` is file creation.
fn spec_edges() -> BTreeSet<(Option<&'static str>, &'static str)> {
    let mut e: BTreeSet<(Option<&str>, &str)> = BTreeSet::new();
    let mut add = |from: Option<&'static str>, tos: &[&'static str]| {
        for to in tos {
            e.insert((from, *to));
        }
    };
    add(None, &["stored"]);
    add(
        Some("stored"),
        // parse_failed: 4.3.2, an unreadable object never reaches queued.
        &[
            "too_large",
            "unsupported",
            "queued",
            "duplicate",
            "parse_failed",
        ],
    );
    add(Some("queued"), &["unsupported", "encrypted", "parsing"]);
    add(Some("parsing"), &["parsed", "queued", "parse_failed"]);
    add(Some("parsed"), &["extracting"]);
    add(Some("extracting"), &["indexed", "extract_partial"]);
    add(Some("indexed"), &["published"]);
    add(Some("extract_partial"), &["published", "queued"]);
    add(Some("parse_failed"), &["queued"]);
    add(Some("outdated"), &["parsing"]);
    // "published etc. terminal -> outdated": the engine reads "etc." as every
    // state that holds a finished or settled result.
    for from in [
        "published",
        "indexed",
        "extract_partial",
        "duplicate",
        "parse_failed",
        "unsupported",
    ] {
        add(Some(from), &["outdated"]);
    }
    // "any active state -> superseded", "any -> deleted": every state except
    // the target itself; a superseded version can still be deleted.
    for from in SPEC_STATES {
        if from != "superseded" && from != "deleted" {
            add(Some(from), &["superseded", "deleted"]);
        }
    }
    add(Some("superseded"), &["deleted", "stored"]);
    // A deleted path that comes back starts again at stored.
    add(Some("deleted"), &["stored"]);
    e
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn names(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn engine_states_equal_the_contract_enum() {
    let engine: BTreeSet<String> = FileState::ALL
        .iter()
        .map(|s| s.as_str().to_owned())
        .collect();
    assert_eq!(engine.len(), FileState::ALL.len(), "no duplicate names");
    let schema: Value =
        serde_json::from_str(&read("contracts/docs-api/docs-api.v1.schema.json")).unwrap();
    let contract: BTreeSet<String> = schema["$defs"]["FileState"]["enum"]
        .as_array()
        .expect("FileState enum")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(engine, contract, "FileState: engine vs docs-api schema");
}

#[test]
fn engine_states_equal_the_spec_list() {
    let engine: BTreeSet<String> = FileState::ALL
        .iter()
        .map(|s| s.as_str().to_owned())
        .collect();
    assert_eq!(engine, names(&SPEC_STATES), "FileState vs 00 section 5.1");
    for s in SPEC_STATES {
        assert_eq!(FileState::parse(s).map(FileState::as_str), Some(s));
    }
    assert!(FileState::parse("uploading").is_none(), "client side only");
}

#[test]
fn engine_states_equal_the_database_check() {
    let sql = read("crates/oo-pipeline/migrations/0138_docs_sources_jobs.sql");
    let table = sql
        .split("CREATE TABLE _oo_docs.file (")
        .nth(1)
        .expect("file table");
    let check = table
        .split("state text NOT NULL CHECK (state IN (")
        .nth(1)
        .expect("state check");
    let list = check.split("))").next().unwrap();
    let db: BTreeSet<String> = list
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    let engine: BTreeSet<String> = FileState::ALL
        .iter()
        .map(|s| s.as_str().to_owned())
        .collect();
    assert_eq!(engine, db, "FileState vs _oo_docs.file CHECK");
}

#[test]
fn transition_table_equals_the_spec_table() {
    let mut engine: BTreeSet<(Option<&str>, &str)> = BTreeSet::new();
    for to in FileState::ALL {
        if is_legal(None, to) {
            engine.insert((None, to.as_str()));
        }
        for from in FileState::ALL {
            if is_legal(Some(from), to) {
                engine.insert((Some(from.as_str()), to.as_str()));
            }
        }
    }
    let spec = spec_edges();
    let extra: Vec<_> = engine.difference(&spec).collect();
    let missing: Vec<_> = spec.difference(&engine).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "engine has extra {extra:?}; engine lacks {missing:?}"
    );
}

#[test]
fn every_state_is_reachable_and_the_rest_has_no_dead_ends() {
    // From creation, following legal edges, every state is reachable.
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut todo: Vec<FileState> = FileState::ALL
        .into_iter()
        .filter(|s| is_legal(None, *s))
        .collect();
    while let Some(s) = todo.pop() {
        if !seen.insert(s.as_str()) {
            continue;
        }
        todo.extend(FileState::ALL.into_iter().filter(|t| is_legal(Some(s), *t)));
    }
    assert_eq!(seen.len(), FileState::ALL.len(), "unreachable: {seen:?}");
    // Every state has a way out (even terminal-looking ones: lifecycle edges).
    for s in FileState::ALL {
        assert!(
            FileState::ALL.into_iter().any(|t| is_legal(Some(s), t)),
            "{} has no outgoing transition",
            s.as_str()
        );
    }
    // Finished paths do not re-enter the middle of the pipeline on their own.
    assert!(!is_legal(Some(FileState::Published), FileState::Parsing));
    assert!(!is_legal(Some(FileState::Encrypted), FileState::Queued));
    assert!(!is_legal(Some(FileState::TooLarge), FileState::Queued));
    // The manual retry edges are the only way out of the failure states.
    assert!(is_legal(Some(FileState::ParseFailed), FileState::Queued));
    assert!(is_legal(Some(FileState::ExtractPartial), FileState::Queued));
}
