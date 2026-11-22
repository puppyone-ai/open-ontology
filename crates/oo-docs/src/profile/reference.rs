//! References (D94): a paper, a book or an external report is one `Reference` instance (the 文献): title, authors, year, venue,
//! DOI or arXiv id, and `cites` to the references it lists. The body is not read for entities: no methods, datasets or metrics.
//!
//! The reference itself and a reference another one cites share the entity when the titles agree, so a citation connects two
//! files. Authors are `Person` with `authored` to the reference, for the reference's own authors only. Deterministic; nothing is
//! asked of a model.

use oo_core::docir::{Block, BlockKind, DocIr};

use super::{REFERENCE, attr, attr_enum, clip, thing};
use crate::extract::model::{
    Collector, EntityRef, EntityType, EvidenceRef, FactMention, Predicate,
};

const MAX_AUTHORS: usize = 30;
const MAX_REFS: usize = 200;
const BP: u16 = 9000;

const REF_HEADINGS: &[&str] = &[
    "references",
    "bibliography",
    "参考文献",
    "literature cited",
    "works cited",
];

fn lower(s: &str) -> String {
    s.to_lowercase()
}

fn is_heading_like(b: &Block, words: &[&str]) -> bool {
    let t =
        lower(b.text.trim().trim_matches(|c: char| {
            !c.is_alphanumeric() && !('\u{4e00}'..='\u{9fff}').contains(&c)
        }));
    let short = t.chars().count() <= 40;
    short
        && words
            .iter()
            .any(|w| t == *w || t.ends_with(&format!(" {w}")) && t.chars().count() <= 20)
}

const NOT_NAME: &[&str] = &[
    "university",
    "institute",
    "laboratory",
    "lab",
    "labs",
    "department",
    "school",
    "college",
    "research",
    "inc",
    "corp",
    "ltd",
    "google",
    "microsoft",
    "meta",
    "abstract",
    "keywords",
    "arxiv",
    "email",
    "preprint",
    "center",
    "centre",
];

/// A person's name from one fragment of an author line, or `None`.
fn person_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '*' | '†' | '‡' | '§' | '¶' | '∗' | '⋆') && !c.is_ascii_digit())
        .collect();
    let cleaned = cleaned
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c: char| matches!(c, ',' | ';' | '.' | ':'))
        .trim()
        .to_owned();
    if cleaned.is_empty() || cleaned.contains('@') || cleaned.chars().count() > 40 {
        return None;
    }
    let cjk = cleaned
        .chars()
        .all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    if cjk {
        return ((2..=4).contains(&cleaned.chars().count())).then_some(cleaned);
    }
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if !(2..=4).contains(&words.len()) {
        return None;
    }
    let ok = words.iter().all(|w| {
        let first = w.chars().next().unwrap_or(' ');
        first.is_uppercase()
            && w.chars()
                .all(|c| c.is_alphabetic() || matches!(c, '.' | '-' | '\''))
            && !NOT_NAME.contains(&lower(w).as_str())
    });
    ok.then_some(cleaned)
}

fn split_authors(line: &str) -> Vec<String> {
    let s = line
        .replace(" and ", ",")
        .replace(" & ", ",")
        .replace(['、', '，', ';', '\n'], ",");
    s.split(',').filter_map(person_name).collect()
}

const VENUES: &[&str] = &[
    "NeurIPS", "ICML", "ICLR", "ACL", "EMNLP", "NAACL", "COLING", "CVPR", "ICCV", "ECCV", "AAAI",
    "IJCAI", "KDD", "WWW", "SIGIR", "SIGMOD", "VLDB", "OSDI", "SOSP", "NSDI", "USENIX", "ICSE",
    "FSE", "PLDI", "POPL", "CHI", "UIST", "JMLR", "TPAMI", "TACL", "Nature", "Science",
];

fn year_in(s: &str) -> Option<String> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(3)).find_map(|i| {
        let w = &s[i..i + 4];
        let before = i == 0 || !b[i - 1].is_ascii_digit();
        let after = b.get(i + 4).is_none_or(|c| !c.is_ascii_digit());
        (before
            && after
            && (w.starts_with("19") || w.starts_with("20"))
            && w.chars().all(|c| c.is_ascii_digit()))
        .then(|| w.to_owned())
    })
}

fn find_venue(blocks: &[Block], limit: usize) -> Option<(String, Option<String>, usize)> {
    for (i, b) in blocks.iter().take(limit).enumerate() {
        for line in b.text.lines() {
            let words: Vec<&str> = line
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .collect();
            for v in VENUES {
                let hit = words.iter().any(|w| w == v);
                if hit
                    && (lower(line).contains("proceedings")
                        || lower(line).contains("published")
                        || lower(line).contains("conference")
                        || line.trim().chars().count() <= 80
                        || lower(line).contains("journal"))
                {
                    return Some(((*v).to_owned(), year_in(line), i));
                }
            }
            if let Some(rest) = lower(line).strip_prefix("arxiv preprint") {
                let _ = rest;
                return Some(("arXiv".to_owned(), year_in(line), i));
            }
        }
    }
    None
}

fn find_after<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    let l = s.to_ascii_lowercase();
    l.find(marker).map(|i| &s[i + marker.len()..])
}

fn doi_in(s: &str) -> Option<String> {
    let l = s.to_ascii_lowercase();
    let i = l.find("10.")?;
    let tail = &s[i..];
    let end = tail
        .find(|c: char| c.is_whitespace() || matches!(c, '"' | '<' | '>' | ')' | ']'))
        .unwrap_or(tail.len());
    let cand = tail[..end].trim_end_matches(['.', ',', ';']);
    let (reg, suffix) = cand.split_once('/')?;
    let digits = reg.strip_prefix("10.")?;
    (digits.len() >= 4
        && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
        && !suffix.is_empty()
        && (l.contains("doi") || l.contains("doi.org")))
    .then(|| cand.to_owned())
}

fn arxiv_in(s: &str) -> Option<String> {
    let rest = find_after(s, "arxiv:")?;
    let id: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    (id.len() >= 9 && id.contains('.')).then_some(id)
}

/// Title of a reference entry, when it can be read.
pub fn reference_title(entry: &str) -> Option<String> {
    let mut e = entry.trim();
    if let Some(rest) = e.strip_prefix('[')
        && let Some((n, r)) = rest.split_once(']')
        && n.len() <= 4
    {
        e = r.trim();
    } else {
        let digits = e.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && digits <= 3 && e[digits..].starts_with(['.', ')']) {
            e = e[digits + 1..].trim();
        }
    }
    for (a, b) in [('“', '”'), ('"', '"')] {
        if let Some(i) = e.find(a)
            && let Some(j) = e[i + a.len_utf8()..].find(b)
        {
            let t = e[i + a.len_utf8()..i + a.len_utf8() + j]
                .trim()
                .trim_end_matches([',', '.']);
            if t.split_whitespace().count() >= 2 {
                return Some(clip(t, 200));
            }
        }
    }
    let segs: Vec<&str> = e
        .split(". ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    // The title is the segment before the one that names the venue or the year (`NAACL, 2019`, `arXiv ...`, `In Proc ...`).
    let venue_seg = segs.iter().enumerate().skip(2).find(|(_, s)| {
        year_in(s).is_some()
            || lower(s).starts_with("in ")
            || lower(s).starts_with("arxiv")
            || lower(s).starts_with("doi")
            || s.split(|c: char| !c.is_alphanumeric())
                .any(|w| VENUES.contains(&w))
    });
    if let Some((v, _)) = venue_seg {
        let t = segs[v - 1];
        if t.split_whitespace().count() >= 2 || t.chars().count() >= 8 {
            return Some(clip(t.trim_end_matches('.'), 200));
        }
    }
    for (i, s) in segs.iter().enumerate() {
        if i == 0 {
            continue; // authors
        }
        let words = s.split_whitespace().count();
        if words >= 3
            && !s.chars().next().is_some_and(|c| c.is_ascii_digit())
            && !lower(s).starts_with("in ")
            && !lower(s).starts_with("arxiv")
            && !lower(s).starts_with("doi")
        {
            return Some(clip(s.trim_end_matches('.'), 200));
        }
    }
    (segs.len() == 1 && segs[0].split_whitespace().count() >= 3).then(|| clip(segs[0], 200))
}

fn split_refs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let starts_new = t.starts_with('[')
            && t[1..]
                .split(']')
                .next()
                .is_some_and(|n| n.len() <= 4 && n.chars().all(|c| c.is_ascii_digit()))
            || t.chars().take_while(char::is_ascii_digit).count().min(3) > 0
                && t.chars()
                    .find(|c| !c.is_ascii_digit())
                    .is_some_and(|c| c == '.' || c == ')');
        if starts_new || out.is_empty() {
            out.push(t.to_owned());
        } else if let Some(last) = out.last_mut() {
            last.push(' ');
            last.push_str(t);
        }
    }
    out
}

/// A venue or identifier line of the front matter: not part of the author list.
fn front_noise(text: &str) -> bool {
    let l = lower(text);
    l.contains("proceedings")
        || l.contains("arxiv")
        || l.contains("doi")
        || l.contains("preprint")
        || l.contains("journal")
        || l.contains("conference")
        || l.contains("university")
        || l.contains('@')
        || text
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| VENUES.contains(&w))
}

fn terminal(text: &str) -> bool {
    text.trim_end()
        .ends_with(['.', '。', '!', '?', '！', '？', ':', '：', ';', '；'])
}

/// `1 Introduction`, `2.1 Method`, `1 引言`: a numbered heading that the parser delivered as a paragraph.
fn numbered_heading(text: &str) -> bool {
    let t = text.trim();
    let digits = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .count();
    digits > 0 && t[digits..].starts_with(' ') && t.chars().count() <= 60 && !terminal(t)
}

/// The parts of a paper the way the parser delivered them: title and authors, the body after the abstract with wrapped
/// paragraphs joined, and the references with wrapped entries joined. Each carries the first block it came from.
struct Parts {
    title: String,
    title_block: usize,
    authors: Option<usize>,
    front: Vec<usize>,
    refs: Vec<(String, usize)>,
}

fn parts(ir: &DocIr) -> Option<Parts> {
    let vis: Vec<usize> = ir
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| {
            !matches!(
                b.kind,
                BlockKind::PageHeader | BlockKind::PageFooter | BlockKind::Frontmatter
            ) && !b.text.trim().is_empty()
        })
        .map(|(i, _)| i)
        .collect();
    let at = |k: usize| &ir.blocks[vis[k]];
    let abstract_k = vis.iter().take(80).position(|&i| {
        let t = lower(ir.blocks[i].text.trim());
        (t.starts_with("abstract") || t.starts_with("摘要"))
            && ir.blocks[i].text.trim().chars().count() <= 60
            || t.starts_with("abstract") && ir.blocks[i].text.trim().chars().count() > 60
    });
    let refs_k = vis
        .iter()
        .position(|&i| is_heading_like(&ir.blocks[i], REF_HEADINGS));
    let region_end = abstract_k.unwrap_or(vis.len().min(8));
    if region_end == 0 {
        return None;
    }
    let region: Vec<usize> = (0..region_end).collect();
    // A first level heading that is not the abstract is the title; else the lines before the authors.
    let first = at(0);
    let (title, title_block, authors, front);
    if first.kind == BlockKind::Heading && first.level == Some(1) && abstract_k != Some(0) {
        title = first.text.lines().next().unwrap_or("").trim().to_owned();
        title_block = vis[0];
        authors = region
            .iter()
            .skip(1)
            .find(|&&k| !front_noise(&at(k).text) && !split_authors(&at(k).text).is_empty())
            .map(|&k| vis[k]);
        front = region.iter().map(|&k| vis[k]).collect::<Vec<_>>();
    } else {
        let a = region
            .iter()
            .skip(1)
            .rev()
            .find(|&&k| {
                !front_noise(&at(k).text)
                    && !numbered_heading(&at(k).text)
                    && !split_authors(&at(k).text).is_empty()
                    && at(k).text.chars().count() <= 400
            })
            .copied();
        let title_end = a.unwrap_or(1.min(region_end));
        title = region[..title_end.max(1)]
            .iter()
            .take(6)
            .map(|&k| at(k).text.lines().next().unwrap_or("").trim().to_owned())
            .collect::<Vec<_>>()
            .join(" ");
        title_block = vis[0];
        authors = a.map(|k| vis[k]);
        front = region.iter().map(|&k| vis[k]).collect();
    }
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() < 4 {
        return None;
    }
    // References: entries start at `[n]` or `n.`; a line that does not start one continues the entry before it.
    let mut refs: Vec<(String, usize)> = Vec::new();
    if let Some(r) = refs_k {
        for (k, _) in vis.iter().enumerate().skip(r + 1) {
            let b = at(k);
            if b.kind == BlockKind::Heading && !is_heading_like(b, REF_HEADINGS) && !refs.is_empty()
            {
                break;
            }
            for line in split_refs(&b.text) {
                let starts = line.starts_with('[')
                    || line.chars().next().is_some_and(|c| c.is_ascii_digit());
                if !starts && let Some(last) = refs.last_mut() {
                    last.0.push(' ');
                    last.0.push_str(&line);
                } else {
                    refs.push((line, vis[k]));
                }
            }
        }
    }
    Some(Parts {
        title,
        title_block,
        authors,
        front,
        refs,
    })
}

pub(crate) fn extract(path: &str, ir: &DocIr, col: &mut Collector) {
    let doc = EntityRef::new(EntityType::Document, path);
    let Some(p) = parts(ir) else { return };
    let title_ev = EvidenceRef::from_block(&ir.blocks[p.title_block]);
    attr_enum(
        col,
        &doc,
        "class",
        "reference",
        "profile.reference",
        10000,
        &title_ev,
    );
    let reference = thing(REFERENCE, clip(&p.title, 200));
    col.entity(&reference, Some(&title_ev));

    // Authors
    if let Some(i) = p.authors {
        let ev = EvidenceRef::from_block(&ir.blocks[i]);
        for name in split_authors(&ir.blocks[i].text)
            .into_iter()
            .take(MAX_AUTHORS)
        {
            let person = EntityRef::new(EntityType::Person, name);
            col.entity(&person, Some(&ev));
            relation(
                col,
                &person,
                Predicate::Authored,
                &reference,
                "reference.author",
                &ev,
            );
        }
    }
    // Venue, year, DOI and arXiv id of the reference itself (the front matter)
    let front: Vec<Block> = p.front.iter().map(|&i| ir.blocks[i].clone()).collect();
    let venue = find_venue(&front, front.len());
    let mut year = venue.as_ref().and_then(|v| v.1.clone());
    if let Some((v, _, k)) = &venue {
        attr(
            col,
            &reference,
            "venue",
            v,
            "reference.id",
            BP,
            &EvidenceRef::from_block(&front[*k]),
        );
    }
    for b in &front {
        let ev = EvidenceRef::from_block(b);
        if let Some(doi) = doi_in(&b.text) {
            attr(col, &reference, "doi", &doi, "reference.id", BP, &ev);
        }
        if let Some(id) = arxiv_in(&b.text) {
            attr(col, &reference, "arxiv", &id, "reference.id", BP, &ev);
            if year.is_none() && id.len() >= 4 && id[..2].chars().all(|c| c.is_ascii_digit()) {
                year = Some(format!("20{}", &id[..2]));
            }
        }
    }
    if year.is_none() {
        year = front.iter().find_map(|b| {
            (b.text.chars().count() < 160)
                .then(|| year_in(&b.text))
                .flatten()
        });
    }
    if let Some(y) = year {
        attr(col, &reference, "year", &y, "reference.id", BP, &title_ev);
    }
    // Cites: every entry of the reference list.
    for (n, (entry, block)) in p.refs.iter().enumerate() {
        if n >= MAX_REFS {
            break;
        }
        let ev = EvidenceRef::from_block(&ir.blocks[*block]);
        let doi = doi_in(entry);
        let Some(name) =
            reference_title(entry).or_else(|| doi.as_ref().map(|d| format!("doi:{d}")))
        else {
            continue;
        };
        let cited = thing(REFERENCE, clip(&name, 200));
        if cited == reference {
            continue;
        }
        col.entity(&cited, Some(&ev));
        if let Some(d) = doi {
            attr(col, &cited, "doi", &d, "reference.cites", BP, &ev);
        }
        relation(
            col,
            &reference,
            Predicate::Cites,
            &cited,
            "reference.cites",
            &ev,
        );
    }
}

fn relation(
    col: &mut Collector,
    subject: &EntityRef,
    predicate: Predicate,
    object: &EntityRef,
    rule: &'static str,
    ev: &EvidenceRef,
) {
    col.fact(FactMention {
        subject: subject.clone(),
        predicate,
        object: Some(object.clone()),
        value: None,
        valid_from: None,
        valid_to: None,
        valid_precision: None,
        confidence_bp: BP,
        rule_id: rule,
        evidence: vec![ev.clone()],
    });
}
