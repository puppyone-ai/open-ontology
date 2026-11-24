//! `doc.anchor`: amounts and dates that tie documents together (D15, D16).
//!
//! The core model has no type for a number: a budget total is a value, and a value is an attribute of the row that holds it.
//! A report that cites "6,850,000" and the sheet that holds the budget row therefore shared nothing in the graph, although
//! the number is the strongest evidence that they talk about the same thing. This module finds the amounts and the dates a
//! document states and names them as open candidates, `Thing{label: Amount}` and `Thing{label: Date}` (D16: searchable,
//! resolvable, shown under "other related", never promoted by themselves). A document `mentions` each of them with the
//! block that states it as the evidence.
//!
//! Only a value that two or more documents state becomes an entity (the pass that calls [`anchor_links`] counts them): a
//! number in one file ties nothing and would only be noise. The canonical name is the value itself (`6,850,000`,
//! `2027-03-16`), so every way of writing it in the text lands on one entity. Pure.
//!
//! What counts: an amount is a number of at least 10,000 written with thousands separators, with a currency marker next
//! to it (`¥`, `$`, `CNY`, `RMB`, `USD`, `元`), or as `N万`/`N million`; a date is `YYYY-MM-DD`, `YYYY/M/D` or `YYYY年M月D日`
//! and is not the timestamp of a log line (a time of day follows it). Percentages, years, ids and phone numbers are not
//! amounts.

use std::collections::{BTreeMap, BTreeSet};

use oo_core::docir::{Block, BlockKind, DocIr};

use super::model::{
    Collector, EntityRef, EntityType, EvidenceRef, Extraction, FactMention, Predicate,
};

/// `Thing` labels of the anchors.
pub const AMOUNT: &str = "Amount";
pub const DATE: &str = "Date";
/// Confidence of a `mentions` link of an anchor (the text states the value).
pub const ANCHOR_BP: u16 = 9000;
/// Smallest amount that is an anchor.
pub const MIN_AMOUNT: u64 = 10_000;
/// Anchors one document may own (the most shared first).
pub const MAX_PER_DOC: usize = 40;

/// A value a document states.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Found {
    pub label: &'static str,
    /// Canonical name (`6,850,000`, `2027-03-16`).
    pub name: String,
    /// First block that states it.
    pub block: usize,
}

fn is_text_block(b: &Block) -> bool {
    !matches!(
        b.kind,
        BlockKind::PageHeader | BlockKind::PageFooter | BlockKind::Footnote
    ) && !b.text.trim().is_empty()
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn digits_at(cs: &[char], from: usize) -> usize {
    cs[from..].iter().take_while(|c| c.is_ascii_digit()).count()
}

const CURRENCY_BEFORE: &[&str] = &[
    "¥", "￥", "$", "cny", "rmb", "usd", "eur", "rmb ", "cny ", "usd ", "eur ", "¥ ", "$ ",
];
const CURRENCY_AFTER: &[&str] = &[
    "元", "cny", "rmb", "usd", "eur", " cny", " rmb", " usd", " eur", " yuan", "块",
];

fn before_is(cs: &[char], at: usize, any: &[&str]) -> bool {
    let tail: String = cs[at.saturating_sub(5)..at]
        .iter()
        .collect::<String>()
        .to_lowercase();
    any.iter().any(|m| tail.ends_with(m))
}

fn after_is(cs: &[char], at: usize, any: &[&str]) -> bool {
    let head: String = cs[at..(at + 9).min(cs.len())]
        .iter()
        .collect::<String>()
        .to_lowercase();
    any.iter().any(|m| head.starts_with(m))
}

/// Amounts of one text, as integers of the smallest written unit (whole currency units).
fn amounts(text: &str) -> Vec<u64> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if !cs[i].is_ascii_digit()
            || (i > 0
                && (cs[i - 1].is_ascii_alphanumeric()
                    || cs[i - 1] == '-'
                    || cs[i - 1] == '/'
                    || cs[i - 1] == ':'))
        {
            // Inside a word, an id, a date or a time: skip the whole run.
            while i < cs.len() && (cs[i].is_ascii_alphanumeric()) {
                i += 1;
            }
            i += usize::from(i < cs.len() && !cs[i].is_ascii_digit());
            continue;
        }
        let start = i;
        // digits with `,` thousands groups
        let mut end = i + digits_at(&cs, i);
        let mut grouped = false;
        while end + 3 < cs.len() && cs[end] == ',' && digits_at(&cs, end + 1) == 3 {
            end += 4;
            grouped = true;
        }
        // decimal part
        let mut frac = String::new();
        if end + 1 < cs.len() && cs[end] == '.' && cs[end + 1].is_ascii_digit() {
            let d = digits_at(&cs, end + 1);
            frac = cs[end + 1..end + 1 + d].iter().collect();
            end += 1 + d;
        }
        let int: String = cs[start..end - if frac.is_empty() { 0 } else { frac.len() + 1 }]
            .iter()
            .filter(|c| c.is_ascii_digit())
            .collect();
        let next = cs.get(end).copied();
        let glued = next.is_some_and(|c| c.is_ascii_alphanumeric() && !matches!(c, 'm' | 'M'))
            || next == Some('-')
            || next == Some(':')
            || next == Some('/')
            || next == Some('%')
            || next == Some('％');
        let value = int.parse::<u64>().ok();
        let mut taken = false;
        if let (Some(v), false) = (value, int.len() > 12) {
            let wan = next == Some('万');
            let million = after_is(&cs, end, &[" million", "million"]);
            if wan || million {
                let f: f64 = format!("{int}.{}", if frac.is_empty() { "0" } else { &frac })
                    .parse()
                    .unwrap_or(0.0);
                let unit = if wan { 10_000.0 } else { 1_000_000.0 };
                let n = (f * unit).round() as u64;
                if n >= MIN_AMOUNT {
                    out.push(n);
                    taken = true;
                }
            } else if !glued && frac.is_empty() && v >= MIN_AMOUNT {
                let cur =
                    before_is(&cs, start, CURRENCY_BEFORE) || after_is(&cs, end, CURRENCY_AFTER);
                if grouped || (cur && int.len() <= 11) {
                    out.push(v);
                    taken = true;
                }
            } else if !glued && !frac.is_empty() && v >= MIN_AMOUNT && grouped {
                // 6,850,000.00 reads as whole units when the decimals are zero.
                if frac.chars().all(|c| c == '0') {
                    out.push(v);
                    taken = true;
                }
            }
        }
        let _ = taken;
        i = end.max(i + 1);
    }
    out
}

fn date_parts(cs: &[char], at: usize) -> Option<(String, usize)> {
    let y = digits_at(cs, at);
    if y != 4 || (at > 0 && cs[at - 1].is_ascii_alphanumeric()) {
        return None;
    }
    let year: u32 = cs[at..at + 4].iter().collect::<String>().parse().ok()?;
    let mut p = at + 4;
    let sep = *cs.get(p)?;
    let (month, day, end) = if sep == '年' {
        p += 1;
        let m = digits_at(cs, p);
        if !(1..=2).contains(&m) {
            return None;
        }
        let month: u32 = cs[p..p + m].iter().collect::<String>().parse().ok()?;
        p += m;
        if cs.get(p) != Some(&'月') {
            return None;
        }
        p += 1;
        let d = digits_at(cs, p);
        if !(1..=2).contains(&d) {
            return None;
        }
        let day: u32 = cs[p..p + d].iter().collect::<String>().parse().ok()?;
        p += d;
        if matches!(cs.get(p), Some('日' | '号')) {
            p += 1;
        }
        (month, day, p)
    } else if sep == '-' || sep == '/' || sep == '.' {
        p += 1;
        let m = digits_at(cs, p);
        if !(1..=2).contains(&m) {
            return None;
        }
        let month: u32 = cs[p..p + m].iter().collect::<String>().parse().ok()?;
        p += m;
        if cs.get(p) != Some(&sep) {
            return None;
        }
        p += 1;
        let d = digits_at(cs, p);
        if !(1..=2).contains(&d) {
            return None;
        }
        let day: u32 = cs[p..p + d].iter().collect::<String>().parse().ok()?;
        p += d;
        (month, day, p)
    } else {
        return None;
    };
    if !(1900..=2100).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // A time of day after the date makes it a log timestamp.
    let mut t = end;
    while cs.get(t) == Some(&' ') || cs.get(t) == Some(&'T') {
        t += 1;
    }
    if t > end && digits_at(cs, t) >= 1 && cs.get(t + digits_at(cs, t)) == Some(&':') {
        return None;
    }
    if cs.get(end).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((format!("{year:04}-{month:02}-{day:02}"), end))
}

fn dates(text: &str) -> Vec<String> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i].is_ascii_digit() {
            if let Some((d, end)) = date_parts(&cs, i) {
                out.push(d);
                i = end;
                continue;
            }
            i += digits_at(&cs, i);
        } else {
            i += 1;
        }
    }
    out
}

/// Every amount and date a document states, once each, with the first block that states it.
pub fn scan(ir: &DocIr) -> Vec<Found> {
    let mut seen: BTreeMap<(&'static str, String), usize> = BTreeMap::new();
    for (i, b) in ir
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| is_text_block(b))
    {
        let text: String = b
            .text
            .chars()
            .map(|c| if c == '\u{a0}' { ' ' } else { c })
            .collect();
        for n in amounts(&text) {
            seen.entry((AMOUNT, group(n))).or_insert(i);
        }
        for d in dates(&text) {
            seen.entry((DATE, d)).or_insert(i);
        }
    }
    seen.into_iter()
        .map(|((label, name), block)| Found { label, name, block })
        .collect()
}

/// The values stated in at least two of `docs` (per document, what it states), as `(label, name)`.
pub fn shared<'a>(docs: impl IntoIterator<Item = &'a [Found]>) -> BTreeSet<(&'static str, String)> {
    let mut count: BTreeMap<(&'static str, String), usize> = BTreeMap::new();
    for d in docs {
        for f in d {
            *count.entry((f.label, f.name.clone())).or_default() += 1;
        }
    }
    count
        .into_iter()
        .filter(|(_, n)| *n >= 2)
        .map(|(k, _)| k)
        .collect()
}

/// The `mentions` of one document to the shared values it states (at most [`MAX_PER_DOC`]).
pub fn anchor_links(
    path: &str,
    ir: &DocIr,
    found: &[Found],
    wanted: &BTreeSet<(&'static str, String)>,
) -> Extraction {
    let doc = EntityRef::new(EntityType::Document, path);
    let mut col = Collector::default();
    for f in found
        .iter()
        .filter(|f| wanted.contains(&(f.label, f.name.clone())))
        .take(MAX_PER_DOC)
    {
        let ev = EvidenceRef::from_block(&ir.blocks[f.block]);
        let thing = EntityRef {
            ty: EntityType::Thing,
            thing_label: Some(f.label.to_owned()),
            subkind: None,
            name: f.name.clone(),
        };
        col.fact(FactMention {
            subject: doc.clone(),
            predicate: Predicate::Mentions,
            object: Some(thing),
            value: None,
            valid_from: None,
            valid_to: None,
            valid_precision: None,
            confidence_bp: ANCHOR_BP,
            rule_id: "doc.anchor",
            evidence: vec![ev],
        });
    }
    col.finish(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::markdown::parse_markdown;

    fn found(md: &str) -> Vec<(String, String)> {
        let ir = parse_markdown(md.as_bytes()).unwrap().ir;
        scan(&ir)
            .into_iter()
            .map(|f| (f.label.to_owned(), f.name))
            .collect()
    }

    #[test]
    fn amounts_with_separators_markers_and_units_are_found() {
        let v = found(
            "Total launch budget 6,850,000.\n\nPrint moves CNY 80000 to video, 142万元 for marketing, 2.5 million for lenses.",
        );
        for want in ["6,850,000", "80,000", "1,420,000", "2,500,000"] {
            assert!(v.contains(&("Amount".into(), want.into())), "{want}: {v:?}");
        }
    }

    #[test]
    fn plain_numbers_ids_percentages_years_and_phones_are_not_amounts() {
        let v = found(
            "Yield 88% against 85%. Order 2027 shipped, ticket 123456, call 13800138000, part A12345678, 9,999 units, 12345 pieces.",
        );
        assert!(v.iter().all(|(l, _)| l != "Amount"), "{v:?}");
    }

    #[test]
    fn dates_in_three_notations_are_one_canonical_form() {
        let v = found(
            "Embargo 2027-03-16.\n\nThe press embargo lifts on 2027年3月16日.\n\nQA review 2027/3/5.",
        );
        assert_eq!(
            v.iter()
                .filter(|(l, n)| l == "Date" && n == "2027-03-16")
                .count(),
            1
        );
        assert!(v.contains(&("Date".into(), "2027-03-05".into())));
    }

    #[test]
    fn a_log_timestamp_and_an_impossible_date_are_not_dates() {
        let v = found(
            "[2027-01-13 10:02] Signed memo is out.\n\n2027-13-40 and 2027-01-135 and 12027-01-13",
        );
        assert!(v.iter().all(|(l, _)| l != "Date"), "{v:?}");
    }

    #[test]
    fn only_a_value_two_documents_state_is_shared() {
        let a = scan(
            &parse_markdown("Budget 6,850,000 due 2027-03-16.".as_bytes())
                .unwrap()
                .ir,
        );
        let b = scan(
            &parse_markdown("Cited budget 6,850,000.".as_bytes())
                .unwrap()
                .ir,
        );
        let s = shared([a.as_slice(), b.as_slice()]);
        assert_eq!(s.len(), 1);
        assert!(s.contains(&(AMOUNT, "6,850,000".to_owned())));
        let ir = parse_markdown("Cited budget 6,850,000.".as_bytes())
            .unwrap()
            .ir;
        let x = anchor_links("a.md", &ir, &b, &s);
        assert_eq!(x.facts.len(), 1);
        assert_eq!(
            x.facts[0].object.as_ref().unwrap().thing_label.as_deref(),
            Some("Amount")
        );
    }
}
