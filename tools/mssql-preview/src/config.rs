use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fail::Fail;

pub const CUSTOMER_PROTOCOL: &str = "oo.mssql-preview.customer/v1";

const INLINE_SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "sa_password",
    "mssql_sa_password",
    "secret",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Customer {
    pub protocol: String,
    pub source_id: String,
    pub secret_ref: String,
    pub endpoint: Endpoint,
    pub encrypt: String,
    pub trust_server_certificate: bool,
    #[serde(default)]
    pub ca_file: Option<String>,
    pub approved_objects: Vec<ObjectName>,
    pub budget: Budget,
    pub permissions_expected: Vec<String>,
    #[serde(default)]
    pub connect_deadline_unix_ms: Option<u64>,
    #[serde(default)]
    pub capabilities_requested: Vec<String>,
    #[serde(default)]
    pub seal: Option<SealInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectName {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub unit: String,
    pub hard_limit: u64,
    pub confidence: String,
    #[serde(default)]
    pub requested_rows: Option<u64>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SealInput {
    pub run_id: String,
    pub owner: String,
    pub source_generation: i64,
    pub target_generation: i64,
    pub snapshot_kind: String,
    pub snapshot_value: String,
}

pub fn parse_customer(bytes: &[u8]) -> Result<Customer, Fail> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Fail::new("invalid_config", "customer json could not be read"))?;
    reject_inline_secrets(&value)?;
    let customer: Customer = serde_json::from_value(value)
        .map_err(|_| Fail::new("invalid_config", "customer json does not match the customer protocol"))?;
    if customer.protocol != CUSTOMER_PROTOCOL {
        return Err(Fail::new(
            "invalid_protocol",
            "customer protocol must be oo.mssql-preview.customer/v1",
        ));
    }
    Ok(customer)
}

fn reject_inline_secrets(value: &Value) -> Result<(), Fail> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let lower = key.to_ascii_lowercase();
                if lower != "secret_ref" && INLINE_SECRET_KEYS.contains(&lower.as_str()) {
                    return Err(Fail::new(
                        "inline_secret",
                        "customer config contains a secret field; use secret_ref",
                    ));
                }
                reject_inline_secrets(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                reject_inline_secrets(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn secret_path(secret_ref: &str) -> Result<std::path::PathBuf, Fail> {
    if secret_ref.starts_with("secret://") || secret_ref.contains("://") && !secret_ref.starts_with("file://") {
        return Err(Fail::new(
            "secret_ref_not_file",
            "secret_ref must be an absolute path or a file URI",
        ));
    }
    let path = if let Some(rest) = secret_ref.strip_prefix("file://") {
        if rest.contains('%') || !rest.starts_with('/') {
            return Err(Fail::new(
                "secret_ref_not_file",
                "file secret_ref must be file:///absolute/path with no encoding",
            ));
        }
        rest.to_string()
    } else if secret_ref.starts_with('/') {
        secret_ref.to_string()
    } else {
        return Err(Fail::new(
            "secret_ref_not_file",
            "secret_ref must be an absolute path or a file URI",
        ));
    };
    if path.contains('\0') {
        return Err(Fail::new("secret_ref_not_file", "secret path is not a file"));
    }
    Ok(std::path::PathBuf::from(path))
}
