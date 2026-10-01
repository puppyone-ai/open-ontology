//! Pure IANA civil-time conversion.
//!
//! `chrono` is built with `std` only. The host zone, the clock feature, and
//! wasm bindings are not enabled. `chrono-tz` embeds a pinned tzdb; this
//! module does not read `/usr/share/zoneinfo` or the network.
//!
//! A local time resolves to one instant, a gap, or an overlap that names both
//! instants. Overlap does not pick an offset.

use chrono::{MappedLocalTime, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;

use crate::limits::{TIMESTAMPTZ_MAX_SECONDS, TIMESTAMPTZ_MIN_SECONDS};

/// IANA tzdb identifier shipped inside the pinned `chrono-tz` crate.
pub const TZDB_VERSION: &str = chrono_tz::IANA_TZDB_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalResolution {
    Unambiguous(i64),
    Gap,
    Overlap {
        fold0_micros: i64,
        fold1_micros: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneError {
    UnknownZone,
    InvalidCivil,
    OutOfRange,
    Precision,
}

/// Gregorian civil components of a local wall time, with a microsecond field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilLocal {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub micros: u32,
}

pub fn resolve_local(zone: &str, local: CivilLocal) -> Result<LocalResolution, ZoneError> {
    if local.micros > 999_999 {
        return Err(ZoneError::Precision);
    }
    if crate::canonical::validate_date(local.year, local.month, local.day).is_err()
        || local.hour > 23
        || local.minute > 59
        || local.second > 59
    {
        return Err(ZoneError::InvalidCivil);
    }
    let tz: Tz = zone.parse().map_err(|_| ZoneError::UnknownZone)?;
    let date = NaiveDate::from_ymd_opt(local.year, u32::from(local.month), u32::from(local.day))
        .ok_or(ZoneError::InvalidCivil)?;
    let time = NaiveTime::from_hms_micro_opt(
        u32::from(local.hour),
        u32::from(local.minute),
        u32::from(local.second),
        local.micros,
    )
    .ok_or(ZoneError::InvalidCivil)?;
    let naive = date.and_time(time);
    // `from_local_datetime` yields instants. `offset_from_local_datetime` yields
    // offsets only. Ambiguous is `(earliest, latest)`: fold 0 then fold 1.
    match tz.from_local_datetime(&naive) {
        MappedLocalTime::Single(instant) => {
            let micros = instant_micros(&instant)?;
            Ok(LocalResolution::Unambiguous(micros))
        }
        MappedLocalTime::Ambiguous(earliest, latest) => {
            let fold0_micros = instant_micros(&earliest)?;
            let fold1_micros = instant_micros(&latest)?;
            Ok(LocalResolution::Overlap {
                fold0_micros,
                fold1_micros,
            })
        }
        MappedLocalTime::None => Ok(LocalResolution::Gap),
    }
}

fn instant_micros(instant: &chrono::DateTime<Tz>) -> Result<i64, ZoneError> {
    let micros = instant.timestamp_micros();
    let seconds = micros.div_euclid(1_000_000);
    if (TIMESTAMPTZ_MIN_SECONDS..=TIMESTAMPTZ_MAX_SECONDS).contains(&seconds) {
        Ok(micros)
    } else {
        Err(ZoneError::OutOfRange)
    }
}
