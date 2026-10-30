use std::process::Command;

#[test]
fn installed_binary_carries_the_same_skill_without_auth_or_network_access() {
    let output = Command::new(env!("CARGO_BIN_EXE_oo"))
        .args([
            "agent-skill",
            "--json",
            "--gateway",
            "https://127.0.0.1:1",
            "--transport",
            "production",
            "--auth",
            "external",
            "--token-file",
            "/does-not-exist/never-read.token",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["protocol"], "oo.agent-skill/v1");
    assert_eq!(body["grants_access"], false);
    assert_eq!(body["offline"], true);
    assert_eq!(
        body["text"],
        include_str!("../../../tools/agent-contract/business-answer-policy.md")
    );
}

#[test]
fn skill_is_discoverable_and_rejects_unexpected_arguments() {
    let help = Command::new(env!("CARGO_BIN_EXE_oo"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).contains("agent-skill"));
    let text = Command::new(env!("CARGO_BIN_EXE_oo"))
        .arg("agent-skill")
        .output()
        .unwrap();
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains("# 业务回答规范"));
    let invalid = Command::new(env!("CARGO_BIN_EXE_oo"))
        .args(["agent-skill", "unexpected"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
}
