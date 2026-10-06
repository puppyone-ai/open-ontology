mod support;

use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

use serde_json::Value;

use oo_question_bank_suite::run;

use support::{AS_OF, ROLE, World, assert_clean, bank, document, tiny_rows};

const HELP: &str = "\
question-bank-suite --input <path> --out-dir <path>
question-bank-suite --help

question-bank-suite reads one oo.benchmark.question-bank-suite/v1 document.
It writes one sql-runner suite and scorer-owned files.
Gold stays out of the agent view and out of this report.
exit 0 suite written
exit 2 config
";

fn run_lib(args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        args.iter().copied().map(str::to_owned),
        &mut stdout,
        &mut stderr,
    );
    (code, stdout, stderr)
}

fn binary(args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    let output = Command::new(env!("CARGO_BIN_EXE_question-bank-suite"))
        .args(args)
        .output()
        .expect("spawn question-bank-suite");
    (
        output.status.code().unwrap_or(127),
        output.stdout,
        output.stderr,
    )
}

#[test]
fn help_is_exact_and_usage_does_not_read_input() {
    let (code, stdout, stderr) = run_lib(&["--help"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = run_lib(&["-h"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());

    let missing = "/tmp/oo-a267-missing-input";
    assert!(!std::path::Path::new(missing).exists());
    let (code, stdout, stderr) =
        run_lib(&["--input", missing, "--out-dir", "/tmp/oo-a267-out", "extra"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"question-bank-suite: usage\n");
    assert!(!std::path::Path::new(missing).exists());

    let (code, stdout, stderr) = binary(&["--help"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&["-h"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP.as_bytes());
    assert!(stderr.is_empty());

    let (code, stdout, stderr) = binary(&["--out-dir", "/tmp/oo-a267-out", "--input", missing]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"question-bank-suite: usage\n");
}

#[test]
fn relative_and_symlink_inputs_are_path_refusals() {
    let (code, stdout, stderr) =
        run_lib(&["--input", "input.json", "--out-dir", "/tmp/oo-a267-cli-out"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"question-bank-suite: path /input\n");

    let world = World::new("cli-link");
    let real = world.root.join("real.json");
    fs::write(&real, b"{}\n").unwrap();
    let link = world.root.join("link.json");
    symlink(&real, &link).unwrap();
    let (code, stdout, stderr) = run_lib(&[
        "--input",
        link.to_str().unwrap(),
        "--out-dir",
        world.out("suite").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"question-bank-suite: path /input\n");
    assert!(!world.out("suite").exists());
}

#[test]
fn compiled_cli_writes_one_report_line() {
    let world = World::new("cli");
    let bank = bank(vec![tiny_rows("pub-one", ROLE, "g-one", AS_OF)]);
    let doc = document(&bank, &world, ROLE, AS_OF, 1);
    let input = world.root.join("input.json");
    fs::write(&input, &doc).unwrap();
    let out = world.out("suite");
    let (code, stdout, stderr) = binary(&[
        "--input",
        input.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty());
    assert!(stdout.ends_with(b"\n"));
    assert!(!stdout.ends_with(b"\n\n"));
    assert_clean(&stdout);
    let report: Value = serde_json::from_slice(&stdout[..stdout.len() - 1]).unwrap();
    assert_eq!(
        report["version"],
        "oo.benchmark.question-bank-suite-report/v1"
    );
    assert_eq!(report["question_count"], 1);
    assert_eq!(report["trial_count"], 1);
    assert_eq!(report["model"], "none");
    assert_eq!(report["quality_claim"], "none");
    assert!(report.get("files").is_none());
    assert!(out.join("suite.json").is_file());
    assert!(out.join("questions/pub-one/gold.json").is_file());
}
