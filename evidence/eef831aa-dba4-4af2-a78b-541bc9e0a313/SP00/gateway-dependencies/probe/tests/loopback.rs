//! Actual 127.0.0.1 HTTP/1.1 checks for the pinned router.

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use oo_gateway_http_baseline_probe::{ProbeGate, BODY_LIMIT};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn echo_returns_body_and_ignores_auth_and_query() {
    let (addr, shutdown) = spawn(ProbeGate::new(oneshot::channel().1)).await;
    let (status, body) = exchange(
        addr,
        "POST /echo?role=admin&claims=all HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n\
         Authorization: Bearer not-a-token\r\n\
         X-OO-Claims: {\"role\":\"admin\"}\r\n\
         Content-Length: 4\r\n\
         Connection: close\r\n\
         \r\n\
         ping",
    )
    .await;
    assert_eq!(status, 200, "status");
    assert_eq!(body, b"ping");
    let text = String::from_utf8(body).expect("utf8");
    assert!(!text.contains("admin"), "query claim leaked into the body");
    assert!(!text.contains("Bearer"), "authorization leaked into the body");
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_at_limit_is_kept_and_one_extra_byte_is_413() {
    let (addr, shutdown) = spawn(ProbeGate::new(oneshot::channel().1)).await;
    let exact = "a".repeat(BODY_LIMIT);
    let (status, body) = exchange(addr, &post_echo(&exact)).await;
    assert_eq!(status, 200, "exact limit");
    assert_eq!(body, exact.as_bytes());
    let over = "b".repeat(BODY_LIMIT + 1);
    let (status, body) = exchange(addr, &post_echo(&over)).await;
    assert_eq!(status, 413, "over limit");
    assert!(
        !body.windows(over.len()).any(|window| window == over.as_bytes()),
        "rejected body was echoed"
    );
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_route_returns_408_before_the_handler_finishes() {
    let (addr, shutdown) = spawn(ProbeGate::new(oneshot::channel().1)).await;
    let started = Instant::now();
    let (status, body) = exchange(
        addr,
        "GET /slow HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    )
    .await;
    let elapsed = started.elapsed();
    assert_eq!(status, 408, "deadline status");
    assert_ne!(body, b"late");
    assert!(
        elapsed < Duration::from_secs(2),
        "handler sleep completed, elapsed {elapsed:?}"
    );
    let _ = shutdown.send(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_request_waits_for_the_single_permit() {
    let (release_tx, release_rx) = oneshot::channel();
    let gate = ProbeGate::new(release_rx);
    let echo_started = std::sync::Arc::clone(&gate.echo_started);
    let hold_entered = std::sync::Arc::clone(&gate.hold_entered);
    let (addr, shutdown) = spawn(gate).await;

    let warmup = Instant::now();
    let (status, body) = exchange(addr, &post_echo("warm")).await;
    assert_eq!(status, 200, "warmup");
    assert_eq!(body, b"warm");
    let warmup_elapsed = warmup.elapsed();
    assert!(
        warmup_elapsed < Duration::from_millis(500),
        "warmup too slow for the bound check: {warmup_elapsed:?}"
    );

    let hold = tokio::spawn(async move {
        exchange(
            addr,
            "GET /hold HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        )
        .await
    });
    let entered = Instant::now();
    while !hold_entered.load(std::sync::atomic::Ordering::SeqCst) {
        if entered.elapsed() > Duration::from_secs(2) {
            panic!("hold handler did not start");
        }
        tokio::task::yield_now().await;
    }

    let echo = tokio::spawn(async move { exchange(addr, &post_echo("queued")).await });
    let budget = warmup_elapsed * 3 + Duration::from_millis(200);
    tokio::time::sleep(budget).await;
    assert_eq!(
        echo_started.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "queued echo entered the handler while /hold held the only permit"
    );

    release_tx.send(()).expect("release hold");
    let (hold_status, hold_body) = hold.await.expect("hold task");
    assert_eq!(hold_status, 200, "hold");
    assert_eq!(hold_body, b"held");
    let (echo_status, echo_body) = echo.await.expect("echo task");
    assert_eq!(echo_status, 200, "queued echo");
    assert_eq!(echo_body, b"queued");
    assert_eq!(echo_started.load(std::sync::atomic::Ordering::SeqCst), 2);
    let _ = shutdown.send(());
}

async fn spawn(gate: ProbeGate) -> (SocketAddr, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, oo_gateway_http_baseline_probe::router(gate))
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await
            .expect("serve");
    });
    (addr, tx)
}

fn post_echo(body: &str) -> String {
    format!(
        "POST /echo HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn exchange(addr: SocketAddr, request: &str) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.expect("read");
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> (u16, Vec<u8>) {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("HTTP header terminator");
    let head = std::str::from_utf8(&bytes[..split]).expect("header utf8");
    let rest = &bytes[split + 4..];
    let status_line = head.lines().next().expect("status line");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .expect("status code")
        .parse()
        .expect("status number");
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
            assert!(
                rest.len() >= length,
                "short body {} < {length}",
                rest.len()
            );
            rest[..length].to_vec()
        }
        None => rest.to_vec(),
    };
    (status, body)
}
