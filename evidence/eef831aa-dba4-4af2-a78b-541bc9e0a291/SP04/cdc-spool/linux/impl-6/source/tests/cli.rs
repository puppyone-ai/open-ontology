#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use common::{begin, bind, end, feed, rollback, tempdir, TS};
use serde_json::{json, Value};

fn executable() -> &'static str {
    if let Some(path) = option_env!("CARGO_BIN_EXE_cdc-spool") {
        return path;
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_cdc_spool") {
        return path;
    }
    panic!("missing cdc-spool executable path");
}

fn run(args: &[&str], input: &[u8]) -> (i32, String) {
    let mut child = Command::new(executable())
        .args(args)
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

fn sample() -> Vec<u8> {
    feed(&[
        bind(&["public.lines", "public.orders"]),
        begin("inv17"),
        common::data(&common::Row {
            txn: "inv17",
            global: 1,
            collection: 1,
            table: "public.orders",
            key: "17",
            op: "upsert",
            payload: json!([{"type": "text", "ordinal": 0, "value": "open"}]),
            unavailable: &[],
        }),
        end("inv17", 1, &[("public.orders", 1)], "16/B6E40", TS),
    ])
}

fn object(stdout: &str) -> Value {
    let line = stdout.lines().next().expect("line");
    serde_json::from_str(line).expect("json")
}

#[test]
fn usage_is_exit_1() {
    for args in [Vec::<&str>::new(), vec!["spool"], vec!["nope", "dir"], vec!["spool", "a", "b"]] {
        let (code, stdout) = run(&args, b"");
        assert_eq!(code, 1, "{stdout}");
        assert_eq!(object(&stdout)["kind"], "usage");
    }
}

#[test]
fn spool_recover_and_replay_exit_0() {
    let root = tempdir();
    let dir = root.join("txn");
    let input = sample();
    let (code, stdout) = run(&["spool", dir.to_str().unwrap()], &input);
    assert_eq!(code, 0, "{stdout}");
    let doc = object(&stdout);
    assert_eq!(doc["kind"], "sealed");
    assert_eq!(doc["txn_id"], "inv17");
    assert_eq!(doc["count"], json!(1));
    assert_eq!(doc["commit_position"], "18446744168199581248");
    assert_eq!(doc["ack_allowed"], false);
    assert!(doc["applied"].is_null());

    let (code, stdout) = run(&["recover", dir.to_str().unwrap()], b"");
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(object(&stdout)["kind"], "committed");
    assert_eq!(object(&stdout)["ack_allowed"], false);

    let seal = fs::read(dir.join("SEAL")).unwrap();
    let (code, stdout) = run(&["replay", dir.to_str().unwrap()], &input);
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(object(&stdout)["kind"], "replay_match");
    assert_eq!(fs::read(dir.join("SEAL")).unwrap(), seal);

    let text = String::from_utf8(input).unwrap().replace("\"open\"", "\"shut\"");
    let (code, stdout) = run(&["replay", dir.to_str().unwrap()], text.as_bytes());
    assert_eq!(code, 2, "{stdout}");
    assert_eq!(object(&stdout)["fault"], "payload_conflict");
    assert_eq!(fs::read(dir.join("SEAL")).unwrap(), seal);
}

#[test]
fn incomplete_and_refusal_and_rollback_exits() {
    let root = tempdir();
    let open = root.join("open");
    let (code, stdout) = run(
        &["spool", open.to_str().unwrap()],
        &feed(&[bind(&["public.orders"]), begin("inv17")]),
    );
    assert_eq!(code, 3, "{stdout}");
    assert_eq!(object(&stdout)["kind"], "incomplete");
    assert_eq!(object(&stdout)["ack_allowed"], false);
    let (code, stdout) = run(&["recover", open.to_str().unwrap()], b"");
    assert_eq!(code, 3, "{stdout}");
    assert_eq!(object(&stdout)["kind"], "unknown");

    let (code, stdout) = run(&["spool", root.join("empty").to_str().unwrap()], b"");
    assert_eq!(code, 2, "{stdout}");
    assert_eq!(object(&stdout)["fault"], "bind_required");

    let bad = root.join("ord");
    let bytes = feed(&[
        bind(&["public.orders"]),
        begin("ord1"),
        "{\"kind\":\"data\",\"txn_id\":\"ord1\",\"global_txn_sequence\":1,\"data_collection_sequence\":1,\"table\":\"public.orders\",\"key\":[{\"type\":\"int\",\"ordinal\":0,\"value\":\"1\"}],\"op\":\"upsert\",\"payload\":[{\"type\":\"int\",\"ordinal\":65535,\"value\":\"1\"}],\"unavailable_ordinals\":[]}".to_owned(),
    ]);
    let (code, stdout) = run(&["spool", bad.to_str().unwrap()], &bytes);
    assert_eq!(code, 2, "{stdout}");
    assert_eq!(object(&stdout)["fault"], "ordinal_range");
    assert!(!bad.join("SEAL").exists());
    assert!(!bad.join("chunk-000001").exists());

    let rolled = root.join("rolled");
    let (code, stdout) = run(
        &["spool", rolled.to_str().unwrap()],
        &feed(&[bind(&["public.orders"]), begin("rb1"), rollback("rb1")]),
    );
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(object(&stdout)["kind"], "rolled_back");
    assert!(!rolled.join("SEAL").exists());
    assert_eq!(fs::read(rolled.join("ROLLED_BACK")).unwrap(), b"rolled_back\n");
    let (code, stdout) = run(&["recover", rolled.to_str().unwrap()], b"");
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(object(&stdout)["kind"], "rolled_back");
    assert_eq!(object(&stdout)["ack_allowed"], false);
}

#[test]
fn cli_checkpoint_continue_seals() {
    let root = tempdir();
    let dir = root.join("txn");
    let mark = root.join("mark");
    let mut child = Command::new(executable())
        .args(["spool", dir.to_str().unwrap()])
        .env("CDC_SPOOL_CHECKPOINT", "before_final_seal")
        .env("CDC_SPOOL_MARK", &mark)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&sample()).unwrap();
    }
    for _ in 0..600 {
        if mark.is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(mark.is_file(), "cli mark missing");
    fs::write(root.join("mark.continue"), b"go\n").unwrap();
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert_eq!(object(&stdout)["kind"], "sealed");
    assert!(dir.join("SEAL").is_file());
    assert!(!dir.join("ROLLED_BACK").exists());
}
