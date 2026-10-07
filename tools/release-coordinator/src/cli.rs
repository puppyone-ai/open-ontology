//! Command intake. Waiver keys and a production-mode flag write nothing.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::canon::{object, reject_waiver};
use crate::error::StoreError;
use crate::store::{self, Endpoint};
use crate::sql::ADMIN_ROLE;

pub struct CommandExit {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn argv_malformed(args: &[String]) -> Option<&'static str> {
    let Some(command) = args.first() else {
        return Some("missing_command");
    };
    if !matches!(
        command.as_str(),
        "install" | "stage" | "status" | "gate-record" | "plan" | "activate" | "recover"
    ) {
        return Some("missing_command");
    }
    match args {
        [_, flag, path] if flag == "--config" && !path.is_empty() => None,
        [_, flag, path] if flag == "--config" && path.is_empty() => Some("config"),
        [_, flag] => match flag.strip_prefix("--config=") {
            Some(path) if !path.is_empty() => None,
            _ => Some("config"),
        },
        [_] => Some("config"),
        _ => Some("extra_arg"),
    }
}

pub async fn dispatch(args: &[String], stdin: &[u8]) -> CommandExit {
    if let Some(reason) = argv_malformed(args) {
        return malformed(reason);
    }
    let document = match serde_json::from_slice::<Value>(stdin) {
        Ok(document) => document,
        Err(_) => return malformed("json"),
    };
    let Some(map) = object(&document) else {
        return malformed("json");
    };
    if reject_waiver(map) {
        return malformed("waiver");
    }
    if map.get("product_mode_can_activate").and_then(Value::as_bool) == Some(true) {
        return malformed("product_mode_can_activate");
    }
    let protocol = match text(map, "protocol") {
        Ok(protocol) => protocol,
        Err(reason) => return malformed(reason),
    };
    let keys: &[&str] = match (args[0].as_str(), protocol) {
        ("install", "oo.release-coordinator/install/v1") => &["protocol", "project_id", "authority_id", "covers_gates"],
        ("stage", "oo.release-coordinator/stage/v1") => &[
            "protocol", "project_id", "candidate_id", "version", "base_version", "effective_oos", "visibility",
            "policy_id", "policy_version", "knowledge_id", "knowledge_version", "catalog_version",
        ],
        ("status", "oo.release-coordinator/status/v1") => &["protocol", "project_id", "candidate_id", "source_hash"],
        ("gate-record", "oo.release-coordinator/gate-record/v1") => &[
            "protocol", "project_id", "candidate_id", "source_hash", "gate_id", "issuer_class", "status", "blocking",
            "report_type", "authority_statement_sha256",
        ],
        ("gate-record", "oo.release-coordinator/checkpoint/v1") => &[
            "protocol", "project_id", "candidate_id", "kind", "source_hash", "epoch", "barrier", "sessions_terminated",
            "issuer_class", "authority_statement_sha256",
        ],
        ("plan", "oo.release-coordinator/plan/v1") | ("activate", "oo.release-coordinator/activate/v1") => {
            &["protocol", "project_id", "candidate_id", "source_hash", "scope", "expected_version"]
        }
        ("recover", "oo.release-coordinator/recover/v1") => &["protocol", "action", "project_id", "candidate_id", "expected_version"],
        _ => return malformed("protocol"),
    };
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return malformed("field");
    }
    let config = match read_config(Path::new(config_path(args))) {
        Ok(config) => config,
        Err(reason) => return malformed(reason),
    };
    let password = match read_secret(Path::new(&config.password_file)) {
        Ok(password) => password,
        Err(reason) => return malformed(reason),
    };
    let ca_pem = match read_limited(Path::new(&config.ca_file), 1_048_576, "ca_file") {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => return malformed("ca_file"),
    };
    let endpoint = Endpoint { host: config.host, port: config.port, database: config.database };
    let result = match (args[0].as_str(), protocol) {
        ("install", "oo.release-coordinator/install/v1") => install(&endpoint, &config.user, &password, &ca_pem, map).await,
        ("stage", "oo.release-coordinator/stage/v1") => stage(&endpoint, &config.user, &password, &ca_pem, map).await,
        ("status", "oo.release-coordinator/status/v1") => status(&endpoint, &config.user, &password, &ca_pem, map).await,
        ("gate-record", "oo.release-coordinator/gate-record/v1") => gate(&endpoint, &config.user, &password, &ca_pem, map).await,
        ("gate-record", "oo.release-coordinator/checkpoint/v1") => checkpoint(&endpoint, &config.user, &password, &ca_pem, map).await,
        ("plan", "oo.release-coordinator/plan/v1") => plan(&endpoint, &config.user, &password, &ca_pem, map).await,
        ("activate", "oo.release-coordinator/activate/v1") => activate(&endpoint, &config.user, &password, &ca_pem, map).await,
        ("recover", "oo.release-coordinator/recover/v1") => recover(&endpoint, &config.user, &password, &ca_pem, map).await,
        _ => return malformed("protocol"),
    };
    match result {
        Ok(receipt) => {
            let bytes = match serde_json::to_vec(&receipt.body) {
                Ok(bytes) => bytes,
                Err(_) => return malformed("json"),
            };
            let stderr = if receipt.code == 2 {
                let reason = receipt.body.get("reason").and_then(Value::as_str).unwrap_or("refused");
                format!("release-coordinator: refused {reason}\n")
            } else {
                String::new()
            };
            CommandExit { code: i32::from(receipt.code), stdout: bytes, stderr: stderr.into_bytes() }
        }
        Err(error) => CommandExit {
            code: 1,
            stdout: Vec::new(),
            stderr: format!("release-coordinator: error {error}\n").into_bytes(),
        },
    }
}

struct Config {
    host: String,
    port: u16,
    database: String,
    user: String,
    password_file: String,
    ca_file: String,
}

fn config_path(args: &[String]) -> &str {
    match args {
        [_, flag, path] if flag == "--config" => path,
        [_, flag] => flag.strip_prefix("--config=").unwrap_or(""),
        _ => "",
    }
}

fn read_config(path: &Path) -> Result<Config, &'static str> {
    let bytes = read_limited(path, 65_536, "config")?;
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| "config")?;
    let map = object(&document).ok_or("config")?;
    let allowed = ["host", "port", "database", "user", "password_file", "ca_file"];
    if map.len() != allowed.len() || allowed.iter().any(|key| !map.contains_key(*key)) {
        return Err("config");
    }
    let port = map.get("port").and_then(Value::as_u64).ok_or("config")?;
    let port = u16::try_from(port).map_err(|_| "config")?;
    if port == 0 {
        return Err("config");
    }
    let user = owned(map, "user")?;
    if user != ADMIN_ROLE {
        return Err("login_role");
    }
    Ok(Config {
        host: owned(map, "host")?,
        port,
        database: owned(map, "database")?,
        user,
        password_file: owned(map, "password_file")?,
        ca_file: owned(map, "ca_file")?,
    })
}

fn read_secret(path: &Path) -> Result<String, &'static str> {
    let bytes = read_limited(path, 4096, "password_file")?;
    let mut text = String::from_utf8(bytes).map_err(|_| "password_file")?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() || text.bytes().any(|byte| byte.is_ascii_control()) {
        return Err("password_file");
    }
    Ok(text)
}

fn read_limited(path: &Path, max: usize, reason: &'static str) -> Result<Vec<u8>, &'static str> {
    let file = fs::File::open(path).map_err(|_| reason)?;
    let meta = file.metadata().map_err(|_| reason)?;
    if meta.len() > max as u64 {
        return Err(reason);
    }
    let bytes = fs::read(path).map_err(|_| reason)?;
    if bytes.len() > max {
        return Err(reason);
    }
    Ok(bytes)
}

fn malformed(reason: &'static str) -> CommandExit {
    CommandExit {
        code: 1,
        stdout: Vec::new(),
        stderr: format!("release-coordinator: malformed {reason}\n").into_bytes(),
    }
}

fn text<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, &'static str> {
    map.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).ok_or("field")
}

fn owned(map: &Map<String, Value>, key: &str) -> Result<String, &'static str> {
    text(map, key).map(str::to_owned)
}

fn optional<'a>(map: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, &'static str> {
    match map.get(key) {
        None => Err("field"),
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.as_str())),
        _ => Err("field"),
    }
}

fn flag(map: &Map<String, Value>, key: &str) -> Result<bool, &'static str> {
    map.get(key).and_then(Value::as_bool).ok_or("field")
}

fn object_field<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, &'static str> {
    match map.get(key) {
        Some(value) if value.is_object() => Ok(value),
        _ => Err("field"),
    }
}

async fn install(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8], map: &Map<String, Value>) -> Result<store::Receipt, StoreError> {
    let covers = match map.get("covers_gates").and_then(Value::as_array) {
        Some(items) if !items.is_empty() => items,
        _ => return Err(StoreError::Input),
    };
    let mut names = Vec::new();
    for item in covers {
        let Some(name) = item.as_str() else {
            return Err(StoreError::Input);
        };
        names.push(name.to_owned());
    }
    store::install(&store::InstallRequest {
        endpoint,
        user,
        password,
        ca_pem,
        project_id: text(map, "project_id").map_err(|_| StoreError::Input)?,
        authority_id: text(map, "authority_id").map_err(|_| StoreError::Input)?,
        covers: &names,
    })
    .await
}

async fn stage(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8], map: &Map<String, Value>) -> Result<store::Receipt, StoreError> {
    store::stage(&store::StageRequest {
        endpoint,
        user,
        password,
        ca_pem,
        project_id: text(map, "project_id").map_err(|_| StoreError::Input)?,
        candidate_id: text(map, "candidate_id").map_err(|_| StoreError::Input)?,
        version: text(map, "version").map_err(|_| StoreError::Input)?,
        base_version: optional(map, "base_version").map_err(|_| StoreError::Input)?,
        effective: object_field(map, "effective_oos").map_err(|_| StoreError::Input)?,
        visibility: object_field(map, "visibility").map_err(|_| StoreError::Input)?,
        policy_id: text(map, "policy_id").map_err(|_| StoreError::Input)?,
        policy_version: text(map, "policy_version").map_err(|_| StoreError::Input)?,
        knowledge_id: text(map, "knowledge_id").map_err(|_| StoreError::Input)?,
        knowledge_version: text(map, "knowledge_version").map_err(|_| StoreError::Input)?,
        catalog_version: text(map, "catalog_version").map_err(|_| StoreError::Input)?,
    })
    .await
}

async fn gate(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8], map: &Map<String, Value>) -> Result<store::Receipt, StoreError> {
    store::gate_record(&store::GateRequest {
        endpoint,
        user,
        password,
        ca_pem,
        project_id: text(map, "project_id").map_err(|_| StoreError::Input)?,
        candidate_id: text(map, "candidate_id").map_err(|_| StoreError::Input)?,
        source_hash: text(map, "source_hash").map_err(|_| StoreError::Input)?,
        gate_id: text(map, "gate_id").map_err(|_| StoreError::Input)?,
        issuer_class: text(map, "issuer_class").map_err(|_| StoreError::Input)?,
        status: text(map, "status").map_err(|_| StoreError::Input)?,
        blocking: flag(map, "blocking").map_err(|_| StoreError::Input)?,
        report_type: text(map, "report_type").map_err(|_| StoreError::Input)?,
        authority_statement_sha256: optional(map, "authority_statement_sha256").map_err(|_| StoreError::Input)?,
    })
    .await
}

async fn checkpoint(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8], map: &Map<String, Value>) -> Result<store::Receipt, StoreError> {
    store::checkpoint(&store::CheckpointRequest {
        endpoint,
        user,
        password,
        ca_pem,
        project_id: text(map, "project_id").map_err(|_| StoreError::Input)?,
        candidate_id: text(map, "candidate_id").map_err(|_| StoreError::Input)?,
        kind: text(map, "kind").map_err(|_| StoreError::Input)?,
        source_hash: text(map, "source_hash").map_err(|_| StoreError::Input)?,
        epoch: text(map, "epoch").map_err(|_| StoreError::Input)?,
        barrier: text(map, "barrier").map_err(|_| StoreError::Input)?,
        sessions_terminated: flag(map, "sessions_terminated").map_err(|_| StoreError::Input)?,
        issuer_class: text(map, "issuer_class").map_err(|_| StoreError::Input)?,
        authority_statement_sha256: optional(map, "authority_statement_sha256").map_err(|_| StoreError::Input)?,
    })
    .await
}

async fn plan(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8], map: &Map<String, Value>) -> Result<store::Receipt, StoreError> {
    scoped(endpoint, user, password, ca_pem, map, false).await
}

async fn activate(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8], map: &Map<String, Value>) -> Result<store::Receipt, StoreError> {
    scoped(endpoint, user, password, ca_pem, map, true).await
}

async fn status(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8], map: &Map<String, Value>) -> Result<store::Receipt, StoreError> {
    store::status(&store::PlanRequest {
        endpoint,
        user,
        password,
        ca_pem,
        project_id: text(map, "project_id").map_err(|_| StoreError::Input)?,
        candidate_id: text(map, "candidate_id").map_err(|_| StoreError::Input)?,
        source_hash: text(map, "source_hash").map_err(|_| StoreError::Input)?,
        scope: "test_scope",
        expected_version: None,
    })
    .await
}

async fn scoped(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
    activate: bool,
) -> Result<store::Receipt, StoreError> {
    let request = scope_request(endpoint, user, password, ca_pem, map)?;
    if activate { store::activate(&request).await } else { store::plan(&request).await }
}

fn scope_request<'a>(
    endpoint: &'a Endpoint,
    user: &'a str,
    password: &'a str,
    ca_pem: &'a [u8],
    map: &'a Map<String, Value>,
) -> Result<store::PlanRequest<'a>, StoreError> {
    Ok(store::PlanRequest {
        endpoint,
        user,
        password,
        ca_pem,
        project_id: text(map, "project_id").map_err(|_| StoreError::Input)?,
        candidate_id: text(map, "candidate_id").map_err(|_| StoreError::Input)?,
        source_hash: text(map, "source_hash").map_err(|_| StoreError::Input)?,
        scope: text(map, "scope").map_err(|_| StoreError::Input)?,
        expected_version: optional(map, "expected_version").map_err(|_| StoreError::Input)?,
    })
}

async fn recover(endpoint: &Endpoint, user: &str, password: &str, ca_pem: &[u8], map: &Map<String, Value>) -> Result<store::Receipt, StoreError> {
    store::recover(&store::RecoverRequest {
        endpoint,
        user,
        password,
        ca_pem,
        action: text(map, "action").map_err(|_| StoreError::Input)?,
        project_id: text(map, "project_id").map_err(|_| StoreError::Input)?,
        candidate_id: text(map, "candidate_id").map_err(|_| StoreError::Input)?,
        expected_version: optional(map, "expected_version").map_err(|_| StoreError::Input)?,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{argv_malformed, dispatch};

    #[test]
    fn argv_requires_one_config_path() {
        assert_eq!(argv_malformed(&[]), Some("missing_command"));
        assert_eq!(argv_malformed(&["stage".to_owned()]), Some("config"));
        assert_eq!(argv_malformed(&["nope".to_owned(), "--config".to_owned(), "c.json".to_owned()]), Some("missing_command"));
        assert_eq!(argv_malformed(&["stage".to_owned(), "--config".to_owned(), "c.json".to_owned(), "extra".to_owned()]), Some("extra_arg"));
        assert_eq!(argv_malformed(&["activate".to_owned(), "--config=c.json".to_owned()]), None);
    }

    #[tokio::test]
    async fn waiver_and_product_mode_do_not_write() {
        let args = vec!["plan".to_owned(), "--config".to_owned(), "unused.json".to_owned()];
        let force = dispatch(&args, br#"{"protocol":"oo.release-coordinator/plan/v1","force":true}"#).await;
        assert_eq!(force.code, 1);
        assert!(force.stdout.is_empty());
        assert!(force.stderr.windows(6).any(|window| window == b"waiver"));
        let product = dispatch(&args, br#"{"protocol":"oo.release-coordinator/plan/v1","product_mode_can_activate":true}"#).await;
        assert_eq!(product.code, 1);
        assert!(product.stdout.is_empty());
        assert!(std::str::from_utf8(&product.stderr).unwrap().contains("product_mode_can_activate"));
        let unknown = dispatch(&args, br#"{"protocol":"oo.release-coordinator/plan/v1","project_id":"shop","candidate_id":"cand-a","source_hash":"ab","scope":"test_scope","expected_version":null,"extra":1}"#).await;
        assert_eq!(unknown.code, 1);
        assert!(unknown.stdout.is_empty());
    }
}
