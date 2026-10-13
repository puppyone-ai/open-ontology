//! Development egress proxy. It holds the gateway session token and the only
//! client certificate. Pi and `oo` present `public-proxy-marker` on plaintext
//! loopback. This process injects the session token over mTLS.
//!
//! The TLS listener calls `query-pg`. It does not parse or execute SQL.
//! The certificate files are a local development PKI, not a production trust store.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::ClientConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::server::{ServerConfig, WebPkiClientVerifier};
use rustls::{Connection, RootCertStore};

use crate::forward::PUBLIC_MARKER;
use crate::gateway::{self, CapturedQuery};

pub struct DevPki {
    pub dir: PathBuf,
}

pub fn mint_pki(dir: &Path) -> Result<DevPki, &'static str> {
    fs::create_dir_all(dir).map_err(|_| "pki")?;
    let ca_key = dir.join("ca.key");
    let ca_crt = dir.join("ca.crt");
    let server_key = dir.join("server.key");
    let server_csr = dir.join("server.csr");
    let server_crt = dir.join("server.crt");
    let client_key = dir.join("client.key");
    let client_csr = dir.join("client.csr");
    let client_crt = dir.join("client.crt");
    let ext = dir.join("server.ext");
    fs::write(&ext, b"subjectAltName=IP:127.0.0.1\n").map_err(|_| "pki")?;
    openssl(&["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", path(&ca_key)?, "-out", path(&ca_crt)?, "-subj", "/CN=oo-a405-dev-ca", "-days", "1"])?;
    openssl(&["req", "-newkey", "rsa:2048", "-nodes", "-keyout", path(&server_key)?, "-out", path(&server_csr)?, "-subj", "/CN=127.0.0.1"])?;
    openssl(&["x509", "-req", "-in", path(&server_csr)?, "-CA", path(&ca_crt)?, "-CAkey", path(&ca_key)?, "-CAcreateserial", "-out", path(&server_crt)?, "-days", "1", "-extfile", path(&ext)?])?;
    openssl(&["req", "-newkey", "rsa:2048", "-nodes", "-keyout", path(&client_key)?, "-out", path(&client_csr)?, "-subj", "/CN=oo-a405-dev-proxy"])?;
    openssl(&["x509", "-req", "-in", path(&client_csr)?, "-CA", path(&ca_crt)?, "-CAkey", path(&ca_key)?, "-CAcreateserial", "-out", path(&client_crt)?, "-days", "1"])?;
    for key in [&ca_key, &server_key, &client_key] {
        fs::set_permissions(key, fs::Permissions::from_mode(0o600)).map_err(|_| "pki")?;
    }
    let _ = fs::remove_file(server_csr);
    let _ = fs::remove_file(client_csr);
    let _ = fs::remove_file(ext);
    Ok(DevPki { dir: dir.to_path_buf() })
}

fn path(value: &Path) -> Result<&str, &'static str> {
    value.to_str().ok_or("pki")
}

fn openssl(args: &[&str]) -> Result<(), &'static str> {
    let output = Command::new("openssl").args(args).env_clear().env("PATH", "/usr/bin:/bin").output().map_err(|_| "pki")?;
    if output.status.success() { Ok(()) } else { Err("pki") }
}

pub struct GatewayServe {
    pub listener: TcpListener,
    pub config: Arc<ServerConfig>,
    pub binary: PathBuf,
    pub policy: serde_json::Value,
    pub session: String,
    pub request_dir: PathBuf,
    pub captured: Arc<Mutex<Vec<CapturedQuery>>>,
    pub stop: Arc<AtomicBool>,
}

pub fn serve_gateway(job: GatewayServe) {
    let GatewayServe { listener, config, binary, policy, session, request_dir, captured, stop } = job;
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((tcp, _)) => {
                let _ = tcp.set_read_timeout(Some(Duration::from_secs(30)));
                if let Ok(mut door) = server_door(Arc::clone(&config), tcp) {
                    let _ = gateway::handle(&mut door, &binary, &policy, &session, &request_dir, &captured);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

pub fn serve_proxy(
    listener: TcpListener,
    gateway: SocketAddr,
    config: Arc<ClientConfig>,
    session: String,
    record: PathBuf,
    stop: Arc<AtomicBool>,
) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut tcp, _)) => {
                let _ = tcp.set_read_timeout(Some(Duration::from_secs(30)));
                let _ = proxy_one(&mut tcp, gateway, &config, &session, &record);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn proxy_one(stream: &mut TcpStream, gateway: SocketAddr, config: &Arc<ClientConfig>, session: &str, record: &Path) -> Result<(), ()> {
    let buffer = gateway::read_message(stream, 96 * 1024)?;
    let end = buffer.windows(4).position(|window| window == b"\r\n\r\n").ok_or(())?;
    let head = std::str::from_utf8(&buffer[..end]).map_err(|_| ())?;
    if !head.starts_with("POST /v1/query ") {
        return gateway::write_raw(stream, 404, b"{\"error\":{\"code\":\"SCHEMA_INVALID\",\"message\":\"the gateway refused the statement\",\"hint\":\"path\",\"retryable\":false}}");
    }
    if presents(head, session) || !presents(head, PUBLIC_MARKER) {
        note(record, "refused", false);
        return gateway::write_raw(stream, 401, b"{\"error\":{\"code\":\"UNAUTHENTICATED\",\"message\":\"the gateway refused the statement\",\"hint\":\"proxy marker\",\"retryable\":false}}");
    }
    let mut rewritten = String::new();
    for line in head.split("\r\n") {
        if line.to_ascii_lowercase().starts_with("authorization:") {
            rewritten.push_str("Authorization: Bearer ");
            rewritten.push_str(session);
            rewritten.push_str("\r\n");
        } else if line.to_ascii_lowercase().starts_with("host:") {
            rewritten.push_str(&format!("Host: 127.0.0.1:{}\r\n", gateway.port()));
        } else if !line.is_empty() {
            rewritten.push_str(line);
            rewritten.push_str("\r\n");
        }
    }
    rewritten.push_str("\r\n");
    let body = &buffer[end + 4..];
    let upstream = TcpStream::connect(gateway).map_err(|_| ())?;
    upstream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut tls = client_door(Arc::clone(config), upstream, gateway)?;
    tls.write_all(rewritten.as_bytes()).map_err(|_| ())?;
    tls.write_all(body).map_err(|_| ())?;
    let response = gateway::read_message(&mut tls, 262_144).unwrap_or_default();
    note(record, "marker", true);
    stream.write_all(&response).map_err(|_| ())?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn presents(head: &str, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    head.lines().any(|line| {
        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        name.eq_ignore_ascii_case("authorization") && value.trim() == expected
    })
}

fn note(path: &Path, presented: &str, injected: bool) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let line = format!("{{\"presented\":{presented:?},\"injected\":{injected},\"mtls\":{injected},\"development_local\":true}}\n");
    let mut options = OpenOptions::new();
    options.create(true).append(true).mode(0o600);
    if let Ok(mut file) = options.open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn server_door(config: Arc<ServerConfig>, tcp: TcpStream) -> Result<Door, ()> {
    let conn = rustls::ServerConnection::new(config).map_err(|_| ())?;
    let mut door = Door { conn: Connection::Server(conn), tcp };
    door.handshake()?;
    Ok(door)
}

fn client_door(config: Arc<ClientConfig>, tcp: TcpStream, gateway: SocketAddr) -> Result<Door, ()> {
    let name: ServerName<'static> = gateway.ip().into();
    let conn = rustls::ClientConnection::new(config, name).map_err(|_| ())?;
    let mut door = Door { conn: Connection::Client(conn), tcp };
    door.handshake()?;
    Ok(door)
}

struct Door {
    conn: Connection,
    tcp: TcpStream,
}

impl Door {
    fn handshake(&mut self) -> Result<(), ()> {
        while self.conn.is_handshaking() {
            self.conn.complete_io(&mut self.tcp).map_err(|_| ())?;
        }
        Ok(())
    }

    fn drive(&mut self) -> io::Result<()> {
        let (read, written) = self.conn.complete_io(&mut self.tcp)?;
        if read == 0 && written == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "tls made no progress"));
        }
        Ok(())
    }
}

impl Read for Door {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let pending = {
                let mut reader = self.conn.reader();
                reader.read(buf)
            };
            match pending {
                Ok(0) if self.conn.wants_read() => self.drive()?,
                Ok(n) => return Ok(n),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => self.drive()?,
                Err(error) => return Err(error),
            }
        }
    }
}

impl Write for Door {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = {
            let mut writer = self.conn.writer();
            writer.write(buf)?
        };
        self.drive()?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.drive()?;
        self.tcp.flush()
    }
}

pub fn server_config(pki: &DevPki) -> Result<Arc<ServerConfig>, &'static str> {
    let certs = pem_certs(&fs::read(pki.dir.join("server.crt")).map_err(|_| "pki")?)?;
    let key = PrivateKeyDer::from_pem_slice(&fs::read(pki.dir.join("server.key")).map_err(|_| "pki")?).map_err(|_| "pki")?;
    let ca = single_cert(&fs::read(pki.dir.join("ca.crt")).map_err(|_| "pki")?)?;
    let mut roots = RootCertStore::empty();
    roots.add(ca).map_err(|_| "pki")?;
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots)).build().map_err(|_| "pki")?;
    let config = ServerConfig::builder().with_client_cert_verifier(verifier).with_single_cert(certs, key).map_err(|_| "pki")?;
    Ok(Arc::new(config))
}

pub fn client_config(pki: &DevPki) -> Result<Arc<ClientConfig>, &'static str> {
    let ca = single_cert(&fs::read(pki.dir.join("ca.crt")).map_err(|_| "pki")?)?;
    let mut roots = RootCertStore::empty();
    roots.add(ca).map_err(|_| "pki")?;
    let chain = pem_certs(&fs::read(pki.dir.join("client.crt")).map_err(|_| "pki")?)?;
    let key = PrivateKeyDer::from_pem_slice(&fs::read(pki.dir.join("client.key")).map_err(|_| "pki")?).map_err(|_| "pki")?;
    let config = ClientConfig::builder().with_root_certificates(roots).with_client_auth_cert(chain, key).map_err(|_| "pki")?;
    Ok(Arc::new(config))
}

fn pem_certs(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>, &'static str> {
    CertificateDer::pem_slice_iter(bytes).collect::<Result<Vec<_>, _>>().map_err(|_| "pki")
}

fn single_cert(bytes: &[u8]) -> Result<CertificateDer<'static>, &'static str> {
    let mut certs = pem_certs(bytes)?;
    if certs.len() != 1 {
        return Err("pki");
    }
    Ok(certs.remove(0))
}

#[cfg(test)]
mod tests {
    use super::{client_config, mint_pki, server_config, serve_gateway, serve_proxy, GatewayServe};
    use crate::forward::PUBLIC_MARKER;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn proxy_injects_the_session_over_mtls_and_refuses_a_client_session() {
        let dir = std::env::temp_dir().join(format!("oo-a405-pki-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pki = mint_pki(&dir).expect("pki");
        let server = server_config(&pki).expect("server");
        let client = client_config(&pki).expect("client");
        let gateway = std::net::TcpListener::bind("127.0.0.1:0").expect("gateway");
        let proxy = std::net::TcpListener::bind("127.0.0.1:0").expect("proxy");
        let gateway_addr = gateway.local_addr().expect("addr");
        let proxy_port = proxy.local_addr().expect("addr").port();
        let stop = Arc::new(AtomicBool::new(false));
        let session = "0123456789abcdef0123456789abcdef".to_owned();
        let record = dir.join("record.jsonl");
        let queries = dir.join("queries");
        let gateway_stop = Arc::clone(&stop);
        let proxy_stop = Arc::clone(&stop);
        let gateway_thread = thread::spawn(move || {
            serve_gateway(GatewayServe {
                listener: gateway,
                config: server,
                binary: PathBuf::from("/no/such/query-pg"),
                policy: serde_json::json!({}),
                session,
                request_dir: queries,
                captured: Arc::new(Mutex::new(Vec::new())),
                stop: gateway_stop,
            });
        });
        let session = "0123456789abcdef0123456789abcdef".to_owned();
        let proxy_thread = thread::spawn(move || serve_proxy(proxy, gateway_addr, client, session, record, proxy_stop));
        let body = b"{\"sql\":\"shop.entry\",\"timeout_ms\":1000,\"format\":\"json\"}";
        let denied = post(proxy_port, "Bearer 0123456789abcdef0123456789abcdef", body);
        assert!(denied.contains("UNAUTHENTICATED"), "{denied}");
        let passed = post(proxy_port, &format!("Bearer {PUBLIC_MARKER}"), body);
        assert!(passed.contains("SCHEMA_INVALID"), "{passed}");
        let notes = std::fs::read_to_string(dir.join("record.jsonl")).unwrap_or_default();
        assert!(notes.contains("\"injected\":true"));
        assert!(!notes.contains("0123456789abcdef0123456789abcdef"));
        stop.store(true, Ordering::Relaxed);
        let _ = gateway_thread.join();
        let _ = proxy_thread.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn post(port: u16, authorization: &str, body: &[u8]) -> String {
        let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        client.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");
        let head = format!(
            "POST /v1/query HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: {authorization}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        client.write_all(head.as_bytes()).expect("write");
        client.write_all(body).expect("write");
        let mut response = String::new();
        let _ = client.read_to_string(&mut response);
        response
    }
}

