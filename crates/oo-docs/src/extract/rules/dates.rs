//! Date recognition for the rules (10 §5.1 `name.date`, `text.date`). Pure.

/// A recognized date, always normalised to `YYYY-MM-DD`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedDate {
    pub iso: String,
    /// `day`, `month` or `year`.
    pub precision: &'static str,
}

fn leap(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn valid_ymd(y: u32, m: u32, d: u32) -> bool {
    if !(1900..=2200).contains(&y) || !(1..=12).contains(&m) || d == 0 {
        return false;
    }
    let max = match m {
        2 if leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    d <= max
}

fn day(y: u32, m: u32, d: u32) -> Option<ParsedDate> {
    valid_ymd(y, m, d).then(|| ParsedDate {
        iso: format!("{y:04}-{m:02}-{d:02}"),
        precision: "day",
    })
}

fn month(y: u32, m: u32) -> Option<ParsedDate> {
    valid_ymd(y, m, 1).then(|| ParsedDate {
        iso: format!("{y:04}-{m:02}-01"),
        precision: "month",
    })
}

fn digits(c: &[char], from: usize, min: usize, max: usize) -> Option<(u32, usize)> {
    let mut n = 0usize;
    let mut v = 0u32;
    while from + n < c.len() && n < max && c[from + n].is_ascii_digit() {
        v = v * 10 + c[from + n].to_digit(10)?;
        n += 1;
    }
    (n >= min).then_some((v, from + n))
}

/// Try to read one date at `i` (which must start a digit run). Returns the
/// date and the index after it.
fn at(c: &[char], i: usize, partial: bool) -> Option<(ParsedDate, usize)> {
    let (y, p) = digits(c, i, 4, 4)?;
    let sep = c.get(p).copied();
    match sep {
        Some('-' | '/' | '.') => {
            let (m, q) = digits(c, p + 1, 1, 2)?;
            if c.get(q) == Some(&sep?) {
                if let Some((d, r)) = digits(c, q + 1, 1, 2)
                    && !c.get(r).is_some_and(char::is_ascii_digit)
                {
                    return day(y, m, d).map(|x| (x, r));
                }
                // `2026-04-周报`: only the month is a date.
                return if partial {
                    month(y, m).map(|x| (x, q))
                } else {
                    None
                };
            }
            if partial && !c.get(q).is_some_and(char::is_ascii_digit) {
                return month(y, m).map(|x| (x, q));
            }
            None
        }
        Some('年') => {
            // `2026年Q1`
            if matches!(c.get(p + 1), Some('Q' | 'q')) {
                let q = c.get(p + 2)?.to_digit(10)?;
                if !(1..=4).contains(&q) || c.get(p + 3).is_some_and(char::is_ascii_digit) {
                    return None;
                }
                return month(y, (q - 1) * 3 + 1).map(|x| (x, p + 3));
            }
            let (m, q) = digits(c, p + 1, 1, 2)?;
            if c.get(q) != Some(&'月') {
                return None;
            }
            if let Some((d, r)) = digits(c, q + 1, 1, 2)
                && matches!(c.get(r), Some('日' | '号'))
            {
                return day(y, m, d).map(|x| (x, r + 1));
            }
            if partial {
                return month(y, m).map(|x| (x, q + 1));
            }
            None
        }
        Some('Q' | 'q') => {
            let q = c.get(p + 1)?.to_digit(10)?;
            if !(1..=4).contains(&q) || c.get(p + 2).is_some_and(char::is_ascii_digit) {
                return None;
            }
            month(y, (q - 1) * 3 + 1).map(|x| (x, p + 2))
        }
        _ => {
            // YYYYMMDD
            let (m, q) = digits(c, p, 2, 2)?;
            let (d, r) = digits(c, q, 2, 2)?;
            if c.get(r).is_some_and(char::is_ascii_digit) {
                return None;
            }
            day(y, m, d).map(|x| (x, r))
        }
    }
}

/// Scan `text` for dates, left to right, non-overlapping. With `partial`,
/// `YYYY-MM` and `YYYY年M月` are accepted too (month precision). Returns the
/// date and the char range it covers.
pub fn scan(text: &str, partial: bool) -> Vec<(ParsedDate, usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_digit()) {
            if let Some((date, end)) = at(&chars, i, partial) {
                out.push((date, i, end));
                i = end;
                continue;
            }
            // Skip the whole digit run so "12026-01-01" is not read from inside.
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out
}

/// `M月D日` (also `号`) without a year: the year is taken from `year` (the
/// document date, 10 §5.1 `text.date`). Dates preceded by `年` or a digit are
/// not read (they belong to a full date). Returns the date and char range.
pub fn scan_month_day(text: &str, year: u32) -> Vec<(ParsedDate, usize, usize)> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let starts = c[i].is_ascii_digit()
            && (i == 0 || !(c[i - 1].is_ascii_digit() || c[i - 1] == '年' || c[i - 1] == '-'));
        if starts
            && let Some((m, p)) = digits(&c, i, 1, 2)
            && c.get(p) == Some(&'月')
            && let Some((d, q)) = digits(&c, p + 1, 1, 2)
            && matches!(c.get(q), Some('日' | '号'))
            && let Some(date) = day(year, m, d)
        {
            out.push((date, i, q + 1));
            i = q + 1;
            continue;
        }
        i += 1;
    }
    out
}

/// A whole string that is one date (`2026-04-02`, `2026年4月2日`, `2026-04`,
/// `2026`).
pub fn parse_exact(text: &str) -> Option<ParsedDate> {
    let t = text.trim();
    let chars: Vec<char> = t.chars().collect();
    if let Some((date, end)) = at(&chars, 0, true)
        && end == chars.len()
    {
        return Some(date);
    }
    if chars.len() == 4
        && chars.iter().all(char::is_ascii_digit)
        && let Ok(y) = t.parse::<u32>()
        && valid_ymd(y, 1, 1)
    {
        return Some(ParsedDate {
            iso: format!("{y:04}-01-01"),
            precision: "year",
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(s: &str, partial: bool) -> Option<(String, &'static str)> {
        scan(s, partial)
            .into_iter()
            .next()
            .map(|(d, _, _)| (d.iso, d.precision))
    }

    #[test]
    fn spec_forms() {
        assert_eq!(
            first("2026-03-05-周会", false),
            Some(("2026-03-05".into(), "day"))
        );
        assert_eq!(
            first("记录20260305", false),
            Some(("2026-03-05".into(), "day"))
        );
        assert_eq!(
            first("2026年3月5日纪要", false),
            Some(("2026-03-05".into(), "day"))
        );
        assert_eq!(
            first("2026Q1规划", false),
            Some(("2026-01-01".into(), "month"))
        );
        assert_eq!(first("2026Q3", false), Some(("2026-07-01".into(), "month")));
    }

    #[test]
    fn partial_forms_only_when_asked() {
        assert_eq!(first("D-2026-04-周报", false), None);
        assert_eq!(
            first("D-2026-04-周报", true),
            Some(("2026-04-01".into(), "month"))
        );
        assert_eq!(
            first("2026年5月起", true),
            Some(("2026-05-01".into(), "month"))
        );
        assert_eq!(first("2026年5月起", false), None);
    }

    #[test]
    fn rejects_invalid_and_embedded() {
        assert_eq!(first("2026-02-30", false), None);
        assert_eq!(first("2026-13-01", false), None);
        assert_eq!(first("12026-01-01", false), None);
        assert_eq!(first("2026-01-011", false), None);
        assert_eq!(first("20261301", false), None);
        assert_eq!(first("2026Q5", false), None);
        assert_eq!(
            first("2024-02-29", false),
            Some(("2024-02-29".into(), "day"))
        );
        assert_eq!(first("2025-02-29", false), None);
    }

    #[test]
    fn scan_finds_ranges_in_order() {
        let v = scan("2026-01-01 至 2026-05-31 由陈子墨负责", false);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0.iso, "2026-01-01");
        assert_eq!(v[1].0.iso, "2026-05-31");
        assert!(v[0].2 <= v[1].1);
    }

    #[test]
    fn exact_forms() {
        assert_eq!(parse_exact(" 2026-04-02 ").unwrap().iso, "2026-04-02");
        assert_eq!(parse_exact("2026年4月2日").unwrap().precision, "day");
        assert_eq!(parse_exact("2026-04").unwrap().precision, "month");
        assert_eq!(parse_exact("2026").unwrap().precision, "year");
        assert_eq!(parse_exact("2026-04-02 起"), None);
        assert_eq!(parse_exact("x"), None);
    }
}
