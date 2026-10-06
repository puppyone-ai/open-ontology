use std::io::Write;
use std::process::{Command, Stdio};

use business_calendar::{CALENDAR_YAML_MAX_BYTES, STDIN_MAX_BYTES, sha256_hex};
use serde_json::{Value, json};

const MIN_YAML: &str = "schema_version: oos/v1\nbusiness_tz: Asia/Shanghai\nfiscal_year_start_month: 4\nweek_start: monday\nanchors:\n  - today\n";
const MIN_HASH: &str = "d3ed068cbf1d4f9558d90b463e113f91209260e16f99fdf929ad05d807851adc";
const EXPECTED: &str = concat!(
    r#"{"contract":"oo.business-calendar/v1","outcome":"resolved","as_of":"2026-04-01T03:30:00Z","business_tz":"Asia/Shanghai","tzdb_version":"2025b","fiscal_year_start_month":4,"week_start":"monday","quarter_convention":"business_fiscal_quarter","quarter_note":"three civil months counted from fiscal_year_start_month; distinct from date_trunc('quarter')","config_sha256":""#,
    "d3ed068cbf1d4f9558d90b463e113f91209260e16f99fdf929ad05d807851adc",
    r#"","finance_approval":false,"authenticated":false,"database_used":false,"clock_used":false,"closure":null,"anchors":[{"name":"today","civil":"2026-04-01","midnight":{"status":"unique","rfc3339":"2026-04-01T00:00:00+08:00"},"next_midnight":{"status":"unique","rfc3339":"2026-04-02T00:00:00+08:00"},"civil_day_seconds":86400}],"phrases":[]}"#,
    "\n"
);
const HELP: &str = "\
business-calendar reads one oo.business-calendar/v1 JSON document from stdin.
It resolves configured anchors and [start, end) phrases at the explicit as_of instant.
business_tz is the calendar IANA zone. week_start is metadata. There is no clock and no database.
";

fn bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_business-calendar")
        .or(option_env!("CARGO_BIN_EXE_business_calendar"))
        .expect("business-calendar test binary")
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
    (
        output.status.code().unwrap_or(99),
        String::from_utf8(output.stdout).expect("stdout"),
        String::from_utf8(output.stderr).expect("stderr"),
    )
}

fn document(yaml: &str, as_of: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "contract": "oo.business-calendar/v1",
        "calendar_yaml": yaml,
        "as_of": as_of
    }))
    .expect("json")
}

#[test]
fn resolved_stdout_is_exact() {
    assert_eq!(sha256_hex(MIN_YAML.as_bytes()), MIN_HASH);
    let (code, stdout, stderr) = run(&document(MIN_YAML, "2026-04-01T03:30:00Z"), &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stderr, "");
    assert_eq!(stdout, EXPECTED);
    assert_eq!(stdout.matches('\n').count(), 1);

    let mut trailed = document(MIN_YAML, "2026-04-01T03:30:00Z");
    trailed.extend_from_slice(b"\n\t \r\n");
    let (code, stdout, stderr) = run(&trailed, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, EXPECTED);
}

#[test]
fn help_and_usage() {
    let (code, stdout, stderr) = run(b"", &["--help"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, HELP);
    assert_eq!(stderr, "");

    let (code, stdout, stderr) = run(b"", &["--help", "more"]);
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "business-calendar: usage\n");

    let (code, stdout, stderr) = run(b"", &["-h"]);
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "business-calendar: usage\n");
}

#[test]
fn malformed_contract_and_second_value() {
    let (code, stdout, stderr) = run(b"", &[]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused malformed\n");
    assert_eq!(
        stdout,
        "{\"contract\":\"oo.business-calendar/v1\",\"outcome\":\"refused\",\"reason\":\"malformed\"}\n"
    );

    let (code, _, stderr) = run(b"{", &[]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused malformed\n");

    let mut second = document(MIN_YAML, "2026-04-01T03:30:00Z");
    second.extend_from_slice(b"{}");
    let (code, _, stderr) = run(&second, &[]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused malformed\n");

    let mut wrong = document(MIN_YAML, "2026-04-01T03:30:00Z");
    let mut value: Value = serde_json::from_slice(&wrong).unwrap();
    value["contract"] = json!("oo.other/v1");
    wrong = serde_json::to_vec(&value).unwrap();
    let (code, stdout, stderr) = run(&wrong, &[]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused contract_mismatch\n");
    assert!(stdout.contains("\"reason\":\"contract_mismatch\""));

    value["contract"] = json!("oo.business-calendar/v1");
    value["extra"] = json!(1);
    let (code, _, stderr) = run(&serde_json::to_vec(&value).unwrap(), &[]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused malformed\n");

    let long_as_of = "x".repeat(41);
    let (code, _, stderr) = run(&document(MIN_YAML, &long_as_of), &[]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused malformed\n");
}

#[test]
fn bounds_and_redaction() {
    let (code, stdout, stderr) = run(&vec![b'{'; STDIN_MAX_BYTES + 1], &[]);
    assert_eq!(code, 3);
    assert_eq!(stderr, "business-calendar: refused budget\n");
    assert!(stdout.len() < 200);
    assert!(!stdout.contains("{{{"));

    let fat = "a".repeat(CALENDAR_YAML_MAX_BYTES + 1);
    let (code, stdout, stderr) = run(&document(&fat, "2026-04-01T03:30:00Z"), &[]);
    assert_eq!(code, 3);
    assert_eq!(stderr, "business-calendar: refused yaml_budget\n");
    assert!(!stdout.contains("aaa"));
    assert!(!stderr.contains("aaa"));

    let yaml = "schema_version: oos/v1\nbusiness_tz: Asia/Shanghai\nfiscal_year_start_month: 4\nweek_start: monday\nanchors:\n  - today\nphrases:\n  SENTINEL_DO_NOT_LEAK_9f3a: { start: \"floor(today)\", end: today, bounds: \"[)\" }\n";
    let (code, stdout, stderr) = run(&document(yaml, "2026-04-01T03:30:00Z"), &[]);
    assert_eq!(code, 4, "{stdout} {stderr}");
    assert_eq!(stderr, "business-calendar: refused calendar_invalid\n");
    assert!(!stdout.contains("SENTINEL_DO_NOT_LEAK_9f3a"));
    assert!(!stderr.contains("SENTINEL_DO_NOT_LEAK_9f3a"));
    assert!(!stdout.contains("floor"));
    assert!(!stderr.contains("floor"));
    assert!(!stdout.contains("unknown"));
    let text = stdout.trim_end();
    let refusal: Value = serde_json::from_str(text).unwrap();
    assert_eq!(refusal["reason"], "calendar_invalid");
    assert!(refusal["diagnostic_count"].as_u64().unwrap_or(0) > 0);
    assert!(refusal.get("detail").is_none());
    let mut at = 0usize;
    for key in ["contract", "outcome", "reason", "diagnostic_count"] {
        let needle = format!("\"{key}\":");
        let pos = text.find(&needle).unwrap_or_else(|| panic!("missing {key}"));
        assert!(pos >= at, "{key}");
        at = pos + needle.len();
    }
}

#[test]
fn closure_and_expression_exits() {
    let yaml = "schema_version: oos/v1\nbusiness_tz: UTC\nfiscal_year_start_month: 1\nweek_start: monday\nanchors:\n  - last_closed_period_end\n";
    let (code, stdout, stderr) = run(&document(yaml, "2026-04-01T12:00:00Z"), &[]);
    assert_eq!(code, 5);
    assert_eq!(stderr, "business-calendar: refused closure_required\n");
    assert!(stdout.contains("\"reason\":\"closure_required\""));

    let mut request: Value = serde_json::from_slice(&document(yaml, "2026-04-01T12:00:00Z")).unwrap();
    request["closure"] = json!({
        "last_closed_period_end": "2026-03-31",
        "label": "close-2026-03",
        "record_sha256": "ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
    });
    let (code, stdout, stderr) = run(&serde_json::to_vec(&request).unwrap(), &[]);
    assert_eq!(code, 5);
    assert_eq!(stderr, "business-calendar: refused closure_invalid\n");
    assert!(stdout.contains("\"detail\":\"hash\""));
    assert!(!stdout.contains("ABCDEF"));
    assert!(!stdout.contains("close-2026-03"));
    assert!(!stderr.contains("ABCDEF"));
}
