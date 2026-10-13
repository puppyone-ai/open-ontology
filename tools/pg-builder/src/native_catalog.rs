//! Public native sealed-catalog command.
//!
//! `import-native-catalog` forwards `oo.pipeline.native-sealed-catalog/v1` to
//! `oo-pipeline catalog-import-native`. The request has no engine field.
//! MySQL stays on `import`. PostgreSQL stays on `import-pg-catalog`.
//! This module does not call a model and does not write a pack.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::error::PgBuilderError;
use crate::profile::{contains_secret, read_capped, read_password, wait_child};

/// Request forwarded unchanged to `oo-pipeline catalog-import-native`.
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.native-sealed-catalog/v1";
/// Catalog document written on success.
pub const CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";

const CLIENT_AUTHORITY: &[&str] = &[
    "admitted",
    "admission_bin",
    "registry_path",
    "registry_sha256",
    "approval",
];

/// Read a native sealed catalog. A refusal returns before `--out` is written.
pub fn run_import_native_catalog(
    request_path: &Path,
    pipeline: &Path,
    stderr_path: Option<&Path>,
) -> Result<Vec<u8>, PgBuilderError> {
    let request_bytes = read_file(request_path, 1024 * 1024)?;
    let request: Value = serde_json::from_slice(&request_bytes)
        .map_err(|_| PgBuilderError::new("input.schema", "an input file is not JSON"))?;
    gate_request(&request)?;
    let password_file = request
        .pointer("/target/password_file")
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("input.schema", "an input file is not JSON"))?;
    let password = read_password(Path::new(password_file))?;
    if contains_secret(&request_bytes, password.as_bytes()) {
        return Err(PgBuilderError::new("catalog.secret", "the catalog output contained a secret"));
    }
    if !pipeline.is_file() {
        return Err(PgBuilderError::new("catalog.bin", "the catalog binary is not a file"));
    }
    let out = spawn_catalog(pipeline, &request_bytes, &password, stderr_path)?;
    let document: Value = serde_json::from_slice(trim_ascii(&out))
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?;
    if document.get("protocol").and_then(Value::as_str) != Some(CATALOG_PROTOCOL) {
        return Err(PgBuilderError::new("catalog.output", "the catalog output was refused"));
    }
    let engine = document.get("engine").and_then(Value::as_str).unwrap_or("");
    if !native_engine(engine) {
        return Err(PgBuilderError::new("catalog.output", "the catalog output was refused"));
    }
    let text = std::str::from_utf8(&out)
        .map_err(|_| PgBuilderError::new("catalog.output", "the catalog output was refused"))?;
    if text.contains("oo.source.pgsql/v1") {
        return Err(PgBuilderError::new("catalog.output", "the catalog output was refused"));
    }
    legal_document_columns(&document)?;
    Ok(out)
}

/// A jsonb column is oid 3802 typmod -1. Json oid 114 stays a json column.
fn legal_document_columns(document: &Value) -> Result<(), PgBuilderError> {
    let refused = || PgBuilderError::new("catalog.output", "the catalog output was refused");
    let tables = document.get("tables").and_then(Value::as_array).ok_or_else(refused)?;
    for table in tables {
        let columns = table.get("columns").and_then(Value::as_array).ok_or_else(refused)?;
        for column in columns {
            let source_type = column.get("source_type").and_then(Value::as_str).unwrap_or("");
            let oid = column.get("oid").and_then(Value::as_i64);
            let typmod = column.get("typmod").and_then(Value::as_i64);
            if (source_type == "jsonb" || oid == Some(3802))
                && (source_type != "jsonb" || oid != Some(3802) || typmod != Some(-1))
            {
                return Err(refused());
            }
        }
    }
    Ok(())
}

/// Refuse an engine retag or a client admission field before a process starts.
pub fn gate_request(request: &Value) -> Result<(), PgBuilderError> {
    let object = request
        .as_object()
        .ok_or_else(|| PgBuilderError::new("input.schema", "an input file is not JSON"))?;
    if object.contains_key("engine") || object.contains_key("source_engine") {
        return Err(PgBuilderError::new("catalog.refused", "the catalog command was refused"));
    }
    if CLIENT_AUTHORITY.iter().any(|key| object.contains_key(*key)) {
        return Err(PgBuilderError::new("catalog.refused", "the catalog command was refused"));
    }
    if request.get("protocol").and_then(Value::as_str) != Some(REQUEST_PROTOCOL) {
        return Err(PgBuilderError::new("input.schema", "the catalog protocol is not the native sealed catalog"));
    }
    Ok(())
}

fn native_engine(engine: &str) -> bool {
    matches!(
        engine,
        "oracle"
            | "clickhouse"
            | "opensearch"
            | "doris"
            | "mongodb"
            | "mongo"
            | "starrocks"
            | "mariadb"
    )
}

fn spawn_catalog(
    pipeline: &Path,
    request: &[u8],
    password: &str,
    stderr_path: Option<&Path>,
) -> Result<Vec<u8>, PgBuilderError> {
    let mut command = Command::new(pipeline);
    command
        .arg("catalog-import-native")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in [
        "OO_NATIVE_ADMISSION_BIN",
        "OO_NATIVE_BINDING_REGISTRY",
        "OO_NATIVE_BINDING_REGISTRY_SHA256",
    ] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|_| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    {
        let mut input = child.stdin.take().ok_or_else(|| {
            PgBuilderError::new("catalog.spawn", "the catalog command could not start")
        })?;
        input
            .write_all(request)
            .map_err(|_| PgBuilderError::new("catalog.spawn", "the catalog command could not start"))?;
    }
    let stdout = child.stdout.take().ok_or_else(|| {
        PgBuilderError::new("catalog.spawn", "the catalog command could not start")
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        PgBuilderError::new("catalog.spawn", "the catalog command could not start")
    })?;
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
    Ok(out)
}

fn read_file(path: &Path, max: u64) -> Result<Vec<u8>, PgBuilderError> {
    let meta = fs::metadata(path)
        .map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > max {
        return Err(PgBuilderError::new("input.read", "an input file could not be read"));
    }
    fs::read(path).map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(0);
    &bytes[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn engine_retag_does_not_spawn() {
        let mysql = json!({
            "protocol": REQUEST_PROTOCOL,
            "engine": "mysql"
        });
        assert!(gate_request(&mysql).is_err());
        let source = json!({
            "protocol": REQUEST_PROTOCOL,
            "source_engine": "postgresql"
        });
        assert!(gate_request(&source).is_err());
        let admitted = json!({
            "protocol": REQUEST_PROTOCOL,
            "admitted": true
        });
        assert!(gate_request(&admitted).is_err());
        let mysql_protocol = json!({"protocol": "oo.pipeline.catalog-import/v1"});
        assert!(gate_request(&mysql_protocol).is_err());
        let pg_protocol = json!({"protocol": "oo.pipeline.catalog-import-pg/v1"});
        assert!(gate_request(&pg_protocol).is_err());
    }

    #[test]
    fn output_engine_cannot_impersonate_mysql_or_postgresql() {
        assert!(!native_engine("mysql"));
        assert!(!native_engine("postgresql"));
        assert!(!native_engine("postgres"));
        assert!(!native_engine("snowflake"));
        assert!(native_engine("starrocks"));
        assert!(native_engine("doris"));
        assert!(native_engine("clickhouse"));
        assert!(native_engine("oracle"));
        assert!(native_engine("mongodb"));
        assert!(native_engine("mariadb"));
        assert!(native_engine("opensearch"));
        assert!(!native_engine("sqlserver"));
    }

    #[test]
    fn legal_document_column_is_jsonb_3802_and_json_114_stays() {
        let json_only = json!({
            "tables": [{
                "columns": [{
                    "logical_name": "payload",
                    "oid": 114,
                    "source_type": "json",
                    "typmod": -1
                }]
            }]
        });
        assert!(legal_document_columns(&json_only).is_ok());
        let document = json!({
            "tables": [{
                "columns": [{
                    "logical_name": "doc",
                    "oid": 3802,
                    "source_type": "jsonb",
                    "typmod": -1
                }]
            }]
        });
        assert!(legal_document_columns(&document).is_ok());
        let typmod = json!({
            "tables": [{
                "columns": [{
                    "logical_name": "doc",
                    "oid": 3802,
                    "source_type": "jsonb",
                    "typmod": 0
                }]
            }]
        });
        assert!(legal_document_columns(&typmod).is_err());
        let retag = json!({
            "tables": [{
                "columns": [{
                    "logical_name": "doc",
                    "oid": 3802,
                    "source_type": "utf8",
                    "typmod": -1
                }]
            }]
        });
        assert!(legal_document_columns(&retag).is_err());
        let labeled = json!({
            "tables": [{
                "columns": [{
                    "logical_name": "doc",
                    "oid": 114,
                    "source_type": "jsonb",
                    "typmod": -1
                }]
            }]
        });
        assert!(legal_document_columns(&labeled).is_err());
        assert!(legal_document_columns(&json!({})).is_err());
    }
}
