#![forbid(unsafe_code)]

//! Own-session create, event, and localprocess close API.
//! This crate does not sign a production release.

mod close;
pub mod config;
pub mod error;
pub mod http;
mod lifecycle;
mod sha256;
mod sql;
mod store;

pub use close::{cancel_session, fixture_stage, finish_stage, CloseReceipt};
pub use config::{load_config, startup_gate, Config, CONTRACT, DEPLOYMENT, OWNER_UUID, PRODUCTION_SIGNED};
pub use error::Fault;
pub use http::serve;
pub use session_transition::CancelTrigger;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use gateway_token_store::TrustedControlAdapter;

use crate::config::find_binding;
use crate::lifecycle::{endpoint_of, random_hex, token_endpoint};
use crate::store::Db;

pub async fn install_all(cfg: &Config) -> Result<String, Fault> {
    let endpoint = endpoint_of(cfg);
    let session = session_store::install(
        &endpoint,
        sql::SESSION_ADMIN,
        &cfg.pg.session_admin_password,
        cfg.pg.ca_pem.as_bytes(),
    )
    .await
    .map_err(lifecycle::store_fault)?;
    let tokens = TrustedControlAdapter::new(
        token_endpoint(&endpoint),
        sql::TOKEN_ADMIN,
        cfg.pg.token_admin_password.clone(),
        cfg.pg.ca_pem.as_bytes().to_vec(),
    )
    .map_err(|_| Fault::Unavailable("token_login"))?;
    let token = tokens.install().await.map_err(|err| match err {
        gateway_token_store::StoreError::CommitUnknown => Fault::Unavailable("commit_unknown"),
        gateway_token_store::StoreError::Connect | gateway_token_store::StoreError::Tls => Fault::Unavailable("store"),
        _ => Fault::Incomplete("token_install"),
    })?;
    let api = store::install(&Db {
        endpoint: &endpoint,
        user: sql::API_ADMIN,
        password: &cfg.pg.api_admin_password,
        ca_pem: cfg.pg.ca_pem.as_bytes(),
    })
    .await?;
    let token_store = match token.disposition {
        gateway_token_store::InstallDisposition::Installed => "installed",
        gateway_token_store::InstallDisposition::Replay => "replay",
    };
    let session_store = serde_json::to_value(session.disposition).map_err(|_| Fault::Unavailable("json"))?;
    Ok(serde_json::json!({
        "contract": CONTRACT,
        "session_store": session_store,
        "token_store": token_store,
        "api_store": api,
        "production_signed": PRODUCTION_SIGNED,
        "deployment": DEPLOYMENT,
    })
    .to_string())
}

pub async fn issue_grant(cfg: &Config) -> Result<String, Fault> {
    let issue = cfg.issue.as_ref().ok_or(Fault::Malformed("issue"))?;
    if !issue.grant_out.starts_with('/') {
        return Err(Fault::Malformed("grant_out"));
    }
    let bindings = config::load_bindings(&cfg.bindings_path)?;
    let binding = find_binding(&bindings, &issue.principal_id, &issue.project_id).ok_or(Fault::Malformed("binding"))?;
    let mut secret = random_hex(32)?;
    let hash = config::digest_text(&secret)?;
    let parent = Path::new(&issue.grant_out).parent().ok_or(Fault::Malformed("grant_out"))?;
    fs::create_dir_all(parent).map_err(|_| Fault::Unavailable("grant_out"))?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&issue.grant_out)
        .map_err(|_| Fault::Conflict("grant_out"))?;
    file.write_all(secret.as_bytes()).map_err(|_| Fault::Unavailable("grant_out"))?;
    file.sync_all().map_err(|_| Fault::Unavailable("grant_out"))?;
    let mut bytes = std::mem::take(&mut secret).into_bytes();
    bytes.fill(0);
    std::hint::black_box(bytes);
    let endpoint = endpoint_of(cfg);
    let inserted = store::insert_grant(
        &Db {
            endpoint: &endpoint,
            user: sql::API_RUNTIME,
            password: &cfg.pg.api_runtime_password,
            ca_pem: cfg.pg.ca_pem.as_bytes(),
        },
        &hash,
        &binding.principal_id,
        &binding.project_id,
    )
    .await;
    if let Err(err) = inserted {
        let _ = fs::remove_file(&issue.grant_out);
        return Err(err);
    }
    Ok(serde_json::json!({
        "contract": CONTRACT,
        "principal_id": binding.principal_id,
        "project_id": binding.project_id,
        "grant_sha256": hash,
        "production_signed": PRODUCTION_SIGNED,
    })
    .to_string())
}
