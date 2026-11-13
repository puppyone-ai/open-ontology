//! Screen 1 live checks that need no credentials: DNS, TCP and the TLS
//! certificate the server presents, plus MySQL's pre-auth version. Blocking
//! std I/O with short timeouts; the runner calls these off the UI thread.
//!
//! The certificate fetch speaks just enough TLS 1.2 to read the plaintext
//! `Certificate` message, then hangs up: no key exchange, no trust decision
//! and nothing sent but the hello. Trusting what it shows is the owner's
//! choice on screen and is offered with one key only for loopback and
//! private-network servers; the saved CA is then verified with the
//! hostname on every real connection, as before.
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use super::source::Engine;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const READ_TIMEOUT: Duration = Duration::from_secs(4);

/// Resolve a host to its addresses (port only for the lookup API).
pub fn resolve(host: &str, port: u16) -> Result<Vec<IpAddr>, &'static str> {
    if host.is_empty() {
        return Err("HOST_EMPTY");
    }
    let mut ips: Vec<IpAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| "HOST_UNRESOLVED")?
        .map(|a| a.ip())
        .collect();
    ips.dedup();
    if ips.is_empty() {
        Err("HOST_UNRESOLVED")
    } else {
        Ok(ips)
    }
}

/// Open a TCP connection and report how long it took.
pub fn port_open(host: &str, port: u16) -> Result<Duration, &'static str> {
    let started = Instant::now();
    connect(host, port).map(|_| started.elapsed())
}

fn connect(host: &str, port: u16) -> Result<TcpStream, &'static str> {
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| "HOST_UNRESOLVED")?
        .collect();
    let mut last = "PORT_CLOSED";
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(READ_TIMEOUT));
                let _ = s.set_write_timeout(Some(READ_TIMEOUT));
                return Ok(s);
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => last = "PORT_TIMEOUT",
            Err(_) => last = "PORT_CLOSED",
        }
    }
    Err(last)
}

/// One certificate as the server presented it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertInfo {
    /// Subject CN (or O when there is no CN).
    pub subject: String,
    pub issuer: String,
    /// `YYYY-MM-DD`.
    pub not_after: String,
    /// DNS names and IP addresses from subjectAltName.
    pub names: Vec<String>,
    pub ca: bool,
    pub self_signed: bool,
    /// Hex SHA-256 of the DER.
    pub sha256: String,
    pub der: Vec<u8>,
}

impl CertInfo {
    /// Does this certificate name `host` (exact or one-label wildcard)?
    pub fn covers(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.names
            .iter()
            .chain(std::iter::once(&self.subject))
            .any(|n| {
                let n = n.to_ascii_lowercase();
                n == host
                    || n.strip_prefix("*.").is_some_and(|tail| {
                        host.split_once('.').is_some_and(|(_, rest)| rest == tail)
                    })
            })
    }

    pub fn pem(&self) -> String {
        let b64 = base64(&self.der);
        let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            out.push('\n');
        }
        out.push_str("-----END CERTIFICATE-----\n");
        out
    }
}

/// What the TLS probe saw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TlsReport {
    /// Leaf first, as sent.
    pub chain: Vec<CertInfo>,
    /// MySQL announces its version before authentication.
    pub server_version: Option<String>,
}

impl TlsReport {
    /// The certificate that can serve as a trust anchor: the top of a sent
    /// chain, or a self-signed leaf that is not itself a CA. `None` when
    /// the server sent only a leaf issued by a CA it did not include (a CA
    /// file is needed) or a CA certificate as its own (TLS clients refuse
    /// a CA used as an end entity).
    pub fn anchor(&self) -> Option<&CertInfo> {
        let top = self.chain.last()?;
        match self.chain.len() {
            1 => (top.self_signed && !top.ca).then_some(top),
            _ => Some(top),
        }
    }

    /// The server presents a CA certificate as its own: unusable.
    pub fn ca_as_leaf(&self) -> bool {
        self.chain.first().is_some_and(|c| c.ca)
    }
}

/// Fetch the server's certificate chain over the engine's TLS upgrade.
pub fn tls_chain(engine: Engine, host: &str, port: u16) -> Result<TlsReport, &'static str> {
    let mut s = connect(host, port)?;
    let mut report = TlsReport::default();
    match engine {
        Engine::Postgres => {
            s.write_all(&[0, 0, 0, 8, 0x04, 0xd2, 0x16, 0x2f])
                .map_err(|_| "TLS_IO")?;
            let mut answer = [0u8; 1];
            s.read_exact(&mut answer).map_err(|_| "TLS_IO")?;
            if answer[0] != b'S' {
                return Err("TLS_NOT_OFFERED");
            }
        }
        Engine::Mysql => {
            let (version, ssl) = mysql_greeting(&mut s)?;
            report.server_version = Some(version);
            if !ssl {
                return Ok(report);
            }
            // SSL request: PROTOCOL_41 | SSL | SECURE_CONNECTION | PLUGIN_AUTH.
            let caps: u32 = 0x0000_0200 | 0x0000_0800 | 0x0000_8000 | 0x0008_0000;
            let mut packet = vec![32, 0, 0, 1];
            packet.extend_from_slice(&caps.to_le_bytes());
            packet.extend_from_slice(&16_777_216u32.to_le_bytes());
            packet.push(0xff);
            packet.extend_from_slice(&[0; 23]);
            s.write_all(&packet).map_err(|_| "TLS_IO")?;
        }
        Engine::Oracle => return Err("TLS_ENGINE"),
    }
    let sni = host.parse::<IpAddr>().is_err().then_some(host);
    s.write_all(&client_hello(sni)).map_err(|_| "TLS_IO")?;
    report.chain = read_certificates(&mut s)?
        .iter()
        .filter_map(|der| parse_cert(der))
        .collect();
    if report.chain.is_empty() {
        return Err("TLS_CERTIFICATE");
    }
    Ok(report)
}

/// MySQL's initial handshake (protocol 10): server version and whether it
/// offers TLS. Reads nothing secret and sends nothing.
pub fn mysql_greeting(s: &mut TcpStream) -> Result<(String, bool), &'static str> {
    let mut head = [0u8; 4];
    s.read_exact(&mut head).map_err(|_| "MYSQL_PROTOCOL")?;
    let len = usize::from(head[0]) | usize::from(head[1]) << 8 | usize::from(head[2]) << 16;
    if !(8..=4096).contains(&len) {
        return Err("MYSQL_PROTOCOL");
    }
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).map_err(|_| "MYSQL_PROTOCOL")?;
    if body[0] == 0xff {
        return Err("MYSQL_REFUSED");
    }
    if body[0] != 10 {
        return Err("MYSQL_PROTOCOL");
    }
    let end = body[1..]
        .iter()
        .position(|&b| b == 0)
        .ok_or("MYSQL_PROTOCOL")?;
    let version = String::from_utf8_lossy(&body[1..1 + end]).into_owned();
    // version NUL, connection id 4, auth data 8, filler 1, capabilities 2.
    let caps_at = 1 + end + 1 + 4 + 8 + 1;
    let caps = body
        .get(caps_at..caps_at + 2)
        .map_or(0, |c| u16::from_le_bytes([c[0], c[1]]));
    Ok((version, caps & 0x0800 != 0))
}

/// A TLS 1.2 ClientHello with the suites every supported server accepts.
fn client_hello(sni: Option<&str>) -> Vec<u8> {
    let mut body = vec![0x03, 0x03];
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    for i in 0..32u32 {
        body.push((super::fx::hash(i, (seed as u32) ^ (seed >> 32) as u32, 77) & 0xff) as u8);
    }
    body.push(0); // session id
    let suites: [u16; 12] = [
        0xc02f, 0xc030, 0xc02b, 0xc02c, 0xcca8, 0xcca9, 0x009c, 0x009d, 0xc013, 0xc014, 0x002f,
        0x0035,
    ];
    body.extend_from_slice(&((suites.len() * 2) as u16).to_be_bytes());
    for s in suites {
        body.extend_from_slice(&s.to_be_bytes());
    }
    body.extend_from_slice(&[1, 0]); // null compression
    let mut ext = Vec::new();
    let mut push = |kind: u16, data: &[u8]| {
        ext.extend_from_slice(&kind.to_be_bytes());
        ext.extend_from_slice(&(data.len() as u16).to_be_bytes());
        ext.extend_from_slice(data);
    };
    if let Some(name) = sni {
        let n = name.as_bytes();
        let mut d = ((n.len() + 3) as u16).to_be_bytes().to_vec();
        d.push(0);
        d.extend_from_slice(&(n.len() as u16).to_be_bytes());
        d.extend_from_slice(n);
        push(0x0000, &d);
    }
    push(0x000a, &[0, 6, 0x00, 0x1d, 0x00, 0x17, 0x00, 0x18]);
    push(0x000b, &[1, 0]);
    push(
        0x000d,
        &[
            0, 20, 4, 3, 5, 3, 6, 3, 8, 4, 8, 5, 8, 6, 4, 1, 5, 1, 6, 1, 2, 1,
        ],
    );
    push(0x0017, &[]);
    push(0xff01, &[0]);
    body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
    body.extend_from_slice(&ext);
    let mut hs = vec![1];
    hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    hs.extend_from_slice(&body);
    let mut rec = vec![0x16, 0x03, 0x01];
    rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
    rec.extend_from_slice(&hs);
    rec
}

/// Read handshake records until the Certificate message; return its DERs.
fn read_certificates(s: &mut TcpStream) -> Result<Vec<Vec<u8>>, &'static str> {
    let mut hs = Vec::new();
    let deadline = Instant::now() + READ_TIMEOUT;
    while Instant::now() < deadline && hs.len() < 256 * 1024 {
        let mut head = [0u8; 5];
        s.read_exact(&mut head).map_err(|_| "TLS_IO")?;
        let len = usize::from(u16::from_be_bytes([head[3], head[4]]));
        if len > 18_432 {
            return Err("TLS_HANDSHAKE");
        }
        let mut body = vec![0u8; len];
        s.read_exact(&mut body).map_err(|_| "TLS_IO")?;
        match head[0] {
            0x15 => {
                // protocol_version: the server requires TLS 1.3.
                return Err(if body.get(1) == Some(&70) {
                    "TLS_13_ONLY"
                } else {
                    "TLS_HANDSHAKE"
                });
            }
            0x16 => hs.extend_from_slice(&body),
            _ => return Err("TLS_HANDSHAKE"),
        }
        let mut at = 0;
        while at + 4 <= hs.len() {
            let kind = hs[at];
            let n = usize::from(hs[at + 1]) << 16
                | usize::from(hs[at + 2]) << 8
                | usize::from(hs[at + 3]);
            if at + 4 + n > hs.len() {
                break;
            }
            let msg = &hs[at + 4..at + 4 + n];
            match kind {
                2 if msg.get(..2) != Some(&[3, 3]) => return Err("TLS_13_ONLY"),
                11 => return certificate_list(msg),
                14 => return Err("TLS_CERTIFICATE"),
                _ => (),
            }
            at += 4 + n;
        }
    }
    Err("TLS_HANDSHAKE")
}

fn certificate_list(msg: &[u8]) -> Result<Vec<Vec<u8>>, &'static str> {
    let u24 = |b: &[u8]| usize::from(b[0]) << 16 | usize::from(b[1]) << 8 | usize::from(b[2]);
    if msg.len() < 3 {
        return Err("TLS_CERTIFICATE");
    }
    let total = u24(&msg[..3]).min(msg.len() - 3);
    let mut at = 3;
    let mut out = Vec::new();
    while at + 3 <= 3 + total && out.len() < 8 {
        let n = u24(&msg[at..at + 3]);
        if at + 3 + n > msg.len() {
            break;
        }
        out.push(msg[at + 3..at + 3 + n].to_vec());
        at += 3 + n;
    }
    Ok(out)
}

// ------------------------------------------------------------------- DER

/// `(tag, content start, content end)` of the TLV at `at`.
fn tlv(b: &[u8], at: usize) -> Option<(u8, usize, usize)> {
    let tag = *b.get(at)?;
    let first = *b.get(at + 1)?;
    let (len, start) = if first & 0x80 == 0 {
        (usize::from(first), at + 2)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 3 {
            return None;
        }
        let mut len = 0usize;
        for k in 0..n {
            len = len << 8 | usize::from(*b.get(at + 2 + k)?);
        }
        (len, at + 2 + n)
    };
    (start + len <= b.len()).then_some((tag, start, start + len))
}

/// Children of the constructed value spanning `start..end`.
fn children(b: &[u8], start: usize, end: usize) -> Vec<(u8, usize, usize)> {
    let mut out = Vec::new();
    let mut at = start;
    while at < end {
        let Some(t) = tlv(&b[..end], at) else { break };
        out.push(t);
        at = t.2;
    }
    out
}

fn text_value(b: &[u8], (tag, s, e): (u8, usize, usize)) -> String {
    match tag {
        0x1e => b[s..e]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .filter_map(|u| char::from_u32(u32::from(u)))
            .collect(),
        _ => String::from_utf8_lossy(&b[s..e]).into_owned(),
    }
}

/// CN, else O, of an X.501 Name.
fn name_of(b: &[u8], (_, s, e): (u8, usize, usize)) -> String {
    let mut cn = None;
    let mut org = None;
    for set in children(b, s, e) {
        for attr in children(b, set.1, set.2) {
            let parts = children(b, attr.1, attr.2);
            if let [oid, value, ..] = parts.as_slice() {
                match &b[oid.1..oid.2] {
                    [0x55, 0x04, 0x03] => cn = Some(text_value(b, *value)),
                    [0x55, 0x04, 0x0a] => org = Some(text_value(b, *value)),
                    _ => (),
                }
            }
        }
    }
    cn.or(org).unwrap_or_default()
}

fn date(b: &[u8], (tag, s, e): (u8, usize, usize)) -> String {
    let t = String::from_utf8_lossy(&b[s..e]);
    match tag {
        0x17 if t.len() >= 6 => {
            let yy: u32 = t[..2].parse().unwrap_or(0);
            let century = if yy >= 50 { 19 } else { 20 };
            format!("{century}{}-{}-{}", &t[..2], &t[2..4], &t[4..6])
        }
        0x18 if t.len() >= 8 => format!("{}-{}-{}", &t[..4], &t[4..6], &t[6..8]),
        _ => String::new(),
    }
}

pub fn parse_cert(der: &[u8]) -> Option<CertInfo> {
    let cert = tlv(der, 0)?;
    let tbs = *children(der, cert.1, cert.2).first()?;
    let mut fields = children(der, tbs.1, tbs.2);
    if fields.first()?.0 == 0xa0 {
        fields.remove(0);
    }
    // serial, signature, issuer, validity, subject, spki, [extensions]
    let issuer = *fields.get(2)?;
    let validity = *fields.get(3)?;
    let subject = *fields.get(4)?;
    let not_after = children(der, validity.1, validity.2)
        .get(1)
        .map(|t| date(der, *t))
        .unwrap_or_default();
    let mut names = Vec::new();
    let mut ca = false;
    if let Some(ext) = fields.iter().find(|f| f.0 == 0xa3) {
        let list = children(der, ext.1, ext.2);
        for e in list
            .first()
            .map(|l| children(der, l.1, l.2))
            .unwrap_or_default()
        {
            let parts = children(der, e.1, e.2);
            let (Some(oid), Some(value)) = (parts.first(), parts.last()) else {
                continue;
            };
            let inner = tlv(der, value.1);
            match (&der[oid.1..oid.2], inner) {
                ([0x55, 0x1d, 0x11], Some(seq)) => {
                    for n in children(der, seq.1, seq.2) {
                        match (n.0, n.2 - n.1) {
                            (0x82, _) => {
                                names.push(String::from_utf8_lossy(&der[n.1..n.2]).into_owned())
                            }
                            (0x87, 4) => {
                                let o = &der[n.1..n.2];
                                names.push(IpAddr::from([o[0], o[1], o[2], o[3]]).to_string());
                            }
                            (0x87, 16) => {
                                let mut o = [0u8; 16];
                                o.copy_from_slice(&der[n.1..n.2]);
                                names.push(IpAddr::from(o).to_string());
                            }
                            _ => (),
                        }
                    }
                }
                ([0x55, 0x1d, 0x13], Some(seq)) => {
                    ca = children(der, seq.1, seq.2)
                        .first()
                        .is_some_and(|b| b.0 == 0x01 && der.get(b.1) == Some(&0xff));
                }
                _ => (),
            }
        }
    }
    Some(CertInfo {
        subject: name_of(der, subject),
        issuer: name_of(der, issuer),
        not_after,
        names,
        ca,
        self_signed: der[issuer.1..issuer.2] == der[subject.1..subject.2],
        sha256: oo_core::api::payload_sha256(der),
        der: der.to_vec(),
    })
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = u32::from(c[0]) << 16
            | u32::from(*c.get(1).unwrap_or(&0)) << 8
            | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A self-signed CA certificate for `localhost` / 127.0.0.1 (test only).
    pub(crate) const FIXTURE_DER_B64: &str =
        include_str!("../../tests/fixtures/tls-localhost.der.b64");
    const LEAF: &str = include_str!("../../tests/fixtures/tls-leaf.der.b64");
    const CA: &str = include_str!("../../tests/fixtures/tls-ca.der.b64");

    pub(crate) fn unbase64(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = 0u32;
        let mut bits = 0;
        for c in s.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => continue,
            };
            buf = buf << 6 | u32::from(v);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
            }
        }
        out
    }

    #[test]
    fn parses_a_certificate_and_round_trips_pem() {
        let der = unbase64(FIXTURE_DER_B64);
        let cert = parse_cert(&der).unwrap();
        assert_eq!(cert.subject, "localhost");
        assert!(cert.self_signed && cert.ca);
        assert!(cert.covers("localhost") && cert.covers("127.0.0.1"));
        assert!(!cert.covers("db.internal"));
        assert_eq!(cert.not_after.len(), 10);
        let pem = cert.pem();
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        assert_eq!(unbase64(&body), der);
        // A CA presented as the server's own certificate is never offered.
        let report = TlsReport {
            chain: vec![cert],
            server_version: None,
        };
        assert!(report.anchor().is_none() && report.ca_as_leaf());
        let leaf = parse_cert(&unbase64(LEAF)).unwrap();
        let ca = parse_cert(&unbase64(CA)).unwrap();
        assert!(!leaf.ca && !leaf.self_signed && leaf.covers("127.0.0.1"));
        assert_eq!(leaf.issuer, ca.subject);
        let chain = TlsReport {
            chain: vec![leaf.clone(), ca],
            server_version: None,
        };
        assert_eq!(chain.anchor().unwrap().subject, "oo fixture CA");
        let alone = TlsReport {
            chain: vec![leaf],
            server_version: None,
        };
        assert!(alone.anchor().is_none(), "CA not sent: a CA file is needed");
    }

    #[test]
    fn hello_is_a_tls12_record() {
        let hello = client_hello(Some("db.internal"));
        assert_eq!(&hello[..3], &[0x16, 3, 1]);
        assert_eq!(
            usize::from(u16::from_be_bytes([hello[3], hello[4]])),
            hello.len() - 5
        );
        assert_eq!(&hello[9..11], &[3, 3]);
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
