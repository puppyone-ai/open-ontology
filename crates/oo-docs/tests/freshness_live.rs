//! Drive freshness (00 5.5, D48, G12) walked through its states on a real
//! engine and database, with the matching `drive.freshness_changed` events and
//! the `Health.freshness` summary.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test freshness_live -- --ignored --test-threads=1`

mod common;

use std::time::Duration;

use common::engine::{Opts, TestEngine};
use oo_docs::jobs::{JobKind, NewJob};
use oo_docs::store::jobs as jobstore;
use serde_json::{Value, json};

async fn drive_state(e: &TestEngine, id: &str) -> Value {
    e.get(&format!("/v1/docs/drives/{id}")).await.json()["freshness"].clone()
}

async fn health_state(e: &TestEngine) -> String {
    e.get("/v1/docs/health").await.json()["freshness"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

async fn announced(e: &TestEngine) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT payload->'freshness'->>'state' FROM _oo_docs.event \
          WHERE type = 'drive.freshness_changed' ORDER BY seq",
    )
    .fetch_all(&e.pool)
    .await
    .unwrap()
}

/// Waits for the state, then gives the watcher (100 ms period) time to
/// observe it, so every step is announced and the order is deterministic.
async fn wait_state(e: &TestEngine, id: &str, want: &str) {
    e.until(&format!("freshness {want}"), || async {
        drive_state(e, id).await["state"] == want
    })
    .await;
    tokio::time::sleep(Duration::from_millis(350)).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn freshness_follows_the_priority_order_and_is_announced() {
    // Workers run hash and sync only: files stay `queued`, a publish job stays
    // queued, so every state can be held still and looked at.
    let e = TestEngine::start(
        "fresh_walk",
        Opts {
            workers: true,
            watcher: true,
            ..Opts::default()
        },
    )
    .await;
    e.write("a.md", "# a\n");
    let drive = e.create_drive("walk").await;
    let id = drive["id"].as_str().unwrap().to_owned();

    // processing: one file between stored and extracting.
    wait_state(&e, &id, "processing").await;
    let f = drive_state(&e, &id).await;
    assert_eq!(f["processing"], 1);
    assert_eq!(health_state(&e).await, "processing");
    // Let the watcher settle. Its first observation is a silent baseline, but
    // on a loaded machine that first look can land while the Drive is still
    // `syncing`, which then correctly announces `processing`. Count from here.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let before = announced(&e).await;
    assert!(
        before.iter().all(|state| state == "processing"),
        "only the move into processing can precede the baseline: {before:?}"
    );
    let base = before.len();

    // up_to_date once the file is done.
    sqlx::query("UPDATE _oo_docs.file SET state = 'published'")
        .execute(&e.pool)
        .await
        .unwrap();
    wait_state(&e, &id, "up_to_date").await;
    let f = drive_state(&e, &id).await;
    assert_eq!(
        (f["processing"].as_i64(), f["reason_code"].is_null()),
        (Some(0), true)
    );
    assert!(f["as_of"].is_string() && f["ontology_version"].is_i64());

    // waiting_review: content stuck in review, the rest published.
    sqlx::query("UPDATE _oo_docs.file SET pending_review = 3")
        .execute(&e.pool)
        .await
        .unwrap();
    wait_state(&e, &id, "waiting_review").await;
    assert_eq!(drive_state(&e, &id).await["pending_review"], 3);
    assert_eq!(health_state(&e).await, "waiting_review");

    // publishing outranks waiting_review.
    {
        let mut c = e.pool.acquire().await.unwrap();
        let mut job = NewJob::new(JobKind::Publish, "publish:freshness-walk");
        job.run_after_ms = 3_600_000;
        jobstore::enqueue(&mut c, &job).await.unwrap();
    }
    wait_state(&e, &id, "publishing").await;
    assert_eq!(health_state(&e).await, "publishing");

    // processing outranks publishing.
    sqlx::query("UPDATE _oo_docs.file SET state = 'parsing'")
        .execute(&e.pool)
        .await
        .unwrap();
    wait_state(&e, &id, "processing").await;
    sqlx::query("UPDATE _oo_docs.file SET state = 'published'")
        .execute(&e.pool)
        .await
        .unwrap();
    wait_state(&e, &id, "publishing").await;

    // degraded outranks everything but held: the root disappears and the
    // next sync fails, with the reason code on the Drive.
    std::fs::remove_dir_all(&e.root).unwrap();
    let r = e
        .post(&format!("/v1/docs/drives/{id}:sync"), &json!({}))
        .await;
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    wait_state(&e, &id, "degraded").await;
    let f = drive_state(&e, &id).await;
    assert!(
        f["reason_code"]
            .as_str()
            .is_some_and(|c| c.starts_with("DOCS_SOURCE_")),
        "{f}"
    );
    assert_eq!(health_state(&e).await, "degraded");

    // Every change was announced, in order, and only changes (the sync that
    // fails is seen as `syncing` first).
    let seq = announced(&e).await.split_off(base);
    assert_eq!(
        seq[..5],
        [
            "up_to_date",
            "waiting_review",
            "publishing",
            "processing",
            "publishing"
        ],
        "{seq:?}"
    );
    // The failing sync may or may not have been seen as `syncing` first.
    let rest = &seq[5..];
    assert_eq!(rest.last().map(String::as_str), Some("degraded"), "{seq:?}");
    assert!(
        rest[..rest.len() - 1].iter().all(|s| s == "syncing"),
        "{seq:?}"
    );
    let payload: String = sqlx::query_scalar(
        "SELECT payload::text FROM _oo_docs.event WHERE type = 'drive.freshness_changed' \
          ORDER BY seq DESC LIMIT 1",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    let v: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(v["drive_id"], id);
    assert_eq!(v["freshness"]["state"], "degraded");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn health_summarises_the_highest_state_of_all_drives() {
    let e = TestEngine::start(
        "fresh_health",
        Opts {
            workers: true,
            ..Opts::default()
        },
    )
    .await;
    assert_eq!(health_state(&e).await, "up_to_date", "no drives");
    e.write("a.md", "# a\n");
    let drive = e.create_drive("one").await;
    let id = drive["id"].as_str().unwrap().to_owned();
    wait_state(&e, &id, "processing").await;
    assert_eq!(health_state(&e).await, "processing");
    sqlx::query("UPDATE _oo_docs.file SET state = 'published'")
        .execute(&e.pool)
        .await
        .unwrap();
    wait_state(&e, &id, "up_to_date").await;
    assert_eq!(health_state(&e).await, "up_to_date");
    e.finish().await;
}
