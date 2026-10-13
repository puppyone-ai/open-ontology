//! Operator file. Passwords are paths, never values.

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::error::Fail;

pub const PROTOCOL: &str = "oo.postgres-preview.config/v1";
pub const MODEL_ORIGIN: &str = "http://127.0.0.1:8317";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub protocol: String,
    pub source_id: String,
    pub secret_ref: String,
    pub role: String,
    pub approved_objects: Vec<ObjectName>,
    #[serde(default)]
    pub approved_fields: Vec<FieldName>,
    #[serde(default)]
    pub pack_role: String,
    pub budget: Budget,
    pub model: Model,
    pub source: EndpointFile,
    pub dba: EndpointFile,
    pub target: EndpointFile,
    pub bins: Bins,
    pub coordinator_state: String,
    pub out_dir: String,
    #[serde(default)]
    pub denied: Option<EndpointFile>,
    /// Caller identity. Absent on a historical file, which still loads.
    /// Seal refuses a missing or handwritten identity that does not match the catalog schema.
    #[serde(default)]
    pub registry: Option<Registry>,
    /// Snapshot generation passed to import-pg. Historical files omit it and stay on generation 1.
    /// The pipeline names the physical table from this generation. An immutable apply receipt
    /// refuses a different payload of a generation that already has a shadow.
    #[serde(default = "one_generation")]
    pub source_generation: u64,
    /// Logical capture. Absent on historical preview files, which still load.
    /// The capture commands validate method, publication, slot, and the secret path.
    /// `load_config` does not open a session and does not turn a missing block into batch mode.
    #[serde(default)]
    pub capture: Option<Capture>,
}

/// Caller-owned C1 logical replication settings. The platform does not create the publication or the slot.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub method: String,
    pub publication: String,
    pub slot: String,
    pub username: String,
    pub password_file: String,
    pub reconcile_ms: u64,
    pub health_ms: u64,
    pub window_rows: u64,
    pub generation: u64,
}

fn one_generation() -> u64 {
    1
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub knowledge_id: String,
    pub knowledge_version: String,
    pub policy_id: String,
    pub policy_version: String,
    pub ontology_name: String,
    pub ontology_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectName {
    pub schema: String,
    pub table: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldName {
    pub schema: String,
    pub table: String,
    pub column: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub max_rows: u64,
    pub max_bytes: u64,
    pub statement_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub origin: String,
    pub path: String,
    pub model_id: String,
    pub bearer_file: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointFile {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password_file: String,
    pub ca_file: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bins {
    pub admission: String,
    pub budget: String,
    pub pipeline: String,
    pub builder: String,
    pub preview: String,
}

impl EndpointFile {
    pub fn path_buf(&self) -> (PathBuf, PathBuf) {
        (PathBuf::from(&self.password_file), PathBuf::from(&self.ca_file))
    }
}

pub fn load_config(bytes: &[u8]) -> Result<Config, Fail> {
    if bytes.windows(8).any(|window| window == b"password") && json_has_password_value(bytes) {
        return Err(Fail::new("config.secret", 1));
    }
    let config: Config = serde_json::from_slice(bytes).map_err(|_| Fail::new("config.schema", 1))?;
    if config.protocol != PROTOCOL {
        return Err(Fail::new("config.protocol", 1));
    }
    if config.model.origin != MODEL_ORIGIN || config.model.path != "/v1/chat/completions" {
        return Err(Fail::new("model.endpoint", 1));
    }
    if config.model.model_id != "grok-4.7" {
        return Err(Fail::new("model.id", 1));
    }
    if config.source.username == config.dba.username
        || config.target.username == config.source.username
        || config.target.username == config.dba.username
    {
        return Err(Fail::new("identity.separate", 6));
    }
    if let Some(denied) = &config.denied
        && (denied.username == config.source.username
            || denied.username == config.dba.username
            || denied.username == config.target.username)
    {
        return Err(Fail::new("identity.separate", 6));
    }
    let mut object_names = std::collections::HashSet::new();
    if config.approved_objects.iter().any(|object| {
        let safe = |name: &str| !name.is_empty() && name.len() <= 63
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        !safe(&object.schema) || !safe(&object.table)
            || !object_names.insert((object.schema.clone(), object.table.clone()))
    }) { return Err(Fail::new("config.objects", 1)); }
    if !pack_role_ok(&config.pack_role) {
        return Err(Fail::new("config.role", 1));
    }
    let mut field_names = std::collections::HashSet::new();
    if config.approved_fields.iter().any(|field| {
        let safe = |name: &str| !name.is_empty() && name.len() <= 63
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        !safe(&field.schema) || !safe(&field.table) || !safe(&field.column)
            || !config.approved_objects.iter().any(|object| object.schema == field.schema && object.table == field.table)
            || !field_names.insert((field.schema.clone(), field.table.clone(), field.column.clone()))
    }) {
        return Err(Fail::new("config.objects", 1));
    }
    if config.source_generation == 0 {
        return Err(Fail::new("import.shape", 1));
    }
    if let Some(registry) = &config.registry {
        let mut schemas = config.approved_objects.iter().map(|object| object.schema.as_str());
        let Some(schema) = schemas.next() else {
            return Err(Fail::new("catalog.identity", 1));
        };
        if schemas.any(|item| item != schema) {
            return Err(Fail::new("catalog.identity", 1));
        }
        let compiled = compiled_registry(schema, &registry.knowledge_version)?;
        if registry != &compiled || registry.knowledge_version != config.source_generation.to_string() {
            return Err(Fail::new("catalog.identity", 1));
        }
    }
    if config.approved_objects.is_empty()
        || config.budget.max_rows == 0
        || config.budget.max_bytes == 0
        || !(1..=120_000).contains(&config.budget.statement_ms)
    {
        return Err(Fail::new("config.budget", 1));
    }
    if !config.secret_ref.starts_with("secret://") || config.secret_ref.contains(' ') || config.secret_ref.len() < 10 {
        return Err(Fail::new("config.secret_ref", 1));
    }
    Ok(config)
}

/// Identity compiled from one catalog schema and that catalog's source generation.
/// Names follow the schema. Versions follow the generation. Nothing here is a fixed product title.
pub fn compiled_registry(schema: &str, generation: &str) -> Result<Registry, Fail> {
    if !schema_token(schema) || !version_token(generation) {
        return Err(Fail::new("catalog.identity", 1));
    }
    let registry = Registry {
        knowledge_id: format!("know-{schema}"),
        knowledge_version: generation.to_owned(),
        policy_id: format!("pol-{schema}"),
        policy_version: generation.to_owned(),
        ontology_name: schema.to_owned(),
        ontology_version: format!("ont-{schema}"),
    };
    if !slug_ok(&registry.knowledge_id)
        || !slug_ok(&registry.policy_id)
        || !version_token(&registry.ontology_version)
        || !schema_token(&registry.ontology_name)
    {
        return Err(Fail::new("catalog.identity", 1));
    }
    Ok(registry)
}

fn schema_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn slug_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-')
}

fn version_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'.' | b'_' | b'+' | b'-'))
}

pub fn digest64(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64 && bytes.iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn pack_role_ok(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let Some(rest) = value.strip_prefix("oo_r_") else {
        return false;
    };
    let bytes = rest.as_bytes();
    !bytes.is_empty()
        && value.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// Compare a caller request with the approved set. This function does not open a session.
pub fn unapproved_refusal(config: &Config, request: &serde_json::Value) -> Result<serde_json::Value, Fail> {
    if request.get("protocol").and_then(serde_json::Value::as_str) != Some("oo.postgres-preview.unapproved-request/v1") {
        return Err(Fail::new("config.schema", 1));
    }
    let objects = request.get("objects").and_then(serde_json::Value::as_array).ok_or(Fail::new("config.schema", 1))?;
    let fields = request.get("fields").and_then(serde_json::Value::as_array).ok_or(Fail::new("config.schema", 1))?;
    let mut refused_objects = Vec::new();
    for object in objects {
        let schema = object.get("schema").and_then(serde_json::Value::as_str).unwrap_or("");
        let table = object.get("table").and_then(serde_json::Value::as_str).unwrap_or("");
        if schema.is_empty() || table.is_empty() {
            return Err(Fail::new("config.schema", 1));
        }
        if !config.approved_objects.iter().any(|item| item.schema == schema && item.table == table) {
            refused_objects.push(serde_json::json!({"schema": schema, "table": table}));
        }
    }
    let mut refused_fields = Vec::new();
    for field in fields {
        let schema = field.get("schema").and_then(serde_json::Value::as_str).unwrap_or("");
        let table = field.get("table").and_then(serde_json::Value::as_str).unwrap_or("");
        let column = field.get("column").and_then(serde_json::Value::as_str).unwrap_or("");
        if schema.is_empty() || table.is_empty() || column.is_empty() {
            return Err(Fail::new("config.schema", 1));
        }
        let approved = config.approved_fields.iter().any(|item| {
            item.schema == schema && item.table == table && item.column == column
        });
        if !approved {
            refused_fields.push(serde_json::json!({"schema": schema, "table": table, "column": column}));
        }
    }
    if refused_objects.is_empty() && refused_fields.is_empty() {
        return Err(Fail::new("usage", 2));
    }
    Ok(serde_json::json!({
        "protocol": "oo.postgres-preview.unapproved-refusal/v1",
        "read_attempted": false,
        "connected": false,
        "objects": refused_objects,
        "fields": refused_fields
    }))
}

fn json_has_password_value(bytes: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    fn walk(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(map) => map.iter().any(|(key, child)| {
                (key == "password" || key == "bearer" || key == "token") || walk(child)
            }),
            serde_json::Value::Array(items) => items.iter().any(walk),
            _ => false,
        }
    }
    walk(&value)
}

pub fn read_text(path: &str) -> Result<Vec<u8>, Fail> {
    fs::read(path).map_err(|_| Fail::new("config.read", 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> serde_json::Value {
        serde_json::json!({
            "protocol": PROTOCOL,
            "source_id": "oo_dl01w1_46",
            "secret_ref": "secret://oo-dl01w1-46/reader",
            "role": "oo_r_reader",
            "approved_objects": [
                {"schema": "public", "table": "hubs"},
                {"schema": "public", "table": "shipments"},
                {"schema": "public", "table": "tracking_events"}
            ],
            "budget": {"max_rows": 100000, "max_bytes": 50000000, "statement_ms": 30000},
            "model": {
                "origin": MODEL_ORIGIN,
                "path": "/v1/chat/completions",
                "model_id": "grok-4.7",
                "bearer_file": "/tmp/oo-customer-demo-20261007/llm-token"
            },
            "source": endpoint("reader"),
            "dba": endpoint("dba"),
            "target": endpoint("target"),
            "bins": {
                "admission": "oo-source-admission",
                "budget": "oo-source-admission-pg-budget",
                "pipeline": "oo-pipeline",
                "builder": "pg-builder",
                "preview": "operator-preview"
            },
            "coordinator_state": "execution/control-state.json",
            "out_dir": "out"
        })
    }

    fn endpoint(user: &str) -> serde_json::Value {
        serde_json::json!({
            "host": "db.example",
            "port": 5432,
            "database": "src",
            "username": user,
            "password_file": format!("/run/secrets/{user}"),
            "ca_file": "/run/secrets/ca.pem"
        })
    }

    #[test]
    fn refuses_invalid_and_duplicate_objects_before_connecting() {
        let mut value = sample();
        value["approved_objects"][0]["table"] = serde_json::json!("hubs;DROP TABLE hubs");
        assert_eq!(load_config(&serde_json::to_vec(&value).unwrap()).unwrap_err().token, "config.objects");
        let mut value = sample();
        value["approved_objects"][1] = value["approved_objects"][0].clone();
        assert_eq!(load_config(&serde_json::to_vec(&value).unwrap()).unwrap_err().token, "config.objects");
    }

    #[test]
    fn accepts_the_operator_file_shape() {
        let bytes = serde_json::to_vec(&sample()).unwrap();
        let config = load_config(&bytes).unwrap();
        assert_eq!(config.approved_objects.len(), 3);
        assert_eq!(config.model.bearer_file, "/tmp/oo-customer-demo-20261007/llm-token");
    }

    #[test]
    fn rejects_a_password_field_and_a_foreign_model() {
        let mut with_password = sample();
        with_password["source"]["password"] = serde_json::json!("secret-value");
        let bytes = serde_json::to_vec(&with_password).unwrap();
        assert_eq!(load_config(&bytes).unwrap_err().token, "config.secret");
        let mut model = sample();
        model["model"]["origin"] = serde_json::json!("http://127.0.0.1:9");
        let bytes = serde_json::to_vec(&model).unwrap();
        assert_eq!(load_config(&bytes).unwrap_err().token, "model.endpoint");
    }

    #[test]
    fn customer_fields_refuse_an_unapproved_object_before_a_session() {
        let mut value = sample();
        value["approved_objects"] = serde_json::json!([{"schema": "customer_east", "table": "delivery_notes"}]);
        value["approved_fields"] = serde_json::json!([
            {"schema": "customer_east", "table": "delivery_notes", "column": "note_id"},
            {"schema": "customer_east", "table": "delivery_notes", "column": "amount"}
        ]);
        value["pack_role"] = serde_json::json!("oo_r_east");
        let config = load_config(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(config.pack_role, "oo_r_east");
        assert!(config.registry.is_none());
        value["registry"] = serde_json::json!({
            "knowledge_id": "know-logistics",
            "knowledge_version": "1",
            "policy_id": "pol-logistics",
            "policy_version": "1",
            "ontology_name": "logistics",
            "ontology_version": "ont-1"
        });
        assert_eq!(load_config(&serde_json::to_vec(&value).unwrap()).unwrap_err().token, "catalog.identity");
        let compiled = compiled_registry("customer_east", "1").unwrap();
        value["registry"] = serde_json::json!({
            "knowledge_id": compiled.knowledge_id,
            "knowledge_version": compiled.knowledge_version,
            "policy_id": compiled.policy_id,
            "policy_version": compiled.policy_version,
            "ontology_name": compiled.ontology_name,
            "ontology_version": compiled.ontology_version
        });
        let config = load_config(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(config.registry.as_ref().unwrap().ontology_name, "customer_east");
        assert_eq!(config.source_generation, 1);
        value["source_generation"] = serde_json::json!(0);
        assert_eq!(load_config(&serde_json::to_vec(&value).unwrap()).unwrap_err().token, "import.shape");
        value["source_generation"] = serde_json::json!(2);
        assert_eq!(load_config(&serde_json::to_vec(&value).unwrap()).unwrap_err().token, "catalog.identity");
        let compiled = compiled_registry("customer_east", "2").unwrap();
        value["registry"] = serde_json::json!({
            "knowledge_id": compiled.knowledge_id,
            "knowledge_version": compiled.knowledge_version,
            "policy_id": compiled.policy_id,
            "policy_version": compiled.policy_version,
            "ontology_name": compiled.ontology_name,
            "ontology_version": compiled.ontology_version
        });
        let config = load_config(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(config.source_generation, 2);
        assert_eq!(config.registry.as_ref().unwrap().knowledge_version, "2");
        let request = serde_json::json!({
            "protocol": "oo.postgres-preview.unapproved-request/v1",
            "objects": [{"schema": "customer_east", "table": "payroll"}],
            "fields": [{"schema": "customer_east", "table": "delivery_notes", "column": "internal_note"}]
        });
        let refusal = unapproved_refusal(&config, &request).unwrap();
        assert_eq!(refusal["read_attempted"], false);
        assert_eq!(refusal["connected"], false);
        assert_eq!(refusal["objects"][0]["table"], "payroll");
        assert_eq!(refusal["fields"][0]["column"], "internal_note");
        value["pack_role"] = serde_json::json!("reader");
        assert_eq!(load_config(&serde_json::to_vec(&value).unwrap()).unwrap_err().token, "config.role");
        value["pack_role"] = serde_json::json!("oo_r_east");
        value["approved_fields"][0]["table"] = serde_json::json!("payroll");
        assert_eq!(load_config(&serde_json::to_vec(&value).unwrap()).unwrap_err().token, "config.objects");
    }
}
