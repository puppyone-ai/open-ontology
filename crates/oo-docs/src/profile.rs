//! Document profiles and admission (D94): every file is classified first, and the class decides whether and how the file enters
//! the ontology.
//!
//! The class is chosen by rules (user override, frontmatter, file name, extension, directory, blob format, content structure);
//! the model is asked only when the rules cannot decide. Nothing here reads a file's body for entities except the `record` and
//! `reference` classes:
//!
//! | class | profiles | what happens |
//! |---|---|---|
//! | `record` | `business`, `meeting`, `table` | enters the ontology through the per-class templates (core rules of 10 section 5.1, model supplement) |
//! | `instruction` | `instruction` | no entities; the file is attached as an instruction (working convention) to its project or directory; shared with an Agent its original text goes into the BRIEF section 工作约定 |
//! | `reference` | `reference` | one `Reference` instance (title, authors, year, venue, cites); no body extraction |
//! | `code` | `code` | no content extraction; structural facts only (project, CODEOWNERS owners) |
//! | `media` | `media` | the media rules (lane AL, D98) |
//! | `other` | `other` | archive only, searchable by file name |
//!
//! "Not in the ontology" is a normal state, not an error: the app shows the class and the reason.

pub mod code;
pub mod instruction;
pub mod reference;

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
    Reference,
    Media,
    Other,
}

/// The admission class of D94: where a file goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    Record,
    Instruction,
    Reference,
    Code,
    Media,
    Other,
}

impl Class {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::Instruction => "instruction",
            Self::Reference => "reference",
            Self::Code => "code",
            Self::Media => "media",
            Self::Other => "other",
        }
    }

    /// Whether files of this class put entities into the ontology (a `reference` only its `Reference` instances).
    pub const fn admits_entities(self) -> bool {
        matches!(self, Self::Record | Self::Reference | Self::Media)
    }
}

impl Profile {
    pub const ALL: [Profile; 8] = [
        Self::Business,
        Self::Meeting,
        Self::Table,
        Self::Instruction,
        Self::Code,
        Self::Reference,
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
            Self::Reference => "reference",
            Self::Media => "media",
            Self::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }

    /// The admission class (D94).
    pub const fn class(self) -> Class {
        match self {
            Self::Business | Self::Meeting | Self::Table => Class::Record,
            Self::Instruction => Class::Instruction,
            Self::Reference => Class::Reference,
            Self::Code => Class::Code,
            Self::Media => Class::Media,
            Self::Other => Class::Other,
        }
    }

    /// Whether the model supplement (10 section 5.2) runs: records and media only.
    pub const fn uses_model(self) -> bool {
        matches!(self.class(), Class::Record | Class::Media)
    }

    /// The reason the app shows for a file that does not enter the ontology.
    pub const fn admitted(self) -> bool {
        self.class().admits_entities()
    }
}

/// What the classifier decided and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Classified {
    pub profile: Profile,
    /// Short code of the deciding evidence (`name`, `dir`, `ext`, `format`, `content`, `default`).
    pub reason: &'static str,
    /// False when no rule decided and the profile is the default (the model may be asked).
    pub decided: bool,
}

/// `Thing` label of a reference (a paper, a book, an external report): the 文献 instance.
pub const REFERENCE: &str = "Reference";

/// Every label the profile extractors own: registered, never proposed for promotion.
pub const LABELS: &[&str] = &[REFERENCE];

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

/// `attr.<name>` fact with an enum value.
pub(crate) fn attr_enum(
    col: &mut Collector,
    subject: &EntityRef,
    name: &str,
    value: &str,
    rule: &'static str,
    conf: u16,
    ev: &EvidenceRef,
) {
    col.fact(FactMention {
        subject: subject.clone(),
        predicate: Predicate::Attr(name.to_owned()),
        object: None,
        value: Some(serde_json::json!({ "enum": value })),
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
    if stem == "sop"
        || stem.starts_with("sop-")
        || stem.starts_with("sop_")
        || stem.ends_with("-sop")
        || stem.contains("操作规范")
        || stem.contains("操作手册")
        || stem.contains("runbook")
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
    if dirs.iter().any(|d| {
        matches!(
            d.as_str(),
            "sop" | "sops" | "runbooks" | "playbooks" | "操作规范"
        )
    }) && textual
    {
        return true;
    }
    if dirs.iter().any(|d| d == ".claude") && textual {
        return true;
    }
    if textual
        && dirs.iter().any(|d| {
            matches!(
                d.as_str(),
                "adr" | "adrs" | "architecture-decisions" | "rfcs" | "rfc"
            )
        })
    {
        return true;
    }
    // `docs/decisions/0007-title.md`: a numbered record inside a documentation tree. A `Decisions/` folder of a
    // business Drive is not one.
    if textual
        && dirs.iter().any(|d| d == "decisions")
        && dirs
            .iter()
            .any(|d| matches!(d.as_str(), "docs" | "doc" | "documentation"))
        && name.chars().take_while(char::is_ascii_digit).count() >= 3
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

/// CODEOWNERS: a code file, read for its owners only.
pub fn is_codeowners(path: &str) -> bool {
    file_name(path).eq_ignore_ascii_case("codeowners")
}

/// Directories and extensions that hold references: papers, books, external reports.
fn reference_by_path(path: &str, ext: &str) -> bool {
    if matches!(ext, "epub" | "mobi" | "bib") {
        return true;
    }
    dir_segments(path).iter().any(|d| {
        matches!(
            d.as_str(),
            "papers"
                | "paper"
                | "literature"
                | "references"
                | "books"
                | "文献"
                | "论文"
                | "参考文献"
                | "书籍"
        )
    }) && matches!(ext, "pdf" | "docx" | "md" | "txt" | "epub")
}

/// The profile from path and blob format alone (`format` is `_oo_docs.blob.format`; `None` when not known yet).
/// Rows of a file list use this when the content based profile is not stored yet.
pub fn classify_path(path: &str, format: Option<&str>) -> Classified {
    let ext = extension(path);
    let name = file_name(path).to_ascii_lowercase();
    let c = |profile, reason: &'static str| Classified {
        profile,
        reason,
        decided: reason != "default",
    };
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
    if reference_by_path(path, &ext) {
        return c(Profile::Reference, "dir");
    }
    if is_manifest(path) || is_codeowners(path) || code_language(path).is_some() {
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

/// A reference (paper, book, external report) reads as one: an abstract and (references or a DOI/arXiv id) or references and a DOI/arXiv id.
pub fn looks_like_reference(ir: &DocIr) -> bool {
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
    let done = |profile, reason| Classified {
        profile,
        reason,
        decided: true,
    };
    // Frontmatter says what the author says it is: `type: instruction | reference | sop | ...`.
    if prose
        && by_path.profile != Profile::Code
        && let Some(p) = frontmatter_profile(ir)
    {
        return done(p, "frontmatter");
    }
    match by_path.profile {
        Profile::Business | Profile::Meeting | Profile::Other if prose => {
            if looks_like_reference(ir) {
                return done(Profile::Reference, "content");
            }
            if by_path.profile != Profile::Meeting && looks_like_meeting(ir) {
                return done(Profile::Meeting, "content");
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
                    reason: "default",
                    decided: false,
                };
            }
            by_path
        }
        _ => by_path,
    }
}

/// `type:` (or `class:`) of the frontmatter, when it names a class.
fn frontmatter_profile(ir: &DocIr) -> Option<Profile> {
    let fm = ir.meta.frontmatter.as_ref()?;
    let v = fm.get("type").or_else(|| fm.get("class"))?.as_str()?;
    Some(match v.trim().to_ascii_lowercase().as_str() {
        "instruction" | "instructions" | "convention" | "conventions" | "sop" | "runbook"
        | "指令" | "工作约定" | "操作规范" => Profile::Instruction,
        "reference" | "paper" | "book" | "literature" | "report-external" | "文献" | "论文" => {
            Profile::Reference
        }
        "code" | "source" => Profile::Code,
        "meeting" | "interview" | "minutes" | "会议" | "访谈" => Profile::Meeting,
        "record" | "decision" | "weekly" | "contract" | "person" | "project" | "preference" => {
            Profile::Business
        }
        "other" | "archive" => Profile::Other,
        _ => return None,
    })
}

/// A user's choice for a file or a directory (`_oo_docs.profile_override`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Override {
    /// Drive-relative path of the file or directory (no trailing slash).
    pub path: String,
    pub dir: bool,
    pub profile: Profile,
}

/// The override that applies to `path`: the file's own, else the nearest directory above it.
pub fn override_for(path: &str, overrides: &[Override]) -> Option<Profile> {
    if let Some(o) = overrides.iter().find(|o| !o.dir && o.path == path) {
        return Some(o.profile);
    }
    overrides
        .iter()
        .filter(|o| o.dir && (o.path.is_empty() || path.starts_with(&format!("{}/", o.path))))
        .max_by_key(|o| o.path.len())
        .map(|o| o.profile)
}

/// [`classify`] with the user's overrides on top.
pub fn classify_with(path: &str, ir: &DocIr, overrides: &[Override]) -> Classified {
    match override_for(path, overrides) {
        Some(profile) => Classified {
            profile,
            reason: "user",
            decided: true,
        },
        None => classify(path, ir),
    }
}

/// [`classify_path`] with the user's overrides on top (file lists).
pub fn classify_path_with(path: &str, format: Option<&str>, overrides: &[Override]) -> Classified {
    match override_for(path, overrides) {
        Some(profile) => Classified {
            profile,
            reason: "user",
            decided: true,
        },
        None => classify_path(path, format),
    }
}

/// Ask the model to decide a file the rules could not (D94): one word out of the six classes, from the name and the head
/// of the text. `None` when the model fails or answers something else; the rule default then stands.
pub async fn classify_by_model(
    port: &dyn crate::extract::llm::egress::LlmPort,
    path: &str,
    ir: &DocIr,
) -> Option<Profile> {
    let head: String = head_text(ir, 12)
        .map(|b| b.text.trim())
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(1500)
        .collect();
    let req = crate::extract::llm::prompt::ChatRequest {
        model: port.model().to_owned(),
        system: "Classify a file for an ontology. Answer with exactly one word: record (anything that names people, owners, dates, figures or decisions about a team's own work: meeting minutes, decisions, plans, test plans, specifications of a product the team builds, reports, chat or message logs, people or project tables, contracts), instruction (only instructions to an agent or an engineer: AGENTS.md, README, SOP, style or naming conventions), reference (paper, book, external report), code, media, other (only material unrelated to the team's work, archives and raw data dumps).".to_owned(),
        user: format!("File name: {path}\n\nStart of the text:\n{head}"),
    };
    let reply = port.complete(&req).await.ok()?;
    match reply
        .content
        .trim()
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_ascii_lowercase()
        .as_str()
    {
        "record" => Some(Profile::Business),
        "instruction" => Some(Profile::Instruction),
        "reference" => Some(Profile::Reference),
        "code" => Some(Profile::Code),
        "media" => Some(Profile::Media),
        "other" => Some(Profile::Other),
        _ => None,
    }
}

/// Run the extractor of the profile on a parsed file; entities and facts go into `col`. `siblings` are the other live paths
/// of the Drive (they tell which directory is the project).
pub(crate) fn apply(
    class: &Classified,
    path: &str,
    ir: &DocIr,
    siblings: &[String],
    col: &mut Collector,
) {
    match class.profile {
        Profile::Instruction => instruction::extract(path, ir, siblings, col),
        Profile::Code => code::extract(path, ir, siblings, col),
        Profile::Reference => reference::extract(path, ir, col),
        _ => {}
    }
}

/// What the profile extractor of `path` finds, on its own (the rules of 10 section 5.1 add the rest). For tests and
/// measurements.
pub fn extraction(path: &str, ir: &DocIr) -> crate::extract::model::Extraction {
    extraction_in(path, ir, &[])
}

/// [`extraction`] with the other paths of the Drive.
pub fn extraction_in(
    path: &str,
    ir: &DocIr,
    siblings: &[String],
) -> crate::extract::model::Extraction {
    let class = classify(path, ir);
    let mut col = Collector::default();
    apply(&class, path, ir, siblings, &mut col);
    col.finish(path)
}

/// File names (lower case) that mark the root of a project.
pub const MARKERS: &[&str] = &[
    "cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "agents.md",
    "claude.md",
    "codeowners",
];

/// The directory of the project a file belongs to: the nearest directory (the file's own first) that holds a project marker.
/// Empty string: the root of the Drive.
pub fn project_dir(path: &str, siblings: &[String]) -> String {
    let mut dir = path.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
    loop {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let has = siblings.iter().any(|s| {
            s.strip_prefix(&prefix).is_some_and(|rest| {
                !rest.contains('/') && MARKERS.contains(&rest.to_ascii_lowercase().as_str())
            })
        });
        if has {
            return dir;
        }
        match dir.rsplit_once('/') {
            Some((up, _)) => dir = up.to_owned(),
            None if dir.is_empty() => {
                return path.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
            }
            None => dir = String::new(),
        }
    }
}

/// Store the profile and why on the file (`_oo_docs.file.profile`, `profile_reason`). Idempotent; no revision bump, no event.
pub async fn store(
    conn: &mut sqlx::PgConnection,
    file_id: &str,
    class: &Classified,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE _oo_docs.file SET profile = $2, profile_reason = $3 WHERE id = $1::uuid \
            AND (profile IS DISTINCT FROM $2 OR profile_reason IS DISTINCT FROM $3)",
    )
    .bind(file_id)
    .bind(class.profile.as_str())
    .bind(class.reason)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The user's overrides of one Drive.
pub async fn load_overrides(
    conn: &mut sqlx::PgConnection,
    drive_id: &str,
) -> Result<Vec<Override>, sqlx::Error> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT path, scope, profile FROM _oo_docs.profile_override WHERE drive_id = $1::uuid ORDER BY scope, path",
    )
    .bind(drive_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(path, scope, profile)| {
            Some(Override {
                path,
                dir: scope == "dir",
                profile: Profile::parse(&profile)?,
            })
        })
        .collect())
}
