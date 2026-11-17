//! Path rules of the fs backend (spec 10 §4.3.2). Pure functions only, so
//! they stay Kani friendly (80 §3 KP1): no I/O, no panics, no unsafe.
//!
//! Everything here works on Drive-relative paths. The output of
//! [`normalize_raw_key`] is always a valid [`DocPath`], which cannot contain
//! `..`, `.`, empty segments, a leading '/', control characters or '\\'; so a
//! path that passes through this module can never leave the Drive root.

use std::collections::BTreeMap;

use unicode_normalization::UnicodeNormalization;

use super::{DocPath, ObjectMeta};

/// Name of the mount-output directory. Never ingested, never removable from
/// the exclude list, so mounted Context Packs are not eaten again.
pub const CONTEXT_DIR: &str = ".context";

/// Why a name is skipped by the built-in rule table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// `.context`, always excluded (D14 mount output).
    ContextDir,
    /// A name starting with '.' (`.git`, `.DS_Store`, ...).
    Hidden,
    /// Office lock file `~$*`.
    OfficeLock,
    /// OS metadata and tool junk (`Thumbs.db`, `desktop.ini`, `node_modules`,
    /// `__pycache__`, `*.tmp`, `*.part`, `*.crdownload`).
    Junk,
}

fn ends_with_ci(name: &str, suffix: &str) -> bool {
    name.len() >= suffix.len()
        && name.as_bytes()[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}

/// Built-in skip rule for one path segment (file or directory name).
pub fn skip_reason(name: &str) -> Option<SkipReason> {
    if name == CONTEXT_DIR {
        return Some(SkipReason::ContextDir);
    }
    // `.~lock.*#` also starts with '.', so the hidden rule covers it.
    if name.starts_with('.') {
        return Some(SkipReason::Hidden);
    }
    if name.starts_with("~$") {
        return Some(SkipReason::OfficeLock);
    }
    if name.eq_ignore_ascii_case("Thumbs.db")
        || name.eq_ignore_ascii_case("desktop.ini")
        || name == "node_modules"
        || name == "__pycache__"
        || ends_with_ci(name, ".tmp")
        || ends_with_ci(name, ".part")
        || ends_with_ci(name, ".crdownload")
    {
        return Some(SkipReason::Junk);
    }
    None
}

/// True when any segment of a '/'-separated relative path is skipped.
pub fn is_skipped_path(path: &str) -> bool {
    path.split('/').any(|s| skip_reason(s).is_some())
}

/// Convert source-side bytes to a Drive path: UTF-8, NFC, structurally valid.
/// `None` means `unsupported(DOCS_SOURCE_PATH_INVALID)`.
///
/// Formal: KP1 (the result is a `DocPath`, hence inside the root).
pub fn normalize_raw_key(raw: &[u8]) -> Option<DocPath> {
    let text = std::str::from_utf8(raw).ok()?;
    let nfc: String = text.nfc().collect();
    DocPath::new(&nfc).ok()
}

/// Structural check for a raw key that is about to be joined under the root.
/// Rejects empty keys, absolute keys, empty/`.`/`..` segments and NUL bytes,
/// on the raw bytes (non-UTF-8 names stay addressable, escapes do not).
pub fn raw_key_is_safe(raw: &[u8]) -> bool {
    if raw.is_empty() || raw.len() > 4096 || raw.contains(&0) {
        return false;
    }
    raw.split(|b| *b == b'/')
        .all(|seg| !seg.is_empty() && seg != b"." && seg != b"..")
}

/// Case and normalization folded form used to detect collisions: NFC, then
/// Unicode lowercase. Full Unicode case folding is not available without a
/// new dependency; `to_lowercase` is the documented approximation.
pub fn path_fold(path: &DocPath) -> String {
    path.as_str().nfc().collect::<String>().to_lowercase()
}

/// A glob over whole Drive paths. `*` and `?` stay inside one segment, `**`
/// crosses segments. Drive-level excludes are additive; the built-in table
/// and `.context` cannot be removed.
#[derive(Clone, Debug, Default)]
pub struct ExcludeSet {
    patterns: Vec<String>,
}

impl ExcludeSet {
    pub fn new(patterns: impl IntoIterator<Item = String>) -> Self {
        Self {
            patterns: patterns.into_iter().collect(),
        }
    }

    /// Built-in rules plus the Drive-level globs.
    pub fn excludes(&self, path: &str) -> bool {
        is_skipped_path(path) || self.patterns.iter().any(|p| glob_match(p, path))
    }
}

/// Segment-aware glob match (iterative, no recursion, bounded by input size).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_inner(&p, &t)
}

fn glob_inner(p: &[char], t: &[char]) -> bool {
    // Backtracking with one saved star position per star class.
    let (mut pi, mut ti) = (0usize, 0usize);
    // (pattern index after the star, text index, crosses '/')
    let mut star: Option<(usize, usize, bool)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            let double = pi + 1 < p.len() && p[pi + 1] == '*';
            pi += if double { 2 } else { 1 };
            star = Some((pi, ti, double));
            continue;
        }
        if pi < p.len() && (p[pi] == t[ti] && p[pi] != '?' || p[pi] == '?' && t[ti] != '/') {
            pi += 1;
            ti += 1;
            continue;
        }
        match star {
            Some((spi, sti, double)) if double || t[sti] != '/' => {
                star = Some((spi, sti + 1, double));
                pi = spi;
                ti = sti + 1;
            }
            _ => return false,
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Outcome of collision analysis for one listed object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStatus {
    /// Normal.
    Ok,
    /// Path could not be normalized: `unsupported(DOCS_SOURCE_PATH_INVALID)`.
    Invalid,
    /// Normalizes to a path an earlier object already owns (NFC vs NFD bytes):
    /// `unsupported(DOCS_SOURCE_PATH_COLLISION)`.
    Collision,
    /// Differs only by case or normalization-insensitive fold from another
    /// object; both are processed, the frontend is told.
    FoldConflict,
}

/// Classify a full listing. The first object (by raw key bytes) owning a
/// normalized path wins; later ones with the identical normalized path are
/// `Collision`. Distinct normalized paths with equal folds are `FoldConflict`
/// for every member. Output order matches the input order.
pub fn annotate(entries: &[ObjectMeta]) -> Vec<PathStatus> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|a, b| entries[*a].raw_key.cmp(&entries[*b].raw_key));
    let mut status = vec![PathStatus::Ok; entries.len()];
    let mut owner: BTreeMap<&str, usize> = BTreeMap::new();
    let mut folds: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for &i in &order {
        match &entries[i].path {
            None => status[i] = PathStatus::Invalid,
            Some(path) => {
                if owner.contains_key(path.as_str()) {
                    status[i] = PathStatus::Collision;
                } else {
                    owner.insert(path.as_str(), i);
                    folds.entry(path_fold(path)).or_default().push(i);
                }
            }
        }
    }
    for members in folds.values().filter(|m| m.len() > 1) {
        for &i in members {
            status[i] = PathStatus::FoldConflict;
        }
    }
    status
}
