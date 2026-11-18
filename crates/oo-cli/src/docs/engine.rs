//! The engine as a host child process (L2 simplification of 10 §8.6 point 3).
//!
//! The spec has `oo serve` start the docs engine container with
//! `docker compose`. For L2 the engine runs as a child process of the daemon
//! instead, with the same contract: it listens on a loopback port, reads its
//! token from a 0600 file and is restarted when it exits while the daemon
//! lives (the proxy picks up the new address). The compose and container path
//! (`compose.rs`, image digests, `compose.override.yaml`, the docs-db PKI)
//! stays for a later card. Postgres is external: the engine connects to the
//! URLs in `OO_DOCS_DATABASE_URL` (runtime login) and optionally
//! `OO_DOCS_ADMIN_DATABASE_URL` (installs the schema).

use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::proxy::EngineLink;

/// Name of the engine binary.
pub const ENGINE_BIN: &str = "oo-docs-engine";
/// Time the engine gets to print its `listening` line.
pub const START_TIMEOUT: Duration = Duration::from_secs(120);

/// Find the engine binary: `OO_DOCS_ENGINE_BIN`, else next to this executable.
pub fn find_engine_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OO_DOCS_ENGINE_BIN") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let me = std::env::current_exe().ok()?;
    let near = me.parent()?.join(ENGINE_BIN);
    near.is_file().then_some(near)
}

/// How to run the engine.
#[derive(Clone)]
pub struct EngineSpec {
    pub exe: PathBuf,
    /// Engine state directory (`docs.json`, `cas/`).
    pub state_dir: PathBuf,
    pub database_url: String,
    pub admin_database_url: Option<String>,
    /// File holding the engine token.
    pub token_file: PathBuf,
    /// Where the engine's stderr goes (0600, appended).
    pub log_file: PathBuf,
}

fn open_log(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

/// Start one engine process; returns it with the address it announced.
pub fn start_once(spec: &EngineSpec) -> Result<(Child, SocketAddr), String> {
    let log = open_log(&spec.log_file).map_err(|e| format!("engine log: {e}"))?;
    let mut cmd = Command::new(&spec.exe);
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("OO_DOCS_LISTEN", "127.0.0.1:0")
        .env("OO_DOCS_STATE_DIR", &spec.state_dir)
        .env("OO_DOCS_DATABASE_URL", &spec.database_url)
        .env("OO_DOCS_ENGINE_TOKEN_FILE", &spec.token_file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    if let Some(admin) = &spec.admin_database_url {
        cmd.env("OO_DOCS_ADMIN_DATABASE_URL", admin);
    }
    if let Some(tmp) = std::env::var_os("TMPDIR") {
        cmd.env("TMPDIR", tmp);
    }
    // The cloud entry (B-S12-01) and a Docker-free parser sidecar (B-S14-01, a process the operator runs on
    // loopback) are opt in by the operator's environment and nothing else.
    for k in [
        "OO_DOCS_CLOUD_LISTEN",
        "OO_DOCS_CLOUD_HOSTS",
        "OO_DOCS_PARSER_URL",
        "OO_DOCS_PARSER_TOKEN_FILE",
        // The model stays off without an egress script (10 2.7); `oo serve` forwards it like the parser.
        "OO_DOCS_MODEL_EGRESS",
        "OO_DOCS_PYTHON",
    ] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    let mut child = cmd.spawn().map_err(|e| format!("engine spawn: {e}"))?;
    let stdout = child.stdout.take().ok_or("engine has no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(Ok(line)) = lines.next() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line)
                && v["event"] == "listening"
                && let Some(a) = v["addr"]
                    .as_str()
                    .and_then(|a| a.parse::<SocketAddr>().ok())
            {
                let _ = tx.send(a);
                break;
            }
        }
        // Keep draining so the engine never blocks on a full pipe.
        for _ in lines {}
    });
    let started = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(addr) => return Ok((child, addr)),
            Err(_) => {
                if let Ok(Some(status)) = child.try_wait() {
                    return Err(format!("engine exited during startup ({status})"));
                }
                if started.elapsed() > START_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("engine did not start in time".into());
                }
            }
        }
    }
}

/// `kill -<signal> <pid>` through the system tool (the crate forbids unsafe).
pub fn signal(pid: u32, name: &str) -> bool {
    Command::new("kill")
        .arg(format!("-{name}"))
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Is `pid` alive and an engine process?
pub fn is_engine_pid(pid: u32) -> bool {
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains(ENGINE_BIN))
}

/// Stop an engine a crashed daemon left behind.
pub fn reap_stale(pid_file: &Path) {
    if let Some(pid) = std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        && is_engine_pid(pid)
    {
        signal(pid, "TERM");
        std::thread::sleep(Duration::from_millis(500));
        if is_engine_pid(pid) {
            signal(pid, "KILL");
        }
    }
}

/// The running supervisor.
pub struct Supervisor {
    stop: Arc<AtomicBool>,
    child_pid: Arc<Mutex<Option<u32>>>,
    thread: Option<JoinHandle<()>>,
}

impl Supervisor {
    /// Keep an engine running for as long as the supervisor lives. `first` is
    /// the already started engine; later restarts use `spec`.
    pub fn spawn(
        spec: EngineSpec,
        first: (Child, SocketAddr),
        link: Arc<EngineLink>,
        pid_file: PathBuf,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let child_pid = Arc::new(Mutex::new(None));
        let (s2, p2) = (stop.clone(), child_pid.clone());
        let thread = std::thread::spawn(move || {
            let mut current = Some(first);
            let mut delay = Duration::from_millis(500);
            while !s2.load(Ordering::SeqCst) {
                let (mut child, addr) = match current.take() {
                    Some(c) => c,
                    None => match start_once(&spec) {
                        Ok(c) => c,
                        Err(_) => {
                            std::thread::sleep(delay);
                            delay = (delay * 2).min(Duration::from_secs(10));
                            continue;
                        }
                    },
                };
                delay = Duration::from_millis(500);
                *p2.lock().unwrap_or_else(|p| p.into_inner()) = Some(child.id());
                let _ = std::fs::write(&pid_file, child.id().to_string());
                link.set(Some(addr));
                let _ = child.wait();
                link.set(None);
                *p2.lock().unwrap_or_else(|p| p.into_inner()) = None;
                if !s2.load(Ordering::SeqCst) {
                    std::thread::sleep(delay);
                }
            }
        });
        Self {
            stop,
            child_pid,
            thread: Some(thread),
        }
    }

    /// Pid of the running engine, if any.
    pub fn pid(&self) -> Option<u32> {
        *self.child_pid.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Stop restarting, ask the engine to stop (SIGTERM, then SIGKILL after
    /// `grace`) and wait for it.
    pub fn stop(mut self, grace: Duration) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(pid) = self.pid() {
            signal(pid, "TERM");
            let until = Instant::now() + grace;
            while Instant::now() < until && is_engine_pid(pid) {
                std::thread::sleep(Duration::from_millis(100));
            }
            if is_engine_pid(pid) {
                signal(pid, "KILL");
            }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
