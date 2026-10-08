//! Typed receipt. The same request and commit always produce the same bytes.

use serde_json::{Map, Value};

use crate::canonical::{canonical_json, canonical_sha256};
use crate::error::PROTOCOL;
use crate::identity::{SERVICE_EMAIL, SERVICE_NAME};
use crate::plan::Plan;
use crate::request::Request;

pub const COMPILER: &str = "oo-curated-git-apply/v1";

pub struct CommitIds<'a> {
    pub old_commit: &'a str,
    pub new_commit: Option<&'a str>,
    pub old_tree: &'a str,
    pub new_tree: &'a str,
    pub dirty: bool,
    pub blobs: &'a [BlobRow],
}

pub struct BlobRow {
    pub dataset: String,
    pub path: String,
    pub old_blob: String,
    pub new_blob: String,
}

pub fn commit_message(request: &Request, plan: &Plan) -> String {
    format!(
        "{PROTOCOL}\nidempotency_key {}\nrequest_sha256 {}\nreview_sha256 {}\noutcome_sha256 {}\nschema_hash {}\nevidence_hash {}\nauthenticated false\nparent_auth required\n",
        request.idempotency_key,
        request.request_sha256,
        plan.review_sha256,
        plan.outcome_sha256,
        request.expected_schema_hash,
        request.expected_evidence_hash
    )
}

pub fn marker_conflict(message: &str, request: &Request) -> bool {
    let key = line_value(message, "idempotency_key");
    let hash = line_value(message, "request_sha256");
    key == Some(request.idempotency_key.as_str()) && hash != Some(request.request_sha256.as_str())
}

pub fn render(request: &Request, plan: &Plan, ids: &CommitIds<'_>, committed: bool) -> String {
    let mut blobs = Vec::new();
    for row in ids.blobs {
        blobs.push(serde_json::json!({
            "dataset": row.dataset,
            "new_blob": row.new_blob,
            "old_blob": row.old_blob,
            "path": row.path,
        }));
    }
    let mut map = Map::new();
    map.insert("authenticated".to_owned(), Value::Bool(false));
    map.insert("author_email".to_owned(), Value::String(SERVICE_EMAIL.to_owned()));
    map.insert("author_is_customer".to_owned(), Value::Bool(false));
    map.insert("author_is_reviewer".to_owned(), Value::Bool(false));
    map.insert("author_name".to_owned(), Value::String(SERVICE_NAME.to_owned()));
    map.insert("blobs".to_owned(), Value::Array(blobs));
    map.insert("checkout".to_owned(), Value::String("require_fresh_checkout".to_owned()));
    map.insert("command_sha256".to_owned(), Value::String(request.command_sha256.clone()));
    map.insert("compiler".to_owned(), Value::String(COMPILER.to_owned()));
    map.insert("decisions".to_owned(), Value::Array(plan.decisions.clone()));
    map.insert("evidence_hash".to_owned(), Value::String(request.expected_evidence_hash.clone()));
    map.insert("idempotency_key".to_owned(), Value::String(request.idempotency_key.clone()));
    map.insert("identity".to_owned(), Value::String("declared_only".to_owned()));
    map.insert(
        "new_commit".to_owned(),
        match ids.new_commit {
            Some(sha) => Value::String(sha.to_owned()),
            None => Value::Null,
        },
    );
    map.insert("new_tree".to_owned(), Value::String(ids.new_tree.to_owned()));
    map.insert("old_commit".to_owned(), Value::String(ids.old_commit.to_owned()));
    map.insert("old_tree".to_owned(), Value::String(ids.old_tree.to_owned()));
    map.insert("outcome".to_owned(), Value::String(if committed { "committed" } else { "unchanged" }.to_owned()));
    map.insert("outcome_sha256".to_owned(), Value::String(plan.outcome_sha256.clone()));
    map.insert("parent_auth".to_owned(), Value::String("required".to_owned()));
    map.insert("protocol".to_owned(), Value::String(PROTOCOL.to_owned()));
    map.insert("ref".to_owned(), Value::String(crate::request::APPROVED_REF.to_owned()));
    map.insert("request_sha256".to_owned(), Value::String(request.request_sha256.clone()));
    map.insert("review_sha256".to_owned(), Value::String(plan.review_sha256.clone()));
    map.insert("schema_hash".to_owned(), Value::String(request.expected_schema_hash.clone()));
    map.insert("statements".to_owned(), statements());
    map.insert("worktree".to_owned(), Value::String("not_updated".to_owned()));
    map.insert("worktree_dirty".to_owned(), Value::Bool(ids.dirty));
    let hash = canonical_sha256(&Value::Object(map.clone()));
    map.insert("receipt_sha256".to_owned(), Value::String(hash));
    canonical_json(&Value::Object(map))
}

fn statements() -> Value {
    serde_json::json!({
        "authentication": "not_claimed",
        "classification": "not_applied",
        "grants": "not_authorized",
        "hooks": "not_executed",
        "index": "private",
        "network": "not_used",
        "parent_auth": "required",
        "publication": "not_performed",
        "roles": "not_written",
        "secrets": "not_written",
        "worktree": "not_updated"
    })
}

fn line_value<'a>(message: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key} ");
    message.lines().find_map(|line| line.strip_prefix(&prefix))
}
