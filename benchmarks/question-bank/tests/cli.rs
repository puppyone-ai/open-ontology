use std::io::Write;
use std::process::{Command, Stdio};

const HELP: &str = "\
question-bank reads oo.benchmark.question-bank/v1 from stdin.
It writes oo.benchmark.question-bank-report/v1 to stdout.
exit 0 fully approved labels
exit 1 needs reannotation
exit 2 config
";

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn binary(args: &[&str], stdin_bytes: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_question-bank"))
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

fn expect_config(name: &str, stderr: &str) {
    let (code, stdout, err) = binary(&[], &fixture(name));
    assert_eq!(code, 2, "{name}");
    assert!(stdout.is_empty(), "{name}");
    assert_eq!(err, stderr.as_bytes(), "{name}");
}

#[test]
fn compiled_cli_help_usage_and_stdin_limits() {
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
    assert_eq!(stderr, b"question-bank: usage\n");

    let (code, stdout, stderr) = binary(&[], b"");
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"question-bank: json\n");

    let (code, stdout, stderr) = binary(&[], &[0xff]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"question-bank: json\n");

    let mut big = vec![b' '; oo_question_bank::DOCUMENT_MAX_BYTES + 1];
    big[0] = b'{';
    let (code, stdout, stderr) = binary(&[], &big);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"question-bank: budget /\n");
}

#[test]
fn compiled_cli_reports_are_deterministic() {
    let (code, stdout, stderr) = binary(&[], &fixture("approved.json"));
    assert_eq!(code, 0);
    assert!(stderr.is_empty());
    assert!(stdout.ends_with(b"\n"));
    let (again, second, stderr) = binary(&[], &fixture("approved.json"));
    assert_eq!(again, 0);
    assert_eq!(second, stdout);
    assert!(stderr.is_empty());

    let (code, mixed, stderr) = binary(&[], &fixture("mixed.json"));
    assert_eq!(code, 1);
    assert!(stderr.is_empty());
    let (again, second, stderr) = binary(&[], &fixture("mixed.json"));
    assert_eq!(again, 1);
    assert_eq!(second, mixed);
    assert!(stderr.is_empty());

    let (code, reordered, stderr) = binary(&[], &fixture("reordered.json"));
    assert_eq!(code, 1);
    assert!(stderr.is_empty());
    assert_ne!(reordered, mixed);
}

#[test]
fn compiled_cli_config_does_not_write_a_report() {
    expect_config(
        "duplicate-id.json",
        "question-bank: schema /questions/1/id\n",
    );
    expect_config(
        "missing-gold-approved.json",
        "question-bank: review /questions/0/review/status\n",
    );
    expect_config(
        "bad-version.json",
        "question-bank: version /questions/0/version\n",
    );
    expect_config(
        "bad-as-of.json",
        "question-bank: as_of /questions/0/as_of\n",
    );
    expect_config(
        "holdout-split.json",
        "question-bank: schema /questions/0/split\n",
    );
    expect_config(
        "customer-distribution.json",
        "question-bank: schema /dataset/distribution\n",
    );
    expect_config(
        "relative-tolerance.json",
        "question-bank: tolerance /questions/0/expected/value_columns/0/tolerance\n",
    );
    expect_config(
        "same-answer.json",
        "question-bank: role /questions/1/expected\n",
    );
    expect_config(
        "same-role.json",
        "question-bank: schema /questions/1/asker/role\n",
    );
    expect_config(
        "bad-decimal.json",
        "question-bank: decimal /questions/0/expected/typed_gold/rows/0/0/coefficient\n",
    );
    expect_config(
        "human-annotator.json",
        "question-bank: schema /questions/0/provenance/annotator\n",
    );
    expect_config(
        "gold-sql-mismatch.json",
        "question-bank: schema /questions/1/accepted_interpretations/0/gold_sql\n",
    );
    expect_config("duplicate-key.json", "question-bank: json\n");
}
