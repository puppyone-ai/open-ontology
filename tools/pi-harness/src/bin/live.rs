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
        _ => Err("usage: pi-harness-live <capture|tool|abort|disconnect|restore> <pi> <node> <out-dir>".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pi-harness-live: {err}");
            ExitCode::from(1)
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
