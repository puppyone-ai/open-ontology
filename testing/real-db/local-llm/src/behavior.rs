use std::net::SocketAddr;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify};

use crate::{https_once, load_identity, serve_on, Fault, Limits};

#[derive(Clone)]
enum Script {
    Models,
    Status(u16),
    Hold,
    Big,
}

struct Fixture {
    hits: Arc<AtomicUsize>,
    release: Arc<Notify>,
    released: Arc<AtomicBool>,
    seen: Arc<Mutex<Vec<u8>>>,
    task: tokio::task::JoinHandle<()>,
    port: u16,
}

impl Fixture {
    async fn spawn(script: Script) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("fixture bind");
        let port = listener.local_addr().expect("addr").port();
        let hits = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let released = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let hits_task = Arc::clone(&hits);
        let release_task = Arc::clone(&release);
        let released_task = Arc::clone(&released);
        let seen_task = Arc::clone(&seen);
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                let hits_one = Arc::clone(&hits_task);
                let release_one = Arc::clone(&release_task);
                let released_one = Arc::clone(&released_task);
                let seen_one = Arc::clone(&seen_task);
                let script_one = script.clone();
                tokio::spawn(async move {
                    one_hit(socket, port, script_one, hits_one, release_one, released_one, seen_one).await;
                });
            }
        });
        Self {
            hits,
            release,
            released,
            seen,
            task,
            port,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.released.store(true, Ordering::SeqCst);
        self.release.notify_waiters();
        self.task.abort();
    }
}

async fn one_hit(
    mut socket: TcpStream,
    port: u16,
    script: Script,
    hits: Arc<AtomicUsize>,
    release: Arc<Notify>,
    released: Arc<AtomicBool>,
    seen: Arc<Mutex<Vec<u8>>>,
) {
    hits.fetch_add(1, Ordering::SeqCst);
    let mut buf = vec![0_u8; 8192];
    let read = socket.read(&mut buf).await.unwrap_or(0);
    buf.truncate(read);
    if seen.lock().await.is_empty() {
        *seen.lock().await = buf;
    }
    if matches!(&script, Script::Hold) {
        while !released.load(Ordering::SeqCst) {
            release.notified().await;
        }
    }
    let body: Vec<u8> = match &script {
        Script::Models => br#"{"data":[{"id":"openai/hidden"},{"id":"local-demo"},{"id":"second_ok"}]}"#.to_vec(),
        Script::Status(_) => Vec::new(),
        Script::Hold => br#"{"data":[]}"#.to_vec(),
        Script::Big => vec![b'x'; 128],
    };
    let status = match &script {
        Script::Status(status) => *status,
        _ => 200,
    };
    let header = if matches!(&script, Script::Status(302)) {
        format!(
            "HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:{port}/v1/models\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        )
    } else {
        let len = body.len();
        format!(
            "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {len}\r\nconnection: close\r\n\r\n"
        )
    };
    let mut bytes = header.into_bytes();
    if !matches!(&script, Script::Status(302)) {
        bytes.extend_from_slice(&body);
    }
    let _ = socket.write_all(&bytes).await;
    let _ = socket.shutdown().await;
}

struct Relay {
    port: u16,
    cert: Vec<u8>,
    task: tokio::task::JoinHandle<()>,
}

impl Relay {
    async fn spawn(upstream: u16, limits: Limits) -> Self {
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("oo-a324-relay-{pid}-{upstream}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            dir.join("leaf.ext"),
            "basicConstraints=CA:FALSE\nsubjectAltName=IP:127.0.0.1\nkeyUsage=digitalSignature\nextendedKeyUsage=serverAuth\n",
        )
        .expect("ext");
        openssl(
            &dir,
            &[
                "req",
                "-x509",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:prime256v1",
                "-keyout",
                "ca.key",
                "-out",
                "ca.pem",
                "-days",
                "1",
                "-nodes",
                "-subj",
                "/CN=oo-a324-ca",
                "-addext",
                "basicConstraints=critical,CA:TRUE",
                "-addext",
                "keyUsage=critical,keyCertSign,cRLSign",
            ],
        );
        openssl(
            &dir,
            &[
                "req",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:prime256v1",
                "-keyout",
                "leaf.key",
                "-out",
                "leaf.csr",
                "-nodes",
                "-subj",
                "/CN=127.0.0.1",
            ],
        );
        openssl(
            &dir,
            &[
                "x509",
                "-req",
                "-in",
                "leaf.csr",
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca.key",
                "-CAcreateserial",
                "-out",
                "leaf.pem",
                "-days",
                "1",
                "-extfile",
                "leaf.ext",
            ],
        );
        let ca = std::fs::read(dir.join("ca.pem")).expect("ca");
        let leaf = std::fs::read(dir.join("leaf.pem")).expect("leaf");
        let key = std::fs::read(dir.join("leaf.key")).expect("key");
        let _ = std::fs::remove_dir_all(&dir);
        let identity = load_identity(&leaf, &key).expect("identity");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("relay bind");
        let port = listener.local_addr().expect("addr").port();
        let upstream = SocketAddr::from(([127, 0, 0, 1], upstream));
        let task = tokio::spawn(async move {
            let _ = serve_on(listener, upstream, identity, limits).await;
        });
        Self { port, cert: ca, task }
    }

    fn origin(&self) -> String {
        let port = self.port;
        format!("https://127.0.0.1:{port}")
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn openssl(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("openssl")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("openssl");
    assert!(status.success());
}

fn limits() -> Limits {
    Limits {
        deadline: Duration::from_secs(3),
        ..Limits::standard()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn forwards_models_and_selects_on_the_client_side() {
    let fixture = Fixture::spawn(Script::Models).await;
    let relay = Relay::spawn(fixture.port, limits()).await;
    let reply = https_once(&relay.origin(), "/v1/models", &relay.cert, Some("synthetic-bearer-token"), None, limits())
        .await
        .expect("models");
    assert_eq!(reply.status, 200);
    let selection = crate::select_model(&reply.body).expect("select");
    assert_eq!(selection.selected.as_deref(), Some("local-demo"));
    assert_eq!(selection.grammar_match_count, 2);
    assert_eq!(fixture.hits.load(Ordering::SeqCst), 1);
    let seen = fixture.seen.lock().await;
    let lower = String::from_utf8_lossy(&seen).to_ascii_lowercase();
    assert!(lower.contains("authorization: bearer synthetic-bearer-token"));
    assert!(!reply.body.windows(b"synthetic-bearer-token".len()).any(|window| window == b"synthetic-bearer-token"));
}

#[tokio::test(flavor = "current_thread")]
async fn rejects_other_paths_without_dialing() {
    let fixture = Fixture::spawn(Script::Models).await;
    let mut tight = limits();
    tight.max_request_bytes = 32;
    let relay = Relay::spawn(fixture.port, tight).await;
    let missing = raw_tls(
        relay.port,
        &relay.cert,
        b"GET /v1/other HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
    )
    .await;
    assert!(missing.windows(3).any(|window| window == b"404"));
    let large = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-type: application/json\r\ncontent-length: 80\r\nconnection: close\r\n\r\n{}",
        "y".repeat(80)
    );
    let blocked = raw_tls(relay.port, &relay.cert, large.as_bytes()).await;
    assert!(blocked.windows(3).any(|window| window == b"413"));
    assert_eq!(fixture.hits.load(Ordering::SeqCst), 0);
}

async fn raw_tls(port: u16, cert_pem: &[u8], request: &[u8]) -> Vec<u8> {
    let ders = crate::pem::cert_der(cert_pem).expect("pem");
    let mut roots = rustls::RootCertStore::empty();
    for der in ders {
        roots.add(rustls::pki_types::CertificateDer::from(der)).expect("root");
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("tcp");
    let name = rustls::pki_types::ServerName::try_from("127.0.0.1").expect("name");
    let mut tls = connector.connect(name, tcp).await.expect("tls");
    tls.write_all(request).await.expect("write");
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), tls.read_to_end(&mut buf)).await;
    buf
}

#[tokio::test(flavor = "current_thread")]
async fn post_chat_reaches_fixture_once() {
    let fixture = Fixture::spawn(Script::Models).await;
    let relay = Relay::spawn(fixture.port, limits()).await;
    let body = br#"{"model":"local-demo","messages":[]}"#;
    let reply = https_once(
        &relay.origin(),
        "/v1/chat/completions",
        &relay.cert,
        Some("synthetic-bearer-token"),
        Some(body),
        limits(),
    )
    .await
    .expect("chat");
    assert_eq!(reply.status, 200);
    assert_eq!(fixture.hits.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn redirect_is_not_followed() {
    let fixture = Fixture::spawn(Script::Status(302)).await;
    let relay = Relay::spawn(fixture.port, limits()).await;
    let reply = https_once(&relay.origin(), "/v1/models", &relay.cert, None, None, limits())
        .await
        .expect("status");
    assert_eq!(reply.status, 502);
    assert_eq!(fixture.hits.load(Ordering::SeqCst), 1);
    assert!(!reply.body.windows(b"location".len()).any(|window| window == b"location"));
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_response_is_dropped() {
    let fixture = Fixture::spawn(Script::Big).await;
    let mut tight = limits();
    tight.max_response_bytes = 16;
    let relay = Relay::spawn(fixture.port, tight).await;
    let reply = https_once(&relay.origin(), "/v1/models", &relay.cert, None, None, limits())
        .await
        .expect("capped");
    assert_eq!(reply.status, 502);
    assert!(reply.body.len() < 64);
    assert!(!reply.body.contains(&b'x'));
}

#[tokio::test(flavor = "current_thread")]
async fn third_connection_is_busy() {
    let fixture = Fixture::spawn(Script::Hold).await;
    let mut tight = limits();
    tight.max_concurrency = 2;
    tight.deadline = Duration::from_secs(5);
    let relay = Relay::spawn(fixture.port, tight).await;
    let origin = relay.origin();
    let cert = relay.cert.clone();
    let first = tokio::spawn({
        let origin = origin.clone();
        let cert = cert.clone();
        async move { https_once(&origin, "/v1/models", &cert, None, None, tight).await }
    });
    let second = tokio::spawn({
        let origin = origin.clone();
        let cert = cert.clone();
        async move { https_once(&origin, "/v1/models", &cert, None, None, tight).await }
    });
    let started = Instant::now();
    while fixture.hits.load(Ordering::SeqCst) < 2 && started.elapsed() < Duration::from_secs(2) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(fixture.hits.load(Ordering::SeqCst), 2);
    let third = https_once(&origin, "/v1/models", &cert, None, None, tight).await.expect("busy");
    assert_eq!(third.status, 503);
    fixture.released.store(true, Ordering::SeqCst);
    fixture.release.notify_waiters();
    let _ = first.await;
    let _ = second.await;
}

#[tokio::test(flavor = "current_thread")]
async fn slow_upstream_stops_at_the_deadline() {
    let fixture = Fixture::spawn(Script::Hold).await;
    let mut tight = limits();
    tight.deadline = Duration::from_millis(300);
    let relay = Relay::spawn(fixture.port, tight).await;
    let started = Instant::now();
    let result = https_once(&relay.origin(), "/v1/models", &relay.cert, None, None, Limits {
        deadline: Duration::from_secs(2),
        ..limits()
    })
    .await;
    assert!(started.elapsed() < Duration::from_secs(2));
    match result {
        Ok(reply) => assert_ne!(reply.status, 200),
        Err(fault) => assert_eq!(fault, Fault::upstream()),
    }
}

#[test]
fn listen_is_only_ipv4_loopback() {
    assert!(crate::parse_listen("127.0.0.1:18317").is_ok());
    assert!(crate::parse_listen("0.0.0.0:18317").is_err());
    assert!(crate::parse_listen("192.168.1.3:18317").is_err());
    assert!(crate::parse_listen("127.0.0.1:0").is_err());
}

#[test]
fn non_loopback_upstream_is_rejected() {
    let remote = "8.8.8.8:8317".parse().expect("addr");
    assert!(crate::upstream_url(remote, "/v1/models").is_err());
}
