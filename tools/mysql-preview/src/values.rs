//! Cell values for the existing COPY encoder. Decimal text is never an f64.

use oo_connectors::types::{ArrowScalar, CellPresence, CellReject, DecodedCell};
use oo_core::canonical::{classify_f64, validate_date, LogicalValue};
use oo_core::{ExactDecimal, ExactInt};
use serde_json::Value as JsonValue;

use crate::error::PreviewError;
use crate::map::StoreKind;

const DAY_MICROS: i128 = 86_400_000_000;
const MYSQL_TIME_MAX_MICROS: i128 = 838 * 3_600_000_000 + 59 * 60_000_000 + 59 * 1_000_000;

#[derive(Clone, Debug)]
pub enum RawCell {
    Null,
    Int(i64),
    Uint(u64),
    Float(f32),
    Double(f64),
    Bytes(Vec<u8>),
    Date {
        year: u16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        micros: u32,
    },
    Time {
        negative: bool,
        days: u32,
        hours: u8,
        minutes: u8,
        seconds: u8,
        micros: u32,
    },
}

#[derive(Clone, Debug)]
pub struct RejectNote {
    pub reason: &'static str,
    pub value_len: u64,
}

pub fn decode_cell(store: StoreKind, raw: &RawCell, nullable: bool) -> Result<(DecodedCell, Option<RejectNote>), PreviewError> {
    if matches!(raw, RawCell::Null) {
        if !nullable {
            return Err(PreviewError::new("value", "null_not_allowed"));
        }
        return Ok((DecodedCell::sql_null(), None));
    }
    match decode_present(store, raw) {
        Ok(cell) => Ok((cell, None)),
        Err(reason) if nullable && matches!(reason, "zero_date" | "bool_domain") => Ok((
            DecodedCell::sql_null(),
            Some(RejectNote {
                reason,
                value_len: raw_len(raw),
            }),
        )),
        Err(reason) => Err(PreviewError::new("value", reason)),
    }
}

fn decode_present(store: StoreKind, raw: &RawCell) -> Result<DecodedCell, &'static str> {
    match store {
        StoreKind::Int { width } => int_cell(width, raw),
        StoreKind::Bool => bool_cell(raw),
        StoreKind::Numeric { precision, scale } => numeric_cell(precision, scale, raw),
        StoreKind::Float4 => float_cell(raw, true),
        StoreKind::Float8 => float_cell(raw, false),
        StoreKind::Text { limit } => text_cell(raw, limit, false),
        StoreKind::Json => text_cell(raw, None, true),
        StoreKind::Bytes => bytes_cell(raw),
        StoreKind::Date => date_cell(raw),
        StoreKind::Time => time_cell(raw),
        StoreKind::TimestampNaive => timestamp_cell(raw, false),
        StoreKind::TimestampTz => timestamp_cell(raw, true),
    }
}

fn int_cell(width: u8, raw: &RawCell) -> Result<DecodedCell, &'static str> {
    let number = match raw {
        RawCell::Int(value) => *value,
        RawCell::Uint(value) => i64::try_from(*value).map_err(|_| "int_range")?,
        RawCell::Bytes(bytes) => parse_i64(bytes)?,
        _ => return Err("value_kind"),
    };
    let fits = match width {
        2 => i16::try_from(number).is_ok(),
        4 => i32::try_from(number).is_ok(),
        _ => true,
    };
    if !fits {
        return Err("int_range");
    }
    let arrow = match width {
        2 => ArrowScalar::Int16(number as i16),
        4 => ArrowScalar::Int32(number as i32),
        _ => ArrowScalar::Int64(number),
    };
    let exact = ExactInt::parse(&number.to_string()).map_err(|_| "int_range")?;
    Ok(value_cell(LogicalValue::Int(exact), arrow))
}

fn bool_cell(raw: &RawCell) -> Result<DecodedCell, &'static str> {
    let number = match raw {
        RawCell::Int(value) => *value,
        RawCell::Uint(value) => i64::try_from(*value).map_err(|_| "bool_domain")?,
        RawCell::Bytes(bytes) => parse_i64(bytes)?,
        _ => return Err("value_kind"),
    };
    let value = match number {
        0 => false,
        1 => true,
        _ => return Err("bool_domain"),
    };
    Ok(value_cell(LogicalValue::Bool(value), ArrowScalar::Bool(value)))
}

fn numeric_cell(precision: u16, scale: i16, raw: &RawCell) -> Result<DecodedCell, &'static str> {
    if matches!(raw, RawCell::Float(_) | RawCell::Double(_)) {
        return Err("float_not_exact");
    }
    let text = match raw {
        RawCell::Bytes(bytes) => std::str::from_utf8(bytes).map_err(|_| "decimal_text")?,
        RawCell::Int(value) => return numeric_from_text(&value.to_string(), precision, scale),
        RawCell::Uint(value) => return numeric_from_text(&value.to_string(), precision, scale),
        _ => return Err("value_kind"),
    };
    numeric_from_text(text.trim(), precision, scale)
}

fn numeric_from_text(text: &str, precision: u16, scale: i16) -> Result<DecodedCell, &'static str> {
    let parsed = ExactDecimal::parse_sql(text).map_err(|_| "decimal_text")?;
    let exact = parsed
        .rescale_exact(i32::from(scale))
        .map_err(|_| "decimal_scale")?;
    if exact.coefficient().len() > usize::from(precision) {
        return Err("decimal_precision");
    }
    let arrow = if precision <= 38 {
        ArrowScalar::Decimal128(unscaled_i128(&exact).ok_or("decimal_precision")?)
    } else {
        ArrowScalar::Decimal256(unscaled_i256(&exact).ok_or("decimal_precision")?)
    };
    Ok(value_cell(LogicalValue::Decimal(exact), arrow))
}

fn float_cell(raw: &RawCell, narrow: bool) -> Result<DecodedCell, &'static str> {
    let bits = match raw {
        RawCell::Float(value) => f64::from(*value).to_bits(),
        RawCell::Double(value) if !narrow => value.to_bits(),
        RawCell::Double(value) if narrow => {
            let narrowed = *value as f32;
            if f64::from(narrowed).to_bits() != value.to_bits() {
                return Err("float_precision");
            }
            value.to_bits()
        }
        _ => return Err("value_kind"),
    };
    if narrow {
        let wide = f64::from_bits(bits);
        let narrowed = wide as f32;
        if narrowed.is_nan() {
            return Err("float4_nan");
        }
        if f64::from(narrowed).to_bits() != bits {
            return Err("float_precision");
        }
    }
    let mut cell = value_cell(LogicalValue::Float(classify_f64(f64::from_bits(bits))), ArrowScalar::Float64(bits));
    cell.float_bits = Some(bits);
    Ok(cell)
}

fn text_cell(raw: &RawCell, limit: Option<usize>, json: bool) -> Result<DecodedCell, &'static str> {
    let bytes = match raw {
        RawCell::Bytes(bytes) => bytes.as_slice(),
        _ => return Err("value_kind"),
    };
    if bytes.contains(&0) {
        return Err("text_nul");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "charset")?;
    if json {
        serde_json::from_str::<JsonValue>(text).map_err(|_| "json")?;
    }
    if limit.is_some_and(|max| text.chars().count() > max) {
        return Err("char_length");
    }
    Ok(value_cell(LogicalValue::Text(text.to_string()), ArrowScalar::Utf8(text.to_string())))
}

fn bytes_cell(raw: &RawCell) -> Result<DecodedCell, &'static str> {
    let bytes = match raw {
        RawCell::Bytes(bytes) => bytes.clone(),
        _ => return Err("value_kind"),
    };
    Ok(value_cell(LogicalValue::Bytes(bytes.clone()), ArrowScalar::Binary(bytes)))
}

fn date_cell(raw: &RawCell) -> Result<DecodedCell, &'static str> {
    let (year, month, day) = date_parts(raw)?;
    if year == 0 || month == 0 || day == 0 {
        return Err("zero_date");
    }
    let year = i32::from(year);
    validate_date(year, month, day).map_err(|_| "date_range")?;
    let unix = unix_days(year, month, day);
    let arrow_days = i32::try_from(unix).map_err(|_| "date_range")?;
    Ok(value_cell(
        LogicalValue::Date { year, month, day },
        ArrowScalar::Date32(arrow_days),
    ))
}

fn time_cell(raw: &RawCell) -> Result<DecodedCell, &'static str> {
    let RawCell::Time {
        negative,
        days,
        hours,
        minutes,
        seconds,
        micros,
    } = raw
    else {
        return Err("value_kind");
    };
    if *hours > 23 || *minutes > 59 || *seconds > 59 || *micros > 999_999 {
        return Err("time_range");
    }
    let total = i128::from(*days) * DAY_MICROS
        + i128::from(*hours) * 3_600_000_000
        + i128::from(*minutes) * 60_000_000
        + i128::from(*seconds) * 1_000_000
        + i128::from(*micros);
    if total > MYSQL_TIME_MAX_MICROS {
        return Err("time_range");
    }
    let signed = if *negative { -total } else { total };
    let value = i64::try_from(signed).map_err(|_| "time_range")?;
    Ok(value_cell(LogicalValue::DurationMicros(value), ArrowScalar::DurationMicros(value)))
}

fn timestamp_cell(raw: &RawCell, utc: bool) -> Result<DecodedCell, &'static str> {
    let (year, month, day, hour, minute, second, micros) = match raw {
        RawCell::Date {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        } => (*year, *month, *day, *hour, *minute, *second, *micros),
        _ => return Err("value_kind"),
    };
    if year == 0 || month == 0 || day == 0 {
        return Err("zero_date");
    }
    let year = i32::from(year);
    validate_date(year, month, day).map_err(|_| "timestamp_range")?;
    if hour > 23 || minute > 59 || second > 59 || micros > 999_999 {
        return Err("timestamp_range");
    }
    let unix = naive_unix(year, month, day, hour, minute, second, micros).map_err(|_| "timestamp_range")?;
    if utc {
        Ok(value_cell(
            LogicalValue::TimestampTzMicros(unix),
            ArrowScalar::TimestampUtc(unix),
        ))
    } else {
        Ok(value_cell(
            LogicalValue::TimestampNaive {
                year,
                month,
                day,
                hour,
                minute,
                second,
                micros,
            },
            ArrowScalar::TimestampNaive(unix),
        ))
    }
}

fn date_parts(raw: &RawCell) -> Result<(u16, u8, u8), &'static str> {
    match raw {
        RawCell::Date { year, month, day, hour, minute, second, micros }
            if *hour == 0 && *minute == 0 && *second == 0 && *micros == 0 =>
        {
            Ok((*year, *month, *day))
        }
        RawCell::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| "date_range")?;
            parse_date_text(text)
        }
        _ => Err("value_kind"),
    }
}

fn parse_date_text(text: &str) -> Result<(u16, u8, u8), &'static str> {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 3 {
        return Err("date_range");
    }
    let year: u16 = parts[0].parse().map_err(|_| "date_range")?;
    let month: u8 = parts[1].parse().map_err(|_| "date_range")?;
    let day: u8 = parts[2].parse().map_err(|_| "date_range")?;
    Ok((year, month, day))
}

fn parse_i64(bytes: &[u8]) -> Result<i64, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "int_range")?;
    text.trim().parse().map_err(|_| "int_range")
}

fn value_cell(value: LogicalValue, arrow: ArrowScalar) -> DecodedCell {
    DecodedCell {
        presence: CellPresence::Value,
        value: Some(value),
        arrow: Some(arrow),
        reject: None,
        actual_precision: None,
        actual_scale: None,
        float_bits: None,
    }
}

fn raw_len(raw: &RawCell) -> u64 {
    match raw {
        RawCell::Null => 0,
        RawCell::Bytes(bytes) => bytes.len() as u64,
        RawCell::Int(_) | RawCell::Uint(_) | RawCell::Float(_) | RawCell::Double(_) => 8,
        RawCell::Date { .. } | RawCell::Time { .. } => 8,
    }
}

pub fn reject_for(note: &RejectNote) -> CellReject {
    CellReject {
        reason: note.reason,
        value_len: note.value_len,
    }
}

fn unscaled_i128(exact: &ExactDecimal) -> Option<i128> {
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value.checked_mul(10)?.checked_add(i128::from(byte - b'0'))?;
    }
    if exact.is_negative() {
        value = value.checked_neg()?;
    }
    Some(value)
}

fn unscaled_i256(exact: &ExactDecimal) -> Option<[u8; 32]> {
    let mut bytes = [0u8; 32];
    for digit in exact.coefficient().bytes() {
        mul_add_le(&mut bytes, 10, u32::from(digit - b'0')).ok()?;
    }
    if exact.is_negative() && exact.coefficient() != "0" {
        neg_le(&mut bytes).ok()?;
    }
    Some(bytes)
}

fn mul_add_le(bytes: &mut [u8; 32], factor: u32, add: u32) -> Result<(), ()> {
    let mut carry = add;
    for byte in bytes.iter_mut() {
        let wide = u32::from(*byte) * factor + carry;
        *byte = (wide & 0xff) as u8;
        carry >>= 8;
    }
    if carry == 0 { Ok(()) } else { Err(()) }
}

fn neg_le(bytes: &mut [u8; 32]) -> Result<(), ()> {
    let mut carry = 1u16;
    for byte in bytes.iter_mut() {
        let wide = u16::from(!*byte) + carry;
        *byte = (wide & 0xff) as u8;
        carry >>= 8;
    }
    if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        Ok(())
    } else {
        Err(())
    }
}

fn unix_days(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year);
    let shifted = if month <= 2 { year - 1 } else { year };
    let era = if shifted >= 0 { shifted } else { shifted - 399 } / 400;
    let year_of_era = u64::try_from(shifted - era * 400).unwrap_or(0);
    let month_prime = if month > 2 {
        u64::from(month) - 3
    } else {
        u64::from(month) + 9
    };
    let day_of_year = (153 * month_prime + 2) / 5 + u64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + i64::try_from(day_of_era).unwrap_or(0) - 719_468
}

fn naive_unix(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8, micros: u32) -> Result<i64, ()> {
    let days = i128::from(unix_days(year, month, day));
    let sod = i128::from(hour) * 3_600 + i128::from(minute) * 60 + i128::from(second);
    let unix = days
        .checked_mul(DAY_MICROS)
        .ok_or(())?
        .checked_add(sod.checked_mul(1_000_000).ok_or(())?)
        .ok_or(())?
        .checked_add(i128::from(micros))
        .ok_or(())?;
    i64::try_from(unix).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_text_is_not_float() {
        let raw = RawCell::Bytes(b"10.50".to_vec());
        let (cell, note) = decode_cell(StoreKind::Numeric { precision: 12, scale: 2 }, &raw, false).expect("dec");
        assert!(note.is_none());
        match cell.value {
            Some(LogicalValue::Decimal(value)) => {
                assert_eq!(value.to_plain_string(), "10.50");
                assert!(!value.is_negative());
            }
            _ => panic!("decimal"),
        }
    }

    #[test]
    fn bigint_unsigned_edges() {
        for text in ["0", "9223372036854775808", "18446744073709551615"] {
            let raw = RawCell::Bytes(text.as_bytes().to_vec());
            let (cell, _) = decode_cell(StoreKind::Numeric { precision: 20, scale: 0 }, &raw, false).expect(text);
            match cell.value {
                Some(LogicalValue::Decimal(value)) => assert_eq!(value.to_plain_string(), text),
                _ => panic!("numeric"),
            }
        }
    }

    #[test]
    fn zero_date_on_nullable_keeps_null_reject() {
        let raw = RawCell::Date {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            micros: 0,
        };
        let (cell, note) = decode_cell(StoreKind::Date, &raw, true).expect("null");
        assert_eq!(cell.presence, CellPresence::Null);
        assert_eq!(note.expect("reject").reason, "zero_date");
    }

    #[test]
    fn time_signed_duration_preserves_range_and_precision() {
        let raw = RawCell::Time {
            negative: false,
            days: 1,
            hours: 2,
            minutes: 0,
            seconds: 0,
            micros: 0,
        };
        let (cell, note) = decode_cell(StoreKind::Time, &raw, true).expect("time");
        assert_eq!(cell.presence, CellPresence::Value);
        assert!(note.is_none());
        assert_eq!(cell.arrow, Some(ArrowScalar::DurationMicros(93_600_000_000)));
        let negative = RawCell::Time { negative: true, days: 0, hours: 1, minutes: 2, seconds: 3, micros: 456_789 };
        let (cell, note) = decode_cell(StoreKind::Time, &negative, true).expect("negative time");
        assert!(note.is_none());
        assert_eq!(cell.arrow, Some(ArrowScalar::DurationMicros(-3_723_456_789)));
        let null = decode_cell(StoreKind::Time, &RawCell::Null, true).expect("null time");
        assert_eq!(null.0.presence, CellPresence::Null);
        let outside = RawCell::Time { negative: false, days: 34, hours: 23, minutes: 0, seconds: 0, micros: 0 };
        assert!(matches!(decode_cell(StoreKind::Time, &outside, true), Err(PreviewError { code: "value", fact: "time_range" })));
    }

    #[test]
    fn tinyint_domain_values_fit_int2() {
        for value in [0_i64, 1, 2, -1, 127] {
            let raw = RawCell::Int(value);
            decode_cell(StoreKind::Int { width: 2 }, &raw, false).expect("int2");
        }
    }
}
