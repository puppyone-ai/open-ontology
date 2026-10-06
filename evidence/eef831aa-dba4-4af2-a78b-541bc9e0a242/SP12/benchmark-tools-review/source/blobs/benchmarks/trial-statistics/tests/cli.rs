use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const HELP: &str = "\
trial-statistics reads oo.benchmark.trial-statistics/v1 from stdin.
It writes oo.benchmark.trial-statistics-report/v1 to stdout.
exit 0 gate passed
exit 1 gate failed
exit 2 config
";

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read(path).unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn binary(args: &[&str], stdin_bytes: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_trial-statistics"))
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
fn compiled_cli_reports_and_exits() {
    let (code, stdout, stderr) = binary(&[], &fixture("k3-passed.json"));
    assert_eq!(code, 0);
    assert_eq!(stdout, fixture("k3-passed.report.json"));
    assert!(stderr.is_empty());

    let (again, second, stderr) = binary(&[], &fixture("k3-passed.json"));
    assert_eq!(again, 0);
    assert_eq!(second, stdout);
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&[], &fixture("k3-reannotation.json"));
    assert_eq!(code, 1);
    assert_eq!(stdout, fixture("k3-reannotation.report.json"));
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&[], &fixture("k3-not-pass-at.json"));
    assert_eq!(code, 0);
    assert_eq!(stdout, fixture("k3-not-pass-at.report.json"));
    assert!(stderr.is_empty());
}

#[test]
fn compiled_cli_config_does_not_write_a_report() {
    let (code, stdout, stderr) = binary(&[], &fixture("mixed-tuple.json"));
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-statistics: tuple /variants/1/tuple/model\n");

    let (code, stdout, stderr) = binary(&[], &fixture("missing-trial.json"));
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        b"trial-statistics: trials /variants/1/questions/1/trials\n"
    );

    let (code, stdout, stderr) = binary(&[], &fixture("duplicate-key.json"));
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-statistics: json\n");
}

#[test]
fn compiled_cli_help_and_usage() {
    let (code, stdout, stderr) = binary(&["--help"], b"not json");
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&["-h"], b"not json");
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&["--input"], b"{}");
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"trial-statistics: usage\n");
}
