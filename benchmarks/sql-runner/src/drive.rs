//! Sequential trials. Each child is a new process with files for stdio.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use oo_core::limits::DOCUMENT_MAX_BYTES;
use oo_core::parse_json_value;
use serde_json::{Map, Value};

use crate::parse::{self, Annotation, Case, Loaded};
use crate::report::{
    self, Reason, Rendered, Status, TrialOut, Verdict, apply_annotation, blank, status_for,
};

const SLICE: Duration = Duration::from_millis(20);
const CHILD_PATH: &str = "/usr/bin:/bin";
const COMPARE_VERSION: &str = "oo.benchmark.result-comparison/v1";

pub(crate) fn execute(loaded: &Loaded, run_dir: &str) -> Rendered {
    let mut trials = Vec::new();
    let mut stop = false;
    for case in &loaded.cases {
        for index in 0..case.k {
            if stop {
                trials.push(blank(&case.id, index));
                continue;
            }
            match run_trial(loaded, case, index, run_dir) {
                Ok(trial) => trials.push(trial),
                Err(trial) => {
                    trials.push(trial);
                    stop = true;
                }
            }
        }
    }
    report::render(loaded, &trials)
}

fn run_trial(
    loaded: &Loaded,
    case: &Case,
    index: u32,
    run_dir: &str,
) -> Result<TrialOut, TrialOut> {
    let dir = PathBuf::from(parse::trial_dir(run_dir, &case.id, index));
    let case_dir = dir.parent().expect("trial dir has a parent").to_path_buf();
    if !ensure_dir(&case_dir) || fs::create_dir(&dir).is_err() {
        return Err(blank(&case.id, index));
    }
    let audit = dir.join("audit.jsonl");
    let audit_text = audit.to_str().expect("audit path is utf-8");
    let rewritten = match parse::rewrite_request(&case.request, audit_text) {
        Ok(bytes) => bytes,
        Err(_) => return Err(blank(&case.id, index)),
    };
    if case.secret.exposes(&rewritten) || contains(&rewritten, case.gold_path.as_bytes()) {
        return Err(blank(&case.id, index));
    }
    if write_new(&dir.join("request.json"), &rewritten).is_err()
        || write_new(&dir.join("query.stdin"), b"").is_err()
    {
        return Err(blank(&case.id, index));
    }
    let deadline = Instant::now() + Duration::from_millis(case.trial_wall_ms);
    let request_path = dir.join("request.json");
    let request_text = request_path.to_str().expect("request path");
    let query = match spawn_named(
        &loaded.executables.query_pg,
        &["--request", request_text],
        &dir,
        "query",
    ) {
        Ok(child) => child,
        Err(_) => return Err(blank(&case.id, index)),
    };
    let mut trial = blank(&case.id, index);
    trial.backend_cleanup = Some("unknown");
    let outcome = supervise(
        query,
        deadline,
        &dir.join("query.stdout"),
        &dir.join("query.stderr"),
    );
    if matches!(outcome, Outcome::Io) {
        trial.reason = Reason::Protocol;
        trial.status = Status::Fail;
        apply_annotation(&mut trial, case.annotation);
        return Err(trial);
    }
    if matches!(outcome, Outcome::TimedOut | Outcome::Budget { .. }) {
        let view = classify(&outcome, b"");
        trial.query_exit = view.query_exit;
        let Next::Stop(reason) = view.next else {
            return Ok(finish(trial, case.annotation, Reason::Protocol, None));
        };
        return Ok(finish(trial, case.annotation, reason, None));
    }
    let stdout = match read_stdio(&dir.join("query.stdout")) {
        Ok(bytes) => bytes,
        Err(ReadIo::Over) => {
            return Ok(finish(trial, case.annotation, Reason::Budget, None));
        }
        Err(ReadIo::Io) => {
            trial.reason = Reason::Protocol;
            trial.status = Status::Fail;
            apply_annotation(&mut trial, case.annotation);
            return Err(trial);
        }
    };
    let view = classify(&outcome, &stdout);
    trial.query_exit = view.query_exit;
    trial.truncated = view.truncated;
    trial.end_audit_ok = view.end_audit_ok;
    trial.partial = view.partial;
    let truncated = match view.next {
        Next::Stop(reason) => {
            return Ok(finish(trial, case.annotation, reason, None));
        }
        Next::Compare { truncated } => truncated,
    };
    if Instant::now() >= deadline {
        return Ok(finish(trial, case.annotation, Reason::Timeout, None));
    }
    if case.binding_line.len().saturating_add(stdout.len()) > DOCUMENT_MAX_BYTES {
        return Ok(finish(trial, case.annotation, Reason::Budget, None));
    }
    let mut adapter_in = case.binding_line.clone();
    adapter_in.extend_from_slice(&stdout);
    if write_new(&dir.join("adapter.stdin"), &adapter_in).is_err() {
        trial.reason = Reason::Protocol;
        trial.status = Status::Fail;
        apply_annotation(&mut trial, case.annotation);
        return Err(trial);
    }
    let adapter = match spawn_named(&loaded.executables.adapter, &[], &dir, "adapter") {
        Ok(child) => child,
        Err(_) => {
            trial.reason = Reason::Protocol;
            trial.status = Status::Fail;
            apply_annotation(&mut trial, case.annotation);
            return Err(trial);
        }
    };
    let adapter_end = supervise(
        adapter,
        deadline,
        &dir.join("adapter.stdout"),
        &dir.join("adapter.stderr"),
    );
    if matches!(adapter_end, Outcome::Io) {
        trial.reason = Reason::Protocol;
        trial.status = Status::Fail;
        apply_annotation(&mut trial, case.annotation);
        return Err(trial);
    }
    trial.adapter_exit = exit_code(&adapter_end);
    if matches!(adapter_end, Outcome::TimedOut) {
        return Ok(finish(trial, case.annotation, Reason::Timeout, None));
    }
    if matches!(adapter_end, Outcome::Budget { .. }) {
        return Ok(finish(trial, case.annotation, Reason::Budget, None));
    }
    if trial.adapter_exit != Some(0) {
        return Ok(finish(trial, case.annotation, Reason::AdapterRefused, None));
    }
    let adapter_out = match read_stdio(&dir.join("adapter.stdout")) {
        Ok(bytes) => bytes,
        Err(ReadIo::Over) => {
            return Ok(finish(trial, case.annotation, Reason::Budget, None));
        }
        Err(ReadIo::Io) => {
            trial.reason = Reason::Protocol;
            trial.status = Status::Fail;
            apply_annotation(&mut trial, case.annotation);
            return Err(trial);
        }
    };
    let actual = match std::str::from_utf8(&adapter_out)
        .ok()
        .and_then(|text| parse_json_value(text, COMPARE_VERSION).ok())
    {
        Some(value) => value,
        None => return Ok(finish(trial, case.annotation, Reason::Protocol, None)),
    };
    if Instant::now() >= deadline {
        return Ok(finish(trial, case.annotation, Reason::Timeout, None));
    }
    let compare_bytes = match comparison_bytes(case, actual) {
        Ok(bytes) => bytes,
        Err(reason) => return Ok(finish(trial, case.annotation, reason, None)),
    };
    if write_new(&dir.join("comparator.stdin"), &compare_bytes).is_err() {
        trial.reason = Reason::Protocol;
        trial.status = Status::Fail;
        apply_annotation(&mut trial, case.annotation);
        return Err(trial);
    }
    let comparator = match spawn_named(&loaded.executables.comparator, &[], &dir, "comparator") {
        Ok(child) => child,
        Err(_) => {
            trial.reason = Reason::Protocol;
            trial.status = Status::Fail;
            apply_annotation(&mut trial, case.annotation);
            return Err(trial);
        }
    };
    let compare_end = supervise(
        comparator,
        deadline,
        &dir.join("comparator.stdout"),
        &dir.join("comparator.stderr"),
    );
    if matches!(compare_end, Outcome::Io) {
        trial.reason = Reason::Protocol;
        trial.status = Status::Fail;
        apply_annotation(&mut trial, case.annotation);
        return Err(trial);
    }
    trial.comparator_exit = exit_code(&compare_end);
    if matches!(compare_end, Outcome::TimedOut) {
        return Ok(finish(trial, case.annotation, Reason::Timeout, None));
    }
    if matches!(compare_end, Outcome::Budget { .. }) {
        return Ok(finish(trial, case.annotation, Reason::Budget, None));
    }
    let (reason, verdict) = match trial.comparator_exit {
        Some(code @ (0 | 1)) => match read_stdio(&dir.join("comparator.stdout")) {
            Ok(bytes) => match verdict_of(&bytes, code) {
                Ok(verdict) => {
                    let reason = if truncated {
                        Reason::Truncated
                    } else if verdict == Verdict::Match {
                        Reason::Match
                    } else {
                        Reason::Mismatch
                    };
                    (reason, verdict)
                }
                Err(()) => (Reason::Protocol, Verdict::Absent),
            },
            Err(ReadIo::Over) => (Reason::Budget, Verdict::Absent),
            Err(ReadIo::Io) => {
                trial.reason = Reason::Protocol;
                trial.status = Status::Fail;
                apply_annotation(&mut trial, case.annotation);
                return Err(trial);
            }
        },
        _ => (Reason::ComparatorRefused, Verdict::Absent),
    };
    Ok(finish(trial, case.annotation, reason, Some(verdict)))
}

fn finish(
    mut trial: TrialOut,
    annotation: Annotation,
    reason: Reason,
    verdict: Option<Verdict>,
) -> TrialOut {
    trial.reason = reason;
    trial.status = status_for(reason);
    if let Some(verdict) = verdict {
        trial.verdict = verdict;
    }
    apply_annotation(&mut trial, annotation);
    trial
}

fn ensure_dir(path: &Path) -> bool {
    path.is_dir() || fs::create_dir(path).is_ok()
}

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)
}

fn comparison_bytes(case: &Case, actual: Value) -> Result<Vec<u8>, Reason> {
    let mut map = Map::new();
    map.insert("actual".to_owned(), actual);
    map.insert("expected".to_owned(), case.expected.clone());
    map.insert(
        "row_comparator".to_owned(),
        Value::String(case.row_comparator.clone()),
    );
    map.insert(
        "version".to_owned(),
        Value::String(COMPARE_VERSION.to_owned()),
    );
    let mut bytes = serde_json::to_vec(&Value::Object(map)).map_err(|_| Reason::Protocol)?;
    if bytes.len() >= DOCUMENT_MAX_BYTES {
        return Err(Reason::Budget);
    }
    bytes.push(b'\n');
    Ok(bytes)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

enum Outcome {
    Exited(ExitStatus),
    TimedOut,
    Budget { exit: Option<i32> },
    Io,
}

enum ReadIo {
    Over,
    Io,
}

fn exit_code(outcome: &Outcome) -> Option<i32> {
    match outcome {
        Outcome::Exited(status) => status.code(),
        Outcome::Budget { exit } => *exit,
        Outcome::TimedOut | Outcome::Io => None,
    }
}

fn spawn_named(exe: &Path, args: &[&str], dir: &Path, stem: &str) -> std::io::Result<Child> {
    let stdin_path = dir.join(format!("{stem}.stdin"));
    let stdout_path = dir.join(format!("{stem}.stdout"));
    let stderr_path = dir.join(format!("{stem}.stderr"));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stdout_path)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stderr_path)?;
    let stdin = File::open(stdin_path)?;
    let stdout = OpenOptions::new().write(true).open(stdout_path)?;
    let stderr = OpenOptions::new().write(true).open(stderr_path)?;
    Command::new(exe)
        .args(args)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .current_dir(dir)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
}

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => Ok(None),
        }
    }

    fn disarm(&mut self) {
        self.child.take();
    }

    fn kill_wait(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child.take();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill_wait();
    }
}

fn supervise(child: Child, deadline: Instant, stdout: &Path, stderr: &Path) -> Outcome {
    let mut guard = ChildGuard::new(child);
    loop {
        match guard.try_wait() {
            Ok(Some(status)) => {
                guard.disarm();
                if over_cap(stdout) || over_cap(stderr) {
                    return Outcome::Budget {
                        exit: status.code(),
                    };
                }
                return Outcome::Exited(status);
            }
            Ok(None) => {
                if over_cap(stdout) || over_cap(stderr) {
                    guard.kill_wait();
                    return Outcome::Budget { exit: None };
                }
                if Instant::now() >= deadline {
                    guard.kill_wait();
                    return Outcome::TimedOut;
                }
                thread::sleep(SLICE);
            }
            Err(_) => {
                guard.kill_wait();
                return Outcome::Io;
            }
        }
    }
}

fn over_cap(path: &Path) -> bool {
    match fs::metadata(path) {
        Ok(meta) => meta.len() > u64::try_from(DOCUMENT_MAX_BYTES).expect("cap"),
        Err(_) => false,
    }
}

fn read_stdio(path: &Path) -> Result<Vec<u8>, ReadIo> {
    let file = File::open(path).map_err(|_| ReadIo::Io)?;
    let mut limited = file.take(u64::try_from(DOCUMENT_MAX_BYTES).expect("cap") + 1);
    let mut buf = Vec::new();
    if limited.read_to_end(&mut buf).is_err() {
        return Err(ReadIo::Io);
    }
    if buf.len() > DOCUMENT_MAX_BYTES {
        Err(ReadIo::Over)
    } else {
        Ok(buf)
    }
}

struct Summary {
    status: String,
    truncated: bool,
    end_audit_ok: bool,
    stop: Option<String>,
}

enum Next {
    Stop(Reason),
    Compare { truncated: bool },
}

struct QueryView {
    next: Next,
    query_exit: Option<i32>,
    truncated: Option<bool>,
    end_audit_ok: Option<bool>,
    partial: Option<bool>,
}

fn classify(outcome: &Outcome, stdout: &[u8]) -> QueryView {
    match outcome {
        Outcome::TimedOut => QueryView {
            next: Next::Stop(Reason::Timeout),
            query_exit: None,
            truncated: None,
            end_audit_ok: None,
            partial: None,
        },
        Outcome::Budget { exit } => QueryView {
            next: Next::Stop(Reason::Budget),
            query_exit: *exit,
            truncated: None,
            end_audit_ok: None,
            partial: None,
        },
        Outcome::Io => QueryView {
            next: Next::Stop(Reason::Protocol),
            query_exit: None,
            truncated: None,
            end_audit_ok: None,
            partial: None,
        },
        Outcome::Exited(status) => classify_exit(status.code(), stdout),
    }
}

fn classify_exit(code: Option<i32>, stdout: &[u8]) -> QueryView {
    if matches!(code, Some(3..=6)) {
        let parsed = parse_stdout(stdout).ok().flatten();
        let (truncated, end_audit_ok, partial) = summary_fields(parsed.as_ref());
        return QueryView {
            next: Next::Stop(Reason::Refused),
            query_exit: code,
            truncated,
            end_audit_ok,
            partial,
        };
    }
    let parsed = parse_stdout(stdout);
    let (truncated, end_audit_ok, partial) = match &parsed {
        Ok(Some(summary)) => summary_fields(Some(summary)),
        _ => (None, None, None),
    };
    let next = match parsed {
        Err(()) => Next::Stop(Reason::Protocol),
        Ok(None) => Next::Stop(if code == Some(0) {
            Reason::Protocol
        } else {
            Reason::NoEnd
        }),
        Ok(Some(summary))
            if summary.status == "failed" && summary.stop.as_deref() == Some("timeout") =>
        {
            Next::Stop(Reason::Timeout)
        }
        Ok(Some(summary)) if summary.status == "partial" => Next::Stop(Reason::Partial),
        Ok(Some(summary)) if !summary.end_audit_ok => Next::Stop(Reason::NoEnd),
        Ok(Some(summary)) if summary.status == "failed" => Next::Stop(Reason::Query),
        Ok(Some(summary)) if summary.status == "succeeded" && code == Some(0) => Next::Compare {
            truncated: summary.truncated,
        },
        Ok(Some(_)) => Next::Stop(Reason::Protocol),
    };
    QueryView {
        next,
        query_exit: code,
        truncated,
        end_audit_ok,
        partial,
    }
}

fn summary_fields(summary: Option<&Summary>) -> (Option<bool>, Option<bool>, Option<bool>) {
    match summary {
        Some(summary) => (
            Some(summary.truncated),
            Some(summary.end_audit_ok),
            Some(summary.status == "partial"),
        ),
        None => (None, None, None),
    }
}

fn parse_stdout(bytes: &[u8]) -> Result<Option<Summary>, ()> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let lines = split_lines(text)?;
    let mut found = None;
    for line in lines {
        let value = parse_json_value(line, "oo.gateway.query-pg/v1").map_err(|_| ())?;
        let Value::Object(map) = value else {
            return Err(());
        };
        let record = match map.get("record") {
            Some(Value::String(text)) => text.as_str(),
            _ => return Err(()),
        };
        match record {
            "row" => {}
            "summary" => {
                if found.is_some() {
                    return Err(());
                }
                found = Some(parse_summary(&map)?);
            }
            _ => return Err(()),
        }
    }
    Ok(found)
}

fn parse_summary(map: &Map<String, Value>) -> Result<Summary, ()> {
    let status = match map.get("status") {
        Some(Value::String(text)) => text.clone(),
        _ => return Err(()),
    };
    let truncated = match map.get("truncated") {
        Some(Value::Bool(flag)) => *flag,
        _ => return Err(()),
    };
    let end_audit_ok = match map.get("end_audit_ok") {
        Some(Value::Bool(flag)) => *flag,
        _ => return Err(()),
    };
    let stop = match map.get("stop") {
        Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        _ => return Err(()),
    };
    Ok(Summary {
        status,
        truncated,
        end_audit_ok,
        stop,
    })
}

fn split_lines(text: &str) -> Result<Vec<&str>, ()> {
    let mut parts: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') && parts.pop() != Some("") {
        return Err(());
    }
    if parts.iter().any(|line| line.is_empty()) {
        Err(())
    } else {
        Ok(parts)
    }
}

fn verdict_of(bytes: &[u8], code: i32) -> Result<Verdict, ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let value = parse_json_value(text, COMPARE_VERSION).map_err(|_| ())?;
    let Value::Object(map) = value else {
        return Err(());
    };
    let verdict = match map.get("verdict") {
        Some(Value::String(text)) => text.as_str(),
        _ => return Err(()),
    };
    match (code, verdict) {
        (0, "match") => Ok(Verdict::Match),
        (1, "mismatch") => Ok(Verdict::Mismatch),
        _ => Err(()),
    }
}
