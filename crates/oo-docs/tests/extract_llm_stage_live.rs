//! The `extract_llm` stage and `GET/PUT /v1/docs/config/model` on a live
//! PostgreSQL with recorded replies only (no network, no real model): a Drive
//! with the model enabled gets the model's facts after the rules, a failed
//! batch leaves `extract_partial` that still publishes, a Drive without the
//! model (or an engine without one) never calls it, and the configuration
//! route takes references and never a key value.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test extract_llm_stage_live -- --ignored --test-threads=1`

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::engine::{Opts, TestEngine};
use oo_docs::extract::llm::config::ModelRuntime;
use oo_docs::extract::llm::egress::{CallFuture, ChatReply, LlmError, LlmPort};
use oo_docs::extract::llm::prompt::ChatRequest;
use serde_json::{Value, json};

const MODEL: &str = "recorded-model-1";

/// Answers by what the batch contains; `{{BLOCK:<text start>}}` in a recorded
/// reply becomes the id of the block of that request which starts so.
struct Scripted {
    rules: Vec<(&'static str, Option<String>)>,
    calls: AtomicUsize,
}

fn block_id(user: &str, start: &str) -> String {
    user.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["text"].as_str().is_some_and(|t| t.starts_with(start)))
        .and_then(|v| v["block_id"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn fill(template: &str, user: &str) -> String {
    let mut s = template.to_owned();
    while let Some(i) = s.find("{{BLOCK:") {
        let end = s[i..].find("}}").unwrap() + i;
        let id = block_id(user, &s[i + 8..end]);
        s.replace_range(i..end + 2, &id);
    }
    s
}

impl LlmPort for Scripted {
    fn model(&self) -> &str {
        MODEL
    }

    fn complete<'a>(&'a self, req: &'a ChatRequest) -> CallFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let out = self
            .rules
            .iter()
            .find(|(needle, _)| req.user.contains(needle))
            .and_then(|(_, reply)| reply.as_ref())
            .map(|t| ChatReply {
                content: fill(t, &req.user),
                tokens_in: 100,
                tokens_out: 40,
            })
            .ok_or_else(|| LlmError::Unavailable("no recorded reply".into()));
        Box::pin(async move { out })
    }
}

fn recorded() -> (String, Value) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/llm");
    (
        std::fs::read_to_string(dir.join("fixture.md")).unwrap(),
        serde_json::from_str(&std::fs::read_to_string(dir.join("replies.json")).unwrap()).unwrap(),
    )
}

fn engine_opts(port: Arc<Scripted>) -> Opts {
    Opts {
        workers: true,
        stages: true,
        model: Some(ModelRuntime::fixed(port)),
        ..Opts::default()
    }
}

async fn enable_model(e: &TestEngine, drive: &Value) {
    let rev = drive["revision"].as_i64().unwrap();
    let r = e
        .req(
            "PATCH",
            &format!("/v1/docs/drives/{}", drive["id"].as_str().unwrap()),
            &[("If-Match", &format!("\"r{rev}\""))],
            Some(&json!({"model_enabled": true})),
        )
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
}

async fn state_of(e: &TestEngine, path: &str) -> String {
    sqlx::query_scalar("SELECT state FROM _oo_docs.file WHERE path = $1")
        .bind(path)
        .fetch_optional(&e.pool)
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn history(e: &TestEngine, path: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT to_state FROM _oo_docs.file_state_history WHERE file_id = \
         (SELECT id FROM _oo_docs.file WHERE path = $1) ORDER BY seq",
    )
    .bind(path)
    .fetch_all(&e.pool)
    .await
    .unwrap()
}

async fn count(e: &TestEngine, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_model_drive_gets_the_models_facts_after_the_rules_and_publishes() {
    let (md, replies) = recorded();
    let port = Arc::new(Scripted {
        rules: vec![
            ("陈子墨负责", Some(replies["division"].to_string())),
            ("2026-03-01", Some(replies["decision"].to_string())),
        ],
        calls: AtomicUsize::new(0),
    });
    let e = TestEngine::start("stage_llm_on", engine_opts(port.clone())).await;
    e.write("Projects/星图项目.md", &md);
    let drive = e.create_drive("llm-on").await;
    enable_model(&e, &drive).await;
    e.until("published", || async {
        state_of(&e, "Projects/星图项目.md").await == "published"
    })
    .await;
    assert_eq!(
        history(&e, "Projects/星图项目.md").await,
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
    assert_eq!(port.calls.load(Ordering::SeqCst), 2, "one call per section");
    assert!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'llm'"
        )
        .await
            >= 3,
        "two owns and the supersedes"
    );
    assert!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'rule'"
        )
        .await
            > 0,
        "the rules ran first and kept their facts"
    );
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.job WHERE kind = 'extract_llm' AND state = 'succeeded'"
        )
        .await,
        1
    );
    // D51: a supersession that only the model proposed is never applied by
    // itself: the fact stays a draft, nothing is ended, a person is asked.
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'llm' AND predicate = 'supersedes' AND status = 'draft'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.fact WHERE valid_to IS NOT NULL AND superseded_by IS NOT NULL"
        )
        .await,
        0
    );
    assert!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.review_item WHERE status = 'pending'"
        )
        .await
            >= 1,
        "the proposal waits in the review queue"
    );
    // Usage and health name the model.
    let u = e.get("/v1/docs/usage").await.json();
    assert_eq!(u["model"]["status"], "ok");
    let h = e.get("/v1/docs/health").await.json();
    assert_eq!(h["components"]["model"]["status"], "ok");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_failed_batch_is_extract_partial_and_the_other_batches_stand_and_publish() {
    let (md, replies) = recorded();
    let port = Arc::new(Scripted {
        rules: vec![
            ("陈子墨负责", Some(replies["division"].to_string())),
            ("2026-03-01", None), // the model is down for this section
        ],
        calls: AtomicUsize::new(0),
    });
    let e = TestEngine::start("stage_llm_partial", engine_opts(port.clone())).await;
    e.write("Projects/星图项目.md", &md);
    let drive = e.create_drive("llm-partial").await;
    enable_model(&e, &drive).await;
    e.until("published", || async {
        state_of(&e, "Projects/星图项目.md").await == "published"
    })
    .await;
    let h = history(&e, "Projects/星图项目.md").await;
    assert_eq!(
        &h[h.len() - 3..],
        ["extracting", "extract_partial", "published"],
        "{h:?}"
    );
    let code: Option<String> = sqlx::query_scalar(
        "SELECT code FROM _oo_docs.file_state_history WHERE to_state = 'extract_partial' LIMIT 1",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(code.as_deref(), Some("DOCS_EXTRACT_PARTIAL"));
    assert!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'llm'"
        )
        .await
            >= 2,
        "the section that worked stands"
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn without_the_drive_switch_or_without_a_model_the_model_is_never_called() {
    let (md, replies) = recorded();
    let port = Arc::new(Scripted {
        rules: vec![("陈子墨负责", Some(replies["division"].to_string()))],
        calls: AtomicUsize::new(0),
    });
    // A model exists but the Drive did not switch it on.
    let e = TestEngine::start("stage_llm_off_drive", engine_opts(port.clone())).await;
    e.write("a.md", &md);
    let _ = e.create_drive("llm-off").await;
    e.until("published", || async {
        state_of(&e, "a.md").await == "published"
    })
    .await;
    assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'llm'"
        )
        .await,
        0
    );
    e.finish().await;

    // No model at all: rules only, health says so.
    let e = TestEngine::start(
        "stage_llm_none",
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    e.write("a.md", &md);
    let d = e.create_drive("llm-none").await;
    enable_model(&e, &d).await;
    e.until("published", || async {
        state_of(&e, "a.md").await == "published"
    })
    .await;
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.job WHERE kind = 'extract_llm'"
        )
        .await,
        0
    );
    assert_eq!(
        e.get("/v1/docs/health").await.json()["components"]["model"]["status"],
        "not_configured"
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_model_configuration_takes_references_and_never_a_key() {
    let dir = std::env::temp_dir().join(format!("oo-model-api-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let model = ModelRuntime::open(dir.clone(), None, None);
    let e = TestEngine::start(
        "stage_llm_config",
        Opts {
            model: Some(model),
            ..Opts::default()
        },
    )
    .await;
    let g = e.get("/v1/docs/config/model").await;
    assert_eq!(g.status, 200);
    assert_eq!(g.json()["configured"], false);
    assert_eq!(g.header("etag"), Some("\"r1\""));
    let good = json!({"provider": "openai-compatible", "base_url": "file:/tmp/oo-endpoint-ref",
                      "model": "gpt-recorded", "key_ref": "file:/tmp/oo-key-ref"});
    // If-Match is required.
    let r = e
        .req("PUT", "/v1/docs/config/model", &[], Some(&good))
        .await;
    assert_eq!(r.status, 428);
    // A key value in key_ref is a 400 that does not echo it.
    let secret = "sk-live-this-must-never-appear-1234567890";
    let mut bad = good.clone();
    bad["key_ref"] = json!(secret);
    let r = e
        .req(
            "PUT",
            "/v1/docs/config/model",
            &[("If-Match", "\"r1\"")],
            Some(&bad),
        )
        .await;
    assert_eq!(r.status, 400);
    assert!(!String::from_utf8_lossy(&r.body).contains(secret));
    // A stale revision is refused.
    let r = e
        .req(
            "PUT",
            "/v1/docs/config/model",
            &[("If-Match", "\"r7\"")],
            Some(&good),
        )
        .await;
    assert_eq!(r.status, 412);
    // Accepted: references echoed, no secret, a new revision, no egress script here.
    let r = e
        .req(
            "PUT",
            "/v1/docs/config/model",
            &[("If-Match", "\"r1\"")],
            Some(&good),
        )
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let v = r.json();
    assert_eq!(v["configured"], true);
    assert_eq!(v["key_ref"], "file:/tmp/oo-key-ref");
    assert_eq!(v["revision"], 2);
    assert_eq!(v["status"], "unavailable");
    assert_eq!(r.header("etag"), Some("\"r2\""));
    assert_eq!(e.get("/v1/docs/config/model").await.json()["revision"], 2);
    assert_eq!(
        e.get("/v1/docs/health").await.json()["components"]["model"]["status"],
        "unavailable"
    );
    // Nothing on disk holds a secret: references only, owner-only file.
    let saved = std::fs::read_to_string(dir.join("model.json")).unwrap();
    assert!(saved.contains("file:/tmp/oo-key-ref") && !saved.contains(secret));
    e.finish().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_file_whose_only_facts_wait_for_a_person_is_still_published() {
    // The rules find nothing in this file; the model's two facts are held for
    // review (a new entity pair lands under the threshold). The file must not
    // stay `indexed` for ever: pending_review counts them (10 4.2).
    let reply = json!({
        "entities": [
            {"id": "e1", "type": "Person", "name": "韩立", "block_id": "{{BLOCK:韩立}}", "quote": "韩立负责灯塔", "confidence": "medium"},
            {"id": "e2", "type": "Project", "name": "灯塔", "block_id": "{{BLOCK:韩立}}", "quote": "灯塔", "confidence": "medium"}],
        "relations": [
            {"subject": "e1", "predicate": "owns", "object": "e2", "block_id": "{{BLOCK:韩立}}", "quote": "韩立负责灯塔", "confidence": "medium"}]
    });
    let port = Arc::new(Scripted {
        rules: vec![("韩立", Some(reply.to_string()))],
        calls: AtomicUsize::new(0),
    });
    let e = TestEngine::start("stage_llm_held", engine_opts(port.clone())).await;
    e.write("note.txt", "韩立负责灯塔。\n");
    let drive = e.create_drive("llm-held").await;
    enable_model(&e, &drive).await;
    e.until("published", || async {
        state_of(&e, "note.txt").await == "published"
    })
    .await;
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'rule'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.fact WHERE origin = 'llm' AND status = 'draft'"
        )
        .await,
        1
    );
    assert!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.review_item WHERE status = 'pending'"
        )
        .await
            >= 1,
        "the held fact waits for a person"
    );
    e.finish().await;
}
