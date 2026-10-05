//! Paired pass^k counts and one question bootstrap.
//!
//! The rules are `CONTRACT.md`. This crate does not score prose, call a model,
//! or estimate pass@k.

#![forbid(unsafe_code)]

mod parse;
mod stats;

use std::io::{Read, Write};

pub use parse::ConfigError;
pub use stats::{splitmix64_step, unbiased_index};

pub use oo_core::limits::DOCUMENT_MAX_BYTES;

const HELP: &str = "\
trial-statistics reads oo.benchmark.trial-statistics/v1 from stdin.
It writes oo.benchmark.trial-statistics-report/v1 to stdout.
exit 0 gate passed
exit 1 gate failed
exit 2 config
";

/// A finished report. `report` has no trailing newline.
#[derive(Debug, PartialEq, Eq)]
pub struct Evaluation {
    pub gate_passed: bool,
    pub report: String,
}

/// Parse one document and render its report.
pub fn evaluate(input: &str) -> Result<Evaluation, ConfigError> {
    let parsed = parse::parse_input(input)?;
    let rendered = stats::render(&parsed);
    Ok(Evaluation {
        gate_passed: rendered.gate_passed,
        report: rendered.report,
    })
}

/// Read a document from `stdin` and write the report or a config line.
///
/// Exit 0 is a passed gate, 1 is a failed gate, and 2 is config or a write failure.
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
