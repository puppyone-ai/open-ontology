//! Durable local checkpoint. The PostgreSQL row stores the evidence hash.
//! This file stores the preimage the next process needs, including the wall epoch.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const CHECKPOINT_CONTRACT: &str = "session-release-checkpoint-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessCheckpoint {
    pub pid: u32,
    pub pgid: u32,
    pub starttime: u64,
    pub signaled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub contract: String,
    pub project_id: String,
    pub session_id: String,
    pub owner_generation: u64,
    pub pre_revision: u64,
    pub durable_revision_after_cancel: u64,
    pub t0_wall_epoch_ms: u64,
    pub cancel_event_id: String,
    pub backend_pid: i32,
    pub application_name: String,
    pub query_role: String,
    pub harness: Option<HarnessCheckpoint>,
}

pub fn save(dir: &Path, checkpoint: &Checkpoint) -> Result<(), &'static str> {
    if checkpoint.contract != CHECKPOINT_CONTRACT {
        return Err("checkpoint_contract");
    }
    let bytes = serde_json::to_vec(checkpoint).map_err(|_| "checkpoint_json")?;
    let temp = dir.join("checkpoint.json.tmp");
    let final_path = dir.join("checkpoint.json");
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp)
            .map_err(|_| "checkpoint_io")?;
        file.write_all(&bytes).map_err(|_| "checkpoint_io")?;
        file.sync_all().map_err(|_| "checkpoint_io")?;
    }
    fs::rename(&temp, &final_path).map_err(|_| "checkpoint_io")?;
    if let Ok(dir_file) = File::open(dir) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}

pub fn load(dir: &Path) -> Result<Checkpoint, &'static str> {
    let bytes = fs::read(dir.join("checkpoint.json")).map_err(|_| "checkpoint_missing")?;
    serde_json::from_slice(&bytes).map_err(|_| "checkpoint_invalid")
}

pub fn marker(dir: &Path, name: &str) -> Result<(), &'static str> {
    if name != "cancel_persisted" && name != "harness_armed" {
        return Err("marker");
    }
    let path = dir.join(name);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|_| "marker_io")?;
    file.write_all(b"1\n").map_err(|_| "marker_io")?;
    file.sync_all().map_err(|_| "marker_io")?;
    Ok(())
}
