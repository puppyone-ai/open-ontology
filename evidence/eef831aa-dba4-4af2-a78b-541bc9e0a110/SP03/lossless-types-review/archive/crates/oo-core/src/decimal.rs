//! Exact decimal and integer values.
//!
//! Amounts, positions, and numeric coefficients are digit strings. Nothing in
//! this module stores those values as `f64`. SQL scale is preserved, including
//! a negative scale: coefficient `12` at scale `-2` is `1200`. JSON numbers
//! drop trailing fractional zeros after the exponent is applied and fold a
//! negative adjusted scale back to scale 0 by appending zeros, so `1`, `1.0`,
//! and `1e0` stay one value. A ratio keeps the lexical JSON/YAML spelling and
//! does not take that JSON normalization step.

use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::limits::CANONICAL_MAX_COEFFICIENT_DIGITS;
use crate::schema_util::schema_object;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecimalStorage {
    Decimal128,
    Decimal256,
    DecimalString,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DecimalError {
    #[error("decimal is empty")]
    Empty,
    #[error("decimal is not finite")]
    NonFinite,
    #[error("decimal exceeds the coefficient digit budget")]
    Budget,
    #[error("invalid decimal `{0}`")]
    Invalid(String),
    #[error("JSON number is not an exact decimal string")]
    JsonNumber,
    #[error("decimal rescale is not exact")]
    Inexact,
}

/// Signed decimal with an explicit scale.
///
/// `digits` is the absolute coefficient without a decimal point and without
/// leading zeros (`0` for zero). `scale` is signed. A positive scale is the
/// number of fractional digits and may be larger than `digits.len()`
/// (`0.001` is digits `1`, scale `3`). A negative scale appends zeros:
/// digits `12` at scale `-2` is `1200`. Trailing coefficient zeros are
/// significant for SQL values. The schema sentinel `i16::MIN` is not a value
/// scale; [`Self::new`] rejects a scale whose magnitude exceeds the digit budget.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExactDecimal {
    negative: bool,
    digits: String,
    scale: i32,
}

impl ExactDecimal {
    pub fn new(
        negative: bool,
        digits: impl Into<String>,
        scale: i32,
    ) -> Result<Self, DecimalError> {
        let mut digits = digits.into();
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(DecimalError::Invalid(digits));
        }
        if digits.len() > CANONICAL_MAX_COEFFICIENT_DIGITS
            || scale.unsigned_abs() as usize > CANONICAL_MAX_COEFFICIENT_DIGITS
        {
            return Err(DecimalError::Budget);
        }
        strip_leading_zeros(&mut digits);
        let negative = negative && digits != "0";
        Ok(Self {
            negative,
            digits,
            scale,
        })
    }

    pub fn zero(scale: i32) -> Result<Self, DecimalError> {
        Self::new(false, "0", scale)
    }

    /// SQL decimal or integer literal. Exponents are rejected.
    pub fn parse_sql(input: &str) -> Result<Self, DecimalError> {
        parse_decimal(input, false)
    }

    /// JSON number text, including an exponent. The result keeps the exponent-
    /// adjusted scale, including trailing zeros. Call [`Self::normalize_json`]
    /// before hashing a JSON number.
    pub fn parse_json_number(input: &str) -> Result<Self, DecimalError> {
        parse_decimal(input, true)
    }

    /// JSON canonical step. Trailing fractional zeros drop. A zero coefficient
    /// becomes unsigned scale 0, including `-0`, `0.00`, `0e100`, and `0e-100`.
    /// [`Self::parse_sql`] does not call this and keeps lexical scale.
    pub fn normalize_json(mut self) -> Self {
        while self.scale > 0 && self.digits.ends_with('0') {
            self.digits.pop();
            self.scale -= 1;
        }
        if self.digits.is_empty() {
            self.digits.push('0');
        }
        strip_leading_zeros(&mut self.digits);
        if self.digits == "0" {
            self.negative = false;
            self.scale = 0;
        }
        self
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }

    pub fn coefficient(&self) -> &str {
        &self.digits
    }

    pub fn scale(&self) -> i32 {
        self.scale
    }

    /// Significant digits. A negative scale does not add fractional digits, so
    /// the storage width is the coefficient length. A positive scale can be
    /// wider than the coefficient (`0.001`).
    pub fn storage_precision(&self) -> usize {
        if self.scale <= 0 {
            self.digits.len()
        } else {
            self.digits.len().max(self.scale as usize)
        }
    }

    pub fn fits_decimal128(&self) -> bool {
        self.digits.len() <= 38 && (-38..=38).contains(&self.scale)
    }

    pub fn fits_decimal256(&self) -> bool {
        self.digits.len() <= 76 && (-76..=76).contains(&self.scale)
    }

    /// Move to `target` scale without changing the number. A larger scale
    /// appends zeros. A smaller scale strips trailing zeros or fails.
    pub fn rescale_exact(&self, target: i32) -> Result<Self, DecimalError> {
        if target.unsigned_abs() as usize > CANONICAL_MAX_COEFFICIENT_DIGITS {
            return Err(DecimalError::Budget);
        }
        let delta = target.checked_sub(self.scale).ok_or(DecimalError::Budget)?;
        if delta == 0 {
            return Ok(self.clone());
        }
        if delta > 0 {
            let zeros = usize::try_from(delta).map_err(|_| DecimalError::Budget)?;
            if self.digits.len().saturating_add(zeros) > CANONICAL_MAX_COEFFICIENT_DIGITS {
                return Err(DecimalError::Budget);
            }
            let mut digits = self.digits.clone();
            digits.extend(std::iter::repeat_n('0', zeros));
            return Self::new(self.negative, digits, target);
        }
        let strip = usize::try_from(-delta).map_err(|_| DecimalError::Budget)?;
        if self.digits.len() < strip || !self.digits.ends_with(&"0".repeat(strip)) {
            return Err(DecimalError::Inexact);
        }
        let mut digits = self.digits.clone();
        digits.truncate(digits.len() - strip);
        Self::new(self.negative, digits, target)
    }

    pub fn storage_class(&self) -> DecimalStorage {
        if self.fits_decimal128() {
            DecimalStorage::Decimal128
        } else if self.fits_decimal256() {
            DecimalStorage::Decimal256
        } else {
            DecimalStorage::DecimalString
        }
    }

    /// `decimal128(38,0)` position: non-negative, scale 0, at most 38 digits
    /// (`<= 10^38 - 1`).
    pub fn fits_position(&self) -> bool {
        !self.negative && self.scale == 0 && self.digits.len() <= 38
    }

    pub fn to_plain_string(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for ExactDecimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            write!(f, "-")?;
        }
        if self.scale == 0 {
            return write!(f, "{}", self.digits);
        }
        if self.scale < 0 {
            write!(f, "{}", self.digits)?;
            for _ in 0..self.scale.unsigned_abs() {
                write!(f, "0")?;
            }
            return Ok(());
        }
        let scale = self.scale as usize;
        if self.digits.len() > scale {
            let split = self.digits.len() - scale;
            write!(f, "{}.{}", &self.digits[..split], &self.digits[split..])
        } else {
            let zeros = scale - self.digits.len();
            write!(f, "0.")?;
            for _ in 0..zeros {
                write!(f, "0")?;
            }
            write!(f, "{}", self.digits)
        }
    }
}

impl Serialize for ExactDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ExactDecimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse_sql(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ExactDecimal {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ExactDecimal")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schema_object(serde_json::json!({
            "type": "string",
            "pattern": r"^[+-]?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$",
            "minLength": 1,
            "maxLength": CANONICAL_MAX_COEFFICIENT_DIGITS + 2,
            "description": "Exact decimal string. JSON numbers are rejected for this type."
        }))
    }
}

/// Ratio or confidence that may be authored as a YAML/JSON number or as a
/// decimal string. The stored value is always an [`ExactDecimal`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct FlexDecimal(pub ExactDecimal);

impl<'de> Deserialize<'de> for FlexDecimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `serde_json` with `arbitrary_precision` still calls `visit_f64` when
        // the lexical form round-trips through `f64`. Reading `Value` first
        // keeps `Number`'s string, including integers that do not fit in `u64`.
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(text) => ExactDecimal::parse_sql(&text)
                .map(FlexDecimal)
                .map_err(serde::de::Error::custom),
            serde_json::Value::Number(number) => {
                let lexical = number.to_string();
                ExactDecimal::parse_json_number(&lexical)
                    .map(FlexDecimal)
                    .map_err(serde::de::Error::custom)
            }
            _ => Err(serde::de::Error::custom(
                "a ratio must be a decimal string or a JSON number",
            )),
        }
    }
}

impl utoipa::PartialSchema for ExactDecimal {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .pattern(Some(r"^[+-]?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$"))
            .description(Some(
                "Exact decimal string. JSON numbers are rejected for this type.",
            ))
            .into()
    }
}

impl utoipa::ToSchema for ExactDecimal {}

/// Source position: scale 0, non-negative, at most 38 digits.
///
/// This is the `decimal128(38,0)` position rule ([`ExactDecimal::fits_position`]).
/// It is not [`ExactDecimal::fits_decimal128`], which allows scale `-38..=38`.
/// A negative Arrow decimal scale stays valid on the decimal type path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Position {
    value: ExactDecimal,
}

impl Position {
    pub fn new(value: ExactDecimal) -> Result<Self, &'static str> {
        let position = Self { value };
        position.check()?;
        Ok(position)
    }

    pub fn as_decimal(&self) -> &ExactDecimal {
        &self.value
    }

    pub fn check(&self) -> Result<(), &'static str> {
        if self.value.fits_position() {
            Ok(())
        } else {
            Err("applied_pos must be a non-negative scale-0 decimal of at most 38 digits")
        }
    }
}

impl Serialize for Position {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.check().map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&self.value.to_string())
    }
}

impl<'de> Deserialize<'de> for Position {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let value = parse_position_lexical(&text).map_err(serde::de::Error::custom)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Schema pattern `^[+]?[0-9]{1,38}$`. No trim, no minus, no exponent.
/// [`Position::new`] remains the value constructor.
fn parse_position_lexical(text: &str) -> Result<ExactDecimal, String> {
    const PATTERN: &str = "^[+]?[0-9]{1,38}$";
    if text.len() > 39 {
        return Err("applied_pos exceeds the 39-character lexical budget".to_owned());
    }
    let digits = match text.as_bytes().first() {
        Some(b'+') => &text[1..],
        Some(b'-') => {
            return Err(format!(
                "applied_pos rejects a leading minus; the lexical pattern is {PATTERN}"
            ));
        }
        Some(_) => text,
        None => return Err(format!("applied_pos must match {PATTERN}")),
    };
    if digits.is_empty() || digits.len() > 38 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("applied_pos must match {PATTERN}"));
    }
    ExactDecimal::new(false, digits, 0)
        .map_err(|_| "applied_pos exceeds the coefficient budget".to_owned())
}

impl JsonSchema for Position {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Position")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schema_object(serde_json::json!({
            "type": "string",
            "pattern": r"^[+]?[0-9]{1,38}$",
            "minLength": 1,
            "maxLength": 39,
            "description": "Source position: non-negative scale-0 decimal of at most 38 digits. JSON numbers are rejected."
        }))
    }
}

impl utoipa::PartialSchema for Position {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .pattern(Some(r"^[+]?[0-9]{1,38}$"))
            .min_length(Some(1))
            .max_length(Some(39))
            .description(Some(
                "Source position: non-negative scale-0 decimal of at most 38 digits. JSON numbers are rejected.",
            ))
            .into()
    }
}

impl utoipa::ToSchema for Position {}

impl utoipa::PartialSchema for FlexDecimal {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        let string = utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::String)
            .pattern(Some(r"^[+-]?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$"))
            .build();
        let number = utoipa::openapi::schema::ObjectBuilder::new()
            .schema_type(utoipa::openapi::schema::Type::Number)
            .description(Some("Finite JSON number. Stored as an exact decimal."))
            .build();
        utoipa::openapi::schema::OneOfBuilder::new()
            .item(string)
            .item(number)
            .description(Some(
                "Finite ratio. The Rust value is an exact decimal, not an f64.",
            ))
            .into()
    }
}

impl utoipa::ToSchema for FlexDecimal {}

impl JsonSchema for FlexDecimal {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("FlexDecimal")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        schema_object(serde_json::json!({
            "oneOf": [
                {
                    "type": "string",
                    "pattern": r"^[+-]?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$",
                    "minLength": 1,
                    "maxLength": 64
                },
                { "type": "number" }
            ],
            "description": "Ratio or confidence. A JSON number is the lexical decimal, not an f64. SQL amount fields stay strings. Exponents that expand past 8192 digits are rejected before any binary float conversion."
        }))
    }
}

/// Unbounded integer. No fractional part and no exponent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExactInt {
    negative: bool,
    digits: String,
}

impl ExactInt {
    pub fn parse(input: &str) -> Result<Self, DecimalError> {
        let decimal = ExactDecimal::parse_sql(input)?;
        if decimal.scale != 0 {
            return Err(DecimalError::Invalid(input.to_owned()));
        }
        Ok(Self {
            negative: decimal.negative,
            digits: decimal.digits,
        })
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }

    pub fn coefficient(&self) -> &str {
        &self.digits
    }
}

impl fmt::Display for ExactInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            write!(f, "-")?;
        }
        write!(f, "{}", self.digits)
    }
}

fn strip_leading_zeros(digits: &mut String) {
    let zeros = digits.chars().take_while(|ch| *ch == '0').count();
    if zeros == 0 {
        return;
    }
    if zeros == digits.len() {
        digits.clear();
        digits.push('0');
        return;
    }
    let rest = digits[zeros..].to_owned();
    *digits = rest;
}

fn parse_decimal(input: &str, allow_exponent: bool) -> Result<ExactDecimal, DecimalError> {
    let text = input.trim();
    if text.is_empty() || text.len() > CANONICAL_MAX_COEFFICIENT_DIGITS + 8 {
        return Err(if text.is_empty() {
            DecimalError::Empty
        } else {
            DecimalError::Budget
        });
    }
    let (negative, rest) = match text.as_bytes()[0] {
        b'+' => (false, &text[1..]),
        b'-' => (true, &text[1..]),
        _ => (false, text),
    };
    if rest.is_empty() {
        return Err(DecimalError::Invalid(text.to_owned()));
    }
    let (body, exponent) = if allow_exponent {
        split_exponent(rest)?
    } else if rest.bytes().any(|b| b == b'e' || b == b'E') {
        return Err(DecimalError::Invalid(text.to_owned()));
    } else {
        (rest, 0i32)
    };
    let (int_part, frac_part) =
        split_fraction(body).map_err(|_| DecimalError::Invalid(text.to_owned()))?;
    if int_part.is_empty() && !allow_exponent {
        return Err(DecimalError::Invalid(text.to_owned()));
    }
    if !int_part.is_empty() && !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(DecimalError::Invalid(text.to_owned()));
    }
    if let Some(frac) = frac_part
        && (frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(DecimalError::Invalid(text.to_owned()));
    }
    let mut digits = String::new();
    digits.push_str(if int_part.is_empty() { "0" } else { int_part });
    let mut scale = 0i32;
    if let Some(frac) = frac_part {
        digits.push_str(frac);
        scale = i32::try_from(frac.len()).map_err(|_| DecimalError::Budget)?;
    }
    apply_exponent(&mut digits, &mut scale, exponent)?;
    ExactDecimal::new(negative, digits, scale)
}

fn split_exponent(input: &str) -> Result<(&str, i32), DecimalError> {
    let Some(pos) = input.find(['e', 'E']) else {
        return Ok((input, 0));
    };
    let body = &input[..pos];
    let exp = &input[pos + 1..];
    if exp.is_empty() {
        return Err(DecimalError::Invalid(input.to_owned()));
    }
    let value: i32 = exp
        .parse()
        .map_err(|_| DecimalError::Invalid(input.to_owned()))?;
    Ok((body, value))
}

fn split_fraction(input: &str) -> Result<(&str, Option<&str>), ()> {
    match input.split_once('.') {
        Some((int_part, frac)) => {
            if frac.contains('.') {
                Err(())
            } else {
                Ok((int_part, Some(frac)))
            }
        }
        None => Ok((input, None)),
    }
}

fn apply_exponent(digits: &mut String, scale: &mut i32, exponent: i32) -> Result<(), DecimalError> {
    if exponent == 0 {
        return Ok(());
    }
    let next = scale.checked_sub(exponent).ok_or(DecimalError::Budget)?;
    if next >= 0 {
        if next as usize > CANONICAL_MAX_COEFFICIENT_DIGITS {
            return Err(DecimalError::Budget);
        }
        *scale = next;
        return Ok(());
    }
    let zeros = usize::try_from(-next).map_err(|_| DecimalError::Budget)?;
    if digits.len().saturating_add(zeros) > CANONICAL_MAX_COEFFICIENT_DIGITS {
        return Err(DecimalError::Budget);
    }
    digits.extend(std::iter::repeat_n('0', zeros));
    *scale = 0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_scale_is_significant() {
        let a = ExactDecimal::parse_sql("1.20").unwrap();
        let b = ExactDecimal::parse_sql("1.2").unwrap();
        assert_ne!(a, b);
        assert_eq!(a.scale(), 2);
        assert_eq!(a.coefficient(), "120");
        assert_eq!(a.to_string(), "1.20");
    }

    #[test]
    fn negative_zero_collapses_and_keeps_scale() {
        let a = ExactDecimal::parse_sql("-0.00").unwrap();
        let b = ExactDecimal::parse_sql("0.00").unwrap();
        assert_eq!(a, b);
        assert!(!a.is_negative());
        assert_eq!(a.scale(), 2);
        assert_ne!(a, ExactDecimal::parse_sql("0.0").unwrap());
    }

    #[test]
    fn json_numbers_with_exponents_match() {
        let a = ExactDecimal::parse_json_number("1")
            .unwrap()
            .normalize_json();
        let b = ExactDecimal::parse_json_number("1.0")
            .unwrap()
            .normalize_json();
        let c = ExactDecimal::parse_json_number("1e0")
            .unwrap()
            .normalize_json();
        let d = ExactDecimal::parse_json_number("1.20")
            .unwrap()
            .normalize_json();
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(d.to_string(), "1.2");
    }

    #[test]
    fn json_zero_coefficient_is_unsigned_scale_zero() {
        for text in ["0", "-0", "0.00", "-0.000", "0e100", "0e-100", "0.00e10"] {
            let once = ExactDecimal::parse_json_number(text)
                .unwrap()
                .normalize_json();
            assert_eq!(once.coefficient(), "0", "{text}");
            assert_eq!(once.scale(), 0, "{text}");
            assert!(!once.is_negative(), "{text}");
            assert_eq!(once.clone().normalize_json(), once, "{text}");
        }
        for scale in [-100, -1, 0, 1, 2, 100] {
            for negative in [false, true] {
                let once = ExactDecimal::new(negative, "0", scale)
                    .unwrap()
                    .normalize_json();
                assert_eq!(once.scale(), 0);
                assert!(!once.is_negative());
                assert_eq!(once.coefficient(), "0");
                assert_eq!(once.clone().normalize_json(), once);
            }
        }
        let sql = ExactDecimal::parse_sql("0.00").unwrap();
        assert_eq!(sql.scale(), 2);
        assert_ne!(sql, ExactDecimal::parse_sql("0.0").unwrap());
        assert_ne!(sql, ExactDecimal::parse_sql("0").unwrap());
        assert_eq!(sql, ExactDecimal::parse_sql("-0.00").unwrap());
    }

    #[test]
    fn position_lexical_pattern_is_stricter_than_the_value_constructor() {
        let accept = ["0", "+0", "00", "+1", "1", "18446744073709551615"];
        for text in accept {
            let parsed: Position = serde_json::from_str(&format!("\"{text}\"")).unwrap();
            let dumped = serde_json::to_string(&parsed).unwrap();
            assert!(serde_json::from_str::<Position>(&dumped).is_ok(), "{text}");
        }
        let plus = serde_json::from_str::<Position>("\"+1\"").unwrap();
        assert_eq!(serde_json::to_string(&plus).unwrap(), "\"1\"");
        let zeros = serde_json::from_str::<Position>("\"00\"").unwrap();
        assert_eq!(zeros.as_decimal().coefficient(), "0");
        assert_eq!(serde_json::to_string(&zeros).unwrap(), "\"0\"");
        for text in ["-0", " 1", "1 ", "+ 1", "1.0", "1.50", "1e2", "1E2", "-1"] {
            assert!(
                serde_json::from_str::<Position>(&format!("\"{text}\"")).is_err(),
                "{text}"
            );
        }
        assert!(serde_json::from_str::<Position>(&format!("\"{}\"", "9".repeat(39))).is_err());
        assert!(serde_json::from_str::<Position>("1").is_err());
        let from_sql = Position::new(ExactDecimal::parse_sql("-0").unwrap()).unwrap();
        assert!(!from_sql.as_decimal().is_negative());
        assert!(Position::new(ExactDecimal::parse_sql(" 1").unwrap()).is_ok());
        assert!(Position::new(ExactDecimal::parse_sql("1.50").unwrap()).is_err());
    }

    #[test]
    fn position_bound_is_38_nines() {
        let max = ExactDecimal::parse_sql(&"9".repeat(38)).unwrap();
        assert!(max.fits_position());
        let over = ExactDecimal::parse_sql(&format!("1{}", "0".repeat(38))).unwrap();
        assert!(!over.fits_position());
        assert!(!ExactDecimal::parse_sql("-1").unwrap().fits_position());
    }
}
