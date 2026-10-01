//! Machine-readable command result. Exit 0 is a successful check, not a phase sign-off.

use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Issue {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

impl Issue {
    pub fn new(code: &'static str, path: impl AsRef<str>, message: impl AsRef<str>) -> Self {
        Self {
            code,
            path: path.as_ref().to_owned(),
            message: message.as_ref().to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Report {
    pub command: String,
    pub status: String,
    pub exit_code: i32,
    pub issues: Vec<Issue>,
    pub details: Value,
}

impl Report {
    pub fn ok(command: impl Into<String>, status: impl Into<String>, details: Value) -> Self {
        Self {
            command: command.into(),
            status: status.into(),
            exit_code: 0,
            issues: Vec::new(),
            details,
        }
    }

    pub fn from_issues(
        command: impl Into<String>,
        ok_status: &str,
        fail_status: &str,
        exit_on_fail: i32,
        issues: Vec<Issue>,
        details: Value,
    ) -> Self {
        if issues.is_empty() {
            Self::ok(command, ok_status, details)
        } else {
            Self {
                command: command.into(),
                status: fail_status.to_owned(),
                exit_code: exit_on_fail,
                issues,
                details,
            }
        }
    }

    pub fn fail(
        command: impl Into<String>,
        status: impl Into<String>,
        exit_code: i32,
        issues: Vec<Issue>,
        details: Value,
    ) -> Self {
        Self {
            command: command.into(),
            status: status.into(),
            exit_code,
            issues,
            details,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "command": self.command,
            "status": self.status,
            "exit_code": self.exit_code,
            "phase_signoff": "in_review",
            "protected_github_policy": false,
            "issues": self.issues.iter().map(|issue| json!({
                "code": issue.code,
                "path": issue.path,
                "message": issue.message,
            })).collect::<Vec<_>>(),
            "details": self.details,
        })
    }

    pub fn render(&self, format: &str) -> String {
        if format == "json" {
            let mut text =
                serde_json::to_string_pretty(&self.to_json()).unwrap_or_else(|_| "{}".to_owned());
            text.push('\n');
            return text;
        }
        let mut lines = vec![format!(
            "xtask {} status={} exit={}",
            self.command, self.status, self.exit_code
        )];
        for issue in &self.issues {
            lines.push(format!(
                "  {}: {} {}",
                issue.code, issue.path, issue.message
            ));
        }
        if !self.details.is_null() {
            lines.push(self.details.to_string());
        }
        lines.push(String::new());
        lines.join("\n")
    }
}

pub const EXIT_OK: i32 = 0;
pub const EXIT_FAIL: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_PENDING: i32 = 3;
