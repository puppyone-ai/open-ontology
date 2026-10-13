//! Frozen response shapes that do not need a database.

use source_control_api::error::ApiError;
use source_control_api::http::connector_document;
use source_control_api::ids::{ident_ok, operation_id_ok};
use source_control_api::sha256::{action_hash, sha256_hex};

#[test]
fn connector_matrix_stays_schema_frozen() {
    let body = connector_document();
    assert_eq!(body["contract"], "oo.source-control-api/v1");
    assert_eq!(body["operation"], "getConnectorMatrix");
    assert_eq!(body["connectors"][0]["binding"], "schema_frozen");
    assert_eq!(body["connectors"][0]["enterprise"], false);
    assert_eq!(body["connectors"][0]["version"], "17.10");
    assert_eq!(body["connectors"][1]["strategy"], "replace");
    assert_eq!(body["build_review"], "not_claimed");
    assert_eq!(body["release"], "not_claimed");
    assert_eq!(body["customer_integration"], "not_claimed");
    assert!(body["not_claimed"].as_array().unwrap().iter().any(|item| item == "enterprise_source"));
    assert!(body["not_claimed"].as_array().unwrap().iter().any(|item| item == "build_review"));
}

#[test]
fn error_body_has_no_ready_status() {
    let body = ApiError::not_admitted().body();
    assert_eq!(body["code"], "source.not_admitted");
    assert_eq!(body["detail"], "host");
    assert_eq!(body["build_review"], "not_claimed");
    assert!(body.get("status").is_none());
    let missing = ApiError::forbidden("scope").body();
    let foreign = ApiError::forbidden("scope").body();
    assert_eq!(missing, foreign);
}

#[test]
fn idempotency_hash_includes_the_action() {
    let body = br#"{"source_ref":"shop_src"}"#;
    assert_ne!(action_hash("registerSource", body), action_hash("createImport", body));
    assert_eq!(action_hash("createImport", body), action_hash("createImport", body));
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}

#[test]
fn identifiers_match_the_frozen_patterns() {
    assert!(ident_ok("a383_main"));
    assert!(!ident_ok("A383"));
    assert!(operation_id_ok("op_0123456789abcdef0123456789abcdef"));
    assert!(!operation_id_ok("op_0123456789abcdef0123456789ABCDEF"));
}
