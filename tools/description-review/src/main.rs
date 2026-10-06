//! Stdin entry for one description review.
//!
//! `description-review apply` reads one review command and writes the review
//! document. It does not call a model, open a network connection, read
//! credentials, use PostgreSQL, write files, grant, certify, authenticate,
//! or publish.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::ExitCode;

use description_review::{DescriptionReviewError, apply_description_review};
use oo_core::limits::DOCUMENT_MAX_BYTES;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let phase = args.next();
    let extra = args.next().is_some();
    match (phase.as_deref(), extra) {
        (Some("apply"), false) => {}
        (Some("apply"), true) => {
            eprintln!("description-review: the phase takes no arguments");
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
    match apply_description_review(&text) {
        Ok(document) => write_stdout(&document),
        Err(err) => fail(&err),
    }
}

fn usage() {
    eprintln!("description-review: usage: description-review apply");
    eprintln!(
        "description-review:   apply    one oo.builder.description-review-command/v1 document on stdin"
    );
    eprintln!("description-review: stdout is the review document and has no added newline");
    eprintln!(
        "description-review: this command does not call a model, open a network connection, read credentials, use PostgreSQL, write files, grant, certify, authenticate, or publish"
    );
}

fn write_stdout(document: &str) -> ExitCode {
    let mut out = std::io::stdout().lock();
    if out.write_all(document.as_bytes()).is_err() {
        return fail(&DescriptionReviewError::new(
            "io.write",
            "stdout could not be written",
        ));
    }
    ExitCode::SUCCESS
}

fn fail(err: &DescriptionReviewError) -> ExitCode {
    eprintln!("description-review: {} {}", err.code, err.message);
    ExitCode::from(1)
}

fn read_stdin() -> Result<String, DescriptionReviewError> {
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    let max = u64::try_from(DOCUMENT_MAX_BYTES.saturating_add(1)).unwrap_or(u64::MAX);
    stdin.by_ref().take(max).read_to_end(&mut buf).map_err(|_| {
        DescriptionReviewError::new("io.read", "stdin could not be read")
    })?;
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(DescriptionReviewError::new(
            "json.budget.bytes",
            "document exceeds the byte budget",
        ));
    }
    let text = std::str::from_utf8(&buf).map_err(|_| {
        DescriptionReviewError::new("json.syntax", "stdin is not UTF-8")
    })?;
    Ok(text.to_string())
}
