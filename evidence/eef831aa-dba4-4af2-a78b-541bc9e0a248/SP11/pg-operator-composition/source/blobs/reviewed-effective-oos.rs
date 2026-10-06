//! Stdin entry for the reviewed effective-OOS compiler.
//!
//! No arguments. One JSON document on stdin. The process does not connect,
//! grant, authenticate, or publish.

use std::io::{Read, Write};
use std::process::ExitCode;

use oo_builder::{ReviewedEffectiveError, compile_reviewed_effective};
use oo_core::limits::DOCUMENT_MAX_BYTES;

fn main() -> ExitCode {
    if std::env::args().nth(1).is_some() {
        eprintln!(
            "reviewed-effective-oos: usage: reviewed-effective-oos reads one JSON document from stdin and takes no arguments"
        );
        return ExitCode::from(2);
    }
    let text = match read_stdin() {
        Ok(text) => text,
        Err(err) => return fail(&err),
    };
    match compile_reviewed_effective(&text) {
        Ok(bytes) => {
            let mut out = std::io::stdout().lock();
            if out.write_all(bytes.as_bytes()).is_err() {
                return fail(&ReviewedEffectiveError::command(
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

fn fail(err: &ReviewedEffectiveError) -> ExitCode {
    eprintln!("reviewed-effective-oos: {} {}", err.code, err.message);
    ExitCode::from(1)
}

fn read_stdin() -> Result<String, ReviewedEffectiveError> {
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    let max = DOCUMENT_MAX_BYTES.saturating_add(1);
    stdin
        .by_ref()
        .take(max as u64)
        .read_to_end(&mut buf)
        .map_err(|_| {
            ReviewedEffectiveError::command("io.read", "/", "stdin could not be read")
        })?;
    let text = std::str::from_utf8(&buf).map_err(|_| {
        ReviewedEffectiveError::command("json.syntax", "/", "stdin is not UTF-8")
    })?;
    Ok(text.to_string())
}
