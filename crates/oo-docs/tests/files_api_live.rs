//! The per file API on a real engine and database (B-S3-07): detail, blocks,
//! original bytes, batch retry, `file.progress`.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test files_api_live -- --ignored --test-threads=2`

mod common;

use std::time::Duration;

use common::engine::{Opts, TOKEN, TestEngine};
use common::http::open_sse;
use oo_docs::api::progress::Progress;
use serde_json::{Value, json};

const PROJECT: &str = "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n";
const PERSON: &str =
    "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n\n角色：工程负责人\n";

fn stages() -> Opts {
    Opts {
        workers: true,
        stages: true,
        ..Opts::default()
    }
}

async fn published(e: &TestEngine, n: i64) -> bool {
    let c: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'published'")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    c == n
}

async fn fid(e: &TestEngine, path: &str) -> String {
    sqlx::query_scalar("SELECT id::text FROM _oo_docs.file WHERE path = $1 AND deleted_at IS NULL")
        .bind(path)
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn file_detail_has_history_entities_facts_and_shares() {
    let e = TestEngine::start("fapi_detail", stages()).await;
    e.write("Projects/a/星火计划.md", PROJECT);
    e.write("People/林川.md", PERSON);
    let drive = e.create_drive("fapi-detail").await;
    let did = drive["id"].as_str().unwrap().to_owned();
    e.until("published", || async { published(&e, 2).await })
        .await;
    let id = fid(&e, "Projects/a/星火计划.md").await;

    // Without includes: the summary, versions, parse, llm.
    let r = e.get(&format!("/v1/docs/files/{id}")).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let d = r.json();
    assert_eq!(
        (d["id"].as_str(), d["state"].as_str()),
        (Some(id.as_str()), Some("published"))
    );
    assert_eq!(d["versions"].as_array().unwrap().len(), 1);
    assert_eq!(d["parse"]["parser_id"], "oo-md");
    assert!(d["parse"]["block_count"].as_i64().unwrap() > 0);
    assert_eq!(d["llm"]["status"], "skipped_not_configured");
    assert!(d["latency_ms"]["total_ms"].is_i64(), "G13");
    for k in ["history", "entities", "facts", "shares"] {
        assert!(d.get(k).is_none(), "{k} only when asked");
    }

    // With everything.
    let d = e
        .get(&format!(
            "/v1/docs/files/{id}?include=history,entities,facts,shares"
        ))
        .await
        .json();
    let h = d["history"].as_array().unwrap();
    let tos: Vec<&str> = h.iter().map(|x| x["to"].as_str().unwrap()).collect();
    assert_eq!(
        tos,
        [
            "stored",
            "queued",
            "parsing",
            "parsed",
            "extracting",
            "indexed",
            "published"
        ]
    );
    assert!(h[0]["from"].is_null() || h[0].get("from").is_none());
    assert_eq!(h[1]["from"], "stored");
    for (i, x) in h.iter().enumerate() {
        assert_eq!(x["seq"].as_i64(), Some(i as i64 + 1));
        if i > 0 {
            assert!(
                x["duration_ms"].as_i64().unwrap() >= 0,
                "time spent in `from`"
            );
        }
    }
    let names: Vec<&str> = d["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap())
        .collect();
    // The document itself, the project directory it sits in and its owner.
    for want in ["Projects/a/星火计划.md", "a", "林川"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    let facts = d["facts"].as_array().unwrap();
    assert!(!facts.is_empty());
    for f in facts {
        assert_eq!(f["status"], "published");
        assert_eq!(
            f["evidence_total"].as_i64().unwrap() as usize,
            f["evidence"].as_array().unwrap().len()
        );
        for ev in f["evidence"].as_array().unwrap() {
            assert_eq!(ev["file_id"], id);
            assert_eq!(ev["path"], "Projects/a/星火计划.md");
            assert!(
                ev["src"].as_str().unwrap().starts_with("[src:Projects/a/"),
                "{ev}"
            );
            assert!(ev["quote"].is_string(), "the block text is the quote");
        }
    }
    assert_eq!(d["shares"], json!([]), "no share yet");

    // A share that used the facts shows up with how many.
    let did2 = did.clone();
    let share = e
        .post(
            "/v1/docs/shares",
            &json!({"compile": {"drive_id": did2, "scope": ["Projects/a"], "budget": 800},
                    "mount": {"target_dir": "/tmp/oo-fapi-unused"}}),
        )
        .await;
    assert_eq!(
        share.status,
        201,
        "{}",
        String::from_utf8_lossy(&share.body)
    );
    let d = e
        .get(&format!("/v1/docs/files/{id}?include=shares"))
        .await
        .json();
    let s = d["shares"].as_array().unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0]["id"], share.json()["id"]);
    assert_eq!(s[0]["state"], "active");
    assert!(s[0]["facts_used"].as_i64().unwrap() > 0);

    // Errors.
    for bad in ["not-a-uuid", "00000000-0000-4000-8000-000000000000"] {
        let r = e.get(&format!("/v1/docs/files/{bad}")).await;
        assert_eq!(
            (r.status, r.json()["code"].as_str().map(str::to_owned)),
            (404, Some("DOCS_ENGINE_NOT_FOUND".into()))
        );
    }
    let r = e.get(&format!("/v1/docs/files/{id}?include=nope")).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str().map(str::to_owned)),
        (400, Some("DOCS_ENGINE_BAD_REQUEST".into()))
    );
    assert_eq!(
        e.req("POST", &format!("/v1/docs/files/{id}"), &[], None)
            .await
            .status,
        405
    );

    // A deleted file can still be looked at.
    std::fs::remove_file(e.root.join("People/林川.md")).unwrap();
    let _ = e
        .post(&format!("/v1/docs/drives/{did}:sync"), &json!({}))
        .await;
    let pid = fid_any(&e, "People/林川.md").await;
    e.until("deleted", || async {
        e.get(&format!("/v1/docs/files/{pid}")).await.json()["state"] == "deleted"
    })
    .await;
    let d = e
        .get(&format!("/v1/docs/files/{pid}?include=history,facts"))
        .await
        .json();
    assert_eq!(
        d["history"].as_array().unwrap().last().unwrap()["to"],
        "deleted"
    );
    e.finish().await;
}

async fn fid_any(e: &TestEngine, path: &str) -> String {
    sqlx::query_scalar("SELECT id::text FROM _oo_docs.file WHERE path = $1")
        .bind(path)
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn blocks_are_paged_filtered_and_not_ready_before_the_parse() {
    let e = TestEngine::start("fapi_blocks", stages()).await;
    let mut text = String::from("# 长文档\n\n");
    for i in 0..12 {
        text.push_str(&format!("第{i}段内容，介绍星火计划的进展。\n\n"));
    }
    e.write("docs/long.md", &text);
    e.create_drive("fapi-blocks").await;
    e.until("published", || async { published(&e, 1).await })
        .await;
    let id = fid(&e, "docs/long.md").await;

    let all = e
        .get(&format!("/v1/docs/files/{id}/blocks?limit=1000"))
        .await
        .json();
    let items = all["items"].as_array().unwrap();
    assert_eq!(items.len(), 13);
    assert!(all["next_cursor"].is_null());
    assert!(all["as_of_seq"].as_i64().unwrap() > 0);
    for (i, b) in items.iter().enumerate() {
        assert_eq!(b["ordinal"].as_i64(), Some(i as i64));
        assert!(b["id"].as_str().unwrap().starts_with("blk_"));
        assert!(b["text"].is_string() && b["anchor"]["kind"] == "text");
        assert!(
            b["src"]
                .as_str()
                .unwrap()
                .starts_with("[src:docs/long.md#L"),
            "{b}"
        );
    }
    // Pages of 5 walk the same blocks in the same order, once each.
    let mut seen: Vec<String> = Vec::new();
    let mut cursor = String::new();
    let mut pages = 0;
    loop {
        let url = if cursor.is_empty() {
            format!("/v1/docs/files/{id}/blocks?limit=5")
        } else {
            format!("/v1/docs/files/{id}/blocks?limit=5&cursor={cursor}")
        };
        let p = e.get(&url).await.json();
        pages += 1;
        seen.extend(
            p["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b["id"].as_str().unwrap().to_owned()),
        );
        match p["next_cursor"].as_str() {
            Some(c) => cursor = c.to_owned(),
            None => break,
        }
    }
    assert_eq!(pages, 3);
    let want: Vec<String> = items
        .iter()
        .map(|b| b["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(seen, want);
    // `page` is for paged formats; a text file has none.
    let r = e.get(&format!("/v1/docs/files/{id}/blocks?page=1")).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["items"], json!([]));
    for bad in ["page=0", "page=x", "limit=0", "limit=5000"] {
        let r = e.get(&format!("/v1/docs/files/{id}/blocks?{bad}")).await;
        assert_eq!(r.status, 400, "{bad}");
    }
    // A cursor belongs to its request: another page filter refuses it.
    let r = e
        .get(&format!(
            "/v1/docs/files/{id}/blocks?limit=5&cursor={cursor_of_first_page}&page=2",
            cursor_of_first_page = {
                let p = e
                    .get(&format!("/v1/docs/files/{id}/blocks?limit=5"))
                    .await
                    .json();
                p["next_cursor"].as_str().unwrap().to_owned()
            }
        ))
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str().map(str::to_owned)),
        (400, Some("DOCS_ENGINE_CURSOR_INVALID".into()))
    );
    assert_eq!(
        e.get("/v1/docs/files/00000000-0000-4000-8000-000000000000/blocks")
            .await
            .status,
        404
    );
    e.finish().await;

    // Before the parse: 409 with the state.
    let e = TestEngine::start(
        "fapi_blocks2",
        Opts {
            workers: true,
            ..Opts::default()
        },
    )
    .await;
    e.write("a.md", "# a\n");
    let drive = e.create_drive("fapi-blocks2").await;
    let did = drive["id"].as_str().unwrap().to_owned();
    e.until("queued", || async {
        e.get(&format!("/v1/docs/drives/{did}/files?state=queued"))
            .await
            .json()["items"]
            .as_array()
            .is_some_and(|a| a.len() == 1)
    })
    .await;
    let id = fid(&e, "a.md").await;
    let r = e.get(&format!("/v1/docs/files/{id}/blocks")).await;
    assert_eq!(r.status, 409);
    let j = r.json();
    assert_eq!(j["code"], "DOCS_PARSE_NOT_READY");
    assert_eq!(
        (j["retryable"].as_bool(), j["details"]["state"].as_str()),
        (Some(true), Some("queued"))
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn content_serves_the_original_with_etag_and_ranges() {
    let e = TestEngine::start("fapi_content", stages()).await;
    e.write("docs/原件.md", PROJECT);
    e.create_drive("fapi-content").await;
    e.until("published", || async { published(&e, 1).await })
        .await;
    let id = fid(&e, "docs/原件.md").await;
    let b3 = blake3::hash(PROJECT.as_bytes()).to_hex().to_string();
    let url = format!("/v1/docs/files/{id}/content");
    let total = PROJECT.len();

    let r = e.get(&url).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body, PROJECT.as_bytes());
    assert_eq!(
        r.header("etag"),
        Some(format!("\"{b3}\"").as_str()),
        "ETag is the content hash"
    );
    assert_eq!(r.header("accept-ranges"), Some("bytes"));
    assert!(
        r.header("content-type")
            .unwrap()
            .starts_with("text/markdown")
    );

    // Ranges.
    let r = e.req("GET", &url, &[("Range", "bytes=0-9")], None).await;
    assert_eq!(r.status, 206);
    assert_eq!(r.body, &PROJECT.as_bytes()[..10]);
    assert_eq!(
        r.header("content-range"),
        Some(format!("bytes 0-9/{total}").as_str())
    );
    assert_eq!(r.header("etag"), Some(format!("\"{b3}\"").as_str()));
    let r = e.req("GET", &url, &[("Range", "bytes=-6")], None).await;
    assert_eq!(
        (r.status, r.body.as_slice()),
        (206, &PROJECT.as_bytes()[total - 6..])
    );
    let r = e
        .req(
            "GET",
            &url,
            &[("Range", &format!("bytes={}-", total - 3))],
            None,
        )
        .await;
    assert_eq!(
        (r.status, r.body.as_slice()),
        (206, &PROJECT.as_bytes()[total - 3..])
    );
    let r = e
        .req("GET", &url, &[("Range", &format!("bytes={total}-"))], None)
        .await;
    assert_eq!(r.status, 416);
    assert_eq!(
        r.header("content-range"),
        Some(format!("bytes */{total}").as_str())
    );
    assert!(r.json()["code"].is_string(), "an Error body");
    // A Range the server does not understand is ignored.
    let r = e
        .req("GET", &url, &[("Range", "bytes=0-1,4-5")], None)
        .await;
    assert_eq!((r.status, r.body.len()), (200, total));

    // Conditional.
    let r = e
        .req(
            "GET",
            &url,
            &[("If-None-Match", &format!("\"{b3}\""))],
            None,
        )
        .await;
    assert_eq!((r.status, r.body.len()), (304, 0));
    assert_eq!(r.header("etag"), Some(format!("\"{b3}\"").as_str()));
    let r = e
        .req("GET", &url, &[("If-None-Match", "\"other\"")], None)
        .await;
    assert_eq!(r.status, 200);

    // The token is required, like everywhere else.
    let anon = common::http::request(e.addr, "GET", &url, &[], None).await;
    assert_eq!(anon.status, 401);
    assert_eq!(
        e.get("/v1/docs/files/00000000-0000-4000-8000-000000000000/content")
            .await
            .status,
        404
    );

    // The file changed on disk after it was hashed: whole reads refuse to
    // wear the old hash; the next sync makes a new version and it works again.
    std::thread::sleep(Duration::from_millis(1100));
    e.write("docs/原件.md", &PROJECT.replace("林川", "周远周远"));
    let r = e.get(&url).await;
    assert_eq!(r.status, 409);
    assert_eq!(r.json()["code"], "DOCS_SOURCE_CHANGED_DURING_READ");
    assert_eq!(r.json()["retryable"], true);
    let _ = TOKEN;
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn batch_retry_answers_each_file() {
    let e = TestEngine::start(
        "fapi_retry",
        Opts {
            workers: true,
            ..Opts::default()
        },
    )
    .await;
    e.write("a.md", "# a\n");
    e.write("b.md", "# b\n");
    e.write("c.md", "# c\n");
    let drive = e.create_drive("fapi-retry").await;
    let did = drive["id"].as_str().unwrap().to_owned();
    e.until("three queued", || async {
        e.get(&format!("/v1/docs/drives/{did}/files?state=queued"))
            .await
            .json()["items"]
            .as_array()
            .is_some_and(|a| a.len() == 3)
    })
    .await;
    let (a, b, c) = (
        fid(&e, "a.md").await,
        fid(&e, "b.md").await,
        fid(&e, "c.md").await,
    );
    // a and b failed (set directly: the stage handlers are not running here);
    // c is just queued.
    sqlx::query("UPDATE _oo_docs.file SET state = 'parse_failed', state_code = 'DOCS_PARSE_TIMEOUT', retryable = true WHERE path IN ('a.md', 'b.md')")
        .execute(&e.pool)
        .await
        .unwrap();
    let ghost = "00000000-0000-4000-8000-000000000000";
    let body = json!({"file_ids": [a, c, ghost, a, b], "mode": "safe"});
    let key = common::engine::key();
    let r = e
        .req(
            "POST",
            "/v1/docs/files:retry",
            &[("Idempotency-Key", &key)],
            Some(&body),
        )
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let res = r.json();
    let results = res["results"].as_array().unwrap();
    assert_eq!(results.len(), 4, "a repeated id is answered once");
    assert_eq!(
        (results[0]["id"].as_str(), results[0]["status"].as_str()),
        (Some(a.as_str()), Some("ok"))
    );
    assert_eq!(results[1]["status"], "not_retryable");
    assert_eq!(results[1]["code"], "state:queued");
    assert_eq!(
        (results[2]["status"].as_str(), results[2]["code"].as_str()),
        (Some("not_found"), Some("DOCS_ENGINE_NOT_FOUND"))
    );
    assert_eq!(results[3]["status"], "ok");
    // a and b are queued again, each with a manual priority job, history marks it.
    for path in ["a.md", "b.md"] {
        let (state, code, retryable): (String, Option<String>, bool) = sqlx::query_as(
            "SELECT state, state_code, retryable FROM _oo_docs.file WHERE path = $1",
        )
        .bind(path)
        .fetch_one(&e.pool)
        .await
        .unwrap();
        assert_eq!(
            (state.as_str(), code, retryable),
            ("queued", None, false),
            "{path}"
        );
    }
    let jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse' AND priority = 50 AND state = 'queued'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(jobs, 2);
    // The same key and body replays; the same key with another body conflicts.
    let again = e
        .req(
            "POST",
            "/v1/docs/files:retry",
            &[("Idempotency-Key", &key)],
            Some(&body),
        )
        .await;
    assert_eq!((again.status, again.json()), (200, res));
    let other = e
        .req(
            "POST",
            "/v1/docs/files:retry",
            &[("Idempotency-Key", &key)],
            Some(&json!({"file_ids": [a]})),
        )
        .await;
    assert_eq!(other.status, 409);

    // Limits and shape.
    let ids500: Vec<String> = (0..500)
        .map(|i| format!("00000000-0000-4000-8000-{i:012x}"))
        .collect();
    let r = e
        .post("/v1/docs/files:retry", &json!({"file_ids": ids500}))
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["results"].as_array().unwrap().len(), 500);
    let ids501: Vec<String> = (0..501)
        .map(|i| format!("00000000-0000-4000-8000-{i:012x}"))
        .collect();
    let r = e
        .post("/v1/docs/files:retry", &json!({"file_ids": ids501}))
        .await;
    assert_eq!(r.status, 413);
    assert_eq!(r.json()["code"], "DOCS_ENGINE_PAYLOAD_TOO_LARGE");
    assert_eq!(r.json()["details"]["max"], 500);
    for bad in [
        json!({"file_ids": []}),
        json!({"file_ids": ["nope"]}),
        json!({"file_ids": [a], "mode": "fast"}),
        json!({"file_ids": [a], "extra": 1}),
        json!({}),
        json!([]),
    ] {
        let r = e.post("/v1/docs/files:retry", &bad).await;
        assert_eq!(r.status, 400, "{bad}");
        assert_eq!(r.json()["code"], "DOCS_ENGINE_BAD_REQUEST");
    }
    let r = e
        .req(
            "POST",
            "/v1/docs/files:retry",
            &[],
            Some(&json!({"file_ids": [a]})),
        )
        .await;
    assert_eq!(r.json()["code"], "DOCS_ENGINE_IDEMPOTENCY_KEY_REQUIRED");
    assert_eq!(e.get("/v1/docs/files:retry").await.status, 405);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn file_progress_is_transient_rate_limited_and_filtered() {
    let e = TestEngine::start("fapi_progress", stages()).await;
    let bearer = format!("Bearer {TOKEN}");
    let auth = [("Authorization", bearer.as_str())];
    let mut all = open_sse(e.addr, "/v1/docs/events", &auth).await;
    let mut only_files = open_sse(e.addr, "/v1/docs/events?types=file.state_changed", &auth).await;
    let _ = all.frame(Duration::from_secs(2)).await; // retry:
    let _ = only_files.frame(Duration::from_secs(2)).await;

    let fid = "0f8fad5b-d9cb-469f-a165-70867728950e";
    let p = |done| Progress {
        file_id: fid.into(),
        drive_id: None,
        unit: "page",
        done,
        total: Some(5),
    };
    assert!(e.app.progress.emit(p(1)));
    assert!(!e.app.progress.emit(p(2)), "once a second per file");
    let frame = all.frame(Duration::from_secs(5)).await.expect("a frame");
    assert!(frame.starts_with("event: file.progress\ndata: "), "{frame}");
    assert!(
        !frame.contains("id:"),
        "transient events carry no id: {frame}"
    );
    let data: Value = serde_json::from_str(
        frame
            .lines()
            .find_map(|l| l.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(data["type"], "file.progress");
    assert!(data.get("seq").is_none());
    assert_eq!(
        data["data"],
        json!({"file_id": fid, "unit": "page", "done": 1, "total": 5})
    );
    // Another stream that asked for other types sees nothing of it.
    assert!(only_files.frame(Duration::from_millis(600)).await.is_none());
    // Not stored: a replay from the start has no progress.
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.event WHERE type = 'file.progress'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(n, 0);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn parsing_a_file_reports_progress_and_clears_it() {
    let e = TestEngine::start("fapi_progress2", stages()).await;
    let bearer = format!("Bearer {TOKEN}");
    let auth = [("Authorization", bearer.as_str())];
    let mut s = open_sse(e.addr, "/v1/docs/events?types=file.progress", &auth).await;
    let _ = s.frame(Duration::from_secs(2)).await;
    let mut text = String::new();
    for i in 0..40 {
        text.push_str(&format!("第{i}行\n"));
    }
    e.write("long.md", &text);
    e.create_drive("fapi-progress2").await;
    let ev = s
        .event(Duration::from_secs(15))
        .await
        .expect("a progress event");
    assert_eq!(ev["type"], "file.progress");
    assert_eq!(ev["data"]["unit"], "line");
    assert_eq!(ev["data"]["done"], 0);
    assert_eq!(ev["data"]["total"], 41);
    e.until("published", || async { published(&e, 1).await })
        .await;
    // The progress column is cleared when the file leaves parsing.
    let p: Option<String> =
        sqlx::query_scalar("SELECT progress::text FROM _oo_docs.file WHERE path = 'long.md'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert!(p.is_none());
    e.finish().await;
}
