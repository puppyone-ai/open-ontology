//! The values inside running text: amounts, dates and ratios with the span they were written in. Pure.
//!
//! A candidate is a value only when its context says so: an amount has a currency next to it, or a scale word
//! (`万`, `亿`, `million`), or digits grouped by thousands; a count of things (`一万人`), an ordinal (`第一`), a degree
//! adverb (`十分好`), a phone number or an id is not one.

use super::dec::Dec;
use super::money::{self, Money};
use super::ratio::{self, Ratio};
use super::{DateCtx, NormDate, date, en, zh};

/// What a span of text says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Amount(Money),
    Date(NormDate),
    Ratio(Ratio),
}

/// One value found in a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Character offsets into the text (`end` exclusive).
    pub start: usize,
    pub end: usize,
    /// The text as written.
    pub text: String,
    pub value: Value,
}

/// Smallest whole amount of digits grouped by thousands that is an amount without a currency (as `extract::anchors`).
pub const MIN_GROUPED: u64 = 10_000;

/// Words and characters after a number that make it a count of things, not money.
const COUNTERS_ZH: &[&str] = &[
    "人",
    "个",
    "台",
    "套",
    "件",
    "次",
    "用户",
    "条",
    "张",
    "辆",
    "只",
    "份",
    "倍",
    "公里",
    "米",
    "吨",
    "天",
    "年",
    "小时",
    "分钟",
    "秒",
    "页",
    "行",
    "字",
    "位",
    "名",
    "家",
    "间",
    "所",
    "项",
    "例",
    "款",
    "种",
    "批",
    "期",
    "场",
    "多",
    "余",
    "几",
    "来",
    "岁",
    "公斤",
    "千克",
    "克",
    "升",
    "平方",
    "亩",
    "度",
    "级",
    "届",
    "代",
    "片",
    "颗",
    "块钱以上",
];
const COUNTERS_EN: &[&str] = &[
    "users",
    "people",
    "units",
    "pcs",
    "pieces",
    "items",
    "customers",
    "employees",
    "devices",
    "downloads",
    "views",
    "visits",
    "km",
    "kg",
    "miles",
    "meters",
    "metres",
    "tons",
    "tonnes",
    "hours",
    "minutes",
    "seconds",
    "days",
    "years",
    "months",
    "weeks",
    "pages",
    "words",
    "lines",
    "rows",
    "orders",
    "sales",
    "shares",
    "votes",
    "stars",
    "mb",
    "gb",
    "tb",
    "bytes",
    "hz",
    "mah",
    "nits",
    "lumens",
    "pixels",
    "px",
    "sqm",
    "sqft",
    "acres",
    "kilometers",
    "kilometres",
    "cases",
    "tickets",
    "points",
    "hits",
    "requests",
    "lenses",
    "frames",
    "boxes",
    "cartons",
    "pairs",
    "sets",
    "copies",
];

fn lower_at(cs: &[char], i: usize, len: usize) -> String {
    cs[i.min(cs.len())..(i + len).min(cs.len())]
        .iter()
        .collect::<String>()
        .to_lowercase()
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// The currency word right after `at` (one optional space), and where it ends.
fn currency_after(cs: &[char], at: usize) -> Option<(&'static str, usize)> {
    let p = if cs.get(at) == Some(&' ') { at + 1 } else { at };
    for (w, code) in money::SUFFIXES {
        let n = w.chars().count();
        if lower_at(cs, p, n) == *w {
            // An ASCII code or word ends at a word boundary: `usd` but not `usdc`, `yuan` but not `yuanbao`.
            if w.is_ascii() && cs.get(p + n).is_some_and(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            // A space before an ASCII word is fine; before `元` it is not a currency (`3 元素`).
            if *w == "元"
                && cs
                    .get(p + 1)
                    .is_some_and(|c| matches!(c, '素' | '旦' | '老' | '宇' | '帅'))
            {
                continue;
            }
            return Some((code, p + n));
        }
    }
    None
}

/// A currency written before the number at `i`, and where it ends (before the number).
fn currency_before(cs: &[char], i: usize) -> Option<(&'static str, usize)> {
    for (w, code) in money::PREFIXES {
        let n = w.chars().count();
        if lower_at(cs, i, n) == *w {
            // `usd`, `rmb`: a code only when it is not part of a longer word.
            if w.is_ascii() && w.chars().all(|c| c.is_ascii_alphabetic()) {
                if i > 0 && cs[i - 1].is_ascii_alphabetic() {
                    continue;
                }
                if cs.get(i + n).is_some_and(|c| c.is_ascii_alphabetic()) {
                    continue;
                }
            }
            let mut p = i + n;
            if cs.get(p) == Some(&' ') {
                p += 1;
            }
            return Some((code, p));
        }
    }
    None
}

fn counter_follows(cs: &[char], end: usize) -> bool {
    let p = if cs.get(end) == Some(&' ') {
        end + 1
    } else {
        end
    };
    for w in COUNTERS_ZH {
        let n = w.chars().count();
        if cs.len() >= p + n && cs[p..p + n].iter().collect::<String>() == *w {
            return true;
        }
    }
    let l = lower_at(cs, p, 12);
    COUNTERS_EN
        .iter()
        .any(|w| l.starts_with(w) && !l[w.len()..].starts_with(|c: char| c.is_ascii_alphabetic()))
}

#[derive(Clone, Copy, Debug)]
struct Num {
    end: usize,
    value: Dec,
    /// Scale word written, and whether it is a strict one (not `k`, `m`, `b`).
    scale: Option<(&'static str, bool)>,
    grouped: bool,
}

fn arabic_run(cs: &[char], i: usize) -> Option<(Dec, usize, bool)> {
    let mut e = i;
    while e < cs.len()
        && (cs[e].is_ascii_digit()
            || ((cs[e] == ',' || cs[e] == '.') && cs.get(e + 1).is_some_and(char::is_ascii_digit)))
    {
        e += 1;
    }
    while e > i {
        let t: String = cs[i..e].iter().collect();
        if let Some(d) = Dec::parse_ascii(&t) {
            return Some((d, e, t.contains(',')));
        }
        e -= 1;
    }
    None
}

/// A number written with digits, possibly followed by Chinese units (`3.5亿`) or an English scale (`6.85 million`).
fn read_arabic(cs: &[char], i: usize) -> Option<Num> {
    read_arabic_g(cs, i, true)
}

/// `guard` rejects a number that starts inside a word, a date, a time or an id; a number after a currency has none.
fn read_arabic_g(cs: &[char], i: usize, guard: bool) -> Option<Num> {
    if guard
        && i > 0
        && (is_word_char(cs[i - 1]) || matches!(cs[i - 1], '-' | '/' | ':' | '.' | ','))
    {
        return None;
    }
    let (value, e, grouped) = arabic_run(cs, i)?;
    // units (`万`, `千万`, `亿`)
    let mut u = e;
    while u < cs.len() && (zh::big_unit(cs[u]).is_some() || zh::small_unit(cs[u]).is_some()) {
        u += 1;
    }
    if u > e {
        let t: String = cs[i..u].iter().collect();
        let t = t.replace(',', "");
        let v = zh::parse(&t)?;
        let scale = if cs[e..u].iter().any(|c| *c == '亿' || *c == '億') {
            "亿"
        } else if cs[e..u].iter().any(|c| *c == '万' || *c == '萬') {
            "万"
        } else {
            return Some(Num {
                end: u,
                value: v,
                scale: None,
                grouped,
            });
        };
        return Some(Num {
            end: u,
            value: v,
            scale: Some((scale, true)),
            grouped,
        });
    }
    // English scale
    let p = if cs.get(e) == Some(&' ') { e + 1 } else { e };
    let mut we = p;
    while we < cs.len() && cs[we].is_ascii_alphabetic() {
        we += 1;
    }
    if we > p {
        let w: String = cs[p..we].iter().collect::<String>().to_lowercase();
        if let Some((mult, strict)) = en::scale(&w)
            && !cs.get(we).is_some_and(|c| c.is_ascii_alphanumeric())
        {
            let name = match w.as_str() {
                "thousand" | "thousands" => "thousand",
                "million" | "millions" => "million",
                "billion" | "billions" => "billion",
                "trillion" | "trillions" => "trillion",
                "mn" => "mn",
                "bn" => "bn",
                "tn" => "tn",
                "k" => "k",
                "m" => "m",
                _ => "b",
            };
            return Some(Num {
                end: we,
                value: value.times_u128(mult)?,
                scale: Some((name, strict)),
                grouped,
            });
        }
    }
    // Glued to letters (`123abc`), a date, a time, an id: not a number.
    if cs
        .get(e)
        .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '-' | ':' | '/'))
    {
        return None;
    }
    Some(Num {
        end: e,
        value,
        scale: None,
        grouped,
    })
}

fn read_zh(cs: &[char], i: usize) -> Option<(Num, usize)> {
    if i > 0 && (cs[i - 1] == '第' || cs[i - 1].is_ascii_digit()) {
        return None;
    }
    let mut e = i;
    while e < cs.len()
        && (zh::is_numeral_char(cs[e])
            || (cs[e] == '点' && cs.get(e + 1).is_some_and(|c| zh::digit(*c).is_some())))
    {
        e += 1;
    }
    if e == i {
        return None;
    }
    // Try the whole run, then shorter runs (`三百万元` followed by more numerals is rare, `一万二` is spoken).
    let mut end = e;
    while end > i {
        let t: String = cs[i..end].iter().collect();
        if let Some(v) = zh::parse(&t) {
            let scale = if t.contains('亿') || t.contains('億') {
                Some(("亿", true))
            } else if t.contains('万') || t.contains('萬') {
                Some(("万", true))
            } else {
                None
            };
            return Some((
                Num {
                    end,
                    value: v,
                    scale,
                    grouped: false,
                },
                e,
            ));
        }
        end -= 1;
    }
    None
}

fn read_en(cs: &[char], i: usize) -> Option<Num> {
    if i > 0 && (is_word_char(cs[i - 1]) || cs[i - 1] == '-') {
        return None;
    }
    // tokens
    let mut toks: Vec<(String, usize, usize)> = Vec::new();
    let mut p = i;
    loop {
        let mut e = p;
        while e < cs.len() && cs[e].is_ascii_alphabetic() {
            e += 1;
        }
        if e == p {
            break;
        }
        let w: String = cs[p..e].iter().collect::<String>().to_lowercase();
        let ok = en::is_number_word(&w) || ((w == "a" || w == "an") && toks.is_empty());
        if !ok {
            break;
        }
        toks.push((w, p, e));
        // separator: one space or hyphen, and more letters
        if matches!(cs.get(e), Some(' ' | '-'))
            && cs.get(e + 1).is_some_and(char::is_ascii_alphabetic)
        {
            p = e + 1;
        } else {
            break;
        }
    }
    if toks.is_empty() || !en::starts_number(&toks[0].0) {
        return None;
    }
    if (toks[0].0 == "a" || toks[0].0 == "an")
        && !toks.get(1).is_some_and(|t| {
            matches!(
                t.0.as_str(),
                "hundred" | "thousand" | "million" | "billion" | "trillion"
            )
        })
    {
        return None;
    }
    while !toks.is_empty() {
        let text: String = toks
            .iter()
            .map(|t| t.0.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(r) = en::parse(&text, false) {
            let end = toks.last().map_or(i, |t| t.2);
            return Some(Num {
                end,
                value: r.value,
                scale: r.scale,
                grouped: false,
            });
        }
        toks.pop();
    }
    None
}

fn amount_from(
    cs: &[char],
    start: usize,
    n: Num,
    prefix_currency: Option<&'static str>,
) -> Option<(Money, usize)> {
    let suffix = currency_after(cs, n.end);
    let currency = prefix_currency.or(suffix.map(|s| s.0));
    let end = suffix.map_or(n.end, |s| s.1);
    let strict_scale = matches!(n.scale, Some((_, true)));
    let _ = start;
    if currency.is_none() {
        let big = strict_scale
            || (n.grouped
                && n.value.to_u64().is_some_and(|v| v >= MIN_GROUPED)
                && n.value.is_integer());
        if !big || counter_follows(cs, n.end) {
            return None;
        }
    }
    // `1.2k` without a currency is not an amount.
    if currency.is_none() && matches!(n.scale, Some((_, false))) {
        return None;
    }
    Some((
        Money {
            amount: n.value,
            currency,
            scale: n.scale.map(|s| match s.0 {
                "万" => "万",
                "亿" => "亿",
                "thousand" => "thousand",
                "million" => "million",
                "billion" => "billion",
                "trillion" => "trillion",
                "mn" => "mn",
                "bn" => "bn",
                "tn" => "tn",
                "k" => "k",
                "m" => "m",
                _ => "b",
            }),
        },
        end,
    ))
}

fn is_zh_or_digit_start(c: char) -> bool {
    c.is_ascii_digit() || zh::is_numeral_char(c)
}

/// What starts at `i`, and where it ends.
fn try_at(cs: &[char], i: usize, ctx: &DateCtx) -> Option<(Value, usize)> {
    let c = cs[i];
    // dates
    if let Some((d, e)) = date::at(cs, i, ctx) {
        return Some((Value::Date(d), e));
    }
    // percentages
    if (c.is_ascii_digit() || c == '百')
        && let Some((r, e)) = ratio::at(cs, i)
    {
        return Some((Value::Ratio(r), e));
    }
    // a currency in front
    if let Some((cur, p)) = currency_before(cs, i)
        && p < cs.len()
    {
        let n = if cs[p].is_ascii_digit() {
            read_arabic_g(cs, p, false)
        } else if zh::is_numeral_char(cs[p]) {
            read_zh(cs, p).map(|x| x.0)
        } else {
            read_en(cs, p)
        };
        if let Some(n) = n
            && let Some((m, e)) = amount_from(cs, i, n, Some(cur))
        {
            return Some((Value::Amount(m), e));
        }
    }
    // a bare number
    let n = if c.is_ascii_digit() {
        read_arabic(cs, i)
    } else if zh::is_numeral_char(c) {
        let (n, _) = read_zh(cs, i)?;
        // `十分好`, `一些`, `万一`: not numbers here.
        if cs.get(i + 1) == Some(&'分') && n.end == i + 1 && !matches!(cs.get(i + 2), Some('之'))
        {
            return None;
        }
        Some(n)
    } else if c.is_ascii_alphabetic() {
        read_en(cs, i)
    } else {
        None
    }?;
    // `十二%`, `twelve percent`
    let after = if cs.get(n.end) == Some(&' ') {
        n.end + 1
    } else {
        n.end
    };
    if !c.is_ascii_digit() {
        if matches!(cs.get(n.end), Some('%' | '％')) {
            return Ratio::from_percent(n.value).map(|r| (Value::Ratio(r), n.end + 1));
        }
        let l = lower_at(cs, after, 8);
        for w in ["per cent", "percent", "pct"] {
            if l.starts_with(w)
                && !cs
                    .get(after + w.chars().count())
                    .is_some_and(|c| c.is_ascii_alphabetic())
            {
                return Ratio::from_percent(n.value)
                    .map(|r| (Value::Ratio(r), after + w.chars().count()));
            }
        }
    }
    // `三成`
    if zh::is_numeral_char(c) && cs.get(n.end) == Some(&'成') {
        let mut e = n.end + 1;
        let mut t: String = cs[i..e].iter().collect();
        if let Some(d) = cs
            .get(e)
            .filter(|d| zh::digit(**d).is_some() || **d == '半')
        {
            t.push(*d);
            e += 1;
        }
        let r = ratio::parse(&t)?;
        return Some((Value::Ratio(r), e));
    }
    let (m, e) = amount_from(cs, i, n, None)?;
    Some((Value::Amount(m), e))
}

/// Every amount, date and ratio of `text`, left to right, without overlaps. Relative dates and a missing year are read
/// against `ctx`.
pub fn scan(text: &str, ctx: &DateCtx) -> Vec<Found> {
    let cs: Vec<char> = text
        .chars()
        .map(|c| if c == '\u{a0}' { ' ' } else { c })
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let start_ok = if c.is_ascii_alphanumeric() {
            i == 0 || !cs[i - 1].is_ascii_alphanumeric()
        } else {
            true
        };
        if start_ok
            && let Some((value, end)) = try_at(&cs, i, ctx)
            && end > i
        {
            out.push(Found {
                start: i,
                end,
                text: cs[i..end].iter().collect(),
                value,
            });
            i = end;
            continue;
        }
        // Skip what cannot start a value in the middle: the rest of a number or a word.
        if is_zh_or_digit_start(c) {
            let mut e = i + 1;
            while e < cs.len()
                && (cs[e].is_ascii_digit()
                    || zh::is_numeral_char(cs[e])
                    || (cs[e].is_ascii_alphanumeric() && cs[i].is_ascii_digit()))
            {
                e += 1;
            }
            i = e;
        } else if c.is_ascii_alphabetic() {
            while i < cs.len() && cs[i].is_ascii_alphanumeric() {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}
