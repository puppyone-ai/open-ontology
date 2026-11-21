//! `.gitignore` and `.ooignore` rules of an fs Drive (a code folder dropped or linked as a source).
//!
//! The same semantics are implemented by the desktop app's copy path and watcher
//! (`apps/super-ontology-desktop/electron/main/super-ontology/ignore-rules.mjs`); both are tested against
//! `crates/oo-connectors/tests/fixtures/ignore-semantics.json`, so a folder copied, linked and watched is read the same way.
//!
//! Semantics (a subset of gitignore, enough for code projects):
//! - A rules file is `.gitignore` or `.ooignore` inside a directory of the Drive. Its patterns are relative to that directory and reach
//!   everything below it. Rules of a deeper directory beat rules of a shallower one; inside one directory `.ooignore` beats `.gitignore`;
//!   within a file the last matching line wins. Both files are only read from inside the Drive, never from a parent of the root.
//! - Blank lines and lines starting with `#` are comments. `\#` and `\!` at the start are literal. Trailing spaces are dropped unless escaped.
//! - A leading `!` re-includes what an earlier rule ignored, but nothing below an ignored directory is ever visited, so a file inside an
//!   ignored directory cannot be re-included.
//! - A trailing `/` makes the rule match directories only.
//! - A pattern with a `/` in it (other than a trailing one) is anchored to the directory of its file; without one it matches a name at any depth.
//! - `*` matches any run of characters inside one name, `?` one character, `[abc]`, `[a-z]` and `[!a-z]` / `[^a-z]` one character of a class,
//!   `\x` the character x. `**` between slashes matches any number of directories (also none); a trailing `/**` matches everything inside;
//!   a leading `**/` matches at any depth. Matching is case sensitive.
//! - A rules file larger than [`MAX_RULES_BYTES`], or a rule with more than [`MAX_SEGMENTS`] segments or more than [`MAX_DOUBLE_STARS`] `**`, is
//!   ignored (rule by rule for the second and third), so a hostile file cannot make a walk slow.

use std::sync::Arc;

/// A rules file bigger than this is not read.
pub const MAX_RULES_BYTES: u64 = 1024 * 1024;
/// Most path segments a single rule may have.
pub const MAX_SEGMENTS: usize = 32;
/// Most `**` segments a single rule may have (they are what makes matching expensive).
pub const MAX_DOUBLE_STARS: usize = 4;
/// File names read as rules, in the order they apply inside one directory (the later one wins).
pub const RULE_FILES: [(&str, Source); 2] = [
    (".gitignore", Source::GitIgnore),
    (".ooignore", Source::OoIgnore),
];

/// Which file a deciding rule came from (the skipped counts are kept apart by it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    GitIgnore,
    OoIgnore,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitIgnore => "gitignore",
            Self::OoIgnore => "ooignore",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Seg {
    /// `**`
    Any,
    /// One name pattern (kept as characters, escapes included).
    Glob(Vec<char>),
}

#[derive(Clone, Debug)]
struct Rule {
    segs: Vec<Seg>,
    negated: bool,
    dir_only: bool,
    source: Source,
}

/// The rules of one directory (`.gitignore` then `.ooignore`).
#[derive(Clone, Debug, Default)]
pub struct RuleSet {
    rules: Vec<Rule>,
}

impl RuleSet {
    /// Adds the rules of one file's text.
    pub fn add(&mut self, text: &str, source: Source) {
        for line in text.lines() {
            if let Some(rule) = parse_line(line, source) {
                self.rules.push(rule);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The last rule that matches `rel` (segments below this set's directory); `Some((ignored, source))`.
    fn verdict(&self, rel: &[&str], is_dir: bool) -> Option<(bool, Source)> {
        self.rules
            .iter()
            .rev()
            .find(|r| (!r.dir_only || is_dir) && match_segs(&r.segs, rel))
            .map(|r| (!r.negated, r.source))
    }
}

fn parse_line(raw: &str, source: Source) -> Option<Rule> {
    let mut line = raw.strip_suffix('\r').unwrap_or(raw);
    // Trailing spaces go unless escaped.
    while line.ends_with(' ') && !line.ends_with("\\ ") {
        line = &line[..line.len() - 1];
    }
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (negated, mut body) = match line.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    if body.starts_with("\\#") || body.starts_with("\\!") {
        body = &body[1..];
    }
    let dir_only = body.len() > 1 && body.ends_with('/');
    let body = if dir_only {
        &body[..body.len() - 1]
    } else {
        body
    };
    if body.is_empty() || body == "/" {
        return None;
    }
    let anchored = body.contains('/');
    let body = body.strip_prefix('/').unwrap_or(body);
    let mut segs: Vec<Seg> = Vec::new();
    if !anchored {
        segs.push(Seg::Any);
    }
    for part in body.split('/') {
        if part.is_empty() {
            continue;
        }
        if part == "**" {
            if segs.last() != Some(&Seg::Any) {
                segs.push(Seg::Any);
            }
        } else {
            segs.push(Seg::Glob(part.chars().collect()));
        }
    }
    if segs.is_empty()
        || segs.len() > MAX_SEGMENTS
        || segs.iter().filter(|s| **s == Seg::Any).count() > MAX_DOUBLE_STARS
    {
        return None;
    }
    Some(Rule {
        segs,
        negated,
        dir_only,
        source,
    })
}

fn match_segs(pattern: &[Seg], path: &[&str]) -> bool {
    match pattern.first() {
        None => path.is_empty(),
        Some(Seg::Any) => {
            if pattern.len() == 1 {
                // A trailing `**` means everything inside, not the directory itself.
                return !path.is_empty();
            }
            (0..=path.len()).any(|skip| match_segs(&pattern[1..], &path[skip..]))
        }
        Some(Seg::Glob(glob)) => {
            !path.is_empty() && glob_name(glob, path[0]) && match_segs(&pattern[1..], &path[1..])
        }
    }
}

/// One name against one glob (`*`, `?`, classes, escapes); iterative, bounded by the input size.
fn glob_name(pattern: &[char], name: &str) -> bool {
    let text: Vec<char> = name.chars().collect();
    let (mut p, mut t) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p + 1, t));
            p += 1;
            continue;
        }
        if let Some(next) = single(pattern, p, text[t]) {
            p = next;
            t += 1;
            continue;
        }
        match star {
            Some((sp, st)) => {
                star = Some((sp, st + 1));
                p = sp;
                t = st + 1;
            }
            None => return false,
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

/// Matches one character at `pattern[p]`; the pattern index after it, or `None`.
fn single(pattern: &[char], p: usize, c: char) -> Option<usize> {
    let head = *pattern.get(p)?;
    match head {
        '*' => None,
        '?' => Some(p + 1),
        '\\' => (pattern.get(p + 1) == Some(&c)).then_some(p + 2),
        '[' => class(pattern, p, c),
        other => (other == c).then_some(p + 1),
    }
}

fn class(pattern: &[char], start: usize, c: char) -> Option<usize> {
    let mut i = start + 1;
    let negate = matches!(pattern.get(i), Some('!' | '^'));
    if negate {
        i += 1;
    }
    let mut hit = false;
    let mut first = true;
    loop {
        let item = *pattern.get(i)?;
        if item == ']' && !first {
            return (hit != negate).then_some(i + 1);
        }
        first = false;
        let low = if item == '\\' {
            i += 1;
            *pattern.get(i)?
        } else {
            item
        };
        if pattern.get(i + 1) == Some(&'-') && pattern.get(i + 2).is_some_and(|x| *x != ']') {
            let high = pattern[i + 2];
            hit |= low <= c && c <= high;
            i += 3;
        } else {
            hit |= low == c;
            i += 1;
        }
    }
}

/// The rule sets of the directories above the entry being looked at, shallowest first. Cloning shares the sets.
#[derive(Clone, Debug, Default)]
pub struct IgnoreStack {
    layers: Vec<(usize, Arc<RuleSet>)>,
}

impl IgnoreStack {
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// A stack that also holds `rules`, which belong to the directory at depth `depth` (0 = the Drive root).
    #[must_use]
    pub fn with(&self, depth: usize, rules: RuleSet) -> Self {
        let mut next = self.clone();
        if !rules.is_empty() {
            next.layers.push((depth, Arc::new(rules)));
        }
        next
    }

    /// Why an entry is ignored: `rel` is its path from the Drive root as segments (the entry last). `None` when it is kept.
    pub fn ignored(&self, rel: &[&str], is_dir: bool) -> Option<Source> {
        let mut state: Option<(bool, Source)> = None;
        for (depth, set) in &self.layers {
            if *depth >= rel.len() {
                continue;
            }
            if let Some(verdict) = set.verdict(&rel[*depth..], is_dir) {
                state = Some(verdict);
            }
        }
        match state {
            Some((true, source)) => Some(source),
            _ => None,
        }
    }
}

/// Reads the rules of the directory `dir` (`.gitignore`, then `.ooignore`). Absent, unreadable, oversized or not regular files add nothing.
pub fn load_dir_rules(dir: &std::path::Path) -> RuleSet {
    use std::io::Read;
    let mut set = RuleSet::default();
    for (name, source) in RULE_FILES {
        let path = dir.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_RULES_BYTES {
            continue;
        }
        let mut bytes = Vec::new();
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        if file.take(MAX_RULES_BYTES).read_to_end(&mut bytes).is_err() {
            continue;
        }
        set.add(&String::from_utf8_lossy(&bytes), source);
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Decides `path` against an in-memory tree of rules files (`"sub/.gitignore" -> text`), the way a walk would: ancestors first.
    pub(crate) fn decide(
        files: &BTreeMap<String, String>,
        path: &str,
        is_dir: bool,
    ) -> Option<Source> {
        let parts: Vec<&str> = path.split('/').collect();
        let mut stack = IgnoreStack::default();
        for depth in 0..parts.len() {
            if depth > 0 || files.keys().any(|k| !k.contains('/')) {
                let dir = parts[..depth].join("/");
                let mut set = RuleSet::default();
                for (name, source) in RULE_FILES {
                    let key = if dir.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{dir}/{name}")
                    };
                    if let Some(text) = files.get(&key) {
                        set.add(text, source);
                    }
                }
                stack = stack.with(depth, set);
            }
            let last = depth + 1 == parts.len();
            if let Some(source) = stack.ignored(&parts[..=depth], !last || is_dir) {
                return Some(source);
            }
        }
        None
    }

    #[test]
    fn shared_fixture_decides_the_same_as_the_app() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/ignore-semantics.json"
        ))
        .unwrap();
        let mut checked = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let files: BTreeMap<String, String> = case["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
                .collect();
            for entry in case["paths"].as_array().unwrap() {
                let path = entry["path"].as_str().unwrap();
                let is_dir = entry["dir"].as_bool().unwrap_or(false);
                let expected = entry["ignored"].as_str();
                let got = decide(&files, path, is_dir).map(Source::as_str);
                assert_eq!(got, expected, "{} / {path}", case["name"]);
                checked += 1;
            }
        }
        assert!(checked >= 60, "the fixture must stay broad ({checked})");
    }

    #[test]
    fn hostile_rules_are_dropped_not_slow() {
        let mut set = RuleSet::default();
        set.add("a/**/b/**/c/**/d/**/e/**/f/z\n", Source::GitIgnore);
        set.add(&format!("{}x\n", "a/".repeat(40)), Source::GitIgnore);
        assert!(set.is_empty(), "five `**` and forty segments are refused");
        let mut set = RuleSet::default();
        set.add("**/a/**/a/**/a/**/a/z\n", Source::OoIgnore);
        assert!(!set.is_empty());
        let deep: Vec<&str> = std::iter::repeat_n("a", 60).collect();
        assert!(set.verdict(&deep, false).is_none());
    }
}
