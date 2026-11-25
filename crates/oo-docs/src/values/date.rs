//! Dates in words: Chinese and English absolute dates, relative dates against the date of the document, and the year
//! a text leaves out. Pure.
//!
//! A date is a value only when it is unambiguous. A missing year is taken from the document date when the nearest
//! reading is within [`NEAREST_DAYS`]; `next Tuesday` is read as the Tuesday of next week only when that cannot also
//! mean a day later this week; a bare weekday (`周二`, `on Tuesday`) is never resolved. Otherwise the text is not a
//! date here: it stays text and does not link.

use super::{en, zh};

/// A year-less date is taken from the document when the nearest reading is at most this many days away.
pub const NEAREST_DAYS: i64 = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ymd {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

fn leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        2 if leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl Ymd {
    pub fn new(y: i32, m: u32, d: u32) -> Option<Ymd> {
        ((1900..=2200).contains(&y) && (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m))
            .then_some(Ymd { y, m, d })
    }

    /// `YYYY-MM-DD`.
    pub fn parse_iso(s: &str) -> Option<Ymd> {
        let b = s.trim().as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let n = |r: std::ops::Range<usize>| std::str::from_utf8(&b[r]).ok()?.parse::<u32>().ok();
        Ymd::new(n(0..4)? as i32, n(5..7)?, n(8..10)?)
    }

    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    /// Days since 1970-01-01.
    pub fn days(&self) -> i64 {
        let y = i64::from(self.y) - i64::from(self.m <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (i64::from(self.m) + 9) % 12;
        let doy = (153 * mp + 2) / 5 + i64::from(self.d) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Option<Ymd> {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        Ymd::new((y + i64::from(m <= 2)) as i32, m, d)
    }

    pub fn plus_days(&self, n: i64) -> Option<Ymd> {
        Ymd::from_days(self.days() + n)
    }

    /// 0 = Monday ... 6 = Sunday.
    pub fn weekday(&self) -> u32 {
        (self.days() + 3).rem_euclid(7) as u32
    }
}

/// How a date was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// Year, month and day all written.
    Explicit,
    /// The year was taken from the document date.
    YearInferred,
    /// Computed from the document date (`下周二`, `tomorrow`, `3 days ago`).
    Relative,
}

impl How {
    pub fn as_str(self) -> &'static str {
        match self {
            How::Explicit => "explicit",
            How::YearInferred => "year_from_document",
            How::Relative => "relative_to_document",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormDate {
    pub ymd: Ymd,
    pub how: How,
    /// `today`, `今天`: relative but not worth linking (every document says it).
    pub trivial: bool,
}

impl NormDate {
    pub fn iso(&self) -> String {
        self.ymd.iso()
    }
}

/// The date the text is read against: the date of the document, or of the meeting it records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DateCtx {
    pub today: Option<Ymd>,
}

impl DateCtx {
    pub fn on(iso: &str) -> DateCtx {
        DateCtx {
            today: Ymd::parse_iso(iso),
        }
    }
    pub const NONE: DateCtx = DateCtx { today: None };
}

fn explicit(y: i32, m: u32, d: u32) -> Option<NormDate> {
    Some(NormDate {
        ymd: Ymd::new(y, m, d)?,
        how: How::Explicit,
        trivial: false,
    })
}

/// The year of a month and day the text leaves without one.
pub fn infer_year(ctx: &DateCtx, m: u32, d: u32) -> Option<NormDate> {
    let today = ctx.today?;
    let mut best: Option<(i64, Ymd)> = None;
    for y in today.y - 1..=today.y + 1 {
        if let Some(c) = Ymd::new(y, m, d) {
            let dist = (c.days() - today.days()).abs();
            if best.is_none_or(|(b, _)| dist < b) {
                best = Some((dist, c));
            }
        }
    }
    let (dist, ymd) = best?;
    (dist <= NEAREST_DAYS).then_some(NormDate {
        ymd,
        how: How::YearInferred,
        trivial: false,
    })
}

fn relative(ymd: Option<Ymd>, trivial: bool) -> Option<NormDate> {
    Some(NormDate {
        ymd: ymd?,
        how: How::Relative,
        trivial,
    })
}

fn week_monday(t: Ymd) -> i64 {
    t.days() - i64::from(t.weekday())
}

fn at_weekday(t: Ymd, week_offset: i64, wd: u32) -> Option<Ymd> {
    Ymd::from_days(week_monday(t) + 7 * week_offset + i64::from(wd))
}

fn add_months(t: Ymd, k: i32) -> (i32, u32) {
    let idx = t.y * 12 + (t.m as i32 - 1) + k;
    (idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u32)
}

// ------------------------------------------------------------------------------------------ reading helpers

fn digits_at(cs: &[char], i: usize, min: usize, max: usize) -> Option<(u32, usize)> {
    let mut n = 0;
    let mut v = 0u32;
    while i + n < cs.len() && n < max && cs[i + n].is_ascii_digit() {
        v = v * 10 + cs[i + n].to_digit(10)?;
        n += 1;
    }
    (n >= min).then_some((v, i + n))
}

/// A number of at most `max` that is written with digits or Chinese numerals, ending at the returned index.
fn zh_or_ascii_num(cs: &[char], i: usize, max: u32) -> Option<(u32, usize)> {
    if let Some((v, e)) = digits_at(cs, i, 1, 3) {
        return (v <= max).then_some((v, e));
    }
    let mut e = i;
    while e < cs.len() && zh::is_numeral_char(cs[e]) && zh::big_unit(cs[e]).is_none() {
        e += 1;
    }
    if e == i {
        return None;
    }
    let s: String = cs[i..e].iter().collect();
    let v = zh::parse(&s)?.to_u64()?;
    (v >= 1 && v <= u64::from(max)).then_some((v as u32, e))
}

fn zh_year(cs: &[char], i: usize) -> Option<(i32, usize)> {
    if let Some((v, e)) = digits_at(cs, i, 4, 4) {
        return Some((v as i32, e));
    }
    if cs.len() >= i + 4 && cs[i..i + 4].iter().all(|c| zh::digit(*c).is_some()) {
        let s: String = cs[i..i + 4].iter().collect();
        return Some((zh::parse(&s)?.to_u64()? as i32, i + 4));
    }
    None
}

fn prev_is_numeralish(cs: &[char], i: usize) -> bool {
    i > 0
        && (cs[i - 1].is_ascii_alphanumeric()
            || zh::is_numeral_char(cs[i - 1])
            || cs[i - 1] == '第')
}

fn zh_weekday(c: char) -> Option<u32> {
    Some(match c {
        '一' | '1' => 0,
        '二' | '2' => 1,
        '三' | '3' => 2,
        '四' | '4' => 3,
        '五' | '5' => 4,
        '六' | '6' => 5,
        '日' | '天' | '七' | '7' => 6,
        _ => return None,
    })
}

fn starts_with(cs: &[char], i: usize, s: &str) -> bool {
    let w: Vec<char> = s.chars().collect();
    cs.len() >= i + w.len() && cs[i..i + w.len()] == w[..]
}

// ------------------------------------------------------------------------------------------ Chinese

fn zh_explicit(cs: &[char], i: usize, ctx: &DateCtx) -> Option<(NormDate, usize)> {
    if prev_is_numeralish(cs, i) {
        return None;
    }
    let mut p = i;
    let mut year = None;
    if let Some((y, e)) = zh_year(cs, p)
        && cs.get(e) == Some(&'年')
    {
        year = Some(y);
        p = e + 1;
    }
    let (m, e) = zh_or_ascii_num(cs, p, 12)?;
    if cs.get(e) != Some(&'月') {
        return None;
    }
    let (d, e2) = zh_or_ascii_num(cs, e + 1, 31)?;
    let mut end = e2;
    let marked = matches!(cs.get(end), Some('日' | '号' | '號'));
    if marked {
        end += 1;
    }
    match year {
        Some(y) => {
            // `2027年3月16` without a marker is accepted for digits only.
            if !marked && !cs[e + 1..e2].iter().all(char::is_ascii_digit) {
                return None;
            }
            if !marked && cs.get(end).is_some_and(char::is_ascii_digit) {
                return None;
            }
            explicit(y, m, d).map(|n| (n, end))
        }
        None => {
            if !marked {
                return None;
            }
            infer_year(ctx, m, d).map(|n| (n, end))
        }
    }
}

fn zh_relative(cs: &[char], i: usize, ctx: &DateCtx) -> Option<(NormDate, usize)> {
    let today = ctx.today?;
    const DAYS: &[(&str, i64)] = &[
        ("大前天", -3),
        ("大后天", 3),
        ("大後天", 3),
        ("前天", -2),
        ("昨天", -1),
        ("昨日", -1),
        ("今天", 0),
        ("今日", 0),
        ("明天", 1),
        ("明日", 1),
        ("后天", 2),
        ("後天", 2),
    ];
    for (w, off) in DAYS {
        if starts_with(cs, i, w) {
            // `前天` inside `大前天`, `后天` inside `大后天`: handled by the longer word first.
            if i > 0 && cs[i - 1] == '大' {
                return None;
            }
            let n = relative(today.plus_days(*off), *off == 0)?;
            return Some((n, i + w.chars().count()));
        }
    }
    // (本|这|这个|下|下个|下下|下下个|上|上个|上上|上上个)(周|星期|礼拜)X
    const PREFIX: &[(&str, i64)] = &[
        ("下下个", 2),
        ("上上个", -2),
        ("下下", 2),
        ("上上", -2),
        ("这个", 0),
        ("下个", 1),
        ("上个", -1),
        ("本", 0),
        ("这", 0),
        ("下", 1),
        ("上", -1),
    ];
    for (pre, off) in PREFIX {
        if !starts_with(cs, i, pre) {
            continue;
        }
        let mut p = i + pre.chars().count();
        let week_len = ["星期", "礼拜", "禮拜", "周", "週"]
            .iter()
            .find(|w| starts_with(cs, p, w))
            .map(|w| w.chars().count());
        if let Some(wl) = week_len {
            p += wl;
            if let Some(wd) = cs.get(p).and_then(|c| zh_weekday(*c)) {
                // `周日` is the last day of the week; `周天` too.
                let n = relative(at_weekday(today, *off, wd), false)?;
                return Some((n, p + 1));
            }
            return None;
        }
        // (这个月|本月|下个月|下月|上个月|上月) N 号
        let month_word = [("个月", 0usize), ("月", 0usize)]
            .iter()
            .find(|(w, _)| starts_with(cs, p, w))
            .map(|(w, _)| w.chars().count());
        if let Some(ml) = month_word
            && matches!(*pre, "本" | "这个" | "这" | "下个" | "下" | "上个" | "上")
        {
            let q = p + ml;
            if let Some((d, e)) = zh_or_ascii_num(cs, q, 31)
                && matches!(cs.get(e), Some('号' | '日' | '號'))
            {
                let (y, m) = add_months(today, *off as i32);
                let n = relative(Ymd::new(y, m, d), false)?;
                return Some((n, e + 1));
            }
        }
    }
    // N天后, N周前
    if let Some((n, e)) = zh_or_ascii_num(cs, i, 999)
        && !prev_is_numeralish(cs, i)
    {
        let mut p = e;
        if cs.get(p) == Some(&'个') {
            p += 1;
        }
        let unit = [
            ("天", 1i64),
            ("日", 1),
            ("星期", 7),
            ("礼拜", 7),
            ("周", 7),
            ("週", 7),
        ]
        .iter()
        .find(|(w, _)| starts_with(cs, p, w))
        .map(|(w, k)| (w.chars().count(), *k));
        if let Some((ul, k)) = unit {
            let q = p + ul;
            let dir = [
                ("之后", 1i64),
                ("以后", 1),
                ("之後", 1),
                ("后", 1),
                ("後", 1),
                ("之前", -1),
                ("以前", -1),
                ("前", -1),
            ]
            .iter()
            .find(|(w, _)| starts_with(cs, q, w))
            .map(|(w, s)| (w.chars().count(), *s));
            if let Some((dl, s)) = dir {
                // `三天后` is a date; `三天前` too. `3周` alone is a duration.
                let n = relative(today.plus_days(s * k * i64::from(n)), false)?;
                return Some((n, q + dl));
            }
        }
    }
    None
}

// ------------------------------------------------------------------------------------------ English

fn month_of(w: &str) -> Option<u32> {
    // Capitalised only: `may` and `march` are common words.
    let mut it = w.chars();
    let first = it.next()?;
    if !first.is_uppercase() {
        return None;
    }
    Some(match w.to_lowercase().as_str() {
        "january" | "jan" => 1,
        "february" | "feb" => 2,
        "march" | "mar" => 3,
        "april" | "apr" => 4,
        "may" => 5,
        "june" | "jun" => 6,
        "july" | "jul" => 7,
        "august" | "aug" => 8,
        "september" | "sept" | "sep" => 9,
        "october" | "oct" => 10,
        "november" | "nov" => 11,
        "december" | "dec" => 12,
        _ => return None,
    })
}

fn weekday_of(w: &str) -> Option<u32> {
    Some(match w.to_lowercase().as_str() {
        "monday" | "mon" => 0,
        "tuesday" | "tue" | "tues" => 1,
        "wednesday" | "wed" => 2,
        "thursday" | "thu" | "thur" | "thurs" => 3,
        "friday" | "fri" => 4,
        "saturday" | "sat" => 5,
        "sunday" | "sun" => 6,
        _ => return None,
    })
}

const ORD_ONES: &[&str] = &[
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
    "twentieth",
];

fn ordinal_word(w: &str) -> Option<u32> {
    let l = w.to_lowercase();
    if let Some(i) = ORD_ONES.iter().position(|o| *o == l) {
        return Some(i as u32 + 1);
    }
    if l == "thirtieth" {
        return Some(30);
    }
    let (tens, ones) = l.split_once([' ', '-'])?;
    let t = match tens {
        "twenty" => 20,
        "thirty" => 30,
        _ => return None,
    };
    let o = ORD_ONES.iter().position(|x| *x == ones)? as u32 + 1;
    (o <= 9).then_some(t + o)
}

/// An alphabetic word at `i` (with a trailing `.` of an abbreviation kept out), and its end.
fn word(cs: &[char], i: usize) -> Option<(String, usize)> {
    let mut e = i;
    while e < cs.len() && cs[e].is_ascii_alphabetic() {
        e += 1;
    }
    (e > i).then(|| (cs[i..e].iter().collect(), e))
}

fn skip_ws(cs: &[char], mut i: usize) -> usize {
    while cs.get(i).is_some_and(|c| *c == ' ' || *c == '\u{a0}') {
        i += 1;
    }
    i
}

/// A day of the month: `16`, `16th`, `sixteenth`, `twenty-first`.
fn en_day(cs: &[char], i: usize) -> Option<(u32, usize)> {
    if let Some((d, e)) = digits_at(cs, i, 1, 2) {
        let mut p = e;
        if let Some((w, we)) = word(cs, p)
            && matches!(w.to_lowercase().as_str(), "st" | "nd" | "rd" | "th")
        {
            p = we;
        }
        if cs.get(p).is_some_and(|c| c.is_ascii_digit()) {
            return None;
        }
        return (1..=31).contains(&d).then_some((d, p));
    }
    let (w, e) = word(cs, i)?;
    if let Some(d) = ordinal_word(&w) {
        return Some((d, e));
    }
    // `twenty first`, `twenty-first`
    if matches!(w.to_lowercase().as_str(), "twenty" | "thirty")
        && matches!(cs.get(e), Some(' ' | '-'))
    {
        let (w2, e2) = word(cs, e + 1)?;
        let d = ordinal_word(&format!("{w} {w2}"))?;
        return Some((d, e2));
    }
    None
}

fn en_year(cs: &[char], i: usize) -> Option<(i32, usize)> {
    let (y, e) = digits_at(cs, i, 4, 4)?;
    if cs.get(e).is_some_and(|c| c.is_ascii_digit() || *c == ':') || !(1900..=2100).contains(&y) {
        return None;
    }
    Some((y as i32, e))
}

/// After a day: an optional `,` and a four-digit year.
fn year_after(cs: &[char], e: usize) -> Option<(i32, usize)> {
    let mut p = e;
    if cs.get(p) == Some(&',') {
        p += 1;
    }
    let q = skip_ws(cs, p);
    if q == p {
        return None;
    }
    en_year(cs, q)
}

fn en_explicit(cs: &[char], i: usize, ctx: &DateCtx) -> Option<(NormDate, usize)> {
    if i > 0 && (cs[i - 1].is_ascii_alphanumeric() || cs[i - 1] == '-') {
        return None;
    }
    let finish = |m: u32, d: u32, after: usize| -> Option<(NormDate, usize)> {
        if let Some((y, e)) = year_after(cs, after) {
            return explicit(y, m, d).map(|n| (n, e));
        }
        infer_year(ctx, m, d).map(|n| (n, after))
    };
    // `the 16th of March`
    let mut p = i;
    if let Some((w, e)) = word(cs, p)
        && w.eq_ignore_ascii_case("the")
    {
        let q = skip_ws(cs, e);
        if q > e {
            p = q;
        }
    }
    // Month D
    if let Some((w, e)) = word(cs, i)
        && let Some(m) = month_of(&w)
    {
        let mut q = e;
        if cs.get(q) == Some(&'.') {
            q += 1;
        }
        let q2 = skip_ws(cs, q);
        if q2 == q {
            return None;
        }
        if let Some((d, de)) = en_day(cs, q2) {
            return finish(m, d, de);
        }
        return None;
    }
    // D of Month, D Month
    if let Some((d, de)) = en_day(cs, p) {
        let mut q = skip_ws(cs, de);
        if let Some((w, e)) = word(cs, q)
            && w.eq_ignore_ascii_case("of")
        {
            q = skip_ws(cs, e);
        }
        if let Some((w, e)) = word(cs, q)
            && let Some(m) = month_of(&w)
        {
            let mut e = e;
            if cs.get(e) == Some(&'.') {
                e += 1;
            }
            return finish(m, d, e);
        }
    }
    None
}

/// A count in words or digits followed by a unit word (`three days`, `2 weeks`, `a week`): days and the end index.
fn en_span(cs: &[char], i: usize) -> Option<(i64, usize)> {
    let mut words: Vec<(String, usize, usize)> = Vec::new();
    let mut p = i;
    while words.len() < 6 {
        let q = skip_ws(cs, p);
        if let Some((v, e)) = digits_at(cs, q, 1, 3) {
            words.push((v.to_string(), q, e));
            p = e;
        } else if let Some((w, e)) = word(cs, q) {
            words.push((w.to_lowercase(), q, e));
            p = e;
        } else if cs.get(q) == Some(&'-') {
            p = q + 1;
        } else {
            break;
        }
    }
    for k in (1..words.len()).rev() {
        let unit = words[k].0.as_str();
        let per = match unit {
            "day" | "days" => 1,
            "week" | "weeks" => 7,
            _ => continue,
        };
        let count: String = words[..k]
            .iter()
            .map(|w| w.0.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let n = if count == "a" || count == "an" {
            Some(1)
        } else {
            en::parse(&count, false).and_then(|r| r.value.to_u64())
        };
        if let Some(n) = n.filter(|n| (1..=999).contains(n)) {
            return Some((per * n as i64, words[k].2));
        }
    }
    None
}

fn en_relative(cs: &[char], i: usize, ctx: &DateCtx) -> Option<(NormDate, usize)> {
    let today = ctx.today?;
    if i > 0 && (cs[i - 1].is_ascii_alphanumeric() || cs[i - 1] == '-') {
        return None;
    }
    let (w, e) = word(cs, i).unwrap_or((String::new(), i));
    let lw = w.to_lowercase();
    match lw.as_str() {
        "today" | "tonight" => return relative(Some(today), true).map(|n| (n, e)),
        "tomorrow" => return relative(today.plus_days(1), false).map(|n| (n, e)),
        "yesterday" => return relative(today.plus_days(-1), false).map(|n| (n, e)),
        "next" | "this" | "last" => {
            let q = skip_ws(cs, e);
            let (w2, e2) = word(cs, q)?;
            let wd = weekday_of(&w2)?;
            let off = match lw.as_str() {
                "next" => 1,
                "last" => -1,
                _ => 0,
            };
            // `next Tuesday` said on a Monday may mean tomorrow: not resolved.
            if lw == "next" && wd > today.weekday() && wd - today.weekday() <= 3 {
                return None;
            }
            return relative(at_weekday(today, off, wd), false).map(|n| (n, e2));
        }
        "in" => {
            let (days, end) = en_span(cs, skip_ws(cs, e))?;
            return relative(today.plus_days(days), false).map(|n| (n, end));
        }
        _ => {}
    }
    // N days ago / N weeks from now
    if let Some((days, end)) = en_span(cs, i) {
        let q = skip_ws(cs, end);
        let (w, e) = word(cs, q)?;
        match w.to_lowercase().as_str() {
            "ago" => return relative(today.plus_days(-days), false).map(|n| (n, e)),
            "from" => {
                let q2 = skip_ws(cs, e);
                let (w2, e2) = word(cs, q2)?;
                if matches!(w2.to_lowercase().as_str(), "now" | "today") {
                    return relative(today.plus_days(days), false).map(|n| (n, e2));
                }
            }
            _ => {}
        }
    }
    None
}

// ------------------------------------------------------------------------------------------ numeric forms

/// `2027-03-16`, `2027/3/16`, `2027.3.16`, `2027年3月16日` is handled in Chinese. A time of day after it makes it a log timestamp.
fn numeric(cs: &[char], i: usize) -> Option<(NormDate, usize)> {
    if i > 0 && (cs[i - 1].is_ascii_alphanumeric() || cs[i - 1] == '-' || cs[i - 1] == '/') {
        return None;
    }
    let (y, p) = digits_at(cs, i, 4, 4)?;
    let sep = *cs.get(p)?;
    if !matches!(sep, '-' | '/' | '.') {
        return None;
    }
    let (m, q) = digits_at(cs, p + 1, 1, 2)?;
    if cs.get(q) != Some(&sep) {
        return None;
    }
    let (d, e) = digits_at(cs, q + 1, 1, 2)?;
    if cs.get(e).is_some_and(char::is_ascii_digit) {
        return None;
    }
    let mut t = e;
    while matches!(cs.get(t), Some(' ' | 'T')) {
        t += 1;
    }
    if t > e && cs.get(t).is_some_and(char::is_ascii_digit) {
        let (_, te) = digits_at(cs, t, 1, 2)?;
        if cs.get(te) == Some(&':') {
            return None;
        }
    }
    explicit(y as i32, m, d).map(|n| (n, e))
}

/// A date that starts at `cs[i]`, with the index after it. Every reading is tried; the first that matches wins.
pub fn at(cs: &[char], i: usize, ctx: &DateCtx) -> Option<(NormDate, usize)> {
    let c = *cs.get(i)?;
    if c.is_ascii_digit() {
        if let Some(r) = numeric(cs, i) {
            return Some(r);
        }
        if let Some(r) = zh_explicit(cs, i, ctx) {
            return Some(r);
        }
        if let Some(r) = en_explicit(cs, i, ctx) {
            return Some(r);
        }
        return zh_relative(cs, i, ctx).or_else(|| en_relative(cs, i, ctx));
    }
    if zh::is_numeral_char(c) || c == '二' {
        return zh_explicit(cs, i, ctx).or_else(|| zh_relative(cs, i, ctx));
    }
    if c.is_ascii_alphabetic() {
        return en_explicit(cs, i, ctx).or_else(|| en_relative(cs, i, ctx));
    }
    zh_relative(cs, i, ctx)
}

/// Read a whole string as one date.
pub fn parse(s: &str, ctx: &DateCtx) -> Option<NormDate> {
    let t = s.trim().trim_end_matches(['。', '.', ',', ';', '；']);
    let cs: Vec<char> = t.chars().collect();
    let (n, e) = at(&cs, 0, ctx)?;
    (e == cs.len()).then_some(n)
}

/// Whether `s` is a date in some document context: an absolute date, or a relative one or one without a year that a
/// document would resolve (`下周二`, `tomorrow`, `3月16日`). Used to type a column of dates before any document is read.
/// Two probe days half a year apart (both Sundays) cover every month and every weekday word.
pub fn is_date_text(s: &str) -> bool {
    parse(s, &DateCtx::NONE).is_some()
        || ["2027-01-03", "2027-07-04"]
            .iter()
            .any(|d| parse(s, &DateCtx::on(d)).is_some())
}
