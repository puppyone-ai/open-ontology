//! Contract checks that do not need PostgreSQL.

#![forbid(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use artifact_control_api::{
    column_ok, error_body, grant_key, idempotency_ok, intake_name_ok, load_config, read_scope_files, scan_secrets, sha256_hex, token_ok, write_scope_files, Fault,
    Scope, CONTRACT,
};

#[test]
fn fault_table_matches_the_contract() {
    let rows = [
        (Fault::malformed(), 400, false),
        (Fault::self_asserted(), 400, false),
        (Fault::path(), 400, false),
        (Fault::budget(), 400, false),
        (Fault::unauthenticated(), 401, false),
        (Fault::forbidden(), 403, false),
        (Fault::scope_rejected(), 403, false),
        (Fault::origin_rejected(), 403, false),
        (Fault::csrf_rejected(), 403, false),
        (Fault::workspace_disabled(), 403, false),
        (Fault::aead(), 403, false),
        (Fault::revoked(), 403, false),
        (Fault::expired(), 403, false),
        (Fault::not_found(), 404, false),
        (Fault::method_not_allowed(), 405, false),
        (Fault::session_extended(), 409, false),
        (Fault::idempotency_conflict(), 409, false),
        (Fault::not_ready(), 503, true),
        (Fault::commit_unknown(), 503, true),
    ];
    for (fault, http, retryable) in rows {
        assert_eq!(fault.http, http, "{}", fault.code);
        let body = error_body(&fault);
        assert_eq!(body["contract"], CONTRACT);
        assert_eq!(body["code"], fault.code);
        assert_eq!(body["retryable"], retryable);
        assert!(body["signed_url"].is_null());
        assert_eq!(body["workspace"], "disabled");
        assert_eq!(body["production_signed"], false);
        assert_eq!(body["runsc"], "not_claimed");
        assert_eq!(body["customer_integration"], "not_claimed");
        assert_eq!(body["parent_complete"], false);
    }
}

#[test]
fn scope_mac_roundtrip_and_rejection() {
    let dir = scratch("scope");
    let scope_path = dir.join("scope.json");
    let mac_path = dir.join("scope.mac");
    let key = [7_u8; 32];
    let scope = sample(1_800_000_000);
    write_scope_files(&scope_path, &mac_path, &scope, &key).unwrap();
    let loaded = read_scope_files(&scope_path, &mac_path, &key).unwrap();
    assert_eq!(loaded, scope);
    let canonical = fs::read(&scope_path).unwrap();
    let text = String::from_utf8(canonical.clone()).unwrap();
    assert!(text.starts_with("{\"version\":\"oo.artifact-control/scope/v1\",\"principal_id\":\"clerk\""));
    assert!(text.contains("\"absolute_end_unix\":1800000000,\"revision\":1}"));
    assert_eq!(canonical, scope.canonical().unwrap().into_bytes());

    let mut edited = text;
    let revision = edited.rfind('1').unwrap();
    edited.replace_range(revision..revision + 1, "2");
    fs::write(&scope_path, &edited).unwrap();
    fs::set_permissions(&scope_path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(read_scope_files(&scope_path, &mac_path, &key).unwrap_err(), Fault::scope_rejected());

    write_scope_files(&scope_path, &mac_path, &scope, &key).unwrap();
    let link = dir.join("linked.json");
    fs::remove_file(&scope_path).unwrap();
    std::os::unix::fs::symlink(&link, &scope_path).unwrap();
    assert_eq!(read_scope_files(&scope_path, &mac_path, &key).unwrap_err(), Fault::scope_rejected());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn predicates_refuse_urls_and_accept_the_scan_marker() {
    assert!(intake_name_ok("query-clerk-order_doc-star.stdout"));
    assert!(!intake_name_ok("../secret"));
    assert!(!intake_name_ok("a/b"));
    assert!(!intake_name_ok("http://example"));
    assert!(!intake_name_ok("https://example"));
    assert!(column_ok("-----BEGIN PRIVATE KEY-----"));
    assert!(!column_ok("https://example"));
    assert!(idempotency_ok("accept-object-a-0001"));
    assert!(!idempotency_ok("short"));
    assert!(token_ok(&"a".repeat(43).replace('a', "A")));
    assert!(!token_ok("short"));
    assert!(grant_key("principal_id"));
    assert!(grant_key("signed_url"));
    assert!(!grant_key("source_file"));
    let key = [9_u8; 32];
    assert!(scan_secrets(b"-----BEGIN PRIVATE KEY-----", &key));
    assert!(scan_secrets(b"-----BEGIN RSA PRIVATE KEY-----", &key));
    assert!(scan_secrets(b"-----BEGIN OPENSSH PRIVATE KEY-----", &key));
    assert!(scan_secrets(b"prefix AKIAIOSFODNN7EXAMPLE suffix", &key));
    assert!(scan_secrets(&key, &key));
    assert!(!scan_secrets(b"order_id,customer_id", &key));
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn workspace_true_refuses_startup() {
    let dir = scratch("config");
    let path = dir.join("config.json");
    fs::write(&path, br#"{"version":"oo.artifact-control/v1","workspace_enabled":true}"#).unwrap();
    let loaded = load_config(&path);
    let Err(fault) = loaded else {
        panic!("workspace true was accepted");
    };
    assert_eq!(fault, Fault::workspace_disabled());
    let _ = fs::remove_dir_all(&dir);
}

fn sample(end: i64) -> Scope {
    Scope {
        principal_id: "clerk".to_owned(),
        project_id: "shop".to_owned(),
        session_id: "session_a398".to_owned(),
        role: "clerk".to_owned(),
        policy_id: "policy_v1".to_owned(),
        knowledge_id: "pack_v1".to_owned(),
        knowledge_digest: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
        absolute_end_unix: end,
        revision: 1,
    }
}

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("oo-a398-{name}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}
