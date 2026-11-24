//! File states beyond the happy path, on the real pipeline and a real
//! database (B-S3-04): duplicates keep their own evidence and show up in
//! packs, a rename moves the evidence, an upgraded parser sends files through
//! `outdated`.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test state_live -- --ignored --test-threads=2`

mod common;

use std::time::Duration;

use common::engine::{Opts, TestEngine};
use oo_docs::jobs::stages;
use serde_json::{Value, json};

const PROJECT: &str = "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n";

fn opts() -> Opts {
    Opts {
        workers: true,
        stages: true,
        ..Opts::default()
    }
}

async fn state_of(e: &TestEngine, path: &str) -> String {
    sqlx::query_scalar("SELECT state FROM _oo_docs.file WHERE path = $1 AND deleted_at IS NULL")
        .bind(path)
        .fetch_optional(&e.pool)
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn history(e: &TestEngine, path: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT to_state FROM _oo_docs.file_state_history WHERE file_id = \
         (SELECT id FROM _oo_docs.file WHERE path = $1 ORDER BY deleted_at NULLS FIRST LIMIT 1) \
         ORDER BY seq",
    )
    .bind(path)
    .fetch_all(&e.pool)
    .await
    .unwrap()
}

async fn live_evidence(e: &TestEngine, path: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.evidence ev JOIN _oo_docs.file f \
           ON f.current_version_id = ev.version_id \
          WHERE f.path = $1 AND f.deleted_at IS NULL AND ev.withdrawn_at IS NULL",
    )
    .bind(path)
    .fetch_one(&e.pool)
    .await
    .unwrap()
}

async fn version(e: &TestEngine) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(max(n), 0) FROM _oo_docs.ontology_version")
        .fetch_one(&e.pool)
        .await
        .unwrap()
}

async fn compile(e: &TestEngine, drive: &str, scope: &str) -> Value {
    e.post(
        "/v1/docs/context-packs:compile",
        &json!({"drive_id": drive, "scope": [scope], "budget": 800}),
    )
    .await
    .json()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_duplicate_keeps_its_own_evidence_and_appears_in_packs() {
    let e = TestEngine::start("state_dup", opts()).await;
    e.write("Projects/a/星火计划.md", PROJECT);
    let drive = e.create_drive("state-dup").await;
    let did = drive["id"].as_str().unwrap().to_owned();
    e.until("first copy published", || async {
        state_of(&e, "Projects/a/星火计划.md").await == "published"
    })
    .await;
    let original = live_evidence(&e, "Projects/a/星火计划.md").await;
    assert!(original > 0);
    // Before the copy there is nothing under Projects/b.
    assert_eq!(
        compile(&e, &did, "Projects/b").await["code"],
        "DOCS_PACK_SCOPE_INVALID"
    );

    let v0 = version(&e).await;
    e.write("Projects/b/星火计划-副本.md", PROJECT);
    let r = e
        .post(&format!("/v1/docs/drives/{did}:sync"), &json!({}))
        .await;
    assert_eq!(r.status, 202);
    e.until("copy is a duplicate", || async {
        state_of(&e, "Projects/b/星火计划-副本.md").await == "duplicate"
    })
    .await;
    // It was never parsed or extracted: same blob, one parse.
    assert_eq!(
        history(&e, "Projects/b/星火计划-副本.md").await,
        ["stored", "duplicate"]
    );
    let parses: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse' AND state = 'succeeded'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(parses, 1);
    // But it has its own evidence rows, the same targets and blocks.
    assert_eq!(
        live_evidence(&e, "Projects/b/星火计划-副本.md").await,
        original
    );
    let same_targets: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT target_id, block_id FROM _oo_docs.evidence \
           WHERE withdrawn_at IS NULL GROUP BY target_id, block_id HAVING count(*) = 2) t",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(same_targets as i64, original, "one copy per original row");

    // A duplicate adds no fact, but the version after it must exist so that a
    // pack can see the path.
    e.until("a version covers the duplicate", || async {
        version(&e).await > v0
    })
    .await;
    // So the duplicate's directory yields a pack that cites its own path.
    let pack = compile(&e, &did, "Projects/b").await;
    let brief = pack["brief_preview"].as_str().unwrap_or_default();
    assert!(brief.contains("[src:Projects/b/星火计划-副本.md"), "{pack}");
    assert!(!brief.contains("[src:Projects/a/"), "scope holds: {brief}");

    // The first copy goes away: the duplicate keeps the facts alive.
    std::fs::remove_file(e.root.join("Projects/a/星火计划.md")).unwrap();
    let _ = e
        .post(&format!("/v1/docs/drives/{did}:sync"), &json!({}))
        .await;
    e.until("original deleted", || async {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.file WHERE path = 'Projects/a/星火计划.md' AND deleted_at IS NOT NULL",
        )
        .fetch_one(&e.pool)
        .await
        .unwrap();
        n == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let retracted: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(retracted, 0, "facts still have the duplicate's evidence");
    let pack = compile(&e, &did, "Projects/b").await;
    assert!(pack["brief_preview"].as_str().unwrap().contains("林川"));
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_rename_moves_the_evidence_and_retracts_nothing() {
    let e = TestEngine::start("state_rename", opts()).await;
    e.write("Projects/a/星火计划.md", PROJECT);
    let drive = e.create_drive("state-rename").await;
    let did = drive["id"].as_str().unwrap().to_owned();
    e.until("published", || async {
        state_of(&e, "Projects/a/星火计划.md").await == "published"
    })
    .await;
    let n = live_evidence(&e, "Projects/a/星火计划.md").await;
    let v0 = version(&e).await;
    std::fs::create_dir_all(e.root.join("Projects/c")).unwrap();
    std::fs::rename(
        e.root.join("Projects/a/星火计划.md"),
        e.root.join("Projects/c/星火计划.md"),
    )
    .unwrap();
    let _ = e
        .post(&format!("/v1/docs/drives/{did}:sync"), &json!({}))
        .await;
    e.until("renamed", || async {
        state_of(&e, "Projects/c/星火计划.md").await == "duplicate"
    })
    .await;
    e.until("a version covers the renamed file", || async {
        version(&e).await > v0
    })
    .await;
    assert_eq!(live_evidence(&e, "Projects/c/星火计划.md").await, n);
    let retracted: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(retracted, 0, "a rename does not retract facts");
    let pack = compile(&e, &did, "Projects/c").await;
    assert!(
        pack["brief_preview"]
            .as_str()
            .unwrap_or_default()
            .contains("[src:Projects/c/星火计划.md"),
        "{pack}"
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn an_upgraded_parser_sends_files_through_outdated_and_back() {
    let e = TestEngine::start("state_outdated", opts()).await;
    e.write("a/x.md", PROJECT);
    e.write(
        "a/y.md",
        "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n\n角色：工程负责人\n",
    );
    e.write("a/z.txt", "随手记。\n");
    let drive = e.create_drive("state-outdated").await;
    let _ = drive;
    e.until("all published", || async {
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'published'")
                .fetch_one(&e.pool)
                .await
                .unwrap();
        n == 3
    })
    .await;
    let facts_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(
        stages::mark_outdated(&e.pool).await.unwrap(),
        0,
        "nothing is old yet"
    );

    // The parser "was upgraded": the stored results are from an older one.
    sqlx::query("UPDATE _oo_docs.parse_result SET parser_version = 'old/0'")
        .execute(&e.pool)
        .await
        .unwrap();
    assert_eq!(stages::mark_outdated(&e.pool).await.unwrap(), 3);
    assert_eq!(state_of(&e, "a/x.md").await, "outdated");
    e.until("reprocessed", || async {
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'published'")
                .fetch_one(&e.pool)
                .await
                .unwrap();
        n == 3
    })
    .await;
    let h = history(&e, "a/x.md").await;
    assert_eq!(
        h[h.len() - 7..],
        [
            "published",
            "outdated",
            "parsing",
            "parsed",
            "extracting",
            "indexed",
            "published"
        ],
        "{h:?}"
    );
    // Reprocessing ran at the low priority and left the facts as they were.
    let prio: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse' AND priority = 900 AND state = 'succeeded'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(prio, 3);
    let facts_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(facts_after, facts_before);
    let retracted: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'retracted'")
            .fetch_one(&e.pool)
            .await
            .unwrap();
    assert_eq!(retracted, 0);
    assert_eq!(
        stages::mark_outdated(&e.pool).await.unwrap(),
        0,
        "current again"
    );
    e.finish().await;
}
