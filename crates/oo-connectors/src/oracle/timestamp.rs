// Reuses the existing Oracle preflight UTC-plus-offset conversion.
pub fn shift_timestamp(utc: &str, hour_offset: i64, minute_offset: i64) -> Result<String, String> {
    let (date, time) = utc.split_once('T').ok_or("tstz date")?;
    let mut date_parts = date.split('-');
    let year: i32 = date_parts
        .next()
        .ok_or("year")?
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let month: u32 = date_parts
        .next()
        .ok_or("month")?
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let day: u32 = date_parts
        .next()
        .ok_or("day")?
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    if time.len() < 8 {
        return Err("tstz time".into());
    }
    let mut hour: i64 = time[0..2]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let mut minute: i64 = time[3..5]
        .parse()
        .map_err(|err: std::num::ParseIntError| err.to_string())?;
    let rest = &time[5..];
    minute += minute_offset;
    hour += hour_offset;
    while minute < 0 {
        minute += 60;
        hour -= 1;
    }
    while minute >= 60 {
        minute -= 60;
        hour += 1;
    }
    let mut day_delta = 0_i32;
    while hour < 0 {
        hour += 24;
        day_delta -= 1;
    }
    while hour >= 24 {
        hour -= 24;
        day_delta += 1;
    }
    let (year, month, day) = shift_days(year, month, day, day_delta)?;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}{rest}"
    ))
}

fn shift_days(
    mut year: i32,
    mut month: u32,
    day: u32,
    delta: i32,
) -> Result<(i32, u32, u32), String> {
    if !(1..=12).contains(&month) || day == 0 {
        return Err("civil date".into());
    }
    let mut day = day as i32 + delta;
    loop {
        let width = days_in_month(year, month) as i32;
        if (1..=width).contains(&day) {
            return Ok((year, month, day as u32));
        }
        if day < 1 {
            month = if month == 1 { 12 } else { month - 1 };
            if month == 12 {
                year -= 1;
            }
            day += days_in_month(year, month) as i32;
        } else {
            day -= width;
            month = if month == 12 { 1 } else { month + 1 };
            if month == 1 {
                year += 1;
            }
        }
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offset_rollover_and_negative_zero_hour() {
        assert_eq!(
            shift_timestamp("2026-10-27T01:32:03.987654321", 0, -30).unwrap(),
            "2026-10-27T01:02:03.987654321"
        );
        assert_eq!(
            shift_timestamp("2028-03-01T00:01:00.000000001", -1, 0).unwrap(),
            "2028-02-29T23:01:00.000000001"
        );
    }
}
