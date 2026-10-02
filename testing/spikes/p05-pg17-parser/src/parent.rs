use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Deserialize;
use std::os::unix::process::CommandExt;

use crate::decision::Decision;
use crate::limits::{PARSE_DEADLINE, SPAWN_BUDGET};
use crate::policy::Policy;
use crate::protocol::{read_frame, write_frame, Frame, KIND_ADMIT, KIND_READY, KIND_RESPONSE};
use crate::worker::proc_starttime;

/// Run admission in a separate worker. Oversize SQL is rejected before spawn.
pub fn admit_in_worker(worker_bin: &Path, sql: &str, policy: &Policy) -> Decision {
    if sql.is_empty() {
        return Decision::deny("EMPTY_SQL");
    }
    if sql.as_bytes().contains(&0) {
        return Decision::deny("NUL_BYTE");
    }
    if sql.len() > crate::limits::MAX_SQL {
        return Decision::deny("OVERSIZE");
    }
    let body = match serde_json::to_vec(&serde_json::json!({"sql": sql, "policy": policy})) {
        Ok(body) => body,
        Err(_) => return Decision::deny("INVALID_OUTPUT"),
    };
    exercise_worker(worker_bin, KIND_ADMIT, &body)
}

/// Send one frame to a fresh worker. Test hooks are honored only by a worker built with that feature.
pub fn exercise_worker(worker_bin: &Path, kind: u16, body: &[u8]) -> Decision {
    let nonce = match fresh_nonce() {
        Ok(nonce) => nonce,
        Err(_) => return Decision::deny("SPAWN_FAILED"),
    };
    let work = match Workdir::create(&nonce) {
        Ok(work) => work,
        Err(_) => return Decision::deny("SPAWN_FAILED"),
    };
    let mut child = match Command::new(worker_bin)
        .arg(&nonce)
        .current_dir(&work.path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return settle(Decision::deny("SPAWN_FAILED"), None, 0, false, false, work),
    };
    let pid = child.id();
    let _stderr = child.stderr.take().map(drain_stderr);
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return settle(Decision::deny("SPAWN_FAILED"), Some(&mut child), pid, false, true, work),
    };
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        loop {
            match read_frame(&mut stdout) {
                Ok(frame) => {
                    if tx.send(Ok(frame)).is_err() {
                        break;
                    }
                }
                Err(code) => {
                    let _ = tx.send(Err(code));
                    break;
                }
            }
        }
    });
    let ready = match recv_frame(&rx, SPAWN_BUDGET) {
        Recv::Frame(frame) => frame,
        Recv::Proto(code) => {
            drop(rx);
            let _ = reader.join();
            return settle(Decision::deny(code), Some(&mut child), pid, false, false, work);
        }
        Recv::Timeout => {
            // Kill before joining the reader. The worker has not closed stdout.
            kill_group(pid);
            drop(rx);
            let _ = reader.join();
            return settle(Decision::deny("SPAWN_FAILED"), Some(&mut child), pid, false, true, work);
        }
    };
    if ready.kind != KIND_READY {
        kill_group(pid);
        drop(rx);
        let _ = reader.join();
        return settle(Decision::deny("INVALID_OUTPUT"), Some(&mut child), pid, false, true, work);
    }
    let identity = match serde_json::from_slice::<ReadyBody>(&ready.body) {
        Ok(identity) => identity,
        Err(_) => {
            kill_group(pid);
            drop(rx);
            let _ = reader.join();
            return settle(Decision::deny("INVALID_OUTPUT"), Some(&mut child), pid, false, true, work);
        }
    };
    if !identity_ok(&identity, &nonce, pid) {
        kill_group(pid);
        drop(rx);
        let _ = reader.join();
        return settle(Decision::deny("INVALID_OUTPUT"), Some(&mut child), pid, false, true, work);
    }
    let starttime_matched = proc_starttime(pid).is_some();
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            kill_group(pid);
            drop(rx);
            let _ = reader.join();
            return settle(Decision::deny("SPAWN_FAILED"), Some(&mut child), pid, starttime_matched, true, work);
        }
    };
    if write_frame(&mut stdin, kind, body).is_err() {
        drop(stdin);
        kill_group(pid);
        drop(rx);
        let _ = reader.join();
        return settle(Decision::deny("INVALID_OUTPUT"), Some(&mut child), pid, starttime_matched, true, work);
    }
    let response = match recv_frame(&rx, PARSE_DEADLINE) {
        Recv::Frame(frame) => frame,
        Recv::Proto(code) => {
            drop(stdin);
            drop(rx);
            let _ = reader.join();
            // Do not kill first. A signal here is the worker's own abnormal exit.
            // Exit 0 with a broken frame stays `code` (invalid output).
            return settle(Decision::deny(code), Some(&mut child), pid, starttime_matched, false, work);
        }
        Recv::Timeout => {
            drop(stdin);
            // The late frame is discarded with `rx`. Kill first so the reader can observe EOF.
            kill_group(pid);
            drop(rx);
            let _ = reader.join();
            return settle(Decision::deny("DEADLINE"), Some(&mut child), pid, starttime_matched, true, work);
        }
    };
    drop(stdin);
    drop(rx);
    let _ = reader.join();
    if response.kind != KIND_RESPONSE {
        return settle(Decision::deny("INVALID_OUTPUT"), Some(&mut child), pid, starttime_matched, true, work);
    }
    let mut decision = match serde_json::from_slice::<Decision>(&response.body) {
        Ok(decision) => decision,
        Err(_) => {
            return settle(Decision::deny("INVALID_OUTPUT"), Some(&mut child), pid, starttime_matched, true, work);
        }
    };
    decision.enforce_parent(&nonce, pid);
    settle(decision, Some(&mut child), pid, starttime_matched, false, work)
}

#[derive(Deserialize)]
struct ReadyBody {
    nonce: String,
    pid: u32,
    starttime: String,
}

fn identity_ok(identity: &ReadyBody, nonce: &str, pid: u32) -> bool {
    if identity.nonce != nonce || identity.pid != pid || pid == 0 {
        return false;
    }
    match proc_starttime(pid) {
        Some(observed) => !observed.is_empty() && observed == identity.starttime,
        None => true,
    }
}

enum Recv {
    Frame(Frame),
    Proto(&'static str),
    Timeout,
}

fn recv_frame(rx: &mpsc::Receiver<Result<Frame, &'static str>>, budget: Duration) -> Recv {
    match rx.recv_timeout(budget) {
        Ok(Ok(frame)) => Recv::Frame(frame),
        Ok(Err(code)) => Recv::Proto(code),
        Err(_) => Recv::Timeout,
    }
}

/// `expect_killed` means this parent is about to SIGKILL a live worker on purpose.
/// That signal must not replace an explicit deny such as DEADLINE.
fn settle(
    mut decision: Decision,
    child: Option<&mut Child>,
    pid: u32,
    starttime_matched: bool,
    expect_killed: bool,
    mut work: Workdir,
) -> Decision {
    if expect_killed && pid != 0 {
        kill_group(pid);
    }
    let mut failed = false;
    if let Some(child) = child {
        match child.wait() {
            Ok(status) => failed = !status.success(),
            Err(_) => {
                decision.syntax_allow = false;
                decision.code = "ABNORMAL_EXIT".to_string();
                decision.reaped = false;
                decision.workdir_removed = work.remove();
                decision.starttime_matched = starttime_matched;
                decision.executed_sql.clear();
                return decision;
            }
        }
    }
    if failed {
        decision.syntax_allow = false;
        if !expect_killed {
            decision.code = "ABNORMAL_EXIT".to_string();
        }
    }
    decision.reaped = pid != 0 && process_gone(pid);
    decision.workdir_removed = work.remove();
    decision.starttime_matched = starttime_matched;
    if !decision.syntax_allow {
        decision.executed_sql.clear();
    }
    decision
}

fn process_gone(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as i32, 0) };
    rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

fn kill_group(pid: u32) {
    if pid == 0 {
        return;
    }
    // The child is the leader of its own process group. Negative pid signals that group.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

fn drain_stderr(mut stderr: impl Read + Send + 'static) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut stored = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            match stderr.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => {
                    if stored.len() < 8192 {
                        let take = n.min(8192 - stored.len());
                        stored.extend_from_slice(&tmp[..take]);
                    }
                }
                Err(_) => break,
            }
        }
        stored
    })
}

struct Workdir {
    path: std::path::PathBuf,
    removed: bool,
}

impl Workdir {
    fn create(nonce: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("oo-p05-{nonce}"));
        fs::create_dir(&path)?;
        Ok(Self { path, removed: false })
    }

    fn remove(&mut self) -> bool {
        if self.removed {
            return !self.path.exists();
        }
        let removed = fs::remove_dir_all(&self.path).is_ok() || !self.path.exists();
        self.removed = removed;
        !self.path.exists()
    }
}

impl Drop for Workdir {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

fn fresh_nonce() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
