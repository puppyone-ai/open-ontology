//! Cross-process cancel. This process writes the request. The query process
//! owns `CancelHandle` and is the only caller of `cancel_query`.

use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::authority::load_authority;
use crate::durable::{execution_id_ok, refuse_symlink, write_durable};
use crate::output::{finished_error, finished_value, shell, Fail, Finished};
use crate::CANCEL_PROTOCOL;

pub fn cancel(authority_path: &Path, execution: &str) -> Finished {
    match cancel_value(authority_path, execution) {
        Ok(value) => finished_value(None, &value),
        Err((fail, Some(value))) => finished_value(Some(fail), &value),
        Err((fail, None)) => finished_error(fail),
    }
}

fn cancel_value(authority_path: &Path, execution: &str) -> Result<Value, (Fail, Option<Value>)> {
    if !execution_id_ok(execution) {
        return Err((Fail::Request, None));
    }
    let authority = load_authority(authority_path).map_err(|fail| (fail, None))?;
    let state_path = authority
        .state_dir
        .join("executions")
        .join(format!("{execution}.json"));
    if !state_path.is_file() {
        return Err((Fail::NotSent, Some(not_sent(&authority, execution, "absent"))));
    }
    refuse_symlink(&state_path).map_err(|fail| (fail, None))?;
    let current = read_state(&state_path).map_err(|fail| (fail, None))?;
    let status = current.get("status").and_then(Value::as_str).unwrap_or("");
    if status != "running" {
        return Err((
            Fail::NotSent,
            Some(not_sent(&authority, execution, "late")),
        ));
    }
    let request = json!({
        "protocol": "oo.operator-preview.cancel-request/v1",
        "execution_id": execution,
    });
    let bytes = serde_json::to_vec(&request).map_err(|_| (Fail::Io, None))?;
    write_durable(
        &authority.state_dir.join("cancel"),
        &format!("{execution}.request"),
        &bytes,
    )
    .map_err(|fail| (fail, None))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let state = read_state(&state_path).map_err(|fail| (fail, None))?;
        let mark = state.get("cancel").and_then(Value::as_str).unwrap_or("none");
        if mark == "sent" {
            let mut fields = Map::new();
            fields.insert("protocol".to_string(), json!(CANCEL_PROTOCOL));
            fields.insert("execution_id".to_string(), json!(execution));
            fields.insert("outcome".to_string(), json!("sent"));
            fields.insert(
                "generation".to_string(),
                state.get("generation").cloned().unwrap_or(Value::Null),
            );
            fields.insert(
                "sqlstate".to_string(),
                state.get("sqlstate").cloned().unwrap_or(Value::Null),
            );
            fields.insert(
                "rollback_confirmed".to_string(),
                state
                    .get("rollback_confirmed")
                    .cloned()
                    .unwrap_or(Value::Bool(false)),
            );
            return Ok(shell(fields));
        }
        if mark == "not_sent" || state.get("status").and_then(Value::as_str) != Some("running")
        {
            return Err((
                Fail::NotSent,
                Some(not_sent(&authority, execution, "not_sent")),
            ));
        }
        if Instant::now() >= deadline {
            return Err((Fail::Query, Some(not_sent(&authority, execution, "timeout"))));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn not_sent(authority: &crate::authority::Authority, execution: &str, reason: &str) -> Value {
    let _ = authority;
    let mut fields = Map::new();
    fields.insert("protocol".to_string(), json!(CANCEL_PROTOCOL));
    fields.insert("execution_id".to_string(), json!(execution));
    fields.insert("outcome".to_string(), json!("not_sent"));
    fields.insert("reason".to_string(), json!(reason));
    shell(fields)
}

fn read_state(path: &Path) -> Result<Value, Fail> {
    let bytes = fs::read(path).map_err(|_| Fail::Io)?;
    if bytes.len() > 1_048_576 {
        return Err(Fail::Request);
    }
    serde_json::from_slice(&bytes).map_err(|_| Fail::Io)
}
