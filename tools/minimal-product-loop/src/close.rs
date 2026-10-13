//! Stop the recorded Pi process. The source database is left in place.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use crate::config::{load_config, ConfigError};
use crate::state::{merge_state, read_state};

#[derive(Clone, Debug)]
pub struct CloseOutcome {
    pub status: String,
    pub pid_recorded: bool,
    pub pid_absent: bool,
    pub database_dropped: bool,
    pub database_observed: bool,
    pub mapping: String,
    pub residue_absent: bool,
}

#[derive(Clone, Debug)]
pub struct CancelOutcome {
    pub status: String,
    pub pid_absent: bool,
    pub database_dropped: bool,
    pub database_observed: bool,
    pub mapping: String,
}

pub(crate) struct ProcId {
    pub comm: String,
    pub start: String,
}

/// Stops one recorded in-flight ask. A closed loop, or a loop with no live ask, is refused.
pub fn cancel(path: &Path) -> Result<CancelOutcome, ConfigError> {
    let config = load_config(path)?;
    let state = read_state(&config).ok();
    let status = state.as_ref().and_then(|value| value.get("status")).and_then(Value::as_str).unwrap_or("");
    let pid = state.as_ref().and_then(|value| value.get("pi_pid")).and_then(Value::as_u64);
    let pid_recorded = pid.is_some();
    let pid_value = pid.and_then(|value| u32::try_from(value).ok());
    let live = pid_value.and_then(identity);
    if cancel_admission(status, pid_recorded, live.is_some()).is_err() {
        return Err(ConfigError::new("cancel.refused", "cancel is refused when no ask is in flight"));
    }
    let pid = pid_value.ok_or_else(|| ConfigError::new("cancel.refused", "cancel is refused when no ask is in flight"))?;
    let live = live.ok_or_else(|| ConfigError::new("cancel.refused", "cancel is refused when no ask is in flight"))?;
    let recorded_comm = state.as_ref().and_then(|value| value.get("pi_comm")).and_then(Value::as_str).unwrap_or("");
    let recorded_start = state.as_ref().and_then(|value| value.get("pi_start")).and_then(Value::as_str).unwrap_or("");
    if recorded_comm.is_empty() || live.start != recorded_start {
        return Err(ConfigError::new("cancel.foreign", "the recorded pid does not match the ask process"));
    }
    if crate::source::observe_database(&config).is_err() {
        return Err(ConfigError::new("cancel.database", "the local database was not observed"));
    }
    let _ = Command::new("kill").arg(pid.to_string()).status();
    if !wait_absent(pid) {
        return Err(ConfigError::new("cancel.alive", "the recorded ask process is still present"));
    }
    let map = revoke_proxy_map(&config.work);
    let database_observed = crate::source::observe_database(&config).is_ok();
    if !database_observed {
        return Err(ConfigError::new("cancel.database", "the local database was not observed"));
    }
    let _ = merge_state(
        &config,
        &json!({
            "status": "cancelled",
            "pi_pid": null,
            "pi_comm": null,
            "pi_start": null,
            "mapping": map.mapping,
            "residue_absent": map.residue_absent,
            "database_dropped": false,
            "database_observed": true,
            "pid_absent": true
        }),
    );
    Ok(CancelOutcome {
        status: "cancelled".to_owned(),
        pid_absent: true,
        database_dropped: false,
        database_observed: true,
        mapping: map.mapping,
    })
}

fn wait_absent(pid: u32) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while identity(pid).is_some() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    true
}

pub(crate) fn cancel_admission(status: &str, pid_recorded: bool, pid_live: bool) -> Result<&'static str, &'static str> {
    if status == "released" || status == "cancelled" || !pid_recorded || !pid_live {
        Err("cancel.refused")
    } else {
        Ok("cancelled")
    }
}

pub fn close(path: &Path) -> Result<CloseOutcome, ConfigError> {
    let config = load_config(path)?;
    let mut map = revoke_proxy_map(&config.work);
    let state = read_state(&config).ok();
    let previously_revoked = state.as_ref().is_some_and(|state| {
        matches!(state.get("status").and_then(Value::as_str), Some("cancelled" | "released"))
            && state.get("residue_absent").and_then(Value::as_bool) == Some(true)
            && !config.work.join("proxy-session-map.json").exists()
            && !config.work.join("proxy-marker").exists()
    });
    if map.mapping == "absent" && previously_revoked {
        map.mapping = "revoked".to_owned();
        map.residue_absent = true;
    }
    let pid_recorded = state.as_ref().and_then(|value| value.get("pi_pid")).and_then(Value::as_u64).is_some();
    let mut pid_absent = !pid_recorded && state.as_ref().and_then(|state| state.get("pid_absent")).and_then(Value::as_bool) == Some(true);
    if let Some(state) = &state
        && let Some(pid) = state.get("pi_pid").and_then(Value::as_u64)
    {
        let pid = u32::try_from(pid).map_err(|_| ConfigError::new("close.foreign", "the recorded pid is not a process id"))?;
        if let Some(live) = identity(pid) {
            let recorded_comm = state.get("pi_comm").and_then(Value::as_str).unwrap_or("");
            let recorded_start = state.get("pi_start").and_then(Value::as_str).unwrap_or("");
            if recorded_comm.is_empty() || live.start != recorded_start {
                return Err(ConfigError::new("close.foreign", "the recorded pid does not match the pi process"));
            }
            let _ = Command::new("kill").arg(pid.to_string()).status();
            if !wait_absent(pid) {
                return Err(ConfigError::new("close.alive", "the recorded pi process is still present"));
            }
        }
        pid_absent = identity(pid).is_none();
        let _ = merge_state(&config, &json!({"pi_pid": null, "pi_comm": null, "pi_start": null}));
    }
    let database_observed = crate::source::observe_database(&config).is_ok();
    let status = if map.mapping == "residue" {
        "residue"
    } else if map.mapping == "absent" {
        "mapping_absent"
    } else if !database_observed {
        "database_unobserved"
    } else if pid_recorded && !pid_absent {
        "pi_alive"
    } else {
        "released"
    };
    let _ = merge_state(
        &config,
        &json!({
            "status": status,
            "mapping": map.mapping,
            "residue_absent": map.residue_absent,
            "database_dropped": false,
            "database_observed": database_observed,
            "pid_recorded": pid_recorded,
            "pid_absent": pid_absent
        }),
    );
    Ok(CloseOutcome {
        status: status.to_owned(),
        pid_recorded,
        pid_absent,
        database_dropped: false,
        database_observed,
        mapping: map.mapping,
        residue_absent: map.residue_absent,
    })
}

struct MapClose {
    mapping: String,
    residue_absent: bool,
}

pub(crate) fn write_active_map(work: &Path, session_sha256: &str) -> Result<(), ConfigError> {
    fs::create_dir_all(work).map_err(|_| ConfigError::new("mapping", "the proxy map could not be written"))?;
    let path = work.join("proxy-session-map.json");
    if path.exists() {
        fs::remove_file(&path).map_err(|_| ConfigError::new("mapping", "the proxy map could not be written"))?;
    }
    let body = json!({
        "protocol": "oo.minimal-product-loop.proxy-map/v1",
        "state": "active",
        "session_sha256": session_sha256,
        "development_local": true
    });
    fs::write(&path, serde_json::to_vec_pretty(&body).unwrap_or_default())
        .map_err(|_| ConfigError::new("mapping", "the proxy map could not be written"))?;
    Ok(())
}

fn revoke_proxy_map(work: &Path) -> MapClose {
    let path = work.join("proxy-session-map.json");
    let marker = work.join("proxy-marker");
    if !path.is_file() {
        return MapClose { mapping: "absent".to_owned(), residue_absent: false };
    }
    let _ = fs::remove_file(&path);
    if marker.is_file() {
        let _ = fs::remove_file(&marker);
    }
    let residue_absent = !path.exists() && !marker.exists();
    MapClose {
        mapping: if residue_absent { "revoked".to_owned() } else { "residue".to_owned() },
        residue_absent,
    }
}

pub(crate) fn identity(pid: u32) -> Option<ProcId> {
    let comm_path = format!("/proc/{pid}/comm");
    let stat_path = format!("/proc/{pid}/stat");
    if let (Ok(comm), Ok(stat)) = (fs::read_to_string(&comm_path), fs::read_to_string(&stat_path))
        && let Some(start) = parse_proc_stat(&stat)
    {
        return Some(ProcId { comm: comm.trim().to_owned(), start });
    }
    let comm = Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output().ok()?;
    let start = Command::new("ps").args(["-o", "lstart=", "-p", &pid.to_string()]).output().ok()?;
    if !comm.status.success() || !start.status.success() {
        return None;
    }
    let comm = String::from_utf8(comm.stdout).ok()?.trim().to_owned();
    let start = String::from_utf8(start.stdout).ok()?.trim().to_owned();
    if comm.is_empty() || start.is_empty() {
        None
    } else {
        Some(ProcId { comm, start })
    }
}

fn parse_proc_stat(stat: &str) -> Option<String> {
    let rest = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    fields.get(19).map(|value| (*value).to_owned())
}

#[cfg(test)]
mod tests {
    use super::{cancel_admission, parse_proc_stat};

    #[test]
    fn cancel_refuses_without_a_live_ask() {
        assert_eq!(cancel_admission("released", false, false), Err("cancel.refused"));
        assert_eq!(cancel_admission("cancelled", true, true), Err("cancel.refused"));
        assert_eq!(cancel_admission("draft_ready", false, false), Err("cancel.refused"));
        assert_eq!(cancel_admission("draft_ready", true, false), Err("cancel.refused"));
        assert_eq!(cancel_admission("draft_ready", true, true), Ok("cancelled"));
    }

    #[test]
    fn starttime_is_field_22() {
        let mut fields = vec!["R"];
        while fields.len() < 19 {
            fields.push("0");
        }
        fields.push("999");
        let stat = format!("12 (pi agent) {}", fields.join(" "));
        assert_eq!(parse_proc_stat(&stat).as_deref(), Some("999"));
    }
}
