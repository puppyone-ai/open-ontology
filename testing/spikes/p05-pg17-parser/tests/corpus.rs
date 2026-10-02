use std::path::PathBuf;

use oo_p05_pg17_parser::{admit_in_worker, admit_native, sha256_file, Policy, PARSER_VERSION_NUM};

#[derive(serde::Deserialize)]
struct Corpus {
    cases: Vec<Case>,
    generated: Vec<Generated>,
}

#[derive(serde::Deserialize)]
struct Case {
    id: String,
    sql: String,
    code: String,
    star: bool,
    contains: Vec<String>,
    absent: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Generated {
    id: String,
    code: String,
}

fn worker_pin() -> (PathBuf, [u8; 32]) {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_oo-p05-parser-worker"));
    let pin = sha256_file(&path).expect("worker image");
    (path, pin)
}

fn load() -> (Policy, Corpus) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let policy = Policy::parse(&std::fs::read_to_string(root.join("fixtures/policy.json")).unwrap()).unwrap();
    let corpus: Corpus = serde_json::from_str(&std::fs::read_to_string(root.join("corpus/expected.json")).unwrap()).unwrap();
    (policy, corpus)
}

fn version_ok(code: &str, version: i32) {
    if matches!(code, "EMPTY_SQL" | "NUL_BYTE" | "OVERSIZE" | "SQL_PARSE_ERROR") {
        assert_eq!(version, 0, "{code}");
    } else {
        assert_eq!(version, PARSER_VERSION_NUM, "{code}");
    }
}

#[test]
fn native_matches_expected_corpus() {
    let (policy, corpus) = load();
    let (worker, pin) = worker_pin();
    for case in &corpus.cases {
        let native = admit_native(&case.sql, &policy);
        assert_eq!(native.code, case.code, "{} native sql={}", case.id, case.sql);
        assert_eq!(native.syntax_allow, case.code == "ALLOW_SYNTAX", "{}", case.id);
        assert!(!native.oid_acl_rls_proven, "{}", case.id);
        assert_eq!(native.original_projection_star, case.star, "{}", case.id);
        version_ok(&case.code, native.parser_version);
        if case.code == "ALLOW_SYNTAX" {
            let mut rest = native.executed_sql.as_str();
            for fragment in &case.contains {
                let Some(at) = rest.find(fragment) else {
                    panic!("{} missing {fragment} in {}", case.id, native.executed_sql);
                };
                rest = &rest[at + fragment.len()..];
            }
            for fragment in &case.absent {
                assert!(!native.executed_sql.contains(fragment), "{} contains {fragment}: {}", case.id, native.executed_sql);
            }
        } else {
            assert!(native.executed_sql.is_empty(), "{}", case.id);
        }

        let remote = admit_in_worker(&worker, &pin, &case.sql, &policy);
        assert_eq!(remote.code, case.code, "{} worker", case.id);
        assert_eq!(remote.syntax_allow, native.syntax_allow, "{}", case.id);
        assert!(!remote.oid_acl_rls_proven, "{}", case.id);
        assert_eq!(remote.executed_sql, native.executed_sql, "{}", case.id);
        if matches!(case.code.as_str(), "EMPTY_SQL" | "OVERSIZE") {
            assert_eq!(remote.pid, 0, "{}", case.id);
            assert!(!remote.reaped, "{}", case.id);
            assert_ne!(remote.containment, "proven", "{}", case.id);
        } else {
            assert!(remote.reaped, "{} not reaped", case.id);
            assert!(remote.workdir_removed, "{} workdir", case.id);
            assert_eq!(remote.containment, "proven", "{}", case.id);
            assert_eq!(remote.worker_identity, "pinned", "{}", case.id);
            assert!(remote.cleanup_wait_ms < 700, "{} cleanup {}", case.id, remote.cleanup_wait_ms);
            if std::path::Path::new("/proc/self/stat").exists() {
                assert!(remote.starttime_matched, "{}", case.id);
            }
        }
    }
}

#[test]
fn generated_limits_match_expected_codes() {
    let (policy, corpus) = load();
    let (worker, pin) = worker_pin();
    let expected = |id: &str| -> String {
        corpus.generated.iter().find(|item| item.id == id).unwrap().code.clone()
    };

    let mut oversize = String::from("SELECT 1 /*");
    oversize.push_str(&"x".repeat(70_000));
    oversize.push_str("*/");
    assert!(oversize.len() > 64 * 1024);
    let native = admit_native(&oversize, &policy);
    let remote = admit_in_worker(&worker, &pin, &oversize, &policy);
    assert_eq!(native.code, expected("oversize"));
    assert_eq!(remote.code, expected("oversize"));
    assert_eq!(remote.pid, 0);
    assert!(!remote.reaped);
    assert!(!remote.syntax_allow);

    let nul = "SELECT 1\0";
    assert_eq!(admit_native(nul, &policy).code, expected("nul"));
    let remote = admit_in_worker(&worker, &pin, nul, &policy);
    assert_eq!(remote.code, expected("nul"));
    assert_eq!(remote.pid, 0);

    // Fifteen nested scalar selects parse. Depth stays under 128 (48 at 15).
    // Sixteen is rejected by the vendored parser before the AST budget.
    // classify::tests::depth_budget_is_one_past_128 locks the walker budget.
    let mut deep = String::from("SELECT 1");
    for _ in 0..20 {
        deep = format!("SELECT ({deep})");
    }
    let native = admit_native(&deep, &policy);
    assert_eq!(native.code, expected("depth"), "depth sql_len={}", deep.len());
    let remote = admit_in_worker(&worker, &pin, &deep, &policy);
    assert_eq!(remote.code, expected("depth"));
    assert!(remote.reaped);
    assert!(!remote.syntax_allow);

    let mut wide = String::from("SELECT 1");
    for _ in 1..6000 {
        wide.push_str(",1");
    }
    assert!(wide.len() <= 64 * 1024, "wide sql {}", wide.len());
    let native = admit_native(&wide, &policy);
    assert_eq!(native.code, expected("nodes"), "nodes={} depth={}", native.node_count, native.max_depth);
    let remote = admit_in_worker(&worker, &pin, &wide, &policy);
    assert_eq!(remote.code, expected("nodes"));
    assert!(remote.reaped);
    assert!(!remote.syntax_allow);
}

#[test]
fn policy_rejects_visible_columns_outside_the_declared_order() {
    let err = Policy::parse(r#"{"relations":{"a.b":{"columns":["id"],"visible":["secret"],"large":false}}}"#);
    assert!(err.is_err());
}

/// a056 showed `SET LOCAL oo.claims` is not stopped by revoking `set_config`.
/// The utility statement is rejected here, before any database send.
/// `SELECT set_config(...)` is still only a syntax allow. It is not an OID or ACL proof.
#[test]
fn set_utility_is_denied_and_set_config_call_is_not_an_acl_proof() {
    let (policy, corpus) = load();
    let set_local = corpus.cases.iter().find(|case| case.id == "set-local-claims").unwrap();
    let call = corpus.cases.iter().find(|case| case.id == "set-config-call-not-oid-proof").unwrap();
    let denied = admit_native(&set_local.sql, &policy);
    assert_eq!(denied.code, "UTILITY");
    assert!(!denied.syntax_allow);
    assert!(denied.executed_sql.is_empty());
    let allowed = admit_native(&call.sql, &policy);
    assert_eq!(allowed.code, "ALLOW_SYNTAX");
    assert!(!allowed.oid_acl_rls_proven);
}
