//! Caller-CA TLS. The root store contains only the configured CA.

use std::io::{self, ErrorKind};
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, ServerConfig};

pub fn client_config(ca_pem: &[u8]) -> Result<ClientConfig, io::Error> {
    let certificate = CertificateDer::from_pem_slice(ca_pem)
        .map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?;
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(certificate)
        .map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    Ok(ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

pub fn server_config(cert_pem: &[u8], key_pem: &[u8]) -> Result<Arc<ServerConfig>, io::Error> {
    let certs = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?;
    let key = PrivateKeyDer::from_pem_slice(key_pem)
        .map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ServerConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|cause| io::Error::new(ErrorKind::InvalidInput, cause))?;
    Ok(Arc::new(config))
}
