//! Failure paths of the file state machine with injected faults (B-S3-04,
//! 10 §4.1.4, §4.1.6, §4.1.8, §12.5). Compiled only with the `failpoints`
//! feature.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --features failpoints --test faults -- --ignored --test-threads=1`
//!
//! The fault points are process wide; every test uses its own point name and
//! blob prefixes, and `--test-threads=1` keeps the runs apart anyway.
#![cfg(feature = "failpoints")]

mod common;

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::engine::{Opts, TOKEN, TestEngine};
use common::{installed_db, pool_admin, pool_runtime};
use oo_docs::failpoints;
use oo_docs::jobs::stages::{self, RetryResult};
use serde_json::{Value, json};

fn opts() -> Opts {
    Opts {
        workers: true,
        stages: true,
        ..Opts::default()
    }
}

fn doc(name: &str) -> String {
    format!("---\ntype: project\nname: {name}\nowner: 林川\n---\n# {name}\n\n负责人：林川\n")
}

fn prefix(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex()[..16].to_owned()
}

async fn file_state(e: &TestEngine, path: &str) -> (String, Option<String>, bool) {
    sqlx::query_as(
        "SELECT state, state_code, retryable FROM _oo_docs.file WHERE path = $1 AND deleted_at IS NULL",
    )
    .bind(path)
    .fetch_optional(&e.pool)
    .await
    .unwrap()
    .unwrap_or_default()
}

async fn history(e: &TestEngine, path: &str) -> Vec<(String, Option<String>, bool)> {
    sqlx::query_as(
        "SELECT h.to_state, h.code, COALESCE((h.detail->>'manual')::boolean, false) \
           FROM _oo_docs.file_state_history h JOIN _oo_docs.file f ON f.id = h.file_id \
          WHERE f.path = $1 ORDER BY h.seq",
    )
    .bind(path)
    .fetch_all(&e.pool)
    .await
    .unwrap()
}

async fn file_id(e: &TestEngine, path: &str) -> String {
    sqlx::query_scalar("SELECT id::text FROM _oo_docs.file WHERE path = $1 AND deleted_at IS NULL")
        .bind(path)
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_retryable_parse_error_backs_off_three_times_then_waits_for_a_manual_retry() {
    let content = doc("重试项目");
    failpoints::set("parse_error", &prefix(&content));
    failpoints::set("backoff_ms", "1200");
    let e = TestEngine::start("fault_retry", opts()).await;
    e.write("a/retry.md", &content);
    e.write("a/other.md", &doc("别的项目"));
    e.create_drive("fault-retry").await;

    e.until("the file gave up", || async {
        file_state(&e, "a/retry.md").await.0 == "parse_failed"
    })
    .await;
    // Three automatic attempts, each waiting in `queued` with the reason.
    let h = history(&e, "a/retry.md").await;
    let tos: Vec<&str> = h.iter().map(|(s, _, _)| s.as_str()).collect();
    assert_eq!(
        tos,
        [
            "stored",
            "queued",
            "parsing",
            "queued",
            "parsing",
            "queued",
            "parsing",
            "parse_failed"
        ],
        "{h:?}"
    );
    for (state, code, _) in &h[3..] {
        if state == "queued" || state == "parse_failed" {
            assert_eq!(code.as_deref(), Some("DOCS_PARSE_TIMEOUT"));
        }
    }
    let (state, code, retryable) = file_state(&e, "a/retry.md").await;
    assert_eq!(
        (state.as_str(), code.as_deref(), retryable),
        ("parse_failed", Some("DOCS_PARSE_TIMEOUT"), true)
    );
    let (attempts, failed): (i32, String) = sqlx::query_as(
        "SELECT attempt, state FROM _oo_docs.job WHERE kind = 'parse' AND file_id = $1::uuid",
    )
    .bind(file_id(&e, "a/retry.md").await)
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!((attempts, failed.as_str()), (3, "failed"));
    // The other file was not held up.
    assert_eq!(file_state(&e, "a/other.md").await.0, "published");

    // Nothing retries it by itself any more.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(file_state(&e, "a/retry.md").await.0, "parse_failed");

    // Manual retry: only for the failure states.
    let fid = file_id(&e, "a/retry.md").await;
    let other = file_id(&e, "a/other.md").await;
    assert_eq!(
        stages::retry_file(&e.pool, &other, false).await.unwrap(),
        RetryResult::NotRetryable("published".into())
    );
    assert_eq!(
        stages::retry_file(&e.pool, "00000000-0000-4000-8000-000000000000", false)
            .await
            .unwrap(),
        RetryResult::NotFound
    );
    failpoints::clear("parse_error");
    assert_eq!(
        stages::retry_file(&e.pool, &fid, false).await.unwrap(),
        RetryResult::Queued
    );
    e.until("manual retry published", || async {
        file_state(&e, "a/retry.md").await.0 == "published"
    })
    .await;
    let h = history(&e, "a/retry.md").await;
    let tail: Vec<&str> = h
        .iter()
        .rev()
        .take(7)
        .rev()
        .map(|(s, _, _)| s.as_str())
        .collect();
    assert_eq!(
        tail,
        [
            "parse_failed",
            "queued",
            "parsing",
            "parsed",
            "extracting",
            "indexed",
            "published"
        ]
    );
    assert!(
        h.iter().any(|(s, _, manual)| s == "queued" && *manual),
        "the retry is marked manual"
    );
    let prio: i16 = sqlx::query_scalar(
        "SELECT priority FROM _oo_docs.job WHERE kind = 'parse' AND file_id = $1::uuid \
          AND dedupe_key LIKE '%#m%'",
    )
    .bind(&fid)
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(prio, 50, "manual retries run before new files");
    failpoints::clear("backoff_ms");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_poison_pill_is_quarantined_after_two_crashes_and_the_rest_flows() {
    let poison = doc("毒丸");
    failpoints::set("parse_crash", &prefix(&poison));
    failpoints::set("backoff_ms", "300");
    let e = TestEngine::start("fault_poison", opts()).await;
    e.write("p/poison.md", &poison);
    for i in 0..20 {
        e.write(&format!("ok/f{i:02}.md"), &doc(&format!("正常{i}")));
    }
    e.create_drive("fault-poison").await;

    e.until("poison quarantined", || async {
        file_state(&e, "p/poison.md").await.0 == "parse_failed"
    })
    .await;
    let (state, code, retryable) = file_state(&e, "p/poison.md").await;
    assert_eq!(
        (state.as_str(), code.as_deref(), retryable),
        ("parse_failed", Some("DOCS_PARSE_QUARANTINED"), true)
    );
    let (jstate, crashes, attempts): (String, i32, i32) = sqlx::query_as(
        "SELECT state, crash_count, attempt FROM _oo_docs.job WHERE kind = 'parse' AND file_id = $1::uuid",
    )
    .bind(file_id(&e, "p/poison.md").await)
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!((jstate.as_str(), crashes), ("quarantined", 2));
    assert!(
        attempts <= 2,
        "quarantined on the second crash, not the third attempt"
    );
    // The other twenty were not slowed into failing: every one published.
    e.until("the rest published", || async {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.file WHERE path LIKE 'ok/%' AND state = 'published'",
        )
        .fetch_one(&e.pool)
        .await
        .unwrap();
        n == 20
    })
    .await;

    // Quarantine is per blob: the same bytes at another path inherit it
    // without another parse.
    e.write("p/copy.md", &poison);
    let drive_id: String = sqlx::query_scalar("SELECT id::text FROM _oo_docs.drive LIMIT 1")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    let r = e
        .post(&format!("/v1/docs/drives/{drive_id}:sync"), &json!({}))
        .await;
    assert_eq!(r.status, 202);
    e.until("the copy inherits the quarantine", || async {
        file_state(&e, "p/copy.md").await.0 == "parse_failed"
    })
    .await;
    let (_, code, retryable) = file_state(&e, "p/copy.md").await;
    assert_eq!(
        (code.as_deref(), retryable),
        (Some("DOCS_PARSE_QUARANTINED"), true)
    );
    let parses: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse' AND file_id = $1::uuid",
    )
    .bind(file_id(&e, "p/copy.md").await)
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(parses, 0, "no second crash");

    // Manual retry once the cause is gone brings both back.
    failpoints::clear("parse_crash");
    for path in ["p/poison.md", "p/copy.md"] {
        let id = file_id(&e, path).await;
        assert_eq!(
            stages::retry_file(&e.pool, &id, true).await.unwrap(),
            RetryResult::Queued
        );
    }
    e.until("both published", || async {
        file_state(&e, "p/poison.md").await.0 == "published"
            && file_state(&e, "p/copy.md").await.0 == "published"
    })
    .await;
    failpoints::clear("backoff_ms");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn an_extract_failure_is_extract_partial_and_a_manual_retry_finishes_it() {
    let content = doc("抽取失败");
    failpoints::set("extract_error", &prefix(&content));
    failpoints::set("backoff_ms", "300");
    let e = TestEngine::start("fault_extract", opts()).await;
    e.write("x/part.md", &content);
    e.create_drive("fault-extract").await;
    e.until("extract_partial", || async {
        file_state(&e, "x/part.md").await.0 == "extract_partial"
    })
    .await;
    let (_, code, retryable) = file_state(&e, "x/part.md").await;
    assert_eq!(
        (code.as_deref(), retryable),
        (Some("DOCS_EXTRACT_FAILPOINT"), true)
    );
    let id = file_id(&e, "x/part.md").await;
    failpoints::clear("extract_error");
    assert_eq!(
        stages::retry_file(&e.pool, &id, false).await.unwrap(),
        RetryResult::Queued
    );
    e.until("published after the retry", || async {
        file_state(&e, "x/part.md").await.0 == "published"
    })
    .await;
    // The parse was not repeated: same blob, finished parse result.
    let parses: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.parse_result WHERE status = 'complete'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(parses, 1);
    failpoints::clear("backoff_ms");
    e.finish().await;
}

// ------------------------------------------------------------- engine kill

struct Engine {
    child: Child,
    addr: String,
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_engine(db: &str, state: &std::path::Path, failpoints: &str) -> Engine {
    let port = std::env::var("OO_DOCS_TEST_PG_PORT").unwrap();
    let host = std::env::var("OO_DOCS_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let url = format!(
        "postgres://{}@{host}:{port}/{db}?sslmode=disable",
        common::RUNTIME_LOGIN
    );
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_oo-docs-engine"));
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("OO_DOCS_LISTEN", "127.0.0.1:0")
        .env("OO_DOCS_STATE_DIR", state)
        .env("OO_DOCS_DATABASE_URL", url)
        .env("OO_DOCS_ENGINE_TOKEN_FILE", state.join("engine.token"))
        .env("OO_DOCS_FAILPOINTS", failpoints)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().expect("spawn engine");
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .expect("first line");
    let v: Value =
        serde_json::from_str(line.trim()).unwrap_or_else(|_| panic!("first line: {line}"));
    Engine {
        child,
        addr: v["addr"].as_str().expect("addr").to_owned(),
    }
}

async fn call(addr: &str, method: &str, path: &str, body: Option<&Value>) -> common::http::Resp {
    let auth = format!("Bearer {TOKEN}");
    let key = common::engine::key();
    let mut headers = vec![("Authorization", auth.as_str())];
    if method == "POST" {
        headers.push(("Idempotency-Key", key.as_str()));
    }
    let text = body.map(Value::to_string);
    common::http::request(
        addr.parse().unwrap(),
        method,
        path,
        &headers,
        text.as_deref(),
    )
    .await
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_killed_engine_resumes_the_leased_job_after_its_lease_ends() {
    let db = "fault_kill";
    {
        let _g = common::CLUSTER_LOCK.lock().await;
        installed_db(db).await;
    }
    let state = std::env::temp_dir().join(format!("oo-docs-kill-{}", std::process::id()));
    let root = state.join("drive");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(state.join("engine.token"), TOKEN).unwrap();
    let slow = doc("被打断");
    std::fs::write(root.join("slow.md"), &slow).unwrap();
    for i in 0..5 {
        std::fs::write(root.join(format!("n{i}.md")), doc(&format!("普通{i}"))).unwrap();
    }
    // The first engine sleeps 60 s inside the parse of one blob.
    let first = spawn_engine(
        db,
        &state,
        &format!("parse_window_mid={}@60000", prefix(&slow)),
    );
    let r = call(
        &first.addr,
        "POST",
        "/v1/docs/drives",
        Some(&json!({"name": "kill", "backend": "fs", "config": {"root": root.to_str().unwrap()}})),
    )
    .await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));

    let pool = pool_runtime(db, 4).await;
    let state_of = |path: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, String>("SELECT state FROM _oo_docs.file WHERE path = $1")
                .bind(path)
                .fetch_optional(&pool)
                .await
                .unwrap()
                .unwrap_or_default()
        }
    };
    let mut seen = false;
    for _ in 0..200 {
        if state_of("slow.md").await == "parsing" {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(seen, "the slow file reached parsing");
    // The others flow past it.
    for _ in 0..200 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.file WHERE path LIKE 'n%' AND state = 'published'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if n == 5 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // kill -9 in the middle of the job.
    drop(first);
    let (jstate, attempt): (String, i32) = sqlx::query_as(
        "SELECT state, attempt FROM _oo_docs.job WHERE kind = 'parse' \
          AND file_id = (SELECT id FROM _oo_docs.file WHERE path = 'slow.md')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (jstate.as_str(), attempt),
        ("running", 1),
        "the lease is still held"
    );
    assert_eq!(state_of("slow.md").await, "parsing");
    // The lease would end after at most 30 s (4.1.1); do not wait for the
    // clock, end it, which is the same state the job is in then.
    let admin = pool_admin(db, 2).await;
    sqlx::query("UPDATE _oo_docs.job SET lease_until = clock_timestamp() - interval '1 second' WHERE state = 'running'")
        .execute(&admin)
        .await
        .unwrap();

    // A new engine without the fault resumes it.
    let second = spawn_engine(db, &state, "");
    let mut done = false;
    for _ in 0..300 {
        if state_of("slow.md").await == "published" {
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(done, "the interrupted file resumed and published");
    let (jstate, attempt, crashes): (String, i32, i32) = sqlx::query_as(
        "SELECT state, attempt, crash_count FROM _oo_docs.job WHERE kind = 'parse' \
          AND file_id = (SELECT id FROM _oo_docs.file WHERE path = 'slow.md')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (jstate.as_str(), attempt, crashes),
        ("succeeded", 2, 0),
        "a killed engine is infrastructure, not a crash of the parse"
    );
    let h: Vec<String> = sqlx::query_scalar(
        "SELECT to_state FROM _oo_docs.file_state_history WHERE file_id = \
         (SELECT id FROM _oo_docs.file WHERE path = 'slow.md') ORDER BY seq",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        h,
        [
            "stored",
            "queued",
            "parsing",
            "parsed",
            "extracting",
            "indexed",
            "published"
        ],
        "one pass through the chain, no half state left behind"
    );
    // No event was lost: sequence numbers are a gap free prefix.
    let (n, max): (i64, i64) = sqlx::query_as("SELECT count(*), max(seq) FROM _oo_docs.event")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, max);
    drop(second);
    pool.close().await;
    admin.close().await;
    let _ = std::fs::remove_dir_all(&state);
    common::drop_db(db).await;
}
