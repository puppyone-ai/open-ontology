//! Supervisor for one pinned source-driver child.
//!
//! The deadline starts before spawn and covers the digest checks, the request
//! write, framed Arrow IPC, the kill, the reap, and the IO-thread join.
//! [`DriverReceipt`] exists only after a verified completion frame, a clean
//! stdout EOF, exit 0, and that wait. There is no second worker and no
//! in-process driver. Kernel network control stays pending.

use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use arrow_array::{Array, Int64Array, StringArray};

use crate::protocol::{
    self, decode_one_batch, file_digest, hash_file, path_text, valid_absolute, ChildBody,
    ARTIFACT_NAME, COMPLETE_MAX_BYTES, DRIVER_MAX_BYTES, DRIVER_NAME, FRAME_OVERHEAD,
    PARENT_DEADLINE_MAX_MS, PARENT_DEADLINE_MIN_MS, TAG_COMPLETE, TAG_IPC, TAG_LINEAGE,
};

/// Kernel network control of the child.
///
/// `Pending` means this process does not install a network namespace, seccomp,
/// or an egress policy. It is not a proof that the child can reach only one
/// source, and it is not a failed mock of that proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelNetwork {
    Pending,
}

/// Public value. Success receipts repeat `kernel_network=pending`.
pub const KERNEL_NETWORK: KernelNetwork = KernelNetwork::Pending;

const IO_SLICE_MS: u64 = 20;
const STDERR_MAX_BYTES: usize = 256;
const REAP_GRACE: Duration = Duration::from_millis(1000);
const GROUP_KILL_BIN: &str = "/bin/kill";

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

/// Why a supervised child did not produce [`DriverReceipt`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Spawn,
    TimedOut,
    Cancelled,
    Malformed,
    Trailer,
    Protocol,
    Ended,
    Worker,
    Io,
    Budget,
}

impl FailureKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spawn => "spawn",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::Malformed => "malformed",
            Self::Trailer => "trailer",
            Self::Protocol => "protocol",
            Self::Ended => "ended",
            Self::Worker => "worker",
            Self::Io => "io",
            Self::Budget => "budget",
        }
    }
}

impl std::fmt::Display for FailureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What `/bin/kill` did with the child's process group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSignal {
    NotNeeded,
    Sent,
    SkippedIdentity,
    Rejected,
}

/// Reap observation. Display text has no path and no credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Teardown {
    pub reaped: bool,
    pub exit_code: Option<i32>,
    pub term_signal: Option<i32>,
    pub elapsed: Duration,
    pub group_signal: GroupSignal,
    pub pid_alive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: FailureKind,
    pub teardown: Teardown,
    pub token: Option<&'static str>,
}

/// Config refusal or a supervised failure. Neither variant is a receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParentError {
    Config(&'static str),
    Failed(Failure),
}

impl std::fmt::Display for ParentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(token) => write!(formatter, "source driver parent {token}"),
            Self::Failed(failure) => {
                write!(formatter, "source driver parent {}", failure.kind.as_str())
            }
        }
    }
}

impl std::error::Error for ParentError {}

/// Absolute compiled child. The request cannot select a different executable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentConfig {
    worker: PathBuf,
    deadline: Duration,
}

impl ParentConfig {
    /// `deadline_ms` is `1..=120000`. The path is not canonicalized and is not
    /// passed to a shell. A symlink is refused.
    pub fn try_new(worker: &Path, deadline_ms: u64) -> Result<Self, ParentError> {
        if !(PARENT_DEADLINE_MIN_MS..=PARENT_DEADLINE_MAX_MS).contains(&deadline_ms) {
            return Err(ParentError::Config("deadline"));
        }
        if !approved_worker(worker) {
            return Err(ParentError::Config("worker_path"));
        }
        Ok(Self {
            worker: worker.to_path_buf(),
            deadline: Duration::from_millis(deadline_ms),
        })
    }

    /// Checks the pins, then supervises one child. A missing driver does not spawn.
    pub fn start(&self, request: &CallerRequest) -> Result<ParentPull, ParentError> {
        start_parent(self, &request.body)
    }
}

/// Caller envelope. The target credential ref is held and is not forwarded.
pub struct CallerRequest {
    body: ChildBody,
}

impl std::fmt::Debug for CallerRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CallerRequest { redacted }")
    }
}

impl CallerRequest {
    /// Parse a caller envelope. `driver_volume` must be the volume the JSON names.
    pub fn try_from_slice(bytes: &[u8], driver_volume: &Path) -> Result<Self, ParentError> {
        Ok(Self {
            body: protocol::parse_caller(bytes, driver_volume)?,
        })
    }

    /// Bytes that will be written to the child. No target credential ref.
    pub fn forwarded_json(&self) -> &[u8] {
        &self.body.forwarded
    }
}

/// One verified Arrow batch. The caller must ask for the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedBatch {
    pub batch_index: u64,
    pub rows: Vec<VerifiedRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRow {
    pub k: i64,
    pub v: Option<String>,
}

/// Receipt after a clean child exit. Import, publish, and fallback stay false.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverReceipt {
    rows: u64,
    batches: u64,
    schema_fingerprint: String,
    arrow_ipc: bool,
}

impl DriverReceipt {
    pub fn rows(&self) -> u64 {
        self.rows
    }
    pub fn batches(&self) -> u64 {
        self.batches
    }
    pub fn schema_fingerprint(&self) -> &str {
        &self.schema_fingerprint
    }
    pub fn arrow_ipc(&self) -> bool {
        self.arrow_ipc
    }
    pub fn admits_import(&self) -> bool {
        false
    }
    pub fn published(&self) -> bool {
        false
    }
    pub fn pg_run_complete(&self) -> bool {
        false
    }
    pub fn fallback(&self) -> bool {
        false
    }
    pub fn vendor_library_loaded(&self) -> bool {
        false
    }
    pub fn kernel_network(&self) -> KernelNetwork {
        KERNEL_NETWORK
    }
}

enum Phase {
    Live,
    Ready(Box<ReadyProof>),
    Finished,
}

struct ReadyProof {
    rows: u64,
    batches: u64,
    schema_fingerprint: String,
    arrow_ipc: bool,
    stderr: Vec<u8>,
    stderr_overflow: bool,
}

enum Cmd {
    Next,
}

enum IoBody {
    Wrote,
    Batch(Box<VerifiedBatch>),
    Ready(Box<ReadyProof>),
    Failed(FailureKind),
}

struct IoMsg {
    body: IoBody,
    stderr: Vec<u8>,
    stderr_overflow: bool,
}

struct PipeEnds {
    parent_write: std::os::unix::net::UnixStream,
    parent_read: std::os::unix::net::UnixStream,
    parent_err: std::os::unix::net::UnixStream,
}

struct VerifyCtx {
    body: ChildBody,
    fingerprint: String,
    frame_cap: usize,
}

pub struct ParentPull {
    stop: Arc<AtomicBool>,
    commands: SyncSender<Cmd>,
    events: Receiver<IoMsg>,
    child: Option<Child>,
    io: Option<JoinHandle<()>>,
    deadline: Instant,
    started: Instant,
    pid: u32,
    ticks: Option<u64>,
    cwd: Option<PathBuf>,
    phase: Phase,
    exit_status: Option<ExitStatus>,
    group_signal: GroupSignal,
    settled: bool,
}

impl std::fmt::Debug for ParentPull {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ParentPull { redacted }")
    }
}

impl ParentPull {
    /// Next verified batch. `Ok(None)` is the verified end, not the receipt.
    pub fn next_batch(&mut self) -> Result<Option<VerifiedBatch>, ParentError> {
        if !matches!(self.phase, Phase::Live) {
            return Err(self.fail(FailureKind::Protocol, None));
        }
        if Instant::now() >= self.deadline {
            return Err(self.fail(FailureKind::TimedOut, None));
        }
        if self.commands.try_send(Cmd::Next).is_err() {
            return Err(self.fail(FailureKind::Io, None));
        }
        loop {
            if Instant::now() >= self.deadline {
                return Err(self.fail(FailureKind::TimedOut, None));
            }
            let wait = slice_budget(self.deadline).unwrap_or(Duration::from_millis(1));
            match self.events.recv_timeout(wait) {
                Ok(msg) => return self.take_msg(msg),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    let kind = if Instant::now() >= self.deadline {
                        FailureKind::TimedOut
                    } else {
                        FailureKind::Io
                    };
                    return Err(self.fail(kind, None));
                }
            }
        }
    }

    /// Receipt only after `Ok(None)`, exit 0, and `wait`.
    pub fn finish(mut self) -> Result<DriverReceipt, ParentError> {
        let ready = match std::mem::replace(&mut self.phase, Phase::Finished) {
            Phase::Ready(proof) => *proof,
            Phase::Live | Phase::Finished => {
                return Err(self.fail(FailureKind::Protocol, None));
            }
        };
        if ready.stderr_overflow || !ready.stderr.is_empty() {
            let token = stderr_token(&ready.stderr);
            return Err(self.fail(kind_for_token(token).unwrap_or(FailureKind::Worker), token));
        }
        let teardown = self.shutdown(false);
        if !teardown.reaped || teardown.exit_code != Some(0) || teardown.term_signal.is_some() {
            return Err(ParentError::Failed(Failure {
                kind: FailureKind::Worker,
                teardown,
                token: None,
            }));
        }
        Ok(DriverReceipt {
            rows: ready.rows,
            batches: ready.batches,
            schema_fingerprint: ready.schema_fingerprint,
            arrow_ipc: ready.arrow_ipc,
        })
    }

    /// Kill the child group and reap. This is not a receipt.
    pub fn cancel(&mut self) -> ParentError {
        self.fail(FailureKind::Cancelled, None)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Private working directory while the child is alive.
    pub fn work_dir(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    fn take_msg(&mut self, msg: IoMsg) -> Result<Option<VerifiedBatch>, ParentError> {
        let token = stderr_token(&msg.stderr);
        match msg.body {
            IoBody::Wrote => Err(self.fail(FailureKind::Io, token)),
            IoBody::Batch(batch) => Ok(Some(*batch)),
            IoBody::Ready(proof) => {
                let mut proof = *proof;
                proof.stderr = msg.stderr;
                proof.stderr_overflow = msg.stderr_overflow;
                if proof.stderr_overflow || !proof.stderr.is_empty() {
                    return Err(self.fail(
                        kind_for_token(token).unwrap_or(FailureKind::Worker),
                        token,
                    ));
                }
                self.phase = Phase::Ready(Box::new(proof));
                Ok(None)
            }
            IoBody::Failed(kind) => {
                let mapped = kind_for_token(token).unwrap_or(kind);
                let mapped = if mapped == FailureKind::Ended {
                    let teardown_hint = self.exit_status.and_then(|status| status.code());
                    if teardown_hint.is_some_and(|code| code != 0) {
                        FailureKind::Worker
                    } else {
                        mapped
                    }
                } else {
                    mapped
                };
                Err(self.fail(mapped, token))
            }
        }
    }

    fn fail(&mut self, kind: FailureKind, token: Option<&'static str>) -> ParentError {
        let teardown = self.shutdown(true);
        let kind = if kind == FailureKind::Ended && teardown.exit_code.is_some_and(|code| code != 0) {
            FailureKind::Worker
        } else {
            kind
        };
        ParentError::Failed(Failure {
            kind,
            teardown,
            token,
        })
    }

    fn shutdown(&mut self, kill: bool) -> Teardown {
        if self.settled {
            return self.observe();
        }
        self.stop.store(true, Ordering::Release);
        let until = if kill { Instant::now() } else { self.deadline };
        self.poll_exit(until);
        self.group_signal = if self.exit_status.is_some() {
            GroupSignal::NotNeeded
        } else {
            self.signal_child()
        };
        if let Some(handle) = self.io.take() {
            let _ = handle.join();
        }
        self.reap_bounded();
        self.settled = true;
        if let Some(cwd) = self.cwd.take() {
            let _ = fs::remove_dir_all(cwd);
        }
        self.observe()
    }

    fn poll_exit(&mut self, until: Instant) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.exit_status = Some(status);
                    return;
                }
                Ok(None) if Instant::now() >= until => return,
                Ok(None) => thread::sleep(Duration::from_millis(IO_SLICE_MS)),
                Err(_) => return,
            }
        }
    }

    fn signal_child(&mut self) -> GroupSignal {
        let Some(child) = self.child.as_mut() else {
            return GroupSignal::NotNeeded;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.exit_status = Some(status);
                return GroupSignal::NotNeeded;
            }
            Ok(None) => {}
            Err(_) => return GroupSignal::Rejected,
        }
        let group = if group_is_owned(self.pid, self.ticks) {
            let leader = self.pid;
            match Command::new(GROUP_KILL_BIN)
                .arg("-KILL")
                .arg("--")
                .arg(format!("-{leader}"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
            {
                Ok(status) if status.success() => GroupSignal::Sent,
                _ => GroupSignal::Rejected,
            }
        } else {
            GroupSignal::SkippedIdentity
        };
        let _ = child.kill();
        group
    }

    fn reap_bounded(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if self.exit_status.is_some() {
            return;
        }
        let grace = Instant::now() + REAP_GRACE;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.exit_status = Some(status);
                    return;
                }
                Ok(None) if Instant::now() >= grace => return,
                Ok(None) => thread::sleep(Duration::from_millis(IO_SLICE_MS)),
                Err(_) => return,
            }
        }
    }

    fn observe(&self) -> Teardown {
        let status = self.exit_status;
        Teardown {
            reaped: status.is_some(),
            exit_code: status.and_then(|item| item.code()),
            term_signal: status.and_then(term_signal),
            elapsed: self.started.elapsed(),
            group_signal: self.group_signal,
            pid_alive: pid_same(self.pid, self.ticks),
        }
    }
}

impl Drop for ParentPull {
    fn drop(&mut self) {
        let _ = self.shutdown(true);
    }
}

fn start_parent(config: &ParentConfig, request: &ChildBody) -> Result<ParentPull, ParentError> {
    let started = Instant::now();
    let deadline = started + config.deadline;
    if Instant::now() >= deadline {
        return Err(ParentError::Config("deadline"));
    }
    let forwarded = request.forwarded.clone();
    if forwarded.len() > protocol::REQUEST_MAX_BYTES {
        return Err(ParentError::Config("request_too_large"));
    }
    match_digest(&config.worker, &request.image_digest, "image_digest", "image_too_large")?;
    let artifact = PathBuf::from(&request.volume).join(ARTIFACT_NAME);
    if !artifact_present(&artifact) {
        return Err(ParentError::Config("DRIVER_MISSING"));
    }
    match_digest(&artifact, &request.driver_digest, "driver_digest", "driver_too_large")?;
    let fingerprint = protocol::fingerprint_hex()?;
    let frame_cap = usize::try_from(request.batch_bytes.saturating_add(FRAME_OVERHEAD))
        .map_err(|_| ParentError::Config("budget"))?;
    let cwd = private_cwd().map_err(|_| ParentError::Config("spawn"))?;
    let spawned = match spawn_child(&config.worker, &cwd) {
        Ok(spawned) => spawned,
        Err(err) => {
            let _ = fs::remove_dir_all(&cwd);
            return Err(err);
        }
    };
    let ctx = VerifyCtx {
        body: request.clone(),
        fingerprint,
        frame_cap,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (cmd_tx, cmd_rx) = mpsc::sync_channel(1);
    let (ev_tx, ev_rx) = mpsc::sync_channel(1);
    let stop_io = Arc::clone(&stop);
    let io = thread::spawn(move || {
        io_main(spawned.pipes, forwarded, ctx, deadline, stop_io, cmd_rx, ev_tx);
    });
    let mut pull = ParentPull {
        stop,
        commands: cmd_tx,
        events: ev_rx,
        child: Some(spawned.child),
        io: Some(io),
        deadline,
        started,
        pid: spawned.pid,
        ticks: spawned.ticks,
        cwd: Some(cwd),
        phase: Phase::Live,
        exit_status: None,
        group_signal: GroupSignal::NotNeeded,
        settled: false,
    };
    wait_wrote(&mut pull)?;
    Ok(pull)
}

struct Spawned {
    child: Child,
    pid: u32,
    ticks: Option<u64>,
    pipes: PipeEnds,
}

fn wait_wrote(pull: &mut ParentPull) -> Result<(), ParentError> {
    loop {
        if Instant::now() >= pull.deadline {
            return Err(pull.fail(FailureKind::TimedOut, None));
        }
        let wait = slice_budget(pull.deadline).unwrap_or(Duration::from_millis(1));
        match pull.events.recv_timeout(wait) {
            Ok(msg) => match msg.body {
                IoBody::Wrote => return Ok(()),
                IoBody::Failed(kind) => {
                    let token = stderr_token(&msg.stderr);
                    return Err(pull.fail(kind_for_token(token).unwrap_or(kind), token));
                }
                IoBody::Batch(_) | IoBody::Ready(_) => {
                    return Err(pull.fail(FailureKind::Protocol, None));
                }
            },
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                let kind = if Instant::now() >= pull.deadline {
                    FailureKind::TimedOut
                } else {
                    FailureKind::Io
                };
                return Err(pull.fail(kind, None));
            }
        }
    }
}

fn spawn_child(worker: &Path, cwd: &Path) -> Result<Spawned, ParentError> {
    let (parent_write, child_read) =
        std::os::unix::net::UnixStream::pair().map_err(|_| ParentError::Config("spawn"))?;
    let (child_write, parent_read) =
        std::os::unix::net::UnixStream::pair().map_err(|_| ParentError::Config("spawn"))?;
    let (child_err, parent_err) =
        std::os::unix::net::UnixStream::pair().map_err(|_| ParentError::Config("spawn"))?;
    let mut command = Command::new(worker);
    command
        .env_clear()
        .current_dir(cwd)
        .stdin(Stdio::from(OwnedFd::from(child_read)))
        .stdout(Stdio::from(OwnedFd::from(child_write)))
        .stderr(Stdio::from(OwnedFd::from(child_err)))
        .process_group(0);
    let child = source_driver_isolate::spawn_isolated(&mut command).map_err(|err| {
        if source_driver_isolate::is_missing_isolate(err.raw_os_error()) {
            ParentError::Config("isolate")
        } else {
            ParentError::Config("spawn")
        }
    })?;
    let pid = child.id();
    let ticks = read_start_ticks(pid);
    Ok(Spawned {
        child,
        pid,
        ticks,
        pipes: PipeEnds {
            parent_write,
            parent_read,
            parent_err,
        },
    })
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn io_main(
    mut pipes: PipeEnds,
    request: Vec<u8>,
    ctx: VerifyCtx,
    deadline: Instant,
    stop: Arc<AtomicBool>,
    commands: Receiver<Cmd>,
    events: SyncSender<IoMsg>,
) {
    let mut stderr = Vec::new();
    let mut stderr_overflow = false;
    if let Err(kind) = write_all_within(&mut pipes.parent_write, &request, deadline, &stop) {
        let _ = send_fail(&events, kind, &mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
        return;
    }
    let _ = pipes.parent_write.shutdown(std::net::Shutdown::Write);
    if !send_msg(
        &events,
        IoMsg {
            body: IoBody::Wrote,
            stderr: Vec::new(),
            stderr_overflow: false,
        },
        deadline,
        &stop,
    ) {
        return;
    }
    let mut seen_batches = 0u64;
    let mut seen_rows = 0u64;
    let mut seen_bytes = 0u64;
    loop {
        if !recv_next(&commands, deadline, &stop) {
            return;
        }
        drain_stderr(&mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
        let frame = match read_frame(&mut pipes.parent_read, ctx.frame_cap, deadline, &stop) {
            Ok(frame) => frame,
            Err(kind) => {
                let _ = send_fail(&events, kind, &mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                return;
            }
        };
        let Some(frame) = frame else {
            let _ = send_fail(&events, FailureKind::Ended, &mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
            return;
        };
        match frame.tag {
            TAG_IPC => {
                let batch = match read_pair(&mut pipes.parent_read, frame.payload, &ctx, seen_batches, deadline, &stop) {
                    Ok(batch) => batch,
                    Err(kind) => {
                        let _ = send_fail(&events, kind, &mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                        return;
                    }
                };
                let rows = batch.rows.len() as u64;
                let next_rows = seen_rows.saturating_add(rows);
                let next_bytes = seen_bytes.saturating_add(batch.ipc_bytes);
                if next_rows > ctx.body.max_rows || next_bytes > ctx.body.max_bytes {
                    let _ = send_fail(&events, FailureKind::Budget, &mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                    return;
                }
                seen_rows = next_rows;
                seen_bytes = next_bytes;
                seen_batches = seen_batches.saturating_add(1);
                let verified = VerifiedBatch {
                    batch_index: batch.batch_index,
                    rows: batch.rows,
                };
                if !send_msg(
                    &events,
                    IoMsg {
                        body: IoBody::Batch(Box::new(verified)),
                        stderr: Vec::new(),
                        stderr_overflow: false,
                    },
                    deadline,
                    &stop,
                ) {
                    return;
                }
            }
            TAG_COMPLETE => {
                if let Err(kind) = read_clean_eof(&mut pipes.parent_read, deadline, &stop) {
                    let _ = send_fail(&events, kind, &mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                    return;
                }
                drain_stderr(&mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                match bind_complete(&frame.payload, &ctx, seen_rows, seen_batches) {
                    Ok(proof) => {
                        let _ = send_msg(
                            &events,
                            IoMsg {
                                body: IoBody::Ready(Box::new(proof)),
                                stderr,
                                stderr_overflow,
                            },
                            deadline,
                            &stop,
                        );
                    }
                    Err(kind) => {
                        let _ = send_fail(&events, kind, &mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                    }
                }
                return;
            }
            _ => {
                let _ = send_fail(&events, FailureKind::Malformed, &mut pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                return;
            }
        }
    }
}

struct OwnedFrame {
    tag: u32,
    payload: Vec<u8>,
}

struct DecodedBatch {
    batch_index: u64,
    rows: Vec<VerifiedRow>,
    ipc_bytes: u64,
}

fn read_pair(
    stdout: &mut std::os::unix::net::UnixStream,
    ipc: Vec<u8>,
    ctx: &VerifyCtx,
    index: u64,
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<DecodedBatch, FailureKind> {
    if ipc.len() > ctx.frame_cap {
        return Err(FailureKind::Malformed);
    }
    let record = decode_one_batch(&ipc).map_err(|_| FailureKind::Malformed)?;
    let row_count = record.num_rows();
    if row_count == 0 || row_count as u64 > ctx.body.batch_rows {
        return Err(FailureKind::Budget);
    }
    let lineage = match read_frame(stdout, ctx.frame_cap, deadline, stop)? {
        Some(frame) if frame.tag == TAG_LINEAGE => frame,
        Some(_) => return Err(FailureKind::Malformed),
        None => return Err(FailureKind::Ended),
    };
    let text = std::str::from_utf8(&lineage.payload).map_err(|_| FailureKind::Malformed)?;
    parse_lineage(text, index, row_count as u64, ctx.frame_cap, &ctx.fingerprint)?;
    let rows = project_rows(&record).map_err(|_| FailureKind::Malformed)?;
    Ok(DecodedBatch {
        batch_index: index,
        rows,
        ipc_bytes: ipc.len() as u64,
    })
}

fn project_rows(batch: &arrow_array::RecordBatch) -> Result<Vec<VerifiedRow>, ()> {
    let keys = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or(())?;
    let values = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or(())?;
    let mut rows = Vec::with_capacity(batch.num_rows());
    for index in 0..batch.num_rows() {
        if keys.is_null(index) {
            return Err(());
        }
        let value = if values.is_null(index) {
            None
        } else {
            Some(values.value(index).to_owned())
        };
        rows.push(VerifiedRow {
            k: keys.value(index),
            v: value,
        });
    }
    Ok(rows)
}

fn parse_lineage(
    text: &str,
    index: u64,
    rows: u64,
    cap: usize,
    fingerprint: &str,
) -> Result<(), FailureKind> {
    if !text.ends_with('\n') {
        return Err(FailureKind::Protocol);
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 4 {
        return Err(FailureKind::Protocol);
    }
    let batch_index = lines[0]
        .strip_prefix("batch_index=")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(FailureKind::Protocol)?;
    let got_rows = lines[1]
        .strip_prefix("rows=")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(FailureKind::Protocol)?;
    let arrow_bytes = lines[2]
        .strip_prefix("arrow_bytes=")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(FailureKind::Protocol)?;
    let got_fp = lines[3]
        .strip_prefix("schema_fingerprint=")
        .ok_or(FailureKind::Protocol)?;
    if batch_index != index || got_rows != rows || got_fp != fingerprint {
        return Err(FailureKind::Protocol);
    }
    if arrow_bytes == 0 || arrow_bytes > cap {
        return Err(FailureKind::Protocol);
    }
    Ok(())
}

fn bind_complete(
    payload: &[u8],
    ctx: &VerifyCtx,
    rows: u64,
    batches: u64,
) -> Result<ReadyProof, FailureKind> {
    let text = std::str::from_utf8(payload).map_err(|_| FailureKind::Protocol)?;
    let expected = protocol::completion_text(
        &ctx.body.statement_ref,
        rows,
        batches,
        &ctx.fingerprint,
        &ctx.body.driver_digest,
        &ctx.body.image_digest,
    );
    if text != expected {
        return Err(FailureKind::Protocol);
    }
    let arrow_ipc = batches > 0;
    Ok(ReadyProof {
        rows,
        batches,
        schema_fingerprint: ctx.fingerprint.clone(),
        arrow_ipc,
        stderr: Vec::new(),
        stderr_overflow: false,
    })
}

fn read_frame(
    stdout: &mut std::os::unix::net::UnixStream,
    cap: usize,
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<Option<OwnedFrame>, FailureKind> {
    let mut head = [0u8; 8];
    if !read_exact_within(stdout, &mut head, deadline, stop)? {
        return Ok(None);
    }
    let tag = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if len == 0 {
        return Err(FailureKind::Malformed);
    }
    let limit = match tag {
        TAG_IPC | TAG_LINEAGE => cap,
        TAG_COMPLETE => COMPLETE_MAX_BYTES,
        _ => return Err(FailureKind::Malformed),
    };
    if len > limit {
        return Err(FailureKind::Malformed);
    }
    let mut payload = vec![0u8; len];
    if !read_exact_within(stdout, &mut payload, deadline, stop)? {
        return Err(FailureKind::Ended);
    }
    Ok(Some(OwnedFrame { tag, payload }))
}

fn read_clean_eof(
    stdout: &mut std::os::unix::net::UnixStream,
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<(), FailureKind> {
    let mut byte = [0u8; 1];
    loop {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(FailureKind::TimedOut);
        }
        arm_read(stdout, deadline)?;
        match stdout.read(&mut byte) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err(FailureKind::Trailer),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) if is_wait(err.kind()) => continue,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(_) => return Err(FailureKind::Io),
        }
    }
}

fn read_exact_within(
    stream: &mut std::os::unix::net::UnixStream,
    buf: &mut [u8],
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<bool, FailureKind> {
    let mut filled = 0;
    while filled < buf.len() {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(FailureKind::TimedOut);
        }
        arm_read(stream, deadline)?;
        match stream.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(FailureKind::Ended),
            Ok(count) => filled += count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) if is_wait(err.kind()) => continue,
            Err(_) => return Err(FailureKind::Io),
        }
    }
    Ok(true)
}

fn write_all_within(
    stream: &mut std::os::unix::net::UnixStream,
    bytes: &[u8],
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<(), FailureKind> {
    let mut offset = 0;
    while offset < bytes.len() {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(FailureKind::TimedOut);
        }
        arm_write(stream, deadline)?;
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err(FailureKind::Io),
            Ok(count) => offset += count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) if is_wait(err.kind()) => continue,
            Err(_) => return Err(FailureKind::Io),
        }
    }
    Ok(())
}

fn recv_next(commands: &Receiver<Cmd>, deadline: Instant, stop: &AtomicBool) -> bool {
    loop {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return false;
        }
        let wait = slice_budget(deadline)
            .unwrap_or(Duration::from_millis(1))
            .min(Duration::from_millis(IO_SLICE_MS));
        match commands.recv_timeout(wait) {
            Ok(Cmd::Next) => return true,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn send_fail(
    events: &SyncSender<IoMsg>,
    kind: FailureKind,
    stderr_pipe: &mut std::os::unix::net::UnixStream,
    stderr: &mut Vec<u8>,
    stderr_overflow: &mut bool,
    deadline: Instant,
    stop: &AtomicBool,
) -> bool {
    drain_stderr(stderr_pipe, stderr, stderr_overflow, deadline, stop);
    send_msg(
        events,
        IoMsg {
            body: IoBody::Failed(kind),
            stderr: stderr.clone(),
            stderr_overflow: *stderr_overflow,
        },
        deadline,
        stop,
    )
}

fn send_msg(events: &SyncSender<IoMsg>, msg: IoMsg, deadline: Instant, stop: &AtomicBool) -> bool {
    let mut pending = Some(msg);
    loop {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return false;
        }
        let Some(item) = pending.take() else {
            return false;
        };
        match events.try_send(item) {
            Ok(()) => return true,
            Err(TrySendError::Full(item)) => {
                pending = Some(item);
                thread::sleep(Duration::from_millis(1));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

fn drain_stderr(
    stderr: &mut std::os::unix::net::UnixStream,
    buf: &mut Vec<u8>,
    overflow: &mut bool,
    _deadline: Instant,
    _stop: &AtomicBool,
) {
    let _ = stderr.set_nonblocking(true);
    let mut scratch = [0u8; 256];
    for _ in 0..8 {
        match stderr.read(&mut scratch) {
            Ok(0) => break,
            Ok(count) => {
                if buf.len().saturating_add(count) > STDERR_MAX_BYTES {
                    *overflow = true;
                    let room = STDERR_MAX_BYTES.saturating_sub(buf.len());
                    buf.extend_from_slice(&scratch[..room]);
                } else {
                    buf.extend_from_slice(&scratch[..count]);
                }
            }
            Err(err) if is_wait(err.kind()) || err.kind() == io::ErrorKind::Interrupted => break,
            Err(_) => break,
        }
    }
    let _ = stderr.set_nonblocking(false);
}

fn arm_read(stream: &std::os::unix::net::UnixStream, deadline: Instant) -> Result<(), FailureKind> {
    let wait = slice_budget(deadline)
        .unwrap_or(Duration::from_millis(1))
        .min(Duration::from_millis(IO_SLICE_MS));
    stream.set_read_timeout(Some(wait)).map_err(|_| FailureKind::Io)
}

fn arm_write(stream: &std::os::unix::net::UnixStream, deadline: Instant) -> Result<(), FailureKind> {
    let wait = slice_budget(deadline)
        .unwrap_or(Duration::from_millis(1))
        .min(Duration::from_millis(IO_SLICE_MS));
    stream.set_write_timeout(Some(wait)).map_err(|_| FailureKind::Io)
}

fn slice_budget(deadline: Instant) -> Option<Duration> {
    deadline.checked_duration_since(Instant::now())
}

fn is_wait(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

fn term_signal(status: ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

fn stderr_token(bytes: &[u8]) -> Option<&'static str> {
    let text = std::str::from_utf8(bytes).ok()?;
    let rest = text.strip_prefix("oo-source-driver-child: ")?;
    let token = rest.strip_suffix('\n')?;
    if rest.matches('\n').count() != 1 {
        return None;
    }
    Some(match token {
        "budget" => "budget",
        "crash" => "crash",
        "credential_ref" => "credential_ref",
        "deadline" => "deadline",
        "DRIVER_MISSING" => "DRIVER_MISSING",
        "driver_digest" => "driver_digest",
        "duplicate_key" => "duplicate_key",
        "forbidden_credential" => "forbidden_credential",
        "image_digest" => "image_digest",
        "io" => "io",
        "protocol" => "protocol",
        "read_unsupported" => "read_unsupported",
        "request_syntax" => "request_syntax",
        "request_too_large" => "request_too_large",
        "schema" => "schema",
        "statement_ref" => "statement_ref",
        "unknown_key" => "unknown_key",
        _ => return None,
    })
}

fn kind_for_token(token: Option<&'static str>) -> Option<FailureKind> {
    match token? {
        "deadline" => Some(FailureKind::TimedOut),
        "budget" => Some(FailureKind::Budget),
        "protocol" | "schema" | "statement_ref" | "duplicate_key" | "unknown_key"
        | "request_syntax" => Some(FailureKind::Protocol),
        "DRIVER_MISSING" | "driver_digest" | "image_digest" | "read_unsupported" | "crash"
        | "forbidden_credential" | "credential_ref" | "io" | "request_too_large" => {
            Some(FailureKind::Worker)
        }
        _ => None,
    }
}

fn match_digest(
    path: &Path,
    expected: &str,
    mismatch: &'static str,
    too_large: &'static str,
) -> Result<(), ParentError> {
    match hash_file(path, DRIVER_MAX_BYTES) {
        Ok(hash) => {
            let got = format!("sha256:{}", protocol::hex(&hash));
            if got == expected {
                Ok(())
            } else {
                Err(ParentError::Config(mismatch))
            }
        }
        Err(ParentError::Config("driver_too_large")) => Err(ParentError::Config(too_large)),
        Err(ParentError::Config("io")) => Err(ParentError::Config(mismatch)),
        Err(err) => Err(err),
    }
}

fn artifact_present(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    meta.file_type().is_file()
}

fn approved_worker(path: &Path) -> bool {
    let Some(text) = path_text(path) else {
        return false;
    };
    if !valid_absolute(&text) {
        return false;
    }
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    meta.file_type().is_file() && meta.permissions().mode() & 0o111 != 0
}

fn private_cwd() -> io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "oo-dl01w1-04-cwd-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path)?;
    let mut perms = fs::metadata(&path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&path, perms)?;
    Ok(path)
}

#[cfg(target_os = "linux")]
fn read_start_ticks(pid: u32) -> Option<u64> {
    proc_field(pid, 19)
}

#[cfg(not(target_os = "linux"))]
fn read_start_ticks(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn proc_field(pid: u32, index: usize) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = text.rfind(')')?;
    let from = end.checked_add(2)?;
    let rest = text.get(from..)?;
    rest.split_whitespace().nth(index)?.parse().ok()
}

#[cfg(target_os = "linux")]
fn group_is_owned(leader: u32, recorded: Option<u64>) -> bool {
    if leader <= 1 || !pid_same(leader, recorded) {
        return false;
    }
    let own = proc_field(std::process::id(), 2);
    if own == Some(u64::from(leader)) {
        return false;
    }
    matches!(
        proc_field(leader, 2),
        Some(pgrp) if pgrp == u64::from(leader) && pgrp > 1
    )
}

#[cfg(not(target_os = "linux"))]
fn group_is_owned(leader: u32, _recorded: Option<u64>) -> bool {
    leader > 1
}

#[cfg(target_os = "linux")]
fn pid_same(pid: u32, recorded: Option<u64>) -> bool {
    match (recorded, read_start_ticks(pid)) {
        (Some(old), Some(now)) => old == now,
        _ => false,
    }
}

#[cfg(not(target_os = "linux"))]
fn pid_same(_pid: u32, _recorded: Option<u64>) -> bool {
    false
}

#[allow(dead_code)]
fn _pins() {
    let _ = (DRIVER_NAME, file_digest as fn(&Path, u64) -> Result<String, ParentError>);
}
