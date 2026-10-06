mod common;

use std::io::Write;
use std::process::{Command, Stdio};

use cdc_transaction::MAX_LINE_BYTES;
use common::{begin, bind, data, end, upsert, TS};
use serde_json::Value;

fn executable() -> &'static str {
    if let Some(path) = option_env!("CARGO_BIN_EXE_cdc-transaction") {
        return path;
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_cdc_transaction") {
        return path;
    }
    panic!("missing cdc-transaction executable path");
}

fn run(input: &[u8], extra_arg: bool) -> (i32, String) {
    let mut command = Command::new(executable());
    if extra_arg {
        command.arg("no");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    let code = output.status.code().expect("status");
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    (code, stdout)
}

fn feed(lines: &[String]) -> Vec<u8> {
    let mut body = String::new();
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    body.into_bytes()
}

fn parse_lines(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("json"))
        .collect()
}

#[test]
fn happy_path_exits_zero_and_keeps_applied_null() {
    let input = feed(&[
        bind(&["public.lines", "public.orders"]),
        begin("inv17"),
        data(&upsert("inv17", 1, 1, "public.orders", "17")),
        end("inv17", 1, &[("public.orders", 1)], "16/B6E40", TS),
    ]);
    let (code, stdout) = run(&input, false);
    assert_eq!(code, 0);
    let lines = parse_lines(&stdout);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["kind"], "complete_transaction");
    assert_eq!(lines[0]["position"], "18446744168199581248");
    assert_eq!(lines[0]["attestations"]["ack_allowed"], false);
    assert_eq!(lines[0]["attestations"]["debezium_normalizer"], false);
    assert_eq!(lines[0]["attestations"]["upstream_commit_lsn_verified"], false);
    assert_eq!(lines[1]["kind"], "progress");
    assert!(lines[1]["applied"].is_null());
    assert_eq!(lines[1]["resume_required"], false);
    assert_eq!(lines[1]["ack_allowed"], false);
    assert_eq!(lines[1]["fault"], Value::Null);
    assert_eq!(lines[1]["complete_decoded"]["transactions"], 1);
}

#[test]
fn missing_end_exits_three_without_ack() {
    let input = feed(&[
        bind(&["public.orders"]),
        begin("inv17"),
        data(&upsert("inv17", 1, 1, "public.orders", "1")),
    ]);
    let (code, stdout) = run(&input, false);
    assert_eq!(code, 3);
    let lines = parse_lines(&stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["kind"], "progress");
    assert_eq!(lines[0]["resume_required"], true);
    assert!(lines[0]["applied"].is_null());
    assert_eq!(lines[0]["ack_allowed"], false);
    assert_eq!(lines[0]["pending"][0]["txn_id"], "inv17");
    assert!(lines[0]["fault"].is_null());
}

#[test]
fn usage_row_lsn_duplicate_key_and_long_line() {
    let (code, stdout) = run(b"", true);
    assert_eq!(code, 1);
    assert_eq!(parse_lines(&stdout)[0]["kind"], "usage");

    let (empty_code, empty_out) = run(b"", false);
    assert_eq!(empty_code, 2);
    let empty_lines = parse_lines(&empty_out);
    assert_eq!(empty_lines[0]["kind"], "refused");
    assert_eq!(empty_lines[0]["fault"], "bind_required");
    assert!(empty_lines[0]["txn_id"].is_null());
    assert_eq!(empty_lines[1]["kind"], "progress");

    let mut bad = feed(&[bind(&["public.orders"]), begin("inv17")]);
    bad.extend_from_slice(b"{\"kind\":\"data\",\"row_lsn\":\"0/1\",\"txn_id\":\"inv17\"}\n");
    let (row_code, row_out) = run(&bad, false);
    assert_eq!(row_code, 2);
    assert_eq!(parse_lines(&row_out)[0]["fault"], "malformed");

    let dup = feed(&[
        bind(&["public.orders"]),
        "{\"kind\":\"begin\",\"txn_id\":\"a\",\"txn_id\":\"a\"}".to_owned(),
    ]);
    let (dup_code, dup_out) = run(&dup, false);
    assert_eq!(dup_code, 2);
    assert_eq!(parse_lines(&dup_out)[0]["fault"], "malformed");

    let mut long = vec![b'x'; MAX_LINE_BYTES + 1];
    long.push(b'\n');
    let (long_code, long_out) = run(&long, false);
    assert_eq!(long_code, 2);
    assert_eq!(parse_lines(&long_out)[0]["fault"], "line_too_long");
    assert_eq!(parse_lines(&long_out)[1]["kind"], "progress");
}
