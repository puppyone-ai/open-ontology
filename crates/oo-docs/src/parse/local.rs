//! `oo-docs-parse`: length-prefixed frame protocol and the parent-side
//! supervisor (10 §4.4.3, §4.4.4).
//!
//! Wire format (all lengths u32 big-endian):
//! request  = frame(header JSON) frame(file bytes)
//! response = frame(JSON), `{"protocol","ok":true,"ir_b3","ir"}` or
//!            `{"protocol","ok":false,"code","retryable"}`.
//!
//! The parent never trusts the child: [`decode_response`] re-validates the IR
//! and recomputes `ir_b3` from the canonical bytes before anything can be
//! written to the CAS. The supervisor clears the environment, enforces a
//! deadline that covers spawn to reap, caps stderr at 256 bytes and kills the
//! child on timeout. Process-group kill needs `unsafe`/`libc`; only the child
//! itself is killed here (the engine container is the isolation boundary, see
//! `file_worker_parent.rs`).

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use oo_core::docir::{DocFormat, DocIr};
use serde_json::{Value, json};

use super::markdown::parse_markdown;
use super::text::parse_text;
use super::{ParseError, blob_b3};
use crate::normalize::{Normalized, canonical_json, seal};

pub const FRAME_PROTOCOL: &str = "oo.docs.parse/v1";
pub const MAX_HEADER_BYTES: usize = 4096;
/// Largest input accepted by the child.
pub const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
/// Largest response the parent reads.
pub const MAX_RESPONSE_BYTES: usize = 512 * 1024 * 1024;
pub const STDERR_CAP: usize = 256;

pub fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(payload)
}

pub fn read_frame<R: Read>(r: &mut R, max: usize) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds the limit",
        ));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

fn format_name(format: DocFormat) -> Option<&'static str> {
    match format {
        DocFormat::Md => Some("md"),
        DocFormat::Txt => Some("txt"),
        DocFormat::Xlsx => Some("xlsx"),
        _ => None,
    }
}

/// Request bytes for the child's stdin. Only `md` and `txt` are served.
pub fn encode_request(format: DocFormat, bytes: &[u8]) -> io::Result<Vec<u8>> {
    let name = format_name(format).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "format is not parsed locally")
    })?;
    let header = json!({"protocol": FRAME_PROTOCOL, "format": name});
    let mut out = Vec::with_capacity(bytes.len() + 64);
    write_frame(&mut out, header.to_string().as_bytes())?;
    write_frame(&mut out, bytes)?;
    Ok(out)
}

/// Child side: serve one request. Parse failures are an `ok: false` response,
/// never a panic; only broken framing returns `Err`.
pub fn serve<R: Read, W: Write>(r: &mut R, w: &mut W) -> io::Result<()> {
    let header: Value = serde_json::from_slice(&read_frame(r, MAX_HEADER_BYTES)?)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad header"))?;
    if header.get("protocol").and_then(Value::as_str) != Some(FRAME_PROTOCOL) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad protocol"));
    }
    let format = header.get("format").and_then(Value::as_str);
    let bytes = read_frame(r, MAX_INPUT_BYTES)?;
    let result = match format {
        Some("md") => parse_markdown(&bytes),
        Some("txt") => parse_text(&bytes),
        Some("xlsx") => super::xlsx::parse_xlsx(&bytes),
        _ => Err(ParseError::NotText),
    };
    let response = match result {
        Ok(n) => {
            let ir = serde_json::to_value(&n.ir)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "ir not serializable"))?;
            json!({"protocol": FRAME_PROTOCOL, "ok": true, "ir_b3": n.ir_b3, "ir": ir})
        }
        Err(e) => json!({
            "protocol": FRAME_PROTOCOL, "ok": false,
            "code": e.code(), "retryable": e.retryable(),
        }),
    };
    write_frame(w, response.to_string().as_bytes())?;
    w.flush()
}

/// Entry point of the `oo-docs-parse` binary. Exit code 0 when a response
/// frame was written, 2 on framing errors.
pub fn run_stdio() -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    match serve(&mut stdin.lock(), &mut stdout.lock()) {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

/// Failure of a supervised parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChildError {
    /// The parser returned a typed failure.
    Parse { code: String, retryable: bool },
    /// Deadline passed; the child was killed (`DOCS_PARSE_TIMEOUT`).
    Timeout,
    /// Spawn failed, the child died, or framing broke (`DOCS_PARSE_CRASH`).
    Crash { stderr: String },
    /// The response was well framed but not a valid IR (`DOCS_PARSE_IR_INVALID`).
    Invalid,
}

impl ChildError {
    pub fn code(&self) -> &str {
        match self {
            ChildError::Parse { code, .. } => code,
            ChildError::Timeout => "DOCS_PARSE_TIMEOUT",
            ChildError::Crash { .. } => "DOCS_PARSE_CRASH",
            ChildError::Invalid => "DOCS_PARSE_IR_INVALID",
        }
    }
}

/// Parent side: validate a response frame against the input it answers.
pub fn decode_response(frame: &[u8], input: &[u8]) -> Result<Normalized, ChildError> {
    let v: Value = serde_json::from_slice(frame).map_err(|_| ChildError::Invalid)?;
    if v.get("protocol").and_then(Value::as_str) != Some(FRAME_PROTOCOL) {
        return Err(ChildError::Invalid);
    }
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        let code = v.get("code").and_then(Value::as_str).unwrap_or("");
        if !code.starts_with("DOCS_PARSE_") || code.len() > 64 {
            return Err(ChildError::Invalid);
        }
        return Err(ChildError::Parse {
            code: code.to_owned(),
            retryable: v.get("retryable").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    let ir_value = v.get("ir").ok_or(ChildError::Invalid)?;
    let ir: DocIr = serde_json::from_value(ir_value.clone()).map_err(|_| ChildError::Invalid)?;
    if ir.blob_b3 != blob_b3(input) {
        return Err(ChildError::Invalid);
    }
    let claimed = v
        .get("ir_b3")
        .and_then(Value::as_str)
        .ok_or(ChildError::Invalid)?;
    let normalized = seal(ir).map_err(|_| ChildError::Invalid)?;
    // The canonical form is recomputed here; the child's claim must match it.
    if normalized.ir_b3 != claimed || canonical_json(ir_value) != normalized.canonical {
        return Err(ChildError::Invalid);
    }
    Ok(normalized)
}

/// Run `exe` (the `oo-docs-parse` binary) on `input` with a deadline.
pub fn run_child(
    exe: &Path,
    format: DocFormat,
    input: &[u8],
    deadline: Duration,
) -> Result<Normalized, ChildError> {
    let crash = |stderr: String| ChildError::Crash { stderr };
    let request = encode_request(format, input).map_err(|_| crash(String::new()))?;
    let started = Instant::now();
    let mut child = Command::new(exe)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| crash(String::new()))?;

    let mut stdin = child.stdin.take();
    let writer = std::thread::spawn(move || {
        if let Some(mut s) = stdin.take() {
            let _ = s.write_all(&request);
        }
    });
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(s) = stdout {
            let _ = s.take(MAX_RESPONSE_BYTES as u64 + 8).read_to_end(&mut out);
        }
        out
    });
    let stderr = child.stderr.take();
    let err_reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(s) = stderr {
            let _ = s.take(STDERR_CAP as u64).read_to_end(&mut out);
        }
        out
    });

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(crash(String::new()));
            }
        }
    };
    let _ = writer.join();
    let out = reader.join().unwrap_or_default();
    let err = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
    let Some(status) = status else {
        return Err(ChildError::Timeout);
    };
    if !status.success() {
        return Err(crash(err));
    }
    let frame = read_frame(&mut &out[..], MAX_RESPONSE_BYTES).map_err(|_| crash(err))?;
    decode_response(&frame, input)
}
