//! Drive the pinned Pi binary through the stdio library.
//! The fixture token is public. The listener is 127.0.0.1 only.

#![forbid(unsafe_code)]
#![allow(clippy::redundant_closure_for_method_calls)]

use oo_p09_pi_rpc::{map_record, parse_value, Command, MapResult, Process, ProductEvent, Spawn};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

const TOKEN: &str = "public-dummy-fixture-token";

fn main() {
    if let Err(err) = run() {
        eprintln!("live: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let pi = args.next().ok_or("usage: pi-rpc-live <pi> <node-dir> <mock> <out>")?;
    let node_dir = args.next().ok_or("missing node dir")?;
    let mock = args.next().ok_or("missing mock")?;
    let out = args.next().ok_or("missing out")?;
    if args.next().is_some() {
        return Err("usage: pi-rpc-live <pi> <node-dir> <mock> <out>".to_owned());
    }
    fs::create_dir_all(&out).map_err(|err| err.to_string())?;
    let home = "/var/pi-home";
    let work = "/var/pi-home/work";
    fs::create_dir_all(work).map_err(|err| err.to_string())?;
    let path = format!("{node_dir}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin");
    version_and_help(&pi, home, &path, &out)?;
    cold_rpc(&pi, home, work, &path, &out)?;
    prompt_abort(&pi, &mock, home, work, &path, &out)?;
    restore(&pi, &mock, home, work, &path, &out)?;
    println!("live_ok");
    Ok(())
}

fn version_and_help(pi: &str, home: &str, path: &str, out: &str) -> Result<(), String> {
    let version = run_once(pi, &["--version"], home, path)?;
    fs::write(format!("{out}/version.stdout"), &version.stdout).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/version.stderr"), &version.stderr).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/version.exit"), format!("{}\n", version.code)).map_err(|err| err.to_string())?;
    if version.code != 0 || std::str::from_utf8(&version.stdout).unwrap_or("").trim() != "1.0.4" {
        return Err(format!("version {:?}", String::from_utf8_lossy(&version.stdout)));
    }
    let help = run_once(pi, &["--help"], home, path)?;
    fs::write(format!("{out}/help.stdout"), &help.stdout).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/help.stderr"), &help.stderr).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/help.exit"), format!("{}\n", help.code)).map_err(|err| err.to_string())?;
    if help.code != 0 || !help.stdout.windows(6).any(|window| window == b"--mode") {
        return Err("help missing --mode".to_owned());
    }
    Ok(())
}

struct Once {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_once(pi: &str, args: &[&str], home: &str, path: &str) -> Result<Once, String> {
    let output = StdCommand::new(pi)
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .env("TERM", "dumb")
        .output()
        .map_err(|err| err.to_string())?;
    Ok(Once {
        code: output.status.code().unwrap_or(-1),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn cold_rpc(pi: &str, home: &str, work: &str, path: &str, out: &str) -> Result<(), String> {
    let agent = format!("{home}/cold-agent");
    fs::create_dir_all(&agent).map_err(|err| err.to_string())?;
    let mut proc = spawn_pi(pi, &["--mode", "rpc", "--no-session"], work, home, &agent, path)?;
    for (id, kind) in [("s1", "get_state"), ("m1", "get_messages"), ("a1", "abort"), ("e1", "export"), ("r1", "restore")] {
        proc.send(&Command {
            id: id.to_owned(),
            kind: kind.to_owned(),
            extra: Vec::new(),
        })
        .map_err(|err| err.to_string())?;
        let raw = proc
            .wait_response(id, Duration::from_secs(15))
            .map_err(|err| format!("{kind}: {err}"))?;
        fs::write(format!("{out}/cold-{kind}.json"), &raw).map_err(|err| err.to_string())?;
    }
    expect_success_false(&fs::read(format!("{out}/cold-export.json")).map_err(|err| err.to_string())?, "Unknown command: export")?;
    expect_success_false(&fs::read(format!("{out}/cold-restore.json")).map_err(|err| err.to_string())?, "Unknown command: restore")?;
    let stop = proc.shutdown(Duration::from_secs(3)).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/cold.exit"), format!("code={:?} killed={}\n", stop.code, stop.killed)).map_err(|err| err.to_string())?;
    Ok(())
}

fn prompt_abort(pi: &str, mock: &str, home: &str, work: &str, path: &str, out: &str) -> Result<(), String> {
    let agent = format!("{home}/abort-agent");
    fs::create_dir_all(&agent).map_err(|err| err.to_string())?;
    let mut mock_child = start_mock(mock, &format!("{out}/abort-mock.log"), "slow")?;
    let port = read_port(mock_child.stdout.as_mut().ok_or("mock stdout")?)?;
    write_models(&agent, port, out, "abort-models.json")?;
    let mut proc = spawn_pi(
        pi,
        &[
            "--mode",
            "rpc",
            "--no-session",
            "--no-tools",
            "--no-extensions",
            "--no-mcp",
            "--no-skills",
            "--no-prompt-templates",
            "--no-context-files",
            "--provider",
            "fixture",
            "--model",
            "fixture-echo",
        ],
        work,
        home,
        &agent,
        path,
    )?;
    proc.send(&Command {
        id: "p1".to_owned(),
        kind: "prompt".to_owned(),
        extra: vec![("message".to_owned(), "Reply with fixture-reply only.".to_owned())],
    })
    .map_err(|err| err.to_string())?;
    let prompt = proc.wait_response("p1", Duration::from_secs(15)).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/abort-prompt.json"), &prompt).map_err(|err| err.to_string())?;
    let mut saw_delta = false;
    let mut transcript = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(12) {
        let remain = Duration::from_secs(12).saturating_sub(started.elapsed());
        let line = match proc.next_line(remain) {
            Ok(line) => line,
            Err(err) => return Err(format!("abort events: {err}")),
        };
        transcript.extend_from_slice(&line);
        let value = parse_value(&line).map_err(|err| format!("event shape: {err}"))?;
        match map_record(&value).map_err(|err| format!("map: {err}"))? {
            MapResult::Event(ProductEvent::AssistantDelta(text)) if text.contains("fixture-reply") => {
                saw_delta = true;
                break;
            }
            MapResult::Unknown(kind) => return Err(format!("unknown event {kind}")),
            MapResult::Event(_) | MapResult::Ignored => {}
        }
    }
    fs::write(format!("{out}/abort-events.bin"), &transcript).map_err(|err| err.to_string())?;
    if !saw_delta {
        let stderr = String::from_utf8_lossy(&proc.stderr()).to_string();
        return Err(format!("no fixture-reply text_delta stderr={stderr}"));
    }
    proc.send(&Command {
        id: "a1".to_owned(),
        kind: "abort".to_owned(),
        extra: Vec::new(),
    })
    .map_err(|err| err.to_string())?;
    let abort = proc.wait_response("a1", Duration::from_secs(12)).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/abort-response.json"), &abort).map_err(|err| err.to_string())?;
    expect_success_true(&abort)?;
    proc.send(&Command {
        id: "m1".to_owned(),
        kind: "get_messages".to_owned(),
        extra: Vec::new(),
    })
    .map_err(|err| err.to_string())?;
    let messages = proc.wait_response("m1", Duration::from_secs(10)).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/abort-messages.json"), &messages).map_err(|err| err.to_string())?;
    if !contains_bytes(&messages, b"fixture-reply") {
        return Err("abort transcript missing fixture-reply".to_owned());
    }
    let _ = proc.shutdown(Duration::from_secs(3)).map_err(|err| err.to_string())?;
    let _ = mock_child.kill();
    let _ = mock_child.wait();
    Ok(())
}

fn restore(pi: &str, mock: &str, home: &str, work: &str, path: &str, out: &str) -> Result<(), String> {
    let agent = format!("{home}/restore-agent");
    let sessions = format!("{home}/sessions");
    fs::create_dir_all(&agent).map_err(|err| err.to_string())?;
    fs::create_dir_all(&sessions).map_err(|err| err.to_string())?;
    let mut mock_child = start_mock(mock, &format!("{out}/restore-mock.log"), "fast")?;
    let port = read_port(mock_child.stdout.as_mut().ok_or("mock stdout")?)?;
    write_models(&agent, port, out, "restore-models.json")?;
    let mut proc = spawn_pi(
        pi,
        &[
            "--mode",
            "rpc",
            "--session-dir",
            &sessions,
            "--no-tools",
            "--no-extensions",
            "--no-mcp",
            "--no-skills",
            "--no-prompt-templates",
            "--no-context-files",
            "--provider",
            "fixture",
            "--model",
            "fixture-echo",
        ],
        work,
        home,
        &agent,
        path,
    )?;
    proc.send(&Command {
        id: "p1".to_owned(),
        kind: "prompt".to_owned(),
        extra: vec![("message".to_owned(), "Reply with fixture-reply only.".to_owned())],
    })
    .map_err(|err| err.to_string())?;
    let _ = proc.wait_response("p1", Duration::from_secs(15)).map_err(|err| err.to_string())?;
    let started = Instant::now();
    let mut settled = false;
    let mut transcript = Vec::new();
    while started.elapsed() < Duration::from_secs(15) {
        let remain = Duration::from_secs(15).saturating_sub(started.elapsed());
        let line = proc
            .next_line(remain)
            .map_err(|err| format!("restore events: {err}"))?;
        transcript.extend_from_slice(&line);
        let value = parse_value(&line).map_err(|err| format!("event shape: {err}"))?;
        if let MapResult::Unknown(kind) = map_record(&value).map_err(|err| format!("map: {err}"))? {
            return Err(format!("unknown event {kind}"));
        }
        if contains_bytes(&line, b"agent_settled") {
            settled = true;
            break;
        }
    }
    fs::write(format!("{out}/restore-events.bin"), &transcript).map_err(|err| err.to_string())?;
    if !settled {
        return Err(format!("no agent_settled stderr={}", String::from_utf8_lossy(&proc.stderr())));
    }
    let _ = proc.shutdown(Duration::from_secs(3)).map_err(|err| err.to_string())?;
    let _ = mock_child.kill();
    let _ = mock_child.wait();
    let session = find_session(Path::new(&sessions))?;
    fs::copy(&session, format!("{out}/session.jsonl")).map_err(|err| err.to_string())?;
    let mut second = spawn_pi(pi, &["--mode", "rpc", "--no-session"], work, home, &agent, path)?;
    second
        .send(&Command {
            id: "sw1".to_owned(),
            kind: "switch_session".to_owned(),
            extra: vec![("sessionPath".to_owned(), session.clone())],
        })
        .map_err(|err| err.to_string())?;
    let switched = second
        .wait_response("sw1", Duration::from_secs(15))
        .map_err(|err| format!("switch_session: {err}"))?;
    fs::write(format!("{out}/switch.json"), &switched).map_err(|err| err.to_string())?;
    expect_success_true(&switched)?;
    let switched_value = parse_value(&switched).map_err(|err| err.to_string())?;
    let cancelled = switched_value
        .get("data")
        .and_then(|data| data.get("cancelled"))
        .and_then(oo_p09_pi_rpc::Value::as_bool);
    if cancelled != Some(false) {
        return Err(format!("switch not loaded: {}", String::from_utf8_lossy(&switched)));
    }
    second
        .send(&Command {
            id: "m2".to_owned(),
            kind: "get_messages".to_owned(),
            extra: Vec::new(),
        })
        .map_err(|err| err.to_string())?;
    let messages = second.wait_response("m2", Duration::from_secs(10)).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/restore-messages.json"), &messages).map_err(|err| err.to_string())?;
    if !contains_bytes(&messages, b"fixture-reply") {
        return Err("restored messages missing fixture-reply".to_owned());
    }
    let _ = second.shutdown(Duration::from_secs(3)).map_err(|err| err.to_string())?;
    Ok(())
}

fn spawn_pi(pi: &str, args: &[&str], work: &str, home: &str, agent: &str, path: &str) -> Result<Process, String> {
    Process::spawn(&Spawn {
        program: PathBuf::from(pi),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: Some(PathBuf::from(work)),
        env: vec![
            ("HOME".to_owned(), home.to_owned()),
            ("PI_CODING_AGENT_DIR".to_owned(), agent.to_owned()),
            ("PATH".to_owned(), path.to_owned()),
            ("TERM".to_owned(), "dumb".to_owned()),
        ],
        clear_env: true,
        line_limit: 1_048_576,
    })
    .map_err(|err| err.to_string())
}

fn start_mock(mock: &str, log: &str, mode: &str) -> Result<std::process::Child, String> {
    StdCommand::new(mock)
        .arg(log)
        .arg(mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())
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
            Err(err) => return Err(err.to_string()),
        }
    }
    let text = String::from_utf8_lossy(&buf);
    text.trim()
        .strip_prefix("port=")
        .ok_or_else(|| format!("bad port {text}"))?
        .parse()
        .map_err(|_| format!("bad port {text}"))
}

fn write_models(agent: &str, port: u16, out: &str, name: &str) -> Result<(), String> {
    let body = format!(
        "{{\"providers\":{{\"fixture\":{{\"baseUrl\":\"http://127.0.0.1:{port}/v1\",\"api\":\"openai-completions\",\"apiKey\":\"{TOKEN}\",\"models\":[{{\"id\":\"fixture-echo\",\"name\":\"Fixture Echo\",\"reasoning\":false,\"input\":[\"text\"],\"contextWindow\":8192,\"maxTokens\":128,\"cost\":{{\"input\":0,\"output\":0,\"cacheRead\":0,\"cacheWrite\":0}}}}]}}}}}}\n"
    );
    fs::write(format!("{agent}/models.json"), &body).map_err(|err| err.to_string())?;
    fs::write(format!("{out}/{name}"), body).map_err(|err| err.to_string())?;
    Ok(())
}

fn expect_success_true(raw: &[u8]) -> Result<(), String> {
    let value = parse_value(raw).map_err(|err| err.to_string())?;
    if value.get("success").and_then(oo_p09_pi_rpc::Value::as_bool) != Some(true) {
        return Err(format!("expected success {}", String::from_utf8_lossy(raw)));
    }
    Ok(())
}

fn expect_success_false(raw: &[u8], error: &str) -> Result<(), String> {
    let value = parse_value(raw).map_err(|err| err.to_string())?;
    if value.get("success").and_then(oo_p09_pi_rpc::Value::as_bool) != Some(false) {
        return Err(format!("expected failure {}", String::from_utf8_lossy(raw)));
    }
    if value.get("error").and_then(oo_p09_pi_rpc::Value::as_str) != Some(error) {
        return Err(format!("expected {error} in {}", String::from_utf8_lossy(raw)));
    }
    Ok(())
}

fn contains_bytes(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|window| window == needle)
}

fn find_session(dir: &Path) -> Result<String, String> {
    let mut found = Vec::new();
    walk(dir, &mut found)?;
    found.sort();
    found
        .into_iter()
        .find(|path| path.ends_with(".jsonl"))
        .ok_or_else(|| format!("no session jsonl under {}", dir.display()))
}

fn walk(dir: &Path, found: &mut Vec<String>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| err.to_string())?;
        if kind.is_symlink() {
            return Err(format!("symlink refused {}", path.display()));
        }
        if kind.is_dir() {
            walk(&path, found)?;
        } else if kind.is_file() {
            found.push(path.display().to_string());
        }
    }
    Ok(())
}
