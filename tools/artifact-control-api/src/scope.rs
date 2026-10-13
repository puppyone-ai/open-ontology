//! Operator scope evidence. The HTTP body never supplies these fields.

use std::path::Path;

use serde_json::Value;

use crate::crypto::{mac_hex, mac_ok};
use crate::error::Fault;
use crate::paths::{read_regular, replace_private};

pub const SCOPE_VERSION: &str = "oo.artifact-control/scope/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub principal_id: String,
    pub project_id: String,
    pub session_id: String,
    pub role: String,
    pub policy_id: String,
    pub knowledge_id: String,
    pub knowledge_digest: String,
    pub absolute_end_unix: i64,
    pub revision: i64,
}

impl Scope {
    pub fn canonical(&self) -> Result<String, Fault> {
        validate_scope(self)?;
        Ok(format!(
            "{{\"version\":\"{SCOPE_VERSION}\",\"principal_id\":\"{}\",\"project_id\":\"{}\",\"session_id\":\"{}\",\"role\":\"{}\",\"policy_id\":\"{}\",\"knowledge_id\":\"{}\",\"knowledge_digest\":\"{}\",\"absolute_end_unix\":{},\"revision\":{}}}",
            self.principal_id,
            self.project_id,
            self.session_id,
            self.role,
            self.policy_id,
            self.knowledge_id,
            self.knowledge_digest,
            self.absolute_end_unix,
            self.revision
        ))
    }

    pub fn same_person(&self, other: &Scope) -> bool {
        self.principal_id == other.principal_id && self.project_id == other.project_id && self.session_id == other.session_id
    }

    pub fn same_tuple(&self, role: &str, policy: &str, knowledge_id: &str, knowledge_digest: &str) -> bool {
        self.role == role && self.policy_id == policy && self.knowledge_id == knowledge_id && self.knowledge_digest == knowledge_digest
    }
}

pub fn write_scope_files(scope_path: &Path, mac_path: &Path, scope: &Scope, key: &[u8]) -> Result<(), Fault> {
    let bytes = scope.canonical()?;
    let mac = mac_hex(key, bytes.as_bytes())?;
    replace_private(scope_path, bytes.as_bytes())?;
    let mut line = mac;
    line.push('\n');
    replace_private(mac_path, line.as_bytes())?;
    Ok(())
}

pub fn read_scope_files(scope_path: &Path, mac_path: &Path, key: &[u8]) -> Result<Scope, Fault> {
    let bytes = read_regular(scope_path, 8192, Some(0o600)).map_err(|_| Fault::scope_rejected())?;
    let mac_bytes = read_regular(mac_path, 128, Some(0o600)).map_err(|_| Fault::scope_rejected())?;
    let mac_text = std::str::from_utf8(&mac_bytes).map_err(|_| Fault::scope_rejected())?;
    let mac_text = mac_text.strip_suffix('\n').unwrap_or(mac_text);
    if mac_text.len() != 64 || !mac_text.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(Fault::scope_rejected());
    }
    let scope = parse_scope(&bytes)?;
    let canonical = scope.canonical()?;
    if canonical.as_bytes() != bytes.as_slice() {
        return Err(Fault::scope_rejected());
    }
    if !mac_ok(key, &bytes, mac_text)? {
        return Err(Fault::scope_rejected());
    }
    Ok(scope)
}

fn parse_scope(bytes: &[u8]) -> Result<Scope, Fault> {
    let text = std::str::from_utf8(bytes).map_err(|_| Fault::scope_rejected())?;
    let value: Value = serde_json::from_str(text).map_err(|_| Fault::scope_rejected())?;
    let object = value.as_object().ok_or_else(Fault::scope_rejected)?;
    if object.len() != 10 {
        return Err(Fault::scope_rejected());
    }
    let version = required_str(object, "version")?;
    if version != SCOPE_VERSION {
        return Err(Fault::scope_rejected());
    }
    let scope = Scope {
        principal_id: required_str(object, "principal_id")?,
        project_id: required_str(object, "project_id")?,
        session_id: required_str(object, "session_id")?,
        role: required_str(object, "role")?,
        policy_id: required_str(object, "policy_id")?,
        knowledge_id: required_str(object, "knowledge_id")?,
        knowledge_digest: required_str(object, "knowledge_digest")?,
        absolute_end_unix: required_i64(object, "absolute_end_unix")?,
        revision: required_i64(object, "revision")?,
    };
    validate_scope(&scope)?;
    Ok(scope)
}

fn required_str(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, Fault> {
    object.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(Fault::scope_rejected)
}

fn required_i64(object: &serde_json::Map<String, Value>, key: &str) -> Result<i64, Fault> {
    let value = object.get(key).ok_or_else(Fault::scope_rejected)?;
    let number = value.as_i64().ok_or_else(Fault::scope_rejected)?;
    if value.as_u64().is_none() || number < 0 {
        return Err(Fault::scope_rejected());
    }
    Ok(number)
}

pub fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn digest_ok(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_scope(scope: &Scope) -> Result<(), Fault> {
    if !ident_ok(&scope.principal_id)
        || !ident_ok(&scope.project_id)
        || !ident_ok(&scope.session_id)
        || !ident_ok(&scope.role)
        || !ident_ok(&scope.policy_id)
        || !ident_ok(&scope.knowledge_id)
        || !digest_ok(&scope.knowledge_digest)
        || scope.absolute_end_unix < 0
        || scope.revision < 0
    {
        return Err(Fault::scope_rejected());
    }
    Ok(())
}

pub fn read_key(path: &Path) -> Result<zeroize::Zeroizing<Vec<u8>>, Fault> {
    let bytes = read_regular(path, 32, Some(0o600))?;
    if bytes.len() != 32 {
        return Err(Fault::path());
    }
    Ok(zeroize::Zeroizing::new(bytes))
}

pub fn grant_key(name: &str) -> bool {
    matches!(
        name,
        "principal_id"
            | "project_id"
            | "session_id"
            | "role"
            | "policy_id"
            | "knowledge_id"
            | "knowledge_digest"
            | "grant"
            | "claims"
            | "identity"
            | "signed_url"
            | "url"
            | "token"
            | "absolute_end_unix"
            | "revision"
    )
}

pub fn scan_secrets(bytes: &[u8], key: &[u8]) -> bool {
    const MARKERS: &[&[u8]] = &[
        b"-----BEGIN PRIVATE KEY-----",
        b"-----BEGIN RSA PRIVATE KEY-----",
        b"-----BEGIN OPENSSH PRIVATE KEY-----",
    ];
    if key.len() == 32 && bytes.windows(32).any(|window| window == key) {
        return true;
    }
    if MARKERS.iter().any(|marker| bytes.windows(marker.len()).any(|window| window == *marker)) {
        return true;
    }
    bytes.windows(20).any(|window| {
        window.starts_with(b"AKIA")
            && window[4..].iter().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    })
}
