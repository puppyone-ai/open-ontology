#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Stdio};

use answer_citations::sha256_hex;
use serde_json::json;

const FIXTURE: &[u8] = include_bytes!("fixtures/desk-explicit.audit.jsonl");

#[test]
fn help_usage_success_and_citation_failure() {
    let (code, stdout, stderr) = run(&["--help"], b"");
    assert_eq!(code, 0);
    let help = String::from_utf8(stdout).unwrap();
    assert!(help.contains("local_operator_declared"));
    assert!(help.contains("not an authenticated") || help.contains("no session, principal"));
    assert!(stderr.is_empty());
    assert!(!help.contains("CANARY_ANSWER_a266"));

    let (code, stdout, stderr) = run(&["--help", "extra"], b"");
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"answer-citations: usage\n");

    let (code, stdout, stderr) = run(&[], b"");
    assert_eq!(code, 3);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"answer-citations: request\n");

    let text = "华东 [q_836041cccc8287ad] CANARY_ANSWER_a266";
    let (code, stdout, stderr) = run(&[], &request(true, text, FIXTURE));
    assert_eq!(code, 0);
    assert!(stderr.is_empty());
    assert!(stdout.ends_with(b"\n"));
    let report = String::from_utf8(stdout).unwrap();
    assert!(report.contains("\"citation_status\":\"ok\""));
    assert!(report.contains("q_836041cccc8287ad"));
    assert!(!report.contains("CANARY_ANSWER_a266"));
    assert!(!report.contains("华东"));
    assert!(!report.contains("24db5e087ebb8f4bd2fdac6711e8df4c72221d5051025a4ecf3f7bb0357f58f5"));

    let (code, stdout, stderr) = run(&[], &request(true, "CANARY_ANSWER_a266", FIXTURE));
    assert_eq!(code, 1);
    assert_eq!(stderr, b"answer-citations: citations\n");
    let report = String::from_utf8(stdout).unwrap();
    assert!(report.contains("\"missing_citation\":true"));
    assert!(!report.contains("CANARY_ANSWER_a266"));

    let mut bad = request(true, "CANARY_ANSWER_a266 [q_836041cccc8287ad]", FIXTURE);
    let value: serde_json::Value = serde_json::from_slice(&bad).unwrap();
    let mut flipped = sha256_hex(FIXTURE);
    let last = if flipped.ends_with('3') { "4" } else { "3" };
    flipped.replace_range(63..64, last);
    let mut edited = value;
    edited["receipts"][0]["receipt_sha256"] = json!(flipped);
    bad = serde_json::to_vec(&edited).unwrap();
    let (code, stdout, stderr) = run(&[], &bad);
    assert_eq!(code, 4);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"answer-citations: hash\n");
}

fn request(declares: bool, text: &str, body: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "version": "oo.answer-citations/v1",
        "trust": "local_operator_declared",
        "binding": {
            "session_id": "sess1",
            "turn_id": "turn1",
            "role": "oo_r_desk",
            "project": "tickets",
            "protocol": "oo.gateway.query-pg-audit/v1"
        },
        "receipts": [{
            "receipt_sha256": sha256_hex(body),
            "body": std::str::from_utf8(body).unwrap()
        }],
        "answer": {"declares_data": declares, "text": text}
    }))
    .unwrap()
}

fn executable() -> std::path::PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_answer-citations") {
        return std::path::PathBuf::from(path);
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_answer_citations") {
        return std::path::PathBuf::from(path);
    }
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("answer-citations")
}

fn run(args: &[&str], stdin: &[u8]) -> (u8, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(executable())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let output = child.wait_with_output().unwrap();
    let code = u8::try_from(output.status.code().unwrap()).unwrap();
    (code, output.stdout, output.stderr)
}
