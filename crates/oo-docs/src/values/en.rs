//! English number words and scales (`six point eight five million`, `two hundred thousand`, `1.2bn`, `3.5m`). Pure.

use super::dec::Dec;

fn small(w: &str) -> Option<u64> {
    Some(match w {
        "zero" | "oh" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        _ => return None,
    })
}

fn tens(w: &str) -> Option<u64> {
    Some(match w {
        "twenty" => 20,
        "thirty" => 30,
        "forty" | "fourty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

/// A scale word or abbreviation and its multiplier; `strict` is false for the short forms (`k`, `m`, `b`) that mean
/// something else without a currency next to them.
pub fn scale(w: &str) -> Option<(u128, bool)> {
    Some(match w {
        "thousand" | "thousands" => (1_000, true),
        "million" | "millions" | "mn" => (1_000_000, true),
        "billion" | "billions" | "bn" => (1_000_000_000, true),
        "trillion" | "trillions" | "tn" => (1_000_000_000_000, true),
        "k" => (1_000, false),
        "m" => (1_000_000, false),
        "b" => (1_000_000_000, false),
        _ => return None,
    })
}

/// A word of a spelled-out number (the scanner uses it to find where a run of words starts and ends).
pub fn is_number_word(w: &str) -> bool {
    small(w).is_some()
        || tens(w).is_some()
        || matches!(
            w,
            "hundred" | "thousand" | "million" | "billion" | "trillion" | "and" | "point"
        )
}

/// Words that start a spelled-out number (not `and`, `point`).
pub fn starts_number(w: &str) -> bool {
    small(w).is_some()
        || tens(w).is_some()
        || matches!(
            w,
            "hundred" | "thousand" | "million" | "billion" | "trillion" | "a"
        )
}

/// Result of a whole-string reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Read {
    pub value: Dec,
    /// A scale word (`million`, `bn`, `thousand`) or short suffix (`k`, `m`) was written.
    pub scale: Option<(&'static str, bool)>,
}

fn scale_name(w: &str) -> &'static str {
    match w {
        "thousand" | "thousands" => "thousand",
        "million" | "millions" => "million",
        "billion" | "billions" => "billion",
        "trillion" | "trillions" => "trillion",
        "mn" => "mn",
        "bn" => "bn",
        "tn" => "tn",
        "k" => "k",
        "m" => "m",
        "b" => "b",
        _ => "",
    }
}

fn arabic(w: &str) -> Option<Dec> {
    Dec::parse_ascii(w)
}

/// Split `1.2bn`, `3.5m`, `$`-less `142k` into number and suffix.
fn split_suffix(w: &str) -> Option<(&str, &str)> {
    let i = w.find(|c: char| c.is_ascii_alphabetic())?;
    (i > 0 && w[..i].starts_with(|c: char| c.is_ascii_digit())).then(|| (&w[..i], &w[i..]))
}

/// Read a whole string as an English number: words, digits, or both with scales. `allow_short` admits `k/m/b`.
pub fn parse(s: &str, allow_short: bool) -> Option<Read> {
    let lower = s.trim().to_lowercase().replace('-', " ");
    let mut words: Vec<String> = Vec::new();
    for w in lower.split_whitespace() {
        let w = w.trim_matches(',');
        if w.is_empty() {
            continue;
        }
        if let Some((n, suf)) = split_suffix(w)
            && scale(suf).is_some()
            && arabic(n).is_some()
        {
            words.push(n.to_owned());
            words.push(suf.to_owned());
        } else {
            words.push(w.to_owned());
        }
    }
    if words.is_empty() {
        return None;
    }
    let mut total = Dec::ZERO;
    let mut cur = Dec::ZERO;
    let mut have_cur = false;
    let mut last_scale: Option<(&'static str, bool)> = None;
    let mut any = false;
    // What the last word was: 0 start/scale/hundred, 1 tens, 2 unit or teen (a unit cannot follow a unit).
    let mut last = 0u8;
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        if w == "a" || w == "an" {
            let next = words.get(i + 1).map(String::as_str).unwrap_or("");
            if matches!(
                next,
                "hundred" | "thousand" | "million" | "billion" | "trillion"
            ) {
                cur = Dec::from_u64(1);
                have_cur = true;
                i += 1;
                continue;
            }
            return None;
        }
        if w == "and" {
            if !any || i + 1 >= words.len() {
                return None;
            }
            i += 1;
            continue;
        }
        if w == "point" {
            if !have_cur && total.is_zero() {
                return None;
            }
            let mut frac = String::new();
            let mut j = i + 1;
            while j < words.len() {
                if let Some(d) = small(&words[j]).filter(|d| *d <= 9) {
                    frac.push_str(&d.to_string());
                } else if words[j].len() == 1 && words[j].as_bytes()[0].is_ascii_digit() {
                    frac.push_str(&words[j]);
                } else {
                    break;
                }
                j += 1;
            }
            if frac.is_empty() || frac.len() > 12 {
                return None;
            }
            cur = cur.plus(Dec::new(frac.parse().ok()?, -(frac.len() as i32))?)?;
            have_cur = true;
            i = j;
            continue;
        }
        if let Some(n) = small(w) {
            let ok = last == 0 || (last == 1 && (1..=9).contains(&n));
            if !ok || (n == 0 && any) {
                return None;
            }
            cur = cur.plus(Dec::from_u64(n))?;
            have_cur = true;
            last = 2;
        } else if let Some(n) = tens(w) {
            if last != 0 {
                return None;
            }
            cur = cur.plus(Dec::from_u64(n))?;
            have_cur = true;
            last = 1;
        } else if w == "hundred" {
            if !have_cur || last == 0 && cur.is_zero() {
                return None;
            }
            cur = cur.times_u128(100)?;
            last = 0;
        } else if let Some((mult, strict)) = scale(w) {
            if !strict && !allow_short {
                return None;
            }
            if !have_cur {
                return None;
            }
            total = total.plus(cur.times_u128(mult)?)?;
            cur = Dec::ZERO;
            have_cur = false;
            last = 0;
            last_scale = Some((scale_name(w), strict));
        } else {
            if have_cur {
                return None;
            }
            cur = arabic(w)?;
            have_cur = true;
        }
        any = true;
        i += 1;
    }
    if !any {
        return None;
    }
    Some(Read {
        value: total.plus(cur)?,
        scale: last_scale,
    })
}
