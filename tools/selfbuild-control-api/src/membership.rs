//! Membership binds project, table, column, and source hash.
//! A review-ledger command hash is not this proof.

use serde_json::Value;

use crate::error::ApiError;
use crate::ids::{hex64, object_id_ok};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub project_id: String,
    pub object_id: String,
    pub object_kind: String,
    pub schema_name: String,
    pub table_name: String,
    pub column_name: Option<String>,
    pub candidate_sha256: String,
    pub source_sha256: String,
    pub schema_sha256: String,
    pub evidence_sha256: String,
    pub generation_sha256: String,
}

const FORGED: &[&str] = &[
    "complete_command",
    "compiled_pack",
    "pack",
    "argv",
    "password",
    "bearer",
    "auto_accept",
    "authorized",
    "grant",
];

pub fn reject_forged(value: &Value) -> Result<(), ApiError> {
    let Some(object) = value.as_object() else {
        return Err(ApiError::malformed());
    };
    if FORGED.iter().any(|key| object.contains_key(*key)) {
        return Err(ApiError::malformed());
    }
    Ok(())
}

pub fn member_matches(member: &Member, project_id: &str, source_sha256: &str, candidate_sha256: &str) -> bool {
    member.project_id == project_id
        && member.source_sha256 == source_sha256
        && member.candidate_sha256 == candidate_sha256
        && hex64(&member.source_sha256)
        && object_id_ok(&member.object_id)
        && kind_matches(&member.object_kind, &member.object_id)
        && column_matches(member)
}

fn kind_matches(kind: &str, object_id: &str) -> bool {
    let dots = object_id.matches('.').count();
    (kind == "dataset" && dots == 1) || (kind == "column" && dots == 2)
}

fn column_matches(member: &Member) -> bool {
    let mut parts = member.object_id.split('.');
    let schema = parts.next();
    let table = parts.next();
    let column = parts.next();
    schema == Some(member.schema_name.as_str())
        && table == Some(member.table_name.as_str())
        && column == member.column_name.as_deref()
}

pub fn receipt_is_not_membership(receipt_command_sha256: &str, member: &Member) -> bool {
    receipt_command_sha256 != member.source_sha256 || receipt_command_sha256 != member.candidate_sha256
}
