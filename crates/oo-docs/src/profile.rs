//! Document profiles (D89): every file gets a profile first, and the profile decides what is extracted.
//!
//! The profile is chosen deterministically from the path (file name conventions, directories, extension), the blob
//! format and, for prose, the content (frontmatter, headings, an abstract with references or a DOI). No model is
//! asked. A file with nothing to extract is still indexed and searchable: "no entities" is not an error.
//!
//! | profile | what it is | extracted |
//! |---|---|---|
//! | `business` | reports, plans, contracts, notes | the core rules of 10 section 5.1 (and the model) |
//! | `meeting` | minutes, interviews, transcripts | the core rules (and the model) |
//! | `table` | csv, xlsx | the table rules |
//! | `instruction` | AGENTS.md, CLAUDE.md, README, CONTRIBUTING, ADR, style guides | `Rule`, `Command`, `Tool`, `Path` |
//! | `code` | source files and package manifests | `Function`, `Class`, `Type`, `Dependency`, `Package`, `Path` |
//! | `paper` | research papers | `Paper`, `Person` (authors), `Venue`, `Method`, `Dataset`, `Metric`, cited `Paper` |
//! | `media` | audio, video, images | the media rules |
//! | `other` | logs, configuration, licences, unknown text | the Document only |
//!
//! The types of the profile extractors are `Thing{label}` (D16, 10 section 5.1.1): the labels in [`LABELS`] are
//! registered by the profile, so they are never proposed for promotion and the open candidate cap does not apply.

pub mod code;
pub mod instruction;
pub mod manifest;
pub mod paper;

#[cfg(test)]
mod tests;

use oo_core::docir::{BlockKind, DocFormat, DocIr};

use crate::extract::model::{
    Collector, EntityRef, EntityType, EvidenceRef, FactMention, Predicate,
};

/// The profile of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Profile {
    Business,
    Meeting,
    Table,
    Instruction,
    Code,
    Paper,
    Media,
    Other,
}

impl Profile {
    pub const ALL: [Profile; 8] = [
        Self::Business,
        Self::Meeting,
        Self::Table,
        Self::Instruction,
        Self::Code,
        Self::Paper,
        Self::Media,
        Self::Other,
    ];

    /// `file.profile` column value and the API field.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Business => "business",
            Self::Meeting => "meeting",
            Self::Table => "table",
            Self::Instruction => "instruction",
            Self::Code => "code",
            Self::Paper => "paper",
            Self::Media => "media",
            Self::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }

    /// Whether the model supplement (10 section 5.2) runs for this profile. Code and instruction documents are read by
    /// parsers and rules only (D89: no model guessing for structure), `other` has nothing to ask.
    pub const fn uses_model(self) -> bool {
        !matches!(self, Self::Code | Self::Instruction | Self::Other)
    }
}

/// What the classifier decided and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Classified {
    pub profile: Profile,
    /// Short code of the deciding evidence (`name`, `dir`, `ext`, `format`, `content`, `default`).
    pub reason: &'static str,
}

/// `Thing` labels the profile extractors create.
pub const RULE: &str = "Rule";
pub const COMMAND: &str = "Command";
pub const TOOL: &str = "Tool";
pub const PATH: &str = "Path";
pub const FUNCTION: &str = "Function";
pub const CLASS: &str = "Class";
pub const TYPE: &str = "Type";
pub const DEPENDENCY: &str = "Dependency";
pub const PACKAGE: &str = "Package";
pub const PAPER: &str = "Paper";
pub const VENUE: &str = "Venue";
pub const METHOD: &str = "Method";
pub const DATASET: &str = "Dataset";
pub const METRIC: &str = "Metric";

/// Every label above: registered by the profile, never promoted as a type.
pub const LABELS: &[&str] = &[
    RULE, COMMAND, TOOL, PATH, FUNCTION, CLASS, TYPE, DEPENDENCY, PACKAGE, PAPER, VENUE, METHOD,
    DATASET, METRIC,
];

/// Whether `label` belongs to a profile extractor.
pub fn is_profile_label(label: &str) -> bool {
    LABELS.contains(&label)
}

pub(crate) fn thing(label: &str, name: impl Into<String>) -> EntityRef {
    EntityRef {
        ty: EntityType::Thing,
        thing_label: Some(label.to_owned()),
        subkind: None,
        name: name.into(),
    }
}

/// `attr.<name>` fact with a text value.
pub(crate) fn attr(
    col: &mut Collector,
    subject: &EntityRef,
    name: &str,
    text: &str,
    rule: &'static str,
    conf: u16,
    ev: &EvidenceRef,
) {
    let text: String = text.trim().chars().take(500).collect();
    if text.is_empty() {
        return;
    }
    col.fact(FactMention {
        subject: subject.clone(),
        predicate: Predicate::Attr(name.to_owned()),
        object: None,
        value: Some(serde_json::json!({ "text": text })),
        valid_from: None,
        valid_to: None,
        valid_precision: None,
        confidence_bp: conf,
        rule_id: rule,
        evidence: vec![ev.clone()],
    });
}

pub(crate) fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

fn dir_segments(path: &str) -> Vec<String> {
    let mut parts: Vec<String> = path.split('/').map(str::to_ascii_lowercase).collect();
    parts.pop();
    parts
}

/// File names that are instructions for people or agents, lower case.
const INSTRUCTION_NAMES: &[&str] = &[
    "agents.md",
    "claude.md",
    "gemini.md",
    "codex.md",
    "agent.md",
    ".cursorrules",
    ".windsurfrules",
    ".clinerules",
    "copilot-instructions.md",
    "contributing.md",
    "contributing.rst",
    "contributing",
    "code_of_conduct.md",
    "security.md",
    "codeowners",
    "development.md",
    "hacking.md",
    "style.md",
    "styleguide.md",
    "style-guide.md",
    "style_guide.md",
    "conventions.md",
    "coding-standards.md",
    "coding-guidelines.md",
    "guidelines.md",
    "governance.md",
    "maintainers.md",
    "pull_request_template.md",
];

fn instruction_name(path: &str) -> bool {
    let name = file_name(path).to_ascii_lowercase();
    if INSTRUCTION_NAMES.contains(&name.as_str()) {
        return true;
    }
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
    if stem == "readme"
        || stem.starts_with("readme.")
        || stem.starts_with("readme-")
        || stem.starts_with("readme_")
    {
        return true;
    }
    if [
        "styleguide",
        "style-guide",
        "style_guide",
        "coding-style",
        "coding_style",
        "conventions",
    ]
    .iter()
    .any(|k| stem.contains(k))
        || [
            "编码规范",
            "代码规范",
            "开发规范",
            "贡献指南",
            "开发指南",
            "使用说明",
        ]
        .iter()
        .any(|k| stem.contains(k))
    {
        return true;
    }
    let dirs = dir_segments(path);
    let textual = matches!(
        extension(path).as_str(),
        "md" | "mdx" | "markdown" | "txt" | "mdc" | "rst" | ""
    );
    // .cursor/rules/*.mdc, .claude/**, docs/adr/*
    if dirs.iter().any(|d| d == ".cursor") && dirs.iter().any(|d| d == "rules") {
        return true;
    }
    if dirs.iter().any(|d| d == ".claude") && textual {
        return true;
    }
    if textual
        && dirs.iter().any(|d| {
            matches!(
                d.as_str(),
                "adr" | "adrs" | "decisions" | "architecture-decisions" | "rfcs" | "rfc"
            )
        })
    {
        return true;
    }
    false
}

const MEETING_DIRS: &[&str] = &[
    "meetings",
    "meeting",
    "interviews",
    "interview",
    "minutes",
    "standup",
    "standups",
    "会议",
    "访谈",
    "纪要",
    "会议纪要",
    "1on1",
];
const MEETING_NAMES: &[&str] = &[
    "minutes",
    "meeting",
    "interview",
    "standup",
    "sync-notes",
    "纪要",
    "会议",
    "访谈",
    "transcript",
    "retro",
    "1on1",
];

fn meeting_by_path(path: &str) -> bool {
    let dirs = dir_segments(path);
    if dirs.iter().any(|d| MEETING_DIRS.contains(&d.as_str())) {
        return true;
    }
    let name = file_name(path).to_ascii_lowercase();
    MEETING_NAMES.iter().any(|k| name.contains(k))
}

/// Source language of a path: extension (or well known file name) to language name.
pub fn code_language(path: &str) -> Option<&'static str> {
    let name = file_name(path).to_ascii_lowercase();
    match name.as_str() {
        "makefile" | "gnumakefile" => return Some("make"),
        "dockerfile" | "containerfile" => return Some("dockerfile"),
        "justfile" | "rakefile" | "gemfile" | "podfile" => return Some("ruby"),
        _ => {}
    }
    Some(match extension(path).as_str() {
        "rs" => "rust",
        "py" | "pyi" | "pyw" => "python",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "sh" | "bash" | "zsh" | "fish" => "shell",
        "sql" => "sql",
        "lua" => "lua",
        "scala" => "scala",
        "dart" => "dart",
        "r" => "r",
        "pl" | "pm" => "perl",
        "ex" | "exs" => "elixir",
        "hs" => "haskell",
        "ml" | "mli" => "ocaml",
        "vue" | "svelte" => "web",
        "css" | "scss" | "sass" | "less" => "css",
        "html" | "htm" => "html",
        "proto" => "protobuf",
        "gradle" => "gradle",
        "tf" => "terraform",
        _ => return None,
    })
}

/// Package manifests: the file name is the evidence.
pub fn is_manifest(path: &str) -> bool {
    matches!(
        file_name(path).to_ascii_lowercase().as_str(),
        "package.json" | "cargo.toml" | "pyproject.toml" | "go.mod" | "requirements.txt"
    )
}

const OTHER_EXTENSIONS: &[&str] = &[
    "log", "out", "err", "lock", "json", "jsonl", "ndjson", "yaml", "yml", "toml", "ini", "cfg",
    "conf", "env", "xml", "svg", "map", "sum", "tmp", "bak", "dat", "pem", "csr", "key", "patch",
    "diff",
];
const OTHER_NAMES: &[&str] = &[
    "license",
    "licence",
    "notice",
    "copying",
    "authors",
    "patents",
    "version",
    "changelog",
];

/// The profile from path and blob format alone (`format` is `_oo_docs.blob.format`; `None` when not known yet).
/// Rows of a file list use this when the content based profile is not stored yet.
pub fn classify_path(path: &str, format: Option<&str>) -> Classified {
    let ext = extension(path);
    let name = file_name(path).to_ascii_lowercase();
    let c = |profile, reason| Classified { profile, reason };
    match format {
        Some("audio" | "video" | "image") => return c(Profile::Media, "format"),
        Some("csv" | "xlsx") => return c(Profile::Table, "format"),
        Some("zip" | "unknown" | "legacy_office") => {
            // The extension of a binary container is not a profile; a legacy office file is still a document.
            return if format == Some("legacy_office") {
                c(Profile::Business, "format")
            } else {
                c(Profile::Other, "format")
            };
        }
        _ => {}
    }
    if matches!(ext.as_str(), "csv" | "tsv" | "xlsx" | "xls") {
        return c(Profile::Table, "ext");
    }
    if matches!(
        ext.as_str(),
        "mp3"
            | "wav"
            | "m4a"
            | "flac"
            | "mp4"
            | "mov"
            | "mkv"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "heic"
    ) {
        return c(Profile::Media, "ext");
    }
    if instruction_name(path) {
        return c(Profile::Instruction, "name");
    }
    if is_manifest(path) || code_language(path).is_some() {
        return c(
            Profile::Code,
            if is_manifest(path) { "name" } else { "ext" },
        );
    }
    if meeting_by_path(path) && !matches!(ext.as_str(), "log" | "json" | "yaml" | "yml") {
        return c(Profile::Meeting, "dir");
    }
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
    if OTHER_EXTENSIONS.contains(&ext.as_str())
        || OTHER_NAMES.contains(&stem)
        || name.starts_with('.')
    {
        return c(Profile::Other, "ext");
    }
    match ext.as_str() {
        "" => c(Profile::Other, "default"),
        _ => c(Profile::Business, "default"),
    }
}

fn head_text(ir: &DocIr, n: usize) -> impl Iterator<Item = &oo_core::docir::Block> {
    ir.blocks
        .iter()
        .filter(|b| !matches!(b.kind, BlockKind::PageHeader | BlockKind::PageFooter))
        .take(n)
}

fn starts_with_word(text: &str, words: &[&str]) -> bool {
    let t = text
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .to_ascii_lowercase();
    words.iter().any(|w| {
        t.starts_with(w)
            && t[w.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() || !w.is_ascii())
    })
}

/// A research paper reads as one: an abstract and (references or a DOI/arXiv id) or references and a DOI/arXiv id.
pub fn looks_like_paper(ir: &DocIr) -> bool {
    let mut abstract_ = false;
    let mut refs = false;
    let mut id = false;
    for (i, b) in ir.blocks.iter().take(6000).enumerate() {
        let t = b.text.trim();
        if t.is_empty() {
            continue;
        }
        let short = t.chars().count() <= 60;
        if !abstract_
            && i < 60
            && starts_with_word(t, &["abstract", "摘要", "summary"])
            && (b.kind == BlockKind::Heading
                || short
                || t.to_ascii_lowercase().starts_with("abstract")
                || t.starts_with("摘要"))
        {
            abstract_ = true;
        }
        if !refs
            && short
            && starts_with_word(
                t,
                &["references", "bibliography", "参考文献", "literature cited"],
            )
            && (b.kind == BlockKind::Heading || short)
        {
            refs = true;
        }
        if !id {
            let lower = t.to_ascii_lowercase();
            id = lower.contains("doi.org/10.")
                || lower.contains("doi:10.")
                || lower.contains("doi: 10.")
                || lower.contains("arxiv:")
                || lower.contains("arxiv.org/abs/");
        }
    }
    (abstract_ && (refs || id)) || (refs && id)
}

/// A meeting reads as one: an attendee list near the top, or a meeting type in the frontmatter.
pub fn looks_like_meeting(ir: &DocIr) -> bool {
    if ir
        .blocks
        .iter()
        .take(80)
        .any(|b| b.kind == BlockKind::Utterance)
    {
        return true;
    }
    if let Some(fm) = &ir.meta.frontmatter
        && fm.get("type").and_then(|v| v.as_str()).is_some_and(|t| {
            matches!(
                t.trim().to_ascii_lowercase().as_str(),
                "meeting" | "interview" | "minutes" | "会议" | "访谈"
            )
        })
    {
        return true;
    }
    head_text(ir, 30).any(|b| {
        let t = b.text.trim();
        matches!(
            b.kind,
            BlockKind::Kv | BlockKind::Paragraph | BlockKind::ListItem | BlockKind::Heading
        ) && t.chars().count() <= 200
            && starts_with_word(
                t,
                &[
                    "attendees",
                    "participants",
                    "参会人",
                    "参会",
                    "与会",
                    "出席",
                    "interviewee",
                    "被访谈",
                    "受访",
                ],
            )
    })
}

fn format_name(f: DocFormat) -> &'static str {
    match f {
        DocFormat::Md => "md",
        DocFormat::Txt => "txt",
        DocFormat::Csv => "csv",
        DocFormat::Xlsx => "xlsx",
        DocFormat::Docx => "docx",
        DocFormat::Pptx => "pptx",
        DocFormat::Pdf => "pdf",
        DocFormat::PdfScanned => "pdf_scanned",
        DocFormat::Audio => "audio",
        DocFormat::Video => "video",
        DocFormat::Image => "image",
    }
}

/// The profile of a parsed file: [`classify_path`] first, then the content decides between prose profiles.
pub fn classify(path: &str, ir: &DocIr) -> Classified {
    let by_path = classify_path(path, Some(format_name(ir.format)));
    let prose = matches!(
        ir.format,
        DocFormat::Md
            | DocFormat::Txt
            | DocFormat::Docx
            | DocFormat::Pdf
            | DocFormat::PdfScanned
            | DocFormat::Pptx
    );
    match by_path.profile {
        Profile::Business | Profile::Meeting | Profile::Other if prose => {
            if looks_like_paper(ir) {
                return Classified {
                    profile: Profile::Paper,
                    reason: "content",
                };
            }
            if by_path.profile != Profile::Meeting && looks_like_meeting(ir) {
                return Classified {
                    profile: Profile::Meeting,
                    reason: "content",
                };
            }
            // A text file without a known extension that holds prose is a document, not an unknown blob.
            if by_path.profile == Profile::Other
                && by_path.reason == "default"
                && ir
                    .blocks
                    .iter()
                    .filter(|b| b.kind == BlockKind::Paragraph)
                    .take(3)
                    .count()
                    >= 2
            {
                return Classified {
                    profile: Profile::Business,
                    reason: "content",
                };
            }
            by_path
        }
        _ => by_path,
    }
}

/// Run the extractor of the profile on a parsed file; entities and facts go into `col`.
pub(crate) fn apply(class: &Classified, path: &str, ir: &DocIr, col: &mut Collector) {
    match class.profile {
        Profile::Instruction => instruction::extract(path, ir, col),
        Profile::Code => code::extract(path, ir, col),
        Profile::Paper => paper::extract(path, ir, col),
        _ => {}
    }
}

/// What the profile extractor of `path` finds, on its own (the rules of 10 section 5.1 add the rest). For tests and
/// measurements.
pub fn extraction(path: &str, ir: &DocIr) -> crate::extract::model::Extraction {
    let class = classify(path, ir);
    let mut col = Collector::default();
    apply(&class, path, ir, &mut col);
    col.finish(path)
}

/// Store the profile on the file (`_oo_docs.file.profile`). Idempotent; no revision bump, no event.
pub async fn store(
    conn: &mut sqlx::PgConnection,
    file_id: &str,
    profile: Profile,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE _oo_docs.file SET profile = $2 WHERE id = $1::uuid AND profile IS DISTINCT FROM $2",
    )
    .bind(file_id)
    .bind(profile.as_str())
    .execute(&mut *conn)
    .await?;
    Ok(())
}
