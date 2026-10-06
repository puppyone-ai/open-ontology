//! Caller-CA TLS for tokio-postgres.
//!
//! `rustls_connector` follows the reviewed session-store recipe and
//! `oo_gateway::policy::rustls_connector`: aws-lc-rs, an empty root store plus
//! the caller PEM, hostname verification, and no client certificate.
//! `SslMode::Require` is the only mode this crate opens.

use rustls::ClientConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::error::StoreError;

pub fn rustls_connector(ca_pem: &[u8]) -> Result<MakeRustlsConnect, StoreError> {
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|_| StoreError::Tls)?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).map_err(|_| StoreError::Tls)?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| StoreError::Tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}
