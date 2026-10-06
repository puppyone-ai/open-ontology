//! One in-flight command is waited by id. Pipelined writes are still paired by id.

use crate::json::{parse_value, Value};
use crate::{Error, Kind, DEFAULT_LINE_LIMIT};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command as StdCommand, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Spawn {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub clear_env: bool,
    pub line_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub id: String,
    pub kind: String,
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub code: Option<i32>,
    pub killed: bool,
}

enum Incoming {
    Line(Vec<u8>),
    Partial(Vec<u8>),
    Eof,
    TooBig,
    Err(String),
}

pub struct Process {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Incoming>,
    reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    queue: Vec<Vec<u8>>,
}

pub fn encode_command(command: &Command) -> Result<Vec<u8>, Error> {
    if command.id.is_empty() || command.kind.is_empty() {
        return Err(Error::new(Kind::Protocol, "empty command id or type"));
    }
    let mut out = Vec::new();
    out.extend_from_slice(br#"{"id":"#);
    push_string(&mut out, &command.id);
    out.extend_from_slice(br#","type":"#);
    push_string(&mut out, &command.kind);
    for (name, value) in &command.extra {
        if name.is_empty() {
            return Err(Error::new(Kind::Protocol, "empty field"));
        }
        out.push(b',');
        push_string(&mut out, name);
        out.push(b':');
        push_string(&mut out, value);
    }
    out.push(b'}');
    Ok(out)
}

impl Process {
    pub fn spawn(spec: &Spawn) -> Result<Self, Error> {
        let mut cmd = StdCommand::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if spec.clear_env {
            cmd.env_clear();
        }
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().map_err(|err| Error::new(Kind::Spawn, err.to_string()))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or_else(|| Error::new(Kind::Spawn, "stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| Error::new(Kind::Spawn, "stderr"))?;
        let limit = if spec.line_limit == 0 {
            DEFAULT_LINE_LIMIT
        } else {
            spec.line_limit
        };
        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || read_stdout(stdout, limit, tx));
        let stderr_buf = Arc::new(Mutex::new(Vec::new()));
        let stderr_slot = Arc::clone(&stderr_buf);
        let stderr_reader = thread::spawn(move || drain_stderr(stderr, stderr_slot));
        Ok(Self {
            child,
            stdin,
            rx,
            reader: Some(reader),
            stderr_reader: Some(stderr_reader),
            stderr: stderr_buf,
            queue: Vec::new(),
        })
    }

    pub fn stderr(&self) -> Vec<u8> {
        self.stderr.lock().map(|guard| guard.clone()).unwrap_or_default()
    }

    pub fn send(&mut self, command: &Command) -> Result<(), Error> {
        let mut line = encode_command(command)?;
        line.push(b'\n');
        let stdin = self.stdin.as_mut().ok_or_else(|| Error::new(Kind::Io, "stdin closed"))?;
        stdin
            .write_all(&line)
            .map_err(|err| Error::new(Kind::Io, err.to_string()))?;
        stdin.flush().map_err(|err| Error::new(Kind::Io, err.to_string()))?;
        Ok(())
    }

    pub fn wait_response(&mut self, id: &str, deadline: Duration) -> Result<Vec<u8>, Error> {
        let end = Instant::now() + deadline;
        loop {
            if let Some(pos) = self
                .queue
                .iter()
                .position(|line| response_id(line).as_deref() == Some(id))
            {
                return Ok(self.queue.remove(pos));
            }
            self.pull(end)?;
        }
    }

    pub fn next_line(&mut self, deadline: Duration) -> Result<Vec<u8>, Error> {
        if !self.queue.is_empty() {
            return Ok(self.queue.remove(0));
        }
        self.pull(Instant::now() + deadline)?;
        if self.queue.is_empty() {
            return Err(Error::new(Kind::Protocol, "no line"));
        }
        Ok(self.queue.remove(0))
    }

    pub fn shutdown(mut self, deadline: Duration) -> Result<Stop, Error> {
        drop(self.stdin.take());
        let started = Instant::now();
        let stop = loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    break Stop {
                        code: status.code(),
                        killed: false,
                    }
                }
                Ok(None) if started.elapsed() >= deadline => {
                    let _ = self.child.kill();
                    let status = self
                        .child
                        .wait()
                        .map_err(|err| Error::new(Kind::Io, err.to_string()))?;
                    break Stop {
                        code: status.code(),
                        killed: true,
                    };
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(err) => return Err(Error::new(Kind::Io, err.to_string())),
            }
        };
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_reader.take() {
            let _ = handle.join();
        }
        Ok(stop)
    }

    fn pull(&mut self, end: Instant) -> Result<(), Error> {
        let remain = end.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            return Err(Error::new(Kind::Deadline, "deadline"));
        }
        match self.rx.recv_timeout(remain) {
            Ok(Incoming::Line(line)) => {
                self.queue.push(line);
                Ok(())
            }
            Ok(Incoming::Partial(line)) => Err(Error::new(
                Kind::Partial,
                format!("partial {} bytes", line.len()),
            )),
            Ok(Incoming::Eof) => Err(Error::new(Kind::Eof, "stdout eof")),
            Ok(Incoming::TooBig) => Err(Error::new(Kind::TooBig, "line limit")),
            Ok(Incoming::Err(detail)) => Err(Error::new(Kind::Io, detail)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::new(Kind::Deadline, "deadline")),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::new(Kind::Eof, "reader ended")),
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_reader.take() {
            let _ = handle.join();
        }
    }
}

fn drain_stderr(mut stderr: impl Read, slot: Arc<Mutex<Vec<u8>>>) {
    let mut buf = [0u8; 4096];
    loop {
        match stderr.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Ok(mut guard) = slot.lock() {
                    if guard.len() < 262_144 {
                        let room = 262_144 - guard.len();
                        guard.extend_from_slice(&buf[..n.min(room)]);
                    }
                }
            }
        }
    }
}

fn response_id(line: &[u8]) -> Option<String> {
    let value = parse_value(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("response") {
        return None;
    }
    value.get("id").and_then(Value::as_str).map(str::to_owned)
}

fn read_stdout(mut stdout: impl Read + Send + 'static, limit: usize, tx: mpsc::Sender<Incoming>) {
    let mut pending = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => {
                if !pending.is_empty() {
                    let _ = tx.send(Incoming::Partial(pending));
                }
                let _ = tx.send(Incoming::Eof);
                break;
            }
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                if !drain_lines(&mut pending, limit, &tx) {
                    break;
                }
            }
            Err(err) => {
                let _ = tx.send(Incoming::Err(err.to_string()));
                break;
            }
        }
    }
}

fn drain_lines(pending: &mut Vec<u8>, limit: usize, tx: &mpsc::Sender<Incoming>) -> bool {
    while let Some(pos) = pending.iter().position(|byte| *byte == b'\n') {
        if pos + 1 > limit {
            let _ = tx.send(Incoming::TooBig);
            return false;
        }
        let line: Vec<u8> = pending.drain(..=pos).collect();
        if tx.send(Incoming::Line(line)).is_err() {
            return false;
        }
    }
    if pending.len() > limit {
        let _ = tx.send(Incoming::TooBig);
        return false;
    }
    true
}

fn push_string(out: &mut Vec<u8>, text: &str) {
    out.push(b'"');
    for ch in text.chars() {
        match ch {
            '"' => out.extend_from_slice(br#"\""#),
            '\\' => out.extend_from_slice(br#"\\"#),
            '\n' => out.extend_from_slice(br#"\n"#),
            '\r' => out.extend_from_slice(br#"\r"#),
            '\t' => out.extend_from_slice(br#"\t"#),
            c if c.is_control() => {
                let encoded = format!("\\u{:04x}", u32::from(c));
                out.extend_from_slice(encoded.as_bytes());
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}
