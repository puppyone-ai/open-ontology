//! Builder config. The bearer stays in its file.

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

use crate::canonical::object;
use crate::error::PgBuilderError;

#[derive(Clone, Debug)]
pub struct Config {
    pub purpose: Purpose,
    pub origin: String,
    pub path: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    pub addr: SocketAddr,
    pub bearer_file: PathBuf,
    pub deadline: Duration,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub max_prompt_tokens: u64,
    pub max_completion_tokens: u64,
    pub max_total_tokens: u64,
    pub max_tables: usize,
    pub price: Price,
    pub postgres: Option<Postgres>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Generate,
    Fault,
}

#[derive(Clone, Debug)]
pub enum Price {
    Unknown,
    Declared { per_total_token: String, units: u128, scale: u32 },
}

#[derive(Clone, Debug)]
pub struct Postgres {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password_file: PathBuf,
    pub ca_file: PathBuf,
    pub catalog_bin: PathBuf,
    pub tables: Vec<(String, String)>,
}

pub fn load_config(text: &str) -> Result<Config, PgBuilderError> {
    if text.len() > 65536 {
        return Err(PgBuilderError::new("config.budget", "the config file is too large"));
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|_| PgBuilderError::new("config.schema", "the config is not one JSON object"))?;
    let map = object(&value, "config.schema")?;
    require_keys(
        map,
        &[
            "adapter",
            "budget",
            "declared_price",
            "endpoint",
            "namespace",
            "protocol",
            "purpose",
            "sample_policy",
            "secret",
            "temperature",
        ],
    )?;
    if map.keys().any(|key| !matches!(key.as_str(), "adapter" | "budget" | "declared_price" | "endpoint" | "namespace" | "protocol" | "purpose" | "sample_policy" | "secret" | "temperature" | "postgres" | "reasoning_effort")) {
        return Err(PgBuilderError::new("config.schema", "the config has an unknown key"));
    }
    eq(map, "protocol", "oo.pg-builder.config/v1")?;
    eq(map, "adapter", "loopback_http_local_dev")?;
    eq(map, "namespace", "pg-builder")?;
    eq(map, "sample_policy", "none")?;
    if map.get("temperature") != Some(&Value::from(0)) {
        return Err(PgBuilderError::new("config.temperature", "temperature must be zero"));
    }
    let purpose = match string(map, "purpose")? {
        "generate" => Purpose::Generate,
        "fault_injection" => Purpose::Fault,
        _ => return Err(PgBuilderError::new("config.purpose", "the purpose is not allowed")),
    };
    let endpoint = object(map.get("endpoint").unwrap_or(&Value::Null), "config.endpoint")?;
    require_exact(endpoint, &["model_id", "origin", "path"])?;
    let origin = string(endpoint, "origin")?.to_owned();
    let path = string(endpoint, "path")?.to_owned();
    let model_id = string(endpoint, "model_id")?.to_owned();
    if path != "/v1/chat/completions" || !matches!(model_id.as_str(), "grok-4.7" | "gpt-6.1-sol") {
        return Err(PgBuilderError::new("config.endpoint", "the endpoint path or model is not the builder endpoint"));
    }
    let addr = parse_origin(&origin, purpose)?;
    let secret = object(map.get("secret").unwrap_or(&Value::Null), "config.secret")?;
    require_exact(secret, &["bearer_file"])?;
    let bearer_file = PathBuf::from(string(secret, "bearer_file")?);
    let budget = object(map.get("budget").unwrap_or(&Value::Null), "config.budget")?;
    require_exact(
        budget,
        &[
            "deadline_ms",
            "max_completion_tokens",
            "max_in_flight",
            "max_prompt_tokens",
            "max_request_bytes",
            "max_response_bytes",
            "max_tables",
            "max_total_tokens",
        ],
    )?;
    if number(budget, "max_in_flight")? != 1 {
        return Err(PgBuilderError::new("config.budget", "concurrency must be one"));
    }
    let deadline_ms = bounded(budget, "deadline_ms", 1, 180_000)?;
    if deadline_ms > 30_000 && (purpose != Purpose::Generate || origin != "http://127.0.0.1:8317") {
        return Err(PgBuilderError::new(
            "config.budget",
            "a deadline above 30000 is only allowed for the local private builder model",
        ));
    }
    let postgres = match map.get("postgres") {
        None => None,
        Some(value) => Some(parse_postgres(value)?),
    };
    let reasoning_effort = match map.get("reasoning_effort") {
        None => None,
        Some(Value::String(value)) if value == "low" => Some(value.clone()),
        _ => return Err(PgBuilderError::new("config.reasoning", "reasoning_effort must be low")),
    };
    Ok(Config {
        purpose,
        origin,
        path,
        model_id,
        reasoning_effort,
        addr,
        bearer_file,
        deadline: Duration::from_millis(deadline_ms),
        max_request_bytes: bounded(budget, "max_request_bytes", 1, 262_144)? as usize,
        max_response_bytes: bounded(budget, "max_response_bytes", 1, 262_144)? as usize,
        max_prompt_tokens: bounded(budget, "max_prompt_tokens", 1, 100_000)?,
        max_completion_tokens: bounded(budget, "max_completion_tokens", 1, 100_000)?,
        max_total_tokens: bounded(budget, "max_total_tokens", 1, 200_000)?,
        max_tables: bounded(budget, "max_tables", 1, 16)? as usize,
        price: parse_price(map.get("declared_price").unwrap_or(&Value::Null))?,
        postgres,
    })
}

pub fn read_bearer(path: &Path) -> Result<String, PgBuilderError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|_| PgBuilderError::new("config.secret", "the bearer file is not a regular file"))?;
    if !meta.file_type().is_file() || meta.len() > 512 {
        return Err(PgBuilderError::new("config.secret", "the bearer file is not a regular file"));
    }
    let bytes = fs::read(path)
        .map_err(|_| PgBuilderError::new("config.secret", "the bearer file is not a regular file"))?;
    let mut token = bytes;
    if token.last() == Some(&b'\n') {
        token.pop();
    }
    if token.is_empty()
        || token.len() > 256
        || !token.iter().all(|byte| (0x21..=0x7e).contains(byte))
    {
        return Err(PgBuilderError::new("config.secret", "the bearer file is not an allowed token"));
    }
    String::from_utf8(token).map_err(|_| PgBuilderError::new("config.secret", "the bearer file is not an allowed token"))
}

pub fn contains_secret(haystack: &[u8], token: &str) -> bool {
    !token.is_empty() && haystack.windows(token.len()).any(|window| window == token.as_bytes())
}

fn parse_origin(origin: &str, purpose: Purpose) -> Result<SocketAddr, PgBuilderError> {
    let rest = origin
        .strip_prefix("http://127.0.0.1:")
        .ok_or_else(|| PgBuilderError::new("config.endpoint", "the origin is not the loopback builder endpoint"))?;
    if rest.chars().any(|ch| !ch.is_ascii_digit()) || rest.is_empty() {
        return Err(PgBuilderError::new("config.endpoint", "the origin is not the loopback builder endpoint"));
    }
    let port: u16 = rest
        .parse()
        .map_err(|_| PgBuilderError::new("config.endpoint", "the origin is not the loopback builder endpoint"))?;
    if port == 0 {
        return Err(PgBuilderError::new("config.endpoint", "the origin is not the loopback builder endpoint"));
    }
    if purpose == Purpose::Generate && port != 8317 {
        return Err(PgBuilderError::new("config.endpoint", "generate uses the approved loopback port"));
    }
    Ok(SocketAddr::from(([127, 0, 0, 1], port)))
}

fn parse_price(value: &Value) -> Result<Price, PgBuilderError> {
    let map = object(value, "config.price")?;
    match string(map, "status")? {
        "unknown" => {
            if map.len() != 1 {
                return Err(PgBuilderError::new("config.price", "unknown price has no amount"));
            }
            Ok(Price::Unknown)
        }
        "declared_non_billing" => {
            require_exact(map, &["currency", "per_total_token", "status"])?;
            if string(map, "currency")? != "USD" {
                return Err(PgBuilderError::new("config.price", "the declared currency is not USD"));
            }
            let lexeme = string(map, "per_total_token")?.to_owned();
            let (units, scale) = parse_decimal(&lexeme)?;
            Ok(Price::Declared { per_total_token: lexeme, units, scale })
        }
        _ => Err(PgBuilderError::new("config.price", "the declared price status is not allowed")),
    }
}

fn parse_decimal(text: &str) -> Result<(u128, u32), PgBuilderError> {
    if text.is_empty() || text.len() > 32 || text.starts_with('+') || text.contains(['e', 'E', '-']) {
        return Err(PgBuilderError::new("config.price", "the declared price is not an allowed decimal"));
    }
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || frac.len() > 8
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(PgBuilderError::new("config.price", "the declared price is not an allowed decimal"));
    }
    let digits = if frac.is_empty() { whole.to_owned() } else { format!("{whole}{frac}") };
    let units = digits
        .parse::<u128>()
        .map_err(|_| PgBuilderError::new("config.price", "the declared price is not an allowed decimal"))?;
    Ok((units, u32::try_from(frac.len()).unwrap_or(9)))
}

pub fn amount(price: &Price, tokens: u64) -> Result<Value, PgBuilderError> {
    match price {
        Price::Unknown => Ok(Value::Null),
        Price::Declared { per_total_token, units, scale } => {
            let product = units
                .checked_mul(u128::from(tokens))
                .ok_or_else(|| PgBuilderError::new("config.price", "the declared amount overflowed"))?;
            Ok(Value::String(format_units(product, *scale, per_total_token)))
        }
    }
}

fn format_units(product: u128, scale: u32, _lexeme: &str) -> String {
    if scale == 0 {
        return product.to_string();
    }
    let factor = 10u128.pow(scale);
    let whole = product / factor;
    let frac = product % factor;
    let width = usize::try_from(scale).unwrap_or(0);
    let frac_text = format!("{frac:0width$}");
    format!("{whole}.{frac_text}")
}

fn parse_postgres(value: &Value) -> Result<Postgres, PgBuilderError> {
    let map = object(value, "config.postgres")?;
    require_exact(
        map,
        &["ca_file", "catalog_bin", "database", "host", "password_file", "port", "tables", "username"],
    )?;
    let host = string(map, "host")?.to_owned();
    if !valid_host(&host) {
        return Err(PgBuilderError::new("config.postgres", "the postgres host is not allowed"));
    }
    let port = u16::try_from(number(map, "port")?)
        .map_err(|_| PgBuilderError::new("config.postgres", "the postgres port is not allowed"))?;
    if port == 0 {
        return Err(PgBuilderError::new("config.postgres", "the postgres port is not allowed"));
    }
    let database = ident(string(map, "database")?)?;
    let username = ident(string(map, "username")?)?;
    let tables_value = map
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("config.postgres", "the table list is empty"))?;
    if tables_value.is_empty() || tables_value.len() > 16 {
        return Err(PgBuilderError::new("config.postgres", "the table list is empty"));
    }
    let mut tables = Vec::new();
    for table in tables_value {
        let row = object(table, "config.postgres")?;
        require_exact(row, &["schema", "table"])?;
        tables.push((ident(string(row, "schema")?)?, ident(string(row, "table")?)?));
    }
    Ok(Postgres {
        host,
        port,
        database,
        username,
        password_file: PathBuf::from(string(map, "password_file")?),
        ca_file: PathBuf::from(string(map, "ca_file")?),
        catalog_bin: PathBuf::from(string(map, "catalog_bin")?),
        tables,
    })
}

fn valid_host(host: &str) -> bool {
    if host == "127.0.0.1" {
        return true;
    }
    !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' || byte == b'_')
        && !host.contains("://")
}

fn ident(value: &str) -> Result<String, PgBuilderError> {
    if (1..=63).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        Ok(value.to_owned())
    } else {
        Err(PgBuilderError::new("config.postgres", "a postgres name is not allowed"))
    }
}

fn require_keys(map: &Map<String, Value>, keys: &[&str]) -> Result<(), PgBuilderError> {
    for key in keys {
        if !map.contains_key(*key) {
            return Err(PgBuilderError::new("config.schema", "a required config field is missing"));
        }
    }
    Ok(())
}

fn require_exact(map: &Map<String, Value>, keys: &[&str]) -> Result<(), PgBuilderError> {
    require_keys(map, keys)?;
    if map.len() != keys.len() {
        return Err(PgBuilderError::new("config.schema", "the config has an unknown key"));
    }
    Ok(())
}

fn eq(map: &Map<String, Value>, key: &str, expect: &str) -> Result<(), PgBuilderError> {
    if string(map, key)? == expect {
        Ok(())
    } else {
        Err(PgBuilderError::new("config.schema", "a config field has the wrong value"))
    }
}

fn string<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, PgBuilderError> {
    map.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("config.schema", "a config field has the wrong type"))
}

fn number(map: &Map<String, Value>, key: &str) -> Result<u64, PgBuilderError> {
    map.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| PgBuilderError::new("config.budget", "a budget field has the wrong type"))
}

fn bounded(map: &Map<String, Value>, key: &str, low: u64, high: u64) -> Result<u64, PgBuilderError> {
    let value = number(map, key)?;
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(PgBuilderError::new("config.budget", "a budget field is outside its range"))
    }
}
