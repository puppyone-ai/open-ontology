//! Independent `oo query` probe. It spawns the production binary.
//! Expected JSON is handwritten. It does not call CLI render helpers.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, thread};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOKEN: &str = "TkA103ext7c2e9b1";
const CANARY: &str = "CanaryA103NotACredential99";
const SQL: &str = "select 1";

const EMPTY: &str = include_str!("../../../contracts/sdk/json-query/fixtures/query-response-empty.json");
const STANDARD: &str = include_str!("../../../contracts/sdk/json-query/fixtures/query-response.json");
const TRUNCATED: &str = include_str!("../../../contracts/sdk/json-query/fixtures/query-response-truncated.json");
const EXTREMA: &str = include_str!("../../../contracts/cli/query/fixtures/response-extrema.canonical.json");
const HOSTILE: &str = include_str!("../../../contracts/cli/query/fixtures/response-hostile.http.json");
const RATE: &str = include_str!("../../../contracts/sdk/json-query/fixtures/error-rate-limited.json");
const UNAUTH: &str = include_str!("../../../contracts/sdk/json-query/fixtures/error-unauthenticated.json");
const OVERLOADED: &str = include_str!("../../../contracts/sdk/json-query/fixtures/error-overloaded.json");
const TIMEOUT_BODY: &str = include_str!("../../../contracts/sdk/json-query/fixtures/error-timeout.json");
const INTERNAL: &str = include_str!("../../../contracts/sdk/json-query/fixtures/error-internal.json");
const SQL_ERR: &str = include_str!("../../../contracts/sdk/json-query/fixtures/error-sql-parse.json");
const DUPLICATE: &str = include_str!("../../../contracts/sdk/json-query/fixtures/reject-duplicate-key.json");
const UNKNOWN: &str = include_str!("../../../contracts/sdk/json-query/fixtures/reject-unknown-field.json");
const LOCALHOST_PEM: &str = include_str!("../../../contracts/sdk/json-query/fixtures/tls/localhost.pem");
const LOCALHOST_KEY: &str = include_str!("../../../contracts/sdk/json-query/fixtures/tls/localhost.key");
const OTHER_PEM: &str = include_str!("../../../contracts/sdk/json-query/fixtures/tls/other-localhost.pem");
const OTHER_KEY: &str = include_str!("../../../contracts/sdk/json-query/fixtures/tls/other-localhost.key");
const CA_PEM: &str = include_str!("../../../contracts/sdk/json-query/fixtures/tls/ca.pem");

const EXPECTED_STANDARD: &str = concat!(
    r#"{"columns":[{"name":"amount","precision":38,"scale":2,"type":"numeric"}],"duration_ms":12,"freshness":[{"as_of":"2026-10-01T01:12:03Z","lag_ms":0,"state":"live","table":"logistics.receipt"}],"hints":[],"knowledge_version":"oos/v1","policy_version":"type-policy/v1","query_id":"q_abc123","returned_rows":3,"rows":[["12.50"],[null],[true]],"sources":[{"applied_pos":"18446744073709551615","consistency":"transactional","cross_table_consistent":true,"source":"tms_pg"}],"truncated":false,"truncation_reasons":[],"warnings":[{"code":"STALE_DATA","message":"lag","rows":2}]}"#,
    "\n"
);

const MESSY_STANDARD: &str = r#"{"warnings":[{"impact":null,"rows":2,"column":null,"code":"STALE_DATA","table":null,"message":"lag"}],"estimated_total_rows":null,"star_expanded":null,"policy_version":"type-policy/v1","knowledge_version":"oos/v1","hints":[],"sources":[{"applied_pos":"18446744073709551615","cross_table_consistent":true,"consistency":"transactional","source":"tms_pg"}],"freshness":[{"state":"live","lag_ms":0,"as_of":"2026-10-01T01:12:03Z","table":"logistics.receipt"}],"duration_ms":12,"truncation_reasons":[],"truncated":false,"returned_rows":3,"rows":[["12.50"],[null],[true]],"columns":[{"timezone":null,"scale":2,"precision":38,"type":"numeric","name":"amount"}],"query_id":"q_abc123"}"#;

const AUDIT: &str = r#"{"error":{"code":"AUDIT_UNAVAILABLE","message":"audit","hint":"later","retryable":true,"query_id":"q_abc123"}}"#;
const INCONSISTENT: &str = r#"{"error":{"code":"RATE_LIMITED","message":"slow down","hint":"retry later","retryable":false,"query_id":"q_abc123"}}"#;
const WIDTH: &str = r#"{"query_id":"q_width1","columns":[{"name":"amount","type":"text"}],"rows":[["12.50","extra"]],"returned_rows":1,"truncated":false,"duration_ms":1,"freshness":[],"sources":[],"warnings":[],"knowledge_version":"oos/v1","policy_version":"type-policy/v1"}"#;
const COUNT: &str = r#"{"query_id":"q_count1","columns":[],"rows":[],"returned_rows":9,"truncated":false,"duration_ms":1,"freshness":[],"sources":[],"warnings":[],"knowledge_version":"oos/v1","policy_version":"type-policy/v1"}"#;
const NUMBER_CELL: &str = r#"{"query_id":"q_numcel","columns":[{"name":"n","type":"text"}],"rows":[[9007199254740993]],"returned_rows":1,"truncated":false,"duration_ms":1,"freshness":[],"sources":[],"warnings":[],"knowledge_version":"oos/v1","policy_version":"type-policy/v1"}"#;

#[derive(Clone)]
struct Hit {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct Server {
    port: u16,
    hits: Arc<Mutex<Vec<Hit>>>,
    accepts: Arc<std::sync::atomic::AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

enum Action {
    Raw(Vec<u8>),
    Stall,
    CloseAfter(Vec<u8>),
}

enum StdinPlan {
    Empty,
    Bytes(Vec<u8>),
    Hold,
}

struct ChildOut {
    code: Option<i32>,
    signal: Option<i32>,
    timed_out: bool,
    reaped: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    elapsed_ms: u128,
    spawn_error: Option<String>,
}

fn report_path() -> PathBuf {
    env::var_os("A103_REPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/a103-probe-report.json"))
}

fn cases_dir() -> PathBuf {
    env::var_os("A103_CASES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/a103-cases"))
}

fn bin_path() -> PathBuf {
    PathBuf::from(env::var_os("CARGO_BIN_EXE_oo").expect("CARGO_BIN_EXE_oo"))
}

fn poison_home() -> PathBuf {
    PathBuf::from("/tmp/a103-poison-home")
}

fn scratch(id: &str) -> PathBuf {
    let path = PathBuf::from("/tmp/a103-scratch").join(id);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("scratch");
    path
}

fn save(cases: &[Value]) {
    let proved = cases.iter().filter(|c| c["status"] == "proved").count();
    let failed = cases.iter().filter(|c| c["status"] == "failed").count();
    let unverified = cases.iter().filter(|c| c["status"] == "unverified").count();
    let harness = cases.iter().filter(|c| c["status"] == "harness").count();
    let doc = json!({
        "probe": "a103",
        "binary_env": "CARGO_BIN_EXE_oo",
        "token_redacted": true,
        "counts": {"proved": proved, "failed": failed, "unverified": unverified, "harness": harness, "cases": cases.len()},
        "cases": cases,
    });
    let path = report_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap_or_default());
}

fn redact(bytes: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    text = text.replace(TOKEN, "[redacted]");
    text = text.replace(CANARY, "[redacted]");
    if text.len() > 4000 {
        text.truncate(4000);
        text.push_str("\n[truncated]");
    }
    text
}

fn contains(bytes: &[u8], needle: &str) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.contains(needle)
}

fn one_json(bytes: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "stdout is not utf-8".to_owned())?;
    let Some(body) = text.strip_suffix('\n') else {
        return Err("stdout does not end with one newline".to_owned());
    };
    if body.ends_with('\n') || body.ends_with('\r') {
        return Err("stdout has more than one trailing newline".to_owned());
    }
    serde_json::from_str(body).map_err(|error| error.to_string())
}

fn error_schema(value: &Value) -> Result<(), String> {
    let obj = value.as_object().ok_or("error is not an object")?;
    for key in obj.keys() {
        if !matches!(
            key.as_str(),
            "class" | "code" | "exit" | "hint" | "message" | "query_id" | "retry" | "retryable"
        ) {
            return Err(format!("unexpected error key {key}"));
        }
    }
    for key in ["class", "exit", "message", "retry", "retryable"] {
        if !obj.contains_key(key) {
            return Err(format!("missing {key}"));
        }
    }
    let retry = obj.get("retry").and_then(Value::as_object).ok_or("retry")?;
    for key in retry.keys() {
        if !matches!(
            key.as_str(),
            "attempt_count" | "max_extra_attempts" | "max_wait_ms" | "nested_cli_retries" | "outcome"
        ) {
            return Err(format!("unexpected retry key {key}"));
        }
    }
    if retry.get("attempt_count").and_then(Value::as_str) != Some("unknown") {
        return Err("attempt_count".to_owned());
    }
    if retry.get("nested_cli_retries").and_then(Value::as_u64) != Some(0) {
        return Err("nested_cli_retries".to_owned());
    }
    if retry.get("max_extra_attempts").and_then(Value::as_u64) != Some(3) {
        return Err("max_extra_attempts".to_owned());
    }
    if retry.get("max_wait_ms").and_then(Value::as_u64) != Some(2000) {
        return Err("max_wait_ms".to_owned());
    }
    Ok(())
}

fn case(
    id: &str,
    status: &str,
    expected: &str,
    actual: &str,
    child: &ChildOut,
    hits: usize,
    accepts: usize,
    extra: Value,
) -> Value {
    let dir = cases_dir().join(id);
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(dir.join("stdout.txt"), redact(&child.stdout));
    let _ = fs::write(dir.join("stderr.txt"), redact(&child.stderr));
    json!({
        "id": id,
        "status": status,
        "expected": expected,
        "actual": actual,
        "exit": child.code,
        "signal": child.signal,
        "timed_out": child.timed_out,
        "reaped": child.reaped,
        "elapsed_ms": child.elapsed_ms,
        "hits": hits,
        "accepts": accepts,
        "stdout_len": child.stdout.len(),
        "stderr_len": child.stderr.len(),
        "stdout_has_token": contains(&child.stdout, TOKEN) || contains(&child.stdout, CANARY),
        "stderr_has_token": contains(&child.stderr, TOKEN) || contains(&child.stderr, CANARY),
        "stdout": redact(&child.stdout),
        "stderr": redact(&child.stderr),
        "spawn_error": child.spawn_error,
        "extra": extra,
        "poison_home_exists": poison_home().exists(),
    })
}

fn http_response(status: u16, reason: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

fn json_ok(body: &str) -> Vec<u8> {
    http_response(200, "OK", &[("Content-Type", "application/json")], body.as_bytes())
}

fn json_status(status: u16, reason: &str, body: &str, retry_after: Option<&str>) -> Vec<u8> {
    let mut headers = vec![("Content-Type", "application/json")];
    if let Some(value) = retry_after {
        headers.push(("Retry-After", value));
    }
    http_response(status, reason, &headers, body.as_bytes())
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|item| item == b"\r\n\r\n")
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let mut found = None;
    for (key, value) in headers {
        if key.eq_ignore_ascii_case(name) {
            if found.is_some() {
                return None;
            }
            found = Some(value.as_str());
        }
    }
    found
}

async fn read_hit<R: tokio::io::AsyncRead + Unpin>(sock: &mut R) -> Result<Hit, ()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let end = loop {
        let n = sock.read(&mut tmp).await.map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = header_end(&buf) {
            break pos;
        }
        if buf.len() > 65_536 {
            return Err(());
        }
    };
    let text = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = text.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    let mut headers = Vec::new();
    let mut length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();
            if name == "content-length" {
                length = value.parse().unwrap_or(usize::MAX);
            }
            headers.push((name, value));
        }
    }
    if length > 2_000_000 {
        return Err(());
    }
    let mut body = buf[end + 4..].to_vec();
    while body.len() < length {
        let n = sock.read(&mut tmp).await.map_err(|_| ())?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
        if body.len() > 2_000_000 {
            break;
        }
    }
    if body.len() > length {
        body.truncate(length);
    }
    Ok(Hit { method, path, headers, body })
}

async fn serve(
    listener: TcpListener,
    hits: Arc<Mutex<Vec<Hit>>>,
    accepts: Arc<std::sync::atomic::AtomicUsize>,
    plan: Arc<dyn Fn(usize, &Hit) -> Action + Send + Sync>,
) {
    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            break;
        };
        accepts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let Ok(hit) = read_hit(&mut sock).await else {
            continue;
        };
        let action = {
            let mut guard = hits.lock().expect("hits");
            let action = plan(guard.len(), &hit);
            guard.push(hit);
            action
        };
        match action {
            Action::Raw(bytes) => {
                let _ = sock.write_all(&bytes).await;
                let _ = sock.shutdown().await;
            }
            Action::CloseAfter(bytes) => {
                let _ = sock.write_all(&bytes).await;
                drop(sock);
            }
            Action::Stall => {
                tokio::time::sleep(Duration::from_secs(45)).await;
            }
        }
    }
}

async fn spawn_server<F>(plan: F) -> Server
where
    F: Fn(usize, &Hit) -> Action + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let accepts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let task = tokio::spawn(serve(listener, Arc::clone(&hits), Arc::clone(&accepts), Arc::new(plan)));
    tokio::task::yield_now().await;
    Server { port, hits, accepts, task }
}

fn b64_val(byte: u8) -> Result<u8, ()> {
    Ok(match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return Err(()),
    })
}

fn b64_decode(input: &str) -> Result<Vec<u8>, ()> {
    let bytes = input.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(());
    }
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let (b0, b1, b2, b3) = (bytes[index], bytes[index + 1], bytes[index + 2], bytes[index + 3]);
        index += 4;
        let n = (u32::from(b64_val(b0)?) << 18)
            | (u32::from(b64_val(b1)?) << 12)
            | (u32::from(if b2 == b'=' { 0 } else { b64_val(b2)? }) << 6)
            | u32::from(if b3 == b'=' { 0 } else { b64_val(b3)? });
        out.push((n >> 16) as u8);
        if b2 != b'=' {
            out.push((n >> 8) as u8);
        }
        if b3 != b'=' {
            out.push(n as u8);
        }
    }
    Ok(out)
}

fn pem_der(pem: &str, label: &str) -> Result<Vec<u8>, String> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = pem.find(&begin).ok_or("pem begin")? + begin.len();
    let stop = pem[start..].find(&end).ok_or("pem end")? + start;
    let encoded: String = pem[start..stop].chars().filter(|ch| !ch.is_whitespace()).collect();
    b64_decode(&encoded).map_err(|_| "pem base64".to_owned())
}

async fn spawn_tls(cert_pem: &str, key_pem: &str, body: Vec<u8>) -> Result<Server, String> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cert = rustls::pki_types::CertificateDer::from(pem_der(cert_pem, "CERTIFICATE")?);
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        pem_der(key_pem, "PRIVATE KEY")?,
    ));
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|error| error.to_string())?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|error| error.to_string())?;
    let port = listener.local_addr().map_err(|error| error.to_string())?.port();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let accepts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stored = Arc::clone(&hits);
    let counted = Arc::clone(&accepts);
    let task = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let Ok(mut tls) = acceptor.accept(tcp).await else {
                continue;
            };
            let Ok(hit) = read_hit(&mut tls).await else {
                continue;
            };
            stored.lock().expect("hits").push(hit);
            let _ = tls.write_all(&body).await;
            let _ = tls.shutdown().await;
        }
    });
    tokio::task::yield_now().await;
    Ok(Server { port, hits, accepts, task })
}

fn write_mode(path: &Path, bytes: &[u8], mode: u32) {
    fs::write(path, bytes).expect("write");
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("mode");
}

fn token_file(dir: &Path) -> PathBuf {
    let path = dir.join("token");
    write_mode(&path, format!("{TOKEN}\n").as_bytes(), 0o600);
    path
}

struct Spec {
    args: Vec<std::ffi::OsString>,
    stdin: StdinPlan,
    timeout: Duration,
    tmpdir: PathBuf,
    proxy: Option<String>,
    broken_stdout: bool,
    env_token: bool,
}

fn plan(args: Vec<std::ffi::OsString>, tmpdir: PathBuf) -> Spec {
    Spec {
        args,
        stdin: StdinPlan::Empty,
        timeout: Duration::from_secs(5),
        tmpdir,
        proxy: None,
        broken_stdout: false,
        env_token: true,
    }
}

fn os_args(items: &[&str]) -> Vec<std::ffi::OsString> {
    items.iter().map(|item| std::ffi::OsString::from(*item)).collect()
}

fn query_args(token: &Path, gateway: &str, transport: &str, tail: &[&str]) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        "query".to_owned(),
        "--auth".to_owned(),
        "external".to_owned(),
        "--token-file".to_owned(),
        token.display().to_string(),
        "--gateway".to_owned(),
        gateway.to_owned(),
        "--transport".to_owned(),
        transport.to_owned(),
    ];
    args.extend(tail.iter().map(|item| (*item).to_owned()));
    os_args(&args.iter().map(String::as_str).collect::<Vec<_>>())
}

fn run_child(spec: Spec) -> ChildOut {
    let mut command = Command::new(bin_path());
    command
        .args(&spec.args)
        .env("HOME", poison_home())
        .env("TMPDIR", &spec.tmpdir)
        .env("OO_GATEWAY_URL", "http://canary-gateway.invalid")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("all_proxy")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if spec.env_token {
        command.env("OO_TOKEN", CANARY);
    } else {
        command.env_remove("OO_TOKEN");
    }
    if let Some(proxy) = &spec.proxy {
        command
            .env("HTTP_PROXY", proxy)
            .env("HTTPS_PROXY", proxy)
            .env("ALL_PROXY", proxy)
            .env("http_proxy", proxy)
            .env("https_proxy", proxy)
            .env("all_proxy", proxy);
    }
    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ChildOut {
                code: None,
                signal: None,
                timed_out: false,
                reaped: false,
                stdout: Vec::new(),
                stderr: Vec::new(),
                elapsed_ms: started.elapsed().as_millis(),
                spawn_error: Some(error.to_string()),
            };
        }
    };
    let held = match (spec.stdin, child.stdin.take()) {
        (StdinPlan::Bytes(bytes), Some(mut stdin)) => {
            let _ = stdin.write_all(&bytes);
            None
        }
        (StdinPlan::Hold, Some(stdin)) => Some(stdin),
        _ => None,
    };
    if spec.broken_stdout {
        drop(child.stdout.take());
    }
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > spec.timeout => {
                timed_out = true;
                let _ = child.kill();
                match child.wait() {
                    Ok(status) => break status,
                    Err(_) => {
                        drop(held);
                        return ChildOut {
                            code: None,
                            signal: None,
                            timed_out: true,
                            reaped: false,
                            stdout: stdout_thread.join().unwrap_or_default(),
                            stderr: stderr_thread.join().unwrap_or_default(),
                            elapsed_ms: started.elapsed().as_millis(),
                            spawn_error: Some("reap failed".to_owned()),
                        };
                    }
                }
            }
            Ok(None) => thread::sleep(Duration::from_millis(15)),
            Err(error) => {
                drop(held);
                return ChildOut {
                    code: None,
                    signal: None,
                    timed_out: false,
                    reaped: false,
                    stdout: Vec::new(),
                    stderr: error.to_string().into_bytes(),
                    elapsed_ms: started.elapsed().as_millis(),
                    spawn_error: Some(error.to_string()),
                };
            }
        }
    };
    drop(held);
    ChildOut {
        code: status.code(),
        signal: status.signal(),
        timed_out,
        reaped: true,
        stdout: stdout_thread.join().unwrap_or_default(),
        stderr: stderr_thread.join().unwrap_or_default(),
        elapsed_ms: started.elapsed().as_millis(),
        spawn_error: None,
    }
}

fn hits_of(server: &Server) -> Vec<Hit> {
    server.hits.lock().expect("hits").clone()
}

fn accepts_of(server: &Server) -> usize {
    server.accepts.load(std::sync::atomic::Ordering::SeqCst)
}

fn privacy_ok(child: &ChildOut, gateway: &str) -> bool {
    !contains(&child.stdout, TOKEN)
        && !contains(&child.stderr, TOKEN)
        && !contains(&child.stdout, CANARY)
        && !contains(&child.stderr, CANARY)
        && !contains(&child.stdout, "Bearer ")
        && !contains(&child.stderr, "Bearer ")
        && !contains(&child.stdout, gateway)
        && !contains(&child.stderr, gateway)
}

fn bearer_ok(hit: &Hit) -> bool {
    header_value(&hit.headers, "authorization") == Some(&format!("Bearer {TOKEN}"))
}

fn wire_ok(hit: &Hit, body: &[u8]) -> bool {
    hit.method == "POST"
        && hit.path == "/v1/query"
        && header_value(&hit.headers, "content-type") == Some("application/json")
        && header_value(&hit.headers, "accept") == Some("application/json")
        && header_value(&hit.headers, "user-agent") == Some("oo-sdk/0.1")
        && bearer_ok(hit)
        && hit.body == body
        && !hit.body.windows(CANARY.len()).any(|item| item == CANARY.as_bytes())
}

fn canonical_body(sql: &str, max_rows: Option<u32>, timeout_ms: Option<u32>) -> Vec<u8> {
    let mut fields = vec![("format".to_owned(), "\"json\"".to_owned()), ("sql".to_owned(), json_string(sql))];
    if let Some(max_rows) = max_rows {
        fields.push(("max_rows".to_owned(), max_rows.to_string()));
    }
    if let Some(timeout_ms) = timeout_ms {
        fields.push(("timeout_ms".to_owned(), timeout_ms.to_string()));
    }
    fields.sort_by(|left, right| left.0.cmp(&right.0));
    let mut out = String::from("{");
    for (index, (key, value)) in fields.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(key));
        out.push(':');
        out.push_str(value);
    }
    out.push('}');
    out.into_bytes()
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn finish_status(ok: bool, child: &ChildOut) -> &'static str {
    if child.spawn_error.is_some() || !child.reaped {
        "harness"
    } else if ok {
        "proved"
    } else {
        "failed"
    }
}

fn run_spec(spec: Spec) -> ChildOut {
    let tmp = spec.tmpdir.clone();
    let out = tokio::task::block_in_place(|| run_child(spec));
    let _ = fs::remove_dir_all(tmp);
    out
}

async fn no_socket(id: &str, args: &[&str], exit: i32, stdout_has: &str, stderr_has: &str, cases: &mut Vec<Value>) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    let _ = fs::create_dir(&tmp);
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut argv = args.to_vec();
    for item in &mut argv {
        if *item == "GATEWAY" {
            *item = &gateway;
        }
    }
    let owned: Vec<String> = argv.iter().map(|item| (*item).to_owned()).collect();
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    let mut spec = plan(os_args(&refs), tmp);
    spec.timeout = Duration::from_secs(2);
    let _ = token;
    let child = run_spec(spec);
    let accepts = accepts_of(&server);
    let ok = child.code == Some(exit)
        && child.timed_out == false
        && accepts == 0
        && (stdout_has.is_empty() || contains(&child.stdout, stdout_has))
        && (stderr_has.is_empty() || contains(&child.stderr, stderr_has))
        && !contains(&child.stdout, TOKEN)
        && !contains(&child.stderr, TOKEN)
        && !contains(&child.stdout, CANARY)
        && !contains(&child.stderr, CANARY)
        && privacy_ok(&child, &gateway);
    let status = finish_status(ok, &child);
    cases.push(case(
        id,
        status,
        &format!("exit {exit}, accepts 0, stdout {stdout_has:?}, stderr {stderr_has:?}"),
        &format!("exit {:?} signal {:?} accepts {accepts}", child.code, child.signal),
        &child,
        hits_of(&server).len(),
        accepts,
        json!({}),
    ));
}

fn mkfifo(path: &Path) -> bool {
    Command::new("mkfifo").arg(path).status().map(|status| status.success()).unwrap_or(false)
}

fn unescape(input: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('|') => out.push('|'),
            Some('`') => out.push('`'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                if chars.next() != Some('{') {
                    return None;
                }
                let mut hex = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => hex.push(ch),
                        None => return None,
                    }
                }
                let code = u32::from_str_radix(&hex, 16).ok()?;
                out.push(char::from_u32(code)?);
            }
            _ => return None,
        }
    }
    Some(out)
}

fn table_cells(line: &str) -> Option<Vec<String>> {
    if !line.starts_with('|') {
        return None;
    }
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    let _ = chars.next();
    let mut escaped = false;
    for ch in chars {
        if escaped {
            cur.push('\\');
            cur.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '|' {
            let padded = cur.strip_prefix(' ').unwrap_or(cur.as_str());
            let padded = padded.strip_suffix(' ').unwrap_or(padded);
            cells.push(unescape(padded)?);
            cur.clear();
            continue;
        }
        cur.push(ch);
    }
    Some(cells)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a103_independent_query_review() {
    let _ = fs::remove_dir_all(poison_home());
    let mut cases = Vec::new();
    dispatch_cases(&mut cases).await;
    credential_cases(&mut cases).await;
    output_cases(&mut cases).await;
    outcome_cases(&mut cases).await;
    save(&cases);
    let failed = cases.iter().filter(|item| item["status"] == "failed" || item["status"] == "harness").count();
    if failed > 0 {
        panic!("a103 cases failed or harness: {failed}");
    }
}

async fn dispatch_cases(cases: &mut Vec<Value>) {
    no_socket("help_top", &["--help"], 0, "oo query", "", cases).await;
    no_socket("help_query", &["query", "--help"], 0, "Examples:", "", cases).await;
    no_socket("help_h", &["query", "-h"], 0, "--token-file", "", cases).await;
    no_socket("unknown_flag", &["query", "--nope", SQL], 2, "", "unknown flag", cases).await;
    no_socket("equals_hides_value", &["query", "--token-file=sentinel-a103-equals", SQL], 2, "", "equals", cases).await;
    no_socket("extra_arg", &["query", "--auth", "external", "--token-stdin", "--gateway", "GATEWAY", "--transport", "local-development", SQL, "extra"], 2, "", "extra argument", cases).await;
    no_socket("sql_and_file", &["query", SQL, "-f", "query.sql"], 2, "", "mutually exclusive", cases).await;
    no_socket("sql_stdin_dash", &["query", "-f", "-"], 2, "", "sql stdin is not supported", cases).await;
    no_socket("max_rows_zero", &["query", "--max-rows", "0", SQL], 2, "", "invalid max-rows", cases).await;
    no_socket("max_rows_high", &["query", "--max-rows", "2001", SQL], 2, "", "invalid max-rows", cases).await;
    no_socket("timeout_zero", &["query", "--timeout-ms", "0", SQL], 2, "", "invalid timeout-ms", cases).await;
    no_socket("timeout_high", &["query", "--timeout-ms", "120001", SQL], 2, "", "invalid timeout-ms", cases).await;
    no_socket("leading_zero", &["query", "--max-rows", "01", SQL], 2, "", "invalid max-rows", cases).await;
    no_socket("duplicate_json", &["query", "--json", "--json", SQL], 2, "", "duplicate flag", cases).await;
    no_socket("missing_sql", &["query", "--json"], 2, "sql is missing", "", cases).await;
    no_socket("empty_sql", &["query", "--json", ""], 2, "sql is missing", "", cases).await;
    no_socket("unknown_command", &["export", "--json"], 2, "", "not implemented", cases).await;
    no_socket("missing_command", &[], 2, "", "command is missing", cases).await;
    no_socket("bad_auth", &["query", "--auth", "admin", SQL], 2, "", "auth must be external or sandbox", cases).await;
    no_socket("missing_auth_value", &["query", "--auth", "--json", SQL], 2, "", "missing flag value", cases).await;

    let id = "invalid_utf8_before_json";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = vec![std::ffi::OsString::from("query"), std::ffi::OsString::from_vec(vec![0xff, 0xfe])];
    args.extend(os_args(&["--json", "--auth", "external", "--token-stdin", "--gateway", &gateway, "--transport", "local-development", SQL]));
    let mut spec = plan(args, tmp);
    spec.timeout = Duration::from_secs(2);
    let child = run_spec(spec);
    let accepts = accepts_of(&server);
    let ok = child.code == Some(2) && accepts == 0 && contains(&child.stderr, "utf-8") && child.stdout.is_empty();
    cases.push(case(id, finish_status(ok, &child), "exit 2 text usage, accepts 0; --json is not reached", &format!("exit {:?} stdout_len {}", child.code, child.stdout.len()), &child, hits_of(&server).len(), accepts, json!({"json_mode_hidden": child.stdout.is_empty()})));

    let id = "invalid_utf8_after_json";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = os_args(&["query", "--json", "--gateway"]);
    args.push(std::ffi::OsString::from_vec(vec![0xff]));
    args.extend(os_args(&["--auth", "external", "--token-stdin", "--transport", "local-development", SQL]));
    let _ = gateway;
    let mut spec = plan(args, tmp);
    spec.timeout = Duration::from_secs(2);
    let child = run_spec(spec);
    let parsed = one_json(&child.stdout).ok();
    let ok = child.code == Some(2) && child.stderr.is_empty() && parsed.as_ref().and_then(|v| error_schema(v).ok()).is_some();
    cases.push(case(id, finish_status(ok, &child), "exit 2 one JSON usage document", &format!("exit {:?} parsed {}", child.code, parsed.is_some()), &child, 0, 0, json!({})));

    let id = "dash_sql_after_end_of_flags";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let sql = "--not-a-flag";
    let mut args = query_args(&token, &gateway, "local-development", &[]);
    args.extend(os_args(&["--", sql]));
    let child = run_spec(plan(args, tmp));
    let hits = hits_of(&server);
    let expected = canonical_body(sql, None, None);
    let ok = child.code == Some(0) && hits.len() == 1 && wire_ok(&hits[0], &expected) && child.stderr.is_empty();
    cases.push(case(id, finish_status(ok, &child), "POST sql is --not-a-flag", &format!("hits {} body_match {}", hits.len(), hits.first().is_some_and(|hit| hit.body == expected)), &child, hits.len(), accepts_of(&server), json!({})));

    let id = "canonical_limits";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let args = query_args(&token, &gateway, "local-development", &["--max-rows", "10", "--timeout-ms", "1000", SQL]);
    let child = run_spec(plan(args, tmp));
    let hits = hits_of(&server);
    let expected = canonical_body(SQL, Some(10), Some(1000));
    let fixture = include_bytes!("../../../contracts/cli/query/fixtures/request-limits.wire.json");
    let ok = child.code == Some(0) && hits.len() == 1 && wire_ok(&hits[0], &expected) && expected.as_slice() == fixture;
    cases.push(case(id, finish_status(ok, &child), "limits wire matches handwritten bytes and the contract fixture", &format!("match {}", hits.first().is_some_and(|hit| hit.body == expected)), &child, hits.len(), accepts_of(&server), json!({"expected_len": expected.len()})));

    let id = "positive_json";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(STANDARD))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let args = query_args(&token, &gateway, "local-development", &["--json", SQL]);
    let child = run_spec(plan(args, tmp));
    let hits = hits_of(&server);
    let expected = canonical_body(SQL, None, None);
    let fixture = include_bytes!("../../../contracts/cli/query/fixtures/request-min.wire.json");
    let parsed = one_json(&child.stdout).ok();
    let null_cell = parsed.as_ref().and_then(|v| v.pointer("/rows/1/0")).is_some_and(Value::is_null);
    let no_estimated = parsed.as_ref().and_then(Value::as_object).is_some_and(|obj| !obj.contains_key("estimated_total_rows"));
    let ok = child.code == Some(0)
        && child.stderr.is_empty()
        && hits.len() == 1
        && wire_ok(&hits[0], &expected)
        && expected.as_slice() == fixture
        && child.stdout == EXPECTED_STANDARD.as_bytes()
        && null_cell
        && no_estimated
        && privacy_ok(&child, &gateway);
    cases.push(case(id, finish_status(ok, &child), "canonical POST, sorted JSON, null cell kept, estimated absent", &format!("stdout_match {} null_cell {null_cell}", child.stdout == EXPECTED_STANDARD.as_bytes()), &child, hits.len(), accepts_of(&server), json!({})));
}

async fn credential_cases(cases: &mut Vec<Value>) {
    sandbox_and_missing(cases).await;
    token_modes(cases).await;
    fifo_and_devices(cases).await;
    tmp_and_links(cases).await;
}

async fn sandbox_and_missing(cases: &mut Vec<Value>) {
    let id = "sandbox_before_fifo_sql_ca_stdin";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token_fifo = dir.join("token");
    let sql_fifo = dir.join("sql");
    let ca_fifo = dir.join("ca");
    let made = mkfifo(&token_fifo) && mkfifo(&sql_fifo) && mkfifo(&ca_fifo);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = os_args(&[
        "query", "--auth", "sandbox", "--token-file",
    ]);
    args.push(token_fifo.into_os_string());
    args.extend(os_args(&["--ca-file"]));
    args.push(ca_fifo.into_os_string());
    args.extend(os_args(&["--gateway", &gateway, "--transport", "local-development", "-f"]));
    args.push(sql_fifo.into_os_string());
    let mut spec = plan(args, tmp);
    spec.stdin = StdinPlan::Hold;
    spec.timeout = Duration::from_millis(1500);
    let child = run_spec(spec);
    let ok = made && child.code == Some(1) && !child.timed_out && accepts_of(&server) == 0 && contains(&child.stderr, "from_sandbox_env") && !contains(&child.stderr, TOKEN);
    cases.push(case(id, if made { finish_status(ok, &child) } else { "harness" }, "sandbox exit 1 before fifo open", &format!("made {made} timeout {}", child.timed_out), &child, hits_of(&server).len(), accepts_of(&server), json!({})));

    let id = "missing_auth_does_not_open_fifo";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let fifo = dir.join("token");
    let made = mkfifo(&fifo);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = os_args(&["query", "--token-file"]);
    args.push(fifo.into_os_string());
    args.extend(os_args(&["--gateway", &gateway, "--transport", "local-development", SQL]));
    let mut spec = plan(args, tmp);
    spec.timeout = Duration::from_millis(1500);
    let child = run_spec(spec);
    let ok = made && child.code == Some(4) && !child.timed_out && accepts_of(&server) == 0 && contains(&child.stderr, "authentication source is missing");
    cases.push(case(id, if made { finish_status(ok, &child) } else { "harness" }, "missing auth exit 4, fifo not opened", &format!("timeout {}", child.timed_out), &child, 0, accepts_of(&server), json!({})));

    let id = "external_without_token_source";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let args = os_args(&["query", "--auth", "external", "--gateway", &gateway, "--transport", "local-development", SQL]);
    let mut spec = plan(args, tmp);
    spec.timeout = Duration::from_secs(2);
    let child = run_spec(spec);
    let ok = child.code == Some(4) && accepts_of(&server) == 0 && contains(&child.stderr, "token source is missing");
    cases.push(case(id, finish_status(ok, &child), "exit 4 before connect", "see stderr", &child, 0, accepts_of(&server), json!({})));

    let id = "gateway_required_before_fifo";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let fifo = dir.join("token");
    let made = mkfifo(&fifo);
    let mut args = os_args(&["query", "--auth", "external", "--token-file"]);
    args.push(fifo.into_os_string());
    args.extend(os_args(&["--transport", "local-development", SQL]));
    let mut spec = plan(args, tmp);
    spec.timeout = Duration::from_millis(1500);
    let child = run_spec(spec);
    let ok = made && child.code == Some(2) && !child.timed_out && contains(&child.stderr, "gateway is required");
    cases.push(case(id, if made { finish_status(ok, &child) } else { "harness" }, "exit 2 before fifo open", &format!("timeout {}", child.timed_out), &child, 0, 0, json!({})));

    let id = "token_stdin_is_not_sql_stdin";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = os_args(&["query", "--auth", "external", "--token-stdin", "--gateway", &gateway, "--transport", "local-development", "-f", "-"]);
    let mut spec = plan(args, tmp.clone());
    spec.stdin = StdinPlan::Hold;
    spec.timeout = Duration::from_millis(1500);
    let child = run_spec(spec);
    let dash_ok = child.code == Some(2) && !child.timed_out && accepts_of(&server) == 0;
    let token = token_file(&dir);
    let sql_path = dir.join("q.sql");
    fs::write(&sql_path, "select a103_file_sql\n").unwrap();
    let mut args = os_args(&["query", "--auth", "external", "--token-stdin", "--gateway", &gateway, "--transport", "local-development", "-f"]);
    args.push(sql_path.into_os_string());
    let mut spec = plan(args, tmp);
    spec.stdin = StdinPlan::Bytes(format!("{TOKEN}\n").into_bytes());
    let child2 = run_spec(spec);
    let hits = hits_of(&server);
    let body_ok = hits.first().is_some_and(|hit| {
        bearer_ok(hit) && String::from_utf8_lossy(&hit.body).contains("select a103_file_sql") && !String::from_utf8_lossy(&hit.body).contains(TOKEN)
    });
    let ok = dash_ok && child2.code == Some(0) && hits.len() == 1 && body_ok;
    let _ = token;
    cases.push(case(id, finish_status(ok, &child2), "-f - does not read stdin; token stdin is the bearer and file SQL is the SQL", &format!("dash_exit {:?} query_exit {:?} body_ok {body_ok}", child.code, child2.code), &child2, hits.len(), accepts_of(&server), json!({"dash_timed_out": child.timed_out})));
}

async fn token_modes(cases: &mut Vec<Value>) {
    let token_line = format!("{TOKEN}\n").into_bytes();
    let mut exact_nl = vec![b'A'; 4096];
    exact_nl.push(b'\n');
    let samples = vec![
        ("mode_0600", token_line.clone(), 0o600, "ok"),
        ("mode_0644", token_line.clone(), 0o644, "mode"),
        ("mode_0400", token_line.clone(), 0o400, "mode"),
        ("mode_0700", token_line.clone(), 0o700, "mode"),
        ("mode_0601", token_line.clone(), 0o601, "mode"),
        ("mode_0666", token_line.clone(), 0o666, "mode"),
        ("mode_setuid", token_line.clone(), 0o4600, "mode"),
        ("mode_setgid", token_line.clone(), 0o2600, "mode"),
        ("mode_sticky", token_line.clone(), 0o1600, "mode"),
        ("mode_0000", token_line, 0o000, "permission_or_mode"),
        ("empty", b"".to_vec(), 0o600, "rejected"),
        ("newline_only", b"\n".to_vec(), 0o600, "rejected"),
        ("two_newlines", b"abc\n\n".to_vec(), 0o600, "rejected"),
        ("space", b"ab c\n".to_vec(), 0o600, "rejected"),
        ("cr", b"ab\r\n".to_vec(), 0o600, "rejected"),
        ("non_ascii", "caf\u{e9}\n".as_bytes().to_vec(), 0o600, "rejected"),
        ("control", b"ab\x01\n".to_vec(), 0o600, "rejected"),
        ("budget_4098", vec![b'A'; 4098], 0o600, "budget"),
        ("over_4096_no_nl", vec![b'A'; 4097], 0o600, "rejected"),
        ("exact_4096", vec![b'A'; 4096], 0o600, "ok_or_query"),
        ("exact_4096_nl", exact_nl, 0o600, "ok_or_query"),
    ];
    for (name, bytes, mode, expect) in samples {
        let id = format!("token_{name}");
        let dir = scratch(&id);
        let tmp = dir.join("tmp");
        fs::create_dir(&tmp).unwrap();
        let path = dir.join("token");
        write_mode(&path, &bytes, mode);
        let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
        let gateway = format!("http://127.0.0.1:{}/", server.port);
        let args = query_args(&path, &gateway, "local-development", &["--json", SQL]);
        let mut spec = plan(args, tmp);
        spec.timeout = Duration::from_secs(3);
        let child = run_spec(spec);
        let hits = hits_of(&server).len();
        let text = format!("{}{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
        let path_hidden = !text.contains(&path.display().to_string());
        let ok = match expect {
            "ok" => child.code == Some(0) && hits == 1 && child.stderr.is_empty() && path_hidden,
            "mode" => child.code == Some(4) && hits == 0 && text.contains("mode") && path_hidden && !child.timed_out,
            "permission_or_mode" => child.code == Some(4) && hits == 0 && (text.contains("permission") || text.contains("mode")) && path_hidden,
            "rejected" => child.code == Some(4) && hits == 0 && text.contains("token rejected") && path_hidden,
            "budget" => child.code == Some(4) && hits == 0 && text.contains("budget") && path_hidden,
            "ok_or_query" => hits == 1 && child.code == Some(0) && !text.contains(TOKEN),
            _ => false,
        };
        cases.push(case(&id, finish_status(ok, &child), expect, &format!("exit {:?} hits {hits}", child.code), &child, hits, accepts_of(&server), json!({"path_hidden": path_hidden})));
    }

    let id = "symlink_token";
    symlink_case(cases, id, false).await;
    let id = "dangling_symlink_token";
    symlink_case(cases, id, true).await;

    let id = "directory_token";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let path = dir.join("token-dir");
    fs::create_dir(&path).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&path, &gateway, "local-development", &[SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stderr).into_owned();
    let ok = child.code == Some(4) && hits_of(&server).is_empty() && text.contains("not-file") && !text.contains(&path.display().to_string());
    cases.push(case(id, finish_status(ok, &child), "directory is not-file", &text, &child, 0, accepts_of(&server), json!({})));

    let id = "foreign_uid";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let path = dir.join("token");
    write_mode(&path, format!("{TOKEN}\n").as_bytes(), 0o644);
    let chown = std::os::unix::fs::chown(&path, Some(65534), None);
    if let Err(error) = chown {
        let child = ChildOut { code: None, signal: None, timed_out: false, reaped: true, stdout: Vec::new(), stderr: Vec::new(), elapsed_ms: 0, spawn_error: None };
        cases.push(case(id, "unverified", "file uid differs and open still reaches the owner check", &format!("chown failed: {error}"), &child, 0, 0, json!({"errno": error.raw_os_error()})));
    } else {
        let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
        let gateway = format!("http://127.0.0.1:{}/", server.port);
        let child = run_spec(plan(query_args(&path, &gateway, "local-development", &[SQL]), tmp));
        let text = String::from_utf8_lossy(&child.stderr).into_owned();
        let ok = child.code == Some(4) && hits_of(&server).is_empty() && text.contains("owner");
        cases.push(case(id, finish_status(ok, &child), "foreign uid rejected as owner", &text, &child, hits_of(&server).len(), accepts_of(&server), json!({})));
    }
}

async fn symlink_case(cases: &mut Vec<Value>, id: &str, dangling: bool) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let target = dir.join("real");
    if !dangling {
        write_mode(&target, format!("{TOKEN}\n").as_bytes(), 0o600);
    }
    let link = dir.join("token");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&link, &gateway, "local-development", &[SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stderr).into_owned();
    let ok = child.code == Some(4) && hits_of(&server).is_empty() && text.contains("symlink") && !text.contains(&link.display().to_string());
    cases.push(case(id, finish_status(ok, &child), "symlink rejected, path hidden, no connect", &text, &child, 0, accepts_of(&server), json!({})));
}

async fn fifo_and_devices(cases: &mut Vec<Value>) {
    device_case(cases, "device_null_token", "/dev/null").await;
    device_case(cases, "device_zero_token", "/dev/zero").await;
    fifo_token(cases, "fifo_token_peerless", false).await;
    fifo_token(cases, "fifo_token_with_writer", true).await;
    fifo_sql(cases, "fifo_sql_peerless", false).await;
    fifo_sql(cases, "fifo_sql_with_writer", true).await;
    fifo_ca(cases, "fifo_ca_peerless", false).await;
    fifo_ca(cases, "fifo_ca_with_writer", true).await;
}

async fn device_case(cases: &mut Vec<Value>, id: &str, path: &str) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut spec = plan(query_args(Path::new(path), &gateway, "local-development", &[SQL]), tmp);
    spec.timeout = Duration::from_millis(1500);
    let child = run_spec(spec);
    let text = String::from_utf8_lossy(&child.stderr).into_owned();
    let ok = !child.timed_out && child.code == Some(4) && hits_of(&server).is_empty() && text.contains("not-file");
    cases.push(case(id, finish_status(ok, &child), "device rejected as not-file within 1500ms", &format!("timeout {} {text}", child.timed_out), &child, 0, accepts_of(&server), json!({})));
}

async fn fifo_token(cases: &mut Vec<Value>, id: &str, writer: bool) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let fifo = dir.join("token");
    let made = mkfifo(&fifo);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let writer_thread = if writer {
        let path = fifo.clone();
        Some(thread::spawn(move || {
            let _ = OpenOptions::new().write(true).open(path);
        }))
    } else {
        None
    };
    let mut spec = plan(query_args(&fifo, &gateway, "local-development", &[SQL]), tmp);
    spec.timeout = Duration::from_millis(if writer { 2500 } else { 1200 });
    let child = run_spec(spec);
    if let Some(handle) = writer_thread {
        let _ = handle.join();
    }
    let text = String::from_utf8_lossy(&child.stderr).into_owned();
    let (status, actual) = if !made {
        ("harness", "mkfifo failed".to_owned())
    } else if writer {
        let ok = !child.timed_out && child.code == Some(4) && text.contains("not-file") && hits_of(&server).is_empty();
        (finish_status(ok, &child), format!("writer unblocks open; timeout {} {text}", child.timed_out))
    } else if child.timed_out {
        ("failed", "peerless fifo did not reach not-file rejection before the deadline".to_owned())
    } else {
        let ok = child.code == Some(4) && text.contains("not-file");
        (finish_status(ok, &child), format!("exited without a peer: {text}"))
    };
    cases.push(case(id, status, "non-regular token rejected within the deadline", &actual, &child, hits_of(&server).len(), accepts_of(&server), json!({"writer": writer})));
}

async fn fifo_sql(cases: &mut Vec<Value>, id: &str, writer: bool) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let fifo = dir.join("query.sql");
    let made = mkfifo(&fifo);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = query_args(&token, &gateway, "local-development", &["-f"]);
    args.push(fifo.clone().into_os_string());
    let writer_thread = if writer {
        let path = fifo.clone();
        Some(thread::spawn(move || {
            let _ = OpenOptions::new().write(true).open(path);
        }))
    } else {
        None
    };
    let mut spec = plan(args, tmp);
    spec.timeout = Duration::from_millis(if writer { 2500 } else { 1200 });
    let child = run_spec(spec);
    if let Some(handle) = writer_thread {
        let _ = handle.join();
    }
    let text = String::from_utf8_lossy(&child.stderr).into_owned();
    let status = if !made {
        "harness"
    } else if writer {
        finish_status(!child.timed_out && child.code == Some(1) && text.contains("not-file") && hits_of(&server).is_empty(), &child)
    } else if child.timed_out {
        "failed"
    } else {
        finish_status(child.code == Some(1) && text.contains("not-file"), &child)
    };
    cases.push(case(id, status, "sql fifo rejected as not-file within the deadline", &format!("timeout {} {text}", child.timed_out), &child, hits_of(&server).len(), accepts_of(&server), json!({"writer": writer})));
}

async fn fifo_ca(cases: &mut Vec<Value>, id: &str, writer: bool) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let fifo = dir.join("ca.pem");
    let made = mkfifo(&fifo);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = query_args(&token, &gateway, "local-development", &["--ca-file"]);
    args.push(fifo.clone().into_os_string());
    args.extend(os_args(&[SQL]));
    let writer_thread = if writer {
        let path = fifo.clone();
        Some(thread::spawn(move || {
            let _ = OpenOptions::new().write(true).open(path);
        }))
    } else {
        None
    };
    let mut spec = plan(args, tmp);
    spec.timeout = Duration::from_millis(if writer { 2500 } else { 1200 });
    let child = run_spec(spec);
    if let Some(handle) = writer_thread {
        let _ = handle.join();
    }
    let text = String::from_utf8_lossy(&child.stderr).into_owned();
    let status = if !made {
        "harness"
    } else if writer {
        finish_status(!child.timed_out && child.code == Some(1) && text.contains("not-file") && hits_of(&server).is_empty(), &child)
    } else if child.timed_out {
        "failed"
    } else {
        finish_status(child.code == Some(1) && text.contains("not-file"), &child)
    };
    cases.push(case(id, status, "ca fifo rejected as not-file within the deadline", &format!("timeout {} {text}", child.timed_out), &child, hits_of(&server).len(), accepts_of(&server), json!({"writer": writer})));
}

async fn tmp_and_links(cases: &mut Vec<Value>) {
    tmp_case(cases, "tmpdir_missing", false, false).await;
    tmp_case(cases, "tmpdir_is_file", true, false).await;
    tmp_case(cases, "tmpdir_mode_000", false, true).await;

    let id = "parent_symlink_followed";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let real = dir.join("real");
    fs::create_dir(&real).unwrap();
    let token = real.join("token");
    write_mode(&token, format!("{TOKEN}\n").as_bytes(), 0o600);
    let link = dir.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let via = link.join("token");
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&via, &gateway, "local-development", &["--json", SQL]), tmp));
    let ok = child.code == Some(0) && hits_of(&server).len() == 1;
    cases.push(case(id, finish_status(ok, &child), "parent symlink is followed; this is not a protection", &format!("exit {:?}", child.code), &child, hits_of(&server).len(), accepts_of(&server), json!({"not_a_control": true})));

    let id = "hard_link_same_inode";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let link = dir.join("token-link");
    fs::hard_link(&token, &link).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&link, &gateway, "local-development", &["--json", SQL]), tmp));
    let ok = child.code == Some(0) && hits_of(&server).len() == 1;
    cases.push(case(id, finish_status(ok, &child), "hard link passes the same inode check; not rejected", &format!("exit {:?}", child.code), &child, hits_of(&server).len(), accepts_of(&server), json!({"not_a_control": true})));

    let id = "owner_probe_cleanup";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Stall).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let watch_dir = tmp.clone();
    let watch_seen = Arc::clone(&seen);
    let watch_flag = Arc::clone(&flag);
    let watcher = thread::spawn(move || {
        while watch_flag.load(std::sync::atomic::Ordering::SeqCst) {
            if let Ok(entries) = fs::read_dir(&watch_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with(".oo-cli-owner-") {
                        let meta = entry.metadata().ok();
                        watch_seen.lock().expect("seen").push(json!({
                            "name_prefix": ".oo-cli-owner-",
                            "len": meta.as_ref().map(|m| m.len()),
                            "uid": meta.as_ref().map(|m| m.uid()),
                            "mode": meta.as_ref().map(|m| m.mode() & 0o7777),
                        }));
                    }
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    let mut spec = plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp.clone());
    spec.timeout = Duration::from_millis(800);
    let child = tokio::task::block_in_place(|| run_child(spec));
    flag.store(false, std::sync::atomic::Ordering::SeqCst);
    let _ = watcher.join();
    let leftover = fs::read_dir(&tmp).map(|entries| entries.flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".oo-cli-owner-"))).unwrap_or(true);
    let seen = seen.lock().expect("seen").clone();
    let child_killed = child.timed_out;
    let ok = child_killed && !leftover;
    let _ = fs::remove_dir_all(&tmp);
    cases.push(case(id, if ok { "proved" } else { "failed" }, "probe file is gone after the token read; creation may be observed", &format!("leftover {leftover} seen {}", seen.len()), &child, hits_of(&server).len(), accepts_of(&server), json!({"seen": seen, "exclusive_collision": "unverified", "acl": "unverified", "same_owner_rewrite": "unverified", "zeroization": "not claimed"})));

    let child = ChildOut { code: None, signal: None, timed_out: false, reaped: true, stdout: Vec::new(), stderr: Vec::new(), elapsed_ms: 0, spawn_error: None };
    cases.push(case("acl_same_owner_zeroization", "unverified", "ACL, same-owner inode rewrite, and zeroization are not claimed", "no setfacl race and no process-memory dump; source drops the String without wiping it", &child, 0, 0, json!({})));
}

async fn tmp_case(cases: &mut Vec<Value>, id: &str, as_file: bool, mode_000: bool) {
    let dir = scratch(id);
    let tmp = if as_file {
        let path = dir.join("tmpfile");
        fs::write(&path, b"x").unwrap();
        path
    } else if mode_000 {
        let path = dir.join("tmp");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        path
    } else {
        dir.join("missing-tmp")
    };
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut spec = plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp.clone());
    spec.timeout = Duration::from_secs(2);
    let child = tokio::task::block_in_place(|| run_child(spec));
    if mode_000 {
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o700));
    }
    let text = String::from_utf8_lossy(&child.stderr).into_owned();
    let ok = child.code == Some(4) && !child.timed_out && hits_of(&server).is_empty() && text.contains("token file rejected: other");
    let status = finish_status(ok, &child);
    let _ = fs::remove_dir_all(dir);
    cases.push(case(id, status, "temp probe create fails closed, no connect", &text, &child, 0, accepts_of(&server), json!({})));
}

async fn output_cases(cases: &mut Vec<Value>) {
    json_round(cases, "messy_nulls_kept", MESSY_STANDARD, true).await;
    json_round(cases, "extrema_precision", EXTREMA, true).await;
    markdown_standard(cases).await;
    markdown_hostile(cases).await;
    broken_pipe(cases).await;
    sql_file_bounds(cases).await;
}

async fn json_round(cases: &mut Vec<Value>, id: &str, body: &str, markdown_too: bool) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server({
        let payload = json_ok(body);
        move |_, _| Action::Raw(payload.clone())
    }).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &["--json", SQL]), tmp.clone()));
    let parsed = one_json(&child.stdout);
    let mut notes = Vec::new();
    let mut ok = child.code == Some(0) && child.stderr.is_empty() && parsed.is_ok();
    if let Ok(value) = &parsed {
        if id == "messy_nulls_kept" {
            let null_cell = value.pointer("/rows/1/0").is_some_and(Value::is_null);
            let no_estimated = !value.as_object().unwrap().contains_key("estimated_total_rows");
            let no_star = !value.as_object().unwrap().contains_key("star_expanded");
            let no_tz = value.pointer("/columns/0/timezone").is_none();
            let bytes_match = child.stdout == EXPECTED_STANDARD.as_bytes();
            ok = ok && null_cell && no_estimated && no_star && no_tz && bytes_match;
            notes.push(format!("null_cell {null_cell} bytes_match {bytes_match}"));
        }
        if id == "extrema_precision" {
            let raw = String::from_utf8_lossy(&child.stdout);
            let digits = raw.contains("9007199254740993") && !raw.contains("9007199254740992");
            let decimal = raw.contains("123456789012345678901234567890.123456789012345678901234567890");
            let pos = raw.contains("18446744073709551615");
            let total = value.pointer("/estimated_total_rows").and_then(Value::as_u64) == Some(424242);
            let null_cell = value.pointer("/rows/0/5").is_some_and(Value::is_null);
            ok = ok && digits && decimal && pos && total && null_cell;
            notes.push(format!("digits {digits} decimal {decimal} total {total} null {null_cell}"));
        }
    }
    cases.push(case(id, finish_status(ok, &child), "one JSON document, null cell kept, optional nulls omitted", &notes.join("; "), &child, hits_of(&server).len(), accepts_of(&server), json!({})));
    if !markdown_too {
        return;
    }
    let server = spawn_server({
        let payload = json_ok(body);
        move |_, _| Action::Raw(payload.clone())
    }).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let tmp = dir.join("tmp2");
    fs::create_dir(&tmp).unwrap();
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stdout).into_owned();
    let ok = if id == "extrema_precision" {
        child.code == Some(0)
            && child.stderr.is_empty()
            && text.contains("9007199254740993")
            && !text.contains("9007199254740992")
            && !text.contains("estimated_total_rows")
            && !text.contains("424242")
            && text.contains("-- warning: code=STALE_DATA")
            && text.contains("-- warning: code=CROSS_TABLE_SKEW")
            && text.contains("-- truncated: true")
    } else {
        child.code == Some(0) && child.stderr.is_empty() && !text.contains("estimated_total_rows") && text.contains("| null |")
    };
    cases.push(case(&format!("{id}_markdown"), finish_status(ok, &child), "markdown keeps digits and does not print estimated_total_rows", "see stdout", &child, hits_of(&server).len(), accepts_of(&server), json!({})));
}

async fn markdown_standard(cases: &mut Vec<Value>) {
    let id = "markdown_footer";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(STANDARD))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stdout).into_owned();
    let expected = "\
| amount |
| --- |
| 12.50 |
| null |
| true |
-- query_id: q_abc123
-- returned_rows: 3
-- duration_ms: 12
-- truncated: false
-- truncation_reasons:
-- freshness: table=logistics.receipt as_of=2026-10-01T01:12:03Z lag_ms=0 state=live
-- source: source=tms_pg consistency=transactional cross_table_consistent=true applied_pos=18446744073709551615
-- warning: code=STALE_DATA rows=2 message=lag
-- hints:
-- knowledge_version: oos/v1
-- policy_version: type-policy/v1
-- star_expanded: none
";
    let ok = child.code == Some(0) && child.stderr.is_empty() && text == expected;
    cases.push(case(id, finish_status(ok, &child), "handwritten footer and cells", &format!("len {}", text.len()), &child, hits_of(&server).len(), accepts_of(&server), json!({})));

    let id = "truncated_exit_0";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(TRUNCATED))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stdout).into_owned();
    let ok = child.code == Some(0) && text.contains("-- truncated: true") && text.contains("-- truncation_reasons: rows") && text.contains("-- warning: code=STALE_DATA");
    cases.push(case(id, finish_status(ok, &child), "truncated success is exit 0 and shows the reason and warning", &format!("exit {:?}", child.code), &child, hits_of(&server).len(), accepts_of(&server), json!({})));

    let id = "empty_star_none";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stdout).into_owned();
    let ok = child.code == Some(0) && text.contains("-- star_expanded: none") && text.contains("-- returned_rows: 0") && text.contains("-- warnings:");
    cases.push(case(id, finish_status(ok, &child), "absent star_expanded renders none", &text, &child, 1, accepts_of(&server), json!({})));
}

async fn markdown_hostile(cases: &mut Vec<Value>) {
    let id = "markdown_hostile_contract_fixture";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(HOSTILE))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stdout).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    let forged = lines.iter().any(|line| *line == "-- query_id: q_forged");
    let real = lines.iter().any(|line| *line == "-- query_id: q_hostile1");
    let header = lines.first().copied().unwrap_or("");
    let cells = table_cells(lines.get(2).copied().unwrap_or(""));
    let recovered = cells.as_ref().and_then(|c| c.first()).cloned().unwrap_or_default();
    let raw_has_esc = !text.contains('\u{1b}') && text.contains("\\u{1b}") && text.contains("\\u{202e}") && text.contains("\\|");
    let ok = child.code == Some(0) && child.stderr.is_empty() && real && !forged && raw_has_esc && recovered.contains("-- query_id: q_forged") && header.contains("label");
    cases.push(case(id, finish_status(ok, &child), "fixture hostile cell round-trips and does not forge a footer line", &format!("forged {forged} recovered_len {}", recovered.len()), &child, hits_of(&server).len(), accepts_of(&server), json!({"recovered_has_forged": recovered.contains("q_forged")})));

    let id = "markdown_escape_limits";
    let body = r#"{"query_id":"q_limits1","columns":[{"name":"a|b","type":"text"}],"rows":[["p/q <b>x</b> [z](http://e.example) `\n-- query_id: q_forged\r\t\u001b\u202e\u200b\u0007"]],"returned_rows":1,"truncated":false,"duration_ms":1,"freshness":[],"sources":[],"warnings":[{"code":"STALE_DATA","message":"hide\n-- query_id: q_forged"}],"hints":["hint\n-- hint: forged"],"knowledge_version":"v1","policy_version":"type-policy/v1","star_expanded":[]}"#;
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server({
        let payload = json_ok(body);
        move |_, _| Action::Raw(payload.clone())
    }).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stdout).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    let data = lines.get(2).copied().unwrap_or("");
    let cells = table_cells(data);
    let cell = cells.as_ref().and_then(|c| c.first()).cloned().unwrap_or_default();
    let slash = text.contains("p/q");
    let html = text.contains("<b>x</b>");
    let link = text.contains("[z](http://e.example)");
    let no_raw_controls = !text.chars().any(|ch| ch == '\u{1b}' || ch == '\u{202e}' || ch == '\u{200b}' || ch == '\u{7}');
    let no_forged = !lines.iter().any(|line| *line == "-- query_id: q_forged" || *line == "-- hint: forged");
    let warning = text.contains("-- warning: code=STALE_DATA") && text.contains("hide\\n");
    let ok = child.code == Some(0)
        && child.stderr.is_empty()
        && slash
        && html
        && link
        && no_raw_controls
        && no_forged
        && warning
        && cell.contains("p/q")
        && cell.contains('\n')
        && cell.contains('\u{200b}');
    cases.push(case(
        id,
        finish_status(ok, &child),
        "escapes C0/bidi/zero-width/pipe/newline; does not escape slash, HTML, or markdown links",
        &format!("slash {slash} html {html} link {link} controls_hidden {no_raw_controls} forged {}", !no_forged),
        &child,
        hits_of(&server).len(),
        accepts_of(&server),
        json!({"cell_len": cell.len()}),
    ));

    let child = ChildOut { code: None, signal: None, timed_out: false, reaped: true, stdout: Vec::new(), stderr: Vec::new(), elapsed_ms: 0, spawn_error: None };
    cases.push(case("render_16mib_cap", "unverified", "16 MiB render rejection executed through the wire", "HTTP body cap is 1 MiB. Documented escapes expand by about 2x, so the post-allocation 16 MiB check was not reached. Source checks the length after the full string exists.", &child, 0, 0, json!({})));
}

async fn broken_pipe(cases: &mut Vec<Value>) {
    let id = "broken_pipe_no_replay";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(STANDARD))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut spec = plan(query_args(&token, &gateway, "local-development", &[SQL]), tmp);
    spec.broken_stdout = true;
    spec.timeout = Duration::from_secs(4);
    let child = run_spec(spec);
    let ok = child.code == Some(1)
        && hits_of(&server).len() == 1
        && contains(&child.stderr, "output delivery failed")
        && !contains(&child.stderr, "delivered");
    cases.push(case(id, finish_status(ok, &child), "exit 1, one hit, delivery failed, no replay", &format!("hits {}", hits_of(&server).len()), &child, hits_of(&server).len(), accepts_of(&server), json!({"signal": child.signal})));
}

async fn sql_file_bounds(cases: &mut Vec<Value>) {
    let id = "sql_file_crlf_and_bom";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let path = dir.join("q.sql");
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(b"select 1\r\n");
    fs::write(&path, &bytes).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = query_args(&token, &gateway, "local-development", &["-f"]);
    args.push(path.into_os_string());
    let child = run_spec(plan(args, tmp));
    let hits = hits_of(&server);
    let sql = String::from_utf8_lossy(&bytes).into_owned();
    let expected = canonical_body(&sql, None, None);
    let ok = child.code == Some(0) && hits.len() == 1 && hits[0].body == expected;
    cases.push(case(id, finish_status(ok, &child), "BOM and CR stay in the SQL bytes", &format!("match {}", hits.first().is_some_and(|hit| hit.body == expected)), &child, hits.len(), accepts_of(&server), json!({})));

    let id = "sql_file_65536";
    file_budget(cases, id, 65_536, true).await;
    let id = "sql_file_65537";
    file_budget(cases, id, 65_537, false).await;

    let id = "sql_symlink";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let real = dir.join("real.sql");
    fs::write(&real, b"select 1").unwrap();
    let link = dir.join("q.sql");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = query_args(&token, &gateway, "local-development", &["-f"]);
    args.push(link.into_os_string());
    let child = run_spec(plan(args, tmp));
    let text = String::from_utf8_lossy(&child.stderr).into_owned();
    let ok = child.code == Some(1) && hits_of(&server).is_empty() && text.contains("symlink") && !text.contains("select 1");
    cases.push(case(id, finish_status(ok, &child), "sql symlink rejected before connect", &text, &child, 0, accepts_of(&server), json!({})));
}

async fn file_budget(cases: &mut Vec<Value>, id: &str, len: usize, allow: bool) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let path = dir.join("q.sql");
    fs::write(&path, vec![b'a'; len]).unwrap();
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut args = query_args(&token, &gateway, "local-development", &["-f"]);
    args.push(path.into_os_string());
    let child = run_spec(plan(args, tmp));
    let hits = hits_of(&server);
    let text = format!("{}{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
    let ok = if allow {
        hits.len() == 1 && child.code == Some(0) && hits[0].body.len() == len + "{\"format\":\"json\",\"sql\":\"\"}".len()
    } else {
        hits.is_empty() && child.code == Some(1) && text.contains("budget")
    };
    cases.push(case(id, finish_status(ok, &child), if allow { "65536 byte SQL is sent" } else { "65537 byte SQL is local budget, no connect" }, &format!("hits {} exit {:?}", hits.len(), child.code), &child, hits.len(), accepts_of(&server), json!({})));
}

async fn outcome_cases(cases: &mut Vec<Value>) {
    reject_case(cases, "duplicate_key", 200, "OK", DUPLICATE, None, 1, 1, "protocol").await;
    reject_case(cases, "unknown_field", 200, "OK", UNKNOWN, None, 1, 1, "protocol").await;
    reject_case(cases, "malformed", 200, "OK", "{", None, 1, 1, "protocol").await;
    reject_case(cases, "row_width", 200, "OK", WIDTH, None, 1, 1, "protocol").await;
    reject_case(cases, "returned_rows_mismatch", 200, "OK", COUNT, None, 1, 1, "protocol").await;
    reject_case(cases, "number_cell", 200, "OK", NUMBER_CELL, None, 1, 1, "protocol").await;
    reject_case(cases, "inconsistent_retryable", 429, "Too Many Requests", INCONSISTENT, Some("0"), 1, 1, "protocol").await;
    reject_case(cases, "status_mismatch", 400, "Bad Request", RATE, None, 1, 1, "protocol").await;
    reject_case(cases, "sql_parse_exit_2", 400, "Bad Request", SQL_ERR, None, 1, 2, "gateway").await;
    reject_case(cases, "timeout_code_exit_2", 408, "Request Timeout", TIMEOUT_BODY, None, 1, 2, "gateway").await;
    reject_case(cases, "unauth_one_hit", 401, "Unauthorized", UNAUTH, None, 1, 4, "auth").await;
    reject_case(cases, "internal_no_replay", 500, "Internal Server Error", INTERNAL, Some("0"), 1, 1, "gateway").await;
    reject_case(cases, "audit_unavailable_no_replay", 503, "Service Unavailable", AUDIT, Some("0"), 1, 1, "gateway").await;
    reject_case(cases, "rate_after_9", 429, "Too Many Requests", RATE, Some("9"), 1, 3, "gateway").await;
    reject_case(cases, "rate_after_2_equal_cap", 429, "Too Many Requests", RATE, Some("2"), 1, 3, "gateway").await;
    reject_case(cases, "rate_after_0_four_hits", 429, "Too Many Requests", RATE, Some("0"), 4, 3, "gateway").await;
    reject_case(cases, "overloaded_after_0_four_hits", 503, "Service Unavailable", OVERLOADED, Some("0"), 4, 3, "gateway").await;
    retry_then_success(cases).await;
    partial_and_redirect(cases).await;
    timeout_cases(cases).await;
    proxy_and_endpoint(cases).await;
    tls_cases(cases).await;
    privacy_reflection(cases).await;
}

async fn reject_case(
    cases: &mut Vec<Value>,
    id: &str,
    status: u16,
    reason: &str,
    body: &str,
    retry_after: Option<&str>,
    hits_expected: usize,
    exit: i32,
    kind: &str,
) {
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let payload = json_status(status, reason, body, retry_after);
    let server = spawn_server(move |index, _| {
        if index + 1 < 6 {
            Action::Raw(payload.clone())
        } else {
            Action::Raw(payload.clone())
        }
    }).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut spec = plan(query_args(&token, &gateway, "local-development", &["--json", SQL]), tmp);
    spec.timeout = Duration::from_secs(8);
    let child = run_spec(spec);
    let hits = hits_of(&server).len();
    let parsed = one_json(&child.stdout).ok();
    let schema_ok = parsed.as_ref().and_then(|value| error_schema(value).ok()).is_some();
    let class_ok = match kind {
        "protocol" => parsed.as_ref().and_then(|v| v.get("class")).and_then(Value::as_str) == Some("protocol"),
        "gateway" => parsed.as_ref().and_then(|v| v.get("class")).and_then(Value::as_str) == Some("gateway"),
        "auth" => parsed.as_ref().and_then(|v| v.get("class")).and_then(Value::as_str) == Some("auth")
            || parsed.as_ref().and_then(|v| v.get("code")).and_then(Value::as_str) == Some("UNAUTHENTICATED"),
        _ => false,
    };
    let secret_free = privacy_ok(&child, &gateway);
    let ok = child.code == Some(exit) && hits == hits_expected && child.stderr.is_empty() && schema_ok && class_ok && secret_free && !child.timed_out;
    cases.push(case(id, finish_status(ok, &child), &format!("exit {exit}, hits {hits_expected}, {kind}, one JSON error"), &format!("hits {hits} schema {schema_ok} class {class_ok}"), &child, hits, accepts_of(&server), json!({})));
}

async fn retry_then_success(cases: &mut Vec<Value>) {
    let id = "rate_after_1_then_success";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let limited = json_status(429, "Too Many Requests", RATE, Some("1"));
    let ok_body = json_ok(EMPTY);
    let server = spawn_server(move |index, _| {
        if index == 0 {
            Action::Raw(limited.clone())
        } else {
            Action::Raw(ok_body.clone())
        }
    }).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut spec = plan(query_args(&token, &gateway, "local-development", &["--json", SQL]), tmp);
    spec.timeout = Duration::from_secs(8);
    let started = Instant::now();
    let child = run_spec(spec);
    let elapsed = started.elapsed().as_millis();
    let hits = hits_of(&server).len();
    let ok = child.code == Some(0) && hits == 2 && child.stderr.is_empty() && child.elapsed_ms >= 800 && child.elapsed_ms < 5000;
    let _ = elapsed;
    cases.push(case(id, finish_status(ok, &child), "one Retry-After 1 then success, no second CLI loop, stderr empty", &format!("hits {hits} elapsed {}", child.elapsed_ms), &child, hits, accepts_of(&server), json!({})));
}

async fn partial_and_redirect(cases: &mut Vec<Value>) {
    let id = "partial_body_no_replay";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let partial = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 80\r\nConnection: close\r\n\r\n{".to_vec();
    let server = spawn_server(move |_, _| Action::CloseAfter(partial.clone())).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &["--json", SQL]), tmp));
    let ok = child.code == Some(4) && hits_of(&server).len() == 1 && contains(&child.stdout, "transport");
    cases.push(case(id, finish_status(ok, &child), "partial body is transport exit 4, one hit", &format!("hits {}", hits_of(&server).len()), &child, hits_of(&server).len(), accepts_of(&server), json!({})));

    let id = "redirect_no_second_door";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let second = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let loc = format!("http://127.0.0.1:{}/v1/query", second.port);
    let first = spawn_server(move |_, _| {
        Action::Raw(http_response(302, "Found", &[("Location", &loc)], b""))
    }).await;
    let gateway = format!("http://127.0.0.1:{}/", first.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &["--json", SQL]), tmp));
    let ok = child.code == Some(1) && hits_of(&first).len() == 1 && hits_of(&second).is_empty() && contains(&child.stdout, "redirect");
    cases.push(case(id, finish_status(ok, &child), "redirect exit 1, second door gets no request", &format!("first {} second {}", hits_of(&first).len(), hits_of(&second).len()), &child, hits_of(&first).len(), accepts_of(&first), json!({"second_hits": hits_of(&second).len(), "second_accepts": accepts_of(&second)})));

    let id = "declared_oversize";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2000000\r\nConnection: close\r\n\r\n".to_vec();
    let server = spawn_server(move |_, _| Action::CloseAfter(raw.clone())).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &["--json", SQL]), tmp));
    let ok = child.code == Some(1) && hits_of(&server).len() == 1 && contains(&child.stdout, "protocol");
    cases.push(case(id, finish_status(ok, &child), "declared oversize is protocol exit 1, one hit", &format!("hits {}", hits_of(&server).len()), &child, hits_of(&server).len(), accepts_of(&server), json!({})));
}

async fn timeout_cases(cases: &mut Vec<Value>) {
    let id = "timeout_flag_tightens";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Stall).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut spec = plan(query_args(&token, &gateway, "local-development", &["--timeout-ms", "800", "--json", SQL]), tmp);
    spec.timeout = Duration::from_secs(6);
    let child = run_spec(spec);
    let ok = child.code == Some(4) && hits_of(&server).len() == 1 && child.elapsed_ms < 3500 && child.elapsed_ms > 400;
    cases.push(case(id, finish_status(ok, &child), "timeout-ms 800 finishes near 800ms, one hit", &format!("elapsed {}", child.elapsed_ms), &child, hits_of(&server).len(), accepts_of(&server), json!({})));

    let id = "timeout_ceiling_stays_30s";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Stall).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let mut spec = plan(query_args(&token, &gateway, "local-development", &["--timeout-ms", "120000", "--json", SQL]), tmp);
    spec.timeout = Duration::from_secs(45);
    let child = run_spec(spec);
    let ok = child.code == Some(4) && hits_of(&server).len() == 1 && (25_000..40_000).contains(&child.elapsed_ms);
    cases.push(case(id, finish_status(ok, &child), "timeout-ms 120000 does not raise the 30s production budget", &format!("elapsed {}", child.elapsed_ms), &child, hits_of(&server).len(), accepts_of(&server), json!({})));
}

async fn proxy_and_endpoint(cases: &mut Vec<Value>) {
    let id = "proxy_env_ignored";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let proxy = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let real = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", real.port);
    let proxy_url = format!("http://127.0.0.1:{}/", proxy.port);
    let mut spec = plan(query_args(&token, &gateway, "local-development", &["--json", SQL]), tmp);
    spec.proxy = Some(proxy_url);
    let child = run_spec(spec);
    let ok = child.code == Some(0) && hits_of(&real).len() == 1 && hits_of(&proxy).is_empty() && accepts_of(&proxy) == 0;
    cases.push(case(id, finish_status(ok, &child), "HTTP_PROXY and friends receive no connection", &format!("real {} proxy_accepts {}", hits_of(&real).len(), accepts_of(&proxy)), &child, hits_of(&real).len(), accepts_of(&real), json!({"proxy_accepts": accepts_of(&proxy)})));

    let id = "production_http_before_connect";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(json_ok(EMPTY))).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "production", &["--json", SQL]), tmp));
    let text = String::from_utf8_lossy(&child.stdout).into_owned();
    let ok = child.code == Some(1) && accepts_of(&server) == 0 && text.contains("endpoint rejected") && !text.contains(&gateway);
    cases.push(case(id, finish_status(ok, &child), "production plaintext loopback rejected before accept", &text, &child, 0, accepts_of(&server), json!({})));

    let id = "local_non_loopback_before_connect";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let started = Instant::now();
    let child = run_spec(plan(query_args(&token, "http://192.0.2.1:9/", "local-development", &["--json", SQL]), tmp));
    let fast = started.elapsed() < Duration::from_millis(1500);
    let text = String::from_utf8_lossy(&child.stdout).into_owned();
    let ok = child.code == Some(1) && fast && text.contains("endpoint rejected") && !text.contains("192.0.2.1");
    cases.push(case(id, finish_status(ok, &child), "non-loopback HTTP rejected locally and the address is not printed", &format!("fast {fast} elapsed {}", child.elapsed_ms), &child, 0, 0, json!({})));
}

async fn tls_cases(cases: &mut Vec<Value>) {
    let dir = scratch("tls_root");
    let ca = dir.join("ca.pem");
    fs::write(&ca, CA_PEM).unwrap();
    fs::set_permissions(&ca, fs::Permissions::from_mode(0o644)).unwrap();
    let ok_body = json_ok(EMPTY);

    let id = "tls_positive";
    match spawn_tls(LOCALHOST_PEM, LOCALHOST_KEY, ok_body.clone()).await {
        Ok(server) => {
            let tmp = dir.join(id);
            fs::create_dir(&tmp).unwrap();
            let token = token_file(&dir);
            let gateway = format!("https://localhost:{}/", server.port);
            let mut args = query_args(&token, &gateway, "production", &["--ca-file"]);
            args.push(ca.clone().into_os_string());
            args.extend(os_args(&["--json", SQL]));
            let mut spec = plan(args, tmp);
            spec.timeout = Duration::from_secs(8);
            let child = run_spec(spec);
            let hits = hits_of(&server);
            let expected = canonical_body(SQL, None, None);
            let ok = child.code == Some(0) && hits.len() == 1 && wire_ok(&hits[0], &expected);
            cases.push(case(id, finish_status(ok, &child), "production TLS with the fixture CA succeeds", &format!("hits {}", hits.len()), &child, hits.len(), accepts_of(&server), json!({})));
        }
        Err(error) => {
            let child = ChildOut { code: None, signal: None, timed_out: false, reaped: true, stdout: Vec::new(), stderr: error.into_bytes(), elapsed_ms: 0, spawn_error: None };
            cases.push(case(id, "harness", "tls server", "cert setup failed", &child, 0, 0, json!({})));
        }
    }

    let id = "tls_wrong_host";
    match spawn_tls(LOCALHOST_PEM, LOCALHOST_KEY, ok_body.clone()).await {
        Ok(server) => {
            let tmp = dir.join(id);
            fs::create_dir(&tmp).unwrap();
            let token = token_file(&dir);
            let gateway = format!("https://127.0.0.1:{}/", server.port);
            let mut args = query_args(&token, &gateway, "production", &["--ca-file"]);
            args.push(ca.clone().into_os_string());
            args.extend(os_args(&["--json", SQL]));
            let child = run_spec(plan(args, tmp));
            let ok = child.code == Some(4) && hits_of(&server).is_empty() && accepts_of(&server) <= 1 && !contains(&child.stdout, TOKEN);
            cases.push(case(id, finish_status(ok, &child), "hostname mismatch is transport, no HTTP replay", &format!("http_hits {} accepts {}", hits_of(&server).len(), accepts_of(&server)), &child, hits_of(&server).len(), accepts_of(&server), json!({})));
        }
        Err(error) => {
            let child = ChildOut { code: None, signal: None, timed_out: false, reaped: true, stdout: Vec::new(), stderr: error.into_bytes(), elapsed_ms: 0, spawn_error: None };
            cases.push(case(id, "harness", "tls", "setup failed", &child, 0, 0, json!({})));
        }
    }

    let id = "tls_wrong_ca";
    match spawn_tls(OTHER_PEM, OTHER_KEY, ok_body.clone()).await {
        Ok(server) => {
            let tmp = dir.join(id);
            fs::create_dir(&tmp).unwrap();
            let token = token_file(&dir);
            let gateway = format!("https://localhost:{}/", server.port);
            let mut args = query_args(&token, &gateway, "production", &["--ca-file"]);
            args.push(ca.clone().into_os_string());
            args.extend(os_args(&["--json", SQL]));
            let child = run_spec(plan(args, tmp));
            let ok = child.code == Some(4) && hits_of(&server).is_empty();
            cases.push(case(id, finish_status(ok, &child), "wrong CA is transport, no HTTP request", &format!("hits {}", hits_of(&server).len()), &child, hits_of(&server).len(), accepts_of(&server), json!({})));
        }
        Err(error) => {
            let child = ChildOut { code: None, signal: None, timed_out: false, reaped: true, stdout: Vec::new(), stderr: error.into_bytes(), elapsed_ms: 0, spawn_error: None };
            cases.push(case(id, "harness", "tls", "setup failed", &child, 0, 0, json!({})));
        }
    }

    let id = "ca_symlink";
    let link = dir.join("ca-link");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&ca, &link).unwrap();
    match spawn_tls(LOCALHOST_PEM, LOCALHOST_KEY, ok_body).await {
        Ok(server) => {
            let tmp = dir.join(id);
            fs::create_dir(&tmp).unwrap();
            let token = token_file(&dir);
            let gateway = format!("https://localhost:{}/", server.port);
            let mut args = query_args(&token, &gateway, "production", &["--ca-file"]);
            args.push(link.into_os_string());
            args.extend(os_args(&["--json", SQL]));
            let child = run_spec(plan(args, tmp));
            let text = String::from_utf8_lossy(&child.stdout).into_owned();
            let ok = child.code == Some(1) && hits_of(&server).is_empty() && text.contains("symlink");
            cases.push(case(id, finish_status(ok, &child), "symlink CA rejected before TLS", &text, &child, hits_of(&server).len(), accepts_of(&server), json!({})));
        }
        Err(error) => {
            let child = ChildOut { code: None, signal: None, timed_out: false, reaped: true, stdout: Vec::new(), stderr: error.into_bytes(), elapsed_ms: 0, spawn_error: None };
            cases.push(case(id, "harness", "tls", "setup failed", &child, 0, 0, json!({})));
        }
    }
}

async fn privacy_reflection(cases: &mut Vec<Value>) {
    let id = "gateway_text_hides_token_sql_host";
    let dir = scratch(id);
    let tmp = dir.join("tmp");
    fs::create_dir(&tmp).unwrap();
    let token = token_file(&dir);
    let server = spawn_server(|_, _| Action::Raw(Vec::new())).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let sql = "select a103_privacy_marker";
    let message = format!("leak {TOKEN} and {sql} at {gateway} Bearer {TOKEN}");
    let body = format!(
        r#"{{"error":{{"code":"SQL_PARSE_ERROR","message":{},"hint":"fix","retryable":false,"query_id":"q_abc123"}}}}"#,
        json_string(&message)
    );
    drop(server);
    let payload = json_status(400, "Bad Request", &body, None);
    let server = spawn_server(move |_, _| Action::Raw(payload.clone())).await;
    let gateway = format!("http://127.0.0.1:{}/", server.port);
    let child = run_spec(plan(query_args(&token, &gateway, "local-development", &["--json", sql]), dir.join("tmp2")));
    let _ = fs::create_dir_all(dir.join("tmp2"));
    let hidden = !contains(&child.stdout, TOKEN)
        && !contains(&child.stderr, TOKEN)
        && !contains(&child.stdout, "a103_privacy_marker")
        && !contains(&child.stderr, "a103_privacy_marker")
        && !contains(&child.stdout, "127.0.0.1")
        && !contains(&child.stderr, "127.0.0.1")
        && !contains(&child.stdout, "Bearer ");
    let ok = child.code == Some(2) && hits_of(&server).len() == 1 && hidden;
    cases.push(case(id, finish_status(ok, &child), "reflected token, SQL, and gateway host are absent", &format!("hidden {hidden}"), &child, hits_of(&server).len(), accepts_of(&server), json!({})));

    let child = ChildOut { code: None, signal: None, timed_out: false, reaped: true, stdout: Vec::new(), stderr: Vec::new(), elapsed_ms: 0, spawn_error: None };
    cases.push(case(
        "disconnect_is_not_cancel_or_zeroization",
        "unverified",
        "HTTP disconnect does not prove backend cancel or token zeroization",
        "not claimed; no PostgreSQL session was opened",
        &child,
        0,
        0,
        json!({}),
    ));
}
