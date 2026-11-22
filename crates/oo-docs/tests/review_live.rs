//! Review queue and publish gates against one throwaway PostgreSQL
//! (B-S5-06, B-S5-07): thresholds, batch decisions with revisions, accept by
//! source in one operation, overlays, orphaned overlays, DOC-G3 to G5 and the
//! time from an accept to the next version.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test review_live -- --ignored --test-threads=1`

mod common;

use std::time::{Duration, Instant};

use common::CLUSTER_LOCK;
use common::fx::Fx;
use oo_docs::publish::run::{PublishConfig, PublishError, PublishOutcome, publish};
use oo_docs::review::decide::{Decision, ItemDecision, decide_by_source, decide_items};
use oo_docs::review::overlay::sweep_orphans;
use serde_json::json;

async fn pending(fx: &mut Fx, kind: &str) -> Vec<(String, i64)> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT id::text, revision FROM _oo_docs.review_item WHERE kind = '{kind}' AND status = 'pending' ORDER BY created_at, id"
    )))
    .fetch_all(&mut fx.admin)
    .await
    .unwrap()
}

fn item(id: &str, revision: i64, decision: Decision) -> ItemDecision {
    ItemDecision {
        id: id.to_owned(),
        revision,
        decision,
        edit: None,
        note: None,
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn thresholds_queue_and_accept_by_source_in_one_operation() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_rev_source";
    let mut fx = Fx::new(db).await;
    fx.index_md("Notes/big.md", "# 议题清单\n\n内容\n").await;
    let high = fx.llm_facts("Notes/big.md", 1, 9500).await;
    let mid = fx.llm_facts("Notes/big.md", 100, 6000).await;
    // Another 20 below the floor stay drafts and are not queued.
    let _low = fx.llm_facts_named("Notes/big.md", "底线", 20, 3000).await;
    assert_eq!(high.len() + mid.len(), 101);
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    // The 8500 fact published with the rule facts; the 6000 ones are queued.
    let published_llm = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE origin = 'llm' AND status = 'published'")
        .await;
    assert!(published_llm >= 1, "high confidence publishes");
    assert_eq!(pending(&mut fx, "low_confidence_fact").await.len(), 100);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE origin = 'llm' AND status = 'draft'")
            .await,
        121 - published_llm
    );
    assert_eq!(
        fx.scalar("SELECT pending_review::bigint FROM _oo_docs.file WHERE path = 'Notes/big.md'")
            .await,
        100,
        "the tree shows the pending count"
    );

    // One operation accepts all 100.
    let file_id = fx
        .text("SELECT id::text FROM _oo_docs.file WHERE path = 'Notes/big.md'")
        .await
        .pop()
        .unwrap();
    let started = Instant::now();
    let out = decide_by_source(&mut fx.rt, "tester", &file_id, &[], true, None)
        .await
        .unwrap();
    assert_eq!((out.accepted, out.rejected), (100, 0));
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'accept_source'")
            .await,
        1
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'fact_assert'")
            .await,
        100
    );
    assert_eq!(pending(&mut fx, "low_confidence_fact").await.len(), 0);
    assert_eq!(
        fx.scalar("SELECT pending_review::bigint FROM _oo_docs.file WHERE path = 'Notes/big.md'")
            .await,
        100,
        "counts refresh on the next sweep"
    );

    // The next version has all 100, within the time of one publish.
    let PublishOutcome::Published(v2) = fx.publish().await else {
        panic!("v2")
    };
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "accept to new version took {:?}",
        started.elapsed()
    );
    assert_eq!(v2.n, v1.n + 1);
    assert!(v2.facts_added >= 100, "{}", v2.facts_added);
    assert_eq!(
        fx.scalar("SELECT pending_review::bigint FROM _oo_docs.file WHERE path = 'Notes/big.md'")
            .await,
        0
    );
    // The floor facts are still drafts and nothing about them was queued.
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.fact WHERE confidence_bp = 3000 AND status = 'draft'"
        )
        .await,
        20
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn batch_decisions_report_each_item_and_races_have_one_winner() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_rev_batch";
    let mut fx = Fx::new(db).await;
    fx.index_md("Notes/b.md", "# 议题\n\n内容\n").await;
    fx.llm_facts("Notes/b.md", 5, 6000).await;
    fx.publish().await;
    let items = pending(&mut fx, "low_confidence_fact").await;
    assert_eq!(items.len(), 5);

    let batch = vec![
        item(&items[0].0, items[0].1, Decision::Accept),
        item(&items[1].0, items[1].1 + 5, Decision::Accept), // stale
        item(&items[2].0, items[2].1, Decision::Reject),
        item("00000000-0000-4000-8000-00000000ffff", 1, Decision::Accept),
        ItemDecision {
            edit: None,
            ..item(&items[3].0, items[3].1, Decision::Edit) // edit needs a body
        },
    ];
    let out = decide_items(&mut fx.rt, "tester", &batch).await.unwrap();
    let statuses: Vec<(&str, Option<&str>)> =
        out.results.iter().map(|r| (r.status, r.code)).collect();
    assert_eq!(
        statuses,
        [
            ("ok", None),
            ("conflict", Some("DOCS_REVIEW_STALE_REVISION")),
            ("ok", None),
            ("not_found", Some("DOCS_REVIEW_ITEM_NOT_FOUND")),
            ("invalid", Some("DOCS_REVIEW_INVALID_DECISION")),
        ]
    );
    assert_eq!(out.results[1].current_revision, Some(items[1].1));
    assert_eq!((out.accepted, out.rejected), (1, 1));
    // Deciding again is reported, not repeated.
    let again = decide_items(&mut fx.rt, "tester", &batch[..1])
        .await
        .unwrap();
    assert_eq!(again.results[0].status, "already_decided");

    // Two clients decide the same item at the same revision: one wins.
    let mut second = common::connect_runtime(db).await;
    let it = item(&items[4].0, items[4].1, Decision::Accept);
    let it2 = item(&items[4].0, items[4].1, Decision::Reject);
    let (a, b) = tokio::join!(
        decide_items(&mut fx.rt, "a", std::slice::from_ref(&it)),
        decide_items(&mut second, "b", std::slice::from_ref(&it2))
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    let oks = [&a, &b]
        .iter()
        .filter(|o| o.results[0].status == "ok")
        .count();
    assert_eq!(oks, 1, "{a:?} {b:?}");
    let loser = if a.results[0].status == "ok" { &b } else { &a };
    assert!(matches!(
        loser.results[0].status,
        "conflict" | "already_decided"
    ));
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn reject_edit_and_overlays_survive_re_extraction() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_rev_overlay";
    let mut fx = Fx::new(db).await;
    // name.version only proposes: queued as supersede_candidate.
    fx.index_md("Proposals/方案.md", "# 方案\n\n负责人：陈子墨\n")
        .await;
    fx.publish().await;
    fx.share_uses_every_fact().await;
    fx.index_md("Proposals/方案-v2.md", "# 方案\n\n内容\n")
        .await;
    fx.publish().await;
    let cand = pending(&mut fx, "low_confidence_fact").await;
    assert_eq!(cand.len(), 1);
    assert_eq!(
        fx.text("SELECT reason_code FROM _oo_docs.review_item WHERE kind = 'low_confidence_fact'")
            .await,
        ["supersede_candidate"]
    );
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[item(&cand[0].0, cand[0].1, Decision::Reject)],
    )
    .await
    .unwrap();
    assert_eq!(out.rejected, 1);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE predicate = 'supersedes' AND status = 'rejected'")
            .await,
        1
    );
    // The same content extracted again does not come back as a draft.
    let again = fx.known_plant("Proposals/方案-v2.md").await;
    fx.index_planted("Proposals/方案-v2.md", &again).await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE predicate = 'supersedes' AND status IN ('draft','published')")
            .await,
        0
    );
    fx.publish().await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE predicate = 'supersedes' AND status = 'published'")
            .await,
        0
    );

    // Edit: a new value by a person replaces an LLM value.
    fx.index_md("Notes/e.md", "# 备忘\n\n内容\n").await;
    let ids = fx.llm_facts("Notes/e.md", 1, 6000).await;
    fx.publish().await;
    let it = pending(&mut fx, "low_confidence_fact").await;
    assert_eq!(it.len(), 1);
    let edit = ItemDecision {
        edit: Some(json!({"valid_from": "2026-05-01"})),
        ..item(&it[0].0, it[0].1, Decision::Edit)
    };
    let out = decide_items(&mut fx.rt, "tester", &[edit]).await.unwrap();
    assert_eq!(out.accepted, 1, "{out:?}");
    assert_eq!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.fact WHERE id = '{}' AND status = 'rejected'",
            ids[0]
        ))
        .await,
        1
    );
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("edited fact publishes")
    };
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE origin = 'human' AND status = 'published' AND valid_from = '2026-05-01'")
            .await,
        1
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.evidence e JOIN _oo_docs.fact f ON f.id = e.target_id WHERE f.origin = 'human' AND e.withdrawn_at IS NULL")
            .await,
        1,
        "the human fact keeps the evidence"
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn overlay_whose_evidence_vanished_becomes_an_orphan_item() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_rev_orphan";
    let mut fx = Fx::new(db).await;
    fx.index_md("Notes/o.md", "# 备忘\n\n内容\n").await;
    fx.llm_facts("Notes/o.md", 1, 6000).await;
    fx.publish().await;
    let it = pending(&mut fx, "low_confidence_fact").await;
    decide_items(
        &mut fx.rt,
        "tester",
        &[item(&it[0].0, it[0].1, Decision::Accept)],
    )
    .await
    .unwrap();
    fx.publish().await;
    assert_eq!(
        sweep_orphans(&mut fx.rt).await.unwrap(),
        0,
        "evidence still there"
    );
    // The file goes away: its evidence is withdrawn.
    sqlx::query(
        "UPDATE _oo_docs.evidence SET withdrawn_at = now(), withdrawn_reason = 'file_deleted'",
    )
    .execute(&mut fx.admin)
    .await
    .unwrap();
    assert_eq!(sweep_orphans(&mut fx.rt).await.unwrap(), 1);
    assert_eq!(sweep_orphans(&mut fx.rt).await.unwrap(), 0, "once");
    let orphans = pending(&mut fx, "orphaned_overlay").await;
    assert_eq!(orphans.len(), 1);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay_current WHERE status = 'orphaned'")
            .await,
        1
    );
    // Withdrawing it ends the overlay; keeping it makes it active again.
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[item(&orphans[0].0, orphans[0].1, Decision::Reject)],
    )
    .await
    .unwrap();
    assert_eq!(out.rejected, 1);
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay_current WHERE status = 'withdrawn'")
            .await,
        1
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn merge_integrity_failure_stops_the_whole_version() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_rev_g5";
    let mut fx = Fx::new(db).await;
    fx.index_md("People/甲.md", "# 甲\n\n内容\n").await;
    fx.index_md("People/乙.md", "# 乙\n\n内容\n").await;
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    assert!(v1.gates.iter().all(|g| g.disposition == "pass"));
    // A merged entity that still has a published fact: a structural defect.
    sqlx::query(
        "UPDATE _oo_docs.entity SET status = 'merged', merged_into = (SELECT id FROM _oo_docs.entity WHERE canonical_name = '乙') \
          WHERE canonical_name = '甲'",
    )
    .execute(&mut fx.admin)
    .await
    .unwrap();
    fx.index_md("People/丙.md", "# 丙\n\n内容\n").await;
    let err = publish(&mut fx.rt, &fx.cas, &PublishConfig::default())
        .await
        .unwrap_err();
    assert!(matches!(err, PublishError::Blocked(_)), "{err}");
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.ontology_version")
            .await,
        1,
        "no version was written"
    );
    fx.finish(db).await;
}
