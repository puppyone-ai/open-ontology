use std::process::{Command, Output, Stdio};

fn oo(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oo"))
        .args(args)
        .stdin(Stdio::null())
        .env_remove("OO_TOKEN")
        .output()
        .unwrap()
}

#[test]
fn admin_help_is_offline_and_all_commands_have_common_options() {
    for command in ["init", "up", "down", "doctor", "build", "ontology", "token"] {
        let out = oo(&[command, "--help"]);
        assert!(out.status.success());
        let text = String::from_utf8(out.stdout).unwrap();
        for required in ["--config", "--yes", "--json", "not yet"] {
            assert!(text.contains(required));
        }
        assert!(oo(&["help", command]).status.success());
    }
    assert!(
        oo(&["status", "--config", "missing", "--help"])
            .status
            .success()
    );
    // Existing consumer status help still describes query lifecycle.
    let out = oo(&["status", "--help"]);
    assert!(String::from_utf8(out.stdout).unwrap().contains("query_id"));
}

#[test]
fn bad_arguments_never_echo_values_or_touch_stdin() {
    for args in [
        vec!["init", "--config"],
        vec!["init", "--unknown=SYNTHETIC_PRIVATE"],
        vec!["init", "--password", "SYNTHETIC_PRIVATE"],
        vec!["up", "SYNTHETIC_PRIVATE"],
        vec!["build", "--config", "a", "--config", "SYNTHETIC_PRIVATE"],
        vec!["ontology", "export", "--format", "SYNTHETIC_PRIVATE"],
        vec!["ontology", "open", "--format", "html"],
        vec!["token", "SYNTHETIC_PRIVATE"],
        vec!["init", "--resume"],
    ] {
        let mut args = args;
        args.push("--json");
        let out = oo(&args);
        assert!(!out.status.success());
        let bytes = [out.stdout, out.stderr].concat();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("ADMIN_ARGUMENT"), "{text}");
        assert!(!text.contains("SYNTHETIC_PRIVATE"));
    }
    let out = oo(&["init", "--yes", "--json"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8([out.stdout, out.stderr].concat())
            .unwrap()
            .contains("ADMIN_CONFIG_REQUIRED")
    );
    for command in ["up", "down", "status", "build"] {
        let out = oo(&[command, "--json", "--yes", "--config", "missing"]);
        assert!(!out.status.success());
        assert!(
            String::from_utf8([out.stdout, out.stderr].concat())
                .unwrap()
                .contains("ADMIN_CONFIG_IO")
        );
    }
    let out = oo(&["doctor", "--json", "--config", "missing"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8([out.stdout, out.stderr].concat())
            .unwrap()
            .contains("ADMIN_CONFIG_IO")
    );
}

#[test]
fn init_normalizes_without_resolving_secret_references() {
    let dir = std::env::temp_dir().join(format!("oo-admin-test-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("stack.yaml");
    let text = r#"{"version":"oo.stack/v1","sources":[{"id":"erp","host":"db.internal",
        "port":3306,"database":"erp","user":"reader","password":{"secret_ref":"absent"}}],
        "model":{"provider":"openai-compatible","base_url":"https://example.com/v1",
        "model":"test-model","key":{"secret_ref":"absent"}}}"#;
    std::fs::write(&path, text).unwrap();
    let out = oo(&[
        "init",
        "--config",
        path.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    assert!(out.status.success(), "{:?}", out.stderr);
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["deployed"], false);
    let normalized: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(normalized["sources"][0]["sync"]["interval"], "8h");
    assert_eq!(normalized["deploy"]["profile"], "lite");
    std::fs::write(
        &path,
        text.replace("{\"secret_ref\":\"absent\"}", "\"SYNTHETIC_PRIVATE\""),
    )
    .unwrap();
    let before = std::fs::read(&path).unwrap();
    let out = oo(&["init", "--config", path.to_str().unwrap(), "--json"]);
    assert!(!out.status.success());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(
        !String::from_utf8([out.stdout, out.stderr].concat())
            .unwrap()
            .contains("SYNTHETIC_PRIVATE")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn lifecycle_replaces_share_stub_and_rejects_secret_arguments() {
    for command in ["login", "whoami", "share-prompt"] {
        assert!(oo(&[command, "--help"]).status.success());
        assert_eq!(
            oo(&[command, "--help"]).stdout,
            oo(&["help", command]).stdout
        );
        let out = oo(&[command, "--password", "SYNTHETIC_PRIVATE", "--json"]);
        assert_eq!(out.status.code(), Some(2));
        assert!(
            !String::from_utf8([out.stdout, out.stderr].concat())
                .unwrap()
                .contains("SYNTHETIC_PRIVATE")
        );
    }
}
