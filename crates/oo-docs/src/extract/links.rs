//! `doc.link`: Document to entity links found by name (10 §5.1, D15).
//!
//! The rules of one file only know the entities that file introduces. A
//! document also *mentions* every other entity of the Drive whose name it
//! writes, and is *about* the ones it puts in its title or headings or keeps
//! coming back to. This module is pure: it takes the names the Drive knows
//! ([`LexEntry`], loaded by `resolve::pg::load_lexicon`) and the IR and returns
//! bind-only hints, so a link never creates an entity; resolve binds each hint
//! to the stored entity, or drops it.
//!
//! Only the canonical name links, never an alias or a derived form: a link is
//! evidence in the document's own text, and a pack of that document alone shows
//! the entity under its canonical name, so the text has to say it (the leak
//! guard, DOC-G6, stops a pack that names an entity its scope never named). An
//! alias match would tell a reader who `Ruby` is from files outside the scope.
//!
//! Matching is on the normalised text (NFKC, lower case, blanks squeezed):
//! a name is a substring, except that a name made of ASCII letters and digits
//! must not sit inside a longer word. A name that two entities of the same
//! lexicon share is ambiguous and links nothing (D13); names shorter than the
//! reliable minimum are left out (an organization needs three characters: two
//! are a department word, not a name).

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::{Block, BlockKind, DocIr};

use super::model::{Collector, EntityRef, EntityType, EvidenceRef, Extraction, Predicate};
use crate::resolve::normalize::norm;

/// Confidence of a link by canonical name.
pub const MENTION_BP: u16 = 9000;
/// Confidence of an `about` link.
pub const ABOUT_BP: u16 = 8500;
/// Distinct blocks that make an entity a subject of the document by repetition.
pub const ABOUT_REPEAT_BLOCKS: usize = 3;

/// One entity the Drive knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexEntry {
    pub ty: EntityType,
    /// Canonical name as stored (the hint binds to it exactly).
    pub name: String,
}

/// Entity types a document links to by name: Decisions and Documents are long
/// titles, not names a text repeats.
pub const fn linkable(ty: EntityType) -> bool {
    matches!(
        ty,
        EntityType::Person
            | EntityType::Organization
            | EntityType::Project
            | EntityType::Topic
            | EntityType::Term
            | EntityType::Event
    )
}

/// Types the core schema lets a Document be `about` (DOC-G1).
pub const fn about_range(ty: EntityType) -> bool {
    matches!(
        ty,
        EntityType::Project | EntityType::Topic | EntityType::Term | EntityType::Organization
    )
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// Shortest name that is reliable as a bare substring.
fn long_enough(ty: EntityType, name: &str) -> bool {
    char_count(name) >= if ty == EntityType::Organization { 3 } else { 2 }
}

fn ascii_word(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Does `needle` (normalised) occur in `hay` (normalised) as a name?
fn occurs(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let bounded = ascii_word(needle);
    let mut from = 0;
    while let Some(at) = hay[from..].find(needle) {
        let start = from + at;
        let end = start + needle.len();
        if !bounded {
            return true;
        }
        let before = hay[..start].chars().next_back();
        let after = hay[end..].chars().next();
        let edge = |c: Option<char>| c.is_none_or(|c| !c.is_ascii_alphanumeric());
        if edge(before) && edge(after) {
            return true;
        }
        from = start + needle.chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// One matchable name of an entity.
struct Form {
    entry: usize,
    text: String,
}

fn forms(lex: &[LexEntry]) -> Vec<Form> {
    // A name that more than one entity of the same type claims is ambiguous.
    let mut owners: BTreeMap<(EntityType, String), BTreeSet<usize>> = BTreeMap::new();
    for (i, e) in lex.iter().enumerate() {
        owners.entry((e.ty, norm(&e.name))).or_default().insert(i);
    }
    let mut out = Vec::new();
    for (i, e) in lex.iter().enumerate() {
        if !linkable(e.ty) {
            continue;
        }
        let text = norm(&e.name);
        if !long_enough(e.ty, &text)
            || owners
                .get(&(e.ty, text.clone()))
                .is_some_and(|o| o.len() > 1)
        {
            continue;
        }
        out.push(Form { entry: i, text });
    }
    out
}

/// Blocks that carry the document's own subject: its title, headings and
/// slide titles.
fn is_title_block(b: &Block) -> bool {
    matches!(b.kind, BlockKind::Heading | BlockKind::SlideTitle)
}

fn is_text_block(b: &Block) -> bool {
    !matches!(
        b.kind,
        BlockKind::PageHeader | BlockKind::PageFooter | BlockKind::Footnote
    ) && !b.text.trim().is_empty()
}

/// What one entity looks like in one document.
#[derive(Default)]
struct Seen {
    first: Option<usize>,
    first_title: Option<usize>,
    blocks: usize,
    in_title: bool,
}

/// Mentions and `about` links of one document to the entities of `lex`, as
/// bind-only hints. `title` is the document's own title (H1, metadata or
/// file name stem) and `stem` its file name without extension.
pub fn doc_links(
    path: &str,
    ir: &DocIr,
    title: Option<&str>,
    stem: &str,
    lex: &[LexEntry],
) -> Extraction {
    let doc = EntityRef::new(EntityType::Document, path);
    let forms = forms(lex);
    let texts: Vec<(usize, String)> = ir
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| is_text_block(b))
        .map(|(i, b)| (i, norm(&b.text)))
        .collect();
    let head = [title.map(norm), Some(norm(stem))];
    let mut seen: BTreeMap<usize, Seen> = BTreeMap::new();
    for f in &forms {
        let slot = seen.entry(f.entry).or_default();
        if head.iter().flatten().any(|h| occurs(h, &f.text)) {
            slot.in_title = true;
        }
        for (i, t) in &texts {
            if occurs(t, &f.text) {
                slot.blocks += 1;
                slot.first = Some(slot.first.map_or(*i, |x| x.min(*i)));
                if is_title_block(&ir.blocks[*i]) {
                    slot.first_title = Some(slot.first_title.map_or(*i, |x| x.min(*i)));
                }
            }
        }
    }
    let anchor = |i: usize| EvidenceRef::from_block(&ir.blocks[i]);
    let mut col = Collector::default();
    for (entry, s) in seen {
        let e = &lex[entry];
        // The evidence is a block that writes the name: a name only the title
        // metadata or the file name carries has no text to point at (and a pack of
        // this document alone could not show where the name comes from).
        let Some(at) = s.first else {
            continue;
        };
        col.hint(
            &doc,
            Predicate::Mentions,
            e.ty,
            &e.name,
            MENTION_BP,
            "doc.link",
            &anchor(at),
        );
        let subject = s.in_title || s.first_title.is_some() || s.blocks >= ABOUT_REPEAT_BLOCKS;
        if subject && about_range(e.ty) {
            col.hint(
                &doc,
                Predicate::About,
                e.ty,
                &e.name,
                ABOUT_BP,
                "doc.about",
                &anchor(s.first_title.unwrap_or(at)),
            );
        }
    }
    col.date_hints(super::rules::file_default_valid_from(path, ir));
    col.finish(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::markdown::parse_markdown;

    fn lex() -> Vec<LexEntry> {
        let e = |ty, name: &str| LexEntry {
            ty,
            name: name.into(),
        };
        vec![
            e(EntityType::Person, "林远舟"),
            e(EntityType::Person, "吴启明"),
            e(EntityType::Project, "星图看板"),
            e(EntityType::Topic, "周报"),
            e(EntityType::Organization, "AI"),
            e(EntityType::Term, "go"),
            e(EntityType::Project, "Atlas"),
            e(EntityType::Organization, "产品"),
            // The same name for two Projects: ambiguous.
            e(EntityType::Project, "灯塔"),
            e(EntityType::Project, "灯塔"),
        ]
    }

    fn run(md: &str, path: &str) -> Extraction {
        let ir = parse_markdown(md.as_bytes()).unwrap().ir;
        let stem = path.rsplit('/').next().unwrap().trim_end_matches(".md");
        let title = ir
            .blocks
            .iter()
            .find(|b| b.kind == BlockKind::Heading)
            .map(|b| b.text.clone());
        doc_links(path, &ir, title.as_deref(), stem, &lex())
    }

    fn has(x: &Extraction, p: Predicate, name: &str) -> bool {
        x.hints
            .iter()
            .any(|h| h.predicate == p && h.target_name == name)
    }

    #[test]
    fn a_name_in_the_text_is_a_mention_with_the_block_it_stands_in() {
        let x = run(
            "# 记录\n\n今天林远舟和同事讨论了星图看板的进度。\n",
            "Notes/a.md",
        );
        assert!(has(&x, Predicate::Mentions, "林远舟"));
        assert!(has(&x, Predicate::Mentions, "星图看板"));
        assert!(
            !has(&x, Predicate::About, "星图看板"),
            "once in the body is no subject"
        );
        let h = x
            .hints
            .iter()
            .find(|h| h.target_name == "星图看板")
            .unwrap();
        assert_eq!(h.rule_id, "doc.link");
        assert_eq!(h.evidence.len(), 1);
    }

    #[test]
    fn only_the_canonical_name_links_never_an_alias_or_a_derived_form() {
        // The pack of this document alone shows the entity under its canonical name, so the
        // text must say it: a nickname or a given name would tell who it is from other files.
        let x = run(
            "# 记录\n\n老林说启明已经完成。Kevin 同意。\n",
            "Notes/aa.md",
        );
        assert!(x.hints.is_empty(), "{:?}", x.hints);
    }

    #[test]
    fn the_title_heading_or_file_name_makes_the_subject_when_a_block_writes_the_name() {
        let x = run("# 周报每周五发送\n\n内容。\n", "Decisions/x.md");
        assert!(has(&x, Predicate::About, "周报"));
        let y = run("正文提到星图看板。\n", "Projects/星图看板.md");
        assert!(has(&y, Predicate::About, "星图看板"), "the file name");
        let z = run("# 记录\n\n## 星图看板进展\n\n无。\n", "Notes/b.md");
        assert!(has(&z, Predicate::About, "星图看板"), "a heading");
        // The file name alone, without a block that writes the name: no link, no evidence.
        let w = run("# 记录\n\n无关。\n", "Projects/星图看板.md");
        assert!(w.hints.is_empty(), "{:?}", w.hints);
    }

    #[test]
    fn repetition_makes_a_subject_but_a_person_is_never_about() {
        let body = "# 记录\n\n星图看板第一。\n\n星图看板第二。\n\n星图看板第三。\n\n林远舟。\n";
        let x = run(body, "Notes/c.md");
        assert!(has(&x, Predicate::About, "星图看板"));
        assert!(!has(&x, Predicate::About, "林远舟"));
    }

    #[test]
    fn short_and_ambiguous_names_link_nothing() {
        let x = run("# 记录\n\n产品和AI；goal；灯塔。\n", "Notes/d.md");
        assert!(
            !has(&x, Predicate::Mentions, "产品"),
            "two characters, not a person"
        );
        assert!(!has(&x, Predicate::Mentions, "AI"), "two letters");
        assert!(
            !has(&x, Predicate::Mentions, "灯塔"),
            "two entities claim it"
        );
    }

    #[test]
    fn an_ascii_name_inside_a_longer_word_is_no_mention() {
        let x = run("# n\n\nThe Atlases here.\n\nAtlas, then.\n", "Notes/e.md");
        assert!(has(&x, Predicate::Mentions, "Atlas"));
        let y = run("# n\n\nThe Atlases here.\n", "Notes/f.md");
        assert!(!has(&y, Predicate::Mentions, "Atlas"));
    }

    #[test]
    fn the_result_is_the_same_for_the_same_input() {
        let a = run("# 周报\n\n林远舟 星图看板\n", "Notes/g.md");
        let b = run("# 周报\n\n林远舟 星图看板\n", "Notes/g.md");
        assert_eq!(a, b);
    }

    #[test]
    fn links_start_on_the_default_date_of_the_file_like_its_other_facts() {
        let x = run("# 记录\n\n星图看板。\n", "Notes/2026-03-05-速记.md");
        assert!(!x.hints.is_empty());
        assert!(
            x.hints
                .iter()
                .all(|h| h.valid_from.as_ref().map(|(d, _)| d.as_str()) == Some("2026-03-05"))
        );
        let y = run("# 记录\n\n星图看板。\n", "Notes/速记.md");
        assert!(y.hints.iter().all(|h| h.valid_from.is_none()));
    }
}
