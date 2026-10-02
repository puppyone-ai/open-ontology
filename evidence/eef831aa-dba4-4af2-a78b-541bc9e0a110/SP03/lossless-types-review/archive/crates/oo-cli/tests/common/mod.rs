//! Loopback oracle for the `oo` binary.
//!
//! The server records one HTTP/1.1 request and writes a scripted response.
//! It is not a Gateway and it does not open PostgreSQL.

#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const TOKEN: &str = "TkA095cliExternal9f3c7e1b";
pub const CANARY: &str = "OOTokenCanaryA095NotRead999";

#[derive(Clone, Debug)]
pub struct Hit {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Hit {
    pub fn one<'a>(&'a self, name: &str) -> Option<&'a str> {
        let mut found = None;
        for (header, value) in &self.headers {
            if header == name {
                if found.is_some() {
                    return None;
                }
                found = Some(value.as_str());
            }
        }
        found
    }
}

pub enum Action {
    Raw(Vec<u8>),
    Stall { delay: Duration },
}

pub struct Oracle {
    pub port: u16,
    hits: Arc<Mutex<Vec<Hit>>>,
    pub count: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Oracle {
    pub async fn spawn<F>(plan: F) -> Self
    where
        F: Fn(usize, &Hit) -> Action + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let hits = Arc::new(Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let stored = Arc::clone(&hits);
        let counted = Arc::clone(&count);
        let plan = Arc::new(plan);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let _ = sock.set_nodelay(true);
                let Ok(hit) = read_hit(&mut sock).await else {
                    continue;
                };
                let action = {
                    let mut guard = stored.lock().expect("hits");
                    let action = plan(guard.len(), &hit);
                    guard.push(hit);
                    counted.store(guard.len(), Ordering::SeqCst);
                    action
                };
                match action {
                    Action::Raw(bytes) => {
                        let _ = sock.write_all(&bytes).await;
                        let _ = sock.flush().await;
                    }
                    Action::Stall { delay } => {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        });
        Self {
            port,
            hits,
            count,
            task,
        }
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    pub fn hits(&self) -> Vec<Hit> {
        self.hits.lock().expect("hits").clone()
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn json_response(status: u16, reason: &str, body: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut out =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n").into_bytes();
    for (name, value) in extra {
        out.extend(format!("{name}: {value}\r\n").into_bytes());
    }
    out.extend(
        format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes(),
    );
    out.extend_from_slice(body.as_bytes());
    out
}

pub async fn read_hit<R>(sock: &mut R) -> std::io::Result<Hit>
where
    R: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let sep = loop {
        let read = sock.read(&mut tmp).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof before headers",
            ));
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 65_536 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "headers too large",
            ));
        }
    };
    let head = std::str::from_utf8(&buf[..sep])
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "header bytes"))?;
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "header line",
            ));
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let want = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    if want > 2_000_000 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request body cap",
        ));
    }
    let mut body = buf[sep + 4..].to_vec();
    while body.len() < want {
        let read = sock.read(&mut tmp).await?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..read]);
    }
    if body.len() > want {
        body.truncate(want);
    }
    Ok(Hit {
        method,
        path,
        headers,
        body,
    })
}

pub struct Door {
    pub port: u16,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Door {
    pub fn open() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("door");
        listener.set_nonblocking(true).expect("nonblock");
        let port = listener.local_addr().expect("addr").port();
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&hits);
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok(_) => {
                        seen.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            port,
            hits,
            stop,
            thread: Some(thread),
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

impl Drop for Door {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct Scratch {
    pub dir: PathBuf,
    pub token_path: PathBuf,
    pub home: PathBuf,
}

impl Scratch {
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "oo-cli-a095-path-canary-a095-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        let token_path = dir.join("owned-token");
        std::fs::write(&token_path, format!("{TOKEN}\n")).expect("token");
        let mut perms = std::fs::metadata(&token_path).expect("meta").permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&token_path, perms).expect("mode");
        let decoy = dir.join("token");
        std::fs::write(&decoy, CANARY).expect("decoy");
        std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o644))
            .expect("decoy mode");
        Self {
            home: dir.join("missing-home"),
            dir,
            token_path,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub struct Spec {
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub env: Vec<(String, String)>,
    pub dir: PathBuf,
    pub home: PathBuf,
    pub drop_stdout_after: Option<Arc<AtomicUsize>>,
    pub timeout: Duration,
}

impl Spec {
    pub fn new(scratch: &Scratch, args: Vec<String>) -> Self {
        Self {
            args,
            stdin: Vec::new(),
            env: Vec::new(),
            dir: scratch.dir.clone(),
            home: scratch.home.clone(),
            drop_stdout_after: None,
            timeout: Duration::from_secs(10),
        }
    }
}

pub struct Outcome {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub async fn run(spec: Spec) -> Outcome {
    tokio::task::spawn_blocking(move || run_blocking(spec))
        .await
        .expect("oo thread")
}

fn run_blocking(spec: Spec) -> Outcome {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_oo"));
    cmd.args(&spec.args)
        .env_clear()
        .env("OO_TOKEN", CANARY)
        .env("HOME", &spec.home)
        .current_dir(&spec.dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().expect("spawn oo");
    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");
    let stdin_bytes = spec.stdin;
    let stdin_thread = std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(&stdin_bytes);
    });
    if let Some(hits) = spec.drop_stdout_after {
        let started = std::time::Instant::now();
        while hits.load(Ordering::SeqCst) == 0 && started.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(100));
        drop(stdout);
        let status = wait_deadline(&mut child, spec.timeout);
        let mut err = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::BufReader::new(stderr), &mut err);
        let _ = stdin_thread.join();
        return Outcome {
            code: status.code().unwrap_or(1),
            stdout: Vec::new(),
            stderr: err,
        };
    }
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::BufReader::new(stdout), &mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::BufReader::new(stderr), &mut buf);
        buf
    });
    let status = wait_deadline(&mut child, spec.timeout);
    let _ = stdin_thread.join();
    Outcome {
        code: status.code().unwrap_or(1),
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
    }
}

fn wait_deadline(child: &mut std::process::Child, timeout: Duration) -> std::process::ExitStatus {
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            return child.wait().expect("kill wait");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn assert_secret_free(bytes: &[u8], extra: &[&str]) {
    let text = String::from_utf8_lossy(bytes);
    assert!(!text.contains(TOKEN), "bearer reached the cli output");
    assert!(
        !text.contains(CANARY),
        "OO_TOKEN canary reached the cli output"
    );
    assert!(!text.contains("path-canary-a095"), "path diagnostic");
    assert!(!text.contains("Bearer "), "authorization prefix");
    for secret in extra {
        assert!(!text.contains(secret), "secret {secret} reached the output");
    }
}

pub fn sdk_tls(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/sdk/json-query/fixtures/tls")
        .join(name)
}

pub fn pem_block(pem: &str, label: &str) -> Vec<u8> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = pem.find(&begin).expect("pem begin") + begin.len();
    let stop = pem[start..].find(&end).expect("pem end") + start;
    let encoded: String = pem[start..stop]
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    b64_decode(&encoded).expect("pem base64")
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    if bytes.len() & 3 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|byte| **byte == b'=').count();
        if pad > 2 {
            return None;
        }
        let first = value(chunk[0])?;
        let second = value(chunk[1])?;
        let third = if chunk[2] == b'=' {
            0
        } else {
            value(chunk[2])?
        };
        let fourth = if chunk[3] == b'=' {
            0
        } else {
            value(chunk[3])?
        };
        out.push((first << 2) | (second >> 4));
        if pad < 2 {
            out.push((second << 4) | (third >> 2));
        }
        if pad < 1 {
            out.push((third << 6) | fourth);
        }
    }
    Some(out)
}
