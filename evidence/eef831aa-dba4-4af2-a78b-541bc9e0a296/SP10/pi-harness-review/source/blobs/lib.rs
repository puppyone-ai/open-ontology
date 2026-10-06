//! Async adapter over a pinned Pi RPC process.
//!
//! Development launch is a local process. It is not a sandbox certificate.
//! This crate does not accept the parent harness, H-D02, or a release.
//! Tool product events stay unbound until a fresh Pi 1.0.4 capture fills CONTRACT.md.

#![forbid(unsafe_code)]

mod child;
mod map;
mod sha256;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use tokio::runtime::Handle;

pub use child::hash_file;
pub use sha256::{hex, sha256};

pub const HARNESS_NAME: &str = "pi";
pub const PINNED_PI_VERSION: &str = "1.0.4";
pub const PUBLIC_PROXY_MARKER: &str = "public-proxy-marker";
pub const DEVELOPMENT_ACK: &str = "local-process-not-a-sandbox-certificate";
pub const LINE_LIMIT: usize = 1_048_576;
pub const EVENT_BOUND: usize = 16;
pub const TOOL_LIMIT: u32 = 60;
pub const RAW_LOG_CAP: u64 = 8 * 1_048_576;

const VERSION_POLL: Duration = Duration::from_millis(20);

/// Development fixture flag. Constructed only by [`LocalProcessNotSandbox::acknowledge`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalProcessNotSandbox {
    statement: String,
}

impl LocalProcessNotSandbox {
    pub fn acknowledge() -> Self {
        Self { statement: DEVELOPMENT_ACK.to_owned() }
    }

    pub fn statement(&self) -> &str {
        &self.statement
    }
}

/// Production runsc input. Neither variant launches a process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunscApproval {
    Absent,
    Presented { runsc_sha256: [u8; 32] },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeMode {
    DevelopmentLocalProcess(LocalProcessNotSandbox),
    ProductionRunsc(RunscApproval),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelConfig {
    pub name: String,
    pub context_window: u32,
    pub max_tokens: u32,
}

#[derive(Clone, Debug)]
pub struct Bounds {
    pub line_limit: usize,
    pub event_bound: usize,
    pub command_deadline: Duration,
    pub abort_ack_deadline: Duration,
    pub kill_wait: Duration,
    pub turn_deadline: Duration,
    pub tool_limit: u32,
    pub session_byte_limit: u64,
}

#[derive(Clone, Debug)]
pub struct LaunchRequest {
    pub executable: PathBuf,
    pub executable_sha256: [u8; 32],
    pub runtime: PathBuf,
    pub runtime_sha256: [u8; 32],
    pub pack: PathBuf,
    pub workspace: PathBuf,
    pub home: PathBuf,
    pub agent_dir: PathBuf,
    pub session_dir: PathBuf,
    pub origin: String,
    pub provider: String,
    pub model: String,
    pub model_config: ModelConfig,
    pub bounds: Bounds,
    pub runtime_mode: RuntimeMode,
    pub raw_event_log: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserMessage {
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptAcceptance {
    pub id: String,
    pub disposition: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProductEvent {
    TurnStarted,
    AssistantDelta { text: String },
    ToolCallStarted { name: String, call_id: String, args_summary: String },
    ToolCallFinished { call_id: String, status: String, output_summary: String },
    Error { reason: String },
    TurnFinished { stop_reason: String, answer: Option<String>, usage: Option<Usage> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequenced {
    pub sequence: u64,
    pub event: ProductEvent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Termination {
    /// Stdout ended after a real `turn_finished` record. EOF itself is not that record.
    Finished,
    Incomplete { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamItem {
    Event(Sequenced),
    Terminated(Termination),
}

pub struct EventStream {
    rx: tokio::sync::mpsc::Receiver<StreamItem>,
}

impl EventStream {
    pub async fn next(&mut self) -> Option<StreamItem> {
        self.rx.recv().await
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbortOutcome {
    pub ack: bool,
    pub child_pid: u32,
    pub exit_code: Option<i32>,
    pub killed: bool,
    pub waited: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptVersion {
    pub pi_version: String,
    pub session_path: PathBuf,
    pub messages_sha256: [u8; 32],
    pub session_sha256: [u8; 32],
    pub session_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transcript {
    pub version: TranscriptVersion,
    pub messages: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Spawn,
    Io,
    Shape,
    Protocol,
    TooBig,
    Eof,
    Deadline,
    Bound,
    Unsupported,
    Hash,
    Path,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub detail: String,
    pub reap: Option<AbortOutcome>,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into(), reap: None }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for Error {}

pub struct Harness {
    child: child::ChildIo,
    request: LaunchRequest,
}

impl Harness {
    pub fn name(&self) -> &'static str {
        HARNESS_NAME
    }

    pub fn child_pid(&self) -> u32 {
        self.child.pid()
    }

    pub fn stderr_tail(&self) -> String {
        self.child.stderr_tail()
    }

    pub async fn launch(request: LaunchRequest) -> Result<Self, Error> {
        launch_with(request, false).await
    }

    pub async fn send(&self, message: UserMessage) -> Result<PromptAcceptance, Error> {
        if message.text.is_empty() {
            return Err(Error::new(ErrorKind::Protocol, "empty prompt"));
        }
        let reply = self.child.command("prompt", &[("message", Value::String(message.text))]).await?;
        let disposition = reply
            .value
            .get("data")
            .and_then(|data| data.get("disposition"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new(ErrorKind::Protocol, "prompt disposition missing"))?
            .to_owned();
        let id = reply
            .value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if disposition == "started" {
            self.child.arm_turn_deadline();
        }
        Ok(PromptAcceptance { id, disposition })
    }

    pub fn events(&self) -> Result<EventStream, Error> {
        self.child.events()
    }

    pub async fn abort(&self) -> Result<AbortOutcome, Error> {
        self.child.abort_and_reap().await
    }

    pub async fn export_transcript(&self) -> Result<Transcript, Error> {
        let reply = self.child.command("get_messages", &[]).await?;
        let session_path = wait_for_session(&self.request.session_dir, self.request.bounds.command_deadline).await?;
        let bytes = child::read_bounded(&session_path, self.request.bounds.session_byte_limit)?;
        let session_sha256 = sha256(&bytes);
        Ok(Transcript {
            version: TranscriptVersion {
                pi_version: PINNED_PI_VERSION.to_owned(),
                session_path,
                messages_sha256: sha256(&reply.raw),
                session_sha256,
                session_bytes: bytes.len() as u64,
            },
            messages: reply.raw,
        })
    }

    /// Fresh process. `switch_session` then `get_messages`. No `restore` command is sent.
    pub async fn restore(
        request: LaunchRequest,
        session_path: &Path,
        session_sha256: [u8; 32],
    ) -> Result<(Self, Vec<u8>), Error> {
        if !session_path.starts_with(&request.session_dir) {
            return Err(Error::new(ErrorKind::Path, "session path is outside the session directory"));
        }
        let bytes = child::read_bounded(session_path, request.bounds.session_byte_limit)?;
        if sha256(&bytes) != session_sha256 {
            return Err(Error::new(ErrorKind::Hash, "session hash mismatch"));
        }
        let harness = launch_with(request, true).await?;
        let path = utf8_path(session_path)?;
        let switched = harness.child.command("switch_session", &[("sessionPath", Value::String(path))]).await?;
        let cancelled = switched.value.get("data").and_then(|data| data.get("cancelled")).and_then(Value::as_bool);
        if cancelled != Some(false) {
            return Err(Error::new(ErrorKind::Protocol, "switch_session did not load the session"));
        }
        let messages = harness.child.command("get_messages", &[]).await?;
        Ok((harness, messages.raw))
    }
}

async fn launch_with(request: LaunchRequest, restore: bool) -> Result<Harness, Error> {
    validate(&request)?;
    match &request.runtime_mode {
        RuntimeMode::DevelopmentLocalProcess(flag) => {
            if flag.statement() != DEVELOPMENT_ACK {
                return Err(Error::new(ErrorKind::Protocol, "development acknowledgement mismatch"));
            }
        }
        RuntimeMode::ProductionRunsc(RunscApproval::Absent) => {
            return Err(Error::new(ErrorKind::Unsupported, "runsc approval is absent"));
        }
        RuntimeMode::ProductionRunsc(RunscApproval::Presented { .. }) => {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "presented runsc digest is not a launch certificate",
            ));
        }
    }
    let runtime = Handle::try_current().map_err(|_| Error::new(ErrorKind::Spawn, "async runtime required"))?;
    verify_digest(&request.executable, request.executable_sha256, "executable")?;
    verify_digest(&request.runtime, request.runtime_sha256, "runtime")?;
    fs::create_dir_all(&request.home).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    fs::create_dir_all(&request.agent_dir).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    fs::create_dir_all(&request.session_dir).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    reject_symlink_chain(&request.home)?;
    reject_symlink_chain(&request.agent_dir)?;
    reject_symlink_chain(&request.session_dir)?;
    write_models_json(&request)?;
    check_version(&request)?;
    let path = request
        .runtime
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::Path, "runtime has no directory"))?
        .to_path_buf();
    let args = rpc_args(&request, restore)?;
    let spec = child::SpawnSpec {
        program: request.executable.clone(),
        args,
        cwd: request.workspace.clone(),
        home: request.home.clone(),
        path,
        agent_dir: request.agent_dir.clone(),
        line_limit: request.bounds.line_limit,
        event_bound: request.bounds.event_bound,
        command_deadline: request.bounds.command_deadline,
        abort_ack_deadline: request.bounds.abort_ack_deadline,
        kill_wait: request.bounds.kill_wait,
        turn_deadline: request.bounds.turn_deadline,
        tool_limit: request.bounds.tool_limit,
        raw_log: request.raw_event_log.clone(),
        runtime,
    };
    let child = child::ChildIo::spawn(&spec)?;
    Ok(Harness { child, request })
}

pub fn write_models_json(request: &LaunchRequest) -> Result<(), Error> {
    let provider = json_key(&request.provider, "provider")?;
    let model = json_key(&request.model, "model")?;
    let name = json_string(&request.model_config.name, "model name")?;
    let origin = json_string(&request.origin, "origin")?;
    if request.model_config.context_window == 0 || request.model_config.max_tokens == 0 {
        return Err(Error::new(ErrorKind::Protocol, "model window is empty"));
    }
    let mut model_entry = Map::new();
    model_entry.insert("id".to_owned(), Value::String(model));
    model_entry.insert("name".to_owned(), Value::String(name));
    model_entry.insert("reasoning".to_owned(), Value::Bool(false));
    model_entry.insert("input".to_owned(), Value::Array(vec![Value::String("text".to_owned())]));
    model_entry.insert("contextWindow".to_owned(), Value::from(request.model_config.context_window));
    model_entry.insert("maxTokens".to_owned(), Value::from(request.model_config.max_tokens));
    let mut cost = Map::new();
    for key in ["input", "output", "cacheRead", "cacheWrite"] {
        cost.insert(key.to_owned(), Value::from(0));
    }
    model_entry.insert("cost".to_owned(), Value::Object(cost));
    let mut provider_entry = Map::new();
    provider_entry.insert("baseUrl".to_owned(), Value::String(origin));
    provider_entry.insert("api".to_owned(), Value::String("openai-completions".to_owned()));
    provider_entry.insert("apiKey".to_owned(), Value::String(PUBLIC_PROXY_MARKER.to_owned()));
    provider_entry.insert("models".to_owned(), Value::Array(vec![Value::Object(model_entry)]));
    let mut providers = Map::new();
    providers.insert(provider, Value::Object(provider_entry));
    let mut root = Map::new();
    root.insert("providers".to_owned(), Value::Object(providers));
    let bytes = serde_json::to_vec_pretty(&Value::Object(root)).map_err(|err| Error::new(ErrorKind::Shape, err.to_string()))?;
    let path = request.agent_dir.join("models.json");
    if path.starts_with(&request.pack) {
        return Err(Error::new(ErrorKind::Path, "models.json would be written under the pack"));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    file.write_all(&bytes).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    file.write_all(b"\n").map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    Ok(())
}

fn validate(request: &LaunchRequest) -> Result<(), Error> {
    if request.bounds.line_limit == 0 || request.bounds.line_limit > LINE_LIMIT {
        return Err(Error::new(ErrorKind::Bound, "line limit"));
    }
    if request.bounds.event_bound != EVENT_BOUND {
        return Err(Error::new(ErrorKind::Bound, "event channel is 16"));
    }
    if request.bounds.tool_limit != TOOL_LIMIT {
        return Err(Error::new(ErrorKind::Bound, "tool limit is 60"));
    }
    if request.bounds.session_byte_limit == 0 || request.bounds.session_byte_limit > RAW_LOG_CAP {
        return Err(Error::new(ErrorKind::Bound, "session byte limit"));
    }
    for (label, value) in [
        ("command deadline", request.bounds.command_deadline),
        ("abort ack deadline", request.bounds.abort_ack_deadline),
        ("kill wait", request.bounds.kill_wait),
        ("turn deadline", request.bounds.turn_deadline),
    ] {
        if value.is_zero() {
            return Err(Error::new(ErrorKind::Bound, label));
        }
    }
    for (label, path) in [
        ("executable", &request.executable),
        ("runtime", &request.runtime),
        ("pack", &request.pack),
        ("workspace", &request.workspace),
        ("home", &request.home),
        ("agent", &request.agent_dir),
        ("session", &request.session_dir),
    ] {
        validate_path(path, label)?;
    }
    if let Some(path) = &request.raw_event_log {
        validate_path(path, "raw event log")?;
        if path.starts_with(&request.pack) {
            return Err(Error::new(ErrorKind::Path, "raw event log is inside the pack"));
        }
    }
    for (label, path) in [
        ("workspace", &request.workspace),
        ("home", &request.home),
        ("agent", &request.agent_dir),
        ("session", &request.session_dir),
    ] {
        if path.starts_with(&request.pack) || request.pack.starts_with(path) {
            return Err(Error::new(ErrorKind::Path, format!("{label} overlaps the pack")));
        }
    }
    if !request.pack.is_dir() {
        return Err(Error::new(ErrorKind::Path, "pack is not a directory"));
    }
    if !request.workspace.is_dir() {
        return Err(Error::new(ErrorKind::Path, "workspace is not a directory"));
    }
    validate_origin(&request.origin)?;
    validate_token(&request.provider, "provider")?;
    validate_token(&request.model, "model")?;
    if request.model_config.name.is_empty()
        || request.model_config.name.contains(PUBLIC_PROXY_MARKER)
        || request.model_config.name.chars().any(|ch| ch == '"' || ch == '\\' || ch.is_control())
    {
        return Err(Error::new(ErrorKind::Protocol, "model name is empty or unsafe"));
    }
    Ok(())
}

fn validate_path(path: &Path, label: &str) -> Result<(), Error> {
    if !path.is_absolute() {
        return Err(Error::new(ErrorKind::Path, format!("{label} is not absolute")));
    }
    if path.components().any(|component| matches!(component, Component::ParentDir)) {
        return Err(Error::new(ErrorKind::Path, format!("{label} contains ..")));
    }
    utf8_path(path)?;
    reject_symlink_chain(path)?;
    if path_text(path).contains(PUBLIC_PROXY_MARKER) {
        return Err(Error::new(ErrorKind::Path, format!("{label} contains the proxy marker")));
    }
    Ok(())
}

fn reject_symlink_chain(path: &Path) -> Result<(), Error> {
    let mut cursor = PathBuf::new();
    for component in path.components() {
        cursor.push(component);
        match fs::symlink_metadata(&cursor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(Error::new(ErrorKind::Path, format!("symlink refused {}", cursor.display())));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(Error::new(ErrorKind::Io, err.to_string())),
        }
    }
    Ok(())
}

fn validate_origin(origin: &str) -> Result<(), Error> {
    if origin.contains(PUBLIC_PROXY_MARKER)
        || origin.contains('@')
        || origin.contains('?')
        || origin.contains('#')
        || origin.contains('\\')
        || origin.chars().any(|ch| ch.is_whitespace())
    {
        return Err(Error::new(ErrorKind::Protocol, "origin is not an approved absolute origin"));
    }
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .ok_or_else(|| Error::new(ErrorKind::Protocol, "origin scheme"))?;
    if rest.is_empty() || rest.starts_with('/') {
        return Err(Error::new(ErrorKind::Protocol, "origin host"));
    }
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return Err(Error::new(ErrorKind::Protocol, "origin host"));
    }
    Ok(())
}

fn validate_token(value: &str, label: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.starts_with('-')
        || value.contains(PUBLIC_PROXY_MARKER)
        || value.chars().any(|ch| ch.is_whitespace())
    {
        return Err(Error::new(ErrorKind::Protocol, format!("{label} is empty or unsafe")));
    }
    Ok(())
}

fn verify_digest(path: &Path, expected: [u8; 32], label: &str) -> Result<(), Error> {
    let actual = hash_file(path)?;
    if actual != expected {
        return Err(Error::new(ErrorKind::Hash, format!("{label} sha256 mismatch")));
    }
    Ok(())
}

fn check_version(request: &LaunchRequest) -> Result<(), Error> {
    let path = request
        .runtime
        .parent()
        .ok_or_else(|| Error::new(ErrorKind::Path, "runtime has no directory"))?;
    let mut child = std::process::Command::new(&request.executable)
        .arg("--version")
        .current_dir(&request.workspace)
        .env_clear()
        .env("HOME", &request.home)
        .env("PATH", path)
        .env("TERM", "dumb")
        .env("PI_CODING_AGENT_DIR", &request.agent_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| Error::new(ErrorKind::Spawn, err.to_string()))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|err| Error::new(ErrorKind::Io, err.to_string()))? {
            Some(status) => break status,
            None if started.elapsed() >= request.bounds.command_deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::new(ErrorKind::Deadline, "version check"));
            }
            None => thread::sleep(VERSION_POLL),
        }
    };
    let mut stdout = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        use std::io::Read;
        pipe.read_to_string(&mut stdout).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
    }
    if !status.success() || stdout.trim() != PINNED_PI_VERSION {
        return Err(Error::new(
            ErrorKind::Protocol,
            format!("pi version stdout is {stdout:?}, expected {PINNED_PI_VERSION}"),
        ));
    }
    Ok(())
}

fn rpc_args(request: &LaunchRequest, restore: bool) -> Result<Vec<String>, Error> {
    let mut args = vec!["--mode".to_owned(), "rpc".to_owned()];
    if restore {
        args.push("--no-session".to_owned());
    } else {
        args.push("--session-dir".to_owned());
        args.push(utf8_path(&request.session_dir)?);
    }
    args.push("--provider".to_owned());
    args.push(request.provider.clone());
    args.push("--model".to_owned());
    args.push(request.model.clone());
    args.extend([
        "--no-extensions".to_owned(),
        "--no-mcp".to_owned(),
        "--no-skills".to_owned(),
        "--no-prompt-templates".to_owned(),
        "--no-context-files".to_owned(),
    ]);
    if args.iter().any(|arg| arg.contains(PUBLIC_PROXY_MARKER)) {
        return Err(Error::new(ErrorKind::Protocol, "proxy marker in argv"));
    }
    Ok(args)
}

async fn wait_for_session(dir: &Path, deadline: Duration) -> Result<PathBuf, Error> {
    let started = Instant::now();
    loop {
        let found = collect_jsonl(dir)?;
        if found.len() > 1 {
            return Err(Error::new(ErrorKind::Protocol, "more than one session jsonl"));
        }
        if let Some(path) = found.into_iter().next() {
            return Ok(path);
        }
        if started.elapsed() >= deadline {
            return Err(Error::new(ErrorKind::Deadline, "session jsonl did not appear"));
        }
        tokio::time::sleep(VERSION_POLL).await;
    }
}

fn collect_jsonl(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    let mut seen = 0_u32;
    while let Some(current) = pending.pop() {
        seen = seen.saturating_add(1);
        if seen > 10_000 {
            return Err(Error::new(ErrorKind::Bound, "session walk"));
        }
        let meta = fs::symlink_metadata(&current).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
        if meta.file_type().is_symlink() {
            return Err(Error::new(ErrorKind::Protocol, "symlink refused in session directory"));
        }
        if meta.is_file() {
            if current.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                found.push(current);
            }
            continue;
        }
        if !meta.is_dir() {
            continue;
        }
        let entries = fs::read_dir(&current).map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|err| Error::new(ErrorKind::Io, err.to_string()))?;
            pending.push(entry.path());
        }
    }
    found.sort();
    Ok(found)
}

fn utf8_path(path: &Path) -> Result<String, Error> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::new(ErrorKind::Path, "path is not utf-8"))
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn json_key(value: &str, label: &str) -> Result<String, Error> {
    validate_token(value, label)?;
    Ok(value.to_owned())
}

fn json_string(value: &str, label: &str) -> Result<String, Error> {
    if value.is_empty() || value.contains(PUBLIC_PROXY_MARKER) || value.chars().any(|ch| ch == '"' || ch == '\\') {
        return Err(Error::new(ErrorKind::Protocol, format!("{label} is empty or unsafe")));
    }
    Ok(value.to_owned())
}
