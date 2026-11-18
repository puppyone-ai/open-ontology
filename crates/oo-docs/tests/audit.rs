//! Cloud proxy mode, engine side, and the sharing audit against a real PostgreSQL over real HTTP (10 §11.3, B-S12-01):
//!
//! * an `oodc_` token works on the cloud entry only, is bound to its Drives, and the control token does not work there;
//! * the local entry never accepts an `oodc_` token (the loopback API is not relaxed);
//! * `share.created`, `share.revoked`, `refs.accessed` and the token events land in an append-only, hash chained
//!   table that answers "who shared what to which agent, and when", and that cannot be edited.
//!
//! Needs migration 0148 (the coordinator's patch `B-S12-01-migration-docs-cloud-audit.patch`).
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test audit -- --ignored --test-threads=1`

mod common;

use common::engine::{Opts, TestEngine, key};
use common::fx::{DRIVE, Fx, golden};
use common::http;
use oo_docs::audit::{self, Chain};
use oo_docs::cas::CasKind;
use serde_json::{Value, json};
use sqlx::Executor;

const FILES: [&str; 2] = [
    "Projects/星图看板.md",
    "Decisions/D-2026-02-星图采用自研引擎.md",
];

fn gold(rel: &str) -> String {
    std::fs::read_to_string(golden().join("docs").join(rel)).unwrap()
}

struct World {
    e: TestEngine,
    fx: Fx,
    cloud: std::net::SocketAddr,
}

async fn world(name: &str) -> World {
    let cl = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cloud = cl.local_addr().unwrap();
    let e = TestEngine::start(
        name,
        Opts {
            workers: true,
            stages: true,
            tweak: Some(Box::new(move |app| {
                app.cloud_hosts = vec![format!("127.0.0.1:{}", cloud.port())];
            })),
            ..Opts::default()
        },
    )
    .await;
    let app = e.app.clone();
    tokio::spawn(async move {
        let _ = oo_docs::api::serve_cloud(cl, app, std::future::pending::<()>()).await;
    });
    let mut fx = Fx::attach(name, &e.cas).await;
    for f in FILES {
        let src = gold(f);
        fx.index_md(f, &src).await;
        fx.cas.put(CasKind::Orig, src.as_bytes()).unwrap();
    }
    for _ in 0..100 {
        match fx.publish().await {
            oo_docs::publish::run::PublishOutcome::Busy => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            _ => break,
        }
    }
    World { e, fx, cloud }
}

async fn via(
    addr: std::net::SocketAddr,
    token: &str,
    method: &str,
    path: &str,
    agent: Option<&str>,
    body: Option<&Value>,
) -> http::Resp {
    let auth = format!("Bearer {token}");
    let k = key();
    let mut h: Vec<(&str, &str)> = vec![("Authorization", &auth), ("Idempotency-Key", &k)];
    if let Some(a) = agent {
        h.push(("OO-Docs-Agent", a));
    }
    let text = body.map(Value::to_string);
    http::request(addr, method, path, &h, text.as_deref()).await
}

fn share_body(drive: &str) -> Value {
    json!({"compile": {"drive_id": drive, "scope": ["Projects"], "budget": 1200, "task": "星图看板", "wait_ms": 0},
           "mount": {"target_dir": "/tmp/oo-audit-test"}, "grantor": "owner"})
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_cloud_token_is_bound_to_the_cloud_entry_and_its_drives() {
    let mut w = world("oo_docs_audit_cloud").await;
    let r =
        w.e.post(
            "/v1/docs/cloud-tokens",
            &json!({"tenant": "acme", "drive_ids": [DRIVE], "label": "proxy"}),
        )
        .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let t = r.json();
    let (token, tid) = (
        t["token"].as_str().unwrap().to_owned(),
        t["id"].as_str().unwrap().to_owned(),
    );
    assert!(token.starts_with("oodc_"));
    let hashed: i64 =
        w.fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.cloud_token WHERE token_b3 = '{}'",
            blake3::hash(token.as_bytes()).to_hex()
        ))
        .await;
    assert_eq!(hashed, 1, "only the hash is stored");
    let plain: i64 =
        w.fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.cloud_token WHERE token_b3 = '{token}'"
        ))
        .await;
    assert_eq!(plain, 0);

    // The local entry never takes it; the cloud entry never takes the control token.
    let r = via(w.e.addr, &token, "GET", "/v1/docs/health", None, None).await;
    assert_eq!(
        r.status, 401,
        "the loopback API does not accept a cloud token"
    );
    let r = via(
        w.cloud,
        common::engine::TOKEN,
        "GET",
        "/v1/docs/health",
        None,
        None,
    )
    .await;
    assert_eq!(
        r.status, 401,
        "the cloud entry does not accept the control token"
    );
    // A wrong Host is refused before anything (DNS rebinding guard of the cloud entry too).
    let r = http::request(
        w.cloud,
        "GET",
        "/v1/docs/health",
        &[
            ("Authorization", &format!("Bearer {token}")),
            ("Host", "evil.example"),
        ],
        None,
    )
    .await;
    assert_eq!(r.status, 400);

    // Routes: shares and packs of its Drives only.
    for (m, p) in [
        ("GET", "/v1/docs/drives"),
        ("GET", "/v1/docs/health"),
        ("POST", "/v1/docs/cloud-tokens"),
        ("GET", "/v1/docs/audit/shares"),
    ] {
        let r = via(w.cloud, &token, m, p, None, Some(&json!({}))).await;
        assert_eq!(
            r.status,
            403,
            "{m} {p}: {}",
            String::from_utf8_lossy(&r.body)
        );
    }
    let other = "00000000-0000-4000-8000-0000000000ff";
    let r = via(
        w.cloud,
        &token,
        "POST",
        "/v1/docs/shares",
        None,
        Some(&share_body(other)),
    )
    .await;
    assert_eq!(r.status, 403, "another drive");
    let r = via(
        w.cloud,
        &token,
        "POST",
        "/v1/docs/shares",
        Some("agent-7"),
        Some(&share_body(DRIVE)),
    )
    .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let share = r.json();
    let sid = share["id"].as_str().unwrap().to_owned();
    // A share of this drive can be read and revoked through the cloud entry.
    let r = via(
        w.cloud,
        &token,
        "GET",
        &format!("/v1/docs/shares/{sid}"),
        None,
        None,
    )
    .await;
    assert_eq!(r.status, 200);

    // Revoke the token: the next call fails.
    let r =
        w.e.req("DELETE", &format!("/v1/docs/cloud-tokens/{tid}"), &[], None)
            .await;
    assert_eq!(r.status, 200);
    let r = via(
        w.cloud,
        &token,
        "GET",
        &format!("/v1/docs/shares/{sid}"),
        None,
        None,
    )
    .await;
    assert_eq!(r.status, 401);
    assert_eq!(
        r.json()["details"]["reason"],
        "revoked",
        "{}",
        String::from_utf8_lossy(&r.body)
    );
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_audit_says_who_shared_what_to_which_agent_when_and_cannot_be_edited() {
    let mut w = world("oo_docs_audit_log").await;
    let t =
        w.e.post(
            "/v1/docs/cloud-tokens",
            &json!({"tenant": "acme", "drive_ids": [DRIVE]}),
        )
        .await
        .json();
    let token = t["token"].as_str().unwrap().to_owned();

    // Share 1 through the cloud entry for agent-7, share 2 locally for agent-9, one of them revoked.
    let s1 = via(
        w.cloud,
        &token,
        "POST",
        "/v1/docs/shares",
        Some("agent-7"),
        Some(&share_body(DRIVE)),
    )
    .await
    .json();
    let s2 = via(
        w.e.addr,
        common::engine::TOKEN,
        "POST",
        "/v1/docs/shares",
        Some("agent-9"),
        Some(&share_body(DRIVE)),
    )
    .await
    .json();
    let (sid1, _sid2) = (
        s1["id"].as_str().unwrap().to_owned(),
        s2["id"].as_str().unwrap().to_owned(),
    );
    let rv = via(
        w.cloud,
        &token,
        "POST",
        &format!("/v1/docs/shares/{sid1}:revoke"),
        Some("agent-7"),
        Some(&json!({})),
    )
    .await;
    assert_eq!(rv.status, 200, "{}", String::from_utf8_lossy(&rv.body));
    // A ref read with the credential of share 2.
    let sj: Value = serde_json::from_str(s2["share_json"].as_str().unwrap()).unwrap();
    let cred = sj["refs"]["credential"].as_str().unwrap().to_owned();
    let b3: String =
        w.fx.text(
            "SELECT fv.b3 FROM _oo_docs.file_version fv WHERE fv.path_at_version LIKE 'Projects/%'",
        )
        .await
        .remove(0);
    let r = via(
        w.e.addr,
        &cred,
        "GET",
        &format!("/v1/docs/refs/{b3}"),
        None,
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));

    // The question.
    let all = w.e.get("/v1/docs/audit/shares").await.json();
    let items = all["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{all}");
    let by = |agent: &str| {
        items
            .iter()
            .find(|i| i["agent"] == agent)
            .unwrap_or_else(|| panic!("{agent}: {all}"))
            .clone()
    };
    let (a7, a9) = (by("agent-7"), by("agent-9"));
    assert_eq!(a7["share_id"], sid1);
    assert!(a7["actor"].as_str().unwrap().starts_with("cloud:"), "{a7}");
    assert_eq!(a7["tenant"], "acme");
    assert_eq!(
        (a7["role"].as_str(), a7["grantor"].as_str()),
        (Some("suggester"), Some("owner"))
    );
    assert_eq!(a7["scope"], json!(["Projects"]), "what was shared");
    assert!(
        a7["revoked_at"].is_string(),
        "the revocation is part of the answer"
    );
    assert!(a7["at"].is_string());
    assert_eq!(a9["actor"], "local");
    assert!(a9["revoked_at"].is_null());
    assert_eq!(
        a9["refs_reads"], 1,
        "the read of a ref by the credential is counted"
    );
    // Filters.
    let q = w.e.get("/v1/docs/audit/shares?agent=agent-9").await.json();
    assert_eq!(q["items"].as_array().unwrap().len(), 1);
    let q = w.e.get("/v1/docs/audit/shares?tenant=acme").await.json();
    assert_eq!(q["items"].as_array().unwrap().len(), 1);
    let q =
        w.e.get("/v1/docs/audit/shares?since=2999-01-01T00:00:00Z")
            .await
            .json();
    assert!(q["items"].as_array().unwrap().is_empty());
    // The audit holds no secret and no content.
    let blob: String = w
        .fx
        .text("SELECT string_agg(detail::text || coalesce(agent, ''), ' ') FROM _oo_docs.audit_log")
        .await
        .remove(0);
    assert!(
        !blob.contains("oodr_") && !blob.contains("oodc_") && !blob.contains(&cred),
        "no credential in the audit"
    );

    // Append only and chained.
    let v = w.e.get("/v1/docs/audit/verify").await.json();
    assert_eq!(v["intact"], true, "{v}");
    let mut admin = w.fx.admin;
    for sql in [
        "UPDATE _oo_docs.audit_log SET actor = 'x'",
        "DELETE FROM _oo_docs.audit_log",
        "TRUNCATE _oo_docs.audit_log",
    ] {
        let err = admin.execute(sql).await.unwrap_err();
        assert_eq!(
            err.as_database_error()
                .and_then(|d| d.code().map(|c| c.into_owned()))
                .as_deref(),
            Some("OO030"),
            "{sql}"
        );
    }
    // A forged row (the owner inserts one with a bad chain) is found by the verifier.
    admin
        .execute(
            "INSERT INTO _oo_docs.audit_log (action, actor, prev_hash, row_hash) \
             VALUES ('share.revoked', 'forger', 'x', 'y')",
        )
        .await
        .unwrap();
    let mut rt = common::connect_runtime("oo_docs_audit_log").await;
    match audit::verify_chain(&mut rt).await.unwrap() {
        Chain::Broken { seq } => assert!(seq > 0),
        other => panic!("{other:?}"),
    }
    // The runtime role cannot insert directly into the table's neighbours it has no grant on, and cannot update this.
    let err = rt
        .execute("UPDATE _oo_docs.audit_log SET actor = 'x'")
        .await
        .unwrap_err();
    assert!(err.as_database_error().is_some());
}
