//! Run the fixed query-result-export binary and check its outputs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use zeroize::Zeroizing;

use crate::config::Config;
use crate::crypto::{random_hex, sha256_hex, MAX_PLAINTEXT};
use crate::error::Fault;
use crate::paths::{intake_file, object_id_ok, read_regular, remove_staging};
use crate::scope::scan_secrets;

pub struct ReadyExport {
    pub csv: Zeroizing<Vec<u8>>,
    pub plaintext_sha256: String,
    pub plaintext_size: i64,
    pub source_sha256: String,
    pub sidecar_sha256: String,
}

pub enum Produced {
    Files { dir: PathBuf, source_sha256: String },
    Failed { reason: &'static str, source_sha256: String },
}

pub enum Checked {
    Ready(ReadyExport),
    Reject { reason: &'static str, source_sha256: String },
}

pub enum Intake {
    Ready(ReadyExport),
    Loss { reason: &'static str, source_sha256: String },
}

pub fn intake(config: &Config, source_name: &str, columns: &[String]) -> Result<Intake, Fault> {
    match produce(config, source_name, columns)? {
        Produced::Failed { reason, source_sha256 } => Ok(Intake::Loss { reason, source_sha256 }),
        Produced::Files { dir, source_sha256 } => {
            let checked = check_dir(&dir, &config.aead_key, &source_sha256);
            let removed = remove_staging(&config.object_dir, &dir);
            match checked {
                Ok(Checked::Ready(ready)) => {
                    removed?;
                    Ok(Intake::Ready(ready))
                }
                Ok(Checked::Reject { reason, source_sha256 }) => {
                    removed?;
                    Ok(Intake::Loss { reason, source_sha256 })
                }
                Err(fault) => {
                    let _ = removed;
                    Err(fault)
                }
            }
        }
    }
}

pub fn produce(config: &Config, source_name: &str, columns: &[String]) -> Result<Produced, Fault> {
    let source = intake_file(&config.intake_root, source_name)?;
    let source_bytes = read_regular(&source, MAX_PLAINTEXT, None)?;
    let source_sha256 = sha256_hex(&source_bytes);
    let staging_root = config.object_dir.join("staging");
    ensure_dir(&staging_root)?;
    let name = random_hex(16)?;
    if !object_id_ok(&name) {
        return Err(Fault::path());
    }
    let dir = staging_root.join(&name);
    fs::create_dir(&dir).map_err(|_| Fault::path())?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|_| Fault::path())?;
    let request_path = dir.join("request.json");
    let source_text = source.to_str().ok_or_else(Fault::path)?;
    let output_text = dir.to_str().ok_or_else(Fault::path)?;
    let request = serde_json::json!({
        "version": "oo.query-result-export/v1",
        "source_file": source_text,
        "output_dir": output_text,
        "columns": columns,
    });
    let request_bytes = serde_json::to_vec(&request).map_err(|_| Fault::malformed())?;
    crate::paths::write_private(&request_path, &request_bytes)?;
    let output = Command::new(&config.export_bin)
        .arg("--request")
        .arg(&request_path)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let _ = fs::remove_file(&request_path);
    let output = match output {
        Ok(output) => output,
        Err(_) => {
            let _ = remove_staging(&config.object_dir, &dir);
            return Err(Fault::not_ready());
        }
    };
    let code = output.status.code().unwrap_or(1);
    if code != 0 {
        let _ = remove_staging(&config.object_dir, &dir);
        let reason = if code == 1 { "incomplete" } else { "producer" };
        return Ok(Produced::Failed { reason, source_sha256 });
    }
    Ok(Produced::Files { dir, source_sha256 })
}

pub fn check_dir(dir: &Path, key: &[u8], source_sha256: &str) -> Result<Checked, Fault> {
    let source = read_named(dir, "export.source.jsonl")?;
    let csv = read_named(dir, "export.csv")?;
    let sidecar = read_named(dir, "export.sidecar.json")?;
    let source_hash = sha256_hex(&source);
    let csv_hash = sha256_hex(&csv);
    let sidecar_hash = sha256_hex(&sidecar);
    let parsed: Value = match serde_json::from_slice(&sidecar) {
        Ok(value) => value,
        Err(_) => {
            return Ok(Checked::Reject { reason: "producer", source_sha256: source_sha256.to_owned() });
        }
    };
    let object = parsed.as_object().ok_or_else(Fault::malformed)?;
    if field_str(object, "version") != Some("oo.query-result-export/v1") || field_str(object, "format") != Some("csv") {
        return Ok(Checked::Reject { reason: "producer", source_sha256: source_sha256.to_owned() });
    }
    let hashes_ok = field_str(object, "source_sha256") == Some(source_hash.as_str())
        && field_str(object, "csv_sha256") == Some(csv_hash.as_str())
        && source_hash == source_sha256
        && field_u64(object, "source_bytes") == Some(source.len() as u64);
    if !hashes_ok {
        return Ok(Checked::Reject { reason: "checksum", source_sha256: source_sha256.to_owned() });
    }
    let complete = field_bool(object, "complete") == Some(true)
        && field_bool(object, "partial") == Some(false)
        && field_bool(object, "http_auth") == Some(false)
        && field_bool(object, "authenticated") == Some(false)
        && field_str(object, "xlsx") == Some("unsupported")
        && field_str(object, "status") == Some("succeeded")
        && field_bool(object, "truncated") == Some(false);
    if !complete {
        return Ok(Checked::Reject { reason: "incomplete", source_sha256: source_sha256.to_owned() });
    }
    if scan_secrets(&csv, key) || scan_secrets(&source, key) || scan_secrets(&sidecar, key) {
        return Ok(Checked::Reject { reason: "secret_scan", source_sha256: source_sha256.to_owned() });
    }
    let plaintext_size = i64::try_from(csv.len()).map_err(|_| Fault::budget())?;
    Ok(Checked::Ready(ReadyExport {
        csv: Zeroizing::new(csv),
        plaintext_sha256: csv_hash,
        plaintext_size,
        source_sha256: source_sha256.to_owned(),
        sidecar_sha256: sidecar_hash,
    }))
}

fn read_named(dir: &Path, name: &str) -> Result<Vec<u8>, Fault> {
    read_regular(&dir.join(name), MAX_PLAINTEXT, None)
}

fn ensure_dir(path: &Path) -> Result<(), Fault> {
    if fs::symlink_metadata(path).is_err() {
        fs::create_dir(path).map_err(|_| Fault::path())?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| Fault::path())?;
    }
    crate::paths::directory_private(path)
}

fn field_str<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn field_bool(object: &serde_json::Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key).and_then(Value::as_bool)
}

fn field_u64(object: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key).and_then(Value::as_u64)
}
