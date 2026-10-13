use crate::fail::Fail;

/// Howard Hinnant days_from_civil. Unix day 0 is 1970-01-01.
pub fn unix_days(year: i32, month: u32, day: u32) -> Result<i32, Fail> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(Fail::new("date", format!("{year:04}-{month:02}-{day:02}")));
    }
    let mut year = year;
    if month <= 2 {
        year -= 1;
    }
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = (year - era * 400) as u32;
    let month_part = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_part + 2) / 5 + day - 1;
    let day_of_era = yoe * 365 + yoe / 4 - yoe / 100 + day_of_year;
    Ok(era * 146097 + day_of_era as i32 - 719468)
}

pub fn parse_date(text: &str) -> Result<i32, Fail> {
    let text = text.trim();
    let date = text.split([' ', 'T']).next().unwrap_or(text);
    let mut parts = date.split('-');
    let year = parts
        .next()
        .ok_or_else(|| Fail::new("date", text.to_string()))?
        .parse::<i32>()
        .map_err(|_| Fail::new("date", text.to_string()))?;
    let month = parts
        .next()
        .ok_or_else(|| Fail::new("date", text.to_string()))?
        .parse::<u32>()
        .map_err(|_| Fail::new("date", text.to_string()))?;
    let day = parts
        .next()
        .ok_or_else(|| Fail::new("date", text.to_string()))?
        .parse::<u32>()
        .map_err(|_| Fail::new("date", text.to_string()))?;
    if parts.next().is_some() {
        return Err(Fail::new("date", text.to_string()));
    }
    unix_days(year, month, day)
}

#[cfg(test)]
mod tests {
    use super::unix_days;

    #[test]
    fn unix_epoch_and_fixture_days() {
        assert_eq!(unix_days(1970, 1, 1).unwrap(), 0);
        assert_eq!(unix_days(2026, 3, 1).unwrap(), 20513);
        assert_eq!(unix_days(2026, 3, 11).unwrap(), 20523);
        assert_eq!(unix_days(2026, 4, 1).unwrap(), 20544);
        assert_eq!(unix_days(2026, 4, 4).unwrap(), 20547);
        assert_eq!(unix_days(2026, 4, 18).unwrap(), 20561);
        assert_eq!(unix_days(2026, 5, 1).unwrap(), 20574);
    }
}
