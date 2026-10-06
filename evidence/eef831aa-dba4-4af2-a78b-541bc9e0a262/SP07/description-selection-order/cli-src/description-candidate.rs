//! Stdin entry for description-candidate prepare and complete.
//!
//! The operator names `prepare_request` or `complete_candidates`. This
//! process calls `prepare_description_request` or
//! `complete_description_candidates` and writes that function's document.
//! It does not call a model, open a network connection, read credentials,
//! use PostgreSQL, write curated or effective text, grant, certify,
//! authenticate, or publish.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::ExitCode;

use oo_builder::{
    DescriptionCandidateError, complete_description_candidates, prepare_description_request,
};
use oo_core::limits::DOCUMENT_MAX_BYTES;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let phase = args.next();
    let extra = args.next().is_some();
    match (phase.as_deref(), extra) {
        (Some("prepare_request"), false) => dispatch(prepare_description_request),
        (Some("complete_candidates"), false) => dispatch(complete_description_candidates),
        (Some("prepare_request" | "complete_candidates"), true) => {
            eprintln!("description-candidate: the phase takes no arguments");
            ExitCode::from(2)
        }
        (Some(_), _) => {
            eprintln!("description-candidate: unknown phase");
            ExitCode::from(2)
        }
        (None, _) => {
            usage();
            ExitCode::from(2)
        }
    }
}

fn usage() {
    eprintln!("description-candidate: usage: description-candidate <phase>");
    eprintln!(
        "description-candidate:   prepare_request        one oo.builder.description-request-command/v1 document on stdin, without captured_response"
    );
    eprintln!(
        "description-candidate:   complete_candidates    the same document plus captured_response"
    );
    eprintln!("description-candidate: the phase is required and is not inferred");
    eprintln!(
        "description-candidate: stdout is the public compiler document and has no added newline"
    );
    eprintln!(
        "description-candidate: this command does not call a model, open a network connection, read credentials, use PostgreSQL, write curated or effective text, grant, certify, authenticate, or publish"
    );
}

fn dispatch(run: fn(&str) -> Result<String, DescriptionCandidateError>) -> ExitCode {
    let text = match read_stdin() {
        Ok(text) => text,
        Err(err) => return fail(&err),
    };
    match run(&text) {
        Ok(document) => {
            let mut out = std::io::stdout().lock();
            if out.write_all(document.as_bytes()).is_err() {
                return fail(&DescriptionCandidateError::command(
                    "io.write",
                    "/",
                    "stdout could not be written",
                ));
            }
            ExitCode::SUCCESS
        }
        Err(err) => fail(&err),
    }
}

fn fail(err: &DescriptionCandidateError) -> ExitCode {
    eprintln!("description-candidate: {} {}", err.code, err.message);
    ExitCode::from(1)
}

fn read_stdin() -> Result<String, DescriptionCandidateError> {
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    let max = u64::try_from(DOCUMENT_MAX_BYTES.saturating_add(1)).unwrap_or(u64::MAX);
    stdin.by_ref().take(max).read_to_end(&mut buf).map_err(|_| {
        DescriptionCandidateError::command("io.read", "/", "stdin could not be read")
    })?;
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(DescriptionCandidateError::command(
            "json.budget.bytes",
            "/",
            "document exceeds the byte budget",
        ));
    }
    let text = std::str::from_utf8(&buf).map_err(|_| {
        DescriptionCandidateError::command("json.syntax", "/", "stdin is not UTF-8")
    })?;
    Ok(text.to_string())
}
