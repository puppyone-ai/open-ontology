//! Instruction documents (D89): AGENTS.md, CLAUDE.md, README, CONTRIBUTING, ADRs, style guides.
//!
//! Each rule is its own item with the block it was written in as the anchor, so an answer can quote it. Around the
//! rules: the commands people are told to run, the tools those commands use and the paths the text points at.
//! A rule carries `attr.uses_tool`, `attr.runs_command` and `attr.applies_to_path` facts for what its own text (or the
//! code block under it) mentions, and the document mentions every item (the generic `doc.entity` pass), so two
//! documents that name the same tool or path share the entity. Deterministic; no model.

use oo_core::docir::{Block, BlockKind, DocIr};

use super::{COMMAND, PATH, RULE, TOOL, attr, clip, thing};
use crate::extract::model::{Collector, EntityRef, EvidenceRef};

const RULE_NAME_CHARS: usize = 160;
const MAX_RULES: usize = 200;
const MAX_COMMANDS: usize = 60;
const MAX_PATHS: usize = 40;
const RULE_BP: u16 = 9000;
const REF_BP: u16 = 9000;

/// Programs whose invocation counts as a command.
pub const KNOWN_TOOLS: &[&str] = &[
    "git",
    "npm",
    "pnpm",
    "yarn",
    "npx",
    "bun",
    "deno",
    "node",
    "cargo",
    "rustc",
    "rustup",
    "rustfmt",
    "clippy",
    "make",
    "cmake",
    "pytest",
    "pip",
    "pip3",
    "uv",
    "poetry",
    "python",
    "python3",
    "ruff",
    "black",
    "mypy",
    "flake8",
    "pylint",
    "eslint",
    "prettier",
    "tsc",
    "vitest",
    "jest",
    "playwright",
    "go",
    "gofmt",
    "golangci-lint",
    "docker",
    "docker-compose",
    "kubectl",
    "helm",
    "terraform",
    "just",
    "task",
    "gradle",
    "mvn",
    "bazel",
    "sbt",
    "dotnet",
    "swift",
    "xcodebuild",
    "pod",
    "bundle",
    "rake",
    "rspec",
    "composer",
    "php",
    "ruby",
    "java",
    "javac",
    "bash",
    "sh",
    "zsh",
    "curl",
    "jq",
    "psql",
    "sqlite3",
    "redis-cli",
    "ffmpeg",
    "pre-commit",
    "husky",
    "lerna",
    "turbo",
    "nx",
    "vite",
    "webpack",
    "rollup",
    "esbuild",
    "sphinx-build",
    "mkdocs",
    "ansible",
    "aws",
    "gcloud",
    "az",
    "gh",
    "oo",
    "xtask",
    "cargo-deny",
    "nextest",
    "tox",
    "nox",
];

const RULE_SECTION_WORDS: &[&str] = &[
    "rule",
    "convention",
    "guideline",
    "style",
    "standard",
    "do not",
    "don't",
    "dos and",
    "must",
    "requirement",
    "constraint",
    "best practice",
    "principle",
    "policy",
    "practices",
    "instructions",
    "规则",
    "规范",
    "约定",
    "约束",
    "要求",
    "注意",
    "原则",
    "禁止",
    "准则",
    "守则",
    "须知",
    "代码风格",
    "风格",
    "做法",
];

const STRONG_EN: &[&str] = &[
    "before every",
    "before each",
    "before you",
    "before committing",
    "before pushing",
    "before opening",
    "before merging",
    "must",
    "should",
    "never",
    "always",
    "do not",
    "don't",
    "dont ",
    "avoid",
    "prefer",
    "required",
    "ensure",
    "make sure",
    "shall",
    "forbidden",
    "not allowed",
    "no more than",
];
const STRONG_ZH: &[&str] = &[
    "提交前",
    "合并前",
    "发布前",
    "必须",
    "禁止",
    "不要",
    "不得",
    "不能",
    "不可",
    "应当",
    "应该",
    "务必",
    "严禁",
    "请勿",
    "一律",
    "优先",
    "避免",
    "确保",
    "需要",
];
const IMPERATIVE_START: &[&str] = &[
    "use ",
    "run ",
    "keep ",
    "write ",
    "add ",
    "prefix ",
    "name ",
    "put ",
    "commit ",
    "format ",
    "document ",
    "test ",
    "wrap ",
    "return ",
    "import ",
    "follow ",
    "check ",
    "update ",
    "pin ",
    "set ",
    "open ",
    "create ",
    "install ",
    "prefer ",
    "declare ",
    "mark ",
    "limit ",
    "treat ",
];

fn lower(s: &str) -> String {
    s.to_lowercase()
}

fn has_cue(text: &str) -> bool {
    let l = lower(text);
    let padded = format!(" {l} ");
    STRONG_EN.iter().any(|c| {
        if c.ends_with(' ') {
            padded.contains(c)
        } else {
            padded.contains(&format!(" {c} "))
                || padded.contains(&format!(" {c},"))
                || padded.contains(&format!(" {c}."))
                || padded.contains(&format!(" {c}:"))
                || padded.contains(&format!("{c} "))
        }
    }) || STRONG_ZH.iter().any(|c| l.contains(c))
}

fn imperative(text: &str) -> bool {
    let l = lower(text);
    IMPERATIVE_START.iter().any(|w| l.starts_with(w))
}

/// The nearest heading names the section (the document title says nothing about its sections).
fn is_rule_section(hpath: &[String]) -> bool {
    hpath.last().is_some_and(|h| {
        let l = lower(h);
        RULE_SECTION_WORDS.iter().any(|w| l.contains(w))
    })
}

/// Markdown markers off the front, whitespace collapsed.
fn clean(text: &str) -> String {
    let mut s = text.trim();
    for p in ["- ", "* ", "+ "] {
        if let Some(r) = s.strip_prefix(p) {
            s = r;
            break;
        }
    }
    if let Some(r) = s.strip_prefix("[ ] ").or_else(|| s.strip_prefix("[x] ")) {
        s = r;
    }
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 4 {
        let rest = &s[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            s = r;
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sentences of a paragraph (`. ` and `。` end one; backtick spans are not cut).
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_code = false;
    let cs: Vec<char> = text.chars().collect();
    for (i, &c) in cs.iter().enumerate() {
        if c == '`' {
            in_code = !in_code;
        }
        cur.push(c);
        let end = !in_code
            && (c == '。'
                || c == '；'
                || c == '！'
                || c == '？'
                || ((c == '.' || c == '!' || c == '?')
                    && cs.get(i + 1).is_none_or(|n| n.is_whitespace())));
        if end {
            let s = cur.trim().to_owned();
            if !s.is_empty() {
                out.push(s);
            }
            cur.clear();
        }
    }
    let s = cur.trim().to_owned();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

/// Inline code spans of a text.
pub fn code_spans(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(a) = rest.find('`') {
        let after = &rest[a + 1..];
        match after.find('`') {
            Some(b) => {
                let span = after[..b].trim();
                if !span.is_empty() && !span.contains('\n') {
                    out.push(span.to_owned());
                }
                rest = &after[b + 1..];
            }
            None => break,
        }
    }
    out
}

fn tool_of(command: &str) -> Option<String> {
    let first = command.split_whitespace().next()?;
    let first = first.trim_start_matches("./");
    let l = lower(first);
    KNOWN_TOOLS.contains(&l.as_str()).then_some(l)
}

/// A command line (prompt and trailing comment off), when its first word is a known tool.
fn command_line(line: &str) -> Option<String> {
    let mut l = line.trim();
    for p in ["$ ", "> ", "% "] {
        if let Some(r) = l.strip_prefix(p) {
            l = r.trim();
        }
    }
    if l.is_empty() || l.starts_with('#') || l.starts_with("//") {
        return None;
    }
    let l = l.split(" # ").next().unwrap_or(l).trim();
    // Environment assignments before the program: `RUST_LOG=debug cargo test`.
    let l = {
        let mut rest = l;
        while let Some((w, r)) = rest.split_once(' ') {
            if w.contains('=')
                && !w.starts_with('-')
                && w.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
            {
                rest = r.trim_start();
            } else {
                break;
            }
        }
        rest
    };
    tool_of(l)?;
    let cmd = l.split_whitespace().collect::<Vec<_>>().join(" ");
    (cmd.chars().count() <= 160).then_some(cmd)
}

const SLASH_PAIRS: &[&str] = &[
    "and/or",
    "w/o",
    "n/a",
    "i/o",
    "tcp/ip",
    "client/server",
    "read/write",
    "true/false",
    "yes/no",
    "input/output",
    "either/or",
    "he/she",
    "on/off",
    "start/stop",
    "up/down",
    "s/he",
    "pass/fail",
    "and/or,",
];
const PATH_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "py", "go", "java", "kt", "swift", "c", "h", "cpp",
    "rb", "php", "sh", "md", "json", "toml", "yaml", "yml", "sql", "css", "html", "lock", "txt",
    "proto", "ini", "cfg", "env", "mdc", "gradle", "tf",
];

/// A path-like token: `src/auth/`, `crates/oo-docs/src/lib.rs`, `./scripts/run.sh`, `Cargo.toml` (only when backticked).
pub fn path_token(raw: &str, quoted: bool) -> Option<String> {
    let t = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | ')' | '(' | '"' | '\'' | '`'));
    if t.is_empty()
        || t.len() > 160
        || t.contains("://")
        || t.starts_with('-')
        || t.starts_with('@')
        || t.contains(' ')
        || t.contains('*') && !quoted
    {
        return None;
    }
    if t.chars().any(|c| {
        !(c.is_alphanumeric()
            || matches!(c, '/' | '.' | '_' | '-' | '~' | '*' | '[' | ']' | '{' | '}'))
    }) {
        return None;
    }
    let l = lower(t);
    if SLASH_PAIRS.contains(&l.as_str())
        || t.starts_with("0.")
        || t.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        return None;
    }
    let ext = t.rsplit_once('.').map(|(_, e)| lower(e));
    let has_ext = ext.as_deref().is_some_and(|e| PATH_EXTS.contains(&e));
    let slashes = t.matches('/').count();
    let trailing = t.ends_with('/');
    let segs_ok = t
        .trim_matches('/')
        .split('/')
        .all(|s| s.chars().count() >= 2 || s == "." || s == "~");
    let ok = if quoted {
        (slashes >= 1 && segs_ok && !t.starts_with('/')
            || t.starts_with("~/")
            || t.starts_with('/') && slashes >= 2)
            || (has_ext
                && !t.contains('/')
                && t.chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_'))
    } else {
        slashes >= 1
            && segs_ok
            && (has_ext || trailing || t.starts_with("./") || slashes >= 2)
            && !t.starts_with('/')
    };
    if !ok {
        return None;
    }
    // A version or a date is not a path.
    if t.chars().next().is_some_and(|c| c.is_ascii_digit()) && !has_ext && slashes < 2 {
        return None;
    }
    let t = t.strip_prefix("./").unwrap_or(t);
    Some(t.trim_end_matches('/').to_owned()).filter(|s| !s.is_empty())
}

struct Seen {
    commands: Vec<String>,
    paths: Vec<String>,
    rules: usize,
}

fn note_command(
    col: &mut Collector,
    seen: &mut Seen,
    cmd: &str,
    ev: &EvidenceRef,
) -> Option<EntityRef> {
    if !seen.commands.iter().any(|c| c == cmd) {
        if seen.commands.len() >= MAX_COMMANDS {
            return None;
        }
        seen.commands.push(cmd.to_owned());
    }
    let e = thing(COMMAND, cmd);
    col.entity(&e, Some(ev));
    if let Some(tool) = tool_of(cmd) {
        col.entity(&thing(TOOL, tool), Some(ev));
    }
    Some(e)
}

fn note_path(
    col: &mut Collector,
    seen: &mut Seen,
    path: &str,
    ev: &EvidenceRef,
) -> Option<EntityRef> {
    if !seen.paths.iter().any(|c| c == path) {
        if seen.paths.len() >= MAX_PATHS {
            return None;
        }
        seen.paths.push(path.to_owned());
    }
    let e = thing(PATH, path);
    col.entity(&e, Some(ev));
    Some(e)
}

/// Words that can be a tool name or an ordinary word: they count only as the head of a command in a code block or a
/// code span, never as a bare word of prose.
const AMBIGUOUS_TOOLS: &[&str] = &[
    "go", "make", "task", "just", "pod", "bundle", "az", "nx", "oo", "uv", "sh", "swift", "gh",
    "node", "java", "php", "ruby", "turbo", "vite", "black", "nextest", "husky", "jest", "helm",
];
/// Tools whose first argument is a subcommand.
const SUBCOMMAND_TOOLS: &[&str] = &[
    "git",
    "cargo",
    "npm",
    "pnpm",
    "yarn",
    "docker",
    "docker-compose",
    "kubectl",
    "pip",
    "pip3",
    "poetry",
    "uv",
    "gh",
    "terraform",
    "helm",
    "dotnet",
    "gradle",
    "bun",
    "deno",
    "go",
    "make",
    "xtask",
    "rustup",
    "just",
    "mvn",
    "composer",
    "bundle",
    "rake",
    "aws",
    "gcloud",
    "az",
    "pre-commit",
    "playwright",
    "vitest",
];
const PROSE_STOP: &[&str] = &[
    "before", "after", "to", "and", "then", "or", "when", "with", "for", "if", "so", "instead",
    "of", "not", "never", "always", "in", "on", "from", "the", "a", "an", "is", "are", "as",
    "that", "which", "once", "while", "until", "by", "but", "it", "you", "we", "should", "must",
    "will", "can", "locally", "first", "again", "here", "too", "also", "only", "every", "each",
    "all",
];

fn arg_like(tok: &str) -> bool {
    tok.starts_with('-')
        || tok.contains('/')
        || tok.contains('=')
        || tok.contains('.') && tok.len() > 2
        || tok.starts_with('@')
}

/// Commands written in running text without code marks: a known tool, its subcommand and its flags and arguments.
pub fn prose_commands(text: &str) -> Vec<String> {
    let toks: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let head =
            toks[i].trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '(' | ')' | '"' | '\''));
        let hl = lower(head);
        i += 1;
        if !KNOWN_TOOLS.contains(&hl.as_str()) || AMBIGUOUS_TOOLS.contains(&hl.as_str()) {
            continue;
        }
        let mut cmd = vec![hl.clone()];
        let mut prev_flag = false;
        let mut j = i;
        let mut ended = false;
        while j < toks.len() && cmd.len() < 8 && !ended {
            let raw = toks[j];
            ended = raw.ends_with(['.', ',', ';', ':']) && !raw.ends_with("..");
            let tok = raw.trim_end_matches(['.', ',', ';', ':', ')']);
            let tl = lower(tok);
            if tok.is_empty() || PROSE_STOP.contains(&tl.as_str()) {
                break;
            }
            let sub = cmd.len() == 1
                && SUBCOMMAND_TOOLS.contains(&hl.as_str())
                && tok
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-' || c == ':')
                && tok.len() <= 20;
            if sub || arg_like(tok) || prev_flag {
                prev_flag = tok.starts_with('-') && !tok.contains('=');
                cmd.push(tok.to_owned());
                j += 1;
            } else if !prev_flag
                && cmd.len() > 1
                && tok
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-' || c == '_')
                && tok.len() <= 24
                && cmd.iter().skip(1).any(|c| c.starts_with('-'))
            {
                // a bare argument after flags: `cargo test -p oo-docs`
                cmd.push(tok.to_owned());
                j += 1;
            } else {
                break;
            }
        }
        i = j.max(i);
        if cmd.len() > 1 {
            out.push(cmd.join(" "));
        }
    }
    out
}

/// What one piece of text mentions: commands, tools and paths (inline code, and bare path tokens).
struct Mentions {
    commands: Vec<String>,
    tools: Vec<String>,
    paths: Vec<String>,
}

fn mentions_of(text: &str) -> Mentions {
    let mut m = Mentions {
        commands: Vec::new(),
        tools: Vec::new(),
        paths: Vec::new(),
    };
    for span in code_spans(text) {
        if let Some(cmd) = command_line(&span)
            && cmd.contains(' ')
        {
            if !m.commands.contains(&cmd) {
                m.commands.push(cmd);
            }
            continue;
        }
        let l = lower(&span);
        if KNOWN_TOOLS.contains(&l.as_str()) {
            if !m.tools.contains(&l) {
                m.tools.push(l);
            }
            continue;
        }
        if let Some(p) = path_token(&span, true)
            && !m.paths.contains(&p)
        {
            m.paths.push(p);
        }
    }
    for cmd in prose_commands(text) {
        if !m.commands.contains(&cmd) {
            m.commands.push(cmd);
        }
    }
    for w in text.split_whitespace() {
        let wl = lower(w.trim_matches(|c: char| !(c.is_alphanumeric() || c == '-')));
        if KNOWN_TOOLS.contains(&wl.as_str())
            && !AMBIGUOUS_TOOLS.contains(&wl.as_str())
            && !m.tools.contains(&wl)
        {
            m.tools.push(wl);
        }
    }
    let stripped: String = {
        let mut out = String::new();
        let mut in_code = false;
        for c in text.chars() {
            if c == '`' {
                in_code = !in_code;
                out.push(' ');
            } else if !in_code {
                out.push(c);
            }
        }
        out
    };
    for w in stripped.split_whitespace() {
        if let Some(p) = path_token(w, false)
            && !m.paths.contains(&p)
        {
            m.paths.push(p);
        }
    }
    m
}

pub(crate) fn extract(_path: &str, ir: &DocIr, col: &mut Collector) {
    let mut seen = Seen {
        commands: Vec::new(),
        paths: Vec::new(),
        rules: 0,
    };
    let mut last_rule: Option<(EntityRef, EvidenceRef, bool)> = None;
    let mut prev_colon = false;
    for b in &ir.blocks {
        match b.kind {
            BlockKind::Frontmatter
            | BlockKind::Heading
            | BlockKind::PageHeader
            | BlockKind::PageFooter => {
                prev_colon = false;
                last_rule = None;
                continue;
            }
            BlockKind::Code => {
                let ev = EvidenceRef::from_block(b);
                for line in b.text.lines() {
                    let Some(cmd) = command_line(line) else {
                        continue;
                    };
                    let Some(c) = note_command(col, &mut seen, &cmd, &ev) else {
                        continue;
                    };
                    if prev_colon && let Some((rule, rev, _)) = &last_rule {
                        attr(col, rule, "runs_command", &cmd, "profile.rule", REF_BP, rev);
                        let _ = c;
                    }
                }
                // Paths named in the block (arguments such as `crates/oo-docs`).
                for w in b.text.split_whitespace() {
                    if let Some(p) = path_token(w, false) {
                        note_path(col, &mut seen, &p, &ev);
                    }
                }
                prev_colon = false;
                continue;
            }
            BlockKind::Paragraph | BlockKind::ListItem | BlockKind::Quote | BlockKind::Kv => {}
            _ => {
                prev_colon = false;
                continue;
            }
        }
        let ev = EvidenceRef::from_block(b);
        let text = clean(&b.text);
        if text.is_empty() {
            continue;
        }
        let in_rules = is_rule_section(&b.hpath);
        let candidates: Vec<String> = if b.kind == BlockKind::ListItem && in_rules {
            vec![first_item_text(&text)]
        } else {
            sentences(&text)
        };
        let mut produced = false;
        for s in candidates {
            let s = s.trim().to_owned();
            if s.chars().count() < 8 {
                continue;
            }
            let normative =
                has_cue(&s) || (in_rules && (imperative(&s) || b.kind == BlockKind::ListItem));
            if !normative || seen.rules >= MAX_RULES {
                continue;
            }
            // A bare command or a path list is not a rule.
            if command_line(&s).is_some() && !s.contains(". ") && !has_cue(&s) {
                continue;
            }
            let name = clip(&s, RULE_NAME_CHARS);
            let rule = thing(RULE, name);
            col.entity(&rule, Some(&ev));
            seen.rules += 1;
            produced = true;
            let m = mentions_of(&s);
            for cmd in &m.commands {
                note_command(col, &mut seen, cmd, &ev);
                attr(col, &rule, "runs_command", cmd, "profile.rule", REF_BP, &ev);
            }
            for t in &m.tools {
                col.entity(&thing(TOOL, t), Some(&ev));
                attr(col, &rule, "uses_tool", t, "profile.rule", REF_BP, &ev);
            }
            for cmd in &m.commands {
                if let Some(t) = tool_of(cmd) {
                    attr(col, &rule, "uses_tool", &t, "profile.rule", REF_BP, &ev);
                }
            }
            for p in &m.paths {
                if note_path(col, &mut seen, p, &ev).is_some() {
                    attr(
                        col,
                        &rule,
                        "applies_to_path",
                        p,
                        "profile.rule",
                        REF_BP,
                        &ev,
                    );
                }
            }
            last_rule = Some((rule, ev.clone(), true));
        }
        if !produced {
            // Commands and paths in plain text still belong to the document.
            let m = mentions_of(&text);
            for cmd in &m.commands {
                note_command(col, &mut seen, cmd, &ev);
            }
            for t in &m.tools {
                col.entity(&thing(TOOL, t), Some(&ev));
            }
            for p in &m.paths {
                note_path(col, &mut seen, p, &ev);
            }
            last_rule = None;
        }
        prev_colon = text.ends_with(':') || text.ends_with('：');
    }
    let _ = RULE_BP;
}

/// The first line of a list item (nested text is kept by the evidence block).
fn first_item_text(text: &str) -> String {
    let first = text.split('\n').next().unwrap_or(text);
    first.trim().to_owned()
}

#[allow(dead_code)]
fn unused(_: &Block) {}
