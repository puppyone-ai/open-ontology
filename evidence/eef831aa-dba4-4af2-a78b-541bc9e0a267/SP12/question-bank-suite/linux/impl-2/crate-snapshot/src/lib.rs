//! Compile one admitted public question bank into an sql-runner suite.
//!
//! Admission is `oo_question_bank::evaluate`. This crate does not score a
//! model, open a database, or spawn the runner executables.

#![forbid(unsafe_code)]

mod compile;

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub use compile::ConfigError;

use oo_core::limits::DOCUMENT_MAX_BYTES;

const HELP: &str = "\
question-bank-suite --input <path> --out-dir <path>
question-bank-suite --help

question-bank-suite reads one oo.benchmark.question-bank-suite/v1 document.
It writes one sql-runner suite and scorer-owned files.
Gold stays out of the agent view and out of this report.
exit 0 suite written
exit 2 config
";

/// A finished compile. `report` has no trailing newline and no gold cells.
#[derive(Debug, PartialEq, Eq)]
pub struct Compiled {
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

/// Compile `document` into `out_dir`.
///
/// The directory is created with `create_dir` only after admission, hash,
/// identity, and group checks succeed. An existing directory is left unchanged.
/// Runner executables are hashed and are not spawned.
pub fn compile(document: &str, out_dir: &Path) -> Result<Compiled, ConfigError> {
    compile::compile_document(document, out_dir)
}

/// Read one compile document and write the suite directory.
///
/// Exit 0 writes the report. Exit 2 is config and writes no suite when the
/// refusal happens before `create_dir`.
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
        [input_flag, input, out_flag, out_dir]
            if input_flag == "--input" && out_flag == "--out-dir" =>
        {
            match compile_path(input, out_dir) {
                Ok(compiled) => {
                    let mut bytes = compiled.report.into_bytes();
                    bytes.push(b'\n');
                    write_stdout(&mut stdout, &mut stderr, &bytes, 0)
                }
                Err(error) => write_config(&mut stderr, &error),
            }
        }
        _ => write_config(&mut stderr, &ConfigError::bare("usage")),
    }
}

fn compile_path(input: &str, out_dir: &str) -> Result<Compiled, ConfigError> {
    compile::check_abs(input, "/input")?;
    let text = read_input(Path::new(input))?;
    compile(text.as_str(), Path::new(out_dir))
}

fn read_input(path: &Path) -> Result<String, ConfigError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ConfigError::at("path", "/input"))?;
    if !meta.file_type().is_file() {
        return Err(ConfigError::at("path", "/input"));
    }
    if meta.len() > u64::try_from(DOCUMENT_MAX_BYTES).expect("size") {
        return Err(ConfigError::at("budget", "/input"));
    }
    let file = File::open(path).map_err(|_| ConfigError::at("io", "/input"))?;
    let mut limited = file.take(u64::try_from(DOCUMENT_MAX_BYTES).expect("size") + 1);
    let mut buf = Vec::new();
    if limited.read_to_end(&mut buf).is_err() {
        return Err(ConfigError::at("io", "/input"));
    }
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(ConfigError::at("budget", "/input"));
    }
    String::from_utf8(buf).map_err(|_| ConfigError::bare("json"))
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
