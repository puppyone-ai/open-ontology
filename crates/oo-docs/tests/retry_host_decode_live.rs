//! `files:retry` for a recording parked as `unsupported(DOCS_PARSE_CODEC_UNAVAILABLE)` (10 17.2, lane I): the host
//! hands over decoded output after the file was parked, and the retry makes the engine parse it again. Any other
//! `unsupported` file is still not retryable. A Markdown file stands in for the recording so that no sidecar is
//! needed: what is under test is the state edge and the job, the sidecar side is the L6 end to end run (A5b).
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test retry_host_decode_live -- --ignored --test-threads=1`

mod common;

use common::engine::{Opts, TestEngine};
use common::pool_admin;
use oo_docs::jobs::stages::{self, RetryResult};

async fn state(e: &TestEngine, path: &str) -> (String, Option<String>) {
    sqlx::query_as(
        "SELECT state, state_code FROM _oo_docs.file WHERE path = $1 AND deleted_at IS NULL",
    )
    .bind(path)
    .fetch_optional(&e.pool)
    .await
    .unwrap()
    .unwrap_or_default()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_codec_parked_file_is_parsed_again_by_a_retry_and_other_unsupported_files_are_not() {
    let e = TestEngine::start(
        "retry_host_decode",
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    for p in ["a/clip.md", "a/odd.md"] {
        e.write(
            p,
            &format!("---\ntype: project\nname: {p}\nowner: 林川\n---\n# {p}\n\n负责人：林川\n"),
        );
    }
    e.create_drive("retry-host-decode").await;
    e.until("both published", || async {
        state(&e, "a/clip.md").await.0 == "published"
            && state(&e, "a/odd.md").await.0 == "published"
    })
    .await;
    // Park them as the media stage does (the runtime login cannot write the state column directly).
    let admin = pool_admin("retry_host_decode", 2).await;
    for (p, code) in [
        ("a/clip.md", "DOCS_PARSE_CODEC_UNAVAILABLE"),
        ("a/odd.md", "DOCS_PARSE_UNSUPPORTED_FORMAT"),
    ] {
        sqlx::query(
            "UPDATE _oo_docs.file SET state = 'unsupported', state_code = $2 WHERE path = $1",
        )
        .bind(p)
        .bind(code)
        .execute(&admin)
        .await
        .unwrap();
    }
    let id = |p: &'static str| {
        let pool = e.pool.clone();
        async move {
            sqlx::query_scalar::<_, String>("SELECT id::text FROM _oo_docs.file WHERE path = $1")
                .bind(p)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    let (clip, odd) = (id("a/clip.md").await, id("a/odd.md").await);
    assert_eq!(
        stages::retry_file(&e.pool, &odd, false).await.unwrap(),
        RetryResult::NotRetryable("unsupported".into())
    );
    assert_eq!(state(&e, "a/odd.md").await.0, "unsupported");
    assert_eq!(
        stages::retry_file(&e.pool, &clip, false).await.unwrap(),
        RetryResult::Queued
    );
    e.until("the retried file is published again", || async {
        let (s, c) = state(&e, "a/clip.md").await;
        s == "published" && c.is_none()
    })
    .await;
    let path: Vec<String> = sqlx::query_scalar(
        "SELECT h.to_state FROM _oo_docs.file_state_history h JOIN _oo_docs.file f ON f.id = h.file_id \
          WHERE f.path = 'a/clip.md' ORDER BY h.seq DESC LIMIT 6",
    )
    .fetch_all(&e.pool)
    .await
    .unwrap();
    assert_eq!(
        path.iter().rev().map(String::as_str).collect::<Vec<_>>(),
        [
            "outdated",
            "parsing",
            "parsed",
            "extracting",
            "indexed",
            "published"
        ]
    );
}
