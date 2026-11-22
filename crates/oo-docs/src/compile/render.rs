//! Text building blocks shared by BRIEF and the pack files: AI labelling
//! (D66), names, slugs, fixed predicate wording, provenance tags. Depends on
//! [`VisibleGraph`] only (no store access, D26).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::kernels;
use super::source;
use super::visible::{VisibleEntity, VisibleEvidence, VisibleFact, VisibleGraph};
use crate::extract::model::Predicate;

/// First line of every generated Markdown file (90 §7).
pub const AIGC_LABEL_LINE: &str =
    "> AI生成 · 由 Super Ontology 根据您的文件自动整理，可能有误，请以原文为准。";
/// Placeholder until the company name is provided (90 §7); no timestamps.
pub const CONTENT_PRODUCER: &str = "<COMPANY>";

/// `aigc` object for `manifest.json` and `SHARE.json` (same structure as the
/// Markdown front matter).
pub fn aigc_object(reserved_code1: &str, produce_id: &str) -> Value {
    json!({
        "Label": "1",
        "ContentProducer": CONTENT_PRODUCER,
        "ProduceID": produce_id,
        "ReservedCode1": reserved_code1,
        "ContentPropagator": "",
        "PropagateID": "",
        "ReservedCode2": "",
    })
}

/// YAML front matter of a generated Markdown file.
pub fn front_matter(basis: &str, path: &str) -> String {
    let mut out: Vec<u8> = Vec::new();
    kernels::put_front_matter(&mut out, basis.as_bytes(), path.as_bytes());
    String::from_utf8(out).expect("front matter is UTF-8")
}

/// A whole generated Markdown file: label on line 1, then the front matter
/// block, then the body lines; ends with a single `\n`.
///
/// The label is the first line because 90 §7 and 30 §249 ask for a visible
/// first line; the YAML block follows after one blank line. A reader that
/// needs the metadata at byte 0 would have to move it; that is a product
/// choice flagged in the B-S2-08 report. The bytes are written by
/// [`kernels::put_md_file`], which `proofs.rs` proves (KP10).
pub fn md_file(basis: &str, path: &str, body: &[String]) -> String {
    let mut out: Vec<u8> = Vec::new();
    kernels::put_md_file(
        &mut out,
        basis.as_bytes(),
        path.as_bytes(),
        body.iter().map(|l| l.as_bytes()),
    );
    String::from_utf8(out).expect("generated Markdown is UTF-8")
}

/// File-name part of an entity: name NFC with `/\:*?"<>|` and control
/// characters replaced by `_`, cut to 40 bytes on a character boundary.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            || c.is_control()
        {
            '_'
        } else {
            c
        };
        if out.len() + c.len_utf8() > 40 {
            break;
        }
        out.push(c);
    }
    out
}

/// First 8 hex characters of the entity key's hash part.
pub fn key8(key: &str) -> String {
    key.split_once(':')
        .map_or(key, |(_, h)| h)
        .chars()
        .take(8)
        .collect()
}

/// `<slug>-<key8>.md` file name of an entity.
pub fn entity_file_name(e: &VisibleEntity) -> String {
    format!("{}-{}.md", slug(&e.name), key8(&e.key))
}

/// Fixed Chinese wording of a predicate (10 §7.3 step 5).
pub fn predicate_cn(p: &Predicate) -> String {
    match p {
        Predicate::Owns => "负责".into(),
        Predicate::MemberOf => "属于".into(),
        Predicate::Decides => "决定".into(),
        Predicate::Rejects => "否决了".into(),
        Predicate::TriedAndFailed => "尝试过但失败".into(),
        Predicate::Supersedes => "取代了".into(),
        Predicate::About => "关于".into(),
        Predicate::Mentions => "提及".into(),
        Predicate::Cites => "引用了".into(),
        Predicate::Authored => "撰写了".into(),
        Predicate::HasParticipant => "参会人".into(),
        Predicate::RecordedIn => "录制于".into(),
        Predicate::Attr(a) => format!("{}为", attr_cn(a)),
    }
}

fn attr_cn(name: &str) -> &str {
    match name {
        "status" => "状态",
        "role" => "职位",
        "author" => "作者",
        "created" => "创建时间",
        "definition" => "定义",
        "class" => "类别",
        "scope" => "适用范围",
        "owners" => "负责人",
        "venue" => "出处",
        "year" => "年份",
        "doi" => "DOI",
        "media_class" => "媒体类别",
        "duration_ms" => "时长（毫秒）",
        other => other,
    }
}

fn value_text(v: &Value) -> String {
    for k in ["text", "date", "enum"] {
        if let Some(s) = v.get(k).and_then(Value::as_str) {
            return s.to_owned();
        }
    }
    v.to_string()
}

/// `星图看板（Starmap、星图）`: name with at most two aliases.
pub fn entity_with_aliases(g: &VisibleGraph, e: &VisibleEntity) -> String {
    let names: Vec<&str> = g
        .aliases(&e.key)
        .iter()
        .filter(|a| a.norm != crate::resolve::normalize::norm(&e.name))
        .take(2)
        .map(|a| a.alias.as_str())
        .collect();
    if names.is_empty() {
        e.name.clone()
    } else {
        format!("{}（{}）", e.name, names.join("、"))
    }
}

fn time_text(f: &VisibleFact) -> String {
    match (&f.valid_from, &f.valid_to) {
        (Some(a), Some(b)) => format!("（{a}起至{b}）"),
        (Some(a), None) => format!("（{a}起）"),
        (None, Some(b)) => format!("（至{b}）"),
        (None, None) => String::new(),
    }
}

/// Full wording of a fact without its provenance tags.
pub fn fact_text(g: &VisibleGraph, f: &VisibleFact, subject_aliases: bool) -> String {
    let subject = g.entity(&f.subject);
    let name = subject.map_or_else(String::new, |s| {
        if subject_aliases {
            entity_with_aliases(g, s)
        } else {
            s.name.clone()
        }
    });
    let object = match (&f.object, &f.value) {
        (Some(o), _) => g.entity(o).map_or_else(String::new, |e| e.name.clone()),
        (None, Some(v)) => value_text(v),
        (None, None) => String::new(),
    };
    format!(
        "{name}：{} {object}{}",
        predicate_cn(&f.predicate),
        time_text(f)
    )
}

/// Evidence that provenance tags pointed at; becomes `refs/`.
#[derive(Default)]
pub struct Cites {
    map: BTreeMap<(String, String, String), VisibleEvidence>,
}

impl Cites {
    pub fn add(&mut self, e: &VisibleEvidence) {
        self.map
            .entry((e.path.clone(), source::frag(&e.anchor), e.block_id.clone()))
            .or_insert_with(|| e.clone());
    }

    /// Cited evidence in `(path, fragment, block id)` order.
    pub fn sorted(&self) -> Vec<&VisibleEvidence> {
        self.map.values().collect()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Provenance tags for a line: the first three of `evs` (sorted), then
/// `[src:+n]` for the rest. All of `evs` are registered as cited.
pub fn src_tags(evs: &[VisibleEvidence], cites: &mut Cites) -> String {
    for e in evs {
        cites.add(e);
    }
    let mut parts: Vec<String> = evs
        .iter()
        .take(3)
        .map(|e| source::tag(&e.path, &e.anchor))
        .collect();
    if evs.len() > 3 {
        parts.push(format!("[src:+{}]", evs.len() - 3));
    }
    parts.join(" ")
}

/// `- 冲突未决：<实体>：甲 [src] vs 乙 [src]` for each open conflict (10 §16.7).
/// The sides are shown only with in-scope evidence; the rest is a count.
pub fn conflict_lines(
    g: &VisibleGraph,
    conflicts: &[&super::visible::VisibleConflict],
    cites: &mut Cites,
) -> Vec<String> {
    let mut out = Vec::new();
    for c in conflicts {
        let name = g.entity(&c.entity_key).map_or("", |e| e.name.as_str());
        let sides: Vec<String> = c
            .sides
            .iter()
            .map(|s| {
                format!(
                    "{} {}",
                    fact_text(g, s, false),
                    src_tags(&s.evidence, cites)
                )
            })
            .collect();
        let mut line = format!("- 冲突未决：{name}：{}", sides.join(" vs "));
        if c.hidden_sides > 0 {
            line.push_str(&format!("（另有 {} 方来源不在共享范围内）", c.hidden_sides));
        }
        out.push(line);
    }
    out
}

/// Only the first evidence (summary and title lines).
pub fn first_src(evs: &[VisibleEvidence], cites: &mut Cites) -> String {
    match evs.first() {
        Some(e) => {
            cites.add(e);
            source::tag(&e.path, &e.anchor)
        }
        None => String::new(),
    }
}

/// All evidence behind an entity: its aliases and its facts, sorted, deduped.
pub fn entity_evidence(g: &VisibleGraph, key: &str) -> Vec<VisibleEvidence> {
    let mut out: Vec<VisibleEvidence> = g
        .aliases(key)
        .iter()
        .flat_map(|a| a.evidence.iter().cloned())
        .chain(g.facts_of(key).flat_map(|f| f.evidence.iter().cloned()))
        .collect();
    out.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_comes_first_then_front_matter_then_body() {
        let t = md_file(
            "abc",
            "BRIEF.md",
            &["# BRIEF".into(), "".into(), "x".into()],
        );
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], AIGC_LABEL_LINE);
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "---");
        assert!(t.contains("  ProduceID: \"abc/BRIEF.md\"\n"));
        assert!(t.contains("  ReservedCode1: \"abc\"\n"));
        assert!(t.contains("ContentProducer: \"<COMPANY>\""));
        assert!(t.ends_with("x\n") && !t.ends_with("\n\n"));
        assert!(!t.contains("T00:") && !t.contains("2026"), "no timestamps");
    }

    #[test]
    fn slugs_are_safe_and_bounded() {
        assert_eq!(slug("a/b\\c:d*e?f\"g<h>i|j\u{7}k"), "a_b_c_d_e_f_g_h_i_j_k");
        let long = "字".repeat(30);
        let s = slug(&long);
        assert_eq!(s.len(), 39, "13 characters of 3 bytes fit in 40 bytes");
        assert!(s.is_char_boundary(s.len()));
        assert_eq!(key8("person:0123456789abcdef01234567"), "01234567");
    }

    #[test]
    fn predicates_use_the_fixed_table() {
        assert_eq!(predicate_cn(&Predicate::Owns), "负责");
        assert_eq!(predicate_cn(&Predicate::TriedAndFailed), "尝试过但失败");
        assert_eq!(predicate_cn(&Predicate::Attr("status".into())), "状态为");
        assert_eq!(predicate_cn(&Predicate::Attr("custom".into())), "custom为");
    }

    #[test]
    fn aigc_object_matches_the_contract_shape() {
        let o = aigc_object("r1", "p/x");
        assert_eq!(o["Label"], "1");
        assert_eq!(o["ContentProducer"], "<COMPANY>");
        assert_eq!(o["ContentPropagator"], "");
    }
}
