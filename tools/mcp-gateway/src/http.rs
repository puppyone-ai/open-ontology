//! Loopback HTTP/1.1 client for the existing Gateway process.
//!
//! One request, no redirect, no connection pool. The bearer is read from the
//! token file on every call and cleared before the function returns.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use zeroize::Zeroize;

const MAX_RESPONSE: usize = 1_048_576 + 8_192;
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) struct GatewayClient {
    port: u16,
    token_file: PathBuf,
}

pub(crate) struct GatewayReply {
    pub(crate) status: u16,
    pub(crate) body: Value,
}

struct Secret(String);

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl GatewayClient {
    pub(crate) fn new(port: u16, token_file: PathBuf) -> Self {
        Self { port, token_file }
    }

    pub(crate) async fn get(&self, path: &str) -> Result<GatewayReply, ()> {
        self.exchange(path, None).await
    }

    pub(crate) async fn post_json(&self, path: &str, body: &Value) -> Result<GatewayReply, ()> {
        let bytes = serde_json::to_vec(body).map_err(|_| ())?;
        self.exchange(path, Some(bytes)).await
    }
}

impl GatewayClient {
    async fn exchange(&self, path: &str, body: Option<Vec<u8>>) -> Result<GatewayReply, ()> {
        if !path.starts_with('/') || path.contains('?') || path.contains('#') {
            return Err(());
        }
        let token = read_token(&self.token_file).await?;
        let mut request = Vec::new();
        let result = write_and_read(&mut request, self.port, path, &token.0, body).await;
        request.zeroize();
        let (status, payload) = result?;
        let body = serde_json::from_slice(&payload).map_err(|_| ())?;
        Ok(GatewayReply { status, body })
    }
}

async fn write_and_read(
    request: &mut Vec<u8>,
    port: u16,
    path: &str,
    token: &str,
    body: Option<Vec<u8>>,
) -> Result<(u16, Vec<u8>), ()> {
    match body {
        None => {
            push_str(
                request,
                &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n"),
            );
        }
        Some(ref bytes) => {
            push_str(
                request,
                &format!(
                    "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
                    bytes.len()
                ),
            );
        }
    }
    push_str(request, "Authorization: Bearer ");
    request.extend_from_slice(token.as_bytes());
    push_str(request, "\r\nConnection: close\r\n\r\n");
    if let Some(bytes) = body {
        request.extend_from_slice(&bytes);
    }
    let exchange = async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.map_err(|_| ())?;
        stream.write_all(request).await.map_err(|_| ())?;
        stream.flush().await.map_err(|_| ())?;
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = stream.read(&mut chunk).await.map_err(|_| ())?;
            if read == 0 {
                break;
            }
            if buffer.len() + read > MAX_RESPONSE {
                return Err(());
            }
            buffer.extend_from_slice(&chunk[..read]);
            if message_complete(&buffer) {
                break;
            }
        }
        parse_response(&buffer)
    };
    timeout(EXCHANGE_TIMEOUT, exchange).await.map_err(|_| ())?
}

fn push_str(buffer: &mut Vec<u8>, text: &str) {
    buffer.extend_from_slice(text.as_bytes());
}

fn message_complete(bytes: &[u8]) -> bool {
    let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = &bytes[..split];
    let body = &bytes[split + 4..];
    match content_length(head) {
        Some(length) => body.len() >= length,
        None => false,
    }
}

fn parse_response(bytes: &[u8]) -> Result<(u16, Vec<u8>), ()> {
    let split = bytes.windows(4).position(|window| window == b"\r\n\r\n").ok_or(())?;
    let head = &bytes[..split];
    let rest = &bytes[split + 4..];
    let status_line = head.split(|byte| *byte == b'\n').next().ok_or(())?;
    let status_line = status_line.strip_suffix(b"\r").unwrap_or(status_line);
    let mut parts = status_line.split(|byte| *byte == b' ');
    let version = parts.next().ok_or(())?;
    if version != b"HTTP/1.1" && version != b"HTTP/1.0" {
        return Err(());
    }
    let code = std::str::from_utf8(parts.next().ok_or(())?).map_err(|_| ())?;
    let status: u16 = code.parse().map_err(|_| ())?;
    let length = content_length(head).ok_or(())?;
    if rest.len() < length {
        return Err(());
    }
    Ok((status, rest[..length].to_vec()))
}

fn content_length(head: &[u8]) -> Option<usize> {
    let mut length = None;
    for line in head.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|byte| *byte == b':') else {
            continue;
        };
        if !line[..colon].eq_ignore_ascii_case(b"content-length") {
            continue;
        }
        let value = std::str::from_utf8(&line[colon + 1..]).ok()?.trim();
        length = value.parse().ok();
    }
    length
}

async fn read_token(path: &Path) -> Result<Secret, ()> {
    let bytes = tokio::fs::read(path).await.map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err(());
    }
    let mut text = String::from_utf8(bytes).map_err(|_| ())?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() || text.bytes().any(|byte| byte.is_ascii_whitespace()) {
        text.zeroize();
        return Err(());
    }
    Ok(Secret(text))
}

pub(crate) fn parse_loopback_port(url: &str) -> Result<u16, &'static str> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err("gateway_url");
    };
    if rest.contains('@') || rest.contains('?') || rest.contains('#') || rest.contains('[') {
        return Err("gateway_url");
    }
    let (host_port, path) = match rest.split_once('/') {
        Some((host_port, path)) => (host_port, path),
        None => (rest, ""),
    };
    if !path.is_empty() {
        return Err("gateway_url");
    }
    let Some((host, port)) = host_port.split_once(':') else {
        return Err("gateway_url");
    };
    if host != "127.0.0.1" || port.is_empty() {
        return Err("gateway_url");
    }
    let port: u16 = port.parse().map_err(|_| "gateway_url")?;
    if port == 0 {
        Err("gateway_url")
    } else {
        Ok(port)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_loopback_port;

    #[test]
    fn loopback_url_accepts_only_a_numeric_port() {
        assert_eq!(parse_loopback_port("http://127.0.0.1:9"), Ok(9));
        assert!(parse_loopback_port("https://127.0.0.1:9").is_err());
        assert!(parse_loopback_port("http://user:secret@127.0.0.1:9").is_err());
        assert!(parse_loopback_port("http://127.0.0.1:9/v1").is_err());
        assert!(parse_loopback_port("http://127.0.0.1:9?x=1").is_err());
        assert!(parse_loopback_port("http://127.0.0.1:9#frag").is_err());
        assert!(parse_loopback_port("http://localhost:9").is_err());
        assert!(parse_loopback_port("http://127.0.0.1").is_err());
        assert!(parse_loopback_port("http://127.0.0.1:0").is_err());
    }
}
