//! Optional call to the fixed `local-pg-mirror` binary.
//!
//! A missing sidecar means this command did not invoke the copy producer.
//! `cache_replay` passes only non-secret paths. A complete manifest returns
//! before the producer opens credentials. `fresh_complete` passes private
//! file paths, never secret bytes, and drops those paths before the result is
//! recorded. A fresh sidecar that names a source config runs `discover` into
//! a new plan, then `copy` into a new manifest. The two provenances are not
//! interchangeable.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Map, Value};

use crate::config::{Config, ConfigError};

const PROTOCOL: &str = "oo.minimal-product-loop.copy-invoke/v1";
const ABSENT: &str = "/absent-not-a-secret";

pub fn invoke(config_path: &Path) -> Result<Value, ConfigError> {
    let sidecar = config_path
        .parent()
        .ok_or_else(|| ConfigError::new("copy.producer", "the config directory is absent"))?
        .join("mirror-invoke.json");
    if !sidecar.is_file() {
        return Ok(json!({"invoked": false, "reason": "sidecar_absent"}));
    }
    let mut spec = load_sidecar(&sidecar)?;
    let discover = spec.source_config.is_some();
    if spec.provenance == "cache_replay" {
        let _ = read_manifest(&spec.manifest, &spec.import_hash)?;
    }
    if discover {
        if spec.plan.is_file() && spec.manifest.is_file() && spec.target_config.is_file() {
            spec.secrets.take();
            return confirm_sealed(&spec);
        }
        absent(&spec.plan)?;
        absent(&spec.manifest)?;
        absent(&spec.target_config)?;
        let discovered = run_discover(&spec)?;
        if !discovered.status.success() {
            return Err(ConfigError::new("copy.discover", "the copy producer did not admit a plan"));
        }
        discover_line(&discovered.stdout)?;
    }
    let output = run_copy(&spec)?;
    spec.secrets.take();
    let line = producer_line(&output.stdout)?;
    if !output.status.success() {
        let message = if spec.provenance == "cache_replay" {
            "the copy producer refused the sealed manifest"
        } else {
            "the copy producer refused the fresh copy"
        };
        return Err(ConfigError::new("copy.producer", message));
    }
    let replay = line.split(' ').any(|field| field == "replay_complete");
    if spec.provenance == "cache_replay" {
        if !replay {
            return Err(ConfigError::new("copy.replay", "the copy producer did not replay a sealed manifest"));
        }
        let manifest = read_manifest(&spec.manifest, &spec.import_hash)?;
        return Ok(receipt(
            &line,
            "cache_replay",
            &spec.import_hash,
            &manifest.snapshot_id,
            false,
            json!([{
                "name": "copy",
                "event": "replay_complete",
                "from": "sealed_manifest",
                "failure": "replay_complete",
                "import_hash": spec.import_hash,
                "snapshot_id": manifest.snapshot_id
            }]),
        ));
    }
    if replay {
        return Err(ConfigError::new("copy.stale", "a fresh copy found a complete manifest"));
    }
    if line.split(' ').nth(3) != Some("none") {
        return Err(ConfigError::new("copy.fresh", "the fresh copy did not finish"));
    }
    let manifest = read_completed(&spec.manifest)?;
    let rows = line.split(' ').nth(1).and_then(|text| text.parse::<i64>().ok()).unwrap_or(0);
    let generation = manifest_generation(&spec.manifest).unwrap_or(0);
    Ok(receipt(&line, "fresh_complete", &manifest.import_hash, &manifest.snapshot_id, discover, fresh_phases(discover, &manifest.import_hash, &manifest.snapshot_id, rows, generation)))
}

fn confirm_sealed(spec: &Spec) -> Result<Value, ConfigError> {
    let output = run_copy(spec)?;
    let line = producer_line(&output.stdout)?;
    if !output.status.success() || !line.split(' ').any(|field| field == "replay_complete") {
        return Err(ConfigError::new("copy.fresh", "the sealed fresh manifest was not confirmed"));
    }
    let seal = read_fresh_seal(&spec.manifest, &spec.plan)?;
    let fields: Vec<&str> = line.split(' ').collect();
    let rows_text = seal.rows.to_string();
    if fields.get(1) != Some(&rows_text.as_str()) || fields.get(2) != Some(&seal.plan_sha256.as_str()) {
        return Err(ConfigError::new("copy.fresh", "the sealed confirmation did not match the manifest"));
    }
    let mut phases = fresh_phases(true, &seal.import_hash, &seal.snapshot_id, seal.rows, seal.generation);
    let Some(list) = phases.as_array_mut() else {
        return Err(ConfigError::new("copy.producer", "the sealed phases could not be recorded"));
    };
    list.push(json!({
        "name": "copy",
        "event": "replay_complete",
        "from": "sealed_manifest",
        "failure": "replay_complete",
        "import_hash": seal.import_hash,
        "snapshot_id": seal.snapshot_id
    }));
    Ok(receipt(&line, "fresh_complete", &seal.import_hash, &seal.snapshot_id, false, phases))
}

fn run_copy(spec: &Spec) -> Result<std::process::Output, ConfigError> {
    let mut command = Command::new(&spec.binary);
    command.arg("copy").arg(&spec.plan);
    if let Some(secrets) = &spec.secrets {
        command.arg(&secrets.source_password).arg(&secrets.source_ca).arg(&secrets.admin_password).arg(&secrets.runtime_password);
    } else {
        command.arg(ABSENT).arg(ABSENT).arg(ABSENT).arg(ABSENT);
    }
    command.arg(&spec.manifest).arg(&spec.target_config);
    if let Some(secrets) = &spec.secrets {
        command.arg(&secrets.target_ca);
    } else {
        command.arg(ABSENT);
    }
    command.arg("finish");
    quiet(&mut command)
}

fn run_discover(spec: &Spec) -> Result<std::process::Output, ConfigError> {
    let secrets = spec.secrets.as_ref().ok_or_else(|| ConfigError::new("copy.secret", "a secret path is absent"))?;
    let source = spec.source_config.as_ref().ok_or_else(|| ConfigError::new("copy.discover", "the source config is absent"))?;
    let mut command = Command::new(&spec.binary);
    command.arg("discover").arg(source).arg(&spec.plan).arg(&secrets.source_password).arg(&secrets.source_ca);
    quiet(&mut command)
}

fn quiet(command: &mut Command) -> Result<std::process::Output, ConfigError> {
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|_| ConfigError::new("copy.producer", "the copy producer could not be started"))
}

struct FreshSecrets {
    source_password: PathBuf,
    source_ca: PathBuf,
    admin_password: PathBuf,
    runtime_password: PathBuf,
    target_ca: PathBuf,
}

pub(crate) struct Spec {
    pub(crate) binary: PathBuf,
    pub(crate) plan: PathBuf,
    pub(crate) manifest: PathBuf,
    pub(crate) target_config: PathBuf,
    pub(crate) import_hash: String,
    pub(crate) provenance: &'static str,
    source_config: Option<PathBuf>,
    admin_user: String,
    secrets: Option<FreshSecrets>,
}

pub(crate) struct FreshAdmin {
    pub user: String,
    pub password: PathBuf,
    pub ca: PathBuf,
}

pub(crate) fn fresh_admin(config_path: &Path) -> Result<Option<FreshAdmin>, ConfigError> {
    let spec = open_sidecar(config_path)?;
    if spec.source_config.is_none() {
        return Ok(None);
    }
    let secrets = spec.secrets.ok_or_else(|| ConfigError::new("copy.secret", "a secret path is absent"))?;
    if !role_ident(&spec.admin_user) {
        return Err(ConfigError::new("copy.producer", "the admin user is not allowed"));
    }
    Ok(Some(FreshAdmin { user: spec.admin_user, password: secrets.admin_password, ca: secrets.target_ca }))
}

pub(crate) fn require_target(config: &Config, config_path: &Path) -> Result<(), ConfigError> {
    let spec = open_sidecar(config_path)?;
    target_names_match(&config.host, config.port, &config.database, &config.username, &spec.target_config)
}

pub(crate) fn target_names_match(host: &str, port: u16, database: &str, user: &str, path: &Path) -> Result<(), ConfigError> {
    let bytes = fs::read(path).map_err(|_| ConfigError::new("copy.target", "the target config could not be read"))?;
    if bytes.len() > 65_536 {
        return Err(ConfigError::new("copy.target", "the target config is not allowed"));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ConfigError::new("copy.target", "the target config is not JSON"))?;
    let map = value.as_object().ok_or_else(|| ConfigError::new("copy.target", "the target config is not one object"))?;
    if map.get("protocol").and_then(Value::as_str) != Some("oo.pipeline.local-pg-mirror-target/v1") {
        return Err(ConfigError::new("copy.target", "the target config protocol does not match"));
    }
    if map.get("tls").and_then(Value::as_str) != Some("verify-full") {
        return Err(ConfigError::new("copy.target", "the target config is not verify-full"));
    }
    let same = map.get("host").and_then(Value::as_str) == Some(host)
        && map.get("database").and_then(Value::as_str) == Some(database)
        && map.get("user").and_then(Value::as_str) == Some(user)
        && map.get("port").and_then(Value::as_u64) == Some(u64::from(port));
    if same {
        Ok(())
    } else {
        Err(ConfigError::new("copy.target", "the database section does not name the copy target"))
    }
}

pub(crate) fn open_sidecar(config_path: &Path) -> Result<Spec, ConfigError> {
    let sidecar = config_path
        .parent()
        .ok_or_else(|| ConfigError::new("copy.producer", "the config directory is absent"))?
        .join("mirror-invoke.json");
    if !sidecar.is_file() {
        return Err(ConfigError::new("copy.required", "the sealed copy sidecar is absent"));
    }
    load_sidecar(&sidecar)
}

struct ManifestFact {
    import_hash: String,
    snapshot_id: String,
}

fn receipt(line: &str, provenance: &str, import_hash: &str, snapshot_id: &str, discover: bool, phases: Value) -> Value {
    let disposition = if line.split(' ').any(|field| field == "replay_complete") {
        "replay_complete"
    } else {
        "fresh_complete"
    };
    let origin = if provenance == "cache_replay" { "cache_replay" } else { "fresh" };
    json!({
        "invoked": true,
        "provenance": provenance,
        "discover": discover,
        "line": line,
        "import_hash": import_hash,
        "snapshot_id": snapshot_id,
        "copy_origin": origin,
        "producer_disposition": disposition,
        "phases": phases,
        "customer_complete": false,
        "parent_acceptance": false,
        "goal_acceptance": false
    })
}

fn fresh_phases(discover: bool, import_hash: &str, snapshot_id: &str, rows: i64, generation: i64) -> Value {
    let mut phases = Vec::new();
    if discover {
        phases.push(json!({"name": "discover", "event": "begin", "from": "source_connection", "failure": "none"}));
    }
    phases.push(json!({"name": "copy", "event": "begin", "from": "source_connection", "failure": "none"}));
    phases.push(json!({
        "name": "copy",
        "event": "complete",
        "from": "source_connection",
        "failure": "none",
        "mirror_state": "complete",
        "customer_complete": false,
        "import_hash": import_hash,
        "snapshot_id": snapshot_id,
        "rows": rows,
        "import_generation": generation
    }));
    Value::Array(phases)
}

fn load_sidecar(path: &Path) -> Result<Spec, ConfigError> {
    let bytes = fs::read(path).map_err(|_| ConfigError::new("copy.producer", "the copy sidecar could not be read"))?;
    if bytes.len() > 8192 {
        return Err(ConfigError::new("copy.producer", "the copy sidecar is not allowed"));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ConfigError::new("copy.producer", "the copy sidecar is not JSON"))?;
    let map = value.as_object().ok_or_else(|| ConfigError::new("copy.producer", "the copy sidecar is not one object"))?;
    if map.get("protocol").and_then(Value::as_str) != Some(PROTOCOL) {
        return Err(ConfigError::new("copy.producer", "the copy sidecar protocol does not match"));
    }
    if map.contains_key("mode") {
        if map.contains_key("source_config") {
            return load_discover(map);
        }
        return load_fresh(map);
    }
    require(map, &["binary", "import_hash", "manifest", "plan", "protocol", "target_config"])?;
    let import_hash = map.get("import_hash").and_then(Value::as_str).unwrap_or("");
    if !hex64(import_hash) {
        return Err(ConfigError::new("copy.producer", "the import hash is not allowed"));
    }
    Ok(Spec {
        binary: file_path(map, "binary")?,
        plan: file_path(map, "plan")?,
        manifest: file_path(map, "manifest")?,
        target_config: file_path(map, "target_config")?,
        import_hash: import_hash.to_owned(),
        provenance: "cache_replay",
        source_config: None,
        admin_user: String::new(),
        secrets: None,
    })
}

fn load_discover(map: &Map<String, Value>) -> Result<Spec, ConfigError> {
    require(map, &[
        "admin_password",
        "admin_user",
        "binary",
        "manifest",
        "mode",
        "plan",
        "protocol",
        "runtime_password",
        "source_ca",
        "source_config",
        "source_password",
        "target_ca",
        "target_config",
    ])?;
    if map.get("mode").and_then(Value::as_str) != Some("fresh") {
        return Err(ConfigError::new("copy.producer", "the copy mode is not allowed"));
    }
    let admin_user = map.get("admin_user").and_then(Value::as_str).unwrap_or("");
    if !role_ident(admin_user) {
        return Err(ConfigError::new("copy.producer", "the admin user is not allowed"));
    }
    let manifest = path_syntax(map, "manifest")?;
    let import_hash = if manifest.is_file() { read_completed(&manifest)?.import_hash } else { String::new() };
    Ok(Spec {
        binary: file_path(map, "binary")?,
        plan: path_syntax(map, "plan")?,
        manifest,
        target_config: path_syntax(map, "target_config")?,
        import_hash,
        provenance: "fresh_complete",
        source_config: Some(file_path(map, "source_config")?),
        admin_user: admin_user.to_owned(),
        secrets: Some(fresh_secrets(map)?),
    })
}

fn load_fresh(map: &Map<String, Value>) -> Result<Spec, ConfigError> {
    require(map, &[
        "admin_password",
        "binary",
        "manifest",
        "mode",
        "plan",
        "protocol",
        "runtime_password",
        "source_ca",
        "source_password",
        "target_ca",
        "target_config",
    ])?;
    if map.get("mode").and_then(Value::as_str) != Some("fresh") {
        return Err(ConfigError::new("copy.producer", "the copy mode is not allowed"));
    }
    let manifest = path_syntax(map, "manifest")?;
    let import_hash = if manifest.is_file() {
        read_completed(&manifest)?.import_hash
    } else {
        String::new()
    };
    Ok(Spec {
        binary: file_path(map, "binary")?,
        plan: file_path(map, "plan")?,
        manifest,
        target_config: file_path(map, "target_config")?,
        import_hash,
        provenance: "fresh_complete",
        source_config: None,
        admin_user: String::new(),
        secrets: Some(fresh_secrets(map)?),
    })
}

fn fresh_secrets(map: &Map<String, Value>) -> Result<FreshSecrets, ConfigError> {
    Ok(FreshSecrets {
        source_password: secret_file(map, "source_password")?,
        source_ca: cert_file(map, "source_ca")?,
        admin_password: secret_file(map, "admin_password")?,
        runtime_password: secret_file(map, "runtime_password")?,
        target_ca: cert_file(map, "target_ca")?,
    })
}

fn read_manifest(path: &Path, import_hash: &str) -> Result<ManifestFact, ConfigError> {
    let bytes = fs::read(path).map_err(|_| ConfigError::new("copy.producer", "the sealed manifest could not be read"))?;
    if bytes.len() > 1_048_576 {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is not allowed"));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ConfigError::new("copy.producer", "the sealed manifest is not JSON"))?;
    let map = value.as_object().ok_or_else(|| ConfigError::new("copy.producer", "the sealed manifest is not one object"))?;
    if map.get("import_hash").and_then(Value::as_str) != Some(import_hash) {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is a different import"));
    }
    if map.get("mirror_state").and_then(Value::as_str) != Some("complete") {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is not complete"));
    }
    if map.get("customer_complete").and_then(Value::as_bool) != Some(false) {
        return Err(ConfigError::new("copy.producer", "customer completion is not claimed"));
    }
    let snapshot_id = map.get("snapshot_id").and_then(Value::as_str).unwrap_or("");
    if snapshot_id.is_empty() || snapshot_id.len() > 64 || snapshot_id.chars().any(|ch| ch.is_whitespace()) {
        return Err(ConfigError::new("copy.producer", "the snapshot id is not allowed"));
    }
    Ok(ManifestFact { import_hash: import_hash.to_owned(), snapshot_id: snapshot_id.to_owned() })
}

struct FreshSeal {
    import_hash: String,
    snapshot_id: String,
    plan_sha256: String,
    rows: i64,
    generation: i64,
}

fn manifest_generation(path: &Path) -> Option<i64> {
    let bytes = fs::read(path).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.get("import_generation").and_then(Value::as_i64)
}

fn read_fresh_seal(manifest_path: &Path, plan_path: &Path) -> Result<FreshSeal, ConfigError> {
    let fact = read_completed(manifest_path)?;
    let bytes = fs::read(manifest_path).map_err(|_| ConfigError::new("copy.producer", "the sealed manifest could not be read"))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ConfigError::new("copy.producer", "the sealed manifest is not JSON"))?;
    let map = value.as_object().ok_or_else(|| ConfigError::new("copy.producer", "the sealed manifest is not one object"))?;
    let plan_bytes = fs::read(plan_path).map_err(|_| ConfigError::new("copy.producer", "the sealed plan could not be read"))?;
    if plan_bytes.len() > 1_048_576 {
        return Err(ConfigError::new("copy.producer", "the sealed plan is not allowed"));
    }
    let plan_value: Value = serde_json::from_slice(&plan_bytes).map_err(|_| ConfigError::new("copy.producer", "the sealed plan is not JSON"))?;
    let plan = plan_value.as_object().ok_or_else(|| ConfigError::new("copy.producer", "the sealed plan is not one object"))?;
    let plan_sha256 = plan.get("plan_sha256").and_then(Value::as_str).unwrap_or("");
    let generation = plan.get("generation").and_then(Value::as_i64).filter(|value| *value >= 1).ok_or_else(|| ConfigError::new("copy.producer", "the sealed plan generation is not allowed"))?;
    if !hex64(plan_sha256) || map.get("plan_sha256").and_then(Value::as_str) != Some(plan_sha256) {
        return Err(ConfigError::new("copy.producer", "the sealed manifest does not match the plan"));
    }
    // The manifest generation is the target adopt counter. The plan generation is the mapping generation.
    if map.get("import_generation").and_then(Value::as_i64).filter(|value| *value >= 1).is_none() {
        return Err(ConfigError::new("copy.producer", "the sealed target generation is not allowed"));
    }
    if map.get("failure").and_then(Value::as_str) != Some("none") || map.get("source_socket_fact").and_then(Value::as_str) != Some("repeatable_read_read_only") {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is not a fresh source copy"));
    }
    let rows = map.get("rows").and_then(Value::as_i64).filter(|value| *value >= 0).ok_or_else(|| ConfigError::new("copy.producer", "the sealed row count is not allowed"))?;
    let peak = map.get("source_socket_peak").and_then(Value::as_u64);
    let writes = map.get("source_write_tuples").and_then(Value::as_u64);
    let fresh_source = peak == Some(1)
        && writes == Some(0)
        && map.get("source_ddl").and_then(Value::as_bool) == Some(false)
        && map.get("source_count_star").and_then(Value::as_bool) == Some(false)
        && map.get("source_explain").and_then(Value::as_bool) == Some(false)
        && map.get("source_read_only").and_then(Value::as_bool) == Some(true);
    if !fresh_source {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is not a fresh source copy"));
    }
    Ok(FreshSeal {
        import_hash: fact.import_hash,
        snapshot_id: fact.snapshot_id,
        plan_sha256: plan_sha256.to_owned(),
        rows,
        generation,
    })
}

fn read_completed(path: &Path) -> Result<ManifestFact, ConfigError> {
    let bytes = fs::read(path).map_err(|_| ConfigError::new("copy.producer", "the sealed manifest could not be read"))?;
    if bytes.len() > 1_048_576 {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is not allowed"));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ConfigError::new("copy.producer", "the sealed manifest is not JSON"))?;
    let map = value.as_object().ok_or_else(|| ConfigError::new("copy.producer", "the sealed manifest is not one object"))?;
    let import_hash = map.get("import_hash").and_then(Value::as_str).unwrap_or("");
    if !hex64(import_hash) {
        return Err(ConfigError::new("copy.producer", "the import hash is not allowed"));
    }
    if map.get("mirror_state").and_then(Value::as_str) != Some("complete") {
        return Err(ConfigError::new("copy.producer", "the sealed manifest is not complete"));
    }
    if map.get("customer_complete").and_then(Value::as_bool) != Some(false) {
        return Err(ConfigError::new("copy.producer", "customer completion is not claimed"));
    }
    let snapshot_id = map.get("snapshot_id").and_then(Value::as_str).unwrap_or("");
    if snapshot_id.is_empty() || snapshot_id.len() > 64 || snapshot_id.chars().any(|ch| ch.is_whitespace()) {
        return Err(ConfigError::new("copy.producer", "the snapshot id is not allowed"));
    }
    Ok(ManifestFact { import_hash: import_hash.to_owned(), snapshot_id: snapshot_id.to_owned() })
}

fn discover_line(stdout: &[u8]) -> Result<String, ConfigError> {
    if stdout.len() > 256 {
        return Err(ConfigError::new("copy.discover", "the discover line was withheld"));
    }
    let text = std::str::from_utf8(stdout).map_err(|_| ConfigError::new("copy.discover", "the discover line was withheld"))?;
    let line = text.trim();
    let fields: Vec<&str> = line.split(' ').collect();
    if fields.len() != 5 || fields[0] != "planned" || fields[1] != "0" || !hex64(fields[2]) || fields[3] != "none" || fields[4] != "false" {
        return Err(ConfigError::new("copy.discover", "the discover line was withheld"));
    }
    Ok(line.to_owned())
}

fn absent(path: &Path) -> Result<(), ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(ConfigError::new("copy.fresh", "a fresh copy path already exists")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ConfigError::new("copy.fresh", "a fresh copy path is not allowed")),
    }
}

fn role_ident(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if first != b'_' && !first.is_ascii_lowercase() {
        return false;
    }
    bytes[1..].iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn producer_line(stdout: &[u8]) -> Result<String, ConfigError> {
    if stdout.len() > 256 {
        return Err(ConfigError::new("copy.producer", "the copy producer output was withheld"));
    }
    let text = std::str::from_utf8(stdout).map_err(|_| ConfigError::new("copy.producer", "the copy producer output was withheld"))?;
    let line = text.trim();
    let fields: Vec<&str> = line.split(' ').collect();
    if fields.len() != 5 || fields[0] != "complete" || fields[4] != "false" || !fields[1].chars().all(|ch| ch.is_ascii_digit()) || !hex64(fields[2]) {
        return Err(ConfigError::new("copy.producer", "the copy producer output was withheld"));
    }
    if fields[3] != "replay_complete" && fields[3] != "none" {
        return Err(ConfigError::new("copy.producer", "the copy producer output was withheld"));
    }
    Ok(line.to_owned())
}

fn require(map: &Map<String, Value>, keys: &[&str]) -> Result<(), ConfigError> {
    if map.len() != keys.len() || keys.iter().any(|key| !map.contains_key(*key)) {
        return Err(ConfigError::new("copy.producer", "the copy sidecar keys do not match"));
    }
    Ok(())
}

fn file_path(map: &Map<String, Value>, key: &str) -> Result<PathBuf, ConfigError> {
    let path = path_syntax(map, key)?;
    let meta = fs::symlink_metadata(&path).map_err(|_| ConfigError::new("copy.producer", "a copy path is absent"))?;
    if meta.file_type().is_file() && meta.len() > 0 {
        Ok(path)
    } else {
        Err(ConfigError::new("copy.producer", "a copy path is absent"))
    }
}

fn path_syntax(map: &Map<String, Value>, key: &str) -> Result<PathBuf, ConfigError> {
    let text = map.get(key).and_then(Value::as_str).unwrap_or("");
    if !text.starts_with('/') || text.contains('\0') || text.chars().any(|ch| ch.is_whitespace()) || text.contains("/secrets/") || text.ends_with(".pass") || text.ends_with(".key") {
        return Err(ConfigError::new("copy.producer", "a copy path is not allowed"));
    }
    Ok(PathBuf::from(text))
}

fn secret_file(map: &Map<String, Value>, key: &str) -> Result<PathBuf, ConfigError> {
    private_file(map, key, true)
}

fn cert_file(map: &Map<String, Value>, key: &str) -> Result<PathBuf, ConfigError> {
    private_file(map, key, false)
}

fn private_file(map: &Map<String, Value>, key: &str, password: bool) -> Result<PathBuf, ConfigError> {
    let text = map.get(key).and_then(Value::as_str).unwrap_or("");
    if !text.starts_with('/') || text.contains('\0') || text.chars().any(|ch| ch.is_whitespace()) {
        return Err(ConfigError::new("copy.secret", "a secret path is not allowed"));
    }
    let path = PathBuf::from(text);
    let meta = fs::symlink_metadata(&path).map_err(|_| ConfigError::new("copy.secret", "a secret path is absent"))?;
    let mode = meta.permissions().mode();
    let private = if password { mode & 0o077 == 0 } else { mode & 0o002 == 0 };
    if !meta.file_type().is_file() || meta.len() == 0 || meta.len() > 65_536 || !private {
        return Err(ConfigError::new("copy.secret", "a secret file is not a private regular file"));
    }
    Ok(path)
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::invoke;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    #[test]
    fn a_missing_sidecar_does_not_invoke_the_producer() {
        let dir = std::env::temp_dir().join(format!("oo-a405-copy-absent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let config = dir.join("config.json");
        fs::write(&config, b"{}\n").expect("config");
        let value = invoke(&config).expect("invoke");
        assert_eq!(value["invoked"], false);
        assert_eq!(value["reason"], "sidecar_absent");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_script_replay_line_is_recorded_without_a_secret_path() {
        let root = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("oo-a411-copy-replay-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let plan = dir.join("plan.json");
        let manifest = dir.join("manifest.json");
        let target = dir.join("target.json");
        let binary = dir.join("local-pg-mirror");
        fs::write(&plan, b"{}\n").expect("plan");
        fs::write(
            &manifest,
            br#"{"import_hash":"49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73","mirror_state":"complete","customer_complete":false,"snapshot_id":"00000004-00000013-1"}"#,
        )
        .expect("manifest");
        fs::write(&target, b"{}\n").expect("target");
        let script = "#!/bin/sh\necho complete 100000 cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea replay_complete false\n";
        let mut binary_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&binary).expect("bin");
        binary_file.write_all(script.as_bytes()).expect("script");
        drop(binary_file);
        let sidecar = format!(
            "{{\"protocol\":\"oo.minimal-product-loop.copy-invoke/v1\",\"binary\":\"{}\",\"plan\":\"{}\",\"manifest\":\"{}\",\"target_config\":\"{}\",\"import_hash\":\"49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73\"}}\n",
            binary.display(),
            plan.display(),
            manifest.display(),
            target.display()
        );
        fs::write(dir.join("mirror-invoke.json"), sidecar).expect("sidecar");
        let value = invoke(&dir.join("config.json")).expect("invoke");
        assert_eq!(value["invoked"], true);
        assert_eq!(value["customer_complete"], false);
        assert_eq!(value["snapshot_id"], "00000004-00000013-1");
        assert!(value["line"].as_str().unwrap_or("").contains("replay_complete"));
        assert_eq!(value["provenance"], "cache_replay");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fresh_sidecar_passes_private_paths_and_drops_them_from_the_receipt() {
        let root = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("oo-a411-copy-fresh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let plan = dir.join("plan.json");
        let manifest = dir.join("manifest.json");
        let target = dir.join("target.json");
        let binary = dir.join("local-pg-mirror");
        let secret = dir.join("source.pass");
        fs::write(&plan, b"{}\n").expect("plan");
        fs::write(&target, b"{}\n").expect("target");
        let mut secret_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&secret).expect("secret");
        secret_file.write_all(b"hidden-secret\n").expect("bytes");
        drop(secret_file);
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$3\" > \"{}/seen-path\"\ncat > \"$7\" << 'EOF'\n{{\"import_hash\":\"49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73\",\"mirror_state\":\"complete\",\"customer_complete\":false,\"snapshot_id\":\"00000004-00000013-1\"}}\nEOF\necho complete 4 cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea none false\n",
            dir.display()
        );
        let mut binary_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&binary).expect("bin");
        binary_file.write_all(script.as_bytes()).expect("script");
        drop(binary_file);
        let sidecar = format!(
            "{{\"protocol\":\"oo.minimal-product-loop.copy-invoke/v1\",\"mode\":\"fresh\",\"binary\":\"{}\",\"plan\":\"{}\",\"manifest\":\"{}\",\"target_config\":\"{}\",\"source_password\":\"{}\",\"source_ca\":\"{}\",\"admin_password\":\"{}\",\"runtime_password\":\"{}\",\"target_ca\":\"{}\"}}\n",
            binary.display(),
            plan.display(),
            manifest.display(),
            target.display(),
            secret.display(),
            secret.display(),
            secret.display(),
            secret.display(),
            secret.display()
        );
        fs::write(dir.join("config.json"), b"{}\n").expect("config");
        fs::write(dir.join("mirror-invoke.json"), sidecar).expect("sidecar");
        let value = invoke(&dir.join("config.json")).expect("invoke");
        let rendered = value.to_string();
        assert_eq!(value["provenance"], "fresh_complete");
        assert_eq!(value["import_hash"], "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73");
        assert!(!rendered.contains("hidden-secret"));
        assert!(!rendered.contains(&secret.display().to_string()));
        let seen = fs::read_to_string(dir.join("seen-path")).expect("seen");
        assert_eq!(seen.trim(), secret.display().to_string());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fresh_source_config_discovers_a_new_plan_then_copies() {
        let root = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("oo-a411-copy-discover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let source = dir.join("source.config.json");
        let plan = dir.join("plan.json");
        let manifest = dir.join("manifest.json");
        let target = dir.join("target.json");
        let binary = dir.join("local-pg-mirror");
        let secret = dir.join("source.pass");
        fs::write(&source, b"{}\n").expect("source");
        let mut secret_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&secret).expect("secret");
        secret_file.write_all(b"hidden-secret\n").expect("bytes");
        drop(secret_file);
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"discover\" ]; then\n  printf '%s\\n' \"$4\" > \"{dir}/seen-path\"\n  printf '{{}}\\n' > \"$3\"\n  echo planned 0 cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea none false\n  exit 0\nfi\nif [ \"$1\" = \"copy\" ]; then\n  printf '%s\\n' \"$3\" >> \"{dir}/seen-path\"\n  cat > \"$7\" << 'EOF'\n{{\"import_hash\":\"49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73\",\"mirror_state\":\"complete\",\"customer_complete\":false,\"snapshot_id\":\"0000000B-00000008-2\"}}\nEOF\n  printf '{{}}\\n' > \"$8\"\n  echo complete 100000 cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea none false\n  exit 0\nfi\nexit 1\n",
            dir = dir.display()
        );
        let mut binary_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&binary).expect("bin");
        binary_file.write_all(script.as_bytes()).expect("script");
        drop(binary_file);
        let sidecar = format!(
            "{{\"protocol\":\"oo.minimal-product-loop.copy-invoke/v1\",\"mode\":\"fresh\",\"admin_user\":\"oo_a407_admin\",\"binary\":\"{}\",\"plan\":\"{}\",\"manifest\":\"{}\",\"target_config\":\"{}\",\"source_config\":\"{}\",\"source_password\":\"{}\",\"source_ca\":\"{}\",\"admin_password\":\"{}\",\"runtime_password\":\"{}\",\"target_ca\":\"{}\"}}\n",
            binary.display(),
            plan.display(),
            manifest.display(),
            target.display(),
            source.display(),
            secret.display(),
            secret.display(),
            secret.display(),
            secret.display(),
            secret.display()
        );
        fs::write(dir.join("config.json"), b"{}\n").expect("config");
        fs::write(dir.join("mirror-invoke.json"), sidecar).expect("sidecar");
        let value = invoke(&dir.join("config.json")).expect("invoke");
        let rendered = value.to_string();
        assert_eq!(value["provenance"], "fresh_complete");
        assert_eq!(value["discover"], true);
        assert!(value["line"].as_str().unwrap_or("").contains("100000"));
        assert!(!rendered.contains("hidden-secret"));
        assert!(!rendered.contains(&secret.display().to_string()));
        assert!(!rendered.contains("replay_complete"));
        let seen = fs::read_to_string(dir.join("seen-path")).expect("seen");
        assert_eq!(seen.lines().next(), Some(secret.display().to_string().as_str()));
        assert!(plan.is_file());
        let reopened = super::open_sidecar(&dir.join("config.json")).expect("reopen");
        assert_eq!(reopened.import_hash, "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sealed_fresh_manifest_is_confirmed_without_a_second_discover() {
        let root = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("oo-a411-copy-confirm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let plan_sha = "3853ada6f0ec238eacc121fefe4a50295fee0821579379802b5916d5134138d8";
        let import_hash = "f7c21ea98c20b88b8a7986da37f77772f9d8874f2efc055b48afdb0fef31aed1";
        let source = dir.join("source.config.json");
        let plan = dir.join("plan.json");
        let manifest = dir.join("manifest.json");
        let target = dir.join("target.json");
        let binary = dir.join("local-pg-mirror");
        let secret = dir.join("source.pass");
        fs::write(&source, b"{}\n").expect("source");
        fs::write(&plan, format!("{{\"plan_sha256\":\"{plan_sha}\",\"generation\":2,\"source_id\":\"a407bulk\"}}\n")).expect("plan");
        fs::write(
            &manifest,
            format!(
                "{{\"import_hash\":\"{import_hash}\",\"mirror_state\":\"complete\",\"customer_complete\":false,\"snapshot_id\":\"0000000D-0000000C-1\",\"failure\":\"none\",\"plan_sha256\":\"{plan_sha}\",\"import_generation\":2,\"rows\":100000,\"source_socket_peak\":1,\"source_socket_fact\":\"repeatable_read_read_only\",\"source_write_tuples\":0,\"source_ddl\":false,\"source_count_star\":false,\"source_explain\":false,\"source_read_only\":true}}\n"
            ),
        )
        .expect("manifest");
        fs::write(&target, b"{}\n").expect("target");
        let mut secret_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&secret).expect("secret");
        secret_file.write_all(b"hidden-secret\n").expect("bytes");
        drop(secret_file);
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" > \"{}\"\nif [ \"$1\" = \"discover\" ]; then exit 1; fi\necho complete 100000 {plan_sha} replay_complete false\nexit 0\n",
            dir.join("called").display()
        );
        let mut binary_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&binary).expect("bin");
        binary_file.write_all(script.as_bytes()).expect("script");
        drop(binary_file);
        let sidecar = format!(
            "{{\"protocol\":\"oo.minimal-product-loop.copy-invoke/v1\",\"mode\":\"fresh\",\"admin_user\":\"oo_a407_admin\",\"binary\":\"{}\",\"plan\":\"{}\",\"manifest\":\"{}\",\"target_config\":\"{}\",\"source_config\":\"{}\",\"source_password\":\"{}\",\"source_ca\":\"{}\",\"admin_password\":\"{}\",\"runtime_password\":\"{}\",\"target_ca\":\"{}\"}}\n",
            binary.display(),
            plan.display(),
            manifest.display(),
            target.display(),
            source.display(),
            secret.display(),
            secret.display(),
            secret.display(),
            secret.display(),
            secret.display()
        );
        fs::write(dir.join("config.json"), b"{}\n").expect("config");
        fs::write(dir.join("mirror-invoke.json"), sidecar).expect("sidecar");
        let value = invoke(&dir.join("config.json")).expect("confirm");
        let rendered = value.to_string();
        assert_eq!(value["provenance"], "fresh_complete");
        assert_eq!(value["discover"], false);
        assert_eq!(value["copy_origin"], "fresh");
        assert_eq!(value["producer_disposition"], "replay_complete");
        assert_eq!(value["import_hash"], import_hash);
        assert_eq!(value["snapshot_id"], "0000000D-0000000C-1");
        assert_eq!(fs::read_to_string(dir.join("called")).expect("called").trim(), "copy");
        assert!(rendered.contains("\"event\":\"begin\""));
        assert!(rendered.contains("\"name\":\"discover\""));
        assert!(rendered.contains(import_hash));
        assert!(!rendered.contains("hidden-secret"));
        assert!(!rendered.contains(&secret.display().to_string()));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_certificate_may_be_world_readable_and_a_password_may_not() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("oo-a411-copy-cert-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let source = dir.join("source.config.json");
        let plan = dir.join("plan.json");
        let manifest = dir.join("manifest.json");
        let target = dir.join("target.json");
        let binary = dir.join("local-pg-mirror");
        let password = dir.join("source.pass");
        let cert = dir.join("ca.crt");
        fs::write(&source, b"{}\n").expect("source");
        let mut password_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o644).open(&password).expect("password");
        password_file.write_all(b"hidden-secret\n").expect("bytes");
        drop(password_file);
        let mut cert_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o644).open(&cert).expect("cert");
        cert_file.write_all(b"certificate\n").expect("cert-bytes");
        drop(cert_file);
        let script = format!("#!/bin/sh\nprintf 'called\\n' > \"{}\"\nexit 1\n", dir.join("called").display());
        let mut binary_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&binary).expect("bin");
        binary_file.write_all(script.as_bytes()).expect("script");
        drop(binary_file);
        let sidecar = format!(
            "{{\"protocol\":\"oo.minimal-product-loop.copy-invoke/v1\",\"mode\":\"fresh\",\"admin_user\":\"oo_a407_admin\",\"binary\":\"{}\",\"plan\":\"{}\",\"manifest\":\"{}\",\"target_config\":\"{}\",\"source_config\":\"{}\",\"source_password\":\"{}\",\"source_ca\":\"{}\",\"admin_password\":\"{}\",\"runtime_password\":\"{}\",\"target_ca\":\"{}\"}}\n",
            binary.display(),
            plan.display(),
            manifest.display(),
            target.display(),
            source.display(),
            password.display(),
            cert.display(),
            password.display(),
            password.display(),
            cert.display()
        );
        fs::write(dir.join("config.json"), b"{}\n").expect("config");
        fs::write(dir.join("mirror-invoke.json"), sidecar).expect("sidecar");
        let error = invoke(&dir.join("config.json")).expect_err("password mode");
        assert_eq!(error.code, "copy.secret");
        assert!(!dir.join("called").exists());
        let mut password_file = fs::OpenOptions::new().write(true).truncate(true).mode(0o600).open(&password).expect("password");
        password_file.write_all(b"hidden-secret\n").expect("bytes");
        drop(password_file);
        fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).expect("mode");
        let script = "#!/bin/sh\nif [ \"$1\" = \"discover\" ]; then printf '{}\\n' > \"$3\"; echo planned 0 cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea none false; exit 0; fi\ncat > \"$7\" << 'EOF'\n{\"import_hash\":\"49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73\",\"mirror_state\":\"complete\",\"customer_complete\":false,\"snapshot_id\":\"0000000B-00000008-2\"}\nEOF\nprintf '{}\\n' > \"$8\"\necho complete 100000 cf9845c5c257a728cd95e3125b5b2500c0724980babd25df0ad32e5e046773ea none false\n".to_string();
        let mut binary_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&binary).expect("bin");
        binary_file.write_all(script.as_bytes()).expect("script");
        drop(binary_file);
        let value = invoke(&dir.join("config.json")).expect("cert");
        assert_eq!(value["discover"], true);
        assert!(!value.to_string().contains("certificate"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fresh_source_config_refuses_an_existing_plan_before_the_producer() {
        let root = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("oo-a411-copy-plan-exists-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let source = dir.join("source.config.json");
        let plan = dir.join("plan.json");
        let binary = dir.join("local-pg-mirror");
        let secret = dir.join("source.pass");
        fs::write(&source, b"{}\n").expect("source");
        fs::write(&plan, b"{}\n").expect("plan");
        let mut secret_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&secret).expect("secret");
        secret_file.write_all(b"hidden-secret\n").expect("bytes");
        drop(secret_file);
        let script = format!("#!/bin/sh\nprintf 'called\\n' > \"{}\"\nexit 1\n", dir.join("called").display());
        let mut binary_file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&binary).expect("bin");
        binary_file.write_all(script.as_bytes()).expect("script");
        drop(binary_file);
        let sidecar = format!(
            "{{\"protocol\":\"oo.minimal-product-loop.copy-invoke/v1\",\"mode\":\"fresh\",\"admin_user\":\"oo_a407_admin\",\"binary\":\"{}\",\"plan\":\"{}\",\"manifest\":\"{}\",\"target_config\":\"{}\",\"source_config\":\"{}\",\"source_password\":\"{}\",\"source_ca\":\"{}\",\"admin_password\":\"{}\",\"runtime_password\":\"{}\",\"target_ca\":\"{}\"}}\n",
            binary.display(),
            plan.display(),
            dir.join("manifest.json").display(),
            dir.join("target.json").display(),
            source.display(),
            secret.display(),
            secret.display(),
            secret.display(),
            secret.display(),
            secret.display()
        );
        fs::write(dir.join("config.json"), b"{}\n").expect("config");
        fs::write(dir.join("mirror-invoke.json"), sidecar).expect("sidecar");
        let error = invoke(&dir.join("config.json")).expect_err("exists");
        assert_eq!(error.code, "copy.fresh");
        assert!(!dir.join("called").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_database_section_must_name_the_written_target() {
        let root = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = root.join(format!("oo-a411-copy-target-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let target = dir.join("target.json");
        fs::write(
            &target,
            br#"{"protocol":"oo.pipeline.local-pg-mirror-target/v1","host":"oo-a407-dst-pg","port":5432,"database":"oo_a407_dst","user":"oo_a407_reader","server_name":"oo-a407-dst-pg","ca_sha256":"abc","tls":"verify-full"}"#,
        )
        .expect("target");
        assert!(super::target_names_match("oo-a407-dst-pg", 5432, "oo_a407_dst", "oo_a407_reader", &target).is_ok());
        let error = super::target_names_match("oo-a410-tgt", 5432, "mirrordb", "oo_mirror_read", &target).expect_err("mismatch");
        assert_eq!(error.code, "copy.target");
        let _ = fs::remove_dir_all(&dir);
    }
}
