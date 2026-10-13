//! Installation probe. Real Pi, private home, public marker, no business answer.
//! Success is `get_state`, a read of the pack, and bash `oo --help`.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::events::{self, Mapped};
use crate::handoff;
use crate::json::{self, Json};
use crate::manifest::{self, verify_fact_hashes};
use crate::provider;
use crate::scan;
use crate::{PACK_MARKER, PI_VERSION, PUBLIC_MARKER};

const LINE_LIMIT: usize = 1_048_576;

pub struct ProbePaths<'a> {
    pub out: &'a Path,
    pub pi: &'a Path,
    pub runtime: &'a Path,
    pub oo: &'a Path,
    pub pack: &'a Path,
    pub facts: &'a Path,
    pub cjk: &'a Path,
    pub handoff: &'a Path,
    pub volume: &'a str,
    pub mountpoint: &'a str,
}

pub fn run(paths: &ProbePaths<'_>) -> Result<(), String> {
    let facts_text = fs::read_to_string(paths.facts).map_err(|err| format!("facts: {err}"))?;
    let facts = manifest::read_facts(&facts_text)?;
    verify_fact_hashes(&facts)?;
    let readme = paths.pack.join("README.md");
    let pack_bytes = fs::read(&readme).map_err(|err| format!("pack: {err}"))?;
    if !pack_bytes.windows(PACK_MARKER.len()).any(|window| window == PACK_MARKER.as_bytes()) {
        return Err("pack marker is absent".to_owned());
    }
    if scan::secret_hit(&pack_bytes).is_some() {
        return Err("pack contains secret material".to_owned());
    }
    let home = paths.out.join("home");
    let work = paths.out.join("work");
    let agent = paths.out.join("agent");
    let session = paths.out.join("session");
    for dir in [&home, &work, &agent, &session] {
        fs::create_dir_all(dir).map_err(|err| format!("mkdir {}: {err}", dir.display()))?;
    }
    for (left, right) in [(paths.pack, &home), (paths.pack, &work), (paths.pack, &agent), (paths.pack, &session)] {
        if overlaps(left, right) {
            return Err("pack overlaps a private directory".to_owned());
        }
    }
    let runtime_dir = paths.runtime.parent().ok_or("runtime has no directory")?;
    let pi_text = path_text(paths.pi)?;
    let version = command_output(paths.runtime, &[&pi_text, "--version"], &work, &home, runtime_dir, &agent)?;
    manifest::write_bytes(&paths.out.join("pi-version.txt"), version.as_bytes())?;
    if version.trim() != PI_VERSION {
        return Err(format!("pi version is {version:?}"));
    }
    let argv = rpc_argv(&session)?;
    let mut shown = vec![path_text(paths.runtime)?, pi_text];
    shown.extend(argv.iter().cloned());
    if !shown.iter().any(|arg| arg == "--no-mcp") || shown.iter().any(|arg| arg.contains(PUBLIC_MARKER)) {
        return Err("rpc argv lost --no-mcp or contains the marker".to_owned());
    }
    manifest::write_bytes(&paths.out.join("argv.txt"), format!("{}\n", shown.join("\n")).as_bytes())?;
    let stop = Arc::new(AtomicBool::new(false));
    let port_file = paths.out.join("provider.port");
    let provider_log = paths.out.join("provider.log");
    let oo_text = path_text(paths.oo)?;
    let stop_provider = Arc::clone(&stop);
    let provider_port = port_file.clone();
    let provider_log_thread = provider_log.clone();
    let provider_thread = thread::spawn(move || provider::serve(&provider_port, &provider_log_thread, &oo_text, stop_provider));
    let port = wait_port(&port_file, Duration::from_secs(5))?;
    write_models(&agent, port)?;
    let mut child = Command::new(paths.runtime)
        .arg(paths.pi)
        .args(&argv)
        .current_dir(&work)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", runtime_dir)
        .env("TERM", "dumb")
        .env("PI_CODING_AGENT_DIR", &agent)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn pi: {err}"))?;
    let stdout = child.stdout.take().ok_or("pi stdout")?;
    let stderr = child.stderr.take().ok_or("pi stderr")?;
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let lines_reader = Arc::clone(&lines);
    let raw_path = paths.out.join("raw.ndjson");
    let too_big = Arc::new(AtomicBool::new(false));
    let too_big_reader = Arc::clone(&too_big);
    let reader = thread::spawn(move || read_stdout(stdout, &raw_path, lines_reader, too_big_reader));
    let stderr_path = paths.out.join("pi.stderr");
    let stderr_thread = thread::spawn(move || read_stderr(stderr, &stderr_path));
    let mut stdin = child.stdin.take().ok_or("pi stdin")?;
    let mut failure = String::new();
    if let Err(err) = drive(&mut stdin, &lines, &too_big) {
        failure = err;
    }
    let _ = stdin.write_all(b"");
    drop(stdin);
    let exit = wait_child(&mut child, Duration::from_secs(15));
    stop.store(true, Ordering::Relaxed);
    let _ = provider_thread.join();
    let _ = reader.join();
    let _ = stderr_thread.join();
    let stored = lines.lock().unwrap_or_else(|err| err.into_inner()).clone();
    let summary = summarize(&stored, &failure, exit);
    let probe_json = format!("{}\n", summary.compact());
    manifest::write_bytes(&paths.out.join("probe.json"), probe_json.as_bytes())?;
    scan::scan_tree(paths.out)?;
    if !failure.is_empty() || summary.get("ok").and_then(Json::as_bool) != Some(true) {
        return Err(if failure.is_empty() { "probe summary is not ok".to_owned() } else { failure });
    }
    let cjk_text = fs::read_to_string(paths.cjk).map_err(|err| format!("cjk: {err}"))?;
    let document = handoff::build(&facts_text, &cjk_text, &probe_json, paths.volume, paths.mountpoint)?;
    manifest::write_bytes(paths.handoff, document.as_bytes())?;
    Ok(())
}

fn drive(stdin: &mut impl Write, lines: &Mutex<Vec<String>>, too_big: &AtomicBool) -> Result<(), String> {
    send(stdin, r#"{"id":"init-1","type":"get_state"}"#)?;
    wait_match(lines, too_big, Duration::from_secs(30), |line| events::response_ok(line, "init-1"))?;
    let message = "Installation probe. Read /knowledge/README.md with the read tool. Then run oo --help with the bash tool. This is not a business question. Do not write SQL.";
    send(stdin, &format!("{{\"id\":\"prompt-1\",\"type\":\"prompt\",\"message\":{}}}", json::escape(message)))?;
    wait_match(lines, too_big, Duration::from_secs(90), events::prompt_started)?;
    wait_tools(lines, too_big, Duration::from_secs(90))?;
    Ok(())
}

struct ToolStatus {
    read_ok: bool,
    bash_ok: bool,
    unknown: Vec<String>,
}

fn tool_status(lines: &[String]) -> Result<ToolStatus, String> {
    let mut read_id = None;
    let mut bash_id = None;
    let mut read_ok = false;
    let mut bash_ok = false;
    let mut unknown = Vec::new();
    for line in lines {
        match events::map_line(line)? {
            Mapped::ToolStarted { name, call_id, .. } if name == "read" && read_id.is_none() => read_id = Some(call_id),
            Mapped::ToolStarted { name, call_id, .. } if name == "bash" && bash_id.is_none() => bash_id = Some(call_id),
            Mapped::ToolFinished { call_id, status, output_summary } => {
                if status == "ok" && read_id.as_ref() == Some(&call_id) && output_summary.contains(PACK_MARKER) {
                    read_ok = true;
                }
                if status == "ok" && bash_id.as_ref() == Some(&call_id) && output_summary.contains("Only query is implemented") {
                    bash_ok = true;
                }
            }
            Mapped::Unknown { kind } => unknown.push(kind),
            Mapped::ToolStarted { .. } | Mapped::Bookkeeping => {}
        }
    }
    Ok(ToolStatus { read_ok, bash_ok, unknown })
}

fn wait_tools(lines: &Mutex<Vec<String>>, too_big: &AtomicBool, budget: Duration) -> Result<(), String> {
    let deadline = Instant::now() + budget;
    loop {
        if too_big.load(Ordering::Relaxed) {
            return Err("pi line exceeded 1 MiB".to_owned());
        }
        let stored = lines.lock().unwrap_or_else(|err| err.into_inner()).clone();
        let status = tool_status(&stored)?;
        if !status.unknown.is_empty() {
            return Err(format!("unknown pi event {}", status.unknown[0]));
        }
        if status.read_ok && status.bash_ok {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("tool events incomplete read={} bash={}", status.read_ok, status.bash_ok));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_match(
    lines: &Mutex<Vec<String>>,
    too_big: &AtomicBool,
    budget: Duration,
    pred: impl Fn(&str) -> Result<bool, String>,
) -> Result<(), String> {
    let deadline = Instant::now() + budget;
    loop {
        if too_big.load(Ordering::Relaxed) {
            return Err("pi line exceeded 1 MiB".to_owned());
        }
        let stored = lines.lock().unwrap_or_else(|err| err.into_inner()).clone();
        for line in &stored {
            if pred(line)? {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err("timed out waiting for a pi response".to_owned());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn summarize(lines: &[String], failure: &str, exit: Option<i32>) -> Json {
    let mut tools = Vec::new();
    let mut initialize_ok = false;
    let mut prompt_started = false;
    let mut settled = false;
    for line in lines {
        if events::response_ok(line, "init-1").ok() == Some(true) {
            initialize_ok = true;
        }
        if events::prompt_started(line).ok() == Some(true) {
            prompt_started = true;
        }
        if line.contains("\"type\":\"agent_settled\"") || line.contains("\"type\": \"agent_settled\"") {
            settled = true;
        }
        match events::map_line(line) {
            Ok(Mapped::ToolStarted { name, call_id, args_summary }) => {
                tools.push(json::obj(vec![
                    ("event", json::string("tool_call_started")),
                    ("name", json::string(name)),
                    ("call_id", json::string(call_id)),
                    ("args_summary", json::string(args_summary)),
                ]));
            }
            Ok(Mapped::ToolFinished { call_id, status, output_summary }) => {
                tools.push(json::obj(vec![
                    ("event", json::string("tool_call_finished")),
                    ("call_id", json::string(call_id)),
                    ("status", json::string(status)),
                    ("output_summary", json::string(output_summary)),
                ]));
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    let status = tool_status(lines).unwrap_or_else(|_| ToolStatus {
        read_ok: false,
        bash_ok: false,
        unknown: vec!["unparsed".to_owned()],
    });
    let read_ok = status.read_ok;
    let bash_ok = status.bash_ok;
    let unknown: Vec<Json> = status.unknown.into_iter().map(json::string).collect();
    let ok = failure.is_empty() && initialize_ok && prompt_started && read_ok && bash_ok && unknown.is_empty();
    json::obj(vec![
        ("ok", json::boolean(ok)),
        ("kind", json::string("installation-probe")),
        ("qa_complete", json::boolean(false)),
        ("pi_version", json::string(PI_VERSION)),
        ("initialize", json::string("get_state")),
        ("initialize_ok", json::boolean(initialize_ok)),
        ("prompt_started", json::boolean(prompt_started)),
        ("read_ok", json::boolean(read_ok)),
        ("bash_help_ok", json::boolean(bash_ok)),
        ("no_mcp", json::boolean(true)),
        ("marker_in_argv", json::boolean(false)),
        ("agent_settled", json::boolean(settled)),
        ("process_exit", json::string(exit.map(|code| code.to_string()).unwrap_or_else(|| "none".to_owned()))),
        ("failure", json::string(failure)),
        ("unknown", json::array(unknown)),
        ("tools", json::array(tools)),
    ])
}

fn send(stdin: &mut impl Write, line: &str) -> Result<(), String> {
    stdin.write_all(line.as_bytes()).map_err(|err| format!("stdin: {err}"))?;
    stdin.write_all(b"\n").map_err(|err| format!("stdin: {err}"))?;
    stdin.flush().map_err(|err| format!("stdin: {err}"))?;
    Ok(())
}

fn rpc_argv(session: &Path) -> Result<Vec<String>, String> {
    let session = path_text(session)?;
    Ok(vec![
        "--mode".to_owned(),
        "rpc".to_owned(),
        "--session-dir".to_owned(),
        session,
        "--provider".to_owned(),
        "grok".to_owned(),
        "--model".to_owned(),
        "grok-4.7".to_owned(),
        "--no-extensions".to_owned(),
        "--no-mcp".to_owned(),
        "--no-skills".to_owned(),
        "--no-prompt-templates".to_owned(),
        "--no-context-files".to_owned(),
    ])
}

fn write_models(agent: &Path, port: u16) -> Result<(), String> {
    let body = format!(
        "{{\"providers\":{{\"grok\":{{\"baseUrl\":\"http://127.0.0.1:{port}/v1\",\"api\":\"openai-completions\",\"apiKey\":\"{PUBLIC_MARKER}\",\"models\":[{{\"id\":\"grok-4.7\",\"name\":\"grok-4.7\",\"reasoning\":false,\"input\":[\"text\"],\"contextWindow\":131072,\"maxTokens\":8192,\"cost\":{{\"input\":0,\"output\":0,\"cacheRead\":0,\"cacheWrite\":0}}}}]}}}}}}\n"
    );
    manifest::write_bytes(&agent.join("models.json"), body.as_bytes())
}

fn command_output(
    program: &Path,
    args: &[&str],
    work: &Path,
    home: &Path,
    runtime_dir: &Path,
    agent: &Path,
) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(work)
        .env_clear()
        .env("HOME", home)
        .env("PATH", runtime_dir)
        .env("TERM", "dumb")
        .env("PI_CODING_AGENT_DIR", agent)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("version: {err}"))?;
    if !output.status.success() {
        return Err(format!("version exit {}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|_| "version stdout is not utf-8".to_owned())
}

fn wait_port(path: &Path, budget: Duration) -> Result<u16, String> {
    let started = Instant::now();
    let err_path = path.with_extension("err");
    loop {
        if let Ok(text) = fs::read_to_string(&err_path)
            && !text.trim().is_empty()
        {
            return Err(format!("provider: {}", text.trim()));
        }
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(port) = text.trim().parse::<u16>()
            && port != 0
        {
            return Ok(port);
        }
        if started.elapsed() >= budget {
            return Err("provider port did not appear".to_owned());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_child(child: &mut std::process::Child, budget: Duration) -> Option<i32> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) if started.elapsed() >= budget => {
                let _ = child.kill();
                return child.wait().ok().and_then(|status| status.code());
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
}

fn read_stdout(stdout: impl Read, raw_path: &Path, lines: Arc<Mutex<Vec<String>>>, too_big: Arc<AtomicBool>) {
    let mut reader = BufReader::new(stdout);
    let mut raw = File::create(raw_path).ok();
    loop {
        let mut buf = Vec::new();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) if buf.len() > LINE_LIMIT => {
                too_big.store(true, Ordering::Relaxed);
                break;
            }
            Ok(_) => {
                if let Some(file) = raw.as_mut() {
                    let _ = file.write_all(&buf);
                }
                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_owned();
                if line.is_empty() {
                    continue;
                }
                lines.lock().unwrap_or_else(|err| err.into_inner()).push(line);
            }
            Err(_) => break,
        }
    }
}

fn read_stderr(mut stderr: impl Read, path: &Path) {
    let mut buf = Vec::new();
    let _ = stderr.read_to_end(&mut buf);
    if buf.len() > LINE_LIMIT {
        buf.truncate(LINE_LIMIT);
    }
    let _ = fs::write(path, buf);
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn path_text(path: &Path) -> Result<String, String> {
    path.to_str().map(str::to_owned).ok_or_else(|| "path is not utf-8".to_owned())
}

pub fn parse_args(args: &[String]) -> Result<Parsed, String> {
    if args.len() != 10 {
        return Err("usage: kit probe OUT PI RUNTIME OO PACK FACTS CJK HANDOFF VOLUME MOUNTPOINT".to_owned());
    }
    Ok(Parsed {
        out: PathBuf::from(&args[0]),
        pi: PathBuf::from(&args[1]),
        runtime: PathBuf::from(&args[2]),
        oo: PathBuf::from(&args[3]),
        pack: PathBuf::from(&args[4]),
        facts: PathBuf::from(&args[5]),
        cjk: PathBuf::from(&args[6]),
        handoff: PathBuf::from(&args[7]),
        volume: args[8].clone(),
        mountpoint: args[9].clone(),
    })
}

pub struct Parsed {
    pub out: PathBuf,
    pub pi: PathBuf,
    pub runtime: PathBuf,
    pub oo: PathBuf,
    pub pack: PathBuf,
    pub facts: PathBuf,
    pub cjk: PathBuf,
    pub handoff: PathBuf,
    pub volume: String,
    pub mountpoint: String,
}
