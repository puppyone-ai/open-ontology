#![forbid(unsafe_code)]

use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use secret_control_api::{canonical, load_config, open, seal, try_from_caller_json, Scope, MIGRATION};

fn decode_hex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    assert_eq!(bytes.len() % 2, 0, "hex length");
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let high = hex_value(bytes[index]);
        let low = hex_value(bytes[index + 1]);
        out.push((high << 4) | low);
        index += 2;
    }
    out
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => panic!("hex digit"),
    }
}

#[test]
fn nist_aes_256_gcm_empty_plaintext_matches_ring() {
    let key = decode_hex("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308");
    let nonce_bytes = decode_hex("cafebabefacedbaddecaf888");
    let mut nonce = [0_u8; 12];
    nonce.copy_from_slice(&nonce_bytes);
    let sealed = seal(&key, &nonce, b"", b"").expect("seal");
    let expected = decode_hex("fd2caa16a5832e76aa132c1453eeda7e");
    assert_eq!(sealed, expected, "nist tag");
    let opened = open(&key, &nonce, b"", &sealed).expect("open");
    assert!(opened.is_empty(), "empty plaintext");
    assert!(open(&key, &nonce, b"x", &sealed).is_err(), "wrong aad");
    let mut wrong_key = key.clone();
    wrong_key[0] ^= 0x01;
    assert!(open(&wrong_key, &nonce, b"", &sealed).is_err(), "wrong key");
    assert!(seal(&[0_u8; 16], &nonce, b"", b"ab").is_err(), "short key");
}

#[test]
fn aad_changes_when_namespace_changes() {
    let left = Scope { deployment: "lab".into(), project: "alpha".into(), namespace: "builder".into(), secret_id: "db".into() };
    let mut right = left.clone();
    right.namespace = "agent".into();
    assert_ne!(canonical(&left, 1), canonical(&right, 1), "namespace is bound");
    assert_ne!(canonical(&left, 1), canonical(&left, 2), "version is bound");
    let key = [7_u8; 32];
    let nonce = [9_u8; 12];
    let sealed = seal(&key, &nonce, &canonical(&left, 1), b"0123456789abcdef0123456789abcdef").expect("seal");
    assert!(open(&key, &nonce, &canonical(&right, 1), &sealed).is_err(), "cross namespace");
    assert!(!sealed.windows(8).any(|window| window == b"01234567"), "ciphertext hides the prefix");
}

#[test]
fn caller_json_is_not_authority() {
    let error = try_from_caller_json(br#"{"contract":"oo.control-identity/v1","authorized":true}"#).expect_err("caller");
    assert_eq!(error.code, "unauthenticated");
}

#[test]
fn migration_has_no_plaintext_column_and_defers_the_head_key() {
    let sql = MIGRATION.to_ascii_lowercase();
    assert!(!sql.contains("plaintext"), "migration text");
    assert!(!sql.contains("secret_value"), "migration text");
    assert!(sql.contains("ring-aes-256-gcm"), "cipher");
    assert!(sql.contains("deferrable initially deferred"), "deferred fk");
    assert!(sql.contains("oo_secret_runtime"), "runtime role");
    assert!(!sql.contains("grant delete"), "no delete grant");
    let schema = include_str!("../schema/metadata.schema.json");
    assert!(schema.contains("\"additionalProperties\": false"), "metadata schema");
    assert!(!schema.contains("\"value\""), "metadata has no value");
    let resolve = include_str!("../schema/resolve.schema.json");
    assert!(resolve.contains("\"value\""), "resolve carries value");
    let openapi = include_str!("../openapi/secret-control-api.openapi.yaml");
    assert!(openapi.contains("putSecret"), "openapi");
    assert!(openapi.contains("/component/v1/resolve"), "resolve path");
}

#[test]
fn kms_refuses_before_opening_a_key_file() {
    let dir = std::env::temp_dir().join(format!("oo-a384-kms-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    let missing = dir.join("missing-master.key");
    let config_path = write_kms_config(&dir, &missing);
    let Err(error) = load_config(&config_path) else {
        panic!("kms");
    };
    assert_eq!(error.code, "kms.external");
    assert_eq!(error.exit_code, 2);
    assert!(!missing.exists(), "master key file was not opened");
    let _ = fs::remove_dir_all(&dir);
}

fn write_kms_config(dir: &Path, missing: &Path) -> PathBuf {
    let body = format!(
        r#"{{
            "https_origin": "https://127.0.0.1:1",
            "listen_host": "127.0.0.1",
            "management_port": 1,
            "component_port": 2,
            "tls_cert_file": "missing.pem",
            "tls_key_file": "missing.key",
            "client_ca_file": "missing-ca.pem",
            "identity_origin": "https://127.0.0.1:3",
            "identity_ca_file": "missing-idp.pem",
            "deployment": "lab",
            "key_source": "kms",
            "master_key_file": "{}",
            "pg": {{
                "host": "127.0.0.1",
                "port": 5432,
                "database": "secret",
                "user": "oo_secret_admin",
                "password_file": "missing.pass",
                "ca_file": "missing-pg.pem"
            }}
        }}"#,
        missing.display()
    );
    let path = dir.join("config.json");
    let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).expect("config");
    use std::io::Write;
    file.write_all(body.as_bytes()).expect("write");
    path
}
