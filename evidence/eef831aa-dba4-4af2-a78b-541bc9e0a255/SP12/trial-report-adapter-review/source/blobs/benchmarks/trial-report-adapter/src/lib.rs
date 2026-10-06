//! Pair two trusted SQL runner reports into one trial-statistics input.
//!
//! The rules are `CONTRACT.md`. This crate does not compute pass^k, call a
//! model, or certify a database snapshot.

#![forbid(unsafe_code)]

mod adapt;
mod parse;

use std::io::{Read, Write};

pub use parse::ConfigError;

pub use oo_core::limits::DOCUMENT_MAX_BYTES;

const HELP: &str = "\
trial-report-adapter reads oo.benchmark.trial-report-adapter/v1 from stdin.
It writes oo.benchmark.trial-statistics/v1 to stdout.
The process does not compute pass^k or a bootstrap.
exit 0 statistics input written
exit 2 config
";

/// Read one pairing document and return the statistics input, without a trailing newline.
pub fn adapt(input: &str) -> Result<String, ConfigError> {
    let parsed = parse::parse_input(input)?;
    Ok(adapt::render(&parsed))
}

/// Read stdin and write the statistics input, or a config line.
///
/// Exit 0 writes a document. Exit 2 is config or a write failure. There is no gate exit.
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
            return write_stdout(&mut stdout, &mut stderr, HELP.as_bytes());
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
    match adapt(&text) {
        Ok(document) => {
            let mut bytes = document.into_bytes();
            bytes.push(b'\n');
            write_stdout(&mut stdout, &mut stderr, &bytes)
        }
        Err(error) => write_config(&mut stderr, &error),
    }
}

fn write_stdout(stdout: &mut impl Write, stderr: &mut impl Write, bytes: &[u8]) -> i32 {
    if stdout.write_all(bytes).is_err() || stdout.flush().is_err() {
        return write_config(stderr, &ConfigError::bare("io"));
    }
    0
}

fn write_config(stderr: &mut impl Write, error: &ConfigError) -> i32 {
    let line = error.stderr_line();
    if stderr.write_all(line.as_bytes()).is_err() {
        return 2;
    }
    2
}
