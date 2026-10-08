//! Private collector for `oo.audit.ingest/v1`.
//!
//! Workload identity is the client certificate URI. Headers are not identity.
//! The database role inserts and reads receipts. This crate has no feedback receiver.

#![forbid(unsafe_code)]

mod pg;

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ServerConfig, WebPkiClientVerifier};
use rustls::RootCertStore;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

pub use pg::PgStore;

const PROTOCOL: &str = "oo.audit.ingest/v1";
const MAX_CERT: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;

#[derive(Debug)]
pub enum IngestError {
    Bound,
    Tls,
    Protocol,
    Forbidden,
    Conflict,
    Db,
    Io(io::Error),
}

impl IngestError {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bound => "bound",
            Self::Tls => "tls",
            Self::Protocol => "protocol",
            Self::Forbidden => "forbidden",
            Self::Conflict => "conflict",
            Self::Db => "db",
            Self::Io(_) => "io",
        }
    }
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Debug)]
pub struct IngestConfig {
    pub listen: String,
    pub ca_file: PathBuf,
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    pub registry_file: PathBuf,
    pub pg_host: String,
    pub pg_port: u16,
    pub pg_db: String,
    pub pg_user: String,
    pub pg_password_file: PathBuf,
    pub pg_ca_file: PathBuf,
}

#[derive(Clone, Debug)]
pub struct PreparedEvent {
    pub spool_id: String,
    pub request_id: String,
    pub event_seq: u64,
    pub payload_hash: [u8; 32],
    pub payload: Vec<u8>,
    pub project: String,
    pub kind: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventBody {
    protocol: String,
    spool_id: String,
    request_id: String,
    event_seq: u64,
    payload_hash: String,
    payload_hex: String,
    project: String,
    kind: String,
}

pub fn load_config(path: &Path) -> Result<IngestConfig, IngestError> {
    private_mode(path)?;
    let text = fs::read_to_string(path).map_err(IngestError::Io)?;
    if text.contains("PRIVATE KEY") || text.contains("BEGIN ") {
        return Err(IngestError::Bound);
    }
    let map = pairs(&text)?;
    let port: u16 = required(&map, "pg_port")?.parse().map_err(|_| IngestError::Bound)?;
    let config = IngestConfig {
        listen: required(&map, "listen")?,
        ca_file: PathBuf::from(required(&map, "ca_file")?),
        cert_file: PathBuf::from(required(&map, "cert_file")?),
        key_file: PathBuf::from(required(&map, "key_file")?),
        registry_file: PathBuf::from(required(&map, "registry_file")?),
        pg_host: required(&map, "pg_host")?,
        pg_port: port,
        pg_db: required(&map, "pg_db")?,
        pg_user: required(&map, "pg_user")?,
        pg_password_file: PathBuf::from(required(&map, "pg_password_file")?),
        pg_ca_file: PathBuf::from(required(&map, "pg_ca_file")?),
    };
    private_mode(&config.key_file)?;
    private_mode(&config.pg_password_file)?;
    Ok(config)
}

pub fn certificate_uri(der: &[u8]) -> Result<String, IngestError> {
    if der.is_empty() || der.len() > MAX_CERT {
        return Err(IngestError::Tls);
    }
    let mut outer = der;
    let (tag, body) = read_tlv(&mut outer).map_err(|_| IngestError::Tls)?;
    if tag != 0x30 {
        return Err(IngestError::Tls);
    }
    let mut tbs_input = body;
    let (tbs_tag, tbs) = read_tlv(&mut tbs_input).map_err(|_| IngestError::Tls)?;
    if tbs_tag != 0x30 {
        return Err(IngestError::Tls);
    }
    let mut cursor = tbs;
    let mut uris = Vec::new();
    while !cursor.is_empty() {
        let (tag, value) = read_tlv(&mut cursor).map_err(|_| IngestError::Tls)?;
        if tag == 0xA3 {
            collect_san(value, &mut uris)?;
        }
    }
    if uris.len() != 1 {
        return Err(IngestError::Tls);
    }
    Ok(uris.remove(0))
}

pub fn prepare_event(body: &[u8], identity_project: &str) -> Result<PreparedEvent, IngestError> {
    let parsed: EventBody = serde_json::from_slice(body).map_err(|_| IngestError::Protocol)?;
    if parsed.protocol != PROTOCOL || !kind_ok(&parsed.kind) {
        return Err(IngestError::Protocol);
    }
    if parsed.project != identity_project {
        return Err(IngestError::Forbidden);
    }
    if parsed.event_seq == 0 || parsed.event_seq > 1_000_000 {
        return Err(IngestError::Protocol);
    }
    uuid_ok(&parsed.spool_id)?;
    uuid_ok(&parsed.request_id)?;
    let payload = decode_hex(&parsed.payload_hex)?;
    if payload.is_empty() || payload.len() > 16 * 1024 {
        return Err(IngestError::Protocol);
    }
    let hash = decode_hex(&parsed.payload_hash)?;
    if hash.len() != 32 {
        return Err(IngestError::Protocol);
    }
    let mut payload_hash = [0u8; 32];
    payload_hash.copy_from_slice(&hash);
    if sha256(&payload) != payload_hash {
        return Err(IngestError::Protocol);
    }
    Ok(PreparedEvent {
        spool_id: parsed.spool_id.to_ascii_lowercase(),
        request_id: parsed.request_id.to_ascii_lowercase(),
        event_seq: parsed.event_seq,
        payload_hash,
        payload,
        project: parsed.project,
        kind: parsed.kind,
    })
}

pub async fn serve(config: IngestConfig) -> Result<(), IngestError> {
    let registry = load_registry(&config.registry_file)?;
    let acceptor = TlsAcceptor::from(Arc::new(server_config(&config)?));
    let mut store = PgStore::connect(&config).await?;
    let listener = TcpListener::bind(&config.listen).await.map_err(IngestError::Io)?;
    println!("listening");
    loop {
        let (tcp, _) = listener.accept().await.map_err(IngestError::Io)?;
        if let Ok(mut stream) = acceptor.accept(tcp).await
            && let Err(err) = handle(&mut stream, &mut store, &registry).await
        {
            let _ = respond(&mut stream, status_parts(&err), &error_body(&err)).await;
        }
    }
}

async fn handle(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    store: &mut PgStore,
    registry: &[(String, String)],
) -> Result<(), IngestError> {
    let (_, connection) = stream.get_ref();
    let leaf = connection
        .peer_certificates()
        .and_then(|chain| chain.first())
        .ok_or(IngestError::Forbidden)?;
    let uri = certificate_uri(leaf.as_ref())?;
    let identity = registry
        .iter()
        .find(|(item, _)| item == &uri)
        .map(|(_, project)| project.clone())
        .ok_or(IngestError::Forbidden)?;
    let (header, body) = read_http(stream).await?;
    if !header.starts_with("POST /v1/audit-events ") {
        return respond(stream, (405, "Method Not Allowed"), &error_body(&IngestError::Protocol)).await;
    }
    let event = prepare_event(&body, &identity)?;
    let receipt = store.insert_event(&event).await?;
    let ack = serde_json::to_vec(&serde_json::json!({
        "protocol": PROTOCOL,
        "spool_id": event.spool_id,
        "request_id": event.request_id,
        "event_seq": event.event_seq,
        "payload_hash": hex_encode(&event.payload_hash),
        "receipt_id": receipt,
    }))
    .map_err(|_| IngestError::Protocol)?;
    respond(stream, (200, "OK"), &ack).await
}

fn status_parts(err: &IngestError) -> (u16, &'static str) {
    match err {
        IngestError::Forbidden => (403, "Forbidden"),
        IngestError::Conflict => (409, "Conflict"),
        IngestError::Protocol | IngestError::Bound => (400, "Bad Request"),
        _ => (500, "Unavailable"),
    }
}

fn error_body(err: &IngestError) -> Vec<u8> {
    format!(r#"{{"protocol":"{PROTOCOL}","error":"{}"}}"#, err.name()).into_bytes()
}

async fn read_http(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
) -> Result<(String, Vec<u8>), IngestError> {
    let mut raw = Vec::new();
    let mut tmp = [0u8; 1024];
    let split = loop {
        if raw.len() > 8192 {
            return Err(IngestError::Protocol);
        }
        let read = stream.read(&mut tmp).await.map_err(IngestError::Io)?;
        if read == 0 {
            return Err(IngestError::Protocol);
        }
        raw.extend_from_slice(&tmp[..read]);
        if let Some(pos) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
    };
    let header = std::str::from_utf8(&raw[..split]).map_err(|_| IngestError::Protocol)?.to_string();
    let length = content_length(&header)?;
    if length > MAX_BODY {
        return Err(IngestError::Protocol);
    }
    let mut body = raw[split + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut tmp).await.map_err(IngestError::Io)?;
        if read == 0 {
            return Err(IngestError::Protocol);
        }
        body.extend_from_slice(&tmp[..read]);
        if body.len() > MAX_BODY {
            return Err(IngestError::Protocol);
        }
    }
    body.truncate(length);
    Ok((header, body))
}

fn content_length(header: &str) -> Result<usize, IngestError> {
    let mut found = None;
    for line in header.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            if found.is_some() {
                return Err(IngestError::Protocol);
            }
            found = Some(value.trim().parse::<usize>().map_err(|_| IngestError::Protocol)?);
        }
    }
    found.ok_or(IngestError::Protocol)
}

async fn respond(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    status: (u16, &str),
    body: &[u8],
) -> Result<(), IngestError> {
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status.0, status.1, body.len()
    );
    stream.write_all(header.as_bytes()).await.map_err(IngestError::Io)?;
    stream.write_all(body).await.map_err(IngestError::Io)?;
    let _ = stream.shutdown().await;
    Ok(())
}

fn server_config(config: &IngestConfig) -> Result<ServerConfig, IngestError> {
    let mut roots = RootCertStore::empty();
    let ca = fs::read(&config.ca_file).map_err(IngestError::Io)?;
    for item in CertificateDer::pem_slice_iter(&ca) {
        roots.add(item.map_err(|_| IngestError::Tls)?).map_err(|_| IngestError::Tls)?;
    }
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|_| IngestError::Tls)?;
    let certs = load_certs(&config.cert_file)?;
    let key = PrivateKeyDer::from_pem_file(&config.key_file).map_err(|_| IngestError::Tls)?;
    ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| IngestError::Tls)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .map_err(|_| IngestError::Tls)
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, IngestError> {
    let bytes = fs::read(path).map_err(IngestError::Io)?;
    let mut chain = Vec::new();
    for item in CertificateDer::pem_slice_iter(&bytes) {
        chain.push(item.map_err(|_| IngestError::Tls)?);
    }
    if chain.is_empty() {
        return Err(IngestError::Tls);
    }
    Ok(chain)
}

fn load_registry(path: &Path) -> Result<Vec<(String, String)>, IngestError> {
    let text = fs::read_to_string(path).map_err(IngestError::Io)?;
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((uri, project)) = line.split_once(' ') else {
            return Err(IngestError::Bound);
        };
        if uri.is_empty() || !project_ok(project) || rows.iter().any(|(seen, _)| seen == uri) {
            return Err(IngestError::Bound);
        }
        rows.push((uri.to_string(), project.to_string()));
    }
    if rows.is_empty() {
        return Err(IngestError::Bound);
    }
    Ok(rows)
}

fn collect_san(explicit_body: &[u8], uris: &mut Vec<String>) -> Result<(), IngestError> {
    let mut input = explicit_body;
    let (tag, seq) = read_tlv(&mut input).map_err(|_| IngestError::Tls)?;
    if tag != 0x30 || !input.is_empty() {
        return Err(IngestError::Tls);
    }
    let mut extensions = seq;
    while !extensions.is_empty() {
        let (tag, extension) = read_tlv(&mut extensions).map_err(|_| IngestError::Tls)?;
        if tag != 0x30 {
            return Err(IngestError::Tls);
        }
        parse_extension(extension, uris)?;
    }
    Ok(())
}

fn parse_extension(extension: &[u8], uris: &mut Vec<String>) -> Result<(), IngestError> {
    let mut input = extension;
    let (tag, oid) = read_tlv(&mut input).map_err(|_| IngestError::Tls)?;
    if tag != 0x06 {
        return Err(IngestError::Tls);
    }
    if !input.is_empty() && input[0] == 0x01 {
        let _ = read_tlv(&mut input).map_err(|_| IngestError::Tls)?;
    }
    let (tag, octet) = read_tlv(&mut input).map_err(|_| IngestError::Tls)?;
    if tag != 0x04 || !input.is_empty() {
        return Err(IngestError::Tls);
    }
    if oid == [0x55, 0x1d, 0x11] {
        push_uris(octet, uris)?;
    }
    Ok(())
}

fn push_uris(octet: &[u8], uris: &mut Vec<String>) -> Result<(), IngestError> {
    let mut input = octet;
    let (tag, seq) = read_tlv(&mut input).map_err(|_| IngestError::Tls)?;
    if tag != 0x30 || !input.is_empty() {
        return Err(IngestError::Tls);
    }
    let mut names = seq;
    while !names.is_empty() {
        let (tag, value) = read_tlv(&mut names).map_err(|_| IngestError::Tls)?;
        if tag == 0x86 {
            let text = std::str::from_utf8(value).map_err(|_| IngestError::Tls)?;
            if text.is_empty() || text.len() > 256 || text.bytes().any(|byte| byte.is_ascii_whitespace()) {
                return Err(IngestError::Tls);
            }
            uris.push(text.to_string());
        }
    }
    Ok(())
}

fn read_tlv<'a>(input: &mut &'a [u8]) -> Result<(u8, &'a [u8]), ()> {
    if input.is_empty() {
        return Err(());
    }
    let tag = input[0];
    *input = &input[1..];
    let length = read_len(input)?;
    if input.len() < length {
        return Err(());
    }
    let (body, rest) = input.split_at(length);
    *input = rest;
    Ok((tag, body))
}

fn read_len(input: &mut &[u8]) -> Result<usize, ()> {
    if input.is_empty() {
        return Err(());
    }
    let first = input[0];
    *input = &input[1..];
    if first & 0x80 == 0 {
        return Ok(first as usize);
    }
    let count = (first & 0x7f) as usize;
    if count == 0 || count > 2 || input.len() < count {
        return Err(());
    }
    let mut value = 0usize;
    for byte in &input[..count] {
        value = (value << 8) | *byte as usize;
    }
    *input = &input[count..];
    if value > MAX_CERT {
        return Err(());
    }
    Ok(value)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[(byte >> 4) as usize]));
        out.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    out
}

fn decode_hex(text: &str) -> Result<Vec<u8>, IngestError> {
    if !text.len().is_multiple_of(2) || text.len() > 32768 {
        return Err(IngestError::Protocol);
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let hi = hex_val(bytes[index])?;
        let lo = hex_val(bytes[index + 1])?;
        out.push((hi << 4) | lo);
        index += 2;
    }
    Ok(out)
}

fn hex_val(byte: u8) -> Result<u8, IngestError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(IngestError::Protocol),
    }
}

fn uuid_ok(text: &str) -> Result<(), IngestError> {
    let stripped: String = text.chars().filter(|char| *char != '-').collect();
    if stripped.len() != 32 || !stripped.bytes().all(|byte| hex_val(byte).is_ok()) {
        return Err(IngestError::Protocol);
    }
    let raw = decode_hex(&stripped)?;
    if raw.len() == 16 && raw[6] >> 4 == 4 && (raw[8] & 0xc0) == 0x80 {
        Ok(())
    } else {
        Err(IngestError::Protocol)
    }
}

fn kind_ok(kind: &str) -> bool {
    matches!(kind, "admission" | "completion" | "refusal")
}

fn project_ok(project: &str) -> bool {
    let bytes = project.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-')
}

fn pairs(text: &str) -> Result<BTreeMap<String, String>, IngestError> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(IngestError::Bound);
        };
        if key.is_empty() || value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(IngestError::Bound);
        }
        if map.insert(key.to_string(), value.to_string()).is_some() {
            return Err(IngestError::Bound);
        }
    }
    Ok(map)
}

fn required(map: &BTreeMap<String, String>, key: &str) -> Result<String, IngestError> {
    map.get(key).cloned().ok_or(IngestError::Bound)
}

fn private_mode(path: &Path) -> Result<(), IngestError> {
    let mode = fs::metadata(path).map_err(IngestError::Io)?.permissions().mode() & 0o777;
    if mode == 0o600 || mode == 0o400 {
        Ok(())
    } else {
        Err(IngestError::Bound)
    }
}

use std::os::unix::fs::PermissionsExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_uri_san_is_required() {
        let uri = b"spiffe://oo/workload/gateway-audit-wal";
        let mut san = vec![0x30, (2 + uri.len()) as u8, 0x86, uri.len() as u8];
        san.extend_from_slice(uri);
        let mut uris = Vec::new();
        push_uris(&san, &mut uris).unwrap();
        assert_eq!(uris, vec!["spiffe://oo/workload/gateway-audit-wal".to_string()]);
        let mut two = vec![0x30, 0, 0x86, uri.len() as u8];
        two.extend_from_slice(uri);
        two.push(0x86);
        two.push(uri.len() as u8);
        two.extend_from_slice(uri);
        two[1] = (two.len() - 2) as u8;
        let mut both = Vec::new();
        push_uris(&two, &mut both).unwrap();
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn event_requires_matching_hash_and_project() {
        let payload = b"abc";
        let hash = hex_encode(&sha256(payload));
        let body = format!(
            r#"{{"protocol":"oo.audit.ingest/v1","spool_id":"11111111-1111-4111-8111-111111111111","request_id":"22222222-2222-4222-8222-222222222222","event_seq":1,"payload_hash":"{hash}","payload_hex":"616263","project":"shop","kind":"refusal"}}"#
        );
        assert!(prepare_event(body.as_bytes(), "shop").is_ok());
        assert!(matches!(prepare_event(body.as_bytes(), "other"), Err(IngestError::Forbidden)));
        assert!(prepare_event(br#"{"ack":true}"#, "shop").is_err());
        let schema = include_str!("../schema/event.v1.json");
        assert!(schema.contains("\"additionalProperties\": false"));
        assert!(schema.contains("\"payload_hash\""));
        let ack = include_str!("../schema/ack.v1.json");
        assert!(!ack.contains("\"ack\""));
        let sql = include_str!("../migrations/oo_audit_ingest_v1.sql");
        assert!(!sql.to_ascii_lowercase().contains("feedback"));
        assert!(sql.contains("GRANT INSERT, SELECT"));
        assert!(!sql.contains("GRANT UPDATE"));
        assert!(!sql.contains("GRANT ALL"));
    }

    #[test]
    fn headers_do_not_select_a_project() {
        let header = "POST /v1/audit-events HTTP/1.1\r\nx-project: other\r\nContent-Length: 2\r\n";
        assert_eq!(content_length(header).unwrap(), 2);
        assert!(header.to_ascii_lowercase().contains("x-project"));
    }
}
