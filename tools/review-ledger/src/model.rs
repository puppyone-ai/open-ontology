//! Request and result shapes. Unknown JSON keys are refused.
//! A request has no role, header, or credential bytes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{err, LedgerError};

pub const RESULT_PROTOCOL: &str = "oo.review-ledger.result/v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_file: String,
    pub ca_file: String,
    pub server_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRequest {
    pub protocol: String,
    pub project_id: String,
    pub control_file: String,
    pub postgres: Endpoint,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnqueueRequest {
    pub protocol: String,
    pub project_id: String,
    pub control_file: String,
    pub credential_file: String,
    pub postgres: Endpoint,
    pub object_kind: String,
    pub object_id: String,
    #[serde(default)]
    pub expected_revision: Option<i64>,
    pub candidate_sha256: String,
    pub input_sha256: String,
    pub evidence_sha256: String,
    pub schema_sha256: String,
    pub curated_sha256: String,
    pub source_sha256: String,
    pub candidate_protocol: String,
    pub owner_label: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideRequest {
    pub protocol: String,
    pub project_id: String,
    pub control_file: String,
    pub credential_file: String,
    pub postgres: Endpoint,
    pub object_id: String,
    pub expected_revision: i64,
    pub idempotency_key: String,
    pub decision_kind: String,
    pub choice: String,
    pub reviewer_label: String,
    pub reviewer_at: String,
    pub candidate_sha256: String,
    pub input_sha256: String,
    pub evidence_sha256: String,
    pub schema_sha256: String,
    pub curated_sha256: String,
    pub source_sha256: String,
    #[serde(default)]
    pub fields: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchItem {
    pub object_id: String,
    pub expected_revision: i64,
    pub idempotency_key: String,
    pub decision_kind: String,
    pub choice: String,
    pub reviewer_label: String,
    pub reviewer_at: String,
    pub candidate_sha256: String,
    pub input_sha256: String,
    pub evidence_sha256: String,
    pub schema_sha256: String,
    pub curated_sha256: String,
    pub source_sha256: String,
    #[serde(default)]
    pub fields: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchRequest {
    pub protocol: String,
    pub project_id: String,
    pub control_file: String,
    pub credential_file: String,
    pub postgres: Endpoint,
    pub batch_id: String,
    pub accept_all: bool,
    pub items: Vec<BatchItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptRequest {
    pub protocol: String,
    pub project_id: String,
    pub control_file: String,
    pub credential_file: String,
    pub postgres: Endpoint,
    pub idempotency_key: String,
    pub compiler: String,
    pub command_file: String,
    #[serde(default)]
    pub required_columns: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryRequest {
    pub protocol: String,
    pub project_id: String,
    pub control_file: String,
    pub credential_file: String,
    pub postgres: Endpoint,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemOut {
    pub object_id: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct Output {
    pub protocol: &'static str,
    pub kind: String,
    pub code: String,
    pub authenticated: bool,
    pub authority: &'static str,
    pub management_identity: &'static str,
    pub curated_applied: bool,
    pub published: bool,
    pub grants: &'static str,
    pub git_applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_status: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ItemOut>,
}

impl Output {
    pub fn refused(code: &str) -> Self {
        Self::bare("refused", code)
    }

    pub fn bare(kind: &str, code: &str) -> Self {
        Self {
            protocol: RESULT_PROTOCOL,
            kind: kind.to_owned(),
            code: code.to_owned(),
            authenticated: false,
            authority: "not_verified",
            management_identity: "not_claimed",
            curated_applied: false,
            published: false,
            grants: "not_authorized",
            git_applied: false,
            revision: None,
            body_sha256: None,
            history_sha256: None,
            batch_status: None,
            items: Vec::new(),
        }
    }

    pub fn from_error(error: &LedgerError) -> Self {
        let kind = if error.code == "commit.unknown" {
            "commit.unknown"
        } else {
            "refused"
        };
        Self::bare(kind, &error.code)
    }
}

const GRANT_KEYS: &[&str] = &[
    "grant",
    "grant_roles",
    "grants",
    "certified",
    "publish",
    "apply",
    "role",
    "authorization",
    "authenticated",
    "headers",
    "visible_roles",
    "column_acl",
    "auto_accept",
];

pub fn reject_request_keys(value: &Value) -> Result<(), LedgerError> {
    let Some(object) = value.as_object() else {
        return Err(err("schema.invalid", "the request is not an object"));
    };
    if object.keys().any(|key| GRANT_KEYS.contains(&key.as_str())) {
        return Err(err("not_grant", "a review decision cannot grant, certify, or publish"));
    }
    if let Some(fields) = object.get("fields").and_then(Value::as_object)
        && fields.keys().any(|key| GRANT_KEYS.contains(&key.as_str()))
    {
        return Err(err(
            "not_grant",
            "a review decision cannot grant, certify, or publish",
        ));
    }
    if let Some(items) = object.get("items").and_then(Value::as_array) {
        for item in items {
            reject_request_keys(item)?;
        }
    }
    Ok(())
}
