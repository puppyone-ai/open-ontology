//! Dialogue identity and the explicit development-local bearer exception.
//!
//! The bearer in this file is a synthetic fixture. It is not the private token.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ontology_dialogue::{Fault, propose};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DIGEST: &str = "e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e";
const SHORT: &str = "t0kref";
const SQL: &str = "SELECT count(*) FROM fixture.entry";
const CA_PEM: &str = include_str!("../../../contracts/sdk/json-query/fixtures/tls/ca.pem");

#[test]
fn help_names_the_development_local_exception() {
    let help = run_cli(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    let text = String::from_utf8(help.stdout).expect("help utf8");
    assert!(text.contains("ontology-dialogue propose --config <path>"));
    assert!(text.contains("ontology-dialogue execute --config <path>"));
    assert!(text.contains("approved HTTPS"));
    assert!(text.contains("development_local"));
    assert!(text.contains("http://127.0.0.1"));
    assert!(text.contains("6-byte bearer"));
    assert!(help.stderr.is_empty());
}

#[test]
fn identity_mismatches_are_config_before_any_socket() {
    let server = HttpMock::bind();
    let port = server.port;
    let files = Fixture::new(SHORT);
    let origin = format!("https://127.0.0.1:{port}");
    let cases = [
        (DIGEST.to_ascii_uppercase(), "know_a317".to_string()),
        ("0".repeat(63), "know_a317".to_string()),
        ("a".repeat(65), "know_a317".to_string()),
        ("g".repeat(64), "know_a317".to_string()),
        ("../pol".to_string(), "know_a317".to_string()),
        (String::new(), "know_a317".to_string()),
        ("pol-a317".to_string(), "know_a317".to_string()),
        ("pol\nshop".to_string(), "know_a317".to_string()),
        (DIGEST.to_string(), "know-a317".to_string()),
    ];
    for (policy, knowledge) in &cases {
        let err = propose(&files.config(&origin, "", policy, knowledge, true)).expect_err("config");
        assert_eq!(err, Fault::config(), "{policy} {knowledge}");
    }
    assert_eq!(server.hits(), 0, "identity config opened a socket");
}

#[test]
fn development_local_rejects_other_hosts_and_short_production_bearers() {
    let server = HttpMock::bind();
    let port = server.port;
    let files = Fixture::new(SHORT);
    let loopback = format!("http://127.0.0.1:{port}");
    let cases = [
        (format!("http://localhost:{port}"), "development_local"),
        (format!("http://127.0.0.2:{port}"), "development_local"),
        (format!("https://127.0.0.1:{port}"), "development_local"),
        (loopback.clone(), "production"),
        (loopback.clone(), "localdev"),
        (loopback.clone(), ""),
        (format!("https://127.0.0.1:{port}"), ""),
    ];
    for (origin, transport) in &cases {
        let err = propose(&files.config(origin, transport, DIGEST, "know_a317", *transport != "development_local"))
            .expect_err("config");
        assert_eq!(err, Fault::config(), "{origin} {transport}");
    }
    for length in [5_usize, 7, 15] {
        let fixture = Fixture::new(&"b".repeat(length));
        let err = propose(&fixture.config(&loopback, "development_local", DIGEST, "know_a317", false)).expect_err("length");
        assert_eq!(err, Fault::config(), "length {length}");
    }
    let spaced = Fixture::new("abc de");
    let err = propose(&spaced.config(&loopback, "development_local", DIGEST, "know_a317", false)).expect_err("space");
    assert_eq!(err, Fault::config());
    assert_eq!(server.hits(), 0, "a refused bearer or origin opened a socket");
}

#[test]
fn six_byte_development_local_returns_one_sql_candidate() {
    install_crypto();
    let body = completion(SQL);
    let server = HttpMock::spawn(200, body);
    let files = Fixture::new(SHORT);
    let origin = format!("http://127.0.0.1:{}", server.port);
    let proposal = propose(&files.config(&origin, "development_local", DIGEST, "know_a317", false)).expect("proposal");
    let doc: Value = serde_json::from_slice(&proposal).expect("proposal json");
    assert_eq!(doc["protocol"], "oo.ontology-dialogue.proposal/v1");
    assert_eq!(doc["sql"], SQL);
    assert_eq!(doc["policy_id"], DIGEST);
    assert_eq!(doc["knowledge_id"], "know_a317");
    assert_eq!(doc["model_sql"], "untrusted");
    assert_eq!(doc["authenticated"], false);
    assert_eq!(doc["auto_execute"], false);
    assert_eq!(server.hits(), 1);
    assert!(!proposal.windows(SHORT.len()).any(|window| window == SHORT.as_bytes()));
}

#[test]
fn provider_failure_writes_no_proposal_and_does_not_execute() {
    install_crypto();
    let server = HttpMock::spawn(500, br#"{"error":"no"}"#.to_vec());
    let files = Fixture::new(SHORT);
    let origin = format!("http://127.0.0.1:{}", server.port);
    let config = files.config(&origin, "development_local", DIGEST, "know_a317", false);
    let err = propose(&config).expect_err("provider");
    assert_eq!(err, Fault::provider());
    assert_eq!(server.hits(), 1);
    let path = files.dir.join("config.json");
    fs::write(&path, &config).expect("config");
    let cli = run_cli(&["propose", "--config", path.to_str().expect("path")]);
    assert_eq!(cli.status.code(), Some(1));
    assert_eq!(cli.stderr, b"ontology-dialogue: provider\n");
    assert!(cli.stdout.is_empty(), "provider wrote a proposal");
    assert!(!files.dir.join("approval.json").exists());
}

struct Fixture {
    dir: PathBuf,
    bearer_name: String,
}

impl Fixture {
    fn new(bearer: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "oo-a399-dialogue-{}-{}",
            std::process::id(),
            unique()
        ));
        fs::create_dir_all(&dir).expect("scratch");
        fs::write(dir.join("bearer"), bearer.as_bytes()).expect("bearer");
        fs::write(dir.join("ca.pem"), CA_PEM).expect("ca");
        let question = json!({
            "protocol": "oo.ontology-dialogue.question/v1",
            "text": "How many entry rows are stored?"
        });
        fs::write(dir.join("question.json"), serde_json::to_vec(&question).unwrap()).expect("question");
        fs::write(dir.join("pack.json"), pack_artifact(DIGEST, "know_a317")).expect("pack");
        Self {
            dir,
            bearer_name: "bearer".to_string(),
        }
    }

    fn config(&self, origin: &str, transport: &str, policy: &str, knowledge: &str, with_ca: bool) -> String {
        let mut endpoint = json!({
            "origin": origin,
            "path": "/v1/chat/completions",
            "model_id": "grok-4.7"
        });
        if !transport.is_empty() {
            endpoint["transport"] = json!(transport);
        }
        let ca = if with_ca {
            self.dir.join("ca.pem")
        } else {
            self.dir.join("missing-ca.pem")
        };
        serde_json::to_string(&json!({
            "protocol": "oo.ontology-dialogue.propose-config/v1",
            "question_file": self.dir.join("question.json").display().to_string(),
            "pack_file": self.dir.join("pack.json").display().to_string(),
            "binding": {
                "role": "oo_r_clerk",
                "project": "shop",
                "policy_id": policy,
                "policy_version": "v1",
                "knowledge_id": knowledge,
                "knowledge_version": "v1",
                "as_of": "2026-10-08T00:00:00Z"
            },
            "endpoint": endpoint,
            "trust": {
                "ca_file": ca.display().to_string(),
                "residency_approved": true
            },
            "secret": {
                "bearer_file": self.dir.join(&self.bearer_name).display().to_string()
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
        }))
        .unwrap()
    }
}

fn pack_artifact(policy: &str, knowledge: &str) -> Vec<u8> {
    let manifest = format!(
        "{{\"knowledge\":{{\"id\":{},\"version\":\"v1\"}},\"policy\":{{\"id\":{},\"version\":\"v1\"}},\"project\":\"shop\",\"protocol\":\"oo.builder.role-pack-manifest/v1\",\"role\":\"oo_r_clerk\"}}",
        serde_json::to_string(knowledge).unwrap(),
        serde_json::to_string(policy).unwrap()
    );
    let table = "column entry_count is visible to this role\n";
    let files = [("manifest.json", manifest.as_str()), ("tables/public.entry.md", table)];
    let mut objects = Vec::new();
    let mut listed = Vec::new();
    let mut total = 0_u64;
    for (path, text) in files {
        let sha = sha256_hex(text.as_bytes());
        let bytes = text.len() as u64;
        total += bytes;
        objects.push(format!(
            "{{\"bytes\":{bytes},\"path\":{},\"sha256\":{}}}",
            serde_json::to_string(path).unwrap(),
            serde_json::to_string(&sha).unwrap()
        ));
        listed.push(json!({
            "path": path,
            "sha256": sha,
            "bytes": bytes,
            "text": text
        }));
    }
    let digest = sha256_hex(format!("[{}]", objects.join(",")).as_bytes());
    serde_json::to_vec(&json!({
        "protocol": "oo.builder.role-pack/v1",
        "compiler": "oo-builder/role-pack/v1",
        "pack_digest": digest,
        "manifest_sha256": sha256_hex(manifest.as_bytes()),
        "entry_count": listed.len(),
        "total_bytes": total,
        "files": listed
    }))
    .unwrap()
}

fn completion(sql: &str) -> Vec<u8> {
    let content = serde_json::to_string(&json!({
        "protocol": "oo.ontology-dialogue.sql-candidate/v1",
        "sql": sql
    }))
    .unwrap();
    serde_json::to_vec(&json!({
        "choices": [{
            "finish_reason": "stop",
            "message": {"role": "assistant", "content": content}
        }],
        "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
    }))
    .unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unique() -> u64 {
    use std::sync::atomic::AtomicU64;
    static SEQ: AtomicU64 = AtomicU64::new(1);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

fn install_crypto() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn run_cli(args: &[&str]) -> std::process::Output {
    let path = option_env!("CARGO_BIN_EXE_ontology-dialogue")
        .or(option_env!("CARGO_BIN_EXE_ontology_dialogue"))
        .expect("ontology-dialogue test binary");
    Command::new(path).args(args).output().expect("dialogue cli")
}

struct HttpMock {
    port: u16,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HttpMock {
    fn bind() -> Self {
        Self::spawn(200, completion(SQL))
    }

    fn spawn(status: u16, body: Vec<u8>) -> Self {
        let hits = Arc::new(AtomicUsize::new(0));
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
                    let accepted = tokio::time::timeout(Duration::from_millis(200), listener.accept()).await;
                    let Ok(Ok((mut sock, _))) = accepted else {
                        continue;
                    };
                    hits_task.fetch_add(1, Ordering::SeqCst);
                    let _ = read_http(&mut sock).await;
                    let head = format!(
                        "HTTP/1.1 {status} Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                    let _ = sock.flush().await;
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

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

impl Drop for HttpMock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn read_http(sock: &mut tokio::net::TcpStream) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 2048];
    loop {
        let read = tokio::time::timeout(Duration::from_secs(2), sock.read(&mut tmp))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "head"))??;
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..read]);
        let Some(index) = buf.windows(4).position(|window| window == b"\r\n\r\n") else {
            if buf.len() > 1_000_000 {
                break;
            }
            continue;
        };
        let head = String::from_utf8_lossy(&buf[..index]);
        let length = head.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        });
        if let Some(length) = length {
            if buf.len() - (index + 4) >= length {
                break;
            }
        } else {
            break;
        }
    }
    Ok(())
}

