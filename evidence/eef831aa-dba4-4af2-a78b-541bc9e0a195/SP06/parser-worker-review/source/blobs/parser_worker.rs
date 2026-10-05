//! One-shot process supervisor for [`guard_select`].
//!
//! The parent sends one length-prefixed frame and reads one frame back. The
//! child is a fresh process for that call. At 500 ms, or on an oversize frame,
//! a malformed frame, a crash, or cancel, the parent sends `SIGKILL` and
//! `wait`s. It does not parse the SQL itself.
//!
//! `env_clear` and this kill are not a kernel network, filesystem, or memory
//! sandbox. [`RESTRICTION_CAPABILITY`] is [`RestrictionCapability::Unsupported`].
//! [`crate::query_executor::UNTRUSTED_ADMISSION`] stays blocked. This module
//! does not authenticate, open a database connection, or serve HTTP.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::sql_guard::{
    guard_select, BoundFunction, BoundOperator, BoundOutputColumn, BoundRelation, GuardError,
    GuardedPlan, LiteralKind, LiteralTypes, OutputItem, PolicyCatalog, PolicyIdentity,
    RecognizedBindings, ResolvedFunction, ResolvedOperator, StarExpansion, VisibleColumn,
    VisibleRelation, NODE_BUDGET, SQL_BYTE_BUDGET,
};

/// ASCII `OOSW`.
pub const MAGIC: [u8; 4] = *b"OOSW";

/// Wire version. A different version is rejected before the body is decoded.
pub const PROTOCOL_VERSION: u16 = 1;

/// Parent to child: identity, catalog, and the original SQL.
pub const KIND_REQUEST: u16 = 1;

/// Child to parent: a [`GuardedPlan`].
pub const KIND_PLAN: u16 = 2;

/// Child to parent: a [`GuardError`].
pub const KIND_REFUSAL: u16 = 3;

/// Inclusive parent deadline for spawn, write, child execution, and the reply read.
pub const PARSE_DEADLINE: Duration = Duration::from_millis(500);

/// Largest frame body accepted. Checked before the body is allocated.
pub const MAX_FRAME_BODY: usize = 8 * 1024 * 1024;

/// Project, role, and policy id cap. Matches the guard's identity rule.
pub const MAX_IDENTITY: usize = 128;

/// Closed identifier cap. Matches the guard's 63-byte identifier rule.
pub const MAX_IDENT: usize = 63;

/// Qualifier, suggestion, cost label, and column-error label cap.
pub const MAX_LABEL: usize = 256;

/// Static guard label cap.
pub const MAX_STATIC: usize = 64;

/// SHA-256 hex digest cap.
pub const MAX_SHA256: usize = 64;

/// `pg_query` fingerprint hex cap.
pub const MAX_FINGERPRINT: usize = 128;

/// Deparsed SQL cap. The original SQL cap is [`SQL_BYTE_BUDGET`].
pub const MAX_EXECUTED_SQL: usize = 1024 * 1024;

/// Relations in one catalog.
pub const MAX_RELATIONS: usize = 128;

/// Columns on one relation.
pub const MAX_COLUMNS: usize = 256;

/// Resolved functions in one catalog.
pub const MAX_FUNCTIONS: usize = 512;

/// Resolved operators in one catalog.
pub const MAX_OPERATORS: usize = 512;

/// Argument OIDs on one function.
pub const MAX_ARGUMENT_OIDS: usize = 100;

/// Star expansions in one plan. Tied to the guard node budget.
pub const MAX_STARS: usize = NODE_BUDGET;

/// Column names recorded for one star.
pub const MAX_STAR_COLUMNS: usize = NODE_BUDGET;

/// Bound `FROM` items in one plan.
pub const MAX_BOUND_RELATIONS: usize = NODE_BUDGET;

/// Select-list items in one plan.
pub const MAX_OUTPUTS: usize = NODE_BUDGET;

/// Bound function calls in one plan.
pub const MAX_BOUND_FUNCTIONS: usize = NODE_BUDGET;

/// Bound operator calls in one plan.
pub const MAX_BOUND_OPERATORS: usize = NODE_BUDGET;

/// Labels inside one column-visibility error.
pub const MAX_AVAILABLE: usize = 512;

pub const TAG_IDENTITY_MISMATCH: u8 = 1;
pub const TAG_CATALOG_INVALID: u8 = 2;
pub const TAG_PARSE: u8 = 3;
pub const TAG_BUDGET: u8 = 4;
pub const TAG_STATEMENT: u8 = 5;
pub const TAG_WRITE: u8 = 6;
pub const TAG_INTO: u8 = 7;
pub const TAG_LOCKING: u8 = 8;
pub const TAG_SYSTEM_SCHEMA: u8 = 9;
pub const TAG_UNSUPPORTED: u8 = 10;
pub const TAG_SERIALIZE: u8 = 11;
pub const TAG_FUNCTION_NOT_ALLOWED: u8 = 12;
pub const TAG_RELATION_NOT_FOUND: u8 = 13;
pub const TAG_COLUMN_NOT_ALLOWED: u8 = 14;
pub const TAG_UNQUALIFIED: u8 = 15;
pub const TAG_COST_TOO_HIGH: u8 = 16;

pub const TAG_OUTPUT_COLUMN: u8 = 1;
pub const TAG_OUTPUT_FUNCTION: u8 = 2;
pub const TAG_OUTPUT_LITERAL: u8 = 3;
pub const TAG_OUTPUT_COMPOUND: u8 = 4;

pub const TAG_LITERAL_INTEGER: u8 = 1;
pub const TAG_LITERAL_TEXT: u8 = 2;
pub const TAG_LITERAL_BOOLEAN: u8 = 3;
pub const TAG_LITERAL_NULL: u8 = 4;

/// `GuardError::CatalogInvalid` labels this guard can emit.
pub const CATALOG_REASONS: &[&str] = &[
    "duplicate column",
    "duplicate function",
    "duplicate operator",
    "duplicate relation",
    "empty column list",
    "function oid",
    "function schema",
    "identifier",
    "identity",
    "operator name",
    "operator oid",
    "physical schema",
    "relation schema",
    "type oid",
];

/// `GuardError::Serialize` labels this guard can emit.
pub const SERIALIZE_REASONS: &[&str] = &[
    "deparse",
    "fingerprint",
    "idempotent",
    "reparse",
    "recheck",
    "second deparse",
    "version",
];

/// `GuardError::Unsupported` labels this guard can emit.
pub const CONSTRUCTS: &[&str] = &[
    "ANY predicate",
    "BETWEEN predicate",
    "CASE",
    "CTE",
    "DISTINCT",
    "GROUP BY",
    "HAVING",
    "IN predicate",
    "JOIN USING",
    "LIKE predicate",
    "LIMIT WITH TIES",
    "NATURAL JOIN",
    "ONLY",
    "VALUES",
    "WINDOW",
    "aggregate star",
    "ast node",
    "bit string",
    "boolean",
    "cast",
    "catalog qualifier",
    "column alias list",
    "column reference",
    "duplicate range name",
    "empty target list",
    "expression",
    "float literal",
    "from item",
    "function",
    "function clause",
    "identifier",
    "join",
    "join alias",
    "join type",
    "limit",
    "literal",
    "null test",
    "operator expression",
    "order key",
    "order nulls",
    "order operator",
    "output alias",
    "parameter",
    "parser version",
    "predicate",
    "range function",
    "relation persistence",
    "set operation",
    "star",
    "star alias",
    "subquery",
    "table function",
    "tablesample",
    "target",
    "target indirection",
    "unary operator",
];

/// Kernel isolation of the worker process.
///
/// `std::process` with a cleared environment is not network, filesystem, or
/// memory isolation. No approved launcher is invoked: `runsc` remains a
/// separate gate, and this crate does not add a dependency, an `unsafe`
/// block, or a privilege to apply namespaces, landlock, seccomp, or rlimits.
/// A grandchild that leaves the process group is not killed by the direct
/// `SIGKILL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestrictionCapability {
    /// The 500 ms kill is implemented. The sandbox is not.
    Unsupported,
}

/// Public untrusted admission stays closed while this is `Unsupported`.
pub const RESTRICTION_CAPABILITY: RestrictionCapability = RestrictionCapability::Unsupported;

/// Codec failure. Display text has no caller bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    /// A count or string exceeded its cap. The value was not allocated.
    Limit,
    /// The buffer ended before the declared field.
    Truncated,
    /// A string was not UTF-8.
    Utf8,
    /// A tag was outside the defined set.
    Tag,
    /// A static label was not one this guard emits.
    Label,
    /// The four-byte magic did not match.
    Magic,
    /// The version was not [`PROTOCOL_VERSION`].
    Version,
    /// The kind was not a request, plan, or refusal, as required by the reader.
    Kind,
    /// Bytes remained after a complete value.
    Trailing,
    /// The pipe failed.
    Io,
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Limit => "frame field exceeds its budget",
            Self::Truncated => "frame ended early",
            Self::Utf8 => "frame string is not utf-8",
            Self::Tag => "frame tag is not valid",
            Self::Label => "frame label is not recognized",
            Self::Magic => "frame magic is not valid",
            Self::Version => "frame version is not valid",
            Self::Kind => "frame kind is not valid",
            Self::Trailing => "frame has trailing bytes",
            Self::Io => "frame io failed",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for CodecError {}

/// One decoded frame. `body` is allocated only after the length cap passes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Always [`PROTOCOL_VERSION`] for a frame this decoder returns.
    pub version: u16,
    /// [`KIND_REQUEST`], [`KIND_PLAN`], or [`KIND_REFUSAL`].
    pub kind: u16,
    /// Payload. Its layout depends on `kind`.
    pub body: Vec<u8>,
}

/// Absolute path approved by the platform. Not part of the SQL payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedWorker {
    path: PathBuf,
}

impl TrustedWorker {
    /// Accept an absolute existing file and store its canonical path.
    ///
    /// The path is separate from [`AdmitRequest`]. Canonicalization resolves
    /// symlinks at approval time; it is not a later substitution check.
    pub fn approve(path: &Path) -> Result<Self, WorkerPathError> {
        if !path.is_absolute() {
            return Err(WorkerPathError::NotAbsolute);
        }
        let canon = path.canonicalize().map_err(|_| WorkerPathError::Unavailable)?;
        if !canon.is_file() {
            return Err(WorkerPathError::Unavailable);
        }
        Ok(Self { path: canon })
    }

    /// Canonical executable path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Why a worker path was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerPathError {
    /// The path was relative.
    NotAbsolute,
    /// The path could not be resolved to a file.
    Unavailable,
}

impl fmt::Display for WorkerPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::NotAbsolute => "worker path is not absolute",
            Self::Unavailable => "worker path is not an available file",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for WorkerPathError {}

/// What the child guards. The executable path is not a field of this value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmitRequest {
    /// Compared with `catalog.identity` by the guard.
    pub identity: PolicyIdentity,
    /// Caller snapshot. OIDs are copied as supplied.
    pub catalog: PolicyCatalog,
    /// Original SQL text.
    pub sql: String,
}

/// Cooperative cancel. The parent kills the live child when this is set.
#[derive(Debug, Default)]
pub struct CancelFlag {
    inner: AtomicBool,
}

impl CancelFlag {
    /// A flag that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the supervisor to kill the current child.
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    /// Whether [`Self::cancel`] has been called.
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}

/// Plan or refusal returned by the child. There is no raw-SQL fallback.
///
/// The plan is boxed so a refusal does not reserve the plan's stack size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParserResponse {
    /// `guard_select` returned a plan.
    Plan(Box<GuardedPlan>),
    /// `guard_select` returned a refusal.
    Refusal(GuardError),
}

/// Observation of the child after `wait`.
///
/// The four flags are separate observations: reaped, still the same process,
/// pid reused, and pid absent. Callers assert them independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ProcessReport {
    /// Operating-system process id captured at spawn.
    pub pid: u32,
    /// Linux `starttime` from `/proc/<pid>/stat`, when it was readable.
    pub start_ticks: Option<u64>,
    /// `wait` returned and the same start time is no longer alive.
    pub reaped: bool,
    /// `/proc/<pid>` still shows the start time captured at spawn.
    pub same_pid_alive: bool,
    /// `/proc/<pid>` exists with a different start time.
    pub pid_reused: bool,
    /// `/proc/<pid>` is absent. Meaningful on Linux.
    pub pid_absent: bool,
    /// Exit status code, when the process was not signalled.
    pub exit_code: Option<i32>,
    /// Terminating signal, when the process was signalled. `9` is `SIGKILL`.
    pub term_signal: Option<i32>,
    /// Time from the start of the call through the reap.
    pub elapsed: Duration,
}

/// Supervisor failure. Display text does not include the SQL or the catalog.
#[derive(Debug, PartialEq, Eq)]
pub enum SuperviseError {
    /// Reserved for a caller that wants this variant. Approval uses [`WorkerPathError`].
    WorkerPath,
    /// The request exceeded a codec budget. No child was started.
    RequestBudget,
    /// The operating system did not start the child.
    Spawn,
    /// The 500 ms clock fired. The child was killed and reaped.
    TimedOut(ProcessReport),
    /// The reply length exceeded [`MAX_FRAME_BODY`] before the body was read.
    Oversize(ProcessReport),
    /// Magic, version, kind, or body did not decode. The child was killed and reaped.
    Malformed(ProcessReport),
    /// The child closed its output or exited without a usable frame.
    Crashed(ProcessReport),
    /// Cancel fired. `None` means it fired before spawn.
    Cancelled(Option<ProcessReport>),
}

impl fmt::Display for SuperviseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::WorkerPath => "worker path was not approved",
            Self::RequestBudget => "parser request exceeds its budget",
            Self::Spawn => "parser worker could not be started",
            Self::TimedOut(_) => "parser worker exceeded the deadline",
            Self::Oversize(_) => "parser worker frame exceeded its budget",
            Self::Malformed(_) => "parser worker frame was malformed",
            Self::Crashed(_) => "parser worker crashed",
            Self::Cancelled(_) => "parser worker was cancelled",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for SuperviseError {}

#[derive(Clone, Copy)]
struct Watch {
    pid: u32,
    start_ticks: Option<u64>,
    started: Instant,
}

/// Run one approved worker. The closure sees the pid before the parent waits.
///
/// `on_start` runs after spawn and before `wait`, so `/proc/<pid>` still names
/// this child on Linux even if the child has already exited. The next call
/// starts a new process. This function never signals a pid from an earlier call.
pub fn supervise_with<F>(
    worker: &TrustedWorker,
    request: &AdmitRequest,
    cancel: &CancelFlag,
    on_start: F,
) -> Result<ParserResponse, SuperviseError>
where
    F: FnOnce(u32),
{
    if cancel.is_cancelled() {
        return Err(SuperviseError::Cancelled(None));
    }
    let body = encode_request(request).map_err(|_| SuperviseError::RequestBudget)?;
    let frame = encode_frame(KIND_REQUEST, &body).map_err(|_| SuperviseError::RequestBudget)?;
    let cwd = OwnedDir::create().map_err(|_| SuperviseError::Spawn)?;
    let (mut child, started) = spawn_worker(worker, &cwd)?;
    let pid = child.id();
    let watch = Watch {
        pid,
        start_ticks: read_start_ticks(pid),
        started,
    };
    on_start(watch.pid);
    let (stdin, stdout) = match (child.stdin.take(), child.stdout.take()) {
        (Some(stdin), Some(stdout)) => (stdin, stdout),
        _ => {
            let _ = stop_child(&mut child, true);
            return Err(SuperviseError::Spawn);
        }
    };
    let (tx, rx) = mpsc::channel();
    let mut io = Some(thread::spawn(move || {
        let outcome = exchange(stdin, stdout, &frame);
        let _ = tx.send(outcome);
    }));
    let deadline = watch.started + PARSE_DEADLINE;
    let outcome = poll_exchange(&mut child, &mut io, &rx, cancel, deadline, watch)?;
    if let Some(handle) = io.take() {
        let _ = handle.join();
    }
    accept_frame(&mut child, outcome, cancel, deadline, watch)
}

fn spawn_worker(worker: &TrustedWorker, cwd: &OwnedDir) -> Result<(Child, Instant), SuperviseError> {
    let mut command = Command::new(worker.path());
    command
        .env_clear()
        .current_dir(&cwd.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let started = Instant::now();
    let child = command.spawn().map_err(|_| SuperviseError::Spawn)?;
    Ok((child, started))
}

fn poll_exchange(
    child: &mut Child,
    io: &mut Option<thread::JoinHandle<()>>,
    rx: &mpsc::Receiver<IoOutcome>,
    cancel: &CancelFlag,
    deadline: Instant,
    watch: Watch,
) -> Result<IoOutcome, SuperviseError> {
    loop {
        if cancel.is_cancelled() {
            return Err(finish(
                child,
                io,
                watch.pid,
                watch.start_ticks,
                watch.started,
                true,
                |report| SuperviseError::Cancelled(Some(report)),
            ));
        }
        if Instant::now() >= deadline {
            return Err(stop_timed_out(child, io, watch));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        let slice = Duration::from_millis(10).min(left);
        match rx.recv_timeout(slice) {
            Ok(_) if Instant::now() >= deadline => return Err(stop_timed_out(child, io, watch)),
            Ok(outcome) => return Ok(outcome),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(finish(
                    child,
                    io,
                    watch.pid,
                    watch.start_ticks,
                    watch.started,
                    true,
                    SuperviseError::Crashed,
                ));
            }
        }
    }
}

fn stop_timed_out(
    child: &mut Child,
    io: &mut Option<thread::JoinHandle<()>>,
    watch: Watch,
) -> SuperviseError {
    finish(
        child,
        io,
        watch.pid,
        watch.start_ticks,
        watch.started,
        true,
        SuperviseError::TimedOut,
    )
}

fn accept_frame(
    child: &mut Child,
    outcome: IoOutcome,
    cancel: &CancelFlag,
    deadline: Instant,
    watch: Watch,
) -> Result<ParserResponse, SuperviseError> {
    let make: fn(ProcessReport) -> SuperviseError = match outcome {
        IoOutcome::Oversize => SuperviseError::Oversize,
        IoOutcome::Malformed => SuperviseError::Malformed,
        IoOutcome::Ended | IoOutcome::Io => SuperviseError::Crashed,
        IoOutcome::Frame { kind, body } => {
            return finish_frame(child, kind, &body, cancel, deadline, watch);
        }
    };
    Err(finish_joined(
        child,
        watch.pid,
        watch.start_ticks,
        watch.started,
        true,
        make,
    ))
}

fn finish_frame(
    child: &mut Child,
    kind: u16,
    body: &[u8],
    cancel: &CancelFlag,
    deadline: Instant,
    watch: Watch,
) -> Result<ParserResponse, SuperviseError> {
    let status = match wait_exit(child, deadline, cancel) {
        WaitStop::Exited(status) => status,
        WaitStop::Cancel => {
            return Err(finish_joined(
                child,
                watch.pid,
                watch.start_ticks,
                watch.started,
                true,
                |report| SuperviseError::Cancelled(Some(report)),
            ));
        }
        WaitStop::Deadline => {
            return Err(finish_joined(
                child,
                watch.pid,
                watch.start_ticks,
                watch.started,
                true,
                SuperviseError::TimedOut,
            ));
        }
        WaitStop::Error => {
            return Err(finish_joined(
                child,
                watch.pid,
                watch.start_ticks,
                watch.started,
                true,
                SuperviseError::Crashed,
            ));
        }
    };
    let report = observe(watch.pid, watch.start_ticks, Some(status), watch.started);
    if !status.success() {
        return Err(SuperviseError::Crashed(report));
    }
    decode_response(kind, body).map_err(|_| SuperviseError::Malformed(report))
}

/// [`supervise_with`] with no spawn observer.
pub fn supervise(
    worker: &TrustedWorker,
    request: &AdmitRequest,
    cancel: &CancelFlag,
) -> Result<ParserResponse, SuperviseError> {
    supervise_with(worker, request, cancel, |_pid| {})
}

/// Child entry. Returns `2` when an argument is present or when
/// `OO_PARSER_WORKER_SLOW` is set, `1` on a protocol failure, and `0` after
/// one reply frame. The variable is rejected. It is not a slow path. Fault
/// modes are not implemented in this binary.
pub fn worker_main() -> i32 {
    if std::env::args_os().nth(1).is_some() || std::env::var_os("OO_PARSER_WORKER_SLOW").is_some()
    {
        return 2;
    }
    match worker_once() {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn worker_once() -> Result<(), CodecError> {
    let mut input = io::stdin().lock();
    let frame = read_frame_from(&mut input)?;
    if frame.kind != KIND_REQUEST {
        return Err(CodecError::Kind);
    }
    let request = decode_request(&frame.body)?;
    drop(input);
    let encoded = match guard_select(&request.identity, &request.catalog, &request.sql) {
        Ok(plan) => encode_frame(KIND_PLAN, &encode_plan(&plan)?)?,
        Err(err) => encode_frame(KIND_REFUSAL, &encode_refusal(&err)?)?,
    };
    let mut output = io::stdout().lock();
    output.write_all(&encoded).map_err(|_| CodecError::Io)?;
    output.flush().map_err(|_| CodecError::Io)?;
    Ok(())
}

fn decode_response(kind: u16, body: &[u8]) -> Result<ParserResponse, CodecError> {
    match kind {
        KIND_PLAN => Ok(ParserResponse::Plan(Box::new(decode_plan(body)?))),
        KIND_REFUSAL => Ok(ParserResponse::Refusal(decode_refusal(body)?)),
        _ => Err(CodecError::Kind),
    }
}

enum WaitStop {
    Exited(ExitStatus),
    Cancel,
    Deadline,
    Error,
}

fn wait_exit(child: &mut Child, deadline: Instant, cancel: &CancelFlag) -> WaitStop {
    loop {
        if cancel.is_cancelled() {
            return WaitStop::Cancel;
        }
        if Instant::now() >= deadline {
            return WaitStop::Deadline;
        }
        match child.try_wait() {
            Ok(Some(status)) => return WaitStop::Exited(status),
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => return WaitStop::Error,
        }
    }
}

fn finish(
    child: &mut Child,
    io: &mut Option<thread::JoinHandle<()>>,
    pid: u32,
    start_ticks: Option<u64>,
    started: Instant,
    kill: bool,
    make: impl FnOnce(ProcessReport) -> SuperviseError,
) -> SuperviseError {
    let status = stop_child(child, kill);
    if let Some(handle) = io.take() {
        let _ = handle.join();
    }
    make(observe(pid, start_ticks, status, started))
}

fn finish_joined(
    child: &mut Child,
    pid: u32,
    start_ticks: Option<u64>,
    started: Instant,
    kill: bool,
    make: impl FnOnce(ProcessReport) -> SuperviseError,
) -> SuperviseError {
    let status = stop_child(child, kill);
    make(observe(pid, start_ticks, status, started))
}

fn stop_child(child: &mut Child, kill: bool) -> Option<ExitStatus> {
    if kill {
        let _ = child.kill();
    }
    child.wait().ok()
}

fn observe(
    pid: u32,
    recorded: Option<u64>,
    status: Option<ExitStatus>,
    started: Instant,
) -> ProcessReport {
    let (same, reused, absent) = pid_view(pid, recorded);
    ProcessReport {
        pid,
        start_ticks: recorded,
        reaped: status.is_some() && !same,
        same_pid_alive: same,
        pid_reused: reused,
        pid_absent: absent,
        exit_code: status.and_then(|item| item.code()),
        term_signal: status.and_then(term_signal),
        elapsed: started.elapsed(),
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

#[cfg(target_os = "linux")]
fn read_start_ticks(pid: u32) -> Option<u64> {
    proc_start_ticks(pid)
}

#[cfg(not(target_os = "linux"))]
fn read_start_ticks(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn proc_start_ticks(pid: u32) -> Option<u64> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let end = text.rfind(')')?;
    let from = end.checked_add(2)?;
    let rest = text.get(from..)?;
    rest.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(target_os = "linux")]
fn pid_view(pid: u32, recorded: Option<u64>) -> (bool, bool, bool) {
    match (recorded, proc_start_ticks(pid)) {
        (_, None) => (false, false, true),
        (Some(old), Some(now)) if old == now => (true, false, false),
        (Some(_), Some(_)) => (false, true, false),
        (None, Some(_)) => (false, false, false),
    }
}

#[cfg(not(target_os = "linux"))]
fn pid_view(_pid: u32, _recorded: Option<u64>) -> (bool, bool, bool) {
    (false, false, false)
}

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

struct OwnedDir(PathBuf);

impl OwnedDir {
    fn create() -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "oo-parser-worker-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for OwnedDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

enum IoOutcome {
    Frame { kind: u16, body: Vec<u8> },
    Oversize,
    Malformed,
    Ended,
    Io,
}

fn exchange(mut stdin: ChildStdin, mut stdout: ChildStdout, request: &[u8]) -> IoOutcome {
    if stdin.write_all(request).and_then(|()| stdin.flush()).is_err() {
        return IoOutcome::Ended;
    }
    drop(stdin);
    let mut magic = [0u8; 4];
    if read_exact(&mut stdout, &mut magic).is_err() {
        return IoOutcome::Ended;
    }
    if magic != MAGIC {
        return IoOutcome::Malformed;
    }
    let mut rest = [0u8; 8];
    if read_exact(&mut stdout, &mut rest).is_err() {
        return IoOutcome::Malformed;
    }
    let version = u16::from_le_bytes([rest[0], rest[1]]);
    if version != PROTOCOL_VERSION {
        return IoOutcome::Malformed;
    }
    let kind = u16::from_le_bytes([rest[2], rest[3]]);
    let len = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
    if len > MAX_FRAME_BODY {
        return IoOutcome::Oversize;
    }
    if kind != KIND_PLAN && kind != KIND_REFUSAL {
        return IoOutcome::Malformed;
    }
    let mut body = vec![0u8; len];
    if len > 0 && read_exact(&mut stdout, &mut body).is_err() {
        return IoOutcome::Ended;
    }
    let mut extra = [0u8; 1];
    loop {
        match stdout.read(&mut extra) {
            Ok(0) => break,
            Ok(_) => return IoOutcome::Malformed,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(_) => return IoOutcome::Io,
        }
    }
    IoOutcome::Frame { kind, body }
}

fn read_exact(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof"));
            }
            Ok(count) => filled += count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Encode a frame. The body is copied only when `body.len()` is within the cap.
pub fn encode_frame(kind: u16, body: &[u8]) -> Result<Vec<u8>, CodecError> {
    if body.len() > MAX_FRAME_BODY {
        return Err(CodecError::Limit);
    }
    let mut out = Vec::new();
    out.try_reserve(12usize.saturating_add(body.len()))
        .map_err(|_| CodecError::Limit)?;
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    let len = u32::try_from(body.len()).map_err(|_| CodecError::Limit)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// Decode one complete buffer. A declared length above the cap returns before
/// the body is copied.
pub fn decode_frame(bytes: &[u8]) -> Result<Frame, CodecError> {
    if bytes.len() < 4 {
        return Err(CodecError::Truncated);
    }
    if bytes[0..4] != MAGIC {
        return Err(CodecError::Magic);
    }
    if bytes.len() < 12 {
        return Err(CodecError::Truncated);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != PROTOCOL_VERSION {
        return Err(CodecError::Version);
    }
    let kind = u16::from_le_bytes([bytes[6], bytes[7]]);
    let len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if len > MAX_FRAME_BODY {
        return Err(CodecError::Limit);
    }
    let total = 12usize.checked_add(len).ok_or(CodecError::Limit)?;
    if bytes.len() < total {
        return Err(CodecError::Truncated);
    }
    if bytes.len() != total {
        return Err(CodecError::Trailing);
    }
    Ok(Frame {
        version,
        kind,
        body: bytes[12..total].to_vec(),
    })
}

fn read_frame_from(reader: &mut impl Read) -> Result<Frame, CodecError> {
    let mut header = [0u8; 12];
    read_exact(reader, &mut header).map_err(|_| CodecError::Truncated)?;
    if header[0..4] != MAGIC {
        return Err(CodecError::Magic);
    }
    let version = u16::from_le_bytes([header[4], header[5]]);
    if version != PROTOCOL_VERSION {
        return Err(CodecError::Version);
    }
    let kind = u16::from_le_bytes([header[6], header[7]]);
    let len = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if len > MAX_FRAME_BODY {
        return Err(CodecError::Limit);
    }
    let mut body = vec![0u8; len];
    if len > 0 {
        read_exact(reader, &mut body).map_err(|_| CodecError::Truncated)?;
    }
    Ok(Frame { version, kind, body })
}

/// Encode the request body.
pub fn encode_request(request: &AdmitRequest) -> Result<Vec<u8>, CodecError> {
    let mut writer = Writer::new();
    write_identity(&mut writer, &request.identity)?;
    write_catalog(&mut writer, &request.catalog)?;
    writer.str(&request.sql, SQL_BYTE_BUDGET)?;
    Ok(writer.into_vec())
}

/// Decode a request body. Counts and string lengths are capped first.
pub fn decode_request(bytes: &[u8]) -> Result<AdmitRequest, CodecError> {
    let mut reader = Reader::new(bytes);
    let identity = read_identity(&mut reader)?;
    let catalog = read_catalog(&mut reader)?;
    let sql = reader.str(SQL_BYTE_BUDGET)?;
    reader.finish()?;
    Ok(AdmitRequest {
        identity,
        catalog,
        sql,
    })
}

/// Encode a plan body.
pub fn encode_plan(plan: &GuardedPlan) -> Result<Vec<u8>, CodecError> {
    let mut writer = Writer::new();
    write_identity(&mut writer, &plan.identity)?;
    writer.str(&plan.original_sql, SQL_BYTE_BUDGET)?;
    writer.str(&plan.executed_sql, MAX_EXECUTED_SQL)?;
    writer.str(&plan.original_sha256, MAX_SHA256)?;
    writer.str(&plan.executed_sha256, MAX_SHA256)?;
    writer.str(&plan.fingerprint_hex, MAX_FINGERPRINT)?;
    writer.i32(plan.parser_version)?;
    write_list(&mut writer, &plan.star_expansions, MAX_STARS, write_star)?;
    write_bindings(&mut writer, &plan.bindings)?;
    Ok(writer.into_vec())
}

/// Decode a plan body.
pub fn decode_plan(bytes: &[u8]) -> Result<GuardedPlan, CodecError> {
    let mut reader = Reader::new(bytes);
    let identity = read_identity(&mut reader)?;
    let original_sql = reader.str(SQL_BYTE_BUDGET)?;
    let executed_sql = reader.str(MAX_EXECUTED_SQL)?;
    let original_sha256 = reader.str(MAX_SHA256)?;
    let executed_sha256 = reader.str(MAX_SHA256)?;
    let fingerprint_hex = reader.str(MAX_FINGERPRINT)?;
    let parser_version = reader.i32()?;
    let star_expansions = read_items(&mut reader, MAX_STARS, read_star)?;
    let bindings = read_bindings(&mut reader)?;
    reader.finish()?;
    Ok(GuardedPlan {
        identity,
        original_sql,
        executed_sql,
        original_sha256,
        executed_sha256,
        fingerprint_hex,
        parser_version,
        star_expansions,
        bindings,
    })
}

/// Encode a refusal body. The SQL text is not a field.
pub fn encode_refusal(err: &GuardError) -> Result<Vec<u8>, CodecError> {
    let mut writer = Writer::new();
    match err {
        GuardError::IdentityMismatch => writer.u8(TAG_IDENTITY_MISMATCH)?,
        GuardError::CatalogInvalid { reason } => {
            writer.u8(TAG_CATALOG_INVALID)?;
            write_label(&mut writer, reason, CATALOG_REASONS)?;
        }
        GuardError::Parse => writer.u8(TAG_PARSE)?,
        GuardError::Budget => writer.u8(TAG_BUDGET)?,
        GuardError::Statement => writer.u8(TAG_STATEMENT)?,
        GuardError::Write => writer.u8(TAG_WRITE)?,
        GuardError::Into => writer.u8(TAG_INTO)?,
        GuardError::Locking => writer.u8(TAG_LOCKING)?,
        GuardError::SystemSchema => writer.u8(TAG_SYSTEM_SCHEMA)?,
        GuardError::Unsupported { construct } => {
            writer.u8(TAG_UNSUPPORTED)?;
            write_label(&mut writer, construct, CONSTRUCTS)?;
        }
        GuardError::Serialize { reason } => {
            writer.u8(TAG_SERIALIZE)?;
            write_label(&mut writer, reason, SERIALIZE_REASONS)?;
        }
        GuardError::FunctionNotAllowed => writer.u8(TAG_FUNCTION_NOT_ALLOWED)?,
        GuardError::RelationNotFound => writer.u8(TAG_RELATION_NOT_FOUND)?,
        GuardError::ColumnNotAllowed {
            available,
            pending,
            ambiguous,
        } => {
            writer.u8(TAG_COLUMN_NOT_ALLOWED)?;
            write_strings(&mut writer, available, MAX_AVAILABLE, MAX_LABEL)?;
            writer.u32(*pending)?;
            writer.flag(*ambiguous)?;
        }
        GuardError::Unqualified { suggestion } => {
            writer.u8(TAG_UNQUALIFIED)?;
            writer.opt_str(suggestion, MAX_LABEL)?;
        }
        GuardError::CostTooHigh { relation } => {
            writer.u8(TAG_COST_TOO_HIGH)?;
            writer.str(relation, MAX_LABEL)?;
        }
    }
    Ok(writer.into_vec())
}

/// Decode a refusal body back onto the guard's static labels.
pub fn decode_refusal(bytes: &[u8]) -> Result<GuardError, CodecError> {
    let mut reader = Reader::new(bytes);
    let err = match reader.u8()? {
        TAG_IDENTITY_MISMATCH => GuardError::IdentityMismatch,
        TAG_CATALOG_INVALID => GuardError::CatalogInvalid {
            reason: read_label(&mut reader, CATALOG_REASONS)?,
        },
        TAG_PARSE => GuardError::Parse,
        TAG_BUDGET => GuardError::Budget,
        TAG_STATEMENT => GuardError::Statement,
        TAG_WRITE => GuardError::Write,
        TAG_INTO => GuardError::Into,
        TAG_LOCKING => GuardError::Locking,
        TAG_SYSTEM_SCHEMA => GuardError::SystemSchema,
        TAG_UNSUPPORTED => GuardError::Unsupported {
            construct: read_label(&mut reader, CONSTRUCTS)?,
        },
        TAG_SERIALIZE => GuardError::Serialize {
            reason: read_label(&mut reader, SERIALIZE_REASONS)?,
        },
        TAG_FUNCTION_NOT_ALLOWED => GuardError::FunctionNotAllowed,
        TAG_RELATION_NOT_FOUND => GuardError::RelationNotFound,
        TAG_COLUMN_NOT_ALLOWED => GuardError::ColumnNotAllowed {
            available: read_strings(&mut reader, MAX_AVAILABLE, MAX_LABEL)?,
            pending: reader.u32()?,
            ambiguous: reader.flag()?,
        },
        TAG_UNQUALIFIED => GuardError::Unqualified {
            suggestion: reader.opt_str(MAX_LABEL)?,
        },
        TAG_COST_TOO_HIGH => GuardError::CostTooHigh {
            relation: reader.str(MAX_LABEL)?,
        },
        _ => return Err(CodecError::Tag),
    };
    reader.finish()?;
    Ok(err)
}

fn write_identity(writer: &mut Writer, identity: &PolicyIdentity) -> Result<(), CodecError> {
    writer.str(&identity.project, MAX_IDENTITY)?;
    writer.str(&identity.role, MAX_IDENTITY)?;
    writer.str(&identity.policy_id, MAX_IDENTITY)
}

fn read_identity(reader: &mut Reader<'_>) -> Result<PolicyIdentity, CodecError> {
    Ok(PolicyIdentity {
        project: reader.str(MAX_IDENTITY)?,
        role: reader.str(MAX_IDENTITY)?,
        policy_id: reader.str(MAX_IDENTITY)?,
    })
}

fn write_catalog(writer: &mut Writer, catalog: &PolicyCatalog) -> Result<(), CodecError> {
    write_identity(writer, &catalog.identity)?;
    write_list(writer, &catalog.relations, MAX_RELATIONS, write_relation)?;
    write_list(writer, &catalog.functions, MAX_FUNCTIONS, write_function)?;
    write_list(writer, &catalog.operators, MAX_OPERATORS, write_operator)?;
    writer.opt_u32(catalog.literal_types.integer)?;
    writer.opt_u32(catalog.literal_types.text)?;
    writer.opt_u32(catalog.literal_types.boolean)?;
    writer.flag(catalog.null_test_allowed)?;
    writer.u64(catalog.large_table_row_threshold)
}

fn read_catalog(reader: &mut Reader<'_>) -> Result<PolicyCatalog, CodecError> {
    Ok(PolicyCatalog {
        identity: read_identity(reader)?,
        relations: read_items(reader, MAX_RELATIONS, read_relation)?,
        functions: read_items(reader, MAX_FUNCTIONS, read_function)?,
        operators: read_items(reader, MAX_OPERATORS, read_operator)?,
        literal_types: LiteralTypes {
            integer: reader.opt_u32()?,
            text: reader.opt_u32()?,
            boolean: reader.opt_u32()?,
        },
        null_test_allowed: reader.flag()?,
        large_table_row_threshold: reader.u64()?,
    })
}

fn write_relation(writer: &mut Writer, relation: &VisibleRelation) -> Result<(), CodecError> {
    writer.str(&relation.schema, MAX_IDENT)?;
    writer.str(&relation.name, MAX_IDENT)?;
    writer.str(&relation.physical_schema, MAX_IDENT)?;
    writer.str(&relation.physical_name, MAX_IDENT)?;
    write_list(writer, &relation.columns, MAX_COLUMNS, write_column)?;
    writer.u32(relation.pending_unclassified)?;
    writer.u64(relation.estimated_rows)
}

fn read_relation(reader: &mut Reader<'_>) -> Result<VisibleRelation, CodecError> {
    Ok(VisibleRelation {
        schema: reader.str(MAX_IDENT)?,
        name: reader.str(MAX_IDENT)?,
        physical_schema: reader.str(MAX_IDENT)?,
        physical_name: reader.str(MAX_IDENT)?,
        columns: read_items(reader, MAX_COLUMNS, read_column)?,
        pending_unclassified: reader.u32()?,
        estimated_rows: reader.u64()?,
    })
}

fn write_column(writer: &mut Writer, column: &VisibleColumn) -> Result<(), CodecError> {
    writer.str(&column.name, MAX_IDENT)?;
    writer.str(&column.physical_name, MAX_IDENT)?;
    writer.u32(column.type_oid)
}

fn read_column(reader: &mut Reader<'_>) -> Result<VisibleColumn, CodecError> {
    Ok(VisibleColumn {
        name: reader.str(MAX_IDENT)?,
        physical_name: reader.str(MAX_IDENT)?,
        type_oid: reader.u32()?,
    })
}

fn write_function(writer: &mut Writer, function: &ResolvedFunction) -> Result<(), CodecError> {
    writer.u32(function.oid)?;
    writer.str(&function.schema, MAX_IDENT)?;
    writer.str(&function.name, MAX_IDENT)?;
    write_u32s(writer, &function.argument_type_oids, MAX_ARGUMENT_OIDS)?;
    writer.flag(function.aggregate_star)
}

fn read_function(reader: &mut Reader<'_>) -> Result<ResolvedFunction, CodecError> {
    Ok(ResolvedFunction {
        oid: reader.u32()?,
        schema: reader.str(MAX_IDENT)?,
        name: reader.str(MAX_IDENT)?,
        argument_type_oids: read_u32s(reader, MAX_ARGUMENT_OIDS)?,
        aggregate_star: reader.flag()?,
    })
}

fn write_operator(writer: &mut Writer, operator: &ResolvedOperator) -> Result<(), CodecError> {
    writer.u32(operator.oid)?;
    writer.u32(operator.procedure_oid)?;
    writer.str(&operator.schema, MAX_IDENT)?;
    writer.str(&operator.name, MAX_IDENT)?;
    writer.u32(operator.left_type_oid)?;
    writer.u32(operator.right_type_oid)
}

fn read_operator(reader: &mut Reader<'_>) -> Result<ResolvedOperator, CodecError> {
    Ok(ResolvedOperator {
        oid: reader.u32()?,
        procedure_oid: reader.u32()?,
        schema: reader.str(MAX_IDENT)?,
        name: reader.str(MAX_IDENT)?,
        left_type_oid: reader.u32()?,
        right_type_oid: reader.u32()?,
    })
}

fn write_star(writer: &mut Writer, star: &StarExpansion) -> Result<(), CodecError> {
    writer.str(&star.qualifier, MAX_LABEL)?;
    writer.str(&star.schema, MAX_IDENT)?;
    writer.str(&star.name, MAX_IDENT)?;
    write_strings(writer, &star.columns, MAX_STAR_COLUMNS, MAX_IDENT)
}

fn read_star(reader: &mut Reader<'_>) -> Result<StarExpansion, CodecError> {
    Ok(StarExpansion {
        qualifier: reader.str(MAX_LABEL)?,
        schema: reader.str(MAX_IDENT)?,
        name: reader.str(MAX_IDENT)?,
        columns: read_strings(reader, MAX_STAR_COLUMNS, MAX_IDENT)?,
    })
}

fn write_bindings(writer: &mut Writer, bindings: &RecognizedBindings) -> Result<(), CodecError> {
    write_list(writer, &bindings.relations, MAX_BOUND_RELATIONS, write_bound_relation)?;
    write_list(writer, &bindings.outputs, MAX_OUTPUTS, write_output)?;
    write_list(
        writer,
        &bindings.functions,
        MAX_BOUND_FUNCTIONS,
        write_bound_function,
    )?;
    write_list(
        writer,
        &bindings.operators,
        MAX_BOUND_OPERATORS,
        write_bound_operator,
    )?;
    writer.u32(bindings.null_tests)
}

fn read_bindings(reader: &mut Reader<'_>) -> Result<RecognizedBindings, CodecError> {
    Ok(RecognizedBindings {
        relations: read_items(reader, MAX_BOUND_RELATIONS, read_bound_relation)?,
        outputs: read_items(reader, MAX_OUTPUTS, read_output)?,
        functions: read_items(reader, MAX_BOUND_FUNCTIONS, read_bound_function)?,
        operators: read_items(reader, MAX_BOUND_OPERATORS, read_bound_operator)?,
        null_tests: reader.u32()?,
    })
}

fn write_bound_relation(writer: &mut Writer, relation: &BoundRelation) -> Result<(), CodecError> {
    writer.str(&relation.schema, MAX_IDENT)?;
    writer.str(&relation.name, MAX_IDENT)?;
    writer.opt_str(&relation.alias, MAX_IDENT)?;
    writer.str(&relation.physical_schema, MAX_IDENT)?;
    writer.str(&relation.physical_name, MAX_IDENT)
}

fn read_bound_relation(reader: &mut Reader<'_>) -> Result<BoundRelation, CodecError> {
    Ok(BoundRelation {
        schema: reader.str(MAX_IDENT)?,
        name: reader.str(MAX_IDENT)?,
        alias: reader.opt_str(MAX_IDENT)?,
        physical_schema: reader.str(MAX_IDENT)?,
        physical_name: reader.str(MAX_IDENT)?,
    })
}

fn write_output(writer: &mut Writer, item: &OutputItem) -> Result<(), CodecError> {
    match item {
        OutputItem::Column(column) => {
            writer.u8(TAG_OUTPUT_COLUMN)?;
            writer.str(&column.relation_schema, MAX_IDENT)?;
            writer.str(&column.relation_name, MAX_IDENT)?;
            writer.str(&column.name, MAX_IDENT)?;
            writer.str(&column.physical_name, MAX_IDENT)?;
            writer.str(&column.output_name, MAX_IDENT)?;
            writer.u32(column.type_oid)
        }
        OutputItem::Function(function) => {
            writer.u8(TAG_OUTPUT_FUNCTION)?;
            write_bound_function(writer, function)
        }
        OutputItem::Literal(kind) => {
            writer.u8(TAG_OUTPUT_LITERAL)?;
            writer.u8(literal_tag(*kind))
        }
        OutputItem::Compound => writer.u8(TAG_OUTPUT_COMPOUND),
    }
}

fn read_output(reader: &mut Reader<'_>) -> Result<OutputItem, CodecError> {
    match reader.u8()? {
        TAG_OUTPUT_COLUMN => Ok(OutputItem::Column(BoundOutputColumn {
            relation_schema: reader.str(MAX_IDENT)?,
            relation_name: reader.str(MAX_IDENT)?,
            name: reader.str(MAX_IDENT)?,
            physical_name: reader.str(MAX_IDENT)?,
            output_name: reader.str(MAX_IDENT)?,
            type_oid: reader.u32()?,
        })),
        TAG_OUTPUT_FUNCTION => Ok(OutputItem::Function(read_bound_function(reader)?)),
        TAG_OUTPUT_LITERAL => Ok(OutputItem::Literal(literal_kind(reader.u8()?)?)),
        TAG_OUTPUT_COMPOUND => Ok(OutputItem::Compound),
        _ => Err(CodecError::Tag),
    }
}

fn write_bound_function(writer: &mut Writer, function: &BoundFunction) -> Result<(), CodecError> {
    writer.u32(function.oid)?;
    writer.str(&function.schema, MAX_IDENT)?;
    writer.str(&function.name, MAX_IDENT)?;
    writer.flag(function.aggregate_star)
}

fn read_bound_function(reader: &mut Reader<'_>) -> Result<BoundFunction, CodecError> {
    Ok(BoundFunction {
        oid: reader.u32()?,
        schema: reader.str(MAX_IDENT)?,
        name: reader.str(MAX_IDENT)?,
        aggregate_star: reader.flag()?,
    })
}

fn write_bound_operator(writer: &mut Writer, operator: &BoundOperator) -> Result<(), CodecError> {
    writer.u32(operator.oid)?;
    writer.u32(operator.procedure_oid)?;
    writer.str(&operator.schema, MAX_IDENT)?;
    writer.str(&operator.name, MAX_IDENT)
}

fn read_bound_operator(reader: &mut Reader<'_>) -> Result<BoundOperator, CodecError> {
    Ok(BoundOperator {
        oid: reader.u32()?,
        procedure_oid: reader.u32()?,
        schema: reader.str(MAX_IDENT)?,
        name: reader.str(MAX_IDENT)?,
    })
}

fn literal_tag(kind: LiteralKind) -> u8 {
    match kind {
        LiteralKind::Integer => TAG_LITERAL_INTEGER,
        LiteralKind::Text => TAG_LITERAL_TEXT,
        LiteralKind::Boolean => TAG_LITERAL_BOOLEAN,
        LiteralKind::Null => TAG_LITERAL_NULL,
    }
}

fn literal_kind(tag: u8) -> Result<LiteralKind, CodecError> {
    match tag {
        TAG_LITERAL_INTEGER => Ok(LiteralKind::Integer),
        TAG_LITERAL_TEXT => Ok(LiteralKind::Text),
        TAG_LITERAL_BOOLEAN => Ok(LiteralKind::Boolean),
        TAG_LITERAL_NULL => Ok(LiteralKind::Null),
        _ => Err(CodecError::Tag),
    }
}

fn write_label(writer: &mut Writer, value: &str, allowed: &[&str]) -> Result<(), CodecError> {
    if !allowed.contains(&value) {
        return Err(CodecError::Label);
    }
    writer.str(value, MAX_STATIC)
}

fn read_label(reader: &mut Reader<'_>, allowed: &[&'static str]) -> Result<&'static str, CodecError> {
    let value = reader.str(MAX_STATIC)?;
    allowed
        .iter()
        .copied()
        .find(|item| *item == value)
        .ok_or(CodecError::Label)
}

fn write_strings(
    writer: &mut Writer,
    values: &[String],
    max_count: usize,
    max_len: usize,
) -> Result<(), CodecError> {
    if values.len() > max_count {
        return Err(CodecError::Limit);
    }
    writer.u32(u32::try_from(values.len()).map_err(|_| CodecError::Limit)?)?;
    for value in values {
        writer.str(value, max_len)?;
    }
    Ok(())
}

fn read_strings(
    reader: &mut Reader<'_>,
    max_count: usize,
    max_len: usize,
) -> Result<Vec<String>, CodecError> {
    let count = reader.count(max_count)?;
    let mut values = Vec::new();
    values.try_reserve(count).map_err(|_| CodecError::Limit)?;
    for _ in 0..count {
        values.push(reader.str(max_len)?);
    }
    Ok(values)
}

fn write_u32s(writer: &mut Writer, values: &[u32], max: usize) -> Result<(), CodecError> {
    if values.len() > max {
        return Err(CodecError::Limit);
    }
    writer.u32(u32::try_from(values.len()).map_err(|_| CodecError::Limit)?)?;
    for value in values {
        writer.u32(*value)?;
    }
    Ok(())
}

fn read_u32s(reader: &mut Reader<'_>, max: usize) -> Result<Vec<u32>, CodecError> {
    let count = reader.count(max)?;
    let mut values = Vec::new();
    values.try_reserve(count).map_err(|_| CodecError::Limit)?;
    for _ in 0..count {
        values.push(reader.u32()?);
    }
    Ok(values)
}

fn write_list<T>(
    writer: &mut Writer,
    values: &[T],
    max: usize,
    mut one: impl FnMut(&mut Writer, &T) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    if values.len() > max {
        return Err(CodecError::Limit);
    }
    writer.u32(u32::try_from(values.len()).map_err(|_| CodecError::Limit)?)?;
    for value in values {
        one(writer, value)?;
    }
    Ok(())
}

fn read_items<T>(
    reader: &mut Reader<'_>,
    max: usize,
    mut one: impl FnMut(&mut Reader<'_>) -> Result<T, CodecError>,
) -> Result<Vec<T>, CodecError> {
    let count = reader.count(max)?;
    let mut items = Vec::new();
    items.try_reserve(count).map_err(|_| CodecError::Limit)?;
    for _ in 0..count {
        items.push(one(reader)?);
    }
    Ok(items)
}

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    fn room(&self, extra: usize) -> Result<(), CodecError> {
        if self.buf.len().saturating_add(extra) > MAX_FRAME_BODY {
            Err(CodecError::Limit)
        } else {
            Ok(())
        }
    }

    fn bytes(&mut self, value: &[u8]) -> Result<(), CodecError> {
        self.room(value.len())?;
        self.buf.extend_from_slice(value);
        Ok(())
    }

    fn u8(&mut self, value: u8) -> Result<(), CodecError> {
        self.bytes(&[value])
    }

    fn u32(&mut self, value: u32) -> Result<(), CodecError> {
        self.bytes(&value.to_le_bytes())
    }

    fn u64(&mut self, value: u64) -> Result<(), CodecError> {
        self.bytes(&value.to_le_bytes())
    }

    fn i32(&mut self, value: i32) -> Result<(), CodecError> {
        self.bytes(&value.to_le_bytes())
    }

    fn flag(&mut self, value: bool) -> Result<(), CodecError> {
        self.u8(u8::from(value))
    }

    fn str(&mut self, value: &str, max: usize) -> Result<(), CodecError> {
        if value.len() > max {
            return Err(CodecError::Limit);
        }
        self.u32(u32::try_from(value.len()).map_err(|_| CodecError::Limit)?)?;
        self.bytes(value.as_bytes())
    }

    fn opt_str(&mut self, value: &Option<String>, max: usize) -> Result<(), CodecError> {
        match value {
            None => self.u8(0),
            Some(text) => {
                self.u8(1)?;
                self.str(text, max)
            }
        }
    }

    fn opt_u32(&mut self, value: Option<u32>) -> Result<(), CodecError> {
        match value {
            None => self.u8(0),
            Some(item) => {
                self.u8(1)?;
                self.u32(item)
            }
        }
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn finish(&self) -> Result<(), CodecError> {
        if self.pos == self.buf.len() {
            Ok(())
        } else {
            Err(CodecError::Trailing)
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CodecError> {
        let end = self.pos.checked_add(len).ok_or(CodecError::Limit)?;
        if end > self.buf.len() {
            return Err(CodecError::Truncated);
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, CodecError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, CodecError> {
        let bytes = self.take(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(array))
    }

    fn i32(&mut self) -> Result<i32, CodecError> {
        let bytes = self.take(4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn flag(&mut self) -> Result<bool, CodecError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::Tag),
        }
    }

    fn str(&mut self, max: usize) -> Result<String, CodecError> {
        let len = usize::try_from(self.u32()?).map_err(|_| CodecError::Limit)?;
        if len > max {
            return Err(CodecError::Limit);
        }
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| CodecError::Utf8)
    }

    fn opt_str(&mut self, max: usize) -> Result<Option<String>, CodecError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.str(max)?)),
            _ => Err(CodecError::Tag),
        }
    }

    fn opt_u32(&mut self) -> Result<Option<u32>, CodecError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u32()?)),
            _ => Err(CodecError::Tag),
        }
    }

    fn count(&mut self, max: usize) -> Result<usize, CodecError> {
        let count = usize::try_from(self.u32()?).map_err(|_| CodecError::Limit)?;
        if count > max {
            Err(CodecError::Limit)
        } else {
            Ok(count)
        }
    }
}
