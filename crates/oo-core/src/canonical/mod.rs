//! Canonical logical-value and primary-key bytes, version 1.
//!
//! The hash is SHA-256 over an explicit frame. The frame contains a domain
//! prefix, the type-policy version, the logical schema, and length-prefixed
//! values. Arrow physical buffers are never hashed. Byte layout is also
//! described in `contracts/canonical-value-v1.md`.

mod json;

use sha2::{Digest, Sha256};

use crate::decimal::{ExactDecimal, ExactInt};
use crate::diagnostic::TYPE_POLICY_V1;
use crate::limits::{
    CANONICAL_MAX_ARRAY_ELEMENTS, CANONICAL_MAX_COEFFICIENT_DIGITS, CANONICAL_MAX_DEPTH,
    CANONICAL_MAX_DOCUMENT_BYTES, CANONICAL_MAX_FIELD_NAME_BYTES, CANONICAL_MAX_VALUE_BYTES,
    MYSQL_TIME_MAX_MICROS, TIMESTAMPTZ_MAX_SECONDS, TIMESTAMPTZ_MIN_SECONDS,
};

pub use json::{JsonNode, parse_json};

pub const VALUE_MAGIC: &[u8] = b"oo-canonical-value/v1\0";
pub const PK_MAGIC: &[u8] = b"oo-canonical-pk/v1\0";
pub const HOOK_MAGIC: &[u8] = b"oo-hook-digest/v1\0";

const TAG_BOOL: u8 = 1;
const TAG_INT: u8 = 2;
const TAG_DECIMAL: u8 = 3;
const TAG_FLOAT: u8 = 4;
const TAG_TEXT: u8 = 5;
const TAG_BYTES: u8 = 6;
const TAG_DATE: u8 = 7;
const TAG_TIME: u8 = 8;
const TAG_TIMESTAMP_NAIVE: u8 = 9;
const TAG_TIMESTAMPTZ: u8 = 10;
const TAG_JSON: u8 = 11;
const TAG_ARRAY: u8 = 12;
const TAG_STRUCT: u8 = 13;
const TAG_DURATION_MICROS: u8 = 14;

const PRESENCE_MISSING: u8 = 0;
const PRESENCE_NULL: u8 = 1;
const PRESENCE_VALUE: u8 = 2;
const PRESENCE_UNAVAILABLE: u8 = 3;

const FLOAT_NAN: u8 = 1;
const FLOAT_ZERO: u8 = 2;
const FLOAT_POS_INF: u8 = 3;
const FLOAT_NEG_INF: u8 = 4;
const FLOAT_FINITE: u8 = 5;

/// `precision == 0` and `scale == i16::MIN` means each value carries its own scale.
pub const ARBITRARY_DECIMAL_SCALE: i16 = i16::MIN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    Budget,
    Depth,
    TypeMismatch,
    InvalidValue,
    NullNotAllowed,
    PkNotValue,
}

impl std::fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Budget => "canonical length or size budget exceeded",
            Self::Depth => "canonical nesting budget exceeded",
            Self::TypeMismatch => "logical value does not match the logical schema",
            Self::InvalidValue => "logical value is outside the canonical domain",
            Self::NullNotAllowed => "null is not allowed for this field",
            Self::PkNotValue => "a primary-key component must be a present value",
        })
    }
}

impl std::error::Error for CanonicalError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogicalType {
    Bool,
    Int,
    Decimal { precision: u16, scale: i16 },
    Float64,
    Text,
    Bytes,
    Date,
    Time,
    DurationMicros,
    TimestampNaive,
    TimestampTz,
    Json,
    Array(Box<LogicalType>),
    Struct(Vec<LogicalField>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalField {
    pub name: String,
    pub logical_type: LogicalType,
    pub nullable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Missing,
    Null,
    Value,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldValue {
    pub presence: Presence,
    pub value: Option<LogicalValue>,
}

impl FieldValue {
    pub fn value(value: LogicalValue) -> Self {
        Self {
            presence: Presence::Value,
            value: Some(value),
        }
    }

    pub fn null() -> Self {
        Self {
            presence: Presence::Null,
            value: None,
        }
    }

    pub fn missing() -> Self {
        Self {
            presence: Presence::Missing,
            value: None,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            presence: Presence::Unavailable,
            value: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatClass {
    Nan,
    Zero,
    PosInf,
    NegInf,
    Finite(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogicalValue {
    Bool(bool),
    Int(ExactInt),
    Decimal(ExactDecimal),
    Float(FloatClass),
    Text(String),
    Bytes(Vec<u8>),
    Date {
        year: i32,
        month: u8,
        day: u8,
    },
    TimeMicros(i64),
    DurationMicros(i64),
    TimestampNaive {
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        micros: u32,
    },
    TimestampTzMicros(i64),
    Json(JsonNode),
    Array(Vec<FieldValue>),
    Struct(Vec<FieldValue>),
}

pub fn classify_f64(value: f64) -> FloatClass {
    if value.is_nan() {
        FloatClass::Nan
    } else if value == 0.0 {
        FloatClass::Zero
    } else if value.is_sign_positive() && value.is_infinite() {
        FloatClass::PosInf
    } else if value.is_infinite() {
        FloatClass::NegInf
    } else {
        FloatClass::Finite(value.to_bits())
    }
}

pub fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn encode_value(
    type_policy: &str,
    fields: &[LogicalField],
    values: &[FieldValue],
) -> Result<Vec<u8>, CanonicalError> {
    encode_frame(VALUE_MAGIC, type_policy, fields, values, true)
}

pub fn encode_pk(
    type_policy: &str,
    fields: &[LogicalField],
    values: &[FieldValue],
) -> Result<Vec<u8>, CanonicalError> {
    if values.len() != fields.len() || values.iter().any(|value| value.presence != Presence::Value)
    {
        return Err(CanonicalError::PkNotValue);
    }
    encode_frame(PK_MAGIC, type_policy, fields, values, false)
}

pub fn encode_hook_digest(names: &[&str]) -> Result<[u8; 32], CanonicalError> {
    let mut buf = Buf::new(CANONICAL_MAX_DOCUMENT_BYTES);
    buf.extend(HOOK_MAGIC)?;
    buf.u32be(u32::try_from(names.len()).map_err(|_| CanonicalError::Budget)?)?;
    for name in names {
        buf.len_bytes(name.as_bytes())?;
    }
    Ok(hash_bytes(&buf.bytes))
}

pub fn empty_hook_preimage() -> Vec<u8> {
    let mut bytes = HOOK_MAGIC.to_vec();
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes
}

fn encode_frame(
    magic: &[u8],
    type_policy: &str,
    fields: &[LogicalField],
    values: &[FieldValue],
    nullable_bit: bool,
) -> Result<Vec<u8>, CanonicalError> {
    if type_policy != TYPE_POLICY_V1 {
        return Err(CanonicalError::InvalidValue);
    }
    if fields.len() != values.len() {
        return Err(CanonicalError::TypeMismatch);
    }
    let mut buf = Buf::new(CANONICAL_MAX_DOCUMENT_BYTES);
    buf.extend(magic)?;
    buf.len_bytes(type_policy.as_bytes())?;
    write_schema(&mut buf, fields, nullable_bit, 1)?;
    for (field, value) in fields.iter().zip(values) {
        write_field(&mut buf, field, value, 1)?;
    }
    Ok(buf.bytes)
}

fn write_schema(
    buf: &mut Buf,
    fields: &[LogicalField],
    nullable_bit: bool,
    depth: usize,
) -> Result<(), CanonicalError> {
    if depth > CANONICAL_MAX_DEPTH {
        return Err(CanonicalError::Depth);
    }
    buf.u32be(u32::try_from(fields.len()).map_err(|_| CanonicalError::Budget)?)?;
    for field in fields {
        if field.name.len() > CANONICAL_MAX_FIELD_NAME_BYTES || field.name.is_empty() {
            return Err(CanonicalError::Budget);
        }
        buf.len_bytes(field.name.as_bytes())?;
        write_type(buf, &field.logical_type, depth)?;
        if nullable_bit {
            buf.u8(u8::from(field.nullable))?;
        }
    }
    Ok(())
}

fn write_type(
    buf: &mut Buf,
    logical_type: &LogicalType,
    depth: usize,
) -> Result<(), CanonicalError> {
    match logical_type {
        LogicalType::Bool => buf.u8(TAG_BOOL),
        LogicalType::Int => buf.u8(TAG_INT),
        LogicalType::Decimal { precision, scale } => {
            if *precision == 0 && *scale != ARBITRARY_DECIMAL_SCALE {
                return Err(CanonicalError::InvalidValue);
            }
            buf.u8(TAG_DECIMAL)?;
            buf.u16be(*precision)?;
            buf.i16be(*scale)
        }
        LogicalType::Float64 => buf.u8(TAG_FLOAT),
        LogicalType::Text => buf.u8(TAG_TEXT),
        LogicalType::Bytes => buf.u8(TAG_BYTES),
        LogicalType::Date => buf.u8(TAG_DATE),
        LogicalType::Time => buf.u8(TAG_TIME),
        LogicalType::DurationMicros => buf.u8(TAG_DURATION_MICROS),
        LogicalType::TimestampNaive => buf.u8(TAG_TIMESTAMP_NAIVE),
        LogicalType::TimestampTz => buf.u8(TAG_TIMESTAMPTZ),
        LogicalType::Json => buf.u8(TAG_JSON),
        LogicalType::Array(item) => {
            buf.u8(TAG_ARRAY)?;
            write_type(buf, item, depth + 1)
        }
        LogicalType::Struct(fields) => {
            buf.u8(TAG_STRUCT)?;
            write_schema(buf, fields, true, depth + 1)
        }
    }
}

fn write_field(
    buf: &mut Buf,
    field: &LogicalField,
    value: &FieldValue,
    depth: usize,
) -> Result<(), CanonicalError> {
    if depth > CANONICAL_MAX_DEPTH {
        return Err(CanonicalError::Depth);
    }
    let mark = buf.bytes.len();
    match value.presence {
        Presence::Missing => buf.u8(PRESENCE_MISSING)?,
        Presence::Null => {
            if !field.nullable {
                return Err(CanonicalError::NullNotAllowed);
            }
            buf.u8(PRESENCE_NULL)?;
        }
        Presence::Unavailable => buf.u8(PRESENCE_UNAVAILABLE)?,
        Presence::Value => {
            buf.u8(PRESENCE_VALUE)?;
            let Some(inner) = &value.value else {
                return Err(CanonicalError::TypeMismatch);
            };
            write_value(buf, &field.logical_type, inner, depth)?;
        }
    }
    if buf.bytes.len() - mark > CANONICAL_MAX_VALUE_BYTES {
        return Err(CanonicalError::Budget);
    }
    Ok(())
}

fn write_value(
    buf: &mut Buf,
    logical_type: &LogicalType,
    value: &LogicalValue,
    depth: usize,
) -> Result<(), CanonicalError> {
    match (logical_type, value) {
        (LogicalType::Bool, LogicalValue::Bool(item)) => buf.u8(u8::from(*item)),
        (LogicalType::Int, LogicalValue::Int(item)) => write_int(buf, item),
        (LogicalType::Decimal { precision, scale }, LogicalValue::Decimal(item)) => {
            write_decimal(buf, *precision, *scale, item)
        }
        (LogicalType::Float64, LogicalValue::Float(item)) => write_float(buf, *item),
        (LogicalType::Text, LogicalValue::Text(item)) => buf.len_bytes(item.as_bytes()),
        (LogicalType::Bytes, LogicalValue::Bytes(item)) => buf.len_bytes(item),
        (LogicalType::Date, LogicalValue::Date { year, month, day }) => {
            validate_date(*year, *month, *day)?;
            buf.i32be(*year)?;
            buf.u8(*month)?;
            buf.u8(*day)
        }
        (LogicalType::Time, LogicalValue::TimeMicros(micros)) => {
            if micros.abs() > MYSQL_TIME_MAX_MICROS {
                return Err(CanonicalError::InvalidValue);
            }
            buf.i64be(*micros)
        }
        (LogicalType::DurationMicros, LogicalValue::DurationMicros(micros)) => {
            if micros.unsigned_abs() > MYSQL_TIME_MAX_MICROS as u64 {
                return Err(CanonicalError::InvalidValue);
            }
            buf.i64be(*micros)
        }
        (
            LogicalType::TimestampNaive,
            LogicalValue::TimestampNaive {
                year,
                month,
                day,
                hour,
                minute,
                second,
                micros,
            },
        ) => {
            validate_date(*year, *month, *day)?;
            if *hour > 23 || *minute > 59 || *second > 59 || *micros > 999_999 {
                return Err(CanonicalError::InvalidValue);
            }
            buf.i32be(*year)?;
            buf.u8(*month)?;
            buf.u8(*day)?;
            buf.u8(*hour)?;
            buf.u8(*minute)?;
            buf.u8(*second)?;
            buf.u32be(*micros)
        }
        (LogicalType::TimestampTz, LogicalValue::TimestampTzMicros(micros)) => {
            let seconds = micros.div_euclid(1_000_000);
            if !(TIMESTAMPTZ_MIN_SECONDS..=TIMESTAMPTZ_MAX_SECONDS).contains(&seconds) {
                return Err(CanonicalError::InvalidValue);
            }
            buf.i64be(*micros)
        }
        (LogicalType::Json, LogicalValue::Json(node)) => json::write_json(buf, node, depth),
        (LogicalType::Array(item_type), LogicalValue::Array(items)) => {
            if items.len() > CANONICAL_MAX_ARRAY_ELEMENTS {
                return Err(CanonicalError::Budget);
            }
            buf.u32be(u32::try_from(items.len()).map_err(|_| CanonicalError::Budget)?)?;
            let item_field = LogicalField {
                name: "item".to_owned(),
                logical_type: (**item_type).clone(),
                nullable: true,
            };
            for item in items {
                write_field(buf, &item_field, item, depth + 1)?;
            }
            Ok(())
        }
        (LogicalType::Struct(fields), LogicalValue::Struct(items)) => {
            if fields.len() != items.len() {
                return Err(CanonicalError::TypeMismatch);
            }
            for (field, item) in fields.iter().zip(items) {
                write_field(buf, field, item, depth + 1)?;
            }
            Ok(())
        }
        _ => Err(CanonicalError::TypeMismatch),
    }
}

fn write_int(buf: &mut Buf, value: &ExactInt) -> Result<(), CanonicalError> {
    let digits = value.coefficient();
    if digits.len() > CANONICAL_MAX_COEFFICIENT_DIGITS || digits.is_empty() {
        return Err(CanonicalError::Budget);
    }
    buf.u8(u8::from(value.is_negative()))?;
    buf.len_bytes(digits.as_bytes())
}

fn write_decimal(
    buf: &mut Buf,
    precision: u16,
    schema_scale: i16,
    value: &ExactDecimal,
) -> Result<(), CanonicalError> {
    if precision == 0 {
        if schema_scale != ARBITRARY_DECIMAL_SCALE {
            return Err(CanonicalError::InvalidValue);
        }
    } else {
        let value_scale = i16::try_from(value.scale()).map_err(|_| CanonicalError::InvalidValue)?;
        if value_scale != schema_scale || value.coefficient().len() > usize::from(precision) {
            return Err(CanonicalError::InvalidValue);
        }
    }
    if value.coefficient().len() > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return Err(CanonicalError::Budget);
    }
    let scale = i16::try_from(value.scale()).map_err(|_| CanonicalError::InvalidValue)?;
    buf.i16be(scale)?;
    buf.u8(u8::from(value.is_negative()))?;
    buf.len_bytes(value.coefficient().as_bytes())
}

fn write_float(buf: &mut Buf, value: FloatClass) -> Result<(), CanonicalError> {
    let value = match value {
        FloatClass::Finite(bits) => classify_f64(f64::from_bits(bits)),
        other => other,
    };
    match value {
        FloatClass::Nan => buf.u8(FLOAT_NAN),
        FloatClass::Zero => buf.u8(FLOAT_ZERO),
        FloatClass::PosInf => buf.u8(FLOAT_POS_INF),
        FloatClass::NegInf => buf.u8(FLOAT_NEG_INF),
        FloatClass::Finite(bits) => {
            buf.u8(FLOAT_FINITE)?;
            buf.u64be(bits)
        }
    }
}

pub fn validate_date(year: i32, month: u8, day: u8) -> Result<(), CanonicalError> {
    if year == 0 || !(-4713..=999_999).contains(&year) || !(1..=12).contains(&month) {
        return Err(CanonicalError::InvalidValue);
    }
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => return Err(CanonicalError::InvalidValue),
    };
    if day == 0 || day > max_day {
        Err(CanonicalError::InvalidValue)
    } else {
        Ok(())
    }
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

struct Buf {
    bytes: Vec<u8>,
    limit: usize,
}

impl Buf {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn reserve(&mut self, extra: usize) -> Result<(), CanonicalError> {
        if self.bytes.len().saturating_add(extra) > self.limit {
            Err(CanonicalError::Budget)
        } else {
            Ok(())
        }
    }

    fn extend(&mut self, data: &[u8]) -> Result<(), CanonicalError> {
        self.reserve(data.len())?;
        self.bytes.extend_from_slice(data);
        Ok(())
    }

    fn u8(&mut self, value: u8) -> Result<(), CanonicalError> {
        self.extend(&[value])
    }

    fn u16be(&mut self, value: u16) -> Result<(), CanonicalError> {
        self.extend(&value.to_be_bytes())
    }

    fn i16be(&mut self, value: i16) -> Result<(), CanonicalError> {
        self.extend(&value.to_be_bytes())
    }

    fn u32be(&mut self, value: u32) -> Result<(), CanonicalError> {
        self.extend(&value.to_be_bytes())
    }

    fn i32be(&mut self, value: i32) -> Result<(), CanonicalError> {
        self.extend(&value.to_be_bytes())
    }

    fn i64be(&mut self, value: i64) -> Result<(), CanonicalError> {
        self.extend(&value.to_be_bytes())
    }

    fn u64be(&mut self, value: u64) -> Result<(), CanonicalError> {
        self.extend(&value.to_be_bytes())
    }

    fn len_bytes(&mut self, data: &[u8]) -> Result<(), CanonicalError> {
        let len = u32::try_from(data.len()).map_err(|_| CanonicalError::Budget)?;
        self.u32be(len)?;
        self.extend(data)
    }
}
