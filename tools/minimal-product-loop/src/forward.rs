//! Loopback forwarder. Pi sends the public marker. The bearer file is read here
//! and is not placed in the child environment or in argv.
//!
//! Model HTTP uses the development_local headers and 30s budget that returned
//! a grok-4.7 body. The response is taken by content length or chunk framing.
//! The child socket is closed so the caller does not wait for an idle upstream.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const LLM_USER_AGENT: &str = "oo-ontology-dialogue/v1";
/// Per-call budget for a description body. A 30s cap returned an empty timeout
/// for shop.account; the same endpoint then returned HTTP 200 at 25s and 24s.
pub const LLM_DEADLINE_MS: u64 = 90_000;
pub const PUBLIC_MARKER: &str = "public-proxy-marker";
/// pg-builder generate accepts only this loopback port.
pub const MODEL_PORT: u16 = 8317;
/// Byte-copy sidecar. The relay owns 8317, so the host proxy is reached here.
pub const MODEL_SIDECAR_PORT: u16 = 18317;

pub fn serve_ports(listener: TcpListener, connect_port: u16, host_port: u16, bearer: String, stop: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                let _ = forward_one(&mut stream, connect_port, host_port, &bearer);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
}

pub fn serve_aligned_ports(listener: TcpListener, connect_port: u16, host_port: u16, stop: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                let _ = align_one(&mut stream, connect_port, host_port);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn align_one(stream: &mut TcpStream, connect_port: u16, host_port: u16) -> Result<(), ()> {
    let buffer = read_message(stream, 262_144)?;
    let header_end = buffer.windows(4).position(|window| window == b"\r\n\r\n").ok_or(())?;
    let head = std::str::from_utf8(&buffer[..header_end]).map_err(|_| ())?;
    let need = content_length(head).unwrap_or(0);
    let body = &buffer[header_end + 4..header_end + 4 + need];
    let rewritten = rewrite_head(head, host_port, None);
    exchange(stream, connect_port, rewritten.as_bytes(), body)
}

fn forward_one(stream: &mut TcpStream, connect_port: u16, host_port: u16, bearer: &str) -> Result<(), ()> {
    let buffer = read_message(stream, 262_144)?;
    let header_end = buffer.windows(4).position(|window| window == b"\r\n\r\n").ok_or(())?;
    let head = std::str::from_utf8(&buffer[..header_end]).map_err(|_| ())?;
    if !marked(head) {
        let body = b"{\"error\":\"marker\"}";
        let response = format!("HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        stream.write_all(response.as_bytes()).ok();
        stream.write_all(body).ok();
        return Ok(());
    }
    let need = content_length(head).unwrap_or(0);
    let body = &buffer[header_end + 4..header_end + 4 + need];
    let rewritten = rewrite_head(head, host_port, Some(bearer));
    exchange(stream, connect_port, rewritten.as_bytes(), body)
}

fn rewrite_head(head: &str, upstream_port: u16, bearer: Option<&str>) -> String {
    let mut rewritten = String::new();
    let mut saw_agent = false;
    for line in head.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        if line.to_ascii_lowercase().starts_with("authorization:") {
            if let Some(bearer) = bearer {
                rewritten.push_str("Authorization: Bearer ");
                rewritten.push_str(bearer);
                rewritten.push_str("\r\n");
            } else {
                rewritten.push_str(line);
                rewritten.push_str("\r\n");
            }
        } else if line.to_ascii_lowercase().starts_with("host:") {
            rewritten.push_str(&format!("Host: 127.0.0.1:{upstream_port}\r\n"));
        } else if line.to_ascii_lowercase().starts_with("user-agent:") {
            saw_agent = true;
            rewritten.push_str("User-Agent: ");
            rewritten.push_str(LLM_USER_AGENT);
            rewritten.push_str("\r\n");
        } else {
            rewritten.push_str(line);
            rewritten.push_str("\r\n");
        }
    }
    if !saw_agent {
        rewritten.push_str("User-Agent: ");
        rewritten.push_str(LLM_USER_AGENT);
        rewritten.push_str("\r\n");
    }
    rewritten.push_str("\r\n");
    rewritten
}

fn exchange(stream: &mut TcpStream, upstream_port: u16, head: &[u8], body: &[u8]) -> Result<(), ()> {
    let mut upstream = TcpStream::connect(("127.0.0.1", upstream_port)).map_err(|_| ())?;
    upstream.set_read_timeout(Some(Duration::from_millis(LLM_DEADLINE_MS))).ok();
    upstream.write_all(head).map_err(|_| ())?;
    upstream.write_all(body).map_err(|_| ())?;
    let response = read_response(&mut upstream, 262_144)?;
    stream.write_all(&response).map_err(|_| ())?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn read_response(stream: &mut TcpStream, cap: usize) -> Result<Vec<u8>, ()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        if response_complete(&buf) {
            break;
        }
        if buf.len() >= cap {
            return Err(());
        }
        let read = match stream.read(&mut tmp) {
            Ok(read) => read,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(_) => return Err(()),
        };
        if read == 0 {
            break;
        }
        let room = cap - buf.len();
        if read > room {
            return Err(());
        }
        buf.extend_from_slice(&tmp[..read]);
    }
    if response_complete(&buf) || close_delimited(&buf) {
        Ok(buf)
    } else {
        Err(())
    }
}

fn response_complete(buf: &[u8]) -> bool {
    let Some(end) = header_end(buf) else {
        return false;
    };
    let head = std::str::from_utf8(&buf[..end]).unwrap_or("");
    if let Some(need) = content_length(head) {
        return buf.len() >= end + 4 + need;
    }
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        return buf[end + 4..].ends_with(b"0\r\n\r\n");
    }
    false
}

fn close_delimited(buf: &[u8]) -> bool {
    let Some(end) = header_end(buf) else {
        return false;
    };
    let head = std::str::from_utf8(&buf[..end]).unwrap_or("");
    content_length(head).is_none()
        && !head.to_ascii_lowercase().contains("transfer-encoding: chunked")
        && buf.len() > end + 4
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn marked(head: &str) -> bool {
    head.lines().any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("authorization") && value.trim() == "Bearer public-proxy-marker"
    })
}

fn read_message(stream: &mut TcpStream, cap: usize) -> Result<Vec<u8>, ()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        if complete(&buf) {
            break;
        }
        if buf.len() >= cap {
            return Err(());
        }
        let read = stream.read(&mut tmp).map_err(|_| ())?;
        if read == 0 {
            break;
        }
        let room = cap - buf.len();
        if read > room {
            return Err(());
        }
        buf.extend_from_slice(&tmp[..read]);
    }
    if complete(&buf) { Ok(buf) } else { Err(()) }
}

fn complete(buf: &[u8]) -> bool {
    let Some(end) = buf.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let head = std::str::from_utf8(&buf[..end]).unwrap_or("");
    let need = content_length(head).unwrap_or(0);
    buf.len() >= end + 4 + need
}

fn content_length(head: &str) -> Option<usize> {
    for line in head.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::thread;

    #[test]
    fn split_chat_body_reaches_upstream_without_the_marker() {
        let upstream = TcpListener::bind("127.0.0.1:0").expect("bind");
        let upstream_port = upstream.local_addr().expect("addr").port();
        let front = TcpListener::bind("127.0.0.1:0").expect("bind");
        let front_port = front.local_addr().expect("addr").port();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || serve_ports(front, upstream_port, upstream_port, "loop-marker".to_owned(), thread_stop));
        let accepted = thread::spawn(move || {
            let (mut sock, _) = upstream.accept().expect("accept");
            sock.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if complete(&buf) {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let body = b"{\"ok\":true}";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            sock.write_all(response.as_bytes()).expect("write");
            sock.write_all(body).expect("write");
            buf
        });
        let mut client = TcpStream::connect(("127.0.0.1", front_port)).expect("connect");
        client.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");
        let payload = b"{\"model\":\"grok-4.7\"}";
        let head = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{front_port}\r\nauthorization: Bearer public-proxy-marker\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        client.write_all(head.as_bytes()).expect("write");
        thread::sleep(Duration::from_millis(40));
        client.write_all(payload).expect("write");
        let mut response = Vec::new();
        let _ = client.read_to_end(&mut response);
        let forwarded = accepted.join().expect("upstream");
        let text = String::from_utf8_lossy(&forwarded);
        assert!(text.contains("Authorization: Bearer loop-marker"));
        assert!(text.contains("User-Agent: oo-ontology-dialogue/v1"));
        assert!(text.contains("{\"model\":\"grok-4.7\"}"));
        assert!(!text.contains("public-proxy-marker"));
        assert!(response.windows(b"{\"ok\":true}".len()).any(|window| window == b"{\"ok\":true}"));
        stop.store(true, Ordering::Relaxed);
        handle.join().expect("join");
    }

    #[test]
    fn close_delimited_model_body_is_returned_on_eof() {
        let upstream = TcpListener::bind("127.0.0.1:0").expect("bind");
        let upstream_port = upstream.local_addr().expect("addr").port();
        let front = TcpListener::bind("127.0.0.1:0").expect("bind");
        let front_port = front.local_addr().expect("addr").port();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || serve_aligned_ports(front, upstream_port, upstream_port, thread_stop));
        let accepted = thread::spawn(move || {
            let (mut sock, _) = upstream.accept().expect("accept");
            sock.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if complete(&buf) {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let response = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"model\":\"grok-4.7\",\"usage\":{\"total_tokens\":1}}";
            sock.write_all(response).expect("write");
            sock.shutdown(Shutdown::Both).expect("shutdown");
            buf
        });
        let mut client = TcpStream::connect(("127.0.0.1", front_port)).expect("connect");
        client.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");
        let payload = b"{\"model\":\"grok-4.7\"}";
        let head = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{front_port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        client.write_all(head.as_bytes()).expect("write");
        client.write_all(payload).expect("write");
        let mut response = Vec::new();
        let _ = client.read_to_end(&mut response);
        let _ = accepted.join();
        let text = String::from_utf8_lossy(&response);
        assert!(text.contains("\"model\":\"grok-4.7\""));
        assert!(text.contains("total_tokens"));
        stop.store(true, Ordering::Relaxed);
        handle.join().expect("join");
    }
}
