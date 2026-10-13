//! Loop state. No secret bytes.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config::Config;

pub fn state_path(config: &Config) -> PathBuf {
    config.work.join("state.json")
}

pub fn write_state(config: &Config, value: &Value) -> Result<(), &'static str> {
    fs::create_dir_all(&config.work).map_err(|_| "state")?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "state")?;
    fs::write(state_path(config), bytes).map_err(|_| "state")
}

pub fn read_state(config: &Config) -> Result<Value, &'static str> {
    let text = fs::read_to_string(state_path(config)).map_err(|_| "state.absent")?;
    serde_json::from_str(&text).map_err(|_| "state")
}

pub(crate) fn merge_state(config: &Config, patch: &Value) -> Result<(), &'static str> {
    let mut state = read_state(config).unwrap_or_else(|_| base(config, "blocked"));
    let Some(extra) = patch.as_object() else {
        return Err("state");
    };
    let Some(map) = state.as_object_mut() else {
        return Err("state");
    };
    for (key, value) in extra {
        map.insert(key.clone(), value.clone());
    }
    map.insert("parent_acceptance".to_owned(), Value::Bool(false));
    map.insert("goal_acceptance".to_owned(), Value::Bool(false));
    map.insert("publication".to_owned(), Value::String("not_published".to_owned()));
    map.insert("llm_review".to_owned(), Value::String("unreviewed".to_owned()));
    write_state(config, &state)
}

pub fn base(config: &Config, status: &str) -> Value {
    json!({
        "protocol": crate::PROTOCOL,
        "status": status,
        "mode": config.mode,
        "publication": "not_published",
        "llm_review": "unreviewed",
        "development_preview": true,
        "runtime": crate::DEVELOPMENT_ACK,
        "llm_transport": "development_local",
        "llm_user_agent": crate::forward::LLM_USER_AGENT,
        "llm_deadline_ms": crate::forward::LLM_DEADLINE_MS,
        "llm_cost": "unknown",
        "parent_acceptance": false,
        "goal_acceptance": false
    })
}
