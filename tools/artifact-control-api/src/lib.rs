//! Current-authorization custody of one completed query export.
//!
//! Caller JSON is not a grant. This crate is not a phase acceptance.

#![forbid(unsafe_code)]

mod config;
mod crypto;
mod error;
mod export_run;
mod http;
mod paths;
mod scope;
mod sql;
mod store;
mod tlsutil;

pub use config::{load_config, Config};
pub use crypto::{sha256_hex, token_ok};
pub use error::{error_body, Fault, CONTRACT};
pub use export_run::{check_dir, intake, produce, Checked, Intake, Produced, ReadyExport};
pub use http::{bound_addr, listen, serve};
pub use paths::{column_ok, idempotency_ok, intake_name_ok, remove_staging};
pub use scope::{grant_key, read_scope_files, scan_secrets, write_scope_files, Scope};
pub use store::{now_unix, CleanupReport, Outcome, Store};
