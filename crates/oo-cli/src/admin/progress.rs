//! Persisted progress for the bounded, unreviewed PostgreSQL draft bridge.
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, IsTerminal, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Failure, Invocation, wizard::Screen};

pub(super) fn state_dir(config: &Path) -> Result<PathBuf, Failure> {
    let config = std::fs::canonicalize(config).map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
    Ok(config
        .parent()
        .ok_or_else(|| Failure::local("ADMIN_CONFIG_IO"))?
        .join(".oo"))
}

pub(super) fn status(config: &Path, json_output: bool) -> Result<Invocation, Failure> {
    let root = state_dir(config)?;
    if root
        .symlink_metadata()
        .is_ok_and(|m| !m.is_dir() || m.mode() & 0o777 != 0o700)
    {
        return Err(Failure::local("BUILD_STATE_DIRECTORY"));
    }
    if root.join("chain.json").exists() {
        return super::build::status(&root, json_output);
    }
    let path = root.join("run.json");
    if !path
        .try_exists()
        .map_err(|_| Failure::local("BUILD_STATE_IO"))?
    {
        return Ok(Invocation::AdminReport(
            json!({"protocol":"oo.build-progress/v1","status":"not_started",
                "last_success":null,"last_failure":null,"next_due":null,
                "nearest_credential_expiry":null,"gateway_health":"unmeasured"})
            .to_string(),
            0,
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Failure::local("BUILD_STATE_IO"))?;
    let meta = file
        .metadata()
        .map_err(|_| Failure::local("BUILD_STATE_IO"))?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.mode() & 0o777 != 0o600
        || meta.len() > 1_048_576
    {
        return Err(Failure::local("BUILD_STATE_IO"));
    }
    let mut text = String::new();
    file.take(1_048_577)
        .read_to_string(&mut text)
        .map_err(|_| Failure::local("BUILD_STATE_IO"))?;
    let mut state: Value =
        serde_json::from_str(&text).map_err(|_| Failure::local("BUILD_STATE_PROTOCOL"))?;
    if state["protocol"] != "oo.build-progress/v1" {
        return Err(Failure::local("BUILD_STATE_PROTOCOL"));
    }
    state["gateway_health"] = json!("unmeasured");
    state["nearest_credential_expiry"] = Value::Null;
    let failed = state["status"] == "failed";
    Ok(Invocation::AdminReport(
        if json_output {
            state.to_string()
        } else {
            display(&state)
        },
        if failed { 1 } else { 0 },
    ))
}

pub(super) fn display(state: &Value) -> String {
    let mut text = format!(
        "Execution: {}\n\n",
        state["status"].as_str().unwrap_or("unknown")
    );
    if let Some(stages) = state["stages"].as_array() {
        for stage in stages {
            let rate = stage["operations_per_second"]
                .as_f64()
                .map(|n| format!("{n:.2} ops/s"))
                .unwrap_or_else(|| "unmeasured".into());
            let attempts = stage["attempts"].as_u64().unwrap_or(0);
            text.push_str(&format!(
                "{}: {}  {}/{}  throughput {}  retries {}\n",
                stage["name"].as_str().unwrap_or("unknown"),
                stage["status"].as_str().unwrap_or("unknown"),
                stage["completed"].as_u64().unwrap_or(0),
                stage["total"].as_u64().unwrap_or(1),
                rate,
                attempts.saturating_sub(1)
            ));
            if let Some(error) = stage["error"].as_str() {
                text.push_str(&format!("  Failure: {error}\n"));
            }
        }
    }
    text.push_str(&format!(
        "\nRow throughput: unmeasured. Agent DB load: {}.\n",
        state["agent_db_load"].as_str().unwrap_or("unmeasured")
    ));
    text.push_str("\
        q / Esc / Ctrl-C: stop; saved state is retained.\n\
        Inspect: oo status --config <same stack>\nResume: oo build --resume --config <same stack>\n");
    text
}

pub(super) fn completion(state: &Value) -> String {
    format!(
        "{}\nDraft: {}\nOntology export path: pending export implementation\n\
        Pack path: pending owner review and publication\nGateway URL: pending publication\n\
        First token: pending publication; use the Gateway enroll flow, never display a long-lived token here\n\
        Agent prompt: pending publication and share-prompt CLI integration\n\
        No output was reviewed or published through Gateway.",
        display(state),
        if state["status"] == "draft" {
            ".oo/draft.json (unreviewed)"
        } else {
            "not produced"
        }
    )
}

pub(super) fn build(
    config: &Path,
    stack: &oo_stack::Stack,
    resume: bool,
    json_output: bool,
    agent_target: Option<&Path>,
) -> Result<Invocation, Failure> {
    let exe = std::env::current_exe().map_err(|_| Failure::local("BUILD_INSTALL"))?;
    let mut body = json!({"stack":stack,"state_dir":state_dir(config)?,
        "bin_dir":exe.parent(),"resume":resume,
        "egress_code":include_str!("../../../../tools/model-egress/model_egress.py")});
    if let Some(target) = agent_target {
        body["load_target"] =
            json!(std::fs::canonicalize(target).map_err(|_| Failure::local("BUILD_TARGET_IO"))?);
    }
    let mut command = Command::new("python3");
    command
        .args(["-c", include_str!("../../../../deploy/lite/draft.py")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().map_err(|_| Failure::local("BUILD_START"))?;
    let result = (|| {
        child
            .stdin
            .take()
            .ok_or_else(|| Failure::local("BUILD_STDIN"))?
            .write_all(body.to_string().as_bytes())
            .map_err(|_| Failure::local("BUILD_STDIN"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Failure::local("BUILD_STDOUT"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.len() > 1_048_576 {
                    break;
                }
                if let Ok(state) = serde_json::from_str::<Value>(&line)
                    && tx.send(state).is_err()
                {
                    break;
                }
            }
        });
        let mut screen =
            if !json_output && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
                Some(Screen::open()?)
            } else {
                None
            };
        let mut latest = json!({"status":"starting"});
        loop {
            while let Ok(state) = rx.try_recv() {
                latest = state;
            }
            if let Some(screen) = &mut screen
                && screen.progress(&display(&latest))?
            {
                return Err(Failure::local(
                    "BUILD_INTERRUPTED: use oo status and oo build --resume",
                ));
            }
            if let Some(exit) = child.try_wait().map_err(|_| Failure::local("BUILD_WAIT"))? {
                // The reader closes its channel after draining the child's pipe.
                for state in rx {
                    latest = state;
                }
                if let Some(screen) = &mut screen {
                    screen.prompt(7, &completion(&latest), "", false)?;
                }
                return Ok(Invocation::AdminReport(
                    if json_output {
                        latest.to_string()
                    } else {
                        completion(&latest)
                    },
                    if exit.success() { 0 } else { 1 },
                ));
            }
            std::thread::sleep(Duration::from_millis(80));
        }
    })();
    if child.try_wait().ok().flatten().is_none() {
        let group = format!("-{}", child.id());
        let _ = Command::new("/bin/kill")
            .args(["-TERM", "--", &group])
            .status();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until && child.try_wait().ok().flatten().is_none() {
            std::thread::sleep(Duration::from_millis(50));
        }
        if child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--", &group])
                .status();
        }
        let _ = child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn screens_distinguish_work_rates_retries_and_unpublished_outputs() {
        let state = json!({"status":"failed","token":"must-not-render",
            "stages":[{"name":"generation","status":"failed","attempts":3,"completed":0,
            "total":1,"error":"DRAFT_NOT_VALIDATED","rows_per_second":null}]});
        let text = completion(&state);
        assert!(text.contains("retries 2"));
        assert!(text.contains("throughput unmeasured"));
        assert!(text.contains("Failure: DRAFT_NOT_VALIDATED"));
        assert!(text.contains("Pack path: pending"));
        assert!(text.contains("enroll"));
        assert!(text.contains("share-prompt"));
        assert!(!text.contains("must-not-render"));
    }
}
