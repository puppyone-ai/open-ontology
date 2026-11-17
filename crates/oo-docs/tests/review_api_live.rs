//! Entities, resolve and review over real HTTP (B-S5-06, B-S5-08): responses
//! validate against the contract, batch and by-source decisions, idempotent
//! replay, events, and the time from an accept to the next ontology version.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test review_api_live -- --ignored --test-threads=1`

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::engine::{Opts, TestEngine, key};
use common::fx::Fx;
use serde_json::{Value, json};

fn load(rel: &str) -> Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    serde_json::from_str(&std::fs::read_to_string(root.join(rel)).unwrap()).unwrap()
}

fn def(doc: &Value, name: &str) -> jsonschema::Validator {
    let wrapper = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": doc["$defs"].clone(),
        "$ref": format!("#/$defs/{name}"),
    });
    oo_core::schema_util::compile_schema(&wrapper).unwrap()
}

fn check(v: &jsonschema::Validator, what: &str, value: &Value) {
    let errors: Vec<String> = v
        .iter_errors(value)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{what}: {}\n{value}", errors.join("; "));
}

async fn versions(fx: &mut Fx) -> i64 {
    fx.scalar("SELECT count(*) FROM _oo_docs.ontology_version")
        .await
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn entities_resolve_review_and_the_five_second_accept() {
    let doc = load("contracts/docs-api/docs-api.v1.schema.json");
    let name = "api_review";
    let e = TestEngine::start(
        name,
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    let mut fx = Fx::attach(name, &e.cas).await;
    fx.index_md(
        "People/陈子墨.md",
        "---\ntype: person\naliases: 小陈\n---\n\n# 陈子墨\n\n内容\n",
    )
    .await;
    fx.index_md("People/周若溪.md", "# 周若溪\n\n内容\n").await;
    fx.index_md("Projects/星图.md", "# 星图\n\n负责人：陈子墨\n")
        .await;
    fx.index_md("Notes/big.md", "# 议题\n\n内容\n").await;
    fx.llm_facts("Notes/big.md", 100, 6000).await;
    // The pump publishes the indexed files by itself; a publish after the
    // facts exist queues the 100 that wait.
    oo_docs::publish::debounce::schedule_publish(&mut fx.rt)
        .await
        .unwrap();
    e.wake();
    let started = Instant::now();
    loop {
        let waiting = fx
            .scalar("SELECT count(*) FROM _oo_docs.review_item WHERE status = 'pending'")
            .await;
        let unpublished = fx
            .scalar("SELECT count(*) FROM _oo_docs.file WHERE state <> 'published'")
            .await;
        if waiting == 100 && unpublished == 0 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{waiting} {unpublished}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // ---- entities
    let r = e.get("/v1/docs/entities?type=Person").await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let page = r.json();
    let names: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let page_v = def(&doc, "Page");
    check(&page_v, "entities page", &page);
    let entity_ref = def(&doc, "EntityRef");
    for it in page["items"].as_array().unwrap() {
        check(&entity_ref, "entity ref", it);
        assert!(it["counts"]["facts"].is_number());
    }
    assert_eq!(
        e.get("/v1/docs/entities?q=小陈").await.json()["items"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "by alias"
    );
    assert_eq!(e.get("/v1/docs/entities?type=Nope").await.status, 400);
    let czm = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "陈子墨")
        .unwrap()
        .clone();
    let r = e
        .get(&format!(
            "/v1/docs/entities/{}",
            czm["id"].as_str().unwrap()
        ))
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.header("oo-docs-as-of"), Some("v1"));
    let ent = r.json();
    check(&def(&doc, "Entity"), "entity", &ent);
    assert!(
        ent["facts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["predicate"] == "owns")
    );
    assert!(
        ent["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["alias"] == "小陈")
    );
    for f in ent["facts"].as_array().unwrap() {
        check(&def(&doc, "Fact"), "fact", f);
        assert!(!f["evidence"].as_array().unwrap().is_empty());
    }
    // by key, with history, at a version; unknown is 404; share is refused.
    let by_key = e
        .get(&format!(
            "/v1/docs/entities/{}?include=history&as_of=v1",
            czm["key"].as_str().unwrap()
        ))
        .await;
    assert_eq!(by_key.status, 200);
    assert!(!by_key.json()["history"].as_array().unwrap().is_empty());
    assert_eq!(
        e.get("/v1/docs/entities/person:ffffffffffffffffffffffff")
            .await
            .status,
        404
    );
    assert_eq!(
        e.get("/v1/docs/entities/00000000-0000-4000-8000-000000000000?as_of=v9")
            .await
            .status,
        404
    );
    assert_eq!(
        e.get(&format!(
            "/v1/docs/entities/{}?as_of=share",
            czm["key"].as_str().unwrap()
        ))
        .await
        .status,
        400
    );

    // ---- resolve
    let r = e
        .post(
            "/v1/docs/resolve",
            &json!({"text": "今天小陈和周若溪在讨论星图"}),
        )
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let res = r.json();
    check(&def(&doc, "ResolveResult"), "resolve", &res);
    let surfaces: Vec<&str> = res["mentions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["surface"].as_str().unwrap())
        .collect();
    assert!(
        surfaces.contains(&"小陈") && surfaces.contains(&"周若溪") && surfaces.contains(&"星图"),
        "{surfaces:?}"
    );
    let alias_hit = res["mentions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["surface"] == "小陈")
        .unwrap();
    assert_eq!(alias_hit["candidates"][0]["entity"]["name"], "陈子墨");
    assert_eq!(
        e.post("/v1/docs/resolve", &json!({"text": ""}))
            .await
            .status,
        400
    );
    assert_eq!(
        e.post("/v1/docs/resolve", &json!({"text": "x", "types": ["Nope"]}))
            .await
            .status,
        400
    );

    // ---- review
    let r = e.get("/v1/docs/review").await;
    assert_eq!(r.status, 200);
    let rv = r.json();
    check(&page_v, "review page", &rv);
    let item_v = def(&doc, "ReviewItem");
    let items = rv["items"].as_array().unwrap();
    assert_eq!(items.len(), 100);
    check(&item_v, "review item", &items[0]);
    assert_eq!(items[0]["kind"], "low_confidence_fact");
    assert!(items[0]["subject"]["fact"]["predicate"].is_string());
    assert!(!items[0]["evidence"].as_array().unwrap().is_empty());
    let grouped = e.get("/v1/docs/review?group_by=source").await.json();
    assert_eq!(grouped["items"][0]["count"], 100);
    assert_eq!(grouped["items"][0]["source"]["path"], "Notes/big.md");
    assert_eq!(e.get("/v1/docs/review?kind=nope").await.status, 400);
    // paging
    let p1 = e.get("/v1/docs/review?limit=40").await.json();
    assert_eq!(p1["items"].as_array().unwrap().len(), 40);
    let cur = p1["next_cursor"].as_str().unwrap();
    let p2 = e
        .get(&format!("/v1/docs/review?limit=70&cursor={cur}"))
        .await
        .json();
    assert_eq!(p2["items"].as_array().unwrap().len(), 60);
    assert_eq!(p2["next_cursor"], Value::Null);
    let other = e
        .get(&format!("/v1/docs/review?kind=entity_merge&cursor={cur}"))
        .await;
    assert_eq!(other.status, 400, "a cursor belongs to its own filters");

    // ---- decide: validation
    let k = key();
    let r = e
        .req(
            "POST",
            "/v1/docs/review:decide",
            &[],
            Some(&json!({"items": []})),
        )
        .await;
    assert_eq!(r.status, 428, "idempotency key required");
    let too_many: Vec<Value> = (0..501)
        .map(|_| json!({"id": "00000000-0000-4000-8000-000000000001", "revision": 1, "decision": "accept"}))
        .collect();
    let r = e
        .post("/v1/docs/review:decide", &json!({"items": too_many}))
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (413, Some("DOCS_REVIEW_BATCH_TOO_LARGE"))
    );
    let r = e.post("/v1/docs/review:decide", &json!({"items": [{"id": "00000000-0000-4000-8000-000000000001", "revision": 1, "decision": "maybe"}]})).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (400, Some("DOCS_REVIEW_INVALID_DECISION"))
    );
    // mixed batch, replayed by key
    let id0 = items[0]["id"].as_str().unwrap().to_owned();
    let rev0 = items[0]["revision"].as_i64().unwrap();
    let body = json!({"items": [
        {"id": id0, "revision": rev0, "decision": "reject"},
        {"id": items[1]["id"], "revision": rev0 + 7, "decision": "accept"},
    ]});
    let r1 = e
        .req(
            "POST",
            "/v1/docs/review:decide",
            &[("Idempotency-Key", &k)],
            Some(&body),
        )
        .await;
    assert_eq!(r1.status, 200, "{}", String::from_utf8_lossy(&r1.body));
    let out = r1.json();
    check(&def(&doc, "ReviewDecideResult"), "decide", &out);
    assert_eq!(
        (out["rejected"].as_i64(), out["accepted"].as_i64()),
        (Some(1), Some(0))
    );
    assert_eq!(out["results"][1]["status"], "conflict");
    assert_eq!(out["results"][1]["code"], "DOCS_REVIEW_STALE_REVISION");
    let r2 = e
        .req(
            "POST",
            "/v1/docs/review:decide",
            &[("Idempotency-Key", &k)],
            Some(&body),
        )
        .await;
    assert_eq!(r2.json(), out, "replay is the stored response");

    // ---- by_source: accept the other 99 in one call, then wait for the version
    let file_id = grouped["items"][0]["source"]["file_id"].as_str().unwrap();
    let before = versions(&mut fx).await;
    let t0 = Instant::now();
    let r = e
        .post(
            "/v1/docs/review:decide",
            &json!({"by_source": {"file_id": file_id, "decision": "accept"}}),
        )
        .await;
    assert_eq!(r.status, 200);
    let out = r.json();
    assert_eq!(out["accepted"], 99);
    assert_eq!(out["publish_pending"], true);
    while versions(&mut fx).await <= before {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "accept to new version took {:?}",
            t0.elapsed()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    eprintln!("accept to new version: {:?}", t0.elapsed());
    assert!(
        e.get("/v1/docs/review").await.json()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // events
    let ev = fx
        .text("SELECT type FROM _oo_docs.event WHERE type LIKE 'review.%' ORDER BY seq")
        .await;
    assert!(
        ev.contains(&"review.decided".to_owned()) && ev.contains(&"review.changed".to_owned()),
        "{ev:?}"
    );
    let sample: Vec<(String, String)> = sqlx::query_as("SELECT type, payload::text FROM _oo_docs.event WHERE type = 'ontology.version_published' ORDER BY seq DESC LIMIT 1")
        .fetch_all(&mut fx.admin).await.unwrap();
    let p: Value = serde_json::from_str(&sample[0].1).unwrap();
    assert!(
        p["as_of"].is_string() && p["gates"]["DOC-G3"] == "pass",
        "{p}"
    );
    drop(fx);
    e.finish().await;
}
