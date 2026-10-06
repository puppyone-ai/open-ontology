//! Public synthetic question bank validator.
//!
//! The rules are `CONTRACT.md`. This crate separates an agent view from a
//! scorer view. It does not score an answer, call a model, or sign a review.

#![forbid(unsafe_code)]

mod emit;
mod parse;

use std::io::{Read, Write};

pub use parse::ConfigError;

pub use oo_core::limits::DOCUMENT_MAX_BYTES;

const HELP: &str = "\
question-bank reads oo.benchmark.question-bank/v1 from stdin.
It writes oo.benchmark.question-bank-report/v1 to stdout.
exit 0 fully approved labels
exit 1 needs reannotation
exit 2 config
";

/// A finished report. `report` has no trailing newline.
#[derive(Debug, PartialEq, Eq)]
pub struct Evaluation {
    pub gate_passed: bool,
    pub report: String,
}

/// SHA-256 hex of `bytes` through `oo_core::canonical::hash_bytes`.
///
/// Tests check this against the published FIPS 180-4 vectors.
pub fn sha256_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = oo_core::canonical::hash_bytes(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// Parse one bank and render its report.
pub fn evaluate(input: &str) -> Result<Evaluation, ConfigError> {
    let parsed = parse::parse_input(input)?;
    let rendered = emit::render(&parsed);
    Ok(Evaluation {
        gate_passed: rendered.gate_passed,
        report: rendered.report,
    })
}

/// Read a bank from `stdin` and write the report or a config line.
///
/// Exit 0 means every label is `fully_approved`. Exit 1 means at least one
/// question is `needs_reannotation` and the denominator is unchanged. Exit 2
/// is config or a write failure.
pub fn run(
    args: impl IntoIterator<Item = String>,
    stdin: impl Read,
    mut stdout: impl Write,
    mut stderr: impl Write,
) -> i32 {
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [] => {}
        [flag] if flag == "--help" || flag == "-h" => {
            return write_stdout(&mut stdout, &mut stderr, HELP.as_bytes(), 0);
        }
        _ => return write_config(&mut stderr, &ConfigError::bare("usage")),
    }
    let mut buf = Vec::new();
    let mut limited = stdin.take(parse::STDIN_CAP);
    if limited.read_to_end(&mut buf).is_err() {
        return write_config(&mut stderr, &ConfigError::bare("stdin"));
    }
    if buf.len() > DOCUMENT_MAX_BYTES {
        return write_config(&mut stderr, &ConfigError::at("budget", "/"));
    }
    let Ok(text) = String::from_utf8(buf) else {
        return write_config(&mut stderr, &ConfigError::bare("json"));
    };
    match evaluate(&text) {
        Ok(evaluation) => {
            let mut bytes = evaluation.report.into_bytes();
            bytes.push(b'\n');
            let code = if evaluation.gate_passed { 0 } else { 1 };
            write_stdout(&mut stdout, &mut stderr, &bytes, code)
        }
        Err(error) => write_config(&mut stderr, &error),
    }
}

fn write_stdout(stdout: &mut impl Write, stderr: &mut impl Write, bytes: &[u8], code: i32) -> i32 {
    if stdout.write_all(bytes).is_err() || stdout.flush().is_err() {
        return write_config(stderr, &ConfigError::bare("io"));
    }
    code
}

fn write_config(stderr: &mut impl Write, error: &ConfigError) -> i32 {
    let line = error.stderr_line();
    if stderr.write_all(line.as_bytes()).is_err() {
        return 2;
    }
    2
}
