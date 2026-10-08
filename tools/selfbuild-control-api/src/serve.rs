//! TLS listener. Plaintext is not the product surface.

use std::io;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::connect_info::Connected;
use axum::serve::IncomingStream;
use axum::serve::Listener;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use crate::config::AppConfig;
use crate::error::ApiError;
use crate::http::{self, AppState};
use crate::tlsutil;
use crate::worker;

pub async fn serve(config: AppConfig) -> Result<(), ApiError> {
    let cert = std::fs::read(&config.listen.cert_file).map_err(|_| ApiError::not_ready())?;
    let key = std::fs::read(&config.listen.key_file).map_err(|_| ApiError::not_ready())?;
    let server = tlsutil::server_config(&cert, &key).map_err(|_| ApiError::not_ready())?;
    let bind: SocketAddr = config.listen.bind.parse().map_err(|_| ApiError::malformed())?;
    let tcp = TcpListener::bind(bind).await.map_err(|_| ApiError::not_ready())?;
    println!("{}", stdout_line("serve", "ready"));
    let listener = TlsListener {
        tcp,
        acceptor: TlsAcceptor::from(server),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let cancels = Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::new()));
    let worker_config = Arc::new(config.clone());
    let worker_stop = stop.clone();
    let worker_cancels = cancels.clone();
    tokio::spawn(async move {
        worker::supervise(worker_config, worker_stop, worker_cancels).await;
    });
    let router = http::router_from(Arc::new(http::AppState { config, cancels }));
    axum::serve(listener, router).await.map_err(|_| ApiError::not_ready())
}

struct TlsListener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
}

#[derive(Clone, Debug)]
struct PeerAddr(#[allow(dead_code)] SocketAddr);

impl Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<TcpStream>;
    type Addr = PeerAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (tcp, remote) = match self.tcp.accept().await {
                Ok(pair) => pair,
                Err(_) => {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            let handshake = self.acceptor.accept(tcp);
            let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(5), handshake).await else {
                continue;
            };
            return (stream, PeerAddr(remote));
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(PeerAddr(self.tcp.local_addr()?))
    }
}

impl Connected<IncomingStream<'_, TlsListener>> for PeerAddr {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        stream.remote_addr().clone()
    }
}

pub fn stdout_line(operation: &str, dependency: &str) -> String {
    format!(
        "{{\"contract\":\"oo.selfbuild-control-api/v1\",\"operation\":\"{operation}\",\"management_identity\":\"not_verified\",\"dependency\":\"{dependency}\",\"published\":false,\"active\":false,\"data_grants\":\"none\",\"compiled_pack\":\"not_published\"}}"
    )
}

#[allow(dead_code)]
fn _state(state: AppState) -> AppState {
    state
}
