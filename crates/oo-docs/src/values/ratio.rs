//! Percentages and ratios (`12%`, `百分之十二点五`, `twelve percent`, `三成五`, `3:1`). Pure.

use super::dec::Dec;
use super::{en, zh};

/// A share: `0.12` is 12 percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub fraction: Dec,
}

impl Ratio {
    pub fn from_percent(p: Dec) -> Option<Ratio> {
        Some(Ratio {
            fraction: p.pow10(-2)?,
        })
    }

    pub fn percent(&self) -> Dec {
        self.fraction.pow10(2).unwrap_or(Dec::ZERO)
    }

    /// `0.12`: the value of a `ratio` fact.
    pub fn canonical(&self) -> String {
        self.fraction.to_plain()
    }

    /// `12%`: the name of the entity that two files that state it share.
    pub fn name(&self) -> String {
        format!("{}%", self.percent().to_plain())
    }
}

fn number_of(s: &str) -> Option<Dec> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars().any(zh::is_numeral_char) {
        return zh::parse(t);
    }
    if let Some(d) = Dec::parse_ascii(t) {
        return Some(d);
    }
    en::parse(t, false).map(|r| r.value)
}

/// `百分之十二` and `百分之 12`: the number after `百分之` (`百分之百` is 100 percent).
fn after_baifenzhi(rest: &str) -> Option<Dec> {
    let rest = rest.trim();
    if rest == "百" {
        return Some(Dec::from_u64(100));
    }
    number_of(rest)
}

/// `三成`, `三成五`, `七成半`: tenths.
fn cheng(s: &str) -> Option<Dec> {
    let (head, tail) = s.split_once('成')?;
    let n = zh::parse(head).or_else(|| Dec::parse_ascii(head))?;
    let mut tenths = n;
    let tail = tail.trim();
    if tail == "半" {
        tenths = tenths.plus(Dec::new(5, -1)?)?;
    } else if !tail.is_empty() {
        let d = zh::parse(tail).or_else(|| Dec::parse_ascii(tail))?;
        if d.cmp_dec(Dec::from_u64(10)) != Some(std::cmp::Ordering::Less) {
            return None;
        }
        tenths = tenths.plus(d.pow10(-1)?)?;
    }
    if tenths.cmp_dec(Dec::from_u64(10)) == Some(std::cmp::Ordering::Greater) {
        return None;
    }
    tenths.pow10(1)
}

/// Read a whole string as a percentage.
pub fn parse(s: &str) -> Option<Ratio> {
    let t = s
        .trim()
        .trim_end_matches(['。', '.', ',', ';', '；'])
        .trim();
    if let Some(rest) = t.strip_prefix("百分之") {
        return Ratio::from_percent(after_baifenzhi(rest)?);
    }
    if t.contains('成') && !t.contains(|c: char| c.is_ascii_alphabetic()) {
        return Ratio::from_percent(cheng(t)?);
    }
    for suf in ["%", "％", "‰"] {
        if let Some(n) = t.strip_suffix(suf) {
            let p = number_of(n)?;
            return if suf == "‰" {
                Some(Ratio {
                    fraction: p.pow10(-3)?,
                })
            } else {
                Ratio::from_percent(p)
            };
        }
    }
    let lower = t.to_lowercase();
    for suf in ["per cent", "percent", "pct", "percentage"] {
        if let Some(n) = lower.strip_suffix(suf) {
            return Ratio::from_percent(number_of(n)?);
        }
    }
    None
}

/// `3:1`, `3 : 1`, `3比1`, `3 to 1`: two non-negative numbers.
pub fn parse_pair(s: &str) -> Option<(Dec, Dec)> {
    let t = s.trim();
    for sep in [":", "：", "比", " to "] {
        if let Some((a, b)) = t.split_once(sep) {
            let (a, b) = (number_of(a)?, number_of(b)?);
            if !b.is_zero() {
                return Some((a, b));
            }
        }
    }
    None
}

/// `3:1` as the canonical text of a ratio of two parts.
pub fn pair_canonical(a: Dec, b: Dec) -> String {
    format!("{}:{}", a.to_plain(), b.to_plain())
}

/// A percentage at the start of `cs[i..]` in running text, and the index after it. `i` must start the number (or
/// `百分之`). A percentage point count (`十二个百分点`) is not a ratio.
pub fn at(cs: &[char], i: usize) -> Option<(Ratio, usize)> {
    // 百分之 + number
    if cs.get(i) == Some(&'百') && cs.get(i + 1) == Some(&'分') && cs.get(i + 2) == Some(&'之') {
        let s = i + 3;
        let mut e = s;
        while e < cs.len()
            && (cs[e].is_ascii_digit()
                || (cs[e] == '.' && cs.get(e + 1).is_some_and(char::is_ascii_digit))
                || zh::is_numeral_char(cs[e])
                || (cs[e] == '点'
                    && cs
                        .get(e + 1)
                        .is_some_and(|c| zh::digit(*c).is_some() || c.is_ascii_digit())))
        {
            e += 1;
        }
        let text: String = cs[s..e].iter().collect();
        let v = after_baifenzhi(&text)?;
        return Ratio::from_percent(v).map(|r| (r, e));
    }
    // number + % / percent
    let mut e = i;
    while e < cs.len()
        && (cs[e].is_ascii_digit()
            || (cs[e] == '.' && cs.get(e + 1).is_some_and(char::is_ascii_digit))
            || (cs[e] == ',' && cs.get(e + 1).is_some_and(char::is_ascii_digit)))
    {
        e += 1;
    }
    if e > i {
        let num: String = cs[i..e].iter().collect();
        let mut p = e;
        while cs.get(p) == Some(&' ') {
            p += 1;
        }
        if matches!(cs.get(p), Some('%' | '％')) {
            return Ratio::from_percent(Dec::parse_ascii(&num)?).map(|r| (r, p + 1));
        }
        let rest: String = cs[p..(p + 9).min(cs.len())]
            .iter()
            .collect::<String>()
            .to_lowercase();
        for w in ["per cent", "percent", "pct"] {
            if rest.starts_with(w) {
                let after = cs.get(p + w.chars().count());
                if after.is_none_or(|c| !c.is_alphabetic()) {
                    return Ratio::from_percent(Dec::parse_ascii(&num)?)
                        .map(|r| (r, p + w.chars().count()));
                }
            }
        }
    }
    None
}
