//! DM8 column admission and lossless Arrow mapping.
//!
//! Typed where lossless: integers, BIT, DECIMAL/NUMBER(p<=38,s), DOUBLE/FLOAT
//! (REAL widened in the source), DATE, TIMESTAMP(<=6), zoned timestamps (<=6)
//! as UTC instants (D17), character/CLOB as text, binary/BLOB as bytes.
//! Exact text: NUMBER without precision, TIMESTAMP(7-9), TIME, INTERVAL.
//! Anything else (BFILE, classes, arrays, spatial) fails closed.
use std::sync::Arc;

use arrow_array::{
    ArrayRef, BinaryArray, BooleanArray, Date32Array, Decimal128Array, Float64Array, Int16Array,
    Int32Array, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};

use super::error;
use crate::{SourceError, engine::KeyValue};

pub(super) const SUMMARY: &str = "integers/BIT/DECIMAL(p<=38)/DOUBLE/REAL/DATE/TIMESTAMP(<=6) typed; \
zoned timestamps as UTC instants; CHAR/VARCHAR/CLOB text; BINARY/BLOB bytes; \
NUMBER without precision, TIMESTAMP(7-9), TIME, INTERVAL exact text; other types refused";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Arrow {
    Int16,
    Int32,
    Int64,
    Bool,
    Float64,
    Decimal(u8, i8),
    Date,
    Timestamp,
    TimestampUtc,
    Text,
    Binary,
}

/// Source-side rendering, so the worker never interprets a temporal value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Render {
    Raw,
    Double,
    Date,
    Timestamp(u8),
    Utc(u8),
    LocalUtc(u8),
    Cast,
}

#[derive(Clone, Debug)]
pub(super) struct Column {
    pub name: String,
    pub arrow: Arrow,
    render: Render,
    character: bool,
    pub nullable: bool,
}

impl Column {
    pub fn classify(
        name: &str,
        kind: &str,
        precision: Option<i64>,
        scale: Option<i64>,
        nullable: bool,
    ) -> Result<Self, SourceError> {
        // DM reports the LOCAL TIME ZONE fraction with a 4096 flag.
        let fraction = scale.map(|s| s % 4096).unwrap_or(6);
        let (arrow, render, character) = match kind {
            "BIGINT" => (Arrow::Int64, Render::Raw, false),
            "INT" | "INTEGER" | "PLS_INTEGER" | "BINARY_INTEGER" => {
                (Arrow::Int32, Render::Raw, false)
            }
            "SMALLINT" | "TINYINT" | "BYTE" => (Arrow::Int16, Render::Raw, false),
            "BIT" => (Arrow::Bool, Render::Raw, false),
            "DECIMAL" | "DEC" | "NUMBER" | "NUMERIC" => match (precision, scale) {
                (Some(p @ 1..=38), Some(s)) if (0..=p).contains(&s) => {
                    (Arrow::Decimal(p as u8, s as i8), Render::Raw, false)
                }
                _ => (Arrow::Text, Render::Raw, false),
            },
            "DOUBLE" | "FLOAT" | "DOUBLE PRECISION" => (Arrow::Float64, Render::Raw, false),
            "REAL" => (Arrow::Float64, Render::Double, false),
            "DATE" => (Arrow::Date, Render::Date, false),
            "TIMESTAMP" | "DATETIME" => temporal(fraction, Arrow::Timestamp, Render::Timestamp)?,
            "TIMESTAMP WITH TIME ZONE" | "DATETIME WITH TIME ZONE" => {
                temporal(fraction, Arrow::TimestampUtc, Render::Utc)?
            }
            "TIMESTAMP WITH LOCAL TIME ZONE" | "DATETIME WITH LOCAL TIME ZONE" => {
                temporal(fraction, Arrow::TimestampUtc, Render::LocalUtc)?
            }
            "TIME" | "TIME WITH TIME ZONE" => (Arrow::Text, Render::Cast, false),
            kind if kind.starts_with("INTERVAL ") => (Arrow::Text, Render::Cast, false),
            "CHAR" | "CHARACTER" | "VARCHAR" | "VARCHAR2" | "NCHAR" | "NVARCHAR" | "NVARCHAR2" => {
                (Arrow::Text, Render::Raw, true)
            }
            "TEXT" | "LONG" | "LONGVARCHAR" | "CLOB" | "NCLOB" => (Arrow::Text, Render::Raw, false),
            "BINARY" | "VARBINARY" | "BLOB" | "IMAGE" | "LONGVARBINARY" | "RAW" => {
                (Arrow::Binary, Render::Raw, false)
            }
            _ => return Err(error("dm8_type_not_admitted")),
        };
        Ok(Self {
            name: name.into(),
            arrow,
            render,
            character,
            nullable,
        })
    }

    pub fn projection(&self, name: &str) -> Result<String, SourceError> {
        let fraction = |f: u8| if f <= 6 { "FF6" } else { "FF9" };
        Ok(match self.render {
            Render::Raw => name.into(),
            Render::Double => format!("CAST({name} AS DOUBLE)"),
            Render::Date => format!("TO_CHAR({name},'YYYY-MM-DD')"),
            Render::Timestamp(f) => {
                format!("TO_CHAR({name},'YYYY-MM-DD HH24:MI:SS.{}')", fraction(f))
            }
            Render::Utc(f) => format!(
                "TO_CHAR({name} AT TIME ZONE '+00:00','YYYY-MM-DD HH24:MI:SS.{}')",
                fraction(f)
            ),
            Render::LocalUtc(f) => format!(
                "TO_CHAR(CAST({name} AS TIMESTAMP(9) WITH TIME ZONE) AT TIME ZONE '+00:00','YYYY-MM-DD HH24:MI:SS.{}')",
                fraction(f)
            ),
            Render::Cast => format!("CAST({name} AS VARCHAR(256))"),
        })
    }

    pub fn key_admitted(&self) -> bool {
        matches!(
            self.arrow,
            Arrow::Int16 | Arrow::Int32 | Arrow::Int64 | Arrow::Decimal(..)
        ) || self.character
    }

    pub fn key_value(&self, text: &str) -> Result<KeyValue, SourceError> {
        Ok(match self.arrow {
            Arrow::Int16 | Arrow::Int32 | Arrow::Int64 => {
                KeyValue::Int(text.trim().parse().map_err(|_| error("dm8_integer"))?)
            }
            Arrow::Decimal(..) => KeyValue::Decimal(decimal_key(text)?),
            Arrow::Text if self.character => KeyValue::Text(text.into()),
            _ => return Err(error("dm8_key_type")),
        })
    }
}

fn temporal(
    fraction: i64,
    typed: Arrow,
    render: fn(u8) -> Render,
) -> Result<(Arrow, Render, bool), SourceError> {
    match fraction {
        0..=6 => Ok((typed, render(6), false)),
        7..=9 => Ok((Arrow::Text, render(9), false)),
        _ => Err(error("dm8_type_not_admitted")),
    }
}

/// A canonical plain decimal literal for a key bind; never SQL text.
pub(super) fn decimal_key(text: &str) -> Result<String, SourceError> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty()
        || whole.len() + fraction.len() > 80
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return Err(error("dm8_decimal"));
    }
    Ok(text.into())
}

/// Exact decimal text to an unscaled coefficient at `scale`; never rounds.
pub(super) fn decimal(text: &str, precision: u8, scale: i8) -> Result<i128, SourceError> {
    let bad = || error("dm8_decimal");
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if (whole.is_empty() && fraction.is_empty())
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return Err(bad());
    }
    let scale = usize::try_from(scale).map_err(|_| bad())?;
    let (kept, dropped) = fraction.split_at(fraction.len().min(scale));
    if dropped.bytes().any(|b| b != b'0') {
        return Err(bad());
    }
    let mut coefficient: i128 = 0;
    let mut significant = 0usize;
    for b in whole
        .bytes()
        .chain(kept.bytes())
        .chain(std::iter::repeat_n(b'0', scale - kept.len()))
    {
        if significant > 0 || b != b'0' {
            significant += 1;
        }
        coefficient = coefficient
            .checked_mul(10)
            .and_then(|c| c.checked_add(i128::from(b - b'0')))
            .ok_or_else(bad)?;
    }
    if significant > usize::from(precision) {
        return Err(bad());
    }
    Ok(if negative { -coefficient } else { coefficient })
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn field(text: &str, range: std::ops::Range<usize>) -> Result<i64, SourceError> {
    let part = text.get(range).ok_or_else(|| error("dm8_temporal"))?;
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error("dm8_temporal"));
    }
    part.parse().map_err(|_| error("dm8_temporal"))
}

/// `YYYY-MM-DD` as days since 1970-01-01, validated as a real calendar day.
pub(super) fn date(text: &str) -> Result<i32, SourceError> {
    if text.len() != 10 || &text[4..5] != "-" || &text[7..8] != "-" {
        return Err(error("dm8_temporal"));
    }
    let (y, m, d) = (field(text, 0..4)?, field(text, 5..7)?, field(text, 8..10)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in_month = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month[(m - 1) as usize] {
        return Err(error("dm8_temporal"));
    }
    i32::try_from(days_from_civil(y, m, d)).map_err(|_| error("dm8_temporal"))
}

/// `YYYY-MM-DD HH24:MI:SS.FF6` as microseconds since the Unix epoch.
pub(super) fn timestamp(text: &str) -> Result<i64, SourceError> {
    if text.len() != 26
        || &text[10..11] != " "
        || &text[13..14] != ":"
        || &text[16..17] != ":"
        || &text[19..20] != "."
    {
        return Err(error("dm8_temporal"));
    }
    let days = i64::from(date(&text[..10])?);
    let (h, mi, s, us) = (
        field(text, 11..13)?,
        field(text, 14..16)?,
        field(text, 17..19)?,
        field(text, 20..26)?,
    );
    if h > 23 || mi > 59 || s > 59 {
        return Err(error("dm8_temporal"));
    }
    Ok(((days * 24 + h) * 60 + mi) * 60_000_000 + s * 1_000_000 + us)
}

fn hex(text: &str) -> Result<Vec<u8>, SourceError> {
    if !text.len().is_multiple_of(2) {
        return Err(error("dm8_binary"));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| error("dm8_binary")))
        .collect()
}

fn values<T>(
    rows: &[Vec<Option<String>>],
    i: usize,
    parse: impl Fn(&str) -> Result<T, SourceError>,
) -> Result<Vec<Option<T>>, SourceError> {
    rows.iter()
        .map(|r| r[i].as_deref().map(&parse).transpose())
        .collect()
}

pub(super) fn batch(
    columns: &[Column],
    rows: &[Vec<Option<String>>],
) -> Result<RecordBatch, SourceError> {
    if rows.iter().any(|r| r.len() != columns.len()) {
        return Err(error("dm8_row_shape"));
    }
    let mut fields = Vec::new();
    let mut arrays: Vec<ArrayRef> = Vec::new();
    for (i, c) in columns.iter().enumerate() {
        if !c.nullable && rows.iter().any(|row| row[i].is_none()) {
            return Err(error("dm8_null_in_required_column"));
        }
        let int = |t: &str| t.trim().parse::<i64>().map_err(|_| error("dm8_integer"));
        let (data_type, array): (DataType, ArrayRef) = match c.arrow {
            Arrow::Int16 => (
                DataType::Int16,
                Arc::new(Int16Array::from(values(rows, i, |t| {
                    i16::try_from(int(t)?).map_err(|_| error("dm8_integer"))
                })?)),
            ),
            Arrow::Int32 => (
                DataType::Int32,
                Arc::new(Int32Array::from(values(rows, i, |t| {
                    i32::try_from(int(t)?).map_err(|_| error("dm8_integer"))
                })?)),
            ),
            Arrow::Int64 => (
                DataType::Int64,
                Arc::new(Int64Array::from(values(rows, i, int)?)),
            ),
            Arrow::Bool => (
                DataType::Boolean,
                Arc::new(BooleanArray::from(values(rows, i, |t| match t {
                    "1" => Ok(true),
                    "0" => Ok(false),
                    _ => Err(error("dm8_bit")),
                })?)),
            ),
            Arrow::Float64 => (
                DataType::Float64,
                Arc::new(Float64Array::from(values(rows, i, |t| {
                    t.parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or_else(|| error("dm8_float"))
                })?)),
            ),
            Arrow::Decimal(p, s) => (
                DataType::Decimal128(p, s),
                Arc::new(
                    Decimal128Array::from(values(rows, i, |t| decimal(t, p, s))?)
                        .with_precision_and_scale(p, s)
                        .map_err(|_| error("dm8_decimal"))?,
                ),
            ),
            Arrow::Date => (
                DataType::Date32,
                Arc::new(Date32Array::from(values(rows, i, date)?)),
            ),
            Arrow::Timestamp => (
                DataType::Timestamp(TimeUnit::Microsecond, None),
                Arc::new(TimestampMicrosecondArray::from(values(rows, i, timestamp)?)),
            ),
            Arrow::TimestampUtc => (
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                Arc::new(
                    TimestampMicrosecondArray::from(values(rows, i, timestamp)?)
                        .with_timezone("UTC"),
                ),
            ),
            Arrow::Text => (
                DataType::Utf8,
                Arc::new(StringArray::from(
                    rows.iter().map(|r| r[i].as_deref()).collect::<Vec<_>>(),
                )),
            ),
            Arrow::Binary => {
                let bytes = values(rows, i, hex)?;
                (
                    DataType::Binary,
                    Arc::new(BinaryArray::from(
                        bytes.iter().map(|b| b.as_deref()).collect::<Vec<_>>(),
                    )),
                )
            }
        };
        fields.push(Field::new(&c.name, data_type, c.nullable));
        arrays.push(array);
    }
    RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).map_err(|_| error("dm8_arrow"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Array;

    fn column(kind: &str, p: Option<i64>, s: Option<i64>) -> Column {
        Column::classify("C", kind, p, s, true).unwrap()
    }

    #[test]
    fn mapping_is_typed_only_where_lossless() {
        assert_eq!(column("BIGINT", None, Some(0)).arrow, Arrow::Int64);
        assert_eq!(column("BYTE", None, Some(0)).arrow, Arrow::Int16);
        assert_eq!(
            column("DECIMAL", Some(31), Some(8)).arrow,
            Arrow::Decimal(31, 8)
        );
        assert_eq!(column("NUMBER", None, None).arrow, Arrow::Text);
        assert_eq!(column("TIMESTAMP", None, Some(6)).arrow, Arrow::Timestamp);
        assert_eq!(column("TIMESTAMP", None, Some(9)).arrow, Arrow::Text);
        assert_eq!(
            column("DATETIME WITH TIME ZONE", None, Some(6)).arrow,
            Arrow::TimestampUtc
        );
        let local = column("TIMESTAMP WITH LOCAL TIME ZONE", None, Some(4099));
        assert_eq!(local.arrow, Arrow::TimestampUtc);
        assert!(
            local
                .projection("\"C\"")
                .unwrap()
                .contains("AT TIME ZONE '+00:00'")
        );
        assert_eq!(
            column("INTERVAL DAY TO SECOND", Some(2), Some(6)).arrow,
            Arrow::Text
        );
        assert_eq!(column("BLOB", None, None).arrow, Arrow::Binary);
        assert_eq!(
            column("REAL", Some(24), Some(0)).projection("R").unwrap(),
            "CAST(R AS DOUBLE)"
        );
        for refused in ["BFILE", "CLASS234", "ST_GEOMETRY"] {
            assert!(Column::classify("C", refused, None, None, true).is_err());
        }
        assert!(column("VARCHAR", None, None).key_admitted());
        assert!(!column("CLOB", None, None).key_admitted());
        assert!(!column("TIMESTAMP", None, Some(6)).key_admitted());
    }

    #[test]
    fn decimal_text_is_exact_or_refused() {
        assert_eq!(decimal("0.14285714", 31, 8), Ok(14_285_714));
        assert_eq!(decimal("-12345678.91", 10, 2), Ok(-1_234_567_891));
        assert_eq!(decimal("-0.0000010000", 20, 10), Ok(-10_000));
        assert_eq!(decimal("5", 3, 2), Ok(500));
        assert_eq!(decimal(".5", 3, 2), Ok(50));
        assert!(decimal("0.125", 5, 2).is_err());
        assert!(decimal("1000", 3, 0).is_err());
        assert!(decimal("1E+2", 5, 0).is_err());
        assert!(decimal("", 5, 0).is_err());
        assert!(decimal_key("1;DROP").is_err());
        assert_eq!(decimal(&"9".repeat(38), 38, 0), Ok(10i128.pow(38) - 1));
    }

    #[test]
    fn temporal_text_is_calendar_checked() {
        assert_eq!(date("1970-01-01"), Ok(0));
        assert_eq!(date("2000-02-29"), Ok(11_016));
        assert_eq!(date("1969-12-31"), Ok(-1));
        assert!(date("2001-02-29").is_err());
        assert!(date("2026-13-01").is_err());
        assert_eq!(timestamp("1970-01-01 00:00:01.000001"), Ok(1_000_001));
        assert_eq!(
            timestamp("2026-11-04 19:32:03.123456"),
            Ok(1_793_820_723_123_456)
        );
        assert_eq!(timestamp("1969-12-31 23:59:59.999999"), Ok(-1));
        assert!(timestamp("2026-11-04 24:00:00.000000").is_err());
        assert!(timestamp("2026-11-04 19:32:03.1234567").is_err());
    }

    #[test]
    fn batch_types_values_and_refuses_bad_cells() {
        let columns = vec![
            Column::classify("ID", "BIGINT", None, None, false).unwrap(),
            column("DECIMAL", Some(31), Some(8)),
            column("DATETIME WITH TIME ZONE", None, Some(6)),
            column("BLOB", None, None),
            column("BIT", None, None),
            column("DATE", None, None),
        ];
        let row = vec![
            Some(i64::MIN.to_string()),
            Some("0.14285714".into()),
            Some("2026-11-04 19:32:03.123456".into()),
            Some("00ff10".into()),
            Some("1".into()),
            Some("2026-11-05".into()),
        ];
        let batch = batch(
            &columns,
            &[
                row.clone(),
                vec![Some("1".into()), None, None, None, None, None],
            ],
        )
        .unwrap();
        let schema = batch.schema();
        assert_eq!(schema.field(1).data_type(), &DataType::Decimal128(31, 8));
        assert_eq!(
            schema.field(2).data_type(),
            &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
        );
        let bytes = batch
            .column(3)
            .as_any()
            .downcast_ref::<BinaryArray>()
            .unwrap();
        assert_eq!(bytes.value(0), [0, 255, 16]);
        assert!(bytes.is_null(1));
        let mut bad = row.clone();
        bad[0] = None;
        assert!(super::batch(&columns, &[bad]).is_err());
        let mut bad = row.clone();
        bad[3] = Some("0g".into());
        assert!(super::batch(&columns, &[bad]).is_err());
        let mut bad = row;
        bad[4] = Some("True".into());
        assert!(super::batch(&columns, &[bad]).is_err());
        assert_eq!(super::batch(&columns, &[]).unwrap().num_columns(), 6);
    }
}
