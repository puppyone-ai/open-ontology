use std::cell::Cell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::error::Stop;
use crate::outcome::CAPTURE_MAX;
use crate::sha256::sha256_hex;

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct BinPin {
    pub path: String,
    pub sha256: String,
}

pub struct ChildRequest {
    pub step: String,
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub run_dir: PathBuf,
    pub deadline: Duration,
    pub secrets: Vec<Vec<u8>>,
}

pub struct ChildOutput {
    pub code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn load_password(path: &Path) -> Result<Vec<u8>, Stop> {
    let meta = fs::symlink_metadata(path).map_err(|_| Stop::new("config", "config"))?;
    if !meta.file_type().is_file() || meta.len() > 1026 {
        return Err(Stop::new("config", "config"));
    }
    let mut bytes = fs::read(path).map_err(|_| Stop::new("config", "config"))?;
    if bytes.len() > 1026 {
        return Err(Stop::new("config", "config"));
    }
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(bytes.len() - 2);
    } else if bytes.ends_with(b"\n") || bytes.ends_with(b"\r") {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(Stop::new("config", "config"));
    }
    Ok(bytes)
}

pub fn contains_secret(haystack: &[u8], secrets: &[Vec<u8>]) -> bool {
    secrets.iter().any(|secret| {
        !secret.is_empty() && haystack.windows(secret.len()).any(|window| window == secret)
    })
}

pub fn pin_binary(pin: &BinPin) -> Result<PathBuf, Stop> {
    let path = PathBuf::from(&pin.path);
    if !path.is_absolute() {
        return Err(Stop::new("binary", "config"));
    }
    let meta = fs::symlink_metadata(&path).map_err(|_| Stop::new("binary", "io"))?;
    if meta.file_type().is_symlink() {
        return Err(Stop::new("binary", "symlink"));
    }
    if !meta.file_type().is_file() {
        return Err(Stop::new("binary", "io"));
    }
    if pin.sha256.len() != 64 || !pin.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Stop::new("binary", "pin"));
    }
    if pin.sha256.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(Stop::new("binary", "pin"));
    }
    let bytes = fs::read(&path).map_err(|_| Stop::new("binary", "io"))?;
    if sha256_hex(&bytes) != pin.sha256 {
        return Err(Stop::new("binary", "pin"));
    }
    Ok(path)
}

pub fn run_child(request: &ChildRequest, started: &Cell<bool>) -> Result<ChildOutput, Stop> {
    let step = request.step.as_str();
    if !safe_name(step) {
        return Err(Stop::new(step, "config"));
    }
    let steps = request.run_dir.join("steps");
    let stdin_path = steps.join(format!("{step}.stdin"));
    let stdout_path = steps.join(format!("{step}.stdout"));
    let stderr_path = steps.join(format!("{step}.stderr"));
    let exit_path = steps.join(format!("{step}.exit"));
    if contains_secret(&request.stdin, &request.secrets) || args_have_secret(&request.args, &request.secrets)
    {
        let _ = write_private(&stdin_path, b"refused before spawn\n");
        let _ = write_private(&stdout_path, b"");
        let _ = write_private(&stderr_path, b"");
        let _ = write_private(&exit_path, b"refused\n");
        return Err(Stop::new(step, "secret"));
    }
    write_private(&stdin_path, &request.stdin)?;
    write_private(&stdout_path, b"")?;
    write_private(&stderr_path, b"")?;
    let stdin = File::open(&stdin_path).map_err(|_| Stop::new(step, "io"))?;
    let stdout = OpenOptions::new()
        .write(true)
        .open(&stdout_path)
        .map_err(|_| Stop::new(step, "io"))?;
    let stderr = OpenOptions::new()
        .write(true)
        .open(&stderr_path)
        .map_err(|_| Stop::new(step, "io"))?;
    let path = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
    let mut command = Command::new(&request.bin);
    command
        .args(&request.args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", "/tmp")
        .env("LANG", "C.UTF-8")
        .current_dir("/tmp")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn().map_err(|_| Stop::new(step, "io"))?;
    started.set(true);
    let started_at = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started_at.elapsed() >= request.deadline => {
                let _ = child.kill();
                let status = child.wait().map_err(|_| Stop::new(step, "io"))?;
                let _ = write_private(&exit_path, b"timeout\n");
                let _ = bound_or_redact(&stdout_path, &request.secrets);
                let _ = bound_or_redact(&stderr_path, &request.secrets);
                return Ok(ChildOutput {
                    code: status.code(),
                    timed_out: true,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                });
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return Err(Stop::new(step, "io")),
        }
    };
    let code = status.code();
    let _ = write_private(
        &exit_path,
        format!("{}\n", code.unwrap_or(-1)).as_bytes(),
    );
    let stdout = bound_or_redact(&stdout_path, &request.secrets)?;
    let stderr = bound_or_redact(&stderr_path, &request.secrets)?;
    Ok(ChildOutput {
        code,
        timed_out: false,
        stdout,
        stderr,
    })
}

pub fn require_code(step: &str, output: &ChildOutput, expected: i32) -> Result<(), Stop> {
    if output.timed_out {
        return Err(Stop::new(step, "deadline"));
    }
    if output.code != Some(expected) {
        return Err(Stop::new(step, "exit"));
    }
    Ok(())
}

fn args_have_secret(args: &[String], secrets: &[Vec<u8>]) -> bool {
    let joined = args.join("\n");
    contains_secret(joined.as_bytes(), secrets)
}

fn bound_or_redact(path: &Path, secrets: &[Vec<u8>]) -> Result<Vec<u8>, Stop> {
    let meta = fs::symlink_metadata(path).map_err(|_| Stop::new("capture", "io"))?;
    if meta.len() > CAPTURE_MAX as u64 {
        let _ = overwrite(path, b"redacted: capture\n");
        return Err(Stop::new("capture", "capture"));
    }
    let mut file = File::open(path).map_err(|_| Stop::new("capture", "io"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| Stop::new("capture", "io"))?;
    if bytes.len() > CAPTURE_MAX {
        let _ = overwrite(path, b"redacted: capture\n");
        return Err(Stop::new("capture", "capture"));
    }
    if contains_secret(&bytes, secrets) {
        let _ = overwrite(path, b"redacted: secret\n");
        return Err(Stop::new("capture", "secret"));
    }
    Ok(bytes)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Stop> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| Stop::new("spawn", "io"))?;
    file.write_all(bytes).map_err(|_| Stop::new("spawn", "io"))?;
    Ok(())
}

fn overwrite(path: &Path, bytes: &[u8]) -> Result<(), ()> {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| ())?;
    file.write_all(bytes).map_err(|_| ())?;
    Ok(())
}

pub fn safe_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=80).contains(&bytes.len())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-' || *byte == b'_'
        })
}

pub fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn physical_ok(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == 60 && rest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn plain_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
        })
}

pub fn parse_json_slice(bytes: &[u8], step: &str) -> Result<serde_json::Value, Stop> {
    crate::json_doc::parse(bytes, CAPTURE_MAX).map_err(|fault| Stop::new(step, fault.detail()))
}

pub fn text_field<'a>(value: &'a serde_json::Value, key: &str, step: &str) -> Result<&'a str, Stop> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Stop::new(step, "shape"))
}
