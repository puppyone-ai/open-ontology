//! Source types. `datetime2(7)` is an Int64 count of 100ns ticks since 0001-01-01.
//! A microsecond conversion is a refusal, not a fallback.

use tiberius::time::{DateTime2, Time};
use tiberius::ColumnData;

use crate::fail::Fail;

pub const TICKS_PER_DAY: u64 = 864_000_000_000;
pub const NANOS_PER_DAY: u64 = 86_400_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlKind {
    Nvarchar,
    Binary,
    Decimal,
    Time,
    Datetime2,
    Date,
    Uniqueidentifier,
    Bit,
    Int,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Null,
    Text(String),
    Bytes(Vec<u8>),
    Decimal(i128),
    TimeNanos(i64),
    Datetime2Ticks(i64),
    DateDays(i32),
    GuidWire([u8; 16]),
    Bit(bool),
    Int(i64),
}

pub fn days_since_year_one(year: i32, month: u32, day: u32) -> Result<u32, Fail> {
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(Fail::new("date_range", "outside 0001-01-01..9999-12-31"));
    }
    let mut y = year;
    let m = month as i32;
    if m <= 2 {
        y -= 1;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let unix = i64::from(era) * 146_097 + i64::from(doe) - 719_468;
    let days = unix + 719_162;
    u32::try_from(days).map_err(|_| Fail::new("date_range", "day count does not fit"))
}

pub fn datetime2_ticks(days: u32, increments: u64, scale: u8) -> Result<i64, Fail> {
    if scale > 7 {
        return Err(Fail::new(
            "datetime2_scale",
            "scale above 7 is refused; datetime2 is not silently stored as microseconds",
        ));
    }
    let factor = 10u64.pow(u32::from(7 - scale));
    let fraction = increments.checked_mul(factor).ok_or_else(|| {
        Fail::new("datetime2_range", "fraction does not fit 100ns ticks")
    })?;
    let day_ticks = u64::from(days).checked_mul(TICKS_PER_DAY).ok_or_else(|| {
        Fail::new("datetime2_range", "day count does not fit 100ns ticks")
    })?;
    let ticks = day_ticks.checked_add(fraction).ok_or_else(|| {
        Fail::new("datetime2_range", "ticks do not fit")
    })?;
    i64::try_from(ticks).map_err(|_| Fail::new("datetime2_range", "ticks do not fit i64"))
}

pub fn time_nanoseconds(increments: u64, scale: u8) -> Result<i64, Fail> {
    if scale > 7 {
        return Err(Fail::new("time_scale", "time scale above 7 is refused"));
    }
    let factor = 10u64.pow(u32::from(9 - scale));
    let nanos = increments.checked_mul(factor).ok_or_else(|| {
        Fail::new("time_range", "time increments do not fit nanoseconds")
    })?;
    if nanos >= NANOS_PER_DAY {
        return Err(Fail::new("time_range", "time is not inside one day"));
    }
    i64::try_from(nanos).map_err(|_| Fail::new("time_range", "time does not fit i64"))
}

/// Tiberius exposes `uniqueidentifier` in RFC byte order. The swap below is
/// the inverse of its mixed-endian wire reorder, and it is its own inverse.
pub fn guid_wire_from_rfc(mut rfc: [u8; 16]) -> [u8; 16] {
    rfc.swap(0, 3);
    rfc.swap(1, 2);
    rfc.swap(4, 5);
    rfc.swap(6, 7);
    rfc
}

pub fn collation_case_insensitive(collation: &str) -> bool {
    let upper = collation.to_ascii_uppercase();
    !upper.contains("_BIN") && upper.contains("_CI")
}

pub fn cell_from_column(
    type_name: &str,
    precision: u8,
    scale: u8,
    data: &ColumnData<'_>,
) -> Result<Cell, Fail> {
    let type_name = type_name.to_ascii_lowercase();
    if matches!(data, ColumnData::String(None) | ColumnData::Binary(None) | ColumnData::Numeric(None)
        | ColumnData::Guid(None) | ColumnData::Bit(None) | ColumnData::U8(None)
        | ColumnData::I16(None) | ColumnData::I32(None) | ColumnData::I64(None)
        | ColumnData::F32(None) | ColumnData::F64(None) | ColumnData::DateTime(None)
        | ColumnData::SmallDateTime(None) | ColumnData::Xml(None))
        || matches!(data, ColumnData::Time(None) | ColumnData::Date(None) | ColumnData::DateTime2(None)
            | ColumnData::DateTimeOffset(None))
    {
        return Ok(Cell::Null);
    }
    match type_name.as_str() {
        "nvarchar" | "nchar" | "ntext" | "sysname" => match data {
            ColumnData::String(Some(value)) => Ok(Cell::Text(value.to_string())),
            _ => Err(Fail::new("nvarchar_type", "nvarchar value was not a string")),
        },
        "varchar" | "char" | "text" => Err(Fail::new(
            "character_type_not_nvarchar",
            "varchar is refused so a code page is not stored as nvarchar",
        )),
        "binary" | "varbinary" | "image" | "timestamp" | "rowversion" => match data {
            ColumnData::Binary(Some(value)) => Ok(Cell::Bytes(value.to_vec())),
            _ => Err(Fail::new("binary_type", "binary value was not raw bytes")),
        },
        "decimal" | "numeric" => decimal_cell(precision, scale, data, "decimal"),
        "money" => decimal_cell(19, 4, data, "money_float"),
        "smallmoney" => decimal_cell(10, 4, data, "money_float"),
        "time" => match data {
            ColumnData::Time(Some(value)) => time_cell(scale, *value),
            ColumnData::String(_) => Err(Fail::new(
                "time_string_not_accepted",
                "time text is refused",
            )),
            _ => Err(Fail::new("time_type", "time value was not tds time")),
        },
        "datetime2" => match data {
            ColumnData::DateTime2(Some(value)) => datetime2_cell(scale, *value),
            ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) => Err(Fail::new(
                "datetime_is_not_datetime2",
                "datetime is not datetime2 and is not stored as microseconds",
            )),
            ColumnData::String(_) => Err(Fail::new(
                "datetime2_string_not_accepted",
                "datetime2 text is refused; 100ns would be ambiguous",
            )),
            _ => Err(Fail::new("datetime2_type", "datetime2 value was not tds datetime2")),
        },
        "date" => match data {
            ColumnData::Date(Some(value)) => {
                let days = i32::try_from(value.days())
                    .map_err(|_| Fail::new("date_range", "date days do not fit"))?;
                Ok(Cell::DateDays(days))
            }
            _ => Err(Fail::new("date_type", "date value was not tds date")),
        },
        "datetime" | "smalldatetime" => Err(Fail::new(
            "datetime_encoding_refused",
            "datetime and smalldatetime are refused; they are not datetime2(7)",
        )),
        "datetimeoffset" => Err(Fail::new(
            "datetimeoffset_offset_not_dropped",
            "datetimeoffset is refused so the offset is not discarded",
        )),
        "uniqueidentifier" => match data {
            ColumnData::Guid(Some(value)) => Ok(Cell::GuidWire(guid_wire_from_rfc(*value.as_bytes()))),
            _ => Err(Fail::new("guid_type", "uniqueidentifier value was not a guid")),
        },
        "bit" => match data {
            ColumnData::Bit(Some(value)) => Ok(Cell::Bit(*value)),
            _ => Err(Fail::new("bit_type", "bit value was not a bit")),
        },
        "tinyint" | "smallint" | "int" | "bigint" => integer_cell(data),
        "float" | "real" => Err(Fail::new(
            "float_not_exact",
            "float and real are refused; decimal is not stored as f64",
        )),
        "xml" => match data {
            ColumnData::Xml(Some(value)) if value.schema().is_none() => {
                Ok(Cell::Text(value.to_string()))
            }
            ColumnData::Xml(Some(_)) => Err(Fail::new(
                "xml_schema_not_dropped",
                "xml with a schema collection is refused",
            )),
            _ => Err(Fail::new("xml_type", "xml value was not xml")),
        },
        _ => Err(Fail::new(
            "unsupported_type",
            "sql type has no precision-preserving arrow encoding",
        )),
    }
}

fn decimal_cell(precision: u8, scale: u8, data: &ColumnData<'_>, float_code: &str) -> Result<Cell, Fail> {
    match data {
        ColumnData::Numeric(Some(value)) => {
            if value.scale() != scale {
                return Err(Fail::new("decimal_scale", "numeric scale does not match the catalog"));
            }
            if precision == 0 || precision > 38 || !unscaled_fits(value.value(), precision) {
                return Err(Fail::new("decimal_precision", "numeric precision does not fit decimal128"));
            }
            Ok(Cell::Decimal(value.value()))
        }
        ColumnData::F64(_) | ColumnData::F32(_) => Err(Fail::new(
            if float_code == "money_float" { "money_float" } else { "float_not_exact" },
            "exact numeric arrived as a binary float and is refused",
        )),
        _ => Err(Fail::new("decimal_type", "decimal value was not numeric")),
    }
}

fn time_cell(scale: u8, value: Time) -> Result<Cell, Fail> {
    if value.scale() != scale {
        return Err(Fail::new("time_scale", "time scale does not match the catalog"));
    }
    Ok(Cell::TimeNanos(time_nanoseconds(value.increments(), value.scale())?))
}

fn datetime2_cell(scale: u8, value: DateTime2) -> Result<Cell, Fail> {
    let time = value.time();
    if time.scale() != scale {
        return Err(Fail::new("datetime2_scale", "datetime2 scale does not match the catalog"));
    }
    let days = value.date().days();
    Ok(Cell::Datetime2Ticks(datetime2_ticks(days, time.increments(), time.scale())?))
}

fn integer_cell(data: &ColumnData<'_>) -> Result<Cell, Fail> {
    let value = match data {
        ColumnData::U8(Some(value)) => i64::from(*value),
        ColumnData::I16(Some(value)) => i64::from(*value),
        ColumnData::I32(Some(value)) => i64::from(*value),
        ColumnData::I64(Some(value)) => *value,
        _ => return Err(Fail::new("integer_type", "integer value was not an integer")),
    };
    Ok(Cell::Int(value))
}

pub fn unscaled_fits(value: i128, precision: u8) -> bool {
    if precision == 0 || precision > 38 {
        return false;
    }
    let mut limit = 1i128;
    for _ in 0..precision {
        limit = match limit.checked_mul(10) {
            Some(next) => next,
            None => return false,
        };
    }
    value.checked_abs().is_some_and(|absolute| absolute < limit)
}

pub fn pg_timestamp6_for_datetime2() -> Result<(), Fail> {
    Err(Fail::new(
        "pg_timestamp6",
        "datetime2(7) is not timestamp(6); 100ns is retained or the value is refused",
    ))
}

pub fn kind_for(type_name: &str) -> Result<SqlKind, Fail> {
    match type_name.to_ascii_lowercase().as_str() {
        "nvarchar" | "nchar" | "ntext" | "sysname" | "xml" => Ok(SqlKind::Nvarchar),
        "binary" | "varbinary" | "image" | "timestamp" | "rowversion" => Ok(SqlKind::Binary),
        "decimal" | "numeric" | "money" | "smallmoney" => Ok(SqlKind::Decimal),
        "time" => Ok(SqlKind::Time),
        "datetime2" => Ok(SqlKind::Datetime2),
        "date" => Ok(SqlKind::Date),
        "uniqueidentifier" => Ok(SqlKind::Uniqueidentifier),
        "bit" => Ok(SqlKind::Bit),
        "tinyint" | "smallint" | "int" | "bigint" => Ok(SqlKind::Int),
        "varchar" | "char" | "text" => Err(Fail::new(
            "character_type_not_nvarchar",
            "varchar is refused so a code page is not stored as nvarchar",
        )),
        "datetime" | "smalldatetime" => Err(Fail::new(
            "datetime_encoding_refused",
            "datetime and smalldatetime are refused; they are not datetime2(7)",
        )),
        "datetimeoffset" => Err(Fail::new(
            "datetimeoffset_offset_not_dropped",
            "datetimeoffset is refused so the offset is not discarded",
        )),
        "float" | "real" => Err(Fail::new(
            "float_not_exact",
            "float and real are refused; decimal is not stored as f64",
        )),
        _ => Err(Fail::new(
            "unsupported_type",
            "sql type has no precision-preserving arrow encoding",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    use tiberius::numeric::Numeric;
    use tiberius::time::{Date, DateTime2, Time};
    use tiberius::Uuid;

    #[test]
    fn datetime2_scale_7_keeps_100ns_and_does_not_divide() {
        assert_eq!(days_since_year_one(1970, 1, 1).unwrap(), 719_162);
        assert_eq!(days_since_year_one(2024, 3, 15).unwrap(), 738_959);
        let ticks = datetime2_ticks(738_959, 452_961_234_567, 7).unwrap();
        let truncated = datetime2_ticks(738_959, 452_961_234_560, 7).unwrap();
        assert_eq!(ticks - truncated, 7);
        assert_eq!(pg_timestamp6_for_datetime2().unwrap_err().code, "pg_timestamp6");
        assert_eq!(datetime2_ticks(738_959, 1, 8).unwrap_err().code, "datetime2_scale");
        let end = days_since_year_one(9999, 12, 31).unwrap();
        assert!(datetime2_ticks(end, 999_999_9, 7).is_ok());
    }

    #[test]
    fn time_scale_7_is_exact_nanoseconds() {
        assert_eq!(time_nanoseconds(1_234_567, 7).unwrap(), 123_456_700);
        assert_eq!(time_nanoseconds(1, 8).unwrap_err().code, "time_scale");
    }

    #[test]
    fn guid_wire_order_is_the_rfc_reorder_inverse() {
        let rfc = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
            0xff,
        ];
        let wire = guid_wire_from_rfc(rfc);
        assert_eq!(
            wire,
            [
                0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff
            ]
        );
        assert_eq!(guid_wire_from_rfc(wire), rfc);
        let cell = cell_from_column(
            "uniqueidentifier",
            0,
            0,
            &ColumnData::Guid(Some(Uuid::from_bytes(rfc))),
        )
        .unwrap();
        assert_eq!(cell, Cell::GuidWire(wire));
    }

    #[test]
    fn nvarchar_binary_and_decimal_keep_source_units() {
        let text = cell_from_column(
            "nvarchar",
            0,
            0,
            &ColumnData::String(Some(Cow::Borrowed("A "))),
        )
        .unwrap();
        assert_eq!(text, Cell::Text("A ".to_string()));
        assert!(collation_case_insensitive("Latin1_General_CI_AS"));
        assert!(!collation_case_insensitive("Latin1_General_BIN2"));
        let raw = cell_from_column(
            "binary",
            0,
            0,
            &ColumnData::Binary(Some(Cow::Owned(vec![0x00, 0x00, 0x00, 0xff]))),
        )
        .unwrap();
        assert_eq!(raw, Cell::Bytes(vec![0x00, 0x00, 0x00, 0xff]));
        let unscaled = 9_007_199_254_740_993_i128 * 10_000_000_000 + 1;
        let decimal = cell_from_column(
            "decimal",
            26,
            10,
            &ColumnData::Numeric(Some(Numeric::new_with_scale(unscaled, 10))),
        )
        .unwrap();
        assert_eq!(decimal, Cell::Decimal(unscaled));
        let money = cell_from_column(
            "money",
            19,
            4,
            &ColumnData::F64(Some(1.25)),
        )
        .unwrap_err();
        assert_eq!(money.code, "money_float");
        let datetime = cell_from_column(
            "datetime2",
            0,
            7,
            &ColumnData::String(Some(Cow::Borrowed("2024-03-15 12:34:56.123456"))),
        )
        .unwrap_err();
        assert_eq!(datetime.code, "datetime2_string_not_accepted");
        let kept = cell_from_column(
            "datetime2",
            0,
            7,
            &ColumnData::DateTime2(Some(DateTime2::new(
                Date::new(738_959),
                Time::new(452_961_234_567, 7),
            ))),
        )
        .unwrap();
        assert_eq!(
            kept,
            Cell::Datetime2Ticks(datetime2_ticks(738_959, 452_961_234_567, 7).unwrap())
        );
    }
}
