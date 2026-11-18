//! Reprocess after a parser or prompt upgrade (B-S10-02, D23, DOC-G10) against one throwaway PostgreSQL: a small
//! difference replaces the old extraction, a difference over 30% or one that drops a fact a live Share uses waits
//! for a person, the old facts stay published meanwhile, and accepting or rejecting finishes the job.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test reprocess -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::{Fx, Planted};
use oo_docs::publish::reprocess::{Reprocess, index_gated};
use oo_docs::publish::run::PublishOutcome;
use oo_docs::publish::write::DocumentInput;
use oo_docs::review::decide::{Decision, ItemDecision, decide_items};

const BASE: &str = "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n状态：进行中\n成员：甲甲、乙乙、丙丙、丁丁、戊戊\n";

async fn reprocess(fx: &mut Fx, p: &Planted, path: &str, src: &str) -> Reprocess {
    let mut n = oo_docs::parse::markdown::parse_markdown(src.as_bytes()).unwrap();
    // The upgraded parser's result for the same blob: the IR the publisher reads evidence from.
    n.ir.blob_b3 = p.b3.clone();
    let canonical = oo_builder::canonical_json(&serde_json::to_value(&n.ir).unwrap());
    let put = fx
        .cas
        .put(oo_docs::cas::CasKind::Ir, canonical.as_bytes())
        .unwrap();
    sqlx::query(
        "INSERT INTO _oo_docs.parse_result (b3, parser_id, parser_version, profile_sha256, status, ir_b3, \
            block_count, finished_at) VALUES ($1, 'oo-md', '2', $2, 'complete', $3, $4, clock_timestamp() + interval '1 minute')",
    )
    .bind(&p.b3)
    .bind("0".repeat(64))
    .bind(&put.b3)
    .bind(i32::try_from(n.ir.blocks.len()).unwrap())
    .execute(&mut fx.admin)
    .await
    .unwrap();
    let doc = DocumentInput {
        version_id: &p.version_id,
        b3: &p.b3,
        path,
        ir: &n.ir,
    };
    index_gated(&mut fx.rt, &doc).await.unwrap()
}

async fn published(fx: &mut Fx, p: &Planted, path: &str) {
    let doc = DocumentInput {
        version_id: &p.version_id,
        b3: &p.b3,
        path,
        ir: &p.ir,
    };
    // A first extraction through the gate: no prior facts, a plain index.
    assert!(matches!(
        index_gated(&mut fx.rt, &doc).await.unwrap(),
        Reprocess::Fresh(_)
    ));
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("v1")
    };
}

fn item(id: &str, revision: i64, d: Decision) -> ItemDecision {
    ItemDecision {
        id: id.to_owned(),
        revision,
        decision: d,
        edit: None,
        note: None,
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_small_diff_replaces_the_old_extraction_without_a_person() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_reprocess_small";
    let mut fx = Fx::new(db).await;
    let path = "Projects/星火计划.md";
    let p = fx.plant_md(path, BASE).await;
    published(&mut fx, &p, path).await;
    let before = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
        .await;
    assert!(before >= 8, "{before}");
    // The same file with one more member: the new run has everything the old one had, plus one.
    let out = reprocess(&mut fx, &p, path, &format!("{BASE}\n成员：己己\n")).await;
    let Reprocess::Applied { diff, .. } = out else {
        panic!("{out:?}")
    };
    assert!(
        diff.removed.is_empty() && !diff.added.is_empty(),
        "{diff:?}"
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'reprocess_diff'")
            .await,
        0
    );
    // Old facts were published all along and stay so.
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
            .await,
        before
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_big_diff_waits_for_a_person_and_the_old_facts_stay_usable() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_reprocess_big";
    let mut fx = Fx::new(db).await;
    let path = "Projects/星火计划.md";
    let p = fx.plant_md(path, BASE).await;
    published(&mut fx, &p, path).await;
    let before = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
        .await;
    let other = "---\ntype: project\nname: 星火计划\nowner: 赵六\n---\n# 星火计划\n\n状态：已暂停\n成员：子子、丑丑\n";
    let out = reprocess(&mut fx, &p, path, other).await;
    let Reprocess::Held { diff, reason, .. } = out else {
        panic!("{out:?}")
    };
    assert_eq!(reason, "diff_over_30_percent");
    assert!(!diff.removed.is_empty() && !diff.added.is_empty());
    // The old extraction is untouched and the new facts are drafts that do not publish.
    let _ = fx.publish().await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
            .await,
        before,
        "old facts remain usable while the diff waits"
    );
    assert!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'draft'")
            .await
            > 0
    );
    let (id, revision): (String, i64) = sqlx::query_as(
        "SELECT id::text, revision FROM _oo_docs.review_item WHERE kind = 'reprocess_diff' AND status = 'pending'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();

    // Accept: the new extraction replaces the old one at the next publish.
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[item(&id, revision, Decision::Accept)],
    )
    .await
    .unwrap();
    assert_eq!(out.results[0].status, "ok", "{:?}", out.results[0]);
    let _ = fx.publish().await;
    let _ = fx.publish().await;
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity o ON o.id = f.object_id \
              WHERE f.status = 'published' AND o.canonical_name = '甲甲'"
        )
        .await,
        0,
        "the member the new run no longer finds is retracted"
    );
    assert!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.status = 'published' AND s.canonical_name = '赵六'"
        )
        .await
            > 0,
        "and the new owner is published"
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn rejecting_keeps_the_old_extraction_and_a_share_used_fact_blocks_a_small_diff() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_reprocess_reject";
    let mut fx = Fx::new(db).await;
    let path = "Projects/星火计划.md";
    let p = fx.plant_md(path, BASE).await;
    published(&mut fx, &p, path).await;
    let before = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
        .await;
    // A live Share's pack used the fact "丙丙 member_of 星火计划".
    sqlx::query(
        "INSERT INTO _oo_docs.context_pack (id, version_n, drive_id, scope_sha256, params_sha256, compiler, \
            pack_digest, snapshot_sha256, manifest, tokens_brief, trimmed, blocked_facts, blocked_entities) \
         SELECT gen_random_uuid(), max(n), (SELECT id FROM _oo_docs.drive LIMIT 1), repeat('0', 64), \
                repeat('0', 64), 'c', repeat('0', 64), repeat('0', 64), '{}'::jsonb, 1, '{}'::jsonb, 0, 0 \
           FROM _oo_docs.ontology_version",
    )
    .execute(&mut fx.admin)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO _oo_docs.share (id, pack_id, drive_id, scope, role, task, budget, status, grantor, mount_ref, \
            expires_at) \
         SELECT gen_random_uuid(), c.id, c.drive_id, '[]'::jsonb, 'viewer', NULL, 2000, 'active', 'g', 'm', \
                now() + interval '1 day' FROM _oo_docs.context_pack c",
    )
    .execute(&mut fx.admin)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO _oo_docs.share_fact_use (share_id, fact_id, section, precision) \
         SELECT s.id, f.id, 'people', 'full' FROM _oo_docs.share s, _oo_docs.fact f \
           JOIN _oo_docs.entity e ON e.id = f.subject_id WHERE e.canonical_name = '丙丙' AND f.predicate = 'member_of'",
    )
    .execute(&mut fx.admin)
    .await
    .unwrap();
    // The new run loses only that member: a 1 fact diff, but a shared fact.
    let reduced = BASE.replace("丙丙、", "");
    let out = reprocess(&mut fx, &p, path, &reduced).await;
    let Reprocess::Held { reason, diff, .. } = out else {
        panic!("{out:?}")
    };
    assert_eq!(reason, "share_used_fact_removed");
    assert_eq!(diff.share_used_removed.len(), 1);
    let (id, revision): (String, i64) = sqlx::query_as(
        "SELECT id::text, revision FROM _oo_docs.review_item WHERE kind = 'reprocess_diff' AND status = 'pending'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[item(&id, revision, Decision::Reject)],
    )
    .await
    .unwrap();
    assert_eq!(out.results[0].status, "ok");
    let _ = fx.publish().await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE status = 'published'")
            .await,
        before,
        "reject keeps the old extraction"
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_parser_upgrade_marks_files_outdated_and_reprocesses_them_in_small_batches() {
    use oo_docs::jobs::stages::{mark_outdated_versions, pump_once, set_current_parser};
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_reprocess_upgrade";
    let mut fx = Fx::new(db).await;
    for i in 0..6 {
        fx.plant_md(
            &format!("Notes/n{i}.md"),
            &format!("# 笔记 {i}\n\n内容 {i}\n"),
        )
        .await;
    }
    // Their parse came from the sidecar parser `docling` 1; they are published.
    sqlx::query("UPDATE _oo_docs.parse_result SET parser_id = 'docling', parser_version = '1'")
        .execute(&mut fx.admin)
        .await
        .unwrap();
    sqlx::query("UPDATE _oo_docs.file SET state = 'published'")
        .execute(&mut fx.admin)
        .await
        .unwrap();
    let pool = common::pool_runtime(db, 4).await;
    // Same version: nothing to do.
    set_current_parser("docling", "1");
    assert_eq!(
        mark_outdated_versions(&pool, "extract/1", false)
            .await
            .unwrap(),
        0
    );
    // The sidecar now reports docling 2: every file made by 1 is outdated.
    set_current_parser("docling", "2");
    assert_eq!(
        mark_outdated_versions(&pool, "extract/1", false)
            .await
            .unwrap(),
        6
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'outdated'")
            .await,
        6
    );
    // The pump starts one reprocess job at a time at low priority, never the whole Drive at once.
    pump_once(&pool).await.unwrap();
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse' AND priority = 900 AND state = 'queued'")
            .await,
        1
    );
    pump_once(&pool).await.unwrap();
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.job WHERE kind = 'parse' AND priority = 900")
            .await,
        1,
        "still one in flight"
    );
    // The version registry is process wide: put it back.
    set_current_parser("docling", "1");
    pool.close().await;
    fx.finish(db).await;
}
