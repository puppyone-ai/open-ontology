//! Supersession against one throwaway PostgreSQL (B-S5-05): functional
//! predicates end the older value in place, rule-certain decision supersession
//! cuts the old decision's facts, candidates and in-use facts wait for a
//! person, and a future `valid_to` never ends a fact.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test temporal_live -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::{DRIVE, Fx, uuid};
use oo_docs::publish::run::PublishOutcome;
use oo_docs::temporal::ReviewReason;
use oo_docs::temporal::pg::{queue_held, reconcile};

async fn drafts(fx: &mut Fx) -> Vec<String> {
    fx.text("SELECT id::text FROM _oo_docs.fact WHERE status = 'draft' ORDER BY id")
        .await
}

async fn status_rows(fx: &mut Fx, predicate: &str) -> Vec<String> {
    fx.text(&format!(
        "SELECT valid_from::text || '..' || coalesce(valid_to::text, '') || ' ' || \
                coalesce(value->>'text', o.canonical_name) \
           FROM _oo_docs.fact f LEFT JOIN _oo_docs.entity o ON o.id = f.object_id \
          WHERE f.predicate = '{predicate}' ORDER BY f.valid_from, f.id"
    ))
    .await
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn status_and_owner_supersession_in_place() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_tmp_functional";
    let mut fx = Fx::new(db).await;
    fx.index_md(
        "Projects/星图/一月.md",
        "---\ndate: 2026-01-01\nstatus: 进行中\nowner: 陈子墨\n---\n\n# 星图\n\n内容\n",
    )
    .await;
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    assert_eq!(v1.n, 1);
    // A later document gives a newer status and owner.
    fx.index_md(
        "Projects/星图/六月.md",
        "---\ndate: 2026-06-01\nstatus: 完成\nowner: 周若溪\n---\n\n# 星图\n\n内容\n",
    )
    .await;
    let ids = drafts(&mut fx).await;
    let r = reconcile(&mut fx.rt, &ids).await.unwrap();
    assert_eq!(r.ended.len(), 2, "{r:?}");
    assert!(r.held.is_empty() && r.excluded.is_empty());
    assert_eq!(
        status_rows(&mut fx, "attr.status").await,
        ["2026-01-01..2026-06-01 进行中", "2026-06-01.. 完成"]
    );
    assert_eq!(
        status_rows(&mut fx, "owns").await,
        ["2026-01-01..2026-06-01 星图", "2026-06-01.. 星图"],
        "owns shows the project as object; the owners differ by subject"
    );
    let chain = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE superseded_by IS NOT NULL")
        .await;
    assert_eq!(chain, 2);
    // Running it again changes nothing (the ended facts are not ended twice).
    let again = reconcile(&mut fx.rt, &ids).await.unwrap();
    assert!(again.ended.is_empty(), "{again:?}");
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn equal_start_conflict_and_future_dates() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_tmp_conflict";
    let mut fx = Fx::new(db).await;
    fx.index_md(
        "Projects/星图/甲.md",
        "---\ndate: 2026-03-01\nstatus: 进行中\n---\n\n# 星图\n\n内容\n",
    )
    .await;
    fx.index_md(
        "Projects/星图/乙.md",
        "---\ndate: 2026-03-01\nstatus: 暂停\n---\n\n# 星图\n\n内容\n",
    )
    .await;
    let ids = drafts(&mut fx).await;
    let r = reconcile(&mut fx.rt, &ids).await.unwrap();
    assert!(r.ended.is_empty());
    assert_eq!(r.held.len(), 1);
    assert_eq!(r.held[0].reason, ReviewReason::FunctionalConflict);
    assert_eq!(r.excluded.len(), 1);
    assert!(r.excluded.contains(&r.held[0].fact_id));
    assert_eq!(queue_held(&mut fx.rt, &r.held).await.unwrap(), 1);
    assert_eq!(
        queue_held(&mut fx.rt, &r.held).await.unwrap(),
        0,
        "deduplicated"
    );
    let reasons = fx
        .text("SELECT reason_code FROM _oo_docs.review_item WHERE kind = 'low_confidence_fact'")
        .await;
    assert_eq!(reasons, ["functional_conflict"]);
    // The item knows its source file.
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.review_item WHERE source_file_id IS NOT NULL AND drive_id IS NOT NULL")
            .await,
        1
    );

    // A fact that says it ends in the future is not ended by anything.
    fx.index_md(
        "Projects/星图/丙.md",
        "# 星图\n\n- 2026-01-01 至 2099-12-31 由陈子墨负责\n",
    )
    .await;
    let ids = drafts(&mut fx).await;
    let r = reconcile(&mut fx.rt, &ids).await.unwrap();
    assert!(r.ended.is_empty());
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE valid_to = '2099-12-31'")
            .await,
        1
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn decision_supersession_candidates_and_shares_in_use() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_tmp_decisions";
    let mut fx = Fx::new(db).await;
    fx.index_md(
        "Decisions/采用方案A.md",
        "---\ntype: decision\ndecided_by: 陈子墨\ndate: 2026-01-10\n---\n\n# 采用方案A\n\n内容\n",
    )
    .await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("v1")
    };
    fx.index_md(
        "Decisions/改用方案B.md",
        "---\ntype: decision\ndecided_by: 陈子墨\ndate: 2026-05-01\nsupersedes: 采用方案A\n---\n\n# 改用方案B\n\n内容\n",
    )
    .await;
    let ids = drafts(&mut fx).await;
    let r = reconcile(&mut fx.rt, &ids).await.unwrap();
    assert_eq!(r.ended.len(), 1, "{r:?}");
    assert_eq!(
        status_rows(&mut fx, "decides").await,
        ["2026-01-10..2026-05-01 采用方案A", "2026-05-01.. 改用方案B"]
    );

    // name.version only proposes, and it is kept out of this run.
    fx.index_md("Proposals/方案.md", "# 方案\n\n内容\n").await;
    fx.index_md("Proposals/方案-v2.md", "# 方案\n\n内容\n")
        .await;
    let ids = drafts(&mut fx).await;
    let r = reconcile(&mut fx.rt, &ids).await.unwrap();
    let cand: Vec<_> = r
        .held
        .iter()
        .filter(|h| h.reason == ReviewReason::SupersedeCandidate)
        .collect();
    assert_eq!(cand.len(), 1, "{r:?}");
    assert!(r.excluded.contains(&cand[0].fact_id));
    fx.finish(db).await;

    // A live Share that used the old decision's fact turns the cut into a prompt.
    let db = "oo_docs_tmp_inuse";
    let mut fx = Fx::new(db).await;
    fx.index_md(
        "Decisions/采用方案A.md",
        "---\ntype: decision\ndecided_by: 陈子墨\ndate: 2026-01-10\n---\n\n# 采用方案A\n\n内容\n",
    )
    .await;
    fx.publish().await;
    let old_fact = fx
        .text("SELECT id::text FROM _oo_docs.fact WHERE predicate = 'decides'")
        .await
        .pop()
        .unwrap();
    let (pack, share) = (uuid(), uuid());
    for sql in [
        format!(
            "INSERT INTO _oo_docs.context_pack (id, version_n, drive_id, scope_sha256, params_sha256, compiler, \
               pack_digest, snapshot_sha256, manifest, tokens_brief, trimmed, blocked_facts, blocked_entities) \
             VALUES ('{pack}', 1, '{DRIVE}', 's', 'p', 'c', '{h}', 's', '{{}}', 1, '{{}}', 0, 0)",
            h = "a".repeat(64)
        ),
        format!(
            "INSERT INTO _oo_docs.share (id, pack_id, drive_id, scope, role, budget, status, grantor, mount_ref, expires_at) \
             VALUES ('{share}', '{pack}', '{DRIVE}', '{{}}', 'viewer', 1000, 'active', 'owner', 'm', now() + interval '1 day')"
        ),
        format!(
            "INSERT INTO _oo_docs.share_fact_use (share_id, fact_id, section, precision) \
             VALUES ('{share}', '{old_fact}', 'decisions', 'full')"
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut fx.admin)
            .await
            .unwrap();
    }
    fx.index_md(
        "Decisions/改用方案B.md",
        "---\ntype: decision\ndecided_by: 陈子墨\ndate: 2026-05-01\nsupersedes: 采用方案A\n---\n\n# 改用方案B\n\n内容\n",
    )
    .await;
    let ids = drafts(&mut fx).await;
    let r = reconcile(&mut fx.rt, &ids).await.unwrap();
    assert!(
        !r.ended.is_empty(),
        "the rule-certain cut runs even for a used fact: {r:?}"
    );
    assert_eq!(r.held.len(), 1);
    assert_eq!(
        r.held[0].reason,
        ReviewReason::SupersedeInUse,
        "one notice for a person"
    );
    assert!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact WHERE valid_to IS NOT NULL AND superseded_by IS NOT NULL")
            .await
            >= 1,
        "the old fact was cut"
    );
    fx.finish(db).await;
}
