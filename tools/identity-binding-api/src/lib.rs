//! HTTPS binding client. It forwards a verified control-identity session and writes one Git ref.
//!
//! This crate does not issue sessions, data grants, owner certificates, or Gateway tokens.

#![forbid(unsafe_code)]

mod config;
mod error;
mod git;
mod http;
mod tls;

pub use config::{load_config, Change, Config};
pub use error::{error_body, Fault, CONTRACT};
pub use git::{cas_commit, seed_repository};
pub use http::{listen, serve};
