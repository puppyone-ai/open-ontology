use crate::fail::{self, Fail, INVALID_TIME};

const DAY_NS: i128 = 86_400 * 1_000_000_000;
pub const SEVEN_NS: i128 = 7 * DAY_NS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub nanos: i128,
}

pub fn parse_rfc3339(value: &str) -> Result<Stamp, Fail> {
    let (body, zone) = if let Some(rest) = value.strip_suffix('Z').or_else(|| value.strip_suffix('z')) {
        (rest, true)
    } else if let Some(rest) = value.strip_suffix("+00:00") {
        (rest, true)
    } else {
        (value, false)
    };
    if !zone || body.len() < 19 {
        return Err(bad());
    }
    let (date, time) = body.split_once('T').or_else(|| body.split_once('t')).ok_or_else(bad)?;
    let mut dates = date.split('-');
    let year: i32 = dates.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let month: u32 = dates.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let day: u32 = dates.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    if dates.next().is_some() || !valid_ymd(year, month, day) {
        return Err(bad());
    }
    let (clock, frac) = match time.split_once('.') {
        Some((clock, frac)) => (clock, frac),
        None => (time, ""),
    };
    if clock.len() != 8 {
        return Err(bad());
    }
    let hour: u32 = clock[0..2].parse().map_err(|_| bad())?;
    let minute: u32 = clock[3..5].parse().map_err(|_| bad())?;
    let second: u32 = clock[6..8].parse().map_err(|_| bad())?;
    if &clock[2..3] != ":" || &clock[5..6] != ":" || hour > 23 || minute > 59 || second > 59 {
        return Err(bad());
    }
    if !frac.is_empty() && (frac.len() > 9 || !frac.bytes().all(|byte| byte.is_ascii_digit())) {
        return Err(bad());
    }
    let mut nanos_frac: u32 = 0;
    let mut digits = 0u32;
    for byte in frac.bytes() {
        nanos_frac = nanos_frac * 10 + u32::from(byte - b'0');
        digits += 1;
    }
    while digits < 9 {
        nanos_frac *= 10;
        digits += 1;
    }
    let days = days_from_civil(year, month, day);
    let nanos = days * DAY_NS
        + i128::from(hour) * 3_600 * 1_000_000_000
        + i128::from(minute) * 60 * 1_000_000_000
        + i128::from(second) * 1_000_000_000
        + i128::from(nanos_frac);
    Ok(Stamp { nanos })
}

pub fn completed_days(elapsed: i128) -> i64 {
    (elapsed / DAY_NS) as i64
}

fn bad() -> Fail {
    fail::fail("invalid-time", INVALID_TIME, "timestamp is not RFC3339 UTC")
}

fn valid_ymd(year: i32, month: u32, day: u32) -> bool {
    if !(1..=12).contains(&month) || day == 0 || year < 1970 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    day <= max
}

fn days_from_civil(mut year: i32, month: u32, day: u32) -> i128 {
    year -= i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = (year - era * 400) as u32;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i128::from(era) * 146097 + i128::from(doe) - 719468
}

#[cfg(test)]
mod tests {
    use super::{completed_days, parse_rfc3339, DAY_NS, SEVEN_NS};

    #[test]
    fn unix_epoch_and_cfg_if_floor() {
        let epoch = parse_rfc3339("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(epoch.nanos, 0);
        let published = parse_rfc3339("2026-09-16T17:36:11.278420Z").unwrap();
        let adopted = parse_rfc3339("2026-10-02T11:10:37Z").unwrap();
        let elapsed = adopted.nanos - published.nanos;
        assert!(elapsed >= SEVEN_NS);
        assert_eq!(completed_days(elapsed), 15);
        assert_eq!(completed_days(SEVEN_NS), 7);
        assert!(SEVEN_NS - 1 < SEVEN_NS);
        assert_eq!(completed_days(DAY_NS - 1), 0);
    }

    #[test]
    fn rejects_date_only_and_nonzero_offset() {
        assert!(parse_rfc3339("2026-09-10").is_err());
        assert!(parse_rfc3339("2026-09-10T00:00:00+01:00").is_err());
        assert!(parse_rfc3339("2026-02-31T00:00:00Z").is_err());
    }
}
