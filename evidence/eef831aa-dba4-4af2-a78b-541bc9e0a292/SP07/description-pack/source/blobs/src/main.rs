//! Stdin entry for one description-pack command.
//!
//! `description-pack apply` reads one command and writes the bridge document.
//! It does not call a model, open a network connection, read credentials, use
//! PostgreSQL, write files, grant, authenticate, or publish.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::ExitCode;

use description_pack::{DescriptionPackError, apply_description_pack};
use oo_core::limits::DOCUMENT_MAX_BYTES;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let phase = args.next();
    let extra = args.next().is_some();
    match (phase.as_deref(), extra) {
        (Some("apply"), false) => {}
        (Some("apply"), true) => {
            eprintln!("description-pack: the phase takes no arguments");
            return ExitCode::from(2);
        }
        _ => {
            usage();
            return ExitCode::from(2);
        }
    }
    let text = match read_stdin() {
        Ok(text) => text,
        Err(err) => return fail(&err),
    };
    match apply_description_pack(&text) {
        Ok(document) => write_stdout(&document),
        Err(err) => fail(&err),
    }
}

fn usage() {
    eprintln!("description-pack: usage: description-pack apply");
    eprintln!(
        "description-pack:   apply    one oo.builder.description-pack-command/v1 document on stdin"
    );
    eprintln!("description-pack: stdout is the bridge document and has no added newline");
    eprintln!(
        "description-pack: this command does not call a model, open a network connection, read credentials, use PostgreSQL, write files, grant, authenticate, or publish"
    );
}

fn write_stdout(document: &str) -> ExitCode {
    let mut out = std::io::stdout().lock();
    if out.write_all(document.as_bytes()).is_err() {
        return fail(&DescriptionPackError::named(
            "io.write",
            "stdout could not be written",
        ));
    }
    ExitCode::SUCCESS
}

fn fail(err: &DescriptionPackError) -> ExitCode {
    eprintln!("description-pack: {} {}", err.code, err.message);
    ExitCode::from(1)
}

fn read_stdin() -> Result<String, DescriptionPackError> {
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    let max = u64::try_from(DOCUMENT_MAX_BYTES.saturating_add(1)).unwrap_or(u64::MAX);
    stdin.by_ref().take(max).read_to_end(&mut buf).map_err(|_| {
        DescriptionPackError::named("io.read", "stdin could not be read")
    })?;
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(DescriptionPackError::named(
            "json.budget.bytes",
            "document exceeds the byte budget",
        ));
    }
    let text = std::str::from_utf8(&buf).map_err(|_| {
        DescriptionPackError::named("json.syntax", "stdin is not UTF-8")
    })?;
    Ok(text.to_string())
}
