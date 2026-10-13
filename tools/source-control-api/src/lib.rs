//! Authenticated local source registration and import.
//!
//! This crate calls the pinned producers. It does not build, review, or publish.

#![forbid(unsafe_code)]

use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::sync::Arc;

use tokio::net::TcpListener;

use crate::config::Config;
use crate::http::AppState;
use crate::store::Store;
use crate::tls::TlsListener;

pub mod config;
pub mod error;
pub mod http;
pub mod identity;
pub mod ids;
pub mod jsonutil;
pub mod producers;
pub mod sha256;
pub mod signal;
pub mod store;
pub mod tls;
pub mod worker;

pub use error::{Fault, CONTRACT};
pub use store::Install;

pub struct Prepared {
    pub addr: String,
    listener: TlsListener,
    state: AppState,
}

pub async fn install(config: &Config) -> Result<Install, Fault> {
    let store = Store::connect(config).await?;
    store.install().await
}

pub async fn prepare(config: Arc<Config>) -> Result<Prepared, Fault> {
    config.verify_pins()?;
    let store = Store::connect(&config).await?;
    store.require_installed().await?;
    store.recover_on_start().await?;
    prepare_dir(&config.object_dir)?;
    prepare_dir(&config.run_dir)?;
    let identity = identity::identity_client(&config.identity_ca_pem).map_err(|_| Fault::Refused)?;
    let listener = TcpListener::bind((config.listen_host, config.listen_port)).await.map_err(|_| Fault::Refused)?;
    let bound = listener.local_addr().map_err(|_| Fault::Refused)?;
    if bound.port() != config.listen_port {
        return Err(Fault::Refused);
    }
    let acceptor = tls::acceptor(&config.tls_cert_pem, &config.tls_key_pem)?;
    let incoming = TlsListener { tcp: listener, acceptor };
    let state = AppState { config, store, identity };
    Ok(Prepared { addr: bound.to_string(), listener: incoming, state })
}

pub async fn run(prepared: Prepared) -> Result<(), Fault> {
    worker::spawn(Arc::clone(&prepared.state.config), prepared.state.store.clone());
    axum::serve(prepared.listener, http::router(prepared.state)).await.map_err(|_| Fault::Refused)?;
    Ok(())
}

fn prepare_dir(path: &Path) -> Result<(), Fault> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(path).map_err(|_| Fault::Refused)?;
    Ok(())
}
