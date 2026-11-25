//! Amounts: a number with a currency and the scale it was written in (`六百八十五万元`, `$3.5m`, `RMB 1.2bn`). Pure.

use super::dec::Dec;
use super::{en, zh};

/// An amount of money (or a bare scaled quantity when no currency is written).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Money {
    /// Whole currency units (`6850000`).
    pub amount: Dec,
    /// ISO code (`CNY`, `USD`, `EUR`, `GBP`, `JPY`, `HKD`) when the text names one.
    pub currency: Option<&'static str>,
    /// The scale word that was written (`万`, `亿`, `million`, `bn`, ...).
    pub scale: Option<&'static str>,
}

impl Money {
    /// `6850000 CNY`, or `6850000` without a currency: the value of an `amount` fact.
    pub fn canonical(&self) -> String {
        match self.currency {
            Some(c) => format!("{} {c}", self.amount.to_plain()),
            None => self.amount.to_plain(),
        }
    }
}

/// Currency symbols and codes written in front of the number, with the code they mean.
pub const PREFIXES: &[(&str, &str)] = &[
    ("us$", "USD"),
    ("cn¥", "CNY"),
    ("rmb", "CNY"),
    ("cny", "CNY"),
    ("usd", "USD"),
    ("eur", "EUR"),
    ("gbp", "GBP"),
    ("jpy", "JPY"),
    ("hkd", "HKD"),
    ("¥", "CNY"),
    ("￥", "CNY"),
    ("$", "USD"),
    ("€", "EUR"),
    ("£", "GBP"),
    ("人民币", "CNY"),
    ("美元", "USD"),
];

/// Currency words written after the number (longest first).
pub const SUFFIXES: &[(&str, &str)] = &[
    ("人民币", "CNY"),
    ("元整", "CNY"),
    ("美元", "USD"),
    ("美金", "USD"),
    ("欧元", "EUR"),
    ("英镑", "GBP"),
    ("日元", "JPY"),
    ("港元", "HKD"),
    ("港币", "HKD"),
    ("dollars", "USD"),
    ("dollar", "USD"),
    ("euros", "EUR"),
    ("euro", "EUR"),
    ("pounds", "GBP"),
    ("yuan", "CNY"),
    ("rmb", "CNY"),
    ("cny", "CNY"),
    ("usd", "USD"),
    ("eur", "EUR"),
    ("gbp", "GBP"),
    ("jpy", "JPY"),
    ("hkd", "HKD"),
    ("bucks", "USD"),
    ("元", "CNY"),
    ("圆", "CNY"),
    ("块", "CNY"),
];

/// The currency written at the start of `s` (lower-cased copy compared), and its length in bytes of `s`.
pub fn prefix_of(s: &str) -> Option<(&'static str, usize)> {
    let lower = s.to_lowercase();
    PREFIXES
        .iter()
        .find(|(p, _)| lower.starts_with(p))
        .map(|(p, c)| (*c, p.len()))
}

/// The currency written at the end of `s`, and its length in bytes.
pub fn suffix_of(s: &str) -> Option<(&'static str, usize)> {
    let lower = s.to_lowercase();
    SUFFIXES
        .iter()
        .find(|(p, _)| lower.ends_with(p))
        .map(|(p, c)| (*c, p.len()))
}

fn scale_label(zh_part: &str) -> Option<&'static str> {
    if zh_part.contains('亿') || zh_part.contains('億') {
        Some("亿")
    } else if zh_part.contains('万') || zh_part.contains('萬') {
        Some("万")
    } else {
        None
    }
}

/// Read a number written as Chinese numerals, English words/scales or digits (no currency). `allow_short` admits `k/m/b`.
pub fn number(s: &str, allow_short: bool) -> Option<(Dec, Option<&'static str>)> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars().any(|c| zh::is_numeral_char(c) || c == '点') {
        let v = zh::parse(t)?;
        return Some((v, scale_label(t)));
    }
    if let Some(d) = Dec::parse_ascii(t) {
        return Some((d, None));
    }
    let r = en::parse(t, allow_short)?;
    Some((r.value, r.scale.map(|(n, _)| n)))
}

/// Read a whole string as an amount: an optional currency before or after, then a number with its scale. A bare number
/// is an amount of unknown currency only when `bare` is set (a cell under an amount column).
pub fn parse(s: &str, bare: bool) -> Option<Money> {
    let mut t = s
        .trim()
        .trim_end_matches(['。', '.', ',', ';', '；'])
        .trim();
    let mut currency = None;
    if let Some((c, n)) = prefix_of(t) {
        currency = Some(c);
        t = t[n..].trim_start();
    }
    if let Some((c, n)) = suffix_of(t) {
        // `12 m` is not `12 meters`: only after a number does a currency word end a value.
        if !t[..t.len() - n].trim().is_empty() {
            currency = currency.or(Some(c));
            t = t[..t.len() - n].trim_end();
        }
    }
    if t.is_empty() {
        return None;
    }
    let (amount, scale) = number(t, currency.is_some())?;
    if currency.is_none() && scale.is_none() && !bare {
        return None;
    }
    Some(Money {
        amount,
        currency,
        scale,
    })
}
