//! Authenticated self-build and review API.
//!
//! Management identity is verified only from a typed control-identity current
//! envelope. The child review ledger stays unauthenticated.

#![forbid(unsafe_code)]

pub mod adapt;
pub mod builder_call;
pub mod builder_inputs;
pub mod canonical;
pub mod corebind;
pub mod compile;
pub mod config;
pub mod error;
pub mod http;
pub mod identity;
pub mod ids;
pub mod jsonutil;
pub mod ledger;
pub mod membership;
pub mod oosyaml;
pub mod overlay;
mod project_catalog;
pub mod pack;
pub mod scope;
pub mod sealed;
pub mod serve;
pub mod spawn;
pub mod stages;
pub mod store;
pub mod suggest;
pub mod tlsutil;
pub mod worker;

pub const CONTRACT: &str = "oo.selfbuild-control-api/v1";
pub const MIGRATION_SQL: &str = include_str!("../migrations/oo_selfbuild_api_v1.sql");

pub use config::AppConfig;
pub use error::ApiError;
pub use http::router;
pub use identity::{IdentityFault, VerifiedIdentity};
pub use serve::serve;
pub use store::install_schema;
