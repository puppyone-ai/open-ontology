//! Command intake. Waiver keys and a production-mode flag write nothing.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::canon::{object, reject_waiver};
use crate::error::StoreError;
use crate::maintain::{self, ControlConn, MaintainRequest};
use crate::sql::{ADMIN_ROLE, TARGET_ROLE};
use crate::store::{self, Endpoint};
use crate::target::{self, FenceRef, TargetConn};

pub struct CommandExit {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn argv_malformed(args: &[String]) -> Option<&'static str> {
    let Some(command) = args.first() else {
        return Some("missing_command");
    };
    if !known_command(command) {
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
    if map
        .get("product_mode_can_activate")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return malformed("product_mode_can_activate");
    }
    let protocol = match text(map, "protocol") {
        Ok(protocol) => protocol,
        Err(reason) => return malformed(reason),
    };
    let keys: &[&str] = match (args[0].as_str(), protocol) {
        ("install", "oo.release-coordinator/install/v1") => {
            &["protocol", "project_id", "authority_id", "covers_gates"]
        }
        ("stage", "oo.release-coordinator/stage/v1") => &[
            "protocol",
            "project_id",
            "candidate_id",
            "version",
            "base_version",
            "effective_oos",
            "visibility",
            "policy_id",
            "policy_version",
            "knowledge_id",
            "knowledge_version",
            "catalog_version",
        ],
        ("status", "oo.release-coordinator/status/v1") => {
            &["protocol", "project_id", "candidate_id", "source_hash"]
        }
        ("gate-record", "oo.release-coordinator/gate-record/v1") => &[
            "protocol",
            "project_id",
            "candidate_id",
            "source_hash",
            "gate_id",
            "issuer_class",
            "status",
            "blocking",
            "report_type",
            "authority_statement_sha256",
        ],
        ("gate-record", "oo.release-coordinator/checkpoint/v1") => &[
            "protocol",
            "project_id",
            "candidate_id",
            "kind",
            "source_hash",
            "epoch",
            "barrier",
            "sessions_terminated",
            "issuer_class",
            "authority_statement_sha256",
        ],
        ("business-install", "oo.release-coordinator/business-install/v1") => &["protocol"],
        ("business-execute", "oo.release-coordinator/business-execute/v1") => {
            &["protocol", "policy_sha256", "run_id"]
        }
        ("business-review", "oo.release-coordinator/business-review/v1") => &[
            "protocol",
            "policy_sha256",
            "run_id",
            "execution_bundle_sha256",
        ],
        ("business-revoke", "oo.release-coordinator/business-revoke/v1") => {
            &["protocol", "policy_sha256"]
        }
        ("plan", "oo.release-coordinator/plan/v1")
        | ("activate", "oo.release-coordinator/activate/v1") => &[
            "protocol",
            "project_id",
            "candidate_id",
            "source_hash",
            "scope",
            "expected_version",
        ],
        ("recover", "oo.release-coordinator/recover/v1") => &[
            "protocol",
            "action",
            "project_id",
            "candidate_id",
            "expected_version",
        ],
        ("maintain-install", "oo.release-coordinator/maintain-install/v1") => {
            &["protocol", "project_id"]
        }
        ("maintain-activate", "oo.release-coordinator/maintain-activate/v1")
        | ("maintain-recover", "oo.release-coordinator/maintain-recover/v1") => &[
            "protocol",
            "project_id",
            "candidate_id",
            "source_hash",
            "expected_version",
            "schema_name",
            "relation_name",
            "role_name",
        ],
        ("maintain-activate", "oo.release-coordinator/maintain-native-activate/v1")
        | ("maintain-recover", "oo.release-coordinator/maintain-native-recover/v1") => &[
            "protocol",
            "project_id",
            "candidate_id",
            "source_hash",
            "expected_version",
            "schema_name",
            "relation_name",
            "role_name",
            "native_mapping",
        ],
        ("maintain-data-only", "oo.release-coordinator/maintain-data-only/v1") => &[
            "protocol", "project_id", "candidate_id", "source_hash", "expected_version",
            "schema_name", "relation_name", "role_name", "native_mapping",
            "parent_candidate_id", "parent_source_hash", "parent_mapping_sha256",
        ],
        ("target-install", "oo.release-coordinator/target-install/v1")
        | ("target-close", "oo.release-coordinator/target-close/v1")
        | ("target-revoke", "oo.release-coordinator/target-revoke/v1") => {
            &["protocol", "schema_name", "relation_name", "role_name"]
        }
        ("publish-retained", "oo.release-coordinator/publish-retained/v1") => &[
            "protocol",
            "library_id",
            "generation",
            "login",
            "reviewed_registry",
        ],
        ("target-grant", "oo.release-coordinator/target-grant/v1")
        | ("target-observe", "oo.release-coordinator/target-observe/v1") => &[
            "protocol",
            "schema_name",
            "relation_name",
            "role_name",
            "columns",
        ],
        ("target-open", "oo.release-coordinator/target-open/v1") => &[
            "protocol",
            "schema_name",
            "relation_name",
            "role_name",
            "columns",
            "pack_digest",
            "version",
            "candidate_id",
        ],
        ("target-status", "oo.release-coordinator/target-status/v1") => &[
            "protocol",
            "schema_name",
            "relation_name",
            "role_name",
            "pointer_pack",
        ],
        _ => return malformed("protocol"),
    };
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return malformed("field");
    }
    let config = match read_config(Path::new(config_path(args)), args[0].as_str()) {
        Ok(config) => config,
        Err(reason) => return malformed(reason),
    };
    let endpoint = Endpoint {
        pack_ceilings: config.pack_ceilings.clone(),
        host: config.host.clone(),
        port: config.port,
        database: config.database.clone(),
    };
    let result = match (args[0].as_str(), protocol) {
        (command, _) if command.starts_with("business-") => {
            Box::pin(business_command(command, &endpoint, &config, map)).await
        }
        ("install", "oo.release-coordinator/install/v1") => {
            install(
                &endpoint,
                &config.user,
                &config.password,
                &config.ca_pem,
                map,
            )
            .await
        }
        ("stage", "oo.release-coordinator/stage/v1") => {
            stage(
                &endpoint,
                &config.user,
                &config.password,
                &config.ca_pem,
                map,
            )
            .await
        }
        ("status", "oo.release-coordinator/status/v1") => {
            status(
                &endpoint,
                &config.user,
                &config.password,
                &config.ca_pem,
                map,
            )
            .await
        }
        ("gate-record", "oo.release-coordinator/gate-record/v1") => {
            gate(
                &endpoint,
                &config.user,
                &config.password,
                &config.ca_pem,
                map,
            )
            .await
        }
        ("gate-record", "oo.release-coordinator/checkpoint/v1") => {
            checkpoint(
                &endpoint,
                &config.user,
                &config.password,
                &config.ca_pem,
                map,
            )
            .await
        }
        ("plan", "oo.release-coordinator/plan/v1") => {
            plan(
                &endpoint,
                &config.user,
                &config.password,
                &config.ca_pem,
                map,
            )
            .await
        }
        ("activate", "oo.release-coordinator/activate/v1") => {
            activate(
                &endpoint,
                &config.user,
                &config.password,
                &config.ca_pem,
                map,
            )
            .await
        }
        ("recover", "oo.release-coordinator/recover/v1") => {
            recover(
                &endpoint,
                &config.user,
                &config.password,
                &config.ca_pem,
                map,
            )
            .await
        }
        ("maintain-install", "oo.release-coordinator/maintain-install/v1") => {
            maintain_install_cmd(&config, map).await
        }
        ("maintain-activate", "oo.release-coordinator/maintain-activate/v1")
        | ("maintain-activate", "oo.release-coordinator/maintain-native-activate/v1")
        | ("maintain-data-only", "oo.release-coordinator/maintain-data-only/v1") => {
            maintain_cmd(&config, map, true).await
        }
        ("maintain-recover", "oo.release-coordinator/maintain-recover/v1")
        | ("maintain-recover", "oo.release-coordinator/maintain-native-recover/v1") => {
            maintain_cmd(&config, map, false).await
        }
        ("target-install", "oo.release-coordinator/target-install/v1") => {
            target_simple(&config, map, "install").await
        }
        ("target-close", "oo.release-coordinator/target-close/v1") => {
            target_simple(&config, map, "close").await
        }
        ("target-revoke", "oo.release-coordinator/target-revoke/v1") => {
            target_simple(&config, map, "revoke").await
        }
        ("target-grant", "oo.release-coordinator/target-grant/v1") => {
            target_columns(&config, map, "grant").await
        }
        ("target-observe", "oo.release-coordinator/target-observe/v1") => {
            target_columns(&config, map, "observe").await
        }
        ("target-open", "oo.release-coordinator/target-open/v1") => {
            target_open_cmd(&config, map).await
        }
        ("target-status", "oo.release-coordinator/target-status/v1") => {
            target_status_cmd(&config, map).await
        }
        ("publish-retained", "oo.release-coordinator/publish-retained/v1") => {
            publish_retained_cmd(&config, map).await
        }
        _ => return malformed("protocol"),
    };
    match result {
        Ok(receipt) => {
            let bytes = match serde_json::to_vec(&receipt.body) {
                Ok(bytes) => bytes,
                Err(_) => return malformed("json"),
            };
            let stderr = if receipt.code == 2 {
                let reason = receipt
                    .body
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("refused");
                format!("release-coordinator: refused {reason}\n")
            } else {
                String::new()
            };
            CommandExit {
                code: i32::from(receipt.code),
                stdout: bytes,
                stderr: stderr.into_bytes(),
            }
        }
        Err(error) => CommandExit {
            code: 1,
            stdout: Vec::new(),
            stderr: format!("release-coordinator: error {error}\n").into_bytes(),
        },
    }
}

fn known_command(command: &str) -> bool {
    matches!(
        command,
        "install"
            | "business-install"
            | "business-execute"
            | "business-review"
            | "business-revoke"
            | "stage"
            | "status"
            | "gate-record"
            | "plan"
            | "activate"
            | "recover"
            | "maintain-install"
            | "maintain-activate"
            | "maintain-data-only"
            | "maintain-recover"
            | "target-install"
            | "target-close"
            | "target-grant"
            | "target-observe"
            | "target-open"
            | "target-revoke"
            | "target-status"
            | "publish-retained"
    )
}

struct LoadedTarget {
    host: String,
    port: u16,
    database: String,
    user: String,
    password: String,
    ca_pem: Vec<u8>,
    reader_user: String,
    reader_password: String,
}

struct Config {
    business_policy: Option<(String, String)>,
    business_actors: std::collections::BTreeMap<String, Login>,
    pack_ceilings: Option<oo_builder::RolePackCeilings>,
    deployment_mode: crate::DeploymentMode,
    host: String,
    port: u16,
    database: String,
    user: String,
    password: String,
    ca_pem: Vec<u8>,
    reader_user: Option<String>,
    reader_password: Option<String>,
    target: Option<LoadedTarget>,
    replica: Option<Login>,
    quality: Option<Login>,
}

fn config_path(args: &[String]) -> &str {
    match args {
        [_, flag, path] if flag == "--config" => path,
        [_, flag] => flag.strip_prefix("--config=").unwrap_or(""),
        _ => "",
    }
}

/// The transport envelope follows the trusted installation's document capacity.
/// Legacy commands retain their original 8 MiB bound.
pub fn stdin_limit(args: &[String]) -> Result<usize, &'static str> {
    if let Some(reason) = argv_malformed(args) {
        return Err(reason);
    }
    let config = read_config(Path::new(config_path(args)), &args[0])?;
    let configured = config.pack_ceilings.as_ref().map_or(0, |c| c.json_scan.max_bytes);
    if configured > 268_435_456 {
        return Err("config");
    }
    Ok(configured.max(8_388_608))
}

fn read_config(path: &Path, command: &str) -> Result<Config, &'static str> {
    let bytes = read_limited(path, 8_388_608, "config")?;
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| "config")?;
    let map = object(&document).ok_or("config")?;
    let deployment_mode = crate::DeploymentMode::from_installed(&document)?;
    let kind = config_kind(command);
    let allowed: &[&str] = match kind {
        "maintain" => &[
            "host",
            "port",
            "database",
            "user",
            "password_file",
            "ca_file",
            "target_host",
            "target_port",
            "target_database",
            "target_user",
            "target_password_file",
            "target_ca_file",
            "target_reader_user",
            "target_reader_password_file",
        ],
        "target" => &[
            "host",
            "port",
            "database",
            "user",
            "password_file",
            "ca_file",
            "reader_user",
            "reader_password_file",
        ],
        _ => &[
            "host",
            "port",
            "database",
            "user",
            "password_file",
            "ca_file",
        ],
    };
    if map.keys().any(|key| {
        !allowed.contains(&key.as_str())
            && key != "deployment_mode"
            && key != "replica_pg"
            && key != "role_pack_ceilings"
            && key != "quality_pg"
            && !(key == "business_policy" && command == "business-install")
            && !(key == "business_actors" && command == "business-execute")
    }) || allowed.iter().any(|key| !map.contains_key(*key))
    {
        return Err("config");
    }
    let Login {
        host,
        port,
        database,
        user,
        password,
        ca_pem,
    } = load_login(
        map,
        "host",
        "port",
        "database",
        "user",
        "password_file",
        "ca_file",
    )?;
    if kind == "target" {
        if user != TARGET_ROLE {
            return Err("login_role");
        }
    } else if kind == "retained" {
        // D15 runs as the Agent DB owner that sealed the cut; the database checks ownership.
    } else if user != ADMIN_ROLE && command != "business-review" {
        return Err("login_role");
    }
    let mut reader_user = None;
    let mut reader_password = None;
    let mut target = None;
    if kind == "target" {
        reader_user = Some(owned(map, "reader_user")?);
        reader_password = Some(read_secret(Path::new(&owned(
            map,
            "reader_password_file",
        )?))?);
    }
    if kind == "maintain" {
        let target_login = load_login(
            map,
            "target_host",
            "target_port",
            "target_database",
            "target_user",
            "target_password_file",
            "target_ca_file",
        )?;
        if target_login.user != TARGET_ROLE {
            return Err("login_role");
        }
        target = Some(LoadedTarget {
            host: target_login.host,
            port: target_login.port,
            database: target_login.database,
            user: target_login.user,
            password: target_login.password,
            ca_pem: target_login.ca_pem,
            reader_user: owned(map, "target_reader_user")?,
            reader_password: read_secret(Path::new(&owned(map, "target_reader_password_file")?))?,
        });
    }
    let replica = map
        .get("replica_pg")
        .map(|value| {
            if kind != "maintain" {
                return Err("config");
            }
            let replica = value.as_object().ok_or("config")?;
            if replica.len() != 6 {
                return Err("config");
            }
            let loaded = load_login(
                replica,
                "host",
                "port",
                "database",
                "user",
                "password_file",
                "ca_file",
            )?;
            if loaded.user != TARGET_ROLE
                || target
                    .as_ref()
                    .is_none_or(|t| t.database != loaded.database)
            {
                return Err("config");
            }
            Ok(loaded)
        })
        .transpose()?;
    let quality = if let Some(value) = map.get("quality_pg") {
        if kind != "maintain" {
            return Err("config");
        }
        let q = value.as_object().ok_or("config")?;
        if q.len() != 6
            || q.keys().any(|key| {
                ![
                    "host",
                    "port",
                    "database",
                    "user",
                    "password_file",
                    "ca_file",
                ]
                .contains(&key.as_str())
            })
        {
            return Err("config");
        }
        let observed = load_login(
            q,
            "host",
            "port",
            "database",
            "user",
            "password_file",
            "ca_file",
        )?;
        let expected = target.as_ref().ok_or("config")?;
        if observed.host != expected.host
            || observed.port != expected.port
            || observed.database != expected.database
        {
            return Err("config");
        }
        Some(observed)
    } else {
        None
    };
    let business_policy = if command == "business-install" {
        let p = map
            .get("business_policy")
            .and_then(Value::as_object)
            .ok_or("config")?;
        if p.len() != 2 {
            return Err("config");
        }
        Some((owned(p, "path")?, owned(p, "sha256")?))
    } else {
        None
    };
    let mut business_actors = std::collections::BTreeMap::new();
    if command == "business-execute" {
        let actors = map
            .get("business_actors")
            .and_then(Value::as_object)
            .ok_or("config")?;
        if actors.len() != 3 {
            return Err("config");
        }
        for kind in ["executor", "oracle", "evaluator"] {
            let a = actors
                .get(kind)
                .and_then(Value::as_object)
                .ok_or("config")?;
            if a.len() != 6 {
                return Err("config");
            }
            let login = load_login(
                a,
                "host",
                "port",
                "database",
                "user",
                "password_file",
                "ca_file",
            )?;
            if login.host != host || login.port != port || login.database != database {
                return Err("config");
            }
            business_actors.insert(kind.into(), login);
        }
    }
    Ok(Config {
        business_policy,
        business_actors,
        quality,
        pack_ceilings: map.get("role_pack_ceilings").map(|v| serde_json::from_value(v.clone()).map_err(|_| "config")).transpose()?,
        replica,
        deployment_mode,
        host,
        port,
        database,
        user,
        password,
        ca_pem,
        reader_user,
        reader_password,
        target,
    })
}

async fn business_command(
    command: &str,
    endpoint: &Endpoint,
    config: &Config,
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
    let control = ControlConn {
        endpoint,
        user: &config.user,
        password: &config.password,
        ca_pem: &config.ca_pem,
    };
    let field = |key| text(map, key).map_err(|_| StoreError::Input);
    match command {
        "business-install" => {
            let (path, pin) = config.business_policy.as_ref().ok_or(StoreError::Input)?;
            crate::install_business_policy(&control, Path::new(path), pin).await
        }
        "business-review" => {
            crate::review_business_checks(
                &control,
                field("policy_sha256")?,
                field("run_id")?,
                field("execution_bundle_sha256")?,
            )
            .await
        }
        "business-revoke" => crate::revoke_business_policy(&control, field("policy_sha256")?).await,
        "business-execute" => {
            let actor = |kind: &str| -> Result<ControlConn<'_>, StoreError> {
                let a = config.business_actors.get(kind).ok_or(StoreError::Input)?;
                Ok(ControlConn {
                    endpoint,
                    user: &a.user,
                    password: &a.password,
                    ca_pem: &a.ca_pem,
                })
            };
            crate::execute_business_checks(
                &control,
                &actor("executor")?,
                &actor("oracle")?,
                &actor("evaluator")?,
                field("policy_sha256")?,
                field("run_id")?,
            )
            .await
        }
        _ => Err(StoreError::Input),
    }
}

fn config_kind(command: &str) -> &'static str {
    match command {
        "maintain-activate" | "maintain-recover" | "maintain-data-only" => "maintain",
        "target-install" | "target-close" | "target-grant" | "target-observe" | "target-open"
        | "target-revoke" | "target-status" => "target",
        "publish-retained" => "retained",
        _ => "control",
    }
}

struct Login {
    host: String,
    port: u16,
    database: String,
    user: String,
    password: String,
    ca_pem: Vec<u8>,
}

fn load_login(
    map: &Map<String, Value>,
    host_key: &str,
    port_key: &str,
    database_key: &str,
    user_key: &str,
    password_key: &str,
    ca_key: &str,
) -> Result<Login, &'static str> {
    let port = map.get(port_key).and_then(Value::as_u64).ok_or("config")?;
    let port = u16::try_from(port).map_err(|_| "config")?;
    if port == 0 {
        return Err("config");
    }
    let password = read_secret(Path::new(&owned(map, password_key)?))?;
    let ca_pem = match read_limited(Path::new(&owned(map, ca_key)?), 1_048_576, "ca_file") {
        Ok(bytes) if !bytes.is_empty() => bytes,
        _ => return Err("ca_file"),
    };
    Ok(Login {
        host: owned(map, host_key)?,
        port,
        database: owned(map, database_key)?,
        user: owned(map, user_key)?,
        password,
        ca_pem,
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
    map.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("field")
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

async fn install(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
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

async fn stage(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
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

async fn gate(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
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
        authority_statement_sha256: optional(map, "authority_statement_sha256")
            .map_err(|_| StoreError::Input)?,
    })
    .await
}

async fn checkpoint(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
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
        authority_statement_sha256: optional(map, "authority_statement_sha256")
            .map_err(|_| StoreError::Input)?,
    })
    .await
}

async fn plan(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
    scoped(endpoint, user, password, ca_pem, map, false).await
}

async fn activate(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
    scoped(endpoint, user, password, ca_pem, map, true).await
}

async fn status(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
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
    if activate {
        store::activate(&request).await
    } else {
        store::plan(&request).await
    }
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

async fn recover(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
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

fn control_of<'a>(config: &'a Config, endpoint: &'a Endpoint) -> ControlConn<'a> {
    ControlConn {
        endpoint,
        user: &config.user,
        password: &config.password,
        ca_pem: &config.ca_pem,
    }
}

fn endpoint_of(config: &Config) -> Endpoint {
    Endpoint {
        pack_ceilings: config.pack_ceilings.clone(),
        host: config.host.clone(),
        port: config.port,
        database: config.database.clone(),
    }
}

async fn maintain_install_cmd(
    config: &Config,
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
    let endpoint = endpoint_of(config);
    maintain::maintain_install(
        &control_of(config, &endpoint),
        text(map, "project_id").map_err(|_| StoreError::Input)?,
    )
    .await
}

async fn maintain_cmd(
    config: &Config,
    map: &Map<String, Value>,
    activate: bool,
) -> Result<store::Receipt, StoreError> {
    let Some(target) = &config.target else {
        return Err(StoreError::Input);
    };
    let control_endpoint = endpoint_of(config);
    let target_endpoint = Endpoint {
        pack_ceilings: None,
        host: target.host.clone(),
        port: target.port,
        database: target.database.clone(),
    };
    let replica_endpoint = config.replica.as_ref().map(|r| Endpoint {
        pack_ceilings: None,
        host: r.host.clone(),
        port: r.port,
        database: r.database.clone(),
    });
    let quality_endpoint = config.quality.as_ref().map(|q| Endpoint {
        pack_ceilings: None,
        host: q.host.clone(),
        port: q.port,
        database: q.database.clone(),
    });
    let request = MaintainRequest {
        quality_observer: config.quality.as_ref().zip(quality_endpoint.as_ref()).map(
            |(q, endpoint)| TargetConn {
                endpoint,
                user: &q.user,
                password: &q.password,
                ca_pem: &q.ca_pem,
            },
        ),
        deployment_mode: config.deployment_mode,
        replica: config
            .replica
            .as_ref()
            .zip(replica_endpoint.as_ref())
            .map(|(r, endpoint)| TargetConn {
                endpoint,
                user: &r.user,
                password: &r.password,
                ca_pem: &r.ca_pem,
            }),
        native_mapping: map.get("native_mapping"),
        control: control_of(config, &control_endpoint),
        target: TargetConn {
            endpoint: &target_endpoint,
            user: &target.user,
            password: &target.password,
            ca_pem: &target.ca_pem,
        },
        reader_user: &target.reader_user,
        reader_password: &target.reader_password,
        project_id: text(map, "project_id").map_err(|_| StoreError::Input)?,
        candidate_id: text(map, "candidate_id").map_err(|_| StoreError::Input)?,
        source_hash: text(map, "source_hash").map_err(|_| StoreError::Input)?,
        expected_version: optional(map, "expected_version").map_err(|_| StoreError::Input)?,
        schema_name: text(map, "schema_name").map_err(|_| StoreError::Input)?,
        relation_name: text(map, "relation_name").map_err(|_| StoreError::Input)?,
        role_name: text(map, "role_name").map_err(|_| StoreError::Input)?,
    };
    if map.get("protocol").and_then(Value::as_str) == Some("oo.release-coordinator/maintain-data-only/v1") {
        maintain::maintain_data_only(&request, &maintain::DataOnlyParent {
            candidate_id: text(map, "parent_candidate_id").map_err(|_| StoreError::Input)?,
            source_hash: text(map, "parent_source_hash").map_err(|_| StoreError::Input)?,
            mapping_sha256: text(map, "parent_mapping_sha256").map_err(|_| StoreError::Input)?,
        }).await
    } else if activate {
        maintain::maintain_activate(&request).await
    } else {
        maintain::maintain_recover(&request).await
    }
}

fn fence_of<'a>(map: &'a Map<String, Value>) -> Result<FenceRef<'a>, StoreError> {
    Ok(FenceRef {
        schema_name: text(map, "schema_name").map_err(|_| StoreError::Input)?,
        relation_name: text(map, "relation_name").map_err(|_| StoreError::Input)?,
        role_name: text(map, "role_name").map_err(|_| StoreError::Input)?,
    })
}

fn column_list(map: &Map<String, Value>) -> Result<Vec<String>, StoreError> {
    let Some(items) = map.get("columns").and_then(Value::as_array) else {
        return Err(StoreError::Input);
    };
    let mut columns = Vec::new();
    for item in items {
        let Some(name) = item.as_str().filter(|name| !name.is_empty()) else {
            return Err(StoreError::Input);
        };
        columns.push(name.to_owned());
    }
    Ok(columns)
}

fn target_conn<'a>(config: &'a Config, endpoint: &'a Endpoint) -> TargetConn<'a> {
    TargetConn {
        endpoint,
        user: &config.user,
        password: &config.password,
        ca_pem: &config.ca_pem,
    }
}

async fn target_simple(
    config: &Config,
    map: &Map<String, Value>,
    action: &str,
) -> Result<store::Receipt, StoreError> {
    let endpoint = endpoint_of(config);
    let conn = target_conn(config, &endpoint);
    let fence = fence_of(map)?;
    match action {
        "install" => target::install_target(&conn, &fence).await,
        "close" => target::close_target(&conn, &fence).await,
        "revoke" => target::revoke_target(&conn, &fence).await,
        _ => Err(StoreError::Input),
    }
}

async fn target_columns(
    config: &Config,
    map: &Map<String, Value>,
    action: &str,
) -> Result<store::Receipt, StoreError> {
    let endpoint = endpoint_of(config);
    let conn = target_conn(config, &endpoint);
    let fence = fence_of(map)?;
    let columns = column_list(map)?;
    match action {
        "grant" => target::grant_target(&conn, &fence, &columns).await,
        "observe" => {
            let reader_user = config.reader_user.as_deref().ok_or(StoreError::Input)?;
            let reader_password = config.reader_password.as_deref().ok_or(StoreError::Input)?;
            target::observe_target(&conn, &fence, &columns, reader_user, reader_password).await
        }
        _ => Err(StoreError::Input),
    }
}

async fn target_open_cmd(
    config: &Config,
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
    let endpoint = endpoint_of(config);
    let conn = target_conn(config, &endpoint);
    let fence = fence_of(map)?;
    target::open_target(
        &conn,
        &fence,
        &column_list(map)?,
        text(map, "pack_digest").map_err(|_| StoreError::Input)?,
        text(map, "version").map_err(|_| StoreError::Input)?,
        text(map, "candidate_id").map_err(|_| StoreError::Input)?,
    )
    .await
}

async fn publish_retained_cmd(
    config: &Config,
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
    let endpoint = endpoint_of(config);
    let conn = target_conn(config, &endpoint);
    let request = crate::RetainedRequest {
        library_id: text(map, "library_id").map_err(|_| StoreError::Input)?,
        generation: text(map, "generation").map_err(|_| StoreError::Input)?,
        login: text(map, "login").map_err(|_| StoreError::Input)?,
        reviewed: map.get("reviewed_registry").ok_or(StoreError::Input)?,
    };
    crate::publish_reviewed_registry(&conn, &request).await
}

async fn target_status_cmd(
    config: &Config,
    map: &Map<String, Value>,
) -> Result<store::Receipt, StoreError> {
    let endpoint = endpoint_of(config);
    let conn = target_conn(config, &endpoint);
    let fence = fence_of(map)?;
    let pointer_pack = match map.get("pointer_pack") {
        Some(Value::Null) => None,
        Some(Value::String(value)) if crate::seal::hex64(value) => Some(value.as_str()),
        _ => return Err(StoreError::Input),
    };
    target::status_target(&conn, &fence, pointer_pack, None).await
}

#[cfg(test)]
mod tests {
    use super::{argv_malformed, dispatch};
    use serde_json::json;

    #[test]
    fn installed_unknown_mode_and_replica_conflict_are_explicit() {
        let path =
            std::env::temp_dir().join(format!("oo-wf-allinone-config-{}.json", std::process::id()));
        for (document, expected) in [
            (r#"{"deployment_mode":"unknown"}"#, "deployment_mode"),
            (
                r#"{"deployment_mode":"all_in_one","replica_pg":{}}"#,
                "all_in_one_replica_conflict",
            ),
        ] {
            std::fs::write(&path, document).unwrap();
            assert_eq!(
                super::read_config(&path, "maintain-activate").err(),
                Some(expected)
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn caller_cannot_select_deployment_mode() {
        let request = json!({"protocol":"oo.release-coordinator/status/v1","project_id":"shop","candidate_id":"candidate","source_hash":"aa".repeat(32),"deployment_mode":"all_in_one"});
        let result = dispatch(
            &[
                "status".to_owned(),
                "--config".to_owned(),
                "not-read.json".to_owned(),
            ],
            &serde_json::to_vec(&request).unwrap(),
        )
        .await;
        assert_eq!(result.code, 1);
        assert_eq!(
            String::from_utf8(result.stderr).unwrap(),
            "release-coordinator: malformed field\n"
        );
    }

    #[test]
    fn argv_requires_one_config_path() {
        assert_eq!(argv_malformed(&[]), Some("missing_command"));
        assert_eq!(argv_malformed(&["stage".to_owned()]), Some("config"));
        assert_eq!(
            argv_malformed(&[
                "nope".to_owned(),
                "--config".to_owned(),
                "c.json".to_owned()
            ]),
            Some("missing_command")
        );
        assert_eq!(
            argv_malformed(&[
                "stage".to_owned(),
                "--config".to_owned(),
                "c.json".to_owned(),
                "extra".to_owned()
            ]),
            Some("extra_arg")
        );
        assert_eq!(
            argv_malformed(&["activate".to_owned(), "--config=c.json".to_owned()]),
            None
        );
        assert_eq!(
            argv_malformed(&[
                "maintain-activate".to_owned(),
                "--config".to_owned(),
                "c.json".to_owned()
            ]),
            None
        );
        assert_eq!(
            argv_malformed(&[
                "target-status".to_owned(),
                "--config".to_owned(),
                "c.json".to_owned()
            ]),
            None
        );
    }

    #[tokio::test]
    async fn waiver_and_product_mode_do_not_write() {
        let args = vec![
            "plan".to_owned(),
            "--config".to_owned(),
            "unused.json".to_owned(),
        ];
        let force = dispatch(
            &args,
            br#"{"protocol":"oo.release-coordinator/plan/v1","force":true}"#,
        )
        .await;
        assert_eq!(force.code, 1);
        assert!(force.stdout.is_empty());
        assert!(force.stderr.windows(6).any(|window| window == b"waiver"));
        let maintain_args = vec![
            "maintain-activate".to_owned(),
            "--config".to_owned(),
            "unused.json".to_owned(),
        ];
        let maintain_force = dispatch(
            &maintain_args,
            br#"{"protocol":"oo.release-coordinator/maintain-activate/v1","force":true}"#,
        )
        .await;
        assert_eq!(maintain_force.code, 1);
        assert!(maintain_force.stdout.is_empty());
        let product = dispatch(
            &args,
            br#"{"protocol":"oo.release-coordinator/plan/v1","product_mode_can_activate":true}"#,
        )
        .await;
        assert_eq!(product.code, 1);
        assert!(product.stdout.is_empty());
        assert!(
            std::str::from_utf8(&product.stderr)
                .unwrap()
                .contains("product_mode_can_activate")
        );
        let unknown = dispatch(&args, br#"{"protocol":"oo.release-coordinator/plan/v1","project_id":"shop","candidate_id":"cand-a","source_hash":"ab","scope":"test_scope","expected_version":null,"extra":1}"#).await;
        assert_eq!(unknown.code, 1);
        assert!(unknown.stdout.is_empty());
    }

    #[tokio::test]
    async fn native_maintenance_protocols_reach_config_validation() {
        for (command, protocol) in [
            (
                "maintain-activate",
                "oo.release-coordinator/maintain-native-activate/v1",
            ),
            (
                "maintain-recover",
                "oo.release-coordinator/maintain-native-recover/v1",
            ),
        ] {
            let source_hash = "a".repeat(64);
            let args = vec![
                command.to_owned(),
                "--config".to_owned(),
                "missing-native-maintain-config.json".to_owned(),
            ];
            let request = json!({
                "protocol": protocol,
                "project_id": "oracle",
                "candidate_id": "candidate-1",
                "source_hash": source_hash,
                "expected_version": null,
                "schema_name": "schema_one",
                "relation_name": "table_one",
                "role_name": "reader_role",
                "native_mapping": {},
            });
            let result = dispatch(&args, request.to_string().as_bytes()).await;
            assert_eq!(result.code, 1);
            assert!(result.stdout.is_empty());
            assert!(String::from_utf8(result.stderr).unwrap().contains("malformed config"));
        }
    }
}
