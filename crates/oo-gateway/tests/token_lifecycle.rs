//! Real PostgreSQL fixture only; token_owned_pg.py creates and destroys it.
use oo_gateway::http_gateway::{Endpoint, token_lifecycle::*};
use serde_json::Value;
use std::{fs, path::PathBuf, time::Duration};
use tokio_postgres::Client;
use zeroize::Zeroizing;

struct Fixture {
    root: PathBuf,
    endpoint: Endpoint,
    ca: Vec<u8>,
    control: Control,
    db: Client,
}
impl Fixture {
    async fn load() -> Self {
        let root =
            PathBuf::from(std::env::var("OO_TOKEN_FIXTURE").expect("owned fixture required"));
        let config: Value =
            serde_json::from_slice(&fs::read(root.join("endpoint.json")).unwrap()).unwrap();
        let endpoint = Endpoint {
            host: "localhost".into(),
            port: config["port"].as_u64().unwrap() as u16,
            database: "postgres".into(),
        };
        let ca = fs::read(root.join("ca.crt")).unwrap();
        let password = Zeroizing::new(fs::read_to_string(root.join("admin.pass")).unwrap());
        let control = Control::new(endpoint.clone(), password.to_string(), ca.clone());
        // The fixture's separate admin connection observes rows; no product source writes.
        let mut cfg = tokio_postgres::Config::new();
        cfg.host("localhost")
            .port(endpoint.port)
            .dbname("postgres")
            .user("oo_token_admin")
            .password(password.as_str());
        let (db, connection) = cfg.connect(tokio_postgres::NoTls).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Self {
            root,
            endpoint,
            ca,
            control,
            db,
        }
    }
    async fn audited(&self, op: Operation, now: i64) -> Result<Receipt, &'static str> {
        let before: i64 = self
            .db
            .query_one("SELECT count(*) FROM oo_token_lifecycle.audit", &[])
            .await
            .unwrap()
            .get(0);
        let result = self.control.execute_at(op, "test-admin", now).await;
        let after: i64 = self
            .db
            .query_one("SELECT count(*) FROM oo_token_lifecycle.audit", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(
            after,
            before + 1,
            "exactly one audit row per operation, including refusal"
        );
        result
    }
    async fn lookup(&self, cache: &LookupCache, token: &str, now: i64) -> Result<(), String> {
        let password = Zeroizing::new(fs::read_to_string(self.root.join("runtime.pass")).unwrap());
        let tuple = CurrentTuple {
            project_id: "p".into(),
            principal_id: "alice".into(),
            role: "reader".into(),
            claims: vec![],
            policy_version: "v1".into(),
            knowledge_version: "v1".into(),
            audience_url: "https://gateway.example/".into(),
            sandbox_id: None,
            session_id: None,
        };
        lookup_for_test(
            LookupRequest {
                endpoint: &self.endpoint,
                user: "oo_token_runtime",
                password: &password,
                ca_pem: &self.ca,
                cache,
                presented_token: token,
                current: &tuple,
                gateway_audience: "https://gateway.example/",
                transport: &DeclaredTransport::ExternalClient,
            },
            &FixedWall(now),
        )
        .await
        .map(|_| ())
        .map_err(|e| e.reason().into())
    }
}
fn grant(ttl: Option<i64>) -> Grant {
    Grant {
        project_id: "p".into(),
        principal_id: "alice".into(),
        role: "reader".into(),
        claims: Default::default(),
        policy_version: "v1".into(),
        knowledge_version: "v1".into(),
        audience_url: "https://gateway.example/".into(),
        ttl_ms: ttl,
        purpose: "integration-test".into(),
    }
}
fn hash(r: &Receipt) -> String {
    r.metadata["token_sha256"].as_str().unwrap().into()
}
fn refused(r: Result<Receipt, &'static str>, reason: &str) {
    assert_eq!(r.err(), Some(reason));
}

#[tokio::test]
#[ignore = "owned PostgreSQL 17 fixture required; run token_owned_pg.py"]
async fn real_postgres_lifecycle_and_cache() {
    let f = Fixture::load().await;
    f.control.install().await.unwrap();
    f.control.install().await.unwrap();
    let privileges = f.db.query_one(
        "SELECT has_table_privilege('oo_token_runtime','oo_token_lifecycle.enrollment','SELECT'), has_table_privilege('oo_token_runtime','oo_token_lifecycle.policy','UPDATE'), has_table_privilege('oo_token_runtime','oo_token_lifecycle.audit','INSERT')", &[],
    ).await.unwrap();
    for i in 0..3 {
        assert!(!privileges.get::<_, bool>(i));
    }
    let now = now_ms().unwrap();
    refused(
        f.audited(
            Operation::List {
                project: "invalid project".into(),
            },
            now,
        )
        .await,
        "input",
    );
    refused(
        f.audited(Operation::Enroll(grant(Some(0))), now).await,
        "max_lifetime",
    );
    let initial = f.audited(Operation::Issue(grant(None)), now).await.unwrap();
    assert_eq!(initial.metadata["expires_at_ms"], now + 90 * DAY);
    let token = initial.secret.as_ref().unwrap();
    assert!(token.starts_with("ooct_"));
    f.lookup(&LookupCache::new(), token, now).await.unwrap();
    let list = f
        .audited(
            Operation::List {
                project: "p".into(),
            },
            now,
        )
        .await
        .unwrap();
    assert!(list.secret.is_none());
    assert!(!list.metadata.to_string().contains("ooct_"));
    refused(
        f.audited(Operation::Issue(grant(Some(91 * DAY))), now)
            .await,
        "max_lifetime",
    );
    let mut invalid = grant(None);
    invalid.role = "admin".into();
    refused(
        f.audited(Operation::Issue(invalid), now).await,
        "principal_or_role",
    );
    refused(
        f.audited(
            Operation::Policy {
                project: "p".into(),
                policy: Policy {
                    max_lifetime_ms: 366 * DAY,
                    ..Policy::default()
                },
            },
            now,
        )
        .await,
        "policy",
    );
    let short = f
        .audited(Operation::Issue(grant(Some(10 * DAY))), now)
        .await
        .unwrap();
    let h = hash(&short);
    let short_token = short.secret.as_ref().unwrap();
    let selfrenew = |aud: &str| Operation::SelfRenew {
        token: Zeroizing::new(short_token.to_string()),
        audience: aud.into(),
    };
    refused(
        f.audited(selfrenew("https://wrong.example/"), now).await,
        "audience",
    );
    refused(
        f.audited(selfrenew("https://gateway.example/"), now).await,
        "self_renew_disabled",
    );
    f.audited(
        Operation::Policy {
            project: "p".into(),
            policy: Policy {
                default_ttl_ms: 10 * DAY,
                self_renew_enabled: true,
                self_renew_window_ms: 2 * DAY,
                ..Policy::default()
            },
        },
        now,
    )
    .await
    .unwrap();
    refused(
        f.audited(selfrenew("https://gateway.example/"), now).await,
        "self_renew_window",
    );
    refused(
        f.audited(selfrenew("https://gateway.example/"), now + 10 * DAY)
            .await,
        "expired",
    );
    let renewed = f
        .audited(selfrenew("https://gateway.example/"), now + 9 * DAY)
        .await
        .unwrap();
    assert!(renewed.secret.is_none());
    assert_eq!(renewed.metadata["expires_at_ms"], now + 19 * DAY);
    f.lookup(&LookupCache::new(), short_token, now + 11 * DAY)
        .await
        .unwrap();
    refused(
        f.audited(
            Operation::Renew {
                project: "p".into(),
                hash: h.clone(),
                deadline_ms: now + 91 * DAY,
            },
            now + 9 * DAY,
        )
        .await,
        "max_lifetime",
    );
    f.audited(
        Operation::Renew {
            project: "p".into(),
            hash: h.clone(),
            deadline_ms: now + 20 * DAY,
        },
        now + 9 * DAY,
    )
    .await
    .unwrap();
    refused(
        f.audited(
            Operation::Renew {
                project: "p".into(),
                hash: h.clone(),
                deadline_ms: now + 30 * DAY,
            },
            now + 20 * DAY,
        )
        .await,
        "expired",
    );
    refused(
        f.audited(
            Operation::Rotate {
                project: "p".into(),
                hash: h.clone(),
            },
            now + 20 * DAY,
        )
        .await,
        "expired",
    );
    let rotated = f
        .audited(
            Operation::Rotate {
                project: "p".into(),
                hash: h.clone(),
            },
            now + DAY,
        )
        .await
        .unwrap();
    assert!(rotated.secret.as_ref().unwrap().as_str() != short_token.as_str());
    assert_eq!(rotated.metadata["rotated_from"], h);
    assert_eq!(rotated.metadata["first_issued_at_ms"], now);
    assert_eq!(rotated.metadata["overlap_until_ms"], now + 8 * DAY);
    f.lookup(&LookupCache::new(), short_token, now + 8 * DAY - 1)
        .await
        .unwrap();
    assert_eq!(
        f.lookup(&LookupCache::new(), short_token, now + 8 * DAY)
            .await
            .err()
            .as_deref(),
        Some("expired")
    );
    refused(
        f.audited(
            Operation::Renew {
                project: "p".into(),
                hash: h.clone(),
                deadline_ms: now + 20 * DAY,
            },
            now + DAY,
        )
        .await,
        "already_rotated",
    );
    refused(
        f.audited(
            Operation::Rotate {
                project: "p".into(),
                hash: h,
            },
            now + DAY,
        )
        .await,
        "already_rotated",
    );
    let code = f
        .audited(Operation::Enroll(grant(None)), now)
        .await
        .unwrap();
    assert_eq!(code.metadata["expires_at_ms"], now + DAY);
    let secret = code.secret.unwrap();
    assert_eq!(secret.len(), 12);
    assert!(secret.starts_with("OO-"));
    let redeem = |aud: &str| Operation::Redeem {
        code: Zeroizing::new(secret.to_string()),
        audience: aud.into(),
    };
    refused(
        f.audited(redeem("https://wrong.example/"), now).await,
        "audience",
    );
    let redeemed = f
        .audited(redeem("https://gateway.example/"), now)
        .await
        .unwrap();
    f.lookup(&LookupCache::new(), redeemed.secret.as_ref().unwrap(), now)
        .await
        .unwrap();
    refused(
        f.audited(redeem("https://gateway.example/"), now).await,
        "already_redeemed",
    );
    let expired = f
        .audited(Operation::Enroll(grant(None)), now)
        .await
        .unwrap()
        .secret
        .unwrap();
    refused(
        f.audited(
            Operation::Redeem {
                code: expired,
                audience: "https://gateway.example/".into(),
            },
            now + DAY,
        )
        .await,
        "expired",
    );
    refused(
        f.audited(
            Operation::Redeem {
                code: Zeroizing::new("OO-0000-0000".into()),
                audience: "https://gateway.example/".into(),
            },
            now,
        )
        .await,
        "not_found",
    );
    // Both contenders address the same code; the lock and transaction allow only one.
    let race_code = f
        .audited(Operation::Enroll(grant(None)), now)
        .await
        .unwrap()
        .secret
        .unwrap();
    let op = || Operation::Redeem {
        code: Zeroizing::new(race_code.to_string()),
        audience: "https://gateway.example/".into(),
    };
    let (a, b) = tokio::join!(
        f.control.execute_at(op(), "race-a", now),
        f.control.execute_at(op(), "race-b", now)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(a.err().or(b.err()), Some("already_redeemed"));
    let cached = LookupCache::new();
    f.lookup(&cached, token, now).await.unwrap();
    assert!(cached.remaining_ms(&hash(&initial)).unwrap().unwrap() <= 5000);
    f.audited(
        Operation::Revoke {
            project: "p".into(),
            hash: hash(&initial),
        },
        now,
    )
    .await
    .unwrap();
    f.audited(
        Operation::Revoke {
            project: "p".into(),
            hash: hash(&initial),
        },
        now,
    )
    .await
    .unwrap();
    refused(
        f.audited(
            Operation::Renew {
                project: "p".into(),
                hash: hash(&initial),
                deadline_ms: now + 91 * DAY,
            },
            now,
        )
        .await,
        "revoked",
    );
    refused(
        f.audited(
            Operation::Rotate {
                project: "p".into(),
                hash: hash(&initial),
            },
            now,
        )
        .await,
        "revoked",
    );
    refused(
        f.audited(
            Operation::Revoke {
                project: "missing".into(),
                hash: hash(&initial),
            },
            now,
        )
        .await,
        "not_found",
    );
    assert_eq!(
        f.lookup(&LookupCache::new(), token, now)
            .await
            .err()
            .as_deref(),
        Some("revoked")
    );
    refused(
        f.audited(
            Operation::SelfRenew {
                token: Zeroizing::new(token.to_string()),
                audience: "https://gateway.example/".into(),
            },
            now,
        )
        .await,
        "revoked",
    );
    // Existing warm positives may survive only their original five-second TTL.
    tokio::time::sleep(Duration::from_millis(5050)).await;
    assert_eq!(
        f.lookup(&cached, token, now).await.err().as_deref(),
        Some("revoked")
    );
    f.audited(
        Operation::Policy {
            project: "p".into(),
            policy: Policy {
                max_lifetime_ms: 365 * DAY,
                default_ttl_ms: 365 * DAY,
                ..Policy::default()
            },
        },
        now,
    )
    .await
    .unwrap();
    let year = f.audited(Operation::Issue(grant(None)), now).await.unwrap();
    assert_eq!(year.metadata["expires_at_ms"], now + 365 * DAY);
    f.lookup(
        &LookupCache::new(),
        year.secret.as_ref().unwrap(),
        now + 364 * DAY,
    )
    .await
    .unwrap();
    f.audited(
        Operation::Policy {
            project: "p".into(),
            policy: Policy {
                rotate_overlap_ms: 0,
                ..Policy::default()
            },
        },
        now,
    )
    .await
    .unwrap();
    let zero = f.audited(Operation::Issue(grant(None)), now).await.unwrap();
    let replacement = f
        .audited(
            Operation::Rotate {
                project: "p".into(),
                hash: hash(&zero),
            },
            now,
        )
        .await
        .unwrap();
    assert_eq!(replacement.metadata["overlap_until_ms"], now);
    assert_eq!(
        f.lookup(&LookupCache::new(), zero.secret.as_ref().unwrap(), now)
            .await
            .err()
            .as_deref(),
        Some("revoked")
    );
    let raw =
        f.db.query("SELECT code_sha256 FROM oo_token_lifecycle.enrollment", &[])
            .await
            .unwrap();
    assert!(raw.iter().all(|r| r.get::<_, String>(0).len() == 64));
    let count: i64 =
        f.db.query_one("SELECT count(*) FROM oo_token_lifecycle.audit", &[])
            .await
            .unwrap()
            .get(0);
    println!(
        "real PostgreSQL lifecycle operations + refusals + concurrent redemption + cache TTL passed; audit rows={count}"
    );
    let version: String =
        f.db.query_one("SELECT version()", &[])
            .await
            .unwrap()
            .get(0);
    println!("engine={version}");
}

#[tokio::test]
#[ignore = "owned PostgreSQL 17 fixture required; run token_owned_pg.py"]
async fn upgrade_v1_preserves_checksum_and_rows() {
    let f = Fixture::load().await;
    let mut endpoint = f.endpoint.clone();
    endpoint.database = "upgrade".into();
    let password = Zeroizing::new(fs::read_to_string(f.root.join("admin.pass")).unwrap());
    let control = Control::new(endpoint, password.to_string(), f.ca.clone());
    control.install().await.unwrap();
    control.install().await.unwrap();
    let list = control
        .execute(
            Operation::List {
                project: "legacy".into(),
            },
            "upgrade-test",
        )
        .await
        .unwrap();
    assert_eq!(list.metadata["tokens"].as_array().unwrap().len(), 1);
    assert_eq!(list.metadata["tokens"][0]["first_issued_at_ms"], 1000);
    let original =
        include_str!("../../../tools/gateway-token-store/migrations/oo_token_control_v1.sql");
    use sha2::{Digest, Sha256};
    assert_eq!(
        format!("{:x}", Sha256::digest(original.as_bytes())),
        "501d52c0f1f8de46470fc3192fca7de3d59dfd1a3b990dc70daef97341b85597"
    );
    println!("v1 upgrade + replay preserve v1 checksum and existing token first_issued_at");
}
