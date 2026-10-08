//! Real control-identity current envelope and review-ledger decide.
//! Caller JSON is never authority. The live half runs only when the fixed
//! service is reachable and OO_SELFBUILD_REQUIRE_IDENTITY=1.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use selfbuild_control_api::config::{load_config_bytes, Endpoint};
use selfbuild_control_api::identity::{self, IdentityFault};
use selfbuild_control_api::membership::Member;
use selfbuild_control_api::store::{self, NewBuild};
use selfbuild_control_api::router;
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn caller_json_is_not_authority() {
    let forged = br#"{"contract":"oo.control-identity/v1","operation":"getIdentityCurrent","authority":"management_session","authorized":true}"#;
    assert_eq!(identity::try_from_caller_json(forged).err(), Some(IdentityFault::Untyped));
}

#[tokio::test]
async fn current_and_reject_are_durable() {
    if std::env::var("OO_SELFBUILD_REQUIRE_IDENTITY").ok().as_deref() != Some("1") {
        return;
    }
    let origin = std::env::var("OO_SELFBUILD_IDENTITY_ORIGIN").expect("origin");
    let ca = std::fs::read(std::env::var("OO_SELFBUILD_IDENTITY_CA").expect("ca")).expect("ca bytes");
    let cookie = read_trim(&std::env::var("OO_SELFBUILD_IDENTITY_COOKIE_FILE").expect("cookie file"));
    let verified = identity::fetch_current(&origin, &ca, &cookie).await.expect("current");
    assert_eq!(verified.subject, "fixture-reviewer");
    assert!(verified.revision >= 1);
    assert!(verified.project("beta").is_some_and(|project| {
        project.role == "reviewer" && project.capabilities.iter().any(|item| item == "review.decide")
    }));
    assert!(identity::try_from_caller_json(br#"{"subject":"fixture-reviewer","authorized":true}"#).is_err());
    eprintln!("identity_joint=current");
    if std::env::var("OO_SELFBUILD_REQUIRE_LEDGER").ok().as_deref() != Some("1") {
        return;
    }
    let config = joint_config(&origin, &verified);
    let endpoint = service_pg();
    store::install_schema(&endpoint).await.expect("api install");
    let hash = "ab".repeat(32);
    let build_id = store::new_build_id().await.expect("id");
    let created = store::create_build(
        &endpoint,
        &NewBuild {
            build_id: build_id.clone(),
            project_id: "beta".to_owned(),
            idempotency_key: format!("buildkey{:08}", std::process::id()),
            body_sha256: hash.clone(),
            config_commit: "b".repeat(64),
            source_ref: "src".to_owned(),
            allowance_id: "allow".to_owned(),
            llm_secret_ref: "secret://llm/beta".to_owned(),
            identity_revision: verified.revision,
            subject: verified.subject.clone(),
            role_name: "reviewer".to_owned(),
            input_sha256: hash.clone(),
        },
    )
    .await
    .expect("create");
    let build_id = match created {
        store::CreateOutcome::Created(row) | store::CreateOutcome::Replay(row) => row.build_id,
        store::CreateOutcome::Conflict | store::CreateOutcome::Unknown => panic!("create outcome"),
    };
    let client = store::connect(&endpoint).await.expect("connect");
    store::insert_member(
        &client,
        &build_id,
        &Member {
            project_id: "beta".to_owned(),
            object_id: "public.party.note".to_owned(),
            object_kind: "column".to_owned(),
            schema_name: "public".to_owned(),
            table_name: "party".to_owned(),
            column_name: Some("note".to_owned()),
            candidate_sha256: hash.clone(),
            source_sha256: hash.clone(),
            schema_sha256: hash.clone(),
            evidence_sha256: hash.clone(),
            generation_sha256: hash.clone(),
        },
    )
    .await
    .expect("member");
    let key = format!("decide{:016x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    let body = json!({
        "build_id": build_id,
        "identity_revision": verified.revision,
        "expected_revision": 1,
        "idempotency_key": key,
        "decision_kind": "label_description",
        "choice": "reject",
        "candidate_sha256": hash
    });
    let wrong = call(&config, &origin, &cookie, &"b".repeat(43), &build_id, &body).await;
    assert_eq!(wrong.0, StatusCode::FORBIDDEN, "{}", wrong.1);
    assert_eq!(wrong.1["error"]["code"], "csrf.rejected");
    assert_eq!(wrong.1["management_identity"], "not_verified");
    assert_eq!(wrong.1["published"], false);
    let first = call(&config, &origin, &cookie, &verified.csrf_token, &build_id, &body).await;
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(first.1["ledger_authenticated"], false);
    assert_eq!(first.1["management_identity"], "verified");
    assert_eq!(first.1["published"], false);
    assert_eq!(first.1["grants"], "not_authorized");
    assert_eq!(first.1["code"], "persisted");
    let refreshed = identity::fetch_current(&origin, &ca, &cookie).await.expect("csrf refresh");
    let replay = call(&config, &origin, &cookie, &refreshed.csrf_token, &build_id, &body).await;
    assert_eq!(replay.0, StatusCode::OK, "{}", replay.1);
    assert_eq!(replay.1["code"], "replay");
    assert_eq!(replay.1["ledger_authenticated"], false);
    let mut other = body.clone();
    other["choice"] = json!("defer");
    let refreshed = identity::fetch_current(&origin, &ca, &cookie).await.expect("csrf refresh");
    let conflict = call(&config, &origin, &cookie, &refreshed.csrf_token, &build_id, &other).await;
    assert_eq!(conflict.0, StatusCode::CONFLICT, "{}", conflict.1);
    assert_eq!(conflict.1["error"]["code"], "idempotency.conflict");
    let anon = router(config.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/control/v1/reviews/{}/decision", "public.party.note"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anon.status(), StatusCode::UNAUTHORIZED);
    eprintln!("identity_joint=ledger");
}

fn joint_config(origin: &str, verified: &selfbuild_control_api::identity::VerifiedIdentity) -> selfbuild_control_api::AppConfig {
    let dir = PathBuf::from(std::env::var("OO_SELFBUILD_WORK_DIR").expect("work"));
    let _ = std::fs::create_dir_all(&dir);
    let credential = dir.join("reviewer.cred");
    std::fs::write(&credential, b"reviewer-credential-fixture-beta-0001").expect("cred");
    let ledger_bin = std::env::var("OO_SELFBUILD_LEDGER_BIN").expect("ledger bin");
    let pg = service_pg();
    let ledger = ledger_pg();
    let text = json!({
        "contract": "oo.selfbuild-control-api.config/v1",
        "deployment_origins": [origin],
        "listen": {"bind": "127.0.0.1:8443", "cert_file": "/tmp/cert.pem", "key_file": "/tmp/key.pem"},
        "identity": {"origin": origin, "ca_file": std::env::var("OO_SELFBUILD_IDENTITY_CA").unwrap()},
        "postgres": endpoint_json(&pg),
        "work_dir": dir.display().to_string(),
        "binaries": {
            "catalog": "/tmp/oo-pipeline",
            "pg_builder": "/tmp/pg-builder",
            "rules": "/tmp/selfbuild-rules",
            "suggestions": "/tmp/selfbuild-suggestions",
            "review_ledger": ledger_bin
        },
        "projects": [{
            "project_id": "beta",
            "config_commit": "b".repeat(64),
            "source_ref": "src",
            "allowance_id": "allow",
            "llm_secret_ref": "secret://llm/beta",
            "source_secret_ref": "secret://pg/beta",
            "source": {
                "host": "127.0.0.1",
                "port": 5432,
                "database": "srcdb",
                "username": "reader",
                "password_file": pg.password_file,
                "ca_file": pg.ca_file,
                "tables": [{"schema": "public", "table": "party"}]
            },
            "sample_policy": "none",
            "rules_auto_accept": "none",
            "llm": {"bearer_file": "/tmp/bearer", "origin": "http://127.0.0.1:8317", "model_id": "grok-4.7"},
            "role_policy_file": "/tmp/roles.json",
            "hidden_columns": ["public.party.email"],
            "visible_columns": ["public.party.note"]
        }],
        "ledger": endpoint_json(&ledger),
        "principals": [{
            "subject": verified.subject,
            "ledger_role": "reviewer",
            "principal_id": "fixture.reviewer",
            "reviewer_label": "fixture-reviewer",
            "credential_file": credential.display().to_string(),
            "object_scope": ["public.party.note"]
        }]
    });
    load_config_bytes(&serde_json::to_vec(&text).unwrap()).expect("config")
}

fn endpoint_json(endpoint: &Endpoint) -> Value {
    json!({
        "host": endpoint.host,
        "port": endpoint.port,
        "database": endpoint.database,
        "user": endpoint.user,
        "password_file": endpoint.password_file,
        "ca_file": endpoint.ca_file,
        "server_name": endpoint.server_name
    })
}

fn service_pg() -> Endpoint {
    endpoint_from("OO_SELFBUILD_PG")
}

fn ledger_pg() -> Endpoint {
    endpoint_from("OO_SELFBUILD_LEDGER")
}

fn endpoint_from(prefix: &str) -> Endpoint {
    let host = std::env::var(format!("{prefix}_HOST")).expect("host");
    Endpoint {
        port: std::env::var(format!("{prefix}_PORT")).expect("port").parse().expect("port number"),
        database: std::env::var(format!("{prefix}_DB")).expect("db"),
        user: std::env::var(format!("{prefix}_USER")).expect("user"),
        password_file: std::env::var(format!("{prefix}_PASSWORD_FILE")).expect("password file"),
        ca_file: std::env::var(format!("{prefix}_CA")).expect("ca"),
        server_name: host.clone(),
        host,
    }
}

async fn call(
    config: &selfbuild_control_api::AppConfig,
    origin: &str,
    cookie: &str,
    csrf: &str,
    build_id: &str,
    body: &Value,
) -> (StatusCode, Value) {
    let _ = build_id;
    let response = router(config.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/control/v1/reviews/public.party.note/decision")
                .header("content-type", "application/json")
                .header("origin", origin)
                .header("x-oo-csrf", csrf)
                .header("cookie", format!("__Host-oo-session={cookie}"))
                .body(Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 65_536).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn read_trim(path: &str) -> String {
    let mut text = std::fs::read_to_string(path).expect("read");
    while text.ends_with(['\n', '\r']) {
        text.pop();
    }
    text
}
