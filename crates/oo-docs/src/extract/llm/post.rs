//! Post-processing of a validated reply (10 §5.2.3): an item that fails a
//! check is dropped and counted, the rest of the batch stands. Survivors are
//! mapped into the same candidate types the rules produce, so resolve and
//! the stores treat them alike (origin `llm`, confidence 8500, 7000 or 5000).
//!
//! 1. `block_id` belongs to the batch;
//! 2. `quote` (NFC, blanks squeezed) is a substring of that block's text;
//! 3. a relation fits the domain and range of the core schema (the DOC-G1
//!    table is the one source of truth);
//! 4. `Thing` labels are normalised (NFKC, no punctuation, at most 20
//!    characters) and a label that is a synonym of a core type becomes that
//!    type;
//! 5. `supersedes` is mapped like the others, but the review queue never lets
//!    an LLM `supersedes` publish by itself (D18, D51).

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::{Anchor, Block, DocIr};
use unicode_normalization::UnicodeNormalization;

use super::schema::{LlmOutput, LlmRelation};
use crate::extract::model::{
    Collector, EntityRef, EntityType, EvidenceRef, FactMention, Predicate,
};
use crate::publish::gates::{FactRow, g1_fact};

/// What was dropped, by reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dropped {
    pub bad_block: u32,
    pub quote_mismatch: u32,
    pub domain_range: u32,
    pub bad_thing: u32,
    pub unknown_end: u32,
    pub bad_dates: u32,
}

impl Dropped {
    pub fn total(&self) -> u32 {
        self.bad_block
            + self.quote_mismatch
            + self.domain_range
            + self.bad_thing
            + self.unknown_end
            + self.bad_dates
    }

    pub fn add(&mut self, o: &Dropped) {
        self.bad_block += o.bad_block;
        self.quote_mismatch += o.quote_mismatch;
        self.domain_range += o.domain_range;
        self.bad_thing += o.bad_thing;
        self.unknown_end += o.unknown_end;
        self.bad_dates += o.bad_dates;
    }
}

/// NFC and blanks squeezed to single spaces.
pub fn squeeze(s: &str) -> String {
    let nfc: String = s.nfc().collect();
    nfc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The quote is a verbatim part of the block text.
pub fn quote_in_block(quote: &str, block_text: &str) -> bool {
    let q = squeeze(quote);
    !q.is_empty() && squeeze(block_text).contains(&q)
}

/// A core type a `Thing` label is a synonym of.
pub fn synonym_type(label: &str) -> Option<EntityType> {
    Some(match label {
        "人" | "人员" | "人物" | "成员" | "员工" => EntityType::Person,
        "组织" | "公司" | "部门" | "团队" | "机构" => EntityType::Organization,
        "项目" => EntityType::Project,
        "决定" | "决策" | "方案" => EntityType::Decision,
        "文档" | "文件" | "文章" => EntityType::Document,
        "主题" | "话题" | "议题" => EntityType::Topic,
        "事件" | "活动" | "会议" => EntityType::Event,
        "术语" | "名词" | "概念" => EntityType::Term,
        _ => return None,
    })
}

/// NFKC, punctuation and blanks removed, at most 20 characters; `None` when
/// nothing is left.
pub fn thing_label(raw: &str) -> Option<String> {
    let s: String = raw
        .nfkc()
        .filter(|c| {
            !c.is_whitespace()
                && !c.is_ascii_punctuation()
                && !"，。、；：！？（）《》“”‘’「」【】·".contains(*c)
        })
        .take(20)
        .collect();
    (!s.is_empty()).then_some(s)
}

/// Where the quote sits in an exact `Text` block: the refined anchor. It is
/// not applied to evidence yet: DOC-G2 requires the stored anchor to equal the
/// block's anchor, so a contained anchor needs a gate change first.
pub fn refine_anchor(block: &Block, quote: &str) -> Option<Anchor> {
    let Anchor::Text {
        line_start,
        char_start,
        char_end,
        ..
    } = &block.anchor
    else {
        return None;
    };
    let verbatim = (char_end - char_start) as usize == block.text.chars().count();
    if !verbatim {
        return None;
    }
    let byte = block.text.find(quote)?;
    let before = &block.text[..byte];
    let off = before.chars().count() as u64;
    let len = quote.chars().count() as u64;
    let nl = |s: &str| u32::try_from(s.matches('\n').count()).unwrap_or(0);
    Some(Anchor::Text {
        line_start: line_start + nl(before),
        line_end: line_start + nl(&block.text[..byte + quote.len()]),
        char_start: char_start + off,
        char_end: char_start + off + len,
    })
}

/// Map one validated reply into `col`. `batch_blocks` are the block ids of the
/// batch the reply is for.
pub(crate) fn map_into(
    col: &mut Collector,
    ir: &DocIr,
    batch_blocks: &BTreeSet<&str>,
    out: &LlmOutput,
) -> Dropped {
    let mut dropped = Dropped::default();
    let by_id: BTreeMap<&str, &Block> = ir.blocks.iter().map(|b| (b.id.as_str(), b)).collect();
    let check = |block_id: &str, quote: &str, d: &mut Dropped| -> Option<EvidenceRef> {
        if !batch_blocks.contains(block_id) {
            d.bad_block += 1;
            return None;
        }
        let block = by_id.get(block_id)?;
        if !quote_in_block(quote, &block.text) {
            d.quote_mismatch += 1;
            return None;
        }
        Some(EvidenceRef::from_block(block))
    };
    let mut refs: BTreeMap<&str, EntityRef> = BTreeMap::new();
    for e in &out.entities {
        let Some(ev) = check(&e.block_id, &e.quote, &mut dropped) else {
            continue;
        };
        let (ty, label) = if e.ty == "Thing" {
            match e.label.as_deref().and_then(thing_label) {
                None => {
                    dropped.bad_thing += 1;
                    continue;
                }
                Some(l) => match synonym_type(&l) {
                    Some(core) => (core, None),
                    None => (EntityType::Thing, Some(l)),
                },
            }
        } else {
            match EntityType::parse(&e.ty) {
                Some(t) => (t, None),
                None => continue,
            }
        };
        let name: String = e.name.trim().chars().take(200).collect();
        let mut r = EntityRef::new(ty, name);
        r.thing_label = label;
        col.entity(&r, Some(&ev));
        for a in &e.aliases {
            // Candidates only: they stay drafts and never trigger a merge.
            col.alias(&r, a, e.confidence.bp().min(5000), "llm", &ev);
        }
        refs.insert(e.id.as_str(), r);
    }
    for rel in &out.relations {
        map_relation(col, rel, &refs, &check, &mut dropped);
    }
    dropped
}

fn map_relation(
    col: &mut Collector,
    rel: &LlmRelation,
    refs: &BTreeMap<&str, EntityRef>,
    check: &dyn Fn(&str, &str, &mut Dropped) -> Option<EvidenceRef>,
    dropped: &mut Dropped,
) {
    let Some(ev) = check(&rel.block_id, &rel.quote, dropped) else {
        return;
    };
    let (Some(subject), Some(object)) = (
        refs.get(rel.subject.as_str()),
        rel.object.as_deref().and_then(|o| refs.get(o)),
    ) else {
        dropped.unknown_end += 1;
        return;
    };
    let Some(predicate) = Predicate::parse(&rel.predicate).filter(|p| !p.is_attr()) else {
        dropped.domain_range += 1;
        return;
    };
    let row = FactRow {
        id: String::new(),
        fact_key: String::new(),
        predicate: predicate.name(),
        subject_type: subject.ty.as_str().to_owned(),
        object_type: Some(object.ty.as_str().to_owned()),
        value: None,
    };
    if g1_fact(&row).is_err() {
        dropped.domain_range += 1;
        return;
    }
    if let (Some(a), Some(b)) = (&rel.valid_from, &rel.valid_to)
        && a > b
    {
        dropped.bad_dates += 1;
        return;
    }
    col.fact(FactMention {
        subject: subject.clone(),
        predicate,
        object: Some(object.clone()),
        value: None,
        valid_from: rel.valid_from.clone(),
        valid_to: rel.valid_to.clone(),
        valid_precision: rel.valid_from.as_ref().map(|_| "day"),
        confidence_bp: rel.confidence.bp(),
        rule_id: "llm",
        evidence: vec![ev],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::llm::batch::batches;
    use crate::extract::llm::schema::parse;
    use crate::parse::markdown::parse_markdown;

    fn setup() -> (DocIr, String) {
        let ir = parse_markdown("# 星图\n\n陈子墨负责星图看板，周若溪  接手。\n".as_bytes())
            .unwrap()
            .ir;
        let id = ir
            .blocks
            .iter()
            .find(|b| b.text.starts_with("陈子墨"))
            .unwrap()
            .id
            .clone();
        (ir, id)
    }

    fn run(ir: &DocIr, json: &str) -> (Collector, Dropped) {
        let ids: BTreeSet<&str> = batches(ir)
            .iter()
            .flat_map(|b| b.blocks.iter().map(|x| x.id.as_str()))
            .collect();
        let mut col = Collector::default();
        let d = map_into(&mut col, ir, &ids, &parse(json).unwrap());
        (col, d)
    }

    #[test]
    fn good_items_map_with_the_confidence_of_the_word() {
        let (ir, id) = setup();
        let (col, d) = run(
            &ir,
            &format!(
                r#"{{"entities":[
                {{"id":"e1","type":"Person","name":"陈子墨","aliases":["小陈"],"block_id":"{id}","quote":"陈子墨","confidence":"high"}},
                {{"id":"e2","type":"Project","name":"星图看板","block_id":"{id}","quote":"星图看板，周若溪 接手","confidence":"medium"}}],
                "relations":[{{"subject":"e1","predicate":"owns","object":"e2","valid_from":"2026-01-01","block_id":"{id}","quote":"陈子墨负责星图看板","confidence":"medium"}}]}}"#
            ),
        );
        assert_eq!(d, Dropped::default());
        let x = col.finish("a.md");
        assert_eq!(x.entities.len(), 2);
        assert_eq!(x.facts.len(), 1);
        assert_eq!(
            (x.facts[0].confidence_bp, x.facts[0].rule_id),
            (7000, "llm")
        );
        assert_eq!(x.facts[0].valid_precision, Some("day"));
        assert_eq!(x.aliases.len(), 1);
        assert_eq!(x.aliases[0].confidence_bp, 5000, "aliases stay candidates");
    }

    #[test]
    fn every_failed_check_drops_only_that_item() {
        let (ir, id) = setup();
        let (col, d) = run(
            &ir,
            &format!(
                r#"{{"entities":[
                {{"id":"e1","type":"Person","name":"陈子墨","block_id":"{id}","quote":"陈子墨","confidence":"high"}},
                {{"id":"e2","type":"Project","name":"星图","block_id":"{id}","quote":"灯塔迁移","confidence":"high"}},
                {{"id":"e3","type":"Project","name":"星图看板","block_id":"blk_ffffffffffffffffffff","quote":"x","confidence":"high"}},
                {{"id":"e4","type":"Thing","label":"。。","name":"n","block_id":"{id}","quote":"陈子墨","confidence":"low"}},
                {{"id":"e5","type":"Organization","name":"星图看板","block_id":"{id}","quote":"星图看板","confidence":"high"}}],
                "relations":[
                {{"subject":"e1","predicate":"owns","object":"e5","block_id":"{id}","quote":"陈子墨负责","confidence":"high"}},
                {{"subject":"e1","predicate":"member_of","object":"e2","block_id":"{id}","quote":"陈子墨","confidence":"high"}},
                {{"subject":"e1","predicate":"mentions","object":"e5","block_id":"{id}","quote":"陈子墨","confidence":"high"}},
                {{"subject":"e1","predicate":"owns","object":"e5","valid_from":"2026-05-01","valid_to":"2026-01-01","block_id":"{id}","quote":"陈子墨","confidence":"high"}},
                {{"subject":"e1","predicate":"owns","object":"e5","block_id":"{id}","quote":"谁也没说过","confidence":"high"}}]}}"#
            ),
        );
        assert_eq!(
            d,
            Dropped {
                bad_block: 1,
                quote_mismatch: 2,
                domain_range: 1,
                bad_thing: 1,
                unknown_end: 1,
                bad_dates: 1
            }
        );
        let x = col.finish("a.md");
        // Kept: e1, e5 and the one `owns` Person -> Organization.
        assert_eq!(x.entities.len(), 2);
        assert_eq!(x.facts.len(), 1);
        assert_eq!(x.facts[0].predicate, Predicate::Owns);
    }

    #[test]
    fn things_are_labelled_or_turned_into_core_types() {
        assert_eq!(thing_label("  “看板 ”（V2）"), Some("看板V2".into()));
        assert_eq!(thing_label("，。"), None);
        assert_eq!(thing_label(&"字".repeat(30)).unwrap().chars().count(), 20);
        assert_eq!(synonym_type("会议"), Some(EntityType::Event));
        assert_eq!(synonym_type("看板"), None);
        let (ir, id) = setup();
        let (col, d) = run(
            &ir,
            &format!(
                r#"{{"entities":[
                {{"id":"e1","type":"Thing","label":"项目","name":"星图看板","block_id":"{id}","quote":"星图看板","confidence":"high"}},
                {{"id":"e2","type":"Thing","label":"看板","name":"星图看板","block_id":"{id}","quote":"星图看板","confidence":"high"}}],"relations":[]}}"#
            ),
        );
        assert_eq!(d.total(), 0);
        let x = col.finish("a.md");
        let kinds: Vec<(EntityType, Option<&str>)> = x
            .entities
            .iter()
            .map(|e| (e.entity.ty, e.entity.thing_label.as_deref()))
            .collect();
        assert!(kinds.contains(&(EntityType::Project, None)));
        assert!(kinds.contains(&(EntityType::Thing, Some("看板"))));
    }

    #[test]
    fn quotes_compare_after_nfc_and_blank_squeeze() {
        assert!(quote_in_block("周若溪 接手", "周若溪  接手。"));
        assert!(!quote_in_block("", "x"));
        assert!(!quote_in_block("周若溪接手", "周若溪  接手"));
    }

    #[test]
    fn the_quote_position_refines_a_text_anchor() {
        let ir = parse_markdown("# T\n\n第一行\n第二行有目标词\n".as_bytes())
            .unwrap()
            .ir;
        let b = ir
            .blocks
            .iter()
            .find(|b| b.text.contains("目标词"))
            .unwrap();
        let a = refine_anchor(b, "目标词").expect("verbatim block");
        let Anchor::Text {
            line_start,
            line_end,
            ..
        } = a
        else {
            panic!()
        };
        assert_eq!((line_start, line_end), (4, 4));
        assert!(refine_anchor(b, "不存在").is_none());
    }
}
