//! `drive_rev`, coverage and the hash chain (B-S5-10, FM10): the freshness
//! token advances with every change, a version covers exactly the changes that
//! reached a terminal state, and an unchanged hash stops the chain.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test consistency_live -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::{DRIVE, Fx, uuid};
use oo_docs::publish::run::PublishOutcome;

async fn rev(fx: &mut Fx) -> i64 {
    fx.scalar(&format!(
        "SELECT rev FROM _oo_docs.drive WHERE id = '{DRIVE}'"
    ))
    .await
}

async fn covers(fx: &mut Fx, n: i64) -> i64 {
    fx.scalar(&format!(
        "SELECT covers_drive_rev FROM _oo_docs.ontology_version WHERE n = {n}"
    ))
    .await
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn drive_rev_advances_and_versions_cover_only_settled_changes() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_cons_rev";
    let mut fx = Fx::new(db).await;
    assert_eq!(rev(&mut fx).await, 0);
    fx.index_md("People/甲.md", "# 甲\n\n内容\n").await;
    fx.index_md("People/乙.md", "# 乙\n\n内容\n").await;
    assert_eq!(rev(&mut fx).await, 2, "one change per file version");
    let revs: Vec<i64> =
        sqlx::query_scalar("SELECT drive_rev FROM _oo_docs.file_version ORDER BY drive_rev")
            .fetch_all(&mut fx.admin)
            .await
            .unwrap();
    assert_eq!(revs, [1, 2]);

    // A file that is still queued with a lower rev than later changes holds coverage back.
    let (f, v) = (uuid(), uuid());
    sqlx::query("INSERT INTO _oo_docs.blob (b3, sha256, size, format) VALUES ($2, $1, 1, 'md')")
        .bind("0".repeat(64))
        .bind("c".repeat(64))
        .execute(&mut fx.admin)
        .await
        .unwrap();
    let q = "c".repeat(64);
    for sql in [
        format!(
            "INSERT INTO _oo_docs.file (id, drive_id, path, raw_key, path_fold, state, last_seen_generation) VALUES ('{f}', '{DRIVE}', 'Wait/q.md', convert_to('Wait/q.md','UTF8'), 'wait/q.md', 'queued', 1)"
        ),
        format!(
            "INSERT INTO _oo_docs.file_version (id, file_id, b3, path_at_version, fp) VALUES ('{v}', '{f}', '{q}', 'Wait/q.md', '{{}}'::jsonb)"
        ),
        format!("UPDATE _oo_docs.file SET current_version_id = '{v}' WHERE id = '{f}'"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut fx.admin)
            .await
            .unwrap();
    }
    assert_eq!(rev(&mut fx).await, 3);
    fx.index_md("People/丙.md", "# 丙\n\n内容\n").await;
    assert_eq!(rev(&mut fx).await, 4);
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    assert_eq!(
        v1.covers_drive_rev, 2,
        "the queued change (rev 3) is not covered, nor are later ones"
    );
    assert_eq!(covers(&mut fx, 1).await, 2);

    // It settles (a terminal failure counts): the next version covers everything.
    sqlx::query("UPDATE _oo_docs.file SET state = 'parse_failed' WHERE path = 'Wait/q.md'")
        .execute(&mut fx.admin)
        .await
        .unwrap();
    fx.index_md("People/丁.md", "# 丁\n\n内容\n").await;
    let r = rev(&mut fx).await;
    let PublishOutcome::Published(v2) = fx.publish().await else {
        panic!("v2")
    };
    assert_eq!(v2.covers_drive_rev, r);
    assert!(
        v2.covers_drive_rev >= v1.covers_drive_rev,
        "coverage never goes back"
    );

    // A deleted or superseded file is a change of its own.
    let before = rev(&mut fx).await;
    sqlx::query("UPDATE _oo_docs.file SET state = 'deleted', deleted_at = now() WHERE path = 'People/甲.md'")
        .execute(&mut fx.admin)
        .await
        .unwrap();
    assert_eq!(rev(&mut fx).await, before + 1);
    assert_eq!(
        fx.scalar("SELECT drive_rev FROM _oo_docs.file_version v JOIN _oo_docs.file f ON f.current_version_id = v.id WHERE f.path = 'People/甲.md'").await,
        before + 1
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn unchanged_assertions_and_entity_states_stop_the_chain() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_cons_hash";
    let mut fx = Fx::new(db).await;
    let body = |tail: &str| format!("---\ntype: person\naliases: 小甲\n---\n\n# 甲\n\n{tail}\n");
    let s = fx.index_md("People/甲.md", &body("第一版正文")).await;
    assert!(!s.early_cutoff, "no earlier version");
    fx.index_md("People/乙.md", "# 乙\n\n内容\n").await;
    fx.index_md("People/丙.md", "# 丙\n\n内容\n").await;
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    let total_entities = fx
        .scalar("SELECT count(*) FROM _oo_docs.entity_state WHERE ontology_version = 1")
        .await;
    assert!(total_entities >= 4, "{total_entities}");
    assert_eq!(v1.changed_entities as i64, total_entities);

    // A new version of 甲.md changes only the body: same assertion set.
    let p = fx
        .plant_version("People/甲.md", &body("第二版正文，只改了正文"))
        .await;
    let s = fx.index_planted("People/甲.md", &p).await;
    assert!(s.early_cutoff, "same assertions: {s:?}");
    let hashes: Vec<String> = fx
        .text("SELECT assertion_set_hash FROM _oo_docs.file_version v JOIN _oo_docs.file f ON f.id = v.file_id WHERE f.path = 'People/甲.md' ORDER BY v.created_at")
        .await;
    assert_eq!(hashes.len(), 2);
    assert_eq!(hashes[0], hashes[1]);
    // Changing a cited line changes the hash.
    let p = fx
        .plant_version(
            "People/甲.md",
            &body("正文").replace("aliases: 小甲", "aliases: 小甲, 阿甲"),
        )
        .await;
    let s = fx.index_planted("People/甲.md", &p).await;
    assert!(!s.early_cutoff, "{s:?}");

    // v2 adds one new fact about 乙 only: only the entities it touches get a new state.
    fx.index_md("Projects/星图.md", "# 星图\n\n负责人：乙\n")
        .await;
    let PublishOutcome::Published(v2) = fx.publish().await else {
        panic!("v2")
    };
    let rows = fx
        .scalar("SELECT count(*) FROM _oo_docs.entity_state WHERE ontology_version = 2")
        .await;
    assert_eq!(rows, i64::from(v2.changed_entities));
    assert!(rows >= 2 && rows < total_entities + 4, "{rows}");
    let untouched = fx
        .scalar("SELECT count(*) FROM _oo_docs.entity e WHERE e.canonical_name = '丙' AND EXISTS (SELECT 1 FROM _oo_docs.entity_state s WHERE s.entity_id = e.id AND s.ontology_version = 2)")
        .await;
    assert_eq!(
        untouched, 0,
        "an entity whose state hash did not change writes no row"
    );
    fx.finish(db).await;
}
