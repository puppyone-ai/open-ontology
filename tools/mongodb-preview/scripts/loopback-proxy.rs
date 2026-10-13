//! Forward 127.0.0.1:8317 to the host builder endpoint.
//! pg-builder reads the response until EOF and drops any bytes still in flight
//! when its deadline fires. A bidirectional copy that waits for the client to
//! close before shutting down the client write side never delivers that EOF,
//! so a finished upstream response still becomes http_status 0 / timeout.
//! This proxy reads one request, writes it, reads one response, then shuts
//! the client write side down.
//! It does not shut the upstream write side first. That FIN is a client cancel
//! on the product proxy and comes back as HTTP 499 "context canceled".
//! The request is already delimited by Content-Length.
//! OO_PROXY_MODE=401 opens the upstream socket, replaces Authorization with
//! Bearer invalid, and returns the provider status. It does not invent 401
//! when the upstream connect fails, and it does not log either bearer.
//! Phase lines are names, millisecond offsets, byte counts, and an HTTP status.
//! They do not include the bearer or any body bytes.
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let mode = std::env::var("OO_PROXY_MODE").unwrap_or_default();
    let upstream = std::env::var("OO_UPSTREAM").unwrap_or_else(|_| "host.docker.internal:8317".to_string());
    let bind = std::env::var("OO_PROXY_BIND").unwrap_or_else(|_| "127.0.0.1:8317".to_string());
    let listener = TcpListener::bind(&bind).expect("bind");
    for incoming in listener.incoming() {
        let Ok(client) = incoming else { continue };
        let mode = mode.clone();
        let upstream = upstream.clone();
        thread::spawn(move || {
            let started = Instant::now();
            if mode == "401" {
                upstream_unauthorized(client, &upstream, started);
            } else {
                forward(client, &upstream, started);
            }
        });
    }
}

fn phase(started: Instant, text: &str) {
    let mut err = std::io::stderr();
    let _ = writeln!(err, "proxy_phase {text} ms={}", started.elapsed().as_millis());
    let _ = err.flush();
}

fn upstream_unauthorized(mut client: TcpStream, upstream: &str, started: Instant) {
    phase(started, "mode=401 accept");
    let _ = client.set_read_timeout(Some(Duration::from_secs(30)));
    let request = match read_request(&mut client) {
        Ok(bytes) => bytes,
        Err(kind) => {
            phase(started, &format!("mode=401 request_read_fail kind={kind}"));
            return;
        }
    };
    phase(started, &format!("mode=401 request_bytes={}", request.len()));
    let mut remote = match connect_upstream(upstream, started) {
        Ok(stream) => stream,
        Err(()) => {
            phase(started, "mode=401 upstream_not_opened synthetic_status=none");
            return;
        }
    };
    phase(started, "mode=401 upstream_socket=opened");
    let rewritten = rewrite_authorization(&request);
    if remote.write_all(&rewritten).is_err() {
        phase(started, "mode=401 upstream_write_fail");
        return;
    }
    let response = match read_response(&mut remote, started) {
        Ok(message) => message,
        Err(kind) => {
            phase(started, &format!("mode=401 response_read_fail kind={kind}"));
            return;
        }
    };
    if let Some(status) = status_code(&response.bytes) {
        phase(started, &format!("mode=401 provider_status={status}"));
    }
    if client.write_all(&response.bytes).is_err() {
        phase(started, "mode=401 client_write_fail");
        return;
    }
    let _ = client.shutdown(Shutdown::Write);
    phase(started, "mode=401 client_shutdown");
}

fn rewrite_authorization(request: &[u8]) -> Vec<u8> {
    let Some(end) = find_header_end(request) else {
        return request.to_vec();
    };
    let header = String::from_utf8_lossy(&request[..end]);
    let mut lines = Vec::new();
    let mut replaced = false;
    for line in header.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        if line.to_ascii_lowercase().starts_with("authorization:") {
            lines.push("Authorization: Bearer invalid".to_string());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.push("Authorization: Bearer invalid".to_string());
    }
    let mut bytes = lines.join("\r\n").into_bytes();
    bytes.extend_from_slice(&request[end..]);
    bytes
}

fn forward(mut client: TcpStream, upstream: &str, started: Instant) {
    phase(started, "mode=forward accept");
    let _ = client.set_read_timeout(Some(Duration::from_secs(30)));
    let request = match read_request(&mut client) {
        Ok(bytes) => bytes,
        Err(kind) => {
            phase(started, &format!("mode=forward request_read_fail kind={kind}"));
            return;
        }
    };
    phase(started, &format!("mode=forward request_bytes={}", request.len()));
    let mut remote = match connect_upstream(upstream, started) {
        Ok(stream) => stream,
        Err(()) => return,
    };
    if remote.write_all(&request).is_err() {
        phase(started, "mode=forward upstream_write_fail");
        return;
    }
    phase(started, "mode=forward request_forwarded");
    phase(started, "mode=forward upstream_write_kept");
    let response = match read_response(&mut remote, started) {
        Ok(message) => message,
        Err(kind) => {
            phase(started, &format!("mode=forward response_read_fail kind={kind}"));
            return;
        }
    };
    if client.write_all(&response.bytes).is_err() {
        phase(started, "mode=forward client_write_fail");
        return;
    }
    phase(
        started,
        &format!(
            "mode=forward response_written bytes={} stop={}",
            response.bytes.len(),
            response.stop
        ),
    );
    let _ = client.shutdown(Shutdown::Write);
    phase(started, "mode=forward client_shutdown");
}

fn connect_upstream(upstream: &str, started: Instant) -> Result<TcpStream, ()> {
    let addr = match upstream.to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(addr) => addr,
            None => {
                phase(started, "mode=forward upstream_dns_empty");
                return Err(());
            }
        },
        Err(err) => {
            phase(started, &format!("mode=forward upstream_dns_fail kind={:?}", err.kind()));
            return Err(());
        }
    };
    match TcpStream::connect_timeout(&addr, Duration::from_secs(10)) {
        Ok(stream) => {
            phase(started, "mode=forward upstream_connect_ok");
            Ok(stream)
        }
        Err(err) => {
            phase(started, &format!("mode=forward upstream_connect_fail kind={:?}", err.kind()));
            Err(())
        }
    }
}

struct ResponseMessage {
    bytes: Vec<u8>,
    stop: &'static str,
}

fn read_request(stream: &mut TcpStream) -> Result<Vec<u8>, &'static str> {
    read_message(stream, 262_144, false, Instant::now())
}

fn read_response(stream: &mut TcpStream, started: Instant) -> Result<ResponseMessage, &'static str> {
    let bytes = read_message(stream, 262_144, true, started)?;
    let stop = completion_stop(&bytes);
    if let Some(status) = status_code(&bytes) {
        phase(started, &format!("mode=forward response_status={status}"));
    }
    Ok(ResponseMessage { bytes, stop })
}

fn read_message(
    stream: &mut TcpStream,
    cap: usize,
    log_first: bool,
    started: Instant,
) -> Result<Vec<u8>, &'static str> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let mut saw_byte = false;
    loop {
        if buf.len() > cap.saturating_add(65_536) {
            return Err("too_large");
        }
        let read = stream.read(&mut tmp).map_err(|_| "read")?;
        if read == 0 {
            if buf.is_empty() {
                return Err("eof");
            }
            return Ok(buf);
        }
        if log_first && !saw_byte {
            saw_byte = true;
            phase(started, "mode=forward first_byte");
        }
        buf.extend_from_slice(&tmp[..read]);
        if message_complete(&buf, cap)? {
            return Ok(truncate_complete(buf));
        }
    }
}

fn truncate_complete(buf: Vec<u8>) -> Vec<u8> {
    let Some(end) = find_header_end(&buf) else {
        return buf;
    };
    let Ok(header) = std::str::from_utf8(&buf[..end]) else {
        return buf;
    };
    let (_status, length, _chunked) = header_facts(header);
    if let Some(length) = length {
        let mark = end + 4 + length;
        if buf.len() > mark {
            let mut owned = buf;
            owned.truncate(mark);
            return owned;
        }
    }
    buf
}

fn message_complete(buf: &[u8], cap: usize) -> Result<bool, &'static str> {
    let Some(end) = find_header_end(buf) else {
        return Ok(false);
    };
    let header = std::str::from_utf8(&buf[..end]).map_err(|_| "header")?;
    let (_status, length, chunked) = header_facts(header);
    let body = &buf[end + 4..];
    if let Some(length) = length {
        if length > cap {
            return Err("too_large");
        }
        return Ok(body.len() >= length);
    }
    if chunked {
        return Ok(chunked_complete(body));
    }
    Ok(false)
}

fn completion_stop(buf: &[u8]) -> &'static str {
    let Some(end) = find_header_end(buf) else {
        return "eof";
    };
    let Ok(header) = std::str::from_utf8(&buf[..end]) else {
        return "eof";
    };
    let (_status, length, chunked) = header_facts(header);
    let body = &buf[end + 4..];
    if let Some(length) = length {
        if body.len() >= length {
            "content_length"
        } else {
            "eof_short"
        }
    } else if chunked && chunked_complete(body) {
        "chunked"
    } else {
        "eof"
    }
}

fn status_code(buf: &[u8]) -> Option<u16> {
    let end = find_header_end(buf)?;
    let header = std::str::from_utf8(&buf[..end]).ok()?;
    let status = header_facts(header).0;
    if status == 0 { None } else { Some(status) }
}

fn header_facts(header: &str) -> (u16, Option<usize>, bool) {
    let mut lines = header.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().ok();
        } else if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }
    (status, length, chunked)
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn chunked_complete(mut rest: &[u8]) -> bool {
    loop {
        let Some(line_end) = rest.windows(2).position(|window| window == b"\r\n") else {
            return false;
        };
        let line = std::str::from_utf8(&rest[..line_end]).unwrap_or("");
        let size_hex = line.split(';').next().unwrap_or("").trim();
        if size_hex.is_empty() {
            return false;
        }
        let Ok(size) = usize::from_str_radix(size_hex, 16) else {
            return false;
        };
        if size == 0 {
            return true;
        }
        let data_start = line_end + 2;
        let data_end = data_start.saturating_add(size);
        if rest.len() < data_end + 2 {
            return false;
        }
        if &rest[data_end..data_end + 2] != b"\r\n" {
            return false;
        }
        rest = &rest[data_end + 2..];
    }
}

#[cfg(test)]
mod framing {
    use super::{forward, message_complete, rewrite_authorization, upstream_unauthorized};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn open_upstream_delivers_eof_without_a_client_fin() {
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = proxy.local_addr().unwrap();
        let (saw, saw_rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut sock, _) = upstream.accept().unwrap();
            sock.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            let mut fin_before_response = false;
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) => {
                        fin_before_response = true;
                        break;
                    }
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if message_complete(&buf, 262_144).unwrap_or(false) {
                            break;
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut => {
                        break;
                    }
                    Err(_) => break,
                }
            }
            if !fin_before_response && message_complete(&buf, 262_144).unwrap_or(false) {
                sock.set_read_timeout(Some(Duration::from_millis(150))).unwrap();
                let mut probe = [0u8; 1];
                if matches!(sock.read(&mut probe), Ok(0)) {
                    fin_before_response = true;
                }
            }
            let (status, body): (&str, &[u8]) = if fin_before_response {
                ("499", br#"{"error":{"message":"context canceled"}}"#)
            } else {
                ("200", br#"{"ok":true}"#)
            };
            let header = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(header.as_bytes());
            let _ = sock.write_all(body);
            let _ = saw.send(status);
            thread::sleep(Duration::from_secs(3));
        });
        let upstream_text = upstream_addr.to_string();
        thread::spawn(move || {
            let (client, _) = proxy.accept().unwrap();
            forward(client, &upstream_text, Instant::now());
        });
        let mut client = TcpStream::connect(proxy_addr).unwrap();
        let body = b"{}";
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        client.write_all(request.as_bytes()).unwrap();
        client.write_all(body).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let started = Instant::now();
        let mut got = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            match client.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&tmp[..n]),
                Err(err) => panic!("client did not see EOF: {:?}", err),
            }
        }
        assert!(
            started.elapsed() < Duration::from_millis(1000),
            "elapsed {:?}",
            started.elapsed()
        );
        let text = String::from_utf8(got).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 "), "status {}", text);
        assert!(text.contains("{\"ok\":true}"));
        assert_eq!(saw_rx.recv_timeout(Duration::from_secs(2)).unwrap(), "200");
    }

    #[test]
    fn mode_401_opens_upstream_and_strips_the_bearer() {
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_addr = upstream.local_addr().unwrap();
        let (saw, saw_rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut sock, _) = upstream.accept().unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            loop {
                match sock.read(&mut tmp) {
                    Ok(0) => break,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if message_complete(&buf, 262_144).unwrap_or(false) {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let text = String::from_utf8_lossy(&buf);
            let leaked = text.contains("CANARY-SECRET-VALUE");
            let replaced = text.contains("Authorization: Bearer invalid");
            let _ = saw.send((leaked, replaced));
            let body = b"unauthorized";
            let header = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(header.as_bytes());
            let _ = sock.write_all(body);
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        let upstream_text = upstream_addr.to_string();
        thread::spawn(move || {
            let (client, _) = listener.accept().unwrap();
            upstream_unauthorized(client, &upstream_text, Instant::now());
        });
        let mut client = TcpStream::connect(proxy_addr).unwrap();
        let body = b"{}";
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer CANARY-SECRET-VALUE\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        client.write_all(request.as_bytes()).unwrap();
        client.write_all(body).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut got = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            match client.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&tmp[..n]),
                Err(err) => panic!("client did not see the provider response: {err:?}"),
            }
        }
        let text = String::from_utf8(got).unwrap();
        assert!(text.starts_with("HTTP/1.1 401 "), "{text}");
        assert!(!text.contains("CANARY-SECRET-VALUE"));
        let (leaked, replaced) = saw_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!leaked);
        assert!(replaced);
        let rewritten = rewrite_authorization(request.as_bytes());
        let rewritten = String::from_utf8(rewritten).unwrap();
        assert!(!rewritten.contains("CANARY-SECRET-VALUE"));
        assert!(rewritten.contains("Authorization: Bearer invalid"));
    }

    #[test]
    fn mode_401_does_not_invent_a_status_when_upstream_refuses() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (client, _) = listener.accept().unwrap();
            upstream_unauthorized(client, "127.0.0.1:1", Instant::now());
        });
        let mut client = TcpStream::connect(proxy_addr).unwrap();
        let body = b"{}";
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer CANARY-SECRET-VALUE\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        client.write_all(request.as_bytes()).unwrap();
        client.write_all(body).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut got = Vec::new();
        let mut tmp = [0u8; 256];
        loop {
            match client.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&tmp[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut => {
                    break;
                }
                Err(err) => panic!("{err:?}"),
            }
        }
        let text = String::from_utf8_lossy(&got);
        assert!(!text.contains("401"), "{text}");
        assert!(!text.contains("CANARY-SECRET-VALUE"));
    }
}
