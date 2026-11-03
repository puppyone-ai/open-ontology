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
    assert!(
        parse("version: oo.stack/v1")
            .err()
            .unwrap()
            .contains("YAML_NOT_ADMITTED")
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
