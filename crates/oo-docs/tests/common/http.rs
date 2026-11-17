//! A minimal HTTP/1.1 client for the API tests (no HTTP client crate is
//! admitted). Plain requests read to EOF (`Connection: close`); event
//! streams are read frame by frame from the chunked body.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// A complete response.
#[derive(Debug)]
pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("not json ({e}): {}", String::from_utf8_lossy(&self.body)))
    }
}

fn build(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Vec<u8> {
    let mut req = format!("{method} {path} HTTP/1.1\r\n");
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: {addr}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("Connection: close\r\n");
    match body {
        Some(b) => req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        )),
        None => req.push_str("\r\n"),
    }
    req.into_bytes()
}

type Head = (usize, u16, Vec<(String, String)>);

fn split_head(buf: &[u8]) -> Option<Head> {
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = head.lines();
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    Some((end + 4, status, headers))
}

fn dechunk(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(pos) = data.windows(2).position(|w| w == b"\r\n") {
        let size =
            usize::from_str_radix(String::from_utf8_lossy(&data[..pos]).trim(), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        let start = pos + 2;
        if data.len() < start + size {
            break;
        }
        out.extend_from_slice(&data[start..start + size]);
        data = &data[(start + size + 2).min(data.len())..];
    }
    out
}

/// One request, read to EOF.
pub async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Resp {
    let mut s = TcpStream::connect(addr).await.expect("connect");
    s.write_all(&build(addr, method, path, headers, body))
        .await
        .expect("write");
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(20), s.read_to_end(&mut buf))
        .await
        .expect("response timeout")
        .expect("read");
    let (off, status, headers) = split_head(&buf)
        .unwrap_or_else(|| panic!("no response head: {:?}", String::from_utf8_lossy(&buf)));
    let chunked = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked"));
    let raw = &buf[off..];
    Resp {
        status,
        headers,
        body: if chunked { dechunk(raw) } else { raw.to_vec() },
    }
}

/// An open event stream.
pub struct Sse {
    stream: TcpStream,
    raw: Vec<u8>,
    text: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Body of a non-200 answer (the stream ended after it).
    pub error_body: Vec<u8>,
}

/// Open a stream; on a non-200 answer the full error body is in `error_body`.
pub async fn open_sse(addr: SocketAddr, path: &str, headers: &[(&str, &str)]) -> Sse {
    let mut s = TcpStream::connect(addr).await.expect("connect");
    let mut head = headers.to_vec();
    head.push(("Accept", "text/event-stream"));
    let mut bytes = build(addr, "GET", path, &head, None);
    // Keep the connection open for the stream.
    let text = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("Connection: close\r\n", "");
    bytes = text.into_bytes();
    s.write_all(&bytes).await.expect("write");
    let mut raw = Vec::new();
    let (off, status, headers) = loop {
        let mut chunk = [0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(10), s.read(&mut chunk))
            .await
            .expect("head timeout")
            .expect("read");
        assert!(n > 0, "closed before the response head");
        raw.extend_from_slice(&chunk[..n]);
        if let Some(h) = split_head(&raw) {
            break h;
        }
    };
    let rest = raw.split_off(off);
    let mut sse = Sse {
        stream: s,
        raw: rest,
        text: String::new(),
        status,
        headers,
        error_body: Vec::new(),
    };
    if status != 200 {
        let mut all = std::mem::take(&mut sse.raw);
        let mut tmp = Vec::new();
        let _ = tokio::time::timeout(Duration::from_millis(400), sse.stream.read_to_end(&mut tmp))
            .await;
        all.extend(tmp);
        sse.error_body = all;
    } else {
        sse.absorb();
    }
    sse
}

impl Sse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn error_json(&self) -> Value {
        serde_json::from_slice(&self.error_body).unwrap_or(Value::Null)
    }

    /// Decode whole chunks from `raw` into `text`.
    fn absorb(&mut self) {
        loop {
            let Some(pos) = self.raw.windows(2).position(|w| w == b"\r\n") else {
                return;
            };
            let size = usize::from_str_radix(String::from_utf8_lossy(&self.raw[..pos]).trim(), 16)
                .unwrap_or(0);
            let start = pos + 2;
            if self.raw.len() < start + size + 2 {
                return;
            }
            self.text
                .push_str(&String::from_utf8_lossy(&self.raw[start..start + size]));
            self.raw.drain(..start + size + 2);
            if size == 0 {
                return;
            }
        }
    }

    /// Next frame (text up to the blank line), `None` on timeout or EOF.
    pub async fn frame(&mut self, within: Duration) -> Option<String> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            if let Some(p) = self.text.find("\n\n") {
                let f = self.text[..p].to_owned();
                self.text.drain(..p + 2);
                return Some(f);
            }
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            let mut chunk = [0u8; 8192];
            match tokio::time::timeout(left, self.stream.read(&mut chunk)).await {
                Ok(Ok(0)) | Ok(Err(_)) => return None,
                Ok(Ok(n)) => {
                    self.raw.extend_from_slice(&chunk[..n]);
                    self.absorb();
                }
                Err(_) => return None,
            }
        }
    }

    /// Next frame that carries an event (skips `retry:` and ping comments).
    pub async fn event(&mut self, within: Duration) -> Option<Value> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let f = self.frame(left).await?;
            if let Some(data) = f.lines().find_map(|l| l.strip_prefix("data: ")) {
                return serde_json::from_str(data).ok();
            }
        }
    }
}
