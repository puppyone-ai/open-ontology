use crate::fail::Fail;

/// Days since the Unix epoch. Howard Hinnant's civil calendar, pinned in tests.
pub fn days_from_civil(year: i32, month: u32, day: u32) -> Result<i32, Fail> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return Err(Fail::new("date_range", format!("{year:04}-{month:02}-{day:02}")));
    }
    let mut y = year;
    if month <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u32;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * shifted + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe as i32 - 719468;
    Ok(days)
}

pub fn civil_from_days(days: i32) -> Result<(i32, u32, u32), Fail> {
    let z = days as i64 + 719468;
    let era = if z >= 0 { z / 146097 } else { (z - 146096) / 146097 };
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    let year = i32::try_from(y).map_err(|_| Fail::new("date_range", days.to_string()))?;
    Ok((year, m as u32, d as u32))
}

pub fn format_date(days: i32) -> Result<String, Fail> {
    let (year, month, day) = civil_from_days(days)?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

const DAY_US: i64 = 86_400_000_000;

pub fn timestamp_us(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32, micro: u32) -> Result<i64, Fail> {
    if hour > 23 || minute > 59 || second > 59 || micro >= 1_000_000 {
        return Err(Fail::new(
            "time_range",
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micro:06}"),
        ));
    }
    let days = i64::from(days_from_civil(year, month, day)?);
    days.checked_mul(DAY_US)
        .and_then(|base| base.checked_add(i64::from(hour) * 3_600_000_000))
        .and_then(|base| base.checked_add(i64::from(minute) * 60_000_000))
        .and_then(|base| base.checked_add(i64::from(second) * 1_000_000))
        .and_then(|base| base.checked_add(i64::from(micro)))
        .ok_or_else(|| Fail::new("time_range", "timestamp overflow"))
}

pub fn format_timestamp(micros: i64) -> Result<String, Fail> {
    let days = i32::try_from(micros.div_euclid(DAY_US)).map_err(|_| Fail::new("time_range", "days"))?;
    let rem = micros.rem_euclid(DAY_US);
    let hour = rem / 3_600_000_000;
    let minute = (rem % 3_600_000_000) / 60_000_000;
    let second = (rem % 60_000_000) / 1_000_000;
    let micro = rem % 1_000_000;
    Ok(format!(
        "{} {hour:02}:{minute:02}:{second:02}.{micro:06}",
        format_date(days)?
    ))
}

pub fn interval_us(negative: bool, days: u32, hour: u32, minute: u32, second: u32, micro: u32) -> Result<i64, Fail> {
    if minute > 59 || second > 59 || micro >= 1_000_000 {
        return Err(Fail::new("time_range", "interval field"));
    }
    let total_hours = u64::from(days)
        .checked_mul(24)
        .and_then(|hours| hours.checked_add(u64::from(hour)))
        .ok_or_else(|| Fail::new("time_range", "interval hours"))?;
    let mut micros = total_hours
        .checked_mul(3_600_000_000)
        .and_then(|base| base.checked_add(u64::from(minute) * 60_000_000))
        .and_then(|base| base.checked_add(u64::from(second) * 1_000_000))
        .and_then(|base| base.checked_add(u64::from(micro)))
        .ok_or_else(|| Fail::new("time_range", "interval overflow"))?;
    if micros > i64::MAX as u64 {
        return Err(Fail::new("time_range", "interval overflow"));
    }
    let mut signed = micros as i64;
    if negative {
        if micros == 0 {
            return Ok(0);
        }
        signed = signed.checked_neg().ok_or_else(|| Fail::new("time_range", "interval neg"))?;
        micros = 0;
    }
    let _ = micros;
    Ok(signed)
}

pub fn format_interval(micros: i64) -> String {
    let negative = micros < 0;
    let mut left = micros.unsigned_abs();
    let micro = left % 1_000_000;
    left /= 1_000_000;
    let second = left % 60;
    left /= 60;
    let minute = left % 60;
    let hour = left / 60;
    format!(
        "{}{hour:02}:{minute:02}:{second:02}.{micro:06}",
        if negative { "-" } else { "" }
    )
}

/// Arrow Time64 is a clock time inside one day. MariaDB TIME is an interval.
pub fn clock_us(negative: bool, days: u32, hour: u32, minute: u32, second: u32, micro: u32) -> Result<i64, Fail> {
    if negative || days != 0 || hour > 23 {
        return Err(Fail::new(
            "time_out_of_clock",
            format!("neg={negative} days={days} {hour:02}:{minute:02}:{second:02}.{micro:06}"),
        ));
    }
    interval_us(false, 0, hour, minute, second, micro)
}
