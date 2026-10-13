//! Frozen `oo.source-control-api/source-register/v1` shape.
//! Host allowlists and secret files stay in source-control-api. This module does not resolve them.

use crate::json::Json;

pub const REGISTER_CONTRACT: &str = "oo.source-control-api/source-register/v1";
pub const NOT_CLAIMED_FORMATS: &[&str] = &["parquet", "excel", "sqlite", "dump"];
pub const FILE_FORMATS: &[&str] = &["csv", "tsv", "jsonl"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub project_id: String,
    pub source_ref: String,
    pub kind: String,
    pub format: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisterError {
    pub code: &'static str,
    pub detail: &'static str,
}

const CLOSED: &[&str] = &[
    "strategy",
    "paths",
    "worker",
    "target",
    "password",
    "password_file",
    "binaries",
    "argv",
];

pub fn schema_drift(schema: &Json) -> bool {
    let Some(properties) = schema.get("properties") else {
        return true;
    };
    let Some(kind) = properties.get("kind") else {
        return true;
    };
    let Some(enums) = kind.get("enum").and_then(Json::array) else {
        return true;
    };
    let names: Vec<&str> = enums.iter().filter_map(Json::string).collect();
    if names != ["pg_local", "file_local"] {
        return true;
    }
    let Some(required) = schema.get("required").and_then(Json::array) else {
        return true;
    };
    let required: Vec<&str> = required.iter().filter_map(Json::string).collect();
    for key in ["project_id", "identity_revision", "source_ref", "kind", "idempotency_key"] {
        if !required.contains(&key) {
            return true;
        }
    }
    for key in ["connection", "secret_ref", "tables"] {
        if properties.get(key).is_none() {
            return true;
        }
    }
    false
}

pub fn parse_registration(value: &Json) -> Result<Registration, RegisterError> {
    let map = value.object().ok_or(RegisterError { code: "malformed", detail: "json" })?;
    for key in CLOSED {
        if map.iter().any(|(name, _)| name == key) {
            return Err(RegisterError { code: "closed_input", detail: key });
        }
    }
    const ALLOWED: &[&str] = &[
        "project_id",
        "identity_revision",
        "source_ref",
        "kind",
        "idempotency_key",
        "secret_ref",
        "connection",
        "tables",
    ];
    if map.iter().any(|(key, _)| !ALLOWED.contains(&key.as_str())) {
        return Err(RegisterError { code: "malformed", detail: "json" });
    }
    let project_id = require_ident(value, "project_id")?;
    let source_ref = require_ident(value, "source_ref")?;
    let kind = require_str(value, "kind")?;
    let idempotency = require_str(value, "idempotency_key")?;
    if !idempotency_ok(idempotency) {
        return Err(RegisterError { code: "malformed", detail: "idempotency" });
    }
    let revision = value.get("identity_revision").and_then(Json::int).ok_or(RegisterError { code: "malformed", detail: "json" })?;
    if revision < 1 {
        return Err(RegisterError { code: "malformed", detail: "json" });
    }
    if kind == "file_local" {
        if value.get("connection").is_some() || value.get("secret_ref").is_some() || value.get("tables").is_some() {
            return Err(RegisterError { code: "closed_input", detail: "kind" });
        }
        return Ok(Registration { project_id, source_ref, kind: kind.to_string(), format: None });
    }
    if kind != "pg_local" {
        return Err(RegisterError { code: "malformed", detail: "kind" });
    }
    let secret_ref = require_str(value, "secret_ref").map_err(|_| RegisterError { code: "malformed", detail: "secret_ref" })?;
    if !secret_ref_ok(secret_ref) {
        return Err(RegisterError { code: "malformed", detail: "secret_ref" });
    }
    parse_connection(value.get("connection").ok_or(RegisterError { code: "malformed", detail: "connection" })?)?;
    parse_tables(value.get("tables").ok_or(RegisterError { code: "malformed", detail: "tables" })?)?;
    Ok(Registration { project_id, source_ref, kind: kind.to_string(), format: None })
}

fn parse_connection(value: &Json) -> Result<(), RegisterError> {
    let map = value.object().ok_or(RegisterError { code: "malformed", detail: "connection" })?;
    if map.len() != 4 || ["host", "port", "database", "username"].iter().any(|key| value.get(key).is_none()) {
        return Err(RegisterError { code: "malformed", detail: "connection" });
    }
    let host = require_str(value, "host")?;
    if !host_ok(host) {
        return Err(RegisterError { code: "malformed", detail: "host" });
    }
    let port = value.get("port").and_then(Json::int).ok_or(RegisterError { code: "malformed", detail: "port" })?;
    if !(1..=65535).contains(&port) {
        return Err(RegisterError { code: "malformed", detail: "port" });
    }
    let database = require_str(value, "database")?;
    let username = require_str(value, "username")?;
    if !ident_ok(database) || !ident_ok(username) {
        return Err(RegisterError { code: "malformed", detail: "connection" });
    }
    Ok(())
}

fn parse_tables(value: &Json) -> Result<(), RegisterError> {
    let items = value.array().ok_or(RegisterError { code: "malformed", detail: "tables" })?;
    if items.is_empty() || items.len() > 32 {
        return Err(RegisterError { code: "malformed", detail: "tables" });
    }
    for item in items {
        let map = item.object().ok_or(RegisterError { code: "malformed", detail: "tables" })?;
        if map.len() != 2 || item.get("schema").is_none() || item.get("table").is_none() {
            return Err(RegisterError { code: "malformed", detail: "tables" });
        }
        let schema = item.get("schema").and_then(Json::string).ok_or(RegisterError { code: "malformed", detail: "tables" })?;
        let table = item.get("table").and_then(Json::string).ok_or(RegisterError { code: "malformed", detail: "tables" })?;
        if !ident_ok(schema) || !ident_ok(table) {
            return Err(RegisterError { code: "malformed", detail: "tables" });
        }
    }
    Ok(())
}

fn require_str<'a>(value: &'a Json, key: &str) -> Result<&'a str, RegisterError> {
    value.get(key).and_then(Json::string).ok_or(RegisterError { code: "malformed", detail: "json" })
}

fn require_ident(value: &Json, key: &str) -> Result<String, RegisterError> {
    let text = require_str(value, key)?;
    if ident_ok(text) { Ok(text.to_string()) } else { Err(RegisterError { code: "malformed", detail: "json" }) }
}

fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn host_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=253).contains(&bytes.len())
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'-'))
}

fn secret_ref_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("secret://") else {
        return false;
    };
    (1..=128).contains(&rest.len())
        && rest.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'/' | b'-'))
}

fn idempotency_ok(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
