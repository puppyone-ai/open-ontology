use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::{json, Value};

use crate::error::Stop;

pub const CAPTURE_MAX: usize = 8_388_608;

pub fn non_claims(analyze_published: bool) -> Value {
    json!({
        "parent_acceptance": false,
        "development_unit_approval": false,
        "component_approval": false,
        "m1_i01": "not_accepted",
        "m1_i02": "not_accepted",
        "semantic_publication": "missing",
        "authentication": "not_claimed",
        "authorization": "not_claimed",
        "wal": "missing",
        "natural_language": "missing",
        "harness_mount": "not_performed",
        "hand_grant_business_columns": false,
        "hand_classified_policies": false,
        "role_login_binding": "configuration_not_auth",
        "analyze_published": analyze_published,
        "database_rollback": false,
        "database_release": false,
        "mapping_rows_read": false,
        "config_identity": "recomputed_from_non_secret_request_fields",
        "public_auth": false
    })
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Stop> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| Stop::new("outcome", "io"))?;
    file.write_all(bytes)
        .map_err(|_| Stop::new("outcome", "io"))?;
    let _ = file.sync_all();
    Ok(())
}

pub fn write_json_new(path: &Path, value: &Value) -> Result<(), Stop> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| Stop::new("outcome", "io"))?;
    bytes.push(b'\n');
    write_new(path, &bytes)
}

pub fn failed_body(stop: &Stop, phase: &str) -> Value {
    json!({
        "protocol": "oo.onboarding.outcome/v1",
        "phase": phase,
        "status": "failed",
        "step": stop.step,
        "detail": stop.detail,
        "database_state": "unchanged_by_this_tool",
        "database_rollback": false,
        "database_release": false,
        "non_claims": non_claims(false)
    })
}

pub fn read_json(path: &Path, max: usize) -> Result<Value, Stop> {
    let bytes = read_file(path, max)?;
    serde_json::from_slice(&bytes).map_err(|_| Stop::new("outcome", "shape"))
}

pub fn read_file(path: &Path, max: usize) -> Result<Vec<u8>, Stop> {
    let meta = fs::symlink_metadata(path).map_err(|_| Stop::new("outcome", "io"))?;
    if !meta.file_type().is_file() || meta.len() > max as u64 {
        return Err(Stop::new("outcome", "io"));
    }
    let bytes = fs::read(path).map_err(|_| Stop::new("outcome", "io"))?;
    if bytes.len() > max {
        return Err(Stop::new("outcome", "capture"));
    }
    Ok(bytes)
}

pub fn status_of(value: &Value) -> Result<&str, Stop> {
    value["status"]
        .as_str()
        .ok_or_else(|| Stop::new("outcome", "shape"))
}
