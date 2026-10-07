//! Typed decision fields. Extra keys are not approval.

use serde_json::Value;

use crate::canonical::canonical_json;
use crate::control::{hex_ok, ident_ok, kind_matches};
use crate::error::{err, LedgerError};

#[derive(Debug)]
pub struct Prepared {
    pub kind: &'static str,
    pub choice: &'static str,
    pub fields_json: String,
    pub fields_value: Value,
}

pub fn prepare(
    kind: &str,
    choice: &str,
    object_id: &str,
    fields: Option<&Value>,
) -> Result<Prepared, LedgerError> {
    if !kind_matches(object_kind(object_id), object_id) {
        return Err(err("schema.invalid", "the object id shape is wrong"));
    }
    let column = object_id.matches('.').count() == 2;
    match (kind, choice) {
        ("label_description", "approve") => approve(column, fields),
        ("label_description", "reject" | "defer") => empty(kind, choice, fields),
        ("classification", "confirm") => classify(fields),
        ("owner_certification", "record") => empty(kind, choice, fields),
        _ => Err(err("schema.invalid", "the decision kind and choice do not match")),
    }
}

fn object_kind(object_id: &str) -> &'static str {
    if object_id.matches('.').count() == 1 {
        "dataset"
    } else {
        "column"
    }
}

fn empty(kind: &str, choice: &str, fields: Option<&Value>) -> Result<Prepared, LedgerError> {
    if fields.is_some() {
        return Err(err("schema.invalid", "this decision does not take fields"));
    }
    Ok(done(kind, choice, "null", Value::Null))
}

fn approve(column: bool, fields: Option<&Value>) -> Result<Prepared, LedgerError> {
    let Some(value) = fields else {
        return Err(err("schema.invalid", "an approval needs its fields"));
    };
    let Some(object) = value.as_object() else {
        return Err(err("schema.invalid", "approval fields are not an object"));
    };
    let allowed: &[&str] = if column {
        &["label", "description"]
    } else {
        &["label", "description", "grain"]
    };
    if object.len() != allowed.len() || allowed.iter().any(|key| !object.contains_key(*key)) {
        return Err(err("schema.invalid", "approval fields are not the allowed set"));
    }
    for key in allowed {
        overwrite(object.get(*key).expect("key checked"))?;
    }
    Ok(done(
        "label_description",
        "approve",
        &canonical_json(value),
        value.clone(),
    ))
}

fn overwrite(value: &Value) -> Result<(), LedgerError> {
    let Some(object) = value.as_object() else {
        return Err(err("schema.invalid", "an approval field is not an object"));
    };
    if object.keys().any(|key| key != "overwrite" && key != "current_sha256") {
        return Err(err("schema.invalid", "an approval field has an unknown key"));
    }
    match object.get("overwrite").and_then(Value::as_bool) {
        Some(false) => {
            if object.contains_key("current_sha256") {
                return Err(err("field.cas", "a kept field cannot carry a current hash"));
            }
            Ok(())
        }
        Some(true) => {
            let Some(hash) = object.get("current_sha256").and_then(Value::as_str) else {
                return Err(err("field.cas", "a replacement needs the current field hash"));
            };
            if hex_ok(hash) {
                Ok(())
            } else {
                Err(err("field.cas", "the current field hash is not 64 lowercase hex characters"))
            }
        }
        None => Err(err("schema.invalid", "overwrite is not a boolean")),
    }
}

fn classify(fields: Option<&Value>) -> Result<Prepared, LedgerError> {
    let Some(value) = fields else {
        return Err(err("pii.unknown", "a classification needs a pii value"));
    };
    let Some(object) = value.as_object() else {
        return Err(err("schema.invalid", "classification fields are not an object"));
    };
    if object.len() != 3
        || !object.contains_key("pii")
        || !object.contains_key("semantic_type")
        || !object.contains_key("state")
    {
        if !object.contains_key("pii") {
            return Err(err("pii.unknown", "a classification needs a pii value"));
        }
        return Err(err("schema.invalid", "classification fields are not the allowed set"));
    }
    let pii = object.get("pii").and_then(Value::as_str).unwrap_or("");
    if pii == "unknown" || pii.is_empty() {
        return Err(err("pii.unknown", "unknown pii is not a classification"));
    }
    if !matches!(pii, "none" | "personal" | "sensitive") {
        return Err(err("schema.invalid", "pii is not none, personal, or sensitive"));
    }
    let semantic = object.get("semantic_type").and_then(Value::as_str).unwrap_or("");
    if !ident_ok(semantic) {
        return Err(err("schema.invalid", "semantic_type is not an identifier"));
    }
    if object.get("state").and_then(Value::as_str) != Some("confirmed") {
        return Err(err("schema.invalid", "classification state is not confirmed"));
    }
    Ok(done(
        "classification",
        "confirm",
        &canonical_json(value),
        value.clone(),
    ))
}

fn done(kind: &str, choice: &str, fields_json: &str, fields_value: Value) -> Prepared {
    let static_kind = match kind {
        "label_description" => "label_description",
        "classification" => "classification",
        "owner_certification" => "owner_certification",
        _ => "label_description",
    };
    let static_choice = match choice {
        "approve" => "approve",
        "reject" => "reject",
        "defer" => "defer",
        "confirm" => "confirm",
        "record" => "record",
        _ => "reject",
    };
    Prepared {
        kind: static_kind,
        choice: static_choice,
        fields_json: fields_json.to_owned(),
        fields_value,
    }
}
