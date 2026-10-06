//! Map one bounded query-pg JSONL result into one typed comparator actual.
//!
//! The rules are `CONTRACT.md`. This crate does not score a result, call a
//! model, or read an expected gold document.

#![forbid(unsafe_code)]

mod parse;

use std::io::{Read, Write};

pub use parse::ConfigError;

const HELP: &str = "\
query-result-adapter reads oo.benchmark.query-result-adapter/v1 JSONL from stdin.
It writes one typed actual result for the result comparator to stdout.
exit 0 actual
exit 2 config
";

/// SHA-256 hex of `bytes` through `oo_core::canonical::hash_bytes`.
///
/// The adapter does not hash a row to decide a value. Tests check this
/// function against the published FIPS 180-4 vectors.
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

/// Read adapter JSONL from `stdin` and write one typed actual result.
///
/// Exit 0 is a typed result. Exit 2 is config or a write failure.
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
    match parse::adapt(&text) {
        Ok(body) => {
            let mut bytes = body.into_bytes();
            bytes.push(b'\n');
            write_stdout(&mut stdout, &mut stderr, &bytes, 0)
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
