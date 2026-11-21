//! Who caused an event (docs-api 1.13): `Event.actor` from the `OO-Docs-Client` and `OO-Docs-Agent` headers, and
//! events that a worker appends later inheriting the actor of the request that caused them.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test actor_live -- --ignored --test-threads=1`

mod common;

use std::time::Duration;

use common::engine::{Opts, TOKEN, TestEngine};
use common::http::open_sse;
use serde_json::{Value, json};

async fn create(e: &TestEngine, name: &str, headers: &[(&str, &str)]) -> String {
    let k = common::engine::key();
    let mut h = vec![("Idempotency-Key", k.as_str())];
    h.extend_from_slice(headers);
    let body = json!({"name": name, "backend": "fs", "config": {"root": e.root.to_str().unwrap()}});
    let r = e.req("POST", "/v1/docs/drives", &h, Some(&body)).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    r.json()["id"].as_str().unwrap().to_owned()
}

async fn events(e: &TestEngine) -> Vec<Value> {
    let auth = format!("Bearer {TOKEN}");
    let mut s = open_sse(
        e.addr,
        "/v1/docs/events?since=0",
        &[("Authorization", &auth)],
    )
    .await;
    let mut out = Vec::new();
    while let Some(ev) = s.event(Duration::from_millis(800)).await {
        out.push(ev);
    }
    out
}

fn actor_of(events: &[Value], kind: &str, drive: &str) -> Option<Value> {
    events
        .iter()
        .find(|v| v["type"] == kind && v["drive_id"] == drive)
        .map(|v| v["actor"].clone())
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_actor_of_a_request_is_on_its_events() {
    let e = TestEngine::start(
        "oo_docs_actor_live",
        Opts {
            workers: true,
            ..Opts::default()
        },
    )
    .await;
    let app = create(&e, "from-app", &[("OO-Docs-Client", "app")]).await;
    let cli = create(&e, "from-cli", &[("OO-Docs-Client", "cli")]).await;
    let agent = create(
        &e,
        "from-agent",
        &[("OO-Docs-Client", "cli"), ("OO-Docs-Agent", "claude")],
    )
    .await;
    let plain = create(&e, "from-nobody", &[]).await;
    let evs = events(&e).await;
    assert_eq!(
        actor_of(&evs, "drive.created", &app),
        Some(json!({"kind": "app"}))
    );
    assert_eq!(
        actor_of(&evs, "drive.created", &cli),
        Some(json!({"kind": "cli"}))
    );
    assert_eq!(
        actor_of(&evs, "drive.created", &agent),
        Some(json!({"kind": "agent", "name": "claude"}))
    );
    assert_eq!(
        actor_of(&evs, "drive.created", &plain),
        Some(json!({"kind": "local"}))
    );
    // The actor is a top level field, not part of `data`.
    assert!(evs.iter().all(|v| v["data"].get("actor").is_none()));

    // A sync the app asked for: the events the worker appends for the Drive carry the app actor.
    e.write("a.md", "# a\n");
    let k = common::engine::key();
    let r = e
        .req(
            "POST",
            &format!("/v1/docs/drives/{app}:sync"),
            &[("Idempotency-Key", &k), ("OO-Docs-Client", "app")],
            Some(&json!({})),
        )
        .await;
    assert!(r.status < 300, "{}", String::from_utf8_lossy(&r.body));
    e.until("a worker event with the app actor", || async {
        events(&e).await.iter().any(|v| {
            v["drive_id"] == app.as_str()
                && v["type"] != "drive.created"
                && v["actor"] == json!({"kind": "app"})
        })
    })
    .await;
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn events_publish_is_for_the_control_token_and_carries_the_actor() {
    let e = TestEngine::start("oo_docs_publish_live", Opts::default()).await;
    let ok = json!({"type": "db.source_changed", "data": {"name": "pg", "action": "added", "stack": "/tmp/s.yaml"}});
    let post = |h: &'static [(&'static str, &'static str)], b: Value| {
        let e = &e;
        async move { e.req("POST", "/v1/docs/events:publish", h, Some(&b)).await }
    };
    let r = post(&[("OO-Docs-Client", "cli")], ok.clone()).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let seq = r.json()["seq"].as_i64().expect("seq");
    let evs = events(&e).await;
    let got = evs.iter().find(|v| v["seq"] == seq).expect("the event");
    assert_eq!(got["type"], "db.source_changed");
    assert_eq!(got["drive_id"], Value::Null);
    assert_eq!(got["actor"], json!({"kind": "cli"}));
    assert_eq!(got["data"], ok["data"]);

    // Closed request: type, extra fields, data shape and size, credential keys, a forged actor.
    for (why, body) in [
        ("other type", json!({"type": "drive.deleted", "data": {}})),
        ("no data", json!({"type": "db.pipeline_changed"})),
        (
            "array data",
            json!({"type": "db.pipeline_changed", "data": []}),
        ),
        (
            "extra field",
            json!({"type": "db.pipeline_changed", "data": {}, "drive_id": "x"}),
        ),
        (
            "too large",
            json!({"type": "db.pipeline_changed", "data": {"x": "a".repeat(4100)}}),
        ),
        (
            "credential key",
            json!({"type": "db.source_changed", "data": {"password": "hunter2"}}),
        ),
        (
            "forged actor",
            json!({"type": "db.source_changed", "data": {"actor": {"kind": "app"}}}),
        ),
    ] {
        let r = post(&[], body).await;
        assert_eq!(r.status, 400, "{why}: {}", String::from_utf8_lossy(&r.body));
    }
    let r = e.req("GET", "/v1/docs/events:publish", &[], None).await;
    assert_eq!(r.status, 405);

    // An Agent token and a missing token are refused.
    let t = e
        .req(
            "POST",
            "/v1/docs/agent-tokens",
            &[],
            Some(&json!({"name": "reader", "scopes": ["docs:read"], "all_drives": true})),
        )
        .await;
    assert_eq!(t.status, 201, "{}", String::from_utf8_lossy(&t.body));
    let token = t.json()["token"].as_str().unwrap().to_owned();
    let auth = format!("Bearer {token}");
    let r = common::http::request(
        e.addr,
        "POST",
        "/v1/docs/events:publish",
        &[("Authorization", &auth)],
        Some(&ok.to_string()),
    )
    .await;
    assert_eq!(r.status, 403, "{}", String::from_utf8_lossy(&r.body));
    let r = common::http::request(
        e.addr,
        "POST",
        "/v1/docs/events:publish",
        &[],
        Some(&ok.to_string()),
    )
    .await;
    assert_eq!(r.status, 401);
    // Nothing of the refused calls was appended.
    let evs = events(&e).await;
    assert_eq!(
        evs.iter()
            .filter(|v| v["type"] == "db.source_changed")
            .count(),
        1
    );
    e.finish().await;
}
