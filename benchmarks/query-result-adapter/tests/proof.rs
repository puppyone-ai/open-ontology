//! Live proof. It runs only when `OO_A222_PROOF=1`.
//!
//! The expected documents under `fixtures/` are handwritten. This file does
//! not build them from the adapter output.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn live_query_pg_stdout_matches_hand_gold_and_mutations_refuse() {
    if std::env::var("OO_A222_PROOF").ok().as_deref() != Some("1") {
        eprintln!("OO_A222_PROOF is unset; the query-pg adapter proof was not run");
        return;
    }
    let capture = PathBuf::from(std::env::var("OO_A222_CAPTURE").expect("capture dir"));
    let desk_path = PathBuf::from(std::env::var("OO_A222_DESK").expect("desk stdout"));
    let comparator = PathBuf::from(std::env::var("OO_A222_COMPARATOR").expect("comparator"));
    let adapter = PathBuf::from(env!("CARGO_BIN_EXE_query-result-adapter"));
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let desk = fs::read(&desk_path).expect("desk stdout bytes");
    assert!(!desk.is_empty(), "desk stdout empty");
    compare_capture(
        &adapter,
        &comparator,
        &root.join("fixtures/desk-header.jsonl"),
        &root.join("fixtures/desk-gold.json"),
        &desk,
        Expect::Match,
    );

    let mut saw_desk = 0u32;
    let mut saw_peek = 0u32;
    let mut saw_count = 0u32;
    let mut saw_truncate = 0u32;
    let mut saw_multi = 0u32;
    for entry in fs::read_dir(&capture).expect("capture dir") {
        let entry = entry.expect("entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".stdout") {
            continue;
        }
        let bytes = fs::read(entry.path()).expect("capture");
        match classify(&bytes) {
            Shape::Desk => {
                compare_capture(
                    &adapter,
                    &comparator,
                    &root.join("fixtures/desk-header.jsonl"),
                    &root.join("fixtures/desk-gold.json"),
                    &bytes,
                    Expect::Match,
                );
                saw_desk += 1;
            }
            Shape::Peek => {
                compare_capture(
                    &adapter,
                    &comparator,
                    &root.join("fixtures/peek-header.jsonl"),
                    &root.join("fixtures/peek-gold.json"),
                    &bytes,
                    Expect::Match,
                );
                saw_peek += 1;
            }
            Shape::Count => {
                compare_capture(
                    &adapter,
                    &comparator,
                    &root.join("fixtures/count-header.jsonl"),
                    &root.join("fixtures/count-gold.json"),
                    &bytes,
                    Expect::Match,
                );
                saw_count += 1;
            }
            Shape::Truncate => {
                compare_capture(
                    &adapter,
                    &comparator,
                    &root.join("fixtures/desk-header.jsonl"),
                    &root.join("fixtures/desk-gold.json"),
                    &bytes,
                    Expect::Mismatch,
                );
                let actual = adapt(
                    &adapter,
                    &fs::read(root.join("fixtures/desk-header.jsonl")).unwrap(),
                    &bytes,
                );
                assert_eq!(actual["completion"], "complete");
                assert_eq!(actual["rows"].as_array().unwrap().len(), 1);
                saw_truncate += 1;
            }
            Shape::Multi => {
                // The saved two-turn stdout starts with the count row. The
                // count binding lets that row through so the later summary
                // is the refusal. A peek binding type-refuses the int first.
                let refused = adapt_code(
                    &adapter,
                    &fs::read(root.join("fixtures/count-header.jsonl")).unwrap(),
                    &bytes,
                );
                assert_eq!(
                    refused.code,
                    2,
                    "{}",
                    String::from_utf8_lossy(&refused.stderr)
                );
                assert!(refused.stdout.is_empty());
                let stderr = String::from_utf8(refused.stderr).unwrap();
                assert!(
                    stderr.starts_with("query-result-adapter: protocol"),
                    "{stderr}"
                );
                saw_multi += 1;
            }
            Shape::Other(shape) => panic!("unclassified query-pg stdout {name}: {shape}"),
        }
    }
    assert!(saw_desk >= 1, "tee missed the desk result");
    assert!(saw_peek >= 1, "tee missed the peek result");
    assert!(saw_count >= 1, "tee missed the count result");
    assert!(saw_truncate >= 1, "tee missed the truncated result");
    assert!(saw_multi >= 1, "tee missed the two-summary result");

    let beta = mutate_once(&desk, br#""value":"beta""#, br#""value":"BETA""#);
    compare_capture(
        &adapter,
        &comparator,
        &root.join("fixtures/desk-header.jsonl"),
        &root.join("fixtures/desk-gold.json"),
        &beta,
        Expect::Mismatch,
    );
    let audit = mutate_once(&desk, br#""end_audit_ok":true"#, br#""end_audit_ok":false"#);
    let refused = adapt_code(
        &adapter,
        &fs::read(root.join("fixtures/desk-header.jsonl")).unwrap(),
        &audit,
    );
    assert_eq!(refused.code, 2);
    assert_eq!(
        String::from_utf8(refused.stderr).unwrap(),
        "query-result-adapter: completion /summary/end_audit_ok\n"
    );
    let partial = mutate_once(&desk, br#""status":"succeeded""#, br#""status":"partial""#);
    let actual = adapt(
        &adapter,
        &fs::read(root.join("fixtures/desk-header.jsonl")).unwrap(),
        &partial,
    );
    assert_eq!(actual["completion"], "partial");
    compare_capture(
        &adapter,
        &comparator,
        &root.join("fixtures/desk-header.jsonl"),
        &root.join("fixtures/desk-gold.json"),
        &partial,
        Expect::Mismatch,
    );
    let missing = drop_summary(&desk);
    let refused = adapt_code(
        &adapter,
        &fs::read(root.join("fixtures/desk-header.jsonl")).unwrap(),
        &missing,
    );
    assert_eq!(refused.code, 2);
    let stderr = String::from_utf8(refused.stderr).unwrap();
    assert!(
        stderr.starts_with("query-result-adapter: protocol"),
        "{stderr}"
    );
}

enum Expect {
    Match,
    Mismatch,
}

enum Shape {
    Desk,
    Peek,
    Count,
    Truncate,
    Multi,
    Other(String),
}

struct Ran {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn classify(bytes: &[u8]) -> Shape {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return Shape::Other("utf8".to_owned()),
    };
    let mut rows = 0usize;
    let mut width: Option<usize> = None;
    let mut summaries = 0usize;
    let mut status = String::new();
    let mut truncated = false;
    let mut returned = 0u64;
    let mut end_ok = false;
    let mut int_cell = false;
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Shape::Other("json".to_owned());
        };
        match value.get("record").and_then(Value::as_str) {
            Some("row") => {
                rows += 1;
                let cells = value["cells"].as_array().map(Vec::len).unwrap_or(0);
                if width.is_none() {
                    width = Some(cells);
                    int_cell = cells == 1 && value["cells"][0]["kind"] == "int";
                }
            }
            Some("summary") => {
                summaries += 1;
                status = value["status"].as_str().unwrap_or("").to_owned();
                truncated = value["truncated"] == true;
                returned = value["returned_rows"].as_u64().unwrap_or(u64::MAX);
                end_ok = value["end_audit_ok"] == true;
            }
            _ => return Shape::Other("record".to_owned()),
        }
    }
    if summaries > 1 {
        return Shape::Multi;
    }
    let width = width.unwrap_or(0);
    if status == "succeeded" && end_ok && returned == rows as u64 {
        if rows == 3 && width == 5 && !truncated {
            return Shape::Desk;
        }
        if rows == 3 && width == 1 && !truncated {
            return Shape::Peek;
        }
        if rows == 1 && width == 1 && int_cell && !truncated {
            return Shape::Count;
        }
        if rows == 1 && width == 5 && truncated {
            return Shape::Truncate;
        }
    }
    Shape::Other(format!(
        "rows={rows} width={width} summaries={summaries} status={status}"
    ))
}

fn compare_capture(
    adapter: &Path,
    comparator: &Path,
    header: &Path,
    gold_path: &Path,
    stdout: &[u8],
    expect: Expect,
) {
    let header_bytes = fs::read(header).expect("header");
    let actual = adapt(adapter, &header_bytes, stdout);
    let gold: Value =
        serde_json::from_slice(&fs::read(gold_path).expect("gold")).expect("gold json");
    let mode = gold["row_comparator"].as_str().expect("mode");
    let document = json!({
        "version": "oo.benchmark.result-comparison/v1",
        "row_comparator": mode,
        "expected": gold["expected"],
        "actual": actual,
    });
    let bytes = serde_json::to_vec(&document).unwrap();
    let ran = command(comparator, &[], &bytes);
    let wanted = match expect {
        Expect::Match => 0,
        Expect::Mismatch => 1,
    };
    assert_eq!(
        ran.code,
        wanted,
        "comparator stdout {} stderr {}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    assert!(ran.stderr.is_empty());
    let report: Value = serde_json::from_slice(trim_nl(&ran.stdout)).unwrap();
    match expect {
        Expect::Match => assert_eq!(report["verdict"], "match"),
        Expect::Mismatch => assert_eq!(report["verdict"], "mismatch"),
    }
}

fn adapt(adapter: &Path, header: &[u8], stdout: &[u8]) -> Value {
    let ran = adapt_code(adapter, header, stdout);
    assert_eq!(ran.code, 0, "{}", String::from_utf8_lossy(&ran.stderr));
    assert!(ran.stderr.is_empty());
    serde_json::from_slice(trim_nl(&ran.stdout)).expect("actual json")
}

fn adapt_code(adapter: &Path, header: &[u8], stdout: &[u8]) -> Ran {
    let mut input = Vec::with_capacity(header.len() + stdout.len() + 1);
    input.extend_from_slice(header);
    if !header.ends_with(b"\n") {
        input.push(b'\n');
    }
    input.extend_from_slice(stdout);
    command(adapter, &[], &input)
}

fn command(bin: &Path, args: &[&str], stdin: &[u8]) -> Ran {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(stdin).expect("stdin");
    let output = child.wait_with_output().expect("wait");
    Ran {
        code: output.status.code().unwrap_or(127),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

fn trim_nl(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

fn mutate_once(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let start = bytes
        .windows(from.len())
        .position(|window| window == from)
        .expect("needle");
    let mut out = Vec::with_capacity(bytes.len() - from.len() + to.len());
    out.extend_from_slice(&bytes[..start]);
    out.extend_from_slice(to);
    out.extend_from_slice(&bytes[start + from.len()..]);
    assert_ne!(out, bytes);
    out
}

fn drop_summary(bytes: &[u8]) -> Vec<u8> {
    let text = std::str::from_utf8(bytes).unwrap();
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    assert!(lines.last().unwrap().contains("\"summary\""));
    lines.pop();
    let mut out = lines.join("\n");
    out.push('\n');
    out.into_bytes()
}
