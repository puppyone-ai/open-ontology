//! Gregorian civil arithmetic.
//!
//! Day steps follow the Hinnant serial day count used by `oo_core` RFC 3339.
//! They are civil days, not 86400-second slots.
//!
//! Quarter boundaries are business fiscal quarters: three civil months counted
//! from `fiscal_year_start_month`. That matches a calendar quarter only when
//! the fiscal month is 1, 4, 7, or 10.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ymd {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

pub fn days_from_civil(year: i32, month: u8, day: u8) -> Option<i64> {
    let year = i64::from(year);
    let month = u32::from(month);
    let day = u32::from(day);
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = u64::try_from(y - era * 400).ok()?;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + u64::from(doy);
    Some(era * 146_097 + i64::try_from(doe).ok()? - 719_468)
}

pub fn civil_from_days(z: i64) -> Option<Ymd> {
    let z = z.checked_add(719_468)?;
    if z < 0 {
        return None;
    }
    let era = z / 146_097;
    let doe = u64::try_from(z - era * 146_097).ok()?;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = i64::try_from(yoe).ok()? + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    let year = i32::try_from(year).ok()?;
    let month = u8::try_from(month).ok()?;
    let day = u8::try_from(day).ok()?;
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    Some(Ymd { year, month, day })
}

pub fn add_days(date: Ymd, delta: i64) -> Option<Ymd> {
    let days = days_from_civil(date.year, date.month, date.day)?;
    civil_from_days(days.checked_add(delta)?)
}

pub fn add_months(year: i32, month: u8, delta: i32) -> Option<(i32, u8)> {
    let shifted = (i32::from(month) - 1).checked_add(delta)?;
    let year = year.checked_add(shifted.div_euclid(12))?;
    let month0 = shifted.rem_euclid(12);
    if !(1..=9999).contains(&year) {
        return None;
    }
    Some((year, u8::try_from(month0).ok()? + 1))
}

pub fn format_date(date: Ymd) -> String {
    let year = date.year;
    let month = date.month;
    let day = date.day;
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn month_start(today: Ymd) -> Ymd {
    Ymd {
        year: today.year,
        month: today.month,
        day: 1,
    }
}

pub fn prev_month_start(today: Ymd) -> Option<Ymd> {
    let (year, month) = add_months(today.year, today.month, -1)?;
    Some(Ymd {
        year,
        month,
        day: 1,
    })
}

pub fn fiscal_year_start(today: Ymd, fiscal_month: u8) -> Option<Ymd> {
    let year = if today.month < fiscal_month {
        today.year.checked_sub(1)?
    } else {
        today.year
    };
    if !(1..=9999).contains(&year) || !(1..=12).contains(&fiscal_month) {
        return None;
    }
    Some(Ymd {
        year,
        month: fiscal_month,
        day: 1,
    })
}

pub fn quarter_start(today: Ymd, fiscal_month: u8) -> Option<Ymd> {
    let start = fiscal_year_start(today, fiscal_month)?;
    let year_delta = today.year.checked_sub(start.year)?;
    let months = year_delta
        .checked_mul(12)?
        .checked_add(i32::from(today.month))?
        .checked_sub(i32::from(start.month))?;
    if !(0..12).contains(&months) {
        return None;
    }
    let (year, month) = add_months(start.year, start.month, (months / 3) * 3)?;
    Some(Ymd {
        year,
        month,
        day: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Ymd, add_days, civil_from_days, days_from_civil, fiscal_year_start, prev_month_start,
        quarter_start,
    };

    #[test]
    fn unix_day_counts() {
        assert_eq!(days_from_civil(1970, 1, 1), Some(0));
        assert_eq!(
            days_from_civil(1, 1, 1).unwrap() * 86_400,
            -62_135_596_800
        );
        assert_eq!(days_from_civil(2024, 1, 1).unwrap() * 86_400, 1_704_067_200);
        assert_eq!(days_from_civil(2024, 2, 29).unwrap() * 86_400, 1_709_164_800);
        assert_eq!(days_from_civil(2026, 1, 1).unwrap() * 86_400, 1_767_225_600);
        assert_eq!(days_from_civil(2026, 3, 1).unwrap() * 86_400, 1_772_323_200);
        assert_eq!(days_from_civil(2026, 4, 1).unwrap() * 86_400, 1_775_001_600);
    }

    #[test]
    fn roundtrip_and_edges() {
        let samples = [
            (1, 1, 1),
            (4, 2, 29),
            (1900, 2, 28),
            (2000, 2, 29),
            (2024, 2, 29),
            (2026, 3, 15),
            (9999, 12, 31),
        ];
        for (year, month, day) in samples {
            let serial = days_from_civil(year, month, day).unwrap();
            let back = civil_from_days(serial).unwrap();
            assert_eq!((back.year, back.month, back.day), (year, month, day));
        }
        assert!(civil_from_days(days_from_civil(1, 1, 1).unwrap() - 1).is_none());
        assert!(add_days(Ymd { year: 1, month: 1, day: 1 }, -1).is_none());
        assert!(add_days(Ymd { year: 9999, month: 12, day: 31 }, 1).is_none());
        assert_eq!(
            add_days(Ymd { year: 2024, month: 3, day: 1 }, -1),
            Some(Ymd { year: 2024, month: 2, day: 29 })
        );
        assert_eq!(
            add_days(Ymd { year: 2025, month: 3, day: 1 }, -1),
            Some(Ymd { year: 2025, month: 2, day: 28 })
        );
        assert!(prev_month_start(Ymd { year: 1, month: 1, day: 15 }).is_none());
    }

    #[test]
    fn fiscal_quarters_are_not_calendar_quarters() {
        let march = Ymd { year: 2026, month: 3, day: 15 };
        assert_eq!(
            fiscal_year_start(march, 2),
            Some(Ymd { year: 2026, month: 2, day: 1 })
        );
        assert_eq!(
            quarter_start(march, 2),
            Some(Ymd { year: 2026, month: 2, day: 1 })
        );
        assert_eq!(
            fiscal_year_start(march, 4),
            Some(Ymd { year: 2025, month: 4, day: 1 })
        );
        assert_eq!(
            quarter_start(march, 4),
            Some(Ymd { year: 2026, month: 1, day: 1 })
        );
        let april = Ymd { year: 2026, month: 4, day: 1 };
        assert_eq!(
            quarter_start(april, 4),
            Some(Ymd { year: 2026, month: 4, day: 1 })
        );
    }
}
