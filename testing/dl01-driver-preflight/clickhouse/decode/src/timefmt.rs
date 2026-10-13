use crate::fail::{fail, Gate};

/// Format a UTC unix instant in nanoseconds as `YYYY-MM-DD HH:MM:SS.nnnnnnnnn`.
/// This is integer arithmetic. It does not call a floating-point clock.
pub fn format_unix_nanos(nanos: i64) -> Gate<String> {
    if nanos < 0 {
        return Err(fail("negative unix time is outside this fixture"));
    }
    let seconds = nanos / 1_000_000_000;
    let fraction = (nanos % 1_000_000_000) as u32;
    let days = seconds / 86_400;
    let sod = (seconds % 86_400) as u32;
    let (year, month, day) = civil_from_days(days)?;
    let hour = sod / 3600;
    let minute = (sod % 3600) / 60;
    let second = sod % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{fraction:09}"
    ))
}

fn civil_from_days(days: i64) -> Gate<(i64, u32, u32)> {
    // Howard Hinnant's civil_from_days, days since 1970-01-01.
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = u64::try_from(z - era * 146097).map_err(|_| fail("day overflow"))?;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = i64::try_from(yoe).map_err(|_| fail("year overflow"))? + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    let month = u32::try_from(month).map_err(|_| fail("month overflow"))?;
    let day = u32::try_from(day).map_err(|_| fail("day of month overflow"))?;
    Ok((year, month, day))
}

#[cfg(test)]
mod tests {
    use super::format_unix_nanos;

    #[test]
    fn fixture_timestamps_match_utc_nanoseconds() {
        let samples = [
            (1_704_153_600_000_000_000, "2024-01-02 00:00:00.000000000"),
            (1_706_745_599_999_999_999, "2024-01-31 23:59:59.999999999"),
            (1_710_504_000_000_000_001, "2024-03-15 12:00:00.000000001"),
            (1_710_506_096_123_456_789, "2024-03-15 12:34:56.123456789"),
        ];
        for (nanos, text) in samples {
            assert_eq!(format_unix_nanos(nanos).unwrap(), text);
        }
    }
}
