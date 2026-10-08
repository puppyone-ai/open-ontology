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
        json!({"project_id": project_id, "evidence": catalog_hash}),
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
        let reltuples = table.get("reltuples").cloned().unwrap_or(Value::Null);
        let columns = table.get("columns").and_then(Value::as_array).ok_or(ApiError::malformed())?;
        let mut names = Vec::new();
        for column in columns {
            let column_name = column.get("name").and_then(Value::as_str).ok_or(ApiError::malformed())?;
            names.push(json!({"name": column_name}));
        }
        projected.push(json!({
            "schema": schema,
            "name": name,
            "reltuples": reltuples,
            "columns": names
        }));
    }
    Ok(json!({
        "schema": "oo.selfbuild-suggestions.profile/v1",
        "sample_policy": "none",
        "tables": projected
    }))
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
