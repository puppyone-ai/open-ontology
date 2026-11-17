//! Person corrections over real HTTP (B-S5-12, D59): `If-Match` on every call,
//! idempotent replay, `drive_rev` and the overlay, the same publish pipeline,
//! merge and split, and an extraction that disagrees with a person.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test correct_api_live -- --ignored --test-threads=1`

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::engine::{Opts, TestEngine, key};
use common::fx::{DRIVE, Fx};
use serde_json::{Value, json};

fn def(name: &str) -> jsonschema::Validator {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("contracts/docs-api/docs-api.v1.schema.json")).unwrap(),
    )
    .unwrap();
    let wrapper = json!({"$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": doc["$defs"].clone(), "$ref": format!("#/$defs/{name}")});
    oo_core::schema_util::compile_schema(&wrapper).unwrap()
}

fn check(v: &jsonschema::Validator, what: &str, value: &Value) {
    let errors: Vec<String> = v
        .iter_errors(value)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{what}: {}\n{value}", errors.join("; "));
}

fn r(rev: i64) -> String {
    format!("\"r{rev}\"")
}

async fn wait_version(fx: &mut Fx, n: i64) {
    let t = Instant::now();
    while fx
        .scalar("SELECT coalesce(max(n), 0) FROM _oo_docs.ontology_version")
        .await
        < n
    {
        assert!(
            t.elapsed() < Duration::from_secs(8),
            "version {n} did not come"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn corrections_take_the_publish_pipeline() {
    let name = "api_correct";
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
        "Projects/星图/一.md",
        "---\ndate: 2026-01-01\nstatus: 进行中\nowner: 陈子墨\n---\n\n# 星图\n\n内容\n",
    )
    .await;
    fx.index_md("People/陈子墨.md", "# 陈子墨\n\n内容\n").await;
    fx.index_md("People/小陈.md", "# 小陈\n\n内容\n").await;
    oo_docs::publish::debounce::schedule_publish(&mut fx.rt)
        .await
        .unwrap();
    e.wake();
    wait_version(&mut fx, 1).await;
    while fx
        .scalar("SELECT count(*) FROM _oo_docs.file WHERE state <> 'published'")
        .await
        > 0
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let proj = e.get("/v1/docs/entities?q=星图&type=Project").await.json();
    let pid = proj["items"][0]["key"].as_str().unwrap().to_owned();
    let ent = e.get(&format!("/v1/docs/entities/{pid}")).await.json();
    let status = ent["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["predicate"] == "attr.status")
        .unwrap()
        .clone();
    let fid = status["id"].as_str().unwrap().to_owned();
    assert_eq!(status["revision"], 1);
    let url = format!("/v1/docs/facts/{fid}:correct");

    // Preconditions.
    let k = key();
    let body = json!({"action": "set_value", "value": {"text": "已完成"}});
    let no_match = e
        .req("POST", &url, &[("Idempotency-Key", &k)], Some(&body))
        .await;
    assert_eq!(no_match.status, 428);
    let stale = e
        .req(
            "POST",
            &url,
            &[("Idempotency-Key", &k), ("If-Match", &r(9))],
            Some(&body),
        )
        .await;
    assert_eq!(
        (stale.status, stale.json()["code"].as_str()),
        (412, Some("DOCS_ENGINE_PRECONDITION_FAILED"))
    );
    let bad = e
        .req(
            "POST",
            &url,
            &[("Idempotency-Key", &key()), ("If-Match", &r(1))],
            Some(&json!({"action": "set_value", "value": "x"})),
        )
        .await;
    assert_eq!(bad.status, 400);
    let bad = e
        .req(
            "POST",
            &url,
            &[("Idempotency-Key", &key()), ("If-Match", &r(1))],
            Some(&json!({"action": "explode"})),
        )
        .await;
    assert_eq!(bad.status, 400);

    // set_value: a person's value, drive_rev advances, a new version carries it.
    let rev0 = fx
        .scalar(&format!(
            "SELECT rev FROM _oo_docs.drive WHERE id = '{DRIVE}'"
        ))
        .await;
    let ok = e
        .req(
            "POST",
            &url,
            &[("Idempotency-Key", &k), ("If-Match", &r(1))],
            Some(&body),
        )
        .await;
    assert_eq!(ok.status, 200, "{}", String::from_utf8_lossy(&ok.body));
    let f = ok.json();
    check(&def("Fact"), "fact", &f);
    assert_eq!(
        (f["origin"].as_str(), f["value"]["text"].as_str()),
        (Some("human"), Some("已完成"))
    );
    assert_eq!(f["confidence_bp"], 10000);
    assert!(
        !f["evidence"].as_array().unwrap().is_empty(),
        "the evidence is copied"
    );
    assert!(ok.header("etag").is_some());
    let replay = e
        .req(
            "POST",
            &url,
            &[("Idempotency-Key", &k), ("If-Match", &r(1))],
            Some(&body),
        )
        .await;
    assert_eq!(replay.json(), f, "replay by key");
    assert_eq!(
        fx.scalar(&format!(
            "SELECT rev FROM _oo_docs.drive WHERE id = '{DRIVE}'"
        ))
        .await,
        rev0 + 1
    );
    assert_eq!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'fact_edit' AND drive_rev = {}",
            rev0 + 1
        ))
        .await,
        1,
        "the overlay carries the rev"
    );
    let before = fx
        .scalar("SELECT max(n) FROM _oo_docs.ontology_version")
        .await;
    wait_version(&mut fx, before + 1).await;
    let now = e.get(&format!("/v1/docs/entities/{pid}")).await.json();
    let vals: Vec<&str> = now["facts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["predicate"] == "attr.status")
        .map(|f| f["value"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(vals, ["已完成"]);
    // The old fact is gone from the visible facts but stays in the history.
    let hist = e
        .get(&format!("/v1/docs/entities/{pid}?include=history"))
        .await
        .json();
    assert!(
        hist["history"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["value"]["text"] == "进行中" && f["status"] == "retracted")
    );

    // A later extraction that disagrees with a person is held, not applied.
    fx.index_md(
        "Projects/星图/二.md",
        "---\ndate: 2026-09-01\nstatus: 暂停\n---\n\n# 星图\n\n内容\n",
    )
    .await;
    oo_docs::publish::debounce::schedule_publish(&mut fx.rt)
        .await
        .unwrap();
    e.wake();
    let t = Instant::now();
    while fx.scalar("SELECT count(*) FROM _oo_docs.review_item WHERE reason_code = 'conflicts_with_human_overlay'").await == 0 {
        assert!(t.elapsed() < Duration::from_secs(8), "no held item");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let item = e.get("/v1/docs/review").await.json();
    let held = item["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["reason_code"] == "conflicts_with_human_overlay")
        .unwrap();
    assert_eq!(held["kind"], "low_confidence_fact");

    // mark_wrong on another fact.
    let owner = now["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["predicate"] == "owns")
        .unwrap()
        .clone();
    let oid = owner["id"].as_str().unwrap();
    let w = e
        .req(
            "POST",
            &format!("/v1/docs/facts/{oid}:correct"),
            &[("Idempotency-Key", &key()), ("If-Match", &r(1))],
            Some(&json!({"action": "mark_wrong", "reason": "wrong owner"})),
        )
        .await;
    assert_eq!(w.status, 200, "{}", String::from_utf8_lossy(&w.body));
    assert_eq!(w.json()["status"], "retracted");
    assert_eq!(fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'fact_reject' AND payload->>'human' = 'true'").await, 1);

    // merge 小陈 into 陈子墨, then split it back.
    let people = e.get("/v1/docs/entities?type=Person").await.json();
    let find = |n: &str| {
        people["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == n)
            .unwrap()
            .clone()
    };
    let (czm, xc) = (find("陈子墨"), find("小陈"));
    let czm_detail = e
        .get(&format!(
            "/v1/docs/entities/{}",
            czm["key"].as_str().unwrap()
        ))
        .await
        .json();
    let rev = czm_detail["revision"].as_i64().unwrap();
    let m_body = json!({"into": czm["key"], "from": [xc["key"]]});
    let m_url = "/v1/docs/entities:merge";
    let stale = e
        .req(
            "POST",
            m_url,
            &[("Idempotency-Key", &key()), ("If-Match", &r(rev + 5))],
            Some(&m_body),
        )
        .await;
    assert_eq!(stale.status, 412);
    let m = e
        .req(
            "POST",
            m_url,
            &[("Idempotency-Key", &key()), ("If-Match", &r(rev))],
            Some(&m_body),
        )
        .await;
    assert_eq!(m.status, 200, "{}", String::from_utf8_lossy(&m.body));
    check(&def("Entity"), "merged entity", &m.json());
    assert_eq!(m.json()["redirect_from"], json!([xc["key"]]));
    let via_old = e
        .get(&format!(
            "/v1/docs/entities/{}",
            xc["key"].as_str().unwrap()
        ))
        .await
        .json();
    assert_eq!(
        via_old["key"], czm["key"],
        "the old key answers as the survivor"
    );
    let proj_key = &pid;
    let bad = e
        .req(
            "POST",
            m_url,
            &[
                ("Idempotency-Key", &key()),
                ("If-Match", &r(m.json()["revision"].as_i64().unwrap())),
            ],
            Some(&json!({"into": czm["key"], "from": [proj_key]})),
        )
        .await;
    assert_eq!(
        (bad.status, bad.json()["code"].as_str()),
        (409, Some("DOCS_REVIEW_MERGE_CONFLICT"))
    );

    // split: the alias goes to a new entity.
    let cur = e
        .get(&format!(
            "/v1/docs/entities/{}",
            czm["key"].as_str().unwrap()
        ))
        .await
        .json();
    let s_rev = cur["revision"].as_i64().unwrap();
    let alias = cur["aliases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["alias"] == "小陈")
        .map(|a| a["alias"].clone());
    let groups = json!({"groups": [{}, {"aliases": [alias.unwrap_or(json!("小陈"))]}]});
    let s_url = format!("/v1/docs/entities/{}:split", czm["key"].as_str().unwrap());
    let one = e
        .req(
            "POST",
            &s_url,
            &[("Idempotency-Key", &key()), ("If-Match", &r(s_rev))],
            Some(&json!({"groups": [{}]})),
        )
        .await;
    assert_eq!(one.status, 400, "a split needs two groups");
    let s = e
        .req(
            "POST",
            &s_url,
            &[("Idempotency-Key", &key()), ("If-Match", &r(s_rev))],
            Some(&groups),
        )
        .await;
    assert_eq!(s.status, 200, "{}", String::from_utf8_lossy(&s.body));
    let out = s.json();
    check(&def("BatchResult"), "split result", &out);
    let new_key = out["results"][0]["id"].as_str().unwrap();
    assert!(new_key.starts_with("person:"));
    let created = e.get(&format!("/v1/docs/entities/{new_key}")).await;
    assert_eq!(created.status, 200);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'entity_split'")
            .await,
        1
    );
    let rev_after = fx
        .scalar(&format!(
            "SELECT rev FROM _oo_docs.drive WHERE id = '{DRIVE}'"
        ))
        .await;
    assert!(rev_after >= rev0 + 4, "{rev_after}");
    drop(fx);
    e.finish().await;
}
