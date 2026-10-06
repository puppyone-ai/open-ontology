#![forbid(unsafe_code)]

use std::process::Command;

fn executable() -> &'static str {
    if let Some(path) = option_env!("CARGO_BIN_EXE_session-transition") {
        return path;
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_session_transition") {
        return path;
    }
    panic!("missing session-transition executable path");
}

fn run(stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(executable());
    child
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env_remove("RUST_LOG");
    let mut child = child.spawn().expect("spawn");
    {
        use std::io::Write;
        let input = child.stdin.as_mut().expect("stdin");
        input.write_all(stdin.as_bytes()).expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    let code = output.status.code().unwrap_or(127);
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    (code, stdout, stderr)
}

#[test]
fn cli_roundtrip_reports_the_post_state_revision() {
    let first = r#"{"contract":"session-transition-v1","state":{"session_id":"sess-1","conversation_id":"conv-1","phase":"requested","revision":0,"owner_id":"","owner_generation":0,"lease_expires_at_ms":0,"bound":null,"workspace_id":null,"session_started_at_ms":0,"last_activity_at_ms":0,"last_now_ms":0,"turn":null,"tool_starts":0,"started_tool_ids":[],"open_tool_ids":[],"last_turn_outcome":null,"failure":null,"release":{"s1":null,"s2":null,"s3":null,"s4":null,"s5":null,"s6":null,"s7":null,"s8":null,"s9":null,"s10":null,"s11":null},"journal":[]},"identity":{"project_id":"project-1","principal_id":"alice","role":"analyst","claims_hash":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","policy_version":"pol-1","knowledge_version":"know-1","config_version":"cfg-1"},"expected_revision":0,"expected_owner_generation":0,"event_id":"e-cli","now_ms":1000,"event":{"type":"begin_bind","owner_id":"entry-1","lease_expires_at_ms":31000}}"#;
    let (code, stdout, stderr) = run(first);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    let body: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(body["outcome"], "applied");
    assert_eq!(body["revision"], 1);
    assert_eq!(body["state"]["revision"], 1);
    assert_eq!(body["owner_generation"], 1);
    assert_eq!(body["phase"], "binding");
    assert_eq!(body["physical_release_certified"], false);
    assert_eq!(body["database_used"], false);
    assert_eq!(body["auth_provider_used"], false);
    assert_eq!(body["runtime_used"], false);
    assert_eq!(body["state"]["workspace_id"], serde_json::Value::Null);

    let replay = serde_json::json!({
        "contract": "session-transition-v1",
        "state": body["state"].clone(),
        "identity": {
            "project_id": "project-1",
            "principal_id": "alice",
            "role": "analyst",
            "claims_hash": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "policy_version": "pol-1",
            "knowledge_version": "know-1",
            "config_version": "cfg-1"
        },
        "expected_revision": 0,
        "expected_owner_generation": 0,
        "event_id": "e-cli",
        "now_ms": 1000,
        "event": {"type": "begin_bind", "owner_id": "entry-1", "lease_expires_at_ms": 31000}
    });
    let (code, stdout, stderr) = run(&serde_json::to_string(&replay).unwrap());
    assert_eq!(code, 0, "{stderr}");
    let again: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(again["outcome"], "idempotent");
    assert_eq!(again["revision"], 1);
    assert_eq!(again["state"]["revision"], 1);
    assert_eq!(again["physical_release_certified"], false);

    let stale_input = serde_json::json!({
        "contract": "session-transition-v1",
        "state": serde_json::from_str::<serde_json::Value>(first).unwrap()["state"].clone(),
        "identity": replay["identity"].clone(),
        "expected_revision": 7,
        "expected_owner_generation": 0,
        "event_id": "e-stale",
        "now_ms": 1000,
        "event": {"type": "begin_bind", "owner_id": "entry-1", "lease_expires_at_ms": 31000}
    });
    let (code, stdout, stderr) = run(&serde_json::to_string(&stale_input).unwrap());
    assert_eq!(code, 2, "{stdout} {stderr}");
    assert_eq!(stderr.trim(), "session-transition: refused stale");
    let refused: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(refused["outcome"], "refused");
    assert_eq!(refused["reason"], "stale");
    assert_eq!(refused["revision"], 0);
    assert_eq!(refused["state"]["revision"], 0);
    assert_eq!(refused["phase"], "requested");
    assert_eq!(refused["physical_release_certified"], false);
}

#[test]
fn cli_rejects_trailing_json_and_arguments() {
    let (code, stdout, _) = run(r#"{"contract":"session-transition-v1"} {"extra":true}"#);
    assert_eq!(code, 1);
    let body: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(body["outcome"], "malformed");
    assert!(body.get("revision").is_none());
    assert_eq!(body["physical_release_certified"], false);

    let output = Command::new(executable()).arg("--help").output().expect("args");
    assert_eq!(output.status.code(), Some(1));
}
