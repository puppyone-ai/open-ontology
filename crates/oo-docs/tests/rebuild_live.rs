//! `POST /drives/{id}:rebuild` over real HTTP and a real PostgreSQL (B-S10-03, D23): the only full rebuild is manual
//! and needs a confirmation; the old facts stay published while the files are reprocessed.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test rebuild_live -- --ignored --test-threads=1`

mod common;

use common::engine::{Opts, TestEngine};
use serde_json::json;

async fn states(e: &TestEngine) -> Vec<String> {
    sqlx::query_scalar("SELECT state FROM _oo_docs.file ORDER BY path")
        .fetch_all(&e.pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_full_rebuild_needs_confirmation_and_keeps_the_old_facts() {
    let e = TestEngine::start(
        "api_rebuild",
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    for i in 0..3 {
        e.write(
            &format!("Projects/p{i}.md"),
            &format!(
                "---\ntype: project\nname: 计划{i}\nowner: 林川\n---\n# 计划{i}\n\n状态：进行中\n"
            ),
        );
    }
    let drive = e.create_drive("rebuild").await;
    let id = drive["id"].as_str().unwrap().to_owned();
    // Creating the drive starts its first sync; a second one may answer 409 while it runs.
    let _ = e
        .post(&format!("/v1/docs/drives/{id}:sync"), &json!({}))
        .await;
    e.until("three published files", || async {
        states(&e).await == ["published", "published", "published"]
    })
    .await;
    let facts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    let path = format!("/v1/docs/drives/{id}:rebuild");

    // Without the confirmation, or with the wrong one, nothing happens.
    for body in [
        json!({}),
        json!({"confirm": "yes"}),
        json!({"confirm": true}),
    ] {
        let r = e.post(&path, &body).await;
        assert_eq!(r.status, 400, "{}", String::from_utf8_lossy(&r.body));
        assert_eq!(r.json()["code"], "DOCS_ENGINE_BAD_REQUEST");
    }
    assert_eq!(states(&e).await, ["published", "published", "published"]);

    // An idempotency key is required like for the other POSTs.
    let r = e
        .req("POST", &path, &[], Some(&json!({"confirm": id})))
        .await;
    assert_eq!(r.status, 428);

    // Confirmed: every file goes back through parse and extraction and ends published again.
    let r = e.post(&path, &json!({"confirm": id})).await;
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(r.json()["files"], 3);
    assert_eq!(r.json()["priority"], 900);
    e.until("rebuilt", || async {
        states(&e).await == ["published", "published", "published"]
            && sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse' AND priority = 900 AND state = 'succeeded'",
            )
            .fetch_one(&e.pool)
            .await
            .unwrap()
                == 3
    })
    .await;
    let after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(after, facts, "same input, same facts, no churn");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'reprocess_diff'"
        )
        .fetch_one(&e.pool)
        .await
        .unwrap(),
        0,
        "an identical extraction has no diff to judge"
    );
    e.finish().await;
}
