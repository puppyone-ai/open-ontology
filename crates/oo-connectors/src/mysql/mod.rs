//! MySQL 5.7/8 source-only catalog, shared batch/CDC mapping and bounded Arrow pulls.
//! No COPY, target credentials, Loader or tool dependencies live here.
//! [`engine::MysqlEngine`] implements the v0.2 `SourceEngine` for 5.7, 8.0,
//! 8.4 and 9.x; [`capability`] holds its version dispatch table.

pub mod c5;
#[cfg(feature = "mysql-binlog")]
pub use engine::change::{BinlogRetention, MysqlChanges};
#[cfg(feature = "mysql-binlog")]
pub use engine::repair::{HandoffBarrier, RepairBinding, RepairBudget, SourceRepair};
pub mod capability;
pub mod chunk;
pub mod engine;
pub mod gtid;
pub mod keyset;
pub mod map;
pub mod protect;
mod reader;
mod source;
pub mod values;
pub mod version;
// Root exports are coordinator-owned; these paths keep the lane deliverable usable.
#[path = "../mariadb/mod.rs"]
pub mod mariadb;
#[path = "../oceanbase/mod.rs"]
pub mod oceanbase;
#[path = "../tidb/mod.rs"]
pub mod tidb;

pub use engine::{EngineOptions, MysqlEngine, RowFingerprint, WatermarkMode};
pub use reader::{
    Batch, CHUNK_DEADLINE_MS, CHUNK_MAX_BYTES, CHUNK_MAX_ROWS, CHUNK_STATEMENT_MS, Cancel,
    ChunkBatch, ChunkCursor, ChunkReader, FullReader, ReadConnection, TableReader,
    cancel_connection,
};
use sha2::{Digest, Sha256};
pub use source::{
    Access, FullReadGrants, catalog_columns, connect, current_user_is, full_read_grants, inspect,
    inspect_full, inspect_replication, keep_requested, keep_requested_keyed, query_drop, raw_cell,
    replication_grants_ok, scalar,
};
use std::{fmt, path::PathBuf};

/// Closed errors never include SQL values, topology or credential contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MysqlError {
    pub code: &'static str,
    pub fact: &'static str,
    pub value_len: Option<u64>,
}
impl MysqlError {
    pub fn new(code: &'static str, fact: &'static str) -> Self {
        Self {
            code,
            fact,
            value_len: None,
        }
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
    /// Optional certificate name. TCP still goes to `mysql_host:mysql_port`, and
    /// the certificate is verified against this name. `None` verifies the host.
    pub mysql_tls_server_name: Option<String>,
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

/// Host name accepted as a certificate name override (`tls_server_name`). Only
/// letters, digits, `-` and `.`, with no empty label. Anything else is refused,
/// so the value cannot carry a path, a space or a URL fragment.
pub fn tls_server_name_ok(name: &str) -> bool {
    name.len() <= 253
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
mod tls_name_tests {
    use super::tls_server_name_ok;

    #[test]
    fn accepts_dns_names_and_refuses_anything_else() {
        assert!(tls_server_name_ok("mysql57.owned.example"));
        assert!(tls_server_name_ok("db-1.internal"));
        assert!(tls_server_name_ok("localhost"));
        for bad in [
            "",
            ".",
            ".leading",
            "trailing.",
            "a..b",
            "has space",
            "host/path",
            "user@host",
            "[::1]",
            "a:3306",
            &"a".repeat(64),
        ] {
            assert!(!tls_server_name_ok(bad), "{bad:?} must be refused");
        }
        assert!(!tls_server_name_ok(&format!("{}.com", "a.".repeat(127))));
    }
}
