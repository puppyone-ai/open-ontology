//! Management OIDC session for control B01.
//!
//! This crate is not an agent session, a harness session, or a Gateway token issuer.

#![forbid(unsafe_code)]

mod config;
mod cookie;
mod crypto;
mod error;
mod guard;
mod http;
mod oidc;
mod sql;
mod store;
mod tls;

pub use config::{canonical_commit, load_config, Binding, Config, Role};
pub use crypto::{pkcs8_rsa_public, sha256_hex};
pub use error::{error_body, Fault, CONTRACT};
pub use guard::{authorize, capabilities_for, VerifiedSession};
pub use http::{listen, serve};
pub use sql::migration_sha256;
pub use store::{InstallReport, IssueReport, Store};
pub use tls::{pg_tls, server_config, TlsListener};
