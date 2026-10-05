//! Explicit trusted SQL trials.
//!
//! The rules are `CONTRACT.md`. This crate does not open a database, call a
//! model, or decide that a truncated prefix is a full answer.

#![forbid(unsafe_code)]

mod drive;
mod parse;
mod report;

use std::io::Write;
use std::path::Path;

pub use parse::ConfigError;

const HELP: &str = "\
sql-runner --suite <path> --run-dir <path>
sql-runner --help

sql-runner reads one oo.benchmark.sql-runner/v1 suite and writes one trial report.
Each trial is a new query-pg process, then the result adapter, then the result comparator.
exit 0 gate passed
exit 1 gate failed
exit 2 config
";

/// SHA-256 hex of `bytes` through `oo_core::canonical::hash_bytes`.
///
/// Executable checks use this digest. Tests check it against the published
/// FIPS 180-4 vectors.
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

/// Read a suite and write the trial report or a config line.
///
/// Exit 0 is a passed gate, 1 is a failed gate, and 2 is config or a write failure.
#[must_use]
pub fn run(
    args: impl IntoIterator<Item = String>,
    mut stdout: impl Write,
    mut stderr: impl Write,
) -> i32 {
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            write_stdout(&mut stdout, &mut stderr, HELP.as_bytes(), 0)
        }
        [suite_flag, suite, run_flag, run_dir]
            if suite_flag == "--suite" && run_flag == "--run-dir" =>
        {
            run_suite(suite, run_dir, &mut stdout, &mut stderr)
        }
        _ => write_config(&mut stderr, &ConfigError::bare("usage")),
    }
}

fn run_suite(suite: &str, run_dir: &str, stdout: &mut impl Write, stderr: &mut impl Write) -> i32 {
    if let Err(error) = parse::check_abs(suite, "/suite") {
        return write_config(stderr, &error);
    }
    if let Err(error) = parse::check_abs(run_dir, "/run_dir") {
        return write_config(stderr, &error);
    }
    let loaded = match parse::load(Path::new(suite), run_dir) {
        Ok(loaded) => loaded,
        Err(error) => return write_config(stderr, &error),
    };
    if let Err(error) = parse::reserve_run_dir(Path::new(run_dir)) {
        return write_config(stderr, &error);
    }
    if std::fs::create_dir(run_dir).is_err() {
        return write_config(stderr, &ConfigError::at("run_dir", "/run_dir"));
    }
    let rendered = drive::execute(&loaded, run_dir);
    let code = if rendered.passed { 0 } else { 1 };
    write_report(run_dir, &rendered.bytes, code, stdout, stderr)
}

fn write_report(
    run_dir: &str,
    bytes: &[u8],
    code: i32,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> i32 {
    let path = Path::new(run_dir).join("report.json");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path);
    let Ok(mut file) = file else {
        return write_config(stderr, &ConfigError::bare("io"));
    };
    if file.write_all(bytes).is_err() {
        return write_config(stderr, &ConfigError::bare("io"));
    }
    write_stdout(stdout, stderr, bytes, code)
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
