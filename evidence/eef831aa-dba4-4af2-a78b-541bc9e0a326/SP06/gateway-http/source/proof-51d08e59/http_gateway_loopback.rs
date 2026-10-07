//! Loopback bounds for the HTTP gateway. These tests do not open PostgreSQL.

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oo_gateway::http_error::{ApiCode, LAYER_BODY_LIMIT_STATUS, LAYER_TIMEOUT_STATUS};
use oo_gateway::http_gateway::{AppState, ProbeHooks};
use oo_gateway::pack_catalog::PackCatalog;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const BODY_LIMIT: usize = 32;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_states_the_loopback_boundary_and_hides_the_bearer() {
    let (addr, shutdown) = spawn_probe().await;
    let (status, body) = exchange(
        addr,
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer ooct_not-a-token\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(status, 200);
    let text = std::str::from_utf8(&body).expect("utf8");
    assert!(!text.contains("ooct_"), "bearer echoed");
    assert!(!text.contains("Bearer"), "authorization echoed");
    let value: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(value["authentication"], "loopback-http");
    assert_eq!(value["external_mtls"], false);
    assert_eq!(value["oauth"], false);
    assert_eq!(value["idp"], false);
    assert_eq!(value["sql_execution"], "unsupported");
    assert_eq!(value["status"], "up");
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn query_strings_are_rejected_without_an_echo() {
    let (addr, shutdown) = spawn_probe().await;
    let marker = "query-must-not-echo";
    let (status, body) = exchange(
        addr,
        &format!("GET /health?token={marker} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    )
    .await;
    assert_eq!(status, ApiCode::QueryNotAllowed.status());
    let text = String::from_utf8(body).expect("utf8");
    assert!(!text.contains(marker), "query echoed");
    assert_eq!(error_code(text.as_bytes()), "QUERY_NOT_ALLOWED");
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sql_at_the_body_limit_is_unsupported_and_one_extra_byte_is_413() {
    let (addr, shutdown) = spawn_probe().await;
    let exact = "a".repeat(BODY_LIMIT);
    let (status, body) = exchange(addr, &post_sql(&exact, None)).await;
    assert_eq!(status, ApiCode::SqlUnsupported.status());
    assert_eq!(error_code(&body), "SQL_UNSUPPORTED");
    let over = "b".repeat(BODY_LIMIT + 1);
    let (status, body) = exchange(addr, &post_sql(&over, None)).await;
    assert_eq!(status, LAYER_BODY_LIMIT_STATUS);
    assert!(!body.windows(over.len()).any(|window| window == over.as_bytes()), "rejected body was echoed");
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_route_returns_408_before_the_handler_finishes() {
    let (addr, shutdown) = spawn_probe().await;
    let started = Instant::now();
    let (status, body) = exchange(addr, "GET /slow HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await;
    assert_eq!(status, LAYER_TIMEOUT_STATUS);
    assert_ne!(body, b"{\"status\":\"late\"}");
    assert!(started.elapsed() < Duration::from_secs(2), "handler sleep completed");
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_request_waits_for_the_single_permit() {
    let (release_tx, release_rx) = oneshot::channel();
    let hooks = Arc::new(ProbeHooks {
        hold_rx: Mutex::new(Some(release_rx)),
        hold_entered: std::sync::atomic::AtomicBool::new(false),
        health_started: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = AppState::for_probe(PackCatalog::empty(), BODY_LIMIT, Arc::clone(&hooks));
    let (addr, shutdown) = spawn(state).await;
    let (status, _) = exchange(addr, "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await;
    assert_eq!(status, 200);
    assert_eq!(hooks.health_started.load(Ordering::SeqCst), 1);

    let hold_hooks = Arc::clone(&hooks);
    let hold = tokio::spawn(async move {
        exchange(addr, "GET /hold HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await
    });
    let entered = Instant::now();
    while !hold_hooks.hold_entered.load(Ordering::SeqCst) {
        if entered.elapsed() > Duration::from_secs(2) {
            panic!("hold handler did not start");
        }
        tokio::task::yield_now().await;
    }
    let health = tokio::spawn(async move {
        exchange(addr, "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(hooks.health_started.load(Ordering::SeqCst), 1, "health ran while hold held the permit");
    release_tx.send(()).expect("release");
    let (hold_status, hold_body) = hold.await.expect("hold task");
    assert_eq!(hold_status, 200);
    assert_eq!(hold_body, b"{\"status\":\"held\"}");
    let (health_status, _) = health.await.expect("health task");
    assert_eq!(health_status, 200);
    assert_eq!(hooks.health_started.load(Ordering::SeqCst), 2);
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_paths_and_methods_use_the_error_schema() {
    let (addr, shutdown) = spawn_probe().await;
    let (status, body) = exchange(addr, "GET /missing HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await;
    assert_eq!(status, ApiCode::NotFound.status());
    assert_eq!(error_code(&body), "NOT_FOUND");
    let (status, body) = exchange(addr, "POST /health HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    assert_eq!(status, ApiCode::MethodNotAllowed.status());
    assert_eq!(error_code(&body), "METHOD_NOT_ALLOWED");
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_router_does_not_mount_the_probe() {
    let state = AppState::for_limits(PackCatalog::empty(), BODY_LIMIT);
    let (addr, shutdown) = spawn(state).await;
    let (status, body) = exchange(addr, "GET /slow HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await;
    assert_eq!(status, ApiCode::NotFound.status());
    assert_eq!(error_code(&body), "NOT_FOUND");
    let (status, _) = exchange(addr, "GET /ready HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").await;
    assert_eq!(status, 200);
    let _ = shutdown.send(());
}

#[test]
fn http_sources_do_not_name_the_sql_executor() {
    let sources = [
        include_str!("../src/http_gateway.rs"),
        include_str!("../src/pack_catalog.rs"),
        include_str!("../src/bin/oo-gateway.rs"),
    ];
    let executor = ["query", "_executor"].concat();
    let trusted = ["execute", "_trusted_select"].concat();
    for source in sources {
        assert!(!source.contains(&executor), "http source names the executor");
        assert!(!source.contains(&trusted), "http source names trusted select");
    }
}

async fn spawn_probe() -> (SocketAddr, oneshot::Sender<()>) {
    let hooks = Arc::new(ProbeHooks {
        hold_rx: Mutex::new(None),
        hold_entered: std::sync::atomic::AtomicBool::new(false),
        health_started: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = AppState::for_probe(PackCatalog::empty(), BODY_LIMIT, hooks);
    spawn(state).await
}

async fn spawn(state: AppState) -> (SocketAddr, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, oo_gateway::http_gateway::router(state))
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
            .expect("serve");
    });
    (addr, tx)
}

fn post_sql(body: &str, query: Option<&str>) -> String {
    let path = match query {
        Some(query) => format!("/v1/sql?{query}"),
        None => "/v1/sql".to_owned(),
    };
    format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn error_code(body: &[u8]) -> String {
    let value: Value = serde_json::from_slice(body).expect("error json");
    assert!(value.get("query_id").is_none(), "query_id at the root");
    let error = value.get("error").expect("error object");
    let object = error.as_object().expect("error object");
    assert!(object.get("query_id").is_none(), "query_id in the error");
    assert!(object.contains_key("code"));
    assert!(object.contains_key("message"));
    assert!(object.contains_key("hint"));
    assert!(object.contains_key("retryable"));
    assert_eq!(object.len(), 4);
    object["code"].as_str().expect("code").to_owned()
}

async fn exchange(addr: SocketAddr, request: &str) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.expect("read");
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> (u16, Vec<u8>) {
    let split = bytes.windows(4).position(|window| window == b"\r\n\r\n").expect("header terminator");
    let head = std::str::from_utf8(&bytes[..split]).expect("header utf8");
    let rest = &bytes[split + 4..];
    let status_line = head.lines().next().expect("status line");
    let status: u16 = status_line.split_whitespace().nth(1).expect("status").parse().expect("status number");
    let length = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    });
    let body = match length {
        Some(length) => {
            assert!(rest.len() >= length, "short body");
            rest[..length].to_vec()
        }
        None => rest.to_vec(),
    };
    (status, body)
}
