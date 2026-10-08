//! HTTPS listeners. Management has no client auth. Resolve verifies the operator CA.

use std::io::{self, ErrorKind};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio_postgres_rustls::MakeRustlsConnect;
use tokio_rustls::TlsAcceptor;

use crate::error::Fault;
use crate::workload::workload_uri;

pub fn client_config(ca_pem: &[u8]) -> Result<ClientConfig, io::Error> {
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?;
    let mut roots = RootCertStore::empty();
    roots.add(certificate).map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    Ok(ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

pub fn pg_tls(ca_pem: &[u8]) -> Result<MakeRustlsConnect, Fault> {
    Ok(MakeRustlsConnect::new(client_config(ca_pem).map_err(|_| Fault::malformed())?))
}

pub fn server_config(cert_pem: &[u8], key_pem: &[u8]) -> Result<ServerConfig, Fault> {
    let (certs, key) = identity_material(cert_pem, key_pem)?;
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

pub fn component_config(cert_pem: &[u8], key_pem: &[u8], client_ca_pem: &[u8]) -> Result<ServerConfig, Fault> {
    let (certs, key) = identity_material(cert_pem, key_pem)?;
    let ca = CertificateDer::from_pem_slice(client_ca_pem).map_err(|_| Fault::malformed())?;
    let mut roots = RootCertStore::empty();
    roots.add(ca).map_err(|_| Fault::malformed())?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
        .build()
        .map_err(|_| Fault::malformed())?;
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| Fault::malformed())?
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .map_err(|_| Fault::malformed())?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

fn identity_material(cert_pem: &[u8], key_pem: &[u8]) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), Fault> {
    let certs = CertificateDer::pem_slice_iter(cert_pem).collect::<Result<Vec<_>, _>>().map_err(|_| Fault::malformed())?;
    if certs.is_empty() {
        return Err(Fault::malformed());
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| Fault::malformed())?;
    Ok((certs, key))
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

#[derive(Clone, Debug)]
pub struct WorkloadPeer {
    pub workload: String,
}

pub struct ComponentListener {
    pub tcp: TcpListener,
    pub acceptor: TlsAcceptor,
}

impl axum::serve::Listener for ComponentListener {
    type Io = tokio_rustls::server::TlsStream<TcpStream>;
    type Addr = WorkloadPeer;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (tcp, _remote) = match self.tcp.accept().await {
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
            let Some(workload) = peer_workload(&stream) else {
                continue;
            };
            return (stream, WorkloadPeer { workload });
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.tcp.local_addr()?;
        Ok(WorkloadPeer { workload: String::new() })
    }
}

fn peer_workload(stream: &tokio_rustls::server::TlsStream<TcpStream>) -> Option<String> {
    let (_, connection) = stream.get_ref();
    let leaf = connection.peer_certificates()?.first()?;
    workload_uri(leaf.as_ref()).ok()
}

fn is_connection_error(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::ConnectionRefused | ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset)
}
