use std::process::Command;

fn invoke(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oo"))
        .args(args)
        .env("HOME", "/nonexistent/oo-help-must-not-read-home")
        .env("OO_TOKEN", "must-not-be-read-or-printed")
        .output()
        .unwrap()
}

#[test]
fn help_aliases_match_direct_help_without_credentials() {
    for command in [
        vec![], vec!["query"], vec!["tables"], vec!["describe"], vec!["validate"],
        vec!["sql"], vec!["sql", "validate"], vec!["freshness"], vec!["knowledge"],
        vec!["knowledge", "cat"], vec!["knowledge", "search"], vec!["cancel"],
        vec!["status"], vec!["operations"], vec!["agent-skill"],
    ] {
        let mut alias = vec!["help"];
        alias.extend(&command);
        let mut direct = command;
        direct.push("--help");
        let output = invoke(&alias);
        let expected = invoke(&direct);
        assert!(output.status.success(), "{alias:?}");
        assert_eq!(output.stdout, expected.stdout, "{alias:?}");
        assert!(output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("must-not-be-read-or-printed"));
    }
}

#[test]
fn version_is_offline_and_help_input_is_bounded() {
    for flag in ["--version", "-V"] {
        let output = invoke(&[flag, "--json"]);
        assert!(output.status.success());
        let version: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(version["name"], "oo");
        assert_eq!(version["version"], env!("CARGO_PKG_VERSION"));
    }
    for args in [vec!["help", "help"], vec!["help", "query", "extra", "extra"],
                 vec!["help", "--token-file"], vec!["--version", "unexpected"],
                 vec!["help", "unknown"], vec!["sql", "unknown", "--help"],
                 vec!["help", "knowledge", "unknown"]] {
        assert_eq!(invoke(&args).status.code(), Some(2));
    }
    let output = invoke(&["unknown", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["class"], "unsupported");
}

#[test]
fn query_limits_have_an_explicit_scope_and_actionable_metadata_error() {
    let help = invoke(&["--help"]);
    let text = String::from_utf8(help.stdout).unwrap();
    let query_section = text.find("Query-only options").unwrap();
    assert!(query_section < text.find("--timeout-ms").unwrap());
    let metadata = invoke(&["freshness", "--help"]);
    assert!(!String::from_utf8_lossy(&metadata.stdout).contains("--timeout-ms"));
    for flag in ["--timeout-ms", "--max-rows", "-f"] {
        let output = invoke(&["freshness", "--json", flag, "1"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["class"], "usage");
        assert!(value["message"].as_str().unwrap().contains("query-only"));
        assert!(value["message"].as_str().unwrap().contains("oo query --help"));
    }
}

#[test]
fn query_help_explains_budget_cancellation_and_no_partial_aggregates() {
    let output = invoke(&["query", "--help"]);
    let text = String::from_utf8(output.stdout).unwrap();
    for term in ["metadata", "1500ms", "oo operations", "oo cancel", "oo status",
        "No partial aggregate", "OVERLOADED", "process-local"] {
        assert!(text.contains(term), "{term}");
    }
}
