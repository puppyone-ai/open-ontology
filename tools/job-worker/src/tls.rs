//! Caller-CA TLS. Same connector shape as the reviewed session-store client:
//! aws-lc-rs, an empty root store plus the caller PEM, hostname verification,
//! and no client certificate.
//!
//! Origin: tools/session-store/src/tls.rs at the session-store lock pins.

use rustls::ClientConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use tokio::sync::watch;
use tokio_postgres::AsyncMessage;
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::api::WAKE_CHANNEL;
use crate::error::{Error, Result};

pub struct ListenSession {
    pub client: tokio_postgres::Client,
    pub wake: watch::Receiver<u64>,
}

enum Drive {
    Discard,
    Listen(watch::Sender<u64>),
}

pub fn rustls_connector(ca_pem: &[u8]) -> Result<MakeRustlsConnect> {
    let certificate = CertificateDer::from_pem_slice(ca_pem).map_err(|_| Error::new("tls"))?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).map_err(|_| Error::new("tls"))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let config = ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|_| Error::new("tls"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

struct Open<'a> {
    host: &'a str,
    port: u16,
    database: &'a str,
    user: &'a str,
    password: &'a str,
    ca_pem: &'a [u8],
}

pub async fn connect(
    host: &str,
    port: u16,
    database: &str,
    user: &str,
    password: &str,
    ca_pem: &[u8],
) -> Result<tokio_postgres::Client> {
    start(
        Open {
            host,
            port,
            database,
            user,
            password,
            ca_pem,
        },
        Drive::Discard,
    )
    .await
}

pub async fn connect_listening(
    host: &str,
    port: u16,
    database: &str,
    user: &str,
    password: &str,
    ca_pem: &[u8],
) -> Result<ListenSession> {
    let (tx, rx) = watch::channel(0_u64);
    let client = start(
        Open {
            host,
            port,
            database,
            user,
            password,
            ca_pem,
        },
        Drive::Listen(tx),
    )
    .await?;
    Ok(ListenSession { client, wake: rx })
}

async fn start(open: Open<'_>, drive: Drive) -> Result<tokio_postgres::Client> {
    if open.host.is_empty()
        || open.database.is_empty()
        || open.user.is_empty()
        || open.password.is_empty()
        || open.port == 0
    {
        return Err(Error::new("config"));
    }
    let tls = rustls_connector(open.ca_pem)?;
    let mut config = tokio_postgres::Config::new();
    config.user(open.user);
    config.password(open.password);
    config.host(open.host);
    config.port(open.port);
    config.dbname(open.database);
    config.ssl_mode(tokio_postgres::config::SslMode::Require);
    config.application_name("job-worker");
    config.connect_timeout(std::time::Duration::from_secs(10));
    let (client, connection) = config.connect(tls).await.map_err(|_| Error::new("connect"))?;
    match drive {
        Drive::Discard => {
            tokio::spawn(async move {
                let _ = connection.await;
            });
        }
        Drive::Listen(tx) => {
            tokio::spawn(async move {
                let mut seq = 0_u64;
                let mut connection = connection;
                loop {
                    let message = std::future::poll_fn(|cx| connection.poll_message(cx)).await;
                    match message {
                        Some(Ok(AsyncMessage::Notification(note)))
                            if note.channel() == WAKE_CHANNEL =>
                        {
                            seq = seq.wrapping_add(1);
                            let _ = tx.send(seq);
                        }
                        Some(Ok(_)) => {}
                        Some(Err(_)) | None => break,
                    }
                }
            });
        }
    }
    confirm_tls(&client).await?;
    Ok(client)
}

async fn confirm_tls(client: &tokio_postgres::Client) -> Result<()> {
    let row = client
        .query_one(
            "SELECT ssl, version FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
            &[],
        )
        .await
        .map_err(Error::sql)?;
    let ssl: bool = row.try_get(0).map_err(|_| Error::new("tls"))?;
    let version: Option<String> = row.try_get(1).map_err(|_| Error::new("tls"))?;
    match version {
        Some(version) if ssl && version.starts_with("TLSv1.") => Ok(()),
        _ => Err(Error::new("tls")),
    }
}
