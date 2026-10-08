//! Operator request. Unknown fields and free paths are rejected.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Deserialize;

use oo_core::api::payload_sha256;
use oo_core::text::{QualifiedTable, TableName};

use crate::canonical::canonical_sha256;
use crate::error::CuratedGitError;
use crate::json::parse_request_json;

pub const APPROVED_REF: &str = "refs/heads/curated";
const MAX_BINDINGS: usize = 16;
const MAX_COLUMNS: usize = 64;
const MAX_COMMAND: usize = 1_048_576;

#[derive(Debug, Clone)]
pub struct Request {
    pub project_root: PathBuf,
    pub expected_head: String,
    pub idempotency_key: String,
    pub expected_schema_hash: String,
    pub expected_evidence_hash: String,
    pub review_command: String,
    pub command_sha256: String,
    pub request_sha256: String,
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub dataset: String,
    pub expected_blob: String,
    pub columns: BTreeSet<String>,
}

pub fn parse_request(input: &str) -> Result<Request, CuratedGitError> {
    let value = parse_request_json(input)?;
    let request_sha256 = canonical_sha256(&value);
    let parsed: RequestIn = serde_json::from_value(value).map_err(map_schema)?;
    validate(parsed, request_sha256)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestIn {
    protocol: String,
    project_root: String,
    #[serde(rename = "ref")]
    git_ref: String,
    expected_head: String,
    idempotency_key: String,
    expected_schema_hash: String,
    expected_evidence_hash: String,
    review_command: String,
    bindings: Vec<BindingIn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingIn {
    dataset: String,
    expected_blob: String,
    #[serde(default)]
    columns: Vec<String>,
}

fn validate(parsed: RequestIn, request_sha256: String) -> Result<Request, CuratedGitError> {
    if parsed.protocol != crate::error::PROTOCOL {
        return Err(invalid());
    }
    if parsed.git_ref != APPROVED_REF {
        return Err(CuratedGitError::new(
            "ref.unregistered",
            "the ref is not the approved branch",
        ));
    }
    if !is_sha1(&parsed.expected_head) {
        return Err(invalid());
    }
    if !is_idempotency(&parsed.idempotency_key) {
        return Err(invalid());
    }
    if !is_sha256(&parsed.expected_schema_hash) || !is_sha256(&parsed.expected_evidence_hash) {
        return Err(invalid());
    }
    if parsed.project_root.len() > 4096
        || parsed.project_root.contains('\0')
        || !parsed.project_root.starts_with('/')
    {
        return Err(CuratedGitError::new(
            "root.unregistered",
            "the project root is not the registered checkout",
        ));
    }
    if parsed.review_command.len() > MAX_COMMAND || parsed.review_command.is_empty() {
        return Err(CuratedGitError::new(
            "limit.bytes",
            "the document exceeds the byte bound",
        ));
    }
    if parsed.bindings.is_empty() || parsed.bindings.len() > MAX_BINDINGS {
        return Err(CuratedGitError::new(
            "limit.files",
            "the change set exceeds the file bound",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut bindings = Vec::with_capacity(parsed.bindings.len());
    for binding in parsed.bindings {
        if QualifiedTable::new(&binding.dataset).is_err() || !is_sha1(&binding.expected_blob) {
            return Err(invalid());
        }
        if !seen.insert(binding.dataset.clone()) {
            return Err(CuratedGitError::new(
                "binding.duplicate",
                "a binding dataset or column is repeated",
            ));
        }
        if binding.columns.len() > MAX_COLUMNS {
            return Err(CuratedGitError::new(
                "limit.files",
                "the change set exceeds the file bound",
            ));
        }
        let mut columns = BTreeSet::new();
        for column in binding.columns {
            if TableName::new(&column).is_err() {
                return Err(invalid());
            }
            if !columns.insert(column) {
                return Err(CuratedGitError::new(
                    "binding.duplicate",
                    "a binding dataset or column is repeated",
                ));
            }
        }
        bindings.push(Binding {
            dataset: binding.dataset,
            expected_blob: binding.expected_blob,
            columns,
        });
    }
    let command_sha256 = payload_sha256(parsed.review_command.as_bytes());
    Ok(Request {
        project_root: PathBuf::from(parsed.project_root),
        expected_head: parsed.expected_head,
        idempotency_key: parsed.idempotency_key,
        expected_schema_hash: parsed.expected_schema_hash,
        expected_evidence_hash: parsed.expected_evidence_hash,
        review_command: parsed.review_command,
        command_sha256,
        request_sha256,
        bindings,
    })
}

fn map_schema(error: serde_json::Error) -> CuratedGitError {
    let text = error.to_string();
    if text.contains("unknown field") {
        CuratedGitError::new("schema.unknown", "the request has an unknown field")
    } else {
        invalid()
    }
}

fn invalid() -> CuratedGitError {
    CuratedGitError::new(
        "schema.invalid",
        "the request is not the curated git apply command",
    )
}

pub fn is_sha1(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_idempotency(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}
