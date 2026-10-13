//! Config and connector checks. This command does not open a database.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::config::{load_config, ConfigError};
use crate::source::{connector_decision, regular_file};

#[derive(Clone, Debug)]
pub struct DoctorReport {
    pub status: &'static str,
    pub database_type: String,
    pub connector: &'static str,
    pub supported: bool,
}

pub fn ready_bins(config: &crate::config::Config) -> Result<(), ConfigError> {
    if !config.pi_executable.is_file() || !config.pi_runtime.is_file() {
        return Err(ConfigError::new("pi.absent", "the pi runtime is absent"));
    }
    if !config.query_pg.is_file() {
        return Err(ConfigError::new("query_pg.absent", "query-pg is absent"));
    }
    if !config.oo_bin.is_file() {
        return Err(ConfigError::new("oo.absent", "the oo CLI is absent"));
    }
    Ok(())
}

pub fn doctor(path: &Path) -> Result<DoctorReport, ConfigError> {
    let config = load_config(path)?;
    let decision = connector_decision(&config.database_type);
    if !decision.supported {
        return Err(ConfigError {
            code: "unsupported",
            message: "the source connector does not support this database type",
        });
    }
    if config.tls != "require" {
        return Err(ConfigError::new("config.tls", "tls must be require"));
    }
    if !config.secret_ref.starts_with("secret://") {
        return Err(ConfigError::new("config.secret", "the secret ref is not a secret:// reference"));
    }
    regular_file(&config.password_file, 4096).map_err(|_| ConfigError::new("config.secret", "the password file is not a regular file"))?;
    regular_file(&config.ca_file, 1_048_576).map_err(|_| ConfigError::new("config.tls", "the CA file is not a regular file"))?;
    regular_file(&config.bearer_file, 512).map_err(|_| ConfigError::new("config.secret", "the bearer file is not a regular file"))?;
    let config_bytes = fs::read(path).map_err(|_| ConfigError::new("config.read", "the config file could not be read"))?;
    let bearer = fs::read(&config.bearer_file).map_err(|_| ConfigError::new("config.secret", "the bearer file is not a regular file"))?;
    if !bearer.is_empty() && config_bytes.windows(bearer.len()).any(|window| window == bearer.as_slice()) {
        return Err(ConfigError::new("config.secret", "the bearer bytes appear in the config"));
    }
    Ok(DoctorReport {
        status: "config_ok",
        database_type: config.database_type,
        connector: decision.protocol,
        supported: true,
    })
}

pub fn report_json(report: &DoctorReport) -> Value {
    json!({
        "protocol": crate::PROTOCOL,
        "status": report.status,
        "database_type": report.database_type,
        "connector": report.connector,
        "supported": report.supported,
        "publication": "not_published",
        "llm_review": "unreviewed",
        "parent_acceptance": false
    })
}
