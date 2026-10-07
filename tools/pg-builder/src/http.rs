//! One HTTP/1.1 POST to 127.0.0.1. No proxy, no redirect, no second origin.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::error::PgBuilderError;

pub struct HttpReply {
    pub http_status: u16,
    pub body: Vec<u8>,
    pub latency_ms: u128,
    pub refusal: Option<&'static str>,
}

pub async fn post_loopback(
    addr: SocketAddr,
    path: &str,
    token: &str,
    body: &[u8],
    deadline: Duration,
    max_response: usize,
) -> Result<HttpReply, PgBuilderError> {
    if !addr.ip().is_loopback() || path != "/v1/chat/completions" {
        return Err(PgBuilderError::new("config.endpoint", "the origin is not the loopback builder endpoint"));
    }
    let started = Instant::now();
    let exchange = timeout(deadline, exchange(addr, path, token, body, max_response)).await;
    let latency_ms = started.elapsed().as_millis();
    match exchange {
        Err(_) => Ok(HttpReply {
            http_status: 0,
            body: Vec::new(),
            latency_ms,
            refusal: Some("timeout"),
        }),
        Ok(Err(kind)) => Ok(HttpReply {
            http_status: 0,
            body: Vec::new(),
            latency_ms,
            refusal: Some(kind),
        }),
        Ok(Ok(mut reply)) => {
            reply.latency_ms = latency_ms;
            Ok(reply)
        }
    }
}

async fn exchange(
    addr: SocketAddr,
    path: &str,
    token: &str,
    body: &[u8],
    max_response: usize,
) -> Result<HttpReply, &'static str> {
    let mut stream = TcpStream::connect(addr).await.map_err(|_| "unavailable")?;
    let header = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n",
        port = addr.port(),
        len = body.len(),
    );
    stream.write_all(header.as_bytes()).await.map_err(|_| "unavailable")?;
    stream.write_all(body).await.map_err(|_| "unavailable")?;
    let mut collected = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if collected.len() > max_response.saturating_add(8192) {
            return Ok(HttpReply {
                http_status: 0,
                body: Vec::new(),
                latency_ms: 0,
                refusal: Some("response_bytes"),
            });
        }
        let read = stream.read(&mut chunk).await.map_err(|_| "unavailable")?;
        if read == 0 {
            break;
        }
        collected.extend_from_slice(&chunk[..read]);
    }
    split_response(&collected, max_response)
}

fn split_response(bytes: &[u8], max_response: usize) -> Result<HttpReply, &'static str> {
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("malformed_envelope")?;
    let header = std::str::from_utf8(&bytes[..split]).map_err(|_| "malformed_envelope")?;
    let mut lines = header.split("\r\n");
    let status_line = lines.next().ok_or("malformed_envelope")?;
    let mut parts = status_line.split(' ');
    let version = parts.next().ok_or("malformed_envelope")?;
    let code = parts.next().ok_or("malformed_envelope")?;
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err("malformed_envelope");
    }
    let http_status: u16 = code.parse().map_err(|_| "malformed_envelope")?;
    if (300..400).contains(&http_status) {
        return Ok(HttpReply {
            http_status,
            body: Vec::new(),
            latency_ms: 0,
            refusal: Some("redirect"),
        });
    }
    let mut content_length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.trim().parse::<usize>().map_err(|_| "malformed_envelope")?);
        }
        if name.eq_ignore_ascii_case("transfer-encoding") && value.to_ascii_lowercase().contains("chunked") {
            chunked = true;
        }
        if name.eq_ignore_ascii_case("location") {
            return Ok(HttpReply {
                http_status,
                body: Vec::new(),
                latency_ms: 0,
                refusal: Some("redirect"),
            });
        }
    }
    if chunked {
        return match decode_chunked(&bytes[split + 4..], max_response) {
            Ok(body) => Ok(HttpReply {
                http_status,
                body,
                latency_ms: 0,
                refusal: None,
            }),
            Err("response_bytes") => Ok(HttpReply {
                http_status,
                body: Vec::new(),
                latency_ms: 0,
                refusal: Some("response_bytes"),
            }),
            Err(kind) => Ok(HttpReply {
                http_status,
                body: Vec::new(),
                latency_ms: 0,
                refusal: Some(kind),
            }),
        };
    }
    let mut body = bytes[split + 4..].to_vec();
    if let Some(length) = content_length {
        if length > max_response {
            return Ok(HttpReply {
                http_status,
                body: Vec::new(),
                latency_ms: 0,
                refusal: Some("response_bytes"),
            });
        }
        if body.len() > length {
            body.truncate(length);
        }
        if body.len() != length {
            return Ok(HttpReply {
                http_status,
                body: Vec::new(),
                latency_ms: 0,
                refusal: Some("partial"),
            });
        }
    } else if body.len() > max_response {
        return Ok(HttpReply {
            http_status,
            body: Vec::new(),
            latency_ms: 0,
            refusal: Some("response_bytes"),
        });
    }
    Ok(HttpReply {
        http_status,
        body,
        latency_ms: 0,
        refusal: None,
    })
}

fn decode_chunked(mut rest: &[u8], max_response: usize) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    loop {
        let line_end = rest.windows(2).position(|window| window == b"\r\n").ok_or("partial")?;
        let line = std::str::from_utf8(&rest[..line_end]).map_err(|_| "malformed_envelope")?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        if size_hex.is_empty() {
            return Err("malformed_envelope");
        }
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| "malformed_envelope")?;
        if size == 0 {
            return Ok(out);
        }
        if size > max_response || out.len().saturating_add(size) > max_response {
            return Err("response_bytes");
        }
        let data_start = line_end + 2;
        let data_end = data_start + size;
        if rest.len() < data_end + 2 {
            return Err("partial");
        }
        out.extend_from_slice(&rest[data_start..data_end]);
        if &rest[data_end..data_end + 2] != b"\r\n" {
            return Err("malformed_envelope");
        }
        rest = &rest[data_end + 2..];
    }
}
