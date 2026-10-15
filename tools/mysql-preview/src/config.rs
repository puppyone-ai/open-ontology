//! Private preview config. Passwords and CA bodies are files, never keys.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use zeroize::Zeroizing;

use crate::error::PreviewError;

const FORBIDDEN: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "authorization",
    "credential",
    "private_key",
    "ca_pem",
    "ca_body",
    "api_key",
    "token",
];

#[derive(Clone, Debug)]
pub struct PreviewConfig {
    pub stop: Stop,
    pub source_id: String,
    pub shard: String,
    pub owner: String,
    pub generation: u64,
    pub boolean_tinyint1: bool,
    pub mysql_host: String,
    pub mysql_port: u16,
    pub mysql_database: String,
    pub mysql_user: String,
    pub mysql_password_file: PathBuf,
    pub mysql_ca_file: PathBuf,
    pub target_host: String,
    pub target_port: u16,
    pub target_database: String,
    pub target_admin_user: String,
    pub target_admin_password_file: PathBuf,
    pub target_runtime_user: String,
    pub target_runtime_password_file: PathBuf,
    pub target_ca_file: PathBuf,
    pub tables: Vec<SelectedTable>,
    pub connect_timeout_ms: u64,
    pub statement_timeout_ms: u64,
    pub lock_timeout_ms: u64,
    pub batch_rows: u64,
    pub snapshot_deadline_ms: u64,
    pub max_rows: u64,
    pub max_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Seal,
    Access,
    CancelBeforeBatch,
    FailBeforeSeal,
    /// Read and note table index 0, then leave the run open.
    PartialAfterFirst,
}

#[derive(Clone, Debug)]
pub struct SelectedTable {
    pub schema: String,
    pub name: String,
    /// Empty keeps every mapped column. A non-empty list is the requested subset, in that order.
    pub columns: Vec<String>,
}

pub fn parse_config(text: &str) -> Result<PreviewConfig, PreviewError> {
    let value: Value =
        serde_json::from_str(text).map_err(|_| PreviewError::new("config", "json"))?;
    reject_keys(&value)?;
    let map = value
        .as_object()
        .ok_or_else(|| PreviewError::new("config", "json"))?;
    exact_keys(
        map,
        &[
            "version",
            "stop",
            "source_id",
            "shard",
            "owner",
            "generation",
            "boolean_tinyint1",
            "mysql",
            "target",
            "tables",
            "budgets",
        ],
    )?;
    if take_str(map, "version")? != "oo.mysql-preview/v1" {
        return Err(PreviewError::new("config", "version"));
    }
    let stop = match take_str(map, "stop")? {
        "seal" => Stop::Seal,
        "access" => Stop::Access,
        "cancel_before_batch" => Stop::CancelBeforeBatch,
        "fail_before_seal" => Stop::FailBeforeSeal,
        "partial_after_first" => Stop::PartialAfterFirst,
        _ => return Err(PreviewError::new("config", "stop")),
    };
    let source_id = take_str(map, "source_id")?.to_string();
    if !source_ok(&source_id) {
        return Err(PreviewError::new("config", "source_id"));
    }
    let shard = take_str(map, "shard")?.to_string();
    if !shard_ok(&shard) {
        return Err(PreviewError::new("config", "shard"));
    }
    let owner = take_str(map, "owner")?.to_string();
    if !uuid_ok(&owner) {
        return Err(PreviewError::new("config", "owner"));
    }
    let generation = take_u64(map, "generation")?;
    if generation < 1 {
        return Err(PreviewError::new("budget", "generation"));
    }
    let boolean_tinyint1 = take_bool(map, "boolean_tinyint1")?;
    let mysql = object(map, "mysql")?;
    exact_keys(
        mysql,
        &[
            "host",
            "port",
            "database",
            "user",
            "password_file",
            "ca_file",
        ],
    )?;
    let target = object(map, "target")?;
    exact_keys(
        target,
        &[
            "host",
            "port",
            "database",
            "admin_user",
            "admin_password_file",
            "runtime_user",
            "runtime_password_file",
            "ca_file",
        ],
    )?;
    let budgets = object(map, "budgets")?;
    exact_keys(
        budgets,
        &[
            "connect_timeout_ms",
            "statement_timeout_ms",
            "lock_timeout_ms",
            "batch_rows",
            "snapshot_deadline_ms",
            "max_rows",
            "max_bytes",
        ],
    )?;
    let tables_value = map
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PreviewError::new("config", "tables"))?;
    if tables_value.is_empty() {
        return Err(PreviewError::new("config", "tables"));
    }
    let mut tables = Vec::new();
    for item in tables_value {
        let table = item
            .as_object()
            .ok_or_else(|| PreviewError::new("config", "tables"))?;
        let columns = if table.contains_key("columns") {
            exact_keys(table, &["columns", "schema", "table"])?;
            let list = table
                .get("columns")
                .and_then(Value::as_array)
                .ok_or_else(|| PreviewError::new("config", "tables"))?;
            let mut names = Vec::new();
            for column in list {
                let name = column
                    .as_str()
                    .ok_or_else(|| PreviewError::new("config", "tables"))?;
                names.push(ident(name, "tables")?);
            }
            names
        } else {
            exact_keys(table, &["schema", "table"])?;
            Vec::new()
        };
        tables.push(SelectedTable {
            schema: take_str(table, "schema")?.to_string(),
            name: take_str(table, "table")?.to_string(),
            columns,
        });
    }
    let connect_timeout_ms = bounded(budgets, "connect_timeout_ms", 1, 30_000)?;
    let statement_timeout_ms = bounded(budgets, "statement_timeout_ms", 1, 30_000)?;
    let lock_timeout_ms = bounded(budgets, "lock_timeout_ms", 1, 1_000)?;
    let batch_rows = bounded(budgets, "batch_rows", 1, 1_000)?;
    let snapshot_deadline_ms = bounded(budgets, "snapshot_deadline_ms", 1, 300_000)?;
    let max_rows = bounded(budgets, "max_rows", 1, 100_000)?;
    let max_bytes = bounded(budgets, "max_bytes", 1, 33_554_432)?;
    Ok(PreviewConfig {
        stop,
        source_id,
        shard,
        owner,
        generation,
        boolean_tinyint1,
        mysql_host: host(take_str(mysql, "host")?)?,
        mysql_port: port(take_u64(mysql, "port")?)?,
        mysql_database: ident(take_str(mysql, "database")?, "database")?,
        mysql_user: ident(take_str(mysql, "user")?, "user")?,
        mysql_password_file: secret_path(take_str(mysql, "password_file")?)?,
        mysql_ca_file: secret_path(take_str(mysql, "ca_file")?)?,
        target_host: host(take_str(target, "host")?)?,
        target_port: port(take_u64(target, "port")?)?,
        target_database: pg_ident(take_str(target, "database")?, "database")?,
        target_admin_user: pg_ident(take_str(target, "admin_user")?, "user")?,
        target_admin_password_file: secret_path(take_str(target, "admin_password_file")?)?,
        target_runtime_user: pg_ident(take_str(target, "runtime_user")?, "user")?,
        target_runtime_password_file: secret_path(take_str(target, "runtime_password_file")?)?,
        target_ca_file: secret_path(take_str(target, "ca_file")?)?,
        tables,
        connect_timeout_ms,
        statement_timeout_ms,
        lock_timeout_ms,
        batch_rows,
        snapshot_deadline_ms,
        max_rows,
        max_bytes,
    })
}

pub fn read_secret(path: &Path) -> Result<Zeroizing<String>, PreviewError> {
    let bytes = read_regular(path, 4096)?;
    let trimmed = if bytes.ends_with(b"\r\n") {
        &bytes[..bytes.len() - 2]
    } else if bytes.ends_with(b"\n") {
        &bytes[..bytes.len() - 1]
    } else {
        bytes.as_slice()
    };
    if trimmed.is_empty() || trimmed.len() > 1024 || trimmed.contains(&0) {
        return Err(PreviewError::new("config", "password_file"));
    }
    let text = String::from_utf8(trimmed.to_vec())
        .map_err(|_| PreviewError::new("config", "password_file"))?;
    Ok(Zeroizing::new(text))
}

pub fn read_ca(path: &Path) -> Result<Vec<u8>, PreviewError> {
    let bytes = read_regular(path, 1_048_576)?;
    if bytes.is_empty() || bytes.windows(11).any(|window| window == b"PRIVATE KEY") {
        return Err(PreviewError::new("tls", "ca_file"));
    }
    Ok(bytes)
}

fn read_regular(path: &Path, max: usize) -> Result<Vec<u8>, PreviewError> {
    let meta = fs::symlink_metadata(path).map_err(|_| PreviewError::new("config", "path"))?;
    if !meta.file_type().is_file() {
        return Err(PreviewError::new("config", "path"));
    }
    if meta.len() > u64::try_from(max).unwrap_or(u64::MAX) {
        return Err(PreviewError::new("config", "path"));
    }
    let file = File::open(path).map_err(|_| PreviewError::new("config", "path"))?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(max).unwrap_or(0) + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PreviewError::new("config", "path"))?;
    if bytes.len() > max {
        return Err(PreviewError::new("config", "path"));
    }
    Ok(bytes)
}

fn reject_keys(value: &Value) -> Result<(), PreviewError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let folded = key.to_ascii_lowercase();
                if FORBIDDEN.iter().any(|denied| folded == *denied) {
                    return Err(PreviewError::new("config", "secret_key"));
                }
                reject_keys(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_keys(item)?;
            }
            Ok(())
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

fn exact_keys(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), PreviewError> {
    if map.len() != allowed.len() || allowed.iter().any(|key| !map.contains_key(*key)) {
        return Err(PreviewError::new("config", "keys"));
    }
    Ok(())
}

fn object<'a>(
    map: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Map<String, Value>, PreviewError> {
    map.get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| PreviewError::new("config", "keys"))
}

fn take_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, PreviewError> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| PreviewError::new("config", "keys"))
}

fn take_u64(map: &Map<String, Value>, key: &str) -> Result<u64, PreviewError> {
    map.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| PreviewError::new("config", "keys"))
}

fn take_bool(map: &Map<String, Value>, key: &str) -> Result<bool, PreviewError> {
    map.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| PreviewError::new("config", "keys"))
}

fn bounded(map: &Map<String, Value>, key: &str, low: u64, high: u64) -> Result<u64, PreviewError> {
    let value = take_u64(map, key)?;
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(PreviewError::new("budget", "range"))
    }
}

fn source_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn shard_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(*byte, b'_' | b'.' | b':' | b'-')
        })
}

fn uuid_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => *byte == b'-',
        _ => byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase(),
    })
}

fn host(value: &str) -> Result<String, PreviewError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 253
        || bytes.first() == Some(&b'-')
        || bytes.last() == Some(&b'-')
        || value.contains("..")
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'.' || *byte == b'-')
    {
        return Err(PreviewError::new("config", "host"));
    }
    Ok(value.to_string())
}

fn port(value: u64) -> Result<u16, PreviewError> {
    u16::try_from(value)
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| PreviewError::new("config", "port"))
}

fn ident(value: &str, fact: &'static str) -> Result<String, PreviewError> {
    if mysql_ident(value) {
        Ok(value.to_string())
    } else {
        Err(PreviewError::new("config", fact))
    }
}

fn pg_ident(value: &str, fact: &'static str) -> Result<String, PreviewError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 63
        || !bytes[0].is_ascii_lowercase()
        || !bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return Err(PreviewError::new("config", fact));
    }
    Ok(value.to_string())
}

pub fn mysql_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        && !bytes[0].is_ascii_digit()
}

fn secret_path(value: &str) -> Result<PathBuf, PreviewError> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(PreviewError::new("config", "path"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_key_is_refused() {
        let text = r#"{"password":"nope"}"#;
        let error = parse_config(text).expect_err("secret");
        assert_eq!(error.fact, "secret_key");
    }

    #[test]
    fn empty_tables_refuse_before_shape() {
        let text = r#"{
            "version":"oo.mysql-preview/v1",
            "stop":"seal",
            "source_id":"mysqlpreview",
            "shard":"mysqlpreview",
            "owner":"11111111-1111-4111-8111-111111111111",
            "generation":1,
            "boolean_tinyint1":false,
            "mysql":{"host":"db","port":3306,"database":"erp","user":"oo_reader","password_file":"/secrets/a","ca_file":"/secrets/ca"},
            "target":{"host":"pg","port":5432,"database":"oo_preview","admin_user":"postgres","admin_password_file":"/secrets/b","runtime_user":"preview_runtime","runtime_password_file":"/secrets/c","ca_file":"/secrets/ca"},
            "tables":[],
            "budgets":{"connect_timeout_ms":5000,"statement_timeout_ms":30000,"lock_timeout_ms":1000,"batch_rows":100,"snapshot_deadline_ms":30000,"max_rows":1000,"max_bytes":1048576}
        }"#;
        let error = parse_config(text).expect_err("tables");
        assert_eq!(error.fact, "tables");
    }
}
