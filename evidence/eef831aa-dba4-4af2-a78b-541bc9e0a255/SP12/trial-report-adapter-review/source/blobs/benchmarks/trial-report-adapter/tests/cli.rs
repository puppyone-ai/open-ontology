use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const HELP: &str = "\
trial-report-adapter reads oo.benchmark.trial-report-adapter/v1 from stdin.
It writes oo.benchmark.trial-statistics/v1 to stdout.
The process does not compute pass^k or a bootstrap.
exit 0 statistics input written
exit 2 config
";

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read(path).unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn binary(args: &[&str], stdin_bytes: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_trial-report-adapter"))
        .args(args)
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
fn compiled_cli_writes_statistics_input() {
    let (code, stdout, stderr) = binary(&[], &fixture("k2-mixed.json"));
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&stderr));
    assert_eq!(stdout, fixture("k2-mixed.stdout"));
    assert!(stderr.is_empty());
    assert!(!contains_bytes(&stdout, b"\"model\":\"none\""));
    assert!(contains_bytes(&stdout, b"\"model\":\"trusted_sql\""));

    let (again, second, stderr) = binary(&[], &fixture("k2-mixed.json"));
    assert_eq!(again, 0);
    assert_eq!(second, stdout);
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&[], &fixture("allfail.json"));
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&stderr));
    assert_eq!(stdout, fixture("allfail.stdout"));
    assert!(stderr.is_empty());
    assert!(!contains_bytes(&stdout, b"\"pass\""));
    assert!(!contains_bytes(&stdout, b"needs_reannotation"));
}

#[test]
fn compiled_cli_refuses_a_partial_vector_or_a_tuple_change() {
    let (code, stdout, stderr) = binary(&[], &fixture("missing-trial.json"));
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-report-adapter: trials /reports/1/trials\n");

    let (code, stdout, stderr) = binary(&[], &fixture("tuple-mismatch.json"));
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        b"trial-report-adapter: tuple /reports/1/tuple/project\n"
    );

    let (code, stdout, stderr) = binary(&[], &fixture("duplicate-key.json"));
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-report-adapter: json\n");
}

#[test]
fn compiled_cli_help_and_usage() {
    let (code, stdout, stderr) = binary(&["--help"], b"not json");
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&["-h"], b"");
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&["--input"], b"{}");
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-report-adapter: usage\n");
}
