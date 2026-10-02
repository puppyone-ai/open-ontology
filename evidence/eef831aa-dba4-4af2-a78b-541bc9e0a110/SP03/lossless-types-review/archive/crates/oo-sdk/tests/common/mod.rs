//! Synthetic loopback protocol oracle.
//!
//! This is not a Gateway and not PostgreSQL. It records one HTTP/1.1 request
//! and writes a scripted response so the client tests can check bytes.
//!
//! Each integration test is its own crate, so helpers used by only one of
//! them are unused in the others.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oo_core::api::{QueryRequest, parse_api_json};
use oo_core::diagnostic::Diagnostic;
use oo_sdk::{BearerToken, Client, ClientConfig, RetryPolicy};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const TOKEN: &str = "sdk-token-a073";

pub const QUERY_REQUEST: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/query-request.json");
pub const QUERY_REQUEST_WIRE: &[u8] =
    include_bytes!("../../../../contracts/sdk/json-query/fixtures/query-request.wire.json");
pub const QUERY_RESPONSE: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/query-response.json");
pub const QUERY_RESPONSE_EMPTY: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/query-response-empty.json");
pub const QUERY_RESPONSE_TRUNCATED: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/query-response-truncated.json");
pub const ERROR_RATE_LIMITED: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/error-rate-limited.json");
pub const ERROR_UNAUTHENTICATED: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/error-unauthenticated.json");
pub const ERROR_OVERLOADED: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/error-overloaded.json");
pub const ERROR_TIMEOUT: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/error-timeout.json");
pub const ERROR_INTERNAL: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/error-internal.json");
pub const ERROR_VERSION: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/error-version-unsupported.json");
pub const ERROR_SQL: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/error-sql-parse.json");
pub const REJECT_ROW_COUNT: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/reject-row-count.json");
pub const REJECT_ROW_WIDTH: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/reject-row-width.json");
pub const REJECT_UNKNOWN: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/reject-unknown-field.json");
pub const REJECT_NUMERIC: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/reject-numeric-cell.json");
pub const REJECT_OBJECT: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/reject-object-cell.json");
pub const REJECT_VERSION: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/reject-empty-version.json");
pub const REJECT_DUPLICATE: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/reject-duplicate-key.json");
pub const REJECT_TRAILING: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/reject-trailing.json");

pub const CA_PEM: &str = include_str!("../../../../contracts/sdk/json-query/fixtures/tls/ca.pem");
pub const LOCALHOST_PEM: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/tls/localhost.pem");
pub const LOCALHOST_KEY: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/tls/localhost.key");
pub const OTHER_CA_PEM: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/tls/other-ca.pem");
pub const OTHER_LOCALHOST_PEM: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/tls/other-localhost.pem");
pub const OTHER_LOCALHOST_KEY: &str =
    include_str!("../../../../contracts/sdk/json-query/fixtures/tls/other-localhost.key");

#[derive(Clone, Debug)]
pub struct Hit {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Hit {
    pub fn one<'a>(&'a self, name: &str) -> Option<&'a str> {
        let mut found = None;
        for (header, value) in &self.headers {
            if header == name {
                if found.is_some() {
                    return None;
                }
                found = Some(value.as_str());
            }
        }
        found
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.body
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }
}

pub enum Action {
    Raw(Vec<u8>),
    Reset(Vec<u8>),
    Stall {
        prefix: Vec<u8>,
        delay: Duration,
        suffix: Vec<u8>,
    },
}

pub struct Oracle {
    pub addr: SocketAddr,
    hits: Arc<Mutex<Vec<Hit>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Oracle {
    pub async fn spawn<F>(plan: F) -> Self
    where
        F: Fn(usize, &Hit) -> Action + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hits = Arc::new(Mutex::new(Vec::new()));
        let stored = Arc::clone(&hits);
        let plan = Arc::new(plan);
        let task = tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                if let Err(error) = handle_tcp(sock, &stored, plan.as_ref()).await {
                    let _ = error;
                }
            }
        });
        Self { addr, hits, task }
    }

    pub fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn hits(&self) -> Vec<Hit> {
        self.hits.lock().expect("hits").clone()
    }
}

impl Drop for Oracle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn sample_request() -> QueryRequest {
    parse_api_json(QUERY_REQUEST, empty_diagnostics).expect("request fixture")
}

fn empty_diagnostics(_: &QueryRequest) -> Vec<Diagnostic> {
    Vec::new()
}

pub fn local_config() -> ClientConfig {
    ClientConfig::local_development().with_retry(RetryPolicy::none())
}

pub fn open(base: &str, token: &str, config: ClientConfig) -> Client {
    let token = BearerToken::new(token).expect("token shape");
    Client::new(base, token, config).expect("client")
}

pub fn http_response(
    status: u16,
    reason: &str,
    content_type: Option<&str>,
    extra: &[(&str, &str)],
    body: &[u8],
) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {reason}\r\n").into_bytes();
    if let Some(content_type) = content_type {
        out.extend(format!("Content-Type: {content_type}\r\n").into_bytes());
    }
    for (name, value) in extra {
        out.extend(format!("{name}: {value}\r\n").into_bytes());
    }
    out.extend(
        format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes(),
    );
    out.extend_from_slice(body);
    out
}

pub fn json_response(status: u16, reason: &str, body: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    http_response(
        status,
        reason,
        Some("application/json"),
        extra,
        body.as_bytes(),
    )
}

pub fn chunked_body(content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
    )
    .into_bytes();
    out.extend(format!("{:x}\r\n", body.len()).into_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(b"\r\n0\r\n\r\n");
    out
}

pub fn assert_query(hit: &Hit, token: &str, path: &str) {
    assert_eq!(hit.method, "POST");
    assert_eq!(hit.path, path);
    let expected = format!("Bearer {token}");
    assert!(
        hit.one("authorization") == Some(expected.as_str()),
        "authorization mismatch"
    );
    assert_eq!(hit.one("content-type"), Some("application/json"));
    assert_eq!(hit.one("accept"), Some("application/json"));
    assert_eq!(hit.one("user-agent"), Some("oo-sdk/0.1"));
    assert!(hit.one("proxy-authorization").is_none(), "proxy header");
    const ALLOWED: &[&str] = &[
        "host",
        "content-type",
        "content-length",
        "accept",
        "authorization",
        "user-agent",
        "connection",
        "accept-encoding",
    ];
    for (name, _) in &hit.headers {
        assert!(ALLOWED.contains(&name.as_str()), "unexpected header {name}");
    }
    let declared = hit
        .one("content-length")
        .and_then(|value| value.parse::<usize>().ok());
    assert_eq!(declared, Some(hit.body.len()), "content-length");
}

async fn handle_tcp(
    mut sock: TcpStream,
    hits: &Mutex<Vec<Hit>>,
    plan: &(dyn Fn(usize, &Hit) -> Action + Send + Sync),
) -> std::io::Result<()> {
    let _ = sock.set_nodelay(true);
    let hit = read_hit(&mut sock).await?;
    let action = {
        let mut guard = hits.lock().expect("hits");
        let action = plan(guard.len(), &hit);
        guard.push(hit);
        action
    };
    match action {
        Action::Reset(bytes) => {
            let _ = sock.write_all(&bytes).await;
            let _ = sock.flush().await;
            if let Ok(std_sock) = sock.into_std() {
                let _ = socket2::SockRef::from(&std_sock).set_linger(Some(Duration::from_secs(0)));
            }
            Ok(())
        }
        other => write_action(&mut sock, other).await,
    }
}

async fn write_action<S>(sock: &mut S, action: Action) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    match action {
        Action::Raw(bytes) => {
            sock.write_all(&bytes).await?;
            sock.flush().await?;
        }
        Action::Stall {
            prefix,
            delay,
            suffix,
        } => {
            sock.write_all(&prefix).await?;
            sock.flush().await?;
            tokio::time::sleep(delay).await;
            let _ = sock.write_all(&suffix).await;
            let _ = sock.flush().await;
        }
        Action::Reset(_) => {}
    }
    Ok(())
}

pub async fn read_hit<R>(sock: &mut R) -> std::io::Result<Hit>
where
    R: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    let sep = loop {
        let read = sock.read(&mut tmp).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof before headers",
            ));
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 65_536 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "headers too large",
            ));
        }
    };
    let head = std::str::from_utf8(&buf[..sep])
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "header bytes"))?;
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "header line",
            ));
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let want = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    if want > 2_000_000 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request body cap",
        ));
    }
    let mut body = buf[sep + 4..].to_vec();
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
