//! Native Arrow physical import.
//!
//! `oo.pipeline.sealed-import-catalog/v1` is a semantic catalog. This command
//! does not relabel it as rows and does not spawn a loader for it. The MySQL
//! manifest stays on `import`. A native manifest is forwarded unchanged to
//! `oo-pipeline import-native`. This module does not call a model.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

use crate::error::PgBuilderError;
use crate::profile::{read_capped, wait_child};
use crate::sealed::DEPENDENCY_SHA;

/// Envelope around one native import result.
pub const ENVELOPE_PROTOCOL: &str = "oo.pg-builder.native-import-envelope/v1";
/// Physical import request accepted by `oo-pipeline import-native`.
pub const NATIVE_PROTOCOL: &str = "oo.pipeline.native-arrow-import/v1";
/// Semantic catalog. It has no Arrow payload.
pub const SEMANTIC_CATALOG_PROTOCOL: &str = "oo.pipeline.sealed-import-catalog/v1";
/// MySQL sealed manifest. That path stays on `pg-builder import`.
pub const MYSQL_MANIFEST_PROTOCOL: &str = "oo.mysql-preview.manifest/v1";
const RESULT_PROTOCOL: &str = "oo.pipeline.native-arrow-import-result/v1";

/// What `import-native` does with one manifest. `Spawn` is the only path that
/// starts `oo-pipeline`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeRoute {
    /// The manifest is not a physical import. PostgreSQL is not opened.
    Refuse(Value),
    /// Forward the manifest bytes to `oo-pipeline import-native`.
    Spawn,
}

/// Classify a manifest before any process starts.
pub fn route(manifest: &Value) -> Result<NativeRoute, PgBuilderError> {
    let protocol = manifest.get("protocol").and_then(Value::as_str).unwrap_or("");
    if protocol == SEMANTIC_CATALOG_PROTOCOL {
        return Ok(NativeRoute::Refuse(local_result(manifest, "semantic_catalog")));
    }
    if protocol == MYSQL_MANIFEST_PROTOCOL {
        return Ok(NativeRoute::Refuse(local_result(manifest, "mysql_entrypoint")));
    }
    if protocol == NATIVE_PROTOCOL {
        return Ok(NativeRoute::Spawn);
    }
    Ok(NativeRoute::Refuse(local_result(manifest, "schema")))
}

/// Read a manifest, refuse a non-physical document locally, or spawn the pipeline.
pub fn run_native_import(
    manifest_path: &Path,
    pipeline: Option<&Path>,
    stderr_path: Option<&Path>,
) -> Result<Value, PgBuilderError> {
    let bytes = read_manifest(manifest_path)?;
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|_| PgBuilderError::new("input.schema", "an input file is not JSON"))?;
    let import = match route(&manifest)? {
        NativeRoute::Refuse(result) => result,
        NativeRoute::Spawn => {
            let pipeline = pipeline.ok_or_else(|| {
                PgBuilderError::new("usage", "a required argument is missing")
            })?;
            spawn_native(pipeline, &bytes, stderr_path)?
        }
    };
    Ok(envelope(import))
}

fn envelope(import: Value) -> Value {
    json!({
        "protocol": ENVELOPE_PROTOCOL,
        "dependency_sha": DEPENDENCY_SHA,
        "compiler": "not_run",
        "pack": "not_produced",
        "model": "not_called",
        "import": import
    })
}

fn local_result(manifest: &Value, reason: &str) -> Value {
    json!({
        "protocol": RESULT_PROTOCOL,
        "load_state": "not_sealed",
        "reason": reason,
        "loader_invoked": false,
        "run_id": Value::Null,
        "source_engine": manifest.get("source_engine").cloned().unwrap_or(Value::Null),
        "target_storage": manifest.get("target_storage").cloned().unwrap_or(Value::Null),
        "source_id": manifest.get("source_id").cloned().unwrap_or(Value::Null),
        "admission": {
            "scope": "readonly_policy_replay",
            "batch_digests_bound": false
        },
        "tables": []
    })
}

fn read_manifest(path: &Path) -> Result<Vec<u8>, PgBuilderError> {
    let meta = fs::metadata(path)
        .map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > 1024 * 1024 {
        return Err(PgBuilderError::new("input.read", "an input file could not be read"));
    }
    let bytes = fs::read(path)
        .map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))?;
    if std::str::from_utf8(&bytes).is_err() {
        return Err(PgBuilderError::new("input.schema", "an input file is not UTF-8"));
    }
    Ok(bytes)
}

fn spawn_native(
    pipeline: &Path,
    manifest: &[u8],
    stderr_path: Option<&Path>,
) -> Result<Value, PgBuilderError> {
    if !pipeline.is_file() {
        return Err(PgBuilderError::new("import.bin", "the import binary is not a file"));
    }
    let mut command = Command::new(pipeline);
    command
        .arg("import-native")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Only the operator binding configuration crosses the cleared environment.
    // Manifest fields cannot select the binary or the registry.
    forward_operator_env(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| PgBuilderError::new("import.spawn", "the import command could not start"))?;
    {
        let mut input = child.stdin.take().ok_or_else(|| {
            PgBuilderError::new("import.spawn", "the import command could not start")
        })?;
        input
            .write_all(manifest)
            .map_err(|_| PgBuilderError::new("import.spawn", "the import command could not start"))?;
    }
    let stdout = child.stdout.take().ok_or_else(|| {
        PgBuilderError::new("import.spawn", "the import command could not start")
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        PgBuilderError::new("import.spawn", "the import command could not start")
    })?;
    let out_handle = std::thread::spawn(move || read_capped(stdout, 8 * 1024 * 1024));
    let err_handle = std::thread::spawn(move || read_capped(stderr, 8192));
    let _status = wait_child(&mut child, Duration::from_secs(60))?;
    let out = out_handle
        .join()
        .map_err(|_| PgBuilderError::new("import.output", "the import output was refused"))?
        .map_err(|_| PgBuilderError::new("import.output", "the import output was refused"))?;
    let err = err_handle
        .join()
        .map_err(|_| PgBuilderError::new("import.output", "the import output was refused"))?
        .unwrap_or_default();
    if let Some(path) = stderr_path {
        let _ = fs::write(path, err);
    }
    let import: Value = serde_json::from_slice(trim_ascii(&out))
        .map_err(|_| PgBuilderError::new("import.schema", "the import output was refused"))?;
    if import.get("protocol").and_then(Value::as_str) != Some(RESULT_PROTOCOL) {
        return Err(PgBuilderError::new("import.schema", "the import output was refused"));
    }
    Ok(import)
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|byte| !byte.is_ascii_whitespace()).map(|index| index + 1).unwrap_or(0);
    &bytes[..end]
}

fn forward_operator_env(command: &mut Command) {
    apply_operator_env(command, |key| std::env::var(key).ok());
}

fn apply_operator_env(command: &mut Command, mut lookup: impl FnMut(&str) -> Option<String>) {
    for key in [
        "OO_NATIVE_ADMISSION_BIN",
        "OO_NATIVE_BINDING_REGISTRY",
        "OO_NATIVE_BINDING_REGISTRY_SHA256",
    ] {
        if let Some(value) = lookup(key) {
            // UNTRUSTED is forwarded so the child refuses it. Stripping it
            // would fall through to the empty-pin path.
            command.env(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semantic_catalog_does_not_spawn() {
        let document = json!({
            "protocol": SEMANTIC_CATALOG_PROTOCOL,
            "source_engine": "clickhouse"
        });
        match route(&document).expect("route") {
            NativeRoute::Refuse(result) => {
                assert_eq!(result["reason"], "semantic_catalog");
                assert_eq!(result["loader_invoked"], false);
                assert_eq!(result["load_state"], "not_sealed");
            }
            NativeRoute::Spawn => panic!("semantic catalog spawned"),
        }
    }

    #[test]
    fn mysql_manifest_does_not_spawn_and_is_not_the_import_refusal() {
        let document = json!({ "protocol": MYSQL_MANIFEST_PROTOCOL, "source_engine": "mysql" });
        match route(&document).expect("route") {
            NativeRoute::Refuse(result) => {
                assert_eq!(result["reason"], "mysql_entrypoint");
                assert_eq!(result["loader_invoked"], false);
                let text = result.to_string();
                assert!(!text.contains("the manifest protocol is not the sealed import manifest"));
            }
            NativeRoute::Spawn => panic!("mysql manifest spawned"),
        }
    }

    #[test]
    fn native_protocol_is_the_spawn_route() {
        let document = json!({ "protocol": NATIVE_PROTOCOL, "source_engine": "clickhouse" });
        assert_eq!(route(&document).expect("route"), NativeRoute::Spawn);
    }

    #[test]
    fn operator_env_is_forwarded_after_clear_and_other_env_is_not() {
        let mut command = Command::new("/usr/bin/printenv");
        command.env_clear();
        apply_operator_env(&mut command, |key| match key {
            "OO_NATIVE_ADMISSION_BIN" => Some("/fixed/oo-source-admission".to_string()),
            "OO_NATIVE_BINDING_REGISTRY" => Some("/fixed/registry.json".to_string()),
            "OO_NATIVE_BINDING_REGISTRY_SHA256" => Some("UNTRUSTED".to_string()),
            _ => None,
        });
        let output = command.output().expect("printenv").stdout;
        let text = String::from_utf8(output).expect("utf8");
        assert!(text.contains("OO_NATIVE_ADMISSION_BIN=/fixed/oo-source-admission"));
        assert!(text.contains("OO_NATIVE_BINDING_REGISTRY=/fixed/registry.json"));
        assert!(text.contains("OO_NATIVE_BINDING_REGISTRY_SHA256=UNTRUSTED"));
        assert!(!text.contains("OO_NATIVE_SHOULD_NOT_PASS"));
        assert!(!text.contains("secret://"));
    }

    #[test]
    fn other_protocol_does_not_spawn() {
        let document = json!({ "protocol": "oo.source.pgsql/v1" });
        match route(&document).expect("route") {
            NativeRoute::Refuse(result) => assert_eq!(result["reason"], "schema"),
            NativeRoute::Spawn => panic!("foreign protocol spawned"),
        }
    }
}
