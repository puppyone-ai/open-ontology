//! Typed cells for the MariaDB fixture. Plans match the sealed-import catalog
//! and the copy encoder. Out-of-range TIME is a row reject, not a stored clock.

use oo_connectors::types::{ColumnMapping, NUMERIC_TYPMOD_VERSION, TYPE_CONVERSION_PROTOCOL};
use oo_core::api::payload_sha256;
use oo_core::canonical::{LogicalType, LogicalValue};
use oo_core::sealed_import::{push_schema_column, push_schema_table};
use oo_core::{ExactDecimal, ExactInt, TYPE_POLICY_V1};
use oo_pipeline::copy_encoder::{
    ArrowPhysical, ArrowScalar, CellPresence, ColumnPlan, CopyBudgets, CopyColumn, CopyEncoder,
    CopyError, CopyPiece, DecodedCell, MapDecision, RunBinding, TypedBatch, UnavailablePolicy,
    assemble_batch,
};

pub const SCHEMA: &str = "oo_synth";
pub const SOURCE_ID: &str = "mariadb_w39";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Numeric { scale: i32 },
    Text,
    Json,
    Hex,
    Date,
    Time,
    Timestamp { utc: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Store {
    Numeric { precision: u16, scale: i16 },
    Int { width: u8, oid: i64 },
    Varchar { chars: i32 },
    Json,
    Bytes,
    Date,
    Time,
    Timestamp,
    Timestamptz,
}

#[derive(Clone, Debug)]
pub struct ColSpec {
    pub name: String,
    pub attnum: i32,
    pub key: bool,
    pub nullable: bool,
    pub caveat: Option<String>,
    pub source_type: String,
    pub compare: Compare,
    store: Store,
}

impl ColSpec {
    pub fn oid(&self) -> i64 {
        match self.store {
            Store::Numeric { .. } => 1700,
            Store::Int { oid, .. } => oid,
            Store::Varchar { .. } => 1043,
            Store::Json => 114,
            Store::Bytes => 17,
            Store::Date => 1082,
            Store::Time => 1083,
            Store::Timestamp => 1114,
            Store::Timestamptz => 1184,
        }
    }

    pub fn typmod(&self) -> i32 {
        match self.store {
            Store::Numeric { precision, scale } => {
                numeric_typmod(i32::from(precision), i32::from(scale))
            }
            Store::Varchar { chars } => varchar_typmod(chars),
            Store::Time | Store::Timestamp | Store::Timestamptz => 6,
            _ => -1,
        }
    }

    pub fn pg_type(&self) -> &'static str {
        match self.store {
            Store::Numeric { .. } => "numeric",
            Store::Int { width: 8, .. } => "int8",
            Store::Int { width: 4, .. } => "int4",
            Store::Int { .. } => "int",
            Store::Varchar { .. } => "varchar",
            Store::Json => "json",
            Store::Bytes => "bytea",
            Store::Date => "date",
            Store::Time => "time",
            Store::Timestamp => "timestamp",
            Store::Timestamptz => "timestamptz",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TableSpec {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColSpec>,
}

pub fn fixture_tables() -> Vec<TableSpec> {
    vec![
        table(
            "k_unsigned",
            vec![
                numeric(
                    "id",
                    1,
                    20,
                    0,
                    true,
                    false,
                    (Some("bigint_unsigned_numeric"), "mariadb_bigint_unsigned"),
                ),
                varchar("label", 2, 64, false, false, "mariadb_varchar"),
            ],
        ),
        table(
            "k_decimal",
            vec![
                numeric("id", 1, 38, 10, true, false, (None, "mariadb_decimal")),
                numeric(
                    "payload",
                    2,
                    65,
                    30,
                    false,
                    false,
                    (None, "mariadb_decimal"),
                ),
            ],
        ),
        table(
            "k_null",
            vec![
                int8("id", 1, true, false, "mariadb_bigint"),
                varchar("note", 2, 64, false, true, "mariadb_varchar"),
                int4("qty", 3, false, true, "mariadb_int"),
                numeric("money", 4, 12, 2, false, true, (None, "mariadb_decimal")),
                json_col("payload", 5, true),
                timestamp("when_at", 6, false, true, "mariadb_datetime"),
                bytes("blob_key", 7, false, true, "mariadb_varbinary"),
            ],
        ),
        table(
            "k_null_unique",
            vec![
                int8("id", 1, true, false, "mariadb_bigint"),
                varchar("code", 2, 32, false, true, "mariadb_varchar"),
            ],
        ),
        table(
            "k_json",
            vec![
                int8("id", 1, true, false, "mariadb_bigint"),
                json_col("doc", 2, true),
            ],
        ),
        table(
            "k_temporal",
            vec![
                timestamp("id", 1, true, false, "mariadb_datetime"),
                date_col("d", 2, false, false, "mariadb_date"),
                time_col("t", 3),
                timestamptz("ts", 4, true, "mariadb_timestamp"),
                varchar("note", 5, 64, false, false, "mariadb_varchar"),
            ],
        ),
        table(
            "k_binary",
            vec![
                bytes("id", 1, true, false, "mariadb_varbinary"),
                varchar("label", 2, 64, false, false, "mariadb_varchar"),
            ],
        ),
        table(
            "k_zero_probe",
            vec![
                int4("id", 1, true, false, "mariadb_int"),
                date_col("d", 2, false, false, "mariadb_date"),
            ],
        ),
    ]
}

fn table(name: &str, columns: Vec<ColSpec>) -> TableSpec {
    TableSpec {
        schema: SCHEMA.to_string(),
        name: name.to_string(),
        columns,
    }
}

fn numeric(
    name: &str,
    attnum: i32,
    precision: u16,
    scale: i16,
    key: bool,
    nullable: bool,
    mark: (Option<&str>, &str),
) -> ColSpec {
    let (caveat, source_type) = mark;
    ColSpec {
        name: name.to_string(),
        attnum,
        key,
        nullable,
        caveat: caveat.map(str::to_string),
        source_type: source_type.to_string(),
        compare: Compare::Numeric {
            scale: i32::from(scale),
        },
        store: Store::Numeric { precision, scale },
    }
}

fn int8(name: &str, attnum: i32, key: bool, nullable: bool, source_type: &str) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key,
        nullable,
        caveat: None,
        source_type: source_type.to_string(),
        compare: Compare::Text,
        store: Store::Int { width: 8, oid: 20 },
    }
}

fn int4(name: &str, attnum: i32, key: bool, nullable: bool, source_type: &str) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key,
        nullable,
        caveat: None,
        source_type: source_type.to_string(),
        compare: Compare::Text,
        store: Store::Int { width: 4, oid: 23 },
    }
}

fn varchar(
    name: &str,
    attnum: i32,
    chars: i32,
    key: bool,
    nullable: bool,
    source_type: &str,
) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key,
        nullable,
        caveat: None,
        source_type: source_type.to_string(),
        compare: Compare::Text,
        store: Store::Varchar { chars },
    }
}

fn json_col(name: &str, attnum: i32, nullable: bool) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key: false,
        nullable,
        caveat: Some("json_not_jsonb".to_string()),
        source_type: "longtext_json_valid".to_string(),
        compare: Compare::Json,
        store: Store::Json,
    }
}

fn bytes(name: &str, attnum: i32, key: bool, nullable: bool, source_type: &str) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key,
        nullable,
        caveat: None,
        source_type: source_type.to_string(),
        compare: Compare::Hex,
        store: Store::Bytes,
    }
}

fn date_col(name: &str, attnum: i32, key: bool, nullable: bool, source_type: &str) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key,
        nullable,
        caveat: None,
        source_type: source_type.to_string(),
        compare: Compare::Date,
        store: Store::Date,
    }
}

fn time_col(name: &str, attnum: i32) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key: false,
        nullable: false,
        caveat: None,
        source_type: "mariadb_time".to_string(),
        compare: Compare::Time,
        store: Store::Time,
    }
}

fn timestamp(name: &str, attnum: i32, key: bool, nullable: bool, source_type: &str) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key,
        nullable,
        caveat: None,
        source_type: source_type.to_string(),
        compare: Compare::Timestamp { utc: false },
        store: Store::Timestamp,
    }
}

fn timestamptz(name: &str, attnum: i32, nullable: bool, source_type: &str) -> ColSpec {
    ColSpec {
        name: name.to_string(),
        attnum,
        key: false,
        nullable,
        caveat: None,
        source_type: source_type.to_string(),
        compare: Compare::Timestamp { utc: true },
        store: Store::Timestamptz,
    }
}

pub fn from_decision(
    name: &str,
    attnum: i32,
    key: bool,
    nullable: bool,
    column_type: &str,
    chars: Option<i64>,
    plan: &mariadb_probe::decide::Plan,
) -> Result<ColSpec, String> {
    use mariadb_probe::decide::Kind;
    match &plan.kind {
        Kind::Int {
            bits: 64,
            unsigned: true,
        } => Ok(numeric(
            name,
            attnum,
            20,
            0,
            key,
            nullable,
            (Some("bigint_unsigned_numeric"), "mariadb_bigint_unsigned"),
        )),
        Kind::Int {
            bits: 64,
            unsigned: false,
        } => Ok(int8(name, attnum, key, nullable, "mariadb_bigint")),
        Kind::Int {
            bits: 32,
            unsigned: true,
        } => Ok(int8(name, attnum, key, nullable, "mariadb_bigint")),
        Kind::Int {
            unsigned: false, ..
        } => Ok(int4(name, attnum, key, nullable, "mariadb_int")),
        Kind::Int { unsigned: true, .. } => Ok(int4(name, attnum, key, nullable, "mariadb_int")),
        Kind::Decimal { precision, scale } => {
            let scale = i16::from(*scale);
            Ok(numeric(
                name,
                attnum,
                u16::from(*precision),
                scale,
                key,
                nullable,
                (None, "mariadb_decimal"),
            ))
        }
        Kind::Utf8 {
            provenance: Some(_),
        } => {
            if key {
                return Err(format!("unsupported_type: {name} json key"));
            }
            Ok(json_col(name, attnum, nullable))
        }
        Kind::Utf8 { provenance: None } => {
            let width = varchar_width(column_type, chars)?;
            Ok(varchar(
                name,
                attnum,
                width,
                key,
                nullable,
                "mariadb_varchar",
            ))
        }
        Kind::Binary => Ok(bytes(name, attnum, key, nullable, "mariadb_varbinary")),
        Kind::Date => Ok(date_col(name, attnum, key, nullable, "mariadb_date")),
        Kind::Datetime => Ok(timestamp(name, attnum, key, nullable, "mariadb_datetime")),
        Kind::Timestamp => {
            if key {
                return Err(format!("unsupported_type: {name} timestamp key"));
            }
            Ok(timestamptz(name, attnum, nullable, "mariadb_timestamp"))
        }
        Kind::TimeInterval => {
            if key {
                return Err(format!("unsupported_type: {name} time key"));
            }
            Ok(time_col(name, attnum))
        }
    }
}

fn varchar_width(column_type: &str, chars: Option<i64>) -> Result<i32, String> {
    if let Some(value) = chars
        && (1..1_000_000).contains(&value)
    {
        return i32::try_from(value).map_err(|_| format!("unsupported_type: {column_type}"));
    }
    let lower = column_type.to_ascii_lowercase();
    let Some(open) = lower.find('(') else {
        return Err(format!("unsupported_type: {column_type}"));
    };
    let Some(close) = lower[open + 1..].find(')') else {
        return Err(format!("unsupported_type: {column_type}"));
    };
    let width: i32 = lower[open + 1..open + 1 + close]
        .parse()
        .map_err(|_| format!("unsupported_type: {column_type}"))?;
    if !(1..1_000_000).contains(&width) {
        return Err(format!("unsupported_type: {column_type}"));
    }
    Ok(width)
}

pub fn numeric_typmod(precision: i32, scale: i32) -> i32 {
    ((precision << 16) | scale) + 4
}

pub fn varchar_typmod(chars: i32) -> i32 {
    chars + 4
}

pub fn schema_preimage(tables: &[TableSpec]) -> String {
    let mut out = String::new();
    for table in tables {
        push_schema_table(&mut out, &table.schema, &table.name);
        for column in &table.columns {
            push_schema_column(
                &mut out,
                &column.name,
                column.oid(),
                column.typmod(),
                column.caveat.as_deref(),
            );
        }
    }
    out
}

pub fn schema_digest(tables: &[TableSpec]) -> String {
    payload_sha256(schema_preimage(tables).as_bytes())
}

pub fn type_preimage(tables: &[TableSpec]) -> String {
    let mut out = String::new();
    for table in tables {
        out.push_str(&table.schema);
        out.push('.');
        out.push_str(&table.name);
        out.push('\n');
        for column in &table.columns {
            out.push_str(&column.name);
            out.push('\t');
            out.push_str(&column.oid().to_string());
            out.push('\t');
            out.push_str(&column.typmod().to_string());
            out.push('\t');
            out.push_str(&column.source_type);
            out.push('\t');
            out.push_str(column.caveat.as_deref().unwrap_or(""));
            out.push('\n');
        }
    }
    out
}

pub fn type_digest(tables: &[TableSpec]) -> String {
    payload_sha256(type_preimage(tables).as_bytes())
}

pub fn copy_budgets() -> CopyBudgets {
    CopyBudgets {
        max_value_bytes: 65_536,
        max_row_bytes: 262_144,
        max_chunk_bytes: 1_048_576,
        max_rows_per_chunk: 1024,
    }
}

pub fn column_plan(column: &ColSpec) -> Result<ColumnPlan, String> {
    let (logical, arrow, precision, scale) = match column.store {
        Store::Numeric { precision, scale } => {
            let physical = if precision <= 38 {
                ArrowPhysical::Decimal128 {
                    precision: u8::try_from(precision).map_err(|_| "precision".to_string())?,
                    scale: i8::try_from(scale).map_err(|_| "scale".to_string())?,
                }
            } else {
                ArrowPhysical::Decimal256 {
                    precision: u8::try_from(precision).map_err(|_| "precision".to_string())?,
                    scale: i8::try_from(scale).map_err(|_| "scale".to_string())?,
                }
            };
            (
                LogicalType::Decimal { precision, scale },
                physical,
                Some(i32::from(precision)),
                Some(i32::from(scale)),
            )
        }
        Store::Int { width: 8, .. } => (LogicalType::Int, ArrowPhysical::Int64, None, None),
        Store::Int { width: 4, .. } => (LogicalType::Int, ArrowPhysical::Int32, None, None),
        Store::Int { .. } => return Err("int width".to_string()),
        Store::Varchar { chars } => (LogicalType::Text, ArrowPhysical::Utf8, Some(chars), None),
        Store::Json => (LogicalType::Text, ArrowPhysical::Utf8, None, None),
        Store::Bytes => (LogicalType::Bytes, ArrowPhysical::Binary, None, None),
        Store::Date => (LogicalType::Date, ArrowPhysical::Date32, None, None),
        Store::Time => (LogicalType::Time, ArrowPhysical::Time64Micros, None, None),
        Store::Timestamp => (
            LogicalType::TimestampNaive,
            ArrowPhysical::TimestampMicrosNaive,
            None,
            None,
        ),
        Store::Timestamptz => (
            LogicalType::TimestampTz,
            ArrowPhysical::TimestampMicrosUtc,
            None,
            None,
        ),
    };
    let arrow_type = arrow.series60()?;
    let numeric_version =
        matches!(column.store, Store::Numeric { .. }).then_some(NUMERIC_TYPMOD_VERSION);
    Ok(ColumnPlan {
        mapping: ColumnMapping {
            protocol: TYPE_CONVERSION_PROTOCOL,
            source_type: column.pg_type().to_string(),
            arrow_type,
            pg_type: column.pg_type().to_string(),
            precision,
            scale,
            catalog_v1_numeric_scale: scale,
            numeric_typmod_version: numeric_version,
            timezone: None,
            nullable: column.nullable,
            type_policy_version: TYPE_POLICY_V1,
            decision: MapDecision::Lossless,
            metadata: Vec::new(),
        },
        attnum: column.attnum,
        name: column.name.to_string(),
        oid: column.oid(),
        decode_oid: column.oid(),
        logical: Some(logical),
        arrow: Some(arrow),
        element: None,
        enum_labels: Vec::new(),
        key: column.key,
    })
}

pub fn plans_for(table: &TableSpec) -> Result<Vec<ColumnPlan>, String> {
    table.columns.iter().map(column_plan).collect()
}

#[derive(Debug)]
pub struct CellFault {
    pub column: String,
    pub reason: &'static str,
    pub value_len: i64,
}

pub fn decode_cell(column: &ColSpec, value: &mysql_async::Value) -> Result<DecodedCell, CellFault> {
    if matches!(value, mysql_async::Value::NULL) {
        if column.nullable && !column.key {
            return Ok(DecodedCell::sql_null());
        }
        return Err(fault(column, "null_not_allowed", 0));
    }
    if matches!(
        value,
        mysql_async::Value::Float(_) | mysql_async::Value::Double(_)
    ) {
        return Err(fault(column, "float_refused", 0));
    }
    match column.store {
        Store::Numeric { precision, scale } => {
            decimal_cell(column, value, precision, i32::from(scale))
        }
        Store::Int { width, .. } => int_cell(column, value, width),
        Store::Varchar { chars } => text_cell(column, value, Some(chars)),
        Store::Json => text_cell(column, value, None),
        Store::Bytes => bytes_cell(column, value),
        Store::Date => date_cell(column, value),
        Store::Time => time_cell(column, value),
        Store::Timestamp => timestamp_cell(column, value, false),
        Store::Timestamptz => timestamp_cell(column, value, true),
    }
}

fn fault(column: &ColSpec, reason: &'static str, value_len: i64) -> CellFault {
    CellFault {
        column: column.name.clone(),
        reason,
        value_len,
    }
}

fn value_text(value: &mysql_async::Value) -> Result<String, ()> {
    match value {
        mysql_async::Value::Bytes(bytes) => String::from_utf8(bytes.clone()).map_err(|_| ()),
        mysql_async::Value::Int(number) => Ok(number.to_string()),
        mysql_async::Value::UInt(number) => Ok(number.to_string()),
        _ => Err(()),
    }
}

fn decimal_cell(
    column: &ColSpec,
    value: &mysql_async::Value,
    precision: u16,
    scale: i32,
) -> Result<DecodedCell, CellFault> {
    let text = value_text(value).map_err(|_| fault(column, "decimal_text", 0))?;
    let parsed = ExactDecimal::parse_sql(text.trim())
        .map_err(|_| fault(column, "decimal_text", text.len() as i64))?;
    let fitted = fit_scale(parsed, scale)
        .ok_or_else(|| fault(column, "decimal_scale", text.len() as i64))?;
    if fitted.coefficient().len() > usize::from(precision) {
        return Err(fault(column, "decimal_precision", text.len() as i64));
    }
    let arrow = if precision <= 38 {
        ArrowScalar::Decimal128(
            unscaled_i128(&fitted)
                .ok_or_else(|| fault(column, "decimal_arrow", text.len() as i64))?,
        )
    } else {
        ArrowScalar::Decimal256(
            unscaled_i256(&fitted)
                .ok_or_else(|| fault(column, "decimal_arrow", text.len() as i64))?,
        )
    };
    Ok(value_cell(LogicalValue::Decimal(fitted), arrow))
}

fn int_cell(
    column: &ColSpec,
    value: &mysql_async::Value,
    width: u8,
) -> Result<DecodedCell, CellFault> {
    let text = value_text(value).map_err(|_| fault(column, "int_text", 0))?;
    let exact =
        ExactInt::parse(text.trim()).map_err(|_| fault(column, "int_text", text.len() as i64))?;
    let number = i64_of(&exact).ok_or_else(|| fault(column, "int_range", text.len() as i64))?;
    let arrow = match width {
        4 => ArrowScalar::Int32(
            i32::try_from(number).map_err(|_| fault(column, "int_range", text.len() as i64))?,
        ),
        8 => ArrowScalar::Int64(number),
        _ => return Err(fault(column, "int_width", text.len() as i64)),
    };
    Ok(value_cell(LogicalValue::Int(exact), arrow))
}

fn text_cell(
    column: &ColSpec,
    value: &mysql_async::Value,
    limit: Option<i32>,
) -> Result<DecodedCell, CellFault> {
    let text = value_text(value).map_err(|_| fault(column, "utf8", 0))?;
    if text.as_bytes().contains(&0) {
        return Err(fault(column, "text_nul", text.len() as i64));
    }
    if limit.is_some_and(|max| text.chars().count() > usize::try_from(max).unwrap_or(usize::MAX)) {
        return Err(fault(column, "char_length", text.len() as i64));
    }
    let arrow = ArrowScalar::Utf8(text.clone());
    Ok(value_cell(LogicalValue::Text(text), arrow))
}

fn bytes_cell(column: &ColSpec, value: &mysql_async::Value) -> Result<DecodedCell, CellFault> {
    let bytes = match value {
        mysql_async::Value::Bytes(bytes) => bytes.clone(),
        _ => return Err(fault(column, "bytes", 0)),
    };
    let arrow = ArrowScalar::Binary(bytes.clone());
    Ok(value_cell(LogicalValue::Bytes(bytes), arrow))
}

fn date_cell(column: &ColSpec, value: &mysql_async::Value) -> Result<DecodedCell, CellFault> {
    let (year, month, day) = date_parts(value).map_err(|_| fault(column, "date", 0))?;
    if year == 0 || month == 0 || day == 0 {
        return Err(fault(column, "zero_date", 10));
    }
    let unix = unix_days(year, month, day);
    let arrow_days = i32::try_from(unix).map_err(|_| fault(column, "date_range", 10))?;
    Ok(value_cell(
        LogicalValue::Date { year, month, day },
        ArrowScalar::Date32(arrow_days),
    ))
}

fn time_cell(column: &ColSpec, value: &mysql_async::Value) -> Result<DecodedCell, CellFault> {
    let (negative, days, hours, minutes, seconds, micros) = match value {
        mysql_async::Value::Time(negative, days, hours, minutes, seconds, micros) => (
            *negative,
            *days,
            u32::from(*hours),
            u32::from(*minutes),
            u32::from(*seconds),
            *micros,
        ),
        mysql_async::Value::Bytes(bytes) => {
            parse_time_text(bytes).map_err(|_| fault(column, "time", bytes.len() as i64))?
        }
        _ => return Err(fault(column, "time", 0)),
    };
    let rendered = if negative { 16 } else { 15 };
    if negative || days > 0 || hours >= 24 || minutes > 59 || seconds > 59 || micros >= 1_000_000 {
        return Err(fault(column, "time_range", rendered));
    }
    let clock = i64::from(hours) * 3_600_000_000
        + i64::from(minutes) * 60_000_000
        + i64::from(seconds) * 1_000_000
        + i64::from(micros);
    if !(0..86_400_000_000).contains(&clock) {
        return Err(fault(column, "time_range", rendered));
    }
    Ok(value_cell(
        LogicalValue::TimeMicros(clock),
        ArrowScalar::Time64(clock),
    ))
}

fn timestamp_cell(
    column: &ColSpec,
    value: &mysql_async::Value,
    utc: bool,
) -> Result<DecodedCell, CellFault> {
    let parts = match value {
        mysql_async::Value::Date(year, month, day, hour, minute, second, micros) => StampParts {
            year: i32::from(*year),
            month: *month,
            day: *day,
            hour: *hour,
            minute: *minute,
            second: *second,
            micros: *micros,
        },
        mysql_async::Value::Bytes(bytes) => parse_timestamp_text(bytes)
            .map_err(|_| fault(column, "timestamp", bytes.len() as i64))?,
        _ => return Err(fault(column, "timestamp", 0)),
    };
    if parts.year == 0 || parts.month == 0 || parts.day == 0 {
        return Err(fault(column, "zero_date", 26));
    }
    let unix = naive_unix(
        parts.year,
        parts.month,
        parts.day,
        parts.hour,
        parts.minute,
        parts.second,
        parts.micros,
    )
    .ok_or_else(|| fault(column, "timestamp_range", 26))?;
    if utc {
        Ok(value_cell(
            LogicalValue::TimestampTzMicros(unix),
            ArrowScalar::TimestampUtc(unix),
        ))
    } else {
        Ok(value_cell(
            LogicalValue::TimestampNaive {
                year: parts.year,
                month: parts.month,
                day: parts.day,
                hour: parts.hour,
                minute: parts.minute,
                second: parts.second,
                micros: parts.micros,
            },
            ArrowScalar::TimestampNaive(unix),
        ))
    }
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

pub fn fit_scale(exact: ExactDecimal, scale: i32) -> Option<ExactDecimal> {
    let current = exact.scale();
    if current == scale {
        return Some(exact);
    }
    if current < scale {
        let mut digits = exact.coefficient().to_string();
        digits.extend(std::iter::repeat_n(
            '0',
            usize::try_from(scale - current).ok()?,
        ));
        return ExactDecimal::new(exact.is_negative(), digits, scale).ok();
    }
    let diff = usize::try_from(current - scale).ok()?;
    let digits = exact.coefficient();
    if digits.len() < diff {
        return None;
    }
    let split = digits.len() - diff;
    if digits[split..].bytes().any(|byte| byte != b'0') {
        return None;
    }
    let head = if split == 0 { "0" } else { &digits[..split] };
    ExactDecimal::new(exact.is_negative(), head.to_string(), scale).ok()
}

pub fn unscaled_i128(exact: &ExactDecimal) -> Option<i128> {
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value
            .checked_mul(10)?
            .checked_add(i128::from(byte - b'0'))?;
    }
    if exact.is_negative() {
        value = value.checked_neg()?;
    }
    Some(value)
}

pub fn unscaled_i256(exact: &ExactDecimal) -> Option<[u8; 32]> {
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
        carry = wide >> 8;
    }
    if carry == 0 { Ok(()) } else { Err(()) }
}

fn neg_le(bytes: &mut [u8; 32]) -> Result<(), ()> {
    let mut carry = 1u16;
    for byte in bytes.iter_mut() {
        let wide = u16::from(!*byte) + carry;
        *byte = (wide & 0xff) as u8;
        carry = wide >> 8;
    }
    if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        Ok(())
    } else {
        Err(())
    }
}

fn i64_of(exact: &ExactInt) -> Option<i64> {
    let mut value: i128 = 0;
    for byte in exact.coefficient().bytes() {
        value = value
            .checked_mul(10)?
            .checked_add(i128::from(byte - b'0'))?;
    }
    if exact.is_negative() {
        value = value.checked_neg()?;
    }
    i64::try_from(value).ok()
}

pub fn unix_days(year: i32, month: u8, day: u8) -> i64 {
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

pub fn naive_unix(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
) -> Option<i64> {
    if hour > 23 || minute > 59 || second > 59 || micros > 999_999 {
        return None;
    }
    let days = i128::from(unix_days(year, month, day));
    let sod = i128::from(hour) * 3_600 + i128::from(minute) * 60 + i128::from(second);
    let unix = days
        .checked_mul(86_400_000_000)?
        .checked_add(sod.checked_mul(1_000_000)?)?
        .checked_add(i128::from(micros))?;
    i64::try_from(unix).ok()
}

fn date_parts(value: &mysql_async::Value) -> Result<(i32, u8, u8), ()> {
    match value {
        mysql_async::Value::Date(year, month, day, _, _, _, _) => {
            Ok((i32::from(*year), *month, *day))
        }
        mysql_async::Value::Bytes(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| ())?;
            let (year, month, day) = split_date(text)?;
            Ok((year, month, day))
        }
        _ => Err(()),
    }
}

fn split_date(text: &str) -> Result<(i32, u8, u8), ()> {
    let date = text.trim().split([' ', 'T']).next().unwrap_or(text);
    let mut parts = date.split('-');
    let year: i32 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let month: u8 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let day: u8 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    if parts.next().is_some() {
        return Err(());
    }
    Ok((year, month, day))
}

fn parse_time_text(bytes: &[u8]) -> Result<(bool, u32, u32, u32, u32, u32), ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?.trim();
    let negative = text.starts_with('-');
    let body = text.trim_start_matches(['-', '+']);
    let (clock, fraction) = body.split_once('.').unwrap_or((body, "0"));
    let mut parts = clock.split(':');
    let hours: u32 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let minutes: u32 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let seconds: u32 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    if parts.next().is_some() {
        return Err(());
    }
    let mut frac = fraction.to_string();
    if frac.len() > 6 || !frac.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    while frac.len() < 6 {
        frac.push('0');
    }
    let micros: u32 = frac.parse().map_err(|_| ())?;
    Ok((negative, 0, hours, minutes, seconds, micros))
}

struct StampParts {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    micros: u32,
}

fn parse_timestamp_text(bytes: &[u8]) -> Result<StampParts, ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?.trim();
    let (date, clock) = text.split_once([' ', 'T']).ok_or(())?;
    let (year, month, day) = split_date(date)?;
    let (negative, _, hours, minutes, seconds, micros) = parse_time_text(clock.as_bytes())?;
    if negative {
        return Err(());
    }
    Ok(StampParts {
        year,
        month,
        day,
        hour: u8::try_from(hours).map_err(|_| ())?,
        minute: u8::try_from(minutes).map_err(|_| ())?,
        second: u8::try_from(seconds).map_err(|_| ())?,
        micros,
    })
}

pub fn assemble_table(
    table: &TableSpec,
    rows: Vec<Vec<DecodedCell>>,
    input_digest: [u8; 32],
    generation: u64,
) -> Result<TypedBatch, String> {
    let plans = plans_for(table)?;
    let keys: Vec<i32> = table
        .columns
        .iter()
        .filter(|column| column.key)
        .map(|column| column.attnum)
        .collect();
    let binding = RunBinding {
        source_id: SOURCE_ID.to_string(),
        schema: table.schema.clone(),
        table: table.name.to_string(),
        run_generation: generation,
        input_digest,
    };
    assemble_batch(&binding, plans, &keys, rows).map_err(|err| err.to_string())
}

pub fn copy_columns(table: &TableSpec) -> Vec<CopyColumn> {
    let mut columns = vec![CopyColumn::SystemPosition];
    for column in &table.columns {
        columns.push(CopyColumn::User {
            attnum: column.attnum,
        });
    }
    columns
}

pub fn encode_ok(batch: &TypedBatch, columns: &[CopyColumn]) -> Result<(), CopyError> {
    let positions = positions(batch.rows.len())?;
    let mut encoder = CopyEncoder::open(
        batch,
        columns,
        &positions,
        copy_budgets(),
        UnavailablePolicy::Refuse,
    )?;
    loop {
        if matches!(encoder.pull()?, CopyPiece::Done) {
            return Ok(());
        }
    }
}

pub fn positions(count: usize) -> Result<Vec<ExactDecimal>, CopyError> {
    let mut positions = Vec::with_capacity(count);
    for index in 0..count {
        positions.push(
            ExactDecimal::new(false, index.to_string(), 0).map_err(|_| CopyError::new_public())?,
        );
    }
    Ok(positions)
}

trait CopyErrorPublic {
    fn new_public() -> CopyError;
}

impl CopyErrorPublic for CopyError {
    fn new_public() -> CopyError {
        CopyError {
            code: "position",
            detail: "",
            row: None,
            attnum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typmods_match_the_sealed_formula() {
        assert_eq!(numeric_typmod(20, 0), 1_310_724);
        assert_eq!(varchar_typmod(64), 68);
        assert_eq!(varchar_typmod(32), 36);
        assert_eq!(numeric_typmod(38, 10), 2_490_382);
        assert_eq!(numeric_typmod(65, 30), 4_259_874);
        assert_eq!(numeric_typmod(12, 2), 786_438);
    }

    #[test]
    fn schema_preimage_uses_the_public_producer_lines() {
        let tables = fixture_tables();
        let text = schema_preimage(&tables);
        assert!(
            text.starts_with("oo_synth.k_unsigned\nid\t1700\t1310724\tbigint_unsigned_numeric\n")
        );
        assert!(text.contains("t\t1083\t6\t\n"));
        assert!(!text.contains("mysql_time_not_interval"));
        assert!(text.contains("json_not_jsonb"));
        assert_eq!(schema_digest(&tables).len(), 64);
    }

    #[test]
    fn uint64_max_and_wide_decimal_encode() {
        let tables = fixture_tables();
        let unsigned = &tables[0];
        let wide = decode_cell(
            &unsigned.columns[0],
            &mysql_async::Value::Bytes(b"18446744073709551615".to_vec()),
        )
        .unwrap();
        let label = decode_cell(
            &unsigned.columns[1],
            &mysql_async::Value::Bytes(b"uint64-max".to_vec()),
        )
        .unwrap();
        let batch = assemble_table(unsigned, vec![vec![wide, label]], [7u8; 32], 1).unwrap();
        encode_ok(&batch, &copy_columns(unsigned)).unwrap();

        let decimal = &tables[1];
        let id = decode_cell(
            &decimal.columns[0],
            &mysql_async::Value::Bytes(b"99999999999999999999.9999999999".to_vec()),
        )
        .unwrap();
        let payload = decode_cell(
            &decimal.columns[1],
            &mysql_async::Value::Bytes(
                b"99999999999999999999999999999999999.999999999999999999999999999999".to_vec(),
            ),
        )
        .unwrap();
        let batch = assemble_table(decimal, vec![vec![id, payload]], [7u8; 32], 1).unwrap();
        encode_ok(&batch, &copy_columns(decimal)).unwrap();
    }

    #[test]
    fn out_of_range_time_is_rejected_and_a_clock_encodes() {
        let tables = fixture_tables();
        let temporal = &tables[5];
        let refused = decode_cell(
            &temporal.columns[2],
            &mysql_async::Value::Time(true, 34, 22, 59, 59, 0),
        );
        assert_eq!(refused.unwrap_err().reason, "time_range");
        let refused_text = decode_cell(
            &temporal.columns[2],
            &mysql_async::Value::Bytes(b"-838:59:59.000000".to_vec()),
        );
        assert_eq!(refused_text.unwrap_err().reason, "time_range");
        let clock = decode_cell(
            &temporal.columns[2],
            &mysql_async::Value::Time(false, 0, 12, 0, 0, 123_456),
        )
        .unwrap();
        match clock.value {
            Some(LogicalValue::TimeMicros(value)) => assert_eq!(value, 43_200_123_456),
            _ => panic!("clock"),
        }
        assert_eq!(unix_days(1970, 1, 1), 0);
        assert!(naive_unix(1000, 1, 1, 0, 0, 0, 0).is_some());
        assert!(naive_unix(9999, 12, 31, 23, 59, 59, 999_999).is_some());
    }

    #[test]
    fn catalog_decision_keeps_the_unsigned_boundary_and_refuses_float() {
        use mariadb_probe::decide::{NativeColumn, Plan, decide};
        let unsigned = NativeColumn {
            table: "k_unsigned".to_string(),
            name: "id".to_string(),
            ordinal: 1,
            data_type: "bigint".to_string(),
            column_type: "bigint(20) unsigned".to_string(),
            nullable: false,
            charset: None,
            collation: None,
            column_key: "PRI".to_string(),
            precision: Some(20),
            scale: Some(0),
            datetime_precision: None,
            check_clause: None,
        };
        let plan = decide(&unsigned).unwrap();
        let spec = from_decision("id", 1, true, false, &unsigned.column_type, None, &plan).unwrap();
        assert_eq!(spec.oid(), 1700);
        assert_eq!(spec.caveat.as_deref(), Some("bigint_unsigned_numeric"));
        let floated = NativeColumn {
            table: "k_float_refused".to_string(),
            name: "measure".to_string(),
            ordinal: 2,
            data_type: "float".to_string(),
            column_type: "float".to_string(),
            nullable: false,
            charset: None,
            collation: None,
            column_key: String::new(),
            precision: Some(12),
            scale: None,
            datetime_precision: None,
            check_clause: None,
        };
        assert!(decide(&floated).is_err());
        let _ = Plan {
            kind: plan.kind.clone(),
            caveats: Vec::new(),
        };
    }
}
