//! TLS transport for external client metadata. Loopback cases do not need PostgreSQL.
//! The ignored test is the private PostgreSQL proof and runs only with `OO_A350_LIVE=1`.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use oo_gateway::http_error::ApiCode;
use oo_gateway::http_gateway::{
    harness_install, harness_issue, harness_revoke_token, harness_tls_exchange, load_operator_dir, AppState,
    CurrentTuple, Endpoint, IssueGrant, TokenKind,
};
use oo_gateway::pack_catalog::PackCatalog;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use zeroize::Zeroize;

const AUDIENCE: &str = "https://gateway.tls.invalid/v1";
const WORKLOAD: &str = "urn:oo:workload:external-client";
const OTHER_WORKLOAD: &str = "urn:oo:workload:external-other";

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[test]
fn loopback_config_keeps_the_historical_file_and_refuses_a_public_bind() {
    let dir = temp_dir("loopback-omit");
    write_common(&dir, "127.0.0.1:9", None, "");
    let loaded = load_operator_dir(&dir).expect("omitted transport");
    assert_eq!(loaded.listen, "127.0.0.1:9".parse::<SocketAddr>().expect("addr"));
    assert!(!dir.join("listen.addr").exists(), "load wrote listen.addr");

    let explicit = temp_dir("loopback-explicit");
    write_common(&explicit, "127.0.0.1:9", Some("loopback"), "");
    load_operator_dir(&explicit).expect("explicit loopback");

    let public = temp_dir("loopback-public");
    write_common(&public, "0.0.0.0:9", None, "");
    assert_eq!(load_reason(&public, "public loopback"), "listen");

    let named = temp_dir("loopback-tls-name");
    write_common(&named, "127.0.0.1:9", Some("loopback"), ",\"tls_key_file\":\"server.key\"");
    assert_eq!(load_reason(&named, "tls name on loopback"), "config");

    let other = temp_dir("transport-other");
    write_common(&other, "127.0.0.1:9", Some("mtls"), "");
    assert_eq!(load_reason(&other, "other transport"), "transport");
}

#[test]
fn invalid_tls_material_fails_before_listen() {
    let material = mint_material();
    let cases = [
        ("missing-key", "tls_key", tls_extra("missing.key", "server.crt", "client-ca.crt", 400)),
        ("empty-key", "tls_key", tls_extra("empty.key", "server.crt", "client-ca.crt", 400)),
        ("garbage-key", "tls_key", tls_extra("garbage.key", "server.crt", "client-ca.crt", 400)),
        ("mismatch", "tls", tls_extra("mismatch.key", "server.crt", "client-ca.crt", 400)),
        ("chain-key", "tls_chain", tls_extra("server.key", "chain-key.pem", "client-ca.crt", 400)),
        ("empty-chain", "tls_chain", tls_extra("server.key", "empty.crt", "client-ca.crt", 400)),
        ("two-ca", "client_ca", tls_extra("server.key", "server.crt", "two-ca.crt", 400)),
        ("timeout-zero", "handshake_timeout", tls_extra("server.key", "server.crt", "client-ca.crt", 0)),
        ("timeout-high", "handshake_timeout", tls_extra("server.key", "server.crt", "client-ca.crt", 30_001)),
        ("timeout-absent", "handshake_timeout", tls_names_only("server.key", "server.crt", "client-ca.crt")),
    ];
    for (name, reason, extra) in cases {
        let dir = temp_dir(name);
        copy_material(&material, &dir);
        std::fs::write(dir.join("empty.key"), b"").expect("empty key");
        std::fs::write(dir.join("garbage.key"), b"not a private key\n").expect("garbage");
        std::fs::write(dir.join("empty.crt"), b"").expect("empty chain");
        let chain_key = join_files(&[&material.server_cert, &material.server_key]);
        std::fs::write(dir.join("chain-key.pem"), chain_key).expect("chain key");
        let two_ca = join_files(&[&material.client_ca, &material.other_ca]);
        std::fs::write(dir.join("two-ca.crt"), two_ca).expect("two ca");
        write_common(&dir, "0.0.0.0:9", Some("tls"), &extra);
        assert_eq!(load_reason(&dir, name), reason, "{name}");
        assert!(!dir.join("listen.addr").exists(), "{name} wrote listen.addr");
    }

    let ok = temp_dir("tls-load");
    copy_material(&material, &ok);
    write_common(&ok, "0.0.0.0:9", Some("tls"), &tls_extra("server.key", "server.crt", "client-ca.crt", 400));
    let loaded = load_operator_dir(&ok).expect("valid tls load");
    assert_eq!(loaded.listen.ip(), IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    assert!(!ok.join("listen.addr").exists(), "valid load wrote listen.addr");
}

#[test]
fn binary_exits_before_listen_when_the_tls_key_is_invalid() {
    let dir = temp_dir("binary-bad");
    write_common(&dir, "127.0.0.1:0", Some("tls"), &tls_extra("garbage.key", "server.crt", "client-ca.crt", 400));
    std::fs::write(dir.join("garbage.key"), b"not a private key\n").expect("garbage");
    std::fs::write(dir.join("server.crt"), b"not a certificate\n").expect("chain");
    std::fs::write(dir.join("client-ca.crt"), b"not a certificate\n").expect("ca");
    let mut running = Running { child: spawn_gateway(&dir) };
    let started = Instant::now();
    let code = loop {
        if let Some(status) = running.child.try_wait().expect("wait") {
            break status.code();
        }
        if dir.join("listen.addr").exists() || started.elapsed() > Duration::from_secs(5) {
            panic!("invalid tls config listened or hung");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(code, Some(1));
    assert!(!dir.join("listen.addr").exists(), "listen.addr after refusal");
    let stderr = std::fs::read_to_string(dir.join("stderr.txt")).unwrap_or_default();
    assert!(stderr.contains("oo-gateway: tls_key"), "stderr={stderr}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn loopback_refuses_proxy_headers_and_internal_routes() {
    let state = AppState::for_limits(PackCatalog::empty(), 32);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (shutdown, rx) = oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, oo_gateway::http_gateway::router(state))
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
            .expect("serve");
    });
    let (status, body) = plain_exchange(
        addr,
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Forwarded-Client-Cert: forged-workload-marker\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(status, ApiCode::Forbidden.status());
    assert_eq!(error_code(&body), "FORBIDDEN");
    assert!(!body_text(&body).contains("forged-workload-marker"));
    let (status, body) = plain_exchange(
        addr,
        "POST /internal/v1/session-tokens HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(status, ApiCode::Forbidden.status());
    assert_eq!(error_code(&body), "FORBIDDEN");
    let (status, _) = plain_exchange(addr, "GET /missing HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await;
    assert_eq!(status, ApiCode::NotFound.status());
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_listener_uses_the_verified_certificate_and_bounds_the_handshake() {
    let material = mint_material();
    let dir = temp_dir("tls-serve");
    copy_material(&material, &dir);
    write_common(&dir, "0.0.0.0:0", Some("tls"), &tls_extra("server.key", "server.crt", "client-ca.crt", 400));
    let running = Running { child: spawn_gateway(&dir) };
    let bound = wait_addr(&dir);
    assert!(bound.ip().is_unspecified(), "bound address is not the public wildcard");
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bound.port());

    let (status, body) = tls_exchange(&addr, &material.client_ca, Some(&material.client_cert), Some(&material.client_key), health_request()).await;
    assert_eq!(status, 200, "health");
    let health = body_json(&body);
    assert_eq!(health["authentication"], "verified-certificate");
    assert_eq!(health["external_mtls"], true);
    assert_eq!(health["oauth"], false);
    assert_eq!(health["idp"], false);
    assert_eq!(health["peer_workload"], WORKLOAD);
    assert_eq!(health["peer_source"], "verified-certificate");
    assert_eq!(health["sql_execution"], "unsupported");
    assert_eq!(health["status"], "up");

    let (status, ready) = tls_exchange(&addr, &material.client_ca, Some(&material.client_cert), Some(&material.client_key), ready_request()).await;
    assert_eq!(status, 200, "ready");
    assert_eq!(body_json(&ready)["database"], "not_probed");
    assert_eq!(body_json(&ready)["peer_workload"], WORKLOAD);

    let (status, other) = tls_exchange(
        &addr,
        &material.client_ca,
        Some(&material.other_cert),
        Some(&material.other_key),
        health_request(),
    )
    .await;
    assert_eq!(status, 200, "other workload");
    assert_eq!(body_json(&other)["peer_workload"], OTHER_WORKLOAD);

    let forged = "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nX-OO-Workload: forged-workload-marker\r\nX-Forwarded-Client-Cert: forwarded-cert-marker\r\nConnection: close\r\n\r\n";
    let (status, body) = tls_exchange(&addr, &material.client_ca, Some(&material.client_cert), Some(&material.client_key), forged.as_bytes()).await;
    assert_eq!(status, ApiCode::Forbidden.status(), "forged header");
    assert_eq!(error_code(&body), "FORBIDDEN");
    let text = body_text(&body);
    assert!(!text.contains("forged-workload-marker"));
    assert!(!text.contains("forwarded-cert-marker"));
    assert!(!text.contains(WORKLOAD));

    let internal = b"POST /internal/v1/session-tokens HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    let (status, body) = tls_exchange(&addr, &material.client_ca, Some(&material.client_cert), Some(&material.client_key), internal).await;
    assert_eq!(status, ApiCode::Forbidden.status(), "internal");
    assert_eq!(error_code(&body), "FORBIDDEN");

    let exact = "a".repeat(32);
    let (status, body) = tls_exchange(&addr, &material.client_ca, Some(&material.client_cert), Some(&material.client_key), &post_sql(&exact)).await;
    assert_eq!(status, ApiCode::SqlUnsupported.status(), "sql at the limit");
    assert_eq!(error_code(&body), "SQL_UNSUPPORTED");
    let over = "b".repeat(33);
    let (status, body) = tls_exchange(&addr, &material.client_ca, Some(&material.client_cert), Some(&material.client_key), &post_sql(&over)).await;
    assert_eq!(status, 413, "body past the limit");
    assert!(!body_text(&body).contains(&over));

    assert!(harness_tls_exchange(addr, &material.client_ca, None, None, health_request()).await.is_err(), "anonymous client");
    assert!(
        harness_tls_exchange(addr, &material.client_ca, Some(&material.wrong_cert), Some(&material.wrong_key), health_request()).await.is_err(),
        "other ca"
    );
    let plain = harness_tls_exchange(addr, &material.client_ca, Some(&material.plain_cert), Some(&material.plain_key), health_request()).await;
    if let Ok(bytes) = plain {
        assert!(!bytes.windows(12).any(|window| window == b"HTTP/1.1 200"));
        assert!(!body_has(&bytes, "peer_workload"));
    }

    let mut stalled = TcpStream::connect(addr).expect("stall connect");
    stalled.set_read_timeout(Some(Duration::from_secs(2))).expect("stall timeout");
    stalled.write_all(&[0x16]).expect("stall byte");
    std::thread::sleep(Duration::from_millis(1_200));
    let mut buf = [0_u8; 8];
    let stalled_open = stalled.read(&mut buf).ok().filter(|count| *count > 0).is_some();
    assert!(!stalled_open, "stalled handshake stayed open");
    let (status, _) = tls_exchange(&addr, &material.client_ca, Some(&material.client_cert), Some(&material.client_key), health_request()).await;
    assert_eq!(status, 200, "handshake after the stall");

    let mut clear = TcpStream::connect(addr).expect("clear connect");
    clear.set_read_timeout(Some(Duration::from_secs(2))).expect("clear timeout");
    clear.write_all(health_request()).expect("clear write");
    let mut clear_bytes = Vec::new();
    let _ = clear.read_to_end(&mut clear_bytes);
    assert!(!clear_bytes.windows(12).any(|window| window == b"HTTP/1.1 200"), "plaintext http was accepted");
    assert!(!body_has(&clear_bytes, "peer_workload"));

    let stderr = std::fs::read_to_string(dir.join("stderr.txt")).unwrap_or_default();
    assert!(!stderr.contains("PRIVATE KEY"), "key label in stderr");
    drop(running);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires OO_A350_LIVE=1, the private PostgreSQL, and role-pack-bundle"]
async fn verified_peer_metadata_uses_the_compiled_pack_and_the_leaf_uri() {
    let live = std::env::var("OO_A350_LIVE").unwrap_or_default();
    assert_eq!(live, "1", "OO_A350_LIVE=1 is required; refusing a green skip");
    let host = required_env("OO_A350_PG_HOST");
    let port: u16 = required_env("OO_A350_PG_PORT").parse().expect("port");
    let database = required_env("OO_A350_PG_DATABASE");
    let admin_path = PathBuf::from(required_env("OO_A350_ADMIN_PASSWORD_FILE"));
    let runtime_path = PathBuf::from(required_env("OO_A350_RUNTIME_PASSWORD_FILE"));
    let ca_path = PathBuf::from(required_env("OO_A350_CA_FILE"));
    let marker_path = PathBuf::from(required_env("OO_A350_MARKER"));
    let fixture_dir = PathBuf::from(required_env("OO_A350_FIXTURE_DIR"));
    std::fs::write(&marker_path, b"ran=1\n").expect("marker");

    let work = temp_dir("live");
    let (picker_text, picker_digest) = compile_pack(&work, &fixture_dir, "visibility-picker.json", "picker.json");
    let (auditor_text, auditor_digest) = compile_pack(&work, &fixture_dir, "visibility-auditor.json", "auditor.json");
    assert_ne!(picker_digest, auditor_digest, "the two roles sealed the same digest");
    assert!(!picker_text.contains("unclassified_pii") && !picker_text.contains("auditor_phone"));
    assert!(!auditor_text.contains("unclassified_pii") && !auditor_text.contains("picker_note"));

    let mut admin = read_secret(&admin_path);
    let mut runtime = read_secret(&runtime_path);
    let ca = std::fs::read(&ca_path).expect("ca");
    let endpoint = Endpoint { host: host.clone(), port, database: database.clone() };
    let admin_password = trim_secret(&admin);
    harness_install(endpoint.clone(), admin_password, &ca).await.expect("install");
    let mut picker = harness_issue(endpoint.clone(), admin_password, &ca, client_grant("oo_r_picker", "picker-user", AUDIENCE, None))
        .await
        .expect("issue picker");
    let mut auditor = harness_issue(endpoint.clone(), admin_password, &ca, client_grant("oo_r_auditor", "auditor-user", AUDIENCE, None))
        .await
        .expect("issue auditor");
    let mut wrong_audience = harness_issue(
        endpoint.clone(),
        admin_password,
        &ca,
        client_grant("oo_r_picker", "picker-user", "https://gateway.other.invalid/v1", None),
    )
    .await
    .expect("issue wrong audience");
    let mut expired = harness_issue(endpoint.clone(), admin_password, &ca, client_grant("oo_r_picker", "picker-user", AUDIENCE, Some(1)))
        .await
        .expect("issue expired");
    let mut session = harness_issue(endpoint.clone(), admin_password, &ca, session_grant()).await.expect("issue session");
    admin.zeroize();

    let material = mint_material();
    let config = temp_dir("live-config");
    copy_material(&material, &config);
    std::fs::copy(&runtime_path, config.join("runtime.pass")).expect("runtime copy");
    std::fs::copy(&ca_path, config.join("pg-ca.crt")).expect("pg ca");
    std::fs::create_dir_all(config.join("packs")).expect("packs");
    std::fs::write(config.join("packs").join("picker.json"), &picker_text).expect("picker pack");
    std::fs::write(config.join("packs").join("auditor.json"), &auditor_text).expect("auditor pack");
    std::fs::write(
        config.join("endpoint.json"),
        format!("{{\"host\":\"{host}\",\"port\":{port},\"database\":\"{database}\"}}\n"),
    )
    .expect("endpoint");
    let config_json = format!(
        "{{\"transport\":\"tls\",\"listen\":\"0.0.0.0:0\",\"audience\":\"{AUDIENCE}\",\"runtime_user\":\"oo_token_runtime\",\"password_file\":\"runtime.pass\",\"ca_file\":\"pg-ca.crt\",\"endpoint_file\":\"endpoint.json\",\"pack_dir\":\"packs\",\"body_limit\":32,\"request_timeout_ms\":5000,\"global_concurrency\":1,{}}}\n",
        tls_extra("server.key", "server.crt", "client-ca.crt", 400).trim_start_matches(',')
    );
    std::fs::write(config.join("config.json"), config_json).expect("config");

    let mut gateway = Running { child: spawn_gateway(&config) };
    let addr = dial(wait_addr(&config));
    let mut picker_token = picker.0.clone();
    let mut auditor_token = auditor.0.clone();

    let (status, body) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", Some(&picker_token), None).await;
    assert_eq!(status, 200, "picker metadata");
    assert_role(&body, &picker_digest, "picker-user", "oo_r_picker", &["order_id", "picker_note"], WORKLOAD);
    assert!(!body_text(&body).contains("auditor_phone") && !body_text(&body).contains("unclassified_pii"));
    assert!(!body_text(&body).contains(&picker_token));
    let origin = Instant::now();

    let (status, body) = tls_http(&addr, &material, "other", "GET", "/v1/pack/metadata", Some(&picker_token), None).await;
    assert_eq!(status, 200, "same token other certificate");
    assert_eq!(body_json(&body)["role"], "oo_r_picker");
    assert_eq!(body_json(&body)["peer_workload"], OTHER_WORKLOAD);
    assert_eq!(body_json(&body)["pack_digest"], picker_digest);
    assert!(!body_text(&body).contains("auditor_phone"));

    let mut revoke_admin = read_secret(&admin_path);
    harness_revoke_token(endpoint.clone(), trim_secret(&revoke_admin), &ca, "depot".to_owned(), picker.1.clone())
        .await
        .expect("revoke");
    revoke_admin.zeroize();
    sleep_until(origin + Duration::from_millis(1_000));
    let (status, _) = tls_http(&addr, &material, "client", "GET", "/v1/session/current", Some(&picker_token), None).await;
    assert_eq!(status, 200, "warm cache inside the original deadline");
    sleep_until(origin + Duration::from_millis(5_200));
    let (status, _) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", Some(&picker_token), None).await;
    assert_eq!(status, 401, "revoked at the original deadline");

    drop(gateway);
    gateway = Running { child: spawn_gateway(&config) };
    let addr = dial(wait_addr(&config));
    let (status, _) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", Some(&picker_token), None).await;
    assert_eq!(status, 401, "revoked token stays refused after restart");

    let (status, body) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", Some(&auditor_token), None).await;
    assert_eq!(status, 200, "auditor metadata");
    assert_role(&body, &auditor_digest, "auditor-user", "oo_r_auditor", &["auditor_phone", "order_id"], WORKLOAD);
    assert!(!body_text(&body).contains("picker_note"));

    for (name, value) in [("X-OO-Role", "oo_r_picker"), ("X-OO-Project", "other-project"), ("X-OO-Scope", "session")] {
        let (status, body) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", Some(&auditor_token), Some((name, value))).await;
        assert_eq!(status, 403, "{name}");
        let text = body_text(&body);
        assert!(!text.contains("picker_note") && !text.contains("auditor_phone"));
        assert!(!text.contains(&auditor_token));
        assert_eq!(error_code(&body), "FORBIDDEN");
    }
    let (status, body) = tls_http(
        &addr,
        &material,
        "client",
        "GET",
        "/v1/pack/metadata",
        Some(&auditor_token),
        Some(("X-Forwarded-Client-Cert", "forged-workload-marker")),
    ).await;
    assert_eq!(status, 403, "forwarded cert");
    assert!(!body_text(&body).contains("forged-workload-marker"));
    assert!(!body_text(&body).contains("auditor_phone"));

    let (status, _) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", None, None).await;
    assert_eq!(status, 401, "missing bearer");
    let (status, _) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", Some("not-a-token"), None).await;
    assert_eq!(status, 401, "malformed bearer");
    let (status, _) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", Some(&wrong_audience.0), None).await;
    assert_eq!(status, 401, "wrong audience");
    std::thread::sleep(Duration::from_millis(50));
    let (status, _) = tls_http(&addr, &material, "client", "GET", "/v1/pack/metadata", Some(&expired.0), None).await;
    assert_eq!(status, 401, "expired client token");
    let (status, body) = tls_http(&addr, &material, "egress", "GET", "/v1/session/current", Some(&session.0), None).await;
    assert_eq!(status, 401, "session token");
    assert_eq!(error_code(&body), "UNAUTHENTICATED");
    assert!(!body_text(&body).contains("egress-proxy"));

    let (status, body) = tls_http(&addr, &material, "client", "POST", "/internal/v1/session-tokens", Some(&auditor_token), None).await;
    assert_eq!(status, 403, "session issuance");
    assert_eq!(error_code(&body), "FORBIDDEN");
    let (status, body) = tls_post(&addr, &material, "/v1/sql", Some(&auditor_token), &"a".repeat(32)).await;
    assert_eq!(status, 501, "sql");
    assert_eq!(error_code(&body), "SQL_UNSUPPORTED");
    let (status, _) = tls_post(&addr, &material, "/v1/sql", None, &"b".repeat(33)).await;
    assert_eq!(status, 413, "tls body limit");

    let stderr = std::fs::read_to_string(config.join("stderr.txt")).unwrap_or_default();
    assert!(!stderr.contains(trim_secret(&runtime)), "password in stderr");
    assert!(!stderr.contains("ooct_") && !stderr.contains("oost_"), "bearer prefix in stderr");
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", gateway.child.id())).expect("cmdline");
    let joined = String::from_utf8_lossy(&cmdline);
    assert!(!joined.contains(trim_secret(&runtime)), "password in argv");
    assert!(!joined.contains("ooct_") && !joined.contains("oost_"), "bearer in argv");

    runtime.zeroize();
    picker.0.zeroize();
    auditor.0.zeroize();
    wrong_audience.0.zeroize();
    expired.0.zeroize();
    session.0.zeroize();
    picker_token.zeroize();
    auditor_token.zeroize();
    drop(gateway);
    std::fs::write(&marker_path, b"ran=1\ncompleted=1\n").expect("marker done");
}

struct Material {
    dir: PathBuf,
    server_key: Vec<u8>,
    server_cert: Vec<u8>,
    client_ca: Vec<u8>,
    other_ca: Vec<u8>,
    client_cert: Vec<u8>,
    client_key: Vec<u8>,
    other_cert: Vec<u8>,
    other_key: Vec<u8>,
    plain_cert: Vec<u8>,
    plain_key: Vec<u8>,
    wrong_cert: Vec<u8>,
    wrong_key: Vec<u8>,
    egress_cert: Vec<u8>,
    egress_key: Vec<u8>,
}

fn mint_material() -> Material {
    let dir = temp_dir("certs");
    openssl(&dir, &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "client-ca.key"]);
    openssl(
        &dir,
        &[
            "req",
            "-new",
            "-x509",
            "-key",
            "client-ca.key",
            "-out",
            "client-ca.crt",
            "-days",
            "7",
            "-subj",
            "/CN=oo-a350-client-ca",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ],
    );
    openssl(&dir, &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "other-ca.key"]);
    openssl(
        &dir,
        &[
            "req",
            "-new",
            "-x509",
            "-key",
            "other-ca.key",
            "-out",
            "other-ca.crt",
            "-days",
            "7",
            "-subj",
            "/CN=oo-a350-other-ca",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
        ],
    );
    sign_cert(
        &dir,
        "server",
        "client-ca",
        "subjectAltName=IP:127.0.0.1\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n",
    );
    sign_cert(
        &dir,
        "client",
        "client-ca",
        &format!("subjectAltName=URI:{WORKLOAD}\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n"),
    );
    sign_cert(
        &dir,
        "other",
        "client-ca",
        &format!(
            "subjectAltName=URI:{OTHER_WORKLOAD}\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n"
        ),
    );
    sign_cert(
        &dir,
        "egress",
        "client-ca",
        "subjectAltName=URI:urn:oo:workload:egress-proxy\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n",
    );
    sign_cert(
        &dir,
        "plain",
        "client-ca",
        "subjectAltName=DNS:oo-a350-plain.invalid\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n",
    );
    sign_cert(
        &dir,
        "wrong",
        "other-ca",
        &format!(
            "subjectAltName=URI:{WORKLOAD}\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n"
        ),
    );
    openssl(&dir, &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "mismatch.key"]);
    Material {
        server_key: std::fs::read(dir.join("server.key")).expect("server key"),
        server_cert: std::fs::read(dir.join("server.crt")).expect("server cert"),
        client_ca: std::fs::read(dir.join("client-ca.crt")).expect("client ca"),
        other_ca: std::fs::read(dir.join("other-ca.crt")).expect("other ca"),
        client_cert: std::fs::read(dir.join("client.crt")).expect("client cert"),
        client_key: std::fs::read(dir.join("client.key")).expect("client key"),
        other_cert: std::fs::read(dir.join("other.crt")).expect("other cert"),
        other_key: std::fs::read(dir.join("other.key")).expect("other key"),
        plain_cert: std::fs::read(dir.join("plain.crt")).expect("plain cert"),
        plain_key: std::fs::read(dir.join("plain.key")).expect("plain key"),
        wrong_cert: std::fs::read(dir.join("wrong.crt")).expect("wrong cert"),
        wrong_key: std::fs::read(dir.join("wrong.key")).expect("wrong key"),
        egress_cert: std::fs::read(dir.join("egress.crt")).expect("egress cert"),
        egress_key: std::fs::read(dir.join("egress.key")).expect("egress key"),
        dir,
    }
}

fn sign_cert(dir: &Path, name: &str, ca: &str, extensions: &str) {
    let key = format!("{name}.key");
    let csr = format!("{name}.csr");
    let crt = format!("{name}.crt");
    let ext = format!("{name}.ext");
    let serial = format!("{name}.srl");
    openssl(dir, &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", &key]);
    openssl(dir, &["req", "-new", "-key", &key, "-out", &csr, "-subj", &format!("/CN=oo-a350-{name}")]);
    std::fs::write(dir.join(&ext), extensions).expect("ext");
    openssl(
        dir,
        &[
            "x509",
            "-req",
            "-in",
            &csr,
            "-CA",
            &format!("{ca}.crt"),
            "-CAkey",
            &format!("{ca}.key"),
            "-CAserial",
            &serial,
            "-CAcreateserial",
            "-out",
            &crt,
            "-days",
            "7",
            "-extfile",
            &ext,
        ],
    );
}

fn openssl(dir: &Path, args: &[&str]) {
    let output = Command::new("openssl").args(args).current_dir(dir).output().expect("openssl spawn");
    assert!(output.status.success(), "openssl {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr));
}

fn copy_material(material: &Material, dir: &Path) {
    std::fs::create_dir_all(dir).expect("dir");
    std::fs::write(dir.join("server.key"), &material.server_key).expect("server key");
    std::fs::write(dir.join("server.crt"), &material.server_cert).expect("server cert");
    std::fs::write(dir.join("client-ca.crt"), &material.client_ca).expect("client ca");
    std::fs::write(dir.join("mismatch.key"), std::fs::read(material.dir.join("mismatch.key")).expect("mismatch")).expect("copy mismatch");
    std::fs::write(dir.join("client.crt"), &material.client_cert).expect("client cert");
    std::fs::write(dir.join("client.key"), &material.client_key).expect("client key");
}

fn write_common(dir: &Path, listen: &str, transport: Option<&str>, extra: &str) {
    std::fs::create_dir_all(dir.join("packs")).expect("packs");
    if !dir.join("runtime.pass").exists() {
        std::fs::write(dir.join("runtime.pass"), b"runtime-secret\n").expect("password");
    }
    if !dir.join("pg-ca.crt").exists() {
        std::fs::write(dir.join("pg-ca.crt"), b"not-a-server-ca\n").expect("pg ca");
    }
    if !dir.join("endpoint.json").exists() {
        std::fs::write(dir.join("endpoint.json"), b"{\"host\":\"db.invalid\",\"port\":5432,\"database\":\"oo_a350\"}\n").expect("endpoint");
    }
    let transport_field = match transport {
        Some(value) => format!(",\"transport\":\"{value}\""),
        None => String::new(),
    };
    let json = format!(
        "{{\"listen\":\"{listen}\"{transport_field},\"audience\":\"{AUDIENCE}\",\"runtime_user\":\"oo_token_runtime\",\"password_file\":\"runtime.pass\",\"ca_file\":\"pg-ca.crt\",\"endpoint_file\":\"endpoint.json\",\"pack_dir\":\"packs\",\"body_limit\":32,\"request_timeout_ms\":5000,\"global_concurrency\":1{extra}}}\n"
    );
    std::fs::write(dir.join("config.json"), json).expect("config");
}

fn tls_extra(key: &str, chain: &str, ca: &str, timeout_ms: u64) -> String {
    format!(",\"tls_key_file\":\"{key}\",\"tls_chain_file\":\"{chain}\",\"client_ca_file\":\"{ca}\",\"handshake_timeout_ms\":{timeout_ms}")
}

fn tls_names_only(key: &str, chain: &str, ca: &str) -> String {
    format!(",\"tls_key_file\":\"{key}\",\"tls_chain_file\":\"{chain}\",\"client_ca_file\":\"{ca}\"")
}

fn join_files(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(part);
        if !part.ends_with(b"\n") {
            out.push(b'\n');
        }
    }
    out
}

fn load_reason(dir: &Path, label: &str) -> &'static str {
    match load_operator_dir(dir) {
        Ok(_) => panic!("{label} loaded"),
        Err(reason) => reason,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("oo-a350-{name}-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp");
    dir
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

fn spawn_gateway(dir: &Path) -> Child {
    let bin = env!("CARGO_BIN_EXE_oo-gateway");
    let _ = std::fs::remove_file(dir.join("listen.addr"));
    let stderr = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("stderr.txt")).expect("stderr");
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

fn wait_addr(dir: &Path) -> SocketAddr {
    let path = dir.join("listen.addr");
    let started = Instant::now();
    loop {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(addr) = text.trim().parse::<SocketAddr>()
        {
            return addr;
        }
        if started.elapsed() > Duration::from_secs(8) {
            let stderr = std::fs::read_to_string(dir.join("stderr.txt")).unwrap_or_default();
            panic!("gateway did not write listen.addr stderr={stderr}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn dial(bound: SocketAddr) -> SocketAddr {
    if bound.ip().is_unspecified() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bound.port())
    } else {
        bound
    }
}

fn health_request() -> &'static [u8] {
    b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
}

fn ready_request() -> &'static [u8] {
    b"GET /ready HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
}

fn post_sql(body: &str) -> Vec<u8> {
    format!(
        "POST /v1/sql HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

async fn plain_exchange(addr: SocketAddr, request: &str) -> (u16, Vec<u8>) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.expect("read");
    parse_response(&bytes)
}

async fn tls_exchange(addr: &SocketAddr, ca: &[u8], cert: Option<&[u8]>, key: Option<&[u8]>, request: &[u8]) -> (u16, Vec<u8>) {
    let bytes = harness_tls_exchange(*addr, ca, cert, key, request).await.expect("tls exchange");
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

fn body_json(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("json")
}

fn body_has(bytes: &[u8], needle: &str) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle.as_bytes())
}

fn error_code(body: &[u8]) -> String {
    let value = body_json(body);
    assert!(value.get("query_id").is_none(), "query_id");
    let error = value["error"].as_object().expect("error");
    assert_eq!(error.len(), 4);
    error["code"].as_str().expect("code").to_owned()
}

async fn tls_http(
    addr: &SocketAddr,
    material: &Material,
    who: &str,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    extra: Option<(&str, &str)>,
) -> (u16, Vec<u8>) {
    let (cert, key) = material_pair(material, who);
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some((name, value)) = extra {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if method == "POST" {
        request.push_str("Content-Length: 2\r\n\r\n{}");
    } else {
        request.push_str("\r\n");
    }
    tls_exchange(addr, &material.client_ca, Some(cert), Some(key), request.as_bytes()).await
}

async fn tls_post(addr: &SocketAddr, material: &Material, path: &str, bearer: Option<&str>, body: &str) -> (u16, Vec<u8>) {
    let (cert, key) = material_pair(material, "client");
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    tls_exchange(addr, &material.client_ca, Some(cert), Some(key), request.as_bytes()).await
}

fn material_pair<'a>(material: &'a Material, who: &str) -> (&'a [u8], &'a [u8]) {
    match who {
        "other" => (&material.other_cert, &material.other_key),
        "egress" => (&material.egress_cert, &material.egress_key),
        _ => (&material.client_cert, &material.client_key),
    }
}

fn assert_role(body: &[u8], digest: &str, principal: &str, role: &str, columns: &[&str], workload: &str) {
    let meta = body_json(body);
    assert_eq!(meta["project_id"], "depot");
    assert_eq!(meta["principal_id"], principal);
    assert_eq!(meta["role"], role);
    assert_eq!(meta["scope"], "client");
    assert_eq!(meta["authentication"], "verified-certificate");
    assert_eq!(meta["external_mtls"], true);
    assert_eq!(meta["oauth"], false);
    assert_eq!(meta["idp"], false);
    assert_eq!(meta["peer_workload"], workload);
    assert_eq!(meta["peer_source"], "verified-certificate");
    assert_eq!(meta["pack_digest"], digest);
    let mut got = Vec::new();
    for column in meta["columns"].as_array().expect("columns") {
        got.push(column["name"].as_str().expect("column name").to_owned());
    }
    assert_eq!(got, columns);
}

fn client_grant(role: &str, principal: &str, audience: &str, ttl: Option<i64>) -> IssueGrant {
    IssueGrant {
        kind: TokenKind::Client,
        client_ttl_ms: ttl,
        current: CurrentTuple {
            project_id: "depot".to_owned(),
            principal_id: principal.to_owned(),
            role: role.to_owned(),
            claims: vec![("region".to_owned(), "north".to_owned())],
            policy_version: "pv1".to_owned(),
            knowledge_version: "kv1".to_owned(),
            audience_url: audience.to_owned(),
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

fn compile_pack(work: &Path, fixture_dir: &Path, visibility: &str, output_name: &str) -> (String, String) {
    let output = work.join(output_name);
    let digest_path = work.join(format!("{output_name}.digest"));
    let produced = Command::new(required_env("OO_A350_FIXTURE_BIN"))
        .arg("compile")
        .arg("--compiler")
        .arg(required_env("OO_A350_ROLE_PACK_BIN"))
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
    assert!(
        produced.status.success(),
        "compile {visibility} failed: {}",
        String::from_utf8_lossy(&produced.stderr)
    );
    let text = std::fs::read_to_string(&output).expect("compiled pack");
    let digest = std::fs::read_to_string(&digest_path).expect("digest").trim().to_owned();
    assert_eq!(digest.len(), 64, "compiler digest length");
    (text, digest)
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn read_secret(path: &Path) -> String {
    std::fs::read_to_string(path).expect("secret file")
}

fn trim_secret(value: &str) -> &str {
    value.trim_end_matches(['\n', '\r'])
}

fn sleep_until(deadline: Instant) {
    let now = Instant::now();
    if deadline > now {
        std::thread::sleep(deadline - now);
    }
}
