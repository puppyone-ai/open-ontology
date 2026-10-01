//! Document version identifiers. Unknown values are errors, never fallbacks.

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::diagnostic::{CONFIG_SCHEMA_VERSION, OOS_SCHEMA_VERSION};
use crate::schema_util::schema_object;

macro_rules! exact_version {
    ($name:ident, $value:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name;

        impl $name {
            pub const VALUE: &'static str = $value;
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(Self::VALUE)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                if value == Self::VALUE {
                    Ok(Self)
                } else {
                    Err(serde::de::Error::custom(format!(
                        "unsupported version; expected {}",
                        Self::VALUE
                    )))
                }
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn inline_schema() -> bool {
                true
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                schema_object(serde_json::json!({
                    "type": "string",
                    "const": $value
                }))
            }
        }

        impl PartialSchema for $name {
            fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .enum_values(Some([$value]))
                    .into()
            }
        }

        impl ToSchema for $name {}

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(Self::VALUE)
            }
        }
    };
}

exact_version!(ConfigVersion, CONFIG_SCHEMA_VERSION);
exact_version!(OosVersion, OOS_SCHEMA_VERSION);

pub fn require_config_version(
    value: &serde_json::Value,
) -> Result<(), crate::diagnostic::Diagnostic> {
    require_version(value, "schema_version", ConfigVersion::VALUE)
}

pub fn require_oos_schema_version(
    value: &serde_json::Value,
) -> Result<(), crate::diagnostic::Diagnostic> {
    require_version(value, "schema_version", OosVersion::VALUE)
}

pub fn require_oos_spec_version(
    value: &serde_json::Value,
) -> Result<(), crate::diagnostic::Diagnostic> {
    require_version(value, "spec_version", OosVersion::VALUE)
}

fn require_version(
    value: &serde_json::Value,
    field: &str,
    expected: &str,
) -> Result<(), crate::diagnostic::Diagnostic> {
    use crate::diagnostic::{CONTRACT_CONFIG, CONTRACT_OOS, Diagnostic};
    let contract = if expected == ConfigVersion::VALUE {
        CONTRACT_CONFIG
    } else {
        CONTRACT_OOS
    };
    match value.get(field) {
        None => Err(Diagnostic::error(
            contract,
            "version.missing",
            format!("/{field}"),
            format!("`{field}` is required and must be `{expected}`"),
        )),
        Some(serde_json::Value::String(found)) if found == expected => Ok(()),
        Some(_) => Err(Diagnostic::error(
            contract,
            "version.unsupported",
            format!("/{field}"),
            format!("`{field}` must be `{expected}`"),
        )),
    }
}
