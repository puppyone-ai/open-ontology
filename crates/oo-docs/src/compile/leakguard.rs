//! DOC-G6 (leak guard) and DOC-G7 (provenance) on the rendered bytes, the third
//! and fourth layers of the visibility guarantee (10 §7.2). They see the
//! output files and the [`VisibleGraph`] only, never the store.

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization;

use super::pack::PackFile;
use super::source;
use super::visible::{self, VisibleGraph};

fn scan_text(s: &str) -> String {
    s.nfkc().collect::<String>().to_lowercase()
}

/// DOC-G6: no blocked word may appear in any output byte unless the scope's
/// own text says it. Returns the number of offending words (never the words).
pub(super) fn leak_guard(files: &[PackFile], terms: &[String], g: &VisibleGraph) -> usize {
    let scope_text: Vec<String> = g.scope_texts().iter().map(|t| scan_text(t)).collect();
    let outputs: Vec<String> = files.iter().map(|f| scan_text(f.as_str())).collect();
    let mut hits = 0;
    for term in terms {
        let t = crate::resolve::normalize::norm(term);
        if t.chars().count() < 2 {
            continue;
        }
        if outputs.iter().any(|o| o.contains(&t)) && !scope_text.iter().any(|s| s.contains(&t)) {
            hits += 1;
        }
    }
    hits
}

/// DOC-G7: every content line (`- ...`) of BRIEF and of the entity files
/// carries at least one tag, and every tag names in-scope evidence.
pub(super) fn provenance_gaps(files: &[PackFile], g: &VisibleGraph) -> usize {
    let mut allowed: BTreeSet<(String, String)> = BTreeSet::new();
    let mut add = |e: &visible::VisibleEvidence| {
        allowed.insert((e.path.clone(), source::frag(&e.anchor)));
    };
    for f in g.facts() {
        f.evidence.iter().for_each(&mut add);
    }
    for e in g.entities() {
        for a in g.aliases(&e.key) {
            a.evidence.iter().for_each(&mut add);
        }
    }
    // 工作约定 (D94): the blocks of the instruction files in scope are sources too.
    for f in g.snapshot() {
        if crate::profile::classify_path(&f.path, None).profile
            == crate::profile::Profile::Instruction
        {
            for (_, _, anchor) in g.file_blocks(&f.version_id) {
                allowed.insert((f.path.clone(), source::frag(anchor)));
            }
        }
    }
    let mut bad = 0;
    for f in files {
        let checked = f.path == "BRIEF.md"
            || f.path.starts_with("people/")
            || f.path.starts_with("projects/")
            || f.path.starts_with("decisions")
            || f.path.starts_with("terms");
        if !checked {
            continue;
        }
        for line in f.as_str().lines().filter(|l| l.starts_with("- ")) {
            let tags = source::tags_in(line);
            let mut ok = !tags.is_empty() && !tags[0].starts_with('+');
            for t in &tags {
                if let Some(n) = t.strip_prefix('+') {
                    ok &= n.parse::<u32>().is_ok();
                } else {
                    ok &= source::split_tag(t).is_some_and(|k| allowed.contains(&k));
                }
            }
            if !ok {
                bad += 1;
            }
        }
    }
    bad
}
