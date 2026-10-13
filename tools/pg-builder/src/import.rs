//! Sealed-import catalog path.
//!
//! `import` spawns `oo-pipeline catalog-import`. It does not open PostgreSQL
//! itself and it does not run `catalog-pg`. `import-cards` keeps logical names.
//! `import-seal` sends `oo.pipeline.sealed-import-catalog/v1` to
//! `compile_sealed_import_catalog` and records dependency `b1ffb079`.
//! `oo.source.pgsql/v1` still uses `compile_captured_pg_catalog`.
//! This module does not call a model or write a pack.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use oo_builder::compile_captured_pg_catalog;

use crate::sealed::{draft_only, DEPENDENCY_SHA};
use serde_json::{json, Map, Value};

use crate::error::PgBuilderError;
use crate::profile::{contains_secret, read_capped, read_password, wait_child};

pub const REQUEST_PROTOCOL: &str = "oo.pg-builder.import-request/v1";
pub const CARDS_PROTOCOL: &str = "oo.pg-builder.import-cards/v1";
pub const SEAL_PROTOCOL: &str = "oo.pg-builder.import-seal/v1";
const CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";
const IMPORT_PROTOCOL: &str = "oo.pipeline.catalog-import/v1";
const PG_IMPORT_PROTOCOL: &str = "oo.pipeline.catalog-import-pg/v1";
const PG_MANIFEST_PROTOCOL: &str = "oo.pipeline.pg-sealed-manifest/v1";

/// Builder request for `import-pg-catalog`. Not the MySQL import request.
pub const PG_CATALOG_REQUEST_PROTOCOL: &str = "oo.pg-builder.import-pg-catalog-request/v1";

/// The compiler on the fixed base accepts only `oo.source.pgsql/v1`.
pub const MISSING_CONTRACT: &str = "\
compile_captured_pg_catalog accepts only a catalog that deserializes as oo.source.pgsql/v1. \
Any other protocol is oo.builder.catalog-evidence/v1/version.unsupported and the message is \
catalog protocol is not oo.source.pgsql/v1. A document that is not that catalog fails earlier, \
and the message is catalog evidence is not oo.source.pgsql/v1.";

/// Spawn `catalog-import` and return its stdout unchanged.
pub fn run_import(request_path: &Path, stderr_path: Option<&Path>) -> Result<Vec<u8>, PgBuilderError> {
    let request_bytes = read_file(request_path, 1024 * 1024)?;
    let request_text = std::str::from_utf8(&request_bytes)
        .map_err(|_| PgBuilderError::new("input.schema", "an input file is not UTF-8"))?;
    let request: Value = serde_json::from_str(request_text)
        .map_err(|_| PgBuilderError::new("input.schema", "an input file is not JSON"))?;
    let parsed = parse_request(&request)?;
    let manifest_bytes = read_file(Path::new(&parsed.manifest_file), 8 * 1024 * 1024)?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| PgBuilderError::new("input.schema", "an input file is not JSON"))?;
    if manifest.get("protocol").and_then(Value::as_str) != Some("oo.mysql-preview.manifest/v1") {
        return Err(PgBuilderError::new("input.schema", "the manifest protocol is not the sealed import manifest"));
    }
    let password = read_password(Path::new(&parsed.password_file))?;
    if contains_secret(&request_bytes, password.as_bytes()) || contains_secret(&manifest_bytes, password.as_bytes()) {
        return Err(PgBuilderError::new("catalog.secret", "the catalog output contained a secret"));
    }
    let stdin = serde_json::to_vec(&json!({
        "protocol": IMPORT_PROTOCOL,
        "source_id": parsed.source_id,
        "config_digest": parsed.config_digest,
        "source_generation": parsed.source_generation,
        "manifest": manifest,
        "target": {
            "host": parsed.host,
            "port": parsed.port,
            "database": parsed.database,
            "runtime_principal": parsed.runtime_principal,
            "password_file": parsed.password_file,
            "ca_file": parsed.ca_file
        }
    }))
    .map_err(|_| PgBuilderError::new("catalog.schema", "the catalog request could not be encoded"))?;
    if !Path::new(&parsed.catalog_bin).is_file() {
        return Err(PgBuilderError::new("catalog.bin", "the catalog binary is not a file"));
    }
    let mut child = Command::new(&parsed.catalog_bin)
        .arg("catalog-import")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    {
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
        input
            .write_all(&stdin)
            .map_err(|_| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    let out_handle = std::thread::spawn(move || read_capped(stdout, 8 * 1024 * 1024));
    let err_handle = std::thread::spawn(move || read_capped(stderr, 8192));
    let status = wait_child(&mut child, Duration::from_secs(60))?;
    let out = out_handle
        .join()
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?;
    let err = err_handle
        .join()
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?
        .unwrap_or_default();
    if let Some(path) = stderr_path {
        let redacted = if contains_secret(&err, password.as_bytes()) {
            b"redacted\n".to_vec()
        } else {
            err
        };
        let _ = fs::write(path, redacted);
    }
    if contains_secret(&out, password.as_bytes()) {
        return Err(PgBuilderError::new("catalog.secret", "the catalog output contained a secret"));
    }
    if !status.success() || out.is_empty() {
        return Err(PgBuilderError::new("catalog.refused", "the catalog command was refused"));
    }
    let text = std::str::from_utf8(&out).map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?;
    if !text.contains(CATALOG_PROTOCOL) || text.contains("oo.source.pgsql/v1") {
        return Err(PgBuilderError::new("catalog.output", "the catalog output was refused"));
    }
    Ok(out)
}

/// Spawn `catalog-import-pg`. A MySQL manifest is refused before the process starts.
pub fn run_import_pg_catalog(request_path: &Path, stderr_path: Option<&Path>) -> Result<Vec<u8>, PgBuilderError> {
    let request_bytes = read_file(request_path, 1024 * 1024)?;
    let request_text = std::str::from_utf8(&request_bytes)
        .map_err(|_| PgBuilderError::new("input.schema", "an input file is not UTF-8"))?;
    let request: Value = serde_json::from_str(request_text)
        .map_err(|_| PgBuilderError::new("input.schema", "an input file is not JSON"))?;
    let parsed = parse_pg_request(&request)?;
    let manifest_bytes = read_file(Path::new(&parsed.manifest_file), 8 * 1024 * 1024)?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|_| PgBuilderError::new("input.schema", "an input file is not JSON"))?;
    if manifest.get("protocol").and_then(Value::as_str) != Some(PG_MANIFEST_PROTOCOL)
        || manifest.get("engine").and_then(Value::as_str) != Some("postgresql")
    {
        return Err(PgBuilderError::new(
            "input.schema",
            "the manifest protocol is not the postgresql sealed import manifest",
        ));
    }
    let password = read_password(Path::new(&parsed.password_file))?;
    if contains_secret(&request_bytes, password.as_bytes()) || contains_secret(&manifest_bytes, password.as_bytes()) {
        return Err(PgBuilderError::new("catalog.secret", "the catalog output contained a secret"));
    }
    let stdin = serde_json::to_vec(&json!({
        "protocol": PG_IMPORT_PROTOCOL,
        "source_id": parsed.source_id,
        "config_digest": parsed.config_digest,
        "source_generation": parsed.source_generation,
        "owner": parsed.owner,
        "manifest": manifest,
        "target": {
            "host": parsed.host,
            "port": parsed.port,
            "database": parsed.database,
            "runtime_principal": parsed.runtime_principal,
            "password_file": parsed.password_file,
            "ca_file": parsed.ca_file
        }
    }))
    .map_err(|_| PgBuilderError::new("catalog.schema", "the catalog request could not be encoded"))?;
    if !Path::new(&parsed.catalog_bin).is_file() {
        return Err(PgBuilderError::new("catalog.bin", "the catalog binary is not a file"));
    }
    let mut child = Command::new(&parsed.catalog_bin)
        .arg("catalog-import-pg")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    {
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
        input
            .write_all(&stdin)
            .map_err(|_| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    let out_handle = std::thread::spawn(move || read_capped(stdout, 8 * 1024 * 1024));
    let err_handle = std::thread::spawn(move || read_capped(stderr, 8192));
    let status = wait_child(&mut child, Duration::from_secs(60))?;
    let out = out_handle
        .join()
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?;
    let err = err_handle
        .join()
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?
        .unwrap_or_default();
    if let Some(path) = stderr_path {
        let redacted = if contains_secret(&err, password.as_bytes()) {
            b"redacted\n".to_vec()
        } else {
            err
        };
        let _ = fs::write(path, redacted);
    }
    if contains_secret(&out, password.as_bytes()) {
        return Err(PgBuilderError::new("catalog.secret", "the catalog output contained a secret"));
    }
    if !status.success() || out.is_empty() {
        return Err(PgBuilderError::new("catalog.refused", "the catalog command was refused"));
    }
    let text = std::str::from_utf8(&out).map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?;
    if !text.contains(CATALOG_PROTOCOL)
        || text.contains("oo.source.pgsql/v1")
        || !text.contains("\"engine\":\"postgresql\"")
    {
        return Err(PgBuilderError::new("catalog.output", "the catalog output was refused"));
    }
    Ok(out)
}

/// Logical cards from one sealed-import catalog. Physical names are dropped.
pub fn import_cards(catalog: &Value) -> Result<Value, PgBuilderError> {
    let map = catalog
        .as_object()
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog is not the sealed import catalog"))?;
    if map.get("protocol").and_then(Value::as_str) != Some(CATALOG_PROTOCOL) {
        return Err(PgBuilderError::new("catalog.schema", "the catalog is not the sealed import catalog"));
    }
    let engine = map.get("engine").and_then(Value::as_str).unwrap_or("");
    if !matches!(engine, "mysql" | "postgresql" | "mariadb") {
        return Err(PgBuilderError::new(
            "catalog.schema",
            "the catalog source engine is not a sealed import engine",
        ));
    }
    if map.get("capture").and_then(Value::as_str) != Some("sealed_import")
        || map.get("source_introspection").and_then(Value::as_str) != Some("not_used")
        || map.get("shadows_queried").and_then(Value::as_bool) != Some(false)
        || map.get("hook_checked").and_then(Value::as_bool) != Some(false)
    {
        return Err(PgBuilderError::new("catalog.schema", "the catalog is not a read of the sealed import"));
    }
    let tables_in = map
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog has no tables"))?;
    if tables_in.is_empty() || tables_in.len() > 16 {
        return Err(PgBuilderError::new("catalog.schema", "the catalog has no tables"));
    }
    let mut tables = Vec::new();
    for table in tables_in {
        let table = table
            .as_object()
            .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog has no tables"))?;
        let columns_in = table
            .get("columns")
            .and_then(Value::as_array)
            .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog has no tables"))?;
        let mut columns = Vec::new();
        for column in columns_in {
            let column = column
                .as_object()
                .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog has no tables"))?;
            columns.push(json!({
                "caveat": column.get("caveat").cloned().unwrap_or(Value::Null),
                "key": column.get("key").cloned().unwrap_or(Value::Null),
                "logical_name": required_string(column, "logical_name")?,
                "oid": required_number(column, "oid")?,
                "source_type": column.get("source_type").cloned().unwrap_or(Value::Null),
                "typmod": required_number(column, "typmod")?
            }));
        }
        tables.push(json!({
            "accepted_rows": required_number(table, "accepted_rows")?,
            "canonical_digest": required_string(table, "canonical_digest")?,
            "columns": columns,
            "logical_schema": required_string(table, "logical_schema")?,
            "logical_table": required_string(table, "logical_table")?,
            "rejects": table.get("rejects").cloned().unwrap_or_else(|| Value::Array(Vec::new()))
        }));
    }
    let quality = strip_physical(map.get("quality").unwrap_or(&Value::Null));
    let cards = json!({
        "capture": "sealed_import",
        "config_digest": required_string(map, "config_digest")?,
        "engine": engine,
        "protocol": CARDS_PROTOCOL,
        "quality": quality,
        "run_id": required_string(map, "run_id")?,
        "sample_policy": "none",
        "schema_digest": required_string(map, "schema_digest")?,
        "shadows_queried": false,
        "source_generation": required_number(map, "source_generation")?,
        "source_id": required_string(map, "source_id")?,
        "source_introspection": "not_used",
        "stats": "withheld",
        "tables": tables,
        "target_generation": required_number(map, "target_generation")?
    });
    if leaks_physical(&cards) {
        return Err(PgBuilderError::new("catalog.schema", "the import cards contained a physical identifier"));
    }
    Ok(cards)
}

/// Dispatch one catalog. Sealed imports use the fixed compiler. A PostgreSQL
/// source catalog still uses `compile_captured_pg_catalog` and is not relabeled.
pub fn import_seal(catalog_json: &str) -> Result<Value, PgBuilderError> {
    let protocol = serde_json::from_str::<Value>(catalog_json)
        .ok()
        .and_then(|value| value.get("protocol").and_then(Value::as_str).map(str::to_owned));
    if protocol.as_deref() == Some(CATALOG_PROTOCOL) {
        let mut document = draft_only(catalog_json)?;
        if document.get("outcome").and_then(Value::as_str) == Some("draft_compiled") {
            document["protocol"] = json!(SEAL_PROTOCOL);
            document["dependency_sha"] = json!(DEPENDENCY_SHA);
        }
        return Ok(document);
    }
    match compile_captured_pg_catalog(catalog_json, None, None) {
        Ok(_) => Err(PgBuilderError::new(
            "compiler.contract",
            "the sealed import catalog was compiled as a PostgreSQL source",
        )),
        Err(err) => Ok(json!({
            "code": err.code,
            "compiler": "compile_captured_pg_catalog",
            "message": err.message,
            "missing_contract": MISSING_CONTRACT,
            "model": "not_called",
            "outcome": "compiler_refused",
            "pack": "not_produced",
            "protocol": SEAL_PROTOCOL
        })),
    }
}

struct ParsedRequest {
    catalog_bin: String,
    source_id: String,
    config_digest: String,
    source_generation: u64,
    manifest_file: String,
    host: String,
    port: u16,
    database: String,
    runtime_principal: String,
    password_file: String,
    ca_file: String,
}

struct ParsedPgRequest {
    catalog_bin: String,
    source_id: String,
    config_digest: String,
    source_generation: u64,
    owner: String,
    manifest_file: String,
    host: String,
    port: u16,
    database: String,
    runtime_principal: String,
    password_file: String,
    ca_file: String,
}

fn parse_pg_request(value: &Value) -> Result<ParsedPgRequest, PgBuilderError> {
    let map = exact(
        value,
        &[
            "catalog_bin",
            "config_digest",
            "manifest_file",
            "owner",
            "protocol",
            "source_generation",
            "source_id",
            "target",
        ],
    )?;
    if map.get("protocol").and_then(Value::as_str) != Some(PG_CATALOG_REQUEST_PROTOCOL) {
        return Err(PgBuilderError::new("input.schema", "the import request protocol is not supported"));
    }
    let owner = request_string(map, "owner")?;
    if !closed_owner(&owner) {
        return Err(PgBuilderError::new("input.schema", "the import request is not closed"));
    }
    let target = exact(
        map.get("target").unwrap_or(&Value::Null),
        &["ca_file", "database", "host", "password_file", "port", "runtime_principal"],
    )?;
    let source_generation = map
        .get("source_generation")
        .and_then(Value::as_u64)
        .filter(|value| *value >= 1)
        .ok_or_else(|| PgBuilderError::new("input.schema", "the import request is not closed"))?;
    let port = target
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| PgBuilderError::new("input.schema", "the import request is not closed"))?;
    Ok(ParsedPgRequest {
        catalog_bin: request_string(map, "catalog_bin")?,
        source_id: request_string(map, "source_id")?,
        config_digest: required_digest(map, "config_digest")?,
        source_generation,
        owner,
        manifest_file: request_string(map, "manifest_file")?,
        host: request_string(target, "host")?,
        port,
        database: request_string(target, "database")?,
        runtime_principal: request_string(target, "runtime_principal")?,
        password_file: request_string(target, "password_file")?,
        ca_file: request_string(target, "ca_file")?,
    })
}

fn closed_owner(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            let hyphen = matches!(index, 8 | 13 | 18 | 23);
            (hyphen && *byte == b'-') || (!hyphen && byte.is_ascii_hexdigit())
        })
}

fn parse_request(value: &Value) -> Result<ParsedRequest, PgBuilderError> {
    let map = exact(
        value,
        &[
            "catalog_bin",
            "config_digest",
            "manifest_file",
            "protocol",
            "source_generation",
            "source_id",
            "target",
        ],
    )?;
    if map.get("protocol").and_then(Value::as_str) != Some(REQUEST_PROTOCOL) {
        return Err(PgBuilderError::new("input.schema", "the import request protocol is not supported"));
    }
    let target = exact(
        map.get("target").unwrap_or(&Value::Null),
        &["ca_file", "database", "host", "password_file", "port", "runtime_principal"],
    )?;
    let source_generation = map
        .get("source_generation")
        .and_then(Value::as_u64)
        .filter(|value| *value >= 1)
        .ok_or_else(|| PgBuilderError::new("input.schema", "the import request is not closed"))?;
    let port = target
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| PgBuilderError::new("input.schema", "the import request is not closed"))?;
    Ok(ParsedRequest {
        catalog_bin: request_string(map, "catalog_bin")?,
        source_id: request_string(map, "source_id")?,
        config_digest: required_digest(map, "config_digest")?,
        source_generation,
        manifest_file: request_string(map, "manifest_file")?,
        host: request_string(target, "host")?,
        port,
        database: request_string(target, "database")?,
        runtime_principal: request_string(target, "runtime_principal")?,
        password_file: request_string(target, "password_file")?,
        ca_file: request_string(target, "ca_file")?,
    })
}

fn request_string(map: &Map<String, Value>, key: &str) -> Result<String, PgBuilderError> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| PgBuilderError::new("input.schema", "the import request is not closed"))
}

fn exact<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, PgBuilderError> {
    let map = value
        .as_object()
        .ok_or_else(|| PgBuilderError::new("input.schema", "the import request is not closed"))?;
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return Err(PgBuilderError::new("input.schema", "the import request is not closed"));
    }
    Ok(map)
}

fn required_string(map: &Map<String, Value>, key: &str) -> Result<String, PgBuilderError> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "the catalog is not the sealed import catalog"))
}

fn required_digest(map: &Map<String, Value>, key: &str) -> Result<String, PgBuilderError> {
    let value = request_string(map, key)?;
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        Ok(value)
    } else {
        Err(PgBuilderError::new("input.schema", "the import request is not closed"))
    }
}

fn required_number(map: &Map<String, Value>, key: &str) -> Result<Value, PgBuilderError> {
    match map.get(key) {
        Some(value) if value.is_number() => Ok(value.clone()),
        _ => Err(PgBuilderError::new("catalog.schema", "the catalog is not the sealed import catalog")),
    }
}

fn strip_physical(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(strip_physical).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, child) in map {
                if key.contains("physical") {
                    continue;
                }
                out.insert(key.clone(), strip_physical(child));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn leaks_physical(value: &Value) -> bool {
    match value {
        Value::String(text) => {
            text.contains("oo.source.pgsql/v1")
                || text.starts_with("pt_")
                || text.starts_with("pc_")
                || text.contains("SELECT ")
                || text.contains("pg_attribute")
        }
        Value::Array(items) => items.iter().any(leaks_physical),
        Value::Object(map) => {
            map.keys().any(|key| key.contains("physical") || key == "sql") || map.values().any(leaks_physical)
        }
        _ => false,
    }
}

fn read_file(path: &Path, max: usize) -> Result<Vec<u8>, PgBuilderError> {
    let meta = fs::symlink_metadata(path).map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))?;
    if !meta.file_type().is_file() || meta.len() > max as u64 {
        return Err(PgBuilderError::new("input.read", "an input file could not be read"));
    }
    let file = fs::File::open(path).map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))?;
    read_capped(file, max).map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_catalog_request_rejects_the_mysql_protocol() {
        let body = r#"{"protocol":"oo.pg-builder.import-request/v1","catalog_bin":"/bin/oo-pipeline","config_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","manifest_file":"/manifest.json","owner":"b5ac4bc8-1872-4fe4-a56a-04a70f7103a2","source_generation":1,"source_id":"oo_dl01w1_46","target":{"ca_file":"/ca","database":"previewdb","host":"db","password_file":"/pw","port":5432,"runtime_principal":"oo_admin"}}"#;
        let value: Value = serde_json::from_str(body).expect("json");
        assert!(parse_pg_request(&value).is_err());
        assert!(parse_request(&value).is_err());
        let dir = std::env::temp_dir().join(format!("oo-pg-catalog-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let manifest = dir.join("manifest.json");
        fs::write(
            &manifest,
            r#"{"protocol":"oo.mysql-preview.manifest/v1","engine":"postgresql"}"#,
        )
        .expect("manifest");
        let request = dir.join("request.json");
        let text = format!(
            "{{\"protocol\":\"oo.pg-builder.import-pg-catalog-request/v1\",\"catalog_bin\":\"/bin/oo-pipeline\",\"config_digest\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"manifest_file\":\"{}\",\"owner\":\"b5ac4bc8-1872-4fe4-a56a-04a70f7103a2\",\"source_generation\":1,\"source_id\":\"oo_dl01w1_46\",\"target\":{{\"ca_file\":\"/ca\",\"database\":\"previewdb\",\"host\":\"db\",\"password_file\":\"/pw\",\"port\":5432,\"runtime_principal\":\"oo_admin\"}}}}",
            manifest.display()
        );
        fs::write(&request, text).expect("request");
        let err = run_import_pg_catalog(&request, None).expect_err("mysql manifest");
        assert_eq!(err.message, "the manifest protocol is not the postgresql sealed import manifest");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_request_rejects_sql_and_a_physical_name() {
        let body = r#"{"protocol":"oo.pg-builder.import-request/v1","sql":"select 1"}"#;
        let value: Value = serde_json::from_str(body).expect("json");
        assert!(parse_request(&value).is_err());
        let physical = r#"{"protocol":"oo.pg-builder.import-request/v1","physical_name":"pt_x"}"#;
        let value: Value = serde_json::from_str(physical).expect("json");
        assert!(parse_request(&value).is_err());
    }

    #[test]
    fn import_cards_drop_stored_physical_names() {
        let catalog = json!({
            "protocol": "oo.pipeline.sealed-import-catalog/v1",
            "engine": "mysql",
            "capture": "sealed_import",
            "source_introspection": "not_used",
            "shadows_queried": false,
            "hook_checked": false,
            "config_digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "schema_digest": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "source_id": "mysqlpreview",
            "run_id": "de8276fc-8634-4543-994f-ce82a46a3c18",
            "source_generation": 1,
            "target_generation": 3,
            "quality": {
                "loss_concealed": false,
                "manifest_reject_count": 1,
                "payload_includes_reject_lines": false,
                "stored_omits_manifest_rejects": true,
                "stored_rejected_rows": 0,
                "stored_rows": [{
                    "physical_name": "pt_9cb24f0ac8a48d5f4f4f6d93f5ebe7f2c4162c9ae5237040365d3f2047ce",
                    "kind": "batch",
                    "rejected_rows": 0
                }]
            },
            "tables": [{
                "logical_schema": "erp",
                "logical_table": "parcel",
                "accepted_rows": 5,
                "canonical_digest": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "stored_namespace": "erp",
                "stored_physical": "pt_9cb24f0ac8a48d5f4f4f6d93f5ebe7f2c4162c9ae5237040365d3f2047ce",
                "columns": [{
                    "logical_name": "day_on",
                    "oid": 1082,
                    "typmod": -1,
                    "caveat": null,
                    "key": false,
                    "stored_physical": "pc_52d0c72963300da68527afe60616493134e2ad82e71c59232797eeafa63f"
                }],
                "rejects": [{"column": "day_on", "reason": "zero_date", "row_index": 1}]
            }]
        });
        let cards = import_cards(&catalog).expect("cards");
        let encoded = serde_json::to_string(&cards).expect("json");
        assert!(!encoded.contains("pt_"));
        assert!(!encoded.contains("pc_"));
        assert!(!encoded.contains("physical"));
        assert_eq!(cards["sample_policy"], "none");
        assert_eq!(cards["stats"], "withheld");
        assert_eq!(cards["shadows_queried"], false);
        assert_eq!(cards["source_introspection"], "not_used");
        assert_eq!(cards["tables"][0]["logical_schema"], "erp");
        assert_eq!(cards["tables"][0]["columns"][0]["oid"], 1082);
        assert_eq!(cards["quality"]["stored_omits_manifest_rejects"], true);
        assert_eq!(cards["quality"]["loss_concealed"], false);
        assert_eq!(cards["engine"], "mysql");
        let mut postgres = catalog.clone();
        postgres["engine"] = json!("postgresql");
        let pg_cards = import_cards(&postgres).expect("postgresql cards");
        assert_eq!(pg_cards["engine"], "postgresql");
        assert_eq!(pg_cards["tables"][0]["columns"][0]["oid"], 1082);
        let mut maria = catalog.clone();
        maria["engine"] = json!("mariadb");
        assert_eq!(import_cards(&maria).expect("mariadb cards")["engine"], "mariadb");
        let mut other = catalog;
        other["engine"] = json!("snowflake");
        assert!(import_cards(&other).is_err());
    }

    #[test]
    fn import_seal_records_the_pgsql_compiler_refusal() {
        let not_catalog = r#"{"not":"a catalog"}"#;
        let early = import_seal(not_catalog).expect("refusal");
        assert_eq!(early["outcome"], "compiler_refused");
        assert_eq!(early["compiler"], "compile_captured_pg_catalog");
        assert_eq!(early["message"], "catalog evidence is not oo.source.pgsql/v1");
        assert_eq!(early["pack"], "not_produced");
        assert_eq!(early["model"], "not_called");
        assert_eq!(early["missing_contract"], MISSING_CONTRACT);

        let shaped = r#"{"protocol":"oo.source.pgsql/v1","core_contract":"x","source_id":"mysqlpreview","database":"previewdb","captured_at":"2026-01-01T00:00:00.000Z","schema_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","hash_algorithm":"sha256","ident_algorithm":"pgident/v1","tables":[],"type_conversion":"unsupported"}"#;
        let shaped_seal = import_seal(shaped).expect("refusal");
        assert_eq!(shaped_seal["outcome"], "compiler_refused");
        assert_eq!(shaped_seal["compiler"], "compile_captured_pg_catalog");
        assert_eq!(
            shaped_seal["message"],
            "catalog hash, ident, contract, or type conversion is not supported"
        );
        assert_eq!(shaped_seal["pack"], "not_produced");
        assert_eq!(shaped_seal["model"], "not_called");
        assert_eq!(shaped_seal["missing_contract"], MISSING_CONTRACT);

        let live = r#"{"protocol":"oo.pipeline.sealed-import-catalog/v1","engine":"mysql","capture":"sealed_import","tables":[]}"#;
        let live_seal = import_seal(live).expect("refusal");
        assert_eq!(live_seal["outcome"], "compiler_refused");
        assert_eq!(live_seal["compiler"], "compile_sealed_import_catalog");
        assert_eq!(live_seal["dependency_sha"], crate::sealed::DEPENDENCY_SHA);
        assert_eq!(live_seal["pack"], "not_produced");
        assert_eq!(live_seal["model"], "not_called");
        let code = live_seal["code"].as_str().unwrap_or("");
        assert!(code.starts_with("oo.builder.sealed-import-draft/v1/"), "{code}");
    }

    #[test]
    fn import_seal_does_not_call_the_model_or_pack() {
        let src = include_str!("import.rs");
        let forbidden = [
            concat!("post_loop", "back"),
            concat!("pack_", "pair"),
            concat!("prepare_description_", "request"),
        ];
        for word in forbidden {
            assert!(!src.contains(word));
        }
        assert!(src.contains("compile_captured_pg_catalog"));
        assert!(src.contains("compile_sealed_import_catalog") || src.contains("draft_only"));
        assert!(src.contains(crate::sealed::DEPENDENCY_SHA) || src.contains("DEPENDENCY_SHA"));
    }
}
