use std::io::{Read, Write};
use std::process::ExitCode;

use oo_builder::{CapturedPgDraft, DraftError, compile_captured_pg_catalog, compile_structure_draft};
use oo_core::limits::DOCUMENT_MAX_BYTES;

const COMMAND_PROTOCOL: &str = "oo.builder.captured-pg-draft-command/v1";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => {
            eprintln!("oo-builder: process entry is not implemented (SP00-W01 skeleton)");
            eprintln!(
                "oo-builder: public draft commands read one JSON document from stdin and take no arguments"
            );
            eprintln!(
                "oo-builder:   structure-draft       one oo.builder.catalog-evidence/v1 envelope"
            );
            eprintln!("oo-builder:   captured-pg-draft     one oo.source.pgsql/v1 catalog");
            eprintln!(
                "oo-builder: curated overlays and review decisions stay on the library API"
            );
            eprintln!(
                "oo-builder: captured-pg-draft does not connect, call a model, import rows, grant, certify, or publish"
            );
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
                Ok(bytes) => {
                    let mut out = std::io::stdout().lock();
                    if out.write_all(bytes.as_bytes()).is_err() {
                        eprintln!(
                            "oo-builder: captured-pg-draft: oo.builder.catalog-evidence/v1/io.write stdout could not be written"
                        );
                        return ExitCode::from(1);
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("oo-builder: captured-pg-draft: {} {}", err.code, err.message);
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

fn read_and_compile() -> Result<String, DraftError> {
    compile_structure_draft(&read_stdin()?)
}

fn read_and_compile_captured() -> Result<String, DraftError> {
    let compiled = compile_captured_pg_catalog(&read_stdin()?, None, None)?;
    command_document(&compiled)
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
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| DraftError::new("schema.invalid", "/", message))?;
    if value.is_object() {
        Ok(())
    } else {
        Err(DraftError::new("schema.invalid", "/", message))
    }
}
