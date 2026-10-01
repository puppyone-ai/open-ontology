//! Integrity records of a command, and the private result of one this process spawned.
//!
//! A directory of capture bytes can be recomputed by anyone. Reading it back is
//! an integrity check. It is not execution authority, and it cannot be
//! deserialized into [`ExecutedCommand`]. That value is built only by
//! [`execute_command`] from the child this process spawned.
//!
//! The CLI has no `xtask-self-test` runner, no GitHub signing key, and no
//! digest-shape escape. Exact echo transcripts stay rejected. A longer forged
//! transcript is still only an imported record.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde_json::{Value, json};

use crate::report::Issue;
use crate::util::{duplicate_key, parse_rfc3339_utc, sha256_hex, top_level_keys};

const SCHEMA: &str = "runner-capture/v1";
const PRODUCER: &str = "coordinator";
const AUTHORITY: &str = "local-public-test";

const FIELDS: &[&str] = &[
    "schema",
    "producer",
    "authority",
    "source",
    "workflow_sha256",
    "run_id",
    "job",
    "step",
    "step_script",
    "exit_code",
    "started_at",
    "finished_at",
    "stdout_sha256",
    "stderr_sha256",
    "binding",
];

pub struct CaptureParts<'a> {
    pub repo: &'a Path,
    pub source: &'a str,
    pub run_id: &'a str,
    pub job: &'a str,
    pub step: &'a str,
    pub step_script: &'a str,
    pub exit_code: i64,
    pub started_at: &'a str,
    pub finished_at: &'a str,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
}

pub struct VerifyExpect<'a> {
    pub repo: &'a Path,
    pub source: &'a str,
    pub job: &'a str,
    pub step: &'a str,
    pub step_script: &'a str,
    pub run_id: Option<&'a str>,
}

pub struct VerifiedCapture {
    pub run_id: String,
    pub binding: String,
    pub exit_code: i64,
    pub step_script: String,
    pub stdout: Vec<u8>,
}

/// Result of a command this process spawned. There is no `Deserialize` impl.
/// JSON, jobs files, and capture directories cannot become this type.
#[derive(Clone, Debug)]
pub struct ExecutedCommand {
    job: String,
    argv: Vec<String>,
    source_commit: String,
    exit_code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    cleanup_incomplete: bool,
    output_limited: bool,
    started_at: String,
    finished_at: String,
    spawn_error: Option<String>,
}

impl ExecutedCommand {
    pub fn job(&self) -> &str {
        &self.job
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn source_commit(&self) -> &str {
        &self.source_commit
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    pub fn timed_out(&self) -> bool {
        self.timed_out
    }

    pub fn cleanup_incomplete(&self) -> bool {
        self.cleanup_incomplete
    }

    pub fn output_limited(&self) -> bool {
        self.output_limited
    }

    pub fn started_at(&self) -> &str {
        &self.started_at
    }

    pub fn finished_at(&self) -> &str {
        &self.finished_at
    }

    pub fn spawn_error(&self) -> Option<&str> {
        self.spawn_error.as_deref()
    }

    pub fn step_script(&self) -> String {
        self.argv.join(" ")
    }
}

pub const GATE_DEADLINE_MIN_SECS: u64 = 1;
pub const GATE_DEADLINE_MAX_SECS: u64 = 2700;
pub const OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const CLEANUP_BUDGET: Duration = Duration::from_millis(1500);
static CAPTURE_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn parse_gate_deadline(raw: Option<&str>) -> Result<Duration, String> {
    match raw {
        None | Some("") => Ok(Duration::from_secs(GATE_DEADLINE_MAX_SECS)),
        Some(value) => match value.parse::<u64>() {
            Ok(seconds) if (GATE_DEADLINE_MIN_SECS..=GATE_DEADLINE_MAX_SECS).contains(&seconds) => {
                Ok(Duration::from_secs(seconds))
            }
            _ => Err(format!(
                "per-command deadline must be an integer from {GATE_DEADLINE_MIN_SECS} to {GATE_DEADLINE_MAX_SECS} seconds"
            )),
        },
    }
}

pub fn gate_deadline_from_env() -> Result<Duration, String> {
    match std::env::var("OO_XTASK_GATE_DEADLINE_SECS") {
        Ok(value) => parse_gate_deadline(Some(value.trim())),
        Err(_) => parse_gate_deadline(None),
    }
}

pub fn echo_transcript(stdout: &[u8]) -> bool {
    let trimmed = trim_ascii(stdout);
    matches!(
        trimmed,
        b"echo ok" | b"echo success" | b"echo true" | b"ok" | b"success" | b"true"
    )
}

pub fn seal_capture(dir: &Path, parts: &CaptureParts<'_>) -> Result<String, String> {
    let workflow_sha256 = workflow_set_sha256(parts.repo)?;
    let stdout_sha256 = sha256_hex(parts.stdout);
    let stderr_sha256 = sha256_hex(parts.stderr);
    let binding = capture_binding(&BindingFields {
        source: parts.source,
        workflow_sha256: &workflow_sha256,
        run_id: parts.run_id,
        job: parts.job,
        step: parts.step,
        step_script: parts.step_script,
        exit_code: parts.exit_code,
        started_at: parts.started_at,
        finished_at: parts.finished_at,
        stdout_sha256: &stdout_sha256,
        stderr_sha256: &stderr_sha256,
    })?;
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    fs::write(dir.join("stdout"), parts.stdout).map_err(|error| error.to_string())?;
    fs::write(dir.join("stderr"), parts.stderr).map_err(|error| error.to_string())?;
    let body = json!({
        "schema": SCHEMA,
        "producer": PRODUCER,
        "authority": AUTHORITY,
        "source": parts.source,
        "workflow_sha256": workflow_sha256,
        "run_id": parts.run_id,
        "job": parts.job,
        "step": parts.step,
        "step_script": parts.step_script,
        "exit_code": parts.exit_code,
        "started_at": parts.started_at,
        "finished_at": parts.finished_at,
        "stdout_sha256": stdout_sha256,
        "stderr_sha256": stderr_sha256,
        "binding": binding,
    });
    fs::write(dir.join("capture.json"), body.to_string()).map_err(|error| error.to_string())?;
    Ok(binding)
}

pub fn execute_command(
    repo: &Path,
    argv: &[String],
    deadline: Duration,
    job: &str,
    source_commit: &str,
) -> ExecutedCommand {
    let started_at = crate::util::now_rfc3339();
    let started_for_failure = started_at.clone();
    let job_owned = job.to_owned();
    let argv_owned = argv.to_vec();
    let source_owned = source_commit.to_owned();
    let failure = move |spawn_error: String| ExecutedCommand {
        job: job_owned.clone(),
        argv: argv_owned.clone(),
        source_commit: source_owned.clone(),
        exit_code: -1,
        stdout: Vec::new(),
        stderr: spawn_error.as_bytes().to_vec(),
        timed_out: false,
        cleanup_incomplete: false,
        output_limited: false,
        started_at: started_for_failure.clone(),
        finished_at: crate::util::now_rfc3339(),
        spawn_error: Some(spawn_error),
    };
    let Some(program) = argv.first().filter(|program| !program.is_empty()) else {
        return failure("coordinator has no command to execute".to_owned());
    };
    let capture = match create_capture(job) {
        Ok(capture) => capture,
        Err(error) => return failure(error),
    };
    let stdout_file = match File::create(&capture.stdout) {
        Ok(file) => file,
        Err(error) => {
            let _ = fs::remove_dir_all(&capture.dir);
            return failure(format!(
                "coordinator could not open stdout capture: {error}"
            ));
        }
    };
    let stderr_file = match File::create(&capture.stderr) {
        Ok(file) => file,
        Err(error) => {
            let _ = fs::remove_dir_all(&capture.dir);
            return failure(format!(
                "coordinator could not open stderr capture: {error}"
            ));
        }
    };
    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .current_dir(repo)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_dir_all(&capture.dir);
            return failure(format!("coordinator could not spawn {program}: {error}"));
        }
    };
    let mut tree = crate::owned_process::OwnedTree::capture(child.id());
    let started = Instant::now();
    let mut timed_out = false;
    let mut output_limited = false;
    let mut natural: Option<i32> = None;
    loop {
        tree.refresh();
        if file_len(&capture.stdout) > OUTPUT_LIMIT_BYTES as u64
            || file_len(&capture.stderr) > OUTPUT_LIMIT_BYTES as u64
        {
            output_limited = true;
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                natural = Some(status.code().unwrap_or(-1));
                break;
            }
            Ok(None) if started.elapsed() >= deadline => {
                timed_out = true;
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    // Record descendants while the child is still alive, then reap it.
    // A same-group signal does not cover a descendant that left the group.
    if natural.is_none() {
        let _ = tree.kill_recorded();
        let _ = child.kill();
        natural = Some(match child.wait() {
            Ok(status) => status.code().unwrap_or(-1),
            Err(_) => -1,
        });
    }
    let cleanup = tree.confirm(CLEANUP_BUDGET);
    let (stdout, stdout_over) = read_capped(&capture.stdout, OUTPUT_LIMIT_BYTES);
    let (stderr, stderr_over) = read_capped(&capture.stderr, OUTPUT_LIMIT_BYTES);
    if stdout_over || stderr_over {
        output_limited = true;
    }
    let _ = fs::remove_dir_all(&capture.dir);
    let waited = natural.unwrap_or(-1);
    let cleanup_incomplete = cleanup.incomplete;
    let mut exit_code = waited;
    if (timed_out || output_limited || cleanup_incomplete) && exit_code == 0 {
        exit_code = -1;
    }
    ExecutedCommand {
        job: job.to_owned(),
        argv: argv.to_vec(),
        source_commit: source_commit.to_owned(),
        exit_code,
        stdout,
        stderr,
        timed_out,
        cleanup_incomplete,
        output_limited,
        started_at,
        finished_at: crate::util::now_rfc3339(),
        spawn_error: None,
    }
}

struct CapturePaths {
    dir: PathBuf,
    stdout: PathBuf,
    stderr: PathBuf,
}

fn create_capture(job: &str) -> Result<CapturePaths, String> {
    let seq = CAPTURE_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "oo-xtask-capture-{}-{}-{seq}",
        std::process::id(),
        sanitize_job(job)
    ));
    fs::create_dir_all(&dir).map_err(|error| format!("capture dir: {error}"))?;
    fs::write(dir.join("owner"), std::process::id().to_string())
        .map_err(|error| format!("capture owner: {error}"))?;
    Ok(CapturePaths {
        stdout: dir.join("stdout"),
        stderr: dir.join("stderr"),
        dir,
    })
}

fn sanitize_job(job: &str) -> String {
    let mut out = String::new();
    for ch in job.chars().take(40) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("job");
    }
    out
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn read_capped(path: &Path, limit: usize) -> (Vec<u8>, bool) {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return (Vec::new(), false),
    };
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut limited = false;
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let room = limit.saturating_sub(kept.len());
                if count > room {
                    kept.extend_from_slice(&chunk[..room]);
                    limited = true;
                    break;
                }
                kept.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    (kept, limited)
}

pub fn seal_executed(
    dir: &Path,
    repo: &Path,
    run_id: &str,
    step: &str,
    executed: &ExecutedCommand,
) -> Result<String, String> {
    seal_capture(
        dir,
        &CaptureParts {
            repo,
            source: &executed.source_commit,
            run_id,
            job: &executed.job,
            step,
            step_script: &executed.step_script(),
            exit_code: i64::from(executed.exit_code),
            started_at: &executed.started_at,
            finished_at: &executed.finished_at,
            stdout: &executed.stdout,
            stderr: &executed.stderr,
        },
    )
}

pub fn execute_and_seal(
    dir: &Path,
    repo: &Path,
    source: &str,
    run_id: &str,
    job: &str,
    step: &str,
    argv: &[String],
) -> Result<i32, String> {
    let executed = execute_command(
        repo,
        argv,
        Duration::from_secs(GATE_DEADLINE_MAX_SECS),
        job,
        source,
    );
    if let Some(error) = executed.spawn_error() {
        return Err(error.to_owned());
    }
    seal_executed(dir, repo, run_id, step, &executed)?;
    Ok(executed.exit_code())
}

pub fn verify_capture(dir: &Path, expect: &VerifyExpect<'_>) -> Result<VerifiedCapture, Issue> {
    let metadata = fs::symlink_metadata(dir).map_err(|_| {
        Issue::new(
            "gate.runner",
            dir.display().to_string(),
            "runner capture is absent. A caller digest is not execution.",
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Issue::new(
            "gate.runner",
            dir.display().to_string(),
            "runner capture is not a directory written by the coordinator",
        ));
    }
    let stdout = read_log(&dir.join("stdout"), "stdout")?;
    let stderr = read_log(&dir.join("stderr"), "stderr")?;
    let raw = fs::read(dir.join("capture.json")).map_err(|_| {
        Issue::new(
            "gate.runner",
            dir.display().to_string(),
            "runner capture json is absent",
        )
    })?;
    let text = std::str::from_utf8(&raw)
        .map_err(|_| Issue::new("gate.runner", "capture.json", "runner capture is not utf-8"))?;
    let keys = top_level_keys(text).map_err(|error| {
        Issue::new(
            "gate.runner",
            "capture.json",
            format!("forged runner capture: {error}"),
        )
    })?;
    if let Some(duplicate) = duplicate_key(&keys) {
        return Err(Issue::new(
            "gate.runner",
            "capture.json",
            format!("forged runner capture: duplicate key {duplicate}"),
        ));
    }
    if keys.len() != FIELDS.len()
        || FIELDS
            .iter()
            .any(|field| !keys.iter().any(|key| key == field))
    {
        return Err(Issue::new(
            "gate.runner",
            "capture.json",
            "runner capture fields do not match runner-capture/v1",
        ));
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|_| Issue::new("gate.runner", "capture.json", "runner capture is not JSON"))?;
    let field = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
    let schema = field("schema").unwrap_or_default();
    let producer = field("producer").unwrap_or_default();
    let authority = field("authority").unwrap_or_default();
    if schema != SCHEMA {
        return Err(Issue::new(
            "gate.runner",
            "capture.json",
            "runner capture schema is not runner-capture/v1",
        ));
    }
    if producer != PRODUCER || authority != AUTHORITY {
        let message = if producer == "github-actions" {
            "github-actions capture has no attestation. This checker does not invent GitHub credentials."
        } else if producer == "xtask-self-test" {
            "forged runner xtask-self-test. A shaped digest is not a log."
        } else if authority != AUTHORITY {
            "capture authority is not local-public-test and cannot grant release"
        } else {
            "forged runner. The producer is not the coordinator."
        };
        return Err(Issue::new("gate.runner", producer, message));
    }
    let source = required_text(&value, "source")?;
    let workflow_sha256 = required_text(&value, "workflow_sha256")?;
    let run_id = required_text(&value, "run_id")?;
    let job = required_text(&value, "job")?;
    let step = required_text(&value, "step")?;
    let step_script = required_text(&value, "step_script")?;
    let started_at = required_text(&value, "started_at")?;
    let finished_at = required_text(&value, "finished_at")?;
    let stored_stdout = required_text(&value, "stdout_sha256")?;
    let stored_stderr = required_text(&value, "stderr_sha256")?;
    let stored_binding = required_text(&value, "binding")?;
    let exit_code = value
        .get("exit_code")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            Issue::new(
                "gate.log",
                "exit_code",
                "captured status is missing or not an integer",
            )
        })?;
    if source != expect.source {
        return Err(Issue::new(
            "gate.log",
            &source,
            "replayed receipt does not match this source",
        ));
    }
    if let Some(expected_run) = expect.run_id
        && expected_run != run_id
    {
        return Err(Issue::new(
            "gate.log",
            &run_id,
            "replayed receipt does not match this run",
        ));
    }
    if job != expect.job || step != expect.step {
        return Err(Issue::new(
            "gate.log",
            &job,
            "replayed receipt is for a different job or step",
        ));
    }
    if step_script != expect.step_script {
        return Err(Issue::new(
            "gate.command",
            expect.job,
            "captured step script does not match the workflow command",
        ));
    }
    let stdout_sha256 = sha256_hex(&stdout);
    let stderr_sha256 = sha256_hex(&stderr);
    if stdout_sha256 != stored_stdout || stderr_sha256 != stored_stderr {
        return Err(Issue::new(
            "gate.log",
            expect.job,
            "tampered log bytes do not match the capture hash",
        ));
    }
    let workflow = workflow_set_sha256(expect.repo)
        .map_err(|error| Issue::new("gate.workflow", ".github/workflows", error))?;
    if workflow != workflow_sha256 {
        return Err(Issue::new(
            "gate.command",
            expect.job,
            "workflow bytes do not match the capture",
        ));
    }
    let binding = capture_binding(&BindingFields {
        source: &source,
        workflow_sha256: &workflow_sha256,
        run_id: &run_id,
        job: &job,
        step: &step,
        step_script: &step_script,
        exit_code,
        started_at: &started_at,
        finished_at: &finished_at,
        stdout_sha256: &stdout_sha256,
        stderr_sha256: &stderr_sha256,
    })
    .map_err(|error| Issue::new("gate.runner", expect.job, error))?;
    if binding != stored_binding {
        return Err(Issue::new(
            "gate.log",
            expect.job,
            "binding does not match the captured source, workflow, run, job, step, and log bytes",
        ));
    }
    let started = parse_rfc3339_utc(&started_at)
        .map_err(|_| Issue::new("gate.log", expect.job, "started_at is not a UTC timestamp"))?;
    let finished = parse_rfc3339_utc(&finished_at)
        .map_err(|_| Issue::new("gate.log", expect.job, "finished_at is not a UTC timestamp"))?;
    if finished < started {
        return Err(Issue::new(
            "gate.log",
            expect.job,
            "finished_at is before started_at",
        ));
    }
    if echo_transcript(&stdout) {
        return Err(Issue::new(
            "gate.echo",
            expect.job,
            "an echo transcript that matches a hash is not execution of the workflow command",
        ));
    }
    Ok(VerifiedCapture {
        run_id,
        binding,
        exit_code,
        step_script,
        stdout,
    })
}

pub fn workflow_set_sha256(repo: &Path) -> Result<String, String> {
    let dir = repo.join(".github/workflows");
    let entries = fs::read_dir(&dir).map_err(|error| error.to_string())?;
    let mut rows = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".yml") && !name.ends_with(".yaml") {
            continue;
        }
        let bytes = fs::read(entry.path()).map_err(|error| error.to_string())?;
        rows.push((name, sha256_hex(&bytes)));
    }
    if rows.is_empty() {
        return Err("no workflow files".to_owned());
    }
    rows.sort();
    let mut text = String::new();
    for (name, hash) in rows {
        text.push_str(&name);
        text.push('\n');
        text.push_str(&hash);
        text.push('\n');
    }
    Ok(sha256_hex(text.as_bytes()))
}

struct BindingFields<'a> {
    source: &'a str,
    workflow_sha256: &'a str,
    run_id: &'a str,
    job: &'a str,
    step: &'a str,
    step_script: &'a str,
    exit_code: i64,
    started_at: &'a str,
    finished_at: &'a str,
    stdout_sha256: &'a str,
    stderr_sha256: &'a str,
}

fn capture_binding(fields: &BindingFields<'_>) -> Result<String, String> {
    for (label, value) in [
        ("source", fields.source),
        ("workflow_sha256", fields.workflow_sha256),
        ("run_id", fields.run_id),
        ("job", fields.job),
        ("step", fields.step),
        ("step_script", fields.step_script),
        ("started_at", fields.started_at),
        ("finished_at", fields.finished_at),
        ("stdout_sha256", fields.stdout_sha256),
        ("stderr_sha256", fields.stderr_sha256),
    ] {
        if value.is_empty() || value.contains(['\n', '\r']) {
            return Err(format!("{label} is empty or contains a newline"));
        }
    }
    let preimage = format!(
        "open-ontology-runner-capture/v1\nproducer={PRODUCER}\nauthority={AUTHORITY}\nsource={}\nworkflow_sha256={}\nrun_id={}\njob={}\nstep={}\nstep_script={}\nexit_code={}\nstarted_at={}\nfinished_at={}\nstdout_sha256={}\nstderr_sha256={}\n",
        fields.source,
        fields.workflow_sha256,
        fields.run_id,
        fields.job,
        fields.step,
        fields.step_script,
        fields.exit_code,
        fields.started_at,
        fields.finished_at,
        fields.stdout_sha256,
        fields.stderr_sha256,
    );
    Ok(sha256_hex(preimage.as_bytes()))
}

fn required_text(value: &Value, name: &str) -> Result<String, Issue> {
    match value.get(name).and_then(Value::as_str) {
        Some(text) if !text.is_empty() => Ok(text.to_owned()),
        _ => Err(Issue::new(
            "gate.runner",
            name,
            "runner capture field is missing",
        )),
    }
}

fn read_log(path: &Path, label: &str) -> Result<Vec<u8>, Issue> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(Issue::new("gate.log", label, "runner log is a symlink"))
        }
        Ok(metadata) if metadata.is_file() => {
            fs::read(path).map_err(|_| Issue::new("gate.log", label, "runner log cannot be read"))
        }
        _ => Err(Issue::new(
            "gate.log",
            label,
            "runner log is missing. A caller hash is not a log.",
        )),
    }
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && matches!(bytes[start], b' ' | b'\n' | b'\r' | b'\t') {
        start += 1;
    }
    while end > start && matches!(bytes[end - 1], b' ' | b'\n' | b'\r' | b'\t') {
        end -= 1;
    }
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn deadline_accepts_only_a_positive_bounded_range() {
        assert!(parse_gate_deadline(None).is_ok());
        assert!(parse_gate_deadline(Some("")).is_ok());
        assert_eq!(
            parse_gate_deadline(Some("1")).unwrap().as_secs(),
            GATE_DEADLINE_MIN_SECS
        );
        assert_eq!(
            parse_gate_deadline(Some("2700")).unwrap().as_secs(),
            GATE_DEADLINE_MAX_SECS
        );
        assert!(parse_gate_deadline(Some("0")).is_err());
        assert!(parse_gate_deadline(Some("-1")).is_err());
        assert!(parse_gate_deadline(Some("2701")).is_err());
        assert!(parse_gate_deadline(Some("nope")).is_err());
    }

    #[test]
    fn failed_child_keeps_streams_and_exit() {
        let repo = std::env::temp_dir();
        let executed = execute_command(
            &repo,
            &[
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "echo shim-stdout; echo shim-stderr >&2; exit 7".to_owned(),
            ],
            Duration::from_secs(5),
            "gate-docs",
            "a".repeat(40).as_str(),
        );
        assert!(executed.spawn_error().is_none());
        assert!(!executed.timed_out());
        assert!(!executed.cleanup_incomplete());
        assert!(!executed.output_limited());
        assert_eq!(executed.exit_code(), 7);
        assert!(
            executed
                .stdout()
                .windows(11)
                .any(|window| window == b"shim-stdout")
        );
        assert!(
            executed
                .stderr()
                .windows(11)
                .any(|window| window == b"shim-stderr")
        );
    }

    #[test]
    fn timeout_reaps_the_owned_group_and_leaves_a_foreign_process() {
        let mut foreign = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("foreign sleep");
        let foreign_pid = foreign.id();
        let root = std::env::temp_dir().join(format!(
            "oo-a051-deadline-{}-{}",
            std::process::id(),
            foreign_pid
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let pid_path = root.join("descendant.pid");
        let script = root.join("sleeper.sh");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 30 &\necho $! > {}\nwait\n",
                pid_path.display()
            ),
        )
        .unwrap();
        let executed = execute_command(
            &root,
            &["/bin/sh".to_owned(), script.display().to_string()],
            Duration::from_secs(1),
            "gate-docs",
            "b".repeat(40).as_str(),
        );
        assert!(executed.timed_out(), "deadline did not fire");
        assert!(
            !executed.cleanup_incomplete(),
            "same-group cleanup incomplete"
        );
        assert!(!executed.output_limited());
        assert_ne!(executed.exit_code(), 0);
        let descendant = fs::read_to_string(&pid_path).unwrap_or_default();
        let descendant_pid: u32 = descendant.trim().parse().unwrap_or(0);
        let descendant_gone = wait_until_gone(descendant_pid);
        let foreign_state = process_state(foreign_pid);
        let _ = foreign.kill();
        let _ = foreign.wait();
        let _ = fs::remove_dir_all(&root);
        assert!(descendant_pid > 1, "descendant pid was not recorded");
        assert!(descendant_gone, "owned descendant was still running");
        assert!(
            foreign_state
                .as_deref()
                .is_some_and(|state| !state.starts_with('Z')),
            "foreign process was killed or zombied: {foreign_state:?}"
        );
    }

    #[test]
    fn timeout_reaps_a_setsid_descendant_and_spares_a_foreign_process() {
        let mut foreign = Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("foreign sleep");
        let foreign_pid = foreign.id();
        let root = std::env::temp_dir().join(format!(
            "oo-a058-setsid-{}-{}",
            std::process::id(),
            foreign_pid
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let pid_path = root.join("descendant.pid");
        let script = root.join("escape.sh");
        fs::write(
            &script,
            "#!/bin/sh\n\
pidfile=$1\n\
if [ -x /usr/bin/setsid ]; then\n\
  /usr/bin/setsid /bin/sleep 300 &\n\
elif [ -x /bin/setsid ]; then\n\
  /bin/setsid /bin/sleep 300 &\n\
elif [ -x /usr/bin/python3 ]; then\n\
  /usr/bin/python3 -c 'import os, time; os.setsid(); time.sleep(300)' &\n\
else\n\
  echo NO_SETSID > \"$pidfile\"\n\
  exit 9\n\
fi\n\
echo $! > \"$pidfile\"\n\
wait\n",
        )
        .unwrap();
        let started = Instant::now();
        let executed = execute_command(
            &root,
            &[
                "/bin/sh".to_owned(),
                script.display().to_string(),
                pid_path.display().to_string(),
            ],
            Duration::from_secs(1),
            "gate-docs",
            "c".repeat(40).as_str(),
        );
        let elapsed = started.elapsed();
        let recorded = fs::read_to_string(&pid_path).unwrap_or_default();
        let descendant_pid: u32 = recorded.trim().parse().unwrap_or(0);
        let descendant_gone = wait_until_gone(descendant_pid);
        let foreign_state = process_state(foreign_pid);
        let _ = foreign.kill();
        let _ = foreign.wait();
        let _ = fs::remove_dir_all(&root);
        assert!(
            elapsed < Duration::from_secs(8),
            "setsid command did not return bounded: {elapsed:?}"
        );
        assert!(!recorded.contains("NO_SETSID"), "{recorded}");
        assert!(
            descendant_pid > 1,
            "setsid pid was not recorded: {recorded}"
        );
        assert!(executed.timed_out(), "deadline did not fire");
        assert!(executed.spawn_error().is_none());
        assert_ne!(executed.exit_code(), 0);
        assert!(
            !executed.cleanup_incomplete(),
            "setsid descendant was not stopped"
        );
        assert!(descendant_gone, "setsid descendant still running");
        assert!(
            foreign_state
                .as_deref()
                .is_some_and(|state| !state.starts_with('Z')),
            "foreign process was killed or zombied: {foreign_state:?}"
        );
    }

    #[test]
    fn output_over_the_limit_is_truncated_and_fails() {
        let started = Instant::now();
        let executed = execute_command(
            &std::env::temp_dir(),
            &[
                "/bin/dd".to_owned(),
                "if=/dev/zero".to_owned(),
                "bs=1048576".to_owned(),
                "count=17".to_owned(),
            ],
            Duration::from_secs(10),
            "gate-docs",
            "d".repeat(40).as_str(),
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(15),
            "output limit did not return bounded: {elapsed:?}"
        );
        assert!(executed.spawn_error().is_none(), "{:?}", executed.stderr());
        assert!(executed.output_limited(), "output limit was not reported");
        assert!(!executed.timed_out());
        assert!(!executed.cleanup_incomplete());
        assert_ne!(executed.exit_code(), 0);
        assert_eq!(executed.stdout().len(), OUTPUT_LIMIT_BYTES);
        assert!(executed.stdout().iter().all(|byte| *byte == 0));
    }

    fn process_state(pid: u32) -> Option<String> {
        if pid <= 1 {
            return None;
        }
        let output = Command::new("ps")
            .args(["-o", "state=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if text.is_empty() { None } else { Some(text) }
    }

    fn wait_until_gone(pid: u32) -> bool {
        if pid <= 1 {
            return false;
        }
        for _ in 0..50 {
            match process_state(pid) {
                None => return true,
                Some(state) if state.starts_with('Z') => {}
                Some(_) => {}
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        process_state(pid).is_none()
    }
}
