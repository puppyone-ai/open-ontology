//! `oo docs upload` starts `oo serve` when none is running (D83). The runtime
//! choice is `oo serve`'s own (`--runtime auto`); this only runs it detached with the
//! same state directory and waits until `serve.json` answers. `--no-start`
//! turns it off. A running daemon is reused: `oo serve` itself prints the
//! existing serve.json and exits 0 when its lock is taken.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::client::{CliError, exit, read_connection};

/// How long a cold start may take: initdb, the schema, the parser sidecar's
/// first import, or Docker's containers.
const START_DEADLINE: Duration = Duration::from_secs(600);

/// Make sure a daemon answers for `state`. `Ok(true)` when this call started one.
pub fn ensure(state: &Path, no_start: bool) -> Result<bool, CliError> {
    if alive(state) {
        return Ok(false);
    }
    if no_start {
        return read_connection(state).map(|_| false);
    }
    let exe = std::env::current_exe().map_err(|_| {
        CliError::new(
            exit::OTHER,
            "DOCS_SERVE_START_FAILED",
            "cannot find the oo executable",
        )
    })?;
    super::secrets::ensure_state_dir(state)
        .map_err(|e| CliError::new(exit::OTHER, "DOCS_SERVE_START_FAILED", e.0))?;
    let log_path = state.join("serve-autostart.log");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| {
            CliError::new(
                exit::OTHER,
                "DOCS_SERVE_START_FAILED",
                format!("{}: {e}", log_path.display()),
            )
        })?;
    eprintln!(
        "oo: no oo serve is running; starting one (state {})",
        state.display()
    );
    let mut child = Command::new(exe)
        .args(["serve", "--json", "--port", "0", "--state-dir"])
        .arg(state)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().map_err(|e| {
            CliError::new(exit::OTHER, "DOCS_SERVE_START_FAILED", e.to_string())
        })?))
        .stderr(Stdio::from(log))
        .process_group(0) // survives this command and its terminal
        .spawn()
        .map_err(|e| {
            CliError::new(
                exit::OTHER,
                "DOCS_SERVE_START_FAILED",
                format!("oo serve: {e}"),
            )
        })?;
    let deadline = Instant::now() + START_DEADLINE;
    loop {
        if alive(state) {
            return Ok(true);
        }
        if let Ok(Some(status)) = child.try_wait() {
            let tail = std::fs::read_to_string(&log_path).unwrap_or_default();
            let lines: Vec<&str> = tail.lines().rev().take(4).collect();
            let tail = lines.into_iter().rev().collect::<Vec<_>>().join(" | ");
            return Err(CliError::new(
                exit::ENGINE_UNAVAILABLE,
                "DOCS_SERVE_START_FAILED",
                format!("oo serve exited ({status}): {tail}"),
            ));
        }
        if Instant::now() > deadline {
            return Err(CliError::new(
                exit::ENGINE_UNAVAILABLE,
                "DOCS_SERVE_START_FAILED",
                format!(
                    "oo serve did not come up in 600 s; see {}",
                    log_path.display()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// serve.json is valid and its port accepts a connection.
fn alive(state: &Path) -> bool {
    read_connection(state).is_ok_and(|(port, _)| {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(500),
        )
        .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_start_reports_the_missing_daemon() {
        let d = std::env::temp_dir().join(format!("oo-as-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let e = ensure(&d, true).unwrap_err();
        assert_eq!(e.exit, exit::ENGINE_UNAVAILABLE);
        let _ = std::fs::remove_dir_all(&d);
    }
}
