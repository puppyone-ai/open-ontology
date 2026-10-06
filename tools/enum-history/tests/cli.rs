use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::json;

fn bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_enum-history")
        .or(option_env!("CARGO_BIN_EXE_enum_history"))
        .expect("enum-history test binary")
}

fn run(input: &[u8], args: &[&str]) -> (i32, String, String) {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().expect("stdin").write_all(input).expect("write");
    let output = child.wait_with_output().expect("wait");
    let code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    (code, stdout, stderr)
}

fn request(yaml: &str, as_of: &str, observations: serde_json::Value, threshold: Option<serde_json::Value>) -> Vec<u8> {
    let mut body = json!({
        "enum_yaml": yaml,
        "as_of": as_of,
        "observations": observations,
    });
    if let Some(threshold) = threshold {
        body["threshold"] = threshold;
    }
    serde_json::to_vec(&body).expect("request")
}

#[test]
fn stdin_compiles_and_refuses() {
    let open = include_str!("fixtures/open.yaml");
    let (code, stdout, stderr) = run(&request(open, "2026-01-01", json!([]), None), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    let report: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(report["outcome"], "compiled");
    assert_eq!(report["observed_coverage"]["defined"], false);
    assert_eq!(report["observed_coverage"]["ratio"], serde_json::Value::Null);
    assert_eq!(report["review_needed"], true);
    assert_eq!(report["owner_authenticated"], false);
    assert_eq!(report["source_modified"], false);

    let overlap = include_str!("fixtures/overlap-historical.yaml");
    let (code, stdout, stderr) = run(
        &request(overlap, "2026-01-01", json!([{"code": "1", "count": 1}]), None),
        &[],
    );
    assert_eq!(code, 2, "{stdout}");
    assert!(stderr.contains("refused overlap"), "{stderr}");
    let refusal: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(refusal["reason"], "overlap");
    assert!(refusal.get("rows").is_none());
}

#[test]
fn threshold_number_duplicate_and_malformed_exits() {
    let open = include_str!("fixtures/open.yaml");
    let (code, stdout, stderr) = run(
        &request(open, "2026-01-01", json!([]), Some(json!(0.95))),
        &[],
    );
    assert_eq!(code, 2, "{stdout}");
    assert!(stderr.contains("threshold_not_decimal_string"), "{stderr}");
    let refusal: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(refusal["reason"], "threshold_not_decimal_string");

    let (code, _, _) = run(b"{", &[]);
    assert_eq!(code, 1);
    let (code, _, stderr) = run(b"", &["--help"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("malformed usage"), "{stderr}");

    let mut trailing = request(open, "2026-01-01", json!([]), None);
    trailing.extend_from_slice(b"{}");
    let (code, _, stderr) = run(&trailing, &[]);
    assert_eq!(code, 1);
    assert!(stderr.contains("trailing"), "{stderr}");
}
