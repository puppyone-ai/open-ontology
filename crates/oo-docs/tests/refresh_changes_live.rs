//! The store side of "自上次以来的变化" (B-S10-01) against one throwaway PostgreSQL: after a change of the ontology
//! the changes since an old Share's version are listed (added, changed, retracted), nothing when there is no newer
//! version.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test refresh_changes_live -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::Fx;
use oo_docs::compile::brief::ChangeKind;
use oo_docs::publish::reprocess::index_gated;
use oo_docs::publish::run::PublishOutcome;
use oo_docs::publish::write::DocumentInput;
use oo_docs::share::changes::compute;

const V1: &str = "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n状态：进行中\n成员：甲甲、乙乙\n";
const V2: &str = "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n状态：已暂停\n成员：甲甲、乙乙、丙丙\n";

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn changes_since_the_old_shares_version_are_listed_in_order() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_refresh_changes";
    let mut fx = Fx::new(db).await;
    let path = "Projects/星火计划.md";
    let p = fx.plant_md(path, V1).await;
    let doc = DocumentInput {
        version_id: &p.version_id,
        b3: &p.b3,
        path,
        ir: &p.ir,
    };
    index_gated(&mut fx.rt, &doc).await.unwrap();
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
    // A share whose pack used every fact of v1.
    sqlx::query(
        "INSERT INTO _oo_docs.context_pack (id, version_n, drive_id, scope_sha256, params_sha256, compiler, \
            pack_digest, snapshot_sha256, manifest, tokens_brief, trimmed, blocked_facts, blocked_entities) \
         VALUES (gen_random_uuid(), $1, (SELECT id FROM _oo_docs.drive LIMIT 1), repeat('0', 64), repeat('0', 64), \
                 'c', repeat('0', 64), repeat('0', 64), '{}'::jsonb, 1, '{}'::jsonb, 0, 0)",
    )
    .bind(v1.n)
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
         SELECT s.id, f.id, 'state', 'full' FROM _oo_docs.share s, _oo_docs.fact f WHERE f.status = 'published'",
    )
    .execute(&mut fx.admin)
    .await
    .unwrap();
    let sid: String = sqlx::query_scalar("SELECT id::text FROM _oo_docs.share")
        .fetch_one(&mut fx.admin)
        .await
        .unwrap();
    assert!(
        compute(&mut fx.rt, &sid, None).await.unwrap().is_none(),
        "no newer version, no changes"
    );

    // The upgraded extraction changes the status and adds a member (small enough for DOC-G10).
    let mut n = oo_docs::parse::markdown::parse_markdown(V2.as_bytes()).unwrap();
    n.ir.blob_b3 = p.b3.clone();
    let put = fx
        .cas
        .put(
            oo_docs::cas::CasKind::Ir,
            oo_builder::canonical_json(&serde_json::to_value(&n.ir).unwrap()).as_bytes(),
        )
        .unwrap();
    sqlx::query(
        "INSERT INTO _oo_docs.parse_result (b3, parser_id, parser_version, profile_sha256, status, ir_b3, \
            block_count, finished_at) VALUES ($1, 'oo-md', '2', $2, 'complete', $3, 5, clock_timestamp() + interval '1 minute')",
    )
    .bind(&p.b3)
    .bind("0".repeat(64))
    .bind(&put.b3)
    .execute(&mut fx.admin)
    .await
    .unwrap();
    let doc = DocumentInput {
        version_id: &p.version_id,
        b3: &p.b3,
        path,
        ir: &n.ir,
    };
    let r = index_gated(&mut fx.rt, &doc).await.unwrap();
    // Whatever the gate decided, apply it for the test: a held diff is accepted by a person in real life.
    if let oo_docs::publish::reprocess::Reprocess::Held { diff, .. } = &r {
        let subject = serde_json::json!({"version_id": p.version_id, "removed": diff.removed, "added": diff.added});
        oo_docs::publish::reprocess::apply(&mut fx.rt, &subject)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE _oo_docs.review_item SET status = 'accepted' WHERE kind = 'reprocess_diff'",
        )
        .execute(&mut fx.admin)
        .await
        .unwrap();
    }
    let PublishOutcome::Published(v2) = fx.publish().await else {
        panic!("v2")
    };
    assert!(v2.n > v1.n);
    let ch = compute(&mut fx.rt, &sid, None)
        .await
        .unwrap()
        .expect("changes");
    assert_eq!(ch.from_version, v1.n);
    let kinds: Vec<ChangeKind> = ch.changes.iter().map(|c| c.kind).collect();
    assert!(
        kinds.contains(&ChangeKind::Changed),
        "the status changed: {kinds:?}"
    );
    assert!(
        kinds.contains(&ChangeKind::Added),
        "a member was added: {kinds:?}"
    );
    let mut sorted = kinds.clone();
    sorted.sort();
    assert_eq!(kinds, sorted, "added, changed, retracted");
    let changed = ch
        .changes
        .iter()
        .find(|c| c.kind == ChangeKind::Changed)
        .unwrap();
    assert_eq!(
        changed.old.as_ref().unwrap().value.as_deref(),
        Some("进行中")
    );
    fx.finish(db).await;
}
