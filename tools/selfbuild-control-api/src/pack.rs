//! Combined pack envelope. Producer bytes keep their review state.

use serde_json::{json, Value};

use crate::canonical::sha256_hex;
use crate::error::ApiError;
use crate::CONTRACT;

pub fn envelope(operation: &str, build_id: &str, producer: &Value) -> Value {
    let bytes = serde_json::to_vec(producer).unwrap_or_default();
    json!({
        "contract": CONTRACT,
        "operation": operation,
        "build_id": build_id,
        "output_sha256": sha256_hex(&bytes),
        "producer": producer,
        "producer_authenticated": false,
        "management_identity": "verified",
        "published": false,
        "active": false,
        "data_grants": "none",
        "compiled_pack": "not_published"
    })
}

pub fn require_two_roles(document: &Value) -> Result<(), ApiError> {
    let roles = document
        .get("roles")
        .and_then(Value::as_array)
        .ok_or(ApiError::malformed())?;
    if roles.len() != 2 {
        return Err(ApiError::malformed());
    }
    Ok(())
}

pub fn review_state_preserved(before: &Value, wrapped: &Value) -> bool {
    wrapped.get("producer").and_then(|item| item.get("review.state")) == before.get("review.state")
        && wrapped.get("published") == Some(&Value::Bool(false))
        && wrapped.get("active") == Some(&Value::Bool(false))
        && wrapped.get("producer_authenticated") == Some(&Value::Bool(false))
        && wrapped.get("compiled_pack").and_then(Value::as_str) == Some("not_published")
}

pub fn unreviewed_prefix_kept(producer: &Value, field: &str) -> bool {
    match producer.get(field).and_then(Value::as_str) {
        Some(text) => text.starts_with("UNREVIEWED"),
        None => false,
    }
}
