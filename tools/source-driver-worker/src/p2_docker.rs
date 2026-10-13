//! Docker Engine HTTP over a Unix socket.
//!
//! The Oracle child does not open this socket. The control plane is the only
//! caller. A missing socket or a missing image fails closed.

use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API: &str = "/v1.43";

pub struct Engine {
    socket: PathBuf,
}

pub struct Attach {
    stream: UnixStream,
    raw: Vec<u8>,
}

pub enum Fill {
    Data,
    Timeout,
    Eof,
}

struct Reply {
    status: u16,
    body: Vec<u8>,
}

impl Engine {
    pub fn open(socket: &Path) -> Result<Self, &'static str> {
        let _probe = connect(socket, Duration::from_secs(5))?;
        Ok(Self {
            socket: socket.to_path_buf(),
        })
    }

    pub fn confirmed_image_id(&self, expect: &str) -> Result<serde_json::Value, &'static str> {
        let path = format!("{API}/images/{}/json", encode_ref(expect));
        let reply = self.call("GET", &path, None, Duration::from_secs(30))?;
        if reply.status == 404 {
            return Err("image_missing");
        }
        if reply.status != 200 {
            return Err("image_missing");
        }
        let doc: serde_json::Value = serde_json::from_slice(&reply.body).map_err(|_| "inspect")?;
        let id = doc.get("Id").and_then(|value| value.as_str()).ok_or("inspect")?;
        if id != expect {
            return Err("image_digest");
        }
        Ok(doc)
    }

    pub fn create(&self, name: &str, spec: &serde_json::Value) -> Result<String, &'static str> {
        let bytes = serde_json::to_vec(spec).map_err(|_| "inspect")?;
        let path = format!("{API}/containers/create?name={name}");
        let reply = self.call("POST", &path, Some(&bytes), Duration::from_secs(30))?;
        if reply.status == 404 {
            return Err("image_missing");
        }
        if reply.status != 201 {
            return Err("inspect");
        }
        let doc: serde_json::Value = serde_json::from_slice(&reply.body).map_err(|_| "inspect")?;
        doc.get("Id")
            .and_then(|value| value.as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .ok_or("inspect")
    }

    pub fn start(&self, id: &str) -> Result<(), &'static str> {
        let reply = self.call(
            "POST",
            &format!("{API}/containers/{id}/start"),
            None,
            Duration::from_secs(30),
        )?;
        if matches!(reply.status, 204 | 304) {
            Ok(())
        } else {
            Err("inspect")
        }
    }

    pub fn inspect_container(&self, id: &str) -> Result<serde_json::Value, &'static str> {
        let reply = self.call(
            "GET",
            &format!("{API}/containers/{id}/json"),
            None,
            Duration::from_secs(30),
        )?;
        if reply.status != 200 {
            return Err("inspect");
        }
        serde_json::from_slice(&reply.body).map_err(|_| "inspect")
    }

    pub fn attach(&self, id: &str) -> Result<Attach, &'static str> {
        let path = format!("{API}/containers/{id}/attach?stream=1&stdin=1&stdout=1&stderr=1");
        let mut stream = connect(&self.socket, Duration::from_secs(30))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|_| "io")?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|_| "io")?;
        let header = format!(
            "POST {path} HTTP/1.1\r\nHost: docker\r\nUser-Agent: oo-source-driver-p2\r\nConnection: Upgrade\r\nUpgrade: tcp\r\nContent-Length: 0\r\n\r\n"
        );
        stream.write_all(header.as_bytes()).map_err(|_| "io")?;
        stream.flush().map_err(|_| "io")?;
        let (status, leftover) = read_header(&mut stream)?;
        if status != 101 {
            return Err("io");
        }
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .map_err(|_| "io")?;
        Ok(Attach {
            stream,
            raw: leftover,
        })
    }

    pub fn kill(&self, id: &str) -> Result<(), &'static str> {
        let reply = self.call(
            "POST",
            &format!("{API}/containers/{id}/kill?signal=KILL"),
            None,
            Duration::from_secs(30),
        )?;
        if matches!(reply.status, 204 | 404 | 409) {
            Ok(())
        } else {
            Err("io")
        }
    }

    pub fn wait_code(&self, id: &str) -> Result<i64, &'static str> {
        let reply = self.call(
            "POST",
            &format!("{API}/containers/{id}/wait"),
            None,
            Duration::from_secs(120),
        )?;
        if reply.status != 200 {
            return Err("io");
        }
        let doc: serde_json::Value = serde_json::from_slice(&reply.body).map_err(|_| "io")?;
        doc.get("StatusCode").and_then(|value| value.as_i64()).ok_or("io")
    }

    pub fn remove(&self, id: &str) -> Result<(), &'static str> {
        let reply = self.call(
            "DELETE",
            &format!("{API}/containers/{id}?force=1"),
            None,
            Duration::from_secs(30),
        )?;
        if matches!(reply.status, 204 | 200 | 404) {
            Ok(())
        } else {
            Err("io")
        }
    }

    pub fn running(&self, id: &str) -> Result<bool, &'static str> {
        let doc = self.inspect_container(id)?;
        Ok(doc
            .pointer("/State/Running")
            .and_then(|value| value.as_bool())
            .unwrap_or(false))
    }

    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        timeout: Duration,
    ) -> Result<Reply, &'static str> {
        let mut stream = connect(&self.socket, timeout)?;
        let mut header = format!(
            "{method} {path} HTTP/1.1\r\nHost: docker\r\nUser-Agent: oo-source-driver-p2\r\nAccept: application/json\r\nConnection: close\r\n"
        );
        if let Some(body) = body {
            header.push_str("Content-Type: application/json\r\n");
            header.push_str(&format!("Content-Length: {}\r\n", body.len()));
        } else if method != "GET" && method != "HEAD" {
            header.push_str("Content-Length: 0\r\n");
        }
        header.push_str("\r\n");
        stream.write_all(header.as_bytes()).map_err(|_| "io")?;
        if let Some(body) = body {
            stream.write_all(body).map_err(|_| "io")?;
        }
        stream.flush().map_err(|_| "io")?;
        let (status, headers, mut payload) = read_header_text(&mut stream)?;
        let body = read_body(&mut stream, &headers, &mut payload)?;
        Ok(Reply { status, body })
    }
}

impl Attach {
    /// The engine copies these bytes to the container stdin as they are.
    /// A multiplex header would be delivered to the process. The child stops
    /// at the newline. Closing the write half makes this daemon return read
    /// EOF while the child is still running, so the socket stays open.
    pub fn write_stdin(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.stream.write_all(bytes).map_err(|_| "io")?;
        self.stream.flush().map_err(|_| "io")?;
        Ok(())
    }

    pub fn fill(&mut self, stdout: &mut Vec<u8>, stderr: &mut Vec<u8>) -> Result<Fill, &'static str> {
        let mut buf = [0u8; 8192];
        match self.stream.read(&mut buf) {
            Ok(0) => {
                self.drain(stdout, stderr)?;
                if !self.raw.is_empty() {
                    return Err("protocol");
                }
                Ok(Fill::Eof)
            }
            Ok(n) => {
                self.raw.extend_from_slice(&buf[..n]);
                self.drain(stdout, stderr)?;
                Ok(Fill::Data)
            }
            Err(err) if err.kind() == ErrorKind::Interrupted => Ok(Fill::Timeout),
            Err(err) if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock => {
                Ok(Fill::Timeout)
            }
            Err(_) => Err("io"),
        }
    }

    fn drain(&mut self, stdout: &mut Vec<u8>, stderr: &mut Vec<u8>) -> Result<(), &'static str> {
        loop {
            if self.raw.len() < 8 {
                return Ok(());
            }
            if self.raw[1] != 0 || self.raw[2] != 0 || self.raw[3] != 0 {
                return Err("protocol");
            }
            let kind = self.raw[0];
            let size = u32::from_be_bytes([self.raw[4], self.raw[5], self.raw[6], self.raw[7]]) as usize;
            // One engine frame may carry a whole protocol frame. The protocol
            // cap is 16 MiB, plus the 8-byte tag and length in front of it.
            if size > 16 * 1024 * 1024 + 8 {
                return Err("protocol");
            }
            if self.raw.len() < 8 + size {
                return Ok(());
            }
            let payload = self.raw[8..8 + size].to_vec();
            self.raw.drain(..8 + size);
            match kind {
                1 => {
                    if stdout.len().saturating_add(payload.len()) > 32 * 1024 * 1024 {
                        return Err("budget");
                    }
                    stdout.extend_from_slice(&payload);
                }
                2 => {
                    if stderr.len().saturating_add(payload.len()) > 1024 * 1024 {
                        return Err("budget");
                    }
                    stderr.extend_from_slice(&payload);
                }
                0 => {}
                _ => return Err("protocol"),
            }
        }
    }
}

fn connect(socket: &Path, timeout: Duration) -> Result<UnixStream, &'static str> {
    let stream = UnixStream::connect(socket).map_err(|_| "launcher_missing")?;
    stream.set_read_timeout(Some(timeout)).map_err(|_| "io")?;
    stream.set_write_timeout(Some(timeout)).map_err(|_| "io")?;
    Ok(stream)
}

fn encode_ref(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn read_header(stream: &mut UnixStream) -> Result<(u16, Vec<u8>), &'static str> {
    let (status, _headers, leftover) = read_header_text(stream)?;
    Ok((status, leftover))
}

fn read_header_text(stream: &mut UnixStream) -> Result<(u16, String, Vec<u8>), &'static str> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if buf.len() > 65_536 {
            return Err("io");
        }
        match stream.read(&mut byte) {
            Ok(0) => return Err("io"),
            Ok(_) => buf.push(byte[0]),
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err("io"),
        }
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let split = buf.len() - 4;
    let header = std::str::from_utf8(&buf[..split]).map_err(|_| "io")?;
    let status_line = header.lines().next().ok_or("io")?;
    let code = status_line.split(' ').nth(1).ok_or("io")?;
    let status: u16 = code.parse().map_err(|_| "io")?;
    Ok((status, header.to_string(), Vec::new()))
}

fn read_body(stream: &mut UnixStream, headers: &str, payload: &mut Vec<u8>) -> Result<Vec<u8>, &'static str> {
    if let Some(len) = content_length(headers) {
        if len > 2 * 1024 * 1024 {
            return Err("io");
        }
        while payload.len() < len {
            let mut buf = [0u8; 8192];
            match stream.read(&mut buf) {
                Ok(0) => return Err("io"),
                Ok(n) => payload.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return Err("io"),
            }
        }
        payload.truncate(len);
        return Ok(std::mem::take(payload));
    }
    loop {
        if payload.len() > 2 * 1024 * 1024 {
            return Err("io");
        }
        let mut buf = [0u8; 8192];
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => payload.extend_from_slice(&buf[..n]),
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err("io"),
        }
    }
    if chunked(headers) {
        return decode_chunked(payload);
    }
    Ok(std::mem::take(payload))
}

fn chunked(headers: &str) -> bool {
    headers.split("\r\n").any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("transfer-encoding") && value.to_ascii_lowercase().contains("chunked")
    })
}

/// Docker Engine 29 answers image inspect with `Transfer-Encoding: chunked`
/// and no `Content-Length`. The JSON is the decoded chunks, not the framing.
fn decode_chunked(raw: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut index = 0usize;
    let mut out = Vec::new();
    loop {
        let rest = raw.get(index..).ok_or("io")?;
        let line_rel = rest.windows(2).position(|pair| pair == b"\r\n").ok_or("io")?;
        let line = std::str::from_utf8(&rest[..line_rel]).map_err(|_| "io")?;
        let size_hex = line.split(';').next().unwrap_or(line).trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| "io")?;
        index += line_rel + 2;
        if size == 0 {
            return Ok(out);
        }
        let end = index.checked_add(size).ok_or("io")?;
        let data = raw.get(index..end).ok_or("io")?;
        if out.len().saturating_add(data.len()) > 2 * 1024 * 1024 {
            return Err("io");
        }
        out.extend_from_slice(data);
        let crlf = raw.get(end..end + 2).ok_or("io")?;
        if crlf != b"\r\n" {
            return Err("io");
        }
        index = end + 2;
    }
}

fn content_length(headers: &str) -> Option<usize> {
    for line in headers.split("\r\n") {
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
    use super::decode_chunked;

    #[test]
    fn chunked_image_body_is_json() {
        let raw = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(decode_chunked(raw).unwrap(), b"hello world");
    }

    #[test]
    fn chunked_rejects_a_short_frame() {
        assert_eq!(decode_chunked(b"5\r\nhi\r\n0\r\n\r\n").unwrap_err(), "io");
    }
}
