//! Caller-CA TLS. aws-lc-rs, empty root store, hostname verification, no client certificate.
//!
//! The connect shape follows tools/cdc-offset-store/src/tls.rs at
//! a13b1cc80ed66c07199a41d47b31fd42530f2be9. The hide-commit wrapper is not
//! copied: a lost commit reply is commit.unknown and is not retried.

use std::io::{self, ErrorKind};
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use rustls::ClientConfig;
use tokio_postgres::config::SslMode;
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::error::{err, LedgerError};
use crate::model::Endpoint;

pub async fn connect(endpoint: &Endpoint, password: &str, ca_pem: &[u8]) -> Result<Client, LedgerError> {
    let config = client_config(ca_pem).map_err(|_| err("tls.config", "the caller CA was refused"))?;
    let mut builder = Config::new();
    builder
        .host(&endpoint.host)
        .port(endpoint.port)
        .dbname(&endpoint.database)
        .user(&endpoint.user)
        .password(password)
        .ssl_mode(SslMode::Require)
        .connect_timeout(Duration::from_secs(20));
    let tls = MakeRustlsConnect::new(config);
    let (client, task) = builder
        .connect(tls)
        .await
        .map_err(|_| err("tls.connect", "the database connection was refused"))?;
    tokio::spawn(async move {
        let _ = task.await;
    });
    Ok(client)
}

fn client_config(ca_pem: &[u8]) -> Result<ClientConfig, io::Error> {
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
