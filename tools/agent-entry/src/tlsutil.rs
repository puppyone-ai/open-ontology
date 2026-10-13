use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::error::{unavailable, Fault};

pub fn pg_tls_config(ca_pem: &[u8]) -> Result<ClientConfig, Fault> {
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|_| unavailable("ca"))?;
    let mut roots = RootCertStore::empty();
    roots.add(certificate).map_err(|_| unavailable("ca"))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    Ok(ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| unavailable("tls"))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

pub fn client_config(ca_pem: &[u8]) -> Result<ClientConfig, Fault> {
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|_| unavailable("ca"))?;
    let mut roots = RootCertStore::empty();
    roots.add(certificate).map_err(|_| unavailable("ca"))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let mut config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| unavailable("tls"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

pub async fn https_exchange(host: &str, port: u16, server_name: &str, ca_pem: &[u8], request: &[u8]) -> Result<Vec<u8>, Fault> {
    exchange(client_config(ca_pem)?, host, port, server_name, request, 20).await
}

pub fn mtls_config(ca_pem: &[u8], cert_pem: &[u8], key_pem: &[u8]) -> Result<ClientConfig, Fault> {
    let mut certs = Vec::new();
    for item in CertificateDer::pem_slice_iter(cert_pem) {
        certs.push(item.map_err(|_| unavailable("cert"))?);
    }
    if certs.is_empty() {
        return Err(unavailable("cert"));
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| unavailable("key"))?;
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|_| unavailable("ca"))?;
    let mut roots = RootCertStore::empty();
    roots.add(certificate).map_err(|_| unavailable("ca"))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let mut config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| unavailable("tls"))?
        .with_root_certificates(roots)
        .with_client_auth_cert(certs, key)
        .map_err(|_| unavailable("tls"))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

pub async fn mtls_exchange(host: &str, port: u16, server_name: &str, ca_pem: &[u8], cert_pem: &[u8], key_pem: &[u8], request: &[u8]) -> Result<Vec<u8>, Fault> {
    exchange(mtls_config(ca_pem, cert_pem, key_pem)?, host, port, server_name, request, 5).await
}

async fn exchange(config: ClientConfig, host: &str, port: u16, server_name: &str, request: &[u8], seconds: u64) -> Result<Vec<u8>, Fault> {
    let name = ServerName::try_from(server_name.to_owned()).map_err(|_| unavailable("sni"))?;
    let tcp = TcpStream::connect((host, port)).await.map_err(|_| unavailable("connect"))?;
    let connector = TlsConnector::from(Arc::new(config));
    let mut stream = connector.connect(name, tcp).await.map_err(|_| unavailable("handshake"))?;
    stream.write_all(request).await.map_err(|_| unavailable("write"))?;
    stream.flush().await.map_err(|_| unavailable("write"))?;
    let mut buf = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(seconds), async {
        let mut chunk = [0_u8; 8192];
        loop {
            let n = stream.read(&mut chunk).await.map_err(|_| unavailable("read"))?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 1_048_576 {
                return Err(unavailable("response"));
            }
            if http_complete(&buf) {
                break;
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| unavailable("deadline"))??;
    Ok(buf)
}

fn http_complete(buf: &[u8]) -> bool {
    let Some(head_end) = find_header_end(buf) else {
        return false;
    };
    let head = String::from_utf8_lossy(&buf[..head_end]);
    let mut length = None;
    for line in head.lines() {
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    match length {
        Some(len) => buf.len() >= head_end + 4 + len,
        None => false,
    }
}

pub fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}
