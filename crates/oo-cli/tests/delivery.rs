use oo_cli::{check, mcp_config, profile, share_prompt};
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn delivery_help_requires_no_profile_or_network() {
    for command in ["profile", "use", "check", "mcp-config", "mcp"] {
        let output = Command::new(env!("CARGO_BIN_EXE_oo"))
            .args([command, "--help"])
            .env_remove("HOME")
            .env("OO_CONFIG_DIR", "invalid-relative-path")
            .output()
            .unwrap();
        assert!(output.status.success(), "{command}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("oo use NAME"));
    }
}

#[test]
fn two_databases_keep_credentials_and_trust_isolated() {
    let root = std::env::temp_dir().join(format!("oo-delivery-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["first", "second"] {
        let p = json!({"schema_version":1, "gateway":format!("https://{name}.invalid"),
            "token_file": root.join(format!("{name}.credential")), "ca_file":root.join(format!("{name}.ca"))});
        profile::save(&root, name, &p).unwrap();
        assert!(profile::save(&root, name, &p).is_err());
    }
    assert_eq!(profile::list(&root).unwrap(), ["first", "second"]);
    let out = Command::new(env!("CARGO_BIN_EXE_oo"))
        .args(["use", "first", "--json"])
        .env("OO_CONFIG_DIR", &root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(profile::active_path(&root).unwrap()).unwrap()).unwrap();
    assert_eq!(stored["gateway"], "https://first.invalid");
    assert_eq!(stored["token_file"], json!(root.join("first.credential")));
    assert_eq!(stored["ca_file"], json!(root.join("first.ca")));
    assert!(profile::select(&root, "../second").is_err());
    assert!(profile::select(&root, "missing").is_err());
    assert_eq!(
        profile::active_path(&root).unwrap(),
        root.join("profiles/first.json")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn renderers_accept_only_a_profile_identifier() {
    for agent in ["claude", "cursor", "codex"] {
        let config = mcp_config::render(agent, "demo").unwrap();
        assert!(config.contains("--profile"));
        assert!(!config.contains("token"));
        assert!(!config.contains("gateway"));
        assert!(mcp_config::render(agent, "../escape").is_err());
        assert!(mcp_config::render(agent, "x\"\ninjected").is_err());
    }
    assert!(mcp_config::render("unknown", "demo").is_err());
    let report = json!({"share_prompt": share_prompt::render("demo").unwrap()});
    assert!(
        report["share_prompt"]
            .as_str()
            .unwrap()
            .contains("oo check --json")
    );
}

#[test]
fn readiness_needs_a_published_pack_and_visible_tables() {
    let tables = json!({"tables":[{"name":"demo.orders"}]});
    assert_eq!(
        check::assess(&tables, &json!({"publication":"live"}))["ready"],
        true
    );
    for f in [
        json!({}),
        json!({"publication":"not_claimed"}),
        json!({"error":"unavailable"}),
    ] {
        assert_eq!(check::assess(&tables, &f)["ready"], false);
    }
    assert_eq!(
        check::assess(&json!({"tables":[]}), &json!({"publication":"live"}))["ready"],
        false
    );
}
