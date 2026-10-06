use std::io::{Cursor, Write};
use std::process::{Command, Stdio};

use oo_query_result_adapter::run;

const HELP: &str = "\
query-result-adapter reads oo.benchmark.query-result-adapter/v1 JSONL from stdin.
It writes one typed actual result for the result comparator to stdout.
exit 0 actual
exit 2 config
";

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

#[test]
fn help_exits_zero_and_does_not_read_stdin() {
    let (code, stdout, stderr) = library(&["--help"], b"not json");
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());
    let (code, stdout, stderr) = library(&["-h"], b"not json");
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());
}

#[test]
fn other_arguments_are_usage_and_do_not_read_stdin() {
    for args in [&["--request"][..], &["--help", "--request"], &["x"]] {
        let (code, stdout, stderr) = library(args, b"{\"record\":\"header\"}\n");
        assert_eq!(code, 2, "{args:?}");
        assert!(stdout.is_empty());
        assert_eq!(stderr, b"query-result-adapter: usage\n");
    }
}

#[test]
fn binary_help_matches_the_library() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_query-result-adapter"))
        .arg("--help")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(b"not json").expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, HELP.as_bytes());
    assert!(output.stderr.is_empty());
}
