//! Duration and byte-size spellings.
//!
//! A duration keeps the authored suffix. `5m` and `300s` are different
//! spellings of the same number of seconds; equality includes both. The
//! ten-year cap is a cross-field check so the schema pattern and the
//! deserializer stay on the same shape language.

use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::limits::DURATION_MAX_SECONDS;
use crate::schema_util::string_schema;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Duration {
    seconds: u64,
    text: String,
}

impl Duration {
    pub fn parse(input: &str) -> Result<Self, String> {
        let bytes = input.as_bytes();
        if bytes.len() < 2 || bytes.len() > 12 {
            return Err("invalid duration".to_owned());
        }
        let (number, unit) = input.split_at(input.len() - 1);
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err("invalid duration".to_owned());
        }
        if number.len() > 1 && number.starts_with('0') {
            return Err("invalid duration".to_owned());
        }
        if number.len() > 10 {
            return Err("invalid duration".to_owned());
        }
        let quantity: u64 = number.parse().map_err(|_| "invalid duration".to_owned())?;
        let factor: u64 = match unit {
            "s" => 1,
            "m" => 60,
            "h" => 60 * 60,
            "d" => 24 * 60 * 60,
            _ => return Err("invalid duration".to_owned()),
        };
        let Some(seconds) = quantity.checked_mul(factor) else {
            return Err("invalid duration".to_owned());
        };
        Ok(Self {
            seconds,
            text: input.to_owned(),
        })
    }

    pub fn seconds(&self) -> u64 {
        self.seconds
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn within_cap(&self) -> bool {
        self.seconds <= DURATION_MAX_SECONDS
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for Duration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for Duration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Duration {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("Duration")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        string_schema(r"^(?:0|[1-9][0-9]{0,9})[smhd]$", 2, 11)
    }
}

impl PartialSchema for Duration {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .pattern(Some(r"^(?:0|[1-9][0-9]{0,9})[smhd]$"))
            .description(Some(
                "Duration suffix s, m, h, or d. The ten-year cap is a cross-field check.",
            ))
            .into()
    }
}

impl ToSchema for Duration {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ByteSize {
    bytes: u64,
    text: String,
}

impl ByteSize {
    pub fn parse(input: &str) -> Result<Self, String> {
        let unit = input.trim_start_matches(|ch: char| ch.is_ascii_digit());
        if unit.len() == input.len() {
            return Err("invalid byte size".to_owned());
        }
        let number = &input[..input.len() - unit.len()];
        if number.is_empty() || (number.len() > 1 && number.starts_with('0')) || number.len() > 18 {
            return Err("invalid byte size".to_owned());
        }
        let quantity: u64 = number.parse().map_err(|_| "invalid byte size".to_owned())?;
        let factor: u64 = match unit {
            "" | "B" => 1,
            "KB" => 1_000,
            "MB" => 1_000_000,
            "GB" => 1_000_000_000,
            "TB" => 1_000_000_000_000,
            "Ki" | "KiB" => 1024,
            "Mi" | "MiB" => 1024 * 1024,
            "Gi" | "GiB" => 1024 * 1024 * 1024,
            "Ti" | "TiB" => 1024u64 * 1024 * 1024 * 1024,
            _ => return Err("invalid byte size".to_owned()),
        };
        let Some(bytes) = quantity.checked_mul(factor) else {
            return Err("invalid byte size".to_owned());
        };
        Ok(Self {
            bytes,
            text: input.to_owned(),
        })
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for ByteSize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ByteSize {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ByteSize")
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        string_schema(
            r"^(?:0|[1-9][0-9]{0,17})(?:B|KB|MB|GB|TB|KiB|MiB|GiB|TiB|Ki|Mi|Gi|Ti)?$",
            1,
            20,
        )
    }
}

impl PartialSchema for ByteSize {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .pattern(Some(
                r"^(?:0|[1-9][0-9]{0,17})(?:B|KB|MB|GB|TB|KiB|MiB|GiB|TiB|Ki|Mi|Gi|Ti)?$",
            ))
            .into()
    }
}

impl ToSchema for ByteSize {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_spellings_keep_their_suffix() {
        assert_eq!(Duration::parse("300s").unwrap().as_str(), "300s");
        assert_eq!(Duration::parse("5m").unwrap().seconds(), 300);
        assert_eq!(Duration::parse("10m").unwrap().seconds(), 600);
        assert_eq!(Duration::parse("90s").unwrap().seconds(), 90);
        assert_eq!(ByteSize::parse("1GB").unwrap().bytes(), 1_000_000_000);
        assert_eq!(
            ByteSize::parse("4Gi").unwrap().bytes(),
            4 * 1024 * 1024 * 1024
        );
    }
}
