use serde_json::{Map, Value};

use crate::error::MirrorError;

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

pub fn parse_object(text: &str) -> Result<Map<String, Value>, MirrorError> {
    let value = oo_core::parse_json_value(text, "oo.pipeline.local-pg-mirror/v1")
        .map_err(|_| MirrorError::new("config", "json"))?;
    reject_keys(&value)?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(MirrorError::new("config", "json")),
    }
}

pub fn reject_keys(value: &Value) -> Result<(), MirrorError> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if FORBIDDEN.contains(&key.as_str()) {
                    return Err(MirrorError::new("config", "forbidden_key"));
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
        _ => Ok(()),
    }
}

pub fn exact_keys(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), MirrorError> {
    if map.len() != allowed.len() || allowed.iter().any(|key| !map.contains_key(*key)) {
        return Err(MirrorError::new("config", "unknown_field"));
    }
    Ok(())
}

pub fn take_str<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, MirrorError> {
    match map.get(key) {
        Some(Value::String(value)) => Ok(value.as_str()),
        _ => Err(MirrorError::new("config", "json")),
    }
}

pub fn take_u64(map: &Map<String, Value>, key: &str) -> Result<u64, MirrorError> {
    match map.get(key).and_then(Value::as_u64) {
        Some(value) => Ok(value),
        None => Err(MirrorError::new("config", "json")),
    }
}

pub fn take_i64(map: &Map<String, Value>, key: &str) -> Result<i64, MirrorError> {
    match map.get(key).and_then(Value::as_i64) {
        Some(value) => Ok(value),
        None => Err(MirrorError::new("config", "json")),
    }
}

pub fn take_bool(map: &Map<String, Value>, key: &str) -> Result<bool, MirrorError> {
    match map.get(key).and_then(Value::as_bool) {
        Some(value) => Ok(value),
        None => Err(MirrorError::new("config", "json")),
    }
}

pub fn json_string(value: &str) -> Result<String, MirrorError> {
    serde_json::to_string(&Value::String(value.to_string())).map_err(|_| MirrorError::new("config", "json"))
}
