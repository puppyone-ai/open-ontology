use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::error::{malformed, Fault};
use crate::tlsutil::find_header_end;

pub struct Incoming {
    pub method: String,
    pub path: String,
    pub query: String,
    pub cookie: String,
    pub csrf: String,
    pub authorization: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub async fn read_request(sock: &mut TcpStream, body_cap: usize) -> Result<Incoming, Fault> {
    let mut buf = Vec::new();
    let head_end = loop {
        let mut chunk = [0_u8; 4096];
        let n = sock.read(&mut chunk).await.map_err(|_| malformed("read"))?;
        if n == 0 {
            return Err(malformed("eof"));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 65_536 + body_cap {
            return Err(malformed("too_large"));
        }
        if let Some(end) = find_header_end(&buf) {
            break end;
        }
        if buf.len() > 65_536 {
            return Err(malformed("headers"));
        }
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| malformed("headers"))?;
    let mut lines = head.lines();
    let request = lines.next().ok_or_else(|| malformed("request"))?;
    let mut parts = request.split(' ');
    let method = parts.next().unwrap_or("").to_owned();
    let target = parts.next().unwrap_or("");
    let (path, query) = target.split_once('?').map(|(p, q)| (p.to_owned(), q.to_owned())).unwrap_or((target.to_owned(), String::new()));
    let mut cookie = String::new();
    let mut csrf = String::new();
    let mut authorization = String::new();
    let mut content_type = String::new();
    let mut length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "cookie" {
                cookie = value.to_owned();
            } else if name == "x-oo-csrf" {
                csrf = value.to_owned();
            } else if name == "authorization" {
                authorization = value.to_owned();
            } else if name == "content-type" {
                content_type = value.to_owned();
            } else if name == "content-length" {
                length = value.parse().map_err(|_| malformed("length"))?;
            }
        }
    }
    if length > body_cap {
        return Err(malformed("too_large"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let mut chunk = [0_u8; 4096];
        let n = sock.read(&mut chunk).await.map_err(|_| malformed("body"))?;
        if n == 0 {
            return Err(malformed("body"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok(Incoming { method, path, query, cookie, csrf, authorization, content_type, body })
}

pub async fn write_json(sock: &mut TcpStream, status: u16, body: &[u8]) -> Result<(), Fault> {
    write_response(sock, status, "application/json", &[], body).await
}

pub async fn write_response(sock: &mut TcpStream, status: u16, content_type: &str, extra: &[(&str, &str)], body: &[u8]) -> Result<(), Fault> {
    let reason = reason(status);
    let mut head = format!("HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
    for (name, value) in extra {
        if name.bytes().any(|b| b == b'\r' || b == b'\n') || value.bytes().any(|b| b == b'\r' || b == b'\n') {
            return Err(malformed("header"));
        }
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    sock.write_all(head.as_bytes()).await.map_err(|_| Fault::new(500, "write", 3))?;
    sock.write_all(body).await.map_err(|_| Fault::new(500, "write", 3))?;
    Ok(())
}

pub fn cookie_pair(header: &str, name: &str) -> Option<String> {
    for part in header.split(';') {
        let part = part.trim();
        let (key, value) = part.split_once('=')?;
        if key.trim() == name {
            return Some(value.trim().to_owned());
        }
    }
    None
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

pub type HeaderPair = (String, String);
pub type HeaderList = Vec<HeaderPair>;
pub type ParsedHttp = (u16, HeaderList, Vec<u8>);

pub fn status_line(bytes: &[u8]) -> Option<ParsedHttp> {
    let end = find_header_end(bytes)?;
    let head = std::str::from_utf8(&bytes[..end]).ok()?;
    let mut lines = head.lines();
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    Some((status, headers, bytes[end + 4..].to_vec()))
}

pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}
