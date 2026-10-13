//! Native-contract types for one trusted consumer.
//!
//! The shared catalog publishes bool, int2, int4, int8, numeric, text, varchar,
//! float8, timestamptz, json, bytea, uuid, bpchar, date, timestamp, and jsonb.
//! `timestamp` is timestamp without time zone. `json` is oid 114 and keeps its
//! stored text. `jsonb` is oid 3802 at typmod `-1`: version byte 1 plus the
//! PostgreSQL `jsonb` text, including spaces and exact integer digits. This
//! module decodes the COPY base types named by `full-import/v1`, plus jsonb,
//! and refuses every other PostgreSQL type. A bytea cell is the hex of the original bytes, including
//! empty, zero, and non-UTF-8 payloads. A uuid cell is the canonical lowercase
//! 8-4-4-4-12 text of the 16 raw bytes, and only typmod `-1` is a published
//! uuid. A bpchar cell keeps the stored characters, including trailing spaces.
//! A date cell is `YYYY-MM-DD` from PostgreSQL days since 2000-01-01. A
//! timestamp cell is that same epoch in microseconds, printed with six
//! fractional digits and no `Z`. Only typmod `-1` publishes date or timestamp.
//! Null stays kind null and has no value. This module does not cast a column
//! to text.

use serde_json::{json, Value};

const DIGIT_BUDGET: usize = 8_192;
const DECIMAL256_DIGITS: i32 = 76;

const NUMERIC_POS: u16 = 0x0000;
const NUMERIC_NEG: u16 = 0x4000;
const PG_UNIX_DAYS: i64 = 10_957;
const DAY_MICROS: i64 = 86_400 * 1_000_000;

/// One published attribute the consumer can place on a visible column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PublishedType {
    pub(crate) name: &'static str,
    pub(crate) oid: u32,
}

/// `Some` when `typtype` is base and the name, OID, and typmod are in the
/// native contract. `None` is an undefined type and must not be queried.
pub(crate) fn classify(type_name: &str, oid: u32, typmod: i32, typtype: &str) -> Option<PublishedType> {
    if typtype != "b" {
        return None;
    }
    let name = match (type_name, oid) {
        ("bool", 16) => "bool",
        ("int2", 21) => "int2",
        ("int4", 23) => "int4",
        ("int8", 20) => "int8",
        ("float4", 700) => "float4",
        ("float8", 701) => "float8",
        ("text", 25) => "text",
        ("bytea", 17) => "bytea",
        ("uuid", 2950) if typmod == -1 => "uuid",
        ("date", 1082) if typmod == -1 => "date",
        ("time", 1083) => "time",
        ("timestamp", 1114) if typmod == -1 => "timestamp",
        ("timestamptz", 1184) => "timestamptz",
        ("json", 114) => "json",
        ("jsonb", 3802) if typmod == -1 => "jsonb",
        ("varchar", 1043) if character_typmod(typmod) => "varchar",
        ("bpchar", 1042) if character_typmod(typmod) => "bpchar",
        ("numeric", 1700) if numeric_typmod(typmod) => "numeric",
        _ => return None,
    };
    Some(PublishedType { name, oid })
}

fn character_typmod(typmod: i32) -> bool {
    if typmod == -1 {
        return true;
    }
    if typmod < 4 {
        return false;
    }
    (1..=10_485_760).contains(&(typmod - 4))
}

/// Decimal128 is 1..=38 and decimal256 is 39..=76. Precision 0, 77, an
/// unconstrained typmod, and a negative 11-bit scale stay outside the contract.
/// `grantable_base_type` uses this same 76-digit ceiling.
fn numeric_typmod(typmod: i32) -> bool {
    let Some((precision, scale)) = numeric_parts(typmod) else {
        return false;
    };
    (1..=DECIMAL256_DIGITS).contains(&precision)
        && (0..=DECIMAL256_DIGITS).contains(&scale)
        && precision >= scale
}

fn numeric_parts(typmod: i32) -> Option<(i32, i32)> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod.wrapping_sub(4);
    let precision = (adjusted >> 16) & 0xFFFF;
    let scale = ((adjusted & 0x7FF) ^ 0x400) - 0x400;
    Some((precision, scale))
}

pub(crate) fn decodable(oid: u32) -> bool {
    matches!(
        oid,
        16 | 21 | 23 | 20 | 700 | 701 | 25 | 1043 | 1042 | 114 | 3802 | 17 | 2950 | 1082 | 1083 | 1114 | 1184 | 1700
    )
}

pub(crate) fn decode(oid: u32, raw: Option<&[u8]>) -> Result<Value, &'static str> {
    let Some(raw) = raw else {
        return Ok(json!({"kind": "null"}));
    };
    let value = match oid {
        16 => json!({"kind": "bool", "value": decode_bool(raw)?}),
        21 => json!({"kind": "int2", "value": i64::from(decode_i16(raw)?).to_string()}),
        23 => json!({"kind": "int4", "value": i64::from(decode_i32(raw)?).to_string()}),
        20 => json!({"kind": "int8", "value": decode_i64(raw)?.to_string()}),
        700 => float_value("float4", f64::from(decode_f32(raw)?), raw)?,
        701 => float_value("float8", decode_f64(raw)?, raw)?,
        25 => text_value("text", raw)?,
        1043 => text_value("varchar", raw)?,
        1042 => text_value("bpchar", raw)?,
        114 => text_value("json", raw)?,
        3802 => jsonb_value(raw)?,
        17 => json!({"kind": "bytea", "value": hex_encode(raw)}),
        2950 => json!({"kind": "uuid", "value": uuid_text(raw)?}),
        1082 => json!({"kind": "date", "value": format_date(decode_i32(raw)?)?}),
        1083 => json!({"kind": "time", "value": format_time(decode_i64(raw)?)?}),
        1114 => json!({"kind": "timestamp", "value": format_timestamp(decode_i64(raw)?, false)?}),
        1184 => {
            let micros = decode_i64(raw)?;
            json!({
                "kind": "timestamptz",
                "value": format_timestamp(micros, true)?,
                "micros": micros.to_string(),
            })
        }
        1700 => numeric_value(raw)?,
        _ => return Err("undefined_type"),
    };
    Ok(value)
}

pub(crate) fn cell_len(cell: &Value) -> u64 {
    match cell.get("kind").and_then(Value::as_str) {
        Some("null") => 0,
        Some("bool") => {
            if cell.get("value").and_then(Value::as_bool) == Some(true) {
                4
            } else {
                5
            }
        }
        Some("bytea") => {
            let hex = cell.get("value").and_then(Value::as_str).unwrap_or("");
            u64::try_from(hex.len() / 2).unwrap_or(u64::MAX)
        }
        Some("numeric") => u64::try_from(numeric_display(cell).len()).unwrap_or(u64::MAX),
        _ => u64::try_from(
            cell.get("value")
                .and_then(Value::as_str)
                .map(str::len)
                .unwrap_or(0),
        )
        .unwrap_or(u64::MAX),
    }
}

fn numeric_display(cell: &Value) -> String {
    let negative = cell.get("negative").and_then(Value::as_bool).unwrap_or(false);
    let coefficient = cell.get("coefficient").and_then(Value::as_str).unwrap_or("0");
    let scale = i32::try_from(cell.get("scale").and_then(Value::as_i64).unwrap_or(0)).unwrap_or(0);
    decimal_text(negative, coefficient, scale)
}

pub(crate) fn stop_before_append(returned: u32, max_rows: u32, used: u64, row_bytes: u64, max_bytes: u64) -> bool {
    returned >= max_rows || used.saturating_add(row_bytes) > max_bytes
}

fn decode_bool(raw: &[u8]) -> Result<bool, &'static str> {
    match raw {
        [0] => Ok(false),
        [1] => Ok(true),
        _ => Err("bool"),
    }
}

fn decode_i16(raw: &[u8]) -> Result<i16, &'static str> {
    let bytes: [u8; 2] = raw.try_into().map_err(|_| "int2")?;
    Ok(i16::from_be_bytes(bytes))
}

fn decode_i32(raw: &[u8]) -> Result<i32, &'static str> {
    let bytes: [u8; 4] = raw.try_into().map_err(|_| "int4")?;
    Ok(i32::from_be_bytes(bytes))
}

fn decode_i64(raw: &[u8]) -> Result<i64, &'static str> {
    let bytes: [u8; 8] = raw.try_into().map_err(|_| "int8")?;
    Ok(i64::from_be_bytes(bytes))
}

fn decode_f32(raw: &[u8]) -> Result<f32, &'static str> {
    let bytes: [u8; 4] = raw.try_into().map_err(|_| "float4")?;
    Ok(f32::from_be_bytes(bytes))
}

fn decode_f64(raw: &[u8]) -> Result<f64, &'static str> {
    let bytes: [u8; 8] = raw.try_into().map_err(|_| "float8")?;
    Ok(f64::from_be_bytes(bytes))
}

fn float_value(kind: &str, value: f64, raw: &[u8]) -> Result<Value, &'static str> {
    let text = if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else {
        format_finite(value)
    };
    Ok(json!({"kind": kind, "value": text, "bits": hex_encode(raw)}))
}

fn format_finite(value: f64) -> String {
    let text = format!("{value:.16}");
    let negative = text.starts_with('-');
    let digits = text.trim_start_matches('-');
    let trimmed = trim_fraction(digits);
    if negative && trimmed != "0" {
        format!("-{trimmed}")
    } else {
        trimmed
    }
}

fn trim_fraction(text: &str) -> String {
    let Some((whole, fraction)) = text.split_once('.') else {
        return text.to_string();
    };
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        whole.to_string()
    } else {
        format!("{whole}.{fraction}")
    }
}

fn text_value(kind: &str, raw: &[u8]) -> Result<Value, &'static str> {
    let text = std::str::from_utf8(raw).map_err(|_| "utf8")?;
    if text.contains('\0') {
        return Err("utf8");
    }
    Ok(json!({"kind": kind, "value": text}))
}

fn jsonb_value(raw: &[u8]) -> Result<Value, &'static str> {
    let [1, rest @ ..] = raw else {
        return Err("jsonb");
    };
    let text = std::str::from_utf8(rest).map_err(|_| "jsonb")?;
    if text.contains('\0') {
        return Err("jsonb");
    }
    Ok(json!({"kind": "jsonb", "value": text}))
}

fn uuid_text(raw: &[u8]) -> Result<String, &'static str> {
    if raw.len() != 16 {
        return Err("uuid");
    }
    let hex = hex_encode(raw);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

fn format_date(days: i32) -> Result<String, &'static str> {
    let unix = i64::from(days) + PG_UNIX_DAYS;
    let (year, month, day) = civil_from_unix_days(unix).ok_or("date")?;
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}

fn format_time(micros: i64) -> Result<String, &'static str> {
    if !(0..DAY_MICROS).contains(&micros) {
        return Err("time");
    }
    let seconds = micros / 1_000_000;
    let sub = micros % 1_000_000;
    let hour = seconds / 3600;
    let minute = (seconds % 3600) / 60;
    let second = seconds % 60;
    Ok(format!("{hour:02}:{minute:02}:{second:02}.{sub:06}"))
}

fn format_timestamp(micros: i64, zulu: bool) -> Result<String, &'static str> {
    let unix = micros
        .checked_add(PG_UNIX_DAYS.checked_mul(DAY_MICROS).ok_or("timestamp")?)
        .ok_or("timestamp")?;
    let day_index = unix.div_euclid(DAY_MICROS);
    let tod = unix.rem_euclid(DAY_MICROS);
    let (year, month, day) = civil_from_unix_days(day_index).ok_or("timestamp")?;
    let seconds = tod / 1_000_000;
    let sub = tod % 1_000_000;
    let hour = seconds / 3600;
    let minute = (seconds % 3600) / 60;
    let second = seconds % 60;
    let zone = if zulu { "Z" } else { "" };
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{sub:06}{zone}"
    ))
}

fn civil_from_unix_days(days: i64) -> Option<(i32, u32, u32)> {
    let z = days.checked_add(719_468)?;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    let year = i32::try_from(year).ok()?;
    Some((year, month as u32, day as u32))
}

fn numeric_value(raw: &[u8]) -> Result<Value, &'static str> {
    let (negative, coefficient, scale) = decode_numeric(raw)?;
    Ok(json!({
        "kind": "numeric",
        "negative": negative,
        "coefficient": coefficient,
        "scale": scale,
    }))
}

fn decode_numeric(raw: &[u8]) -> Result<(bool, String, i32), &'static str> {
    if raw.len() < 8 {
        return Err("numeric");
    }
    let ndigits = i16::from_be_bytes([raw[0], raw[1]]);
    let weight = i16::from_be_bytes([raw[2], raw[3]]);
    let sign = u16::from_be_bytes([raw[4], raw[5]]);
    let dscale = i16::from_be_bytes([raw[6], raw[7]]);
    if ndigits < 0 || dscale < 0 {
        return Err("numeric");
    }
    let count = usize::try_from(ndigits).map_err(|_| "numeric")?;
    let expected = count.checked_mul(2).and_then(|body| body.checked_add(8)).ok_or("numeric")?;
    if raw.len() != expected {
        return Err("numeric");
    }
    let negative = match sign {
        NUMERIC_POS => false,
        NUMERIC_NEG => true,
        _ => return Err("numeric"),
    };
    let scale = usize::try_from(dscale).map_err(|_| "numeric")?;
    if scale > usize::try_from(DECIMAL256_DIGITS).unwrap_or(0) {
        return Err("numeric");
    }
    let mut groups = Vec::with_capacity(count);
    for index in 0..count {
        let off = 8 + index * 2;
        let digit = i16::from_be_bytes([raw[off], raw[off + 1]]);
        if !(0..=9_999).contains(&digit) {
            return Err("numeric");
        }
        groups.push(digit as u16);
    }
    if groups.is_empty() || groups.iter().all(|digit| *digit == 0) {
        return Ok((false, "0".to_string(), i32::from(dscale)));
    }
    let (int_part, frac_part) = split_numeric(weight, scale, &groups)?;
    let coefficient = numeric_coefficient(&int_part, &frac_part)?;
    let negative = negative && coefficient != "0";
    Ok((negative, coefficient, i32::from(dscale)))
}

fn numeric_coefficient(int_part: &str, frac_part: &str) -> Result<String, &'static str> {
    let trimmed_int = int_part.trim_start_matches('0');
    let coefficient = if trimmed_int.is_empty() {
        let trimmed_frac = frac_part.trim_start_matches('0');
        if trimmed_frac.is_empty() {
            "0".to_string()
        } else {
            trimmed_frac.to_string()
        }
    } else {
        let mut combined = String::with_capacity(trimmed_int.len() + frac_part.len());
        combined.push_str(trimmed_int);
        combined.push_str(frac_part);
        combined
    };
    if coefficient.len() > usize::try_from(DECIMAL256_DIGITS).unwrap_or(0) {
        return Err("numeric");
    }
    Ok(coefficient)
}

fn split_numeric(weight: i16, dscale: usize, groups: &[u16]) -> Result<(String, String), &'static str> {
    let mut int_part = String::new();
    let mut frac_part = String::new();
    if weight < 0 {
        let lead_groups = i32::from(weight).checked_neg().and_then(|value| value.checked_sub(1)).ok_or("numeric")?;
        let lead_digits = usize::try_from(lead_groups).ok().and_then(|value| value.checked_mul(4)).ok_or("numeric")?;
        if lead_digits >= dscale {
            if groups.iter().any(|digit| *digit != 0) {
                return Err("numeric");
            }
            return Ok(("0".to_string(), "0".repeat(dscale)));
        }
        frac_part.push_str(&"0".repeat(lead_digits));
        for digit in groups {
            push_group(&mut frac_part, *digit, dscale)?;
        }
        int_part.push('0');
    } else {
        for (index, digit) in groups.iter().enumerate() {
            let group_weight = i32::from(weight) - i32::try_from(index).unwrap_or(i32::MAX);
            if group_weight >= 0 {
                push_group(&mut int_part, *digit, DIGIT_BUDGET)?;
            } else {
                push_group(&mut frac_part, *digit, dscale)?;
            }
        }
        let count = i32::try_from(groups.len()).map_err(|_| "numeric")?;
        let last = i32::from(weight) - count + 1;
        if last > 0 {
            let zeros = usize::try_from(last).ok().and_then(|value| value.checked_mul(4)).ok_or("numeric")?;
            if int_part.len().saturating_add(zeros) > DIGIT_BUDGET {
                return Err("numeric");
            }
            int_part.push_str(&"0".repeat(zeros));
        }
        if int_part.is_empty() {
            int_part.push('0');
        }
    }
    if frac_part.len() < dscale {
        frac_part.push_str(&"0".repeat(dscale - frac_part.len()));
    }
    if int_part.len() > DIGIT_BUDGET || frac_part.len() > DIGIT_BUDGET {
        return Err("numeric");
    }
    Ok((int_part, frac_part))
}

fn push_group(out: &mut String, digit: u16, limit: usize) -> Result<(), &'static str> {
    let text = group_digits(digit);
    if out.len() >= limit {
        if text.iter().any(|byte| *byte != b'0') {
            return Err("numeric");
        }
        return Ok(());
    }
    let room = limit - out.len();
    if room >= text.len() {
        out.push_str(std::str::from_utf8(&text).map_err(|_| "numeric")?);
        return Ok(());
    }
    if text[room..].iter().any(|byte| *byte != b'0') {
        return Err("numeric");
    }
    out.push_str(std::str::from_utf8(&text[..room]).map_err(|_| "numeric")?);
    Ok(())
}

fn group_digits(digit: u16) -> [u8; 4] {
    let mut text = [b'0'; 4];
    let mut value = u32::from(digit);
    for slot in text.iter_mut().rev() {
        *slot = b'0' + (value % 10) as u8;
        value /= 10;
    }
    text
}

fn decimal_text(negative: bool, coefficient: &str, scale: i32) -> String {
    let scale = usize::try_from(scale.max(0)).unwrap_or(usize::MAX);
    let digits = if coefficient.is_empty() { "0" } else { coefficient };
    let mut body = if scale == 0 {
        digits.to_string()
    } else if digits.len() > scale {
        let split = digits.len() - scale;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("0.{}{}", "0".repeat(scale - digits.len()), digits)
    };
    if negative {
        body.insert(0, '-');
    }
    body
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[(byte >> 4) as usize]));
        out.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{classify, decode, decimal_text, stop_before_append};

    #[test]
    fn contract_admits_the_import_bases_and_jsonb_only() {
        let admitted = [
            ("bool", 16_u32),
            ("int2", 21),
            ("int4", 23),
            ("int8", 20),
            ("float4", 700),
            ("float8", 701),
            ("text", 25),
            ("varchar", 1043),
            ("bpchar", 1042),
            ("json", 114),
            ("jsonb", 3802),
            ("bytea", 17),
            ("uuid", 2950),
            ("date", 1082),
            ("time", 1083),
            ("timestamp", 1114),
            ("timestamptz", 1184),
        ];
        for (name, oid) in admitted {
            assert!(classify(name, oid, -1, "b").is_some(), "{name}");
        }
        assert!(classify("numeric", 1700, (10 << 16) + 2 + 4, "b").is_some());
        // numeric(76,10), the pinned ClickHouse amount typmod. Precision 77 stays out.
        assert!(classify("numeric", 1700, 4_980_750, "b").is_some());
        assert!(classify("numeric", 1700, ((77 << 16) | 10) + 4, "b").is_none());
        let scale = 0;
        assert!(classify("numeric", 1700, ((1000 << 16) | scale) + 4, "b").is_none());
        assert!(classify("numeric", 1700, -1, "b").is_none());
        let negative_scale = ((10 << 16) | 0x7FF) + 4;
        assert!(classify("numeric", 1700, negative_scale, "b").is_none());
        for (name, oid) in [
            ("interval", 1186_u32),
            ("point", 600),
            ("_int4", 1007),
            ("unknown", 705),
            ("xml", 142),
            ("varchar", 25),
        ] {
            assert!(classify(name, oid, -1, "b").is_none(), "{name}");
        }
        assert!(classify("varchar", 1043, -1, "d").is_none());
        assert!(classify("mood", 16384, -1, "e").is_none());
    }

    #[test]
    fn bytea_keeps_null_empty_zero_non_utf8_and_fixed_lengths() {
        let null = decode(17, None).expect("null bytea");
        assert_eq!(null["kind"], "null");
        assert!(null.get("value").is_none());

        let empty = decode(17, Some(b"")).expect("empty bytea");
        assert_eq!(empty["kind"], "bytea");
        assert_eq!(empty["value"], "");
        assert_eq!(super::cell_len(&empty), 0);

        let zeros = decode(17, Some(&[0, 0, 0, 0])).expect("zero bytea");
        assert_eq!(zeros["kind"], "bytea");
        assert_eq!(zeros["value"], "00000000");
        assert_ne!(zeros["kind"], "text");

        let raw = [0xff, 0xfe, 0x00, 0x41];
        fn utf8_rejected(bytes: &[u8]) -> bool {
            std::str::from_utf8(bytes).is_err()
        }
        assert!(utf8_rejected(&raw));
        let non_utf = decode(17, Some(&raw)).expect("non-utf8 bytea");
        assert_eq!(non_utf["kind"], "bytea");
        assert_eq!(non_utf["value"], "fffe0041");
        assert_eq!(super::cell_len(&non_utf), 4);

        let object_id = [
            0xc1, 0x41, 0xd0, 0x41, 0x41, 0xee, 0xf0, 0x41, 0x00, 0x00, 0x00, 0x00,
        ];
        let id = decode(17, Some(&object_id)).expect("12-byte id");
        assert_eq!(id["kind"], "bytea");
        assert_eq!(id["value"], "c141d04141eef04100000000");
        assert_eq!(super::cell_len(&id), 12);

        let uid_bytes = [
            0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let uid = decode(17, Some(&uid_bytes)).expect("16-byte uid");
        assert_eq!(uid["kind"], "bytea");
        assert_eq!(uid["value"], "11111111222240008000000000000000");
        assert_eq!(super::cell_len(&uid), 16);
    }

    #[test]
    fn uuid_canonical_and_bpchar_width_keep_null_and_raw_text() {
        assert!(classify("uuid", 2950, -1, "b").is_some());
        assert!(classify("uuid", 2950, 0, "b").is_none());
        assert!(classify("uuid", 2950, 4, "b").is_none());
        assert!(classify("bpchar", 1042, -1, "b").is_some());
        assert!(classify("bpchar", 1042, 10, "b").is_some());
        assert!(classify("bpchar", 1042, 0, "b").is_none());
        assert!(classify("bpchar", 1042, 3, "b").is_none());
        assert!(classify("bpchar", 1042, 4, "b").is_none());

        let uid_bytes = [
            0x11, 0x11, 0x11, 0x11, 0x22, 0x22, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let uid = decode(2950, Some(&uid_bytes)).expect("uuid");
        assert_eq!(uid["kind"], "uuid");
        assert_eq!(uid["value"], "11111111-2222-4000-8000-000000000000");
        assert!(decode(2950, Some(&[0_u8; 15])).is_err());
        let null_uuid = decode(2950, None).expect("null uuid");
        assert_eq!(null_uuid["kind"], "null");
        assert!(null_uuid.get("value").is_none());

        let padded = decode(1042, Some(b"ab    ")).expect("bpchar");
        assert_eq!(padded["kind"], "bpchar");
        assert_eq!(padded["value"], "ab    ");
        assert_ne!(padded["value"], "ab");
        let null_char = decode(1042, None).expect("null bpchar");
        assert_eq!(null_char["kind"], "null");
        assert!(null_char.get("value").is_none());
    }

    #[test]
    fn date_and_naive_timestamp_keep_epoch_fraction_and_null() {
        assert!(classify("date", 1082, -1, "b").is_some());
        assert!(classify("date", 1082, 0, "b").is_none());
        assert!(classify("date", 1082, 4, "b").is_none());
        assert!(classify("timestamp", 1114, -1, "b").is_some());
        assert!(classify("timestamp", 1114, 0, "b").is_none());
        assert!(classify("timestamp", 1114, 6, "b").is_none());

        let epoch_day = decode(1082, Some(&0_i32.to_be_bytes())).expect("epoch date");
        assert_eq!(epoch_day["kind"], "date");
        assert_eq!(epoch_day["value"], "2000-01-01");
        let previous = decode(1082, Some(&(-1_i32).to_be_bytes())).expect("previous date");
        assert_eq!(previous["value"], "1999-12-31");
        // 2026-03-11 is the Doris part_day sample, as PostgreSQL days since 2000-01-01.
        let part_day = decode(1082, Some(&9566_i32.to_be_bytes())).expect("part day");
        assert_eq!(part_day["kind"], "date");
        assert_eq!(part_day["value"], "2026-03-11");
        let null_day = decode(1082, None).expect("null date");
        assert_eq!(null_day["kind"], "null");
        assert!(null_day.get("value").is_none());

        let epoch = decode(1114, Some(&0_i64.to_be_bytes())).expect("epoch timestamp");
        assert_eq!(epoch["kind"], "timestamp");
        assert_eq!(epoch["value"], "2000-01-01T00:00:00.000000");
        assert!(!epoch["value"].as_str().unwrap().contains('Z'));
        let fraction = decode(1114, Some(&123_456_i64.to_be_bytes())).expect("fraction");
        assert_eq!(fraction["value"], "2000-01-01T00:00:00.123456");
        let star_second = decode(1114, Some(&1_000_000_i64.to_be_bytes())).expect("second");
        assert_eq!(star_second["value"], "2000-01-01T00:00:01.000000");
        let before = decode(1114, Some(&(-1_i64).to_be_bytes())).expect("before epoch");
        assert_eq!(before["value"], "1999-12-31T23:59:59.999999");
        let stored = decode(1114, Some(&763_821_296_123_456_i64.to_be_bytes())).expect("stored");
        assert_eq!(stored["value"], "2024-03-15T12:34:56.123456");
        assert!(!stored["value"].as_str().unwrap().ends_with('Z'));
        let null_ts = decode(1114, None).expect("null timestamp");
        assert_eq!(null_ts["kind"], "null");
        assert!(null_ts.get("value").is_none());

        let zoned = decode(1184, Some(&0_i64.to_be_bytes())).expect("timestamptz");
        assert_eq!(zoned["value"], "2000-01-01T00:00:00.000000Z");
    }

    #[test]
    fn binary_cells_keep_varchar_float_time_json_and_jsonb() {
        let float_bits = 1.5_f64.to_be_bytes();
        let float_cell = decode(701, Some(&float_bits)).expect("float8");
        assert_eq!(float_cell["kind"], "float8");
        assert_eq!(float_cell["value"], "1.5");
        assert_eq!(float_cell["bits"], format!("{:016x}", 1.5_f64.to_bits()));
        let varchar = decode(1043, Some(br#"{"a":1}"#)).expect("varchar");
        assert_eq!(varchar["kind"], "varchar");
        assert_eq!(varchar["value"], r#"{"a":1}"#);
        let json = decode(114, Some(br#"{"t":"x"}"#)).expect("json");
        assert_eq!(json["kind"], "json");
        assert!(classify("jsonb", 3802, -1, "b").is_some());
        assert!(classify("jsonb", 3802, 0, "b").is_none());
        assert!(classify("jsonb", 3802, 4, "b").is_none());
        assert!(classify("jsonb", 114, -1, "b").is_none());
        assert!(classify("json", 3802, -1, "b").is_none());
        assert!(classify("jsonb", 3802, -1, "d").is_none());
        let jsonb = decode(3802, Some(b"\x01{\"t\":\"x\"}")).expect("jsonb");
        assert_eq!(jsonb["kind"], "jsonb");
        assert_eq!(jsonb["value"], r#"{"t":"x"}"#);
        // PostgreSQL 17 jsonb_send is version 1 plus jsonb::text, spaces included.
        let mut stored = vec![1_u8];
        stored.extend_from_slice(
            br#"{"a": {"n": 9007199254740993}, "z": 1, "box": {}, "tags": ["red", "blue"], "missing": null}"#,
        );
        let document = decode(3802, Some(&stored)).expect("jsonb document");
        assert_eq!(document["kind"], "jsonb");
        assert_eq!(
            document["value"],
            r#"{"a": {"n": 9007199254740993}, "z": 1, "box": {}, "tags": ["red", "blue"], "missing": null}"#
        );
        assert!(document["value"].as_str().unwrap().contains("9007199254740993"));
        let json_null = decode(3802, Some(b"\x01null")).expect("json null");
        assert_eq!(json_null["kind"], "jsonb");
        assert_eq!(json_null["value"], "null");
        let sql_null = decode(3802, None).expect("sql null");
        assert_eq!(sql_null["kind"], "null");
        assert!(sql_null.get("value").is_none());
        assert!(decode(3802, Some(b"\x02{}")).is_err());
        let bytes = decode(17, Some(b"catalog-text")).expect("bytea");
        assert_eq!(bytes["kind"], "bytea");
        assert_ne!(bytes["kind"], "text");
        let epoch = decode(1184, Some(&0_i64.to_be_bytes())).expect("epoch");
        assert_eq!(epoch["kind"], "timestamptz");
        assert_eq!(epoch["value"], "2000-01-01T00:00:00.000000Z");
        assert_eq!(epoch["micros"], "0");
        assert!(decode(1186, Some(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])).is_err());
    }

    #[test]
    fn numeric_display_matches_the_reviewed_wire_shapes() {
        let one = decode_groups(0, 0, 2, &[1, 100]);
        assert_eq!(decimal_text(one.0, &one.1, one.2), "1.01");
        let cent = decode_groups(-1, 0, 2, &[100]);
        assert_eq!(decimal_text(cent.0, &cent.1, cent.2), "0.01");
        let neg = decode_groups(0, 0x4000, 2, &[1, 100]);
        assert_eq!(decimal_text(neg.0, &neg.1, neg.2), "-1.01");
        let tiny = decode_groups(-2, 0, 6, &[100]);
        assert_eq!(decimal_text(tiny.0, &tiny.1, tiny.2), "0.000001");
        let padded = decode_groups(0, 0, 4, &[1]);
        assert_eq!(decimal_text(padded.0, &padded.1, padded.2), "1.0000");
        assert_eq!(padded.1, "10000");
        let wide = decode_groups(18, 0, 0, &[9999; 19]);
        assert_eq!(wide.1.len(), 76);
        assert!(wide.1.chars().all(|ch| ch == '9'));
        assert_eq!(decimal_text(wide.0, &wide.1, wide.2), "9".repeat(76));
        let mut too_wide = Vec::new();
        too_wide.extend_from_slice(&20_i16.to_be_bytes());
        too_wide.extend_from_slice(&19_i16.to_be_bytes());
        too_wide.extend_from_slice(&0_u16.to_be_bytes());
        too_wide.extend_from_slice(&0_i16.to_be_bytes());
        for _ in 0..20 {
            too_wide.extend_from_slice(&9999_i16.to_be_bytes());
        }
        assert!(super::decode_numeric(&too_wide).is_err());
    }

    #[test]
    fn byte_limit_keeps_a_fitting_first_row() {
        assert!(!stop_before_append(0, 10, 0, 20, 20));
        assert!(stop_before_append(0, 10, 20, 20, 20));
        assert!(stop_before_append(1, 1, 0, 1, 100));
    }

    fn decode_groups(weight: i16, sign: u16, dscale: i16, digits: &[u16]) -> (bool, String, i32) {
        let mut raw = Vec::new();
        raw.extend_from_slice(&i16::try_from(digits.len()).unwrap().to_be_bytes());
        raw.extend_from_slice(&weight.to_be_bytes());
        raw.extend_from_slice(&sign.to_be_bytes());
        raw.extend_from_slice(&dscale.to_be_bytes());
        for digit in digits {
            raw.extend_from_slice(&i16::try_from(*digit).unwrap().to_be_bytes());
        }
        super::decode_numeric(&raw).expect("numeric")
    }
}
