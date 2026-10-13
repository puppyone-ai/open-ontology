//! Customer allowlist. Names come from the request. This module does not
//! open a socket, a password file, or a cursor.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub secret_ref: String,
    pub namespace: String,
    pub approved_objects: Vec<String>,
    pub approved_fields: Vec<String>,
    pub role: String,
    pub budget_limit: u64,
    pub batch_object: String,
    pub type_boundary_objects: Vec<String>,
    pub deny_object: String,
    pub deny_fields: Vec<String>,
}

pub fn load_policy(text: &str) -> Result<Policy, String> {
    let value: Value = serde_json::from_str(text).map_err(|_| "customer json".to_string())?;
    let secret_ref = required_string(&value, "secret_ref")?;
    if !secret_ref_ok(&secret_ref) {
        return Err("secret_ref".to_string());
    }
    let namespace = required_string(&value, "namespace")?;
    if !ident_ok(&namespace) {
        return Err("namespace".to_string());
    }
    let approved_objects = required_strings(&value, "approved_objects")?;
    let approved_fields = required_strings(&value, "approved_fields")?;
    let role = required_string(&value, "role")?;
    if role != "read" {
        return Err("role".to_string());
    }
    let budget_limit = value.get("budget_limit").and_then(Value::as_u64).ok_or("budget_limit")?;
    if budget_limit == 0 {
        return Err("budget_limit".to_string());
    }
    let batch_object = required_string(&value, "batch_object")?;
    let type_boundary_objects = optional_strings(&value, "type_boundary_objects")?;
    let deny_object = required_string(&value, "deny_object")?;
    let deny_fields = required_strings(&value, "deny_fields")?;
    let policy = Policy {
        secret_ref,
        namespace,
        approved_objects,
        approved_fields,
        role,
        budget_limit,
        batch_object,
        type_boundary_objects,
        deny_object,
        deny_fields,
    };
    check_names(&policy)?;
    Ok(policy)
}

pub fn authorize(policy: &Policy, object: &str, fields: &[String]) -> Result<(), &'static str> {
    if !policy.approved_objects.iter().any(|item| item == object) {
        return Err("unapproved_object");
    }
    let (namespace, collection) = object.split_once('.').ok_or("object")?;
    if namespace != policy.namespace || !ident_ok(collection) {
        return Err("namespace");
    }
    if fields.is_empty() || fields.iter().any(|field| !policy.approved_fields.iter().any(|item| item == field)) {
        return Err("unapproved_field");
    }
    Ok(())
}

pub fn pre_read_denial(policy: &Policy) -> Value {
    let reason = authorize(policy, &policy.deny_object, &policy.deny_fields).err().unwrap_or("approved");
    serde_json::json!({
        "object": policy.deny_object,
        "fields": policy.deny_fields,
        "refused": reason != "approved",
        "reason": reason,
        "find_sent": false,
        "password_read": false,
    })
}

pub fn resolve_secret(policy: &Policy, registry_text: &str) -> Result<String, String> {
    let registry: Value = serde_json::from_str(registry_text).map_err(|_| "registry json".to_string())?;
    if registry.get("protocol").and_then(Value::as_str) != Some("oo.operator.secret-registry/v1") {
        return Err("registry".to_string());
    }
    let entries = registry.get("entries").and_then(Value::as_array).ok_or("registry")?;
    for entry in entries {
        if entry.get("secret_ref").and_then(Value::as_str) == Some(policy.secret_ref.as_str()) {
            let file = entry.get("file").and_then(Value::as_str).ok_or("registry")?;
            if file.is_empty() || file.contains('\0') || file.contains("PASSWORD") {
                return Err("registry".to_string());
            }
            return Ok(file.to_string());
        }
    }
    Err("unknown_secret_ref".to_string())
}

pub fn object_parts(object: &str) -> Result<(&str, &str), &'static str> {
    let (namespace, collection) = object.split_once('.').ok_or("object")?;
    if !ident_ok(namespace) || !ident_ok(collection) {
        return Err("object");
    }
    Ok((namespace, collection))
}

fn check_names(policy: &Policy) -> Result<(), String> {
    if policy.approved_objects.is_empty() || policy.approved_fields.is_empty() {
        return Err("allowlist".to_string());
    }
    for object in policy.approved_objects.iter().chain(policy.type_boundary_objects.iter()) {
        let (namespace, _) = object_parts(object).map_err(|err| err.to_string())?;
        if namespace != policy.namespace {
            return Err("namespace".to_string());
        }
    }
    if !policy.approved_objects.iter().any(|item| item == &policy.batch_object) {
        return Err("batch_object".to_string());
    }
    for field in policy.approved_fields.iter().chain(policy.deny_fields.iter()) {
        if !ident_ok(field) {
            return Err("field".to_string());
        }
    }
    if authorize(policy, &policy.deny_object, &policy.deny_fields).is_ok() {
        return Err("deny_object".to_string());
    }
    Ok(())
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    value.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| key.to_string())
}

fn required_strings(value: &Value, key: &str) -> Result<Vec<String>, String> {
    let items = value.get(key).and_then(Value::as_array).ok_or_else(|| key.to_string())?;
    let mut names = Vec::new();
    for item in items {
        let Some(text) = item.as_str() else {
            return Err(key.to_string());
        };
        names.push(text.to_string());
    }
    if names.is_empty() {
        return Err(key.to_string());
    }
    Ok(names)
}

fn optional_strings(value: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(items) = value.get(key).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for item in items {
        let Some(text) = item.as_str() else {
            return Err(key.to_string());
        };
        names.push(text.to_string());
    }
    Ok(names)
}

pub fn secret_ref_ok(value: &str) -> bool {
    value.starts_with("secret://")
        && (10..200).contains(&value.len())
        && !value.contains("PASSWORD")
        && !value.contains("mongodb://")
        && !value.contains('@')
        && !value.chars().any(char::is_whitespace)
        && value.bytes().skip("secret://".len()).all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-')
        })
}

pub fn ident_ok(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    value.len() <= 63 && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::{authorize, load_policy, pre_read_denial, resolve_secret};

    fn sample() -> &'static str {
        r#"{
            "secret_ref": "secret://oo-dl01w1-41/reader",
            "namespace": "oo_w141",
            "approved_objects": ["oo_w141.shipments"],
            "approved_fields": ["_id", "code"],
            "role": "read",
            "budget_limit": 16,
            "batch_object": "oo_w141.shipments",
            "deny_object": "oo_w141.not_approved",
            "deny_fields": ["notApproved"]
        }"#
    }

    #[test]
    fn unapproved_object_and_field_fail_before_any_file_read() {
        let policy = load_policy(sample()).expect("policy");
        assert!(authorize(&policy, "oo_w141.shipments", &["_id".to_string(), "code".to_string()]).is_ok());
        assert_eq!(authorize(&policy, "oo_w141.not_approved", &["code".to_string()]).unwrap_err(), "unapproved_object");
        assert_eq!(authorize(&policy, "oo_w141.shipments", &["notApproved".to_string()]).unwrap_err(), "unapproved_field");
        let denial = pre_read_denial(&policy);
        assert_eq!(denial.get("refused").and_then(|value| value.as_bool()), Some(true));
        assert_eq!(denial.get("find_sent").and_then(|value| value.as_bool()), Some(false));
        assert_eq!(denial.get("password_read").and_then(|value| value.as_bool()), Some(false));
        assert_eq!(denial.get("reason").and_then(|value| value.as_str()), Some("unapproved_object"));
    }

    #[test]
    fn file_uri_and_unknown_secret_ref_are_rejected() {
        let mut text = sample().replace("secret://oo-dl01w1-41/reader", "file:/secrets/admin-password");
        assert!(load_policy(&text).is_err());
        text = sample().to_string();
        let policy = load_policy(&text).expect("policy");
        let registry = r#"{"protocol":"oo.operator.secret-registry/v1","entries":[{"secret_ref":"secret://other/reader","file":"/run/secrets/reader"}]}"#;
        assert_eq!(resolve_secret(&policy, registry).unwrap_err(), "unknown_secret_ref");
        let known = r#"{"protocol":"oo.operator.secret-registry/v1","entries":[{"secret_ref":"secret://oo-dl01w1-41/reader","file":"/run/secrets/reader"}]}"#;
        assert_eq!(resolve_secret(&policy, known).expect("path"), "/run/secrets/reader");
    }
}
