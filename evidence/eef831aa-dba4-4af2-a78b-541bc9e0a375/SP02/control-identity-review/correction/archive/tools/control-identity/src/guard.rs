//! Management roles come from the operator file, not from the ID Token.

use serde_json::Value;

use crate::config::{Config, Role};
use crate::error::Fault;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedSession {
    subject: String,
    issuer: String,
    config_commit: String,
    revision: i64,
    initialized: bool,
}

impl VerifiedSession {
    pub(crate) fn new(subject: String, issuer: String, config_commit: String, revision: i64, initialized: bool) -> Self {
        Self { subject, issuer, config_commit, revision, initialized }
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn revision(&self) -> i64 {
        self.revision
    }

    pub fn try_from_caller_json(_value: &Value) -> Result<Self, Fault> {
        Err(Fault::unauthenticated())
    }
}

pub fn capabilities_for(role: &str) -> Option<&'static [&'static str]> {
    Some(match role {
        "admin" => &["binding.read", "identity.current", "project.admin", "session.revoke"],
        "auditor" => &["audit.read", "identity.current"],
        "business_owner" => &["business.record", "identity.current"],
        "reviewer" => &["identity.current", "review.decide"],
        "steward" => &["identity.current", "ontology.draft"],
        _ => return None,
    })
}

pub fn authorize(session: &VerifiedSession, config: &Config, project_id: &str, capability: &str, expected_revision: i64) -> Result<(), Fault> {
    if expected_revision != session.revision || session.revision != config.revision || session.config_commit != config.config_commit {
        return Err(Fault::revision());
    }
    if session.issuer != config.issuer {
        return Err(Fault::issuer());
    }
    if !session.initialized {
        return Err(Fault::forbidden());
    }
    let Some(binding) = config.bindings.iter().find(|item| item.project_id == project_id && item.subject == session.subject) else {
        return Err(Fault::forbidden());
    };
    let Some(capabilities) = capabilities_for(binding.role.as_str()) else {
        return Err(Fault::forbidden());
    };
    if capabilities.contains(&capability) { Ok(()) } else { Err(Fault::forbidden()) }
}

pub fn projects_for(config: &Config, subject: &str, initialized: bool) -> Vec<ProjectView> {
    if !initialized {
        return Vec::new();
    }
    let mut rows: Vec<_> = config
        .bindings
        .iter()
        .filter(|item| item.subject == subject)
        .map(|item| ProjectView {
            project_id: item.project_id.clone(),
            role: item.role,
            capabilities: capabilities_for(item.role.as_str()).unwrap_or(&[]).to_vec(),
        })
        .collect();
    rows.sort_by(|left, right| left.project_id.cmp(&right.project_id));
    rows
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectView {
    pub project_id: String,
    pub role: Role,
    pub capabilities: Vec<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::capabilities_for;
    use crate::error::Fault;
    use crate::guard::VerifiedSession;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn caller_json_is_never_a_session() {
        let value = json!({"authorized": true, "role": "admin"});
        assert_eq!(VerifiedSession::try_from_caller_json(&value), Err(Fault::unauthenticated()));
    }

    #[test]
    fn matrix_matches_the_frozen_file_and_is_disjoint() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/capability-matrix.json");
        let bytes = std::fs::read(path).expect("matrix");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        let roles = value.get("roles").and_then(|item| item.as_object()).expect("roles");
        assert_eq!(roles.len(), 5);
        let mut sets = Vec::new();
        for (role, listed) in roles {
            let expected = capabilities_for(role).expect("role");
            let from_file: Vec<_> = listed.as_array().expect("list").iter().map(|item| item.as_str().expect("cap")).collect();
            assert_eq!(from_file, expected);
            sets.push(BTreeSet::from_iter(expected.iter().copied()));
        }
        for (index, left) in sets.iter().enumerate() {
            for right in sets.iter().skip(index + 1) {
                assert!(!left.is_subset(right));
                assert!(!right.is_subset(left));
            }
        }
    }
}
