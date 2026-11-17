//! Shared helpers of the `sidecar_*` integration tests: start the sidecar
//! container (or attach to one named by `OO_SIDECAR_ADDR`/`OO_SIDECAR_TOKEN`),
//! and run docker commands. Containers carry the lane label and are removed on
//! drop. The image is `OO_DOCS_PARSER_IMAGE`, default `oo-docs-parser:dev`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use oo_docs::parse::sidecar::{SidecarClient, SidecarConfig};

pub const LANE: &str = "super-ontology-be2";

pub fn testing_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testing/super-ontology")
}

pub fn golden(rel: &str) -> Vec<u8> {
    std::fs::read(testing_root().join("golden").join(rel)).unwrap()
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(testing_root().join("fixtures/files").join(name)).unwrap()
}

pub fn docker(args: &[&str]) -> (bool, String) {
    let out = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("docker is required for the sidecar tests");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text.trim().to_owned())
}

/// A free loopback port (bound and released; a race is possible but unlikely).
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free port")
}

pub struct Sidecar {
    pub name: Option<String>,
    pub addr: String,
    pub token: String,
}

impl Sidecar {
    pub fn client(&self) -> SidecarClient {
        SidecarClient::new(SidecarConfig::new(self.addr.clone(), self.token.clone()))
    }

    pub fn config(&self) -> SidecarConfig {
        SidecarConfig::new(self.addr.clone(), self.token.clone())
    }

    /// Start a container with `workers` parse workers.
    pub fn start(workers: u32) -> Sidecar {
        if let (Ok(addr), Ok(token)) = (
            std::env::var("OO_SIDECAR_ADDR"),
            std::env::var("OO_SIDECAR_TOKEN"),
        ) {
            return Sidecar {
                name: None,
                addr,
                token,
            };
        }
        let image =
            std::env::var("OO_DOCS_PARSER_IMAGE").unwrap_or_else(|_| "oo-docs-parser:dev".into());
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let name = format!("oo-be2-sidecar-{}-{nanos}", std::process::id());
        let token = format!("test-token-{nanos}");
        let label = format!("oo.lane={LANE}");
        // A fixed host port, so the address survives `docker kill` + `start`.
        let port = free_port();
        let publish = format!("127.0.0.1:{port}:7481");
        let workers_env = format!("OO_PARSER_WORKERS={workers}");
        let token_env = format!("OO_PARSER_TOKEN={token}");
        let (ok, out) = docker(&[
            "run",
            "-d",
            "--name",
            &name,
            "--label",
            &label,
            "-p",
            &publish,
            "--read-only",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            "--memory",
            "3g",
            "--cpus",
            "2",
            "--pids-limit",
            "128",
            "--tmpfs",
            "/tmp:size=256m,mode=1777",
            "--tmpfs",
            "/scratch:size=2g,uid=10001,gid=10001",
            "-e",
            &token_env,
            "-e",
            &workers_env,
            "-e",
            "HOME=/tmp",
            &image,
        ]);
        assert!(ok, "docker run failed: {out}");
        let s = Sidecar {
            name: Some(name),
            addr: format!("127.0.0.1:{port}"),
            token,
        };
        s.wait_healthy(Duration::from_secs(90));
        s
    }

    pub fn wait_healthy(&self, within: Duration) {
        let client = self.client();
        let t0 = Instant::now();
        while t0.elapsed() < within {
            if client.healthy() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let logs = self
            .name
            .as_ref()
            .map(|n| docker(&["logs", "--tail", "30", n]).1);
        panic!("sidecar did not become healthy: {logs:?}");
    }

    /// SIGKILL every parse worker (children of the fork server), keeping the
    /// HTTP process and the fork server alive. Returns how many were killed.
    pub fn kill_workers(&self) -> usize {
        let name = self.name.as_ref().expect("needs an owned container");
        kill_workers_in(name)
    }
}

/// [`Sidecar::kill_workers`] by container name (usable from another thread).
pub fn kill_workers_in(name: &str) -> usize {
    {
        let script = r#"
import os, signal
procs = {}
for d in os.listdir('/proc'):
    if d.isdigit():
        try:
            stat = open(f'/proc/{d}/stat').read()
            ppid = int(stat.rsplit(')', 1)[1].split()[1])
            cmd = open(f'/proc/{d}/cmdline').read().replace('\0', ' ')
            procs[int(d)] = (ppid, cmd)
        except Exception:
            pass
fs = [p for p, (pp, c) in procs.items() if pp == 1 and 'forkserver' in c]
killed = 0
for p, (pp, c) in procs.items():
    if pp in fs and 'resource_tracker' not in c:
        os.kill(p, signal.SIGKILL); killed += 1
print(killed)
"#;
        let (ok, out) = docker(&["exec", name, "python", "-c", script]);
        assert!(ok, "exec failed: {out}");
        out.lines().last().and_then(|l| l.parse().ok()).unwrap_or(0)
    }
}

impl Sidecar {
    /// `docker kill` the container (the main process dies with it).
    pub fn kill(&self) {
        let name = self.name.as_ref().expect("needs an owned container");
        let (ok, out) = docker(&["kill", name]);
        assert!(ok, "kill failed: {out}");
    }

    /// `docker start` it again (same name, same published port).
    pub fn start_again(&self) {
        let name = self.name.as_ref().expect("needs an owned container");
        let (ok, out) = docker(&["start", name]);
        assert!(ok, "start failed: {out}");
    }

    /// Kill workers as soon as one exists (the slot is taken before the child
    /// is forked), giving up after `within`.
    pub fn kill_workers_soon(&self, within: Duration) -> usize {
        let t0 = Instant::now();
        loop {
            let n = self.kill_workers();
            if n > 0 || t0.elapsed() > within {
                return n;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Some(name) = &self.name {
            let _ = docker(&["rm", "-f", name]);
        }
    }
}
