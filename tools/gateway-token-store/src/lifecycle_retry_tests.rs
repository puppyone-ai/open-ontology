use super::*;

fn grant() -> Grant {
    Grant {
        project_id: "retry-fixture".into(),
        principal_id: "owner".into(),
        role: "reader".into(),
        claims: BTreeMap::new(),
        policy_version: "v1".into(),
        knowledge_version: "v1".into(),
        audience_url: "https://gateway.example/v1".into(),
        ttl_ms: Some(DAY),
        purpose: "build".into(),
    }
}

fn material() -> String {
    let mut bytes = legacy::draw_32().unwrap();
    let token = legacy::PlainToken::issue(TokenKind::Client, &bytes).reveal_once();
    bytes.zeroize();
    token
}

fn retry(grant: Grant, key: &str, token: &str) -> Operation {
    Operation::IssueRetry {
        grant,
        key: key.into(),
        token: Zeroizing::new(token.into()),
    }
}

#[test]
fn retry_request_is_optional_and_strict() {
    let ordinary: AdminRequest =
        serde_json::from_value(json!({"operation":"issue","grant":grant()})).unwrap();
    assert!(matches!(Operation::from(ordinary), Operation::Issue(_)));
    let token = material();
    let request = json!({"operation":"issue","grant":grant(),
        "retry":{"key":"first-user","token":token}});
    let parsed: AdminRequest = serde_json::from_value(request.clone()).unwrap();
    assert!(matches!(
        Operation::from(parsed),
        Operation::IssueRetry { .. }
    ));
    let mut extra = request;
    extra["retry"]["unsafe"] = json!(true);
    assert!(serde_json::from_value::<AdminRequest>(extra).is_err());
}

#[tokio::test]
#[ignore = "requires owned PostgreSQL; run test_issue_retry.py"]
async fn owned_pg_issue_retry() {
    use std::{fs, path::PathBuf};
    let root = PathBuf::from(std::env::var("OO_RETRY_FIXTURE").unwrap());
    let endpoint: Value =
        serde_json::from_slice(&fs::read(root.join("endpoint.json")).unwrap()).unwrap();
    let port = endpoint["port"].as_u64().unwrap() as u16;
    let password = Zeroizing::new(fs::read_to_string(root.join("admin.pass")).unwrap());
    let control = Control::new(
        Endpoint {
            host: "localhost".into(),
            port,
            database: "postgres".into(),
        },
        password.to_string(),
        fs::read(root.join("ca.crt")).unwrap(),
    );
    control.install().await.unwrap();
    control.install().await.unwrap();
    let mut cfg = tokio_postgres::Config::new();
    cfg.host("localhost")
        .port(port)
        .user("oo_token_admin")
        .dbname("postgres")
        .password(password.as_str());
    let (db, connection) = cfg.connect(tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    let count =
        || {
            db.query_one(
        "SELECT count(*) FROM oo_token_control.token_record WHERE project_id='retry-fixture'", &[])
        };
    let now = now_ms().unwrap();
    let token = Zeroizing::new(material());
    let first = control
        .execute_at(retry(grant(), "first-user", &token), "owner", now)
        .await
        .unwrap();
    // Simulate a committed request whose response was lost. Retry at a new time.
    let replay = control
        .execute_at(retry(grant(), "first-user", &token), "owner", now + 1)
        .await
        .unwrap();
    assert_eq!(first.metadata, replay.metadata);
    assert!(first.secret.as_deref() == replay.secret.as_deref());
    assert_eq!(count().await.unwrap().get::<_, i64>(0), 1);
    let mut changed = grant();
    changed.role = "other".into();
    for (g, actor, credential) in [
        (changed, "owner", token.to_string()),
        (grant(), "other-owner", token.to_string()),
        (grant(), "owner", material()),
    ] {
        assert_eq!(
            control
                .execute_at(retry(g, "first-user", &credential), actor, now + 2)
                .await
                .err(),
            Some("idempotency_conflict")
        );
    }
    // Both requests race the same project lock, with a different idempotency key.
    let race_token = Zeroizing::new(material());
    let (a, b) = tokio::join!(
        control.execute(retry(grant(), "race", &race_token), "owner"),
        control.execute(retry(grant(), "race", &race_token), "owner")
    );
    assert_eq!(a.unwrap().metadata, b.unwrap().metadata);
    assert_eq!(count().await.unwrap().get::<_, i64>(0), 2);
    let hash = first.metadata["token_sha256"].as_str().unwrap().to_owned();
    control
        .execute_at(
            Operation::Revoke {
                project: grant().project_id,
                hash,
            },
            "owner",
            now + 5,
        )
        .await
        .unwrap();
    assert_eq!(
        control
            .execute_at(retry(grant(), "first-user", &token), "owner", now + 6)
            .await
            .err(),
        Some("revoked")
    );
    assert_eq!(
        control
            .execute_at(retry(grant(), "race", &race_token), "owner", now + 2 * DAY)
            .await
            .err(),
        Some("expired")
    );
    assert_eq!(count().await.unwrap().get::<_, i64>(0), 2);
    // Runtime cannot inspect the idempotency map. No plaintext is in either row.
    let row = db.query_one(
        "SELECT has_table_privilege('oo_token_runtime','oo_token_lifecycle.issue_retry','SELECT'), \
         (SELECT count(*) FROM oo_token_lifecycle.issue_retry WHERE length(key_sha256)=64 AND length(request_sha256)=64 AND length(token_sha256)=64)", &[]
    ).await.unwrap();
    assert!(!row.get::<_, bool>(0));
    assert_eq!(row.get::<_, i64>(1), 2);
    let audits: i64 = db
        .query_one(
            "SELECT count(*) FROM oo_token_lifecycle.audit WHERE project_id='retry-fixture'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(audits, 10);
}
