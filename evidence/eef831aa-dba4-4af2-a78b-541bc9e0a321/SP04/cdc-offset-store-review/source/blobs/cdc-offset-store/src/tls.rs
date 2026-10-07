//! Caller-CA TLS for one tokio-postgres connection.
//!
//! aws-lc-rs, an empty root store plus the caller PEM, hostname verification,
//! no client certificate. The hide-commit stream drops the simple-query
//! COMMIT reply after the bytes have been written, so a lost reply is looked
//! up before any retry.

use std::future::Future;
use std::io::{self, ErrorKind};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::ClientConfig;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_postgres::config::SslMode;
use tokio_postgres::tls::{ChannelBinding, MakeTlsConnect, TlsConnect, TlsStream};
use tokio_postgres::Client;
use tokio_postgres::Config;
use tokio_postgres_rustls::MakeRustlsConnect;
use tokio_rustls::TlsConnector;

use crate::model::Endpoint;

pub async fn connect(endpoint: &Endpoint, password: &str, ca_pem: &[u8], hide_commit: bool) -> Result<Client, ()> {
    let config = client_config(ca_pem).map_err(|_| ())?;
    let mut builder = Config::new();
    builder
        .host(&endpoint.host)
        .port(endpoint.port)
        .dbname(&endpoint.database)
        .user(&endpoint.user)
        .password(password)
        .ssl_mode(SslMode::Require)
        .connect_timeout(Duration::from_secs(20));
    if hide_commit {
        let tls = HideCommitTls { config: Arc::new(config) };
        let (client, task) = builder.connect(tls).await.map_err(|_| ())?;
        tokio::spawn(async move {
            let _ = task.await;
        });
        Ok(client)
    } else {
        let tls = MakeRustlsConnect::new(config);
        let (client, task) = builder.connect(tls).await.map_err(|_| ())?;
        tokio::spawn(async move {
            let _ = task.await;
        });
        Ok(client)
    }
}

fn client_config(ca_pem: &[u8]) -> Result<ClientConfig, io::Error> {
    let certificate = CertificateDer::from_pem_slice(ca_pem)
        .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?;
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(certificate)
        .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?;
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    Ok(ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

#[derive(Clone)]
struct HideCommitTls {
    config: Arc<ClientConfig>,
}

struct HideCommitConnect {
    hostname: String,
    connector: TlsConnector,
}

struct HideCommitFuture<S> {
    inner: Pin<Box<dyn Future<Output = io::Result<HideCommitStream<S>>> + Send>>,
}

struct HideCommitStream<S> {
    inner: tokio_rustls::client::TlsStream<S>,
    pending: Vec<u8>,
    commit_written: bool,
    drop_reads: bool,
}

impl<S> MakeTlsConnect<S> for HideCommitTls
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Stream = HideCommitStream<S>;
    type TlsConnect = HideCommitConnect;
    type Error = io::Error;

    fn make_tls_connect(&mut self, domain: &str) -> Result<Self::TlsConnect, Self::Error> {
        Ok(HideCommitConnect {
            hostname: domain.to_owned(),
            connector: TlsConnector::from(Arc::clone(&self.config)),
        })
    }
}

impl<S> TlsConnect<S> for HideCommitConnect
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Stream = HideCommitStream<S>;
    type Error = io::Error;
    type Future = HideCommitFuture<S>;

    fn connect(self, stream: S) -> Self::Future {
        HideCommitFuture {
            inner: Box::pin(async move {
                let name = ServerName::try_from(self.hostname)
                    .map_err(|err| io::Error::new(ErrorKind::InvalidInput, err))?;
                let inner = self.connector.connect(name, stream).await?;
                Ok(HideCommitStream {
                    inner,
                    pending: Vec::new(),
                    commit_written: false,
                    drop_reads: false,
                })
            }),
        }
    }
}

impl<S> Future for HideCommitFuture<S>
where
    S: Send,
{
    type Output = io::Result<HideCommitStream<S>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}

impl<S> AsyncRead for HideCommitStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.drop_reads {
            let mut hidden = [0_u8; 256];
            let mut hidden_buf = ReadBuf::new(&mut hidden);
            match Pin::new(&mut this.inner).poll_read(cx, &mut hidden_buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(())) => Poll::Ready(Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "commit response not delivered",
                ))),
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
            }
        } else {
            Pin::new(&mut this.inner).poll_read(cx, buf)
        }
    }
}

impl<S> AsyncWrite for HideCommitStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let written = std::task::ready!(Pin::new(&mut this.inner).poll_write(cx, buf))?;
        this.pending.extend_from_slice(&buf[..written]);
        if commit_query_visible(&this.pending) {
            this.commit_written = true;
        }
        if this.pending.len() > 256 {
            let keep = this.pending.len() - 64;
            this.pending.drain(..keep);
        }
        if this.commit_written && !this.drop_reads {
            match Pin::new(&mut this.inner).poll_flush(cx) {
                Poll::Ready(Ok(())) => this.drop_reads = true,
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => {}
            }
        }
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_flush(cx);
        if this.commit_written && matches!(result, Poll::Ready(Ok(()))) {
            this.drop_reads = true;
        }
        result
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

impl<S> TlsStream for HideCommitStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn channel_binding(&self) -> ChannelBinding {
        ChannelBinding::none()
    }
}

pub fn commit_query_visible(buf: &[u8]) -> bool {
    buf.windows(12).any(|window| {
        window[0] == b'Q'
            && window[1..5] == [0, 0, 0, 11]
            && window[5..11] == *b"COMMIT"
            && window[11] == 0
    })
}

#[cfg(test)]
mod tests {
    use super::commit_query_visible;

    #[test]
    fn commit_message_is_the_simple_query() {
        let message = [b'Q', 0, 0, 0, 11, b'C', b'O', b'M', b'M', b'I', b'T', 0];
        assert!(commit_query_visible(&message));
        assert!(!commit_query_visible(b"COMMIT"));
    }
}
