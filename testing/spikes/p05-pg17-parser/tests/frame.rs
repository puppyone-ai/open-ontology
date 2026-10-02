use oo_p05_pg17_parser::Decision;
use oo_p05_pg17_parser::PARSER_VERSION_NUM;

fn allow_shape() -> Decision {
    let mut decision = Decision::deny("ALLOW_SYNTAX");
    decision.syntax_allow = true;
    decision.code = "ALLOW_SYNTAX".to_string();
    decision.parser_version = PARSER_VERSION_NUM;
    decision.executed_sql = "SELECT 1".to_string();
    decision.nonce = "abc".to_string();
    decision.pid = 9;
    decision
}

#[test]
fn parent_keeps_allow_only_when_every_check_matches() {
    let mut decision = allow_shape();
    decision.enforce_parent("abc", 9);
    assert!(decision.syntax_allow);
    assert_eq!(decision.code, "ALLOW_SYNTAX");
}

#[test]
fn parent_rejects_parser_mismatch_oid_claim_and_bad_nonce() {
    let mut mismatch = allow_shape();
    mismatch.parser_version = 1;
    mismatch.enforce_parent("abc", 9);
    assert!(!mismatch.syntax_allow);
    assert_eq!(mismatch.code, "PARSER_MISMATCH");

    let mut oid = allow_shape();
    oid.oid_acl_rls_proven = true;
    oid.enforce_parent("abc", 9);
    assert!(!oid.syntax_allow);
    assert!(!oid.oid_acl_rls_proven);
    assert_eq!(oid.code, "INVALID_OUTPUT");

    let mut nonce = allow_shape();
    nonce.executed_sql = "DROP TABLE analytics.orders".to_string();
    nonce.enforce_parent("other", 9);
    assert_eq!(nonce.code, "INVALID_OUTPUT");
    assert!(!nonce.syntax_allow);
    assert!(nonce.executed_sql.is_empty(), "bad nonce kept executable sql");

    let mut pid = allow_shape();
    pid.executed_sql = "DROP TABLE analytics.orders".to_string();
    pid.enforce_parent("abc", 8);
    assert_eq!(pid.code, "INVALID_OUTPUT");
    assert!(pid.executed_sql.is_empty());
}

#[test]
fn false_allow_syntax_never_becomes_true() {
    let mut decision = allow_shape();
    decision.syntax_allow = false;
    decision.executed_sql = "DROP TABLE analytics.orders".to_string();
    decision.enforce_parent("abc", 9);
    assert!(!decision.syntax_allow);
    assert_eq!(decision.code, "INVALID_OUTPUT");
    assert!(decision.executed_sql.is_empty());
    assert!(!decision.executed_sql.to_ascii_lowercase().contains("drop"));

    let mut wrong_version = decision_from(false, "ALLOW_SYNTAX", 1, false, "DROP TABLE analytics.orders");
    wrong_version.enforce_parent("abc", 9);
    assert!(!wrong_version.syntax_allow);
    assert_eq!(wrong_version.code, "INVALID_OUTPUT");
    assert_ne!(wrong_version.code, "PARSER_MISMATCH");
}

#[test]
fn contradictory_shapes_clear_sql_and_never_allow() {
    let codes = ["ALLOW_SYNTAX", "NOT_SELECT", "EMPTY_SQL", "PARSER_MISMATCH", "NOPE"];
    let versions = [PARSER_VERSION_NUM, 170010, 0];
    let sqls = ["", "SELECT 1", "DROP TABLE analytics.orders"];
    for allow in [false, true] {
        for code in codes {
            for version in versions {
                for oid in [false, true] {
                    for sql in sqls {
                        let mut decision = decision_from(allow, code, version, oid, sql);
                        decision.enforce_parent("abc", 9);
                        let positive = allow
                            && code == "ALLOW_SYNTAX"
                            && version == PARSER_VERSION_NUM
                            && !oid
                            && !sql.is_empty()
                            && !sql.as_bytes().contains(&0)
                            && sql.len() <= 64 * 1024;
                        assert_eq!(decision.syntax_allow, positive, "allow={allow} code={code} version={version} oid={oid} sql={sql}");
                        if !positive {
                            assert!(decision.executed_sql.is_empty(), "{code} kept sql");
                        } else {
                            assert_eq!(decision.executed_sql, sql);
                            assert_eq!(decision.code, "ALLOW_SYNTAX");
                        }
                        if !allow && code == "ALLOW_SYNTAX" {
                            assert_eq!(decision.code, "INVALID_OUTPUT");
                        }
                        if oid {
                            assert!(!decision.oid_acl_rls_proven);
                            assert_eq!(decision.code, "INVALID_OUTPUT");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn deny_with_sql_unknown_code_and_caps_are_invalid() {
    let mut deny_sql = decision_from(false, "NOT_SELECT", PARSER_VERSION_NUM, false, "DROP TABLE analytics.orders");
    deny_sql.enforce_parent("abc", 9);
    assert_eq!(deny_sql.code, "INVALID_OUTPUT");
    assert!(deny_sql.executed_sql.is_empty());

    let mut unknown = decision_from(false, "DROP_DATABASE", PARSER_VERSION_NUM, false, "");
    unknown.enforce_parent("abc", 9);
    assert_eq!(unknown.code, "INVALID_OUTPUT");

    let mut empty_allow = decision_from(true, "ALLOW_SYNTAX", PARSER_VERSION_NUM, false, "");
    empty_allow.enforce_parent("abc", 9);
    assert_eq!(empty_allow.code, "INVALID_OUTPUT");

    let mut nul = decision_from(true, "ALLOW_SYNTAX", PARSER_VERSION_NUM, false, "SELECT 1\0hidden");
    nul.enforce_parent("abc", 9);
    assert_eq!(nul.code, "INVALID_OUTPUT");
    assert!(nul.executed_sql.is_empty());

    let mut deep = allow_shape();
    deep.max_depth = 129;
    deep.enforce_parent("abc", 9);
    assert_eq!(deep.code, "INVALID_OUTPUT");

    let mut wide = allow_shape();
    wide.node_count = 10_001;
    wide.enforce_parent("abc", 9);
    assert_eq!(wide.code, "INVALID_OUTPUT");

    let mut detail = allow_shape();
    detail.detail = "d".repeat(2049);
    detail.enforce_parent("abc", 9);
    assert_eq!(detail.code, "INVALID_OUTPUT");
    assert!(detail.executed_sql.is_empty());

    let mut pre = decision_from(false, "EMPTY_SQL", 0, false, "");
    pre.enforce_parent("abc", 9);
    assert_eq!(pre.code, "EMPTY_SQL");

    let mut mismatch = decision_from(false, "PARSER_MISMATCH", 170010, false, "");
    mismatch.enforce_parent("abc", 9);
    assert_eq!(mismatch.code, "PARSER_MISMATCH");
    assert!(!mismatch.syntax_allow);

    let mut version_zero = decision_from(false, "NOT_SELECT", 0, false, "");
    version_zero.enforce_parent("abc", 9);
    assert_eq!(version_zero.code, "PARSER_MISMATCH");
}

#[test]
fn oversize_allow_sql_is_cleared() {
    let mut decision = allow_shape();
    decision.executed_sql = "S".repeat(70_000);
    decision.enforce_parent("abc", 9);
    assert_eq!(decision.code, "INVALID_OUTPUT");
    assert!(decision.executed_sql.is_empty());
    assert!(!decision.syntax_allow);
}

#[test]
fn worker_cannot_set_parent_fields_or_unknown_json() {
    let mut lied = allow_shape();
    lied.syntax_allow = false;
    lied.executed_sql = "DROP TABLE analytics.orders".to_string();
    lied.reaped = true;
    lied.workdir_removed = true;
    lied.containment = "proven".to_string();
    lied.worker_identity = "pinned".to_string();
    lied.cleanup_proof = "FINAL 0".to_string();
    let bytes = serde_json::to_vec(&lied).unwrap();
    let screened = oo_p05_pg17_parser::accept_worker_decision(&bytes, "abc", 9);
    assert!(!screened.syntax_allow);
    assert!(screened.executed_sql.is_empty());
    assert!(!screened.reaped);
    assert!(!screened.workdir_removed);
    assert!(screened.worker_identity.is_empty());
    assert!(screened.cleanup_proof.is_empty());
    assert_ne!(screened.containment, "proven");

    let mut value = serde_json::to_value(allow_shape()).unwrap();
    value.as_object_mut().unwrap().insert("injected".to_string(), serde_json::json!(true));
    let bytes = serde_json::to_vec(&value).unwrap();
    let screened = oo_p05_pg17_parser::accept_worker_decision(&bytes, "abc", 9);
    assert_eq!(screened.code, "INVALID_OUTPUT");
    assert!(!screened.syntax_allow);
    assert!(screened.executed_sql.is_empty());
}

fn decision_from(allow: bool, code: &str, version: i32, oid: bool, sql: &str) -> Decision {
    let mut decision = Decision::deny(code);
    decision.syntax_allow = allow;
    decision.code = code.to_string();
    decision.parser_version = version;
    decision.oid_acl_rls_proven = oid;
    decision.executed_sql = sql.to_string();
    decision.nonce = "abc".to_string();
    decision.pid = 9;
    decision
}
