//! Batch metadata and row checks.
//!
//! This module is the contract, not a Source PositionCodec and not a Loader.
//! No database driver is used. `hook_digest` is required; an empty hook list
//! hashes the empty name list rather than omitting the field.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::openapi::schema::{ObjectBuilder, Type};
use utoipa::{PartialSchema, ToSchema};

use crate::arrow_meta::ArrowTypeName;
use crate::config::model::FileMode;
use crate::config::version::ConfigVersion;
use crate::decimal::ExactDecimal;
use crate::diagnostic::{BATCH_PROTOCOL_VERSION, CONTRACT_BATCH, Diagnostic, TYPE_POLICY_V1};
use crate::text::{HexDigest, Label, SourceName, TableName};

macro_rules! exact_string {
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
                crate::schema_util::schema_object(serde_json::json!({"type": "string", "const": $value}))
            }
        }

        impl PartialSchema for $name {
            fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                ObjectBuilder::new().schema_type(Type::String).enum_values(Some([$value])).into()
            }
        }

        impl ToSchema for $name {}
    };
}

exact_string!(BatchVersion, BATCH_PROTOCOL_VERSION);
exact_string!(TypePolicyVersion, TYPE_POLICY_V1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BatchPath {
    Cdc,
    Incremental,
    Full,
    Snapshot,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchColumn {
    pub name: TableName,
    pub arrow: ArrowTypeName,
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchMeta {
    pub protocol_version: BatchVersion,
    pub source: SourceName,
    pub table: TableName,
    pub schema_fingerprint: HexDigest,
    pub batch_id: Label,
    pub path: BatchPath,
    #[serde(default)]
    pub file_mode: Option<FileMode>,
    pub config_version: ConfigVersion,
    pub type_policy_version: TypePolicyVersion,
    pub hook_digest: HexDigest,
    pub txn_id: Label,
    pub txn_sequence: u32,
    pub epoch: u32,
    #[serde(default)]
    pub primary_key: Vec<TableName>,
    pub columns: Vec<BatchColumn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellSlot {
    Value,
    Null,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRow {
    pub cells: Vec<CellSlot>,
    pub op: i8,
    pub position: ExactDecimal,
    pub source_ts_us: Option<i64>,
    pub source_ts_unavailable: bool,
    pub unavailable: Vec<i16>,
}

pub fn validate_batch(meta: &BatchMeta) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if meta.path == BatchPath::File && meta.file_mode.is_none() {
        out.push(batch(
            "cross_field",
            "/file_mode",
            "a file batch names replace, append, or upsert",
        ));
    }
    if meta.path != BatchPath::File && meta.file_mode.is_some() {
        out.push(batch(
            "cross_field",
            "/file_mode",
            "file_mode is only valid on a file batch",
        ));
    }
    if meta.path == BatchPath::Snapshot && meta.txn_sequence != 0 {
        out.push(batch(
            "cross_field",
            "/txn_sequence",
            "a snapshot uses txn_sequence 0 and puts the snapshot id in txn_id",
        ));
    }
    let pk_required = matches!(meta.path, BatchPath::Cdc | BatchPath::Incremental)
        || (meta.path == BatchPath::File && meta.file_mode == Some(FileMode::Upsert));
    if pk_required && meta.primary_key.is_empty() {
        out.push(batch(
            "cross_field",
            "/primary_key",
            "this path requires a primary key",
        ));
    }
    let mut names = Vec::new();
    for (index, column) in meta.columns.iter().enumerate() {
        let stored = column.name.as_str();
        if ArrowTypeName::is_system_name(stored) {
            let suggested = ArrowTypeName::business_name(stored);
            out.push(batch(
                "cross_field",
                &format!("/columns/{index}/name"),
                format!(
                    "system column `{stored}` collides with a reserved name. The caller MUST explicitly rename it before batch creation (suggested name `{suggested}`). The input is unaltered and the column is not dropped"
                ),
            ));
        }
        if names.iter().any(|seen| seen == stored) {
            out.push(batch(
                "cross_field",
                &format!("/columns/{index}/name"),
                "column name collides with another column or a system column",
            ));
        }
        names.push(stored.to_owned());
        let mut arrow_errors = Vec::new();
        column
            .arrow
            .append_diagnostics(&format!("/columns/{index}/arrow"), &mut arrow_errors);
        for (path, message) in arrow_errors {
            out.push(batch("cross_field", &path, message));
        }
    }
    for key in &meta.primary_key {
        if !meta
            .columns
            .iter()
            .any(|column| column.name.as_str() == key.as_str())
        {
            out.push(batch(
                "cross_field",
                "/primary_key",
                format!("primary key `{key}` is not a business column"),
            ));
        }
    }
    out
}

pub fn validate_batch_row(meta: &BatchMeta, row: &BatchRow) -> Vec<Diagnostic> {
    let mut out = validate_batch(meta);
    if !matches!(row.op, 1..=3) {
        out.push(batch(
            "cross_field",
            "/op",
            "op must be 1 upsert, 2 delete, or 3 fill",
        ));
    }
    if !row.position.fits_position() {
        out.push(batch(
            "cross_field",
            "/position",
            "position must be a non-negative decimal128(38,0) value",
        ));
    }
    if row.source_ts_unavailable && row.source_ts_us.is_some() {
        out.push(batch(
            "cross_field",
            "/source_ts",
            "unavailable is separate from a present or null source timestamp",
        ));
    }
    if row.cells.len() != meta.columns.len() {
        out.push(batch(
            "cross_field",
            "/cells",
            "row width does not match the batch schema",
        ));
        return out;
    }
    for (index, slot) in row.unavailable.iter().enumerate() {
        if *slot < 0 || *slot as usize >= meta.columns.len() {
            out.push(batch(
                "cross_field",
                &format!("/unavailable/{index}"),
                "unavailable index is outside the row",
            ));
        }
    }
    for (index, (column, slot)) in meta.columns.iter().zip(&row.cells).enumerate() {
        let listed = row.unavailable.iter().any(|item| *item as usize == index);
        let path = format!("/cells/{index}");
        if *slot == CellSlot::Unavailable && !listed {
            out.push(batch(
                "cross_field",
                &path,
                "unavailable cell is not listed in _oo_unavailable",
            ));
        }
        if *slot == CellSlot::Null && listed {
            out.push(batch(
                "cross_field",
                &path,
                "null and unavailable are different states",
            ));
        }
        if *slot == CellSlot::Value && listed {
            out.push(batch(
                "cross_field",
                &path,
                "a listed unavailable column cannot also carry a value",
            ));
        }
        if *slot == CellSlot::Null && !column.nullable {
            out.push(batch(
                "cross_field",
                &path,
                "null is not allowed for a non-null column",
            ));
        }
        let primary = meta
            .primary_key
            .iter()
            .any(|key| key.as_str() == column.name.as_str());
        if primary && *slot != CellSlot::Value {
            out.push(batch(
                "cross_field",
                &path,
                "a primary-key column cannot be null or unavailable",
            ));
        }
    }
    out
}

fn batch(name: &str, path: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(CONTRACT_BATCH, name, path, message)
}
