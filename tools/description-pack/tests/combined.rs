//! Schema refusals for the combined pack command.
//!
//! Behavioral success, stale, double, and conflict live in pg-builder.
//! This crate cannot build a one-table review without that bridge.

use std::process::Command;

use description_pack::{apply_description_pack_combined, COMBINED_COMMAND_PROTOCOL};

const ZEROS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn assert_schema(input: &str) {
    let err = apply_description_pack_combined(input).expect_err("schema refusal");
    assert!(err.code.starts_with(COMBINED_COMMAND_PROTOCOL), "{}", err.code);
    assert!(err.code.ends_with("/schema.invalid"), "{}", err.code);
}

fn command(reviews: serde_json::Value, roles: serde_json::Value, schema: &str) -> String {
    serde_json::json!({
        "protocol": COMBINED_COMMAND_PROTOCOL,
        "reviews": reviews,
        "expected_schema_hash": schema,
        "expected_evidence_hash": ZEROS,
        "expected_curated_sha256": ZEROS,
        "expected_artifact_sha256": ZEROS,
        "expected_prior_review_sha256": ZEROS,
        "settings": {},
        "decisions": [],
        "roles": roles
    })
    .to_string()
}

fn two_roles() -> serde_json::Value {
    serde_json::json!([{"role": "oo_r_narrow"}, {"role": "oo_r_restricted"}])
}

#[test]
fn empty_object_is_schema_invalid() {
    assert_schema("{}");
}

#[test]
fn unknown_field_is_schema_invalid() {
    let mut value: serde_json::Value = serde_json::from_str(&command(serde_json::json!([]), two_roles(), ZEROS)).expect("command");
    value["extra"] = serde_json::json!(true);
    assert_schema(&value.to_string());
}

#[test]
fn empty_reviews_are_schema_invalid() {
    assert_schema(&command(serde_json::json!([]), two_roles(), ZEROS));
}

#[test]
fn one_role_is_schema_invalid() {
    assert_schema(&command(serde_json::json!([]), serde_json::json!([{"role": "oo_r_narrow"}]), ZEROS));
}

#[test]
fn bad_hash_is_schema_invalid() {
    assert_schema(&command(serde_json::json!([]), two_roles(), "abc"));
}

#[test]
fn apply_combined_cli_refuses_an_empty_object_and_keeps_apply_usage() {
    let refused = run(&["apply-combined"], b"{}");
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    let stderr = String::from_utf8(refused.stderr).expect("stderr");
    assert!(stderr.starts_with("description-pack:"), "{stderr}");
    assert!(stderr.contains(COMBINED_COMMAND_PROTOCOL), "{stderr}");

    let usage = run(&[], b"");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    let text = String::from_utf8(usage.stderr).expect("usage");
    assert!(text.contains("description-pack: usage: description-pack apply"), "{text}");
    assert!(text.contains("description-pack apply-combined"), "{text}");
}

fn run(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_description-pack"));
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env_remove("RUST_BACKTRACE");
    let mut child = command.spawn().expect("spawn");
    use std::io::Write;
    child.stdin.as_mut().expect("stdin").write_all(stdin).expect("write");
    drop(child.stdin.take());
    child.wait_with_output().expect("wait")
}
