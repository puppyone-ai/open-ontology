//! Agent tokens (`ooa_`, D83) over real HTTP and a real PostgreSQL: scope enforcement on every route, Drive scope,
//! TTL, revocation (401 at once), the audit rows and the hash chain, and that the token never reaches a log row.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test agent_tokens_live -- --ignored --test-threads=1`

mod common;

use common::engine::{Opts, TestEngine};
use common::fx::{DRIVE, Fx};
use common::http::{Resp, request};
use oo_docs::publish::run::PublishOutcome;
use serde_json::{Value, json};

const DRIVE_B: &str = "00000000-0000-4000-8000-0000000000d2";
const CONTRACT: &str = "---\ntype: 组织\nname: 澄海物流\n---\n# 澄海物流\n\n合同：澄海物流合作合同\n\n合同编号：HT-2026-017\n";

/// The response must be an instance of the `$def` of the API schema (contracts/docs-api).
fn conforms(name: &str, value: &Value) {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/docs-api/docs-api.v1.schema.json"),
    )
    .unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    let wrapper = json!({"$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": doc["$defs"].clone(), "$ref": format!("#/$defs/{name}")});
    let v = oo_core::schema_util::compile_schema(&wrapper).unwrap();
    let errors: Vec<String> = v
        .iter_errors(value)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{name}: {}\n{value}", errors.join("; "));
}

async fn call(e: &TestEngine, token: &str, method: &str, path: &str, body: Option<&Value>) -> Resp {
    let auth = format!("Bearer {token}");
    let text = body.map(Value::to_string);
    request(
        e.addr,
        method,
        path,
        &[("Authorization", &auth)],
        text.as_deref(),
    )
    .await
}

async fn issue(e: &TestEngine, body: Value) -> (String, String) {
    let r = e
        .req("POST", "/v1/docs/agent-tokens", &[], Some(&body))
        .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let j = r.json();
    conforms("AgentToken", &j);
    let t = j["token"].as_str().unwrap().to_owned();
    assert!(t.starts_with("ooa_"));
    (j["id"].as_str().unwrap().to_owned(), t)
}

struct Env {
    e: TestEngine,
    key: String,
}

async fn setup(name: &str) -> Env {
    let e = TestEngine::start(
        name,
        Opts {
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    let mut fx = Fx::attach(name, &e.cas).await;
    fx.admin_sql(&format!(
        "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES ('{DRIVE_B}', 'b', 'fs', '{{}}')"
    ))
    .await;
    fx.index_md("Contracts/澄海物流合作合同.md", CONTRACT).await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("publish")
    };
    let r = e
        .get(&format!(
            "/v1/docs/entities?type=Organization&drive_id={DRIVE}"
        ))
        .await;
    let key = r.json()["items"][0]["key"]
        .as_str()
        .expect("entity")
        .to_owned();
    Env { e, key }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn scopes_drives_and_the_default_deny() {
    let Env { e, key, .. } = setup("agent_scopes").await;

    // Issue: closed fields, scopes and Drive rules.
    for (body, why) in [
        (
            json!({"name": "a", "scopes": ["docs:admin"], "drive_ids": [DRIVE]}),
            "unknown scope",
        ),
        (
            json!({"name": "a", "scopes": ["docs:read"]}),
            "docs scope without a drive",
        ),
        (
            json!({"name": "a", "scopes": ["docs:write"], "drive_ids": [DRIVE]}),
            "write without read",
        ),
        (
            json!({"name": "a", "scopes": ["db:read"]}),
            "db without a profile",
        ),
        (
            json!({"name": "a", "scopes": ["docs:read"], "drive_ids": [DRIVE], "gw_profile": "p"}),
            "profile without db",
        ),
        (
            json!({"name": "a b", "scopes": ["docs:read"], "drive_ids": [DRIVE]}),
            "bad name",
        ),
        (
            json!({"name": "a", "scopes": ["docs:read"], "drive_ids": [DRIVE], "all_drives": true}),
            "both",
        ),
        (
            json!({"name": "a", "scopes": ["docs:read"], "drive_ids": [DRIVE], "expires_in_seconds": 5}),
            "ttl",
        ),
        (
            json!({"name": "a", "scopes": ["docs:read"], "drive_ids": [DRIVE], "extra": 1}),
            "unknown field",
        ),
        (
            json!({"name": "a", "scopes": ["db:read"], "gw_profile": "p", "drive_ids": [DRIVE]}),
            "drives with db only",
        ),
    ] {
        let r = e
            .req("POST", "/v1/docs/agent-tokens", &[], Some(&body))
            .await;
        assert_eq!(r.status, 400, "{why}: {}", String::from_utf8_lossy(&r.body));
    }
    let r = e
        .req(
            "POST",
            "/v1/docs/agent-tokens",
            &[],
            Some(&json!({"name": "ghost", "scopes": ["docs:read"], "drive_ids": ["00000000-0000-4000-8000-0000000000ff"]})),
        )
        .await;
    assert_eq!(r.status, 404, "a Drive that does not exist");

    let (id_a, tok_a) = issue(
        &e,
        json!({"name": "reader-a", "scopes": ["docs:read"], "drive_ids": [DRIVE]}),
    )
    .await;
    let (_, tok_b) = issue(
        &e,
        json!({"name": "reader-b", "scopes": ["docs:read"], "drive_ids": [DRIVE_B]}),
    )
    .await;
    let (_, tok_all) = issue(
        &e,
        json!({"name": "reader-all", "scopes": ["docs:read"], "all_drives": true}),
    )
    .await;
    let (_, tok_db) = issue(
        &e,
        json!({"name": "db-only", "scopes": ["db:read"], "gw_profile": "sales"}),
    )
    .await;
    let (_, tok_sug) = issue(
        &e,
        json!({"name": "suggester", "scopes": ["docs:read", "docs:suggest"], "drive_ids": [DRIVE]}),
    )
    .await;
    // One live token per name.
    let r = e
        .req(
            "POST",
            "/v1/docs/agent-tokens",
            &[],
            Some(&json!({"name": "reader-a", "scopes": ["docs:read"], "drive_ids": [DRIVE]})),
        )
        .await;
    assert_eq!(r.status, 400);

    // docs:read on Drive A: its entity, nothing outside.
    let list = format!("/v1/docs/entities?type=Organization&drive_id={DRIVE}");
    let r = call(&e, &tok_a, "GET", &list, None).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(r.json()["items"][0]["key"], key.as_str());
    let r = call(&e, &tok_a, "GET", &format!("/v1/docs/entities/{key}"), None).await;
    assert_eq!(r.status, 200);
    assert!(r.json()["facts"].is_array());
    let r = call(
        &e,
        &tok_a,
        "GET",
        "/v1/docs/entities?type=Organization",
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (403, Some("drive_id_required"))
    );
    let r = call(
        &e,
        &tok_a,
        "GET",
        &format!("/v1/docs/entities?drive_id={DRIVE_B}"),
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (403, Some("drive"))
    );
    let r = call(
        &e,
        &tok_a,
        "GET",
        &format!("/v1/docs/drives/{DRIVE_B}"),
        None,
    )
    .await;
    assert_eq!(
        r.status, 404,
        "a Drive outside the set is as good as absent"
    );

    // Drive B's token cannot see Drive A's entity: 404, as good as absent.
    let r = call(&e, &tok_b, "GET", &format!("/v1/docs/entities/{key}"), None).await;
    assert_eq!(r.status, 404);
    // Resolve filters its candidates to the token's Drives.
    let body = json!({"text": "澄海物流合作合同"});
    let r = call(&e, &tok_a, "POST", "/v1/docs/resolve", Some(&body)).await;
    assert_eq!(r.status, 200);
    assert!(
        !r.json()["mentions"].as_array().unwrap().is_empty(),
        "A resolves its own entity"
    );
    let r = call(&e, &tok_b, "POST", "/v1/docs/resolve", Some(&body)).await;
    assert_eq!(r.status, 200);
    assert!(
        r.json()["mentions"].as_array().unwrap().is_empty(),
        "B resolves nothing of A"
    );
    let r = call(
        &e,
        &tok_all,
        "GET",
        "/v1/docs/entities?type=Organization",
        None,
    )
    .await;
    assert_eq!(r.status, 200, "all_drives needs no drive_id");

    // The default is deny: everything not listed is 403, for every scope combination.
    for (method, path) in [
        ("GET", "/v1/docs/drives"),
        ("POST", "/v1/docs/drives"),
        ("GET", "/v1/docs/shares"),
        ("POST", "/v1/docs/shares"),
        ("GET", "/v1/docs/review"),
        ("POST", "/v1/docs/entities:merge"),
        ("GET", "/v1/docs/config/model"),
        ("GET", "/v1/docs/agent-tokens"),
        ("POST", "/v1/docs/agent-tokens"),
        ("POST", "/v1/docs/agent-tokens:introspect"),
        (
            "DELETE",
            "/v1/docs/cloud-tokens/00000000-0000-4000-8000-0000000000aa",
        ),
        ("GET", "/v1/docs/audit/agents"),
        ("GET", "/v1/docs/audit/verify"),
        ("PUT", "/v1/docs/link-rules"),
        ("PUT", "/v1/docs/agent-db-binding"),
        ("POST", "/v1/docs/links:project"),
        ("GET", "/v1/docs/suggestions"),
    ] {
        for t in [&tok_a, &tok_all, &tok_sug, &tok_db] {
            let r = call(&e, t, method, path, Some(&json!({}))).await;
            assert_eq!(
                r.status,
                403,
                "{method} {path}: {}",
                String::from_utf8_lossy(&r.body)
            );
            assert_eq!(r.json()["code"], "DOCS_SHARE_REFS_OUT_OF_SCOPE");
        }
    }
    // Scope: db:read alone reads no document; docs:read alone cannot suggest or decide.
    let r = call(&e, &tok_db, "GET", &list, None).await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (403, Some("scope"))
    );
    let r = call(
        &e,
        &tok_a,
        "POST",
        "/v1/docs/suggestions",
        Some(&json!({"share_id": DRIVE})),
    )
    .await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (403, Some("scope"))
    );
    let r = call(
        &e,
        &tok_a,
        "POST",
        "/v1/docs/links:decide",
        Some(&json!({"drive_id": DRIVE, "doc_entity_key": key, "db_table": "public.customers", "db_key": "17", "status": "confirmed"})),
    )
    .await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (403, Some("scope"))
    );
    // docs:suggest names a share of a Drive the token has; a share that is not there is 403, never a write.
    let r = call(
        &e,
        &tok_sug,
        "POST",
        "/v1/docs/suggestions",
        Some(&json!({"share_id": DRIVE})),
    )
    .await;
    assert_eq!(r.status, 403);

    // `self` tells the Agent what it holds and which Drives it reaches; the database token carries a profile NAME.
    let r = call(&e, &tok_db, "GET", "/v1/docs/agent-tokens/self", None).await;
    assert_eq!(r.status, 200);
    let me = r.json();
    conforms("AgentTokenInfo", &me);
    assert_eq!(
        (me["gw_profile"].as_str(), me["scopes"][0].as_str()),
        (Some("sales"), Some("db:read"))
    );
    assert!(me.get("token").is_none() && me["drives"].as_array().unwrap().is_empty());
    let r = call(&e, &tok_a, "GET", "/v1/docs/agent-tokens/self", None).await;
    assert_eq!(r.json()["drives"].as_array().unwrap().len(), 1);
    assert_eq!(r.json()["drives"][0]["id"], DRIVE);
    // The control token has no `self`.
    assert_eq!(e.get("/v1/docs/agent-tokens/self").await.status, 404);

    // Introspection (control only): active, with the principal; not a use of the token.
    let ib = json!({"token": tok_a});
    let r = e
        .req("POST", "/v1/docs/agent-tokens:introspect", &[], Some(&ib))
        .await;
    assert_eq!(r.status, 200);
    conforms("AgentIntrospect", &r.json());
    assert_eq!(
        (r.json()["active"].as_bool(), r.json()["name"].as_str()),
        (Some(true), Some("reader-a"))
    );
    let r = e
        .req(
            "POST",
            "/v1/docs/agent-tokens:introspect",
            &[],
            Some(&json!({"token": "ooa_nothing"})),
        )
        .await;
    assert_eq!(
        (r.json()["active"].as_bool(), r.json()["reason"].as_str()),
        (Some(false), Some("unknown"))
    );

    // The list never carries a token or a hash.
    let r = e.get("/v1/docs/agent-tokens").await;
    conforms("AgentTokenList", &r.json());
    let text = String::from_utf8_lossy(&r.body).into_owned();
    assert!(r.json()["items"].as_array().unwrap().len() >= 5);
    for t in [&tok_a, &tok_b, &tok_all, &tok_db, &tok_sug] {
        assert!(!text.contains(t.as_str()), "the list leaked a token");
    }
    assert!(!text.contains("token_b3"));

    // Revocation: the next call is 401, at once, with the reason; a second revoke finds nothing live.
    let r = e
        .req(
            "DELETE",
            &format!("/v1/docs/agent-tokens/{id_a}"),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    let r = call(&e, &tok_a, "GET", &list, None).await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (401, Some("revoked"))
    );
    let r = e
        .req("POST", "/v1/docs/agent-tokens:introspect", &[], Some(&ib))
        .await;
    assert_eq!(
        (r.json()["active"].as_bool(), r.json()["reason"].as_str()),
        (Some(false), Some("revoked"))
    );
    let r = e
        .req(
            "DELETE",
            &format!("/v1/docs/agent-tokens/{id_a}"),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 404);
    // The name is free again after a revoke.
    issue(
        &e,
        json!({"name": "reader-a", "scopes": ["docs:read"], "drive_ids": [DRIVE]}),
    )
    .await;
    // The cloud-token route does not revoke an Agent token and a cloud token is never an Agent token.
    let r = e
        .req(
            "DELETE",
            &format!("/v1/docs/cloud-tokens/{id_a}"),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 404);

    // TTL: an expired token is 401 `expired`.
    let (id_x, tok_x) = issue(
        &e,
        json!({"name": "short", "scopes": ["docs:read"], "drive_ids": [DRIVE], "expires_in_seconds": 60}),
    )
    .await;
    assert_eq!(call(&e, &tok_x, "GET", &list, None).await.status, 200);
    let mut admin = common::connect("agent_scopes").await;
    sqlx::query("UPDATE _oo_docs.cloud_token SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1::uuid")
        .bind(&id_x)
        .execute(&mut admin)
        .await
        .unwrap();
    let r = call(&e, &tok_x, "GET", &list, None).await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (401, Some("expired"))
    );

    // Unknown and malformed tokens.
    for t in [
        "ooa_",
        "ooa_nonsense",
        "oodc_nonsense",
        "ooa_ooa_ooa_ooa_ooa",
    ] {
        let r = call(&e, t, "GET", &list, None).await;
        assert_eq!(r.status, 401, "{t}");
    }

    // Audit: every call and every refusal is a row, issue and revoke too; never a token, never a path value.
    let r = e.get("/v1/docs/audit/agents?limit=1000").await;
    assert_eq!(r.status, 200);
    let body = String::from_utf8_lossy(&r.body).into_owned();
    for t in [&tok_a, &tok_b, &tok_all, &tok_db, &tok_sug, &tok_x] {
        assert!(!body.contains(t.as_str()), "the audit leaked a token");
    }
    assert!(
        !body.contains(&key),
        "the audit holds route templates, not values"
    );
    conforms("AuditAgents", &r.json());
    let items = r.json()["items"].as_array().unwrap().clone();
    let count = |action: &str, agent: &str| {
        items
            .iter()
            .filter(|i| i["action"] == action && i["agent"] == agent)
            .count()
    };
    assert_eq!(count("agent_token.issued", "reader-a"), 2);
    assert_eq!(count("agent_token.revoked", "reader-a"), 1);
    assert!(count("agent.read", "reader-a") >= 3, "{items:?}");
    assert!(count("agent.denied", "reader-a") >= 3);
    assert!(count("agent.denied", "db-only") >= 1);
    let denied = items
        .iter()
        .find(|i| i["action"] == "agent.denied" && i["agent"] == "reader-a")
        .unwrap();
    assert!(
        denied["detail"]["route"]
            .as_str()
            .unwrap()
            .contains(" /v1/docs/")
    );
    assert_eq!(
        oo_docs::api::agent_tokens::audit_route("/v1/docs/entities/organization:abc"),
        "/v1/docs/entities/{id}"
    );
    assert_eq!(denied["actor"], "agent:reader-a");
    let r = e.get("/v1/docs/audit/agents?agent=suggester").await;
    assert!(
        r.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["agent"] == "suggester")
    );
    assert_eq!(
        e.get("/v1/docs/audit/verify").await.json()["intact"],
        true,
        "the chain holds"
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_control_token_and_the_cloud_entry_are_unchanged() {
    let Env { e, key, .. } = setup("agent_unchanged").await;
    // The control token keeps full access; an Agent token on the Origin / Host guards is refused like any call.
    assert_eq!(e.get("/v1/docs/drives").await.status, 200);
    assert_eq!(e.get(&format!("/v1/docs/entities/{key}")).await.status, 200);
    let (_, tok) = issue(
        &e,
        json!({"name": "o", "scopes": ["docs:read"], "all_drives": true}),
    )
    .await;
    let auth = format!("Bearer {tok}");
    let r = request(
        e.addr,
        "GET",
        "/v1/docs/drives/x",
        &[("Authorization", &auth), ("Origin", "http://evil.example")],
        None,
    )
    .await;
    assert_eq!(r.status, 403);
    let r = request(
        e.addr,
        "GET",
        "/v1/docs/entities",
        &[("Authorization", &auth), ("Host", "evil.example")],
        None,
    )
    .await;
    assert_eq!(
        r.status, 400,
        "DNS rebinding is refused before the token is looked at"
    );
    // A cloud token is not an Agent token: the local entry refuses it, as before.
    let r = e
        .req(
            "POST",
            "/v1/docs/cloud-tokens",
            &[],
            Some(&json!({"tenant": "t", "drive_ids": [DRIVE]})),
        )
        .await;
    assert_eq!(r.status, 201);
    let cloud = r.json()["token"].as_str().unwrap().to_owned();
    assert_eq!(
        call(&e, &cloud, "GET", "/v1/docs/drives", None)
            .await
            .status,
        401
    );
    let mut admin = common::connect("agent_unchanged").await;
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.cloud_token WHERE kind = 'cloud'")
            .fetch_one(&mut admin)
            .await
            .unwrap();
    assert_eq!(n, 1);
    e.finish().await;
}
