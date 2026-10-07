//! Durable cache of validated drafts. One namespace. Atomic rename.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::canonical::{canonical_json, canonical_sha256};
use crate::error::PgBuilderError;

pub fn cache_path(dir: &Path, namespace: &str, key: &str) -> PathBuf {
    dir.join(namespace).join(format!("{key}.json"))
}

pub fn read_validated(dir: &Path, key: &str, evidence_hash: &str) -> Result<Option<Value>, PgBuilderError> {
    let path = cache_path(dir, "pg-builder", key);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|_| PgBuilderError::new("cache.read", "the cache entry could not be read"))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| PgBuilderError::new("cache.read", "the cache entry is not JSON"))?;
    let stored_evidence = value.get("evidence_hash").and_then(Value::as_str).unwrap_or("");
    let stored_key = value.get("input_sha256").and_then(Value::as_str).unwrap_or("");
    if stored_evidence != evidence_hash || stored_key != key {
        return Ok(None);
    }
    if value.get("outcome").and_then(Value::as_str) != Some("validated") {
        return Ok(None);
    }
    Ok(Some(value))
}

pub fn store_validated(dir: &Path, key: &str, value: &Value) -> Result<(), PgBuilderError> {
    if value.get("outcome").and_then(Value::as_str) != Some("validated") {
        return Err(PgBuilderError::new("cache.refused", "only a validated draft is stored"));
    }
    let name_space = dir.join("pg-builder");
    fs::create_dir_all(&name_space)
        .map_err(|_| PgBuilderError::new("cache.write", "the cache directory could not be created"))?;
    let path = cache_path(dir, "pg-builder", key);
    let pid = std::process::id();
    let temp = name_space.join(format!(".{key}.{pid}.tmp"));
    let bytes = canonical_json(value);
    if canonical_sha256(value).is_empty() {
        return Err(PgBuilderError::new("cache.write", "the cache entry could not be written"));
    }
    {
        let mut file = File::create(&temp)
            .map_err(|_| PgBuilderError::new("cache.write", "the cache entry could not be written"))?;
        file.write_all(bytes.as_bytes())
            .map_err(|_| PgBuilderError::new("cache.write", "the cache entry could not be written"))?;
        file.sync_all()
            .map_err(|_| PgBuilderError::new("cache.write", "the cache entry could not be written"))?;
    }
    fs::rename(&temp, &path).map_err(|_| PgBuilderError::new("cache.write", "the cache entry could not be written"))?;
    if let Ok(dir_file) = File::open(&name_space) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}
