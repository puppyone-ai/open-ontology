//! Parent of one trusted `oo-file-parser-worker` process.
//!
//! The deadline starts before `spawn` and covers the request write, the framed
//! Arrow IPC read, the kill, the reap, and the IO-thread join. A
//! [`ParentParseComplete`] exists only after a verified completion frame, a
//! clean stdout EOF, exit 0, and that wait. Kernel network, filesystem, and
//! memory restriction stay [`KernelRestriction::Unsupported`]. A descendant
//! that has left the child's process group is not killed. This module does
//! not mint a public reader token and it does not build a PostgreSQL
//! `RunComplete`.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use arrow_array::{Array, RecordBatch};
use arrow_ipc::reader::StreamReader;
use arrow_schema::{DataType, Field, Schema};
use sha2::{Digest, Sha256};

use crate::{
    DelimitedBudget, DelimitedProfile, DelimitedType, JsonlBudget, JsonlProfile, JsonlType,
    NullPolicy, DELIMITED_LINEAGE_VERSION, DELIMITED_PROFILE_VERSION, DELIMITED_PROTOCOL,
    DELIMITED_RECEIPT_VERSION, JSONL_LINEAGE_VERSION, JSONL_PROFILE_VERSION, JSONL_PROTOCOL,
    JSONL_RECEIPT_VERSION,
};

/// Parent protocol. This is not `oo.source.pgsql/v1` and it is not the child
/// worker protocol.
pub const FILE_WORKER_PARENT_PROTOCOL: &str = "oo.source.file-worker-parent/v1";

/// Inclusive lower bound of the parent deadline, in milliseconds.
pub const PARENT_DEADLINE_MIN_MS: u64 = 1;

/// Inclusive upper bound of the parent deadline, in milliseconds.
pub const PARENT_DEADLINE_MAX_MS: u64 = 120_000;

/// Longest socket wait before the IO thread checks the deadline again.
const IO_SLICE_MS: u64 = 20;

const _: () = assert!(IO_SLICE_MS <= 20);

const WORKER_PROTOCOL: &str = "oo.source.file-parser-worker/v1";
const WORKER_DEADLINE_MAX_MS: u64 = 3_600_000;
const REQUEST_MAX_BYTES: usize = 1_048_576;
const PATH_MAX_BYTES: usize = 4096;
const FRAME_OVERHEAD: u64 = 65_536;
const COMPLETE_MAX_BYTES: usize = 65_536;
const STDERR_MAX_BYTES: usize = 256;
const REAP_GRACE: Duration = Duration::from_millis(1000);
const GROUP_KILL_BIN: &str = "/bin/kill";

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

/// Kernel isolation of the worker process.
///
/// `std::process` with a cleared environment and a process group is not
/// network, filesystem, or memory isolation. No launcher, seccomp, landlock,
/// or rlimit is applied. This value is not a containment certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelRestriction {
    /// The group signal and the direct reap are implemented. The sandbox is not.
    Unsupported,
}

/// Public admission of an untrusted worker stays closed while this is `Unsupported`.
pub const KERNEL_RESTRICTION: KernelRestriction = KernelRestriction::Unsupported;

/// Why a parent call did not produce [`ParentParseComplete`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// The process could not be created.
    Spawn,
    /// The parent deadline expired.
    TimedOut,
    /// A frame, an IPC stream, or a tag was not the worker contract.
    Malformed,
    /// Bytes followed the completion frame.
    Trailer,
    /// The object hash, the request hash, and the completion hash did not agree,
    /// or the child reported `hash_mismatch`.
    HashMismatch,
    /// Role, profile, order, schema, count, lineage, or the completion binding failed.
    Protocol,
    /// Stdout ended before a verified completion frame.
    Ended,
    /// The child exited nonzero, or exit 0 arrived without a verified end.
    Worker,
    /// A pipe or a thread failed.
    Io,
}

impl FailureKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Spawn => "spawn",
            Self::TimedOut => "timed_out",
            Self::Malformed => "malformed",
            Self::Trailer => "trailer",
            Self::HashMismatch => "hash_mismatch",
            Self::Protocol => "protocol",
            Self::Ended => "ended",
            Self::Worker => "worker",
            Self::Io => "io",
        }
    }
}

impl std::fmt::Display for FailureKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What the parent did with `/bin/kill` on the child's process group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSignal {
    /// The child had already exited, or no kill was required.
    NotNeeded,
    /// `/bin/kill -KILL -- -<group>` returned success for the owned group.
    Sent,
    /// The group id failed the identity check, so the group was not signaled.
    SkippedIdentity,
    /// `/bin/kill` did not return success. The direct child is still signaled
    /// when `try_wait` says it is ours.
    Rejected,
}

/// Process observation after wait. Display text has no path and no payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Teardown {
    /// `wait` observed this child.
    pub reaped: bool,
    /// Exit status code when the child was not signaled.
    pub exit_code: Option<i32>,
    /// Termination signal when the kernel reported one.
    pub term_signal: Option<i32>,
    /// Time from the pre-spawn instant to this observation.
    pub elapsed: Duration,
    /// Group-signal outcome. `Sent` is not a sandbox certificate.
    pub group_signal: GroupSignal,
    /// The recorded pid is still the same live process.
    pub pid_alive: bool,
}

/// A parent failure. `token` is a static child stderr token when the line matches
/// the worker's closed list. The line bytes are not retained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// Classification. None of these values is a completion.
    pub kind: FailureKind,
    /// Reap observation.
    pub teardown: Teardown,
    /// Static stderr token, when the line matched the worker list.
    pub token: Option<&'static str>,
}

/// Config refusal or a supervised failure. Neither variant is a completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParentError {
    /// The call was refused before a verified receipt. The text is a static token.
    Config(&'static str),
    /// The child was supervised and did not verify.
    Failed(Failure),
}

impl std::fmt::Display for ParentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(token) => write!(formatter, "file worker parent config {token}"),
            Self::Failed(failure) => {
                write!(formatter, "file worker parent {}", failure.kind.as_str())
            }
        }
    }
}

impl std::error::Error for ParentError {}

/// Trusted absolute path of the compiled worker. The request cannot select it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentConfig {
    worker: PathBuf,
    deadline: Duration,
}

impl ParentConfig {
    /// Accept an absolute regular executable and a deadline of `1..=120000` ms.
    ///
    /// The path is not canonicalized and is not passed to a shell. A symlink is
    /// refused. A relative path is refused.
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

    /// Supervises one child. The returned pull delivers one verified batch at a time.
    pub fn start(&self, request: &ChildRequest) -> Result<ParentPull, ParentError> {
        start_parent(self, request)
    }
}

/// Worker pull caps. The worker deadline is the child's between-read check.
/// It is not the parent deadline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullBudget {
    kind: BudgetKind,
    batch_rows: u32,
    batch_bytes: u64,
    max_record_bytes: u32,
    max_field_bytes: u32,
    max_total_bytes: u64,
    max_rows: u64,
    max_columns: u32,
    max_fields: u32,
    worker_deadline_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BudgetKind {
    Jsonl,
    Delimited,
}

impl PullBudget {
    /// JSONL caps. `worker_deadline_ms` is `1..=3600000`.
    #[allow(clippy::too_many_arguments)]
    pub fn jsonl(
        batch_rows: u32,
        batch_bytes: u64,
        max_record_bytes: u32,
        max_total_bytes: u64,
        max_rows: u64,
        max_columns: u32,
        max_fields: u32,
        worker_deadline_ms: u64,
    ) -> Result<Self, ParentError> {
        check_worker_deadline(worker_deadline_ms)?;
        JsonlBudget::try_new(
            batch_rows,
            batch_bytes,
            max_record_bytes,
            max_total_bytes,
            max_rows,
            max_columns,
            max_fields,
            Instant::now(),
        )
        .map_err(|_| ParentError::Config("budget"))?;
        Ok(Self {
            kind: BudgetKind::Jsonl,
            batch_rows,
            batch_bytes,
            max_record_bytes,
            max_field_bytes: 0,
            max_total_bytes,
            max_rows,
            max_columns,
            max_fields,
            worker_deadline_ms,
        })
    }

    /// Delimited caps, including `max_field_bytes`.
    #[allow(clippy::too_many_arguments)]
    pub fn delimited(
        batch_rows: u32,
        batch_bytes: u64,
        max_record_bytes: u32,
        max_field_bytes: u32,
        max_total_bytes: u64,
        max_rows: u64,
        max_columns: u32,
        max_fields: u32,
        worker_deadline_ms: u64,
    ) -> Result<Self, ParentError> {
        check_worker_deadline(worker_deadline_ms)?;
        DelimitedBudget::try_new(
            batch_rows,
            batch_bytes,
            max_record_bytes,
            max_field_bytes,
            max_total_bytes,
            max_rows,
            max_columns,
            max_fields,
            Instant::now(),
        )
        .map_err(|_| ParentError::Config("budget"))?;
        Ok(Self {
            kind: BudgetKind::Delimited,
            batch_rows,
            batch_bytes,
            max_record_bytes,
            max_field_bytes,
            max_total_bytes,
            max_rows,
            max_columns,
            max_fields,
            worker_deadline_ms,
        })
    }
}

fn check_worker_deadline(millis: u64) -> Result<(), ParentError> {
    if millis == 0 || millis > WORKER_DEADLINE_MAX_MS {
        Err(ParentError::Config("worker_deadline"))
    } else {
        Ok(())
    }
}

/// Explicit profile. The parent does not infer a format from the path or the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceProfile {
    /// Public JSONL profile.
    Jsonl(JsonlProfile),
    /// Public CSV or TSV profile.
    Delimited(DelimitedProfile),
}

/// One child request. The input is a caller-admitted immutable regular file.
/// This type does not prove that the file stays unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildRequest {
    path: String,
    object_sha256: [u8; 32],
    profile: SourceProfile,
    budget: PullBudget,
}

impl ChildRequest {
    /// Build the worker JSON from a path, an expected object hash, and a matching budget.
    pub fn try_new(
        path: &Path,
        object_sha256: [u8; 32],
        profile: SourceProfile,
        budget: PullBudget,
    ) -> Result<Self, ParentError> {
        let text = path_text(path).ok_or(ParentError::Config("path_form"))?;
        if !valid_absolute(&text) {
            return Err(ParentError::Config("path_form"));
        }
        let meta = fs::symlink_metadata(path).map_err(|_| ParentError::Config("input_io"))?;
        if !meta.file_type().is_file() {
            return Err(ParentError::Config("not_regular"));
        }
        match (&profile, budget.kind) {
            (SourceProfile::Jsonl(_), BudgetKind::Jsonl)
            | (SourceProfile::Delimited(_), BudgetKind::Delimited) => {}
            _ => return Err(ParentError::Config("profile_budget")),
        }
        Ok(Self {
            path: text,
            object_sha256,
            profile,
            budget,
        })
    }
}

/// One decoded Arrow batch plus the lineage that bound it.
///
/// The parent retains no earlier batch. The caller owns this value.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedBatch {
    /// Zero-based index from the lineage frame.
    pub batch_index: u64,
    /// The single record batch inside the IPC frame.
    pub record: RecordBatch,
    /// Row lineage in profile order.
    pub rows: Vec<VerifiedRow>,
    /// Raw source bytes the lineage attributed to this batch.
    pub raw_bytes: u64,
    /// Arrow memory size from the lineage. IPC decode changes buffer lengths,
    /// so this is the worker's pre-encode sum, bounded by the frame cap.
    pub arrow_bytes: u64,
    /// Schema identity of this batch.
    pub schema_identity: [u8; 32],
    /// Public profile digest.
    pub profile_digest: [u8; 32],
    /// Public config identity for the expected object hash.
    pub config_identity: [u8; 32],
}

/// One source row inside a verified batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRow {
    /// Reader row index.
    pub row_index: u64,
    /// 1-based line number.
    pub line: u64,
    /// Absolute byte offset of the record.
    pub byte_offset: u64,
    /// Fields in profile order.
    pub fields: Vec<VerifiedField>,
}

/// One field inside a verified row. `presence` is `value`, `null`, `missing`, or `empty`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedField {
    /// Profile column name.
    pub name: String,
    /// 1-based line number of the field.
    pub line: u64,
    /// Absolute byte offset the lineage recorded.
    pub byte_offset: u64,
    /// Closed presence token.
    pub presence: &'static str,
}

/// Verified parse receipt. Constructed only after completion, clean EOF, exit 0,
/// and `wait`. `admits_import`, `published`, and `pg_run_complete` are false.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentParseComplete {
    rows: u64,
    batches: u64,
    columns: u32,
    object_sha256: [u8; 32],
    schema_identity: [u8; 32],
    profile_digest: [u8; 32],
    config_identity: [u8; 32],
    profile_id: String,
    reader_protocol: &'static str,
}

impl ParentParseComplete {
    /// Row count bound by the completion frame and the decoded batches.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// Batch count. This is also the IPC frame count and the lineage frame count.
    pub fn batches(&self) -> u64 {
        self.batches
    }

    /// Profile width.
    pub fn columns(&self) -> u32 {
        self.columns
    }

    /// Object SHA-256 that matched the request, the file bytes, and the completion frame.
    pub fn object_sha256(&self) -> [u8; 32] {
        self.object_sha256
    }

    /// Schema identity of the profile reconstruction.
    pub fn schema_identity(&self) -> [u8; 32] {
        self.schema_identity
    }

    /// Public profile digest.
    pub fn profile_digest(&self) -> [u8; 32] {
        self.profile_digest
    }

    /// Public config identity.
    pub fn config_identity(&self) -> [u8; 32] {
        self.config_identity
    }

    /// Profile id.
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    /// `oo.source.jsonl/v1` or `oo.source.delimited/v1`.
    pub fn reader_protocol(&self) -> &'static str {
        self.reader_protocol
    }

    /// This receipt does not admit an import.
    pub const fn admits_import(&self) -> bool {
        false
    }

    /// This receipt is not a publication.
    pub const fn published(&self) -> bool {
        false
    }

    /// This receipt is not a PostgreSQL run completion.
    pub const fn pg_run_complete(&self) -> bool {
        false
    }

    /// True only when at least one IPC frame was verified.
    pub const fn arrow_ipc(&self) -> bool {
        self.batches > 0
    }
}

/// Pull of one live child. Drop kills and reaps if [`Self::finish`] has not run.
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

enum Phase {
    Live,
    Ready(Box<ReadyProof>),
    Finished,
}

#[derive(Clone, Debug)]
struct ReadyProof {
    rows: u64,
    batches: u64,
    columns: u32,
    object_sha256: [u8; 32],
    schema_identity: [u8; 32],
    profile_digest: [u8; 32],
    config_identity: [u8; 32],
    profile_id: String,
    reader_protocol: &'static str,
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

impl ParentPull {
    /// Next verified batch. `Ok(None)` means the completion frame and a clean EOF
    /// were verified. The receipt is still withheld until [`Self::finish`].
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

    /// Wait for exit 0. Returns the receipt only when the pull is already at a
    /// verified clean EOF. An earlier call does not return a receipt.
    pub fn finish(mut self) -> Result<ParentParseComplete, ParentError> {
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
            let token = stderr_token(&ready.stderr);
            let kind = if token == Some("hash_mismatch") || teardown.exit_code == Some(6) {
                FailureKind::HashMismatch
            } else if Instant::now() >= self.deadline {
                FailureKind::TimedOut
            } else {
                FailureKind::Worker
            };
            return Err(ParentError::Failed(Failure {
                kind,
                teardown,
                token,
            }));
        }
        Ok(ParentParseComplete {
            rows: ready.rows,
            batches: ready.batches,
            columns: ready.columns,
            object_sha256: ready.object_sha256,
            schema_identity: ready.schema_identity,
            profile_digest: ready.profile_digest,
            config_identity: ready.config_identity,
            profile_id: ready.profile_id,
            reader_protocol: ready.reader_protocol,
        })
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
                Err(self.fail(mapped, token))
            }
        }
    }

    fn fail(&mut self, kind: FailureKind, token: Option<&'static str>) -> ParentError {
        let teardown = self.shutdown(true);
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
            match Command::new(GROUP_KILL_BIN)
                .arg("-KILL")
                .arg("--")
                .arg(format!("-{}", self.pid))
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

fn term_signal(status: ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

struct VerifyCtx {
    schema_identity: [u8; 32],
    profile_digest: [u8; 32],
    config_identity: [u8; 32],
    profile_id: String,
    reader_protocol: &'static str,
    profile_version: &'static str,
    lineage_version: &'static str,
    receipt_version: &'static str,
    columns: Vec<String>,
    jsonl: bool,
    object_sha256: [u8; 32],
    file_hash: [u8; 32],
    frame_cap: usize,
    column_count: u32,
}

struct PipeEnds {
    parent_write: std::os::unix::net::UnixStream,
    parent_read: std::os::unix::net::UnixStream,
    parent_err: std::os::unix::net::UnixStream,
}

fn start_parent(config: &ParentConfig, request: &ChildRequest) -> Result<ParentPull, ParentError> {
    let started = Instant::now();
    let deadline = started + config.deadline;
    let file_hash = hash_file(Path::new(&request.path), request.budget.max_total_bytes, deadline)?;
    let ctx = verify_ctx(request, file_hash)?;
    let bytes = encode_request(request)?;
    let cwd = private_cwd().map_err(|_| ParentError::Config("spawn"))?;
    let spawned = spawn_child(&config.worker, &cwd).inspect_err(|_| {
        let _ = fs::remove_dir_all(&cwd);
    })?;
    let stop = Arc::new(AtomicBool::new(false));
    let (cmd_tx, cmd_rx) = mpsc::sync_channel(1);
    let (ev_tx, ev_rx) = mpsc::sync_channel(1);
    let stop_io = Arc::clone(&stop);
    let io = thread::spawn(move || {
        io_main(
            spawned.pipes,
            bytes,
            ctx,
            deadline,
            stop_io,
            cmd_rx,
            ev_tx,
        );
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
    match wait_wrote(&mut pull) {
        Ok(()) => Ok(pull),
        Err(err) => Err(err),
    }
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
    let child = command.spawn().map_err(|_| ParentError::Config("spawn"))?;
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
        let _ = send_fail(&events, kind, &pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
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
    loop {
        if !recv_next(&commands, deadline, &stop) {
            return;
        }
        drain_stderr(&pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
        let frame = match read_frame(&mut pipes.parent_read, ctx.frame_cap, COMPLETE_MAX_BYTES, deadline, &stop)
        {
            Ok(frame) => frame,
            Err(kind) => {
                let _ = send_fail(&events, kind, &pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                return;
            }
        };
        let Some(frame) = frame else {
            let _ = send_fail(&events, FailureKind::Ended, &pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
            return;
        };
        match frame.tag {
            1 => {
                let batch = match read_pair(
                    &mut pipes.parent_read,
                    frame.payload,
                    &ctx,
                    seen_batches,
                    deadline,
                    &stop,
                ) {
                    Ok(batch) => batch,
                    Err(kind) => {
                        let _ = send_fail(&events, kind, &pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                        return;
                    }
                };
                seen_rows = seen_rows.saturating_add(batch.record.num_rows() as u64);
                seen_batches = seen_batches.saturating_add(1);
                if !send_msg(
                    &events,
                    IoMsg {
                        body: IoBody::Batch(Box::new(batch)),
                        stderr: Vec::new(),
                        stderr_overflow: false,
                    },
                    deadline,
                    &stop,
                ) {
                    return;
                }
            }
            3 => {
                if let Err(kind) = read_clean_eof(&mut pipes.parent_read, deadline, &stop) {
                    let _ = send_fail(&events, kind, &pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                    return;
                }
                drain_stderr(&pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
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
                        let _ = send_fail(&events, kind, &pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                    }
                }
                return;
            }
            _ => {
                let _ = send_fail(&events, FailureKind::Malformed, &pipes.parent_err, &mut stderr, &mut stderr_overflow, deadline, &stop);
                return;
            }
        }
    }
}

fn recv_next(commands: &Receiver<Cmd>, deadline: Instant, stop: &AtomicBool) -> bool {
    loop {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return false;
        }
        let wait = slice_budget(deadline).unwrap_or(Duration::from_millis(1));
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
    stderr_pipe: &std::os::unix::net::UnixStream,
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
                thread::sleep(slice_budget(deadline).unwrap_or(Duration::from_millis(1)));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

fn read_pair(
    stdout: &mut std::os::unix::net::UnixStream,
    ipc: Vec<u8>,
    ctx: &VerifyCtx,
    index: u64,
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<VerifiedBatch, FailureKind> {
    let record = decode_ipc(&ipc)?;
    let lineage = match read_frame(stdout, ctx.frame_cap, COMPLETE_MAX_BYTES, deadline, stop)? {
        Some(frame) if frame.tag == 2 => frame.payload,
        Some(_) => return Err(FailureKind::Malformed),
        None => return Err(FailureKind::Ended),
    };
    bind_batch(record, &lineage, ctx, index)
}

fn bind_batch(
    record: RecordBatch,
    lineage: &[u8],
    ctx: &VerifyCtx,
    index: u64,
) -> Result<VerifiedBatch, FailureKind> {
    if record.num_rows() == 0 || record.num_columns() != ctx.columns.len() {
        return Err(FailureKind::Protocol);
    }
    let actual_schema = schema_identity(record.schema().as_ref()).map_err(|_| FailureKind::Protocol)?;
    if actual_schema != ctx.schema_identity {
        return Err(FailureKind::Protocol);
    }
    let text = std::str::from_utf8(lineage).map_err(|_| FailureKind::Protocol)?;
    let parsed = parse_lineage(text, ctx).map_err(|_| FailureKind::Protocol)?;
    if parsed.batch_index != index
        || parsed.reader_protocol != ctx.reader_protocol
        || parsed.profile_digest != ctx.profile_digest
        || parsed.config_identity != ctx.config_identity
        || parsed.schema_identity != ctx.schema_identity
        || parsed.rows.len() != record.num_rows()
    {
        return Err(FailureKind::Protocol);
    }
    let memory = arrow_memory(&record);
    let cap = u64::try_from(ctx.frame_cap).map_err(|_| FailureKind::Protocol)?;
    if parsed.arrow_bytes > cap || memory > cap {
        return Err(FailureKind::Protocol);
    }
    if record.num_rows() > 0 && (parsed.raw_bytes == 0 || parsed.arrow_bytes == 0 || memory == 0) {
        return Err(FailureKind::Protocol);
    }
    Ok(VerifiedBatch {
        batch_index: parsed.batch_index,
        record,
        rows: parsed.rows,
        raw_bytes: parsed.raw_bytes,
        arrow_bytes: parsed.arrow_bytes,
        schema_identity: parsed.schema_identity,
        profile_digest: parsed.profile_digest,
        config_identity: parsed.config_identity,
    })
}

struct ParsedLineage {
    batch_index: u64,
    reader_protocol: String,
    profile_digest: [u8; 32],
    config_identity: [u8; 32],
    schema_identity: [u8; 32],
    raw_bytes: u64,
    arrow_bytes: u64,
    rows: Vec<VerifiedRow>,
}

fn parse_lineage(text: &str, ctx: &VerifyCtx) -> Result<ParsedLineage, ()> {
    if !text.ends_with('\n') || text.contains('\r') {
        return Err(());
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.first().copied() != Some("oo-file-parser-lineage/v1") || lines.len() < 8 {
        return Err(());
    }
    let batch_index = keyed_u64(lines[1], "batch_index")?;
    let reader_protocol = keyed_str(lines[2], "reader_protocol")?.to_owned();
    let profile_digest = hex_decode(keyed_str(lines[3], "profile_digest")?)?;
    let config_identity = hex_decode(keyed_str(lines[4], "config_identity")?)?;
    let schema_identity = hex_decode(keyed_str(lines[5], "schema_identity")?)?;
    let raw_bytes = keyed_u64(lines[6], "raw_bytes")?;
    let arrow_bytes = keyed_u64(lines[7], "arrow_bytes")?;
    let mut rows = Vec::new();
    let mut cursor = 8;
    while cursor < lines.len() {
        let (row_index, line, byte_offset) = row_line(lines[cursor])?;
        cursor += 1;
        if line == 0 {
            return Err(());
        }
        let mut fields = Vec::with_capacity(ctx.columns.len());
        for name in &ctx.columns {
            if cursor >= lines.len() {
                return Err(());
            }
            let field = field_line(lines[cursor], ctx.jsonl)?;
            if field.name != *name || field.line == 0 {
                return Err(());
            }
            cursor += 1;
            fields.push(field);
        }
        rows.push(VerifiedRow {
            row_index,
            line,
            byte_offset,
            fields,
        });
    }
    Ok(ParsedLineage {
        batch_index,
        reader_protocol,
        profile_digest,
        config_identity,
        schema_identity,
        raw_bytes,
        arrow_bytes,
        rows,
    })
}

fn bind_complete(
    payload: &[u8],
    ctx: &VerifyCtx,
    rows: u64,
    batches: u64,
) -> Result<ReadyProof, FailureKind> {
    let text = std::str::from_utf8(payload).map_err(|_| FailureKind::Protocol)?;
    if !text.ends_with('\n') || text.contains('\r') {
        return Err(FailureKind::Protocol);
    }
    let lines: Vec<&str> = text.lines().collect();
    let arrow_ipc = if batches == 0 { "false" } else { "true" };
    let expect = vec![
        "oo-file-parser-complete/v1".to_owned(),
        format!("protocol={WORKER_PROTOCOL}"),
        format!("reader_protocol={}", ctx.reader_protocol),
        format!("reader_profile_version={}", ctx.profile_version),
        format!("reader_lineage_version={}", ctx.lineage_version),
        format!("reader_receipt_version={}", ctx.receipt_version),
        format!("profile_id={}", ctx.profile_id),
        format!("profile_digest={}", hex_encode(&ctx.profile_digest)),
        format!("config_identity={}", hex_encode(&ctx.config_identity)),
        format!("schema_identity={}", hex_encode(&ctx.schema_identity)),
        format!("object_sha256={}", hex_encode(&ctx.object_sha256)),
        format!("rows={rows}"),
        format!("columns={}", ctx.column_count),
        format!("batches={batches}"),
        format!("ipc_frames={batches}"),
        format!("lineage_frames={batches}"),
        "admits_import=false".to_owned(),
        "published=false".to_owned(),
        format!("arrow_ipc={arrow_ipc}"),
        "executes_native=false".to_owned(),
        "kernel_network=unsupported".to_owned(),
        "kernel_filesystem=unsupported".to_owned(),
        "kernel_memory=unsupported".to_owned(),
        "parent_kill=caller".to_owned(),
        "parent_reap=caller".to_owned(),
        "launcher=not_this_process".to_owned(),
        "pg_run_complete=false".to_owned(),
        "resumable=false".to_owned(),
    ];
    let same = lines.len() == expect.len()
        && lines
            .iter()
            .zip(expect.iter())
            .all(|(got, want)| got == want);
    if !same {
        if completion_hash(&lines).is_some_and(|got| got != ctx.object_sha256 || got != ctx.file_hash)
        {
            return Err(FailureKind::HashMismatch);
        }
        return Err(FailureKind::Protocol);
    }
    if ctx.object_sha256 != ctx.file_hash {
        return Err(FailureKind::HashMismatch);
    }
    Ok(ReadyProof {
        rows,
        batches,
        columns: ctx.column_count,
        object_sha256: ctx.object_sha256,
        schema_identity: ctx.schema_identity,
        profile_digest: ctx.profile_digest,
        config_identity: ctx.config_identity,
        profile_id: ctx.profile_id.clone(),
        reader_protocol: ctx.reader_protocol,
        stderr: Vec::new(),
        stderr_overflow: false,
    })
}

fn completion_hash(lines: &[&str]) -> Option<[u8; 32]> {
    let line = lines
        .iter()
        .find(|line| line.starts_with("object_sha256="))?;
    hex_decode(keyed_str(line, "object_sha256").ok()?).ok()
}

fn decode_ipc(payload: &[u8]) -> Result<RecordBatch, FailureKind> {
    let cursor = io::Cursor::new(payload);
    let mut reader = StreamReader::try_new(cursor, None).map_err(|_| FailureKind::Malformed)?;
    let batch = match reader.next() {
        Some(Ok(batch)) => batch,
        Some(Err(_)) => return Err(FailureKind::Malformed),
        None => return Err(FailureKind::Malformed),
    };
    if reader.next().is_some() {
        return Err(FailureKind::Malformed);
    }
    Ok(batch)
}

fn arrow_memory(batch: &RecordBatch) -> u64 {
    batch
        .columns()
        .iter()
        .map(|column| column.get_array_memory_size() as u64)
        .sum()
}

struct OwnedFrame {
    tag: u32,
    payload: Vec<u8>,
}

fn read_frame(
    stdout: &mut std::os::unix::net::UnixStream,
    ipc_cap: usize,
    complete_cap: usize,
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
    let cap = match tag {
        1 | 2 => ipc_cap,
        3 => complete_cap,
        _ => return Err(FailureKind::Malformed),
    };
    if len > cap {
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
    loop {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(FailureKind::TimedOut);
        }
        arm_write(stream, deadline)?;
        match stream.flush() {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) if is_wait(err.kind()) => continue,
            Err(_) => return Err(FailureKind::Io),
        }
    }
}

fn drain_stderr(
    mut stderr: &std::os::unix::net::UnixStream,
    buf: &mut Vec<u8>,
    overflow: &mut bool,
    deadline: Instant,
    stop: &AtomicBool,
) {
    let mut scratch = [0u8; 256];
    for _ in 0..8 {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return;
        }
        if arm_read(stderr, deadline).is_err() {
            return;
        }
        match stderr.read(&mut scratch) {
            Ok(0) => return,
            Ok(count) => {
                if buf.len() >= STDERR_MAX_BYTES {
                    *overflow = true;
                } else {
                    let room = STDERR_MAX_BYTES - buf.len();
                    if count > room {
                        buf.extend_from_slice(&scratch[..room]);
                        *overflow = true;
                    } else {
                        buf.extend_from_slice(&scratch[..count]);
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) if is_wait(err.kind()) => return,
            Err(_) => return,
        }
    }
}

fn arm_read(stream: &std::os::unix::net::UnixStream, deadline: Instant) -> Result<(), FailureKind> {
    let Some(budget) = slice_budget(deadline) else {
        return Err(FailureKind::TimedOut);
    };
    stream
        .set_read_timeout(Some(budget))
        .map_err(|_| FailureKind::Io)
}

fn arm_write(stream: &std::os::unix::net::UnixStream, deadline: Instant) -> Result<(), FailureKind> {
    let Some(budget) = slice_budget(deadline) else {
        return Err(FailureKind::TimedOut);
    };
    stream
        .set_write_timeout(Some(budget))
        .map_err(|_| FailureKind::Io)
}

fn slice_budget(deadline: Instant) -> Option<Duration> {
    let now = Instant::now();
    if now >= deadline {
        return None;
    }
    Some(
        Duration::from_millis(IO_SLICE_MS)
            .min(deadline.saturating_duration_since(now))
            .max(Duration::from_millis(1)),
    )
}

fn is_wait(kind: io::ErrorKind) -> bool {
    matches!(kind, io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

fn hash_file(path: &Path, cap: u64, deadline: Instant) -> Result<[u8; 32], ParentError> {
    let meta = fs::symlink_metadata(path).map_err(|_| ParentError::Config("input_io"))?;
    if !meta.file_type().is_file() {
        return Err(ParentError::Config("not_regular"));
    }
    if meta.len() > cap {
        return Err(ParentError::Config("input_too_large"));
    }
    let mut file = File::open(path).map_err(|_| ParentError::Config("input_io"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut seen = 0u64;
    loop {
        if seen != meta.len() && Instant::now() >= deadline {
            return Err(ParentError::Config("deadline"));
        }
        let count = loop {
            match file.read(&mut buf) {
                Ok(count) => break count,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(ParentError::Config("input_io")),
            }
        };
        if count == 0 {
            break;
        }
        seen = seen.saturating_add(count as u64);
        if seen > cap {
            return Err(ParentError::Config("input_too_large"));
        }
        hasher.update(&buf[..count]);
    }
    if seen != meta.len() {
        return Err(ParentError::Config("input_changed"));
    }
    Ok(hasher.finalize().into())
}

fn verify_ctx(request: &ChildRequest, file_hash: [u8; 32]) -> Result<VerifyCtx, ParentError> {
    let frame_cap = usize::try_from(request.budget.batch_bytes.saturating_add(FRAME_OVERHEAD))
        .map_err(|_| ParentError::Config("budget"))?;
    let (schema, reader_protocol, profile_version, lineage_version, receipt_version, jsonl) =
        match &request.profile {
            SourceProfile::Jsonl(profile) => (
                jsonl_schema(profile)?,
                JSONL_PROTOCOL,
                JSONL_PROFILE_VERSION,
                JSONL_LINEAGE_VERSION,
                JSONL_RECEIPT_VERSION,
                true,
            ),
            SourceProfile::Delimited(profile) => (
                delimited_schema(profile)?,
                DELIMITED_PROTOCOL,
                DELIMITED_PROFILE_VERSION,
                DELIMITED_LINEAGE_VERSION,
                DELIMITED_RECEIPT_VERSION,
                false,
            ),
        };
    let schema_identity = schema_identity(&schema).map_err(|_| ParentError::Config("schema"))?;
    let (profile_id, profile_digest, config_identity, columns) = match &request.profile {
        SourceProfile::Jsonl(profile) => (
            profile.id().to_owned(),
            profile.profile_digest(),
            profile.config_identity(request.object_sha256),
            profile
                .columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect::<Vec<String>>(),
        ),
        SourceProfile::Delimited(profile) => (
            profile.id().to_owned(),
            profile.profile_digest(),
            profile.config_identity(request.object_sha256),
            profile
                .columns()
                .iter()
                .map(|column| column.name().to_owned())
                .collect::<Vec<String>>(),
        ),
    };
    let column_count = u32::try_from(columns.len()).map_err(|_| ParentError::Config("budget"))?;
    Ok(VerifyCtx {
        schema_identity,
        profile_digest,
        config_identity,
        profile_id,
        reader_protocol,
        profile_version,
        lineage_version,
        receipt_version,
        columns,
        jsonl,
        object_sha256: request.object_sha256,
        file_hash,
        frame_cap,
        column_count,
    })
}

fn encode_request(request: &ChildRequest) -> Result<Vec<u8>, ParentError> {
    let profile = match &request.profile {
        SourceProfile::Jsonl(profile) => jsonl_profile_json(profile),
        SourceProfile::Delimited(profile) => delimited_profile_json(profile),
    };
    let mut budget = serde_json::Map::new();
    budget.insert("batch_rows".to_owned(), json_u64(u64::from(request.budget.batch_rows)));
    budget.insert("batch_bytes".to_owned(), json_u64(request.budget.batch_bytes));
    budget.insert(
        "max_record_bytes".to_owned(),
        json_u64(u64::from(request.budget.max_record_bytes)),
    );
    if request.budget.kind == BudgetKind::Delimited {
        budget.insert(
            "max_field_bytes".to_owned(),
            json_u64(u64::from(request.budget.max_field_bytes)),
        );
    }
    budget.insert("max_total_bytes".to_owned(), json_u64(request.budget.max_total_bytes));
    budget.insert("max_rows".to_owned(), json_u64(request.budget.max_rows));
    budget.insert(
        "max_columns".to_owned(),
        json_u64(u64::from(request.budget.max_columns)),
    );
    budget.insert(
        "max_fields".to_owned(),
        json_u64(u64::from(request.budget.max_fields)),
    );
    budget.insert(
        "deadline_ms".to_owned(),
        json_u64(request.budget.worker_deadline_ms),
    );
    let body = serde_json::json!({
        "protocol": WORKER_PROTOCOL,
        "input": {
            "class": "local_file",
            "admission": "caller_admitted_immutable",
            "path": request.path,
        },
        "object_sha256": hex_encode(&request.object_sha256),
        "profile": profile,
        "budget": serde_json::Value::Object(budget),
    });
    let bytes = serde_json::to_vec(&body).map_err(|_| ParentError::Config("request"))?;
    if bytes.is_empty() || bytes.len() > REQUEST_MAX_BYTES {
        return Err(ParentError::Config("request_too_large"));
    }
    Ok(bytes)
}

fn json_u64(value: u64) -> serde_json::Value {
    serde_json::Value::Number(serde_json::Number::from(value))
}

fn jsonl_profile_json(profile: &JsonlProfile) -> serde_json::Value {
    let columns = profile
        .columns()
        .iter()
        .map(|column| {
            serde_json::json!({
                "name": column.name(),
                "type": column.ty().as_str(),
                "nullable": column.nullable(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "format": "jsonl",
        "id": profile.id(),
        "columns": columns,
    })
}

fn delimited_profile_json(profile: &DelimitedProfile) -> serde_json::Value {
    let columns = profile
        .columns()
        .iter()
        .map(|column| {
            let mut value = serde_json::json!({
                "name": column.name(),
                "type": column.ty().as_str(),
                "nullable": column.nullable(),
            });
            if let DelimitedType::Decimal { precision, scale } = column.ty() {
                value["precision"] = json_u64(u64::from(precision));
                value["scale"] = serde_json::json!(scale);
            }
            value
        })
        .collect::<Vec<_>>();
    let null_policy = match profile.nulls() {
        NullPolicy::Empty => serde_json::json!({"policy": "empty"}),
        NullPolicy::Token(text) => serde_json::json!({"policy": "token", "text": text}),
    };
    serde_json::json!({
        "format": "delimited",
        "id": profile.id(),
        "dialect": profile.dialect().as_str(),
        "bom": profile.bom().as_str(),
        "header": profile.header().as_str(),
        "null": null_policy,
        "columns": columns,
    })
}

fn jsonl_schema(profile: &JsonlProfile) -> Result<Schema, ParentError> {
    let mut fields = Vec::new();
    for column in profile.columns() {
        let (data_type, logical) = match column.ty() {
            JsonlType::Utf8 => (DataType::Utf8, "utf8"),
            JsonlType::JsonText => (DataType::Utf8, "json_text"),
            JsonlType::Bool => (DataType::Boolean, "bool"),
            JsonlType::Int64 => (DataType::Int64, "int64"),
        };
        let mut metadata = HashMap::new();
        metadata.insert("oo.jsonl.protocol".to_owned(), JSONL_PROTOCOL.to_owned());
        metadata.insert("oo.jsonl.logical".to_owned(), logical.to_owned());
        if logical == "json_text" {
            metadata.insert(
                "oo.jsonl.encoding".to_owned(),
                "raw-json-utf8".to_owned(),
            );
        }
        fields.push(Field::new(column.name(), data_type, column.nullable()).with_metadata(metadata));
    }
    Ok(Schema::new(fields))
}

fn delimited_schema(profile: &DelimitedProfile) -> Result<Schema, ParentError> {
    let mut fields = Vec::new();
    for column in profile.columns() {
        let (data_type, logical) = match column.ty() {
            DelimitedType::Utf8 => (DataType::Utf8, "utf8"),
            DelimitedType::Bool => (DataType::Boolean, "bool"),
            DelimitedType::Int64 => (DataType::Int64, "int64"),
            DelimitedType::Decimal { precision, scale } => {
                (DataType::Decimal128(precision, scale), "decimal")
            }
        };
        let mut metadata = HashMap::new();
        metadata.insert(
            "oo.delimited.protocol".to_owned(),
            DELIMITED_PROTOCOL.to_owned(),
        );
        metadata.insert("oo.delimited.logical".to_owned(), logical.to_owned());
        if let DelimitedType::Decimal { precision, scale } = column.ty() {
            metadata.insert("oo.delimited.precision".to_owned(), precision.to_string());
            metadata.insert("oo.delimited.scale".to_owned(), scale.to_string());
        }
        fields.push(Field::new(column.name(), data_type, column.nullable()).with_metadata(metadata));
    }
    Ok(Schema::new(fields))
}

fn schema_identity(schema: &Schema) -> Result<[u8; 32], ()> {
    let text = schema_preimage(schema)?;
    Ok(sha256(text.as_bytes()))
}

fn schema_preimage(schema: &Schema) -> Result<String, ()> {
    let mut body = String::from("oo-file-parser-schema/v1\n");
    for field in schema.fields() {
        plain(field.name())?;
        body.push_str("field=");
        body.push_str(field.name());
        body.push('\t');
        body.push_str(&type_token(field.data_type())?);
        body.push('\t');
        body.push(if field.is_nullable() { '1' } else { '0' });
        body.push('\n');
        let mut keys: Vec<&str> = field.metadata().keys().map(String::as_str).collect();
        keys.sort_unstable();
        for key in keys {
            let value = &field.metadata()[key];
            plain(key)?;
            plain(value)?;
            body.push_str("meta=");
            body.push_str(field.name());
            body.push('\t');
            body.push_str(key);
            body.push('\t');
            body.push_str(value);
            body.push('\n');
        }
    }
    Ok(body)
}

fn type_token(ty: &DataType) -> Result<String, ()> {
    match ty {
        DataType::Utf8 => Ok("utf8".to_owned()),
        DataType::Boolean => Ok("bool".to_owned()),
        DataType::Int64 => Ok("int64".to_owned()),
        DataType::Decimal128(precision, scale) => Ok(format!("decimal128:{precision}:{scale}")),
        _ => Err(()),
    }
}

fn plain(text: &str) -> Result<(), ()> {
    if text.contains(['\n', '\t']) {
        Err(())
    } else {
        Ok(())
    }
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
    if !meta.file_type().is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn path_text(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned)
}

fn valid_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.len() > PATH_MAX_BYTES || bytes.contains(&0) {
        return false;
    }
    if !path.starts_with('/') || path.ends_with('/') {
        return false;
    }
    let mut parts = path.split('/');
    if parts.next() != Some("") {
        return false;
    }
    let mut saw_name = false;
    for part in parts {
        if part.is_empty() || part == "." || part == ".." {
            return false;
        }
        saw_name = true;
    }
    saw_name
}

fn private_cwd() -> io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "oo-file-worker-parent-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path)?;
    Ok(path)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(text: &str) -> Result<[u8; 32], ()> {
    if text.len() != 64 {
        return Err(());
    }
    let bytes = text.as_bytes();
    let mut out = [0u8; 32];
    for index in 0..32 {
        let hi = hex_val(bytes[index * 2])?;
        let lo = hex_val(bytes[index * 2 + 1])?;
        out[index] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(()),
    }
}

fn keyed_str<'a>(line: &'a str, key: &str) -> Result<&'a str, ()> {
    let prefix = format!("{key}=");
    line.strip_prefix(&prefix).ok_or(())
}

fn keyed_u64(line: &str, key: &str) -> Result<u64, ()> {
    let text = keyed_str(line, key)?;
    if text.is_empty() || text.starts_with('+') {
        return Err(());
    }
    text.parse().map_err(|_| ())
}

fn row_line(line: &str) -> Result<(u64, u64, u64), ()> {
    let rest = line.strip_prefix("row=").ok_or(())?;
    let mut parts = rest.split('\t');
    let row_index = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let line_no = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let offset = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    if parts.next().is_some() {
        return Err(());
    }
    Ok((row_index, line_no, offset))
}

fn field_line(line: &str, jsonl: bool) -> Result<VerifiedField, ()> {
    let rest = line.strip_prefix("field=").ok_or(())?;
    let mut parts = rest.split('\t');
    let name = parts.next().ok_or(())?.to_owned();
    let line_no = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let offset = parts.next().ok_or(())?.parse().map_err(|_| ())?;
    let presence = match parts.next().ok_or(())? {
        "value" => "value",
        "null" => "null",
        "missing" if jsonl => "missing",
        "empty" if !jsonl => "empty",
        _ => return Err(()),
    };
    if parts.next().is_some() || name.is_empty() {
        return Err(());
    }
    Ok(VerifiedField {
        name,
        line: line_no,
        byte_offset: offset,
        presence,
    })
}

fn kind_for_token(token: Option<&'static str>) -> Option<FailureKind> {
    match token {
        Some("hash_mismatch") => Some(FailureKind::HashMismatch),
        Some("deadline") => Some(FailureKind::Worker),
        _ => None,
    }
}

fn stderr_token(bytes: &[u8]) -> Option<&'static str> {
    let text = std::str::from_utf8(bytes).ok()?;
    let rest = text.strip_prefix("oo-file-parser-worker: ")?;
    let token = rest.strip_suffix('\n')?;
    if rest.matches('\n').count() != 1 {
        return None;
    }
    Some(match token {
        "config" => "config",
        "request_too_large" => "request_too_large",
        "request_syntax" => "request_syntax",
        "duplicate_key" => "duplicate_key",
        "unknown_key" => "unknown_key",
        "unknown_protocol" => "unknown_protocol",
        "credentials" => "credentials",
        "path_form" => "path_form",
        "not_regular" => "not_regular",
        "budget" => "budget",
        "deadline" => "deadline",
        "protocol" => "protocol",
        "header_required" => "header_required",
        "truncated" => "truncated",
        "schema_drift" => "schema_drift",
        "arrow_type" => "arrow_type",
        "schema_meta" => "schema_meta",
        "encoding" => "encoding",
        "hash_mismatch" => "hash_mismatch",
        "io" => "io",
        "ipc_ceiling" => "ipc_ceiling",
        "lineage_ceiling" => "lineage_ceiling",
        "cancelled" => "cancelled",
        "internal" => "internal",
        _ => return None,
    })
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

