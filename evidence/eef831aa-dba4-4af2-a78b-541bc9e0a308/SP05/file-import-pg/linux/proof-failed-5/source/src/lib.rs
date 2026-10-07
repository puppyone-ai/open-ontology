//! Replace-only import of a validated CSV, TSV, or JSONL file into one
//! unpublished LOGGED PostgreSQL shadow.
//!
//! This is a first-release child of SP05-W05. It does not publish a live
//! pointer, grant a role, or accept append, upsert, or an untrusted parser.

#![forbid(unsafe_code)]

mod apply;
mod digest;
mod project;
mod request;

pub use apply::{import_replace, run_paths, stage_replace, Stage, StagedImport};
pub use project::{hash_typed_batch, project_verified, COPY_BUDGETS};
pub use request::{config_digest, parse_request, physical_plan, ColumnSpec, ImportRequest, PhysicalNames};

use serde::Serialize;

/// Request document name.
pub const REQUEST_PROTOCOL: &str = "oo.pipeline.file-import-pg/v1";

/// Successful caller result. This is not the stored run state.
pub const RESULT_PROTOCOL: &str = "oo.pipeline.file-import-pg-result/v1";

/// Refusal document name.
pub const ERROR_PROTOCOL: &str = "oo.pipeline.file-import-pg-error/v1";

/// Static refusal. `detail` is a token, never a path, a secret, or a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportError {
    /// Stable class.
    pub code: &'static str,
    /// Static qualifier.
    pub detail: &'static str,
    /// True after `ParentConfig::start` was called.
    pub child_started: bool,
}

impl ImportError {
    pub const fn new(code: &'static str, detail: &'static str) -> Self {
        Self {
            code,
            detail,
            child_started: false,
        }
    }

    pub const fn with_child(mut self, started: bool) -> Self {
        self.child_started = started;
        self
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} {}", self.code, self.detail)
    }
}

impl std::error::Error for ImportError {}

/// Caller view of one committed replace or one exact replay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportResult {
    /// [`RESULT_PROTOCOL`].
    pub protocol: &'static str,
    /// Always `awaiting_review`. Admin review is outside this child.
    pub state: &'static str,
    /// This child does not publish a live pointer.
    pub published: bool,
    /// This child does not grant a role.
    pub role_grant: bool,
    /// Row stored by `full_import_seal_run`. Not a publication.
    pub stored_run_state: &'static str,
    /// True when every batch receipt was already stored.
    pub replay: bool,
    /// Present only on the inserting call.
    pub run_id: Option<String>,
    /// Physical namespace from `plan_physical_mapping`.
    pub physical_namespace: String,
    /// Physical shadow table. Recomputed on replay.
    pub physical_table: String,
    /// Physical column names in profile order.
    pub physical_columns: Vec<String>,
    /// Data rows copied or already stored.
    pub rows: u64,
    /// Object SHA-256 the receipt is bound to.
    pub object_sha256: String,
    /// Config digest. File bytes are not an input.
    pub config_digest: String,
    /// Always `trusted_public_fixture`.
    pub parsing_mode: &'static str,
    /// Always `unsupported`. Not a containment certificate.
    pub kernel_restriction: &'static str,
    /// True when this call's worker `finish` returned.
    pub child_reaped: bool,
    /// Hardcoded by `full_import_write_batch`.
    pub stored_position_codec: &'static str,
    /// Hardcoded by `full_import_write_batch`.
    pub stored_position_lineage: &'static str,
    /// Fence generation. Present only on the inserting call.
    pub target_generation: Option<i64>,
    /// Source token.
    pub source: String,
}

/// CLI status. `0` is awaiting review, not parent acceptance.
pub fn exit_code(error: &ImportError) -> i32 {
    match error.code {
        "hash_conflict" => 3,
        "truncated" | "malformed" | "worker" | "typed_overflow" => 4,
        "database" | "coverage" | "run_closed" | "writer_gate" | "fence_session"
        | "mapping_conflict" | "mapping_absent" | "connect" | "tls" | "server_version"
        | "incomplete_receipt" | "encoding" | "principal" => 1,
        _ => 2,
    }
}

/// Error document. Field values are static tokens.
pub fn error_document(error: &ImportError) -> String {
    format!(
        "{{\"protocol\":\"{ERROR_PROTOCOL}\",\"code\":\"{}\",\"detail\":\"{}\",\"child_started\":{},\"published\":false}}",
        error.code,
        error.detail,
        if error.child_started { "true" } else { "false" }
    )
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn decode_sha256(text: &str) -> Result<[u8; 32], ImportError> {
    if text.len() != 64 {
        return Err(ImportError::new("closed_input", "sha256"));
    }
    let bytes = text.as_bytes();
    let mut out = [0u8; 32];
    for index in 0..32 {
        out[index] = (nibble(bytes[index * 2])? << 4) | nibble(bytes[index * 2 + 1])?;
    }
    Ok(out)
}

fn nibble(byte: u8) -> Result<u8, ImportError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(ImportError::new("closed_input", "sha256")),
    }
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}
