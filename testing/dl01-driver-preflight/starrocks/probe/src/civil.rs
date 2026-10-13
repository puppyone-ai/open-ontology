use crate::fail::Fail;

pub fn ymd_to_days(year: i32, month: u32, day: u32) -> Result<i32, Fail> {
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month)? {
        return Err(Fail::new("date", format!("{year:04}-{month:02}-{day:02}")));
    }
    let mut year_adj = year;
    if month <= 2 {
        year_adj -= 1;
    }
    let era = if year_adj >= 0 {
        year_adj
    } else {
        year_adj - 399
    } / 400;
    let yoe = u32::try_from(year_adj - era * 400).map_err(|_| Fail::new("date", "year"))?;
    let month_part = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month_part + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + i32::try_from(doe).map_err(|_| Fail::new("date", "range"))? - 719468;
    Ok(days)
}

pub fn days_to_ymd(days: i32) -> Result<(i32, u32, u32), Fail> {
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = u64::try_from(z - era * 146097).map_err(|_| Fail::new("date", "range"))?;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = i32::try_from(yoe as i64 + era * 400).map_err(|_| Fail::new("date", "year"))?;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_part = (5 * doy + 2) / 153;
    let day = doy - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    if month <= 2 {
        year += 1;
    }
    Ok((
        year,
        u32::try_from(month).map_err(|_| Fail::new("date", "month"))?,
        u32::try_from(day).map_err(|_| Fail::new("date", "day"))?,
    ))
}

pub fn naive_seconds(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Result<i64, Fail> {
    if hour > 23 || minute > 59 || second > 59 {
        return Err(Fail::new(
            "datetime",
            format!("{hour:02}:{minute:02}:{second:02}"),
        ));
    }
    let days = i64::from(ymd_to_days(year, month, day)?);
    Ok(days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
}

pub fn format_date(days: i32) -> Result<String, Fail> {
    let (year, month, day) = days_to_ymd(days)?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

pub fn format_naive(seconds: i64) -> Result<String, Fail> {
    let days = seconds.div_euclid(86_400);
    let tod = seconds.rem_euclid(86_400);
    let hour = tod / 3_600;
    let minute = (tod % 3_600) / 60;
    let second = tod % 60;
    let date = format_date(i32::try_from(days).map_err(|_| Fail::new("datetime", "range"))?)?;
    Ok(format!("{date} {hour:02}:{minute:02}:{second:02}"))
}

fn days_in_month(year: i32, month: u32) -> Result<u32, Fail> {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    Ok(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(Fail::new("date", month.to_string())),
    })
}

#[cfg(test)]
mod tests {
    use super::{days_to_ymd, format_naive, naive_seconds, ymd_to_days};

    #[test]
    fn unix_epoch_and_leap_day_roundtrip() {
        assert_eq!(ymd_to_days(1970, 1, 1).unwrap(), 0);
        assert_eq!(days_to_ymd(0).unwrap(), (1970, 1, 1));
        let leap = ymd_to_days(2024, 2, 29).unwrap();
        assert_eq!(days_to_ymd(leap).unwrap(), (2024, 2, 29));
        assert!(ymd_to_days(2025, 2, 29).is_err());
    }

    #[test]
    fn naive_datetime_has_no_offset() {
        let seconds = naive_seconds(2026, 5, 6, 1, 2, 3).unwrap();
        assert_eq!(format_naive(seconds).unwrap(), "2026-05-06 01:02:03");
    }
}
