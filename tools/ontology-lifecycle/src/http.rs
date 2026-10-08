//! Loopback HTTP door. A request cannot publish, commit, or bind a pack.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use crate::assess::{claims_success, to_json, Assessment};
use crate::execute::{execute, Plan};

const MAX_HEADERS: usize = 8 * 1024;
const MAX_BODY: usize = 4 * 1024;

pub fn bind_loopback(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
}

pub fn serve_once(listener: TcpListener, root: &Path) -> std::io::Result<Assessment> {
    listener.set_nonblocking(false)?;
    let (mut stream, peer) = listener.accept()?;
    handle(&mut stream, peer, root)
}

fn handle(stream: &mut TcpStream, peer: SocketAddr, root: &Path) -> std::io::Result<Assessment> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    if peer.ip() != Ipv4Addr::LOCALHOST {
        write_response(stream, 403, &error_json("forbidden"))?;
        return Ok(refusal());
    }
    let mut header_buf = [0_u8; MAX_HEADERS];
    let mut used = 0;
    let header_end = loop {
        if used == header_buf.len() {
            write_response(stream, 400, &error_json("malformed"))?;
            return Ok(refusal());
        }
        let n = stream.read(&mut header_buf[used..])?;
        if n == 0 {
            write_response(stream, 400, &error_json("malformed"))?;
            return Ok(refusal());
        }
        used += n;
        if let Some(pos) = find_header_end(&header_buf[..used]) {
            break pos;
        }
    };
    let head = String::from_utf8_lossy(&header_buf[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let Some(request) = lines.next() else {
        write_response(stream, 400, &error_json("malformed"))?;
        return Ok(refusal());
    };
    let mut parts = request.split(' ');
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        write_response(stream, 400, &error_json("malformed"))?;
        return Ok(refusal());
    };
    let path = target.split('?').next().unwrap_or("");
    if path.contains("..") {
        write_response(stream, 400, &error_json("malformed"))?;
        return Ok(refusal());
    }
    let length = content_length(&head);
    let body = if method == "POST" {
        match length {
            Some(len) if len > MAX_BODY => {
                write_response(stream, 400, &error_json("malformed"))?;
                return Ok(refusal());
            }
            Some(len) => match read_body(stream, &header_buf[header_end + 4..used], len) {
                Some(body) => body,
                None => {
                    write_response(stream, 400, &error_json("malformed"))?;
                    return Ok(refusal());
                }
            },
            None => {
                write_response(stream, 400, &error_json("malformed"))?;
                return Ok(refusal());
            }
        }
    } else if length.is_some_and(|len| len > 0) {
        write_response(stream, 400, &error_json("malformed"))?;
        return Ok(refusal());
    } else {
        String::new()
    };
    if claims_success(&body) {
        write_response(stream, 400, &error_json("malformed"))?;
        return Ok(refusal());
    }
    match (method, path) {
        ("GET", "/control/v1/lifecycle/assessment") | ("POST", "/control/v1/lifecycle/runs") => {
            let assessment = execute(root, &Plan::default()).unwrap_or_else(|_| refusal());
            write_response(stream, 409, &to_json(&assessment))?;
            Ok(assessment)
        }
        ("GET" | "POST", _) => {
            write_response(stream, 404, &error_json("not_found"))?;
            Ok(refusal())
        }
        _ => {
            write_response(stream, 405, &error_json("method_not_allowed"))?;
            Ok(refusal())
        }
    }
}

fn read_body(stream: &mut TcpStream, already: &[u8], len: usize) -> Option<String> {
    let mut buf = Vec::with_capacity(len);
    let take = len.min(already.len());
    buf.extend_from_slice(&already[..take]);
    while buf.len() < len {
        let mut tmp = [0_u8; 1024];
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        let room = len - buf.len();
        buf.extend_from_slice(&tmp[..n.min(room)]);
    }
    if buf.len() != len {
        return None;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(head: &str) -> Option<usize> {
    for line in head.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "OK",
    };
    let bytes = body.as_bytes();
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        bytes.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn error_json(code: &str) -> String {
    format!(
        "{{\"contract\":\"oo.ontology-lifecycle/v1\",\"code\":\"{code}\",\"chain_closed\":false,\"execution\":\"not_delivered\",\"production_signed\":false,\"compiled_pack_is_publication\":false}}"
    )
}

fn refusal() -> Assessment {
    Assessment {
        chain_closed: false,
        execution: "not_delivered",
        production_signed: false,
        compiled_pack_is_publication: false,
        parent_acceptance: false,
        children_spawned: 0,
        producer_calls: 0,
        gaps: Vec::new(),
    }
}
