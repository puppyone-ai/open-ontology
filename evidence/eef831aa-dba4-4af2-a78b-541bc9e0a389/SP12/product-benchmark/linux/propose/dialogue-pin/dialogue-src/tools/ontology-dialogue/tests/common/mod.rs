#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::thread;
use std::time::Duration;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

pub const CA_PEM: &str = include_str!("../../../../contracts/sdk/json-query/fixtures/tls/ca.pem");
pub const LOCALHOST_PEM: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/tls/localhost.pem");
pub const LOCALHOST_KEY: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/tls/localhost.key");
pub const BEARER: &str = "bearer-a300-visible-token";
pub const POLICY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const SQL: &str = "SELECT headcount FROM depot.counter ORDER BY headcount";

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(1);

pub fn scratch() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "oo-a300-dialogue-{}-{}",
        std::process::id(),
        SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).expect("scratch");
    path
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn one_line(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    &bytes[..end]
}

pub fn dialogue_bin() -> PathBuf {
    let path = option_env!("CARGO_BIN_EXE_ontology-dialogue")
        .or(option_env!("CARGO_BIN_EXE_ontology_dialogue"))
        .expect("ontology-dialogue test binary");
    PathBuf::from(path)
}

pub fn run_cli(args: &[&str]) -> Output {
    Command::new(dialogue_bin())
        .args(args)
        .output()
        .expect("dialogue cli")
}

pub fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::write(path, bytes).expect("write");
}

pub struct Files {
    pub dir: PathBuf,
}

impl Files {
    pub fn new() -> Self {
        let dir = scratch();
        write(&dir.join("ca.pem"), CA_PEM);
        write(&dir.join("bearer"), format!("{BEARER}\n"));
        Self { dir }
    }

    pub fn question(&self, text: &str) -> PathBuf {
        let path = self.dir.join("question.json");
        let doc = json!({
            "protocol": "oo.ontology-dialogue.question/v1",
            "text": text
        });
        write(&path, serde_json::to_vec(&doc).unwrap());
        path
    }

    pub fn propose_config(&self, origin: &str, question: &Path, pack: &Path) -> String {
        self.propose_config_role(origin, question, pack, "oo_r_north")
    }

    pub fn propose_config_role(
        &self,
        origin: &str,
        question: &Path,
        pack: &Path,
        role: &str,
    ) -> String {
        let doc = json!({
            "protocol": "oo.ontology-dialogue.propose-config/v1",
            "question_file": question.display().to_string(),
            "pack_file": pack.display().to_string(),
            "binding": {
                "role": role,
                "project": "depot",
                "policy_id": POLICY,
                "policy_version": "v1",
                "knowledge_id": "desk",
                "knowledge_version": "v1",
                "as_of": "2026-10-06T00:00:00Z"
            },
            "endpoint": {
                "origin": origin,
                "path": "/v1/chat/completions",
                "model_id": "fixture-model"
            },
            "trust": {
                "ca_file": self.dir.join("ca.pem").display().to_string(),
                "residency_approved": true
            },
            "secret": {
                "bearer_file": self.dir.join("bearer").display().to_string()
            },
            "budget": {
                "max_question_bytes": 8192,
                "max_context_bytes": 65536,
                "max_request_bytes": 65536,
                "max_response_bytes": 16384,
                "max_response_chunks": 32,
                "deadline_ms": 5000,
                "max_prompt_tokens": 1000,
                "max_completion_tokens": 256,
                "max_total_tokens": 2000,
                "max_sql_bytes": 4096
            }
        });
        serde_json::to_string(&doc).unwrap()
    }
}

pub fn manifest_text(role: &str) -> String {
    format!(
        "{{\"knowledge\":{{\"id\":\"desk\",\"version\":\"v1\"}},\"policy\":{{\"id\":\"{POLICY}\",\"version\":\"v1\"}},\"project\":\"depot\",\"protocol\":\"oo.builder.role-pack-manifest/v1\",\"role\":\"{role}\"}}"
    )
}

pub fn pack_artifact(role: &str, table_text: &str) -> (Vec<u8>, String) {
    let manifest = manifest_text(role);
    let files = [
        ("manifest.json", manifest.as_str()),
        ("tables/depot.counter.md", table_text),
    ];
    let mut objects = Vec::new();
    let mut listed = Vec::new();
    let mut total = 0_u64;
    for (path, text) in files {
        let sha = sha256_hex(text.as_bytes());
        let bytes = text.len() as u64;
        total += bytes;
        objects.push(format!(
            "{{\"bytes\":{bytes},\"path\":{},\"sha256\":{}}}",
            json_string(path),
            json_string(&sha)
        ));
        listed.push(json!({
            "path": path,
            "sha256": sha,
            "bytes": bytes,
            "text": text
        }));
    }
    let preimage = format!("[{}]", objects.join(","));
    let digest = sha256_hex(preimage.as_bytes());
    let manifest_sha = sha256_hex(manifest.as_bytes());
    let artifact = json!({
        "protocol": "oo.builder.role-pack/v1",
        "compiler": "oo-builder/role-pack/v1",
        "pack_digest": digest,
        "manifest_sha256": manifest_sha,
        "entry_count": listed.len(),
        "total_bytes": total,
        "files": listed
    });
    (serde_json::to_vec(&artifact).unwrap(), digest)
}

pub fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub fn canonical_object(pairs: &[(&str, &str)]) -> String {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert((*key).to_string(), Value::String((*value).to_string()));
    }
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut out = String::from("{");
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(key));
        out.push(':');
        out.push_str(&json_string(map[*key].as_str().unwrap()));
    }
    out.push('}');
    out
}

pub fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

pub enum Script {
    Sql(String),
    Status(u16),
    EchoBearer,
    Redirect,
}

pub struct Hit {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Hit {
    pub fn one(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

pub struct Mock {
    pub port: u16,
    hits: Arc<Mutex<Vec<Hit>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Mock {
    pub fn spawn(script: Script) -> Self {
        crypto();
        let cert = rustls::pki_types::CertificateDer::from(pem_block(LOCALHOST_PEM, "CERTIFICATE"));
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(pem_block(LOCALHOST_KEY, "PRIVATE KEY")),
        );
        let mut server = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("cert");
        server.alpn_protocols = vec![b"http/1.1".to_vec()];
        let acceptor = TlsAcceptor::from(Arc::new(server));
        let hits = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let hits_task = Arc::clone(&hits);
        let stop_task = Arc::clone(&stop);
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .expect("runtime");
            runtime.block_on(async move {
                let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
                let port = listener.local_addr().expect("addr").port();
                tx.send(port).expect("port");
                loop {
                    if stop_task.load(Ordering::SeqCst) {
                        break;
                    }
                    let accepted =
                        tokio::time::timeout(Duration::from_millis(200), listener.accept()).await;
                    let Ok(Ok((tcp, _))) = accepted else {
                        continue;
                    };
                    let Ok(mut tls) = acceptor.accept(tcp).await else {
                        continue;
                    };
                    let Ok(hit) = read_hit(&mut tls).await else {
                        continue;
                    };
                    let response = match &script {
                        Script::Sql(sql) => {
                            let content = serde_json::json!({
                                "protocol": "oo.ontology-dialogue.sql-candidate/v1",
                                "sql": sql
                            });
                            let content = serde_json::to_string(&content).unwrap();
                            http_json(200, &envelope("stop", &content, 11, 7, 18))
                        }
                        Script::Status(status) => http_json(*status, b"{\"error\":\"no\"}"),
                        Script::EchoBearer => {
                            let body = format!("{{\"echo\":\"{BEARER}\"}}");
                            http_json(200, body.as_bytes())
                        }
                        Script::Redirect => {
                            let mut bytes = Vec::new();
                            bytes.extend_from_slice(
                                b"HTTP/1.1 302 Found\r\nlocation: https://localhost/other\r\ncontent-length: 0\r\n\r\n",
                            );
                            bytes
                        }
                    };
                    hits_task.lock().expect("hits").push(hit);
                    let _ = tls.write_all(&response).await;
                    let _ = tls.flush().await;
                }
            });
        });
        let port = rx.recv_timeout(Duration::from_secs(5)).expect("port");
        Self {
            port,
            hits,
            stop,
            thread: Some(thread),
        }
    }

    pub fn hits(&self) -> Vec<Hit> {
        self.hits.lock().expect("hits").clone()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Clone for Hit {
    fn clone(&self) -> Self {
        Self {
            method: self.method.clone(),
            path: self.path.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
        }
    }
}

fn crypto() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

fn envelope(reason: &str, content: &str, prompt: u64, completion: u64, total: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "choices": [{
            "finish_reason": reason,
            "message": {"role": "assistant", "content": content}
        }],
        "usage": {
            "prompt_tokens": prompt,
            "completion_tokens": completion,
            "total_tokens": total
        }
    }))
    .unwrap()
}

fn http_json(status: u16, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

async fn read_hit(sock: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>) -> std::io::Result<Hit> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 4096];
    let header_end = loop {
        let read = sock.read(&mut tmp).await?;
        if read == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof"));
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(index) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
        if buf.len() > 1_000_000 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "head"));
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let want = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[header_end + 4..].to_vec();
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

pub fn pem_block(pem: &str, label: &str) -> Vec<u8> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = pem.find(&begin).expect("begin") + begin.len();
    let stop = pem[start..].find(&end).expect("end") + start;
    let encoded: String = pem[start..stop]
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    b64(&encoded).expect("pem")
}

fn b64(input: &str) -> Option<Vec<u8>> {
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
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|byte| **byte == b'=').count();
        if pad > 2 {
            return None;
        }
        let first = value(chunk[0])?;
        let second = value(chunk[1])?;
        let third = if chunk[2] == b'=' { 0 } else { value(chunk[2])? };
        let fourth = if chunk[3] == b'=' { 0 } else { value(chunk[3])? };
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

pub fn marker_script(marker: &Path) -> String {
    format!("#!/bin/sh\n/bin/touch {}\nexit 0\n", shell_quote(marker))
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

pub fn write_executable(path: &Path, text: &str) {
    write(path, text);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

pub fn absent(path: &Path) {
    assert!(!path.exists(), "marker was created at {}", path.display());
}
