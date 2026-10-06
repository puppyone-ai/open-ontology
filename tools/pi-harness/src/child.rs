//! Stdio child. Tokio bounds the event stream. The process itself is `std::process`.
//! The reaped pid is the child this adapter spawned. That is not a containment claim.

use crate::map::{map_record, MapResult};
use crate::sha256::Hasher;
use crate::{
    AbortOutcome, Error, ErrorKind, EventStream, ProductEvent, Sequenced, StreamItem, Termination,
    LINE_LIMIT,
};
use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tokio::sync::{mpsc as async_mpsc, oneshot, Mutex as AsyncMutex};
use tokio::time::timeout;

const STDERR_CAP: usize = 262_144;
const RAW_CAP: u64 = 8 * 1_048_576;
const POLL: Duration = Duration::from_millis(10);
#[cfg(target_os = "linux")]
const O_NOFOLLOW: i32 = 0x20000;

enum Incoming {
    Line(Vec<u8>),
    Partial(usize),
    Eof,
    TooBig,
    Io(String),
}

struct Gate {
    next_sequence: u64,
    failed: bool,
    saw_turn_finished: bool,
    tools: u32,
}

pub(crate) struct RawReply {
    pub raw: Vec<u8>,
    pub value: Value,
}

pub(crate) struct ChildIo {
    inner: Arc<Inner>,
    reader: Option<JoinHandle<()>>,
    demux: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<()>>,
}

struct Inner {
    pid: u32,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    pending: Mutex<Option<(String, oneshot::Sender<Vec<u8>>)>>,
    events: Mutex<Option<async_mpsc::Receiver<StreamItem>>>,
    events_tx: async_mpsc::Sender<StreamItem>,
    stderr: Arc<Mutex<Vec<u8>>>,
    gate: Mutex<Gate>,
    reaped: Mutex<Option<AbortOutcome>>,
    abort_gate: AsyncMutex<()>,
    deadline_armed: AtomicBool,
    done: AtomicBool,
    shutdown: AtomicBool,
    terminal_sent: AtomicBool,
    line_limit: usize,
    command_deadline: Duration,
    abort_ack_deadline: Duration,
    kill_wait: Duration,
    turn_deadline: Duration,
    tool_limit: u32,
    ids: AtomicU64,
    runtime: Handle,
}

pub(crate) struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub home: PathBuf,
    pub path: PathBuf,
    pub agent_dir: PathBuf,
    pub line_limit: usize,
    pub event_bound: usize,
    pub command_deadline: Duration,
    pub abort_ack_deadline: Duration,
    pub kill_wait: Duration,
    pub turn_deadline: Duration,
    pub tool_limit: u32,
    pub raw_log: Option<PathBuf>,
    pub runtime: Handle,
}

impl ChildIo {
    pub(crate) fn spawn(spec: &SpawnSpec) -> Result<Self, Error> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .env_clear()
            .env("HOME", &spec.home)
            .env("PATH", &spec.path)
            .env("TERM", "dumb")
            .env("PI_CODING_AGENT_DIR", &spec.agent_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|err| Error::new(ErrorKind::Spawn, err.to_string()))?;
        let pid = child.id();
        let stdin = child.stdin.take().ok_or_else(|| Error::new(ErrorKind::Spawn, "stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| Error::new(ErrorKind::Spawn, "stdout"))?;
        let stderr_pipe = child.stderr.take().ok_or_else(|| Error::new(ErrorKind::Spawn, "stderr"))?;
        let (line_tx, line_rx) = mpsc::sync_channel(spec.event_bound);
        let stderr_buf = Arc::new(Mutex::new(Vec::new()));
        let (event_tx, event_rx) = async_mpsc::channel(spec.event_bound);
        let inner = Arc::new(Inner {
            pid,
            stdin: Mutex::new(Some(stdin)),
            child: Mutex::new(Some(child)),
            pending: Mutex::new(None),
            events: Mutex::new(Some(event_rx)),
            events_tx: event_tx.clone(),
            stderr: Arc::clone(&stderr_buf),
            gate: Mutex::new(Gate {
                next_sequence: 0,
                failed: false,
                saw_turn_finished: false,
                tools: 0,
            }),
            reaped: Mutex::new(None),
            abort_gate: AsyncMutex::new(()),
            deadline_armed: AtomicBool::new(false),
            done: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            terminal_sent: AtomicBool::new(false),
            line_limit: spec.line_limit,
            command_deadline: spec.command_deadline,
            abort_ack_deadline: spec.abort_ack_deadline,
            kill_wait: spec.kill_wait,
            turn_deadline: spec.turn_deadline,
            tool_limit: spec.tool_limit,
            ids: AtomicU64::new(1),
            runtime: spec.runtime.clone(),
        });
        let reader_inner = Arc::clone(&inner);
        let limit = spec.line_limit;
        let reader = thread::Builder::new()
            .name("pi-stdout".to_owned())
            .spawn(move || read_stdout(stdout, limit, line_tx, reader_inner))
            .map_err(|err| Error::new(ErrorKind::Spawn, err.to_string()))?;
        let stderr_inner = Arc::clone(&inner);
        let stderr = thread::Builder::new()
            .name("pi-stderr".to_owned())
            .spawn(move || drain_stderr(stderr_pipe, stderr_buf, stderr_inner))
            .map_err(|err| Error::new(ErrorKind::Spawn, err.to_string()))?;
        let demux_inner = Arc::clone(&inner);
        let raw = spec.raw_log.clone();
        let demux = thread::Builder::new()
            .name("pi-demux".to_owned())
            .spawn(move || demux(line_rx, event_tx, demux_inner, raw))
            .map_err(|err| Error::new(ErrorKind::Spawn, err.to_string()))?;
        Ok(Self { inner, reader: Some(reader), demux: Some(demux), stderr: Some(stderr) })
    }

    pub(crate) fn pid(&self) -> u32 {
        self.inner.pid
    }

    pub(crate) fn stderr_tail(&self) -> String {
        self.inner.stderr.lock().map(|buf| String::from_utf8_lossy(&buf).into_owned()).unwrap_or_default()
    }

    pub(crate) fn events(&self) -> Result<EventStream, Error> {
        let rx = lock(&self.inner.events)?
            .take()
            .ok_or_else(|| Error::new(ErrorKind::Protocol, "event stream already taken"))?;
        Ok(EventStream { rx })
    }

    pub(crate) async fn command(&self, kind: &str, fields: &[(&str, Value)]) -> Result<RawReply, Error> {
        command_inner(&self.inner, kind, fields, self.inner.command_deadline).await
    }

    pub(crate) fn arm_turn_deadline(&self) {
        if self.inner.deadline_armed.swap(true, Ordering::Relaxed) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let wait = self.inner.turn_deadline;
        let _task = tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            if inner.done.load(Ordering::Relaxed) {
                return;
            }
            mark_failed(&inner);
            let _ = push_terminal(&inner, Termination::Incomplete { reason: "turn-deadline".to_owned() });
            let _ = abort_and_reap(&inner).await;
        });
    }

    pub(crate) async fn abort_and_reap(&self) -> Result<AbortOutcome, Error> {
        mark_failed(&self.inner);
        let _ = push_terminal(&self.inner, Termination::Incomplete { reason: "aborted".to_owned() });
        abort_and_reap(&self.inner).await
    }
}

impl Drop for ChildIo {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, Ordering::Relaxed);
        drop_stdin(&self.inner);
        if self.inner.reaped.lock().ok().and_then(|slot| slot.clone()).is_none() {
            let _ = kill_now(&self.inner);
        }
        for handle in [self.reader.take(), self.demux.take(), self.stderr.take()].into_iter().flatten() {
            let _ = handle.join();
        }
    }
}

async fn command_inner(inner: &Inner, kind: &str, fields: &[(&str, Value)], deadline: Duration) -> Result<RawReply, Error> {
    if kind == "export" || kind == "restore" {
        return Err(Error::new(ErrorKind::Protocol, "export and restore are not commands"));
    }
    let id = format!("c{}", inner.ids.fetch_add(1, Ordering::Relaxed));
    let bytes = encode_command(&id, kind, fields)?;
    if bytes.len() + 1 > inner.line_limit {
        return Err(Error::new(ErrorKind::TooBig, "request line"));
    }
    let (tx, rx) = oneshot::channel();
    {
        let mut pending = lock(&inner.pending)?;
        if pending.is_some() {
            return Err(Error::new(ErrorKind::Protocol, "a command is already outstanding"));
        }
        *pending = Some((id, tx));
    }
    if let Err(err) = write_line(inner, &bytes) {
        let _ = lock(&inner.pending).map(|mut slot| {
            *slot = None;
        });
        return Err(err);
    }
    let raw = match timeout(deadline, rx).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return Err(Error::new(ErrorKind::Eof, "response waiter closed")),
        Err(_) => {
            let _ = lock(&inner.pending).map(|mut slot| {
                *slot = None;
            });
            return Err(Error::new(ErrorKind::Deadline, "command deadline"));
        }
    };
    let value = parse_response(&raw)?;
    Ok(RawReply { raw, value })
}

async fn abort_and_reap(inner: &Arc<Inner>) -> Result<AbortOutcome, Error> {
    let _guard = inner.abort_gate.lock().await;
    if let Some(done) = lock(&inner.reaped)?.clone() {
        return Ok(done);
    }
    inner.done.store(true, Ordering::Relaxed);
    let ack_ok = command_inner(inner, "abort", &[], inner.abort_ack_deadline).await.is_ok();
    fail_pending(inner);
    drop_stdin(inner);
    let mut outcome = reap(inner, inner.kill_wait)?;
    outcome.ack = ack_ok && outcome.waited;
    *lock(&inner.reaped)? = Some(outcome.clone());
    Ok(outcome)
}

fn mark_failed(inner: &Inner) {
    inner.done.store(true, Ordering::Relaxed);
    if let Ok(mut gate) = inner.gate.lock() {
        gate.failed = true;
    }
}

fn push_terminal(inner: &Inner, termination: Termination) -> Result<(), Error> {
    if inner.terminal_sent.swap(true, Ordering::Relaxed) {
        return Ok(());
    }
    inner.events_tx.try_send(StreamItem::Terminated(termination)).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    Ok(())
}

fn fail_pending(inner: &Inner) {
    if let Ok(mut slot) = inner.pending.lock() {
        *slot = None;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, Error> {
    mutex.lock().map_err(|err| Error::new(ErrorKind::Io, err.to_string()))
}

fn write_line(inner: &Inner, bytes: &[u8]) -> Result<(), Error> {
    let mut stdin = lock(&inner.stdin)?;
    let slot = stdin.as_mut().ok_or_else(|| Error::new(ErrorKind::Io, "stdin closed"))?;
    slot.write_all(bytes).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    slot.write_all(b"\n").map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    slot.flush().map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    Ok(())
}

fn drop_stdin(inner: &Inner) {
    if let Ok(mut stdin) = inner.stdin.lock() {
        let _ = stdin.take();
    }
}

fn encode_command(id: &str, kind: &str, fields: &[(&str, Value)]) -> Result<Vec<u8>, Error> {
    if id.is_empty() || kind.is_empty() {
        return Err(Error::new(ErrorKind::Protocol, "empty command id or type"));
    }
    let mut map = Map::new();
    map.insert("id".to_owned(), Value::String(id.to_owned()));
    map.insert("type".to_owned(), Value::String(kind.to_owned()));
    for (name, value) in fields {
        if name.is_empty() {
            return Err(Error::new(ErrorKind::Protocol, "empty field"));
        }
        map.insert((*name).to_owned(), value.clone());
    }
    serde_json::to_vec(&Value::Object(map)).map_err(|err| Error::new(ErrorKind::Shape, err.to_string()))
}

fn parse_response(bytes: &[u8]) -> Result<Value, Error> {
    let value: Value = serde_json::from_slice(bytes).map_err(|err| Error::new(ErrorKind::Shape, err.to_string()))?;
    if value.get("type").and_then(Value::as_str) != Some("response") {
        return Err(Error::new(ErrorKind::Protocol, "not a response"));
    }
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        let detail = value.get("error").and_then(Value::as_str).unwrap_or("command failed");
        return Err(Error::new(ErrorKind::Protocol, detail));
    }
    Ok(value)
}

fn reap(inner: &Inner, kill_wait: Duration) -> Result<AbortOutcome, Error> {
    let started = Instant::now();
    loop {
        let mut slot = lock(&inner.child)?;
        let Some(child) = slot.as_mut() else {
            return Ok(AbortOutcome {
                ack: false,
                child_pid: inner.pid,
                exit_code: None,
                killed: false,
                waited: false,
            });
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(AbortOutcome {
                    ack: false,
                    child_pid: inner.pid,
                    exit_code: status.code(),
                    killed: false,
                    waited: true,
                });
            }
            Ok(None) if started.elapsed() >= kill_wait => {
                let killed = child.kill().is_ok();
                let status = child.wait().map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
                return Ok(AbortOutcome {
                    ack: false,
                    child_pid: inner.pid,
                    exit_code: status.code(),
                    killed,
                    waited: true,
                });
            }
            Ok(None) => {
                drop(slot);
                thread::sleep(POLL);
            }
            Err(err) => return Err(Error::new(ErrorKind::Io, err.to_string())),
        }
    }
}

fn kill_now(inner: &Inner) -> Result<(), Error> {
    let outcome = reap(inner, Duration::ZERO)?;
    if let Ok(mut slot) = inner.reaped.lock()
        && slot.is_none()
    {
        *slot = Some(outcome);
    }
    Ok(())
}

fn demux(rx: Receiver<Incoming>, events: async_mpsc::Sender<StreamItem>, inner: Arc<Inner>, raw: Option<PathBuf>) {
    let mut raw_file = raw.and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok());
    let mut raw_bytes = 0_u64;
    loop {
        if inner.shutdown.load(Ordering::Relaxed) {
            break;
        }
        let incoming = match rx.recv_timeout(POLL) {
            Ok(incoming) => incoming,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = send_item(&events, &inner, StreamItem::Terminated(Termination::Incomplete { reason: "reader ended".to_owned() }));
                break;
            }
        };
        match incoming {
            Incoming::Line(line) => {
                if let Some(file) = raw_file.as_mut()
                    && raw_bytes < RAW_CAP
                {
                    let _ = file.write_all(&line);
                    let _ = file.write_all(b"\n");
                    raw_bytes = raw_bytes.saturating_add(line.len() as u64 + 1);
                }
                if deliver_response(&inner, &line) {
                    continue;
                }
                let items = items_for_line(&inner, &line);
                let mut tool_abort = false;
                let mut stop = false;
                for item in items {
                    if matches!(&item, StreamItem::Terminated(Termination::Incomplete { reason }) if reason == "tool-limit") {
                        tool_abort = true;
                    }
                    if matches!(item, StreamItem::Terminated(_)) {
                        stop = true;
                    }
                    if !send_item(&events, &inner, item) {
                        return;
                    }
                }
                if tool_abort {
                    let abort_inner = Arc::clone(&inner);
                    let _task = inner.runtime.spawn(async move {
                        let _ = abort_and_reap(&abort_inner).await;
                    });
                }
                if stop && !tool_abort {
                    return;
                }
            }
            Incoming::Partial(len) => {
                let _ = send_item(
                    &events,
                    &inner,
                    StreamItem::Terminated(Termination::Incomplete { reason: format!("partial {len} bytes") }),
                );
                return;
            }
            Incoming::Eof => {
                let finished = inner.gate.lock().ok().is_some_and(|gate| gate.saw_turn_finished && !gate.failed);
                let reason = if finished {
                    Termination::Finished
                } else {
                    Termination::Incomplete { reason: "stdout-eof".to_owned() }
                };
                let _ = send_item(&events, &inner, StreamItem::Terminated(reason));
                return;
            }
            Incoming::TooBig => {
                let _ = send_item(
                    &events,
                    &inner,
                    StreamItem::Terminated(Termination::Incomplete { reason: "line limit".to_owned() }),
                );
                return;
            }
            Incoming::Io(detail) => {
                let _ = send_item(&events, &inner, StreamItem::Terminated(Termination::Incomplete { reason: detail }));
                return;
            }
        }
    }
}

fn send_item(events: &async_mpsc::Sender<StreamItem>, inner: &Inner, mut item: StreamItem) -> bool {
    if matches!(item, StreamItem::Terminated(_)) && inner.terminal_sent.swap(true, Ordering::Relaxed) {
        return true;
    }
    loop {
        if inner.shutdown.load(Ordering::Relaxed) {
            return false;
        }
        match events.try_send(item) {
            Ok(()) => return true,
            Err(async_mpsc::error::TrySendError::Full(value)) => {
                item = value;
                thread::sleep(POLL);
            }
            Err(async_mpsc::error::TrySendError::Closed(_)) => return false,
        }
    }
}

fn deliver_response(inner: &Inner, line: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return false;
    };
    if value.get("type").and_then(Value::as_str) != Some("response") {
        return false;
    }
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return true;
    };
    let Ok(mut pending) = inner.pending.lock() else {
        return true;
    };
    if pending.as_ref().is_some_and(|(waiting, _)| waiting == id)
        && let Some((_, tx)) = pending.take()
    {
        let _ = tx.send(line.to_vec());
    }
    true
}

fn items_for_line(inner: &Inner, line: &[u8]) -> Vec<StreamItem> {
    let value: Value = match serde_json::from_slice(line) {
        Ok(value) => value,
        Err(err) => return fail(inner, format!("json {err}")),
    };
    let mapped = match map_record(&value) {
        Ok(mapped) => mapped,
        Err(err) => return fail(inner, err.detail),
    };
    let Ok(mut gate) = inner.gate.lock() else {
        return Vec::new();
    };
    if gate.failed {
        return Vec::new();
    }
    match mapped {
        MapResult::Ignored => Vec::new(),
        MapResult::Unknown(kind) => {
            gate.failed = true;
            inner.done.store(true, Ordering::Relaxed);
            let sequence = take_sequence(&mut gate);
            vec![
                StreamItem::Event(Sequenced {
                    sequence,
                    event: ProductEvent::Error { reason: format!("unsupported-event:{kind}") },
                }),
                StreamItem::Terminated(Termination::Incomplete { reason: format!("unsupported-event:{kind}") }),
            ]
        }
        MapResult::Event(ProductEvent::TurnFinished { stop_reason, answer, usage }) => {
            gate.saw_turn_finished = true;
            inner.done.store(true, Ordering::Relaxed);
            let sequence = take_sequence(&mut gate);
            vec![StreamItem::Event(Sequenced {
                sequence,
                event: ProductEvent::TurnFinished { stop_reason, answer, usage },
            })]
        }
        MapResult::Event(event @ ProductEvent::Error { .. }) => {
            gate.failed = true;
            inner.done.store(true, Ordering::Relaxed);
            let sequence = take_sequence(&mut gate);
            vec![
                StreamItem::Event(Sequenced { sequence, event }),
                StreamItem::Terminated(Termination::Incomplete { reason: "error".to_owned() }),
            ]
        }
        MapResult::Event(event @ ProductEvent::ToolCallStarted { .. }) => {
            gate.tools = gate.tools.saturating_add(1);
            if gate.tools > inner.tool_limit {
                gate.failed = true;
                inner.done.store(true, Ordering::Relaxed);
                let sequence = take_sequence(&mut gate);
                return vec![
                    StreamItem::Event(Sequenced {
                        sequence,
                        event: ProductEvent::Error { reason: "tool-limit".to_owned() },
                    }),
                    StreamItem::Terminated(Termination::Incomplete { reason: "tool-limit".to_owned() }),
                ];
            }
            let sequence = take_sequence(&mut gate);
            vec![StreamItem::Event(Sequenced { sequence, event })]
        }
        MapResult::Event(event) => {
            let sequence = take_sequence(&mut gate);
            vec![StreamItem::Event(Sequenced { sequence, event })]
        }
    }
}

fn fail(inner: &Inner, reason: String) -> Vec<StreamItem> {
    let sequence = if let Ok(mut gate) = inner.gate.lock() {
        gate.failed = true;
        take_sequence(&mut gate)
    } else {
        0
    };
    inner.done.store(true, Ordering::Relaxed);
    vec![
        StreamItem::Event(Sequenced {
            sequence,
            event: ProductEvent::Error { reason: reason.clone() },
        }),
        StreamItem::Terminated(Termination::Incomplete { reason }),
    ]
}

fn take_sequence(gate: &mut Gate) -> u64 {
    let sequence = gate.next_sequence;
    gate.next_sequence = gate.next_sequence.saturating_add(1);
    sequence
}

fn read_stdout(mut stdout: impl Read + Send + 'static, limit: usize, tx: SyncSender<Incoming>, inner: Arc<Inner>) {
    let mut pending = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        if inner.shutdown.load(Ordering::Relaxed) {
            break;
        }
        match stdout.read(&mut chunk) {
            Ok(0) => {
                if !pending.is_empty() {
                    let _ = send_incoming(&tx, &inner, Incoming::Partial(pending.len()));
                }
                let _ = send_incoming(&tx, &inner, Incoming::Eof);
                break;
            }
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                if !drain_lines(&mut pending, limit, &tx, &inner) {
                    break;
                }
            }
            Err(err) => {
                let _ = send_incoming(&tx, &inner, Incoming::Io(err.to_string()));
                break;
            }
        }
    }
}

fn send_incoming(tx: &SyncSender<Incoming>, inner: &Inner, mut item: Incoming) -> bool {
    loop {
        if inner.shutdown.load(Ordering::Relaxed) {
            return false;
        }
        match tx.try_send(item) {
            Ok(()) => return true,
            Err(TrySendError::Full(value)) => {
                item = value;
                thread::sleep(POLL);
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

fn drain_lines(pending: &mut Vec<u8>, limit: usize, tx: &SyncSender<Incoming>, inner: &Inner) -> bool {
    let cap = if limit == 0 { LINE_LIMIT } else { limit };
    while let Some(pos) = pending.iter().position(|byte| *byte == b'\n') {
        if pos + 1 > cap {
            let _ = send_incoming(tx, inner, Incoming::TooBig);
            return false;
        }
        let mut line: Vec<u8> = pending.drain(..=pos).collect();
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if !send_incoming(tx, inner, Incoming::Line(line)) {
            return false;
        }
    }
    if pending.len() > cap {
        let _ = send_incoming(tx, inner, Incoming::TooBig);
        return false;
    }
    true
}

fn drain_stderr(mut stderr: impl Read, slot: Arc<Mutex<Vec<u8>>>, inner: Arc<Inner>) {
    let mut buf = [0_u8; 4096];
    loop {
        if inner.shutdown.load(Ordering::Relaxed) {
            break;
        }
        match stderr.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Ok(mut guard) = slot.lock()
                    && guard.len() < STDERR_CAP
                {
                    let room = STDERR_CAP - guard.len();
                    guard.extend_from_slice(&buf[..n.min(room)]);
                }
            }
        }
    }
}

pub fn hash_file(path: &Path) -> Result<[u8; 32], Error> {
    let mut file = File::open(path).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    let mut hasher = Hasher::new();
    let mut buf = [0_u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finish())
}

pub(crate) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, Error> {
    let meta = fs::symlink_metadata(path).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(Error::new(ErrorKind::Protocol, "session file must be a regular file"));
    }
    if meta.len() > limit {
        return Err(Error::new(ErrorKind::Bound, "session file byte limit"));
    }
    let mut file = open_nofollow(path)?;
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let n = file.read(&mut chunk).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
        if n == 0 {
            break;
        }
        if buf.len() as u64 + n as u64 > limit {
            return Err(Error::new(ErrorKind::Bound, "session file byte limit"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
}

fn open_nofollow(path: &Path) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(O_NOFOLLOW);
    }
    options.open(path).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))
}
