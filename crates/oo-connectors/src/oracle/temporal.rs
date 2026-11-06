//! Oracle civil time to exact PostgreSQL microseconds. No invented naive timezone.
use super::chunk::Result;

pub fn timestamp_precision(kind: &str) -> Result<Option<u32>> {
    if kind == "DATE" {
        return Ok(None);
    }
    let rest = kind.strip_prefix("TIMESTAMP(").ok_or("temporal_type")?;
    let (p, suffix) = rest.split_once(')').ok_or("temporal_type")?;
    if p.len() != 1 || !matches!(suffix, "" | " WITH TIME ZONE") {
        return Err("temporal_type".into());
    }
    Ok(Some(
        p.parse::<u32>()
            .ok()
            .filter(|p| *p <= 9)
            .ok_or("temporal_precision")?,
    ))
}

fn digits(value: &str) -> Result<i64> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("temporal_syntax".into());
    }
    value.parse().map_err(|_| "temporal_syntax".into())
}

pub fn micros(value: &str, kind: &str) -> Result<i64> {
    let precision = timestamp_precision(kind)?;
    let zoned = kind.ends_with(" WITH TIME ZONE");
    if !value.is_ascii() {
        return Err("temporal_syntax".into());
    }
    let (civil, offset) = if zoned {
        let (civil, zone) = value.rsplit_once(' ').ok_or("temporal_offset")?;
        if zone.len() != 6 || !matches!(&zone[0..1], "+" | "-") || &zone[3..4] != ":" {
            return Err("temporal_offset".into());
        }
        let minutes = digits(&zone[1..3])? * 60 + digits(&zone[4..6])?;
        if digits(&zone[4..6])? > 59
            || minutes
                > if zone.starts_with('-') {
                    12 * 60
                } else {
                    14 * 60
                }
        {
            return Err("temporal_offset".into());
        }
        (
            civil,
            if zone.starts_with('-') {
                -minutes
            } else {
                minutes
            },
        )
    } else {
        (value, 0)
    };
    if civil.len() < 19
        || &civil[4..5] != "-"
        || &civil[7..8] != "-"
        || &civil[10..11] != "T"
        || &civil[13..14] != ":"
        || &civil[16..17] != ":"
    {
        return Err("temporal_syntax".into());
    }
    let (year, month, day) = (
        digits(&civil[0..4])?,
        digits(&civil[5..7])?,
        digits(&civil[8..10])?,
    );
    let (hour, minute, second) = (
        digits(&civil[11..13])?,
        digits(&civil[14..16])?,
        digits(&civil[17..19])?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if !(1..=9999).contains(&year)
        || day < 1
        || day > max_day
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err("temporal_range".into());
    }
    let nanos = if civil.len() == 19 {
        0
    } else {
        if &civil[19..20] != "." || civil.len() > 29 {
            return Err("temporal_fraction".into());
        }
        let fraction = &civil[20..];
        digits(fraction)? * 10_i64.pow(9 - fraction.len() as u32)
    };
    if precision.is_none() && civil.len() != 19 {
        return Err("date_fraction".into());
    }
    if let Some(p) = precision
        && nanos % 10_i64.pow(9 - p) != 0
    {
        return Err("source_temporal_precision".into());
    }
    if nanos % 1000 != 0 {
        return Err("submicrosecond_target_contract_required".into());
    }
    // Gregorian civil-day conversion; all intermediate operations fit i64 for AD 1..9999.
    let y = year - i64::from(month <= 2);
    let era = y / 400;
    let yoe = y - era * 400;
    let shifted = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * shifted + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Ok(
        ((days * 86400 + hour * 3600 + minute * 60 + second) - offset * 60) * 1_000_000
            + nanos / 1000,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_civil_and_offset_without_rounding() {
        assert_eq!(micros("1970-01-01T00:00:00", "DATE").unwrap(), 0);
        assert_eq!(micros("1969-12-31T23:59:59", "DATE").unwrap(), -1_000_000);
        assert_eq!(
            micros(
                "1970-01-01T05:45:00.123456000 +05:45",
                "TIMESTAMP(9) WITH TIME ZONE"
            )
            .unwrap(),
            123456
        );
        assert_eq!(
            micros(
                "1969-12-31T23:30:00.000000000 -00:30",
                "TIMESTAMP(9) WITH TIME ZONE"
            )
            .unwrap(),
            0
        );
        assert_eq!(
            micros("2000-02-29T12:00:00", "DATE").unwrap(),
            951825600000000
        );
        assert!(micros("0001-01-01T00:00:00", "DATE").is_ok());
        assert!(micros("9999-12-31T23:59:59.999999", "TIMESTAMP(6)").is_ok());
    }
    #[test]
    fn invalid_and_submicrosecond_values_fail_closed() {
        for v in [
            "1900-02-29T00:00:00",
            "2026-02-30T00:00:00",
            "0000-01-01T00:00:00",
            "2026-01-01T24:00:00",
        ] {
            assert!(micros(v, "DATE").is_err());
        }
        assert_eq!(
            micros("2026-10-27T01:02:03.123456789", "TIMESTAMP(9)").unwrap_err(),
            "submicrosecond_target_contract_required"
        );
        assert!(micros("2026-10-27T01:02:03.1", "TIMESTAMP(0)").is_err());
        assert!(
            micros(
                "2026-10-27T01:02:03.000000 +14:01",
                "TIMESTAMP(6) WITH TIME ZONE"
            )
            .is_err()
        );
        assert!(timestamp_precision("TIMESTAMP(6) WITH LOCAL TIME ZONE").is_err());
    }
}
