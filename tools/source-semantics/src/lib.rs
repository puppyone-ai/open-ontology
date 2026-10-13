//! Normalize source semantics and render them through the role pack.
//!
//! `oo.source.pgsql/v1` still uses the captured PostgreSQL compiler.
//! `oo.pipeline.sealed-import-catalog/v1` uses `compile_sealed_import_catalog`
//! and `compile_reviewed_sealed_import`. Target facts carry the source family.
//! Source consistency is T, S1, S0, or B.
//! The effective dataset stores `source_semantics`, and the role-pack
//! renderer emits `## 来源与语义` plus the PostgreSQL dialect sentence.
//! Gateway `get_freshness`, live database reads, and binary-sort proof stay
//! pending. This crate does not open a database.

mod dialect;
mod pack;
mod semantics;

use serde::Deserialize;
use serde_json::Value;

use pack::Binding;
use semantics::Target;

pub const COMMAND_PROTOCOL: &str = "oo.source-semantics.command/v1";
pub const RESULT_PROTOCOL: &str = "oo.source-semantics.result/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticsError {
    pub code: String,
    pub path: String,
    pub message: String,
}

impl SemanticsError {
    pub fn new(code: impl Into<String>, path: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        let code = if code.contains('/') {
            code
        } else {
            format!("{RESULT_PROTOCOL}/{code}")
        };
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SemanticsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} at {}: {}", self.code, self.path, self.message)
    }
}

impl std::error::Error for SemanticsError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    protocol: String,
    catalog: Value,
    target: Target,
    #[serde(default)]
    binding: Option<Binding>,
    /// Full BSON document. Absent means the jsonb column has no document payload.
    #[serde(default)]
    document: Option<Value>,
    /// Dictionary rows and source identity. Absent keeps the hand-boolean path.
    #[serde(default)]
    source_bound: Option<Value>,
    /// Optional claim that must equal the derived column flags.
    #[serde(default)]
    asserted_semantics: Option<Value>,
}

/// Normalize one command and, when nothing blocks, compile a role pack.
///
/// A block still proves the catalog through the compiler for its protocol.
/// The returned text is one JSON object. `packed` includes the parsed table
/// pages and `examples/verified.sql`. `blocked` has `pack: null`.
pub fn compile_source_semantics(command_json: &str) -> Result<String, SemanticsError> {
    let command: Command = serde_json::from_str(command_json).map_err(|err| map_de(err, "/"))?;
    if command.protocol != COMMAND_PROTOCOL {
        return Err(SemanticsError::new(
            "protocol.mismatch",
            "/protocol",
            "command protocol is not oo.source-semantics.command/v1",
        ));
    }
    let catalog_protocol = command
        .catalog
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("");
    if command.source_bound.is_some()
        && catalog_protocol != oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL
    {
        return Err(SemanticsError::new(
            "protocol.mismatch",
            "/source_bound",
            "source-bound observation requires a sealed import catalog",
        ));
    }
    if command.asserted_semantics.is_some() && command.source_bound.is_none() {
        return Err(SemanticsError::new(
            "field.missing",
            "/source_bound",
            "asserted semantics require a source-bound observation",
        ));
    }
    let normalized = if command.source_bound.is_some() {
        semantics::normalize_bound(&command.target)?
    } else {
        semantics::normalize(&command.target)?
    };
    let result = if catalog_protocol == "oo.source.pgsql/v1" {
        let prepared = pack::prove_catalog(&command.catalog)?;
        pack::pack_or_block(
            &command.target,
            command.binding.as_ref(),
            &normalized,
            &prepared,
        )?
    } else if catalog_protocol == oo_core::sealed_import::SEALED_IMPORT_CATALOG_PROTOCOL {
        pack::compile_sealed(
            &command.target,
            command.binding.as_ref(),
            normalized,
            &command.catalog,
            command.document.as_ref(),
            command.source_bound.as_ref(),
            command.asserted_semantics.as_ref(),
        )?
    } else {
        return Err(SemanticsError::new(
            "protocol.mismatch",
            "/catalog/protocol",
            "catalog protocol is not a sealed import or oo.source.pgsql/v1",
        ));
    };
    serde_json::to_string(&result).map_err(|_| {
        SemanticsError::new("schema.invalid", "/", "result is not JSON")
    })
}

fn map_de(err: serde_json::Error, path: &str) -> SemanticsError {
    let text = err.to_string();
    let (code, message) = if text.contains("unknown field") {
        ("field.unknown", "command has an unknown field")
    } else if text.contains("missing field") {
        ("field.missing", "command is missing a required field")
    } else {
        ("schema.invalid", "command JSON is not valid")
    };
    SemanticsError::new(code, path, message)
}
