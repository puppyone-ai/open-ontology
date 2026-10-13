//! Operator config. Secret values stay in files. The JSON document has paths only.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const MAX_BYTES: usize = 65_536;

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: String,
    pub development_preview: bool,
    pub publication: String,
    pub llm_review: String,
    pub runtime_acknowledgement: String,
    pub database_type: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub tls: String,
    pub secret_ref: String,
    pub password_file: PathBuf,
    pub ca_file: PathBuf,
    pub tables: Vec<(String, String)>,
    pub model_id: String,
    pub llm_origin: String,
    pub llm_path: String,
    pub bearer_file: PathBuf,
    pub deadline_ms: u64,
    pub catalog_bin: PathBuf,
    pub query_pg: PathBuf,
    pub oo_bin: PathBuf,
    pub gateway_token_file: PathBuf,
    pub policy_file: PathBuf,
    pub pi_executable: PathBuf,
    pub pi_executable_sha256: String,
    pub pi_runtime: PathBuf,
    pub pi_runtime_sha256: String,
    pub pi_provider: String,
    pub pi_model: String,
    pub work: PathBuf,
    pub knowledge: PathBuf,
    pub operator_label: String,
    pub operator_at: String,
}

/// Operator-development Pi child identity. Production runsc is unchanged.
pub const OPERATOR_PI_UID: u32 = 65532;
/// Operator-development Pi child identity. Production runsc is unchanged.
pub const OPERATOR_PI_GID: u32 = 65532;

impl Config {
    /// Uid of the operator-development Pi child. The only accepted value is 65532.
    pub fn pi_uid(&self) -> u32 {
        debug_assert_eq!(self.mode, "operator-development");
        OPERATOR_PI_UID
    }

    /// Gid of the operator-development Pi child. The only accepted value is 65532.
    pub fn pi_gid(&self) -> u32 {
        debug_assert_eq!(self.mode, "operator-development");
        OPERATOR_PI_GID
    }
}

#[derive(Debug)]
pub struct ConfigError {
    pub code: &'static str,
    pub message: &'static str,
}

impl ConfigError {
    pub(crate) fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

pub fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let bytes = fs::read(path).map_err(|_| ConfigError::new("config.read", "the config file could not be read"))?;
    if bytes.len() > MAX_BYTES {
        return Err(ConfigError::new("config.budget", "the config file is too large"));
    }
    let text = String::from_utf8(bytes).map_err(|_| ConfigError::new("config.schema", "the config is not UTF-8"))?;
    let value: Value = serde_json::from_str(&text).map_err(|_| ConfigError::new("config.schema", "the config is not one JSON object"))?;
    let map = object(&value)?;
    reject_secret_keys(map)?;
    require(map, &["database", "development_preview", "gateway", "llm", "llm_review", "mode", "operator", "paths", "pi", "protocol", "publication", "runtime_acknowledgement", "tables"])?;
    eq(map, "protocol", "oo.minimal-product-loop.config/v1")?;
    eq(map, "mode", "operator-development")?;
    eq(map, "publication", "not_published")?;
    eq(map, "llm_review", "unreviewed")?;
    eq(map, "runtime_acknowledgement", crate::DEVELOPMENT_ACK)?;
    if map.get("development_preview") != Some(&Value::Bool(true)) {
        return Err(ConfigError::new("config.preview", "development preview must be explicit"));
    }
    let database = object(map.get("database").unwrap_or(&Value::Null))?;
    require(database, &["ca_file", "host", "name", "password_file", "port", "secret_ref", "tls", "type", "username"])?;
    let tables = tables(map.get("tables").unwrap_or(&Value::Null))?;
    let llm = object(map.get("llm").unwrap_or(&Value::Null))?;
    require(llm, &["bearer_file", "deadline_ms", "model_id", "origin", "path"])?;
    eq(llm, "model_id", "grok-4.7")?;
    eq(llm, "path", "/v1/chat/completions")?;
    let gateway = object(map.get("gateway").unwrap_or(&Value::Null))?;
    require(gateway, &["oo", "policy_file", "query_pg", "token_file"])?;
    let pi = object(map.get("pi").unwrap_or(&Value::Null))?;
    require_pi(pi)?;
    let paths = object(map.get("paths").unwrap_or(&Value::Null))?;
    require(paths, &["catalog_bin", "knowledge", "work"])?;
    let operator = object(map.get("operator").unwrap_or(&Value::Null))?;
    require(operator, &["at", "label"])?;
    let port = number(database, "port")?;
    let port = u16::try_from(port).map_err(|_| ConfigError::new("config.database", "the port is not allowed"))?;
    if port == 0 {
        return Err(ConfigError::new("config.database", "the port is not allowed"));
    }
    let deadline = number(llm, "deadline_ms")?;
    if !(1..=180_000).contains(&deadline) {
        return Err(ConfigError::new("config.budget", "the deadline is not allowed"));
    }
    Ok(Config {
        mode: "operator-development".to_owned(),
        development_preview: true,
        publication: "not_published".to_owned(),
        llm_review: "unreviewed".to_owned(),
        runtime_acknowledgement: crate::DEVELOPMENT_ACK.to_owned(),
        database_type: string(database, "type")?.to_owned(),
        host: string(database, "host")?.to_owned(),
        port,
        database: string(database, "name")?.to_owned(),
        username: string(database, "username")?.to_owned(),
        tls: string(database, "tls")?.to_owned(),
        secret_ref: string(database, "secret_ref")?.to_owned(),
        password_file: PathBuf::from(string(database, "password_file")?),
        ca_file: PathBuf::from(string(database, "ca_file")?),
        tables,
        model_id: "grok-4.7".to_owned(),
        llm_origin: string(llm, "origin")?.to_owned(),
        llm_path: "/v1/chat/completions".to_owned(),
        bearer_file: PathBuf::from(string(llm, "bearer_file")?),
        deadline_ms: deadline,
        catalog_bin: PathBuf::from(string(paths, "catalog_bin")?),
        query_pg: PathBuf::from(string(gateway, "query_pg")?),
        oo_bin: PathBuf::from(string(gateway, "oo")?),
        gateway_token_file: PathBuf::from(string(gateway, "token_file")?),
        policy_file: PathBuf::from(string(gateway, "policy_file")?),
        pi_executable: PathBuf::from(string(pi, "executable")?),
        pi_executable_sha256: string(pi, "executable_sha256")?.to_owned(),
        pi_runtime: PathBuf::from(string(pi, "runtime")?),
        pi_runtime_sha256: string(pi, "runtime_sha256")?.to_owned(),
        pi_provider: string(pi, "provider")?.to_owned(),
        pi_model: string(pi, "model")?.to_owned(),
        work: PathBuf::from(string(paths, "work")?),
        knowledge: PathBuf::from(string(paths, "knowledge")?),
        operator_label: string(operator, "label")?.to_owned(),
        operator_at: string(operator, "at")?.to_owned(),
    })
}

fn reject_secret_keys(map: &Map<String, Value>) -> Result<(), ConfigError> {
    const FORBIDDEN: &[&str] = &["api_key", "bearer", "password", "secret", "token"];
    fn walk(value: &Value) -> Result<(), ConfigError> {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if FORBIDDEN.contains(&key.as_str()) {
                        return Err(ConfigError::new("config.secret", "the config contains a secret value field"));
                    }
                    walk(child)?;
                }
                Ok(())
            }
            Value::Array(items) => {
                for item in items {
                    walk(item)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    walk(&Value::Object(map.clone()))
}

fn tables(value: &Value) -> Result<Vec<(String, String)>, ConfigError> {
    let items = value.as_array().ok_or_else(|| ConfigError::new("config.tables", "the table list is missing"))?;
    if items.is_empty() || items.len() > 16 {
        return Err(ConfigError::new("config.tables", "the table list is not allowed"));
    }
    let mut out = Vec::new();
    for item in items {
        let row = object(item)?;
        require(row, &["schema", "table"])?;
        out.push((string(row, "schema")?.to_owned(), string(row, "table")?.to_owned()));
    }
    Ok(out)
}

fn object(value: &Value) -> Result<&Map<String, Value>, ConfigError> {
    value.as_object().ok_or_else(|| ConfigError::new("config.schema", "a value is not an object"))
}

fn require(map: &Map<String, Value>, keys: &[&str]) -> Result<(), ConfigError> {
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return Err(ConfigError::new("config.schema", "the config keys do not match"));
    }
    Ok(())
}

fn eq(map: &Map<String, Value>, key: &str, expect: &str) -> Result<(), ConfigError> {
    if map.get(key).and_then(Value::as_str) != Some(expect) {
        return Err(ConfigError::new("config.schema", "a required value does not match"));
    }
    Ok(())
}

fn string<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, ConfigError> {
    map.get(key).and_then(Value::as_str).filter(|text| !text.is_empty()).ok_or_else(|| ConfigError::new("config.schema", "a string field is missing"))
}

fn number(map: &Map<String, Value>, key: &str) -> Result<u64, ConfigError> {
    map.get(key).and_then(Value::as_u64).ok_or_else(|| ConfigError::new("config.schema", "a number field is missing"))
}

fn require_pi(map: &Map<String, Value>) -> Result<(), ConfigError> {
    const REQUIRED: &[&str] = &[
        "executable",
        "executable_sha256",
        "model",
        "provider",
        "runtime",
        "runtime_sha256",
    ];
    const OPTIONAL: &[&str] = &["pi_gid", "pi_uid"];
    if REQUIRED.iter().any(|key| !map.contains_key(*key)) {
        return Err(ConfigError::new("config.schema", "the config keys do not match"));
    }
    for key in map.keys() {
        if !REQUIRED.contains(&key.as_str()) && !OPTIONAL.contains(&key.as_str()) {
            return Err(ConfigError::new("config.schema", "the config keys do not match"));
        }
    }
    accept_fixed_bound(map, "pi_uid")?;
    accept_fixed_bound(map, "pi_gid")?;
    Ok(())
}

fn accept_fixed_bound(map: &Map<String, Value>, key: &str) -> Result<(), ConfigError> {
    let Some(value) = map.get(key) else {
        return Ok(());
    };
    match value.as_u64() {
        Some(65532) => Ok(()),
        _ => Err(ConfigError::new("config.pi", "the pi identity bound is not allowed")),
    }
}

#[cfg(test)]
mod tests {
    use super::{load_config, OPERATOR_PI_GID, OPERATOR_PI_UID};
    use std::fs;
    use std::path::PathBuf;

    fn write_config(dir: &std::path::Path, pi_extra: &str) -> PathBuf {
        let config = dir.join("loop.json");
        let body = format!(
            "{{\n\
               \"protocol\": \"oo.minimal-product-loop.config/v1\",\n\
               \"mode\": \"operator-development\",\n\
               \"development_preview\": true,\n\
               \"publication\": \"not_published\",\n\
               \"llm_review\": \"unreviewed\",\n\
               \"runtime_acknowledgement\": \"local-process-not-a-sandbox-certificate\",\n\
               \"database\": {{\"type\": \"postgresql\", \"host\": \"127.0.0.1\", \"port\": 9, \"name\": \"shop\", \"username\": \"reader\", \"tls\": \"require\", \"secret_ref\": \"secret://pg/operator/reader\", \"password_file\": \"/tmp/password\", \"ca_file\": \"/tmp/ca.pem\"}},\n\
               \"tables\": [{{\"schema\": \"public\", \"table\": \"batch_c\"}}],\n\
               \"llm\": {{\"model_id\": \"grok-4.7\", \"origin\": \"http://127.0.0.1:8317\", \"path\": \"/v1/chat/completions\", \"bearer_file\": \"/tmp/bearer\", \"deadline_ms\": 90000}},\n\
               \"gateway\": {{\"oo\": \"/tmp/oo\", \"query_pg\": \"/tmp/query-pg\", \"policy_file\": \"/tmp/policy.json\", \"token_file\": \"/tmp/gateway-token\"}},\n\
               \"pi\": {{\"executable\": \"/tmp/pi\", \"executable_sha256\": \"{sha}\", \"runtime\": \"/tmp/node\", \"runtime_sha256\": \"{sha}\", \"provider\": \"grok\", \"model\": \"grok-4.7\"{extra}}},\n\
               \"paths\": {{\"catalog_bin\": \"/tmp/catalog\", \"knowledge\": \"/tmp/knowledge\", \"work\": \"/tmp/work\"}},\n\
               \"operator\": {{\"label\": \"a411\", \"at\": \"2026-10-09T00:00:00Z\"}}\n\
             }}\n",
            sha = "0000000000000000000000000000000000000000000000000000000000000000",
            extra = pi_extra,
        );
        fs::write(&config, body).expect("config");
        config
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oo-a411-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn operator_pi_bounds_default_when_the_fields_are_omitted() {
        let dir = scratch("pi-omit");
        let loaded = load_config(&write_config(&dir, "")).expect("config");
        assert_eq!(loaded.pi_uid(), OPERATOR_PI_UID);
        assert_eq!(loaded.pi_gid(), OPERATOR_PI_GID);
        assert_eq!(loaded.pi_uid(), 65532);
        assert_eq!(loaded.pi_gid(), 65532);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn operator_pi_bounds_accept_only_the_fixed_pair() {
        let dir = scratch("pi-explicit");
        let loaded = load_config(&write_config(&dir, ", \"pi_uid\": 65532, \"pi_gid\": 65532")).expect("config");
        assert_eq!(loaded.pi_uid(), 65532);
        assert_eq!(loaded.pi_gid(), 65532);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn operator_pi_bounds_reject_zero_custom_and_unknown_keys() {
        let dir = scratch("pi-reject");
        let zero = load_config(&write_config(&dir, ", \"pi_uid\": 0")).expect_err("zero");
        assert_eq!(zero.code, "config.pi");
        let custom = load_config(&write_config(&dir, ", \"pi_gid\": 1000")).expect_err("custom");
        assert_eq!(custom.code, "config.pi");
        let text = load_config(&write_config(&dir, ", \"pi_uid\": \"65532\"")).expect_err("text");
        assert_eq!(text.code, "config.pi");
        let extra = load_config(&write_config(&dir, ", \"home\": \"/tmp/pi\"")).expect_err("extra");
        assert_eq!(extra.code, "config.schema");
        let _ = fs::remove_dir_all(dir);
    }
}
