use std::io::{Read, Write};
use std::process::ExitCode;

use oo_builder::{
    CapturedPgDraft, DraftError, EVIDENCE_PROTOCOL, compile_captured_pg_catalog,
    compile_structure_draft,
};
use oo_core::limits::DOCUMENT_MAX_BYTES;
use serde::Deserialize;
use serde::de::Error as SerdeError;
use serde_json::{Map, Value};

const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => {
            // The entry implements the three draft commands below. The old
            // SP00-W01 skeleton sentence said this entry was not implemented.
            usage();
            ExitCode::from(2)
        }
        Some("structure-draft") => {
            if args.next().is_some() {
                eprintln!(
                    "oo-builder: structure-draft reads JSON from stdin and takes no arguments"
                );
                return ExitCode::from(2);
            }
            match read_and_compile() {
                Ok(bytes) => {
                    let mut out = std::io::stdout().lock();
                    if out.write_all(bytes.as_bytes()).is_err() {
                        return ExitCode::from(1);
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("oo-builder: {err}");
                    ExitCode::from(1)
                }
            }
        }
        Some("captured-pg-draft") => {
            if args.next().is_some() {
                eprintln!(
                    "oo-builder: captured-pg-draft reads one oo.source.pgsql/v1 JSON document from stdin and takes no arguments"
                );
                return ExitCode::from(2);
            }
            match read_and_compile_captured() {
                Ok(bytes) => write_command("captured-pg-draft", &bytes),
                Err(err) => {
                    eprintln!("oo-builder: captured-pg-draft: {} {}", err.code, err.message);
                    ExitCode::from(1)
                }
            }
        }
        Some("review-pg-draft") => {
            if args.next().is_some() {
                eprintln!(
                    "oo-builder: review-pg-draft reads one JSON document from stdin and takes no arguments"
                );
                return ExitCode::from(2);
            }
            match read_and_compile_review() {
                Ok(bytes) => write_command("review-pg-draft", &bytes),
                Err(err) => {
                    eprintln!("oo-builder: review-pg-draft: {} {}", err.code, err.message);
                    ExitCode::from(1)
                }
            }
        }
        Some(_) => {
            eprintln!("oo-builder: unknown command");
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("oo-builder: usage: oo-builder <command>");
    eprintln!(
        "oo-builder: public draft commands read one JSON document from stdin and take no arguments"
    );
    eprintln!("oo-builder:   structure-draft       one oo.builder.catalog-evidence/v1 envelope");
    eprintln!("oo-builder:   captured-pg-draft     one oo.source.pgsql/v1 catalog");
    eprintln!(
        "oo-builder:   review-pg-draft       catalog object, optional curated object, optional reviews array"
    );
    eprintln!(
        "oo-builder: review-pg-draft passes those values to compile_captured_pg_catalog"
    );
    eprintln!(
        "oo-builder: these commands do not connect, call a model, import rows, grant, certify, or publish"
    );
    eprintln!(
        "oo-builder: a review decision binds one structural item and does not approve semantics, grants, or publication"
    );
}

fn write_command(command: &str, bytes: &str) -> ExitCode {
    let mut out = std::io::stdout().lock();
    if out.write_all(bytes.as_bytes()).is_err() {
        eprintln!(
            "oo-builder: {command}: {EVIDENCE_PROTOCOL}/io.write stdout could not be written"
        );
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn read_and_compile() -> Result<String, DraftError> {
    compile_structure_draft(&read_stdin()?)
}

fn read_and_compile_captured() -> Result<String, DraftError> {
    let compiled = compile_captured_pg_catalog(&read_stdin()?, None, None)?;
    command_document(&compiled)
}

fn read_and_compile_review() -> Result<String, DraftError> {
    let text = read_stdin()?;
    let value = oo_core::parse_json_value(&text, EVIDENCE_PROTOCOL).map_err(map_scan)?;
    let document: ReviewPgDocument =
        serde_json::from_value(value).map_err(map_review_document)?;
    if !document.catalog.is_object() {
        return Err(DraftError::new(
            "schema.invalid",
            "/catalog",
            "catalog is not an object",
        ));
    }
    let catalog = canonical_json(&document.catalog);
    let curated = document.curated.as_ref().map(canonical_json);
    let reviews = document.reviews.as_ref().map(canonical_json);
    let compiled = compile_captured_pg_catalog(&catalog, curated.as_deref(), reviews.as_deref())?;
    command_document(&compiled)
}

fn map_review_document(err: serde_json::Error) -> DraftError {
    let text = err.to_string();
    let (name, message) = if text.contains("unknown field") {
        ("field.unknown", "review document has an unknown field")
    } else if text.contains("missing field") {
        ("field.missing", "review document is missing a required field")
    } else {
        (
            "schema.invalid",
            "review document is not catalog with optional curated and reviews",
        )
    };
    DraftError::new(name, "/", message)
}

fn map_scan(err: oo_core::DiagnosticList) -> DraftError {
    let item = err.items.first();
    let code = item
        .map(|diag| diag.code.clone())
        .unwrap_or_else(|| format!("{EVIDENCE_PROTOCOL}/json.syntax"));
    let path = item
        .map(|diag| diag.path.clone())
        .unwrap_or_else(|| "/".to_string());
    let message = if code.ends_with("/json.budget.bytes") {
        "document exceeds the byte budget"
    } else if code.ends_with("/json.budget.depth") {
        "JSON nesting exceeds the depth budget"
    } else if code.ends_with("/json.budget.nodes") {
        "JSON node count exceeds the budget"
    } else if code.ends_with("/json.duplicate_key") {
        "duplicate JSON object key"
    } else {
        "review document JSON is not valid"
    };
    DraftError::from_full(code, path, message)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewPgDocument {
    catalog: Value,
    #[serde(default, deserialize_with = "optional_object")]
    curated: Option<Value>,
    #[serde(default, deserialize_with = "optional_array")]
    reviews: Option<Value>,
}

fn optional_object<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    require_shape(deserializer, Value::is_object, "curated is not an object")
}

fn optional_array<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    require_shape(deserializer, Value::is_array, "reviews is not an array")
}

fn require_shape<'de, D>(
    deserializer: D,
    shape: fn(&Value) -> bool,
    message: &'static str,
) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if shape(&value) {
        Ok(Some(value))
    } else {
        Err(SerdeError::custom(message))
    }
}

fn read_stdin() -> Result<String, DraftError> {
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    let max = DOCUMENT_MAX_BYTES.saturating_add(1);
    stdin
        .by_ref()
        .take(max as u64)
        .read_to_end(&mut buf)
        .map_err(|_| DraftError::new("io.read", "/", "stdin could not be read"))?;
    let text = std::str::from_utf8(&buf)
        .map_err(|_| DraftError::new("json.syntax", "/", "stdin is not UTF-8"))?;
    Ok(text.to_string())
}

/// One command result. The three compiler strings are embedded as JSON objects.
/// The whole document is parsed before the caller writes stdout.
fn command_document(compiled: &CapturedPgDraft) -> Result<String, DraftError> {
    require_object(&compiled.wrapper, "compiled wrapper is not an object")?;
    require_object(
        &compiled.evidence_envelope,
        "compiled evidence envelope is not an object",
    )?;
    require_object(&compiled.draft, "compiled draft is not an object")?;
    let document = format!(
        "{{\"draft\":{},\"evidence_envelope\":{},\"protocol\":\"{COMMAND_PROTOCOL}\",\"wrapper\":{}}}",
        compiled.draft, compiled.evidence_envelope, compiled.wrapper
    );
    require_object(&document, "command result is not an object")?;
    Ok(document)
}

fn require_object(text: &str, message: &'static str) -> Result<(), DraftError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| DraftError::new("schema.invalid", "/", message))?;
    if value.is_object() {
        Ok(())
    } else {
        Err(DraftError::new("schema.invalid", "/", message))
    }
}

/// Key-sorted JSON for the catalog, curated, and reviews values. This is the
/// extracted input, not a second draft compiler. `compile_captured_pg_catalog`
/// recomputes every hash.
fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => push_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => write_object(out, map),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>) {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    out.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_string(out, key);
        out.push(':');
        write_value(out, &map[*key]);
    }
    out.push('}');
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}
