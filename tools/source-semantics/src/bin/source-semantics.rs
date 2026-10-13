//! Read one source-semantics command from stdin and write the result to stdout.

use std::io::{self, Read, Write};
use std::process::ExitCode;

use source_semantics::compile_source_semantics;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    if args.next().is_some() {
        let _ = writeln!(io::stderr(), "source-semantics: usage: stdin JSON, no arguments");
        return ExitCode::from(2);
    }
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() || input.trim().is_empty() {
        let _ = writeln!(io::stderr(), "source-semantics: usage: stdin JSON is empty");
        return ExitCode::from(2);
    }
    match compile_source_semantics(&input) {
        Ok(text) => {
            let status = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|value| value.get("status").and_then(|item| item.as_str()).map(str::to_string))
                .unwrap_or_default();
            if writeln!(io::stdout(), "{text}").is_err() {
                return ExitCode::from(1);
            }
            if status == "packed" {
                ExitCode::SUCCESS
            } else if status == "blocked" {
                ExitCode::from(3)
            } else {
                ExitCode::from(1)
            }
        }
        Err(err) => {
            let _ = writeln!(io::stderr(), "source-semantics: {} {}", err.code, err.message);
            ExitCode::from(1)
        }
    }
}
