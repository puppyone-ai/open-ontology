//! Closed preview failures. Display text has no password and no cell value.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewError {
    pub code: &'static str,
    pub fact: &'static str,
}

impl PreviewError {
    pub fn new(code: &'static str, fact: &'static str) -> Self {
        Self { code, fact }
    }

    /// A missing config field keeps a closed fact. The caller's key is only
    /// a borrowed name, so it is projected onto this static list.
    pub fn config_field(key: &str) -> Self {
        Self { code: "config", fact: static_field(key) }
    }
}

fn static_field(key: &str) -> &'static str {
    const FIELDS: &[&str] = &[
        "admin_secret_file",
        "admin_secret_ref",
        "admin_user",
        "audit_secret_file",
        "audit_secret_ref",
        "audit_user",
        "approved_objects",
        "boolean_tinyint1",
        "budgets",
        "ca_file",
        "columns",
        "connection",
        "database",
        "dataset",
        "datasets",
        "host",
        "name",
        "objects",
        "password_file",
        "port",
        "project",
        "protocol",
        "reader",
        "reader_password_file",
        "role",
        "runtime_secret_ref",
        "runtime_user",
        "schema",
        "secret_file",
        "secret_ref",
        "stop",
        "table",
        "tables",
        "target",
        "type",
        "user",
        "version",
        "visibility_file",
    ];
    FIELDS.iter().copied().find(|item| *item == key).unwrap_or("key")
}

impl fmt::Display for PreviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.code, self.fact)
    }
}

impl std::error::Error for PreviewError {}

pub fn redact(text: &str, secret: &str) -> bool {
    !secret.is_empty() && text.contains(secret)
}
