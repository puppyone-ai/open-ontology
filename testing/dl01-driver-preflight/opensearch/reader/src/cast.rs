//! Precision rules for OpenSearch JSON values. Unsafe casts return errors.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use serde::de::{Deserializer as _, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use serde_json::{Deserializer, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    Text(String),
    Bytes(Vec<u8>),
    TimestampMillis(i64),
    ListText(Vec<Option<String>>),
    Struct(BTreeMap<String, (bool, Cell)>),
    Json(String),
}

#[derive(Clone, Debug)]
pub struct FieldPlan {
    pub name: String,
    pub os_type: String,
    pub nullable: bool,
    pub unsupported: bool,
    pub unsupported_reason: Option<String>,
    pub caveat: Option<String>,
    pub scaling_factor: Option<i64>,
    pub children: Vec<FieldPlan>,
    pub json_text: bool,
    pub metadata: BTreeMap<String, String>,
}

pub fn reject_unsafe_pair(os_type: &str, arrow: &str) -> Result<(), String> {
    const BLOCKED: &[(&str, &str)] = &[
        ("scaled_float", "float64"),
        ("unsigned_long", "int64"),
        ("double", "int64"),
        ("date", "utf8"),
        ("binary", "utf8"),
        ("boolean", "int64"),
        ("long", "float64"),
    ];
    if BLOCKED.contains(&(os_type, arrow)) {
        return Err(format!("unsafe_cast:{os_type}_to_{arrow}"));
    }
    Ok(())
}

pub fn f64_from_exact_lexeme(lexeme: &str) -> Result<f64, String> {
    let value: f64 = lexeme
        .parse()
        .map_err(|_| format!("unsafe_float:{lexeme}"))?;
    if !value.is_finite() || !decimal_equals_f64(lexeme, value) {
        return Err(format!("unsafe_float:{lexeme}"));
    }
    Ok(value)
}

pub fn scaled_unscaled(lexeme: &str, factor: i64) -> Result<i64, String> {
    if factor <= 0 {
        return Err("scaled_float:bad_factor".into());
    }
    let Some((negative, digits, scale)) = parse_decimal(lexeme) else {
        return Err(format!("scaled_float:not_decimal:{lexeme}"));
    };
    if digits.len() > 24 || scale > 18 {
        return Err(format!("scaled_float:overflow:{lexeme}"));
    }
    let mut magnitude: i128 = 0;
    for byte in digits.bytes() {
        magnitude = magnitude * 10 + i128::from(byte - b'0');
    }
    let ten = pow10_i128(scale);
    let numerator = magnitude * i128::from(factor);
    if ten == 0 || numerator % ten != 0 {
        return Err(format!("scaled_float:inexact:{lexeme}"));
    }
    let whole = numerator / ten;
    if whole > i128::from(i64::MAX) {
        return Err(format!("scaled_float:overflow:{lexeme}"));
    }
    let signed = if negative { -whole } else { whole };
    i64::try_from(signed).map_err(|_| format!("scaled_float:overflow:{lexeme}"))
}

pub fn parse_i64_lexeme(lexeme: &str) -> Result<i64, String> {
    if lexeme.contains(['.', 'e', 'E', '+']) || lexeme.is_empty() {
        return Err(format!("unsafe_cast:non_integer_long:{lexeme}"));
    }
    lexeme
        .parse::<i64>()
        .map_err(|_| format!("unsafe_cast:long:{lexeme}"))
}

fn parse_decimal(lexeme: &str) -> Option<(bool, String, u32)> {
    let mut rest = lexeme;
    let negative = if let Some(stripped) = rest.strip_prefix('-') {
        rest = stripped;
        true
    } else {
        false
    };
    if rest.is_empty() || rest.starts_with('+') || rest.contains(['e', 'E']) {
        return None;
    }
    let (whole, frac) = match rest.split_once('.') {
        Some((whole, frac)) => (whole, frac),
        None => (rest, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((negative, format!("{whole}{frac}"), frac.len() as u32))
}

fn decimal_equals_f64(lexeme: &str, value: f64) -> bool {
    let Some((negative, digits, scale)) = parse_decimal(lexeme) else {
        return false;
    };
    if digits.len() > 20 || scale > 18 {
        return false;
    }
    let mut integer: u128 = 0;
    for byte in digits.bytes() {
        integer = integer * 10 + u128::from(byte - b'0');
    }
    if value == 0.0 {
        return integer == 0;
    }
    let bits = value.to_bits();
    let sign = (bits >> 63) != 0;
    if sign != negative {
        return false;
    }
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (mantissa, power) = if exponent == 0 {
        (u128::from(fraction), -1022 - 52)
    } else {
        (u128::from((1_u64 << 52) | fraction), exponent - 1023 - 52)
    };
    let ten = pow10_u128(scale);
    if power >= 0 {
        let shift = power as u32;
        if shift >= 128 {
            return false;
        }
        let Some(shifted) = mantissa.checked_shl(shift) else {
            return false;
        };
        let Some(right) = shifted.checked_mul(ten) else {
            return false;
        };
        integer == right
    } else {
        let shift = (-power) as u32;
        if shift >= 128 {
            return false;
        }
        let Some(left) = integer.checked_shl(shift) else {
            return false;
        };
        let Some(right) = mantissa.checked_mul(ten) else {
            return false;
        };
        left == right
    }
}

fn pow10_u128(scale: u32) -> u128 {
    let mut value = 1_u128;
    for _ in 0..scale {
        value *= 10;
    }
    value
}

fn pow10_i128(scale: u32) -> i128 {
    let mut value = 1_i128;
    for _ in 0..scale {
        value *= 10;
    }
    value
}

pub fn raw_fields(text: &str) -> Result<BTreeMap<String, String>, String> {
    struct Fields;
    impl<'de> Visitor<'de> for Fields {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some(key) = access.next_key::<String>()? {
                let value = access.next_value::<Box<RawValue>>()?;
                out.insert(key, value.get().to_string());
            }
            Ok(out)
        }
    }
    let mut deserializer = Deserializer::from_str(text);
    let fields = deserializer.deserialize_map(Fields).map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(fields)
}

fn raw_items(text: &str) -> Result<Vec<String>, String> {
    struct Items;
    impl<'de> Visitor<'de> for Items {
        type Value = Vec<String>;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("array")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(value) = access.next_element::<Box<RawValue>>()? {
                out.push(value.get().to_string());
            }
            Ok(out)
        }
    }
    let mut deserializer = Deserializer::from_str(text);
    let items = deserializer.deserialize_seq(Items).map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(items)
}

pub fn encode_field(plan: &FieldPlan, raw: Option<&str>) -> Result<(bool, Cell), String> {
    let Some(raw) = raw else {
        return Ok((false, Cell::Null));
    };
    let text = raw.trim();
    if text == "null" {
        return Ok((true, Cell::Null));
    }
    if plan.unsupported {
        return Ok((true, Cell::Null));
    }
    if plan.json_text {
        if !text.starts_with('{') {
            return Err(format!("json_object_required:{}", plan.name));
        }
        return Ok((true, Cell::Json(text.to_string())));
    }
    match plan.os_type.as_str() {
        "boolean" => encode_bool(text).map(|value| (true, Cell::Bool(value))),
        "long" => parse_i64_lexeme(text).map(|value| (true, Cell::I64(value))),
        "double" => f64_from_exact_lexeme(text).map(|value| (true, Cell::F64(value))),
        "scaled_float" => {
            let factor = plan.scaling_factor.ok_or_else(|| "scaled_float:missing_factor".to_string())?;
            scaled_unscaled(text, factor).map(|value| (true, Cell::I64(value)))
        }
        "keyword" | "text" => encode_string(text).map(|value| (true, Cell::Text(value))),
        "binary" => encode_binary(text).map(|value| (true, Cell::Bytes(value))),
        "date" => parse_i64_lexeme(text).map(|value| (true, Cell::TimestampMillis(value))),
        "object" => encode_struct(plan, text).map(|value| (true, value)),
        other => Err(format!("unhandled_type:{other}")),
    }
}

fn encode_bool(text: &str) -> Result<bool, String> {
    match text {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("unsafe_cast:boolean:{text}")),
    }
}

fn encode_string(text: &str) -> Result<String, String> {
    serde_json::from_str(text).map_err(|_| format!("utf8_required:{text}"))
}

fn encode_binary(text: &str) -> Result<Vec<u8>, String> {
    let encoded = encode_string(text)?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(encoded.as_bytes()))
        .map_err(|_| format!("unsafe_cast:binary:{encoded}"))
}

fn encode_struct(plan: &FieldPlan, text: &str) -> Result<Cell, String> {
    let map = raw_fields(text).map_err(|error| format!("object_required:{}:{error}", plan.name))?;
    let known: BTreeMap<&str, &FieldPlan> = plan.children.iter().map(|child| (child.name.as_str(), child)).collect();
    for key in map.keys() {
        if !known.contains_key(key.as_str()) {
            return Err(format!("strict_object_extra:{}:{key}", plan.name));
        }
    }
    let mut out = BTreeMap::new();
    for child in &plan.children {
        let (present, cell) = encode_field(child, map.get(&child.name).map(String::as_str))?;
        out.insert(child.name.clone(), (present, cell));
    }
    Ok(Cell::Struct(out))
}

pub fn encode_multivalue(plan: &FieldPlan, raw: Option<&str>) -> Result<(bool, Cell), String> {
    let Some(raw) = raw else {
        return Ok((false, Cell::Null));
    };
    let text = raw.trim();
    if text == "null" {
        return Ok((true, Cell::Null));
    }
    if !text.starts_with('[') {
        return Err(format!("multivalue_not_array:{}", plan.name));
    }
    let items = raw_items(text).map_err(|error| format!("array_required:{}:{error}", plan.name))?;
    match plan.os_type.as_str() {
        "keyword" | "text" => {
            let mut values = Vec::with_capacity(items.len());
            for item in &items {
                let (present, cell) = encode_field(plan, Some(item.as_str()))?;
                if !present {
                    values.push(None);
                } else {
                    match cell {
                        Cell::Null => values.push(None),
                        Cell::Text(value) => values.push(Some(value)),
                        _ => return Err(format!("list_element_type:{}", plan.name)),
                    }
                }
            }
            Ok((true, Cell::ListText(values)))
        }
        "long" => {
            for item in &items {
                let (present, cell) = encode_field(plan, Some(item.as_str()))?;
                if present {
                    match cell {
                        Cell::Null | Cell::I64(_) => {}
                        _ => return Err(format!("list_element_type:{}", plan.name)),
                    }
                }
            }
            // Null elements cannot be list_int64. Keep the original JSON array text.
            Ok((true, Cell::Json(text.trim().to_string())))
        }
        _ => Err(format!("list_unsupported:{}", plan.os_type)),
    }
}

pub fn source_keys_known(raw: &str, plans: &[FieldPlan]) -> Result<(), String> {
    let map: serde_json::Map<String, Value> = serde_json::from_str(raw).map_err(|error| format!("source_json:{error}"))?;
    let known: BTreeMap<&str, ()> = plans.iter().map(|plan| (plan.name.as_str(), ())).collect();
    for key in map.keys() {
        if !known.contains_key(key.as_str()) {
            return Err(format!("unmapped_field_in_source:{key}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_scaled_float_as_float64() {
        let error = reject_unsafe_pair("scaled_float", "float64").unwrap_err();
        assert_eq!(error, "unsafe_cast:scaled_float_to_float64");
    }

    #[test]
    fn exact_doubles_and_rejects_tenth() {
        assert_eq!(f64_from_exact_lexeme("0.5").unwrap().to_bits(), 0.5_f64.to_bits());
        assert_eq!(f64_from_exact_lexeme("-1.5").unwrap().to_bits(), (-1.5_f64).to_bits());
        assert_eq!(f64_from_exact_lexeme("2.0").unwrap().to_bits(), 2.0_f64.to_bits());
        assert_eq!(f64_from_exact_lexeme("0.0").unwrap().to_bits(), 0.0_f64.to_bits());
        assert!(f64_from_exact_lexeme("0.1").is_err());
    }

    #[test]
    fn scaled_factor_is_exact_integer() {
        assert_eq!(scaled_unscaled("1.25", 100).unwrap(), 125);
        assert_eq!(scaled_unscaled("-2.5", 100).unwrap(), -250);
        assert_eq!(scaled_unscaled("0", 100).unwrap(), 0);
        assert_eq!(scaled_unscaled("0.25", 100).unwrap(), 25);
        assert!(scaled_unscaled("1.255", 100).is_err());
    }

    #[test]
    fn long_bounds_and_rejects_fraction() {
        assert_eq!(parse_i64_lexeme("9223372036854775807").unwrap(), i64::MAX);
        assert_eq!(parse_i64_lexeme("-9223372036854775808").unwrap(), i64::MIN);
        assert!(parse_i64_lexeme("1.0").is_err());
        assert!(parse_i64_lexeme("true").is_err());
    }

    #[test]
    fn boolean_rejects_numeric() {
        assert!(encode_bool("true").unwrap());
        assert!(!encode_bool("false").unwrap());
        assert!(encode_bool("1").is_err());
        assert!(encode_bool("0").is_err());
    }

    #[test]
    fn binary_decodes_padded_and_rejects_text_cast() {
        let plan = FieldPlan {
            name: "blob".into(),
            os_type: "binary".into(),
            nullable: true,
            unsupported: false,
            unsupported_reason: None,
            caveat: None,
            scaling_factor: None,
            children: Vec::new(),
            json_text: false,
            metadata: BTreeMap::new(),
        };
        let (present, cell) = encode_field(&plan, Some("\"AP8Q\"")).unwrap();
        assert!(present);
        assert_eq!(cell, Cell::Bytes(vec![0x00, 0xff, 0x10]));
        assert!(reject_unsafe_pair("binary", "utf8").is_err());
    }
}
