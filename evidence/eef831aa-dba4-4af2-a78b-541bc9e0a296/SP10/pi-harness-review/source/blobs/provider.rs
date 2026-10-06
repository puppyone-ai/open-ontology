//! Local OpenAI-compatible fixture. Public proxy marker only. No upstream calls.
//! This process is the observation provider. It is not the Pi adapter and not a sandbox.

#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const MARKER: &str = "public-proxy-marker";
const MAX_BODY: usize = 1_048_576;
const TOOL_MARK: &str = "pi-harness-fixture";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fixture-provider: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let log_path = args.next().ok_or("usage: fixture-provider <log> <tool|slow>")?;
    let mode = args.next().ok_or("usage: fixture-provider <log> <tool|slow>")?;
    if mode != "tool" && mode != "slow" {
        return Err("mode must be tool or slow".to_owned());
    }
    if args.next().is_some() {
        return Err("usage: fixture-provider <log> <tool|slow>".to_owned());
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| format!("bind: {err}"))?;
    let port = listener.local_addr().map_err(|err| format!("addr: {err}"))?.port();
    println!("port={port}");
    let _ = std::io::stdout().flush();
    let mut log = OpenOptions::new().create(true).append(true).open(&log_path).map_err(|err| format!("log: {err}"))?;
    writeln!(log, "listen=127.0.0.1:{port} mode={mode}").map_err(|err| format!("log: {err}"))?;
    for _ in 0..8 {
        let (stream, peer) = listener.accept().map_err(|err| format!("accept: {err}"))?;
        writeln!(log, "accept={peer}").map_err(|err| format!("log: {err}"))?;
        if let Err(err) = handle(stream, &mut log, &mode) {
            writeln!(log, "handle-error={err}").map_err(|e| format!("log: {e}"))?;
        }
    }
    Ok(())
}

struct RequestView {
    method: String,
    path: String,
    local_host: bool,
    auth_ok: bool,
    auth_other: bool,
    followup: bool,
    body_bytes: usize,
}

fn handle(mut stream: TcpStream, log: &mut dyn Write, mode: &str) -> Result<(), String> {
    stream.set_read_timeout(Some(Duration::from_secs(20))).map_err(|err| format!("timeout: {err}"))?;
    let view = read_request(&mut stream)?;
    let RequestView { method, path, local_host, auth_ok, auth_other, body_bytes, followup } = &view;
    writeln!(
        log,
        "method={method} path={path} local_host={local_host} auth_ok={auth_ok} auth_other={auth_other} body_bytes={body_bytes} followup={followup}"
    )
    .map_err(|err| format!("log: {err}"))?;
    if !view.local_host || !view.path.ends_with("/chat/completions") || !view.auth_ok {
        return deny(&mut stream, log, view.auth_ok);
    }
    write_sse(&mut stream, log, mode, view.followup)
}

fn read_request(stream: &mut TcpStream) -> Result<RequestView, String> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 4096];
    let header_end = read_header(stream, &mut buf, &mut tmp)?;
    let parsed = parse_header(&buf[..header_end])?;
    let body = read_body(stream, &mut buf, header_end, parsed.content_length, &mut tmp)?;
    Ok(RequestView {
        method: parsed.method,
        path: parsed.path,
        local_host: parsed.host.starts_with("127.0.0.1:"),
        auth_ok: parsed.auth_ok,
        auth_other: parsed.auth_other,
        followup: body.windows(TOOL_MARK.len()).any(|window| window == TOOL_MARK.as_bytes()),
        body_bytes: body.len(),
    })
}

fn read_header(stream: &mut TcpStream, buf: &mut Vec<u8>, tmp: &mut [u8]) -> Result<usize, String> {
    loop {
        if buf.len() > MAX_BODY {
            return Err("header too large".to_owned());
        }
        let n = stream.read(tmp).map_err(|err| format!("read: {err}"))?;
        if n == 0 {
            return Err("eof before header".to_owned());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(buf) {
            return Ok(pos);
        }
    }
}

struct Header {
    method: String,
    path: String,
    host: String,
    content_length: usize,
    auth_ok: bool,
    auth_other: bool,
}

fn parse_header(bytes: &[u8]) -> Result<Header, String> {
    let header = String::from_utf8_lossy(bytes);
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
        apply_header_line(line, &mut host, &mut content_length, &mut auth_ok, &mut auth_other)?;
    }
    Ok(Header { method, path, host, content_length, auth_ok, auth_other })
}

fn apply_header_line(
    line: &str,
    host: &mut String,
    content_length: &mut usize,
    auth_ok: &mut bool,
    auth_other: &mut bool,
) -> Result<(), String> {
    let Some((name, value)) = line.split_once(':') else {
        return Ok(());
    };
    let name = name.trim();
    let value = value.trim();
    if name.eq_ignore_ascii_case("content-length") {
        *content_length = value.parse().map_err(|_| format!("bad length {value}"))?;
    } else if name.eq_ignore_ascii_case("host") {
        *host = value.to_owned();
    } else if name.eq_ignore_ascii_case("authorization") {
        let marker = format!("Bearer {MARKER}");
        if value == marker {
            *auth_ok = true;
        } else {
            *auth_other = true;
        }
    }
    Ok(())
}

fn read_body<'a>(
    stream: &mut TcpStream,
    buf: &'a mut Vec<u8>,
    header_end: usize,
    content_length: usize,
    tmp: &mut [u8],
) -> Result<&'a [u8], String> {
    if content_length > MAX_BODY {
        let _ = stream.write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return Err("body too large".to_owned());
    }
    let header_bytes = header_end + 4;
    while buf.len() < header_bytes + content_length {
        let n = stream.read(tmp).map_err(|err| format!("body: {err}"))?;
        if n == 0 {
            return Err("eof in body".to_owned());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > header_bytes + MAX_BODY {
            return Err("body too large".to_owned());
        }
    }
    Ok(&buf[header_bytes..header_bytes + content_length])
}

fn deny(stream: &mut TcpStream, log: &mut dyn Write, auth_ok: bool) -> Result<(), String> {
    let status = if auth_ok { "404" } else { "401" };
    let raw = format!("HTTP/1.1 {status} denied\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(raw.as_bytes()).map_err(|err| format!("deny: {err}"))?;
    writeln!(log, "denied={status}").map_err(|err| format!("log: {err}"))?;
    Ok(())
}

fn write_sse(stream: &mut TcpStream, log: &mut dyn Write, mode: &str, followup: bool) -> Result<(), String> {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    stream.write_all(head.as_bytes()).map_err(|err| format!("head: {err}"))?;
    if mode == "slow" {
        return write_slow(stream, log);
    }
    if followup {
        stream.write_all(sse_text("fixture-reply-after-tool").as_bytes()).map_err(|err| format!("after: {err}"))?;
        stream.write_all(sse_stop().as_bytes()).map_err(|err| format!("stop: {err}"))?;
        stream.flush().map_err(|err| format!("flush: {err}"))?;
        writeln!(log, "sent-after-tool").map_err(|err| format!("log: {err}"))?;
        return Ok(());
    }
    stream.write_all(sse_text("fixture-reply").as_bytes()).map_err(|err| format!("text: {err}"))?;
    stream.flush().map_err(|err| format!("flush: {err}"))?;
    writeln!(log, "sent-text").map_err(|err| format!("log: {err}"))?;
    stream.write_all(sse_tool().as_bytes()).map_err(|err| format!("tool: {err}"))?;
    stream.write_all(sse_finish("tool_calls").as_bytes()).map_err(|err| format!("finish: {err}"))?;
    stream.write_all(b"data: [DONE]\n\n").map_err(|err| format!("done: {err}"))?;
    stream.flush().map_err(|err| format!("flush: {err}"))?;
    writeln!(log, "sent-tool").map_err(|err| format!("log: {err}"))?;
    Ok(())
}

fn write_slow(stream: &mut TcpStream, log: &mut dyn Write) -> Result<(), String> {
    stream.write_all(sse_text("fixture-reply").as_bytes()).map_err(|err| format!("slow: {err}"))?;
    stream.flush().map_err(|err| format!("flush: {err}"))?;
    writeln!(log, "sent-first").map_err(|err| format!("log: {err}"))?;
    thread::sleep(Duration::from_secs(30));
    let _ = stream.write_all(sse_stop().as_bytes());
    let _ = stream.flush();
    writeln!(log, "sent-last").map_err(|err| format!("log: {err}"))?;
    Ok(())
}

fn sse_text(text: &str) -> String {
    let value = serde_json::json!({
        "id": "chatcmpl-fixture",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant", "content": text},
            "finish_reason": null
        }]
    });
    format!("data: {value}\n\n")
}

fn sse_tool() -> String {
    let arguments = r#"{"command":"printf 'pi-harness-fixture\n'"}"#;
    let value = serde_json::json!({
        "id": "chatcmpl-fixture",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_fixed",
                    "type": "function",
                    "function": {"name": "bash", "arguments": arguments}
                }]
            },
            "finish_reason": null
        }]
    });
    format!("data: {value}\n\n")
}

fn sse_finish(reason: &str) -> String {
    let value = serde_json::json!({
        "id": "chatcmpl-fixture",
        "object": "chat.completion.chunk",
        "choices": [{"index": 0, "delta": {}, "finish_reason": reason}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    });
    format!("data: {value}\n\n")
}

fn sse_stop() -> String {
    format!("{}data: [DONE]\n\n", sse_finish("stop"))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}
