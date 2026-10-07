//! Independent HTTP proof for the frozen loopback gateway.
//!
//! This file is added only on a private copy of the archive after the
//! pre-overlay library tests, loopback tests, and Clippy have passed.
//! It is not part of commit 51d08e59.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oo_gateway::http_gateway::{
    harness_install, harness_issue, harness_revoke_token, CurrentTuple, Endpoint, IssueGrant, TokenKind,
};
use oo_gateway::pack_catalog::fixture_artifact;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const AUDIENCE: &str = "https://gateway.loopback.invalid/v1";

#[test]
#[ignore = "requires OO_A337_LIVE=1 and the private PostgreSQL"]
fn independent_http_boundaries() {
    let live = std::env::var("OO_A337_LIVE").unwrap_or_default();
    assert_eq!(live, "1", "OO_A337_LIVE=1 is required; refusing a green skip");
    let host = env_var("OO_A337_PG_HOST");
    let port: u16 = env_var("OO_A337_PG_PORT").parse().expect("port");
    let database = env_var("OO_A337_PG_DATABASE");
    let admin_path = PathBuf::from(env_var("OO_A337_ADMIN_PASSWORD_FILE"));
    let runtime_path = PathBuf::from(env_var("OO_A337_RUNTIME_PASSWORD_FILE"));
    let ca_path = PathBuf::from(env_var("OO_A337_CA_FILE"));
    let marker = PathBuf::from(env_var("OO_A337_MARKER"));
    let result = PathBuf::from(env_var("OO_A337_RESULT"));
    std::fs::write(&marker, b"ran=1\n").expect("marker");
    let _ = std::fs::remove_file(&result);

    let http_source = include_str!("../src/http_gateway.rs");
    let pack_source = include_str!("../src/pack_catalog.rs");
    let bin_source = include_str!("../src/bin/oo-gateway.rs");
    let executor = ["query", "_executor"].concat();
    let trusted = ["execute", "_trusted_select"].concat();
    for source in [http_source, pack_source, bin_source] {
        assert!(!source.contains(&executor), "http source names the executor");
        assert!(!source.contains(&trusted), "http source names trusted select");
    }
    note(&result, "executor_name=0");

    let mut admin = read_secret(&admin_path);
    let ca = std::fs::read(&ca_path).expect("ca");
    let endpoint = Endpoint {
        host: host.clone(),
        port,
        database: database.clone(),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    runtime
        .block_on(harness_install(endpoint.clone(), trim_secret(&admin), &ca))
        .expect("install");
    let mut picker = runtime
        .block_on(harness_issue(
            endpoint.clone(),
            trim_secret(&admin),
            &ca,
            client_grant("oo_r_picker", "picker-user"),
        ))
        .expect("issue picker");
    let mut auditor = runtime
        .block_on(harness_issue(
            endpoint.clone(),
            trim_secret(&admin),
            &ca,
            client_grant("oo_r_auditor", "auditor-user"),
        ))
        .expect("issue auditor");
    let mut session = runtime
        .block_on(harness_issue(endpoint.clone(), trim_secret(&admin), &ca, session_grant()))
        .expect("issue session");
    admin.zeroize();

    let picker_pack = fixture_artifact("depot", "oo_r_picker", "pv1", "kv1", "orders", "picker_note").expect("picker pack");
    let alt_pack = fixture_artifact("depot", "oo_r_picker", "pv2", "kv1", "orders", "picker_alt").expect("alt pack");
    let auditor_pack = fixture_artifact("depot", "oo_r_auditor", "pv1", "kv1", "orders", "auditor_phone").expect("auditor pack");
    assert_operator_pack(&picker_pack);
    note(&result, "operator_pack=not_compiler");

    let dir = write_config(
        &runtime_path,
        &ca_path,
        &host,
        port,
        &database,
        (1_048_576, 5_000, 1),
        &[
            ("picker.json", picker_pack.as_str()),
            ("picker-alt.json", alt_pack.as_str()),
            ("auditor.json", auditor_pack.as_str()),
        ],
    );
    let mut gateway = spawn_gateway(&dir);
    let addr = wait_addr(&dir);
    note(&result, "gateway=up");

    let (status, body) = http(addr, "GET", "/health", None, None, Duration::from_secs(8));
    assert_eq!(status, 200, "health");
    assert_boundary(&body);
    note(&result, "health=200");
    let (status, ready) = http(addr, "GET", "/ready", None, None, Duration::from_secs(8));
    assert_eq!(status, 200, "ready");
    let ready_json: Value = serde_json::from_slice(&ready).expect("ready json");
    assert_eq!(ready_json["database"], "not_probed");
    note(&result, "ready=not_probed");

    let (status, _) = http(addr, "GET", "/slow", None, None, Duration::from_secs(8));
    assert_eq!(status, 404, "probe slow is not mounted");
    let (status, _) = http(addr, "GET", "/hold", None, None, Duration::from_secs(8));
    assert_eq!(status, 404, "probe hold is not mounted");
    note(&result, "probe_routes=404");

    let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&picker.0), None, Duration::from_secs(8));
    assert_eq!(status, 200, "picker metadata");
    assert_exact_pack(&body, &picker_pack, "picker-user", "oo_r_picker", "pv1", "kv1");
    assert!(!body_text(&body).contains("picker_alt"), "other policy column returned");
    assert!(!body_text(&body).contains("auditor_phone"), "other role column returned");
    assert!(!body_text(&body).contains("semantic"), "pack file text returned");
    assert!(!body_text(&body).contains(&picker.0), "bearer echoed");
    let origin = Instant::now();
    note(&result, "picker_metadata=200");

    let mut revoke_admin = read_secret(&admin_path);
    runtime
        .block_on(harness_revoke_token(
            endpoint.clone(),
            trim_secret(&revoke_admin),
            &ca,
            "depot".to_owned(),
            picker.1.clone(),
        ))
        .expect("revoke");
    revoke_admin.zeroize();
    note(&result, "revoke=committed");
    sleep_until(origin + Duration::from_millis(1_000));
    let (status, body) = http(addr, "GET", "/v1/session/current", Some(&picker.0), None, Duration::from_secs(8));
    assert_eq!(status, 200, "warm cache inside the original deadline");
    assert_tuple(&body, "picker-user", "oo_r_picker", "pv1", "kv1", "client");
    note(&result, "warm_hit=200");
    sleep_until(origin + Duration::from_millis(5_200));
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&picker.0), None, Duration::from_secs(8));
    assert_eq!(status, 401, "revoked at the original deadline");
    note(&result, "revoked_after_deadline=401");

    drop(gateway);
    gateway = spawn_gateway(&dir);
    let addr = wait_addr(&dir);
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&picker.0), None, Duration::from_secs(8));
    assert_eq!(status, 401, "revoked token stays refused after restart");
    note(&result, "restart_revoked=401");

    let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor.0), None, Duration::from_secs(8));
    assert_eq!(status, 200, "auditor metadata");
    assert_exact_pack(&body, &auditor_pack, "auditor-user", "oo_r_auditor", "pv1", "kv1");
    assert!(!body_text(&body).contains("picker_note") && !body_text(&body).contains("picker_alt"), "picker columns leaked");
    let outage_origin = Instant::now();
    note(&result, "auditor_metadata=200");
    std::fs::write(dir.join("endpoint.json"), format!("{{\"host\":\"127.0.0.1\",\"port\":1,\"database\":\"{database}\"}}\n")).expect("outage");
    sleep_until(outage_origin + Duration::from_millis(1_000));
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor.0), None, Duration::from_secs(8));
    assert_eq!(status, 200, "warm cache does not consult the rewritten endpoint");
    note(&result, "outage_warm=200");
    sleep_until(outage_origin + Duration::from_millis(5_200));
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor.0), None, Duration::from_secs(8));
    assert_eq!(status, 401, "closed database after the deadline");
    let (status, ready) = http(addr, "GET", "/ready", None, None, Duration::from_secs(8));
    assert_eq!(status, 200, "ready during outage");
    let ready_json: Value = serde_json::from_slice(&ready).expect("ready json");
    assert_eq!(ready_json["database"], "not_probed");
    note(&result, "outage_after_deadline=401");

    drop(gateway);
    gateway = spawn_gateway(&dir);
    let addr = wait_addr(&dir);
    let (status, _) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor.0), None, Duration::from_secs(8));
    assert_eq!(status, 401, "restart during the outage fails closed");
    note(&result, "outage_restart=401");
    std::fs::write(
        dir.join("endpoint.json"),
        format!("{{\"host\":\"{host}\",\"port\":{port},\"database\":\"{database}\"}}\n"),
    )
    .expect("restore");
    let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor.0), None, Duration::from_secs(8));
    assert_eq!(status, 200, "restored endpoint");
    assert!(body_text(&body).contains("auditor_phone"), "auditor column after restore");
    assert!(!body_text(&body).contains("picker_note"), "picker column after restore");
    note(&result, "restored=200");

    for (name, value) in [("X-OO-Role", "oo_r_picker"), ("X-OO-Project", "other-project"), ("X-OO-Scope", "session")] {
        let (status, body) = http(addr, "GET", "/v1/pack/metadata", Some(&auditor.0), Some((name, value)), Duration::from_secs(8));
        assert_eq!(status, 403, "{name}");
        let text = body_text(&body);
        assert!(!text.contains("picker_note") && !text.contains("picker_alt") && !text.contains("auditor_phone"), "forged header returned a column");
        assert!(!text.contains("semantic"), "forged header returned pack text");
        assert!(!text.contains(&auditor.0), "bearer echoed");
        assert_eq!(error_code(&body), "FORBIDDEN");
    }
    note(&result, "forged_header=403");

    let (status, body) = http(addr, "GET", "/v1/session/current", Some(&session.0), None, Duration::from_secs(8));
    assert_eq!(status, 401, "session token");
    assert_eq!(error_code(&body), "UNAUTHENTICATED");
    assert!(!body_text(&body).contains(&session.0), "session bearer echoed");
    note(&result, "session_binding=401");

    let (status, body) = http(addr, "GET", "/health?marker=query-must-not-echo", None, None, Duration::from_secs(8));
    assert_eq!(status, 400, "query string");
    assert_eq!(error_code(&body), "QUERY_NOT_ALLOWED");
    assert!(!body_text(&body).contains("query-must-not-echo"), "query echoed");
    note(&result, "query=400");

    let (status, body) = post(addr, "/v1/sql", Some(&auditor.0), "{\"sql\":\"select 1\"}", Duration::from_secs(8));
    assert_eq!(status, 501, "sql");
    assert_eq!(error_code(&body), "SQL_UNSUPPORTED");
    assert!(!body_text(&body).contains("select 1"), "sql body echoed");
    let (status, _) = post(addr, "/v1/sql", None, "{}", Duration::from_secs(8));
    assert_eq!(status, 501, "sql without a bearer is still unsupported");
    note(&result, "sql=501");
    note(&result, "sql_without_bearer=501");

    let stderr = std::fs::read(dir.join("stderr.txt")).unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr);
    let mut runtime_secret = read_secret(&runtime_path);
    assert!(!stderr_text.contains(trim_secret(&runtime_secret)), "password in stderr");
    assert!(!stderr_text.contains("ooct_") && !stderr_text.contains("oost_"), "bearer prefix in stderr");
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", gateway.child.id())).expect("cmdline");
    let joined = String::from_utf8_lossy(&cmdline);
    assert!(!joined.contains(trim_secret(&runtime_secret)), "password in argv");
    assert!(!joined.contains("ooct_") && !joined.contains("oost_"), "bearer in argv");
    runtime_secret.zeroize();
    note(&result, "secret_channels=clean");
    drop(gateway);

    assert!(refuse_startup(&runtime_path, &ca_path, &host, port, &database, &tamper_sha(&picker_pack)), "tampered file digest started");
    note(&result, "tamper_file_sha=refused");
    let declared = declare_digest(&picker_pack);
    let declared_dir = write_config(
        &runtime_path,
        &ca_path,
        &host,
        port,
        &database,
        (1_048_576, 5_000, 1),
        &[("picker.json", declared.as_str())],
    );
    let declared_gateway = spawn_gateway(&declared_dir);
    let declared_addr = wait_addr(&declared_dir);
    let (status, body) = http(declared_addr, "GET", "/v1/pack/metadata", Some(&auditor.0), None, Duration::from_secs(8));
    assert_eq!(status, 403, "auditor token must not receive the picker pack");
    assert!(!body_text(&body).contains("picker_note"), "declared pack leaked through the wrong role");
    drop(declared_gateway);
    let mut picker_again = runtime_issue_picker(&admin_path, &ca, &endpoint);
    let declared_gateway = spawn_gateway(&declared_dir);
    let declared_addr = wait_addr(&declared_dir);
    let (status, body) = http(declared_addr, "GET", "/v1/pack/metadata", Some(&picker_again.0), None, Duration::from_secs(8));
    assert_eq!(status, 200, "declared pack digest still loads");
    let meta: Value = serde_json::from_slice(&body).expect("declared json");
    assert_eq!(meta["pack_digest"], "ab".repeat(32));
    assert_ne!(meta["pack_digest"], manifest_sha(&declared));
    note(&result, "pack_digest_check=declared");
    drop(declared_gateway);
    picker_again.0.zeroize();

    let foreign = write_config(
        &runtime_path,
        &ca_path,
        &host,
        port,
        &database,
        (1_048_576, 5_000, 1),
        &[("picker.json", picker_pack.as_str())],
    );
    replace_listen(&foreign, "0.0.0.0:9");
    assert!(process_exits(&foreign), "non-loopback listen stayed up");
    note(&result, "foreign_listen=refused");

    let stall = Stall::open();
    let bounds = write_config(
        &runtime_path,
        &ca_path,
        "127.0.0.1",
        stall.port(),
        &database,
        (32, 800, 1),
        &[("picker.json", picker_pack.as_str())],
    );
    let bounds_gateway = spawn_gateway(&bounds);
    let bounds_addr = wait_addr(&bounds);
    let (status, body) = post(bounds_addr, "/v1/sql", None, &"a".repeat(32), Duration::from_secs(8));
    assert_eq!(status, 501, "body at the limit");
    assert_eq!(error_code(&body), "SQL_UNSUPPORTED");
    let over = "b".repeat(33);
    let (status, body) = post(bounds_addr, "/v1/sql", None, &over, Duration::from_secs(8));
    assert_eq!(status, 413, "body over the limit");
    assert!(!body.windows(over.len()).any(|window| window == over.as_bytes()), "rejected body echoed");
    note(&result, "body_limit=501/413");

    let token = auditor.0.clone();
    let meta_addr = bounds_addr;
    let meta_start = Instant::now();
    let meta_thread = thread::spawn(move || http(meta_addr, "GET", "/v1/pack/metadata", Some(&token), None, Duration::from_secs(3)));
    thread::sleep(Duration::from_millis(200));
    let health_start = Instant::now();
    let health_thread = thread::spawn(move || http(bounds_addr, "GET", "/health", None, None, Duration::from_secs(3)));
    let (health_status, _) = health_thread.join().expect("health thread");
    let health_elapsed = health_start.elapsed();
    let (meta_status, _) = meta_thread.join().expect("metadata thread");
    let meta_elapsed = meta_start.elapsed();
    assert_eq!(meta_status, 408, "stalled lookup");
    assert!(meta_elapsed >= Duration::from_millis(400) && meta_elapsed < Duration::from_secs(2), "deadline was not the stall bound");
    assert!(health_elapsed >= Duration::from_millis(400), "second request did not wait for the single permit");
    assert!(health_status == 200 || health_status == 408, "queued health status {health_status}");
    note(&result, &format!("stall=408 elapsed_ms={}", meta_elapsed.as_millis()));
    note(&result, &format!("concurrency_held=1 health={health_status} wait_ms={}", health_elapsed.as_millis()));
    if health_status == 408 {
        note(&result, "waiter_cut=1");
    } else {
        note(&result, "waiter_cut=0");
    }
    drop(bounds_gateway);
    drop(stall);

    picker.0.zeroize();
    auditor.0.zeroize();
    session.0.zeroize();
    std::fs::write(&marker, b"ran=1\ncompleted=1\n").expect("marker done");
    note(&result, "completed=1");
}

fn runtime_issue_picker(admin_path: &Path, ca: &[u8], endpoint: &Endpoint) -> (String, String) {
    let mut admin = read_secret(admin_path);
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let issued = runtime
        .block_on(harness_issue(endpoint.clone(), trim_secret(&admin), ca, client_grant("oo_r_picker", "picker-user")))
        .expect("reissue picker");
    admin.zeroize();
    issued
}

fn note(path: &Path, line: &str) {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).expect("result");
    writeln!(file, "{line}").expect("result write");
}

fn assert_operator_pack(text: &str) {
    let value: Value = serde_json::from_str(text).expect("artifact");
    assert!(value.get("compiler").is_none(), "operator fixture carries a compiler id");
    assert_eq!(value["protocol"], "oo.builder.role-pack/v1");
    assert_eq!(value["pack_digest"], value["manifest_sha256"]);
    let files = value["files"].as_array().expect("files");
    assert!(files.iter().all(|file| file.get("text").is_some()), "embedded text");
}

fn assert_exact_pack(body: &[u8], artifact: &str, principal: &str, role: &str, policy: &str, knowledge: &str) {
    let meta: Value = serde_json::from_slice(body).expect("metadata json");
    assert_tuple(body, principal, role, policy, knowledge, "client");
    let artifact: Value = serde_json::from_str(artifact).expect("artifact");
    assert_eq!(meta["pack_digest"], artifact["pack_digest"]);
    assert_eq!(meta["protocol"], "oo.builder.role-pack/v1");
    let http_files = meta["files"].as_array().expect("http files");
    let stored = artifact["files"].as_array().expect("stored files");
    assert_eq!(http_files.len(), stored.len());
    for stored_file in stored {
        let path = stored_file["path"].as_str().expect("path");
        let text = stored_file["text"].as_str().expect("text");
        let digest = sha256_hex(text.as_bytes());
        assert_eq!(stored_file["sha256"], digest, "stored file digest");
        let http_file = http_files.iter().find(|file| file["path"] == path).expect("http file");
        assert_eq!(http_file["sha256"], digest);
        assert_eq!(http_file["bytes"], text.len() as u64);
        assert!(http_file.get("text").is_none(), "file text in the response");
    }
    let names = meta["columns"].as_array().expect("columns").iter().map(|column| column["name"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
    assert_eq!(names.len(), 1, "one visible column");
}

fn assert_tuple(body: &[u8], principal: &str, role: &str, policy: &str, knowledge: &str, scope: &str) {
    let meta: Value = serde_json::from_slice(body).expect("tuple json");
    assert_eq!(meta["authentication"], "loopback-http");
    assert_eq!(meta["external_mtls"], false);
    assert_eq!(meta["oauth"], false);
    assert_eq!(meta["idp"], false);
    assert_eq!(meta["project_id"], "depot");
    assert_eq!(meta["principal_id"], principal);
    assert_eq!(meta["role"], role);
    assert_eq!(meta["policy_version"], policy);
    assert_eq!(meta["knowledge_version"], knowledge);
    assert_eq!(meta["scope"], scope);
    assert_eq!(meta["audience_url"], AUDIENCE);
}

fn assert_boundary(body: &[u8]) {
    let value: Value = serde_json::from_slice(body).expect("health json");
    assert_eq!(value["authentication"], "loopback-http");
    assert_eq!(value["external_mtls"], false);
    assert_eq!(value["oauth"], false);
    assert_eq!(value["idp"], false);
    assert_eq!(value["sql_execution"], "unsupported");
}

fn tamper_sha(artifact: &str) -> String {
    let mut value: Value = serde_json::from_str(artifact).expect("artifact");
    let files = value["files"].as_array_mut().expect("files");
    files[1]["sha256"] = Value::String("0".repeat(64));
    serde_json::to_string(&value).expect("tamper")
}

fn declare_digest(artifact: &str) -> String {
    let mut value: Value = serde_json::from_str(artifact).expect("artifact");
    value["pack_digest"] = Value::String("ab".repeat(32));
    serde_json::to_string(&value).expect("declare")
}

fn manifest_sha(artifact: &str) -> String {
    let value: Value = serde_json::from_str(artifact).expect("artifact");
    value["manifest_sha256"].as_str().expect("manifest").to_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn refuse_startup(runtime_file: &Path, ca_file: &Path, host: &str, port: u16, database: &str, pack: &str) -> bool {
    let dir = write_config(runtime_file, ca_file, host, port, database, (1_048_576, 5_000, 1), &[("picker.json", pack)]);
    process_exits(&dir)
}

fn process_exits(dir: &Path) -> bool {
    let mut child = spawn_gateway(dir);
    let started = Instant::now();
    let code = loop {
        if let Some(status) = child.child.try_wait().expect("try_wait") {
            break status.code();
        }
        if started.elapsed() > Duration::from_secs(3) || dir.join("listen.addr").exists() {
            let _ = child.child.kill();
            let _ = child.child.wait();
            return false;
        }
        thread::sleep(Duration::from_millis(30));
    };
    code == Some(1) && !dir.join("listen.addr").exists()
}

fn replace_listen(dir: &Path, listen: &str) {
    let path = dir.join("config.json");
    let text = std::fs::read_to_string(&path).expect("config");
    let mut value: Value = serde_json::from_str(&text).expect("config json");
    value["listen"] = Value::String(listen.to_owned());
    std::fs::write(path, serde_json::to_string(&value).expect("config")).expect("rewrite");
}

struct Stall {
    port: u16,
    stop: Arc<Mutex<bool>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Stall {
    fn open() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("stall");
        let port = listener.local_addr().expect("stall addr").port();
        let stop = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            listener.set_nonblocking(true).expect("nonblocking");
            let mut held = Vec::new();
            while !*flag.lock().expect("flag") {
                match listener.accept() {
                    Ok((stream, _)) => held.push(stream),
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
                    Err(_) => break,
                }
            }
            drop(held);
        });
        Self { port, stop, thread: Some(thread) }
    }

    fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Stall {
    fn drop(&mut self) {
        *self.stop.lock().expect("flag") = true;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Running {
    child: Child,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn read_secret(path: &Path) -> String {
    let text = std::fs::read_to_string(path).expect("secret file");
    if text.is_empty() {
        panic!("empty secret");
    }
    text
}

fn trim_secret(value: &str) -> &str {
    value.trim_end_matches(['\n', '\r'])
}

fn write_config(
    runtime_file: &Path,
    ca_file: &Path,
    host: &str,
    port: u16,
    database: &str,
    limits: (u64, u64, u64),
    packs: &[(&str, &str)],
) -> PathBuf {
    let (body_limit, timeout_ms, concurrency) = limits;
    let dir = std::env::temp_dir().join(format!("oo-a337-http-{}-{}", std::process::id(), next_id()));
    std::fs::create_dir_all(dir.join("packs")).expect("config dir");
    std::fs::copy(runtime_file, dir.join("runtime.pass")).expect("runtime copy");
    std::fs::copy(ca_file, dir.join("ca.crt")).expect("ca copy");
    std::fs::write(
        dir.join("endpoint.json"),
        format!("{{\"host\":\"{host}\",\"port\":{port},\"database\":\"{database}\"}}\n"),
    )
    .expect("endpoint");
    for (name, text) in packs {
        std::fs::write(dir.join("packs").join(name), text).expect("pack");
    }
    let config = format!(
        "{{\"listen\":\"127.0.0.1:0\",\"audience\":\"{AUDIENCE}\",\"runtime_user\":\"oo_token_runtime\",\"password_file\":\"runtime.pass\",\"ca_file\":\"ca.crt\",\"endpoint_file\":\"endpoint.json\",\"pack_dir\":\"packs\",\"body_limit\":{body_limit},\"request_timeout_ms\":{timeout_ms},\"global_concurrency\":{concurrency}}}\n"
    );
    std::fs::write(dir.join("config.json"), config).expect("config");
    dir
}

fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn spawn_gateway(dir: &Path) -> Running {
    let bin = env!("CARGO_BIN_EXE_oo-gateway");
    let _ = std::fs::remove_file(dir.join("listen.addr"));
    let stderr = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("stderr.txt")).expect("stderr");
    let path = std::env::var("PATH").unwrap_or_default();
    let child = Command::new(bin)
        .arg(dir)
        .env_clear()
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn gateway");
    Running { child }
}

fn wait_addr(dir: &Path) -> SocketAddr {
    let path = dir.join("listen.addr");
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(addr) = text.trim().parse::<SocketAddr>() {
                return addr;
            }
        }
        if started.elapsed() > Duration::from_secs(5) {
            panic!("gateway did not write listen.addr");
        }
        thread::sleep(Duration::from_millis(30));
    }
}

fn sleep_until(deadline: Instant) {
    let now = Instant::now();
    if deadline > now {
        thread::sleep(deadline - now);
    }
}

fn http(addr: SocketAddr, method: &str, path: &str, bearer: Option<&str>, extra: Option<(&str, &str)>, timeout: Duration) -> (u16, Vec<u8>) {
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        request.push_str("Authorization: Bearer ");
        request.push_str(token);
        request.push_str("\r\n");
    }
    if let Some((name, value)) = extra {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    exchange(addr, request.as_bytes(), timeout)
}

fn post(addr: SocketAddr, path: &str, bearer: Option<&str>, body: &str, timeout: Duration) -> (u16, Vec<u8>) {
    let mut request = format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", body.len());
    if let Some(token) = bearer {
        request.push_str("Authorization: Bearer ");
        request.push_str(token);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    request.push_str(body);
    exchange(addr, request.as_bytes(), timeout)
}

fn exchange(addr: SocketAddr, request: &[u8], timeout: Duration) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.set_read_timeout(Some(timeout)).expect("timeout");
    stream.set_write_timeout(Some(timeout)).expect("timeout");
    stream.write_all(request).expect("write");
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> (u16, Vec<u8>) {
    let split = bytes.windows(4).position(|window| window == b"\r\n\r\n").expect("header terminator");
    let head = std::str::from_utf8(&bytes[..split]).expect("header");
    let rest = &bytes[split + 4..];
    let status: u16 = head.lines().next().expect("status").split_whitespace().nth(1).expect("code").parse().expect("number");
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

fn error_code(body: &[u8]) -> String {
    let value: Value = serde_json::from_slice(body).expect("error json");
    assert!(value.get("query_id").is_none(), "query_id");
    let error = value["error"].as_object().expect("error");
    assert!(error.get("query_id").is_none(), "query_id in error");
    assert_eq!(error.len(), 4);
    error["code"].as_str().expect("code").to_owned()
}
