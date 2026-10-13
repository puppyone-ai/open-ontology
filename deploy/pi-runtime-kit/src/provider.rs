//! Local OpenAI-compatible fixture for the installation probe.
//! It returns a read tool call, then a bash `oo --help` tool call, then a stop.
//! It does not call a model and it does not complete a business answer.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::json;
use crate::{PACK_MARKER, PUBLIC_MARKER};

const MAX_BODY: usize = 1_048_576;

pub fn serve(port_file: &Path, log_path: &Path, oo_bin: &str, stop: Arc<AtomicBool>) -> Result<(), String> {
    let result = listen(port_file, log_path, oo_bin, stop);
    if let Err(err) = &result {
        let _ = std::fs::write(port_file.with_extension("err"), err.as_bytes());
    }
    result
}

fn listen(port_file: &Path, log_path: &Path, oo_bin: &str, stop: Arc<AtomicBool>) -> Result<(), String> {
    validate_oo(oo_bin)?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| format!("bind: {err}"))?;
    listener.set_nonblocking(true).map_err(|err| format!("nonblocking: {err}"))?;
    let port = listener.local_addr().map_err(|err| format!("addr: {err}"))?.port();
    std::fs::write(port_file, format!("{port}\n")).map_err(|err| format!("port file: {err}"))?;
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|err| format!("log: {err}"))?;
    writeln!(log, "listen=127.0.0.1:{port}").map_err(|err| format!("log: {err}"))?;
    let mut accepted = 0_u32;
    while !stop.load(Ordering::Relaxed) && accepted < 8 {
        match listener.accept() {
            Ok((stream, peer)) => {
                accepted += 1;
                writeln!(log, "accept={peer}").map_err(|err| format!("log: {err}"))?;
                if let Err(err) = handle(stream, &mut log, oo_bin) {
                    writeln!(log, "handle-error={err}").map_err(|e| format!("log: {e}"))?;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
            Err(err) => return Err(format!("accept: {err}")),
        }
    }
    writeln!(log, "stop").map_err(|err| format!("log: {err}"))?;
    Ok(())
}

fn validate_oo(path: &str) -> Result<(), String> {
    if !path.starts_with('/') || !path.ends_with("/bin/oo") {
        return Err("oo path is not the kit binary".to_owned());
    }
    if !path.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.')) {
        return Err("oo path has an unsafe character".to_owned());
    }
    if path.split('/').skip(1).any(|part| part.is_empty() || part == "." || part == "..") {
        return Err("oo path is not normal".to_owned());
    }
    Ok(())
}

fn handle(mut stream: TcpStream, log: &mut dyn Write, oo_bin: &str) -> Result<(), String> {
    stream.set_read_timeout(Some(Duration::from_secs(20))).map_err(|err| format!("timeout: {err}"))?;
    let (view, body_bytes) = read_request(&mut stream)?;
    writeln!(
        log,
        "method={} path={} local_host={} auth_ok={} auth_other={} body_bytes={}",
        view.method, view.path, view.local_host, view.auth_ok, view.auth_other, view.body_bytes
    )
    .map_err(|err| format!("log: {err}"))?;
    if !view.local_host || view.method != "POST" || !view.path.ends_with("/chat/completions") || !view.auth_ok {
        let status = if view.auth_ok { "404" } else { "401" };
        let raw = format!("HTTP/1.1 {status} denied\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        stream.write_all(raw.as_bytes()).map_err(|err| format!("deny: {err}"))?;
        return Ok(());
    }
    let current = stage_of(&body_bytes);
    let body = match current {
        0 => tool_sse("read", "call_read_pack", r#"{"path":"/knowledge/README.md"}"#),
        1 => tool_sse("bash", "call_bash_help", &format!(r#"{{"command":"{oo_bin} --help"}}"#)),
        _ => stop_sse(),
    };
    stream.write_all(body.as_bytes()).map_err(|err| format!("write: {err}"))?;
    stream.flush().map_err(|err| format!("flush: {err}"))?;
    writeln!(log, "stage={current}").map_err(|err| format!("log: {err}"))?;
    Ok(())
}

struct View {
    method: String,
    path: String,
    local_host: bool,
    auth_ok: bool,
    auth_other: bool,
    body_bytes: usize,
}

fn stage_of(body: &[u8]) -> u32 {
    if find_slice(body, b"Only query is implemented").is_some() {
        2
    } else if find_slice(body, PACK_MARKER.as_bytes()).is_some() {
        1
    } else {
        0
    }
}

fn find_slice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn read_request(stream: &mut TcpStream) -> Result<(View, Vec<u8>), String> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 4096];
    let header_end = loop {
        if buf.len() > MAX_BODY {
            return Err("header too large".to_owned());
        }
        let count = stream.read(&mut tmp).map_err(|err| format!("read: {err}"))?;
        if count == 0 {
            return Err("eof before header".to_owned());
        }
        buf.extend_from_slice(&tmp[..count]);
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
    };
    let header = String::from_utf8_lossy(&buf[..header_end]);
    let mut method = String::new();
    let mut path = String::new();
    let mut host = String::new();
    let mut content_length = 0_usize;
    let mut auth_ok = false;
    let mut auth_other = false;
    for (index, line) in header.split("\r\n").enumerate() {
        if index == 0 {
            let mut parts = line.split(' ');
            method = parts.next().unwrap_or("").to_owned();
            path = parts.next().unwrap_or("").to_owned();
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().map_err(|_| format!("bad length {value}"))?;
        } else if name.eq_ignore_ascii_case("host") {
            host = value.to_owned();
        } else if name.eq_ignore_ascii_case("authorization") {
            if value == format!("Bearer {PUBLIC_MARKER}") {
                auth_ok = true;
            } else if !value.is_empty() {
                auth_other = true;
            }
        }
    }
    if content_length > MAX_BODY {
        return Err("body too large".to_owned());
    }
    let header_bytes = header_end + 4;
    while buf.len() < header_bytes + content_length {
        let count = stream.read(&mut tmp).map_err(|err| format!("body: {err}"))?;
        if count == 0 {
            return Err("eof in body".to_owned());
        }
        buf.extend_from_slice(&tmp[..count]);
    }
    let body = buf[header_bytes..header_bytes + content_length].to_vec();
    Ok((
        View {
            method,
            path,
            local_host: host.starts_with("127.0.0.1:"),
            auth_ok,
            auth_other,
            body_bytes: content_length,
        },
        body,
    ))
}

fn tool_sse(name: &str, id: &str, arguments: &str) -> String {
    let arguments = json::escape(arguments);
    let name = json::escape(name);
    let id = json::escape(id);
    let text = "{\"id\":\"chatcmpl-kit\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"installation-probe\"},\"finish_reason\":null}]}";
    let mut tool = String::from(
        "{\"id\":\"chatcmpl-kit\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":",
    );
    tool.push_str(&id);
    tool.push_str(",\"type\":\"function\",\"function\":{\"name\":");
    tool.push_str(&name);
    tool.push_str(",\"arguments\":");
    tool.push_str(&arguments);
    tool.push_str("}}]},\"finish_reason\":null}]}");
    let finish = "{\"id\":\"chatcmpl-kit\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}";
    sse_response(&[text, &tool, finish])
}

fn sse_response(events: &[&str]) -> String {
    let mut out = String::from(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
    );
    for event in events {
        out.push_str("data: ");
        out.push_str(event);
        out.push_str("\n\n");
    }
    out.push_str("data: [DONE]\n\n");
    out
}

fn stop_sse() -> String {
    let chunk = "{\"id\":\"chatcmpl-kit\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"installation-probe-stopped\"},\"finish_reason\":null}]}";
    let finish = "{\"id\":\"chatcmpl-kit\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}";
    sse_response(&[chunk, finish])
}

#[cfg(test)]
mod tests {
    use super::validate_oo;

    #[test]
    fn absolute_kit_oo_path_is_accepted() {
        assert!(validate_oo("/opt/pi-runtime-kit/bin/oo").is_ok());
        assert!(validate_oo("bin/oo").is_err());
        assert!(validate_oo("/opt/pi-runtime-kit/bin/other").is_err());
        assert!(validate_oo("/opt/pi-runtime-kit/bin/../bin/oo").is_err());
        assert!(validate_oo("/opt//bin/oo").is_err());
    }
}
