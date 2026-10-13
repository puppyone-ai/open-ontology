//! Real Pi integration driver. The provider is local. The marker is not a provider key.
//! Capture is a raw stdout probe. The adapter scenarios call `Harness::launch`.

#![forbid(unsafe_code)]

use pi_harness::{
    hash_file, hex, write_models_json, Bounds, EventStream, Harness, LaunchRequest, LocalProcessNotSandbox,
    ModelConfig, ProductEvent, RuntimeMode, Sequenced, StreamItem, Termination, UserMessage, EVENT_BOUND,
    LINE_LIMIT, PINNED_PI_VERSION, PUBLIC_PROXY_MARKER, RAW_LOG_CAP, TOOL_LIMIT,
};
use serde_json::Value;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn show(err: impl ToString) -> String {
    err.to_string()
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let pi = args.next();
    let node = args.next();
    let out = args.next();
    let extra = args.next();
    let result = match (command.as_deref(), pi, node, out, extra) {
        (Some("capture"), Some(pi), Some(node), Some(out), None) => capture(Path::new(&pi), Path::new(&node), Path::new(&out)).await,
        (Some("tool"), Some(pi), Some(node), Some(out), None) => tool_turn(Path::new(&pi), Path::new(&node), Path::new(&out)).await,
        (Some("abort"), Some(pi), Some(node), Some(out), None) => abort_turn(Path::new(&pi), Path::new(&node), Path::new(&out)).await,
        (Some("disconnect"), Some(pi), Some(node), Some(out), None) => disconnect(Path::new(&pi), Path::new(&node), Path::new(&out)).await,
        (Some("restore"), Some(pi), Some(node), Some(out), None) => restore_turn(Path::new(&pi), Path::new(&node), Path::new(&out)).await,
        (Some("grok"), Some(pi), Some(node), Some(out), Some(origin)) => {
            grok_turn(Path::new(&pi), Path::new(&node), Path::new(&out), &origin).await
        }
        _ => Err("usage: pi-harness-live <capture|tool|abort|disconnect|restore|grok> <pi> <node> <out-dir> [origin]".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pi-harness-live: {err}");
            if err.starts_with("harness-rejected-thinking") { ExitCode::from(2) } else { ExitCode::from(1) }
        }
    }
}

struct Tree {
    pack: PathBuf,
    work: PathBuf,
    home: PathBuf,
    agent: PathBuf,
    session: PathBuf,
}

fn tree(out: &Path) -> Result<Tree, String> {
    let pack = out.join("pack");
    let work = out.join("work");
    let home = out.join("home");
    let agent = out.join("agent");
    let session = out.join("session");
    for dir in [&pack, &work, &home, &agent, &session] {
        fs::create_dir_all(dir).map_err(|err| format!("mkdir: {err}"))?;
    }
    fs::write(pack.join("canary"), b"pack-canary").map_err(|err| format!("canary: {err}"))?;
    Ok(Tree { pack, work, home, agent, session })
}

fn check_canary(tree: &Tree) -> Result<(), String> {
    let bytes = fs::read(tree.pack.join("canary")).map_err(|err| format!("canary read: {err}"))?;
    if bytes != b"pack-canary" {
        return Err("pack canary changed".to_owned());
    }
    Ok(())
}

fn request(tree: &Tree, pi: &Path, node: &Path, origin: String, raw: Option<PathBuf>) -> Result<LaunchRequest, String> {
    Ok(LaunchRequest {
        executable: pi.to_path_buf(),
        executable_sha256: hash_file(pi).map_err(show)?,
        runtime: node.to_path_buf(),
        runtime_sha256: hash_file(node).map_err(show)?,
        pack: tree.pack.clone(),
        workspace: tree.work.clone(),
        home: tree.home.clone(),
        agent_dir: tree.agent.clone(),
        session_dir: tree.session.clone(),
        origin,
        provider: "fixture".to_owned(),
        model: "fixture-echo".to_owned(),
        model_config: ModelConfig { name: "Fixture Echo".to_owned(), context_window: 8192, max_tokens: 512 },
        bounds: Bounds {
            line_limit: LINE_LIMIT,
            event_bound: EVENT_BOUND,
            command_deadline: Duration::from_secs(20),
            abort_ack_deadline: Duration::from_secs(5),
            kill_wait: Duration::from_secs(2),
            turn_deadline: Duration::from_secs(45),
            tool_limit: TOOL_LIMIT,
            session_byte_limit: RAW_LOG_CAP,
        },
        runtime_mode: RuntimeMode::DevelopmentLocalProcess(LocalProcessNotSandbox::acknowledge()),
        raw_event_log: raw,
    })
}

fn provider_bin() -> Result<PathBuf, String> {
    let mut path = std::env::current_exe().map_err(|err| format!("exe: {err}"))?;
    path.pop();
    path.push("fixture-provider");
    if !path.is_file() {
        return Err(format!("fixture provider missing at {}", path.display()));
    }
    Ok(path)
}

struct Provider {
    child: std::process::Child,
    log: PathBuf,
    _stderr: thread::JoinHandle<()>,
}

impl Provider {
    fn start(log: PathBuf, mode: &str) -> Result<(Self, u16), String> {
        let bin = provider_bin()?;
        let mut child = Command::new(&bin)
            .arg(&log)
            .arg(mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("provider spawn: {err}"))?;
        let stderr = child.stderr.take().ok_or("provider stderr")?;
        let stderr_path = log.with_extension("stderr");
        let _stderr = thread::Builder::new()
            .name("provider-stderr".to_owned())
            .spawn(move || {
                let mut buf = Vec::new();
                let mut pipe = stderr;
                let _ = pipe.read_to_end(&mut buf);
                let _ = fs::write(stderr_path, buf);
            })
            .map_err(|err| format!("stderr thread: {err}"))?;
        let mut stdout = child.stdout.take().ok_or("provider stdout")?;
        let port = read_port(&mut stdout)?;
        Ok((Self { child, log, _stderr }, port))
    }

    fn stop(mut self) -> Result<(), String> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let text = fs::read_to_string(&self.log).unwrap_or_default();
        if text.contains("Bearer ") || text.to_ascii_lowercase().contains("authorization:") {
            return Err("provider log contains authorization material".to_owned());
        }
        Ok(())
    }
}

fn read_port(stdout: &mut impl Read) -> Result<u16, String> {
    let mut buf = Vec::new();
    let mut byte = [0_u8; 1];
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

async fn tool_turn(pi: &Path, node: &Path, out: &Path) -> Result<(), String> {
    run_tool(pi, node, out, false).await
}

async fn restore_turn(pi: &Path, node: &Path, out: &Path) -> Result<(), String> {
    run_tool(pi, node, out, true).await
}

async fn run_tool(pi: &Path, node: &Path, out: &Path, restore: bool) -> Result<(), String> {
    fs::create_dir_all(out).map_err(show)?;
    let tree = tree(out)?;
    let (provider, port) = Provider::start(out.join("provider.log"), "tool")?;
    let origin = format!("http://127.0.0.1:{port}/v1");
    let raw = out.join("raw.ndjson");
    let launched = Harness::launch(request(&tree, pi, node, origin, Some(raw.clone()))?).await.map_err(show)?;
    let mut stream = launched.events().map_err(show)?;
    launched
        .send(UserMessage { text: "Say fixture-reply and use the fixture command.".to_owned() })
        .await
        .map_err(|err| format!("send: {err}"))?;
    let items = collect(&mut stream, Duration::from_secs(40), true).await?;
    write_items(&out.join("events.json"), &items)?;
    if !items.iter().any(is_turn_finished) {
        let stderr = launched.stderr_tail();
        fs::write(out.join("stderr.txt"), &stderr).map_err(show)?;
        return Err(format!("tool turn did not finish stderr={stderr}"));
    }
    let text = item_text(&items);
    if !text.contains("fixture-reply") || !text.contains("fixture-reply-after-tool") {
        return Err(format!("tool turn text missing replies: {text}"));
    }
    if restore {
        let transcript = launched.export_transcript().await.map_err(|err| format!("export: {err}"))?;
        fs::write(out.join("export-messages.json"), &transcript.messages).map_err(show)?;
        let session = transcript.version.session_path.clone();
        let digest = transcript.version.session_sha256;
        drop(launched);
        let again = request(&tree, pi, node, format!("http://127.0.0.1:{port}/v1"), None)?;
        let (restored, raw_messages) = Harness::restore(again, &session, digest).await.map_err(|err| format!("restore: {err}"))?;
        fs::write(out.join("restore-messages.json"), &raw_messages).map_err(show)?;
        if messages_field(&transcript.messages)? != messages_field(&raw_messages)? {
            return Err("restored messages differ".to_owned());
        }
        drop(restored);
    } else {
        drop(launched);
    }
    provider.stop()?;
    check_canary(&tree)?;
    fs::write(out.join("result.txt"), format!("ok pin={PINNED_PI_VERSION} marker={PUBLIC_PROXY_MARKER}\n")).map_err(show)?;
    Ok(())
}

async fn abort_turn(pi: &Path, node: &Path, out: &Path) -> Result<(), String> {
    fs::create_dir_all(out).map_err(show)?;
    let tree = tree(out)?;
    let (provider, port) = Provider::start(out.join("provider.log"), "slow")?;
    let launched = Harness::launch(request(&tree, pi, node, format!("http://127.0.0.1:{port}/v1"), Some(out.join("raw.ndjson")))?).await.map_err(show)?;
    let pid = launched.child_pid();
    let mut stream = launched.events().map_err(show)?;
    launched
        .send(UserMessage { text: "Say fixture-reply.".to_owned() })
        .await
        .map_err(|err| format!("send: {err}"))?;
    let items = collect(&mut stream, Duration::from_secs(20), false).await?;
    write_items(&out.join("events.json"), &items)?;
    if !item_text(&items).contains("fixture-reply") {
        return Err(format!("abort run saw no text stderr={}", launched.stderr_tail()));
    }
    let outcome = launched.abort().await.map_err(|err| format!("abort: {err}"))?;
    fs::write(
        out.join("abort.txt"),
        format!("ack={} pid={} killed={} waited={} exit={:?}\n", outcome.ack, outcome.child_pid, outcome.killed, outcome.waited, outcome.exit_code),
    )
    .map_err(show)?;
    if outcome.child_pid != pid || !outcome.waited {
        return Err("abort reap did not report the spawned pid".to_owned());
    }
    if items.iter().any(is_turn_finished) {
        return Err("abort run emitted turn_finished".to_owned());
    }
    drop(launched);
    provider.stop()?;
    check_canary(&tree)?;
    Ok(())
}

async fn disconnect(pi: &Path, node: &Path, out: &Path) -> Result<(), String> {
    fs::create_dir_all(out).map_err(show)?;
    let tree = tree(out)?;
    let (provider, port) = Provider::start(out.join("provider.log"), "slow")?;
    let launched = Harness::launch(request(&tree, pi, node, format!("http://127.0.0.1:{port}/v1"), Some(out.join("raw.ndjson")))?).await.map_err(show)?;
    let pid = launched.child_pid();
    let mut stream = launched.events().map_err(show)?;
    launched
        .send(UserMessage { text: "Say fixture-reply.".to_owned() })
        .await
        .map_err(|err| format!("send: {err}"))?;
    let mut items = collect(&mut stream, Duration::from_secs(15), false).await?;
    if !item_text(&items).contains("fixture-reply") {
        return Err(format!("disconnect saw no text stderr={}", launched.stderr_tail()));
    }
    let status = Command::new("/bin/kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status()
        .map_err(|err| format!("kill: {err}"))?;
    if !status.success() {
        return Err(format!("kill exited {status}"));
    }
    let rest = collect(&mut stream, Duration::from_secs(10), true).await?;
    items.extend(rest);
    write_items(&out.join("events.json"), &items)?;
    if items.iter().any(is_turn_finished) {
        return Err("disconnect emitted turn_finished".to_owned());
    }
    let incomplete = items.iter().any(|item| matches!(item, StreamItem::Terminated(Termination::Incomplete { .. })));
    if !incomplete {
        return Err("disconnect did not end incomplete".to_owned());
    }
    drop(launched);
    provider.stop()?;
    check_canary(&tree)?;
    Ok(())
}

async fn capture(pi: &Path, node: &Path, out: &Path) -> Result<(), String> {
    fs::create_dir_all(out).map_err(show)?;
    let tree = tree(out)?;
    let (provider, port) = Provider::start(out.join("provider.log"), "tool")?;
    let origin = format!("http://127.0.0.1:{port}/v1");
    let req = request(&tree, pi, node, origin, None)?;
    write_models_json(&req).map_err(|err| format!("models: {err}"))?;
    let version = Command::new(pi)
        .arg("--version")
        .current_dir(&tree.work)
        .env_clear()
        .env("HOME", &tree.home)
        .env("PATH", node.parent().ok_or("node directory")?)
        .env("TERM", "dumb")
        .env("PI_CODING_AGENT_DIR", &tree.agent)
        .output()
        .map_err(|err| format!("version: {err}"))?;
    fs::write(out.join("version.txt"), &version.stdout).map_err(show)?;
    if String::from_utf8_lossy(&version.stdout).trim() != PINNED_PI_VERSION {
        return Err(format!("version stdout {}", String::from_utf8_lossy(&version.stdout)));
    }
    let mut child = Command::new(pi)
        .args([
            "--mode",
            "rpc",
            "--session-dir",
            tree.session.to_str().ok_or("session utf8")?,
            "--provider",
            "fixture",
            "--model",
            "fixture-echo",
            "--no-extensions",
            "--no-mcp",
            "--no-skills",
            "--no-prompt-templates",
            "--no-context-files",
        ])
        .current_dir(&tree.work)
        .env_clear()
        .env("HOME", &tree.home)
        .env("PATH", node.parent().ok_or("node directory")?)
        .env("TERM", "dumb")
        .env("PI_CODING_AGENT_DIR", &tree.agent)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("pi: {err}"))?;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let mut stdout = child.stdout.take().ok_or("stdout")?;
    let prompt = br#"{"id":"p1","type":"prompt","message":"Say fixture-reply and use the fixture command."}"#;
    stdin.write_all(prompt).map_err(show)?;
    stdin.write_all(b"\n").map_err(show)?;
    stdin.flush().map_err(show)?;
    let mut raw = File::create(out.join("capture.ndjson")).map_err(show)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let _reader = thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => {
                    let _ = tx.send(Vec::new());
                    break;
                }
                Ok(n) => {
                    if tx.send(chunk[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(Vec::new());
                    break;
                }
            }
        }
    });
    let started = Instant::now();
    let mut pending = Vec::new();
    let mut saw_settled = false;
    while started.elapsed() < Duration::from_secs(40) {
        let remain = Duration::from_secs(40).saturating_sub(started.elapsed());
        let chunk = match rx.recv_timeout(remain) {
            Ok(chunk) => chunk,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if chunk.is_empty() {
            break;
        }
        pending.extend_from_slice(&chunk);
        while let Some(pos) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=pos).collect();
            raw.write_all(&line).map_err(show)?;
            if line.windows(b"agent_settled".len()).any(|window| window == b"agent_settled") {
                saw_settled = true;
            }
        }
        if saw_settled {
            break;
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    if !saw_settled {
        return Err("capture did not observe agent_settled".to_owned());
    }
    let exe = hex(&hash_file(pi).map_err(show)?);
    let runtime = hex(&hash_file(node).map_err(show)?);
    fs::write(out.join("hashes.txt"), format!("pi={exe}\nnode={runtime}\n")).map_err(show)?;
    fs::write(out.join("probe.txt"), "capture-not-adapter\n").map_err(show)?;
    provider.stop()?;
    check_canary(&tree)?;
    Ok(())
}

async fn grok_turn(pi: &Path, node: &Path, out: &Path, origin: &str) -> Result<(), String> {
    // Real Pi 1.0.4 process and a caller-supplied origin. Not the stub, and not a product QA claim.
    fs::create_dir_all(out).map_err(show)?;
    let tree = tree(out)?;
    let result = grok_turn_inner(pi, node, out, origin, &tree).await;
    scrub_private(out, &tree);
    result
}

async fn grok_turn_inner(pi: &Path, node: &Path, out: &Path, origin: &str, tree: &Tree) -> Result<(), String> {
    if origin.contains(PUBLIC_PROXY_MARKER) || origin.contains("llm-token") {
        return Err("origin is not a forwarder".to_owned());
    }
    let raw_path = out.join("raw.ndjson");
    let mut req = request(tree, pi, node, origin.to_owned(), Some(raw_path.clone()))?;
    req.provider = "grok".to_owned();
    req.model = "grok-4.7".to_owned();
    req.model_config = ModelConfig { name: "Grok 4.7".to_owned(), context_window: 131_072, max_tokens: 256 };
    req.bounds.command_deadline = Duration::from_secs(60);
    req.bounds.turn_deadline = Duration::from_secs(90);
    req.bounds.abort_ack_deadline = Duration::from_secs(10);
    req.bounds.kill_wait = Duration::from_secs(5);
    let launched = Harness::launch(req).await.map_err(show)?;
    let reasoning_flag = read_reasoning_flag(&tree.agent.join("models.json"))?;
    if reasoning_flag != "false" {
        return Err(format!("models reasoning flag is {reasoning_flag}"));
    }
    let mut stream = launched.events().map_err(show)?;
    let acceptance = launched
        .send(UserMessage { text: "Reply with the single word pong.".to_owned() })
        .await
        .map_err(|err| format!("send: {err}"))?;
    let items = collect(&mut stream, Duration::from_secs(100), true).await?;
    let stderr_len = launched.stderr_tail().len();
    drop(launched);
    let published = publish_raw(&raw_path, &out.join("raw-public.ndjson"))?;
    write_public_items(&out.join("events.txt"), &items)?;
    let finished = items.iter().any(is_turn_finished);
    let rejected = items.iter().any(|item| match item {
        StreamItem::Event(Sequenced { event: ProductEvent::Error { reason }, .. }) => reason.contains("thinking_start"),
        StreamItem::Terminated(Termination::Incomplete { reason }) => reason.contains("thinking_start"),
        _ => false,
    });
    let termination = items
        .iter()
        .rev()
        .find_map(|item| match item {
            StreamItem::Terminated(Termination::Finished) => Some("finished".to_owned()),
            StreamItem::Terminated(Termination::Incomplete { reason }) => Some(format!("incomplete:{reason}")),
            _ => None,
        })
        .unwrap_or_else(|| "none".to_owned());
    let label = if published.thinking_start == 0 && finished {
        "real-model-no-thinking-delta"
    } else if published.thinking_start > 0 && finished {
        "real-pi-thinking-ignored"
    } else {
        "real-pi-incomplete"
    };
    fs::write(
        out.join("result.txt"),
        format!(
            "label={label}\nkind=real-pi-1.0.4-grok-4.7-probe\npin={PINNED_PI_VERSION}\nprovider=grok\nmodel=grok-4.7\nreasoning_flag={reasoning_flag}\nacceptance_id={}\nthinking_start={}\nfinished={finished}\ntermination={termination}\nstderr_bytes={stderr_len}\nparent_acceptance=false\ngoal_acceptance=false\nqa=false\n",
            acceptance.id, published.thinking_start,
        ),
    )
    .map_err(show)?;
    check_canary(tree)?;
    if rejected {
        return Err("harness-rejected-thinking_start".to_owned());
    }
    if !finished {
        return Err(format!("turn did not finish termination={termination}"));
    }
    Ok(())
}

fn read_reasoning_flag(path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("models: {err}"))?;
    let value: Value = serde_json::from_str(&text).map_err(|err| format!("models json: {err}"))?;
    let flag = value
        .get("providers")
        .and_then(|providers| providers.get("grok"))
        .and_then(|provider| provider.get("models"))
        .and_then(|models| models.get(0))
        .and_then(|model| model.get("reasoning"))
        .and_then(Value::as_bool);
    Ok(match flag {
        Some(true) => "true".to_owned(),
        Some(false) => "false".to_owned(),
        None => "missing".to_owned(),
    })
}

struct PublishedRaw {
    thinking_start: u64,
}

fn publish_raw(raw_path: &Path, public_path: &Path) -> Result<PublishedRaw, String> {
    let bytes = fs::read(raw_path).unwrap_or_default();
    let mut thinking_start = 0_u64;
    let mut public = String::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_slice(line).map_err(|_| "raw log line did not parse".to_owned())?;
        if reasoning_leaked(&value) {
            let _ = fs::remove_file(raw_path);
            let _ = fs::remove_file(public_path);
            return Err("raw log failed redaction".to_owned());
        }
        if value.get("assistantMessageEvent").and_then(|event| event.get("type")).and_then(Value::as_str) == Some("thinking_start") {
            thinking_start = thinking_start.saturating_add(1);
        }
        public.push_str(&public_record(&value));
        public.push('\n');
    }
    fs::write(public_path, public).map_err(show)?;
    let _ = fs::remove_file(raw_path);
    Ok(PublishedRaw { thinking_start })
}

fn reasoning_leaked(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(reasoning_leaked),
        Value::Object(map) => {
            let kind = map.get("type").and_then(Value::as_str);
            if kind == Some("thinking") && map.get("thinking").and_then(Value::as_str).is_some_and(|text| text != "redacted") {
                return true;
            }
            if let Some(event) = map.get("assistantMessageEvent").and_then(Value::as_object) {
                let event_kind = event.get("type").and_then(Value::as_str);
                if event_kind == Some("thinking_delta") && event.get("delta").and_then(Value::as_str) != Some("redacted") {
                    return true;
                }
                if event_kind == Some("thinking_end") && event.get("content").and_then(Value::as_str) != Some("redacted") {
                    return true;
                }
            }
            if let Some(usage) = map.get("usage").and_then(Value::as_object)
                && let Some(text) = usage.get("reasoning").and_then(Value::as_str)
                && text != "redacted"
            {
                return true;
            }
            map.values().any(reasoning_leaked)
        }
        _ => false,
    }
}

fn public_record(value: &Value) -> String {
    let mut out = serde_json::Map::new();
    if let Some(kind) = value.get("type").and_then(Value::as_str) {
        out.insert("type".to_owned(), Value::String(kind.to_owned()));
    }
    if let Some(event) = value.get("assistantMessageEvent").and_then(Value::as_object) {
        let mut kept = serde_json::Map::new();
        if let Some(kind) = event.get("type").and_then(Value::as_str) {
            kept.insert("type".to_owned(), Value::String(kind.to_owned()));
        }
        if let Some(index) = event.get("contentIndex").and_then(Value::as_u64) {
            kept.insert("contentIndex".to_owned(), Value::from(index));
        }
        match event.get("type").and_then(Value::as_str) {
            Some("thinking_delta") => {
                kept.insert("delta".to_owned(), Value::String("redacted".to_owned()));
            }
            Some("thinking_end") => {
                kept.insert("content".to_owned(), Value::String("redacted".to_owned()));
            }
            Some("text_delta") => {
                let text = event.get("delta").and_then(Value::as_str).unwrap_or("");
                kept.insert("delta_chars".to_owned(), Value::from(text.chars().count()));
                if text.chars().count() <= 64 && !text.contains('\n') {
                    kept.insert("delta".to_owned(), Value::String(text.to_owned()));
                }
            }
            Some("error") => {
                if let Some(reason) = event.get("reason").and_then(Value::as_str) {
                    kept.insert("reason".to_owned(), Value::String(reason.to_owned()));
                }
            }
            _ => {}
        }
        if let Some(call) = event.get("toolCall").and_then(Value::as_object) {
            if let Some(name) = call.get("name").and_then(Value::as_str) {
                kept.insert("tool".to_owned(), Value::String(name.to_owned()));
            }
            if let Some(id) = call.get("id").and_then(Value::as_str) {
                kept.insert("call_id".to_owned(), Value::String(id.to_owned()));
            }
        }
        out.insert("assistantMessageEvent".to_owned(), Value::Object(kept));
    }
    if let Some(message) = value.get("message").and_then(Value::as_object)
        && let Some(stop) = message.get("stopReason").and_then(Value::as_str)
    {
        out.insert("stopReason".to_owned(), Value::String(stop.to_owned()));
    }
    serde_json::to_string(&Value::Object(out)).unwrap_or_else(|_| "{\"type\":\"redacted\"}".to_owned())
}

fn write_public_items(path: &Path, items: &[StreamItem]) -> Result<(), String> {
    let mut lines = String::new();
    for item in items {
        match item {
            StreamItem::Event(Sequenced { sequence, event }) => {
                lines.push_str(&format!("sequence={sequence} {}\n", event_summary(event)));
            }
            StreamItem::Terminated(Termination::Finished) => lines.push_str("terminated=finished\n"),
            StreamItem::Terminated(Termination::Incomplete { reason }) => {
                lines.push_str(&format!("terminated=incomplete reason={reason}\n"));
            }
        }
    }
    fs::write(path, lines).map_err(show)
}

fn event_summary(event: &ProductEvent) -> String {
    match event {
        ProductEvent::TurnStarted => "kind=turn_started".to_owned(),
        ProductEvent::AssistantDelta { text } => format!("kind=assistant_delta chars={}", text.chars().count()),
        ProductEvent::ToolCallStarted { name, call_id, args_summary } => {
            format!("kind=tool_call_started name={name} call_id={call_id} args_chars={}", args_summary.chars().count())
        }
        ProductEvent::ToolCallFinished { call_id, status, output_summary } => {
            format!("kind=tool_call_finished call_id={call_id} status={status} output_chars={}", output_summary.chars().count())
        }
        ProductEvent::TurnFinished { stop_reason, answer, usage } => {
            let chars = answer.as_ref().map(|text| text.chars().count()).unwrap_or(0);
            let usage_text = usage
                .as_ref()
                .map(|item| format!(" usage={}/{}/{}", item.input, item.output, item.total))
                .unwrap_or_default();
            format!("kind=turn_finished stop={stop_reason} answer_chars={chars}{usage_text}")
        }
        ProductEvent::Error { reason } => format!("kind=error reason={reason}"),
    }
}

fn scrub_private(out: &Path, tree: &Tree) {
    let _ = fs::remove_dir_all(&tree.session);
    let _ = fs::remove_dir_all(&tree.home);
    let _ = fs::remove_dir_all(&tree.agent);
    let _ = fs::remove_file(out.join("raw.ndjson"));
    let _ = remove_jsonl(out);
}

fn remove_jsonl(dir: &Path) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|err| format!("read dir: {err}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| format!("file type: {err}"))?;
        if kind.is_symlink() {
            if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                let _ = fs::remove_file(&path);
            }
            continue;
        }
        if kind.is_dir() {
            remove_jsonl(&path)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            fs::remove_file(&path).map_err(|err| format!("remove jsonl: {err}"))?;
        }
    }
    Ok(())
}

async fn collect(stream: &mut EventStream, limit: Duration, until_end: bool) -> Result<Vec<StreamItem>, String> {
    let mut items = Vec::new();
    let started = Instant::now();
    while started.elapsed() < limit {
        let remain = limit.saturating_sub(started.elapsed());
        match tokio::time::timeout(remain, stream.next()).await {
            Ok(Some(item)) => {
                let stop = match &item {
                    StreamItem::Terminated(_) => true,
                    StreamItem::Event(Sequenced { event: ProductEvent::TurnFinished { .. }, .. }) => until_end,
                    StreamItem::Event(Sequenced { event: ProductEvent::AssistantDelta { text }, .. }) => !until_end && text.contains("fixture-reply"),
                    _ => false,
                };
                items.push(item);
                if stop {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    Ok(items)
}

fn write_items(path: &Path, items: &[StreamItem]) -> Result<(), String> {
    let body = format!("{items:?}\n");
    fs::write(path, body).map_err(show)
}

fn item_text(items: &[StreamItem]) -> String {
    let mut out = String::new();
    for item in items {
        if let StreamItem::Event(Sequenced { event: ProductEvent::AssistantDelta { text }, .. }) = item {
            out.push_str(text);
        }
        if let StreamItem::Event(Sequenced {
            event: ProductEvent::TurnFinished { answer: Some(answer), .. },
            ..
        }) = item
        {
            out.push_str(answer);
        }
    }
    out
}

fn is_turn_finished(item: &StreamItem) -> bool {
    matches!(item, StreamItem::Event(Sequenced { event: ProductEvent::TurnFinished { .. }, .. }))
}

fn messages_field(raw: &[u8]) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(raw).map_err(show)?;
    value
        .get("data")
        .and_then(|data| data.get("messages"))
        .cloned()
        .ok_or_else(|| "get_messages data.messages missing".to_owned())
}
