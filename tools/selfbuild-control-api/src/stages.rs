//! Stage resume rules and producer argv. No second compiler.

use serde_json::{json, Value};

use crate::config::ProjectCfg;
use crate::error::ApiError;
use crate::jsonutil::parse_exact;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resume {
    Run,
    Skip,
    Uncertain,
    Failed,
    Await,
}

pub fn resume_action(stage: &str, status: &str, output_sha256: Option<&str>) -> Resume {
    match status {
        "succeeded" if output_sha256.is_some() => Resume::Skip,
        "running" if output_sha256.is_some() => Resume::Skip,
        "running" if stage == "S5" && output_sha256.is_none() => Resume::Uncertain,
        "running" => Resume::Run,
        "failed" | "refused" => Resume::Failed,
        "awaiting" => Resume::Await,
        _ => Resume::Run,
    }
}

pub fn catalog_stdin(project: &ProjectCfg) -> Vec<u8> {
    let tables: Vec<Value> = project
        .source
        .tables
        .iter()
        .map(|(schema, table)| json!({"schema": schema, "table": table}))
        .collect();
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.catalog-pg/v1",
        "source": {
            "source_id": project.source_ref,
            "host": project.source.host,
            "port": project.source.port,
            "database": project.source.database,
            "username": project.source.username,
            "secret_ref": project.source_secret_ref,
            "password_file": project.source.password_file,
            "ca_file": project.source.ca_file,
            "tables": tables,
            "budget": {
                "connect_ms": 30000,
                "statement_ms": 30000,
                "lock_ms": 1000,
                "max_rows": 10000,
                "max_bytes": 8388608
            }
        }
    }))
    .unwrap_or_default()
}

pub fn builder_config(project: &ProjectCfg, catalog_bin: &str) -> Value {
    // One local model call may exceed 30s. Three calls still fit the 300s child budget.
    let tables: Vec<Value> = project
        .source
        .tables
        .iter()
        .map(|(schema, table)| json!({"schema": schema, "table": table}))
        .collect();
    json!({
        "protocol": "oo.pg-builder.config/v1",
        "adapter": "loopback_http_local_dev",
        "namespace": "pg-builder",
        "purpose": "generate",
        "sample_policy": "none",
        "temperature": 0,
        "endpoint": {
            "model_id": "grok-4.7",
            "origin": "http://127.0.0.1:8317",
            "path": "/v1/chat/completions"
        },
        "secret": {"bearer_file": project.llm.bearer_file},
        "budget": {
            "deadline_ms": 90000,
            "max_completion_tokens": 8192,
            "max_in_flight": 1,
            "max_prompt_tokens": 100000,
            "max_request_bytes": 262144,
            "max_response_bytes": 262144,
            "max_tables": 16,
            "max_total_tokens": 108192
        },
        "declared_price": {"status": "unknown"},
        "postgres": {
            "ca_file": project.source.ca_file,
            "catalog_bin": catalog_bin,
            "database": project.source.database,
            "host": project.source.host,
            "password_file": project.source.password_file,
            "port": project.source.port,
            "tables": tables,
            "username": project.source.username
        }
    })
}

pub fn rules_args(catalog: &str, profile: &str, cards: &str, auto_accept: &str) -> Vec<String> {
    vec![
        "produce".to_owned(),
        "--catalog".to_owned(),
        catalog.to_owned(),
        "--profile".to_owned(),
        profile.to_owned(),
        "--cards".to_owned(),
        cards.to_owned(),
        "--auto-accept".to_owned(),
        auto_accept.to_owned(),
    ]
}

pub fn suggestions_args() -> Vec<String> {
    vec!["produce".to_owned()]
}

pub fn accept_ledger(bytes: &[u8]) -> Result<Value, ApiError> {
    let value = parse_exact(bytes)?;
    let object = value.as_object().ok_or(ApiError::malformed())?;
    if object.get("protocol").and_then(Value::as_str) != Some("oo.review-ledger.result/v1") {
        return Err(ApiError::not_ready());
    }
    if object.get("authenticated") != Some(&Value::Bool(false))
        || object.get("authority").and_then(Value::as_str) != Some("not_verified")
        || object.get("management_identity").and_then(Value::as_str) != Some("not_claimed")
        || object.get("published") != Some(&Value::Bool(false))
    {
        return Err(ApiError::membership());
    }
    let kind = object.get("kind").and_then(Value::as_str).unwrap_or("");
    if kind == "commit.unknown" {
        return Err(ApiError::commit_unknown());
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassPreview {
    pub ready: Vec<String>,
    pub hidden: Vec<String>,
}

pub fn preview_classification(
    visible: &[String],
    hidden: &[String],
    items: &[(String, String)],
    accept_all: bool,
) -> Result<ClassPreview, ApiError> {
    if accept_all {
        return Err(ApiError::accept_all());
    }
    let mut ready = Vec::new();
    for (object_id, pii) in items {
        if hidden.iter().any(|item| item == object_id) {
            return Err(ApiError::membership());
        }
        if pii == "unknown" {
            return Err(ApiError::pii_unknown());
        }
        if !matches!(pii.as_str(), "none" | "personal" | "sensitive") {
            return Err(ApiError::malformed());
        }
        if !visible.iter().any(|item| item == object_id) {
            return Err(ApiError::membership());
        }
        ready.push(object_id.clone());
    }
    if visible.iter().any(|column| !ready.iter().any(|item| item == column)) {
        return Err(ApiError::classification_incomplete());
    }
    let hidden_out = hidden.to_vec();
    Ok(ClassPreview {
        ready,
        hidden: hidden_out,
    })
}
