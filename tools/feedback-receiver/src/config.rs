//! `--config` points at files. Secrets stay in those files.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::RecvError;
use crate::review::{project_ok, role_ok};

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: String,
    pub ca_file: PathBuf,
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    pub registry_file: PathBuf,
    pub pg_host: String,
    pub pg_port: u16,
    pub pg_db: String,
    pub pg_user: String,
    pub pg_password_file: PathBuf,
    pub pg_ca_file: PathBuf,
    pub catalog_file: PathBuf,
    pub relationships_file: PathBuf,
    pub profile_file: PathBuf,
    pub source_id: String,
    pub role: String,
}

const KEYS: &[&str] = &[
    "listen",
    "ca_file",
    "cert_file",
    "key_file",
    "registry_file",
    "pg_host",
    "pg_port",
    "pg_db",
    "pg_user",
    "pg_password_file",
    "pg_ca_file",
    "catalog_file",
    "relationships_file",
    "profile_file",
    "source_id",
    "role",
];

pub fn load_config(path: &Path) -> Result<Config, RecvError> {
    private_mode(path)?;
    let text = fs::read_to_string(path).map_err(RecvError::Io)?;
    if text.contains("PRIVATE KEY") || text.contains("BEGIN ") {
        return Err(RecvError::Bound);
    }
    let map = pairs(&text)?;
    if map.len() != KEYS.len() || KEYS.iter().any(|key| !map.contains_key(*key)) {
        return Err(RecvError::Bound);
    }
    let port: u16 = map["pg_port"].parse().map_err(|_| RecvError::Bound)?;
    let source_id = map["source_id"].clone();
    let role = map["role"].clone();
    if !project_ok(&source_id) || !role_ok(&role) || map["pg_user"] != "feedback_receiver" {
        return Err(RecvError::Bound);
    }
    let config = Config {
        listen: map["listen"].clone(),
        ca_file: PathBuf::from(&map["ca_file"]),
        cert_file: PathBuf::from(&map["cert_file"]),
        key_file: PathBuf::from(&map["key_file"]),
        registry_file: PathBuf::from(&map["registry_file"]),
        pg_host: map["pg_host"].clone(),
        pg_port: port,
        pg_db: map["pg_db"].clone(),
        pg_user: map["pg_user"].clone(),
        pg_password_file: PathBuf::from(&map["pg_password_file"]),
        pg_ca_file: PathBuf::from(&map["pg_ca_file"]),
        catalog_file: PathBuf::from(&map["catalog_file"]),
        relationships_file: PathBuf::from(&map["relationships_file"]),
        profile_file: PathBuf::from(&map["profile_file"]),
        source_id,
        role,
    };
    private_mode(&config.key_file)?;
    private_mode(&config.pg_password_file)?;
    Ok(config)
}

pub fn private_mode(path: &Path) -> Result<(), RecvError> {
    let mode = fs::metadata(path).map_err(RecvError::Io)?.permissions().mode() & 0o777;
    if mode == 0o600 || mode == 0o400 {
        Ok(())
    } else {
        Err(RecvError::Bound)
    }
}

fn pairs(text: &str) -> Result<BTreeMap<String, String>, RecvError> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(RecvError::Bound);
        };
        if key.is_empty() || value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(RecvError::Bound);
        }
        if map.insert(key.to_string(), value.to_string()).is_some() {
            return Err(RecvError::Bound);
        }
    }
    Ok(map)
}
