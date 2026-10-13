//! Private PostgreSQL proof for the loopback HTTP gateway.
//!
//! The test fails closed when `OO_A326_LIVE` is not `1`. Cargo leaves it ignored
//! until the proof passes `--ignored`. The packs are the raw spans from
//! `role-pack-bundle` via the a341 fixture tool. This test does not call
//! `fixture_artifact`.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use oo_gateway::http_gateway::{
    CurrentTuple, Endpoint, IssueGrant, TokenKind, harness_install, harness_issue,
    harness_revoke_token,
};
use serde_json::Value;
use zeroize::Zeroize;

const AUDIENCE: &str = "https://gateway.loopback.invalid/v1";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires OO_A326_LIVE=1 and the private PostgreSQL"]
async fn issued_token_reads_its_pack_and_failures_stay_closed() {
    let live = std::env::var("OO_A326_LIVE").unwrap_or_default();
    assert_eq!(
        live, "1",
        "OO_A326_LIVE=1 is required; refusing a green skip"
    );
    let host = required_env("OO_A326_PG_HOST");
    let port: u16 = required_env("OO_A326_PG_PORT").parse().expect("port");
    let database = required_env("OO_A326_PG_DATABASE");
    let admin_path = PathBuf::from(required_env("OO_A326_ADMIN_PASSWORD_FILE"));
    let runtime_path = PathBuf::from(required_env("OO_A326_RUNTIME_PASSWORD_FILE"));
    let ca_path = PathBuf::from(required_env("OO_A326_CA_FILE"));
    let marker_path = PathBuf::from(required_env("OO_A326_MARKER"));
    std::fs::write(&marker_path, b"ran=1\n").expect("marker");

    let mut admin = read_secret(&admin_path);
    let mut runtime = read_secret(&runtime_path);
    let ca = std::fs::read(&ca_path).expect("ca");
    let endpoint = Endpoint {
        host: host.clone(),
        port,
        database: database.clone(),
    };
    let admin_password = trim_secret(&admin);
    harness_install(endpoint.clone(), admin_password, &ca)
        .await
        .expect("install");
    let mut picker = harness_issue(
        endpoint.clone(),
        admin_password,
        &ca,
        client_grant("oo_r_picker", "picker-user"),
    )
    .await
    .expect("issue picker");
    let mut auditor = harness_issue(
        endpoint.clone(),
        admin_password,
        &ca,
        client_grant("oo_r_auditor", "auditor-user"),
    )
    .await
    .expect("issue auditor");
    let mut session = harness_issue(endpoint.clone(), admin_password, &ca, session_grant())
        .await
        .expect("issue session");
    admin.zeroize();

    let config = write_config(&runtime_path, &ca_path, &host, port, &database);
    let mut child = spawn_gateway(&config);
    let addr = wait_addr(&config);
    let mut picker_token = picker.0.clone();
    let mut auditor_token = auditor.0.clone();
    let mut session_token = session.0.clone();

    let (status, body) = http(addr, "GET", "/health", None, None);
    assert_eq!(status, 200, "health");
    assert_boundary(&body);

    let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&picker_token), None);
    assert_eq!(status, 200, "picker metadata");
    assert!(
        body_text(&body).contains("picker_note"),
        "picker column missing"
    );
    assert!(
        !body_text(&body).contains("auditor_phone"),
        "auditor column leaked"
    );
    assert!(!body_text(&body).contains(&picker_token), "bearer echoed");
    let meta: Value = serde_json::from_slice(&body).expect("metadata json");
    assert_eq!(meta["project_id"], "depot");
    assert_eq!(meta["role"], "oo_r_picker");
    assert_eq!(meta["scope"], "client");
    assert_eq!(meta["authentication"], "loopback-http");
    assert_eq!(meta["external_mtls"], false);
    assert_eq!(meta["oauth"], false);
    assert_eq!(meta["idp"], false);
    assert_eq!(meta["protocol"], "oo.builder.role-pack/v1");

    for (name, value) in [
        ("X-OO-Role", "oo_r_auditor"),
        ("X-OO-Project", "other"),
        ("X-OO-Scope", "session"),
    ] {
        let (status, body) = http(
            addr,
            "GET",
            "/v1/pack/metadata",
            Some(&picker_token),
            Some((name, value)),
        );
        assert_eq!(status, 403, "{name}");
        let text = body_text(&body);
        assert!(
            !text.contains("picker_note") && !text.contains("auditor_phone"),
            "hidden pack bytes"
        );
        assert!(!text.contains(&picker_token), "bearer echoed");
    }

    let (status, body) = http(
        addr,
        "GET",
        "/v1/session/current",
        Some(&session_token),
        None,
    );
    assert_eq!(status, 401, "session token is not an external client");
    assert!(
        !body_text(&body).contains(&session_token),
        "session bearer echoed"
    );

    let (status, _) = http(addr, "GET", "/v1/pack/metadata", None, None);
    assert_eq!(status, 401, "missing bearer");

    let query = format!("/v1/pack/metadata?access_token={picker_token}");
    let (status, body) = http(addr, "GET", &query, Some(&picker_token), None);
    assert_eq!(status, 400, "query");
    assert!(
        !body_text(&body).contains(&picker_token),
        "query bearer echoed"
    );

    let (status, body) = http(addr, "POST", "/v1/sql", Some(&picker_token), None);
    assert_eq!(status, 501, "sql");
    assert!(
        !body_text(&body).contains(&picker_token),
        "sql bearer echoed"
    );

    let mut admin = read_secret(&admin_path);
    harness_revoke_token(
        endpoint.clone(),
        trim_secret(&admin),
        &ca,
        "depot".to_owned(),
        picker.1.clone(),
    )
    .await
    .expect("revoke");
    admin.zeroize();
    std::thread::sleep(Duration::from_millis(5_200));
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&picker_token), None);
    assert_eq!(status, 401, "revoked after cache expiry");

    stop(&mut child);
    let mut child = spawn_gateway(&config);
    let addr = wait_addr(&config);
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&picker_token), None);
    assert_eq!(status, 401, "revoked after restart");
    let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor_token), None);
    assert_eq!(status, 200, "auditor after restart");
    assert!(
        body_text(&body).contains("auditor_phone"),
        "auditor column missing"
    );
    assert!(
        !body_text(&body).contains("picker_note"),
        "picker column leaked"
    );

    std::fs::write(
        config.join("endpoint.json"),
        format!("{{\"host\":\"127.0.0.1\",\"port\":1,\"database\":\"{database}\"}}\n"),
    )
    .expect("outage endpoint");
    std::thread::sleep(Duration::from_millis(5_200));
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor_token), None);
    assert_eq!(status, 401, "database outage");
    let (status, ready) = http(addr, "GET", "/ready", None, None);
    assert_eq!(status, 200, "ready during outage");
    let ready: Value = serde_json::from_slice(&ready).expect("ready json");
    assert_eq!(ready["database"], "not_probed");
    std::fs::write(
        config.join("endpoint.json"),
        format!("{{\"host\":\"{host}\",\"port\":{port},\"database\":\"{database}\"}}\n"),
    )
    .expect("restore endpoint");
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor_token), None);
    assert_eq!(status, 200, "restored database");

    let stderr = std::fs::read(config.join("stderr.txt")).unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr);
    assert!(
        !stderr_text.contains(runtime.trim_end_matches(['\n', '\r'])),
        "password in stderr"
    );
    assert!(!stderr_text.contains("ooct_"), "client bearer in stderr");
    assert!(!stderr_text.contains("oost_"), "session bearer in stderr");
    assert!(
        !stderr_text.contains(&picker_token),
        "picker bearer in stderr"
    );
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", child.id())).expect("cmdline");
    let joined = String::from_utf8_lossy(&cmdline);
    assert!(
        !joined.contains(runtime.trim_end_matches(['\n', '\r'])),
        "password in argv"
    );
    assert!(
        !joined.contains("ooct_") && !joined.contains("oost_"),
        "bearer in argv"
    );

    runtime.zeroize();
    picker.0.zeroize();
    auditor.0.zeroize();
    session.0.zeroize();
    picker_token.zeroize();
    auditor_token.zeroize();
    session_token.zeroize();
    stop(&mut child);
    std::fs::write(&marker_path, b"ran=1\ncompleted=1\n").expect("marker done");
}

fn trim_secret(value: &str) -> &str {
    value.trim_end_matches(['\n', '\r'])
}

fn client_grant(role: &str, principal: &str) -> IssueGrant {
    IssueGrant {
        kind: TokenKind::Client,
        client_ttl_ms: None,
        current: CurrentTuple {
            project_id: "depot".to_owned(),
            principal_id: principal.to_owned(),
            role: role.to_owned(),
            claims: vec![("region".to_owned(), "north".to_owned())],
            policy_version: "pv1".to_owned(),
            knowledge_version: "kv1".to_owned(),
            audience_url: AUDIENCE.to_owned(),
            sandbox_id: None,
            session_id: None,
        },
    }
}

fn session_grant() -> IssueGrant {
    IssueGrant {
        kind: TokenKind::Session,
        client_ttl_ms: None,
        current: CurrentTuple {
            project_id: "depot".to_owned(),
            principal_id: "picker-user".to_owned(),
            role: "oo_r_picker".to_owned(),
            claims: vec![("region".to_owned(), "north".to_owned())],
            policy_version: "pv1".to_owned(),
            knowledge_version: "kv1".to_owned(),
            audience_url: AUDIENCE.to_owned(),
            sandbox_id: Some("sb-1".to_owned()),
            session_id: Some("ss-1".to_owned()),
        },
    }
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn read_secret(path: &std::path::Path) -> String {
    let mut text = std::fs::read_to_string(path).expect("secret file");
    if text.is_empty() {
        text.zeroize();
        panic!("empty secret");
    }
    text
}

fn write_config(
    runtime_file: &std::path::Path,
    ca_file: &std::path::Path,
    host: &str,
    port: u16,
    database: &str,
) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oo-a326-http-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("packs")).expect("config dir");
    std::fs::copy(runtime_file, dir.join("runtime.pass")).expect("runtime copy");
    std::fs::copy(ca_file, dir.join("ca.crt")).expect("ca copy");
    std::fs::write(
        dir.join("endpoint.json"),
        format!("{{\"host\":\"{host}\",\"port\":{port},\"database\":\"{database}\"}}\n"),
    )
    .expect("endpoint");
    let packs = dir.join("packs");
    let fixture_dir = PathBuf::from(required_env("OO_A341_FIXTURE_DIR"));
    let (picker_text, picker_digest) = compile_pack(
        &packs,
        &fixture_dir,
        "visibility-picker.json",
        "picker.json",
    );
    let (auditor_text, auditor_digest) = compile_pack(
        &packs,
        &fixture_dir,
        "visibility-auditor.json",
        "auditor.json",
    );
    assert_ne!(
        picker_digest, auditor_digest,
        "the two roles sealed the same digest"
    );
    let picker_artifact: Value = serde_json::from_str(&picker_text).expect("picker artifact");
    let auditor_artifact: Value = serde_json::from_str(&auditor_text).expect("auditor artifact");
    assert_eq!(
        picker_artifact["pack_digest"].as_str(),
        Some(picker_digest.as_str())
    );
    assert_eq!(
        auditor_artifact["pack_digest"].as_str(),
        Some(auditor_digest.as_str())
    );
    std::fs::remove_file(packs.join("picker.json.digest")).expect("picker digest");
    std::fs::remove_file(packs.join("auditor.json.digest")).expect("auditor digest");
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&packs).expect("packs") {
        let entry = entry.expect("pack entry");
        names.push(entry.file_name().into_string().expect("pack name"));
    }
    names.sort();
    assert_eq!(
        names,
        vec!["auditor.json".to_owned(), "picker.json".to_owned()]
    );
    let config = format!(
        "{{\"listen\":\"127.0.0.1:0\",\"audience\":\"{AUDIENCE}\",\"runtime_user\":\"oo_token_runtime\",\"password_file\":\"runtime.pass\",\"ca_file\":\"ca.crt\",\"endpoint_file\":\"endpoint.json\",\"pack_dir\":\"packs\",\"body_limit\":1048576,\"request_timeout_ms\":5000,\"global_concurrency\":1}}\n"
    );
    std::fs::write(dir.join("config.json"), config).expect("config");
    dir
}

fn compile_pack(
    work: &Path,
    fixture_dir: &Path,
    visibility: &str,
    output_name: &str,
) -> (String, String) {
    let output = work.join(output_name);
    let digest_path = work.join(format!("{output_name}.digest"));
    let produced = Command::new(required_env("OO_A341_FIXTURE_BIN"))
        .arg("compile")
        .arg("--compiler")
        .arg(required_env("OO_A341_ROLE_PACK_BIN"))
        .arg("--effective")
        .arg(fixture_dir.join("effective.json"))
        .arg("--visibility")
        .arg(fixture_dir.join(visibility))
        .arg("--output")
        .arg(&output)
        .arg("--digest")
        .arg(&digest_path)
        .stdin(Stdio::null())
        .output()
        .expect("fixture spawn");
    let tool_stderr = String::from_utf8_lossy(&produced.stderr).into_owned();
    assert!(
        produced.status.success(),
        "compile {visibility} failed: {tool_stderr}"
    );
    let text = std::fs::read_to_string(&output).expect("compiled pack");
    let digest = std::fs::read_to_string(&digest_path)
        .expect("digest")
        .trim()
        .to_owned();
    assert_eq!(digest.len(), 64, "compiler digest length");
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    (text, digest)
}

fn spawn_gateway(dir: &std::path::Path) -> Child {
    let bin = env!("CARGO_BIN_EXE_oo-gateway");
    let _ = std::fs::remove_file(dir.join("listen.addr"));
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("stderr.txt"))
        .expect("stderr");
    let path = std::env::var("PATH").unwrap_or_default();
    Command::new(bin)
        .arg(dir)
        .env_clear()
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn gateway")
}

fn wait_addr(dir: &std::path::Path) -> SocketAddr {
    let path = dir.join("listen.addr");
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let text = text.trim();
            if let Ok(addr) = text.parse::<SocketAddr>() {
                return addr;
            }
        }
        if started.elapsed() > Duration::from_secs(5) {
            panic!("gateway did not write listen.addr");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    extra: Option<(&str, &str)>,
) -> (u16, Vec<u8>) {
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some((name, value)) = extra {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if method == "POST" {
        request.push_str("Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{}");
    } else {
        request.push_str("\r\n");
    }
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .expect("timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> (u16, Vec<u8>) {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("header terminator");
    let head = std::str::from_utf8(&bytes[..split]).expect("header");
    let rest = &bytes[split + 4..];
    let status: u16 = head
        .lines()
        .next()
        .expect("status")
        .split_whitespace()
        .nth(1)
        .expect("code")
        .parse()
        .expect("number");
    let length = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    });
    let body = match length {
        Some(length) => rest[..length.min(rest.len())].to_vec(),
        None => rest.to_vec(),
    };
    (status, body)
}

fn body_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).into_owned()
}

fn assert_boundary(body: &[u8]) {
    let value: Value = serde_json::from_slice(body).expect("health json");
    assert_eq!(value["authentication"], "loopback-http");
    assert_eq!(value["external_mtls"], false);
    assert_eq!(value["oauth"], false);
    assert_eq!(value["idp"], false);
}
