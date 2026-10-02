use oo_p05_pg17_parser::Decision;
use oo_p05_pg17_parser::PARSER_VERSION_NUM;

fn allow_shape() -> Decision {
    let mut decision = Decision::deny("ALLOW_SYNTAX");
    decision.syntax_allow = true;
    decision.code = "ALLOW_SYNTAX".to_string();
    decision.parser_version = PARSER_VERSION_NUM;
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
    nonce.enforce_parent("other", 9);
    assert_eq!(nonce.code, "INVALID_OUTPUT");
    assert!(!nonce.syntax_allow);
}
