use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use oo_result_comparator::run;

const HELP: &str = "\
result-comparator reads oo.benchmark.result-comparison/v1 from stdin.
It writes oo.benchmark.result-comparison-report/v1 to stdout.
exit 0 match
exit 1 mismatch
exit 2 config
";

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read(path).unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn library(args: &[&str], stdin: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        args.iter().map(|arg| (*arg).to_owned()),
        Cursor::new(stdin),
        &mut stdout,
        &mut stderr,
    );
    (code, stdout, stderr)
}

fn binary(stdin_bytes: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_result-comparator"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(stdin_bytes).expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(127),
        output.stdout,
        output.stderr,
    )
}

#[test]
fn help_exits_zero_and_does_not_read_stdin() {
    let (code, stdout, stderr) = library(&["--help"], b"not json");
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());
}

#[test]
fn unknown_argument_is_usage() {
    let (code, stdout, stderr) = library(&["--input"], b"{}");
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"result-comparator: usage\n");
}

#[test]
fn binary_match_and_mismatch_exits() {
    let (code, stdout, stderr) = binary(&fixture("match-exact-decimal.json"));
    assert_eq!(code, 0);
    assert_eq!(stdout, fixture("report-match.json"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&fixture("mismatch-partial.json"));
    assert_eq!(code, 1);
    assert_eq!(stdout, fixture("mismatch-partial.report.json"));
    assert!(stderr.is_empty());
}

#[test]
fn binary_duplicate_key_exits_config() {
    let (code, stdout, stderr) =
        binary(br#"{"version":"oo.benchmark.result-comparison/v1","version":"again"}"#);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"result-comparator: json\n");
}
