//! Byte-level observation of a real Pi RPC process. Not the adapter library.
//! The fixture token and origin are public and local.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const TOKEN: &str = "public-dummy-fixture-token";
const MAX_LINE: usize = 1_048_576;

enum Incoming {
    Line(Vec<u8>),
    Partial(Vec<u8>),
    Eof,
    TooBig,
    Err(String),
}

fn main() {
    if let Err(err) = run() {
        eprintln!("observe: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or("usage: observe <abort|restore> <pi> <node-dir> <home> <out> <mock>")?;
    let pi = args.next().ok_or("missing pi")?;
    let node_dir = args.next().ok_or("missing node dir")?;
    let home = args.next().ok_or("missing home")?;
    let out = args.next().ok_or("missing out")?;
    let mock = args.next().ok_or("missing mock")?;
    if args.next().is_some() || (mode != "abort" && mode != "restore") {
        return Err("usage: observe <abort|restore> <pi> <node-dir> <home> <out> <mock>".to_owned());
    }
    fs::create_dir_all(&out).map_err(|err| format!("out: {err}"))?;
    let agent = format!("{home}/agent");
    let work = format!("{home}/work");
    let sessions = format!("{home}/sessions");
    fs::create_dir_all(&agent).map_err(|err| format!("agent: {err}"))?;
    fs::create_dir_all(&work).map_err(|err| format!("work: {err}"))?;
    fs::create_dir_all(&sessions).map_err(|err| format!("sessions: {err}"))?;
    let mock_mode = if mode == "abort" { "slow" } else { "fast" };
    let mock_log = format!("{out}/mock.log");
    let mut mock_child = Command::new(&mock)
        .arg(&mock_log)
        .arg(mock_mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn mock: {err}"))?;
    let port = read_port(mock_child.stdout.as_mut().ok_or("mock stdout")?)?;
    fs::write(format!("{out}/mock-port.txt"), format!("{port}\n")).map_err(|err| format!("port: {err}"))?;
    let models = models_json(port);
    let models_path = format!("{agent}/models.json");
    fs::write(&models_path, &models).map_err(|err| format!("models: {err}"))?;
    fs::write(format!("{out}/models.json"), &models).map_err(|err| format!("copy models: {err}"))?;
    let path = format!("{node_dir}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
    let mut pi_args = vec![
        "--mode".to_owned(),
        "rpc".to_owned(),
        "--no-tools".to_owned(),
        "--no-extensions".to_owned(),
        "--no-mcp".to_owned(),
        "--no-skills".to_owned(),
        "--no-prompt-templates".to_owned(),
        "--no-context-files".to_owned(),
        "--provider".to_owned(),
        "fixture".to_owned(),
        "--model".to_owned(),
        "fixture-echo".to_owned(),
    ];
    if mode == "abort" {
        pi_args.push("--no-session".to_owned());
    } else {
        pi_args.push("--session-dir".to_owned());
        pi_args.push(sessions.clone());
    }
    fs::write(format!("{out}/pi-argv.txt"), pi_args.join("\n") + "\n").map_err(|err| format!("argv: {err}"))?;
    let mut child = spawn_pi(&pi, &pi_args, &work, &home, &agent, &path)?;
    let stdout = child.stdout.take().ok_or("pi stdout")?;
    let rx = spawn_reader(stdout);
    let mut transcript = File::create(format!("{out}/stdout.bin")).map_err(|err| format!("stdout: {err}"))?;
    let started = Instant::now();
    let mut lines: Vec<Vec<u8>> = Vec::new();
    write_cmd(
        child.stdin.as_mut().ok_or("stdin")?,
        "p1",
        "prompt",
        r#""message":"Reply with fixture-reply only.""#,
    )?;
    let mut note = String::new();
    if mode == "abort" {
        // The user prompt itself contains the fixture word. Wait for the
        // documented assistant text_delta record, not that echo.
        let delta = wait_for(&rx, &mut transcript, &mut lines, started, Duration::from_secs(12), |line| {
            contains(line, b"\"type\":\"text_delta\"") && contains(line, b"fixture-reply")
        })?;
        fs::write(
            format!("{out}/delta-seen.txt"),
            if delta { "yes\n" } else { "no\n" },
        )
        .map_err(|err| format!("delta note: {err}"))?;
        if delta {
            write_cmd(child.stdin.as_mut().ok_or("stdin")?, "a1", "abort", "")?;
            let abort_seen = wait_for(&rx, &mut transcript, &mut lines, started, Duration::from_secs(12), |line| {
                contains(line, b"\"id\":\"a1\"")
            })?;
            if !abort_seen {
                note = "abort response id a1 missing".to_owned();
            }
        } else {
            note = "abort run did not see fixture-reply text_delta".to_owned();
        }
    } else {
        let prompt_seen = wait_for(&rx, &mut transcript, &mut lines, started, Duration::from_secs(12), |line| {
            contains(line, b"agent_settled") || contains(line, b"\"command\":\"prompt\"")
        })?;
        if !prompt_seen {
            return Err("restore run did not see a prompt response".to_owned());
        }
        let _ = wait_for(&rx, &mut transcript, &mut lines, started, Duration::from_secs(12), |line| {
            contains(line, b"agent_settled")
        })?;
    }
    write_cmd(child.stdin.as_mut().ok_or("stdin")?, "s1", "get_state", "")?;
    let _ = wait_for(&rx, &mut transcript, &mut lines, started, Duration::from_secs(8), |line| {
        contains(line, b"\"id\":\"s1\"")
    })?;
    write_cmd(child.stdin.as_mut().ok_or("stdin")?, "m1", "get_messages", "")?;
    let _ = wait_for(&rx, &mut transcript, &mut lines, started, Duration::from_secs(8), |line| {
        contains(line, b"\"id\":\"m1\"")
    })?;
    finish_child(&mut child, &rx, &mut transcript, &mut lines, &out, "first")?;
    let _ = mock_child.kill();
    let mock_status = mock_child.wait().map_err(|err| format!("wait mock: {err}"))?;
    fs::write(
        format!("{out}/mock.exit"),
        format!("{}\n", mock_status.code().unwrap_or(-1)),
    )
    .map_err(|err| format!("mock exit: {err}"))?;
    if mode == "restore" {
        restore_second(&pi, &pi_args, &work, &home, &agent, &path, &sessions, &out)?;
    }
    if !note.is_empty() {
        return Err(note);
    }
    println!("observe_ok mode={mode}");
    Ok(())
}

fn restore_second(
    pi: &str,
    first_args: &[String],
    work: &str,
    home: &str,
    agent: &str,
    path: &str,
    sessions: &str,
    out: &str,
) -> Result<(), String> {
    let session_path = find_session(sessions)?;
    fs::copy(&session_path, format!("{out}/session.jsonl")).map_err(|err| format!("copy session: {err}"))?;
    fs::write(format!("{out}/session-path.txt"), format!("{session_path}\n")).map_err(|err| format!("path: {err}"))?;
    let mut args = first_args.to_vec();
    args.retain(|arg| arg != "--session-dir" && arg != sessions);
    args.push("--no-session".to_owned());
    let mut child = spawn_pi(pi, &args, work, home, agent, path)?;
    let stdout = child.stdout.take().ok_or("second stdout")?;
    let rx = spawn_reader(stdout);
    let mut transcript = File::create(format!("{out}/restore-stdout.bin")).map_err(|err| format!("restore stdout: {err}"))?;
    let started = Instant::now();
    let mut lines = Vec::new();
    let escaped = session_path.replace('\\', "\\\\").replace('"', "\\\"");
    write_cmd(
        child.stdin.as_mut().ok_or("stdin")?,
        "sw1",
        "switch_session",
        &format!(r#""sessionPath":"{escaped}""#),
    )?;
    // Pi handles stdin commands concurrently. Wait for switch_session before
    // get_messages, or the empty in-memory session answers first and stdin
    // EOF shuts down before the switch response is written.
    let switched = wait_for(&rx, &mut transcript, &mut lines, started, Duration::from_secs(20), |line| {
        contains(line, b"\"id\":\"sw1\"")
    })?;
    fs::write(format!("{out}/sw-seen.txt"), if switched { "yes\n" } else { "no\n" }).map_err(|err| format!("sw note: {err}"))?;
    if !switched {
        finish_child(&mut child, &rx, &mut transcript, &mut lines, out, "restore")?;
        return Err("switch_session response id sw1 missing".to_owned());
    }
    write_cmd(child.stdin.as_mut().ok_or("stdin")?, "m2", "get_messages", "")?;
    let messages = wait_for(&rx, &mut transcript, &mut lines, started, Duration::from_secs(10), |line| {
        contains(line, b"\"id\":\"m2\"")
    })?;
    finish_child(&mut child, &rx, &mut transcript, &mut lines, out, "restore")?;
    if !messages {
        return Err("get_messages response id m2 missing after switch_session".to_owned());
    }
    Ok(())
}

fn find_session(dir: &str) -> Result<String, String> {
    let mut found = Vec::new();
    walk_files(Path::new(dir), &mut found)?;
    found.sort();
    found
        .into_iter()
        .find(|path| path.ends_with(".jsonl"))
        .ok_or_else(|| format!("no session jsonl under {dir}"))
}

fn walk_files(dir: &Path, found: &mut Vec<String>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("entry: {err}"))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| format!("type: {err}"))?;
        if kind.is_symlink() {
            return Err(format!("symlink refused: {}", path.display()));
        }
        if kind.is_dir() {
            walk_files(&path, found)?;
        } else if kind.is_file() {
            found.push(path.display().to_string());
        }
    }
    Ok(())
}

fn spawn_pi(pi: &str, args: &[String], work: &str, home: &str, agent: &str, path: &str) -> Result<Child, String> {
    Command::new(pi)
        .args(args)
        .current_dir(work)
        .env_clear()
        .env("HOME", home)
        .env("PI_CODING_AGENT_DIR", agent)
        .env("PATH", path)
        .env("TERM", "dumb")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn pi: {err}"))
}

fn finish_child(
    child: &mut Child,
    rx: &Receiver<Incoming>,
    transcript: &mut File,
    lines: &mut Vec<Vec<u8>>,
    out: &str,
    label: &str,
) -> Result<(), String> {
    drop(child.stdin.take());
    let started = Instant::now();
    let _ = wait_for(rx, transcript, lines, started, Duration::from_secs(8), |_| false);
    let status = match child.try_wait() {
        Ok(Some(status)) => status,
        Ok(None) => {
            child.kill().map_err(|err| format!("kill pi: {err}"))?;
            let status = child.wait().map_err(|err| format!("wait killed pi: {err}"))?;
            fs::write(format!("{out}/{label}-killed.txt"), "killed-after-deadline\n")
                .map_err(|err| format!("killed note: {err}"))?;
            status
        }
        Err(err) => return Err(format!("try_wait: {err}")),
    };
    if let Some(mut stderr) = child.stderr.take() {
        let mut err_buf = Vec::new();
        let _ = stderr.read_to_end(&mut err_buf);
        fs::write(format!("{out}/{label}-stderr.bin"), &err_buf).map_err(|err| format!("stderr: {err}"))?;
    }
    fs::write(
        format!("{out}/{label}.exit"),
        format!("{}\n", status.code().unwrap_or(-1)),
    )
    .map_err(|err| format!("exit: {err}"))?;
    Ok(())
}

fn write_cmd(stdin: &mut impl Write, id: &str, kind: &str, extra: &str) -> Result<(), String> {
    let line = if extra.is_empty() {
        format!("{{\"id\":\"{id}\",\"type\":\"{kind}\"}}\n")
    } else {
        format!("{{\"id\":\"{id}\",\"type\":\"{kind}\",{extra}}}\n")
    };
    stdin.write_all(line.as_bytes()).map_err(|err| format!("write {kind}: {err}"))?;
    stdin.flush().map_err(|err| format!("flush {kind}: {err}"))?;
    Ok(())
}

fn wait_for(
    rx: &Receiver<Incoming>,
    transcript: &mut File,
    lines: &mut Vec<Vec<u8>>,
    started: Instant,
    budget: Duration,
    mut pred: impl FnMut(&[u8]) -> bool,
) -> Result<bool, String> {
    let deadline = Instant::now() + budget;
    let mut saw = lines.iter().any(|line| pred(line));
    while Instant::now() < deadline && started.elapsed() < Duration::from_secs(60) {
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            break;
        }
        match rx.recv_timeout(remain.min(Duration::from_millis(200))) {
            Ok(Incoming::Line(line)) => {
                transcript.write_all(&line).map_err(|err| format!("record: {err}"))?;
                let matched = pred(&line);
                lines.push(line);
                if matched {
                    saw = true;
                    if lines.iter().filter(|item| pred(item)).count() >= 1 && matched {
                        // Keep reading briefly so a paired response already queued is stored.
                    }
                    if pred_satisfied(&lines, &mut pred) {
                        return Ok(true);
                    }
                }
            }
            Ok(Incoming::Partial(line)) => {
                transcript.write_all(&line).map_err(|err| format!("record partial: {err}"))?;
                fs::write("/tmp/observe-partial", &line).ok();
                return Ok(saw);
            }
            Ok(Incoming::Eof) | Ok(Incoming::TooBig) => return Ok(saw),
            Ok(Incoming::Err(err)) => return Err(err),
            Err(_) => {}
        }
        if saw {
            return Ok(true);
        }
    }
    Ok(saw)
}

fn pred_satisfied(lines: &[Vec<u8>], pred: &mut impl FnMut(&[u8]) -> bool) -> bool {
    lines.iter().any(|line| pred(line))
}

fn contains(line: &[u8], needle: &[u8]) -> bool {
    line.windows(needle.len()).any(|window| window == needle)
}

fn read_port(stdout: &mut impl Read) -> Result<u16, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        match stdout.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            Err(err) => return Err(format!("mock port: {err}")),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let rest = text.trim().strip_prefix("port=").ok_or_else(|| format!("bad port line {text}"))?;
    rest.parse().map_err(|_| format!("bad port {rest}"))
}

fn spawn_reader(mut stdout: impl Read + Send + 'static) -> Receiver<Incoming> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
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
                    while let Some(pos) = pending.iter().position(|byte| *byte == b'\n') {
                        let line: Vec<u8> = pending.drain(..=pos).collect();
                        if tx.send(Incoming::Line(line)).is_err() {
                            return;
                        }
                    }
                    if pending.len() > MAX_LINE {
                        let _ = tx.send(Incoming::TooBig);
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx.send(Incoming::Err(format!("stdout: {err}")));
                    break;
                }
            }
        }
    });
    rx
}

fn models_json(port: u16) -> String {
    format!(
        "{{\n  \"providers\": {{\n    \"fixture\": {{\n      \"baseUrl\": \"http://127.0.0.1:{port}/v1\",\n      \"api\": \"openai-completions\",\n      \"apiKey\": \"{TOKEN}\",\n      \"models\": [{{\n        \"id\": \"fixture-echo\",\n        \"name\": \"Fixture Echo\",\n        \"reasoning\": false,\n        \"input\": [\"text\"],\n        \"contextWindow\": 8192,\n        \"maxTokens\": 128,\n        \"cost\": {{\"input\": 0, \"output\": 0, \"cacheRead\": 0, \"cacheWrite\": 0}}\n      }}]\n    }}\n  }}\n}}\n"
    )
}
