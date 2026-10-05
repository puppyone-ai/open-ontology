//! Exact comparison of one expected typed result and one actual typed result.
//!
//! The rules are `CONTRACT.md`. This crate does not score prose, call a model,
//! or compute pass^k.

#![forbid(unsafe_code)]

mod compare;
mod parse;

use std::io::{Read, Write};

use serde::Serialize;

pub use compare::{Comparison, compare_document};
pub use parse::ConfigError;

const HELP: &str = "\
result-comparator reads oo.benchmark.result-comparison/v1 from stdin.
It writes oo.benchmark.result-comparison-report/v1 to stdout.
exit 0 match
exit 1 mismatch
exit 2 config
";

const REPORT_VERSION: &str = "oo.benchmark.result-comparison-report/v1";

/// SHA-256 hex of `bytes` through `oo_core::canonical::hash_bytes`.
///
/// Equality of rows does not use this digest. Tests check it against the
/// published FIPS 180-4 vectors.
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

/// Read a comparison from `stdin` and write the report or a config line.
///
/// Exit 0 is a match, 1 is a mismatch, and 2 is config or a write failure.
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
    if buf.len() > parse::DOCUMENT_MAX_BYTES {
        return write_config(&mut stderr, &ConfigError::at("budget", "/"));
    }
    let Ok(text) = String::from_utf8(buf) else {
        return write_config(&mut stderr, &ConfigError::bare("json"));
    };
    match compare_document(&text) {
        Ok(comparison) => {
            let mut bytes = comparison.report.into_bytes();
            bytes.push(b'\n');
            let code = if comparison.matched { 0 } else { 1 };
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

#[derive(Serialize)]
struct Report<'a> {
    differences: Vec<Difference<'a>>,
    verdict: &'a str,
    version: &'a str,
}

#[derive(Serialize)]
struct Difference<'a> {
    actual: &'a str,
    expected: &'a str,
    path: &'a str,
    reason: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    row: Option<&'a str>,
}

pub(crate) fn render_report(differences: &[compare::Diff]) -> String {
    let verdict = if differences.is_empty() {
        "match"
    } else {
        "mismatch"
    };
    let report = Report {
        differences: differences
            .iter()
            .map(|diff| Difference {
                actual: &diff.actual,
                expected: &diff.expected,
                path: &diff.path,
                reason: diff.reason,
                row: diff.row.as_deref(),
            })
            .collect(),
        verdict,
        version: REPORT_VERSION,
    };
    serde_json::to_string(&report).expect("report json")
}
