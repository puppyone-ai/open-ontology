//! Arrow series-60 type names as data.
//!
//! This crate does not depend on the `arrow` crates. `series60` is the contract
//! spelling (`decimal128(10,-2)`, `list<int16>`, `timestamp[us,UTC]`). It is
//! not arrow-schema Display. arrow-schema 60.0.0 Display, executed in an
//! isolated probe, prints `Decimal128(10, -2)`, `List(Int16)`, and
//! `Timestamp(µs, "UTC")`.
//!
//! Physical parameters follow arrow-schema 60.0.0 `validate_decimal`: precision
//! is `1..=38` or `1..=76`, and a positive scale must be `<= precision`. A
//! negative scale is valid and is not limited to `abs(scale) <= precision`.
//! [`ArrowTypeName::validate`] runs on deserialize and serialize.
//! [`crate::batch::validate_batch`] runs it again for an enum literal.
//! An enum literal alone is not trusted.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::ToSchema;

use crate::schema_util::schema_object;

pub const COL_OP: &str = "_oo_op";
pub const COL_POS: &str = "_oo_pos";
pub const COL_SRC_TS: &str = "_oo_src_ts";
pub const COL_UNAVAILABLE: &str = "_oo_unavailable";

pub const DISPLAY_OP: &str = "_oo_op:int8(1=upsert,2=delete,3=fill)";
pub const DISPLAY_POS: &str = "_oo_pos:decimal128(38,0)";
pub const DISPLAY_SRC_TS: &str = "_oo_src_ts:timestamp[us,UTC]";
pub const DISPLAY_UNAVAILABLE: &str = "_oo_unavailable:list<int16>";

#[derive(Debug, Clone, PartialEq, Eq, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArrowTypeName {
    Int8,
    Int16,
    Int32,
    Int64,
    Float64,
    Boolean,
    Utf8,
    Binary,
    Date32,
    Time64Micros,
    DurationMicros,
    TimestampMicrosNaive,
    TimestampMicrosUtc,
    Decimal128 { precision: u8, scale: i8 },
    Decimal256 { precision: u8, scale: i8 },
    List { item: Box<ArrowTypeName> },
}

impl ArrowTypeName {
    pub fn decimal128(precision: u8, scale: i8) -> Result<Self, String> {
        let value = Self::Decimal128 { precision, scale };
        value.validate()?;
        Ok(value)
    }

    pub fn decimal256(precision: u8, scale: i8) -> Result<Self, String> {
        let value = Self::Decimal256 { precision, scale };
        value.validate()?;
        Ok(value)
    }

    pub fn list(item: Self) -> Result<Self, String> {
        let value = Self::List {
            item: Box::new(item),
        };
        value.validate()?;
        Ok(value)
    }

    /// Hand-rolled series-60 spelling. This is not arrow-schema 60 Display.
    pub fn series60(&self) -> Result<String, String> {
        self.validate()?;
        Ok(self.spell())
    }

    fn spell(&self) -> String {
        match self {
            Self::Int8 => "int8".to_owned(),
            Self::Int16 => "int16".to_owned(),
            Self::Int32 => "int32".to_owned(),
            Self::Int64 => "int64".to_owned(),
            Self::Float64 => "float64".to_owned(),
            Self::Boolean => "bool".to_owned(),
            Self::Utf8 => "utf8".to_owned(),
            Self::Binary => "binary".to_owned(),
            Self::Date32 => "date32".to_owned(),
            Self::Time64Micros => "time64[us]".to_owned(),
            Self::DurationMicros => "duration[us]".to_owned(),
            Self::TimestampMicrosNaive => "timestamp[us]".to_owned(),
            Self::TimestampMicrosUtc => "timestamp[us,UTC]".to_owned(),
            Self::Decimal128 { precision, scale } => format!("decimal128({precision},{scale})"),
            Self::Decimal256 { precision, scale } => format!("decimal256({precision},{scale})"),
            Self::List { item } => format!("list<{}>", item.spell()),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        self.append_diagnostics("", &mut errors);
        match errors.into_iter().next() {
            Some((_, message)) => Err(message),
            None => Ok(()),
        }
    }

    /// Physical parameter and list-depth errors. `path` is a JSON pointer prefix
    /// such as `/columns/0/arrow`; nested lists append `/item`.
    pub(crate) fn append_diagnostics(&self, path: &str, out: &mut Vec<(String, String)>) {
        match self {
            Self::Decimal128 { precision, scale } => {
                if let Err(message) = decimal_params(*precision, *scale, 38, "decimal128") {
                    out.push((path.to_owned(), message));
                }
            }
            Self::Decimal256 { precision, scale } => {
                if let Err(message) = decimal_params(*precision, *scale, 76, "decimal256") {
                    out.push((path.to_owned(), message));
                }
            }
            Self::List { item } => {
                if list_depth(self) > crate::limits::CANONICAL_MAX_DEPTH {
                    out.push((
                        path.to_owned(),
                        "arrow list nesting exceeds the depth budget".to_owned(),
                    ));
                }
                item.append_diagnostics(&format!("{path}/item"), out);
            }
            _ => {}
        }
    }

    pub fn is_system_name(name: &str) -> bool {
        matches!(name, COL_OP | COL_POS | COL_SRC_TS | COL_UNAVAILABLE)
    }

    /// Suggested business name. This does not rename the stored column.
    /// The caller must rename before batch creation.
    pub fn business_name(name: &str) -> String {
        if Self::is_system_name(name) {
            format!("src_{name}")
        } else {
            name.to_owned()
        }
    }
}

/// arrow-schema 60.0.0 `validate_decimal`. Negative scale is not bounded by
/// `abs(scale) <= precision`. Precision `0` and precision above the physical
/// maximum fail. `series60` still spells `decimal128(38,-100)`, which is not
/// the Display form `Decimal128(38, -100)`.
fn decimal_params(precision: u8, scale: i8, max_precision: u8, name: &str) -> Result<(), String> {
    if precision == 0 || precision > max_precision {
        return Err(format!("{name} precision must be 1..={max_precision}"));
    }
    if scale > 0 && scale as u8 > precision {
        return Err(format!(
            "{name} positive scale must be less than or equal to precision; a negative scale is valid"
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ArrowTypeDe {
    Int8,
    Int16,
    Int32,
    Int64,
    Float64,
    Boolean,
    Utf8,
    Binary,
    Date32,
    Time64Micros,
    DurationMicros,
    TimestampMicrosNaive,
    TimestampMicrosUtc,
    Decimal128 { precision: u8, scale: i8 },
    Decimal256 { precision: u8, scale: i8 },
    List { item: Box<ArrowTypeName> },
}

impl From<ArrowTypeDe> for ArrowTypeName {
    fn from(raw: ArrowTypeDe) -> Self {
        match raw {
            ArrowTypeDe::Int8 => Self::Int8,
            ArrowTypeDe::Int16 => Self::Int16,
            ArrowTypeDe::Int32 => Self::Int32,
            ArrowTypeDe::Int64 => Self::Int64,
            ArrowTypeDe::Float64 => Self::Float64,
            ArrowTypeDe::Boolean => Self::Boolean,
            ArrowTypeDe::Utf8 => Self::Utf8,
            ArrowTypeDe::Binary => Self::Binary,
            ArrowTypeDe::Date32 => Self::Date32,
            ArrowTypeDe::Time64Micros => Self::Time64Micros,
            ArrowTypeDe::DurationMicros => Self::DurationMicros,
            ArrowTypeDe::TimestampMicrosNaive => Self::TimestampMicrosNaive,
            ArrowTypeDe::TimestampMicrosUtc => Self::TimestampMicrosUtc,
            ArrowTypeDe::Decimal128 { precision, scale } => Self::Decimal128 { precision, scale },
            ArrowTypeDe::Decimal256 { precision, scale } => Self::Decimal256 { precision, scale },
            ArrowTypeDe::List { item } => Self::List { item },
        }
    }
}

impl<'de> Deserialize<'de> for ArrowTypeName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = ArrowTypeName::from(ArrowTypeDe::deserialize(deserializer)?);
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

impl Serialize for ArrowTypeName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        ArrowTypeSer::from(self).serialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ArrowTypeSer<'a> {
    Int8,
    Int16,
    Int32,
    Int64,
    Float64,
    Boolean,
    Utf8,
    Binary,
    Date32,
    Time64Micros,
    DurationMicros,
    TimestampMicrosNaive,
    TimestampMicrosUtc,
    Decimal128 { precision: u8, scale: i8 },
    Decimal256 { precision: u8, scale: i8 },
    List { item: &'a ArrowTypeName },
}

impl<'a> From<&'a ArrowTypeName> for ArrowTypeSer<'a> {
    fn from(value: &'a ArrowTypeName) -> Self {
        match value {
            ArrowTypeName::Int8 => Self::Int8,
            ArrowTypeName::Int16 => Self::Int16,
            ArrowTypeName::Int32 => Self::Int32,
            ArrowTypeName::Int64 => Self::Int64,
            ArrowTypeName::Float64 => Self::Float64,
            ArrowTypeName::Boolean => Self::Boolean,
            ArrowTypeName::Utf8 => Self::Utf8,
            ArrowTypeName::Binary => Self::Binary,
            ArrowTypeName::Date32 => Self::Date32,
            ArrowTypeName::Time64Micros => Self::Time64Micros,
            ArrowTypeName::DurationMicros => Self::DurationMicros,
            ArrowTypeName::TimestampMicrosNaive => Self::TimestampMicrosNaive,
            ArrowTypeName::TimestampMicrosUtc => Self::TimestampMicrosUtc,
            ArrowTypeName::Decimal128 { precision, scale } => Self::Decimal128 {
                precision: *precision,
                scale: *scale,
            },
            ArrowTypeName::Decimal256 { precision, scale } => Self::Decimal256 {
                precision: *precision,
                scale: *scale,
            },
            ArrowTypeName::List { item } => Self::List { item },
        }
    }
}

impl JsonSchema for ArrowTypeName {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ArrowTypeName")
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item = generator.subschema_for::<ArrowTypeName>().to_value();
        schema_object(serde_json::json!({
            "oneOf": [
                unit_variant("int8"),
                unit_variant("int16"),
                unit_variant("int32"),
                unit_variant("int64"),
                unit_variant("float64"),
                unit_variant("boolean"),
                unit_variant("utf8"),
                unit_variant("binary"),
                unit_variant("date32"),
                unit_variant("time64_micros"),
                unit_variant("duration_micros"),
                unit_variant("timestamp_micros_naive"),
                unit_variant("timestamp_micros_utc"),
                decimal_variant("decimal128", 38),
                decimal_variant("decimal256", 76),
                list_variant(item),
            ]
        }))
    }
}

fn unit_variant(kind: &str) -> serde_json::Value {
    serde_json::json!({
        "additionalProperties": false,
        "properties": {
            "kind": { "const": kind, "type": "string" }
        },
        "required": ["kind"],
        "type": "object"
    })
}

/// Precision is `1..=max_precision`. Positive scale must be `<= precision`.
/// Scale `-128` stays valid: this is not `abs(scale) <= precision`.
/// Draft 2020-12 has no numeric comparison keyword, so each positive scale is
/// an `if`/`then`. A scale above `max_precision` matches `then: false`.
fn decimal_variant(kind: &str, max_precision: i64) -> serde_json::Value {
    let mut guards = Vec::with_capacity(max_precision as usize + 1);
    for scale in 1..=max_precision {
        guards.push(serde_json::json!({
            "if": {
                "properties": { "scale": { "minimum": scale } },
                "required": ["scale"]
            },
            "then": {
                "properties": { "precision": { "minimum": scale } }
            }
        }));
    }
    guards.push(serde_json::json!({
        "if": {
            "properties": { "scale": { "minimum": max_precision + 1 } },
            "required": ["scale"]
        },
        "then": false
    }));
    serde_json::json!({
        "additionalProperties": false,
        "allOf": guards,
        "properties": {
            "kind": { "const": kind, "type": "string" },
            "precision": {
                "maximum": max_precision,
                "minimum": 1,
                "type": "integer"
            },
            "scale": {
                "maximum": 127,
                "minimum": -128,
                "type": "integer"
            }
        },
        "required": ["kind", "precision", "scale"],
        "type": "object"
    })
}

fn list_variant(item: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "additionalProperties": false,
        "description": "Nested list item. Rust rejects list depth above 32. JSON Schema draft 2020-12 has no standard keyword for maximum depth, so this schema does not enforce that limit.",
        "properties": {
            "item": item,
            "kind": { "const": "list", "type": "string" }
        },
        "required": ["kind", "item"],
        "type": "object"
    })
}

fn list_depth(value: &ArrowTypeName) -> usize {
    match value {
        ArrowTypeName::List { item } => 1 + list_depth(item),
        _ => 0,
    }
}

pub fn system_column_displays() -> [&'static str; 4] {
    [DISPLAY_OP, DISPLAY_POS, DISPLAY_SRC_TS, DISPLAY_UNAVAILABLE]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_display_matches_the_spec_spelling() {
        assert_eq!(ArrowTypeName::Int8.series60().unwrap(), "int8");
        assert_eq!(
            ArrowTypeName::DurationMicros.series60().unwrap(),
            "duration[us]"
        );
        assert_eq!(
            serde_json::to_string(&ArrowTypeName::DurationMicros).unwrap(),
            r#"{"kind":"duration_micros"}"#
        );
        assert_eq!(DISPLAY_OP, "_oo_op:int8(1=upsert,2=delete,3=fill)");
        assert_eq!(
            format!(
                "{COL_POS}:{}",
                ArrowTypeName::Decimal128 {
                    precision: 38,
                    scale: 0
                }
                .series60()
                .unwrap()
            ),
            "_oo_pos:decimal128(38,0)"
        );
        assert_eq!(
            format!(
                "{COL_SRC_TS}:{}",
                ArrowTypeName::TimestampMicrosUtc.series60().unwrap()
            ),
            DISPLAY_SRC_TS
        );
        assert_eq!(
            format!(
                "{COL_UNAVAILABLE}:{}",
                ArrowTypeName::List {
                    item: Box::new(ArrowTypeName::Int16)
                }
                .series60()
                .unwrap()
            ),
            DISPLAY_UNAVAILABLE
        );
        assert!(
            ArrowTypeName::Decimal128 {
                precision: 255,
                scale: -1
            }
            .series60()
            .is_err()
        );
        assert!(
            ArrowTypeName::Decimal128 {
                precision: 2,
                scale: -1
            }
            .series60()
            .is_ok()
        );
        assert_eq!(ArrowTypeName::business_name("_oo_op"), "src__oo_op");
        assert_eq!(ArrowTypeName::business_name("amount"), "amount");
    }

    #[test]
    fn serialize_checks_precision_scale_and_list_depth() {
        assert!(
            serde_json::to_string(&ArrowTypeName::Decimal128 {
                precision: 0,
                scale: 0
            })
            .is_err()
        );
        assert!(
            serde_json::to_string(&ArrowTypeName::List {
                item: Box::new(ArrowTypeName::Decimal128 {
                    precision: 0,
                    scale: 0
                })
            })
            .is_err()
        );
        let mut deep = ArrowTypeName::Int16;
        for _ in 0..=crate::limits::CANONICAL_MAX_DEPTH {
            deep = ArrowTypeName::List {
                item: Box::new(deep),
            };
        }
        assert!(serde_json::to_string(&deep).is_err());
        let mut ok_depth = ArrowTypeName::Int16;
        for _ in 0..crate::limits::CANONICAL_MAX_DEPTH {
            ok_depth = ArrowTypeName::List {
                item: Box::new(ok_depth),
            };
        }
        assert!(serde_json::to_string(&ok_depth).is_ok());
        let negative = ArrowTypeName::decimal128(1, -128).unwrap();
        let dumped = serde_json::to_string(&negative).unwrap();
        assert_eq!(
            serde_json::from_str::<ArrowTypeName>(&dumped).unwrap(),
            negative
        );
        assert_eq!(negative.series60().unwrap(), "decimal128(1,-128)");
        assert!(!negative.series60().unwrap().contains("Decimal128"));
        assert_eq!(
            ArrowTypeName::decimal128(10, -2)
                .unwrap()
                .series60()
                .unwrap(),
            "decimal128(10,-2)"
        );
        assert!(ArrowTypeName::decimal128(38, -100).is_ok());
        assert!(ArrowTypeName::decimal256(76, -128).is_ok());
        assert!(ArrowTypeName::decimal256(76, 76).is_ok());
        assert!(
            serde_json::to_string(&ArrowTypeName::Decimal128 {
                precision: 5,
                scale: 6
            })
            .is_err()
        );
    }
}
