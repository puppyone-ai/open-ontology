use oo_stack::{parse, secret::SecretStore};
use serde_json::{Value, json};
use std::os::unix::fs::{PermissionsExt, symlink};

fn config() -> Value {
    json!({
        "version":"oo.stack/v1",
        "sources":[{"id":"erp","host":"db.internal","port":3306,"database":"erp",
            "user":"reader","password":{"secret_ref":"erp-password"}}],
        "model":{"provider":"openai-compatible","base_url":"https://api.example.com/v1",
            "model":"model","key":{"secret_ref":"model-key"}}
    })
}

#[test]
fn model_endpoint_reference_roundtrips_without_resolving() {
    let mut c = config();
    c["model"]["base_url"] = json!({"secret_ref":"model-endpoint"});
    let stack = parse(&c.to_string()).unwrap();
    assert_eq!(
        serde_json::to_value(stack).unwrap()["model"]["base_url"],
        c["model"]["base_url"]
    );
    for value in [
        json!({"secret_ref":"../endpoint"}),
        json!({"secret_ref":"endpoint","value":"https://example.com"}),
        json!({}),
    ] {
        c["model"]["base_url"] = value;
        assert!(parse(&c.to_string()).is_err());
    }
}

#[test]
fn defaults_and_roundtrip() {
    let s = parse(&config().to_string()).unwrap();
    assert_eq!(s.sources[0].sync.interval, "8h");
    assert_eq!(s.deploy.profile, "lite");
    let normalized = serde_json::to_string(&s).unwrap();
    assert!(parse(&normalized).is_ok());
    let mut explicit = config();
    explicit["sources"][0]["engine"] = json!("mysql-5.7");
    assert!(parse(&explicit.to_string()).is_ok());
    assert!(parse(include_str!("../example.stack.json")).is_ok());
    let schema: Value = serde_json::from_str(oo_stack::JSON_SCHEMA).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    for definition in schema["$defs"].as_object().unwrap().values() {
        assert_eq!(definition["additionalProperties"], false);
    }
}

#[test]
fn snapshot_mode_is_explicit_and_closed() {
    let mut c = config();
    assert!(matches!(
        parse(&c.to_string()).unwrap().sources[0].snapshot,
        oo_stack::stack::Snapshot::ChunkConsistent
    ));
    c["sources"][0]["snapshot"] = json!("single");
    assert!(matches!(
        parse(&c.to_string()).unwrap().sources[0].snapshot,
        oo_stack::stack::Snapshot::Single
    ));
    c["sources"][0]["snapshot"] = json!("eventually");
    assert!(parse(&c.to_string()).is_err());
}

#[test]
fn source_tls_anchor_is_a_reference_not_inline_material() {
    let mut value = config();
    value["sources"][0]["tls_ca"] = json!({"secret_ref":"fixture-ca"});
    let stack = parse(&value.to_string()).unwrap();
    assert_eq!(
        stack.sources[0].tls_ca.as_ref().unwrap().secret_ref,
        "fixture-ca"
    );
    for bad in [
        json!("certificate"),
        json!({"secret_ref":"../ca"}),
        json!({"path":"ca.pem"}),
    ] {
        value["sources"][0]["tls_ca"] = bad;
        assert!(parse(&value.to_string()).is_err());
    }
}

#[test]
fn reject_invalid_values_without_echoing_them() {
    for (pointer, bad) in [
        ("/sources/0/password", json!("SYNTHETIC-PRIVATE")),
        ("/sources/0/password/secret_ref", json!("../escape")),
        ("/model/key/secret_ref", json!("a/b")),
        ("/model/base_url", json!("http://api.example.com/v1")),
        (
            "/model/base_url",
            json!("https://user:password@example.com/v1"),
        ),
        (
            "/model/base_url",
            json!("https://example.com/v1?key=PRIVATE"),
        ),
        ("/model/context_window", json!(0)),
        ("/model/context_window", json!("guess")),
        ("/model/model", json!("sk-SYNTHETIC-PRIVATE")),
        ("/version", json!("oo.stack/v2")),
        ("/sources/0/port", json!(0)),
    ] {
        let mut c = config();
        if pointer == "/model/context_window" {
            c["model"]["context_window"] = bad;
        } else {
            *c.pointer_mut(pointer).unwrap() = bad;
        }
        let error = parse(&c.to_string()).err().unwrap();
        assert!(!error.contains("PRIVATE"));
    }
    for interval in ["0h", "-1h", "1.5h", "8", "01h", "18446744073709551615d"] {
        let mut c = config();
        c["sources"][0]["sync"] = json!({"interval": interval});
        assert!(parse(&c.to_string()).is_err());
    }
}

#[test]
fn reject_unknown_duplicate_and_inline_nested_fields() {
    for pointer in ["", "/sources/0", "/model"] {
        let mut c = config();
        c.pointer_mut(pointer).unwrap()["password_extra"] = json!("PRIVATE");
        assert!(parse(&c.to_string()).is_err());
    }
    let text = config().to_string().replacen(
        "\"version\":",
        "\"version\":\"oo.stack/v1\",\"version\":",
        1,
    );
    assert!(parse(&text).is_err());
    let mut c = config();
    c["sources"][0]["ssh"] = json!({"host":"bastion", "user":"ops",
        "key":{"secret_ref":"ssh-key"}, "host_key_sha256":"..."});
    assert!(parse(&c.to_string()).is_err());
    c["sources"][0]["ssh"]["host_key_sha256"] = json!(format!("SHA256:{}", "A".repeat(43)));
    assert!(parse(&c.to_string()).is_ok());
    c["sources"][0]["ssh"]["key"]["unknown"] = json!(true);
    assert!(parse(&c.to_string()).is_err());
    assert_eq!(parse("version: oo.stack/v1").err(), Some("STACK_SCHEMA"));
}

#[test]
fn yaml_preserves_json_validation_and_secret_redaction() {
    let yaml = "\
version: oo.stack/v1
sources:
  - id: erp
    host: db.internal
    port: 3306
    database: erp
    user: reader
    password: {secret_ref: erp-password}
model:
  provider: openai-compatible
  base_url: https://api.example.com/v1
  model: model
  key: {secret_ref: model-key}
";
    let stack = parse(yaml).unwrap();
    let flow = config()
        .to_string()
        .replacen("\"version\":", "version: ", 1);
    assert!(parse(&flow).is_ok());
    assert_eq!(
        serde_json::to_value(stack).unwrap(),
        serde_json::to_value(parse(&config().to_string()).unwrap()).unwrap()
    );
    for bad in [
        yaml.replace("{secret_ref: model-key}", "inline-private-value"),
        yaml.replace(
            "{secret_ref: model-key}",
            "{secret_ref: model-key, password: inline-private-value}",
        ),
        yaml.replace(
            "{secret_ref: model-key}",
            "{secret_ref: model-key, secret_ref: duplicate}",
        ),
        format!("{yaml}version: oo.stack/v1\n"),
        format!("{yaml}---\n{yaml}"),
        yaml.replace("model: model", "model: !include private-file"),
        yaml.replace("model: model", "model: &alias model"),
        yaml.replace("model: model", "model: *alias"),
        yaml.replace("model: model", "<<: {model: model}"),
    ] {
        assert_eq!(parse(&bad).err(), Some("STACK_SCHEMA"));
    }
    assert_eq!(parse(&" ".repeat(1_048_577)).err(), Some("STACK_BUDGET"));
    assert!(
        parse(&yaml.replace("https://api.example.com/v1", "http://api.example.com/v1")).is_err()
    );
}

#[test]
fn loopback_http_only() {
    for url in [
        "http://127.0.0.1:8317/v1",
        "http://[::1]:8317/v1",
        "http://localhost/v1",
    ] {
        let mut c = config();
        c["model"]["base_url"] = json!(url);
        assert!(parse(&c.to_string()).is_ok(), "{url}");
    }
    for url in [
        "http://127.0.0.1.evil/v1",
        "http://0.0.0.0",
        "https://[::1]evil",
        "ftp://localhost",
    ] {
        assert!(!oo_stack::stack::endpoint(url), "{url}");
    }
}

#[test]
fn secret_files_fail_closed() {
    let dir = std::env::temp_dir().join(format!("oo-stack-test-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let store = SecretStore::create(dir.join("secrets")).unwrap();
    store.write_new("key", b"synthetic-fixture\n").unwrap();
    assert_eq!(store.read("key").unwrap(), "synthetic-fixture\n");
    assert!(store.write_new("key", b"replacement").is_err());
    assert!(store.read("../key").is_err());
    symlink(dir.join("secrets/key"), dir.join("secrets/link")).unwrap();
    assert!(store.read("link").is_err());
    std::fs::set_permissions(
        dir.join("secrets/key"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(store.read("key").is_err());
    std::fs::set_permissions(dir.join("secrets"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(SecretStore::open(dir.join("secrets")).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn secret_store_accepts_symlinked_ancestors_not_private_leaves() {
    let dir = std::env::temp_dir().join(format!("oo-stack-ancestor-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::create_dir(dir.join("config")).unwrap();
    symlink(dir.join("config"), dir.join("alias")).unwrap();
    let store = SecretStore::create(dir.join("alias/secrets")).unwrap();
    store.write_new("key", b"fixture").unwrap();
    assert_eq!(
        SecretStore::open(dir.join("alias/secrets"))
            .unwrap()
            .read("key")
            .unwrap(),
        "fixture"
    );
    symlink(dir.join("config/secrets"), dir.join("private-link")).unwrap();
    assert!(SecretStore::open(dir.join("private-link")).is_err());
    assert!(SecretStore::create(dir.join("private-link")).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn remote_deploy_requires_one_reference_credential_and_pinned_key() {
    let pin = format!("SHA256:{}", "A".repeat(43));
    let mut stack = config();
    stack["deploy"] = json!({"target":"ssh","ssh":{"host":"deploy.example.com",
        "user":"ops","key":{"secret_ref":"deploy-key"},"host_key_sha256":pin}});
    let parsed = parse(&stack.to_string()).unwrap();
    let ssh = parsed.deploy.ssh.as_ref().unwrap();
    assert_eq!(ssh.port, 22);
    assert!(ssh.password.is_none());
    let normalized = serde_json::to_value(&parsed).unwrap();
    assert!(normalized["deploy"]["ssh"].get("password").is_none());
    assert!(parse(&normalized.to_string()).is_ok());
    let mut password = stack.clone();
    password["deploy"]["ssh"]["port"] = json!(2222);
    password["deploy"]["ssh"]
        .as_object_mut()
        .unwrap()
        .remove("key");
    password["deploy"]["ssh"]["password"] = json!({"secret_ref":"deploy-password"});
    assert!(parse(&password.to_string()).is_ok());
    let mut local = stack.clone();
    local["deploy"]["target"] = json!("local");
    assert!(parse(&local.to_string()).is_ok());
    let cases: Vec<(&str, Value)> = vec![
        ("host", json!("-oProxyCommand")),
        ("host", json!("bad host")),
        ("user", json!("-l")),
        ("user", json!("a b")),
        ("port", json!(0)),
        ("host_key_sha256", json!("SHA256:short")),
        ("password", json!({"secret_ref":"also"})),
        (
            "key",
            json!(concat!("-----BEGIN OPENSSH ", "PRIVATE KEY-----")),
        ),
        ("key", json!({"secret_ref":"../escape"})),
        ("extra", json!(true)),
    ];
    for (field, value) in cases {
        let mut bad = stack.clone();
        bad["deploy"]["ssh"][field] = value.clone();
        let text = bad.to_string();
        assert!(parse(&text).is_err(), "{field}");
    }
    let mut neither = stack.clone();
    neither["deploy"]["ssh"]
        .as_object_mut()
        .unwrap()
        .remove("key");
    assert_eq!(parse(&neither.to_string()).err(), Some("STACK_DEPLOY_SSH"));
    let mut missing = stack;
    missing["deploy"].as_object_mut().unwrap().remove("ssh");
    assert_eq!(parse(&missing.to_string()).err(), Some("STACK_DEPLOY_SSH"));
}

#[test]
fn ssh_auth_is_exactly_one_secret_reference_and_port_is_bounded() {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../example.stack.json")).unwrap();
    value["sources"][0]["ssh"] = serde_json::json!({
        "host":"bastion", "user":"ops", "password":{"secret_ref":"jump-password"},
        "host_key_sha256":format!("SHA256:{}", "A".repeat(43))
    });
    let parse = |value: &serde_json::Value| oo_stack::parse(&value.to_string());
    assert_eq!(
        parse(&value).unwrap().sources[0].ssh.as_ref().unwrap().port,
        22
    );
    value["sources"][0]["ssh"]["port"] = serde_json::json!(0);
    assert!(parse(&value).is_err());
    value["sources"][0]["ssh"]["port"] = serde_json::json!(2222);
    assert!(parse(&value).is_ok());
    value["sources"][0]["ssh"]["password"] = serde_json::json!("inline");
    assert!(parse(&value).is_err());
    value["sources"][0]["ssh"]["password"] = serde_json::json!({"secret_ref":"jump-password"});
    value["sources"][0]["ssh"]["key"] = serde_json::json!({"secret_ref":"jump-key"});
    assert!(parse(&value).is_err());
    value["sources"][0]["ssh"]
        .as_object_mut()
        .unwrap()
        .remove("password");
    assert!(parse(&value).is_ok());
    value["sources"][0]["ssh"]
        .as_object_mut()
        .unwrap()
        .remove("key");
    assert!(parse(&value).is_err());
}
