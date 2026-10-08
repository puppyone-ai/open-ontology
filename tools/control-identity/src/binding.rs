//! Management-role plan. The plan does not write and does not read caller authority.

use crate::config::{management_document, Binding, Config, Role};
use crate::crypto::sha256_hex;
use crate::error::Fault;

pub struct Change {
    pub effect: String,
    pub project_id: String,
    pub subject: String,
    pub role: Option<Role>,
    pub expected_revision: i64,
    pub expected_config_commit: String,
    pub expected_git_head: String,
    pub idempotency_key: String,
}

pub struct Plan {
    pub outcome: &'static str,
    pub effect: String,
    pub project_id: String,
    pub subject: String,
    pub role: Option<Role>,
    pub next_revision: i64,
    pub next_config_commit: String,
    pub document: String,
    pub document_sha256: String,
}

pub fn request_sha256(change: &Change) -> Result<String, Fault> {
    let role = change.role.map(Role::as_str).unwrap_or("");
    let value = serde_json::json!({
        "effect": change.effect,
        "expected_config_commit": change.expected_config_commit,
        "expected_git_head": change.expected_git_head,
        "expected_revision": change.expected_revision,
        "idempotency_key": change.idempotency_key,
        "project_id": change.project_id,
        "protocol": "oo.identity-binding/approve/v1",
        "role": role,
        "subject": change.subject
    });
    let text = serde_json::to_string(&value).map_err(|_| Fault::malformed())?;
    Ok(sha256_hex(text.as_bytes()))
}

pub fn plan(config: &Config, caller: &str, current: &[Binding], revision: i64, config_commit: &str, change: &Change) -> Result<Plan, Fault> {
    if change.expected_revision != revision || change.expected_config_commit != config_commit {
        return Err(Fault::revision());
    }
    if !key_ok(&change.idempotency_key) || !hex40_or_empty(&change.expected_git_head) {
        return Err(Fault::malformed());
    }
    let caller_binding = current.iter().find(|item| item.subject == caller && item.project_id == change.project_id);
    if !caller_binding.is_some_and(|item| item.role == Role::Admin) {
        return Err(Fault::forbidden());
    }
    if change.subject == caller {
        return Err(Fault::elevation());
    }
    if change.subject == config.bootstrap.subject && change.project_id == config.bootstrap.project_id {
        return Err(Fault::frozen());
    }
    if !crate::crypto::subject_ok(&change.subject) || !project_ok(&change.project_id) {
        return Err(Fault::malformed());
    }
    let mut next = current.to_vec();
    let existing = next.iter().position(|item| item.project_id == change.project_id && item.subject == change.subject);
    let changed = match change.effect.as_str() {
        "grant" => {
            let Some(role) = change.role else {
                return Err(Fault::malformed());
            };
            match existing {
                Some(index) if next[index].role == role => false,
                Some(index) => {
                    next[index].role = role;
                    true
                }
                None => {
                    next.push(Binding { subject: change.subject.clone(), project_id: change.project_id.clone(), role });
                    true
                }
            }
        }
        "revoke" => {
            if change.role.is_some() {
                return Err(Fault::malformed());
            }
            let Some(index) = existing else {
                return Err(Fault::malformed());
            };
            next.remove(index);
            true
        }
        _ => return Err(Fault::malformed()),
    };
    if next.len() > 64 {
        return Err(Fault::malformed());
    }
    if !next.iter().any(|item| item == &config.bootstrap) {
        return Err(Fault::frozen());
    }
    let next_revision = if changed { revision.checked_add(1).ok_or_else(Fault::malformed)? } else { revision };
    if !(1..=1_000_000_000).contains(&next_revision) {
        return Err(Fault::malformed());
    }
    let next_config_commit = if changed {
        crate::config::canonical_commit(
            &config.issuer,
            &config.audience,
            &config.client_id,
            &config.redirect_uris,
            &next,
            &config.bootstrap,
            next_revision,
        )?
    } else {
        config_commit.to_string()
    };
    let document = management_document(&next)?;
    let document_sha256 = sha256_hex(document.as_bytes());
    Ok(Plan {
        outcome: if changed { "planned" } else { "unchanged" },
        effect: change.effect.clone(),
        project_id: change.project_id.clone(),
        subject: change.subject.clone(),
        role: change.role,
        next_revision,
        next_config_commit,
        document,
        document_sha256,
    })
}

pub fn key_ok(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

pub fn hex40(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hex40_or_empty(value: &str) -> bool {
    value.is_empty() || hex40(value)
}

fn project_ok(value: &str) -> bool {
    let mut bytes = value.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() => {
            value.len() <= 63 && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        }
        _ => false,
    }
}

pub fn receipt_sha256(outcome: &str, revision: i64, config_commit: &str, git_commit: &str, document_sha256: &str) -> Result<String, Fault> {
    let value = serde_json::json!({
        "config_commit": config_commit,
        "document_sha256": document_sha256,
        "git_commit": git_commit,
        "operation": "approveManagementBinding",
        "outcome": outcome,
        "protocol": "oo.control-identity/binding-receipt/v1",
        "revision": revision
    });
    let text = serde_json::to_string(&value).map_err(|_| Fault::malformed())?;
    Ok(sha256_hex(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::request_sha256;
    use crate::binding::Change;
    use crate::config::{management_document, Binding, Role};

    #[test]
    fn management_document_is_sorted_compact_json() {
        let rows = vec![
            Binding { subject: "zed".to_string(), project_id: "beta".to_string(), role: Role::Auditor },
            Binding { subject: "fixture-admin".to_string(), project_id: "alpha".to_string(), role: Role::Admin },
        ];
        let text = management_document(&rows).expect("document");
        assert_eq!(
            text,
            concat!(
                r#"{"bindings":["#,
                r#"{"project_id":"alpha","role":"admin","subject":"fixture-admin"},"#,
                r#"{"project_id":"beta","role":"auditor","subject":"zed"}"#,
                r#"],"protocol":"oo.identity-binding/document/v1"}"#
            )
        );
    }

    #[test]
    fn request_hash_ignores_the_git_result() {
        let change = Change {
            effect: "grant".to_string(),
            project_id: "alpha".to_string(),
            subject: "person-reviewer".to_string(),
            role: Some(Role::Reviewer),
            expected_revision: 1,
            expected_config_commit: "ab".repeat(32),
            expected_git_head: "cd".repeat(20),
            idempotency_key: "grant-reviewer-1".to_string(),
        };
        let hash = request_sha256(&change).expect("hash");
        assert_eq!(hash.len(), 64);
        assert_eq!(request_sha256(&change).expect("stable"), hash);
    }
}
