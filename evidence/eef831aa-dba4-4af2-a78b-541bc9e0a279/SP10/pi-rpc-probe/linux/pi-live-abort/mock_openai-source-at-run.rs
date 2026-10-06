//! Local OpenAI-compatible fixture. Public dummy token only. No upstream calls.
//! This is the observation provider, not the Pi adapter.

#![forbid(unsafe_code)]

use std::env;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const TOKEN: &str = "public-dummy-fixture-token";
const MAX_BODY: usize = 1_048_576;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mock_openai: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let log_path = args.next().ok_or("usage: mock_openai <log> <fast|slow>")?;
    let mode = args.next().ok_or("usage: mock_openai <log> <fast|slow>")?;
    if mode != "fast" && mode != "slow" {
        return Err("mode must be fast or slow".to_owned());
    }
    if args.next().is_some() {
        return Err("usage: mock_openai <log> <fast|slow>".to_owned());
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| format!("bind: {err}"))?;
    let port = listener
        .local_addr()
        .map_err(|err| format!("addr: {err}"))?
        .port();
    println!("port={port}");
    let _ = std::io::stdout().flush();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| format!("log: {err}"))?;
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

fn handle(mut stream: TcpStream, log: &mut dyn Write, mode: &str) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|err| format!("timeout: {err}"))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        if buf.len() > MAX_BODY {
            return Err("header too large".to_owned());
        }
        let n = stream.read(&mut tmp).map_err(|err| format!("read: {err}"))?;
        if n == 0 {
            return Err("eof before header".to_owned());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
    };
    let header = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut content_length = 0usize;
    let mut method = String::new();
    let mut path = String::new();
    let mut host = String::new();
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
            if value == format!("Bearer {TOKEN}") {
                auth_ok = true;
            } else {
                auth_other = true;
            }
        }
    }
    if content_length > MAX_BODY {
        let _ = stream.write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return Err("body too large".to_owned());
    }
    let header_bytes = header_end + 4;
    while buf.len() < header_bytes + content_length {
        let n = stream.read(&mut tmp).map_err(|err| format!("body: {err}"))?;
        if n == 0 {
            return Err("eof in body".to_owned());
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = &buf[header_bytes..header_bytes + content_length];
    let local_host = host.starts_with("127.0.0.1:");
    writeln!(
        log,
        "method={method} path={path} host={host} local_host={local_host} auth_ok={auth_ok} auth_other={auth_other} body_bytes={}",
        body.len()
    )
    .map_err(|err| format!("log: {err}"))?;
    writeln!(log, "body={}", String::from_utf8_lossy(body)).map_err(|err| format!("log: {err}"))?;
    if !local_host || !path.ends_with("/chat/completions") || !auth_ok {
        let status = if auth_ok { "404" } else { "401" };
        let raw = format!("HTTP/1.1 {status} denied\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        stream
            .write_all(raw.as_bytes())
            .map_err(|err| format!("deny: {err}"))?;
        writeln!(log, "denied={status}").map_err(|err| format!("log: {err}"))?;
        return Ok(());
    }
    let first = "data: {\"id\":\"chatcmpl-fixture\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"fixture-reply\"},\"finish_reason\":null}]}\n\n";
    let last = "data: {\"id\":\"chatcmpl-fixture\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"total_tokens\":2}}\n\ndata: [DONE]\n\n";
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    stream
        .write_all(head.as_bytes())
        .map_err(|err| format!("head: {err}"))?;
    stream
        .write_all(first.as_bytes())
        .map_err(|err| format!("first: {err}"))?;
    stream.flush().map_err(|err| format!("flush: {err}"))?;
    writeln!(log, "sent-first").map_err(|err| format!("log: {err}"))?;
    if mode == "slow" {
        thread::sleep(Duration::from_secs(4));
    }
    match stream.write_all(last.as_bytes()) {
        Ok(()) => {
            let _ = stream.flush();
            writeln!(log, "sent-last").map_err(|err| format!("log: {err}"))?;
        }
        Err(err) => {
            writeln!(log, "client-closed-before-last={err}").map_err(|e| format!("log: {e}"))?;
        }
    }
    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}
