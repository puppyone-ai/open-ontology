//! rustls client. Checkpoint rows are written only from a verified ack.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tokio_rustls::TlsConnector;

use crate::error::WalError;
use crate::frame::{hex_decode, hex_encode, parse_uuid, sha256};
use crate::store::{DiskWal, SealedFrame};

const PROTOCOL: &str = "oo.audit.ingest/v1";
const BACKOFF_MS: [u64; 3] = [50, 100, 200];

#[derive(Clone, Debug)]
pub struct SendConfig {
    pub spool: PathBuf,
    pub collector_host: String,
    pub collector_port: u16,
    pub server_name: String,
    pub ca_file: PathBuf,
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    pub force_project: Option<String>,
}

pub struct VerifiedAck {
    receipt_id: String,
}

impl VerifiedAck {
    pub fn receipt_id(&self) -> &str {
        &self.receipt_id
    }
}

#[derive(Debug)]
pub enum DeliveryStop {
    Delivered { acked: u32 },
    Down,
    Conflict,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckBody {
    protocol: String,
    spool_id: String,
    request_id: String,
    event_seq: u64,
    payload_hash: String,
    receipt_id: String,
}

pub fn load_send_config(path: &Path) -> Result<SendConfig, WalError> {
    private_mode(path)?;
    let text = fs::read_to_string(path).map_err(WalError::Io)?;
    if text.contains("PRIVATE KEY") || text.contains("BEGIN ") {
        return Err(WalError::Bound);
    }
    let map = pairs(&text)?;
    for key in map.keys() {
        if key.contains("password") || key.contains("secret") || key.contains("token") {
            return Err(WalError::Bound);
        }
    }
    let port: u16 = required(&map, "collector_port")?.parse().map_err(|_| WalError::Bound)?;
    Ok(SendConfig {
        spool: PathBuf::from(required(&map, "spool")?),
        collector_host: required(&map, "collector_host")?,
        collector_port: port,
        server_name: required(&map, "server_name")?,
        ca_file: PathBuf::from(required(&map, "ca_file")?),
        cert_file: PathBuf::from(required(&map, "cert_file")?),
        key_file: PathBuf::from(required(&map, "key_file")?),
        force_project: map.get("force_project").cloned(),
    })
}

pub fn match_ack(frame: &SealedFrame, body: &[u8]) -> Result<VerifiedAck, WalError> {
    let parsed: AckBody = serde_json::from_slice(body).map_err(|_| WalError::AckMismatch)?;
    if parsed.protocol != PROTOCOL {
        return Err(WalError::AckMismatch);
    }
    let spool = parse_uuid(&parsed.spool_id).map_err(|_| WalError::AckMismatch)?;
    let expected_spool = parse_uuid(&frame.spool_id)?;
    if spool != expected_spool {
        return Err(WalError::AckMismatch);
    }
    let request = parse_uuid(&parsed.request_id).map_err(|_| WalError::AckMismatch)?;
    if request != frame.request || parsed.event_seq != frame.event_seq {
        return Err(WalError::AckMismatch);
    }
    let hash = hex_decode(&parsed.payload_hash.to_ascii_lowercase()).map_err(|_| WalError::AckMismatch)?;
    if hash.as_slice() != frame.payload_hash {
        return Err(WalError::AckMismatch);
    }
    let receipt = parse_uuid(&parsed.receipt_id).map_err(|_| WalError::AckMismatch)?;
    if receipt[6] >> 4 != 4 || (receipt[8] & 0xc0) != 0x80 {
        return Err(WalError::AckMismatch);
    }
    Ok(VerifiedAck {
        receipt_id: parsed.receipt_id.to_ascii_lowercase(),
    })
}

pub async fn deliver_sealed(wal: &mut DiskWal, config: &SendConfig) -> Result<DeliveryStop, WalError> {
    let frames = wal.sealed_frames()?;
    if frames.is_empty() {
        return Ok(DeliveryStop::Delivered { acked: 0 });
    }
    let connector = connector(config)?;
    let mut acked = 0u32;
    for frame in frames {
        match post_frame(&connector, config, &frame).await? {
            Post::Ack(ack) => {
                let _ = ack;
                wal.checkpoint_ack(frame.request, frame.event_seq, frame.payload_hash)?;
                acked += 1;
            }
            Post::Conflict => return Ok(DeliveryStop::Conflict),
            Post::Down => return Ok(DeliveryStop::Down),
        }
    }
    Ok(DeliveryStop::Delivered { acked })
}

pub async fn post_event_body(config: &SendConfig, body: &[u8]) -> Result<u16, WalError> {
    let connector = connector(config)?;
    let response = exchange(&connector, config, body).await?;
    Ok(response.status)
}

enum Post {
    Ack(VerifiedAck),
    Conflict,
    Down,
}

async fn post_frame(connector: &TlsConnector, config: &SendConfig, frame: &SealedFrame) -> Result<Post, WalError> {
    let project = config.force_project.clone().unwrap_or_else(|| frame.project.clone());
    let body = serde_json::to_vec(&serde_json::json!({
        "protocol": PROTOCOL,
        "spool_id": frame.spool_id,
        "request_id": frame.request_id,
        "event_seq": frame.event_seq,
        "payload_hash": hex_encode(&frame.payload_hash),
        "payload_hex": frame.payload_hex,
        "project": project,
        "kind": frame.kind,
    }))
    .map_err(|_| WalError::Protocol)?;
    if sha256(&hex_decode(&frame.payload_hex)?) != frame.payload_hash {
        return Err(WalError::Corrupt);
    }
    for (attempt, delay) in BACKOFF_MS.iter().enumerate() {
        match timeout(Duration::from_secs(2), exchange(connector, config, &body)).await {
            Ok(Ok(response)) if response.status == 200 => {
                return Ok(Post::Ack(match_ack(frame, &response.body)?));
            }
            Ok(Ok(response)) if response.status == 409 => return Ok(Post::Conflict),
            other => {
                if attempt + 1 == BACKOFF_MS.len() {
                    match other {
                        Ok(Ok(response)) => {
                            let note = String::from_utf8_lossy(&response.body);
                            let note: String = note
                                .chars()
                                .filter(|char| char.is_ascii_graphic() || *char == ' ')
                                .take(160)
                                .collect();
                            eprintln!("down={} {note}", response.status);
                        }
                        Ok(Err(err)) => eprintln!("down={}", err.name()),
                        Err(_) => eprintln!("down=timeout"),
                    }
                    return Ok(Post::Down);
                }
                sleep(Duration::from_millis(*delay)).await;
            }
        }
    }
    Ok(Post::Down)
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

async fn exchange(connector: &TlsConnector, config: &SendConfig, body: &[u8]) -> Result<HttpResponse, WalError> {
    let address = format!("{}:{}", config.collector_host, config.collector_port);
    let tcp = TcpStream::connect(&address).await.map_err(|_| WalError::Timeout)?;
    let name = ServerName::try_from(config.server_name.clone()).map_err(|_| WalError::Tls)?;
    let mut stream = connector.connect(name, tcp).await.map_err(|_| WalError::Tls)?;
    let header = format!(
        "POST /v1/audit-events HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nx-project: not-the-identity\r\n\r\n",
        config.server_name,
        body.len()
    );
    stream.write_all(header.as_bytes()).await.map_err(WalError::Io)?;
    stream.write_all(body).await.map_err(WalError::Io)?;
    stream.flush().await.map_err(WalError::Io)?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.map_err(WalError::Io)?;
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Result<HttpResponse, WalError> {
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n").ok_or(WalError::Protocol)?;
    let header = std::str::from_utf8(&raw[..split]).map_err(|_| WalError::Protocol)?;
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or(WalError::Protocol)?;
    let body = raw[split + 4..].to_vec();
    if body.len() > 8192 {
        return Err(WalError::Protocol);
    }
    Ok(HttpResponse { status, body })
}

fn connector(config: &SendConfig) -> Result<TlsConnector, WalError> {
    private_mode(&config.key_file)?;
    let mut roots = RootCertStore::empty();
    let ca = fs::read(&config.ca_file).map_err(WalError::Io)?;
    for item in CertificateDer::pem_slice_iter(&ca) {
        roots.add(item.map_err(|_| WalError::Tls)?).map_err(|_| WalError::Tls)?;
    }
    if roots.is_empty() {
        return Err(WalError::Tls);
    }
    let chain = load_certs(&config.cert_file)?;
    let key = PrivateKeyDer::from_pem_file(&config.key_file).map_err(|_| WalError::Tls)?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let client = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| WalError::Tls)?
        .with_root_certificates(roots)
        .with_client_auth_cert(chain, key)
        .map_err(|_| WalError::Tls)?;
    Ok(TlsConnector::from(Arc::new(client)))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, WalError> {
    let bytes = fs::read(path).map_err(WalError::Io)?;
    let mut chain = Vec::new();
    for item in CertificateDer::pem_slice_iter(&bytes) {
        chain.push(item.map_err(|_| WalError::Tls)?);
    }
    if chain.is_empty() {
        return Err(WalError::Tls);
    }
    Ok(chain)
}

fn pairs(text: &str) -> Result<BTreeMap<String, String>, WalError> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(WalError::Bound);
        };
        if key.is_empty() || value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(WalError::Bound);
        }
        if map.insert(key.to_string(), value.to_string()).is_some() {
            return Err(WalError::Bound);
        }
    }
    Ok(map)
}

fn required(map: &BTreeMap<String, String>, key: &str) -> Result<String, WalError> {
    map.get(key).cloned().ok_or(WalError::Bound)
}

pub(crate) fn private_mode(path: &Path) -> Result<(), WalError> {
    let mode = fs::metadata(path).map_err(WalError::Io)?.permissions().mode() & 0o777;
    if mode == 0o600 || mode == 0o400 {
        Ok(())
    } else {
        Err(WalError::Bound)
    }
}

use std::os::unix::fs::PermissionsExt;

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> SealedFrame {
        SealedFrame {
            spool_id: "11111111-1111-4111-8111-111111111111".to_string(),
            request_id: "22222222-2222-4222-8222-222222222222".to_string(),
            request: parse_uuid("22222222-2222-4222-8222-222222222222").unwrap(),
            event_seq: 1,
            payload_hash: [0xab; 32],
            payload_hex: "00".to_string(),
            project: "shop".to_string(),
            kind: "refusal".to_string(),
        }
    }

    #[test]
    fn caller_ack_flag_is_rejected() {
        let sample = frame();
        assert!(match_ack(&sample, br#"{"ack":true}"#).is_err());
        let body = format!(
            r#"{{"protocol":"oo.audit.ingest/v1","spool_id":"{}","request_id":"{}","event_seq":1,"payload_hash":"{}","receipt_id":"33333333-3333-4333-8333-333333333333"}}"#,
            sample.spool_id,
            sample.request_id,
            hex_encode(&sample.payload_hash)
        );
        let ack = match_ack(&sample, body.as_bytes()).unwrap();
        assert_eq!(ack.receipt_id(), "33333333-3333-4333-8333-333333333333");
        let mut flipped = body.clone();
        flipped = flipped.replace(&hex_encode(&sample.payload_hash), &hex_encode(&[0xcd; 32]));
        assert!(match_ack(&sample, flipped.as_bytes()).is_err());
    }
}
