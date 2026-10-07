//! Independent E48 oracle. This file is not a product test.
//! It is copied into the container work tree after the archive smoke.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oo_gateway::http_gateway::{
    harness_install, harness_issue, harness_revoke_token, harness_tls_exchange, load_operator_dir, CurrentTuple, Endpoint,
    IssueGrant, TokenKind,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use zeroize::Zeroize;

const AUDIENCE: &str = "https://gateway.review.invalid/v1";
const LEAF: &str = "urn:oo:workload:review-leaf";
const OTHER_LEAF: &str = "urn:oo:workload:review-other";

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[test]
fn response_budgets_stay_on_the_shared_router() {
    let source = include_str!("../src/http_gateway.rs");
    assert!(source.contains("TimeoutLayer::with_status_code"), "request deadline layer");
    assert!(source.contains("StatusCode::REQUEST_TIMEOUT"), "deadline status");
    assert!(source.contains("RequestBodyLimitLayer::new"), "body budget");
    assert!(source.contains("GlobalConcurrencyLimitLayer::new"), "concurrency permit");
    assert!(source.contains("fn serve_tls"), "tls listener");
    assert!(source.contains("router(state)"), "tls uses the shared router");
    assert!(source.contains("DeclaredTransport::ExternalClient"), "external client lookup");
    assert!(!source.contains("EgressProxy"), "workload is not an egress proxy");
    assert!(!source.contains("webpki-roots"), "public roots are not a trust anchor");
    assert!(!source.contains("allow_unauthenticated"), "anonymous clients are not allowed");
    let executor = ["query", "_executor"].concat();
    let trusted = ["execute", "_trusted_select"].concat();
    assert!(!source.contains(&executor), "http source names the executor");
    assert!(!source.contains(&trusted), "http source names trusted select");
    note("budgets=shared_router");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abrupt_close_accepts_only_a_complete_body() {
    let material = mint_material();
    let complete = http_bytes(4, b"okay");
    let truncated = http_bytes(8, b"short");
    let missing = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nokay".to_vec();
    let unterminated = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n".to_vec();
    let tail = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nokayTAIL".to_vec();
    let empty = http_bytes(0, b"");
    expect_abrupt(&material, "complete", complete, true).await;
    expect_abrupt(&material, "truncated", truncated, false).await;
    expect_abrupt(&material, "missing-length", missing, false).await;
    expect_abrupt(&material, "unterminated", unterminated, false).await;
    expect_abrupt(&material, "tail", tail, true).await;
    expect_abrupt(&material, "empty", empty, true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_bind_rejects_bad_material_and_unverified_leaves() {
    let material = mint_material();
    let missing = temp_dir("missing-key");
    copy_server(&material, &missing);
    write_config(&missing, "0.0.0.0:9", "tls", &tls_fields("missing.key", "server.crt", "client-ca.crt", 400));
    assert_eq!(load_reason(&missing), "tls_key");
    assert!(!missing.join("listen.addr").exists(), "missing key wrote listen.addr");

    let open = temp_dir("public-load");
    copy_server(&material, &open);
    write_config(&open, "0.0.0.0:9", "tls", &tls_fields("server.key", "server.crt", "client-ca.crt", 400));
    let loaded = load_operator_dir(&open).expect("public tls load");
    assert_eq!(loaded.listen.ip(), IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    assert!(!open.join("listen.addr").exists(), "load wrote listen.addr");

    let loopback = temp_dir("loopback-public");
    write_config(&loopback, "0.0.0.0:9", "loopback", "");
    assert_eq!(load_reason(&loopback), "listen");

    let historical = temp_dir("historical");
    write_config(&historical, "127.0.0.1:9", "omit", "");
    load_operator_dir(&historical).expect("omitted transport");

    let binary = temp_dir("binary-bad");
    write_config(&binary, "127.0.0.1:0", "tls", &tls_fields("garbage.key", "server.crt", "client-ca.crt", 400));
    std::fs::write(binary.join("garbage.key"), b"not a private key\n").expect("garbage");
    std::fs::write(binary.join("server.crt"), b"not a certificate\n").expect("chain");
    std::fs::write(binary.join("client-ca.crt"), b"not a certificate\n").expect("ca");
    let mut running = Running { child: spawn_gateway(&binary) };
    let started = Instant::now();
    let code = loop {
        if let Some(status) = running.child.try_wait().expect("wait") {
            break status.code();
        }
        if binary.join("listen.addr").exists() || started.elapsed() > Duration::from_secs(5) {
            panic!("invalid tls config listened or hung");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(code, Some(1));
    assert!(!binary.join("listen.addr").exists(), "listen.addr after refusal");
    let stderr = std::fs::read_to_string(binary.join("stderr.txt")).unwrap_or_default();
    assert!(stderr.contains("oo-gateway: tls_key"), "stderr missing tls_key");
    drop(running);
    note("material=refused_before_listen");

    let dir = temp_dir("serve");
    copy_server(&material, &dir);
    write_config(&dir, "0.0.0.0:0", "tls", &tls_fields("server.key", "server.crt", "client-ca.crt", 400));
    let running = Running { child: spawn_gateway(&dir) };
    let bound = wait_addr(&dir);
    assert!(bound.ip().is_unspecified(), "tls bind is not the configured public address");
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bound.port());
    let argc = argument_count(running.child.id());
    assert_eq!(argc, 2, "gateway argc");
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", running.child.id())).expect("cmdline");
    let rendered = String::from_utf8_lossy(&cmdline);
    assert!(!rendered.contains("review-runtime-secret"), "password in argv");
    note("argv=directory_only");

    let (status, body) = tls_ok(&addr, &material.client_ca, Some(&material.leaf_cert), Some(&material.leaf_key), health_request(), "leaf health").await;
    assert_eq!(status, 200);
    let health = body_json(&body);
    assert_eq!(health["authentication"], "verified-certificate");
    assert_eq!(health["external_mtls"], true);
    assert_eq!(health["oauth"], false);
    assert_eq!(health["idp"], false);
    assert_eq!(health["peer_source"], "verified-certificate");
    assert_eq!(health["peer_workload"], LEAF);
    assert_eq!(health["sql_execution"], "unsupported");

    let (status, other) = tls_ok(&addr, &material.client_ca, Some(&material.other_cert), Some(&material.other_key), health_request(), "other leaf").await;
    assert_eq!(status, 200);
    assert_eq!(body_json(&other)["peer_workload"], OTHER_LEAF);

    let forged = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nX-OO-Workload: forged-workload-marker\r\nX-Forwarded-Client-Cert: forwarded-cert-marker\r\nConnection: close\r\n\r\n";
    let (status, body) = tls_ok(&addr, &material.client_ca, Some(&material.leaf_cert), Some(&material.leaf_key), forged, "forged").await;
    assert_eq!(status, 403);
    assert_eq!(error_code(&body), "FORBIDDEN");
    let text = body_text(&body);
    assert!(!text.contains("forged-workload-marker"));
    assert!(!text.contains("forwarded-cert-marker"));
    assert!(!text.contains(LEAF));
    note("forged_header=403");

    let internal = b"POST /internal/v1/session-tokens HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    let (status, body) = tls_ok(&addr, &material.client_ca, Some(&material.leaf_cert), Some(&material.leaf_key), internal, "internal").await;
    assert_eq!(status, 403);
    assert_eq!(error_code(&body), "FORBIDDEN");
    note("internal=403");

    let exact = "a".repeat(32);
    let (status, body) = tls_ok(&addr, &material.client_ca, Some(&material.leaf_cert), Some(&material.leaf_key), &post_sql(&exact), "sql limit").await;
    assert_eq!(status, 501);
    assert_eq!(error_code(&body), "SQL_UNSUPPORTED");
    let over = "b".repeat(33);
    let (status, body) = tls_ok(&addr, &material.client_ca, Some(&material.leaf_cert), Some(&material.leaf_key), &post_sql(&over), "sql over").await;
    assert_eq!(status, 413);
    assert!(!body_text(&body).contains(&over));
    note("body_limit=413");

    assert_exchange_err(&addr, &material, None, None, "anonymous").await;
    assert_exchange_err(&addr, &material, Some(&material.wrong_cert), Some(&material.wrong_key), "other-ca").await;
    assert_exchange_err(&addr, &material, Some(&material.dns_cert), Some(&material.dns_key), "dns-only").await;
    note("anonymous_other_ca_dns=refused");

    let mut stalled = TcpStream::connect(addr).expect("stall connect");
    stalled.set_read_timeout(Some(Duration::from_millis(400))).expect("stall timeout");
    stalled.write_all(&[0x16]).expect("stall byte");
    std::thread::sleep(Duration::from_millis(1_200));
    let started = Instant::now();
    let mut buf = [0_u8; 8];
    let read = stalled.read(&mut buf);
    let elapsed = started.elapsed();
    let closed = match &read {
        Ok(0) => true,
        Err(err) => matches!(
            err.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
        ),
        Ok(_) => false,
    };
    assert!(closed, "stalled handshake stayed open: {read:?}");
    assert!(elapsed < Duration::from_millis(400), "handshake close waited on the client");
    let (status, _) = tls_ok(&addr, &material.client_ca, Some(&material.leaf_cert), Some(&material.leaf_key), health_request(), "after stall").await;
    assert_eq!(status, 200);
    note("handshake_timeout=dropped_then_ok");

    let mut clear = TcpStream::connect(addr).expect("clear connect");
    clear.set_read_timeout(Some(Duration::from_secs(2))).expect("clear timeout");
    clear.write_all(health_request()).expect("clear write");
    let mut clear_bytes = Vec::new();
    let _ = clear.read_to_end(&mut clear_bytes);
    assert!(!clear_bytes.windows(12).any(|window| window == b"HTTP/1.1 200"), "plaintext http was accepted");
    note("plaintext=not_http_200");

    let stderr = std::fs::read_to_string(dir.join("stderr.txt")).unwrap_or_default();
    assert!(!stderr.contains("PRIVATE KEY"), "key label in stderr");
    drop(running);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires OO_A360_LIVE=1 and the private PostgreSQL"]
async fn verified_leaf_does_not_select_the_token_tuple() {
    let live = std::env::var("OO_A360_LIVE").unwrap_or_default();
    assert_eq!(live, "1", "OO_A360_LIVE=1 is required; refusing a green skip");
    let host = required_env("OO_A360_PG_HOST");
    let port: u16 = required_env("OO_A360_PG_PORT").parse().expect("port");
    let database = required_env("OO_A360_PG_DATABASE");
    let admin_path = PathBuf::from(required_env("OO_A360_ADMIN_PASSWORD_FILE"));
    let runtime_path = PathBuf::from(required_env("OO_A360_RUNTIME_PASSWORD_FILE"));
    let ca_path = PathBuf::from(required_env("OO_A360_CA_FILE"));
    let marker_path = PathBuf::from(required_env("OO_A360_MARKER"));
    let fixture_dir = PathBuf::from(required_env("OO_A360_FIXTURE_DIR"));
    std::fs::write(&marker_path, b"ran=1\n").expect("marker");

    let work = temp_dir("packs");
    let (picker_text, picker_digest) = compile_pack(&work, &fixture_dir, "visibility-picker.json", "picker.json");
    let (auditor_text, auditor_digest) = compile_pack(&work, &fixture_dir, "visibility-auditor.json", "auditor.json");
    assert_ne!(picker_digest, auditor_digest, "the two roles sealed the same digest");
    assert!(!picker_text.contains("unclassified_pii") && !picker_text.contains("auditor_phone"));
    assert!(!auditor_text.contains("unclassified_pii") && !auditor_text.contains("picker_note"));

    let mut admin = read_secret(&admin_path);
    let mut runtime = read_secret(&runtime_path);
    let ca = std::fs::read(&ca_path).expect("ca");
    let endpoint = Endpoint { host: host.clone(), port, database: database.clone() };
    harness_install(endpoint.clone(), trim_secret(&admin), &ca).await.expect("install");
    let mut picker = harness_issue(endpoint.clone(), trim_secret(&admin), &ca, client_grant("oo_r_picker", "picker-user", AUDIENCE, None))
        .await
        .expect("issue picker");
    let mut auditor = harness_issue(endpoint.clone(), trim_secret(&admin), &ca, client_grant("oo_r_auditor", "auditor-user", AUDIENCE, None))
        .await
        .expect("issue auditor");
    let mut wrong_audience = harness_issue(
        endpoint.clone(),
        trim_secret(&admin),
        &ca,
        client_grant("oo_r_picker", "picker-user", "https://gateway.other.invalid/v1", None),
    )
    .await
    .expect("issue wrong audience");
    let mut expired = harness_issue(endpoint.clone(), trim_secret(&admin), &ca, client_grant("oo_r_picker", "picker-user", AUDIENCE, Some(1)))
        .await
        .expect("issue expired");
    let mut session = harness_issue(endpoint.clone(), trim_secret(&admin), &ca, session_grant()).await.expect("issue session");
    admin.zeroize();

    let material = mint_material();
    let config = temp_dir("live-config");
    copy_server(&material, &config);
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
    write_config(&config, "0.0.0.0:0", "tls", &tls_fields("server.key", "server.crt", "client-ca.crt", 400));
    let mut gateway = Running { child: spawn_gateway(&config) };
    let addr = dial(wait_addr(&config));
    let mut picker_token = picker.0.clone();
    let mut auditor_token = auditor.0.clone();

    let (status, body) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", Some(&picker_token), None).await;
    assert_eq!(status, 200, "picker metadata");
    assert_role(&body, &picker_digest, "picker-user", "oo_r_picker", &["order_id", "picker_note"], LEAF);
    assert!(!body_text(&body).contains("auditor_phone"));
    let picker_in_body = body_text(&body).contains(&picker_token);
    assert!(!picker_in_body, "picker token echoed");
    let origin = Instant::now();

    let (status, body) = tls_get(&addr, &material, "leaf", "/v1/session/current", Some(&picker_token), Some(("X-OO-Role", "oo_r_auditor"))).await;
    assert_eq!(status, 200, "current tuple");
    let current = body_json(&body);
    assert_eq!(current["project_id"], "depot");
    assert_eq!(current["principal_id"], "picker-user");
    assert_eq!(current["role"], "oo_r_picker");
    assert_eq!(current["policy_version"], "pv1");
    assert_eq!(current["knowledge_version"], "kv1");
    assert_eq!(current["audience_url"], AUDIENCE);
    assert_eq!(current["scope"], "client");
    assert!(current["sandbox_id"].is_null());
    assert!(current["session_id"].is_null());
    assert_eq!(current["peer_workload"], LEAF);
    assert_eq!(current["peer_source"], "verified-certificate");
    note("current_tuple=store_not_header");

    let (status, body) = tls_get(&addr, &material, "other", "/v1/pack/metadata", Some(&picker_token), None).await;
    assert_eq!(status, 200, "other certificate");
    assert_eq!(body_json(&body)["role"], "oo_r_picker");
    assert_eq!(body_json(&body)["peer_workload"], OTHER_LEAF);
    assert_eq!(body_json(&body)["pack_digest"], picker_digest);
    assert!(!body_text(&body).contains("auditor_phone"));
    note("leaf_uri=verified_certificate_not_role");

    let (status, body) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", Some(&auditor_token), None).await;
    assert_eq!(status, 200, "auditor metadata");
    assert_role(&body, &auditor_digest, "auditor-user", "oo_r_auditor", &["auditor_phone", "order_id"], LEAF);
    assert!(!body_text(&body).contains("picker_note"));

    for (name, value) in [("X-OO-Role", "oo_r_picker"), ("X-OO-Project", "other-project"), ("X-OO-Scope", "session")] {
        let (status, body) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", Some(&auditor_token), Some((name, value))).await;
        assert_eq!(status, 403, "{name}");
        let text = body_text(&body);
        assert!(!text.contains("picker_note") && !text.contains("auditor_phone"));
        assert_eq!(error_code(&body), "FORBIDDEN");
    }
    let (status, body) = tls_get(
        &addr,
        &material,
        "leaf",
        "/v1/pack/metadata",
        Some(&auditor_token),
        Some(("X-Forwarded-Client-Cert", "forged-workload-marker")),
    )
    .await;
    assert_eq!(status, 403, "forwarded cert");
    assert!(!body_text(&body).contains("forged-workload-marker"));
    assert!(!body_text(&body).contains("auditor_phone"));
    note("forged_tuple_header=403");

    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", None, None).await;
    assert_eq!(status, 401, "missing bearer");
    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", Some("not-a-token"), None).await;
    assert_eq!(status, 401, "malformed bearer");
    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", Some(&wrong_audience.0), None).await;
    assert_eq!(status, 401, "wrong audience");
    note("audience=401");
    std::thread::sleep(Duration::from_millis(50));
    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", Some(&expired.0), None).await;
    assert_eq!(status, 401, "expired client token");
    let (status, body) = tls_get(&addr, &material, "egress", "/v1/session/current", Some(&session.0), None).await;
    assert_eq!(status, 401, "session token");
    assert_eq!(error_code(&body), "UNAUTHENTICATED");
    assert!(!body_text(&body).contains("egress-proxy"));
    note("session_token=401");

    let internal = format!(
        "POST /internal/v1/session-tokens HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {auditor_token}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    let (status, body) = tls_ok(&addr, &material.client_ca, Some(&material.leaf_cert), Some(&material.leaf_key), internal.as_bytes(), "internal").await;
    assert_eq!(status, 403, "session issuance");
    assert_eq!(error_code(&body), "FORBIDDEN");
    note("internal_remains=403");

    let mut revoke_admin = read_secret(&admin_path);
    harness_revoke_token(endpoint.clone(), trim_secret(&revoke_admin), &ca, "depot".to_owned(), picker.1.clone())
        .await
        .expect("revoke");
    revoke_admin.zeroize();
    sleep_until(origin + Duration::from_millis(1_000));
    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/session/current", Some(&picker_token), None).await;
    assert_eq!(status, 200, "warm cache inside the original deadline");
    sleep_until(origin + Duration::from_millis(5_200));
    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", Some(&picker_token), None).await;
    assert_eq!(status, 401, "revoked at the original deadline");
    note("revoke=401_after_deadline");

    drop(gateway);
    gateway = Running { child: spawn_gateway(&config) };
    let addr = dial(wait_addr(&config));
    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/pack/metadata", Some(&picker_token), None).await;
    assert_eq!(status, 401, "revoked token stays refused after restart");
    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/session/current", Some(&auditor_token), None).await;
    assert_eq!(status, 200, "auditor still resolves before the database closes");
    let auditor_filled = Instant::now();
    sleep_until(auditor_filled + Duration::from_millis(5_200));
    std::fs::write(config.join("endpoint.json"), "{\"host\":\"127.0.0.1\",\"port\":9,\"database\":\"oo_a360\"}\n").expect("closed endpoint");
    let (status, _) = tls_get(&addr, &material, "leaf", "/v1/session/current", Some(&auditor_token), None).await;
    assert_eq!(status, 401, "closed database after the cache deadline");
    note("closed_database=401");

    let stderr = std::fs::read_to_string(config.join("stderr.txt")).unwrap_or_default();
    let secret = trim_secret(&runtime).to_owned();
    let leaked = stderr.contains(&secret) || stderr.contains("ooct_") || stderr.contains("oost_");
    assert!(!leaked, "secret material in stderr");
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", gateway.child.id())).expect("cmdline");
    let joined = String::from_utf8_lossy(&cmdline);
    assert!(!joined.contains(&secret), "password in argv");
    assert_eq!(argument_count(gateway.child.id()), 2, "live argc");

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
    note("live=completed");
}

struct Material {
    server_key: Vec<u8>,
    server_cert: Vec<u8>,
    client_ca: Vec<u8>,
    leaf_cert: Vec<u8>,
    leaf_key: Vec<u8>,
    other_cert: Vec<u8>,
    other_key: Vec<u8>,
    dns_cert: Vec<u8>,
    dns_key: Vec<u8>,
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
            "req", "-new", "-x509", "-key", "client-ca.key", "-out", "client-ca.crt", "-days", "7", "-subj", "/CN=oo-a360-review-ca",
            "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
        ],
    );
    openssl(&dir, &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "other-ca.key"]);
    openssl(
        &dir,
        &[
            "req", "-new", "-x509", "-key", "other-ca.key", "-out", "other-ca.crt", "-days", "7", "-subj", "/CN=oo-a360-other-ca",
            "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
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
        "leaf",
        "client-ca",
        &format!("subjectAltName=URI:{LEAF}\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n"),
    );
    sign_cert(
        &dir,
        "other",
        "client-ca",
        &format!("subjectAltName=URI:{OTHER_LEAF}\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n"),
    );
    sign_cert(
        &dir,
        "dns",
        "client-ca",
        "subjectAltName=DNS:review-dns.invalid\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n",
    );
    sign_cert(
        &dir,
        "wrong",
        "other-ca",
        &format!("subjectAltName=URI:{LEAF}\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n"),
    );
    sign_cert(
        &dir,
        "egress",
        "client-ca",
        "subjectAltName=URI:urn:oo:workload:egress-proxy\nextendedKeyUsage=clientAuth\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\n",
    );
    Material {
        server_key: std::fs::read(dir.join("server.key")).expect("server key"),
        server_cert: std::fs::read(dir.join("server.crt")).expect("server cert"),
        client_ca: std::fs::read(dir.join("client-ca.crt")).expect("client ca"),
        leaf_cert: std::fs::read(dir.join("leaf.crt")).expect("leaf cert"),
        leaf_key: std::fs::read(dir.join("leaf.key")).expect("leaf key"),
        other_cert: std::fs::read(dir.join("other.crt")).expect("other cert"),
        other_key: std::fs::read(dir.join("other.key")).expect("other key"),
        dns_cert: std::fs::read(dir.join("dns.crt")).expect("dns cert"),
        dns_key: std::fs::read(dir.join("dns.key")).expect("dns key"),
        wrong_cert: std::fs::read(dir.join("wrong.crt")).expect("wrong cert"),
        wrong_key: std::fs::read(dir.join("wrong.key")).expect("wrong key"),
        egress_cert: std::fs::read(dir.join("egress.crt")).expect("egress cert"),
        egress_key: std::fs::read(dir.join("egress.key")).expect("egress key"),
    }
}

fn sign_cert(dir: &Path, name: &str, ca: &str, extensions: &str) {
    let key = format!("{name}.key");
    let csr = format!("{name}.csr");
    let crt = format!("{name}.crt");
    let ext = format!("{name}.ext");
    let serial = format!("{name}.srl");
    openssl(dir, &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", &key]);
    openssl(dir, &["req", "-new", "-key", &key, "-out", &csr, "-subj", &format!("/CN=oo-a360-{name}")]);
    std::fs::write(dir.join(&ext), extensions).expect("ext");
    openssl(
        dir,
        &["x509", "-req", "-in", &csr, "-CA", &format!("{ca}.crt"), "-CAkey", &format!("{ca}.key"), "-CAserial", &serial, "-CAcreateserial", "-out", &crt, "-days", "7", "-extfile", &ext],
    );
}

fn openssl(dir: &Path, args: &[&str]) {
    let output = Command::new("openssl").args(args).current_dir(dir).output().expect("openssl spawn");
    assert!(output.status.success(), "openssl failed: {}", String::from_utf8_lossy(&output.stderr));
}

fn copy_server(material: &Material, dir: &Path) {
    std::fs::create_dir_all(dir).expect("dir");
    std::fs::write(dir.join("server.key"), &material.server_key).expect("server key");
    std::fs::write(dir.join("server.crt"), &material.server_cert).expect("server cert");
    std::fs::write(dir.join("client-ca.crt"), &material.client_ca).expect("client ca");
}

fn write_config(dir: &Path, listen: &str, transport: &str, extra: &str) {
    std::fs::create_dir_all(dir.join("packs")).expect("packs");
    if !dir.join("runtime.pass").exists() {
        std::fs::write(dir.join("runtime.pass"), b"review-runtime-secret\n").expect("password");
    }
    if !dir.join("pg-ca.crt").exists() {
        std::fs::write(dir.join("pg-ca.crt"), b"not-a-server-ca\n").expect("pg ca");
    }
    if !dir.join("endpoint.json").exists() {
        std::fs::write(dir.join("endpoint.json"), b"{\"host\":\"db.invalid\",\"port\":5432,\"database\":\"oo_a360\"}\n").expect("endpoint");
    }
    let transport_field = if transport == "omit" { String::new() } else { format!(",\"transport\":\"{transport}\"") };
    let json = format!(
        "{{\"listen\":\"{listen}\"{transport_field},\"audience\":\"{AUDIENCE}\",\"runtime_user\":\"oo_token_runtime\",\"password_file\":\"runtime.pass\",\"ca_file\":\"pg-ca.crt\",\"endpoint_file\":\"endpoint.json\",\"pack_dir\":\"packs\",\"body_limit\":32,\"request_timeout_ms\":5000,\"global_concurrency\":1{extra}}}\n"
    );
    std::fs::write(dir.join("config.json"), json).expect("config");
}

fn tls_fields(key: &str, chain: &str, ca: &str, timeout_ms: u64) -> String {
    format!(",\"tls_key_file\":\"{key}\",\"tls_chain_file\":\"{chain}\",\"client_ca_file\":\"{ca}\",\"handshake_timeout_ms\":{timeout_ms}")
}

fn load_reason(dir: &Path) -> &'static str {
    match load_operator_dir(dir) {
        Ok(_) => panic!("invalid config loaded"),
        Err(reason) => reason,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("oo-a360-{name}-{}-{seq}", std::process::id()));
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
            panic!("gateway did not write listen.addr");
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

fn argument_count(pid: u32) -> usize {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).expect("cmdline");
    bytes.split(|byte| *byte == 0).filter(|part| !part.is_empty()).count()
}

fn health_request() -> &'static [u8] {
    b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
}

fn post_sql(body: &str) -> Vec<u8> {
    format!(
        "POST /v1/sql HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn http_bytes(length: usize, body: &[u8]) -> Vec<u8> {
    let mut message = format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").into_bytes();
    message.extend_from_slice(body);
    message
}

async fn expect_abrupt(material: &Material, label: &str, payload: Vec<u8>, accept: bool) {
    let addr = serve_abrupt(&material.server_cert, &material.server_key, payload).await;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        harness_tls_exchange(addr, &material.client_ca, None, None, health_request()),
    )
    .await;
    match (accept, result) {
        (true, Ok(Ok(_body))) => note(&format!("abrupt_{label}=accepted")),
        (false, Ok(Err(err))) => {
            assert_eq!(err, "read", "{label}");
            note(&format!("abrupt_{label}=read"));
        }
        (true, Ok(Err(err))) => panic!("{label} rejected with {err}"),
        (false, Ok(Ok(_body))) => panic!("{label} accepted a short message"),
        (_, Err(_elapsed)) => panic!("{label} timed out"),
    }
}

async fn serve_abrupt(cert_pem: &[u8], key_pem: &[u8], payload: Vec<u8>) -> SocketAddr {
    let certs = CertificateDer::pem_slice_iter(cert_pem).collect::<Result<Vec<_>, _>>().expect("server pem");
    let key = PrivateKeyDer::from_pem_slice(key_pem).expect("server key");
    let config = ServerConfig::builder().with_no_client_auth().with_single_cert(certs, key).expect("server config");
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind abrupt");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("accept");
        let mut tls = acceptor.accept(tcp).await.expect("server handshake");
        let mut buf = [0_u8; 1024];
        let read = tls.read(&mut buf).await.expect("server read");
        assert!(read > 0, "client request was empty");
        tls.write_all(&payload).await.expect("server write");
        tls.flush().await.expect("server flush");
        let (tcp, _session) = tls.into_inner();
        drop(tcp);
    });
    tokio::task::yield_now().await;
    addr
}

async fn tls_ok(addr: &SocketAddr, ca: &[u8], cert: Option<&[u8]>, key: Option<&[u8]>, request: &[u8], label: &str) -> (u16, Vec<u8>) {
    let bytes = harness_tls_exchange(*addr, ca, cert, key, request).await.unwrap_or_else(|err| panic!("{label} tls {err}"));
    parse_response(&bytes)
}

async fn assert_exchange_err(addr: &SocketAddr, material: &Material, cert: Option<&[u8]>, key: Option<&[u8]>, label: &str) {
    let result = harness_tls_exchange(*addr, &material.client_ca, cert, key, health_request()).await;
    match result {
        Ok(bytes) => {
            assert!(!bytes.windows(12).any(|window| window == b"HTTP/1.1 200"), "{label} returned http 200");
            assert!(!body_has(&bytes, "peer_workload"), "{label} returned a workload");
        }
        Err(_err) => {}
    }
    note(&format!("refused_{label}=yes"));
}

async fn tls_get(
    addr: &SocketAddr,
    material: &Material,
    who: &str,
    path: &str,
    bearer: Option<&str>,
    extra: Option<(&str, &str)>,
) -> (u16, Vec<u8>) {
    let (cert, key) = match who {
        "other" => (&material.other_cert, &material.other_key),
        "egress" => (&material.egress_cert, &material.egress_key),
        _ => (&material.leaf_cert, &material.leaf_key),
    };
    let mut headers = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some(token) = bearer {
        headers.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some((name, value)) = extra {
        headers.push_str(&format!("{name}: {value}\r\n"));
    }
    headers.push_str("\r\n");
    tls_ok(addr, &material.client_ca, Some(cert), Some(key), headers.as_bytes(), path).await
}

fn parse_response(bytes: &[u8]) -> (u16, Vec<u8>) {
    let split = bytes.windows(4).position(|window| window == b"\r\n\r\n").expect("header terminator");
    let head = std::str::from_utf8(&bytes[..split]).expect("header");
    let rest = &bytes[split + 4..];
    let status: u16 = head.lines().next().expect("status").split_whitespace().nth(1).expect("code").parse().expect("number");
    let length = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") { value.trim().parse::<usize>().ok() } else { None }
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

fn assert_role(body: &[u8], digest: &str, principal: &str, role: &str, columns: &[&str], workload: &str) {
    let meta = body_json(body);
    assert_eq!(meta["project_id"], "depot");
    assert_eq!(meta["principal_id"], principal);
    assert_eq!(meta["role"], role);
    assert_eq!(meta["scope"], "client");
    assert_eq!(meta["audience_url"], AUDIENCE);
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
            sandbox_id: Some("sb-review".to_owned()),
            session_id: Some("ss-review".to_owned()),
        },
    }
}

fn compile_pack(work: &Path, fixture_dir: &Path, visibility: &str, output_name: &str) -> (String, String) {
    let output = work.join(output_name);
    let digest_path = work.join(format!("{output_name}.digest"));
    let produced = Command::new(required_env("OO_A360_FIXTURE_BIN"))
        .arg("compile")
        .arg("--compiler")
        .arg(required_env("OO_A360_ROLE_PACK_BIN"))
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
    assert!(produced.status.success(), "compile {visibility} failed: {}", String::from_utf8_lossy(&produced.stderr));
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

fn note(label: &str) {
    let Ok(path) = std::env::var("OO_A360_SUMMARY") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{label}");
    }
}
