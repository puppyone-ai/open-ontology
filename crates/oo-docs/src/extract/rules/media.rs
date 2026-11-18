//! Rules for audio and video IR (10 17.4, D74, board B-S10-08): who is speaking, and what a speaker says about
//! what they own. Pure and deterministic.
//!
//! Speaker labels (`S1`, `S2`) are file-local and carry no identity. A statement such as "我负责星火计划" is
//! recorded for a placeholder Person, `说话人 S1 · <blob prefix>`, whose facts stay drafts: the publisher holds
//! them until a person confirms who `S1` is (`review::speaker`), when the placeholder is merged into the named
//! Person and the facts publish like any others. No voiceprint is stored or compared.
//!
//! Candidates for "who is S1" come in three layers, all of which end in a `speaker_identity` review item:
//! 1. `media.attendees`: meeting metadata, the attendee names in the file name (`参会：甲、乙`) or in a minutes
//!    file with the same stem (`参会人：甲、乙`); a set of names, not a mapping;
//! 2. self introductions and being called by name (`我是小王`, `小王你说一下`), here by pattern; the LLM
//!    supplement produces the same [`SpeakerCandidate`] values from `speaker_is(S1, 小王)` entries;
//! 3. the person's decision in the review queue.

use std::collections::BTreeSet;

use oo_core::docir::{Anchor, Block, BlockKind, DocFormat, DocIr};

use super::kv;
use super::{Ctx, EvidenceRef};
use crate::extract::model::{EntityRef, EntityType, Predicate};

/// Prefix of the placeholder Person name of one speaker.
pub const PLACEHOLDER_PREFIX: &str = "说话人 ";
/// Confidence of a statement made by a speaker whose identity is still unknown (held until the speaker is confirmed).
const STATEMENT_BP: u16 = 9000;

/// Where a candidate name came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateSource {
    /// Layer 1: attendee list from the file name or a minutes file.
    Attendees,
    /// Layer 2: the speaker named themselves.
    SelfIntro,
    /// Layer 2: another speaker called this one by name.
    Addressed,
    /// Layer 2: the LLM supplement (`speaker_is`).
    Llm,
}

impl CandidateSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attendees => "attendees",
            Self::SelfIntro => "self_intro",
            Self::Addressed => "addressed",
            Self::Llm => "llm",
        }
    }
}

/// One proposed mapping `speaker -> name`. `speaker` is `None` for the attendee set, which names people but not
/// who is which.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpeakerCandidate {
    pub speaker: Option<String>,
    pub name: String,
    pub source: CandidateSource,
    /// Block of the sentence that says so; `None` for the attendee set.
    pub block_id: Option<String>,
    /// The sentence, at most 80 characters, for the review card.
    pub quote: Option<String>,
}

/// `说话人 S1 · 1a2b3c4d`: unique per file, never shown as a person's name once confirmed.
pub fn placeholder_name(blob_b3: &str, speaker: &str) -> String {
    format!(
        "{PLACEHOLDER_PREFIX}{speaker} · {}",
        blob_b3.get(..8).unwrap_or(blob_b3)
    )
}

/// True for a placeholder name.
pub fn is_placeholder(name: &str) -> bool {
    name.starts_with(PLACEHOLDER_PREFIX) && name.contains(" · ")
}

/// The speaker label of a block, if it is an utterance.
fn speaker_of(b: &Block) -> Option<&str> {
    match (&b.kind, &b.anchor) {
        (BlockKind::Utterance, Anchor::Media { speaker, .. }) => speaker.as_deref(),
        _ => None,
    }
}

/// Speaker labels of the file in order of first appearance.
pub fn speakers(ir: &DocIr) -> Vec<String> {
    let mut seen = Vec::new();
    for b in &ir.blocks {
        if let Some(s) = speaker_of(b)
            && !seen.iter().any(|x| x == s)
        {
            seen.push(s.to_owned());
        }
    }
    seen
}

fn clip(s: &str) -> String {
    s.trim().chars().take(80).collect()
}

fn plausible_name(n: &str) -> bool {
    let c = n.chars().count();
    (2..=4).contains(&c)
        && n.chars()
            .all(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch) || ch.is_ascii_alphabetic())
}

/// Layer 1. `path` is the media file's Drive path; `minutes` the text of the minutes file with the same stem,
/// when there is one. Names are NFC-trimmed, de-duplicated and sorted.
pub fn attendees(path: &str, minutes: Option<&str>) -> Vec<SpeakerCandidate> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    let stem = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit_once('.')
        .map_or(path, |(s, _)| s);
    for key in ["参会人", "参会", "与会", "attendees"] {
        for sep in ['=', '：', ':'] {
            if let Some(i) = stem.find(&format!("{key}{sep}")) {
                let rest = &stem[i + key.len() + sep.len_utf8()..];
                let rest = rest.replace(['_', '+', '&'], "、");
                for p in kv::split_names(&rest) {
                    names.insert(p.name);
                }
            }
        }
    }
    if let Some(text) = minutes {
        for line in text.lines() {
            if let Some((k, v)) = kv::split_kv(line)
                && matches!(
                    k.as_str(),
                    "参会人" | "参会人员" | "与会人" | "出席" | "attendees"
                )
            {
                for p in kv::split_names(&v) {
                    names.insert(p.name);
                }
            }
        }
    }
    names
        .into_iter()
        .filter(|n| plausible_name(n))
        .map(|name| SpeakerCandidate {
            speaker: None,
            name,
            source: CandidateSource::Attendees,
            block_id: None,
            quote: None,
        })
        .collect()
}

fn after<'a>(text: &'a str, markers: &[&str]) -> Option<&'a str> {
    markers
        .iter()
        .filter_map(|m| text.find(m).map(|i| &text[i + m.len()..]))
        .min_by_key(|rest| rest.len())
        .map(|rest| rest.trim_start())
}

fn leading_name(s: &str) -> Option<String> {
    let name: String = s
        .chars()
        .take_while(|c| ('\u{4e00}'..='\u{9fff}').contains(c) || c.is_ascii_alphabetic())
        .collect();
    // Longest plausible prefix that is followed by a break (so `我是项目经理` is not a name).
    for len in (2..=4).rev() {
        if name.chars().count() >= len {
            let cand: String = name.chars().take(len).collect();
            let rest: String = s.chars().skip(len).collect();
            let boundary = rest.chars().next().is_none_or(|c| {
                !('\u{4e00}'..='\u{9fff}').contains(&c) || "的是在来负说讲你您啊吧呢".contains(c)
            });
            if plausible_name(&cand) && boundary && !is_role_word(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

fn is_role_word(w: &str) -> bool {
    const ROLES: &[&str] = &[
        "项目经理",
        "负责人",
        "产品经理",
        "工程师",
        "设计师",
        "测试",
        "老师",
        "助理",
        "实习生",
        "新同事",
        "主持人",
        "记录员",
    ];
    ROLES.iter().any(|r| w.starts_with(r) || r.starts_with(w))
}

/// Layer 2 by pattern: `我是X`, `我叫X` (the speaker is X) and `X你说一下` / `X，你…` (the next other speaker is X).
pub fn introductions(ir: &DocIr) -> Vec<SpeakerCandidate> {
    let mut out: BTreeSet<SpeakerCandidate> = BTreeSet::new();
    let utt: Vec<(&Block, &str)> = ir
        .blocks
        .iter()
        .filter_map(|b| speaker_of(b).map(|s| (b, s)))
        .collect();
    for (i, (b, spk)) in utt.iter().enumerate() {
        let text = b.text.trim();
        if let Some(rest) = after(text, &["我是", "我叫", "我名字叫"])
            && let Some(name) = leading_name(rest)
        {
            out.insert(SpeakerCandidate {
                speaker: Some((*spk).to_owned()),
                name,
                source: CandidateSource::SelfIntro,
                block_id: Some(b.id.clone()),
                quote: Some(clip(text)),
            });
        }
        // `小王你说一下`: a name, then 你/您.
        let chars: Vec<char> = text.chars().collect();
        for len in 2..=4usize {
            let Some(&next) = chars.get(len) else { break };
            let name: String = chars[..len].iter().collect();
            let mut rest = chars[len..].iter().peekable();
            while rest.peek().is_some_and(|c| matches!(c, '，' | ',' | ' ')) {
                rest.next();
            }
            let called = rest.next().is_some_and(|c| matches!(c, '你' | '您'));
            if (next == '你' || next == '您' || next == '，' || next == ',' || next == ' ')
                && called
                && plausible_name(&name)
                && !is_role_word(&name)
                && let Some((_, other)) = utt[i + 1..].iter().find(|(_, s)| s != spk)
            {
                out.insert(SpeakerCandidate {
                    speaker: Some((*other).to_owned()),
                    name,
                    source: CandidateSource::Addressed,
                    block_id: Some(b.id.clone()),
                    quote: Some(clip(text)),
                });
                break;
            }
        }
    }
    out.into_iter().collect()
}

/// `media.statement`: "我负责X" in an utterance becomes `owns(<placeholder of the speaker>, X)` with the
/// utterance as evidence. The subject is the placeholder; the publisher holds the fact until the speaker is
/// confirmed.
pub(super) fn apply(ctx: &mut Ctx<'_>, ir: &DocIr) {
    if !matches!(ir.format, DocFormat::Audio | DocFormat::Video) {
        return;
    }
    for b in &ir.blocks {
        let Some(spk) = speaker_of(b) else { continue };
        let ev = EvidenceRef::from_block(b);
        for sentence in b.text.split(['。', '！', '？', '.', '\n']) {
            let Some(rest) = after(sentence.trim(), &["我负责", "我来负责"]) else {
                continue;
            };
            let thing = kv::clean_item(rest, 60);
            let thing = thing.trim_end_matches(['的', '了']).trim().to_owned();
            if thing.is_empty() || thing.chars().count() > 40 {
                continue;
            }
            let subject = EntityRef::new(EntityType::Person, placeholder_name(&ir.blob_b3, spk));
            let object = EntityRef::new(EntityType::Project, thing);
            ctx.fact(
                &subject,
                Predicate::Owns,
                Some(object),
                None,
                None,
                None,
                STATEMENT_BP,
                "media.statement",
                &ev,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oo_core::docir::*;

    fn utt(id: &str, ord: u32, speaker: &str, text: &str) -> Block {
        Block {
            id: id.into(),
            ordinal: ord,
            kind: BlockKind::Utterance,
            level: None,
            text: text.into(),
            hpath: vec![],
            parent: None,
            anchor: Anchor::Media {
                t_start_ms: u64::from(ord) * 1000,
                t_end_ms: u64::from(ord) * 1000 + 900,
                speaker: Some(speaker.into()),
                word_start: None,
                word_end: None,
            },
            table: None,
            flags: vec![],
        }
    }

    fn ir(blocks: Vec<Block>) -> DocIr {
        DocIr {
            protocol: "oo.doc-ir/v1.1".into(),
            blob_b3: "0123456789abcdef".repeat(4),
            format: DocFormat::Audio,
            parser: ParserInfo {
                id: ParserId::Media,
                version: "1".into(),
                adapter_version: None,
                profile_sha256: "0".repeat(64),
                profile: None,
            },
            meta: DocMeta::default(),
            blocks,
            warnings: vec![],
        }
    }

    #[test]
    fn attendees_come_from_the_file_name_and_the_minutes_file() {
        let c = attendees("会议/周会 参会=林川、王芳.m4a", None);
        let names: Vec<_> = c.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["林川", "王芳"]);
        assert!(c.iter().all(|c| c.speaker.is_none()));
        let c = attendees("会议/周会.m4a", Some("# 周会\n\n参会人：李甲、林川\n"));
        let names: Vec<_> = c.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["李甲", "林川"]);
        assert!(attendees("会议/周会.m4a", Some("没有名单")).is_empty());
    }

    #[test]
    fn a_self_introduction_and_a_call_by_name_name_the_speakers() {
        let doc = ir(vec![
            utt("blk_a", 1, "S1", "大家好，我是小王，负责后端。"),
            utt("blk_b", 2, "S2", "好的。"),
            utt("blk_c", 3, "S1", "小李你说一下进度。"),
            utt("blk_d", 4, "S2", "进度正常。"),
            utt("blk_e", 5, "S3", "我是项目经理，先开始。"),
        ]);
        let c = introductions(&doc);
        let pairs: Vec<_> = c
            .iter()
            .map(|c| (c.speaker.clone().unwrap(), c.name.clone(), c.source))
            .collect();
        assert!(pairs.contains(&("S1".into(), "小王".into(), CandidateSource::SelfIntro)));
        assert!(pairs.contains(&("S2".into(), "小李".into(), CandidateSource::Addressed)));
        assert!(
            !pairs.iter().any(|(s, _, _)| s == "S3"),
            "a role word is not a name: {pairs:?}"
        );
        assert!(c.iter().all(|c| c.block_id.is_some() && c.quote.is_some()));
    }

    #[test]
    fn speakers_are_listed_in_order_of_appearance() {
        let doc = ir(vec![
            utt("blk_a", 1, "S2", "你好"),
            utt("blk_b", 2, "S1", "你好"),
            utt("blk_c", 3, "S2", "再见"),
        ]);
        assert_eq!(speakers(&doc), ["S2", "S1"]);
    }

    #[test]
    fn placeholder_names_are_per_file_and_recognisable() {
        let a = placeholder_name(&"a".repeat(64), "S1");
        let b = placeholder_name(&"b".repeat(64), "S1");
        assert_ne!(a, b);
        assert!(is_placeholder(&a) && !is_placeholder("林川"));
    }
}
