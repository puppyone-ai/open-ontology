//! Project pg-builder profile into the suggestions command.
//! `authorization.authorized` is that producer's seal flag.

use serde_json::{json, Map, Value};

use crate::canonical::{canonical_json, canonical_sha256};
use crate::error::ApiError;

pub fn command(project_id: &str, catalog: &Value, profile: &Value) -> Result<Value, ApiError> {
    if catalog.get("protocol").and_then(Value::as_str) != Some("oo.source.pgsql/v1") {
        return Err(ApiError::malformed());
    }
    if profile.get("protocol").and_then(Value::as_str) != Some("oo.pg-builder.profile/v1") {
        return Err(ApiError::malformed());
    }
    let source_id = catalog.get("source_id").and_then(Value::as_str).ok_or(ApiError::malformed())?;
    let projected = project_profile(profile)?;
    let relationships = json!({
        "schema": "oo.selfbuild-suggestions.relationships/v1",
        "items": []
    });
    let feedback = json!({
        "schema": "oo.selfbuild-suggestions.feedback/v1",
        "items": []
    });
    let catalog_hash = canonical_sha256(catalog);
    let profile_hash = canonical_sha256(&projected);
    let relationships_hash = canonical_sha256(&relationships);
    let feedback_hash = canonical_sha256(&feedback);
    let mut document = Map::new();
    document.insert("schema".to_owned(), json!("oo.selfbuild-suggestions.command/v1"));
    document.insert(
        "scope".to_owned(),
        json!({
            "project": project_id,
            "role": "oo_r_draft",
            "evidence_sha256": catalog_hash,
            "source_id": source_id,
            "schema_version": "oos/v1",
            "model_scope": "none"
        }),
    );
    document.insert(
        "authorization".to_owned(),
        json!({
            "authorized": true,
            "catalog_sha256": catalog_hash,
            "relationships_sha256": relationships_hash,
            "profile_sha256": profile_hash,
            "feedback_sha256": feedback_hash
        }),
    );
    document.insert("catalog".to_owned(), catalog.clone());
    document.insert("relationships".to_owned(), relationships);
    document.insert("profile".to_owned(), projected);
    document.insert("feedback".to_owned(), feedback);
    document.insert("negative_pairs".to_owned(), json!([]));
    document.insert("curated_overrides".to_owned(), json!([]));
    document.insert("prior_candidate_ids".to_owned(), json!([]));
    let value = Value::Object(document);
    if canonical_json(&value).len() > 2_097_152 {
        return Err(ApiError::malformed());
    }
    Ok(value)
}

fn project_profile(profile: &Value) -> Result<Value, ApiError> {
    let tables = profile.get("tables").and_then(Value::as_array).ok_or(ApiError::malformed())?;
    let mut projected = Vec::new();
    for table in tables {
        let schema = table.get("schema").and_then(Value::as_str).ok_or(ApiError::malformed())?;
        let name = table.get("name").and_then(Value::as_str).ok_or(ApiError::malformed())?;
        let columns = table.get("columns").and_then(Value::as_array).ok_or(ApiError::malformed())?;
        let mut names = Vec::new();
        for column in columns {
            let column_name = column.get("name").and_then(Value::as_str).ok_or(ApiError::malformed())?;
            names.push(json!({"name": column_name}));
        }
        let mut projected_table = Map::new();
        projected_table.insert("schema".to_owned(), json!(schema));
        projected_table.insert("name".to_owned(), json!(name));
        if let Some(text) = table.get("reltuples").and_then(Value::as_str)
            && reltuples_ok(text)
        {
            projected_table.insert("reltuples".to_owned(), json!(text));
        }
        projected_table.insert("columns".to_owned(), Value::Array(names));
        projected.push(Value::Object(projected_table));
    }
    Ok(json!({
        "schema": "oo.selfbuild-suggestions.profile/v1",
        "sample_policy": "none",
        "tables": projected
    }))
}

fn reltuples_ok(text: &str) -> bool {
    if text.is_empty() || text.len() > 32 {
        return false;
    }
    let mut parts = text.split('.');
    let Some(whole) = parts.next() else {
        return false;
    };
    let fraction = parts.next();
    if parts.next().is_some() || whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    if whole.len() > 1 && whole.starts_with('0') {
        return false;
    }
    match fraction {
        None => true,
        Some(fraction) => !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit()),
    }
}

pub fn candidate_id(project_id: &str, kind: &str, key: &str) -> String {
    let text = format!("oo.selfbuild-suggestions.candidate/v1\n{project_id}\n{kind}\n{key}\n");
    let hash = crate::canonical::sha256_hex(text.as_bytes());
    format!("cand_{}", &hash[..32])
}

pub const COMMAND_KEYS: &[&str] = &[
    "authorization",
    "catalog",
    "curated_overrides",
    "feedback",
    "negative_pairs",
    "prior_candidate_ids",
    "profile",
    "relationships",
    "schema",
    "scope",
];
