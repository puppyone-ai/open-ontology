//! Read the support matrix and admit or reject a source registration.
//! This crate is not the `oo` CLI and it does not submit a source-control registration.

use std::fs;
use std::path::Path;

use json::Json;
use output::Outcome;

pub mod admit;
pub mod downgrade;
pub mod evidence;
pub mod json;
pub mod matrix;
pub mod output;
pub mod register;
pub mod sha256;
pub mod yaml;

pub use admit::{check, decide, supported};
pub use downgrade::downgrade;
pub use output::CONTRACT;

struct Flags {
    matrix: Option<String>,
    request: Option<String>,
    event: Option<String>,
    evidence: Option<String>,
    doc: Option<String>,
    schema: Option<String>,
    allow_private: bool,
}

pub fn execute(args: &[String]) -> Outcome {
    let Some(operation) = operation_name(args.get(1).map(String::as_str)) else {
        return usage("usage");
    };
    let flags = match parse_flags(&args[2..]) {
        Ok(flags) => flags,
        Err(reason) => return usage(reason),
    };
    if flags.matrix.is_none() || !flags_match(operation, &flags) {
        return Outcome::new(operation, false, 1, "malformed", "usage", "");
    }
    let matrix_path = flags.matrix.as_deref().unwrap_or_default();
    let matrix_text = match fs::read_to_string(matrix_path) {
        Ok(text) => text,
        Err(_) => return Outcome::new(operation, false, 1, "malformed", "matrix", ""),
    };
    let matrix = match matrix::load(&matrix_text) {
        Ok(matrix) => matrix,
        Err(_) => return Outcome::new(operation, false, 1, "malformed", "yaml", &matrix_text),
    };
    match operation {
        "supported" => supported(&matrix_text, &matrix),
        "check" => {
            let doc = match read_text(flags.doc.as_deref(), "doc", operation, &matrix_text) {
                Ok(text) => text,
                Err(outcome) => return outcome,
            };
            let evidence = flags.evidence.as_deref().map(Path::new);
            check(&matrix_text, &matrix, &doc, evidence, !flags.allow_private)
        }
        "select" | "admit" => {
            let request = match read_json(flags.request.as_deref(), "request", operation, &matrix_text) {
                Ok(value) => value,
                Err(outcome) => return outcome,
            };
            let schema = if operation == "admit" {
                match read_json(flags.schema.as_deref(), "schema", operation, &matrix_text) {
                    Ok(value) => Some(value),
                    Err(outcome) => return outcome,
                }
            } else {
                None
            };
            let evidence = flags.evidence.as_deref().map(Path::new);
            decide(operation, &matrix_text, &matrix, &request, evidence, schema.as_ref())
        }
        "downgrade" => {
            let event = match read_json(flags.event.as_deref(), "event", operation, &matrix_text) {
                Ok(value) => value,
                Err(outcome) => return outcome,
            };
            downgrade(&matrix_text, &matrix, &event)
        }
        _ => usage("usage"),
    }
}

fn operation_name(name: Option<&str>) -> Option<&'static str> {
    match name {
        Some("supported") => Some("supported"),
        Some("select") => Some("select"),
        Some("admit") => Some("admit"),
        Some("check") => Some("check"),
        Some("downgrade") => Some("downgrade"),
        _ => None,
    }
}

fn flags_match(operation: &str, flags: &Flags) -> bool {
    let request = flags.request.is_some();
    let event = flags.event.is_some();
    let doc = flags.doc.is_some();
    let schema = flags.schema.is_some();
    match operation {
        "supported" => !request && !event && !doc && !schema && !flags.allow_private && flags.evidence.is_none(),
        "select" => request && !event && !doc && !schema && !flags.allow_private,
        "admit" => request && schema && !event && !doc && !flags.allow_private,
        "check" => doc && !request && !event && !schema,
        "downgrade" => event && !request && !doc && !schema && !flags.allow_private && flags.evidence.is_none(),
        _ => false,
    }
}

fn parse_flags(args: &[String]) -> Result<Flags, &'static str> {
    let mut flags = Flags {
        matrix: None,
        request: None,
        event: None,
        evidence: None,
        doc: None,
        schema: None,
        allow_private: false,
    };
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        if name == "--allow-private" {
            if flags.allow_private {
                return Err("usage");
            }
            flags.allow_private = true;
            index += 1;
            continue;
        }
        let value = args.get(index + 1).ok_or("usage")?;
        if value.starts_with("--") {
            return Err("usage");
        }
        let slot = match name {
            "--matrix" => &mut flags.matrix,
            "--request" => &mut flags.request,
            "--event" => &mut flags.event,
            "--evidence" => &mut flags.evidence,
            "--doc" => &mut flags.doc,
            "--schema" => &mut flags.schema,
            _ => return Err("usage"),
        };
        if slot.is_some() {
            return Err("usage");
        }
        *slot = Some(value.clone());
        index += 2;
    }
    Ok(flags)
}

fn usage(reason: &'static str) -> Outcome {
    Outcome::new("usage", false, 1, "malformed", reason, "")
}

fn read_text(path: Option<&str>, reason: &'static str, operation: &'static str, matrix_text: &str) -> Result<String, Outcome> {
    let Some(path) = path else {
        return Err(Outcome::new(operation, false, 1, "malformed", reason, matrix_text));
    };
    fs::read_to_string(path).map_err(|_| Outcome::new(operation, false, 1, "malformed", reason, matrix_text))
}

fn read_json(path: Option<&str>, reason: &'static str, operation: &'static str, matrix_text: &str) -> Result<Json, Outcome> {
    let text = read_text(path, reason, operation, matrix_text)?;
    json::parse(text.as_bytes()).map_err(|error| Outcome::new(operation, false, 1, "malformed", error.detail, matrix_text))
}
