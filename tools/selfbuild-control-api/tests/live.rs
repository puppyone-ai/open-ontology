use selfbuild_control_api::config::Endpoint;
use selfbuild_control_api::store::{self, NewBuild};

fn endpoint_from_env() -> Option<Endpoint> {
    let host = std::env::var("OO_SELFBUILD_PG_HOST").ok()?;
    let port = std::env::var("OO_SELFBUILD_PG_PORT").ok()?.parse().ok()?;
    let database = std::env::var("OO_SELFBUILD_PG_DB").ok()?;
    let user = std::env::var("OO_SELFBUILD_PG_USER").ok()?;
    let password_file = std::env::var("OO_SELFBUILD_PG_PASSWORD_FILE").ok()?;
    let ca_file = std::env::var("OO_SELFBUILD_PG_CA").ok()?;
    Some(Endpoint {
        host: host.clone(),
        port,
        database,
        user,
        password_file,
        ca_file,
        server_name: host,
    })
}

#[tokio::test]
async fn install_audit_and_idempotency() {
    let Some(endpoint) = endpoint_from_env() else {
        if std::env::var("OO_SELFBUILD_REQUIRE_PG").ok().as_deref() == Some("1") {
            panic!("private postgres was required and was not configured");
        }
        return;
    };
    store::install_schema(&endpoint).await.expect("install");
    store::install_schema(&endpoint).await.expect("replay install");
    store::audit_update_fails(&endpoint).await.expect("insert-only audit");
    let commit = "c".repeat(64);
    let hash = "d".repeat(64);
    let job = NewBuild {
        build_id: store::new_build_id().await.expect("id"),
        project_id: "demo".to_owned(),
        idempotency_key: live_key(),
        body_sha256: hash.clone(),
        config_commit: commit.clone(),
        source_ref: "src".to_owned(),
        allowance_id: "allow".to_owned(),
        llm_secret_ref: "secret://llm/demo".to_owned(),
        identity_revision: 1,
        subject: "user-1".to_owned(),
        role_name: "admin".to_owned(),
        input_sha256: hash.clone(),
    };
    let created = store::create_build(&endpoint, &job).await.expect("create");
    let again = store::create_build(&endpoint, &job).await.expect("replay");
    match (created, again) {
        (store::CreateOutcome::Created(first), store::CreateOutcome::Replay(second)) => {
            assert_eq!(first.build_id, second.build_id);
        }
        other => panic!("unexpected idempotency pair {other:?}"),
    }
    let mut changed = job;
    changed.build_id = store::new_build_id().await.expect("id");
    changed.body_sha256 = "e".repeat(64);
    let conflict = store::create_build(&endpoint, &changed).await.expect("conflict");
    assert!(matches!(conflict, store::CreateOutcome::Conflict));
    let _ = commit;
}

fn live_key() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!("idem-live-{millis:016x}")
}
