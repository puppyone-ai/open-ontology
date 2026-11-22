//! A SQL string that is continued over lines ends each line with ONE backslash, which Rust swallows with the newline. Two
//! backslashes (`\\`) put a literal backslash into the statement and PostgreSQL answers 42601 at the first `\`; that broke
//! `pump_once` (every stage query runs in it) while no default test touched it, because the tests that run the pump need a
//! database. This scan needs none, so the default `cargo test` catches it.

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_continued_string_line_ends_in_two_backslashes() {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(files.len() > 50, "the scan found the sources");
    let mut bad = Vec::new();
    for file in files {
        for (n, line) in std::fs::read_to_string(&file).unwrap().lines().enumerate() {
            let code = line.trim_end();
            if code.ends_with("\\\\") && !code.trim_start().starts_with("//") {
                bad.push(format!("{}:{}", file.display(), n + 1));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "a string continued with two backslashes puts a backslash into the SQL: {bad:?}"
    );
}

/// The scan itself catches the bug it names.
#[test]
fn the_scan_rule_flags_the_broken_line() {
    let broken = "\"SELECT 1 \\\\";
    assert!(broken.trim_end().ends_with("\\\\"));
    let good = "\"SELECT 1 \\";
    assert!(!good.trim_end().ends_with("\\\\"));
}
