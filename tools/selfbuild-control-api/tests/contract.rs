use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use selfbuild_control_api::canonical::canonical_sha256;
use selfbuild_control_api::config::{https_origin_ok, load_config_bytes};
use selfbuild_control_api::identity::{self, IdentityFault};
use selfbuild_control_api::jsonutil::parse_exact;
use selfbuild_control_api::membership::reject_forged;
use selfbuild_control_api::pack::{self, envelope};
use selfbuild_control_api::scope::{self, allows, scopes_for};
use selfbuild_control_api::spawn::may_mark_canceled;
use selfbuild_control_api::stages::{self, Resume};
use selfbuild_control_api::suggest::{self, COMMAND_KEYS};
use selfbuild_control_api::{router, MIGRATION_SQL};
use serde_json::json;
use tower::ServiceExt;

fn sample_config() -> selfbuild_control_api::AppConfig {
    let commit = "a".repeat(64);
    let text = format!(
        r#"{{
            "contract": "oo.selfbuild-control-api.config/v1",
            "deployment_origins": ["https://control.example"],
            "listen": {{"bind": "127.0.0.1:8443", "cert_file": "/tmp/cert.pem", "key_file": "/tmp/key.pem"}},
            "identity": {{"origin": "https://127.0.0.1:1", "ca_file": "/tmp/ca.pem"}},
            "postgres": {{
                "host": "127.0.0.1", "port": 1, "database": "oo_selfbuild",
                "user": "oo_selfbuild_api_admin", "password_file": "/tmp/pw",
                "ca_file": "/tmp/ca.pem", "server_name": "127.0.0.1"
            }},
            "work_dir": "/tmp/oo-selfbuild-work",
            "binaries": {{
                "catalog": "/tmp/oo-pipeline", "pg_builder": "/tmp/pg-builder",
                "rules": "/tmp/selfbuild-rules", "suggestions": "/tmp/selfbuild-suggestions",
                "review_ledger": "/tmp/review-ledger"
            }},
            "projects": [{{
                "project_id": "demo",
                "config_commit": "{commit}",
                "source_ref": "src",
                "allowance_id": "allow",
                "llm_secret_ref": "secret://llm/demo",
                "source_secret_ref": "secret://pg/demo",
                "source": {{
                    "host": "127.0.0.1", "port": 5432, "database": "srcdb", "username": "reader",
                    "password_file": "/tmp/srcpw", "ca_file": "/tmp/ca.pem",
                    "tables": [{{"schema": "app", "table": "customer"}}]
                }},
                "sample_policy": "none",
                "rules_auto_accept": "none",
                "llm": {{"bearer_file": "/tmp/bearer", "origin": "http://127.0.0.1:8317", "model_id": "grok-4.7"}},
                "role_policy_file": "/tmp/roles.json",
                "hidden_columns": ["app.customer.email"],
                "visible_columns": ["app.customer.name"]
            }}],
            "ledger": {{
                "host": "127.0.0.1", "port": 1, "database": "ledgerdb", "user": "ledger_user",
                "password_file": "/tmp/lpw", "ca_file": "/tmp/ca.pem", "server_name": "127.0.0.1"
            }},
            "principals": []
        }}"#
    );
    load_config_bytes(text.as_bytes()).expect("config")
}

#[test]
fn canonical_known_vector() {
    let hash = canonical_sha256(&json!({"b": 2, "a": 1}));
    assert_eq!(hash, "43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777");
}

#[test]
fn duplicate_keys_and_trailing_bytes_are_malformed() {
    assert!(parse_exact(br#"{"a":1,"a":2}"#).is_err());
    assert!(parse_exact(br#"{"a":1} "#).is_ok());
    assert!(parse_exact(br#"{"a":1}{"b":2}"#).is_err());
    assert!(parse_exact(br#"{"authorized":true}"#).is_ok());
}

#[test]
fn scope_map_matches_the_frozen_matrix() {
    let admin = vec!["project.admin".to_owned(), "identity.current".to_owned()];
    assert!(allows(&admin, scope::BUILD_CREATE));
    assert!(allows(&admin, scope::PACK_COMPILE));
    assert!(!allows(&admin, scope::REVIEW_DECIDE));
    assert!(!allows(&admin, scope::CLASSIFICATION_CONFIRM));
    let reviewer = vec!["review.decide".to_owned()];
    assert!(allows(&reviewer, scope::REVIEW_DECIDE));
    assert!(!allows(&reviewer, scope::CLASSIFICATION_CONFIRM));
    assert!(!allows(&reviewer, scope::PACK_COMPILE));
    let auditor = vec!["audit.read".to_owned()];
    assert!(allows(&auditor, scope::PACK_READ));
    assert!(!allows(&auditor, scope::BUILD_CREATE));
    assert!(!allows(&auditor, scope::REVIEW_DECIDE));
    let owner = vec!["business.record".to_owned()];
    assert!(allows(&owner, scope::CLASSIFICATION_CONFIRM));
    assert!(!allows(&owner, scope::REVIEW_DECIDE));
    assert!(!allows(&owner, scope::BUILD_READ));
    let steward = vec!["ontology.draft".to_owned()];
    assert!(allows(&steward, scope::CLASSIFICATION_PREVIEW));
    assert!(!allows(&steward, scope::PACK_COMPILE));
    assert!(scopes_for(&["binding.read".to_owned(), "session.revoke".to_owned()]).is_empty());
}

#[test]
fn caller_json_is_never_authority() {
    let err = identity::try_from_caller_json(br#"{"authorized":true}"#).unwrap_err();
    assert_eq!(err, IdentityFault::Untyped);
    let envelope = current_envelope();
    let bytes = serde_json::to_vec(&envelope).unwrap();
    assert!(identity::try_from_caller_json(&bytes).is_err());
    let parsed = identity::parse_current_envelope(&bytes).expect("typed envelope");
    assert_eq!(parsed.subject, "user-1");
    assert_eq!(parsed.revision, 3);
    let mut wrong = envelope;
    wrong["install_state"] = json!("uninitialized");
    assert!(identity::parse_current_envelope(&serde_json::to_vec(&wrong).unwrap()).is_err());
    assert_eq!(identity::classify_status(302).unwrap_err(), IdentityFault::Redirect);
    assert_eq!(identity::classify_status(401).unwrap_err(), IdentityFault::Unauthenticated);
    assert!(identity::classify_status(200).is_ok());
}

#[test]
fn origin_pin_rejects_user_urls() {
    assert!(https_origin_ok("https://identity.example"));
    assert!(https_origin_ok("https://127.0.0.1:1"));
    assert!(!https_origin_ok("http://127.0.0.1:1"));
    assert!(!https_origin_ok("https://user:pass@127.0.0.1"));
    assert!(!https_origin_ok("https://127.0.0.1/control"));
    assert!(!https_origin_ok("https://*"));
    assert!(load_config_bytes(br#"{"contract":"oo.selfbuild-control-api.config/v1"}"#).is_err());
}

#[test]
fn membership_and_pack_do_not_forge_review_state() {
    assert!(reject_forged(&json!({"complete_command": "drop table"})).is_err());
    assert!(reject_forged(&json!({"project_id": "demo"})).is_ok());
    let producer = json!({
        "protocol": "oo.pg-builder.pack-combined/v1",
        "authenticated": false,
        "text": "UNREVIEWED role pack"
    });
    let wrapped = envelope("compileBuildPack", &format!("b_{}", "ab".repeat(16)), &producer);
    assert!(pack::review_state_preserved(&producer, &wrapped));
    assert!(wrapped.get("producer").unwrap().get("review.state").is_none());
    assert!(pack::unreviewed_prefix_kept(&producer, "text"));
    assert_eq!(wrapped["compiled_pack"], "not_published");
    assert_eq!(wrapped["producer_authenticated"], false);
    assert!(pack::require_two_roles(&json!({"roles": [{}, {}]})).is_ok());
    assert!(pack::require_two_roles(&json!({"roles": [{}]})).is_err());
}

#[test]
fn resume_does_not_replay_an_uncertain_model_stage() {
    assert_eq!(stages::resume_action("S5", "running", None), Resume::Uncertain);
    assert_eq!(stages::resume_action("S5", "running", Some("ab")), Resume::Skip);
    assert_eq!(stages::resume_action("S5", "succeeded", Some("ab")), Resume::Skip);
    assert_eq!(stages::resume_action("S0", "failed", None), Resume::Failed);
    assert_eq!(stages::resume_action("S9", "awaiting", None), Resume::Await);
    assert_eq!(stages::resume_action("S9", "succeeded", None), Resume::Skip);
    assert_eq!(stages::resume_action("S10", "succeeded", None), Resume::Skip);
    assert!(!may_mark_canceled(false));
    assert!(may_mark_canceled(true));
}

#[test]
fn catalog_budget_and_suggestion_command_shape() {
    let config = sample_config();
    let project = &config.projects[0];
    let stdin = stages::catalog_stdin(project);
    let value: serde_json::Value = serde_json::from_slice(&stdin).unwrap();
    assert_eq!(value["source"]["budget"]["lock_ms"], 1000);
    assert_eq!(value["source"]["budget"]["statement_ms"], 30000);
    assert_eq!(value["source"]["budget"]["connect_ms"], 30000);
    let catalog = json!({"protocol": "oo.source.pgsql/v1", "source_id": "shop_src", "tables": []});
    let profile = json!({
        "protocol": "oo.pg-builder.profile/v1",
        "tables": [{
            "schema": "app",
            "name": "customer",
            "reltuples": "1",
            "columns": [{"name": "name", "attnum": 1}]
        }]
    });
    let command = suggest::command("demo", &catalog, &profile).unwrap();
    let keys: Vec<&str> = command.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, COMMAND_KEYS);
    assert_eq!(command["authorization"]["authorized"], true);
    assert_eq!(command["scope"]["project"], "demo");
    assert_eq!(command["scope"]["role"], "oo_r_draft");
    assert_eq!(command["scope"]["source_id"], "shop_src");
    assert_eq!(command["scope"]["schema_version"], "oos/v1");
    assert_eq!(command["scope"]["model_scope"], "none");
    assert_eq!(command["scope"]["evidence_sha256"], command["authorization"]["catalog_sha256"]);
    assert!(command["profile"].get("semantic").is_none());
    assert!(stages::suggestions_args() == vec!["produce".to_owned()]);
    let rules = stages::rules_args("/c", "/p", "/d", "none");
    assert!(rules.contains(&"--auto-accept".to_owned()));
    assert!(!rules.iter().any(|arg| arg == "--config"));
}

#[test]
fn migration_does_not_grant_or_create_a_role() {
    for line in MIGRATION_SQL.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("--") {
            continue;
        }
        let upper = trimmed.to_ascii_uppercase();
        assert!(!upper.contains("GRANT "), "{trimmed}");
        assert!(!upper.contains("CREATE ROLE"), "{trimmed}");
    }
}

#[test]
fn classification_preview_hides_unknown_and_refuses_accept_all() {
    let visible = vec!["app.customer.name".to_owned()];
    let hidden = vec!["app.customer.email".to_owned()];
    let err = stages::preview_classification(&visible, &hidden, &[], true).unwrap_err();
    assert_eq!(err.code, "batch.accept_all");
    let err = stages::preview_classification(
        &visible,
        &hidden,
        &[("app.customer.name".to_owned(), "unknown".to_owned())],
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, "pii.unknown");
    let err = stages::preview_classification(&visible, &hidden, &[], false).unwrap_err();
    assert_eq!(err.code, "classification.incomplete");
    let err = stages::preview_classification(
        &visible,
        &hidden,
        &[("app.customer.email".to_owned(), "personal".to_owned())],
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, "membership.refused");
}

#[tokio::test]
async fn missing_cookie_is_401_and_closed_identity_is_503() {
    let app = router(sample_config());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/control/v1/builds/b_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let bytes = axum::body::to_bytes(response.into_body(), 65_536).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "unauthenticated");
    assert_eq!(value["management_identity"], "not_verified");

    let missing = router(sample_config())
        .oneshot(Request::builder().uri("/control/v1/nope").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let method = router(sample_config())
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/control/v1/builds/b_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/cancel")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);

    let commit = "a".repeat(64);
    let body = json!({
        "project_id": "demo",
        "identity_revision": 3,
        "config_commit": commit,
        "source_ref": "src",
        "allowance_id": "allow",
        "llm_secret_ref": "secret://llm/demo",
        "idempotency_key": "idempotency-key01"
    });
    let cookie = "a".repeat(43);
    let created = router(sample_config())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/control/v1/builds")
                .header("content-type", "application/json")
                .header("origin", "https://control.example")
                .header("x-oo-csrf", "c".repeat(43))
                .header("cookie", format!("__Host-oo-session={cookie}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(created.into_body(), 65_536).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], "dependency.not_ready");
    assert_eq!(value["compiled_pack"], "not_published");
    assert_eq!(value["management_identity"], "not_verified");
    let _ = Arc::new(cookie);
}

fn current_envelope() -> serde_json::Value {
    json!({
        "contract": "oo.control-identity/v1",
        "operation": "getIdentityCurrent",
        "subject": "user-1",
        "issuer": "https://idp.example",
        "config_commit": "a".repeat(64),
        "revision": 3,
        "install_state": "initialized",
        "projects": [{
            "project_id": "demo",
            "role": "admin",
            "capabilities": ["identity.current", "project.admin"]
        }],
        "csrf_token": "b".repeat(43),
        "session": {
            "idle_expires_at": "2026-10-08T00:00:00Z",
            "absolute_expires_at": "2026-10-09T00:00:00Z",
            "revoked": false
        },
        "authority": "management_session",
        "session_class": "management_cookie",
        "data_grants": "none",
        "owner_certification": "not_issued",
        "agent_session": "not_issued",
        "harness_session": "not_issued",
        "gateway_token": "not_issued",
        "customer_integration": "not_claimed"
    })
}
