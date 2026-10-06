//! Child processes and query-pg oracles. Stdout is saved only after a secret scan.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::live::Live;
use crate::stop::{disk, Stop};

pub struct CmdOut {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub struct CellExpect {
    pub kind: &'static str,
    pub value: Option<&'static str>,
}

pub fn cell_int(value: &'static str) -> CellExpect {
    CellExpect {
        kind: "int",
        value: Some(value),
    }
}

pub fn cell_text(value: &'static str) -> CellExpect {
    CellExpect {
        kind: "text",
        value: Some(value),
    }
}

pub fn cell_bool(value: &'static str) -> CellExpect {
    CellExpect {
        kind: "bool",
        value: Some(value),
    }
}

pub fn cell_null() -> CellExpect {
    CellExpect {
        kind: "null",
        value: None,
    }
}

pub fn run_cmd(bin: &Path, args: &[&str], stdin_bytes: &[u8], timeout_secs: u64) -> Result<CmdOut, Stop> {
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        .env("HOME", "/tmp")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Stop::new("spawn", "command did not start"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| Stop::new("spawn", "stdin"))?;
    stdin
        .write_all(stdin_bytes)
        .map_err(|_| Stop::new("spawn", "stdin write"))?;
    drop(stdin);
    let mut stdout = child.stdout.take().ok_or_else(|| Stop::new("spawn", "stdout"))?;
    let mut stderr = child.stderr.take().ok_or_else(|| Stop::new("spawn", "stderr"))?;
    let stdout_handle = thread::spawn(move || read_all(&mut stdout));
    let stderr_handle = thread::spawn(move || read_all(&mut stderr));
    let status = wait_child(&mut child, timeout_secs)?;
    Ok(CmdOut {
        code: status,
        stdout: stdout_handle.join().unwrap_or_default(),
        stderr: stderr_handle.join().unwrap_or_default(),
    })
}

fn read_all(pipe: &mut impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = pipe.read_to_end(&mut buf);
    buf
}

fn wait_child(child: &mut std::process::Child, timeout_secs: u64) -> Result<i32, Stop> {
    let started = Instant::now();
    let limit = Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code().unwrap_or(9)),
            Ok(None) if started.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Stop::new("spawn", "command timed out"));
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => return Err(Stop::new("spawn", "wait failed")),
        }
    }
}

pub fn save_cmd(live: &Live, name: &str, output: &CmdOut) -> Result<(), Stop> {
    let dir = live.evidence.join("steps");
    fs::create_dir_all(&dir).map_err(|err| disk(name, &err))?;
    let stdout = reject_secrets(&output.stdout, live)?;
    let stderr = reject_secrets(&output.stderr, live)?;
    fs::write(dir.join(format!("{name}.stdout")), stdout).map_err(|err| disk(name, &err))?;
    fs::write(dir.join(format!("{name}.stderr")), stderr).map_err(|err| disk(name, &err))?;
    fs::write(dir.join(format!("{name}.exit")), format!("{}\n", output.code)).map_err(|err| disk(name, &err))?;
    Ok(())
}

pub fn reject_secrets<'a>(bytes: &'a [u8], live: &Live) -> Result<&'a [u8], Stop> {
    for secret in live.passwords() {
        if !secret.is_empty() && bytes.windows(secret.len()).any(|window| window == secret.as_bytes()) {
            return Err(Stop::new("redaction", "command output contained a password"));
        }
    }
    Ok(bytes)
}

pub fn stderr_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).chars().take(400).collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn contains_bytes(haystack: &[u8], needle: &str) -> bool {
    let raw = needle.as_bytes();
    !raw.is_empty() && haystack.windows(raw.len()).any(|window| window == raw)
}

pub fn parse_json(bytes: &[u8], step: &str) -> Result<Value, Stop> {
    serde_json::from_slice(bytes.trim_ascii_end()).map_err(|_| Stop::new(step, "stdout is not one JSON value"))
}

pub fn json_lines(bytes: &[u8]) -> Result<Vec<Value>, Stop> {
    let text = std::str::from_utf8(bytes).map_err(|_| Stop::new("json", "utf8"))?;
    let mut out = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line).map_err(|_| Stop::new("json", "line"))?);
    }
    Ok(out)
}

pub fn require_exit(name: &str, output: &CmdOut, code: i32) -> Result<(), Stop> {
    if output.code == code {
        Ok(())
    } else {
        Err(Stop::new(
            name,
            format!("exit {} stderr={}", output.code, stderr_text(&output.stderr)),
        ))
    }
}

pub fn require_reject(name: &str, output: &CmdOut, needles: &[&str]) -> Result<(), Stop> {
    if output.code == 0 {
        return Err(Stop::new(name, "exit 0"));
    }
    let err = stderr_text(&output.stderr);
    if needles.iter().any(|needle| err.contains(needle)) {
        Ok(())
    } else {
        Err(Stop::new(name, format!("exit {} stderr={err}", output.code)))
    }
}

pub fn expect_query(stdout: &[u8], turns: &[(u32, &[Vec<CellExpect>])]) -> Result<(), Stop> {
    let lines = json_lines(stdout)?;
    for (turn, rows) in turns {
        let got: Vec<&Value> = lines
            .iter()
            .filter(|line| line["record"] == "row" && line["turn"].as_u64() == Some(u64::from(*turn)))
            .collect();
        if got.len() != rows.len() {
            return Err(Stop::new("query-rows", format!("turn {turn} row count")));
        }
        check_turn_rows(*turn, &got, rows)?;
        check_summary(&lines, *turn, rows.len())?;
    }
    Ok(())
}

fn check_turn_rows(turn: u32, got: &[&Value], rows: &[Vec<CellExpect>]) -> Result<(), Stop> {
    for (index, expect) in rows.iter().enumerate() {
        let cells = got[index]["cells"]
            .as_array()
            .ok_or_else(|| Stop::new("query-rows", "cells"))?;
        if cells.len() != expect.len() {
            return Err(Stop::new("query-rows", format!("turn {turn} width")));
        }
        for (cell, wanted) in cells.iter().zip(expect.iter()) {
            if !cell_matches(cell, wanted) {
                return Err(Stop::new("query-rows", format!("turn {turn} cell {index}")));
            }
        }
    }
    Ok(())
}

fn check_summary(lines: &[Value], turn: u32, rows: usize) -> Result<(), Stop> {
    let summary = lines
        .iter()
        .find(|line| line["record"] == "summary" && line["turn"].as_u64() == Some(u64::from(turn)))
        .ok_or_else(|| Stop::new("query-rows", "summary"))?;
    let width = u64::try_from(rows).unwrap_or(u64::MAX);
    if summary["status"] == "succeeded"
        && summary["truncated"] == false
        && summary["end_audit_ok"] == true
        && summary["returned_rows"].as_u64() == Some(width)
    {
        Ok(())
    } else {
        Err(Stop::new("query-rows", format!("turn {turn} summary")))
    }
}

fn cell_matches(cell: &Value, expect: &CellExpect) -> bool {
    if cell["kind"] != expect.kind {
        return false;
    }
    match expect.kind {
        "null" => cell.get("value").is_none() || cell["value"].is_null(),
        "bool" => bool_text(cell) == expect.value,
        "int" | "text" => cell["value"].as_str() == expect.value,
        _ => false,
    }
}

fn bool_text(cell: &Value) -> Option<&'static str> {
    match cell["value"].as_bool() {
        Some(true) => Some("true"),
        Some(false) => Some("false"),
        None => None,
    }
}

pub fn expect_audit(live: &Live, path: &str, role: &str, digest: &str, sqls: &[&str]) -> Result<(), Stop> {
    let raw = fs::read(path).map_err(|err| disk("audit", &err))?;
    if contains_bytes(&raw, "SELECT") {
        return Err(Stop::new("audit", "SQL text was stored"));
    }
    if raw
        .windows(live.login_password.len())
        .any(|window| window == live.login_password.as_bytes())
    {
        return Err(Stop::new("audit", "password was stored"));
    }
    let lines = json_lines(&raw)?;
    if lines.len() != sqls.len() * 2 {
        return Err(Stop::new("audit", "record count"));
    }
    for (index, sql) in sqls.iter().enumerate() {
        check_audit_pair(&lines[index * 2], &lines[index * 2 + 1], role, digest, sql)?;
    }
    Ok(())
}

fn check_audit_pair(admit: &Value, complete: &Value, role: &str, digest: &str, sql: &str) -> Result<(), Stop> {
    if admit["record"] != "admit" || complete["record"] != "complete" {
        return Err(Stop::new("audit", "admit did not precede complete"));
    }
    if admit["protocol"] != "oo.gateway.query-pg-audit/v1"
        || admit["role"] != role
        || admit["project"] != crate::live::SOURCE_ID
        || admit["policy_id"] != digest
    {
        return Err(Stop::new("audit", "identity"));
    }
    let hash = sha256_hex(sql.as_bytes());
    if admit["original_sha256"] != hash || complete["original_sha256"] != hash {
        return Err(Stop::new("audit", "original hash"));
    }
    if complete["status"] != "succeeded" {
        return Err(Stop::new("audit", "completion status"));
    }
    Ok(())
}

pub fn expect_no_audit(name: &str, output: &CmdOut, audit: &Path) -> Result<(), Stop> {
    if output.code == 0 {
        return Err(Stop::new(name, "exit 0"));
    }
    if !output.stdout.is_empty() && (output.code == 5 || output.code == 6) {
        return Err(Stop::new(name, format!("exit {} wrote stdout", output.code)));
    }
    if audit.exists() && (output.code == 5 || output.code == 6) {
        return Err(Stop::new(name, "policy or guard refusal created an audit file"));
    }
    Ok(())
}

pub fn write_json(live: &Live, name: &str, value: &Value) -> Result<(), Stop> {
    let text = serde_json::to_string_pretty(value).map_err(|_| Stop::new("json", name))?;
    let clean = scrub(&text, live);
    fs::write(live.evidence.join(name), clean + "\n").map_err(|err| disk(name, &err))
}

fn scrub(text: &str, live: &Live) -> String {
    let mut out = text.to_string();
    for secret in live.passwords() {
        if !secret.is_empty() {
            out = out.replace(secret, "[redacted]");
        }
    }
    out
}
