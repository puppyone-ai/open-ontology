//! RFC 3339 timestamps with a fixed offset spelling.
//!
//! `Z`, `z`, `+00:00`, and `-00:00` are stored as `Z`. Any other offset keeps
//! its `±HH:MM` or `±HH:MM:SS` spelling. The value is not collapsed onto `Z`.
//! Fractional seconds are microseconds: a shorter fraction is padded, a
//! non-zero nanosecond is rejected, and a leap second is rejected. The UTC
//! second must sit inside the canonical timestamptz bounds.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::canonical::validate_date;
use crate::limits::{TIMESTAMPTZ_MAX_SECONDS, TIMESTAMPTZ_MIN_SECONDS};
use crate::schema_util::schema_object;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rfc3339Timestamp(String);

impl Rfc3339Timestamp {
    pub fn new(value: impl AsRef<str>) -> Result<Self, String> {
        let normalized =
            normalize(value.as_ref()).map_err(|()| "invalid Rfc3339Timestamp".to_owned())?;
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Rfc3339Timestamp {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Rfc3339Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for Rfc3339Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Rfc3339Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Rfc3339Timestamp {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Rfc3339Timestamp")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schema_object(serde_json::json!({
            "type": "string",
            "format": "date-time",
            "minLength": 20,
            "maxLength": 35,
            "description": "RFC 3339 with offset. Z and zero offsets normalize to Z. Other offsets keep their spelling. Rust also enforces a real Gregorian date, microsecond precision, and the timestamptz second bounds. JSON Schema format date-time is not that whole check."
        }))
    }
}

impl PartialSchema for Rfc3339Timestamp {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .format(Some(utoipa::openapi::schema::SchemaFormat::KnownFormat(
                utoipa::openapi::schema::KnownFormat::DateTime,
            )))
            .min_length(Some(20))
            .max_length(Some(35))
            .into()
    }
}

impl ToSchema for Rfc3339Timestamp {}

fn normalize(input: &str) -> Result<String, ()> {
    let bytes = input.as_bytes();
    if bytes.len() < 20 || bytes.len() > 40 || !input.is_ascii() {
        return Err(());
    }
    if bytes.get(10).copied() != Some(b'T') && bytes.get(10).copied() != Some(b't') {
        return Err(());
    }
    let year = parse_n(&bytes[0..4])?;
    let month = parse_n(&bytes[5..7])?;
    let day = parse_n(&bytes[8..10])?;
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(());
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return Err(());
    }
    let hour = parse_n(&bytes[11..13])?;
    let minute = parse_n(&bytes[14..16])?;
    let second = parse_n(&bytes[17..19])?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err(());
    }
    let (micros, offset_at) = if bytes.get(19) == Some(&b'.') {
        let rest = &bytes[20..];
        let frac_len = rest
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(rest.len());
        if !(1..=6).contains(&frac_len) {
            return Err(());
        }
        let mut padded = [b'0'; 6];
        padded[..frac_len].copy_from_slice(&rest[..frac_len]);
        let micros = parse_n(&padded)?;
        (micros, 20 + frac_len)
    } else {
        (0u32, 19)
    };
    let offset = &input[offset_at..];
    let offset_seconds = parse_offset(offset)?;
    let year_i = i32::try_from(year).map_err(|_| ())?;
    let month_u = u8::try_from(month).map_err(|_| ())?;
    let day_u = u8::try_from(day).map_err(|_| ())?;
    validate_date(year_i, month_u, day_u).map_err(|_| ())?;
    let days = civil_days(year_i, month_u, day_u)?;
    let local = days
        .checked_mul(86_400)
        .and_then(|day_seconds| {
            day_seconds
                .checked_add(i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))
        })
        .ok_or(())?;
    let utc = local.checked_sub(i64::from(offset_seconds)).ok_or(())?;
    if !(TIMESTAMPTZ_MIN_SECONDS..=TIMESTAMPTZ_MAX_SECONDS).contains(&utc) {
        return Err(());
    }
    let mut out = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    if micros != 0 {
        let mut frac = format!("{micros:06}");
        while frac.ends_with('0') {
            frac.pop();
        }
        out.push('.');
        out.push_str(&frac);
    }
    if offset_seconds == 0 {
        out.push('Z');
    } else {
        out.push_str(&format_offset(offset_seconds));
    }
    Ok(out)
}

fn parse_offset(text: &str) -> Result<i32, ()> {
    if text == "Z" || text == "z" {
        return Ok(0);
    }
    let bytes = text.as_bytes();
    if bytes.len() != 6 && bytes.len() != 9 {
        return Err(());
    }
    let sign = match bytes[0] {
        b'+' => 1i32,
        b'-' => -1,
        _ => return Err(()),
    };
    if bytes[3] != b':' || !bytes[1].is_ascii_digit() || !bytes[2].is_ascii_digit() {
        return Err(());
    }
    let hour = parse_n(&bytes[1..3])?;
    let minute = parse_n(&bytes[4..6])?;
    if hour > 23 || minute > 59 {
        return Err(());
    }
    let second = if bytes.len() == 9 {
        if bytes[6] != b':' {
            return Err(());
        }
        let second = parse_n(&bytes[7..9])?;
        if second > 59 {
            return Err(());
        }
        second
    } else {
        0
    };
    if second == 0 && bytes.len() == 9 {
        return Err(());
    }
    Ok(sign * (hour as i32 * 3600 + minute as i32 * 60 + second as i32))
}

fn format_offset(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let abs = seconds.unsigned_abs();
    let hour = abs / 3600;
    let minute = (abs % 3600) / 60;
    let second = abs % 60;
    if second == 0 {
        format!("{sign}{hour:02}:{minute:02}")
    } else {
        format!("{sign}{hour:02}:{minute:02}:{second:02}")
    }
}

fn parse_n(bytes: &[u8]) -> Result<u32, ()> {
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    text.parse().map_err(|_| ())
}

fn civil_days(year: i32, month: u8, day: u8) -> Result<i64, ()> {
    let year = i64::from(year);
    let month = u32::from(month);
    let day = u32::from(day);
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = u64::try_from(y - era * 400).map_err(|_| ())?;
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + u64::from(doy);
    Ok(era * 146_097 + i64::try_from(doe).map_err(|_| ())? - 719_468)
}
