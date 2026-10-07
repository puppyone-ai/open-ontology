//! Platform control file and private credential. Roles are not request fields.

use serde::Deserialize;
use serde_json::Value;

use crate::canonical::sha256_hex;
use crate::error::{err, LedgerError};

pub const CONTROL_PROTOCOL: &str = "oo.review-ledger.control/v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlFile {
    protocol: String,
    project_id: String,
    authority: String,
    management_identity: String,
    principals: Vec<PrincipalFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrincipalFile {
    principal_id: String,
    role: String,
    reviewer_label: String,
    credential_sha256: String,
    scope: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Principal {
    pub principal_id: String,
    pub role: String,
    pub reviewer_label: String,
    pub credential_sha256: String,
    pub scope: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Control {
    pub project_id: String,
    pub sha256: String,
    pub principals: Vec<Principal>,
}

pub fn parse_control(bytes: &[u8]) -> Result<Control, LedgerError> {
    if bytes.len() > 65_536 {
        return Err(err("io.budget", "the control file exceeds the byte budget"));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| err("schema.invalid", "the control file is not JSON"))?;
    reject_control_shape(&value)?;
    let file: ControlFile = serde_json::from_value(value)
        .map_err(|_| err("schema.invalid", "the control file is not the control shape"))?;
    if file.protocol != CONTROL_PROTOCOL {
        return Err(err("protocol.mismatch", "the control protocol is not the review ledger control"));
    }
    if file.authority != "not_verified" {
        return Err(err(
            "authority.not_verified",
            "verified management authority is not implemented",
        ));
    }
    if file.management_identity != "not_claimed" {
        return Err(err(
            "authority.not_verified",
            "public management identity is not claimed",
        ));
    }
    if !ident_ok(&file.project_id) {
        return Err(err("schema.invalid", "the project id is not a single identifier"));
    }
    if file.principals.is_empty() || file.principals.len() > 16 {
        return Err(err("control.role", "the control file needs one to sixteen principals"));
    }
    let mut principals = Vec::with_capacity(file.principals.len());
    for principal in file.principals {
        principals.push(check_principal(principal)?);
    }
    if duplicate(&principals, |item| item.principal_id.as_str())
        || duplicate(&principals, |item| item.credential_sha256.as_str())
    {
        return Err(err("control.role", "a principal or credential is repeated"));
    }
    Ok(Control {
        project_id: file.project_id,
        sha256: sha256_hex(bytes)?,
        principals,
    })
}

pub fn authenticate<'a>(control: &'a Control, credential: &[u8]) -> Result<&'a Principal, LedgerError> {
    if credential.is_empty() || credential.len() > 256 || credential.iter().any(|byte| !(0x21..=0x7e).contains(byte)) {
        return Err(err("credential.mismatch", "the caller credential does not match a principal"));
    }
    let digest = sha256_hex(credential)?;
    let mut found = None;
    for principal in &control.principals {
        if principal.credential_sha256 == digest {
            if found.is_some() {
                return Err(err("control.role", "a credential matches more than one principal"));
            }
            found = Some(principal);
        }
    }
    found.ok_or_else(|| err("credential.mismatch", "the caller credential does not match a principal"))
}

pub fn in_scope(principal: &Principal, object_id: &str) -> bool {
    principal.scope.iter().any(|item| item == object_id)
}

fn check_principal(principal: PrincipalFile) -> Result<Principal, LedgerError> {
    if !principal_ok(&principal.principal_id) || !label_ok(&principal.reviewer_label) {
        return Err(err("schema.invalid", "a principal identifier is not allowed"));
    }
    if !matches!(principal.role.as_str(), "reviewer" | "owner" | "auditor") {
        return Err(err("control.role", "a principal role is not a single review role"));
    }
    if !hex_ok(&principal.credential_sha256) {
        return Err(err("schema.invalid", "a credential hash is not 64 lowercase hex characters"));
    }
    if principal.scope.is_empty() || principal.scope.len() > 64 {
        return Err(err("schema.invalid", "a principal scope is empty or too long"));
    }
    for object_id in &principal.scope {
        if !object_id_ok(object_id) {
            return Err(err("schema.invalid", "a scope object id is not allowed"));
        }
    }
    Ok(Principal {
        principal_id: principal.principal_id,
        role: principal.role,
        reviewer_label: principal.reviewer_label,
        credential_sha256: principal.credential_sha256,
        scope: principal.scope,
    })
}

fn reject_control_shape(value: &Value) -> Result<(), LedgerError> {
    let Some(object) = value.as_object() else {
        return Err(err("schema.invalid", "the control file is not an object"));
    };
    if object.contains_key("roles") || object.contains_key("grant") || object.contains_key("grants") {
        return Err(err("not_grant", "the control file cannot grant a role"));
    }
    let Some(principals) = object.get("principals").and_then(Value::as_array) else {
        return Err(err("schema.invalid", "the control file has no principal list"));
    };
    for principal in principals {
        let Some(one) = principal.as_object() else {
            return Err(err("schema.invalid", "a principal is not an object"));
        };
        if one.contains_key("roles") || one.contains_key("grant") || one.contains_key("grants") {
            return Err(err("control.role", "a principal cannot hold more than one role"));
        }
        if !one.contains_key("role") {
            return Err(err("control.role", "a principal needs exactly one role"));
        }
    }
    Ok(())
}

fn duplicate(principals: &[Principal], key: impl Fn(&Principal) -> &str) -> bool {
    let mut seen = Vec::new();
    for principal in principals {
        let value = key(principal);
        if seen.contains(&value) {
            return true;
        }
        seen.push(value);
    }
    false
}

pub fn ident_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

pub fn principal_ok(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.chars().count() <= 63
        && (first.is_ascii_alphabetic())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
}

pub fn object_id_ok(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    (parts.len() == 2 || parts.len() == 3) && parts.iter().all(|part| ident_ok(part))
}

pub fn label_ok(value: &str) -> bool {
    let count = value.chars().count();
    (1..=256).contains(&count)
        && value.chars().all(|ch| {
            !matches!(
                ch,
                '\u{0000}'..='\u{0008}'
                    | '\u{000b}'
                    | '\u{000c}'
                    | '\u{000e}'..='\u{001f}'
                    | '\u{007f}'
            )
        })
}

pub fn hex_ok(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub fn key_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

pub fn civil_date_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let year: i32 = match value[0..4].parse() {
        Ok(year) => year,
        Err(_) => return false,
    };
    let month: u32 = match value[5..7].parse() {
        Ok(month) => month,
        Err(_) => return false,
    };
    let day: u32 = match value[8..10].parse() {
        Ok(day) => day,
        Err(_) => return false,
    };
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    (1..=max).contains(&day) && (1..=9999).contains(&year)
}

pub fn kind_matches(kind: &str, object_id: &str) -> bool {
    let dots = object_id.matches('.').count();
    (kind == "dataset" && dots == 1) || (kind == "column" && dots == 2)
}
