//! Stdin entry for the accepted role-pack compiler.
//!
//! The visibility object is caller-supplied public data. This module does not
//! check it against a live permission system. It does not authenticate, grant,
//! mount, or publish. Closure stays in `compile_role_pack`.

use serde::Deserialize;
use serde_json::Value;

use crate::canonical::{canonical_json, push_string};
use crate::role_pack::{RolePack, RolePackError, compile_role_pack};

pub const ROLE_PACK_COMMAND_PROTOCOL: &str = "oo.builder.role-pack-command/v1";

/// Static command failure. The message does not echo the request body.
pub struct RolePackCommandError {
    pub code: String,
    pub message: &'static str,
}

impl RolePackCommandError {
    fn named(name: &'static str, message: &'static str) -> Self {
        Self {
            code: format!("{ROLE_PACK_COMMAND_PROTOCOL}/{name}"),
            message,
        }
    }
}

impl From<RolePackError> for RolePackCommandError {
    fn from(error: RolePackError) -> Self {
        Self {
            code: error.code,
            message: error.message,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RolePackCommand {
    protocol: String,
    effective_oos: Value,
    visibility: Value,
}

/// Read one command document and return the canonical command result.
///
/// `effective_oos` and `visibility` are objects. They are scanned, rejected
/// when the envelope has an unknown field, then passed to `compile_role_pack`.
/// Digests, the manifest, and file text come from that `RolePack`.
pub fn compile_role_pack_command(input: &str) -> Result<String, RolePackCommandError> {
    let value = oo_core::parse_json_value(input, ROLE_PACK_COMMAND_PROTOCOL).map_err(map_scan)?;
    let document: RolePackCommand = serde_json::from_value(value).map_err(map_document)?;
    if document.protocol != ROLE_PACK_COMMAND_PROTOCOL {
        return Err(RolePackCommandError::named(
            "protocol.mismatch",
            "the command protocol is not the role-pack command this entry accepts",
        ));
    }
    if !document.effective_oos.is_object() {
        return Err(RolePackCommandError::named(
            "schema.invalid",
            "effective_oos is not an object",
        ));
    }
    if !document.visibility.is_object() {
        return Err(RolePackCommandError::named(
            "schema.invalid",
            "visibility is not an object",
        ));
    }
    let pack = compile_role_pack(
        &canonical_json(&document.effective_oos),
        &canonical_json(&document.visibility),
    )?;
    command_document(&pack)
}

fn map_scan(err: oo_core::DiagnosticList) -> RolePackCommandError {
    let code = err
        .items
        .first()
        .map(|item| item.code.clone())
        .unwrap_or_else(|| format!("{ROLE_PACK_COMMAND_PROTOCOL}/json.syntax"));
    let message = if code.ends_with("/json.budget.bytes") {
        "document exceeds the byte budget"
    } else if code.ends_with("/json.budget.depth") {
        "JSON nesting exceeds the depth budget"
    } else if code.ends_with("/json.budget.nodes") {
        "JSON node count exceeds the budget"
    } else if code.ends_with("/json.duplicate_key") {
        "duplicate JSON object key"
    } else {
        "role-pack command JSON is not valid"
    };
    RolePackCommandError { code, message }
}

fn map_document(err: serde_json::Error) -> RolePackCommandError {
    let text = err.to_string();
    let (name, message) = if text.contains("unknown field") {
        ("field.unknown", "role-pack command has an unknown field")
    } else if text.contains("missing field") {
        ("field.missing", "role-pack command is missing a required field")
    } else {
        (
            "schema.invalid",
            "role-pack command is not protocol, effective_oos, and visibility",
        )
    };
    RolePackCommandError::named(name, message)
}

fn command_document(pack: &RolePack) -> Result<String, RolePackCommandError> {
    let manifest = pack.file("manifest.json")?;
    let artifact: Value = serde_json::from_str(&pack.artifact).map_err(|_| {
        RolePackCommandError::named("schema.invalid", "compiled artifact is not an object")
    })?;
    if !artifact.is_object() {
        return Err(RolePackCommandError::named(
            "schema.invalid",
            "compiled artifact is not an object",
        ));
    }
    require_compiler_status(manifest)?;
    require_pack_files(pack, &artifact)?;
    let document = format!(
        "{{\"artifact\":{},\"cache_key\":{},\"cache_key_sha256\":{},\"manifest\":{},\"manifest_sha256\":{},\"pack_digest\":{},\"protocol\":\"{ROLE_PACK_COMMAND_PROTOCOL}\"}}",
        pack.artifact,
        json_string(&pack.cache_key),
        json_string(&pack.cache_key_sha256),
        manifest,
        json_string(&pack.manifest_sha256),
        json_string(&pack.pack_digest),
    );
    let parsed: Value = serde_json::from_str(&document).map_err(|_| {
        RolePackCommandError::named("schema.invalid", "command result is not an object")
    })?;
    if !parsed.is_object() {
        return Err(RolePackCommandError::named(
            "schema.invalid",
            "command result is not an object",
        ));
    }
    Ok(document)
}

fn require_compiler_status(manifest: &str) -> Result<(), RolePackCommandError> {
    let value: Value = serde_json::from_str(manifest).map_err(|_| {
        RolePackCommandError::named("schema.invalid", "compiled manifest is not an object")
    })?;
    let expected = [
        ("authentication", "not_claimed"),
        ("authorization", "not_claimed"),
        ("publication", "not_claimed"),
        ("mount", "not_performed"),
        ("samples", "omitted"),
    ];
    for (key, status) in expected {
        if value.get(key).and_then(Value::as_str) != Some(status) {
            return Err(RolePackCommandError::named(
                "manifest.status",
                "compiled manifest status was not left unchanged",
            ));
        }
    }
    if value.get("row_security_reproven") != Some(&Value::Bool(false)) {
        return Err(RolePackCommandError::named(
            "manifest.status",
            "compiled manifest status was not left unchanged",
        ));
    }
    Ok(())
}

fn require_pack_files(pack: &RolePack, artifact: &Value) -> Result<(), RolePackCommandError> {
    if artifact.get("pack_digest").and_then(Value::as_str) != Some(pack.pack_digest.as_str())
        || artifact.get("manifest_sha256").and_then(Value::as_str)
            != Some(pack.manifest_sha256.as_str())
    {
        return Err(RolePackCommandError::named(
            "artifact.mismatch",
            "compiled artifact digests do not match the pack",
        ));
    }
    let files = artifact.get("files").and_then(Value::as_array).ok_or_else(|| {
        RolePackCommandError::named("schema.invalid", "compiled artifact is not an object")
    })?;
    for file in files {
        let path = file.get("path").and_then(Value::as_str).ok_or_else(|| {
            RolePackCommandError::named("schema.invalid", "compiled artifact is not an object")
        })?;
        let text = file.get("text").and_then(Value::as_str).ok_or_else(|| {
            RolePackCommandError::named("schema.invalid", "compiled artifact is not an object")
        })?;
        let stored = pack.file(path)?;
        if stored != text || file.get("bytes").and_then(Value::as_u64) != Some(text.len() as u64) {
            return Err(RolePackCommandError::named(
                "artifact.mismatch",
                "compiled artifact text does not match the pack file",
            ));
        }
    }
    Ok(())
}

fn json_string(value: &str) -> String {
    let mut out = String::new();
    push_string(&mut out, value);
    out
}
