//! Chinese numerals (`六百八十五万`, `3.5亿`, `一点五万`, `二〇二七`, `两千`, financial `壹佰贰拾`). Pure.

use super::dec::Dec;

/// A digit character: `零〇一二两三四五六七八九` and the financial forms.
pub fn digit(c: char) -> Option<u32> {
    Some(match c {
        '零' | '〇' => 0,
        '一' | '壹' => 1,
        '二' | '两' | '贰' | '貳' | '兩' => 2,
        '三' | '叁' | '參' => 3,
        '四' | '肆' => 4,
        '五' | '伍' => 5,
        '六' | '陆' | '陸' => 6,
        '七' | '柒' => 7,
        '八' | '捌' => 8,
        '九' | '玖' => 9,
        _ => return None,
    })
}

/// `十 百 千` (and the financial forms).
pub fn small_unit(c: char) -> Option<u128> {
    Some(match c {
        '十' | '拾' => 10,
        '百' | '佰' => 100,
        '千' | '仟' => 1000,
        _ => return None,
    })
}

/// `万 亿`.
pub fn big_unit(c: char) -> Option<u128> {
    match c {
        '万' | '萬' => Some(10_000),
        '亿' | '億' => Some(100_000_000),
        _ => None,
    }
}

/// A character that can be part of a Chinese numeral run.
pub fn is_numeral_char(c: char) -> bool {
    digit(c).is_some() || small_unit(c).is_some() || big_unit(c).is_some()
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    Num(Dec),
    Zero,
    Small(u128),
    Big(u128),
}

fn tokens(cs: &[char]) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_ascii_digit() {
            let s = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == ',') {
                i += 1;
            }
            if i + 1 < cs.len() && cs[i] == '.' && cs[i + 1].is_ascii_digit() {
                i += 1;
                while i < cs.len() && cs[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let t: String = cs[s..i].iter().collect();
            out.push(Tok::Num(Dec::parse_ascii(&t)?));
            continue;
        }
        if c == '点' || c == '點' {
            // Decimal: the digits that follow (Chinese or ASCII) are the fraction of the number before.
            let Some(Tok::Num(whole)) = out.last().copied() else {
                return None;
            };
            let mut j = i + 1;
            let mut frac = String::new();
            while j < cs.len() {
                if let Some(d) = digit(cs[j]) {
                    frac.push(char::from_digit(d, 10)?);
                } else if cs[j].is_ascii_digit() {
                    frac.push(cs[j]);
                } else {
                    break;
                }
                j += 1;
            }
            if frac.is_empty() || frac.len() > 12 {
                return None;
            }
            let f = Dec::new(frac.parse().ok()?, -(frac.len() as i32))?;
            out.pop();
            out.push(Tok::Num(whole.plus(f)?));
            i = j;
            continue;
        }
        if let Some(d) = digit(c) {
            out.push(if d == 0 {
                Tok::Zero
            } else {
                Tok::Num(Dec::from_u64(u64::from(d)))
            });
        } else if let Some(u) = small_unit(c) {
            out.push(Tok::Small(u));
        } else {
            out.push(Tok::Big(big_unit(c)?));
        }
        i += 1;
    }
    Some(out)
}

/// A whole numeral string to a decimal. `None` when anything in it is not part of a number.
pub fn parse(s: &str) -> Option<Dec> {
    let cs: Vec<char> = s.trim().chars().collect();
    if cs.is_empty() {
        return None;
    }
    // Digit by digit (`二〇二七`, `一三八`): only digits, no unit.
    if cs.len() >= 2 && cs.iter().all(|c| digit(*c).is_some()) {
        let t: String = cs
            .iter()
            .filter_map(|c| digit(*c).and_then(|d| char::from_digit(d, 10)))
            .collect();
        return Dec::new(t.parse().ok()?, 0);
    }
    let toks = tokens(&cs)?;
    if toks.is_empty() {
        return None;
    }
    let mut total = Dec::ZERO;
    let mut section = Dec::ZERO;
    let mut pending: Option<Dec> = None;
    // The last small unit written directly before the pending digit (`一百二` is 120, `三千五` is 3500).
    let mut spoken: Option<u128> = None;
    let mut seen_value = false;
    for (idx, t) in toks.iter().enumerate() {
        match *t {
            Tok::Num(n) => {
                if pending.is_some() {
                    return None;
                }
                pending = Some(n);
                seen_value = true;
            }
            Tok::Zero => {
                seen_value = true;
                spoken = None;
            }
            Tok::Small(u) => {
                let n = match pending.take() {
                    Some(n) => n,
                    // `十二`, and `百万`/`千万` at the start of a number.
                    None if u == 10 => Dec::from_u64(1),
                    None if idx == 0 && matches!(toks.get(1), Some(Tok::Big(_))) => {
                        Dec::from_u64(1)
                    }
                    None => return None,
                };
                section = section.plus(n.times_u128(u)?)?;
                spoken = Some(u);
                seen_value = true;
            }
            Tok::Big(u) => {
                let s = section.plus(pending.take().unwrap_or(Dec::ZERO))?;
                if s.is_zero() && total.is_zero() {
                    return None;
                }
                if u == 100_000_000 {
                    total = total.plus(s)?.times_u128(u)?;
                } else {
                    total = total.plus(s.times_u128(u)?)?;
                }
                section = Dec::ZERO;
                spoken = None;
                seen_value = true;
            }
        }
    }
    if !seen_value {
        return None;
    }
    if let (Some(p), Some(u)) = (pending, spoken)
        && u >= 100
        && matches!(toks.last(), Some(Tok::Num(_)))
        && matches!(toks.get(toks.len().wrapping_sub(2)), Some(Tok::Small(x)) if *x == u)
        && p.cmp_dec(Dec::from_u64(10)) == Some(std::cmp::Ordering::Less)
    {
        pending = Some(p.times_u128(u / 10)?);
    }
    total.plus(section)?.plus(pending.unwrap_or(Dec::ZERO))
}
