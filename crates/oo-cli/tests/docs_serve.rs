//! `oo serve` as a process: state, tokens, guards, proxy, restart, shutdown.
//! The engine is a stand-in script named `oo-docs-engine`, so no database is
//! needed; the real engine runs in the L2 end-to-end script.
#![cfg(feature = "docs")]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const FAKE_ENGINE: &str = r#"#!/usr/bin/env python3
import json, os, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
tok = open(os.environ["OO_DOCS_ENGINE_TOKEN_FILE"]).read().strip()
class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"
    def log_message(self, *a): pass
    def do_GET(self):
        if self.headers.get("Authorization") != "Bearer " + tok:
            self.send_response(401); self.end_headers(); return
        if self.path.startswith("/v1/docs/events"):
            self.send_response(200); self.send_header("Content-Type", "text/event-stream"); self.end_headers()
            for i in (1, 2, 3):
                self.wfile.write(("id: %d\ndata: {\"seq\":%d}\n\n" % (i, i)).encode()); self.wfile.flush()
            return
        body = json.dumps({"status": "ok", "pid": os.getpid(), "public": self.headers.get("X-OO-Public-Host")}).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
srv = ThreadingHTTPServer(("127.0.0.1", 0), H)
print(json.dumps({"event": "listening", "addr": "127.0.0.1:%d" % srv.server_address[1]}), flush=True)
srv.serve_forever()
"#;

fn oo() -> &'static str {
    env!("CARGO_BIN_EXE_oo")
}

struct Fx {
    dir: PathBuf,
    state: PathBuf,
    engine: PathBuf,
}

fn fx(name: &str) -> Fx {
    let dir = std::env::temp_dir().join(format!("oo-docs-serve-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let state = dir.join("state");
    let engine = dir.join("oo-docs-engine");
    fs::write(&engine, FAKE_ENGINE).unwrap();
    fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
    Fx { dir, state, engine }
}

impl Fx {
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(oo());
        c.args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("OO_DOCS_ENGINE_BIN", &self.engine)
            .env("OO_DOCS_DATABASE_URL", "postgres://unused@127.0.0.1:1/none")
            .arg("--state-dir")
            .arg(&self.state);
        c
    }

    fn serve(&self) -> (Daemon, serde_json::Value) {
        let mut child = self
            .cmd(&["serve", "--json", "--port", "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut line)
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(line.trim())
            .unwrap_or_else(|e| panic!("serve.json line {line:?}: {e}"));
        (Daemon(child), v)
    }
}

/// Stops the daemon (SIGTERM, so it takes its engine along) even when an
/// assertion fails.
struct Daemon(Child);

impl Daemon {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn wait(&mut self) -> std::process::ExitStatus {
        self.0.wait().unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = Command::new("kill")
                .args(["-TERM", &self.0.id().to_string()])
                .status();
            let _ = self.0.wait();
        }
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn http(port: u64, head_lines: &[&str]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port as u16)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut req = head_lines.join("\r\n");
    req.push_str("\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn get(port: u64, token: Option<&str>, path: &str, extra: &[&str]) -> String {
    let line = format!("GET {path} HTTP/1.1");
    let host = format!("Host: 127.0.0.1:{port}");
    let auth = token.map(|t| format!("Authorization: Bearer {t}"));
    let mut lines: Vec<&str> = vec![&line, &host];
    if let Some(a) = &auth {
        lines.push(a);
    }
    lines.extend_from_slice(extra);
    http(port, &lines)
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().mode() & 0o777
}

fn pid_alive(pid: u64) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn state_tokens_guards_proxy_restart_and_shutdown() {
    let f = fx("life");
    let (mut child, info) = f.serve();
    let port = info["port"].as_u64().unwrap();
    assert_eq!(info["protocol"], "oo.docs.serve/v1");
    assert_eq!(info["api_version"], "1.0");
    assert_eq!(info["pid"].as_u64().unwrap(), u64::from(child.id()));

    // Private state: 0700 directory, 0600 token and serve.json.
    assert_eq!(mode(&f.state), 0o700);
    let token_file = PathBuf::from(info["token_file"].as_str().unwrap());
    assert_eq!(mode(&token_file), 0o600);
    assert_eq!(mode(&f.state.join("serve.json")), 0o600);
    assert_eq!(mode(&f.state.join("engine.token")), 0o600);
    let token = fs::read_to_string(&token_file).unwrap();
    assert!(token.starts_with("oodt_") && token.len() == 48, "{token}");
    let engine_token = fs::read_to_string(f.state.join("engine.token")).unwrap();
    assert!(engine_token.starts_with("oodi_"));

    // Guards, in the engine's order.
    let r = get(
        port,
        Some(&token),
        "/v1/docs/health",
        &["Origin: http://evil.example"],
    );
    assert!(r.starts_with("HTTP/1.1 403"), "{r}");
    let r = http(
        port,
        &[
            "GET /v1/docs/health HTTP/1.1",
            "Host: evil.example",
            &format!("Authorization: Bearer {token}"),
        ],
    );
    assert!(r.starts_with("HTTP/1.1 400"), "{r}");
    let r = get(port, None, "/v1/docs/health", &[]);
    assert!(r.starts_with("HTTP/1.1 401"), "{r}");
    let r = get(port, Some("oodt_wrong"), "/v1/docs/health", &[]);
    assert!(r.starts_with("HTTP/1.1 401"), "{r}");
    assert!(get(port, None, "/livez", &[]).starts_with("HTTP/1.1 204"));

    // Proxied: the engine saw the engine token, not ours, and the public host.
    let r = get(port, Some(&token), "/v1/docs/health", &[]);
    assert!(
        r.starts_with("HTTP/1.0 200"),
        "engine response passes verbatim: {r}"
    );
    assert!(
        r.contains(&format!("\"public\": \"127.0.0.1:{port}\"")),
        "{r}"
    );
    assert!(!r.contains(&token));
    let engine_pid = serde_json::from_str::<serde_json::Value>(r.split("\r\n\r\n").nth(1).unwrap())
        .unwrap()["pid"]
        .as_u64()
        .unwrap();
    let r = get(port, Some(&token), "/v1/docs/events?since=0", &[]);
    assert!(r.contains("id: 1") && r.contains("id: 3"), "{r}");

    // A second daemon on the same state does not start; it prints the first.
    let out = f.cmd(&["serve", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let again: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {:?}", out.stdout));
    assert_eq!(again["pid"], info["pid"]);
    let st = f.cmd(&["serve", "--status", "--json"]).output().unwrap();
    assert_eq!(st.status.code(), Some(0));

    // Kill the engine: the proxy says unavailable, then recovers by itself.
    assert!(
        Command::new("kill")
            .args(["-KILL", &engine_pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
    wait_until("engine death seen", || {
        get(port, Some(&token), "/v1/docs/drives", &[]).starts_with("HTTP/1.1 503")
    });
    let h = get(port, Some(&token), "/v1/docs/health", &[]);
    assert!(
        h.starts_with("HTTP/1.1 200") && h.contains("unavailable")
            || h.contains("\"status\": \"ok\""),
        "{h}"
    );
    wait_until("engine restarted", || {
        get(port, Some(&token), "/v1/docs/health", &[]).contains("\"status\": \"ok\"")
    });
    assert_eq!(
        fs::read_to_string(&token_file).unwrap(),
        token,
        "token is stable within one run"
    );

    // SIGTERM: clean exit, secrets gone, engine gone.
    let engine_now = fs::read_to_string(f.state.join("engine.pid"))
        .unwrap()
        .trim()
        .parse::<u64>()
        .unwrap();
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let status = child.wait();
    assert_eq!(status.code(), Some(0));
    for name in ["serve.json", "serve.token", "engine.token", "engine.pid"] {
        assert!(!f.state.join(name).exists(), "{name} must be removed");
    }
    wait_until("engine stopped", || !pid_alive(engine_now));

    // Tokens rotate at every start.
    let (mut child2, info2) = f.serve();
    let token2 = fs::read_to_string(info2["token_file"].as_str().unwrap()).unwrap();
    assert_ne!(token, token2);
    assert!(
        Command::new("kill")
            .args(["-TERM", &child2.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(child2.wait().code(), Some(0));
}

#[test]
fn startup_failures_use_the_documented_exit_codes() {
    let f = fx("exit");
    // 5: state directory with loose permissions.
    fs::create_dir_all(&f.state).unwrap();
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(f.cmd(&["serve"]).status().unwrap().code(), Some(5));
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();

    // 4: engine binary missing.
    let mut c = f.cmd(&["serve"]);
    c.env("OO_DOCS_ENGINE_BIN", f.dir.join("nope"));
    assert_eq!(c.status().unwrap().code(), Some(4));

    // 6: port taken.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port().to_string();
    let st = f.cmd(&["serve", "--port", &port]).status().unwrap();
    assert_eq!(st.code(), Some(6));
    assert!(
        !f.state.join("serve.token").exists(),
        "failed start leaves no token"
    );

    // 7 is the engine start timeout (120 s); an engine that exits at once is exit 1.
    fs::write(&f.engine, "#!/bin/sh\nexit 3\n").unwrap();
    assert_eq!(f.cmd(&["serve"]).status().unwrap().code(), Some(1));

    // 2: arguments.
    assert_eq!(
        f.cmd(&["serve", "--port", "99999"])
            .status()
            .unwrap()
            .code(),
        Some(2)
    );
}
