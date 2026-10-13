//! Read-only adapter for fixed source artifacts.
//!
//! The capture catalog, compare profile, semantics profile, and sealed type
//! basis stay byte-for-byte. This module records their hashes and the operator
//! commit pins. It maps a PostgreSQL OID to a closed OOS type only when
//! `oo-core` sealed-import `target_type` at `8397fd5b984df364cc3638ffa1e69ea06e125d74`
//! names a type this service already accepts. The storage name matches
//! `target_type` in `oo-core` `sealed_import` at
//! `983577c9b89453b08ab4e1a02cab88df78c3e005`. OID 17 is `bytea`. The closed
//! OOS set has no bytea, so the column stays unknown and is not rewritten as
//! boolean or text. This path does not call `derive_source_bound`: the engine
//! is postgres. Grade and delete detection stay unknown when the observation
//! is null. Nothing here is auto-approved, and this file does not call a model.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::canonical::sha256_hex;
use crate::error::ApiError;

pub const CAPTURE_CATALOG: &str = "oo.postgres-preview.capture-check/v1";
pub const CAPTURE_COMPARE: &str = "oo.postgres-preview.capture-compare/v1";
pub const SEMANTICS_PROFILE: &str = "oo.builder.source-semantics-profile/v1";
pub const SEALED_MANIFEST: &str = "oo.pipeline.pg-sealed-manifest/v1";
pub const CATALOG_IMPORT: &str = "oo.pipeline.catalog-import-pg/v1";

#[derive(Clone, Debug)]
pub struct ProtocolPin {
    pub catalog: String,
    pub profile: String,
    pub semantics: String,
    pub source_commit: String,
    pub semantics_commit: String,
}

#[derive(Clone, Debug)]
pub struct BasisPin {
    pub protocol: String,
    pub outer_protocol: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Projection {
    pub typed_catalog: Value,
    pub receipt: Value,
}

pub fn project(
    catalog_bytes: &[u8],
    profile_bytes: &[u8],
    semantics_bytes: &[u8],
    pin: &ProtocolPin,
    bases: &[BasisPin],
) -> Result<Projection, ApiError> {
    if pin.catalog != CAPTURE_CATALOG || pin.profile != CAPTURE_COMPARE || pin.semantics != SEMANTICS_PROFILE {
        return Err(ApiError::malformed());
    }
    if bases.is_empty() {
        return Err(ApiError::malformed());
    }
    let catalog = parse_object(catalog_bytes)?;
    let profile = parse_object(profile_bytes)?;
    let semantics = parse_object(semantics_bytes)?;
    if catalog.get("protocol").and_then(Value::as_str) != Some(pin.catalog.as_str())
        || profile.get("protocol").and_then(Value::as_str) != Some(pin.profile.as_str())
        || semantics.get("protocol").and_then(Value::as_str) != Some(pin.semantics.as_str())
    {
        return Err(ApiError::malformed());
    }
    if profile.get("sampled") != Some(&Value::Bool(false)) || profile.get("full_scan") != Some(&Value::Bool(true)) {
        return Err(ApiError::malformed());
    }
    let rows_compared = profile.get("rows_compared").and_then(Value::as_u64).ok_or_else(ApiError::malformed)?;
    let (grade, grade_status) = observe_token(semantics.get("consistency_grade"), true)?;
    let (delete_detection, delete_status) = observe_token(semantics.get("delete_detection"), false)?;
    let mut typed_by_table: Vec<(String, String, Vec<Value>)> = Vec::new();
    let mut columns = Vec::new();
    let tables = catalog.get("tables").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    if tables.is_empty() {
        return Err(ApiError::malformed());
    }
    let basis = load_basis(bases)?;
    for table in tables {
        let schema = text_ident(table, "schema")?;
        let name = text_ident(table, "table")?;
        let listed = table.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
        if listed.is_empty() {
            return Err(ApiError::malformed());
        }
        let mut typed_columns = Vec::new();
        for column in listed {
            let column_name = column.as_str().filter(|item| crate::ids::source_ident_ok(item)).ok_or_else(ApiError::malformed)?;
            let key = (schema.to_owned(), name.to_owned(), column_name.to_owned());
            let found = basis.get(&key);
            let (target_type, data_type, oid, type_status, oos_support) = match found {
                Some((oid, typmod)) => {
                    let target = pg_target_name(*oid, *typmod);
                    let mapped = target.and_then(oos_from_target_name);
                    let (status, support) = match (target, mapped) {
                        (Some(_), Some(_)) => ("mapped", "mapped"),
                        (Some(_), None) => ("unknown", "oos_unsupported"),
                        (None, _) => ("unknown", "unknown_oid"),
                    };
                    (target, mapped, Some(*oid), status, support)
                }
                None => (None, None, None, "unknown", "unknown_oid"),
            };
            if let Some(mapped) = data_type {
                typed_columns.push(json!({
                    "data_type": mapped,
                    "name": column_name
                }));
            }
            columns.push(json!({
                "auto_approved": false,
                "column": column_name,
                "data_type": data_type,
                "dataset": format!("{schema}.{name}"),
                "oid": oid,
                "oos_support": oos_support,
                "pii": "unknown",
                "pii_core_level": crate::corebind::core_pii_level("unknown"),
                "semantic": Value::Null,
                "semantic_core": crate::corebind::core_semantic("unknown"),
                "target_type": target_type,
                "type_status": type_status
            }));
        }
        typed_by_table.push((schema.to_owned(), name.to_owned(), typed_columns));
    }
    let mapped = columns.iter().filter(|item| item.get("type_status").and_then(Value::as_str) == Some("mapped")).count();
    if mapped == 0 {
        return Err(ApiError::malformed());
    }
    let mut typed_tables = Vec::new();
    for (schema, name, typed_columns) in &typed_by_table {
        if typed_columns.is_empty() {
            continue;
        }
        typed_tables.push(json!({
            "columns": typed_columns,
            "schema": schema,
            "table": name
        }));
    }
    let typed_catalog = json!({"tables": typed_tables});
    let mut basis_receipt = Vec::new();
    for item in bases {
        basis_receipt.push(json!({
            "byte_sha256": sha256_hex(&item.bytes),
            "outer_protocol": item.outer_protocol,
            "protocol": item.protocol
        }));
    }
    let receipt = json!({
        "auto_approved": false,
        "basis": basis_receipt,
        "catalog_byte_sha256": sha256_hex(catalog_bytes),
        "catalog_protocol": pin.catalog,
        "columns": columns,
        "consistency_grade": grade,
        "delete_detection": delete_detection,
        "delete_status": delete_status,
        "grade_status": grade_status,
        "mapped_columns": mapped,
        "profile_byte_sha256": sha256_hex(profile_bytes),
        "profile_protocol": pin.profile,
        "rows_compared": rows_compared,
        "semantics_byte_sha256": sha256_hex(semantics_bytes),
        "semantics_commit": pin.semantics_commit,
        "semantics_protocol": pin.semantics,
        "source_commit": pin.source_commit,
        "unknown_auto_approved": false,
        "arrow_basis": "not_present",
        "core_commit": crate::corebind::CORE_COMMIT,
        "derive_skip_reason": "engine_not_oracle",
        "derive_source_bound_called": false,
        "engine": "postgres",
        "field_statement": "PostgreSQL columns use the sealed basis oid and typmod. The storage name matches oo-core sealed-import target_type at 983577c9b89453b08ab4e1a02cab88df78c3e005. An OOS data_type is set only for the closed set. bytea is unsupported and is not mapped to another type. derive_source_bound is not called because this engine is postgres. PII unknown is not a core PiiLevel and is not approved.",
        "version_binding": {
            "catalog_byte_sha256": sha256_hex(catalog_bytes),
            "catalog_protocol": pin.catalog,
            "core_commit": crate::corebind::CORE_COMMIT,
            "profile_byte_sha256": sha256_hex(profile_bytes),
            "profile_protocol": pin.profile,
            "semantics_byte_sha256": sha256_hex(semantics_bytes),
            "semantics_commit": pin.semantics_commit,
            "semantics_protocol": pin.semantics,
            "source_commit": pin.source_commit
        },
        "withheld_columns": columns.len() - mapped
    });
    Ok(Projection { typed_catalog, receipt })
}

/// PostgreSQL OID to a closed OOS type.
///
/// The OID list matches private `target_type` in `crates/oo-core/src/sealed_import.rs`
/// at commit `983577c9b89453b08ab4e1a02cab88df78c3e005`. That function is not
/// public. Names outside the closed OOS set stay unknown. `bytea` (17) is not
/// `boolean` (16) and is not rewritten as `text`.
pub fn oos_from_pg_oid(oid: i64, typmod: i32) -> Option<&'static str> {
    pg_target_name(oid, typmod).and_then(oos_from_target_name)
}

/// Storage name cited from `oo-core` `target_type`. Not an OOS `data_type`.
pub fn pg_target_name(oid: i64, typmod: i32) -> Option<&'static str> {
    let name = match oid {
        16 => "bool",
        17 => "bytea",
        20 => "int8",
        21 => "int2",
        23 => "int4",
        25 => "text",
        114 => "json",
        700 => "float4",
        701 => "float8",
        1042 => "bpchar",
        1043 => "varchar",
        1082 => "date",
        1083 => "time",
        1114 => "timestamp",
        1184 => "timestamptz",
        1700 => "numeric",
        2950 => "uuid",
        3802 => "jsonb",
        _ => return None,
    };
    if typmod_ok(oid, typmod) { Some(name) } else { None }
}

pub fn oos_from_target_name(name: &str) -> Option<&'static str> {
    match name {
        "bool" => Some("boolean"),
        "int8" => Some("bigint"),
        "int4" => Some("integer"),
        "text" => Some("text"),
        "date" => Some("date"),
        "timestamp" => Some("timestamp"),
        "timestamptz" => Some("timestamptz"),
        "numeric" => Some("numeric"),
        "uuid" => Some("uuid"),
        "jsonb" => Some("jsonb"),
        _ => None,
    }
}

fn typmod_ok(oid: i64, typmod: i32) -> bool {
    match oid {
        1042 => typmod >= 5,
        1043 => typmod == -1 || typmod >= 4,
        1700 => {
            if typmod < 0 {
                return true;
            }
            match numeric_parts(typmod) {
                Some((precision, scale)) => precision > 0 && scale >= 0 && scale <= precision,
                None => false,
            }
        }
        1083 | 1114 | 1184 => typmod == -1 || (0..=6).contains(&typmod),
        _ => typmod == -1,
    }
}

fn numeric_parts(typmod: i32) -> Option<(i32, i32)> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod - 4;
    let precision = (adjusted >> 16) & 0xFFFF;
    let scale = adjusted & 0xFFFF;
    Some((precision, scale))
}

type BasisIndex = BTreeMap<(String, String, String), (i64, i32)>;

fn load_basis(bases: &[BasisPin]) -> Result<BasisIndex, ApiError> {
    let mut out = BTreeMap::new();
    for basis in bases {
        if basis.protocol != SEALED_MANIFEST || basis.outer_protocol != CATALOG_IMPORT {
            return Err(ApiError::malformed());
        }
        if sha256_hex(&basis.bytes) != basis.sha256 {
            return Err(ApiError::malformed());
        }
        let document = parse_object(&basis.bytes)?;
        if document.get("protocol").and_then(Value::as_str) != Some(basis.outer_protocol.as_str()) {
            return Err(ApiError::malformed());
        }
        let manifest = document.get("manifest").and_then(Value::as_object).ok_or_else(ApiError::malformed)?;
        if manifest.get("protocol").and_then(Value::as_str) != Some(basis.protocol.as_str()) {
            return Err(ApiError::malformed());
        }
        let tables = manifest.get("tables").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
        for table in tables {
            let schema = text_ident(table, "logical_schema")?;
            let name = text_ident(table, "logical_table")?;
            let columns = table.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
            for column in columns {
                let logical = text_ident(column, "logical_name")?;
                let oid = column.get("oid").and_then(Value::as_i64).ok_or_else(ApiError::malformed)?;
                let typmod = column.get("typmod").and_then(Value::as_i64).ok_or_else(ApiError::malformed)?;
                let typmod = i32::try_from(typmod).map_err(|_| ApiError::malformed())?;
                if column.get("data_type").is_some() {
                    return Err(ApiError::malformed());
                }
                let key = (schema.to_owned(), name.to_owned(), logical.to_owned());
                if out.get(&key).is_some_and(|(previous, previous_mod)| *previous != oid || *previous_mod != typmod) {
                    return Err(ApiError::malformed());
                }
                out.insert(key, (oid, typmod));
            }
        }
    }
    Ok(out)
}

fn observe_token(value: Option<&Value>, grade: bool) -> Result<(Value, &'static str), ApiError> {
    match value {
        None | Some(Value::Null) => Ok((Value::Null, "unknown")),
        Some(Value::String(token)) => {
            let allowed = if grade {
                matches!(token.as_str(), "T" | "S1" | "S0" | "B")
            } else {
                matches!(token.as_str(), "native" | "partition_replace" | "reconcile" | "none")
            };
            if allowed {
                Ok((Value::String(token.clone()), "present"))
            } else {
                Err(ApiError::malformed())
            }
        }
        _ => Err(ApiError::malformed()),
    }
}

fn parse_object(bytes: &[u8]) -> Result<Value, ApiError> {
    if bytes.len() > 1_000_000 {
        return Err(ApiError::malformed());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ApiError::malformed())?;
    if !value.is_object() {
        return Err(ApiError::malformed());
    }
    Ok(value)
}

fn text_ident<'a>(value: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|item| crate::ids::source_ident_ok(item))
        .ok_or_else(ApiError::malformed)
}

pub fn git_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// A real source dictionary, not the capture-check stand-in.
pub fn measured_catalog(protocol: &str) -> bool {
    let Some(rest) = protocol.strip_prefix("oo.source.") else {
        return false;
    };
    let Some((engine, version)) = rest.split_once('/') else {
        return false;
    };
    !engine.is_empty()
        && engine.len() <= 32
        && engine.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && version.starts_with('v')
        && (2..=8).contains(&version.len())
        && version.bytes().skip(1).all(|byte| byte.is_ascii_digit())
}

/// Read a measured dictionary, profile, and audit without retagging the protocol.
///
/// PostgreSQL columns use the dictionary `type_oid` and `typmod`. Other source
/// protocols use a type name only when it is already in the closed OOS set.
/// `_id` and other source identifiers stay as written. `bytea` stays unknown.
pub fn project_measured(
    catalog_bytes: &[u8],
    profile_bytes: &[u8],
    semantics_bytes: &[u8],
    pin: &ProtocolPin,
) -> Result<Projection, ApiError> {
    if !measured_catalog(&pin.catalog) || pin.profile.is_empty() || pin.semantics.is_empty() {
        return Err(ApiError::malformed());
    }
    let catalog = parse_object(catalog_bytes)?;
    let profile = parse_object(profile_bytes)?;
    let semantics = parse_object(semantics_bytes)?;
    if catalog.get("protocol").and_then(Value::as_str) != Some(pin.catalog.as_str())
        || profile.get("protocol").and_then(Value::as_str) != Some(pin.profile.as_str())
        || semantics.get("protocol").and_then(Value::as_str) != Some(pin.semantics.as_str())
    {
        return Err(ApiError::malformed());
    }
    let tables = catalog.get("tables").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    if tables.is_empty() {
        return Err(ApiError::malformed());
    }
    let (grade, grade_status) = observe_loose(semantics.get("consistency_grade").or_else(|| semantics.get("grade")));
    let (delete_detection, delete_status) = observe_loose(
        semantics
            .get("delete_detection")
            .or_else(|| semantics.get("delete_grade")),
    );
    let mut columns = Vec::new();
    let mut typed_tables = Vec::new();
    for table in tables {
        let schema = text_ident(table, "schema")?;
        let name = table
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| table.get("table").and_then(Value::as_str))
            .filter(|item| crate::ids::source_ident_ok(item))
            .ok_or_else(ApiError::malformed)?;
        let listed = table.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
        if listed.is_empty() {
            return Err(ApiError::malformed());
        }
        let mut typed_columns = Vec::new();
        for column in listed {
            let (column_name, data_type, oid, target_type, type_status, oos_support) = measure_column(&pin.catalog, column)?;
            if let Some(mapped) = data_type
                && crate::ids::source_ident_ok(&column_name)
            {
                typed_columns.push(json!({"data_type": mapped, "name": column_name}));
            }
            columns.push(json!({
                "auto_approved": false,
                "column": column_name,
                "data_type": data_type,
                "dataset": format!("{schema}.{name}"),
                "oid": oid,
                "oos_support": oos_support,
                "pii": "unknown",
                "pii_core_level": crate::corebind::core_pii_level("unknown"),
                "semantic": Value::Null,
                "semantic_core": crate::corebind::core_semantic("unknown"),
                "target_type": target_type,
                "type_status": type_status
            }));
        }
        if !typed_columns.is_empty() {
            typed_tables.push(json!({"columns": typed_columns, "schema": schema, "table": name}));
        }
    }
    let mapped = columns.iter().filter(|item| item.get("type_status").and_then(Value::as_str) == Some("mapped")).count();
    if mapped == 0 || typed_tables.is_empty() {
        return Err(ApiError::malformed());
    }
    let source_rows = source_rows(&semantics);
    let engine_name = pin
        .catalog
        .strip_prefix("oo.source.")
        .and_then(|rest| rest.split_once('/'))
        .map(|(name, _)| name)
        .unwrap_or("unknown");
    let engine = if engine_name == "pgsql" { "postgres" } else { engine_name };
    let receipt = json!({
        "auto_approved": false,
        "catalog_byte_sha256": sha256_hex(catalog_bytes),
        "catalog_protocol": pin.catalog,
        "columns": columns,
        "consistency_grade": grade,
        "delete_detection": delete_detection,
        "delete_status": delete_status,
        "grade_status": grade_status,
        "mapped_columns": mapped,
        "profile_byte_sha256": sha256_hex(profile_bytes),
        "profile_protocol": pin.profile,
        "rows_compared": Value::Null,
        "schema_hash": catalog.get("schema_hash").cloned().unwrap_or(Value::Null),
        "semantics_byte_sha256": sha256_hex(semantics_bytes),
        "semantics_commit": pin.semantics_commit,
        "semantics_protocol": pin.semantics,
        "source_commit": pin.source_commit,
        "source_id": catalog.get("source_id").cloned().unwrap_or(Value::Null),
        "source_rows": source_rows,
        "unknown_auto_approved": false,
        "arrow_basis": "not_present",
        "core_commit": crate::corebind::CORE_COMMIT,
        "derive_skip_reason": if engine == "postgres" { "engine_not_oracle" } else { "measured_dictionary" },
        "derive_source_bound_called": false,
        "engine": engine,
        "field_statement": "Measured dictionary bytes are kept. PostgreSQL uses type_oid and typmod. Other sources map a type name only when it is already a closed OOS type. bytea stays unsupported. Source identifiers are not renamed. Unknown grade and delete stay unknown. Nothing is auto-approved.",
        "version_binding": {
            "catalog_byte_sha256": sha256_hex(catalog_bytes),
            "catalog_protocol": pin.catalog,
            "core_commit": crate::corebind::CORE_COMMIT,
            "profile_byte_sha256": sha256_hex(profile_bytes),
            "profile_protocol": pin.profile,
            "semantics_byte_sha256": sha256_hex(semantics_bytes),
            "semantics_commit": pin.semantics_commit,
            "semantics_protocol": pin.semantics,
            "source_commit": pin.source_commit
        },
        "withheld_columns": columns.len() - mapped
    });
    Ok(Projection {
        typed_catalog: json!({"tables": typed_tables}),
        receipt,
    })
}

type MeasuredColumn = (
    String,
    Option<&'static str>,
    Option<i64>,
    Option<String>,
    &'static str,
    &'static str,
);

fn measure_column(protocol: &str, column: &Value) -> Result<MeasuredColumn, ApiError> {
    let object = column.as_object().ok_or_else(ApiError::malformed)?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty() && item.len() <= 128)
        .ok_or_else(ApiError::malformed)?;
    if !crate::ids::source_ident_ok(name) {
        return Ok((name.to_owned(), None, None, None, "source_identifier", "not_renamed"));
    }
    if protocol == PGSQL_PROTOCOL {
        let oid = object.get("type_oid").and_then(Value::as_i64).ok_or_else(ApiError::malformed)?;
        let typmod = object.get("typmod").and_then(Value::as_i64).ok_or_else(ApiError::malformed)?;
        let typmod = i32::try_from(typmod).map_err(|_| ApiError::malformed())?;
        let target = pg_target_name(oid, typmod);
        let mapped = target.and_then(oos_from_target_name);
        let (status, support) = match (target, mapped) {
            (Some(_), Some(_)) => ("mapped", "mapped"),
            (Some(_), None) => ("unknown", "oos_unsupported"),
            (None, _) => ("unknown", "unknown_oid"),
        };
        return Ok((name.to_owned(), mapped, Some(oid), target.map(str::to_owned), status, support));
    }
    let type_name = object
        .get("data_type")
        .and_then(Value::as_str)
        .or_else(|| object.get("type_name").and_then(Value::as_str))
        .unwrap_or("");
    if let Some(mapped) = oos_from_target_name(type_name).or_else(|| closed_oos(type_name)) {
        return Ok((name.to_owned(), Some(mapped), None, Some(type_name.to_owned()), "mapped", "mapped"));
    }
    let support = if type_name == "bytea" { "oos_unsupported" } else { "unknown" };
    let stored = if type_name.is_empty() { None } else { Some(type_name.to_owned()) };
    Ok((name.to_owned(), None, None, stored, "unknown", support))
}

fn closed_oos(name: &str) -> Option<&'static str> {
    match name {
        "boolean" => Some("boolean"),
        "bigint" => Some("bigint"),
        "integer" => Some("integer"),
        "text" => Some("text"),
        "date" => Some("date"),
        "timestamp" => Some("timestamp"),
        "timestamptz" => Some("timestamptz"),
        "numeric" => Some("numeric"),
        "uuid" => Some("uuid"),
        "jsonb" => Some("jsonb"),
        _ => None,
    }
}

fn observe_loose(value: Option<&Value>) -> (Value, &'static str) {
    match value {
        None | Some(Value::Null) => (Value::Null, "unknown"),
        Some(Value::String(token)) if token == "unknown" || token.is_empty() => (Value::Null, "unknown"),
        Some(Value::String(token)) if matches!(token.as_str(), "T" | "S1" | "S0" | "B" | "native" | "partition_replace" | "reconcile" | "none") => {
            (Value::String(token.clone()), "present")
        }
        _ => (Value::Null, "unrecognized"),
    }
}

fn source_rows(semantics: &Value) -> Value {
    let Some(counts) = semantics.pointer("/slot/source_counts").and_then(Value::as_array) else {
        return Value::Null;
    };
    let mut sum = 0u64;
    for item in counts {
        let Some(rows) = item.get("rows").and_then(Value::as_u64) else {
            return Value::Null;
        };
        sum = sum.saturating_add(rows);
    }
    json!(sum)
}

const PGSQL_PROTOCOL: &str = "oo.source.pgsql/v1";

/// Native importer catalog retains its engine and its original source type/caveat.
/// This is a structural projection of the actual importer export, never a PG protocol.
pub fn project_native(catalog_bytes: &[u8], profile_bytes: &[u8], semantics_bytes: &[u8], pin: &ProtocolPin) -> Result<Projection, ApiError> {
    let text = std::str::from_utf8(catalog_bytes).map_err(|_| ApiError::malformed())?;
    let native = oo_core::sealed_import::parse_sealed_import_catalog(text).map_err(|_| ApiError::malformed())?;
    let profile = parse_object(profile_bytes)?; let semantics = parse_object(semantics_bytes)?;
    if pin.catalog != "oo.pipeline.sealed-import-catalog/v1" || profile.get("protocol").and_then(Value::as_str) != Some(pin.profile.as_str()) || semantics.get("protocol").and_then(Value::as_str) != Some(pin.semantics.as_str()) { return Err(ApiError::malformed()); }
    for bound in [&profile, &semantics] {
        if bound.get("source_id").and_then(Value::as_str).is_some_and(|id| id != native.source_id) { return Err(ApiError::malformed()); }
    }
    let mut columns = Vec::new(); let mut tables = Vec::new();
    for table in &native.tables {
        let dataset = format!("{}.{}", table.logical_schema, table.logical_table);
        let mut typed = Vec::new();
        for column in &table.columns {
            let data_type = oos_from_target_name(column.target_type).unwrap_or(column.target_type);
            typed.push(json!({"name":column.logical_name,"data_type":data_type}));
            columns.push(json!({"dataset":dataset,"column":column.logical_name,"data_type":data_type,"target_type":column.target_type,"source_type":column.source_type,"caveat":column.caveat,"oid":column.oid,"typmod":column.typmod,"key":column.key,"auto_approved":false,"pii":"unknown","type_status":"mapped","oos_support":"mapped"}));
        }
        tables.push(json!({"schema":table.logical_schema,"table":table.logical_table,"columns":typed,"accepted_rows":table.accepted_rows}));
    }
    let mut receipt = json!({"catalog_protocol":pin.catalog,"source_engine":native.engine,"source_id":native.source_id,"source_generation":native.source_generation,"target_generation":native.target_generation,"schema_digest":native.schema_digest,"type_digest":native.type_digest,"catalog_sha256":sha256_hex(catalog_bytes),"profile_sha256":sha256_hex(profile_bytes),"semantics_sha256":sha256_hex(semantics_bytes),"columns":columns,"source_semantics":semantics,"grade_status":"source_declared","delete_status":"source_declared","source_commit":pin.source_commit,"semantics_commit":pin.semantics_commit});
    if let Some(vector) = &native.partition_vector { receipt["partition_vector"] = serde_json::to_value(vector).map_err(|_| ApiError::malformed())?; }
    Ok(Projection { typed_catalog:json!({"tables":tables}), receipt })
}

/// Project every producer-exported member of a collection without collapsing
/// its distinct run and digest bindings into a fabricated single catalog.
pub fn project_native_collection(catalog_bytes: &[u8], profile_bytes: &[u8], semantics_bytes: &[u8], pin: &ProtocolPin) -> Result<Projection, ApiError> {
    const COLLECTION: &str = "oo.pipeline.sealed-import-collection/v1";
    if pin.catalog != COLLECTION { return Err(ApiError::malformed()); }
    let mut document = parse_object(catalog_bytes)?;
    let declared = document.as_object_mut().ok_or_else(ApiError::malformed)?
        .remove("collection_sha256").and_then(|v| v.as_str().map(str::to_owned)).ok_or_else(ApiError::malformed)?;
    if document.as_object().is_none_or(|object| object.len() != 3)
        || document.get("protocol").and_then(Value::as_str) != Some(COLLECTION)
        || sha256_hex(&serde_json::to_vec(&document).map_err(|_| ApiError::malformed())?) != declared {
        return Err(ApiError::malformed());
    }
    let entries = document.get("entries").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    let bindings = document.get("binding_vector").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    if entries.is_empty() || entries.len() > 32 || entries.len() != bindings.len() { return Err(ApiError::malformed()); }
    let profile = parse_object(profile_bytes)?; let semantics = parse_object(semantics_bytes)?;
    if profile.get("protocol").and_then(Value::as_str) != Some(pin.profile.as_str())
        || profile.get("sampled") != Some(&Value::Bool(false))
        || semantics.get("protocol").and_then(Value::as_str) != Some(pin.semantics.as_str()) { return Err(ApiError::malformed()); }
    let vector_sha256 = sha256_hex(&serde_json::to_vec(bindings).map_err(|_| ApiError::malformed())?);
    if semantics.get("collection_sha256").and_then(Value::as_str) != Some(sha256_hex(catalog_bytes).as_str())
        || semantics.get("binding_vector_sha256").and_then(Value::as_str) != Some(vector_sha256.as_str()) { return Err(ApiError::malformed()); }
    let mut columns = Vec::new(); let mut tables = Vec::new(); let mut seen = std::collections::BTreeSet::new();
    let mut shared_engine: Option<String> = None;
    let mut source_ids = std::collections::BTreeSet::new();
    let mut generations = serde_json::Map::new();
    let mut max_source_generation = 0_i64; let mut max_target_generation = 0_i64;
    let mut first_generation: Option<(i64, i64)> = None; let mut mixed_generations = false;
    for (entry, binding) in entries.iter().zip(bindings) {
        let native = oo_core::sealed_import::parse_sealed_import_catalog(&serde_json::to_string(entry).map_err(|_| ApiError::malformed())?).map_err(|_| ApiError::malformed())?;
        if native.tables.len() != 1 { return Err(ApiError::malformed()); }
        if shared_engine.as_ref().is_some_and(|value| value != &native.engine) { return Err(ApiError::malformed()); }
        shared_engine = Some(native.engine.clone());
        source_ids.insert(native.source_id.clone());
        let table = &native.tables[0];
        let dataset = format!("{}.{}", table.logical_schema, table.logical_table);
        let generation = (native.source_generation, native.target_generation);
        if first_generation.is_some_and(|first| first != generation) { mixed_generations = true; }
        first_generation.get_or_insert(generation);
        max_source_generation = max_source_generation.max(native.source_generation);
        max_target_generation = max_target_generation.max(native.target_generation);
        generations.insert(dataset.clone(), json!({"source_id":native.source_id,"source_generation":native.source_generation,"target_generation":native.target_generation,"run_id":native.run_id}));
        if !seen.insert(dataset.clone()) || binding.as_object().is_none_or(|object| object.len() != 11)
            || binding.get("dataset").and_then(Value::as_str) != Some(dataset.as_str()) { return Err(ApiError::malformed()); }
        for key in ["source_id", "run_id", "source_generation", "target_generation", "config_digest", "schema_digest", "type_digest", "input_digest"] {
            if binding.get(key) != entry.get(key) { return Err(ApiError::malformed()); }
        }
        for (key, source_key) in [("stored_namespace", "stored_namespace"), ("stored_physical", "stored_physical")] {
            if binding.get(key) != entry.pointer(&format!("/tables/0/{source_key}")) { return Err(ApiError::malformed()); }
        }
        let mut typed = Vec::new();
        for column in &table.columns {
            let data_type = oos_from_target_name(column.target_type).unwrap_or(column.target_type);
            typed.push(json!({"name":column.logical_name,"data_type":data_type}));
            columns.push(json!({"dataset":dataset,"column":column.logical_name,"data_type":data_type,"target_type":column.target_type,"source_type":column.source_type,"caveat":column.caveat,"oid":column.oid,"typmod":column.typmod,"key":column.key,"auto_approved":false,"pii":"unknown","type_status":"mapped","oos_support":"mapped"}));
        }
        tables.push(json!({"schema":table.logical_schema,"table":table.logical_table,"columns":typed,"accepted_rows":table.accepted_rows}));
    }
    let engine = shared_engine.ok_or_else(ApiError::malformed)?;
    let multiple_sources = source_ids.len() > 1;
    let source_id = source_ids.iter().next().ok_or_else(ApiError::malformed)?.clone();
    if !multiple_sources {
        for generation in generations.values_mut() {
            generation.as_object_mut().ok_or_else(ApiError::malformed)?.remove("source_id");
        }
    }
    for bound in [&profile, &semantics] {
        if multiple_sources {
            if bound.get("source_id").is_some()
                || bound.get("source_ids") != Some(&json!(source_ids.iter().collect::<Vec<_>>()))
                || bound.get("binding_vector") != Some(&Value::Array(bindings.clone())) { return Err(ApiError::malformed()); }
        } else if bound.get("source_id").and_then(Value::as_str) != Some(source_id.as_str()) { return Err(ApiError::malformed()); }
    }
    if profile.get("source_generation").and_then(Value::as_i64) != Some(max_source_generation)
        || semantics.get("source_generation").and_then(Value::as_i64) != Some(max_source_generation)
        || semantics.get("target_generation").and_then(Value::as_i64) != Some(max_target_generation) { return Err(ApiError::malformed()); }
    let expected_generations = Value::Object(generations);
    for bound in [&profile, &semantics] {
        if (mixed_generations || multiple_sources) && (bound.get("generation_semantics").and_then(Value::as_str) != Some("high_watermark")
            || bound.get("table_generations") != Some(&expected_generations)
            || bound.get("binding_vector_sha256").and_then(Value::as_str) != Some(vector_sha256.as_str())
            || bound.get("collection_sha256").and_then(Value::as_str) != Some(sha256_hex(catalog_bytes).as_str())) { return Err(ApiError::malformed()); }
        if bound.get("table_generations").is_some() && bound.get("table_generations") != Some(&expected_generations) { return Err(ApiError::malformed()); }
    }
    let profile_tables = profile.get("tables").and_then(Value::as_array).ok_or_else(ApiError::malformed)?;
    if profile_tables.len() != seen.len() { return Err(ApiError::malformed()); }
    let mut profiled = std::collections::BTreeSet::new();
    let mut source_types = std::collections::BTreeMap::new();
    for item in profile_tables {
        let dataset = item.get("dataset").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
        let rows = item.get("rows").and_then(Value::as_u64).ok_or_else(ApiError::malformed)?;
        if item.get("full_scan") != Some(&Value::Bool(true)) || !seen.contains(dataset) || !profiled.insert(dataset)
            || !tables.iter().any(|table| table.get("accepted_rows").and_then(Value::as_u64) == Some(rows)
                && table.get("schema").and_then(Value::as_str).zip(table.get("table").and_then(Value::as_str)).is_some_and(|(s,t)| format!("{s}.{t}") == dataset)) { return Err(ApiError::malformed()); }
        if multiple_sources && item.get("source_id") != expected_generations.get(dataset).and_then(|v| v.get("source_id")) { return Err(ApiError::malformed()); }
        if (mixed_generations || multiple_sources) && (item.get("source_generation") != expected_generations.get(dataset).and_then(|v| v.get("source_generation"))
            || item.get("target_generation") != expected_generations.get(dataset).and_then(|v| v.get("target_generation"))
            || item.get("run_id") != expected_generations.get(dataset).and_then(|v| v.get("run_id"))) { return Err(ApiError::malformed()); }
        for column in item.get("columns").and_then(Value::as_array).ok_or_else(ApiError::malformed)? {
            let name = column.get("name").and_then(Value::as_str).ok_or_else(ApiError::malformed)?;
            let source_type = column.get("source_type").and_then(Value::as_str).filter(|value| !value.is_empty()).ok_or_else(ApiError::malformed)?;
            if source_types.insert((dataset.to_owned(), name.to_owned()), source_type.to_owned()).is_some() { return Err(ApiError::malformed()); }
        }
    }
    if source_types.len() != columns.len() { return Err(ApiError::malformed()); }
    for column in &mut columns {
        let key = (column.get("dataset").and_then(Value::as_str).ok_or_else(ApiError::malformed)?.to_owned(), column.get("column").and_then(Value::as_str).ok_or_else(ApiError::malformed)?.to_owned());
        let original = source_types.get(&key).ok_or_else(ApiError::malformed)?;
        column["captured_type"] = column["source_type"].clone();
        column["source_type"] = json!(original);
    }
    Ok(Projection { typed_catalog:json!({"tables":tables}), receipt:json!({"catalog_protocol":COLLECTION,"source_engine":engine,"source_id":if multiple_sources { Value::Null } else { json!(source_id) },"source_ids":source_ids,"generation_semantics":if mixed_generations || multiple_sources { "per_dataset_binding_vector" } else { "uniform" },"source_generation_high_watermark":max_source_generation,"target_generation_high_watermark":max_target_generation,"table_generations":expected_generations,"collection_sha256":declared,"catalog_sha256":sha256_hex(catalog_bytes),"profile_sha256":sha256_hex(profile_bytes),"semantics_sha256":sha256_hex(semantics_bytes),"binding_vector":bindings,"columns":columns,"source_semantics":semantics,"grade_status":"source_declared","delete_status":"source_declared","source_commit":pin.source_commit,"semantics_commit":pin.semantics_commit}) })
}
