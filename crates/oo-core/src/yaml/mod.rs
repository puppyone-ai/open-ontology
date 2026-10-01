//! Strict YAML front door.
//!
//! The byte budget is applied by the caller because `serde-saphyr` enforces
//! `max_reader_input_bytes` only for readers. Tags are rejected by
//! [`scan::scan_yaml`] before deserialization; `reject_unsupported_tags` does
//! not reject core tags such as `!!str`.

mod lexical;
mod scan;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::diagnostic::{Diagnostic, DiagnosticList};
use crate::limits::DOCUMENT_MAX_BYTES;

pub use scan::scan_yaml;

pub fn parse_yaml_value(input: &str, contract: &str) -> Result<Value, DiagnosticList> {
    check_budget(input, contract)?;
    if let Err(diagnostic) = scan_yaml(input, contract) {
        return Err(DiagnosticList::one(diagnostic));
    }
    lexical::parse_events(input, contract).map_err(DiagnosticList::one)
}

pub fn parse_yaml_typed<T: DeserializeOwned>(
    input: &str,
    contract: &str,
) -> Result<T, DiagnosticList> {
    let value = parse_yaml_value(input, contract)?;
    serde_json::from_value(value).map_err(|error| {
        DiagnosticList::one(Diagnostic::error(
            contract,
            "schema",
            "/",
            error.to_string(),
        ))
    })
}

fn check_budget(input: &str, contract: &str) -> Result<(), DiagnosticList> {
    if input.len() > DOCUMENT_MAX_BYTES {
        return Err(DiagnosticList::one(Diagnostic::error(
            contract,
            "yaml.budget.bytes",
            "/",
            format!(
                "document is {} bytes; limit is {DOCUMENT_MAX_BYTES}",
                input.len()
            ),
        )));
    }
    Ok(())
}
