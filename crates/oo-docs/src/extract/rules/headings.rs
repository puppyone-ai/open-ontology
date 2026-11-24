//! `md.headings`: list items under decision / tried / rejected sections and
//! dated replacement sentences (10 §5.1, §5.5). Pure.

use oo_core::docir::{Block, BlockKind};
use unicode_normalization::UnicodeNormalization;

/// Section kinds recognised on level 2 headings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Decisions,
    Tried,
    Rejected,
}

/// Classify a heading text. `不做`/`否决`/`Rejected` is checked before the
/// decision keywords so `否决的决定` is a rejection list.
pub fn classify_heading(text: &str) -> Option<Section> {
    let t: String = text.nfkc().collect::<String>().to_lowercase();
    let t = t.trim().trim_end_matches([':', '：']);
    let has = |keys: &[&str]| keys.iter().any(|k| t.contains(k));
    if has(&["不做", "否决", "rejected"]) {
        Some(Section::Rejected)
    } else if has(&["已尝试", "放弃", "踩坑", "tried"]) {
        Some(Section::Tried)
    } else if has(&["决定", "决策", "decision"]) {
        Some(Section::Decisions)
    } else {
        None
    }
}

/// The section each block sits in: the nearest preceding level 2 heading
/// (reset by a level 1 heading), if it is one of the recognised sections.
pub fn sections(blocks: &[Block]) -> Vec<Option<Section>> {
    let mut current: Option<Section> = None;
    blocks
        .iter()
        .map(|b| {
            if b.kind == BlockKind::Heading {
                match b.level {
                    Some(1) => current = None,
                    Some(2) => current = classify_heading(&b.text),
                    _ => {}
                }
            }
            current
        })
        .collect()
}

const REPLACE_WORDS: &[&str] = &["取代", "替代", "替换"];
const QUOTES: &[(char, char)] = &[('“', '”'), ('「', '」'), ('《', '》'), ('"', '"')];

/// The decision a sentence says it replaces: the first quoted title after
/// `取代`/`替代`/`替换` (`本决定取代2026年1月10日“周报每周五发送”的决定`),
/// unless the word is passive (`被…取代`) or a sentence end intervenes.
pub fn replaced_title(text: &str) -> Option<String> {
    for line in text.split('\n') {
        let mut best: Option<(usize, &str)> = None;
        for w in REPLACE_WORDS {
            if let Some(i) = line.find(w)
                && best.is_none_or(|(j, _)| i < j)
            {
                best = Some((i, w));
            }
        }
        let Some((i, word)) = best else { continue };
        let before = &line[..i];
        if before.ends_with('被') || before.ends_with("已被") {
            continue;
        }
        let rest = &line[i + word.len()..];
        let Some((open_at, (open, close))) = rest
            .char_indices()
            .find_map(|(p, c)| QUOTES.iter().find(|(o, _)| *o == c).map(|q| (p, *q)))
        else {
            continue;
        };
        if rest[..open_at].contains(['。', '；', ';', '.']) {
            continue;
        }
        let inner = &rest[open_at + open.len_utf8()..];
        if let Some(end) = inner.find(close) {
            let title = inner[..end].trim();
            if !title.is_empty() {
                return Some(title.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_classify() {
        assert_eq!(classify_heading("决定"), Some(Section::Decisions));
        assert_eq!(classify_heading("Decisions:"), Some(Section::Decisions));
        assert_eq!(classify_heading("已尝试"), Some(Section::Tried));
        assert_eq!(classify_heading("踩坑记录"), Some(Section::Tried));
        assert_eq!(classify_heading("Rejected"), Some(Section::Rejected));
        assert_eq!(classify_heading("否决的决定"), Some(Section::Rejected));
        assert_eq!(classify_heading("背景"), None);
    }

    #[test]
    fn replacement_sentences() {
        assert_eq!(
            replaced_title("本决定取代2026年1月10日“周报每周五发送”的决定。").as_deref(),
            Some("周报每周五发送")
        );
        assert_eq!(
            replaced_title("本决定取代“星图看板采用自研图表引擎”，自研方案作废。").as_deref(),
            Some("星图看板采用自研图表引擎")
        );
        assert_eq!(replaced_title("本决定已被“周报改为每周四发送”取代。"), None);
        assert_eq!(replaced_title("取代旧流程。新流程见“手册”"), None);
        assert_eq!(replaced_title("没有关键词“引用”"), None);
        assert_eq!(replaced_title("替代「旧方案」").as_deref(), Some("旧方案"));
    }
}
