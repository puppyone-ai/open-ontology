//! HTTPS listener and PostgreSQL TLS. Both trust only the configured PEM.

use std::io::{self, ErrorKind};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio_postgres_rustls::MakeRustlsConnect;
use tokio_rustls::TlsAcceptor;

use crate::error::Fault;

pub fn pg_tls(ca_pem: &[u8]) -> Result<MakeRustlsConnect, Fault> {
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|_| Fault::malformed())?;
    let mut roots = RootCertStore::empty();
    roots.add(certificate).map_err(|_| Fault::malformed())?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| Fault::malformed())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

pub fn server_config(cert_pem: &[u8], key_pem: &[u8]) -> Result<ServerConfig, Fault> {
    let certs = CertificateDer::pem_slice_iter(cert_pem).collect::<Result<Vec<_>, _>>().map_err(|_| Fault::malformed())?;
    if certs.is_empty() {
        return Err(Fault::malformed());
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| Fault::malformed())?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let mut config = ServerConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| Fault::malformed())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|_| Fault::malformed())?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

pub fn acceptor(cert_pem: &[u8], key_pem: &[u8]) -> Result<TlsAcceptor, Fault> {
    Ok(TlsAcceptor::from(Arc::new(server_config(cert_pem, key_pem)?)))
}

pub struct TlsListener {
    pub tcp: TcpListener,
    pub acceptor: TlsAcceptor,
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (tcp, remote) = match self.tcp.accept().await {
                Ok(pair) => pair,
                Err(error) => {
                    if !is_connection_error(&error) {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    continue;
                }
            };
            let handshake = self.acceptor.accept(tcp);
            let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(5), handshake).await else {
                continue;
            };
            return (stream, remote);
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

fn is_connection_error(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::ConnectionRefused | ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset)
}
