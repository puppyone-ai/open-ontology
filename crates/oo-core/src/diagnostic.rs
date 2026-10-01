//! Versioned diagnostics shared by the strict parser and validators.
//!
//! Codes look like `oo.config/v1/yaml.tag`. Severity `error` rejects the
//! document. Line and column are 1-based when the source location is known.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const CONTRACT_CONFIG: &str = "oo.config/v1";
pub const CONTRACT_OOS: &str = "oo.oos/v1";
pub const CONTRACT_BATCH: &str = "oo.batch/v1";
pub const CONTRACT_API: &str = "oo.api/v1";
pub const CONTRACT_CANON: &str = "oo.canonical/v1";

pub const CONFIG_SCHEMA_VERSION: &str = "config/v1";
pub const OOS_SCHEMA_VERSION: &str = "oos/v1";
pub const BATCH_PROTOCOL_VERSION: &str = "batch/v1";
pub const ERROR_SCHEMA_VERSION: &str = "error/v1";
pub const API_VERSION: &str = "api/v1";
pub const CANONICAL_VALUE_VERSION: &str = "canonical-value/v1";
pub const CANONICAL_PK_VERSION: &str = "canonical-pk/v1";
pub const TYPE_POLICY_V1: &str = "type-policy/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

impl Diagnostic {
    pub fn error(
        contract: &str,
        name: &str,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: format!("{contract}/{name}"),
            severity: Severity::Error,
            path: path.into(),
            message: message.into(),
            line: None,
            column: None,
        }
    }

    pub fn at(mut self, line: u32, column: u32) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticList {
    pub items: Vec<Diagnostic>,
}

impl DiagnosticList {
    pub fn new(items: Vec<Diagnostic>) -> Self {
        Self { items }
    }

    pub fn one(item: Diagnostic) -> Self {
        Self { items: vec![item] }
    }
}

impl fmt::Display for DiagnosticList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(
                f,
                "{} {} {} {}",
                item.code,
                item.path,
                item.severity_label(),
                item.message
            )?;
            if let (Some(line), Some(column)) = (item.line, item.column) {
                write!(f, " ({line}:{column})")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for DiagnosticList {}

impl Severity {
    fn severity_label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

impl Diagnostic {
    fn severity_label(&self) -> &'static str {
        self.severity.severity_label()
    }
}
