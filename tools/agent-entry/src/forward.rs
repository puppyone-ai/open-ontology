//! Private forwarder. The inbound bearer is only the public Pi marker.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::error::Fault;
use crate::httputil::{read_request, write_json};

pub const PUBLIC_MARKER: &str = "public-proxy-marker";
const CLIENT_VERSION: &str = "1.0.44";
const USER_AGENT: &str = "xai-grok-workspace/1.0.44";

pub struct Forward {
    pub port: u16,
    pub statuses: Arc<Mutex<Vec<u16>>>,
}

pub async fn serve(token_file: String, path_note: String) -> Result<Forward, Fault> {
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|_| Fault::new(500, "forward_bind", 3))?;
    let port = listener.local_addr().map_err(|_| Fault::new(500, "forward_bind", 3))?.port();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&statuses);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { continue };
            let statuses = Arc::clone(&shared);
            let token_file = token_file.clone();
            let path_note = path_note.clone();
            tokio::spawn(async move {
                if let Err(status) = one(&mut sock, &token_file, &path_note, &statuses).await {
                    let body = format!("{{\"error\":\"{status}\"}}");
                    let code = if status == "secret" { 502 } else if status == "model" { 422 } else if status == "client_token" { 401 } else { 502 };
                    let _ = write_json(&mut sock, code, body.as_bytes()).await;
                }
            });
        }
    });
    Ok(Forward { port, statuses })
}

async fn one(sock: &mut TcpStream, token_file: &str, path_note: &str, statuses: &Mutex<Vec<u16>>) -> Result<(), &'static str> {
    let incoming = tokio::time::timeout(Duration::from_secs(30), read_request(sock, 1_048_576)).await.map_err(|_| "deadline")?.map_err(|_| "request")?;
    let allowed = (incoming.method == "GET" && incoming.path == "/v1/models" && incoming.body.is_empty())
        || (incoming.method == "POST" && incoming.path == "/v1/chat/completions");
    if !allowed {
        let _ = write_json(sock, 404, b"{\"error\":\"route\"}").await;
        return Ok(());
    }
    if incoming.authorization != format!("Bearer {PUBLIC_MARKER}") {
        return Err("client_token");
    }
    if incoming.method == "POST" {
        let value: Value = serde_json::from_slice(&incoming.body).map_err(|_| "model")?;
        if value.get("model").and_then(Value::as_str) != Some("grok-4.7") {
            return Err("model");
        }
    }
    let token = read_token(token_file)?;
    if incoming.body.windows(token.len()).any(|window| window == token.as_bytes()) || incoming.body.windows(PUBLIC_MARKER.len()).any(|window| window == PUBLIC_MARKER.as_bytes()) {
        return Err("secret");
    }
    let (path, mut upstream) = dial().await?;
    let _ = fs::write(path_note, path);
    let mut request = format!(
        "{} {} HTTP/1.1\r\nhost: 127.0.0.1:8317\r\nauthorization: Bearer {}\r\nx-grok-client-version: {CLIENT_VERSION}\r\nuser-agent: {USER_AGENT}\r\ncontent-length: {}\r\nconnection: close\r\n",
        incoming.method,
        incoming.path,
        token,
        incoming.body.len()
    );
    if !incoming.content_type.is_empty() {
        request.push_str("content-type: ");
        request.push_str(&incoming.content_type);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    upstream.write_all(request.as_bytes()).await.map_err(|_| "upstream")?;
    if !incoming.body.is_empty() {
        upstream.write_all(&incoming.body).await.map_err(|_| "upstream")?;
    }
    // The token leaves this stack frame only inside the request bytes, which are not logged.
    let response = read_upstream(&mut upstream).await?;
    if response.windows(token.len()).any(|window| window == token.as_bytes()) {
        return Err("secret");
    }
    let response = strip_reasoning(&response).map_err(|_| "upstream")?;
    if response.windows(token.len()).any(|window| window == token.as_bytes()) {
        return Err("secret");
    }
    let status = response_status(&response).unwrap_or(502);
    statuses.lock().await.push(status);
    sock.write_all(&response).await.map_err(|_| "write")?;
    Ok(())
}

fn read_token(path: &str) -> Result<String, &'static str> {
    let meta = fs::symlink_metadata(path).map_err(|_| "token")?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("token");
    }
    if meta.permissions().mode() & 0o777 != 0o600 {
        return Err("token");
    }
    let mut text = fs::read_to_string(path).map_err(|_| "token")?;
    if text.ends_with('\n') {
        text.pop();
    }
    if text.is_empty() || text.len() > 512 || text.bytes().any(|byte| !(0x21..=0x7e).contains(&byte)) || text == PUBLIC_MARKER {
        return Err("token");
    }
    Ok(text)
}

async fn dial() -> Result<(&'static str, TcpStream), &'static str> {
    if let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_millis(400), TcpStream::connect("127.0.0.1:8317")).await {
        return Ok(("127.0.0.1:8317", stream));
    }
    let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect("host.docker.internal:8317"))
        .await
        .map_err(|_| "upstream")?
        .map_err(|_| "upstream")?;
    Ok(("host.docker.internal:8317", stream))
}

async fn read_upstream(stream: &mut TcpStream) -> Result<Vec<u8>, &'static str> {
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(900), async {
        let mut chunk = [0_u8; 16384];
        loop {
            let n = stream.read(&mut chunk).await.map_err(|_| "upstream")?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 8 * 1_048_576 {
                return Err("upstream");
            }
        }
        Ok::<(), &'static str>(())
    })
    .await
    .map_err(|_| "deadline")??;
    if buf.is_empty() { Err("upstream") } else { Ok(buf) }
}

fn response_status(bytes: &[u8]) -> Option<u16> {
    let line = std::str::from_utf8(bytes).ok()?.lines().next()?;
    line.split_whitespace().nth(1)?.parse().ok()
}

const REASONING_KEYS: &[&str] = &["reasoning", "reasoning_content", "reasoning_details", "thinking", "thinking_content"];

/// Drop model reasoning fields so Pi does not emit `thinking_*`. The token is not logged.
fn strip_reasoning(response: &[u8]) -> Result<Vec<u8>, ()> {
    let Some(split) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Ok(response.to_vec());
    };
    let head = &response[..split];
    let body = &response[split + 4..];
    let new_body = if body.starts_with(b"{") { strip_json_body(body)? } else { strip_sse_body(body)? };
    if new_body == body {
        return Ok(response.to_vec());
    }
    let header = std::str::from_utf8(head).map_err(|_| ())?;
    let mut out = String::new();
    for line in header.split("\r\n") {
        if line.to_ascii_lowercase().starts_with("content-length:") {
            out.push_str("content-length: ");
            out.push_str(&new_body.len().to_string());
        } else {
            out.push_str(line);
        }
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(&new_body);
    Ok(bytes)
}

fn strip_json_body(body: &[u8]) -> Result<Vec<u8>, ()> {
    let mut value: Value = serde_json::from_slice(body).map_err(|_| ())?;
    strip_value(&mut value);
    serde_json::to_vec(&value).map_err(|_| ())
}

fn strip_sse_body(body: &[u8]) -> Result<Vec<u8>, ()> {
    let text = std::str::from_utf8(body).map_err(|_| ())?;
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let ending = &line[bare.len()..];
        if let Some(payload) = bare.strip_prefix("data: ") {
            if payload == "[DONE]" || !payload.starts_with('{') {
                out.push_str(line);
                continue;
            }
            match serde_json::from_str::<Value>(payload) {
                Ok(mut value) => {
                    strip_value(&mut value);
                    out.push_str("data: ");
                    out.push_str(&serde_json::to_string(&value).map_err(|_| ())?);
                    out.push_str(ending);
                }
                Err(_) => out.push_str(line),
            }
        } else {
            out.push_str(line);
        }
    }
    Ok(out.into_bytes())
}

fn strip_value(value: &mut Value) {
    match value {
        Value::Array(items) => {
            items.retain(|item| {
                item.get("type").and_then(Value::as_str).is_none_or(|kind| kind != "thinking" && kind != "reasoning")
            });
            for item in items {
                strip_value(item);
            }
        }
        Value::Object(map) => {
            map.retain(|key, _| !REASONING_KEYS.contains(&key.as_str()));
            for item in map.values_mut() {
                strip_value(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

pub fn note_path(state_dir: &str) -> String {
    Path::new(state_dir).join("upstream-path.txt").display().to_string()
}

#[cfg(test)]
mod tests {
    use super::strip_reasoning;

    #[test]
    fn sse_drops_reasoning_and_keeps_content() {
        let payload = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hidden-thought\",\"content\":\"hi\"}}]}\n\n";
        let raw = format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{payload}", payload.len());
        let out = strip_reasoning(raw.as_bytes()).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("reasoning_content"));
        assert!(!text.contains("hidden-thought"));
        assert!(text.contains("\"content\":\"hi\""));
        let length = text.split("\r\n\r\n").nth(1).unwrap().len();
        assert!(text.contains(&format!("content-length: {length}")));
    }
}
