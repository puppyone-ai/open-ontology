//! Minimal PostgreSQL simple-query client. Trust auth, text rows, no C library.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Outcome {
    pub rows: Vec<Vec<Option<String>>>,
    pub sqlstate: Option<String>,
    pub message: String,
    pub detail: String,
    pub status: u8,
}

impl Outcome {
    fn conn(message: String) -> Self {
        Self {
            rows: Vec::new(),
            sqlstate: Some("CONN".to_string()),
            message,
            detail: String::new(),
            status: b'E',
        }
    }

    pub fn ok(&self) -> bool {
        self.sqlstate.is_none()
    }

    pub fn state(&self) -> String {
        self.sqlstate.clone().unwrap_or_default()
    }
}

pub struct Pg {
    stream: TcpStream,
}

impl Pg {
    pub fn connect(host: &str, port: u16, user: &str, database: &str) -> Result<Self, String> {
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or_else(|| format!("resolve {host}:{port}"))?;
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
        stream.set_nodelay(true).ok();
        stream
            .set_read_timeout(Some(Duration::from_secs(90)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .ok();
        let mut pg = Pg { stream };
        pg.startup(user, database)?;
        Ok(pg)
    }

    pub fn exec(&mut self, sql: &str) -> Outcome {
        let mut payload = sql.as_bytes().to_vec();
        payload.push(0);
        let mut msg = Vec::with_capacity(payload.len() + 5);
        msg.push(b'Q');
        msg.extend_from_slice(&(payload.len() as u32 + 4).to_be_bytes());
        msg.extend_from_slice(&payload);
        if let Err(e) = self.stream.write_all(&msg) {
            return Outcome::conn(e.to_string());
        }
        let mut rows = Vec::new();
        let mut sqlstate = None;
        let mut message = String::new();
        let mut detail = String::new();
        loop {
            let (tag, body) = match self.read_msg() {
                Ok(v) => v,
                Err(e) => return Outcome::conn(e),
            };
            match tag {
                b'D' => rows.push(parse_row(&body)),
                b'E' => {
                    let (code, msg, all) = parse_err(&body);
                    if sqlstate.is_none() {
                        sqlstate = code;
                        message = msg;
                    }
                    if !detail.is_empty() {
                        detail.push('\n');
                    }
                    detail.push_str(&all);
                }
                b'Z' => {
                    let status = body.first().copied().unwrap_or(b'?');
                    return Outcome {
                        rows,
                        sqlstate,
                        message,
                        detail,
                        status,
                    };
                }
                _ => {}
            }
        }
    }

    fn startup(&mut self, user: &str, database: &str) -> Result<(), String> {
        let mut body = Vec::new();
        body.extend_from_slice(&196608u32.to_be_bytes());
        push_cstr(&mut body, "user");
        push_cstr(&mut body, user);
        push_cstr(&mut body, "database");
        push_cstr(&mut body, database);
        body.push(0);
        let len = (body.len() + 4) as u32;
        self.stream
            .write_all(&len.to_be_bytes())
            .map_err(|e| e.to_string())?;
        self.stream.write_all(&body).map_err(|e| e.to_string())?;
        loop {
            let (tag, body) = self.read_msg()?;
            match tag {
                b'R' => {
                    if body.len() < 4 {
                        return Err("short auth".into());
                    }
                    let code = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                    if code != 0 {
                        return Err(format!("auth code {code}, expected trust"));
                    }
                }
                b'E' => {
                    let (code, msg, _) = parse_err(&body);
                    return Err(format!("startup {} {msg}", code.unwrap_or_default()));
                }
                b'Z' => return Ok(()),
                _ => {}
            }
        }
    }

    fn read_msg(&mut self) -> Result<(u8, Vec<u8>), String> {
        let mut hdr = [0u8; 5];
        self.stream.read_exact(&mut hdr).map_err(|e| e.to_string())?;
        let tag = hdr[0];
        let len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
        if len < 4 {
            return Err("short message".into());
        }
        let mut body = vec![0u8; len - 4];
        if !body.is_empty() {
            self.stream
                .read_exact(&mut body)
                .map_err(|e| e.to_string())?;
        }
        Ok((tag, body))
    }
}

fn push_cstr(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
}

fn parse_row(body: &[u8]) -> Vec<Option<String>> {
    if body.len() < 2 {
        return Vec::new();
    }
    let n = i16::from_be_bytes([body[0], body[1]]) as usize;
    let mut i = 2;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        if i + 4 > body.len() {
            break;
        }
        let flen = i32::from_be_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]);
        i += 4;
        if flen < 0 {
            out.push(None);
        } else {
            let end = i + flen as usize;
            if end > body.len() {
                break;
            }
            out.push(Some(String::from_utf8_lossy(&body[i..end]).into_owned()));
            i = end;
        }
    }
    out
}

fn parse_err(body: &[u8]) -> (Option<String>, String, String) {
    let mut i = 0;
    let mut code = None;
    let mut msg = String::new();
    let mut all = String::new();
    while i < body.len() {
        let typ = body[i];
        if typ == 0 {
            break;
        }
        i += 1;
        let start = i;
        while i < body.len() && body[i] != 0 {
            i += 1;
        }
        let val = String::from_utf8_lossy(&body[start..i]).into_owned();
        if i < body.len() {
            i += 1;
        }
        if !all.is_empty() {
            all.push(' ');
        }
        all.push(typ as char);
        all.push('=');
        all.push_str(&val);
        if typ == b'C' {
            code = Some(val);
        } else if typ == b'M' {
            msg = val;
        }
    }
    (code, msg, all)
}

pub fn cell(row: &[Option<String>], i: usize) -> String {
    row.get(i).and_then(|v| v.clone()).unwrap_or_default()
}

pub fn q(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push('\'');
        }
        out.push(c);
    }
    out.push('\'');
    out
}
