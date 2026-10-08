//! Std-only TCP forwarder. Raw mode copies bytes. HTTP mode logs a redacted response.
#![forbid(unsafe_code)]

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn main() {
    let mut args = env::args().skip(1);
    let mode = args.next().expect("mode");
    let listen = args.next().expect("listen");
    let dest = args.next().expect("dest");
    let log_path = args.next();
    let listener = TcpListener::bind(&listen).expect("bind");
    for incoming in listener.incoming() {
        let Ok(client) = incoming else { continue };
        let dest = dest.clone();
        let mode = mode.clone();
        let log_path = log_path.clone();
        thread::spawn(move || {
            if mode == "http" {
                let _ = http_once(client, &dest, log_path.as_deref());
            } else {
                let _ = raw_once(client, &dest);
            }
        });
    }
}

fn raw_once(mut client: TcpStream, dest: &str) -> io::Result<()> {
    let mut remote = TcpStream::connect(dest)?;
    client.set_read_timeout(Some(Duration::from_secs(60)))?;
    remote.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut client_half = client.try_clone()?;
    let mut remote_half = remote.try_clone()?;
    thread::spawn(move || {
        let _ = io::copy(&mut remote_half, &mut client_half);
    });
    let _ = io::copy(&mut client, &mut remote);
    Ok(())
}

fn http_once(mut client: TcpStream, dest: &str, log_path: Option<&str>) -> io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(45)))?;
    client.set_write_timeout(Some(Duration::from_secs(45)))?;
    let request = read_http(&mut client)?;
    let mut remote = match TcpStream::connect(dest) {
        Ok(stream) => stream,
        Err(err) => {
            if let Some(path) = log_path {
                let _ = std::fs::write(path, b"upstream_connect_failed\n");
            }
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            return Err(err);
        }
    };
    remote.set_read_timeout(Some(Duration::from_secs(45)))?;
    remote.set_write_timeout(Some(Duration::from_secs(45)))?;
    remote.write_all(&request)?;
    let response = read_http(&mut remote)?;
    client.write_all(&response)?;
    if let Some(path) = log_path {
        let _ = write_redacted(path, &response);
    }
    Ok(())
}

fn write_redacted(path: &str, response: &[u8]) -> io::Result<()> {
    let (headers, body) = split_head(response);
    let header_text = String::from_utf8_lossy(headers);
    let mut kept = String::new();
    for line in header_text.split("\r\n") {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("authorization:") || lower.starts_with("proxy-authorization:") {
            continue;
        }
        if lower.contains("bearer ") {
            continue;
        }
        let interesting = lower.starts_with("http/")
            || lower.starts_with("content-type:")
            || lower.contains("cache")
            || lower.contains("cost")
            || lower.contains("token")
            || lower.starts_with("x-");
        if interesting {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    let mut file = File::create(path)?;
    file.write_all(b"headers:\n")?;
    file.write_all(kept.as_bytes())?;
    file.write_all(b"\nbody:\n")?;
    file.write_all(&redact_bearer(body))?;
    Ok(())
}

fn redact_bearer(body: &[u8]) -> Vec<u8> {
    let lower: Vec<u8> = body.iter().map(u8::to_ascii_lowercase).collect();
    let needle = b"bearer ";
    let mut out = Vec::with_capacity(body.len());
    let mut index = 0;
    while index < body.len() {
        if index + needle.len() <= lower.len() && &lower[index..index + needle.len()] == needle {
            out.extend_from_slice(b"bearer redacted");
            index += needle.len();
            while index < body.len() && (0x21..=0x7e).contains(&body[index]) {
                index += 1;
            }
            continue;
        }
        out.push(body[index]);
        index += 1;
    }
    out
}

fn read_http(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 8192];
    let head_end = loop {
        let size = stream.read(&mut tmp)?;
        if size == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof"));
        }
        buf.extend_from_slice(&tmp[..size]);
        if let Some(end) = find_sep(&buf) {
            break end;
        }
        if buf.len() > 2_000_000 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "head"));
        }
    };
    let header_text = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut length = None;
    let mut chunked = false;
    for line in header_text.split("\r\n") {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().ok();
        }
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }
    if let Some(length) = length {
        while buf.len() < head_end + length {
            let size = stream.read(&mut tmp)?;
            if size == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..size]);
            if buf.len() > 4_000_000 {
                break;
            }
        }
        buf.truncate(head_end + length);
    } else if chunked {
        while !buf.windows(5).any(|window| window == b"0\r\n\r\n") && buf.len() < 4_000_000 {
            let size = stream.read(&mut tmp)?;
            if size == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..size]);
        }
    }
    Ok(buf)
}

fn find_sep(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n").map(|index| index + 4)
}

fn split_head(buf: &[u8]) -> (&[u8], &[u8]) {
    match find_sep(buf) {
        Some(end) => (&buf[..end], &buf[end..]),
        None => (buf, b""),
    }
}
