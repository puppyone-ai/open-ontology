//! The control API over real HTTP and a real PostgreSQL (B-S2-09).
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test api_live -- --ignored`

mod common;

use std::time::Duration;

use common::engine::{Opts, TOKEN, TestEngine, key};
use common::http::{open_sse, request};
use serde_json::{Value, json};

fn rev_header(r: &common::http::Resp) -> String {
    r.header("etag").expect("etag").to_owned()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn guards_in_order() {
    let e = TestEngine::start("api_guards", Opts::default()).await;
    let a = e.addr;
    let bearer = format!("Bearer {TOKEN}");

    // livez: the only unauthenticated path, 204, still versioned.
    let r = request(a, "GET", "/livez", &[], None).await;
    assert_eq!(r.status, 204);
    assert_eq!(r.header("oo-docs-api"), Some("1.0"));
    assert!(r.body.is_empty());

    // Authentication.
    for h in [
        vec![],
        vec![("Authorization", "Bearer wrong")],
        vec![("Authorization", "Basic abc")],
    ] {
        let r = request(a, "GET", "/v1/docs/drives", &h, None).await;
        assert_eq!(r.status, 401);
        assert_eq!(r.json()["code"], "DOCS_ENGINE_UNAUTHENTICATED");
        assert_eq!(
            r.header("oo-docs-api"),
            Some("1.0"),
            "errors are versioned too"
        );
    }
    // Origin is refused even with a valid token, and before the token check.
    let r = request(
        a,
        "GET",
        "/v1/docs/drives",
        &[
            ("Authorization", &bearer),
            ("Origin", "http://evil.example"),
        ],
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (403, Some("DOCS_ENGINE_FORBIDDEN_ORIGIN"))
    );
    let r = request(a, "GET", "/v1/docs/drives", &[("Origin", "null")], None).await;
    assert_eq!(r.status, 403, "no token needed to be refused");
    // Host: DNS rebinding.
    for host in ["evil.example", "127.0.0.1:1", "localhost", "127.0.0.1"] {
        let r = request(
            a,
            "GET",
            "/v1/docs/drives",
            &[("Authorization", &bearer), ("Host", host)],
            None,
        )
        .await;
        assert_eq!(
            (r.status, r.json()["code"].as_str()),
            (400, Some("DOCS_ENGINE_BAD_HOST")),
            "{host}"
        );
    }
    let r = request(a, "GET", "/livez", &[("Host", "evil.example")], None).await;
    assert_eq!(r.status, 400, "livez checks the host too");
    let ok_host = format!("localhost:{}", a.port());
    let r = request(
        a,
        "GET",
        "/v1/docs/drives",
        &[("Authorization", &bearer), ("Host", &ok_host)],
        None,
    )
    .await;
    assert_eq!(r.status, 200);

    // Version floor.
    let r = request(
        a,
        "GET",
        "/v1/docs/drives",
        &[("Authorization", &bearer), ("OO-Docs-Api-Min", "1.0")],
        None,
    )
    .await;
    assert_eq!(r.status, 200);
    let r = request(
        a,
        "GET",
        "/v1/docs/drives",
        &[("Authorization", &bearer), ("OO-Docs-Api-Min", "1.1")],
        None,
    )
    .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (400, Some("DOCS_ENGINE_VERSION_UNSUPPORTED"))
    );
    assert_eq!(r.json()["details"]["server"], "1.0");
    let r = request(
        a,
        "GET",
        "/v1/docs/drives",
        &[("Authorization", &bearer), ("OO-Docs-Api-Min", "junk")],
        None,
    )
    .await;
    assert_eq!(r.status, 400);

    // Routing errors.
    let r = e.get("/v1/docs/nothing").await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (404, Some("DOCS_ENGINE_NOT_FOUND"))
    );
    let r = e
        .get("/v1/docs/files/0f8fad5b-d9cb-469f-a165-70867728950e")
        .await;
    assert_eq!(r.status, 404, "documented but not served in L2");
    let r = e.req("PUT", "/v1/docs/drives", &[], None).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (405, Some("DOCS_ENGINE_METHOD_NOT_ALLOWED"))
    );
    assert_eq!(r.header("allow"), Some("GET, POST"));
    let r = e.req("POST", "/v1/docs/health", &[], None).await;
    assert_eq!(r.status, 405);

    // Body size.
    let big = "x".repeat(1024 * 1024 + 1);
    let k = key();
    let r = e
        .req(
            "POST",
            "/v1/docs/drives",
            &[("Idempotency-Key", &k)],
            Some(&json!({"name": big})),
        )
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (413, Some("DOCS_ENGINE_PAYLOAD_TOO_LARGE"))
    );
    assert_eq!(r.json()["details"]["max_bytes"], 1024 * 1024);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn rate_limit_is_per_token() {
    let e = TestEngine::start(
        "api_rate",
        Opts {
            tweak: Some(Box::new(|a| a.rate_per_second = 5)),
            ..Opts::default()
        },
    )
    .await;
    let mut limited = None;
    for _ in 0..30 {
        let r = e.get("/v1/docs/health").await;
        if r.status == 429 {
            limited = Some(r);
            break;
        }
    }
    let r = limited.expect("429 expected");
    assert_eq!(r.json()["code"], "DOCS_ENGINE_RATE_LIMITED");
    assert!(r.json()["retryable"].as_bool().unwrap());
    assert!(r.json()["details"]["retry_after_ms"].as_u64().unwrap() > 0);
    assert!(r.header("retry-after").is_some());
    // Unauthenticated callers do not drain the token's budget.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    for _ in 0..20 {
        request(e.addr, "GET", "/v1/docs/health", &[], None).await;
    }
    assert_eq!(e.get("/v1/docs/health").await.status, 200);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn health_reports_components() {
    let e = TestEngine::start("api_health", Opts::default()).await;
    let r = e.get("/v1/docs/health").await;
    assert_eq!(r.status, 200);
    let h = r.json();
    assert_eq!(h["api_version"], "1.0");
    assert_eq!(h["status"], "ok");
    assert_eq!(h["components"]["db"]["status"], "ok");
    assert_eq!(h["components"]["cas"]["status"], "ok");
    assert_eq!(h["components"]["model"]["status"], "not_configured");
    assert_eq!(h["events_head"], 0);
    assert_eq!(h["freshness"], "up_to_date");
    assert_eq!(h["queue"]["queued"], 0);
    assert!(h["disk"]["free_bytes"].as_u64().unwrap() > 0);

    // Database gone: the engine still answers, as unavailable.
    e.pool.close().await;
    let r = e.get("/v1/docs/health").await;
    assert_eq!(r.status, 200);
    let h = r.json();
    assert_eq!(h["status"], "unavailable");
    assert_eq!(h["components"]["db"]["status"], "unavailable");
    assert_eq!(h["components"]["db"]["code"], "DOCS_ENGINE_DB_UNAVAILABLE");
    // Data routes say so with 503 + retryable.
    let r = e.get("/v1/docs/drives").await;
    assert_eq!(r.status, 503);
    assert_eq!(r.json()["code"], "DOCS_ENGINE_DB_UNAVAILABLE");
    assert!(r.json()["retryable"].as_bool().unwrap());
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn drive_crud_idempotency_and_revisions() {
    let e = TestEngine::start("api_drives", Opts::default()).await;
    let body = json!({"name": "团队资料", "backend": "fs", "config": {"root": e.root.to_str().unwrap(), "exclude": ["*.bak"]}});

    // Idempotency-Key is required.
    let r = e.req("POST", "/v1/docs/drives", &[], Some(&body)).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (428, Some("DOCS_ENGINE_IDEMPOTENCY_KEY_REQUIRED"))
    );
    let r = e
        .req(
            "POST",
            "/v1/docs/drives",
            &[("Idempotency-Key", "short")],
            Some(&body),
        )
        .await;
    assert_eq!(r.status, 400);

    let k = key();
    let r = e
        .req(
            "POST",
            "/v1/docs/drives",
            &[("Idempotency-Key", &k)],
            Some(&body),
        )
        .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let drive = r.json();
    let id = drive["id"].as_str().unwrap().to_owned();
    assert_eq!(
        r.header("location"),
        Some(format!("/v1/docs/drives/{id}").as_str())
    );
    assert_eq!(r.header("etag"), Some("\"r1\""));
    assert_eq!(
        (drive["revision"].as_i64(), drive["status"].as_str()),
        (Some(1), Some("active"))
    );
    assert_eq!(drive["config"]["exclude"], json!(["*.bak"]));
    assert_eq!(drive["freshness"]["ontology_version"], 0);
    assert!(r.header("oo-idempotent-replay").is_none());

    // Same key, same body: the original response, marked.
    let again = e
        .req(
            "POST",
            "/v1/docs/drives",
            &[("Idempotency-Key", &k)],
            Some(&body),
        )
        .await;
    assert_eq!(again.status, 201);
    assert_eq!(again.header("oo-idempotent-replay"), Some("true"));
    assert_eq!(again.json(), drive);
    assert_eq!(again.header("location"), r.header("location"));
    // Same key, other body: conflict.
    let mut other = body.clone();
    other["name"] = json!("other");
    let r = e
        .req(
            "POST",
            "/v1/docs/drives",
            &[("Idempotency-Key", &k)],
            Some(&other),
        )
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (409, Some("DOCS_ENGINE_IDEMPOTENCY_CONFLICT"))
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.drive")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "the replay did not create a second drive");

    // Name collision (case insensitive) with a new key.
    let mut dup = body.clone();
    dup["name"] = json!("团队资料");
    let r = e.post("/v1/docs/drives", &dup).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (409, Some("DOCS_SOURCE_DRIVE_EXISTS"))
    );
    assert_eq!(r.json()["details"]["drive_id"], id);

    // Validation and registration probe.
    let r = e
        .post(
            "/v1/docs/drives",
            &json!({"name": "x", "backend": "fs", "config": {"root": "relative"}}),
        )
        .await;
    assert_eq!(
        (r.status, r.json()["details"]["pointer"].as_str()),
        (400, Some("/config/root"))
    );
    let r = e
        .post(
            "/v1/docs/drives",
            &json!({"name": "x", "backend": "fs", "config": {"root": "/definitely/not/here"}}),
        )
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (422, Some("DOCS_SOURCE_ROOT_MISSING"))
    );
    let r = e.post("/v1/docs/drives", &json!({"name": "x", "backend": "fs", "config": {"root": e.root.to_str().unwrap()}, "bogus": 1})).await;
    assert_eq!(r.status, 400);
    let r = e
        .req(
            "POST",
            "/v1/docs/drives",
            &[("Idempotency-Key", &key())],
            None,
        )
        .await;
    assert_eq!(
        r.status, 400,
        "an empty body is an empty object, which lacks a name"
    );

    // GET, list, ETag.
    let r = e.get(&format!("/v1/docs/drives/{id}")).await;
    assert_eq!((r.status, r.header("etag")), (200, Some("\"r1\"")));
    assert_eq!(
        e.get("/v1/docs/drives/not-a-uuid").await.json()["code"],
        "DOCS_SOURCE_DRIVE_NOT_FOUND"
    );
    assert_eq!(
        e.get("/v1/docs/drives/0f8fad5b-d9cb-469f-a165-70867728950e")
            .await
            .status,
        404
    );
    let page = e.get("/v1/docs/drives").await.json();
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["next_cursor"], Value::Null);
    assert!(page["as_of_seq"].as_i64().unwrap() >= 1);

    // PATCH needs If-Match.
    let path = format!("/v1/docs/drives/{id}");
    let r = e
        .req("PATCH", &path, &[], Some(&json!({"paused": true})))
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (428, Some("DOCS_ENGINE_PRECONDITION_REQUIRED"))
    );
    let r = e
        .req(
            "PATCH",
            &path,
            &[("If-Match", "\"r9\"")],
            Some(&json!({"paused": true})),
        )
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (412, Some("DOCS_ENGINE_PRECONDITION_FAILED"))
    );
    assert_eq!(r.json()["details"]["current_revision"], 1);
    let r = e
        .req("PATCH", &path, &[("If-Match", "\"r1\"")], Some(&json!({})))
        .await;
    assert_eq!(r.status, 400, "minProperties 1");
    let r = e.req("PATCH", &path, &[("If-Match", "\"r1\"")], Some(&json!({"paused": true, "name": "Renamed", "llm_daily_calls": 5, "exclude": ["*.tmp2"]}))).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let d = r.json();
    assert_eq!(
        (
            d["status"].as_str(),
            d["name"].as_str(),
            d["revision"].as_i64()
        ),
        (Some("paused"), Some("Renamed"), Some(2))
    );
    assert_eq!(
        (
            d["llm_daily_calls"].as_i64(),
            d["config"]["exclude"].clone()
        ),
        (Some(5), json!(["*.tmp2"]))
    );
    assert_eq!(rev_header(&r), "\"r2\"");
    // The old revision is now stale.
    let r = e
        .req(
            "PATCH",
            &path,
            &[("If-Match", "\"r1\"")],
            Some(&json!({"paused": false})),
        )
        .await;
    assert_eq!(r.status, 412);
    assert_eq!(r.json()["details"]["current_revision"], 2);
    // Paused drives refuse a sync.
    let r = e
        .post(&format!("/v1/docs/drives/{id}:sync"), &json!({}))
        .await;
    assert_eq!(r.status, 422);
    let r = e
        .req(
            "PATCH",
            &path,
            &[("If-Match", "\"r2\"")],
            Some(&json!({"paused": false})),
        )
        .await;
    assert_eq!(r.json()["status"], "active");

    // Events for all of it, in order.
    let types: Vec<String> = sqlx::query_scalar("SELECT type FROM _oo_docs.event ORDER BY seq")
        .fetch_all(&e.pool)
        .await
        .unwrap();
    assert!(types.iter().filter(|t| *t == "drive.created").count() == 1);
    assert!(types.iter().filter(|t| *t == "drive.updated").count() == 2);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn sync_files_tree_end_to_end() {
    let e = TestEngine::start(
        "api_sync",
        Opts {
            workers: true,
            watcher: true,
            ..Opts::default()
        },
    )
    .await;
    for i in 0..7 {
        e.write(&format!("docs/f{i}.md"), &format!("# file {i}\n"));
    }
    e.write("docs/deep/a.md", "deep a\n");
    e.write("docs/deep/er/b.txt", "deeper b\n");
    e.write("root.md", "root\n");
    let drive = e.create_drive("sync-test").await;
    let id = drive["id"].as_str().unwrap().to_owned();

    // The creation kicks the first reconcile; wait until all files are queued.
    let list = format!("/v1/docs/drives/{id}/files?state=queued&limit=1000");
    e.until("10 files queued", || async {
        e.get(&list).await.json()["items"]
            .as_array()
            .is_some_and(|a| a.len() == 10)
    })
    .await;
    let d = e.get(&format!("/v1/docs/drives/{id}")).await.json();
    assert_eq!(d["counts"]["queued"], 10);
    assert_eq!(d["files_total"], 10);
    assert_eq!(d["last_sync"]["status"], "complete");
    assert_eq!(d["last_sync"]["generation"], 1);
    assert_eq!(d["last_sync"]["added"], 10);

    // Listing: recursive, sorted by path bytes, keyset paging.
    let all = e
        .get(&format!("/v1/docs/drives/{id}/files?limit=4"))
        .await
        .json();
    assert_eq!(all["items"].as_array().unwrap().len(), 4);
    let mut paths: Vec<String> = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_owned())
        .collect();
    let mut cursor = all["next_cursor"].as_str().map(str::to_owned);
    assert!(cursor.is_some());
    while let Some(c) = cursor {
        let page = e
            .get(&format!("/v1/docs/drives/{id}/files?limit=4&cursor={c}"))
            .await
            .json();
        paths.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["path"].as_str().unwrap().to_owned()),
        );
        cursor = page["next_cursor"].as_str().map(str::to_owned);
    }
    let mut sorted = paths.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(paths, sorted, "no duplicates, no gaps, byte order");
    assert_eq!(paths.len(), 10);
    // A cursor from another query is refused.
    let c = all["next_cursor"].as_str().unwrap();
    let r = e
        .get(&format!(
            "/v1/docs/drives/{id}/files?limit=4&state=published&cursor={c}"
        ))
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (400, Some("DOCS_ENGINE_CURSOR_INVALID"))
    );
    let r = e
        .get(&format!("/v1/docs/drives/{id}/files?cursor=garbage"))
        .await;
    assert_eq!(r.json()["code"], "DOCS_ENGINE_CURSOR_INVALID");

    // Filters.
    let sub = e
        .get(&format!(
            "/v1/docs/drives/{id}/files?path=docs/deep&recursive=false"
        ))
        .await
        .json();
    assert_eq!(sub["items"].as_array().unwrap().len(), 1);
    assert_eq!(sub["items"][0]["name"], "a.md");
    let sub = e
        .get(&format!("/v1/docs/drives/{id}/files?path=docs/deep"))
        .await
        .json();
    assert_eq!(sub["items"].as_array().unwrap().len(), 2);
    let q = e
        .get(&format!("/v1/docs/drives/{id}/files?q=F3"))
        .await
        .json();
    assert_eq!(q["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        e.get(&format!("/v1/docs/drives/{id}/files?state=published"))
            .await
            .json()["items"],
        json!([])
    );
    assert_eq!(
        e.get(&format!("/v1/docs/drives/{id}/files?state=bogus"))
            .await
            .status,
        400
    );
    assert_eq!(
        e.get(&format!("/v1/docs/drives/{id}/files?sort=bogus"))
            .await
            .status,
        400
    );
    assert_eq!(
        e.get(&format!("/v1/docs/drives/{id}/files?limit=0"))
            .await
            .status,
        400
    );
    let by_state = e
        .get(&format!(
            "/v1/docs/drives/{id}/files?sort=state_since&limit=3"
        ))
        .await
        .json();
    assert!(by_state["next_cursor"].is_string());
    let by_mtime = e
        .get(&format!(
            "/v1/docs/drives/{id}/files?sort=modified_at&limit=3"
        ))
        .await
        .json();
    assert!(by_mtime["items"][0]["modified_at"].is_string(), "G1");
    let one = &all["items"][0];
    assert_eq!(one["format"], "md");
    assert!(one["b3"].as_str().unwrap().len() == 64);
    assert_eq!(one["retryable"], false);
    assert_eq!(
        e.get("/v1/docs/drives/0f8fad5b-d9cb-469f-a165-70867728950e/files")
            .await
            .json()["code"],
        "DOCS_SOURCE_DRIVE_NOT_FOUND"
    );

    // Tree: one level, with G9 counts.
    let t = e.get(&format!("/v1/docs/drives/{id}/tree")).await.json();
    let dirs = t["dirs"].as_array().unwrap();
    assert_eq!(dirs.len(), 1);
    assert_eq!(
        (dirs[0]["path"].as_str(), dirs[0]["files"].as_i64()),
        (Some("docs"), Some(9))
    );
    assert_eq!(dirs[0]["counts"]["queued"], 9);
    assert_eq!(dirs[0]["counts"]["failed"], 0);
    assert_eq!(dirs[0]["counts"]["pending_review"], 0);
    let files: Vec<&str> = t["files"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(files, ["root.md"]);
    let t = e
        .get(&format!("/v1/docs/drives/{id}/tree?path=docs"))
        .await
        .json();
    let dnames: Vec<&str> = t["dirs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert_eq!(dnames, ["docs/deep"]);
    assert_eq!(t["dirs"][0]["files"], 2);
    assert_eq!(t["files"]["items"].as_array().unwrap().len(), 7);

    // :sync. Idempotency required; a quiet manual sync completes.
    let r = e
        .req(
            "POST",
            &format!("/v1/docs/drives/{id}:sync"),
            &[],
            Some(&json!({})),
        )
        .await;
    assert_eq!(r.status, 428);
    let k = key();
    let sync_path = format!("/v1/docs/drives/{id}:sync");
    let r = e
        .req(
            "POST",
            &sync_path,
            &[("Idempotency-Key", &k)],
            Some(&json!({})),
        )
        .await;
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    let info = r.json();
    assert_eq!(
        (
            info["status"].as_str(),
            info["trigger"].as_str(),
            info["generation"].as_i64()
        ),
        (Some("running"), Some("manual"), Some(2))
    );
    let replay = e
        .req(
            "POST",
            &sync_path,
            &[("Idempotency-Key", &k)],
            Some(&json!({})),
        )
        .await;
    assert_eq!(
        (replay.status, replay.header("oo-idempotent-replay")),
        (202, Some("true"))
    );
    assert_eq!(replay.json(), info);
    e.until("generation 2", || async {
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["last_sync"]["generation"] == 2
            && e.get(&format!("/v1/docs/drives/{id}")).await.json()["last_sync"]["status"]
                == "complete"
    })
    .await;

    // Change on disk, then a deep sync.
    e.write("docs/f0.md", "# file 0, edited\n");
    e.rm("docs/f1.md");
    let r = e.post(&sync_path, &json!({"deep": true})).await;
    assert_eq!(r.status, 202);
    e.until("generation 3", || async {
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["last_sync"]["generation"] == 3
            && e.get(&format!("/v1/docs/drives/{id}")).await.json()["last_sync"]["status"]
                == "complete"
    })
    .await;
    let d = e.get(&format!("/v1/docs/drives/{id}")).await.json();
    assert_eq!(d["last_sync"]["removed"], 1);
    assert_eq!(d["files_total"], 9);
    let gone = e
        .get(&format!("/v1/docs/drives/{id}/files?state=deleted&q=f1"))
        .await
        .json();
    assert_eq!(
        gone["items"].as_array().unwrap().len(),
        1,
        "deleted files are listed on request"
    );

    // Targeted sync: validated, no generation, answers right away.
    e.write("docs/new.md", "targeted\n");
    let before = e.get(&format!("/v1/docs/drives/{id}")).await.json()["last_sync"]["generation"]
        .as_i64()
        .unwrap();
    let r = e
        .post(&sync_path, &json!({"paths": ["docs/new.md", "ghost.md"]}))
        .await;
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    let info = r.json();
    assert_eq!(
        (info["added"].as_i64(), info["status"].as_str()),
        (Some(1), Some("complete"))
    );
    assert_eq!(info["maybe_deleted"], json!(["ghost.md"]));
    assert_eq!(
        info["generation"], before,
        "targeted sync does not advance the generation"
    );
    for bad in [
        json!({"paths": ["/abs"]}),
        json!({"paths": ["a/../b"]}),
        json!({"paths": ["a"], "deep": true}),
        json!({"nope": 1}),
    ] {
        assert_eq!(e.post(&sync_path, &bad).await.status, 400, "{bad}");
    }
    let many: Vec<String> = (0..501).map(|i| format!("p{i}.md")).collect();
    assert_eq!(
        e.post(&sync_path, &json!({"paths": many})).await.status,
        400
    );
    e.until("targeted file queued", || async {
        e.get(&format!("/v1/docs/drives/{id}/files?q=new.md&state=queued"))
            .await
            .json()["items"]
            .as_array()
            .is_some_and(|a| a.len() == 1)
    })
    .await;
    assert_eq!(
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["last_sync"]["generation"],
        before
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn mass_delete_hold_and_confirm_over_http() {
    let e = TestEngine::start(
        "api_mass",
        Opts {
            workers: true,
            ..Opts::default()
        },
    )
    .await;
    for i in 0..150 {
        e.write(&format!("bulk/f{i:03}.md"), &format!("content {i}\n"));
    }
    let drive = e.create_drive("mass").await;
    let id = drive["id"].as_str().unwrap().to_owned();
    let list = format!("/v1/docs/drives/{id}/files?state=queued&limit=1000");
    e.until("150 queued", || async {
        e.get(&list).await.json()["items"]
            .as_array()
            .is_some_and(|a| a.len() == 150)
    })
    .await;
    for i in 0..120 {
        e.rm(&format!("bulk/f{i:03}.md"));
    }
    let sync = format!("/v1/docs/drives/{id}:sync");
    assert_eq!(e.post(&sync, &json!({})).await.status, 202);
    e.until("held", || async {
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["freshness"]["state"] == "held"
    })
    .await;
    let d = e.get(&format!("/v1/docs/drives/{id}")).await.json();
    assert_eq!(d["last_sync"]["status"], "held");
    assert_eq!(
        d["freshness"]["reason_code"],
        "DOCS_SOURCE_MASS_DELETE_HELD"
    );
    assert_eq!(d["files_total"], 150, "nothing deleted while held");
    assert_eq!(
        e.get("/v1/docs/health").await.json()["freshness"],
        "held",
        "Health summarises the highest priority"
    );

    // A plain sync is refused with the generation to confirm.
    let r = e.post(&sync, &json!({})).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (422, Some("DOCS_SOURCE_MASS_DELETE_HELD"))
    );
    assert_eq!(
        (
            r.json()["details"]["generation"].as_i64(),
            r.json()["details"]["would_delete"].as_i64()
        ),
        (Some(2), Some(120))
    );
    // The wrong generation too.
    let r = e.post(&sync, &json!({"confirm_mass_delete": 7})).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (422, Some("DOCS_SOURCE_MASS_DELETE_HELD"))
    );
    // The right one releases it.
    let r = e.post(&sync, &json!({"confirm_mass_delete": 2})).await;
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    e.until("sweep", || async {
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["files_total"] == 30
    })
    .await;
    assert_eq!(
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["last_sync"]["status"],
        "complete"
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn delete_drive_reaps_files() {
    let e = TestEngine::start(
        "api_delete",
        Opts {
            workers: true,
            ..Opts::default()
        },
    )
    .await;
    e.write("a.md", "a\n");
    e.write("b.md", "b\n");
    let drive = e.create_drive("to-delete").await;
    let id = drive["id"].as_str().unwrap().to_owned();
    let path = format!("/v1/docs/drives/{id}");
    e.until("queued", || async {
        e.get(&path).await.json()["counts"]["queued"] == 2
    })
    .await;

    let r = e.req("DELETE", &path, &[], None).await;
    assert_eq!(r.status, 428);
    let r = e
        .req("DELETE", &path, &[("If-Match", "\"r5\"")], None)
        .await;
    assert_eq!(
        (r.status, r.json()["details"]["current_revision"].as_i64()),
        (412, Some(1))
    );
    // An active share blocks it.
    let pack = "00000000-0000-0000-0000-0000000000a1";
    sqlx::query("INSERT INTO _oo_docs.ontology_version (n, as_of, facts_added, facts_retracted, digest, gate_report_sha256, trigger) VALUES (1, current_date, 0, 0, repeat('0', 64), 'x', 'auto')")
        .execute(&sqlx::PgPool::connect_with(admin_opts("api_delete")).await.unwrap()).await.unwrap();
    let admin = sqlx::PgPool::connect_with(admin_opts("api_delete"))
        .await
        .unwrap();
    sqlx::query("INSERT INTO _oo_docs.context_pack (id, version_n, drive_id, scope_sha256, params_sha256, compiler, pack_digest, snapshot_sha256, manifest, tokens_brief, trimmed, blocked_facts, blocked_entities) \
                 VALUES ($1::uuid, 1, $2::uuid, 'a', 'b', 'c', repeat('0', 64), 'd', '{}', 1, '{}', 0, 0)")
        .bind(pack).bind(&id).execute(&admin).await.unwrap();
    sqlx::query("INSERT INTO _oo_docs.share (id, pack_id, drive_id, scope, role, budget, status, grantor, mount_ref, expires_at) \
                 VALUES ('00000000-0000-0000-0000-0000000000b1', $1::uuid, $2::uuid, '{}', 'viewer', 800, 'active', 'it', 'm1', now() + interval '1 day')")
        .bind(pack).bind(&id).execute(&admin).await.unwrap();
    let r = e
        .req("DELETE", &path, &[("If-Match", "\"r1\"")], None)
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (409, Some("DOCS_SOURCE_DRIVE_HAS_ACTIVE_SHARES"))
    );
    assert_eq!(
        r.json()["details"]["share_ids"],
        json!(["00000000-0000-0000-0000-0000000000b1"])
    );
    sqlx::query("UPDATE _oo_docs.share SET status = 'revoked'")
        .execute(&admin)
        .await
        .unwrap();

    let r = e
        .req("DELETE", &path, &[("If-Match", "\"r1\"")], None)
        .await;
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(r.json()["status"], "deleting");
    assert_eq!(r.json()["revision"], 2);
    // The reaper is not part of this test engine: run one pass by hand.
    oo_docs::api::lifecycle::reap_once(&e.pool).await.unwrap();
    assert_eq!(e.get(&path).await.status, 404, "a deleted drive is gone");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.file WHERE deleted_at IS NULL")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let kinds: Vec<String> = sqlx::query_scalar("SELECT type FROM _oo_docs.event WHERE type IN ('drive.deleted', 'files.bulk_changed') ORDER BY seq")
        .fetch_all(&e.pool).await.unwrap();
    assert_eq!(kinds, ["files.bulk_changed", "drive.deleted"]);
    assert_eq!(e.get("/v1/docs/drives").await.json()["items"], json!([]));
    admin.close().await;
    e.finish().await;
}

fn admin_opts(db: &str) -> sqlx::postgres::PgConnectOptions {
    let port: u16 = std::env::var("OO_DOCS_TEST_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    sqlx::postgres::PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("postgres")
        .database(db)
        .ssl_mode(sqlx::postgres::PgSslMode::Disable)
}

// ---------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------

async fn seq_of(e: &TestEngine) -> i64 {
    e.get("/v1/docs/health").await.json()["events_head"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn events_since_replay_is_gap_free() {
    let e = TestEngine::start(
        "api_events",
        Opts {
            workers: true,
            ..Opts::default()
        },
    )
    .await;
    let bearer = format!("Bearer {TOKEN}");
    let auth = [("Authorization", bearer.as_str())];

    // Nothing yet: since=0 on an empty log is fine, and so is a live stream.
    let mut s = open_sse(e.addr, "/v1/docs/events?since=0", &auth).await;
    assert_eq!(s.status, 200);
    assert_eq!(s.header("content-type"), Some("text/event-stream"));
    assert_eq!(s.header("oo-docs-api"), Some("1.0"));
    assert_eq!(
        s.frame(Duration::from_secs(5)).await.as_deref(),
        Some("retry: 3000")
    );

    // Produce events: a drive and 30 files.
    for i in 0..30 {
        e.write(&format!("d{}/f{i:02}.md", i % 4), &format!("n{i}\n"));
    }
    let drive = e.create_drive("events").await;
    let id = drive["id"].as_str().unwrap().to_owned();
    let list = format!("/v1/docs/drives/{id}/files?state=queued&limit=1000");
    e.until("queued", || async {
        e.get(&list).await.json()["items"]
            .as_array()
            .is_some_and(|a| a.len() == 30)
    })
    .await;
    e.until("generation", || async {
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["last_sync"]["status"] == "complete"
    })
    .await;
    let head = seq_of(&e).await;
    assert!(
        head > 60,
        "drive + sync + 30 files x2 transitions, got {head}"
    );

    // The live stream saw every event in order, once.
    let mut live = Vec::new();
    while let Some(ev) = s.event(Duration::from_secs(2)).await {
        live.push(ev);
        if live.last().is_some_and(|e| e["seq"].as_i64() == Some(head)) {
            break;
        }
    }
    let seqs: Vec<i64> = live.iter().map(|e| e["seq"].as_i64().unwrap()).collect();
    assert_eq!(
        seqs,
        (1..=head).collect::<Vec<_>>(),
        "live stream: gap free, ordered"
    );
    for ev in &live {
        assert!(ev["at"].is_string() && ev["data"].is_object());
    }
    let fsc = live
        .iter()
        .find(|e| e["type"] == "file.state_changed")
        .unwrap();
    assert!(
        fsc["data"]["file"]["path"].is_string(),
        "file events carry a FileSummary"
    );
    assert!(fsc["data"]["file"]["state"].is_string());
    drop(s);

    // Replay from several points reaches the same head with no hole.
    for since in [0, 1, head / 2, head - 1, head] {
        let mut r = open_sse(e.addr, &format!("/v1/docs/events?since={since}"), &auth).await;
        assert_eq!(r.status, 200, "since={since}");
        let mut got = Vec::new();
        while got.len() < (head - since) as usize {
            match r.event(Duration::from_secs(5)).await {
                Some(ev) => got.push(ev["seq"].as_i64().unwrap()),
                None => panic!("since={since}: stream stalled after {} events", got.len()),
            }
        }
        assert_eq!(
            got,
            ((since + 1)..=head).collect::<Vec<_>>(),
            "since={since}"
        );
    }

    // Last-Event-ID is the same cursor; since wins when both are there.
    let mut r = open_sse(
        e.addr,
        "/v1/docs/events",
        &[
            ("Authorization", &bearer),
            ("Last-Event-ID", &(head - 2).to_string()),
        ],
    )
    .await;
    let first = r.event(Duration::from_secs(5)).await.unwrap();
    assert_eq!(first["seq"].as_i64(), Some(head - 1));
    let mut r = open_sse(
        e.addr,
        &format!("/v1/docs/events?since={}", head - 1),
        &[("Authorization", &bearer), ("Last-Event-ID", "1")],
    )
    .await;
    assert_eq!(
        r.event(Duration::from_secs(5)).await.unwrap()["seq"].as_i64(),
        Some(head)
    );

    // Replay, then the live tail, with events committed in between: no gap
    // and no repeat at the seam.
    let mut r = open_sse(
        e.addr,
        &format!("/v1/docs/events?since={}", head - 5),
        &auth,
    )
    .await;
    e.write("late/one.md", "late 1\n");
    e.write("late/two.md", "late 2\n");
    let r2 = e
        .post(
            &format!("/v1/docs/drives/{id}:sync"),
            &json!({"paths": ["late/one.md", "late/two.md"]}),
        )
        .await;
    assert_eq!(r2.status, 202);
    let want = seq_of(&e).await;
    let mut seen = Vec::new();
    while seen.last().copied() != Some(want) {
        let ev = r.event(Duration::from_secs(5)).await.expect("tail stalled");
        seen.push(ev["seq"].as_i64().unwrap());
    }
    assert_eq!(seen, ((head - 4)..=want).collect::<Vec<_>>());
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn events_filter_expiry_and_limits() {
    let e = TestEngine::start("api_events2", Opts::default()).await;
    let bearer = format!("Bearer {TOKEN}");
    let auth = [("Authorization", bearer.as_str())];
    e.write("a.md", "a\n");
    let _ = e.create_drive("ev2").await;
    let id = e.get("/v1/docs/drives").await.json()["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = e
        .req(
            "PATCH",
            &format!("/v1/docs/drives/{id}"),
            &[("If-Match", "\"r1\"")],
            Some(&json!({"name": "ev2b"})),
        )
        .await;
    assert_eq!(r.status, 200);
    for rev in 2..=4 {
        let r = e
            .req(
                "PATCH",
                &format!("/v1/docs/drives/{id}"),
                &[("If-Match", &format!("\"r{rev}\""))],
                Some(&json!({"llm_daily_calls": rev})),
            )
            .await;
        assert_eq!(r.status, 200);
    }
    let head = seq_of(&e).await;
    assert!(head >= 5, "head {head}");

    // `types` filters delivery but not the cursor: seq values keep their gaps.
    let mut s = open_sse(e.addr, "/v1/docs/events?since=0&types=drive.updated", &auth).await;
    let ev = s.event(Duration::from_secs(5)).await.unwrap();
    assert_eq!(ev["type"], "drive.updated");
    assert!(
        ev["seq"].as_i64().unwrap() > 1,
        "drive.created (seq 1) was filtered out"
    );
    assert_eq!(ev["data"]["drive"]["name"], "ev2b");
    // The other two updates follow; nothing of another type is delivered.
    for _ in 0..3 {
        assert_eq!(
            s.event(Duration::from_secs(3)).await.unwrap()["type"],
            "drive.updated"
        );
    }
    assert!(
        s.event(Duration::from_millis(600)).await.is_none(),
        "nothing else matches"
    );
    assert_eq!(
        open_sse(e.addr, "/v1/docs/events?types=Bad Type", &auth)
            .await
            .status,
        400
    );

    // Expired and impossible cursors: 410 with the bounds.
    let admin = sqlx::PgPool::connect_with(admin_opts("api_events2"))
        .await
        .unwrap();
    sqlx::query("ALTER TABLE _oo_docs.event DISABLE TRIGGER event_insert_only")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM _oo_docs.event WHERE seq < 3")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE _oo_docs.event ENABLE TRIGGER event_insert_only")
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let s = open_sse(e.addr, "/v1/docs/events?since=0", &auth).await;
    assert_eq!(s.status, 410);
    let body = s.error_json();
    assert_eq!(body["code"], "DOCS_ENGINE_CURSOR_EXPIRED");
    assert_eq!(body["details"]["oldest_seq"], 3);
    assert_eq!(body["details"]["head_seq"], head);
    assert_eq!(s.header("oo-docs-api"), Some("1.0"));
    // since = oldest - 1 still has everything it needs.
    assert_eq!(
        open_sse(e.addr, "/v1/docs/events?since=2", &auth)
            .await
            .status,
        200
    );
    assert_eq!(
        open_sse(
            e.addr,
            &format!("/v1/docs/events?since={}", head + 100),
            &auth
        )
        .await
        .status,
        410
    );
    assert_eq!(
        open_sse(e.addr, "/v1/docs/events?since=abc", &auth)
            .await
            .status,
        400
    );

    // Idle streams get a ping comment.
    let mut ping = TestEngine::start(
        "api_events3",
        Opts {
            tweak: Some(Box::new(|a| {
                a.sse.ping = Duration::from_millis(300);
                a.sse.poll = Duration::from_millis(100);
            })),
            ..Opts::default()
        },
    )
    .await;
    let mut s = open_sse(ping.addr, "/v1/docs/events", &auth).await;
    assert_eq!(
        s.frame(Duration::from_secs(5)).await.as_deref(),
        Some("retry: 3000")
    );
    assert_eq!(
        s.frame(Duration::from_secs(5)).await.as_deref(),
        Some(": ping")
    );
    drop(s);
    let _ = &mut ping;
    ping.finish().await;
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn sse_slots_are_limited() {
    let e = TestEngine::start("api_sse_slots", Opts::default()).await;
    let bearer = format!("Bearer {TOKEN}");
    let auth = [("Authorization", bearer.as_str())];
    let mut open = Vec::new();
    for _ in 0..16 {
        let s = open_sse(e.addr, "/v1/docs/events", &auth).await;
        assert_eq!(s.status, 200);
        open.push(s);
    }
    let s = open_sse(e.addr, "/v1/docs/events", &auth).await;
    assert_eq!(s.status, 429);
    assert_eq!(s.error_json()["code"], "DOCS_ENGINE_RATE_LIMITED");
    assert!(s.header("retry-after").is_some());
    // Closing one frees its slot.
    open.pop();
    let mut ok = false;
    for _ in 0..40 {
        if open_sse(e.addr, "/v1/docs/events", &auth).await.status == 200 {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ok, "a closed stream releases its slot");
    drop(open);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn freshness_changes_are_events() {
    let e = TestEngine::start(
        "api_fresh",
        Opts {
            workers: true,
            watcher: true,
            ..Opts::default()
        },
    )
    .await;
    e.write("a.md", "a\n");
    let drive = e.create_drive("fresh").await;
    let id = drive["id"].as_str().unwrap().to_owned();
    e.until("queued", || async {
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["freshness"]["state"] == "processing"
    })
    .await;
    // Let the watcher take its (silent) baseline, then finish the file: the
    // Drive goes processing -> up_to_date, which must be announced.
    tokio::time::sleep(Duration::from_millis(500)).await;
    sqlx::query("UPDATE _oo_docs.file SET state = 'published'")
        .execute(&e.pool)
        .await
        .unwrap();
    e.until("freshness event", || async {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.event WHERE type = 'drive.freshness_changed'",
        )
        .fetch_one(&e.pool)
        .await
        .unwrap();
        n >= 1
    })
    .await;
    let ev: String = sqlx::query_scalar(
        "SELECT payload::text FROM _oo_docs.event WHERE type = 'drive.freshness_changed' ORDER BY seq LIMIT 1",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    let v: Value = serde_json::from_str(&ev).unwrap();
    assert_eq!(v["drive_id"], id);
    assert_eq!(v["freshness"]["state"], "up_to_date");
    // A quiet watcher stays quiet.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.event WHERE type = 'drive.freshness_changed'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
    e.finish().await;
}
