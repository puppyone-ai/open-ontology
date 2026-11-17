//! The disk guard (10 §4.12, G7, B-S4-05): below 2 GiB free, hashing and
//! parsing wait with `blocked_reason = disk_low` and health says
//! `DOCS_ENGINE_DISK_LOW`; nothing fails, and everything resumes by itself
//! when space is freed.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test disk_guard_live -- --ignored --test-threads=1`

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use common::engine::{Opts, TestEngine};
use oo_docs::health::DISK_LOW_BYTES;

const GIB: u64 = 1024 * 1024 * 1024;

async fn count(e: &TestEngine, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn low_disk_pauses_the_pipeline_without_failing_anything_and_resumes() {
    assert_eq!(DISK_LOW_BYTES, 2 * GIB, "the threshold is the spec's");
    let free = Arc::new(AtomicU64::new(GIB)); // 1 GiB: below the line
    let e = TestEngine::start(
        "disk_guard",
        Opts {
            workers: true,
            stages: true,
            free_bytes: Some(free.clone()),
            ..Opts::default()
        },
    )
    .await;
    for i in 0..6 {
        e.write(
            &format!("d/f{i}.md"),
            &format!("# 文件{i}\n\n负责人：林川\n"),
        );
    }
    let id = e.create_drive("disk").await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Health says why, with the code of G7.
    let h = e.get("/v1/docs/health").await.json();
    assert_eq!(h["status"], "degraded");
    assert_eq!(h["components"]["cas"]["status"], "degraded");
    assert_eq!(h["components"]["cas"]["code"], "DOCS_ENGINE_DISK_LOW");
    assert_eq!(h["disk"]["free_bytes"].as_u64(), Some(GIB));

    // The sync listed the files but nothing moves past stored/queued, and the
    // files say why (and an event announced it).
    e.until("files blocked", || async {
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.file WHERE blocked_reason = 'disk_low'",
        )
        .await
            == 6
    })
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        count(&e, "SELECT count(*) FROM _oo_docs.file WHERE state IN ('parsing', 'parsed', 'extracting', 'indexed', 'published')").await,
        0,
        "no work while the disk is low"
    );
    let listed = e.get(&format!("/v1/docs/drives/{id}/files")).await.json();
    for f in listed["items"].as_array().unwrap() {
        assert_eq!(f["blocked_reason"], "disk_low", "{f}");
        assert!(
            matches!(f["state"].as_str(), Some("stored" | "queued")),
            "{f}"
        );
    }
    assert!(
        count(&e, "SELECT count(*) FROM _oo_docs.event WHERE type = 'file.state_changed' AND payload->>'code' = 'DOCS_ENGINE_DISK_LOW'").await >= 6,
        "each block is an event"
    );
    // Nothing failed: jobs wait (a paused job is not a failed attempt).
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.job WHERE state IN ('failed', 'quarantined')"
        )
        .await,
        0
    );
    assert_eq!(
        count(&e, "SELECT count(*) FROM _oo_docs.job WHERE kind = 'hash' AND attempt > 0 AND last_error <> 'DOCS_ENGINE_DISK_LOW'").await,
        0
    );
    let fresh = e.get(&format!("/v1/docs/drives/{id}")).await.json()["freshness"].clone();
    assert_eq!(fresh["state"], "processing");

    // Space comes back: everything resumes by itself.
    free.store(50 * GIB, Ordering::Relaxed);
    e.until("all published", || async {
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.file WHERE state = 'published'",
        )
        .await
            == 6
    })
    .await;
    assert_eq!(
        count(
            &e,
            "SELECT count(*) FROM _oo_docs.file WHERE blocked_reason IS NOT NULL"
        )
        .await,
        0,
        "the reason is cleared"
    );
    let h = e.get("/v1/docs/health").await.json();
    assert_eq!(h["status"], "ok");
    assert_eq!(h["components"]["cas"]["status"], "ok");
    e.finish().await;
}
