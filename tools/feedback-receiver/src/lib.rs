//! Private rustls receiver for `oo.feedback.receiver/v1`.
//!
//! Workload identity is `audit_ingest::certificate_uri`. Headers are not identity.
//! Review candidates come from `selfbuild_suggestions::produce`.

#![forbid(unsafe_code)]

mod config;
mod error;
mod pg;
mod review;

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ServerConfig, WebPkiClientVerifier};
use rustls::RootCertStore;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

pub use config::load_config;
pub use error::RecvError;
pub use review::{
    Decoded, Item, Operator, PROTOCOL, decode, hex_encode, load_operator_value, probe, project_ok,
    review as submit_review, sha256,
};

use config::Config;
use pg::PgStore;
use review::MAX_BODY;

const MAX_HEADER: usize = 8192;

pub async fn serve(config: Config) -> Result<(), RecvError> {
    let registry = load_registry(&config.registry_file)?;
    let operator = load_operator(&config)?;
    probe(&operator, &registry[0].1)?;
    let acceptor = TlsAcceptor::from(Arc::new(server_config(&config)?));
    let mut store = PgStore::connect(&config).await?;
    let listener = TcpListener::bind(&config.listen).await.map_err(RecvError::Io)?;
    println!("listening");
    let _ = io::stdout().flush();
    loop {
        let (tcp, _) = listener.accept().await.map_err(RecvError::Io)?;
        if let Ok(mut stream) = acceptor.accept(tcp).await
            && let Err(err) = handle(&mut stream, &operator, &mut store, &registry).await
        {
            let _ = respond(&mut stream, err.status(), &error_body(&err)).await;
        }
    }
}

pub fn load_registry(path: &Path) -> Result<Vec<(String, String)>, RecvError> {
    let text = fs::read_to_string(path).map_err(RecvError::Io)?;
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((uri, project)) = line.split_once(' ') else {
            return Err(RecvError::Bound);
        };
        if uri.is_empty()
            || uri.len() > 256
            || uri.bytes().any(|byte| byte.is_ascii_whitespace())
            || !project_ok(project)
            || rows.iter().any(|(seen, _)| seen == uri)
        {
            return Err(RecvError::Bound);
        }
        rows.push((uri.to_string(), project.to_string()));
    }
    if rows.is_empty() {
        return Err(RecvError::Bound);
    }
    Ok(rows)
}

fn load_operator(config: &Config) -> Result<Operator, RecvError> {
    Ok(Operator {
        role: config.role.clone(),
        source_id: config.source_id.clone(),
        catalog: read_json(&config.catalog_file)?,
        relationships: read_json(&config.relationships_file)?,
        profile: read_json(&config.profile_file)?,
    })
}

fn read_json(path: &Path) -> Result<Value, RecvError> {
    let bytes = fs::read(path).map_err(RecvError::Io)?;
    if bytes.windows(11).any(|window| window == b"PRIVATE KEY") || bytes.windows(6).any(|window| window == b"BEGIN ")
    {
        return Err(RecvError::Bound);
    }
    load_operator_value(&bytes)
}

async fn handle(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    operator: &Operator,
    store: &mut PgStore,
    registry: &[(String, String)],
) -> Result<(), RecvError> {
    let (_, connection) = stream.get_ref();
    let leaf = connection
        .peer_certificates()
        .and_then(|chain| chain.first())
        .ok_or(RecvError::Forbidden)?;
    let uri = audit_ingest::certificate_uri(leaf.as_ref()).map_err(|_| RecvError::Tls)?;
    let project = registry
        .iter()
        .find(|(item, _)| item == &uri)
        .map(|(_, project)| project.clone())
        .ok_or(RecvError::Forbidden)?;
    let (header, body) = read_http(stream).await?;
    let line = header.lines().next().ok_or(RecvError::Protocol)?;
    if let Some(rest) = line.strip_prefix("POST /v1/feedback ") {
        if !rest.starts_with("HTTP/1.1") || !json_content(&header) {
            return Err(RecvError::Protocol);
        }
        let decoded = decode(&project, &body)?;
        let prior = store.prior_ids(&project).await?;
        let reviewed = review::review(operator, &project, &decoded.items, &prior, Some(&decoded.receipt_id))?;
        let ack = store.commit(&decoded, &reviewed).await?;
        let bytes = serde_json::to_vec(&ack).map_err(|_| RecvError::Producer)?;
        return respond(stream, (200, "OK"), &bytes).await;
    }
    if let Some(rest) = line.strip_prefix("GET /v1/reviews ") {
        if !rest.starts_with("HTTP/1.1") || !body.is_empty() {
            return Err(RecvError::Protocol);
        }
        let page = store.list(&project).await?;
        let bytes = serde_json::to_vec(&page).map_err(|_| RecvError::Producer)?;
        return respond(stream, (200, "OK"), &bytes).await;
    }
    respond(stream, (405, "Method Not Allowed"), &error_body(&RecvError::Protocol)).await
}

fn json_content(header: &str) -> bool {
    let mut found = None;
    for line in header.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-type") {
            if found.is_some() {
                return false;
            }
            found = Some(value.trim());
        }
    }
    found == Some("application/json")
}

fn error_body(err: &RecvError) -> Vec<u8> {
    format!(r#"{{"protocol":"{PROTOCOL}","error":"{}"}}"#, err.name()).into_bytes()
}

async fn read_http(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
) -> Result<(String, Vec<u8>), RecvError> {
    let mut raw = Vec::new();
    let mut tmp = [0u8; 1024];
    let split = loop {
        if raw.len() > MAX_HEADER {
            return Err(RecvError::Protocol);
        }
        let read = stream.read(&mut tmp).await.map_err(RecvError::Io)?;
        if read == 0 {
            return Err(RecvError::Protocol);
        }
        raw.extend_from_slice(&tmp[..read]);
        if let Some(pos) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
    };
    let header = std::str::from_utf8(&raw[..split]).map_err(|_| RecvError::Protocol)?.to_string();
    let length = content_length(&header)?;
    if length > MAX_BODY {
        return Err(RecvError::Oversized);
    }
    let mut body = raw[split + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut tmp).await.map_err(RecvError::Io)?;
        if read == 0 {
            return Err(RecvError::Protocol);
        }
        body.extend_from_slice(&tmp[..read]);
        if body.len() > MAX_BODY {
            return Err(RecvError::Oversized);
        }
    }
    body.truncate(length);
    Ok((header, body))
}

fn content_length(header: &str) -> Result<usize, RecvError> {
    let mut found = None;
    for line in header.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            if found.is_some() {
                return Err(RecvError::Protocol);
            }
            found = Some(value.trim().parse::<usize>().map_err(|_| RecvError::Protocol)?);
        }
    }
    found.ok_or(RecvError::Protocol)
}

async fn respond(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    status: (u16, &str),
    body: &[u8],
) -> Result<(), RecvError> {
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status.0, status.1, body.len()
    );
    stream.write_all(header.as_bytes()).await.map_err(RecvError::Io)?;
    stream.write_all(body).await.map_err(RecvError::Io)?;
    let _ = stream.shutdown().await;
    Ok(())
}

fn server_config(config: &Config) -> Result<ServerConfig, RecvError> {
    let mut roots = RootCertStore::empty();
    let ca = fs::read(&config.ca_file).map_err(RecvError::Io)?;
    for item in CertificateDer::pem_slice_iter(&ca) {
        roots.add(item.map_err(|_| RecvError::Tls)?).map_err(|_| RecvError::Tls)?;
    }
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|_| RecvError::Tls)?;
    let certs = load_certs(&config.cert_file)?;
    let key = PrivateKeyDer::from_pem_file(&config.key_file).map_err(|_| RecvError::Tls)?;
    ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| RecvError::Tls)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .map_err(|_| RecvError::Tls)
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, RecvError> {
    let bytes = fs::read(path).map_err(RecvError::Io)?;
    let mut chain = Vec::new();
    for item in CertificateDer::pem_slice_iter(&bytes) {
        chain.push(item.map_err(|_| RecvError::Tls)?);
    }
    if chain.is_empty() {
        return Err(RecvError::Tls);
    }
    Ok(chain)
}
