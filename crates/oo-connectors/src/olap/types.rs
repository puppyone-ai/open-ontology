//! Column mapping and Arrow encoding for the OLAP engines.
//!
//! Values travel as text over the MySQL wire protocol. Each column has one
//! select expression chosen so that the text is exact (shortest round-trip
//! floats, canonical decimals, epoch micros for ClickHouse `DateTime`), and
//! one closed Arrow type that the Agent DB loader accepts. Types without an
//! exact closed mapping are carried as declared engine text
//! (`oo.text_rendering`) or excluded; nothing is stringified silently.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{
    ArrayRef, BinaryArray, BooleanArray, Date32Array, Decimal128Array, Float64Array, Int8Array,
    Int16Array, Int32Array, Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray,
    UInt8Array, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};

use super::sql::{KeyKind, ident};
use crate::SourceError;

type Result<T> = std::result::Result<T, SourceError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt64,
    /// Float32 text parsed as f32, widened exactly to f64.
    F32,
    F64,
    Decimal(u8, i8),
    Date,
    /// Civil timestamp without zone, microseconds.
    TsNaive,
    /// ClickHouse `DateTime`: integer epoch seconds.
    TsUtcSeconds,
    /// ClickHouse `DateTime64(p<=6)`: integer epoch microseconds.
    TsUtcMicros,
    Utf8,
    Binary,
}

/// One loaded column.
#[derive(Clone, Debug)]
pub(crate) struct Col {
    pub name: String,
    pub source_type: String,
    pub kind: Kind,
    pub nullable: bool,
    /// SQL that produces the exact text for `kind`.
    pub select: String,
    /// SQL used as a deterministic tie-breaker in `ORDER BY`, if orderable.
    pub order: Option<String>,
    /// Doris/StarRocks: SQL rendering the value as text inside the C5 digest.
    pub digest: String,
    /// Keyset comparison kind and literal cast when the column can be a key.
    pub key: Option<(KeyKind, String)>,
    /// ClickHouse non-Nullable columns store a default for a missing value.
    pub default_for_missing: bool,
    /// Declared text rendering for types without a closed Arrow mapping.
    pub text_rendering: Option<&'static str>,
}

/// Mapping result: a loadable column or an explicit exclusion.
pub(crate) enum Mapped {
    Col(Col),
    Excluded,
}

fn split_args(inner: &str) -> Vec<&str> {
    let (mut depth, mut start, mut out) = (0i32, 0usize, Vec::new());
    let mut quoted = false;
    for (i, c) in inner.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                out.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(inner[start..].trim());
    out
}

fn unwrap_call<'a>(ty: &'a str, name: &str) -> Option<&'a str> {
    ty.strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')
}

fn decimal_args(args: &[&str]) -> Option<(u32, u32)> {
    match args {
        [p, s] => Some((p.parse().ok()?, s.parse().ok()?)),
        [p] => Some((p.parse().ok()?, 0)),
        _ => None,
    }
}

fn decimal_kind(p: u32, s: u32) -> Option<Kind> {
    ((1..=38).contains(&p) && s <= p).then_some(Kind::Decimal(p as u8, s as i8))
}

/// ClickHouse type string from `system.columns.type`.
pub(crate) fn clickhouse(name: &str, ty: &str) -> Result<Mapped> {
    let q = ident(name)?;
    let mut inner = ty.trim();
    let mut nullable = false;
    loop {
        if let Some(t) = unwrap_call(inner, "Nullable") {
            nullable = true;
            inner = t.trim();
        } else if let Some(t) = unwrap_call(inner, "LowCardinality") {
            inner = t.trim();
        } else if let Some(t) = unwrap_call(inner, "SimpleAggregateFunction") {
            let args = split_args(t);
            inner = args.get(1).copied().unwrap_or("");
        } else {
            break;
        }
    }
    let base = inner.split('(').next().unwrap_or("");
    let text = |rendering: &'static str| Col {
        name: name.into(),
        source_type: ty.into(),
        kind: Kind::Utf8,
        nullable,
        select: format!("toString({q})"),
        order: Some(format!("toString({q})")),
        digest: format!("toString({q})"),
        key: None,
        default_for_missing: !nullable,
        text_rendering: Some(rendering),
    };
    let plain = |kind: Kind, select: String, key: Option<KeyKind>| Col {
        name: name.into(),
        source_type: ty.into(),
        kind,
        nullable,
        digest: select.clone(),
        select,
        order: Some(q.clone()),
        key: key.map(|k| (k, inner.to_string())),
        default_for_missing: !nullable,
        text_rendering: None,
    };
    let col = match base {
        "UInt8" => plain(Kind::UInt8, q.clone(), Some(KeyKind::Int)),
        "UInt16" => plain(Kind::Int32, q.clone(), Some(KeyKind::Int)),
        "UInt32" => plain(Kind::Int64, q.clone(), Some(KeyKind::Int)),
        "UInt64" => plain(Kind::UInt64, q.clone(), Some(KeyKind::Int)),
        "Int8" => plain(Kind::Int8, q.clone(), Some(KeyKind::Int)),
        "Int16" => plain(Kind::Int16, q.clone(), Some(KeyKind::Int)),
        "Int32" => plain(Kind::Int32, q.clone(), Some(KeyKind::Int)),
        "Int64" => plain(Kind::Int64, q.clone(), Some(KeyKind::Int)),
        "Int128" | "Int256" | "UInt128" | "UInt256" => text("integer_text"),
        "Float32" => plain(Kind::F32, format!("toString({q})"), None),
        "Float64" => plain(Kind::F64, format!("toString({q})"), None),
        "Bool" => plain(Kind::Bool, format!("toUInt8({q})"), None),
        "Decimal" | "Decimal32" | "Decimal64" | "Decimal128" | "Decimal256" => {
            let args = unwrap_call(inner, base).map(split_args).unwrap_or_default();
            let (p, s) = match base {
                "Decimal" => decimal_args(&args),
                "Decimal32" => args.first().and_then(|s| s.parse().ok()).map(|s| (9, s)),
                "Decimal64" => args.first().and_then(|s| s.parse().ok()).map(|s| (18, s)),
                "Decimal128" => args.first().and_then(|s| s.parse().ok()).map(|s| (38, s)),
                _ => args.first().and_then(|s| s.parse().ok()).map(|s| (76, s)),
            }
            .ok_or_else(|| SourceError::catalog("clickhouse_decimal_type"))?;
            match decimal_kind(p, s) {
                Some(kind) => plain(kind, format!("toString({q})"), Some(KeyKind::Decimal)),
                None => text("decimal_text"),
            }
        }
        "Date" | "Date32" => plain(Kind::Date, format!("toString({q})"), Some(KeyKind::Text)),
        "DateTime" => {
            let mut c = plain(Kind::TsUtcSeconds, format!("toUnixTimestamp({q})"), None);
            c.key = Some((KeyKind::Text, inner.into()));
            c
        }
        "DateTime64" => {
            let args = unwrap_call(inner, base).map(split_args).unwrap_or_default();
            let precision: u32 = args
                .first()
                .and_then(|p| p.parse().ok())
                .ok_or_else(|| SourceError::catalog("clickhouse_datetime64_type"))?;
            if precision <= 6 {
                let mut c = plain(
                    Kind::TsUtcMicros,
                    format!("toUnixTimestamp64Micro({q})"),
                    None,
                );
                c.key = Some((KeyKind::Text, inner.into()));
                c
            } else {
                text("datetime64_text_sub_micro")
            }
        }
        "String" | "FixedString" => {
            let mut c = plain(Kind::Binary, q.clone(), Some(KeyKind::Bytes));
            c.key = Some((KeyKind::Bytes, inner.into()));
            c
        }
        "UUID" | "IPv4" | "IPv6" | "Enum8" | "Enum16" | "Enum" => text("engine_text"),
        "AggregateFunction" | "Nothing" => return Ok(Mapped::Excluded),
        "Array" | "Map" | "Tuple" | "Nested" | "JSON" | "Object" | "Variant" | "Dynamic"
        | "Point" | "Ring" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon"
        | "Time" | "Time64" => {
            let mut c = text("engine_text");
            if matches!(
                base,
                "Array" | "Map" | "Tuple" | "Nested" | "JSON" | "Object" | "Variant" | "Dynamic"
            ) {
                c.default_for_missing = !nullable;
            }
            c
        }
        b if b.starts_with("Interval") => text("engine_text"),
        _ => return Err(SourceError::catalog("clickhouse_unsupported_type")),
    };
    Ok(Mapped::Col(col))
}

/// Doris / StarRocks type string from `DESC` (lowercased).
pub(crate) fn mpp(name: &str, ty: &str, nullable: bool, doris: bool) -> Result<Mapped> {
    let q = ident(name)?;
    let lower = ty.trim().to_ascii_lowercase();
    let base = lower.split(['(', '<']).next().unwrap_or("").trim();
    let col = |kind: Kind, key: Option<KeyKind>, rendering: Option<&'static str>| Col {
        name: name.into(),
        source_type: ty.into(),
        kind,
        nullable,
        select: q.clone(),
        order: rendering.is_none().then(|| q.clone()),
        digest: format!("CAST({q} AS STRING)"),
        key: key.map(|k| {
            (
                k,
                if k == KeyKind::Int {
                    String::new()
                } else {
                    lower.clone()
                },
            )
        }),
        default_for_missing: false,
        text_rendering: rendering,
    };
    let mapped = match base {
        "boolean" | "bool" => col(Kind::Bool, None, None),
        "tinyint" => col(Kind::Int8, Some(KeyKind::Int), None),
        "smallint" => col(Kind::Int16, Some(KeyKind::Int), None),
        "int" | "integer" => col(Kind::Int32, Some(KeyKind::Int), None),
        "bigint" => col(Kind::Int64, Some(KeyKind::Int), None),
        "largeint" => {
            let mut c = col(Kind::Utf8, Some(KeyKind::Int), Some("integer_text"));
            c.order = Some(q.clone());
            c
        }
        "float" => col(Kind::F32, None, None),
        "double" => col(Kind::F64, None, None),
        "decimal" | "decimalv2" | "decimalv3" | "decimal32" | "decimal64" | "decimal128"
        | "decimal256" => {
            let args = lower
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .map(|a| a.split(',').map(str::trim).collect::<Vec<_>>())
                .unwrap_or_default();
            let (p, s) =
                decimal_args(&args).ok_or_else(|| SourceError::catalog("mpp_decimal_type"))?;
            match decimal_kind(p, s) {
                Some(kind) => col(kind, Some(KeyKind::Decimal), None),
                None => {
                    let mut c = col(Kind::Utf8, Some(KeyKind::Decimal), Some("decimal_text"));
                    c.order = Some(q.clone());
                    c
                }
            }
        }
        "date" | "datev2" => col(Kind::Date, Some(KeyKind::Text), None),
        "datetime" | "datetimev2" => col(Kind::TsNaive, Some(KeyKind::Text), None),
        "char" | "varchar" | "string" | "text" => {
            let mut c = col(Kind::Utf8, Some(KeyKind::Text), None);
            // Compared as strings by the source; no cast needed or wanted.
            c.key = Some((KeyKind::Text, String::new()));
            c
        }
        "varbinary" | "binary" => {
            // StarRocks refuses ORDER BY on binary values; no tie-breaker.
            let mut c = col(Kind::Binary, None, None);
            c.order = None;
            c
        }
        "ipv4" | "ipv6" => {
            let mut c = col(Kind::Utf8, None, Some("engine_text"));
            c.order = Some(q.clone());
            c
        }
        "json" | "jsonb" | "variant" => col(Kind::Utf8, None, Some("engine_text")),
        "array" | "map" | "struct" => {
            let mut c = col(Kind::Utf8, None, Some("engine_text"));
            // Neither engine casts nested types to STRING. Doris casts them to
            // JSON; StarRocks renders JSON for structs, so wrap the value.
            c.digest = if doris {
                format!("CAST(CAST({q} AS JSON) AS STRING)")
            } else {
                format!("CAST(to_json(named_struct('v', {q})) AS STRING)")
            };
            c
        }
        "bitmap" | "hll" | "quantile_state" | "agg_state" | "percentile" | "object" => {
            return Ok(Mapped::Excluded);
        }
        _ => return Err(SourceError::catalog("mpp_unsupported_type")),
    };
    Ok(Mapped::Col(mapped))
}

pub(crate) fn field(col: &Col) -> Field {
    let data_type = match col.kind {
        Kind::Bool => DataType::Boolean,
        Kind::Int8 => DataType::Int8,
        Kind::Int16 => DataType::Int16,
        Kind::Int32 => DataType::Int32,
        Kind::Int64 => DataType::Int64,
        Kind::UInt8 => DataType::UInt8,
        Kind::UInt64 => DataType::UInt64,
        Kind::F32 | Kind::F64 => DataType::Float64,
        Kind::Decimal(p, s) => DataType::Decimal128(p, s),
        Kind::Date => DataType::Date32,
        Kind::TsNaive => DataType::Timestamp(TimeUnit::Microsecond, None),
        Kind::TsUtcSeconds | Kind::TsUtcMicros => {
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
        }
        Kind::Utf8 => DataType::Utf8,
        Kind::Binary => DataType::Binary,
    };
    let mut metadata = HashMap::from([("oo.source_type".to_string(), col.source_type.clone())]);
    if col.default_for_missing {
        metadata.insert("oo.default_for_missing".into(), "true".into());
    }
    if let Some(rendering) = col.text_rendering {
        metadata.insert("oo.text_rendering".into(), rendering.into());
    }
    Field::new(&col.name, data_type, col.nullable).with_metadata(metadata)
}

fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| SourceError::protocol("olap_non_utf8_text"))
}

fn parse<T: std::str::FromStr>(bytes: &[u8]) -> Result<T> {
    utf8(bytes)?
        .trim()
        .parse()
        .map_err(|_| SourceError::protocol("olap_value_parse"))
}

fn parse_bool(bytes: &[u8]) -> Result<bool> {
    match bytes {
        b"1" | b"true" | b"TRUE" => Ok(true),
        b"0" | b"false" | b"FALSE" => Ok(false),
        _ => Err(SourceError::protocol("olap_value_parse")),
    }
}

/// Canonical decimal text to an unscaled integer at `scale`, refusing
/// precision overflow and fractional digits beyond the scale.
pub(crate) fn parse_decimal(text: &str, precision: u8, scale: i8) -> Result<i128> {
    let bad = || SourceError::protocol("olap_decimal_parse");
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if int.is_empty()
        || !int.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(bad());
    }
    let scale = usize::try_from(scale).map_err(|_| bad())?;
    let frac = frac.trim_end_matches('0');
    if frac.len() > scale {
        return Err(bad());
    }
    let int = int.trim_start_matches('0');
    if int.len() + scale > precision as usize {
        return Err(bad());
    }
    let digits = format!("{int}{frac}{}", "0".repeat(scale - frac.len()));
    let value: i128 = if digits.is_empty() {
        0
    } else {
        digits.parse().map_err(|_| bad())?
    };
    Ok(if negative { -value } else { value })
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn digits(text: &str, len: usize) -> Option<i64> {
    (text.len() == len && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

pub(crate) fn parse_date(text: &str) -> Result<i32> {
    let bad = || SourceError::protocol("olap_date_parse");
    let mut parts = text.split('-');
    let (y, m, d) = (
        parts.next().and_then(|p| digits(p, 4)).ok_or_else(bad)?,
        parts.next().and_then(|p| digits(p, 2)).ok_or_else(bad)?,
        parts.next().and_then(|p| digits(p, 2)).ok_or_else(bad)?,
    );
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let max = [
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
    if parts.next().is_some() || !(1..=12).contains(&m) || d < 1 || d > max[(m - 1) as usize] {
        return Err(bad());
    }
    i32::try_from(days_from_civil(y, m, d)).map_err(|_| bad())
}

/// `YYYY-MM-DD HH:MM:SS[.f{1,6}]` to microseconds since the civil epoch.
pub(crate) fn parse_datetime(text: &str) -> Result<i64> {
    let bad = || SourceError::protocol("olap_datetime_parse");
    let (date, time) = text.split_once(' ').ok_or_else(bad)?;
    let days = i64::from(parse_date(date)?);
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut parts = hms.split(':');
    let (h, mi, s) = (
        parts.next().and_then(|p| digits(p, 2)).ok_or_else(bad)?,
        parts.next().and_then(|p| digits(p, 2)).ok_or_else(bad)?,
        parts.next().and_then(|p| digits(p, 2)).ok_or_else(bad)?,
    );
    if parts.next().is_some() || h > 23 || mi > 59 || s > 59 || frac.len() > 6 {
        return Err(bad());
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let micros: i64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<6}").parse().map_err(|_| bad())?
    };
    Ok(((days * 24 + h) * 60 + mi) * 60_000_000 + s * 1_000_000 + micros)
}

/// Build one Arrow batch from text cells. `cells[row][col]`, `None` is NULL.
pub(crate) fn batch(cols: &[Col], cells: &[Vec<Option<Vec<u8>>>]) -> Result<RecordBatch> {
    let schema = Arc::new(Schema::new(cols.iter().map(field).collect::<Vec<_>>()));
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(cols.len());
    for (i, col) in cols.iter().enumerate() {
        let values: Vec<Option<&[u8]>> = cells
            .iter()
            .map(|row| row.get(i).and_then(|v| v.as_deref()))
            .collect();
        if !col.nullable && values.iter().any(Option::is_none) {
            return Err(SourceError::protocol("olap_null_in_not_null"));
        }
        fn each<T>(
            values: &[Option<&[u8]>],
            f: impl Fn(&[u8]) -> Result<T>,
        ) -> Result<Vec<Option<T>>> {
            values.iter().map(|v| v.map(&f).transpose()).collect()
        }
        let array: ArrayRef = match col.kind {
            Kind::Bool => Arc::new(BooleanArray::from(each(&values, parse_bool)?)),
            Kind::Int8 => Arc::new(Int8Array::from(each(&values, parse::<i8>)?)),
            Kind::Int16 => Arc::new(Int16Array::from(each(&values, parse::<i16>)?)),
            Kind::Int32 => Arc::new(Int32Array::from(each(&values, parse::<i32>)?)),
            Kind::Int64 => Arc::new(Int64Array::from(each(&values, parse::<i64>)?)),
            Kind::UInt8 => Arc::new(UInt8Array::from(each(&values, parse::<u8>)?)),
            Kind::UInt64 => Arc::new(UInt64Array::from(each(&values, parse::<u64>)?)),
            Kind::F32 => Arc::new(Float64Array::from(each(&values, |b| {
                parse::<f32>(b).map(f64::from)
            })?)),
            Kind::F64 => Arc::new(Float64Array::from(each(&values, parse::<f64>)?)),
            Kind::Decimal(p, s) => Arc::new(
                Decimal128Array::from(each(&values, |b| parse_decimal(utf8(b)?, p, s))?)
                    .with_precision_and_scale(p, s)
                    .map_err(|_| SourceError::protocol("olap_decimal_parse"))?,
            ),
            Kind::Date => Arc::new(Date32Array::from(each(&values, |b| parse_date(utf8(b)?))?)),
            Kind::TsNaive => Arc::new(TimestampMicrosecondArray::from(each(&values, |b| {
                parse_datetime(utf8(b)?)
            })?)),
            Kind::TsUtcSeconds => Arc::new(
                TimestampMicrosecondArray::from(each(&values, |b| {
                    parse::<i64>(b)?
                        .checked_mul(1_000_000)
                        .ok_or_else(|| SourceError::protocol("olap_datetime_parse"))
                })?)
                .with_timezone("UTC"),
            ),
            Kind::TsUtcMicros => Arc::new(
                TimestampMicrosecondArray::from(each(&values, parse::<i64>)?).with_timezone("UTC"),
            ),
            Kind::Utf8 => Arc::new(StringArray::from(
                values
                    .iter()
                    .map(|v| v.map(utf8).transpose())
                    .collect::<Result<Vec<Option<&str>>>>()?,
            )),
            Kind::Binary => Arc::new(BinaryArray::from_opt_vec(values.clone())),
        };
        arrays.push(array);
    }
    RecordBatch::try_new(schema, arrays).map_err(|_| SourceError::protocol("olap_arrow_batch"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Array;

    fn ch(ty: &str) -> Col {
        match clickhouse("c", ty).unwrap() {
            Mapped::Col(c) => c,
            Mapped::Excluded => panic!("excluded {ty}"),
        }
    }

    #[test]
    fn clickhouse_types_map_to_closed_arrow_types() {
        assert_eq!(ch("UInt64").kind, Kind::UInt64);
        assert_eq!(ch("UInt32").kind, Kind::Int64);
        let n = ch("Nullable(Decimal(12, 2))");
        assert_eq!(
            (n.kind, n.nullable, n.default_for_missing),
            (Kind::Decimal(12, 2), true, false)
        );
        assert_eq!(ch("Decimal64(4)").kind, Kind::Decimal(18, 4));
        assert_eq!(ch("Decimal(50, 10)").text_rendering, Some("decimal_text"));
        assert_eq!(ch("Int128").text_rendering, Some("integer_text"));
        let lc = ch("LowCardinality(Nullable(String))");
        assert_eq!((lc.kind, lc.nullable), (Kind::Binary, true));
        assert_eq!(ch("DateTime('UTC')").kind, Kind::TsUtcSeconds);
        assert_eq!(
            ch("DateTime64(3, 'UTC')").select,
            "toUnixTimestamp64Micro(`c`)"
        );
        assert_eq!(
            ch("DateTime64(9)").text_rendering,
            Some("datetime64_text_sub_micro")
        );
        assert!(ch("String").default_for_missing);
        assert_eq!(ch("Map(String, UInt8)").text_rendering, Some("engine_text"));
        assert_eq!(ch("SimpleAggregateFunction(max, Int64)").kind, Kind::Int64);
        assert!(matches!(
            clickhouse("c", "AggregateFunction(uniq, UInt64)").unwrap(),
            Mapped::Excluded
        ));
        assert!(clickhouse("c", "Unknown42").is_err());
    }

    #[test]
    fn mpp_types_map_and_exclude_aggregate_states() {
        let m = |ty: &str| match mpp("c", ty, true, true).unwrap() {
            Mapped::Col(c) => c,
            Mapped::Excluded => panic!("excluded"),
        };
        assert_eq!(m("decimal(12,2)").kind, Kind::Decimal(12, 2));
        assert_eq!(m("decimalv3(12, 2)").key.unwrap().1, "decimalv3(12, 2)");
        assert_eq!(m("decimal(76,10)").text_rendering, Some("decimal_text"));
        assert_eq!(m("largeint").key.unwrap().0, KeyKind::Int);
        assert_eq!(m("datetime(6)").kind, Kind::TsNaive);
        assert_eq!(m("array<int>").text_rendering, Some("engine_text"));
        assert!(m("json").order.is_none());
        for excluded in ["bitmap", "HLL", "quantile_state", "agg_state<sum(int)>"] {
            assert!(matches!(
                mpp("c", excluded, true, false).unwrap(),
                Mapped::Excluded
            ));
        }
        assert!(mpp("c", "geometry", true, false).is_err());
        assert_eq!(m("array<int>").digest, "CAST(CAST(`c` AS JSON) AS STRING)");
        let Mapped::Col(sr) = mpp("c", "map<varchar(8),int>", true, false).unwrap() else {
            panic!("map")
        };
        assert_eq!(sr.digest, "CAST(to_json(named_struct('v', `c`)) AS STRING)");
    }

    #[test]
    fn values_parse_exactly() {
        assert_eq!(parse_decimal("12.5", 12, 2).unwrap(), 1250);
        assert_eq!(parse_decimal("-0.01", 12, 2).unwrap(), -1);
        assert_eq!(parse_decimal("0", 12, 2).unwrap(), 0);
        assert!(parse_decimal("1.234", 12, 2).is_err());
        assert!(parse_decimal("12345678901", 12, 2).is_err());
        assert!(parse_decimal("1e5", 12, 2).is_err());
        assert_eq!(parse_date("1970-01-01").unwrap(), 0);
        assert_eq!(parse_date("2024-02-29").unwrap(), 19782);
        assert_eq!(parse_date("1969-12-31").unwrap(), -1);
        assert!(parse_date("2023-02-29").is_err());
        assert_eq!(parse_datetime("1970-01-01 00:00:01.5").unwrap(), 1_500_000);
        assert_eq!(
            parse_datetime("2024-01-05 01:02:03.123456").unwrap(),
            1_704_416_523_123_456
        );
        assert!(parse_datetime("2024-01-05T01:02:03").is_err());
    }

    #[test]
    fn batches_keep_floats_bytes_and_nulls() {
        let cols = vec![ch("Float32"), ch("Nullable(String)"), ch("Bool")];
        let rows = vec![
            vec![
                Some(b"3.4e38".to_vec()),
                Some(vec![0x61, 0xff]),
                Some(b"1".to_vec()),
            ],
            vec![Some(b"-inf".to_vec()), None, Some(b"0".to_vec())],
        ];
        let b = batch(&cols, &rows).unwrap();
        let f = b.column(0).as_any().downcast_ref::<Float64Array>().unwrap();
        assert_eq!(f.value(0), f64::from(3.4e38f32));
        assert!(f.value(1).is_infinite());
        let s = b.column(1).as_any().downcast_ref::<BinaryArray>().unwrap();
        assert_eq!(s.value(0), [0x61, 0xff]);
        assert!(s.is_null(1));
        assert_eq!(
            b.schema().field(1).metadata()["oo.source_type"],
            "Nullable(String)"
        );
        let empty = batch(&cols, &[]).unwrap();
        assert_eq!((empty.num_rows(), empty.num_columns()), (0, 3));
        assert!(batch(&cols, &[vec![None, None, None]]).is_err());
    }
}
