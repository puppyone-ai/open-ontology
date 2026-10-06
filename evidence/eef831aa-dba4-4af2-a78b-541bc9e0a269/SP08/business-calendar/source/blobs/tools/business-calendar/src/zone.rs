//! Business-zone local dates and midnights.
//!
//! A gap stays nonexistent. An overlap keeps both folds. Neither case is
//! moved onto a nearby instant.

use chrono::{DateTime, Datelike, MappedLocalTime, NaiveDate, Offset, TimeZone};
use chrono_tz::Tz;
use oo_core::rfc3339::Rfc3339Timestamp;
use oo_core::tz::{CivilLocal, LocalResolution, ZoneError, resolve_local};
use std::str::FromStr;

use crate::civil::{Ymd, days_from_civil};
use crate::fault::{Refusal, refused};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Midnight {
    Unique { rfc3339: String },
    Nonexistent,
    Ambiguous {
        fold0_rfc3339: String,
        fold1_rfc3339: String,
    },
    OutOfRange,
}

pub struct Projected {
    pub as_of: String,
    pub today: Ymd,
    pub zone: Tz,
}

pub fn project(tz_name: &str, as_of: &str) -> Result<Projected, Refusal> {
    if as_of.len() > crate::fault::AS_OF_MAX_BYTES {
        return Err(refused("malformed"));
    }
    let stamp = Rfc3339Timestamp::new(as_of).map_err(|_| refused("malformed"))?;
    let utc = utc_seconds(stamp.as_str()).ok_or_else(|| refused("malformed"))?;
    let parsed = DateTime::parse_from_rfc3339(stamp.as_str()).map_err(|_| refused("zone_disagreement"))?;
    if parsed.timestamp() != utc {
        return Err(refused("zone_disagreement"));
    }
    let zone = Tz::from_str(tz_name).map_err(|_| refused("zone_unknown"))?;
    let today = match zone.timestamp_opt(utc, 0) {
        MappedLocalTime::Single(instant) => {
            if instant.timestamp() != utc {
                return Err(refused("zone_disagreement"));
            }
            let local = instant.naive_local();
            let year = local.year();
            let month = u8::try_from(local.month()).map_err(|_| refused("date_out_of_range"))?;
            let day = u8::try_from(local.day()).map_err(|_| refused("date_out_of_range"))?;
            if !(1..=9999).contains(&year) || oo_core::canonical::validate_date(year, month, day).is_err() {
                return Err(refused("date_out_of_range"));
            }
            Ymd { year, month, day }
        }
        MappedLocalTime::None => return Err(refused("as_of_unrepresentable")),
        MappedLocalTime::Ambiguous(_, _) => return Err(refused("zone_disagreement")),
    };
    Ok(Projected {
        as_of: stamp.as_str().to_owned(),
        today,
        zone,
    })
}

pub fn midnight(tz_name: &str, zone: Tz, date: Ymd) -> Result<Midnight, Refusal> {
    let local = CivilLocal {
        year: date.year,
        month: date.month,
        day: date.day,
        hour: 0,
        minute: 0,
        second: 0,
        micros: 0,
    };
    let resolution = match resolve_local(tz_name, local) {
        Ok(resolution) => resolution,
        Err(ZoneError::OutOfRange) => return Ok(Midnight::OutOfRange),
        Err(ZoneError::UnknownZone) => return Err(refused("zone_unknown")),
        Err(ZoneError::InvalidCivil) => return Err(refused("date_out_of_range")),
        Err(ZoneError::Precision) => return Err(refused("zone_disagreement")),
    };
    let naive = NaiveDate::from_ymd_opt(date.year, u32::from(date.month), u32::from(date.day))
        .and_then(|item| item.and_hms_opt(0, 0, 0))
        .ok_or_else(|| refused("date_out_of_range"))?;
    match (resolution, zone.from_local_datetime(&naive)) {
        (LocalResolution::Unambiguous(micros), MappedLocalTime::Single(instant)) => {
            if instant.timestamp_micros() != micros {
                return Err(refused("zone_disagreement"));
            }
            let stamp = stamp_from_micros(date, micros, instant.offset().fix().local_minus_utc())?;
            Ok(Midnight::Unique { rfc3339: stamp })
        }
        (LocalResolution::Gap, MappedLocalTime::None) => Ok(Midnight::Nonexistent),
        (
            LocalResolution::Overlap { fold0_micros, fold1_micros },
            MappedLocalTime::Ambiguous(earliest, latest),
        ) => {
            if earliest.timestamp_micros() != fold0_micros || latest.timestamp_micros() != fold1_micros {
                return Err(refused("zone_disagreement"));
            }
            if fold0_micros >= fold1_micros {
                return Err(refused("zone_disagreement"));
            }
            let fold0 = stamp_from_micros(date, fold0_micros, earliest.offset().fix().local_minus_utc())?;
            let fold1 = stamp_from_micros(date, fold1_micros, latest.offset().fix().local_minus_utc())?;
            Ok(Midnight::Ambiguous {
                fold0_rfc3339: fold0,
                fold1_rfc3339: fold1,
            })
        }
        _ => Err(refused("zone_disagreement")),
    }
}

pub fn utc_of_unique(midnight: &Midnight) -> Result<Option<i64>, Refusal> {
    match midnight {
        Midnight::Unique { rfc3339 } => utc_seconds(rfc3339).map(Some).ok_or_else(|| refused("zone_disagreement")),
        _ => Ok(None),
    }
}

fn stamp_from_micros(date: Ymd, micros: i64, chrono_offset: i32) -> Result<String, Refusal> {
    if micros.rem_euclid(1_000_000) != 0 {
        return Err(refused("zone_disagreement"));
    }
    let utc = micros.div_euclid(1_000_000);
    let wall = days_from_civil(date.year, date.month, date.day)
        .and_then(|days| days.checked_mul(86_400))
        .ok_or_else(|| refused("zone_disagreement"))?;
    let offset = wall.checked_sub(utc).ok_or_else(|| refused("zone_disagreement"))?;
    let offset = i32::try_from(offset).map_err(|_| refused("zone_disagreement"))?;
    if offset != chrono_offset {
        return Err(refused("zone_disagreement"));
    }
    let text = format_stamp(date, offset).ok_or_else(|| refused("zone_disagreement"))?;
    let parsed = Rfc3339Timestamp::new(&text).map_err(|_| refused("zone_disagreement"))?;
    if parsed.as_str() != text || utc_seconds(parsed.as_str()) != Some(utc) {
        return Err(refused("zone_disagreement"));
    }
    Ok(text)
}

fn format_stamp(date: Ymd, offset: i32) -> Option<String> {
    let tail = if offset == 0 {
        "Z".to_owned()
    } else {
        format_offset(offset)?
    };
    let year = date.year;
    let month = date.month;
    let day = date.day;
    Some(format!("{year:04}-{month:02}-{day:02}T00:00:00{tail}"))
}

fn format_offset(seconds: i32) -> Option<String> {
    let sign = if seconds < 0 { '-' } else { '+' };
    let abs = seconds.unsigned_abs();
    let hour = abs / 3600;
    let minute = (abs % 3600) / 60;
    let second = abs % 60;
    if hour > 23 {
        return None;
    }
    if second == 0 {
        Some(format!("{sign}{hour:02}:{minute:02}"))
    } else {
        Some(format!("{sign}{hour:02}:{minute:02}:{second:02}"))
    }
}

pub fn utc_seconds(stamp: &str) -> Option<i64> {
    let bytes = stamp.as_bytes();
    if bytes.len() < 20 || bytes.get(10) != Some(&b'T') {
        return None;
    }
    let year = parse_n(&bytes[0..4])?;
    let month = parse_n(&bytes[5..7])?;
    let day = parse_n(&bytes[8..10])?;
    let hour = parse_n(&bytes[11..13])?;
    let minute = parse_n(&bytes[14..16])?;
    let second = parse_n(&bytes[17..19])?;
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') || bytes.get(13) != Some(&b':') || bytes.get(16) != Some(&b':') {
        return None;
    }
    let mut index = 19;
    if bytes.get(19) == Some(&b'.') {
        index = 20;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return None;
        }
    }
    let offset = parse_offset(&stamp[index..])?;
    let month = u8::try_from(month).ok()?;
    let day = u8::try_from(day).ok()?;
    let days = days_from_civil(year, month, day)?;
    let local = days.checked_mul(86_400)?.checked_add(
        i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second),
    )?;
    local.checked_sub(i64::from(offset))
}

fn parse_offset(text: &str) -> Option<i32> {
    if text == "Z" {
        return Some(0);
    }
    let bytes = text.as_bytes();
    if bytes.len() != 6 && bytes.len() != 9 {
        return None;
    }
    let sign = match bytes[0] {
        b'+' => 1i32,
        b'-' => -1,
        _ => return None,
    };
    if bytes.get(3) != Some(&b':') {
        return None;
    }
    let hour = parse_n(&bytes[1..3])?;
    let minute = parse_n(&bytes[4..6])?;
    let second = if bytes.len() == 9 {
        if bytes.get(6) != Some(&b':') {
            return None;
        }
        parse_n(&bytes[7..9])?
    } else {
        0
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(sign * (hour * 3600 + minute * 60 + second))
}

fn parse_n(bytes: &[u8]) -> Option<i32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::utc_seconds;

    #[test]
    fn handwritten_instants() {
        assert_eq!(utc_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(utc_seconds("0001-01-01T00:00:00Z"), Some(-62_135_596_800));
        assert_eq!(utc_seconds("2024-01-01T00:00:00Z"), Some(1_704_067_200));
        assert_eq!(utc_seconds("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(utc_seconds("2026-01-01T00:00:00Z"), Some(1_767_225_600));
        assert_eq!(utc_seconds("2026-04-01T00:00:00Z"), Some(1_775_001_600));
        assert_eq!(utc_seconds("2026-04-01T03:30:00Z"), Some(1_775_014_200));
        assert_eq!(utc_seconds("2026-03-08T00:00:00-05:00"), Some(1_772_946_000));
        assert_eq!(utc_seconds("2026-03-09T00:00:00-04:00"), Some(1_773_028_800));
        assert_eq!(
            utc_seconds("2026-03-09T00:00:00-04:00").unwrap()
                - utc_seconds("2026-03-08T00:00:00-05:00").unwrap(),
            82_800
        );
        assert_eq!(utc_seconds("2026-11-01T00:00:00-04:00"), Some(1_793_505_600));
        assert_eq!(utc_seconds("2026-11-02T00:00:00-05:00"), Some(1_793_595_600));
        assert_eq!(
            utc_seconds("2026-11-02T00:00:00-05:00").unwrap()
                - utc_seconds("2026-11-01T00:00:00-04:00").unwrap(),
            90_000
        );
    }
}
