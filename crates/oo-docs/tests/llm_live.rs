//! The LLM supplement on a live PostgreSQL with recorded replies only (B-S5-03):
//! mapping, drafts with `origin = llm`, the block level cache, schema retry,
//! the per file and per Drive caps (the rules' output is untouched), and the
//! usage API. No test calls a model.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test llm_live -- --ignored --test-threads=1`

mod common;

use std::path::PathBuf;

use common::CLUSTER_LOCK;
use common::engine::{Opts, TestEngine};
use common::fx::{DRIVE, Fx};
use oo_core::docir::DocIr;
use oo_docs::extract::llm::batch::batches;
use oo_docs::extract::llm::egress::{ChatReply, LlmError, ReplayPort};
use oo_docs::extract::llm::prompt::{cache_key, request};
use oo_docs::extract::llm::run::{Limits, LlmInput, extract_llm};
use oo_docs::publish::run::PublishOutcome;
use oo_docs::publish::write::{DocumentInput, index_extraction};
use serde_json::{Value, json};

const MODEL: &str = "recorded-model-1";
const PATH: &str = "Projects/星图/概述.md";

fn fixtures() -> (String, Value) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/llm");
    (
        std::fs::read_to_string(dir.join("fixture.md")).unwrap(),
        serde_json::from_str(&std::fs::read_to_string(dir.join("replies.json")).unwrap()).unwrap(),
    )
}

/// Replace `{{BLOCK:<text>}}` by block ids.
fn fill(template: &Value, ir: &DocIr) -> String {
    let mut s = template.to_string();
    while let Some(i) = s.find("{{BLOCK:") {
        let end = s[i..].find("}}").unwrap() + i;
        let want = s[i + 8..end].to_owned();
        let id = ir
            .blocks
            .iter()
            .find(|b| b.text.starts_with(&want))
            .unwrap()
            .id
            .clone();
        s.replace_range(i..end + 2, &id);
    }
    s
}

fn reply(content: String) -> Result<ChatReply, LlmError> {
    Ok(ChatReply {
        content,
        tokens_in: 100,
        tokens_out: 40,
    })
}

struct Rig {
    fx: Fx,
    ir: DocIr,
    version_id: String,
    b3: String,
    port: ReplayPort,
    reqs: Vec<oo_docs::extract::llm::prompt::ChatRequest>,
    replies: Value,
}

async fn rig(db: &str) -> Rig {
    let (md, replies) = fixtures();
    let mut fx = Fx::new(db).await;
    let p = fx.plant_md(PATH, &md).await;
    fx.index_planted(PATH, &p).await;
    let port = ReplayPort::new(MODEL);
    let bs = batches(&p.ir);
    assert_eq!(bs.len(), 2, "the fixture has two sections");
    let reqs: Vec<_> = bs
        .iter()
        .map(|b| request(MODEL, "星图项目", "md", &[], &[], b))
        .collect();
    // The prompt is pinned: the keys of the two batches.
    let keys: Vec<String> = bs.iter().map(|b| cache_key(MODEL, &[], &[], b)).collect();
    if replies["pinned_keys"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        assert_eq!(
            json!(keys),
            replies["pinned_keys"],
            "prompt, batching or key serialisation changed"
        );
    } else if std::env::var("OO_DOCS_PRINT_KEYS").is_ok() {
        eprintln!("pinned_keys = {}", json!(keys));
    }
    port.record(
        &reqs[0].user,
        vec![reply(fill(&replies["division"], &p.ir))],
    );
    port.record(
        &reqs[1].user,
        vec![reply(fill(&replies["decision"], &p.ir))],
    );
    Rig {
        fx,
        ir: p.ir,
        version_id: p.version_id,
        b3: p.b3,
        port,
        reqs,
        replies,
    }
}

impl Rig {
    async fn run(
        &mut self,
        limits: Limits,
        used: u32,
    ) -> (
        oo_docs::extract::model::Extraction,
        oo_docs::extract::llm::run::RunStats,
    ) {
        let input = LlmInput {
            types: Vec::new(),
            drive_id: DRIVE,
            path: PATH,
            title: "星图项目",
            ir: &self.ir,
            known: vec![],
            calls_used: used,
        };
        extract_llm(&mut self.fx.rt, &self.fx.cas, &self.port, limits, &input)
            .await
            .unwrap()
    }

    async fn store(&mut self, x: &oo_docs::extract::model::Extraction) {
        let doc = DocumentInput {
            version_id: &self.version_id,
            b3: &self.b3,
            path: PATH,
            ir: &self.ir,
        };
        index_extraction(&mut self.fx.rt, &doc, x).await.unwrap();
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn recorded_replies_become_llm_drafts_and_the_cache_pays_once() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_llm_replay";
    let mut r = rig(db).await;
    let (x, stats) = r.run(Limits::default(), 0).await;
    assert_eq!((stats.batches, stats.calls, stats.cache_hits), (2, 2, 0));
    assert_eq!(
        stats.dropped.quote_mismatch, 1,
        "the invented quote is dropped: {stats:?}"
    );
    assert_eq!(stats.failed_batches, 0);
    assert_eq!(x.facts.len(), 3, "two owns and one supersedes");
    assert!(
        x.facts
            .iter()
            .all(|f| f.rule_id == "llm" && !f.evidence.is_empty())
    );
    let llm = x.llm.clone().unwrap();
    assert_eq!(llm.prompt_version, "extract/1");
    assert_eq!(llm.extraction_key.len(), 64);
    r.store(&x).await;
    // Drafts with origin llm, confidence by word, provenance columns set.
    assert_eq!(
        r.fx.text("SELECT predicate || '/' || confidence_bp || '/' || prompt_version FROM _oo_docs.fact WHERE origin = 'llm' ORDER BY predicate, confidence_bp").await,
        ["owns/7000/extract/1", "owns/8500/extract/1", "supersedes/8500/extract/1"]
    );
    assert!(
        r.fx.scalar("SELECT count(*) FROM _oo_docs.evidence WHERE origin = 'llm'")
            .await
            > 0
    );
    assert_eq!(
        r.fx.text("SELECT alias || '/' || status || '/' || confidence_bp FROM _oo_docs.alias WHERE origin = 'llm'").await,
        ["小陈/draft/5000"]
    );
    // Usage and cache rows.
    let (calls, completed, ti, to): (i32, i32, i64, i64) =
        sqlx::query_as("SELECT reserved, completed, tokens_in, tokens_out FROM _oo_docs.llm_usage")
            .fetch_one(&mut r.fx.admin)
            .await
            .unwrap();
    assert_eq!((calls, completed, ti, to), (2, 2, 200, 80));
    assert_eq!(
        r.fx.scalar("SELECT count(*) FROM _oo_docs.llm_cache WHERE outcome = 'ok'")
            .await,
        2
    );

    // The second run is answered from the cache: no call, nothing new.
    let before = r.port.calls().len();
    let (x2, stats2) = r.run(Limits::default(), 0).await;
    assert_eq!((stats2.calls, stats2.cache_hits), (0, 2));
    assert_eq!(r.port.calls().len(), before, "no model call");
    assert_eq!(x2.facts.len(), x.facts.len());
    r.store(&x2).await;
    assert_eq!(
        r.fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE origin = 'llm'")
            .await,
        3,
        "stored once"
    );
    assert_eq!(
        r.fx.scalar("SELECT cache_hits::bigint FROM _oo_docs.llm_usage")
            .await,
        2
    );

    // Publishing: the 8500 owns is automatic, the 7000 one waits, supersedes is never automatic.
    let PublishOutcome::Published(_) = r.fx.publish().await else {
        panic!("v1")
    };
    assert_eq!(r.fx.text("SELECT predicate || '/' || status FROM _oo_docs.fact WHERE origin = 'llm' ORDER BY predicate, confidence_bp").await,
        ["owns/draft", "owns/draft", "supersedes/draft"]);
    assert_eq!(
        r.fx.text("SELECT reason_code FROM _oo_docs.review_item WHERE kind = 'low_confidence_fact' ORDER BY reason_code").await,
        ["low_confidence", "low_confidence", "supersede_candidate"]
    );
    r.fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_broken_reply_is_retried_once_and_then_remembered() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_llm_schema";
    let mut r = rig(db).await;
    let good = fill(&r.replies["division"], &r.ir);
    // Batch 1: broken, then good (the retry). Batch 2: broken twice.
    r.port.record(
        &r.reqs[0].user,
        vec![reply("这不是 JSON".into()), reply(good)],
    );
    r.port.record(&r.reqs[1].user, vec![reply("[]".into())]);
    let (x, stats) = r.run(Limits::default(), 0).await;
    assert_eq!(
        (stats.calls, stats.schema_invalid, stats.failed_batches),
        (4, 3, 1),
        "{stats:?}"
    );
    assert_eq!(x.facts.len(), 2, "batch 1 came through on the retry");
    assert_eq!(
        r.fx.scalar("SELECT count(*) FROM _oo_docs.llm_cache WHERE outcome = 'ok'")
            .await,
        1
    );
    assert_eq!(
        r.fx.scalar("SELECT count(*) FROM _oo_docs.llm_cache WHERE outcome = 'schema_invalid'")
            .await,
        1
    );
    // A failed batch is not paid again, a good one is cached.
    let calls = r.port.calls().len();
    let (_, again) = r.run(Limits::default(), 0).await;
    assert_eq!(
        (again.calls, again.cache_hits, again.failed_batches),
        (0, 2, 1)
    );
    assert_eq!(r.port.calls().len(), calls);
    // An unreachable model is a failed batch without a cache entry.
    let r2 = &mut r;
    r2.port.record(
        &r2.reqs[0].user,
        vec![Err(LlmError::Unavailable("down".into()))],
    );
    sqlx::query("DELETE FROM _oo_docs.llm_cache")
        .execute(&mut r2.fx.admin)
        .await
        .unwrap();
    r2.port.record(
        &r2.reqs[1].user,
        vec![Err(LlmError::Unavailable("down".into()))],
    );
    let (x, down) = r2.run(Limits::default(), 0).await;
    assert_eq!((down.failed_batches, x.facts.len()), (2, 0));
    assert_eq!(
        r2.fx
            .scalar("SELECT count(*) FROM _oo_docs.llm_cache")
            .await,
        0
    );
    assert_eq!(
        r2.fx
            .scalar("SELECT failed::bigint FROM _oo_docs.llm_usage")
            .await,
        2
    );
    r.fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn caps_stop_the_model_but_never_the_rules() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_llm_caps";
    let mut r = rig(db).await;
    // Per file: one call, so the second batch is not asked.
    let (x, s) = r
        .run(
            Limits {
                daily: 2000,
                per_file: 1,
            },
            0,
        )
        .await;
    assert_eq!((s.calls, s.file_limited, s.drive_limited), (1, true, false));
    assert_eq!(x.facts.len(), 2, "the first batch stands");
    assert_eq!(
        r.fx.scalar("SELECT files_limited::bigint FROM _oo_docs.llm_usage")
            .await,
        1
    );
    // Calls used earlier count (the checkpoint).
    let (_, s) = r
        .run(
            Limits {
                daily: 2000,
                per_file: 1,
            },
            1,
        )
        .await;
    assert_eq!(
        s.calls, 0,
        "the first batch is cached, the second is over the cap: {s:?}"
    );
    assert!(s.file_limited);

    // Per Drive per day: the reservation is refused and does not overshoot.
    sqlx::query("DELETE FROM _oo_docs.llm_cache")
        .execute(&mut r.fx.admin)
        .await
        .unwrap();
    sqlx::query("UPDATE _oo_docs.llm_usage SET reserved = 0, files_limited = 0")
        .execute(&mut r.fx.admin)
        .await
        .unwrap();
    let (x, s) = r
        .run(
            Limits {
                daily: 1,
                per_file: 20,
            },
            0,
        )
        .await;
    assert_eq!((s.calls, s.drive_limited), (1, true), "{s:?}");
    assert_eq!(x.facts.len(), 2);
    assert_eq!(
        r.fx.scalar("SELECT reserved::bigint FROM _oo_docs.llm_usage")
            .await,
        1
    );
    // A rate limit asks to wait and stops the file.
    sqlx::query("DELETE FROM _oo_docs.llm_cache")
        .execute(&mut r.fx.admin)
        .await
        .unwrap();
    r.port
        .record(&r.reqs[0].user, vec![Err(LlmError::RateLimited(2500))]);
    let (_, s) = r.run(Limits::default(), 0).await;
    assert_eq!(s.rate_limited_ms, Some(2500));

    // With the model out of calls the rules still publish.
    let PublishOutcome::Published(v) = r.fx.publish().await else {
        panic!("rules publish")
    };
    assert!(v.facts_added > 0);
    r.fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn usage_api_reports_calls_limits_and_cache() {
    let name = "api_usage";
    let e = TestEngine::start(name, Opts::default()).await;
    let mut fx = Fx::attach(name, &e.cas).await;
    let empty = e.get("/v1/docs/usage").await;
    assert_eq!(empty.status, 200);
    assert_eq!(empty.json()["model"]["status"], "not_configured");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE _oo_docs.drive SET model_enabled = true, llm_daily_calls = 3 WHERE id = '{DRIVE}'"
    )))
    .execute(&mut fx.admin)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO _oo_docs.llm_usage (drive_id, day, reserved, completed, cache_hits, tokens_in, tokens_out, files_limited) VALUES ('{DRIVE}', (now() AT TIME ZONE 'UTC')::date, 3, 3, 2, 300, 120, 1)")))
        .execute(&mut fx.admin).await.unwrap();
    let u = e
        .get(&format!("/v1/docs/usage?drive_id={DRIVE}&days=7"))
        .await
        .json();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("contracts/docs-api/docs-api.v1.schema.json")).unwrap(),
    )
    .unwrap();
    let wrapper = json!({"$schema": "https://json-schema.org/draft/2020-12/schema", "$defs": doc["$defs"].clone(), "$ref": "#/$defs/Usage"});
    let v = oo_core::schema_util::compile_schema(&wrapper).unwrap();
    assert!(v.is_valid(&u), "{u}");
    assert_eq!(u["model"]["status"], "limited", "today's limit is used up");
    let row = &u["drives"][0];
    assert_eq!(
        (
            row["calls"].as_i64(),
            row["limit"].as_i64(),
            row["cache_hits"].as_i64()
        ),
        (Some(3), Some(3), Some(2))
    );
    assert_eq!(
        (
            row["tokens_in"].as_i64(),
            row["tokens_out"].as_i64(),
            row["files_limited"].as_i64()
        ),
        (Some(300), Some(120), Some(1))
    );
    assert_eq!(u["per_file_limit"], 20);
    assert_eq!(e.get("/v1/docs/usage?days=0").await.status, 400);
    assert_eq!(e.get("/v1/docs/usage?drive_id=x").await.status, 400);
    drop(fx);
    e.finish().await;
}
