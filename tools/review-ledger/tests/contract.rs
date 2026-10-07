//! Request, control, and field rules that do not need PostgreSQL.

#![forbid(unsafe_code)]

use serde_json::json;
use review_ledger::{
    canonical_sha, civil_date_ok, in_scope, parse_control, prepare, screen_request, sha256_hex,
    Principal,
};

const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn sha256_matches_the_empty_and_abc_vectors() {
    assert_eq!(sha256_hex(b"").unwrap(), EMPTY);
    assert_eq!(sha256_hex(b"abc").unwrap(), ABC);
}

#[test]
fn canonical_object_keys_are_sorted() {
    let text = review_ledger::canonical_json(&json!({"b": 1, "a": "x"}));
    assert_eq!(text, "{\"a\":\"x\",\"b\":1}");
    assert_eq!(canonical_sha(&json!({"b": 1, "a": "x"})).unwrap().len(), 64);
}

#[test]
fn control_rejects_a_second_role_and_verified_authority() {
    let mut value = control_value();
    value["principals"][0]["roles"] = json!(["owner"]);
    let error = parse_control(&serde_json::to_vec(&value).unwrap()).unwrap_err();
    assert_eq!(error.code, "control.role");

    let mut verified = control_value();
    verified["authority"] = json!("verified");
    let error = parse_control(&serde_json::to_vec(&verified).unwrap()).unwrap_err();
    assert_eq!(error.code, "authority.not_verified");

    let mut claimed = control_value();
    claimed["management_identity"] = json!("claimed");
    let error = parse_control(&serde_json::to_vec(&claimed).unwrap()).unwrap_err();
    assert_eq!(error.code, "authority.not_verified");

    let control = parse_control(&serde_json::to_vec(&control_value()).unwrap()).unwrap();
    assert_eq!(control.principals.len(), 1);
    assert_eq!(control.principals[0].role, "reviewer");
}

#[test]
fn scope_does_not_expand_from_a_dataset_to_a_column() {
    let principal = Principal {
        principal_id: "local-operator".to_owned(),
        role: "reviewer".to_owned(),
        reviewer_label: "local-operator".to_owned(),
        credential_sha256: EMPTY.to_owned(),
        scope: vec!["public.b".to_owned()],
    };
    assert!(in_scope(&principal, "public.b"));
    assert!(!in_scope(&principal, "public.b.id"));
}

#[test]
fn grant_keys_are_refused_before_a_decision_is_parsed() {
    let mut request = json!({
        "protocol": "oo.review-ledger.decide/v1",
        "project_id": "shop",
        "choice": "approve"
    });
    request["grant"] = json!(true);
    let error = screen_request(&serde_json::to_vec(&request).unwrap()).unwrap_err();
    assert_eq!(error.code, "not_grant");

    let mut nested = json!({"fields": {"publish": true}, "items": []});
    nested["items"] = json!([{"role": "owner"}]);
    let error = screen_request(&serde_json::to_vec(&nested).unwrap()).unwrap_err();
    assert_eq!(error.code, "not_grant");
}

#[test]
fn approval_and_classification_fields_are_exact() {
    let extra = json!({
        "label": {"overwrite": false},
        "description": {"overwrite": false},
        "grain": {"overwrite": false},
        "pii": "none"
    });
    let error = prepare("label_description", "approve", "public.b", Some(&extra)).unwrap_err();
    assert_eq!(error.code, "schema.invalid");

    let kept = json!({
        "label": {"overwrite": false},
        "description": {"overwrite": false}
    });
    let prepared = prepare("label_description", "approve", "public.b.id", Some(&kept)).unwrap();
    assert_eq!(prepared.kind, "label_description");
    assert_eq!(prepared.choice, "approve");

    let unknown = json!({
        "pii": "unknown",
        "semantic_type": "identifier",
        "state": "confirmed"
    });
    let error = prepare("classification", "confirm", "public.b.id", Some(&unknown)).unwrap_err();
    assert_eq!(error.code, "pii.unknown");

    let missing = json!({"semantic_type": "identifier", "state": "confirmed"});
    let error = prepare("classification", "confirm", "public.b.id", Some(&missing)).unwrap_err();
    assert_eq!(error.code, "pii.unknown");

    let confirmed = json!({
        "pii": "none",
        "semantic_type": "identifier",
        "state": "confirmed"
    });
    let prepared = prepare("classification", "confirm", "public.b.id", Some(&confirmed)).unwrap();
    assert_eq!(prepared.choice, "confirm");

    let error = prepare("label_description", "reject", "public.b", Some(&kept)).unwrap_err();
    assert_eq!(error.code, "schema.invalid");
    assert!(prepare("owner_certification", "record", "public.b", None).is_ok());
}

#[test]
fn civil_dates_follow_the_calendar() {
    assert!(civil_date_ok("2026-10-08"));
    assert!(civil_date_ok("2024-02-29"));
    assert!(!civil_date_ok("2023-02-29"));
    assert!(!civil_date_ok("2026-13-01"));
    assert!(!civil_date_ok("2026-10-8"));
}

fn control_value() -> serde_json::Value {
    json!({
        "protocol": "oo.review-ledger.control/v1",
        "project_id": "shop",
        "authority": "not_verified",
        "management_identity": "not_claimed",
        "principals": [{
            "principal_id": "local-operator",
            "role": "reviewer",
            "reviewer_label": "local-operator",
            "credential_sha256": EMPTY,
            "scope": ["public.b", "public.b.id"]
        }]
    })
}
