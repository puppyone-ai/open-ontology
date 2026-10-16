//! MySQL 5.7/8 source-only catalog, shared batch/CDC mapping and bounded Arrow pulls.
//! No COPY, target credentials, Loader or tool dependencies live here.

pub mod map;
mod reader;
mod source;
pub mod values;
pub mod version;

pub use reader::{Batch, Cancel, ReadConnection, TableReader, cancel_connection};
use sha2::{Digest, Sha256};
pub use source::{
    Access, catalog_columns, connect, current_user_is, inspect, inspect_replication,
    keep_requested, query_drop, raw_cell, replication_grants_ok, scalar,
};
use std::{fmt, path::PathBuf};

/// Closed errors never include SQL values, topology or credential contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MysqlError {
    pub code: &'static str,
    pub fact: &'static str,
}
impl MysqlError {
    pub fn new(code: &'static str, fact: &'static str) -> Self {
        Self { code, fact }
    }
}
impl fmt::Display for MysqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code, self.fact)
    }
}
impl std::error::Error for MysqlError {}

#[derive(Clone, Debug)]
pub struct SelectedTable {
    pub schema: String,
    pub name: String,
    pub columns: Vec<String>,
}

/// Only source endpoint/TLS and a reference; the caller resolves that reference.
#[derive(Clone)]
pub struct MysqlConfig {
    pub mysql_host: String,
    pub mysql_port: u16,
    pub mysql_database: String,
    pub mysql_user: String,
    pub mysql_ca_file: PathBuf,
    pub secret_ref: crate::SecretRef,
    pub connect_timeout_ms: u64,
}

#[derive(Clone, Debug)]
pub struct StatementNote {
    pub verb: String,
    pub sha256: String,
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn mysql_ident(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
