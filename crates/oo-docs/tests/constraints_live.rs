//! Integrity constraints on a live PostgreSQL (B-S5-11, D57): the four core
//! constraints, partial blocking, `constraint_conflict` items, resolution by a
//! person, and rule-certain supersession that is not a violation.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test constraints_live -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::{Fx, uuid};
use oo_docs::publish::constraints::{duplicate_identities, rejected_not_adopted};
use oo_docs::publish::run::PublishOutcome;
use oo_docs::review::decide::{Decision, ItemDecision, decide_items};
use serde_json::json;

fn proj(owner: &str, date: &str) -> String {
    format!("---\ndate: {date}\nowner: {owner}\n---\n\n# 星图\n\n内容\n")
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn project_single_owner_blocks_the_loser_and_lets_the_rest_publish() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_con_owner";
    let mut fx = Fx::new(db).await;
    fx.index_md("Projects/星图/一.md", &proj("陈子墨", "2026-01-01"))
        .await;
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    assert!(v1.gates.iter().all(|g| g.disposition == "pass"));

    // Same start day, another owner, plus an unrelated file that is fine.
    fx.index_md("Projects/星图/二.md", &proj("周若溪", "2026-01-01"))
        .await;
    fx.index_md(
        "Projects/灯塔/一.md",
        "---\ndate: 2026-02-01\nowner: 韩立\n---\n\n# 灯塔\n\n内容\n",
    )
    .await;
    let PublishOutcome::Published(v2) = fx.publish().await else {
        panic!("v2")
    };
    let held = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.predicate = 'owns' AND o.canonical_name = '星图' AND f.status = 'published'")
        .await;
    assert_eq!(held, 1, "one owner at a time");
    assert!(
        fx.scalar("SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.predicate = 'owns' AND o.canonical_name = '灯塔' AND f.status = 'published'")
            .await
            == 1,
        "the other project published"
    );
    assert!(v2.facts_added > 0);
    assert_eq!(
        fx.text("SELECT constraint_id || '/' || status FROM _oo_docs.constraint_violation")
            .await,
        ["project_single_owner/open"]
    );
    assert_eq!(
        fx.text("SELECT kind || '/' || status || '/' || reason_code FROM _oo_docs.review_item WHERE kind = 'constraint_conflict'").await,
        ["constraint_conflict/pending/constraint:project_single_owner"]
    );
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.event WHERE type = 'ontology.constraint_violated'"
        )
        .await,
        1
    );
    // A second run adds nothing new.
    fx.index_md("Notes/n.md", "# 备忘\n\n内容\n").await;
    fx.publish().await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.constraint_violation")
            .await,
        1
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'constraint_conflict'")
            .await,
        1
    );

    // A person picks the other owner: it is asserted, the first is rejected.
    let (item, rev): (String, i64) = sqlx::query_as(
        "SELECT id::text, revision FROM _oo_docs.review_item WHERE kind = 'constraint_conflict'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    let loser: String = sqlx::query_scalar(
        "SELECT f.id::text FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
          WHERE f.predicate = 'owns' AND s.canonical_name = '周若溪'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    let bad = ItemDecision {
        edit: Some(json!({"winner": "00000000-0000-4000-8000-00000000abcd"})),
        id: item.clone(),
        revision: rev,
        decision: Decision::Accept,
        note: None,
    };
    let out = decide_items(&mut fx.rt, "tester", &[bad]).await.unwrap();
    assert_eq!(out.results[0].code, Some("DOCS_REVIEW_CONSTRAINT_CONFLICT"));
    let ok = ItemDecision {
        edit: Some(json!({"winner": loser})),
        id: item,
        revision: rev,
        decision: Decision::Accept,
        note: None,
    };
    let out = decide_items(&mut fx.rt, "tester", &[ok]).await.unwrap();
    assert_eq!(out.accepted, 1, "{out:?}");
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.constraint_violation WHERE status = 'open'")
            .await,
        0
    );
    fx.publish().await;
    assert_eq!(
        fx.text("SELECT s.canonical_name FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id JOIN _oo_docs.entity o ON o.id = f.object_id WHERE f.predicate = 'owns' AND o.canonical_name = '星图' AND f.status = 'published'").await,
        ["周若溪"]
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn rule_certain_supersession_is_not_a_violation_and_decisions_have_one_status() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_con_super";
    let mut fx = Fx::new(db).await;
    fx.index_md("Projects/星图/一.md", &proj("陈子墨", "2026-01-01"))
        .await;
    fx.publish().await;
    // A later owner supersedes by date: no violation.
    fx.index_md("Projects/星图/二.md", &proj("周若溪", "2026-06-01"))
        .await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("v2")
    };
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.constraint_violation")
            .await,
        0
    );
    assert_eq!(
        fx.text("SELECT valid_to::text FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id WHERE f.predicate = 'owns' AND s.canonical_name = '陈子墨'").await,
        ["2026-06-01"]
    );

    // A Decision with two statuses on the same day.
    for (path, status) in [
        ("Decisions/定价/甲.md", "已采纳"),
        ("Decisions/定价/乙.md", "已搁置"),
    ] {
        fx.index_md(
            path,
            &format!(
                "---\ntype: decision\ndate: 2026-03-01\nstatus: {status}\n---\n\n# 定价\n\n内容\n"
            ),
        )
        .await;
    }
    fx.publish().await;
    assert_eq!(
        fx.text("SELECT constraint_id FROM _oo_docs.constraint_violation")
            .await,
        ["decision_single_status"]
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn identity_unique_person_and_rejected_not_adopted() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_con_identity";
    let mut fx = Fx::new(db).await;
    fx.index_md("People/甲.md", "# 甲\n\n内容\n").await;
    fx.index_md("People/乙.md", "# 乙\n\n内容\n").await;
    sqlx::query("UPDATE _oo_docs.entity SET identity = jsonb_build_object('email', 'x@y.io', 'employee_id', CASE canonical_name WHEN '甲' THEN '1' ELSE '2' END) WHERE canonical_name IN ('甲', '乙')")
        .execute(&mut fx.admin)
        .await
        .unwrap();
    assert_eq!(duplicate_identities(&mut fx.rt).await.unwrap().len(), 1);
    fx.publish().await;
    assert_eq!(
        fx.text("SELECT constraint_id FROM _oo_docs.constraint_violation")
            .await,
        ["identity_unique_person"]
    );

    // rejected_not_adopted: P decides X (published), Q rejects X (incoming).
    let ent = |t: &'static str, name: &'static str| {
        let id = uuid();
        (id, t, name)
    };
    let (p, x, q) = (
        ent("Person", "丙"),
        ent("Decision", "方案X"),
        ent("Person", "丁"),
    );
    for (id, t, name) in [&p, &x, &q] {
        sqlx::query("INSERT INTO _oo_docs.entity (id, key, type, canonical_name, norm_name) VALUES ($1::uuid, $2, $3, $4, $4)")
            .bind(id)
            .bind(format!("{}:{}", t.to_lowercase(), &id.replace('-', "")[8..32]))
            .bind(*t)
            .bind(*name)
            .execute(&mut fx.admin)
            .await
            .unwrap();
    }
    let (decides, rejects) = (uuid(), uuid());
    for (id, pred, subj, status) in [
        (&decides, "decides", &p.0, "published"),
        (&rejects, "rejects", &q.0, "draft"),
    ] {
        sqlx::query(
            "INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, object_id, original_object_id, \
                valid_from, confidence_bp, origin, status) VALUES ($1::uuid, $2, $3::uuid, $3::uuid, $4, $5::uuid, $5::uuid, '2026-01-01', 9000, 'rule', $6)",
        )
        .bind(id)
        .bind(id.replace('-', "").repeat(2))
        .bind(subj)
        .bind(pred)
        .bind(&x.0)
        .bind(status)
        .execute(&mut fx.admin)
        .await
        .unwrap();
    }
    let hits = rejected_not_adopted(&mut fx.rt, std::slice::from_ref(&rejects))
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        (hits[0].0.as_str(), hits[0].1.as_str()),
        (rejects.as_str(), decides.as_str())
    );
    // Not overlapping in time: no violation.
    sqlx::query("UPDATE _oo_docs.fact SET valid_to = '2026-02-01' WHERE id = $1::uuid")
        .bind(&decides)
        .execute(&mut fx.admin)
        .await
        .unwrap();
    sqlx::query("UPDATE _oo_docs.fact SET valid_from = '2026-03-01' WHERE id = $1::uuid")
        .bind(&rejects)
        .execute(&mut fx.admin)
        .await
        .unwrap();
    assert!(
        rejected_not_adopted(&mut fx.rt, &[rejects])
            .await
            .unwrap()
            .is_empty()
    );
    fx.finish(db).await;
}
