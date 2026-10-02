//! Parent side of the parser IPC.
//!
//! The native parser is not called here. Untrusted worker output is not repaired
//! by an in-process parse. `syntax_allow` is not authorization.
//!
//! Production admission execs only a memfd whose bytes match the caller pin.
//! Test hooks are separate: `exercise_worker` never keeps an allow, and
//! `exercise_untrusted` runs a fault image only when this crate is built with
//! `test-hooks`. Clearing the environment and using a temp directory does not
//! isolate the network, credentials, or the mounted source tree.

use std::fs;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::decision::{accept_worker_decision, Decision};
use crate::identity::{self, ImageError};
use crate::limits::{CLEANUP_BUDGET, MAX_STDOUT_BYTES, PARSE_DEADLINE, SPAWN_BUDGET, WORKDIR_BYTE_BUDGET};
use crate::policy::Policy;
use crate::procinfo::{self, Desc};
use crate::protocol::{write_frame, KIND_ADMIT, KIND_READY, KIND_RESPONSE};
use crate::sha256::sha256_file;

const STDERR_STORE: usize = 8192;
const PROOF_CAP: usize = 32 * 1024;

/// Run admission in a pinned worker. Oversize, empty, and NUL SQL are rejected
/// before any process is spawned. The pin is the caller's trust root.
pub fn admit_in_worker(worker_bin: &Path, pin: &[u8; 32], sql: &str, policy: &Policy) -> Decision {
    if let Some(early) = sql_precheck(sql) {
        return early;
    }
    let supervisor = match identity::sealed_pinned(worker_bin, pin) {
        Ok(fd) => fd,
        Err(error) => return image_decision(error),
    };
    let worker = match supervisor.try_clone() {
        Ok(fd) => fd,
        Err(_) => return Decision::deny("SPAWN_FAILED"),
    };
    let body = match serde_json::to_vec(&serde_json::json!({"sql": sql, "policy": policy})) {
        Ok(body) => body,
        Err(_) => return Decision::deny("INVALID_OUTPUT"),
    };
    launch(Launch {
        supervisor,
        worker,
        kind: KIND_ADMIT,
        body,
        fault: None,
        identity: "pinned",
        keep_allow: true,
    })
}

/// Exercise the official worker binary. The image is the nofollow bytes of
/// `worker_bin`, checked against a hash of that same path. That self-hash is
/// not an external trust root. This hook never returns `syntax_allow`.
pub fn exercise_worker(worker_bin: &Path, kind: u16, body: &[u8]) -> Decision {
    let Ok(pin) = sha256_file(worker_bin) else {
        return Decision::deny("SPAWN_FAILED");
    };
    let supervisor = match identity::sealed_pinned(worker_bin, &pin) {
        Ok(fd) => fd,
        Err(error) => return image_decision(error),
    };
    let worker = match supervisor.try_clone() {
        Ok(fd) => fd,
        Err(_) => return Decision::deny("SPAWN_FAILED"),
    };
    launch(Launch {
        supervisor,
        worker,
        kind,
        body: body.to_vec(),
        fault: None,
        identity: "test-hooks-kind",
        keep_allow: false,
    })
}

/// Fault-worker hook. The supervisor image is pinned. The fault image is not
/// treated as trusted. Built only with `test-hooks`. Production workers reject
/// `allow-fault` and this function is not compiled into that feature set.
#[cfg(feature = "test-hooks")]
pub fn exercise_untrusted(supervisor_bin: &Path, fault_bin: &Path, mode: &str) -> Decision {
    let Ok(pin) = sha256_file(supervisor_bin) else {
        return Decision::deny("SPAWN_FAILED");
    };
    let supervisor = match identity::sealed_pinned(supervisor_bin, &pin) {
        Ok(fd) => fd,
        Err(error) => return image_decision(error),
    };
    let worker = match identity::sealed_unpinned(fault_bin) {
        Ok(fd) => fd,
        Err(error) => return image_decision(error),
    };
    launch(Launch {
        supervisor,
        worker,
        kind: KIND_ADMIT,
        body: b"{}".to_vec(),
        fault: Some(mode.to_string()),
        identity: "untrusted-test-hook",
        keep_allow: false,
    })
}

struct Launch {
    supervisor: OwnedFd,
    worker: OwnedFd,
    kind: u16,
    body: Vec<u8>,
    fault: Option<String>,
    identity: &'static str,
    keep_allow: bool,
}

fn image_decision(error: ImageError) -> Decision {
    match error {
        ImageError::Untrusted => Decision::deny("UNTRUSTED_WORKER"),
        ImageError::Spawn(error) => Decision::deny("SPAWN_FAILED").with_detail(error.to_string()),
    }
}

fn sql_precheck(sql: &str) -> Option<Decision> {
    if sql.is_empty() {
        return Some(Decision::deny("EMPTY_SQL"));
    }
    if sql.as_bytes().contains(&0) {
        return Some(Decision::deny("NUL_BYTE"));
    }
    if sql.len() > crate::limits::MAX_SQL {
        return Some(Decision::deny("OVERSIZE"));
    }
    None
}

fn launch(launch: Launch) -> Decision {
    ignore_sigpipe();
    let nonce = match fresh_nonce() {
        Ok(nonce) => nonce,
        Err(_) => return Decision::deny("SPAWN_FAILED"),
    };
    let mut work = match Workdir::create(&nonce) {
        Ok(work) => work,
        Err(_) => return Decision::deny("SPAWN_FAILED"),
    };
    let pipes = match Pipes::open() {
        Ok(pipes) => pipes,
        Err(_) => {
            let _ = work.remove();
            return Decision::deny("SPAWN_FAILED");
        }
    };
    let supervisor_fd = launch.supervisor.as_raw_fd();
    let worker_fd = launch.worker.as_raw_fd();
    let ctrl_fd = pipes.ctrl_r.as_raw_fd();
    let in_fd = pipes.in_r.as_raw_fd();
    let out_fd = pipes.out_w.as_raw_fd();
    let err_fd = pipes.err_w.as_raw_fd();
    let proof_fd = pipes.proof_w.as_raw_fd();
    let mut cmd = Command::new(format!("/proc/self/fd/{supervisor_fd}"));
    cmd.arg("--supervise")
        .arg(worker_fd.to_string())
        .arg(ctrl_fd.to_string())
        .arg(in_fd.to_string())
        .arg(out_fd.to_string())
        .arg(err_fd.to_string())
        .arg(proof_fd.to_string())
        .arg(&nonce)
        .arg(work.path.as_os_str())
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(mode) = &launch.fault {
        cmd.arg("allow-fault").arg(mode);
    }
    let kept = [supervisor_fd, worker_fd, ctrl_fd, in_fd, out_fd, err_fd, proof_fd];
    // SAFETY: runs in the forked supervisor child before exec, and only clears
    // close-on-exec on the descriptors that child must inherit. fcntl is
    // async-signal-safe. No locks are taken.
    unsafe {
        cmd.pre_exec(move || {
            for fd in kept {
                if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => {
            let _ = work.remove();
            return Decision::deny("SPAWN_FAILED");
        }
    };
    drop(launch.supervisor);
    drop(launch.worker);
    drop(pipes.ctrl_r);
    drop(pipes.in_r);
    drop(pipes.out_w);
    drop(pipes.err_w);
    drop(pipes.proof_w);
    let mut live = Live {
        child,
        ctrl_w: pipes.ctrl_w,
        in_w: Some(pipes.in_w),
        out_r: pipes.out_r,
        err_r: pipes.err_r,
        proof_r: pipes.proof_r,
        work,
        nonce,
        kind: launch.kind,
        body: launch.body,
        identity: launch.identity,
        keep_allow: launch.keep_allow,
        finished: false,
        stdout_eof: false,
    };
    set_nonblock(live.out_r.as_raw_fd());
    set_nonblock(live.err_r.as_raw_fd());
    set_nonblock(live.proof_r.as_raw_fd());
    let decision = live.drive();
    live.finish(decision)
}

struct Pipes {
    ctrl_r: OwnedFd,
    ctrl_w: OwnedFd,
    in_r: OwnedFd,
    in_w: OwnedFd,
    out_r: OwnedFd,
    out_w: OwnedFd,
    err_r: OwnedFd,
    err_w: OwnedFd,
    proof_r: OwnedFd,
    proof_w: OwnedFd,
}

impl Pipes {
    fn open() -> io::Result<Self> {
        let (ctrl_r, ctrl_w) = pipe_pair()?;
        let (in_r, in_w) = pipe_pair()?;
        let (out_r, out_w) = pipe_pair()?;
        let (err_r, err_w) = pipe_pair()?;
        let (proof_r, proof_w) = pipe_pair()?;
        Ok(Self { ctrl_r, ctrl_w, in_r, in_w, out_r, out_w, err_r, err_w, proof_r, proof_w })
    }
}

struct Live {
    child: Child,
    ctrl_w: OwnedFd,
    in_w: Option<OwnedFd>,
    out_r: OwnedFd,
    err_r: OwnedFd,
    proof_r: OwnedFd,
    work: Workdir,
    nonce: String,
    kind: u16,
    body: Vec<u8>,
    identity: &'static str,
    keep_allow: bool,
    finished: bool,
    stdout_eof: bool,
}

impl Live {
    fn drive(&mut self) -> Decision {
        let spawned = Instant::now();
        let mut ready_at: Option<Instant> = None;
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        let mut worker_pid = 0u32;
        let mut start_matched = false;
        let mut fail: Option<&'static str> = None;
        let mut parsed: Option<Decision> = None;

        loop {
            if self.read_pipes(&mut stdout_buf, &mut stderr_buf).is_err() {
                fail = Some("INVALID_OUTPUT");
                break;
            }
            if ready_at.is_none() {
                match crate::protocol::parse_prefix(&stdout_buf) {
                    Ok(Some((frame, n))) => {
                        let _ = stdout_buf.drain(..n);
                        if frame.kind != KIND_READY {
                            fail = Some("INVALID_OUTPUT");
                            break;
                        }
                        match screen_ready(&frame.body, &self.nonce, self.child.id()) {
                            Ok(pid) => {
                                worker_pid = pid;
                                start_matched = true;
                                ready_at = Some(Instant::now());
                                if self.write_request().is_err() {
                                    fail = Some("INVALID_OUTPUT");
                                    break;
                                }
                            }
                            Err(code) => {
                                fail = Some(code);
                                break;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(code) => {
                        fail = Some(code);
                        break;
                    }
                }
            }
            if ready_at.is_some() && parsed.is_none() && fail.is_none() {
                match crate::protocol::parse_prefix(&stdout_buf) {
                    Ok(Some((frame, n))) => {
                        if frame.kind != KIND_RESPONSE || stdout_buf.len() != n || self.extra_stdout(&mut stdout_buf) {
                            fail = Some("INVALID_OUTPUT");
                            break;
                        }
                        let body = frame.body;
                        stdout_buf.clear();
                        parsed = Some(accept_worker_decision(&body, &self.nonce, worker_pid));
                        break;
                    }
                    Ok(None) => {
                        if stdout_buf.len() > MAX_STDOUT_BYTES {
                            fail = Some("RESPONSE_CAP");
                            break;
                        }
                    }
                    Err(code) => {
                        fail = Some(code);
                        break;
                    }
                }
            }
            if fail.is_some() || parsed.is_some() {
                break;
            }
            if self.stdout_eof {
                if ready_at.is_none() {
                    fail = Some(if stdout_buf.is_empty() { "SPAWN_FAILED" } else { "INVALID_OUTPUT" });
                } else {
                    fail = Some(if stdout_buf.is_empty() { "ABNORMAL_EXIT" } else { "INVALID_OUTPUT" });
                }
                break;
            }
            let (budget, start) = if let Some(ready) = ready_at {
                (PARSE_DEADLINE, ready)
            } else {
                (SPAWN_BUDGET, spawned)
            };
            if start.elapsed() >= budget {
                fail = Some(if ready_at.is_none() { "SPAWN_FAILED" } else { "DEADLINE" });
                break;
            }
            let left = budget.saturating_sub(start.elapsed());
            let _ = poll_two(self.out_r.as_raw_fd(), self.err_r.as_raw_fd(), millis_i32(left));
        }

        let ready_wait = if let Some(ready) = ready_at {
            ready.saturating_duration_since(spawned)
        } else {
            spawned.elapsed()
        };
        let parse_wait = ready_at.map(|ready| Instant::now().saturating_duration_since(ready)).unwrap_or_default();
        let mut decision = if let Some(code) = fail {
            Decision::deny(code)
        } else if let Some(decision) = parsed {
            decision
        } else {
            Decision::deny("INVALID_OUTPUT")
        };
        decision.ready_wait_ms = as_millis(ready_wait);
        decision.parse_wait_ms = as_millis(parse_wait);
        decision.starttime_matched = start_matched;
        if worker_pid != 0 {
            decision.pid = worker_pid;
        }
        decision
    }

    fn read_pipes(&mut self, stdout_buf: &mut Vec<u8>, stderr_buf: &mut Vec<u8>) -> io::Result<()> {
        let eof = read_capped(self.out_r.as_raw_fd(), stdout_buf, MAX_STDOUT_BYTES.saturating_add(64))?;
        if eof {
            self.stdout_eof = true;
        }
        let _ = drain_stderr(self.err_r.as_raw_fd(), stderr_buf)?;
        Ok(())
    }

    fn extra_stdout(&mut self, buf: &mut Vec<u8>) -> bool {
        if !pending(self.out_r.as_raw_fd()) {
            return false;
        }
        let before = buf.len();
        let _ = read_capped(self.out_r.as_raw_fd(), buf, before.saturating_add(64));
        buf.len() > before
    }

    fn write_request(&mut self) -> io::Result<()> {
        let Some(fd) = self.in_w.as_ref().map(|fd| fd.as_raw_fd()) else {
            return Err(io::Error::other("stdin"));
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, self.kind, &self.body).map_err(|_| io::Error::other("frame"))?;
        write_all_fd(fd, &buf)?;
        self.in_w.take();
        Ok(())
    }

    fn finish(&mut self, mut decision: Decision) -> Decision {
        let started = Instant::now();
        let snapshot = procinfo::descendants(self.child.id());
        let _ = write_all_fd(self.ctrl_w.as_raw_fd(), b"K");
        self.in_w.take();
        // Drop the read ends after the snapshot so a holder unblocks, then wait.
        let exited = bounded_reap(&mut self.child, CLEANUP_BUDGET);
        let status_code = match self.child.try_wait() {
            Ok(Some(status)) => status.code(),
            _ => None,
        };
        decision.cleanup_wait_ms = as_millis(started.elapsed());
        let proof = read_proof(self.proof_r.as_raw_fd());
        let bytes = dir_bytes(&self.work.path).unwrap_or(u64::MAX);
        let removed = self.work.remove();
        let unavailable = status_code == Some(3) || proof.contains("UNAVAILABLE");
        let budget_ok = bytes <= WORKDIR_BYTE_BUDGET;
        let proof_ok = snapshot.as_ref().map(|nodes| proof_holds(&proof, nodes)).unwrap_or(false);
        self.finished = true;
        decision.cleanup_proof = truncate_proof(&proof);
        decision.workdir_removed = removed && !self.work.path.exists();
        decision.worker_identity = self.identity.to_string();
        decision.oid_acl_rls_proven = false;
        if !self.keep_allow && decision.syntax_allow {
            decision.syntax_allow = false;
            decision.executed_sql.clear();
            decision.code = "UNTRUSTED_WORKER".to_string();
        }
        if unavailable {
            decision.syntax_allow = false;
            decision.executed_sql.clear();
            decision.code = "CONTAINMENT_UNAVAILABLE".to_string();
            decision.containment = "unavailable".to_string();
            decision.reaped = false;
            return decision;
        }
        let proven = exited && proof_ok && decision.workdir_removed && budget_ok;
        if proven {
            decision.containment = "proven".to_string();
            decision.reaped = true;
        } else {
            decision.containment = "not-proven".to_string();
            decision.reaped = false;
            if decision.syntax_allow {
                decision.syntax_allow = false;
                decision.executed_sql.clear();
                decision.code = if budget_ok { "CLEANUP_UNPROVEN" } else { "RESOURCE_BUDGET" }.to_string();
            } else if !budget_ok {
                decision.executed_sql.clear();
                decision.code = "RESOURCE_BUDGET".to_string();
            }
        }
        if !decision.syntax_allow {
            decision.executed_sql.clear();
        }
        decision
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = write_all_fd(self.ctrl_w.as_raw_fd(), b"K");
        let pid = self.child.id() as i32;
        unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = self.child.try_wait();
        let _ = self.work.remove();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadyBody {
    nonce: String,
    pid: u32,
    starttime: String,
}

fn screen_ready(body: &[u8], nonce: &str, supervisor_pid: u32) -> Result<u32, &'static str> {
    let ready: ReadyBody = serde_json::from_slice(body).map_err(|_| "INVALID_OUTPUT")?;
    if ready.nonce != nonce || ready.pid == 0 || ready.pid == supervisor_pid || ready.starttime.is_empty() {
        return Err("INVALID_OUTPUT");
    }
    match procinfo::is_descendant(ready.pid, supervisor_pid) {
        Some(true) => {}
        _ => return Err("INVALID_OUTPUT"),
    }
    match procinfo::proc_starttime(ready.pid) {
        Some(observed) if observed == ready.starttime => Ok(ready.pid),
        _ => Err("INVALID_OUTPUT"),
    }
}

fn proof_holds(text: &str, snapshot: &[Desc]) -> bool {
    if text.contains("SCAN_ERROR") || text.contains("UNAVAILABLE") || text.contains("SPAWN_ERROR") {
        return false;
    }
    let mut final_n = None;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("FINAL ") {
            if final_n.is_some() {
                return false;
            }
            final_n = rest.trim().parse::<usize>().ok();
            if final_n.is_none() {
                return false;
            }
        } else if let Some(rest) = line.strip_prefix("PID ") {
            let mut parts = rest.split_whitespace();
            let Some(pid) = parts.next().and_then(|item| item.parse::<u32>().ok()) else {
                return false;
            };
            let Some(start) = parts.next() else {
                return false;
            };
            if procinfo::same_start(pid, start) {
                return false;
            }
        } else {
            return false;
        }
    }
    if final_n != Some(0) {
        return false;
    }
    snapshot.iter().all(|desc| !procinfo::same_start(desc.pid, &desc.starttime))
}

fn bounded_reap(child: &mut Child, budget: Duration) -> bool {
    let start = Instant::now();
    let pid = child.id() as i32;
    wait_pidfd(pid, budget);
    if matches!(child.try_wait(), Ok(Some(_))) {
        return true;
    }
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let left = budget.saturating_sub(start.elapsed());
    if !left.is_zero() {
        wait_pidfd(pid, left);
    }
    matches!(child.try_wait(), Ok(Some(_)))
}

fn wait_pidfd(pid: i32, budget: Duration) {
    // SAFETY: pidfd_open is the Linux syscall (number 434 on this aarch64
    // libc). libc 0.2.189 has no pidfd_open wrapper. The fd is closed here.
    // A failure falls through to a single non-blocking try_wait in the caller
    // after this function; we do not spin and we do not call wait.
    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if pidfd < 0 {
        return;
    }
    let mut pfd = libc::pollfd { fd: pidfd as i32, events: libc::POLLIN, revents: 0 };
    unsafe {
        libc::poll(&mut pfd, 1, millis_i32(budget));
        libc::close(pidfd as i32);
    }
}

fn read_proof(fd: i32) -> String {
    let mut buf = Vec::new();
    let _ = read_capped(fd, &mut buf, PROOF_CAP.saturating_add(1));
    String::from_utf8_lossy(&buf).into_owned()
}

fn truncate_proof(text: &str) -> String {
    if text.len() <= PROOF_CAP {
        text.to_string()
    } else {
        let mut cut = text.chars().take(PROOF_CAP).collect::<String>();
        cut.push_str("\nTRUNCATED\n");
        cut
    }
}

fn dir_bytes(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total = total.saturating_add(dir_bytes(&entry.path())?);
        } else {
            total = total.saturating_add(meta.len());
        }
    }
    Ok(total)
}

fn pipe_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 with CLOEXEC. Both ends are owned by the returned descriptors.
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn set_nonblock(fd: i32) {
    // SAFETY: F_GETFL/F_SETFL on a pipe this process owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}

fn pending(fd: i32) -> bool {
    let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let rc = unsafe { libc::poll(&mut pfd, 1, 0) };
    rc > 0 && pfd.revents & libc::POLLIN != 0
}

fn poll_two(a: i32, b: i32, timeout_ms: i32) -> io::Result<()> {
    let mut fds = [
        libc::pollfd { fd: a, events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: b, events: libc::POLLIN, revents: 0 },
    ];
    loop {
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };
        if rc < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io::Error::last_os_error());
        }
        return Ok(());
    }
}

/// Read until EAGAIN, EOF, or `cap`. Returns whether EOF was seen.
fn read_capped(fd: i32, dst: &mut Vec<u8>, cap: usize) -> io::Result<bool> {
    let mut tmp = [0u8; 8192];
    let mut eof = false;
    loop {
        if dst.len() >= cap {
            break;
        }
        let space = (cap - dst.len()).min(tmp.len());
        let n = unsafe { libc::read(fd, tmp.as_mut_ptr().cast(), space) };
        if n == 0 {
            eof = true;
            break;
        }
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if err.raw_os_error() == Some(libc::EAGAIN) || err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                break;
            }
            return Err(err);
        }
        dst.extend_from_slice(&tmp[..n as usize]);
    }
    Ok(eof)
}

fn drain_stderr(fd: i32, stored: &mut Vec<u8>) -> io::Result<bool> {
    let mut tmp = [0u8; 8192];
    let mut eof = false;
    loop {
        let n = unsafe { libc::read(fd, tmp.as_mut_ptr().cast(), tmp.len()) };
        if n == 0 {
            eof = true;
            break;
        }
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if err.raw_os_error() == Some(libc::EAGAIN) || err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                break;
            }
            return Err(err);
        }
        let n = n as usize;
        if stored.len() < STDERR_STORE {
            let take = n.min(STDERR_STORE - stored.len());
            stored.extend_from_slice(&tmp[..take]);
        }
    }
    Ok(eof)
}

fn write_all_fd(fd: i32, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        bytes = &bytes[n as usize..];
    }
    Ok(())
}

fn millis_i32(duration: Duration) -> i32 {
    duration.as_millis().min(i32::MAX as u128) as i32
}

fn as_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn ignore_sigpipe() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    });
}

struct Workdir {
    path: std::path::PathBuf,
    removed: bool,
}

impl Workdir {
    fn create(nonce: &str) -> io::Result<Self> {
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

fn fresh_nonce() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

