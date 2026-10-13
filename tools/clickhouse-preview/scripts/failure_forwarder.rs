use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::thread;
use std::time::Duration;

fn main() {
    let mut args = env::args().skip(1);
    let mode = args.next().expect("mode");
    let log_path = args.next().expect("log");
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("log");
    let listener = TcpListener::bind("127.0.0.1:8317").expect("bind");
    writeln!(log, "FORWARDER_READY {mode}").expect("ready");
    log.flush().expect("flush");
    let mut client = listener.accept().expect("accept").0;
    client
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("client timeout");
    let request = read_request(&mut client).expect("request");
    writeln!(log, "REQUEST_BYTES {}", request.len()).expect("request bytes");
    log.flush().expect("flush");
    match mode.as_str() {
        "timeout" => {
            writeln!(log, "HOLD").expect("hold");
            log.flush().expect("flush");
            thread::sleep(Duration::from_secs(600));
        }
        "status" => {
            let code = args.next().expect("status");
            let body_path = args.next().expect("body");
            send_file(&mut client, &code, &body_path, &mut log);
        }
        "complete" => {
            let body_path = args.next().expect("body");
            send_file(&mut client, "200", &body_path, &mut log);
        }
        "proxy" => proxy(client, request, &mut log),
        other => {
            let _ = writeln!(log, "FORWARDER_DONE unknown {other}");
            std::process::exit(2);
        }
    }
}

fn send_file(client: &mut TcpStream, code: &str, body_path: &str, log: &mut dyn Write) {
    let body = fs::read(body_path).expect("body");
    let header = format!(
        "HTTP/1.1 {code} builder\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        len = body.len()
    );
    client.write_all(header.as_bytes()).expect("header");
    client.write_all(&body).expect("body");
    let _ = client.shutdown(Shutdown::Write);
    writeln!(log, "FORWARDER_DONE {code} bytes {}", body.len()).expect("done");
    log.flush().expect("flush");
}

fn proxy(mut client: TcpStream, request: Vec<u8>, log: &mut dyn Write) {
    let upstream_addr = "host.docker.internal:8317"
        .to_socket_addrs()
        .expect("resolve")
        .find(|addr| addr.is_ipv4())
        .expect("ipv4");
    writeln!(log, "UPSTREAM {upstream_addr}").expect("upstream");
    log.flush().expect("flush");
    let mut upstream = TcpStream::connect(upstream_addr).expect("connect");
    upstream
        .set_read_timeout(Some(Duration::from_secs(100)))
        .expect("upstream timeout");
    upstream.write_all(&request).expect("request write");
    let response = read_request(&mut upstream).expect("response");
    let code = status_code(&response);
    writeln!(log, "FORWARDER_DONE {code} bytes {}", response.len()).expect("done");
    log.flush().expect("flush");
    client.write_all(&response).expect("response write");
    let _ = client.shutdown(Shutdown::Write);
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        if let Some(total) = message_len(&buf) {
            if buf.len() >= total {
                buf.truncate(total);
                return Ok(buf);
            }
        } else if headers_done(&buf) && !length_delimited(&buf) {
            let read = match stream.read(&mut chunk) {
                Ok(read) => read,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut => {
                    return Ok(buf);
                }
                Err(err) => return Err(err),
            };
            if read == 0 {
                return Ok(buf);
            }
            buf.extend_from_slice(&chunk[..read]);
            continue;
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(buf);
        }
        if buf.len().saturating_add(read) > 2_000_000 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "message exceeds 2000000 bytes",
            ));
        }
        buf.extend_from_slice(&chunk[..read]);
    }
}

fn headers_done(buf: &[u8]) -> bool {
    buf.windows(4).any(|window| window == b"\r\n\r\n")
}

fn length_delimited(buf: &[u8]) -> bool {
    let Some(split) = buf.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let Ok(header) = std::str::from_utf8(&buf[..split]) else {
        return false;
    };
    header.split("\r\n").skip(1).any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("content-length")
            || (name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked"))
    })
}

fn message_len(buf: &[u8]) -> Option<usize> {
    let body_at = buf.windows(4).position(|window| window == b"\r\n\r\n")? + 4;
    let header = std::str::from_utf8(buf.get(..body_at - 4)?).ok()?;
    let mut content_length = None;
    let mut chunked = false;
    for line in header.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().ok();
        }
        if name.eq_ignore_ascii_case("transfer-encoding") && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }
    if chunked {
        return chunked_len(buf.get(body_at..)?).map(|len| body_at + len);
    }
    content_length.map(|len| body_at + len)
}

fn chunked_len(mut rest: &[u8]) -> Option<usize> {
    let mut consumed = 0usize;
    loop {
        let line_end = rest.windows(2).position(|window| window == b"\r\n")?;
        let line = std::str::from_utf8(rest.get(..line_end)?).ok()?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        if size_hex.is_empty() {
            return None;
        }
        let size = usize::from_str_radix(size_hex, 16).ok()?;
        let data_start = line_end + 2;
        if size == 0 {
            let after = rest.get(data_start..)?;
            if after.starts_with(b"\r\n") {
                return Some(consumed + data_start + 2);
            }
            let tail = after.windows(4).position(|window| window == b"\r\n\r\n")?;
            return Some(consumed + data_start + tail + 4);
        }
        let data_end = data_start.checked_add(size)?;
        let marked = data_end.checked_add(2)?;
        if rest.len() < marked || rest.get(data_end..marked)? != b"\r\n" {
            return None;
        }
        consumed = consumed.checked_add(marked)?;
        rest = rest.get(marked..)?;
    }
}

fn status_code(response: &[u8]) -> u16 {
    let line = response.split(|&byte| byte == b'\n').next().unwrap_or(b"");
    let text = String::from_utf8_lossy(line);
    text.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}
