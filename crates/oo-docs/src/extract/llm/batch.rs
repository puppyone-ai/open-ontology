//! Batches (10 §5.2.1): one batch is one section of the IR, so a local edit
//! leaves the other batches (and their cache keys) unchanged.
//!
//! A section is the blocks under one second level heading path; a file
//! without headings is cut every ~1,500 tokens. A batch holds at most 3,000
//! tokens; a longer section is cut at block boundaries. Page headers and
//! footers, code and front matter are not sent.

use oo_builder::cjk_calibrated_tokens;
use oo_core::docir::{Block, BlockKind, DocIr};

use super::{MAX_BATCH_TOKENS, SECTION_TOKENS};

/// One batch of blocks, in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch<'a> {
    pub blocks: Vec<&'a Block>,
}

impl Batch<'_> {
    pub fn tokens(&self) -> u64 {
        self.blocks
            .iter()
            .map(|b| cjk_calibrated_tokens(&b.text))
            .sum()
    }
}

fn sendable(b: &Block) -> bool {
    !matches!(
        b.kind,
        BlockKind::PageHeader | BlockKind::PageFooter | BlockKind::Code | BlockKind::Frontmatter
    ) && !b.text.trim().is_empty()
}

/// Section key of a block: its heading path cut at the second level.
fn section(b: &Block) -> Vec<&str> {
    b.hpath.iter().take(2).map(String::as_str).collect()
}

/// Cut a list of blocks into batches of at most `limit` tokens (a block
/// larger than the limit stands alone).
fn cut<'a>(blocks: Vec<&'a Block>, limit: u64) -> Vec<Batch<'a>> {
    let mut out = Vec::new();
    let mut cur: Vec<&Block> = Vec::new();
    let mut used = 0u64;
    for b in blocks {
        let t = cjk_calibrated_tokens(&b.text);
        if !cur.is_empty() && used + t > limit {
            out.push(Batch {
                blocks: std::mem::take(&mut cur),
            });
            used = 0;
        }
        used += t;
        cur.push(b);
    }
    if !cur.is_empty() {
        out.push(Batch { blocks: cur });
    }
    out
}

/// The batches of a document.
pub fn batches(ir: &DocIr) -> Vec<Batch<'_>> {
    let blocks: Vec<&Block> = ir.blocks.iter().filter(|b| sendable(b)).collect();
    let has_headings = blocks.iter().any(|b| !b.hpath.is_empty());
    let mut out = Vec::new();
    if has_headings {
        let mut cur: Vec<&Block> = Vec::new();
        let mut key: Option<Vec<&str>> = None;
        for b in blocks {
            let k = section(b);
            if key.as_ref().is_some_and(|c| *c != k) {
                out.extend(cut(std::mem::take(&mut cur), MAX_BATCH_TOKENS));
            }
            key = Some(k);
            cur.push(b);
        }
        out.extend(cut(cur, MAX_BATCH_TOKENS));
        // A batch of headings only (the title) has nothing to extract: it rides
        // with the section that follows.
        let mut merged: Vec<Batch<'_>> = Vec::new();
        let mut carry: Vec<&Block> = Vec::new();
        for b in out {
            if b.blocks.iter().all(|x| x.kind == BlockKind::Heading) {
                carry.extend(b.blocks);
                continue;
            }
            let mut blocks = std::mem::take(&mut carry);
            blocks.extend(b.blocks);
            merged.push(Batch { blocks });
        }
        if !carry.is_empty() {
            merged.push(Batch { blocks: carry });
        }
        return merged;
    } else {
        out.extend(cut(blocks, SECTION_TOKENS));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::markdown::parse_markdown;

    fn ir(src: &str) -> DocIr {
        parse_markdown(src.as_bytes()).unwrap().ir
    }

    #[test]
    fn one_batch_per_second_level_section() {
        let d = ir(
            "# 文档\n\n前言\n\n## 甲\n\n甲的内容\n\n### 甲的细节\n\n更多\n\n## 乙\n\n乙的内容\n",
        );
        let bs = batches(&d);
        let texts: Vec<Vec<&str>> = bs
            .iter()
            .map(|b| b.blocks.iter().map(|x| x.text.as_str()).collect())
            .collect();
        assert_eq!(bs.len(), 3, "{texts:?}");
        assert!(texts[1].contains(&"甲的内容") && texts[1].contains(&"更多"));
        assert!(texts[2].contains(&"乙的内容"));
        assert!(
            texts[0].contains(&"前言") && texts[0].contains(&"文档"),
            "the title rides with the preface"
        );
    }

    #[test]
    fn editing_a_section_leaves_the_sections_before_it_alone() {
        // Block ids carry the anchor, so an edit moves what follows it; the
        // sections before the edit keep their blocks and their cache keys.
        let a = ir("# T\n\n## 甲\n\n甲\n\n## 乙\n\n乙\n");
        let b = ir("# T\n\n## 甲\n\n甲\n\n## 乙\n\n乙改了\n");
        let ids = |d: &DocIr| -> Vec<Vec<String>> {
            batches(d)
                .iter()
                .map(|x| x.blocks.iter().map(|y| y.id.clone()).collect())
                .collect()
        };
        let (ia, ib) = (ids(&a), ids(&b));
        assert_eq!(ia.len(), ib.len());
        assert_eq!(ia.len(), 2, "the title rides with the first section");
        assert_eq!(ia[0], ib[0], "the section before the edit");
        assert_ne!(ia[1], ib[1]);
    }

    #[test]
    fn long_sections_are_cut_and_headingless_files_by_size() {
        let para = "字".repeat(600);
        let mut src = String::from("# T\n\n## 长\n\n");
        for _ in 0..8 {
            src.push_str(&para);
            src.push_str("\n\n");
        }
        let d = ir(&src);
        let bs = batches(&d);
        assert!(bs.len() >= 2);
        assert!(
            bs.iter()
                .all(|b| b.tokens() <= MAX_BATCH_TOKENS || b.blocks.len() == 1)
        );
        let flat = ir(&format!("{para}\n\n{para}\n\n{para}\n\n{para}\n"));
        let fb = batches(&flat);
        assert!(fb.len() >= 2, "{}", fb.len());
        assert!(fb.iter().all(|b| b.tokens() <= SECTION_TOKENS + 700));
    }

    #[test]
    fn code_and_front_matter_are_not_sent() {
        let d = ir("---\ntype: x\n---\n\n# T\n\n```\nsecret\n```\n\n正文\n");
        let all: Vec<&str> = batches(&d)
            .iter()
            .flat_map(|b| b.blocks.iter().map(|x| x.text.as_str()))
            .collect();
        assert!(all.contains(&"正文"));
        assert!(
            !all.iter()
                .any(|t| t.contains("secret") || t.contains("type: x"))
        );
    }
}
