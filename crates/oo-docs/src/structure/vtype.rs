//! The type of a property from its values (D93), the way a database column is profiled. Pure.

use std::collections::BTreeMap;

use unicode_normalization::UnicodeNormalization;

use super::ValueType;
use crate::extract::rules::dates::parse_exact;
use crate::resolve::normalize::norm;

/// Share of the values that must agree before a type is chosen.
const AGREE: f64 = 0.8;
/// An enum has few distinct values that repeat.
const ENUM_MAX_DISTINCT: usize = 12;
const ENUM_MIN_VALUES: usize = 5;
const ENUM_MAX_RATIO: f64 = 0.6;
const ENUM_MAX_CHARS: usize = 24;
/// Characters from which a text is long.
const LONG_CHARS: usize = 60;

/// Entities of the Drive by canonical name, per entity type (`Person`, `Project`, `Organization`).
#[derive(Clone, Debug, Default)]
pub struct Lexicon {
    names: BTreeMap<String, Vec<String>>,
}

impl Lexicon {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, ty: &str, name: &str) {
        let n = norm(name);
        if n.is_empty() {
            return;
        }
        let v = self.names.entry(n).or_default();
        if !v.iter().any(|t| t == ty) {
            v.push(ty.to_owned());
        }
    }
    /// The entity types a value names.
    pub fn types_of(&self, value: &str) -> &[String] {
        self.names.get(&norm(value)).map_or(&[], Vec::as_slice)
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// What the profile of one property found.
#[derive(Clone, Debug, PartialEq)]
pub struct Inferred {
    pub value_type: ValueType,
    pub multi: bool,
    pub enum_values: Vec<String>,
    /// Share of the items named by an entity of the lexicon, for the winning entity type.
    pub resolved: f64,
}

/// Split one value into its items: names and lists are separated by `、，,；;/` or a line break.
pub fn items_of(value: &str) -> Vec<String> {
    value
        .split(['、', '，', ',', '；', ';', '/', '\n', '／'])
        .map(|s| {
            strip_parens(s)
                .trim()
                .trim_end_matches(['。', '.'])
                .trim()
                .to_owned()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn strip_parens(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '(' | '（' => depth += 1,
            ')' | '）' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn is_email(s: &str) -> bool {
    let s = s.trim();
    let Some((l, d)) = s.split_once('@') else {
        return false;
    };
    !l.is_empty()
        && d.contains('.')
        && !d.starts_with('.')
        && !d.ends_with('.')
        && !s.contains(char::is_whitespace)
}

fn is_url(s: &str) -> bool {
    let s = s.trim().to_lowercase();
    (s.starts_with("http://") || s.starts_with("https://")) && !s.contains(char::is_whitespace)
}

fn is_date(s: &str) -> bool {
    let t = s.trim();
    if parse_exact(t).is_some() {
        return true;
    }
    // A date written in words (`二〇二七年三月十六日`, `March 16, 2027`, `3月16日`, `下周二`, `tomorrow`): read against a document later.
    if crate::values::date::is_date_text(t) {
        return true;
    }
    // A date with a time of day (`2026-03-04 10:00`, `2026-03-04T10:00:00`).
    let head: String = t
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != 'T')
        .collect();
    let rest = t[head.len()..].trim_start_matches(['T', ' ']).trim();
    parse_exact(&head).is_some()
        && !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ':' | '.' | 'Z' | '+' | '-'))
}

const CURRENCY: &[&str] = &[
    "¥",
    "￥",
    "$",
    "€",
    "£",
    "元",
    "万元",
    "万",
    "亿",
    "usd",
    "rmb",
    "cny",
    "eur",
    "美元",
    "人民币",
    "k",
    "m",
];

/// A number or an amount written in words (`六百八十五万元`, `six point eight five million`): `c:` marks an amount.
fn number_words(s: &str) -> Option<String> {
    let t = s.trim();
    if let Some(m) = crate::values::money::parse(t, false) {
        return Some(format!("c:{}", m.amount.to_plain()));
    }
    let has_word = t
        .chars()
        .any(|c| crate::values::zh::is_numeral_char(c) || c.is_ascii_alphabetic());
    if !has_word {
        return None;
    }
    crate::values::money::number(t, false).map(|(d, _)| d.to_plain())
}

fn number_core(s: &str) -> Option<String> {
    let t: String = s
        .nfkc()
        .collect::<String>()
        .trim()
        .to_lowercase()
        .replace(',', "");
    let mut stripped = t.as_str();
    let mut had_currency = false;
    loop {
        let before = stripped;
        for c in CURRENCY {
            if let Some(r) = stripped.strip_prefix(c) {
                stripped = r.trim_start();
                had_currency = true;
            }
            if let Some(r) = stripped.strip_suffix(c) {
                stripped = r.trim_end();
                had_currency = true;
            }
        }
        if stripped == before {
            break;
        }
    }
    let core = stripped.trim();
    let digits = core.chars().filter(char::is_ascii_digit).count();
    let ok = digits > 0
        && core
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | ' '));
    ok.then(|| {
        if had_currency {
            format!("c:{core}")
        } else {
            core.to_owned()
        }
    })
}

/// `v1.2.3`, `2.0.1`, `1.4`, with an optional pre-release or build suffix.
fn is_version(s: &str) -> bool {
    let t = s.trim().trim_start_matches(['v', 'V']);
    let core = t.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    (2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// A short level code such as `sev1`, `P0`, `L2`, `Tier-3`.
fn level_code(s: &str) -> bool {
    let b = s.trim().as_bytes();
    let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    let rest = &b[letters..];
    (1..=5).contains(&letters)
        && !rest.is_empty()
        && rest.len() <= 3
        && rest
            .iter()
            .enumerate()
            .all(|(i, c)| c.is_ascii_digit() || (i == 0 && *c == b'-'))
}

fn is_percent(s: &str) -> bool {
    let t: String = s.nfkc().collect();
    t.trim()
        .strip_suffix('%')
        .is_some_and(|n| n.trim().parse::<f64>().is_ok())
        || crate::values::ratio::parse(s).is_some()
}

fn looks_like_id(s: &str) -> bool {
    let b = s.trim().as_bytes();
    if b.len() < 3 || b.len() > 32 {
        return false;
    }
    let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    let digits = b.iter().filter(|c| c.is_ascii_digit()).count();
    (1..=6).contains(&letters)
        && digits >= 2
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

/// A person's name as written: 2 to 4 Han characters, or one to three Latin words.
pub fn name_like(s: &str) -> bool {
    let s = s.trim();
    let n = s.chars().count();
    if n == 0
        || n > 30
        || s.chars()
            .any(|c| c.is_ascii_digit() || matches!(c, ':' | '：' | '，' | ',' | '。' | '.' | '@'))
    {
        return false;
    }
    let han = s
        .chars()
        .filter(|c| crate::resolve::pinyin::is_han(*c))
        .count();
    if han > 0 {
        return han == n && (2..=4).contains(&n);
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    (1..=3).contains(&words.len())
        && words
            .iter()
            .all(|w| w.chars().next().is_some_and(char::is_uppercase))
}

fn org_like(s: &str) -> bool {
    let s = s.trim();
    let n = s.chars().count();
    (2..=40).contains(&n) && !s.chars().any(|c| matches!(c, '：' | ':' | '。' | '，'))
}

fn project_like(s: &str) -> bool {
    let s = s.trim();
    let n = s.chars().count();
    (2..=40).contains(&n)
        && !s
            .chars()
            .any(|c| matches!(c, '：' | ':' | '。' | '，' | ',' | '.'))
}

/// The entity type a property key suggests (`owner` names a person, `customer` an organization).
pub fn key_hint(key: &str) -> Option<ValueType> {
    match key {
        "attendees" | "owner" | "decided_by" | "author" | "manager" | "host" => {
            Some(ValueType::Person)
        }
        "project" => Some(ValueType::Project),
        "customer" | "party_a" | "party_b" => Some(ValueType::Organization),
        _ => None,
    }
}

fn share(items: &[String], f: impl Fn(&str) -> bool) -> f64 {
    if items.is_empty() {
        return 0.0;
    }
    items.iter().filter(|i| f(i)).count() as f64 / items.len() as f64
}

/// Infer the type of a property from the values of every record that carries it. `hint` is the entity type the
/// property's name suggests; `key` is its canonical key.
pub fn infer(key: &str, values: &[String], lexicon: &Lexicon, allow_enum: bool) -> Inferred {
    let raw: Vec<&str> = values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect();
    let single = |t: ValueType| Inferred {
        value_type: t,
        multi: false,
        enum_values: Vec::new(),
        resolved: 0.0,
    };
    if raw.is_empty() {
        return single(ValueType::Text);
    }
    let whole: Vec<String> = raw.iter().map(|s| (*s).to_owned()).collect();
    if share(&whole, is_email) >= AGREE {
        return single(ValueType::Email);
    }
    if share(&whole, is_url) >= AGREE {
        return single(ValueType::Url);
    }
    if share(&whole, is_date) >= AGREE {
        return single(ValueType::Date);
    }
    if share(&whole, is_percent) >= AGREE {
        return single(ValueType::Percent);
    }
    if share(&whole, is_version) >= AGREE
        && (key == "version"
            || whole
                .iter()
                .all(|w| w.starts_with(['v', 'V']) || w.matches('.').count() >= 2))
    {
        return single(ValueType::Version);
    }
    let cores: Vec<Option<String>> = whole
        .iter()
        .map(|s| number_core(s).or_else(|| number_words(s)))
        .collect();
    let numeric = cores.iter().filter(|c| c.is_some()).count() as f64 / cores.len() as f64;
    if numeric >= AGREE {
        let currency = cores
            .iter()
            .filter(|c| c.as_deref().is_some_and(|c| c.starts_with("c:")))
            .count() as f64
            / cores.len() as f64;
        let money_key = matches!(key, "amount" | "budget" | "price");
        return single(if currency >= 0.5 || money_key {
            ValueType::Amount
        } else {
            ValueType::Number
        });
    }
    let distinct_whole = {
        let mut d: Vec<&String> = whole.iter().collect();
        d.sort();
        d.dedup();
        d.len()
    };
    if share(&whole, looks_like_id) >= AGREE && distinct_whole as f64 / whole.len() as f64 >= 0.9 {
        return single(ValueType::Id);
    }
    // Names of entities: the items of every value, matched against the Drive's entities and the shape of a name.
    let items: Vec<String> = whole.iter().flat_map(|v| items_of(v)).collect();
    let multi = items.len() > whole.len();
    let resolved_for = |ty: &str| -> f64 {
        if items.is_empty() {
            return 0.0;
        }
        items
            .iter()
            .filter(|i| lexicon.types_of(i).iter().any(|t| t == ty))
            .count() as f64
            / items.len() as f64
    };
    let hint = key_hint(key);
    let mut best: Option<(ValueType, f64)> = None;
    for (vt, ty) in [
        (ValueType::Person, "Person"),
        (ValueType::Project, "Project"),
        (ValueType::Organization, "Organization"),
    ] {
        // Any phrase can be an organization (a department, a category): only a field that says so names one by its values.
        if vt == ValueType::Organization && hint != Some(ValueType::Organization) {
            continue;
        }
        let r = resolved_for(ty);
        if r >= 0.6 && best.is_none_or(|(_, b)| r > b) {
            best = Some((vt, r));
        }
    }
    if let Some((vt, r)) = best {
        return Inferred {
            value_type: vt,
            multi,
            enum_values: Vec::new(),
            resolved: r,
        };
    }
    if let Some(h) = hint {
        let ok = match h {
            ValueType::Person => share(&items, name_like) >= AGREE,
            ValueType::Project => share(&items, project_like) >= AGREE,
            _ => share(&items, org_like) >= AGREE,
        };
        if ok {
            return Inferred {
                value_type: h,
                multi,
                enum_values: Vec::new(),
                resolved: 0.0,
            };
        }
    }
    // Enum: few distinct short values that repeat.
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for w in &whole {
        *counts.entry(w.as_str()).or_default() += 1;
    }
    let max_chars = whole.iter().map(|w| w.chars().count()).max().unwrap_or(0);
    // A category is a label: it has no digits (`12个月` is a quantity), it repeats, and the body of a section is prose, not a label.
    let has_digit = whole.iter().any(|w| w.chars().any(|c| c.is_ascii_digit()))
        && !whole.iter().all(|w| level_code(w));
    let repeats = counts.values().any(|n| *n >= 2);
    if allow_enum
        && !has_digit
        && repeats
        && whole.len() >= ENUM_MIN_VALUES
        && counts.len() <= ENUM_MAX_DISTINCT
        && (counts.len() as f64 / whole.len() as f64 <= ENUM_MAX_RATIO
            || whole.iter().all(|w| level_code(w)))
        && max_chars <= ENUM_MAX_CHARS
    {
        let mut by: Vec<(&str, usize)> = counts.into_iter().collect();
        by.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        return Inferred {
            value_type: ValueType::Enum,
            multi: false,
            enum_values: by.into_iter().map(|(v, _)| v.to_owned()).collect(),
            resolved: 0.0,
        };
    }
    let avg = whole.iter().map(|w| w.chars().count()).sum::<usize>() as f64 / whole.len() as f64;
    Inferred {
        value_type: if avg > LONG_CHARS as f64 {
            ValueType::LongText
        } else {
            ValueType::Text
        },
        multi,
        enum_values: Vec::new(),
        resolved: 0.0,
    }
}
