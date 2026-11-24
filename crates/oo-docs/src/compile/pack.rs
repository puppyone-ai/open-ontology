//! The files of a pack other than BRIEF, plus `manifest.json` and
//! `SHARE.json` (10 §7.4). Depends on [`VisibleGraph`] only (D26).
//!
//! Layout: `people/<slug>-<key8>.md`, `projects/<slug>-<key8>.md`,
//! `decisions.md`, `terms.md`, `refs/<nnnn>.md`, `INDEX.md`; a file over
//! 4,000 tokens continues in `<stem>/_page/0002.md` and on, never truncated.
//! `Thing`, `Term`, `Topic`, `Event` and `Organization` entities all live in
//! `terms.md` (the spec names only Term and Thing for it; the others have no
//! file of their own, and BRIEF summary lines point there).

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::PackError;
use super::render::{
    Cites, aigc_object, entity_file_name, entity_with_aliases, fact_text, md_file, src_tags,
};
use super::tokens;
use super::visible::{VisibleEntity, VisibleGraph};
use crate::extract::model::EntityType;

/// One file of the pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub tokens: u64,
}

impl PackFile {
    pub fn text(path: &str, text: String) -> Self {
        Self {
            path: path.to_owned(),
            tokens: tokens::count(&text),
            bytes: text.into_bytes(),
        }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes).unwrap_or("")
    }
}

/// Token limit of one file; longer files continue on `_page/` files.
pub const MAX_FILE_TOKENS: u64 = 4000;
/// Largest pack, in bytes (`DOCS_PACK_TOO_LARGE`).
pub const MAX_PACK_BYTES: usize = 8 * 1024 * 1024;

pub fn sha256_hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(64);
    for b in Sha256::digest(data) {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Write `body` as `path`, continuing on page files when it would exceed
/// [`MAX_FILE_TOKENS`]. A single line is never split.
fn emit(files: &mut Vec<PackFile>, basis: &str, path: &str, body: &[String]) {
    let overhead = tokens::count(&md_file(basis, path, &[]));
    let limit = MAX_FILE_TOKENS.saturating_sub(overhead).max(1);
    let mut pages: Vec<Vec<String>> = vec![Vec::new()];
    let mut used = 0u64;
    for line in body {
        let t = tokens::count(line);
        let page = pages.last().map_or(0, Vec::len);
        if used + t > limit && page > 0 {
            pages.push(Vec::new());
            used = 0;
        }
        used += t;
        if let Some(p) = pages.last_mut() {
            p.push(line.clone());
        }
    }
    let stem = path.strip_suffix(".md").unwrap_or(path);
    for (i, page) in pages.iter().enumerate() {
        let p = if i == 0 {
            path.to_owned()
        } else {
            format!("{stem}/_page/{:04}.md", i + 1)
        };
        files.push(PackFile::text(&p, md_file(basis, &p, page)));
    }
}

fn entity_section(
    g: &VisibleGraph,
    e: &VisibleEntity,
    level: &str,
    cites: &mut Cites,
) -> Vec<String> {
    let mut out = vec![format!("{level} {}", e.name)];
    let mut meta = format!("类型：{}", e.ty.as_str());
    if let Some(label) = &e.thing_label {
        meta.push_str(&format!("（候选类型：{label}）"));
    }
    out.push(meta);
    let aliases: Vec<&str> = g.aliases(&e.key).iter().map(|a| a.alias.as_str()).collect();
    if !aliases.is_empty() {
        out.push(format!("别名：{}", aliases.join("、")));
    }
    out.push(String::new());
    let facts: Vec<_> = g.facts_of(&e.key).collect();
    if facts.is_empty() {
        out.push("（无事实）".into());
    }
    for f in facts {
        out.push(format!(
            "- {} {}",
            fact_text(g, f, false),
            src_tags(&f.evidence, cites)
        ));
    }
    let mine: Vec<_> = g
        .conflicts()
        .iter()
        .filter(|c| c.entity_key == e.key)
        .collect();
    out.extend(super::render::conflict_lines(g, &mine, cites));
    for a in g.aliases(&e.key) {
        out.push(format!(
            "- 别名 {} {}",
            a.alias,
            src_tags(&a.evidence, cites)
        ));
    }
    out
}

/// Everything except BRIEF, manifest and SHARE: entity files, `decisions.md`,
/// `terms.md`, `refs/`, `INDEX.md`. `brief_file` is listed in INDEX; `cites`
/// already holds the BRIEF's citations and gets the rest added.
pub fn build_files(
    g: &VisibleGraph,
    basis: &str,
    brief_file: &PackFile,
    cites: &mut Cites,
) -> Result<Vec<PackFile>, PackError> {
    let mut files: Vec<PackFile> = Vec::new();

    for (ty, dir) in [
        (EntityType::Person, "people"),
        (EntityType::Project, "projects"),
    ] {
        for e in g.entities().filter(|e| e.ty == ty) {
            let path = format!("{dir}/{}", entity_file_name(e));
            let body = entity_section(g, e, "#", cites);
            emit(&mut files, basis, &path, &body);
        }
    }

    let mut decisions = vec!["# 决定与偏好".to_owned(), String::new(), "## 决定".into()];
    let mut any = false;
    for e in g
        .entities()
        .filter(|e| e.ty == EntityType::Decision && e.subkind.is_none())
    {
        any = true;
        decisions.extend(entity_section(g, e, "###", cites));
        decisions.push(String::new());
    }
    if !any {
        decisions.push("（无）".into());
    }
    decisions.push("## 偏好".into());
    any = false;
    for e in g
        .entities()
        .filter(|e| e.ty == EntityType::Decision && e.subkind.is_some())
    {
        any = true;
        decisions.extend(entity_section(g, e, "###", cites));
        decisions.push(String::new());
    }
    if !any {
        decisions.push("（无）".into());
    }
    emit(&mut files, basis, "decisions.md", &decisions);

    let mut terms = vec!["# 术语与其他".to_owned(), String::new()];
    for (heading, pick) in [
        ("## Term", &[EntityType::Term][..]),
        ("## Thing（候选类型）", &[EntityType::Thing][..]),
        (
            "## 其他（Topic、Event、Organization）",
            &[
                EntityType::Topic,
                EntityType::Event,
                EntityType::Organization,
            ][..],
        ),
    ] {
        terms.push(heading.to_owned());
        let mut any = false;
        for e in g.entities().filter(|e| pick.contains(&e.ty)) {
            any = true;
            terms.extend(entity_section(g, e, "###", cites));
            terms.push(String::new());
        }
        if !any {
            terms.push("（无）".into());
        }
    }
    emit(&mut files, basis, "terms.md", &terms);

    // refs/: one file per cited evidence, in citation order.
    let cited: Vec<_> = cites.sorted().into_iter().cloned().collect();
    for (i, ev) in cited.iter().enumerate() {
        let path = format!("refs/{:04}.md", i + 1);
        let quote = g
            .quote(ev)
            .unwrap_or_else(|_| "（原文片段不可用）".to_owned());
        let longest = quote
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(2)
            + 1;
        let fence = "`".repeat(longest);
        let body = vec![
            format!("# 出处 {:04}", i + 1),
            format!("文件：{}", ev.path),
            format!("位置：{}", super::source::frag(&ev.anchor)),
            String::new(),
            fence.clone(),
            quote,
            fence,
        ];
        files.push(PackFile::text(&path, md_file(basis, &path, &body)));
    }

    // INDEX.md: every other file with its token count, then the entities.
    let mut listed: Vec<&PackFile> = files.iter().collect();
    listed.push(brief_file);
    listed.sort_by(|a, b| a.path.cmp(&b.path));
    let mut index = vec!["# INDEX".to_owned(), String::new(), "## 文件".into()];
    for f in &listed {
        index.push(format!("- {} · {} tokens", f.path, f.tokens));
    }
    index.push(String::new());
    index.push("## 实体".into());
    for e in g.entities().filter(|e| e.ty != EntityType::Document) {
        let target = match e.ty {
            EntityType::Person => format!("people/{}", entity_file_name(e)),
            EntityType::Project => format!("projects/{}", entity_file_name(e)),
            EntityType::Decision => "decisions.md".to_owned(),
            _ => "terms.md".to_owned(),
        };
        index.push(format!(
            "- {}（{}）→ {target}",
            entity_with_aliases(g, e),
            e.ty.as_str()
        ));
    }
    files.push(PackFile::text(
        "INDEX.md",
        md_file(basis, "INDEX.md", &index),
    ));
    Ok(files)
}

/// Pack-level numbers the manifest needs.
pub struct ManifestInput<'a> {
    pub compiler: &'a str,
    pub version: i64,
    pub drive_id: &'a str,
    pub scope: &'a [String],
    pub snapshot_sha256: &'a str,
    pub params_sha256: &'a str,
    pub basis: &'a str,
    pub trimmed: Value,
    pub blocked_facts: u64,
    pub blocked_entities: u64,
    pub guarantees: super::coverage::Guarantees,
    pub coverage: Option<&'a super::coverage::Coverage>,
}

/// `manifest.json` value and the pack digest (`sha256(canonical(manifest))`).
/// `files` must not contain `manifest.json` or `SHARE.json`.
pub fn manifest(files: &[PackFile], m: &ManifestInput<'_>) -> (Value, String) {
    let mut sorted: Vec<&PackFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let list: Vec<Value> = sorted
        .iter()
        .map(|f| {
            let mut v = json!({
                "path": f.path,
                "sha256": sha256_hex(&f.bytes),
                "bytes": f.bytes.len(),
                "tokens": f.tokens,
            });
            if f.path.ends_with(".md") {
                v["section_digests"] = json!(super::sections::section_digests(f.as_str()));
            }
            v
        })
        .collect();
    let mut manifest = json!({
        "protocol": "oo.context-pack.manifest/v1",
        "compiler": m.compiler,
        "token_estimator": tokens::ESTIMATOR,
        "version": m.version,
        "drive_id": m.drive_id,
        "scope": m.scope,
        "snapshot_sha256": m.snapshot_sha256,
        "params_sha256": m.params_sha256,
        "files": list,
        "trimmed": m.trimmed,
        "blocked": {"facts": m.blocked_facts, "entities": m.blocked_entities},
        "guarantees": m.guarantees.to_json(),
        "aigc": aigc_object(m.basis, &format!("{}/manifest.json", m.basis)),
    });
    if let Some(c) = m.coverage {
        manifest["coverage"] = c.to_json();
    }
    let digest = sha256_hex(oo_builder::canonical_json(&manifest).as_bytes());
    (manifest, digest)
}

/// Fields of `SHARE.json` that are not part of the deterministic pack.
#[derive(Clone, Debug)]
pub struct ShareFields {
    pub share_id: String,
    pub grantor: String,
    pub created_at: String,
    pub expires_at: String,
    pub refs_endpoint: String,
    /// Starts with `oodr_`.
    pub refs_credential: String,
    pub refs_expires_at: String,
}

/// Pack-level values `SHARE.json` repeats.
pub struct ShareInput<'a> {
    pub pack_digest: &'a str,
    pub basis: &'a str,
    pub role: &'a str,
    pub scope: &'a [String],
    pub task: Option<&'a str>,
    pub budget: u32,
    pub version: i64,
    pub drive_id: &'a str,
    pub backend: &'a str,
    pub snapshot_sha256: &'a str,
    pub blocked_facts: u64,
    pub blocked_entities: u64,
    pub guarantees: super::coverage::Guarantees,
}

/// `SHARE.json` (`oo.context-pack.share/v1`) as canonical JSON plus `\n`.
pub fn share_json(s: &ShareInput<'_>, f: &ShareFields) -> PackFile {
    let v = json!({
        "protocol": "oo.context-pack.share/v1",
        "share_id": f.share_id,
        "pack_digest": s.pack_digest,
        "role": s.role,
        "scope": s.scope,
        "task": s.task,
        "budget": s.budget,
        "ontology_version": s.version,
        "drive": {"id": s.drive_id, "backend": s.backend, "snapshot_sha256": s.snapshot_sha256},
        "created_at": f.created_at,
        "expires_at": f.expires_at,
        "grantor": f.grantor,
        "blocked": {"facts": s.blocked_facts, "entities": s.blocked_entities},
        "guarantees": s.guarantees.to_json(),
        "refs": {"endpoint": f.refs_endpoint, "credential": f.refs_credential, "expires_at": f.refs_expires_at},
        "notice": "撤销后本目录会被删除；已读取的内容无法收回。",
        "aigc": aigc_object(s.pack_digest, &format!("{}/SHARE.json", s.basis)),
    });
    let mut text = oo_builder::canonical_json(&v);
    text.push('\n');
    PackFile::text("SHARE.json", text)
}
