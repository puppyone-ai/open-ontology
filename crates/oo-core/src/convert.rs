//! Typed conversion of synthetic raw cells.
//!
//! No PostgreSQL or MySQL driver is invoked. Callers pass already-extracted
//! text, bits, or bytes. A rejection keeps the row: the outcome carries the
//! masked original and a stable reason, and it does not invent a replacement
//! except where `type_policy` explicitly says to strip NUL or null a zero date.

use crate::canonical::{FloatClass, JsonNode, LogicalValue, classify_f64, validate_date};
use crate::config::model::{Tinyint1Policy, TypePolicy, ZeroDatePolicy};
use crate::decimal::{ExactDecimal, ExactInt};
use crate::limits::TIMESTAMPTZ_MAX_SECONDS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MysqlIntKind {
    Tiny,
    Small,
    Medium,
    Int,
    Big,
    Year,
    Tinyint1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    EncodingNotInSp00Decoder,
    NulRejected,
    ZeroDate,
    InvalidDate,
    DstGap,
    DstOverlap,
    TimezoneUnresolved,
    TimezoneUnknown,
    Precision,
    Tinyint1NotBoolean,
    OutOfRange,
    InvalidValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionReject {
    pub reason: RejectReason,
    pub masked: String,
    pub raw_len: usize,
    pub sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellOutcome {
    pub value: Option<LogicalValue>,
    pub reject: Option<ConversionReject>,
}

impl CellOutcome {
    fn clean(value: LogicalValue) -> Self {
        Self {
            value: Some(value),
            reject: None,
        }
    }

    fn both(value: Option<LogicalValue>, reject: ConversionReject) -> Self {
        Self {
            value,
            reject: Some(reject),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawCell {
    Null,
    Unavailable,
    Bool(bool),
    MysqlInt {
        text: String,
        unsigned: bool,
        kind: MysqlIntKind,
    },
    Decimal(String),
    FloatBits(u64),
    Text(String),
    Bytes {
        data: Vec<u8>,
        charset: CharsetClaim,
    },
    Json(String),
    Date(String),
    Time(String),
    /// Civil local timestamp `YYYY-MM-DD HH:MM:SS`.
    LocalTimestamp(String),
    /// Already-UTC microseconds, including values before the Unix epoch.
    EpochMicros(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharsetClaim {
    Utf8,
    Gbk,
    Gb18030,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeDecision {
    Unambiguous(i64),
    Gap,
    /// Both instants. Neither offset is selected.
    Overlap {
        fold0_micros: i64,
        fold1_micros: i64,
    },
    UnknownZone,
    InvalidCivil,
    OutOfRange,
    Precision,
    Unresolved,
}

pub fn convert_cell(
    raw: &RawCell,
    policy: &TypePolicy,
    timezone: &str,
    sensitive: bool,
) -> CellOutcome {
    match raw {
        RawCell::Null => CellOutcome {
            value: None,
            reject: None,
        },
        RawCell::Unavailable => CellOutcome {
            value: None,
            reject: None,
        },
        RawCell::Bool(value) => CellOutcome::clean(LogicalValue::Bool(*value)),
        RawCell::MysqlInt {
            text,
            unsigned,
            kind,
        } => convert_int(text, *unsigned, *kind, policy, sensitive),
        RawCell::Decimal(text) => match ExactDecimal::parse_sql(text) {
            Ok(value) => CellOutcome::clean(LogicalValue::Decimal(value)),
            Err(_) => CellOutcome::both(None, reject(RejectReason::InvalidValue, text, sensitive)),
        },
        RawCell::FloatBits(bits) => {
            CellOutcome::clean(LogicalValue::Float(classify_f64(f64::from_bits(*bits))))
        }
        RawCell::Text(text) => convert_text(text, policy, sensitive),
        RawCell::Bytes { data, charset } => convert_bytes(data, *charset, policy, sensitive),
        RawCell::Json(text) => match crate::canonical::parse_json(text) {
            Ok(node) => CellOutcome::clean(LogicalValue::Json(node)),
            Err(_) => CellOutcome::both(None, reject(RejectReason::InvalidValue, text, sensitive)),
        },
        RawCell::Date(text) => convert_date(text, policy, sensitive),
        RawCell::Time(text) => convert_time(text, sensitive),
        RawCell::LocalTimestamp(text) => convert_local(text, timezone, policy, sensitive),
        RawCell::EpochMicros(micros) => {
            let seconds = micros.div_euclid(1_000_000);
            if !(crate::limits::TIMESTAMPTZ_MIN_SECONDS..=TIMESTAMPTZ_MAX_SECONDS)
                .contains(&seconds)
            {
                CellOutcome::both(
                    None,
                    reject(RejectReason::OutOfRange, &micros.to_string(), sensitive),
                )
            } else {
                CellOutcome::clean(LogicalValue::TimestampTzMicros(*micros))
            }
        }
    }
}

pub fn decide_local_time(timezone: &str, local: crate::tz::CivilLocal) -> TimeDecision {
    match crate::tz::resolve_local(timezone, local) {
        Ok(crate::tz::LocalResolution::Unambiguous(instant)) => TimeDecision::Unambiguous(instant),
        Ok(crate::tz::LocalResolution::Gap) => TimeDecision::Gap,
        Ok(crate::tz::LocalResolution::Overlap {
            fold0_micros,
            fold1_micros,
        }) => TimeDecision::Overlap {
            fold0_micros,
            fold1_micros,
        },
        Err(crate::tz::ZoneError::UnknownZone) => TimeDecision::UnknownZone,
        Err(crate::tz::ZoneError::InvalidCivil) => TimeDecision::InvalidCivil,
        Err(crate::tz::ZoneError::OutOfRange) => TimeDecision::OutOfRange,
        Err(crate::tz::ZoneError::Precision) => TimeDecision::Precision,
    }
}

fn convert_int(
    text: &str,
    unsigned: bool,
    kind: MysqlIntKind,
    policy: &TypePolicy,
    sensitive: bool,
) -> CellOutcome {
    let Ok(integer) = ExactInt::parse(text) else {
        return CellOutcome::both(None, reject(RejectReason::InvalidValue, text, sensitive));
    };
    if kind == MysqlIntKind::Tinyint1 && policy.tinyint1 == Tinyint1Policy::Boolean {
        if !integer.is_negative() && (integer.coefficient() == "0" || integer.coefficient() == "1")
        {
            return CellOutcome::clean(LogicalValue::Bool(integer.coefficient() == "1"));
        }
        return CellOutcome::both(
            Some(LogicalValue::Int(integer)),
            reject(RejectReason::Tinyint1NotBoolean, text, sensitive),
        );
    }
    if kind == MysqlIntKind::Big && unsigned {
        let Ok(decimal) = ExactDecimal::parse_sql(text) else {
            return CellOutcome::both(None, reject(RejectReason::InvalidValue, text, sensitive));
        };
        return CellOutcome::clean(LogicalValue::Decimal(decimal));
    }
    if integer.is_negative() && unsigned {
        return CellOutcome::both(None, reject(RejectReason::OutOfRange, text, sensitive));
    }
    CellOutcome::clean(LogicalValue::Int(integer))
}

fn convert_text(text: &str, policy: &TypePolicy, sensitive: bool) -> CellOutcome {
    if text.contains('\0') {
        let cleaned = policy
            .strip_nul
            .then(|| LogicalValue::Text(text.chars().filter(|ch| *ch != '\0').collect()));
        return CellOutcome::both(cleaned, reject(RejectReason::NulRejected, text, sensitive));
    }
    CellOutcome::clean(LogicalValue::Text(text.to_owned()))
}

fn convert_bytes(
    data: &[u8],
    charset: CharsetClaim,
    policy: &TypePolicy,
    sensitive: bool,
) -> CellOutcome {
    if charset != CharsetClaim::Utf8 {
        return CellOutcome::both(None, byte_reject(data, sensitive));
    }
    match std::str::from_utf8(data) {
        Ok(text) => convert_text(text, policy, sensitive),
        Err(_) => CellOutcome::both(None, byte_reject(data, sensitive)),
    }
}

fn byte_reject(data: &[u8], sensitive: bool) -> ConversionReject {
    ConversionReject {
        reason: RejectReason::EncodingNotInSp00Decoder,
        raw_len: data.len(),
        sensitive,
        masked: mask_bytes(data, sensitive),
    }
}

fn convert_date(text: &str, policy: &TypePolicy, sensitive: bool) -> CellOutcome {
    if text == "0000-00-00" || text.starts_with("0000-00-00 ") {
        return zero_date(text, policy, sensitive);
    }
    let Some((year, month, day)) = parse_civil_date(text) else {
        return CellOutcome::both(None, reject(RejectReason::InvalidDate, text, sensitive));
    };
    if validate_date(year, month, day).is_err() {
        return CellOutcome::both(None, reject(RejectReason::InvalidDate, text, sensitive));
    }
    CellOutcome::clean(LogicalValue::Date { year, month, day })
}

fn convert_local(text: &str, timezone: &str, policy: &TypePolicy, sensitive: bool) -> CellOutcome {
    if text.starts_with("0000-00-00") {
        return zero_date(text, policy, sensitive);
    }
    let Some((date, time)) = text.split_once(' ') else {
        return CellOutcome::both(None, reject(RejectReason::InvalidDate, text, sensitive));
    };
    let Some((year, month, day)) = parse_civil_date(date) else {
        return CellOutcome::both(None, reject(RejectReason::InvalidDate, text, sensitive));
    };
    let Some((hour, minute, second, micros)) = parse_hms_micros(time) else {
        let reason = if fraction_too_long(time) {
            RejectReason::Precision
        } else {
            RejectReason::InvalidDate
        };
        return CellOutcome::both(None, reject(reason, text, sensitive));
    };
    if validate_date(year, month, day).is_err() {
        return CellOutcome::both(None, reject(RejectReason::InvalidDate, text, sensitive));
    }
    match decide_local_time(
        timezone,
        crate::tz::CivilLocal {
            year,
            month,
            day,
            hour,
            minute,
            second,
            micros,
        },
    ) {
        TimeDecision::Unambiguous(instant) => {
            CellOutcome::clean(LogicalValue::TimestampTzMicros(instant))
        }
        TimeDecision::Gap => CellOutcome::both(None, reject(RejectReason::DstGap, text, sensitive)),
        TimeDecision::Overlap { .. } => {
            CellOutcome::both(None, reject(RejectReason::DstOverlap, text, sensitive))
        }
        TimeDecision::UnknownZone => {
            CellOutcome::both(None, reject(RejectReason::TimezoneUnknown, text, sensitive))
        }
        TimeDecision::InvalidCivil => {
            CellOutcome::both(None, reject(RejectReason::InvalidDate, text, sensitive))
        }
        TimeDecision::OutOfRange => {
            CellOutcome::both(None, reject(RejectReason::OutOfRange, text, sensitive))
        }
        TimeDecision::Precision => {
            CellOutcome::both(None, reject(RejectReason::Precision, text, sensitive))
        }
        TimeDecision::Unresolved => CellOutcome::both(
            None,
            reject(RejectReason::TimezoneUnresolved, text, sensitive),
        ),
    }
}

fn convert_time(text: &str, sensitive: bool) -> CellOutcome {
    let negative = text.starts_with('-');
    let body = text.strip_prefix('-').unwrap_or(text);
    let mut parts = body.split(':');
    let Some(hours) = parts.next().and_then(|item| item.parse::<i64>().ok()) else {
        return CellOutcome::both(None, reject(RejectReason::InvalidValue, text, sensitive));
    };
    let Some(minutes) = parts.next().and_then(|item| item.parse::<i64>().ok()) else {
        return CellOutcome::both(None, reject(RejectReason::InvalidValue, text, sensitive));
    };
    let Some(seconds) = parts.next().and_then(|item| item.parse::<i64>().ok()) else {
        return CellOutcome::both(None, reject(RejectReason::InvalidValue, text, sensitive));
    };
    if parts.next().is_some() || minutes > 59 || seconds > 59 {
        return CellOutcome::both(None, reject(RejectReason::InvalidValue, text, sensitive));
    }
    let micros = (hours * 3600 + minutes * 60 + seconds) * 1_000_000;
    let micros = if negative { -micros } else { micros };
    if micros.abs() > crate::limits::MYSQL_TIME_MAX_MICROS {
        return CellOutcome::both(None, reject(RejectReason::OutOfRange, text, sensitive));
    }
    CellOutcome::clean(LogicalValue::TimeMicros(micros))
}

fn zero_date(text: &str, policy: &TypePolicy, sensitive: bool) -> CellOutcome {
    let value = match policy.zero_date {
        ZeroDatePolicy::Null => None,
        ZeroDatePolicy::Raw => Some(LogicalValue::Text(text.to_owned())),
    };
    CellOutcome::both(value, reject(RejectReason::ZeroDate, text, sensitive))
}

pub fn mysql_integer_pg_type(
    kind: MysqlIntKind,
    unsigned: bool,
    policy: &TypePolicy,
) -> &'static str {
    match (kind, unsigned, policy.tinyint1) {
        (MysqlIntKind::Tinyint1, _, Tinyint1Policy::Boolean) => "boolean",
        (MysqlIntKind::Big, true, _) => "numeric(20,0)",
        (MysqlIntKind::Tiny | MysqlIntKind::Tinyint1 | MysqlIntKind::Year, false, _) => "smallint",
        (MysqlIntKind::Tiny | MysqlIntKind::Tinyint1, true, _) => "integer",
        (MysqlIntKind::Small | MysqlIntKind::Medium | MysqlIntKind::Int, false, _) => "integer",
        (
            MysqlIntKind::Small | MysqlIntKind::Medium | MysqlIntKind::Int | MysqlIntKind::Year,
            true,
            _,
        ) => "bigint",
        (MysqlIntKind::Big, false, _) => "bigint",
    }
}

fn parse_civil_date(text: &str) -> Option<(i32, u8, u8)> {
    let mut parts = text.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        None
    } else {
        Some((year, month, day))
    }
}

fn parse_hms_micros(text: &str) -> Option<(u8, u8, u8, u32)> {
    let (body, fraction) = match text.split_once('.') {
        Some((body, fraction)) => {
            if fraction.is_empty()
                || fraction.len() > 6
                || !fraction.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let mut padded = [b'0'; 6];
            padded[..fraction.len()].copy_from_slice(fraction.as_bytes());
            let micros = std::str::from_utf8(&padded).ok()?.parse().ok()?;
            (body, micros)
        }
        None => (text, 0u32),
    };
    let mut parts = body.split(':');
    let hour = parts.next()?.parse().ok()?;
    let minute = parts.next()?.parse().ok()?;
    let second = parts.next()?.parse().ok()?;
    if parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second, fraction))
}

fn fraction_too_long(text: &str) -> bool {
    text.split_once('.').is_some_and(|(_, fraction)| {
        fraction.len() > 6 && fraction.bytes().all(|b| b.is_ascii_digit())
    })
}

fn reject(reason: RejectReason, raw: &str, sensitive: bool) -> ConversionReject {
    ConversionReject {
        reason,
        raw_len: raw.len(),
        sensitive,
        masked: mask_text(raw, sensitive),
    }
}

fn mask_text(raw: &str, sensitive: bool) -> String {
    if sensitive {
        return format!("masked:{len}chars", len = raw.chars().count());
    }
    let mut out: String = raw.chars().take(200).collect();
    if raw.chars().count() > 200 {
        out.push('…');
    }
    out
}

fn mask_bytes(raw: &[u8], sensitive: bool) -> String {
    if sensitive {
        return format!("masked:{}bytes", raw.len());
    }
    let take = raw.len().min(32);
    let mut out = String::new();
    for byte in &raw[..take] {
        out.push_str(&format!("{byte:02x}"));
    }
    if raw.len() > take {
        out.push_str(&format!("…len={}", raw.len()));
    }
    out
}

/// Re-export so fixture tests can name the JSON node without a second parser.
pub fn json_node(node: JsonNode) -> LogicalValue {
    LogicalValue::Json(node)
}

pub fn float_class(bits: u64) -> FloatClass {
    classify_f64(f64::from_bits(bits))
}
