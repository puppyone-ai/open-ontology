//! L5 Suggest against one throwaway PostgreSQL (10 §11.1, B-S7-03, B-S7-07, B-S8-01): the four impact rules, the
//! idempotent resubmission, the cross-share impact and the update broadcast on accept, undo, the viewer refusal,
//! the file-level diff draft and the digest.
//!
//! Needs migration 0143 (the coordinator's patch `B-L5-migration-docs-suggest.patch`).
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test suggest_live -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::Fx;
use oo_docs::publish::run::PublishOutcome;
use oo_docs::suggest::schema::validate;
use oo_docs::suggest::store::{self, ShareCtx};
use oo_docs::suggest::{diff, digest, impact};
use serde_json::{Value, json};

const DOC: &str =
    "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n状态：进行中\n";
const PATH: &str = "Projects/星火计划.md";

struct World {
    fx: Fx,
    sid: String,
    ctx: ShareCtx,
}

async fn world(db: &str, role: &str) -> World {
    let mut fx = Fx::new(db).await;
    fx.index_md(PATH, DOC).await;
    let PublishOutcome::Published(v1) = fx.publish().await else {
        panic!("v1")
    };
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
         SELECT gen_random_uuid(), c.id, c.drive_id, '[\"Projects\"]'::jsonb, $1, 'task A', 2000, 'active', 'g', 'm', \
                now() + interval '1 day' FROM _oo_docs.context_pack c",
    )
    .bind(role)
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
    let ctx = ShareCtx {
        id: sid.clone(),
        drive_id: sqlx::query_scalar("SELECT drive_id::text FROM _oo_docs.share")
            .fetch_one(&mut fx.admin)
            .await
            .unwrap(),
        role: role.into(),
        scope: vec!["Projects".into()],
    };
    World { fx, sid, ctx }
}

fn rec(v: Value) -> oo_docs::suggest::schema::Suggestion {
    validate(&v).unwrap()
}

fn ev() -> Value {
    json!([{"kind": "brief_line", "ref": format!("[src:{PATH}]")}])
}

async fn submit(w: &mut World, v: Value) -> Value {
    let s = rec(v);
    store::submit(&mut w.fx.rt, &w.ctx, &s, None, "collector")
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_four_rules_and_the_broadcast() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut w = world("oo_docs_suggest_rules", "suggester").await;
    let status_key: String = sqlx::query_scalar(
        "SELECT fact_key FROM _oo_docs.fact WHERE predicate = 'attr.status' AND status = 'published'",
    )
    .fetch_one(&mut w.fx.admin)
    .await
    .unwrap();

    // Rule 1: a new fact with evidence inside the scope is auto accepted and becomes a published fact.
    let a = submit(
        &mut w,
        json!({"kind": "new_fact", "subject": "星火计划", "predicate": "attr.deadline", "value": "2026-12-31",
               "rationale": "周会", "evidence": ev()}),
    )
    .await;
    assert_eq!(a["status"], "auto_accepted", "{a}");
    assert_eq!(a["rule"], "new_fact");
    let fid = a["fact_id"].as_str().unwrap().to_owned();
    let origin: String = sqlx::query_scalar("SELECT origin FROM _oo_docs.fact WHERE id = $1::uuid")
        .bind(&fid)
        .fetch_one(&mut w.fx.admin)
        .await
        .unwrap();
    assert_eq!(origin, "suggest");
    let ev_n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.evidence WHERE target_id = $1::uuid AND origin = 'suggest'",
    )
    .bind(&fid)
    .fetch_one(&mut w.fx.admin)
    .await
    .unwrap();
    assert!(ev_n >= 1, "the cited block is copied as evidence (DOC-G2)");
    // The same record again changes nothing.
    let again = submit(
        &mut w,
        json!({"kind": "new_fact", "subject": "星火计划", "predicate": "attr.deadline", "value": "2026-12-31",
               "rationale": "周会", "evidence": ev()}),
    )
    .await;
    assert_eq!(again["replayed"], true);
    assert_eq!(again["id"], a["id"]);
    assert_eq!(
        w.fx.scalar("SELECT count(*) FROM _oo_docs.suggestion")
            .await,
        1
    );
    assert!(matches!(w.fx.publish().await, PublishOutcome::Published(_)));
    assert_eq!(
        w.fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.fact WHERE id = '{fid}' AND status = 'published'"
        ))
        .await,
        1,
        "the auto accepted fact publishes"
    );

    // A new fact without evidence in scope waits for a person.
    let noev = submit(
        &mut w,
        json!({"kind": "new_fact", "subject": "星火计划", "predicate": "attr.phase", "value": "试运行", "rationale": "听说",
               "evidence": [{"kind": "external", "ref": "https://example.invalid"}]}),
    )
    .await;
    assert_eq!(noev["status"], "pending");
    assert_eq!(noev["reason"], "suggestion_no_scope_evidence");

    // A functional conflict (attr.status already has a value) is a person's call.
    let conflict = submit(
        &mut w,
        json!({"kind": "new_fact", "subject": "星火计划", "predicate": "attr.status", "value": "已暂停", "rationale": "x",
               "evidence": ev()}),
    )
    .await;
    assert_eq!(conflict["status"], "pending");
    assert_eq!(conflict["reason"], "suggestion_functional_conflict");

    // Rule 2: superseding a fact no Share used is silent. The deadline fact above was made after the share.
    let deadline_key =
        w.fx.text(&format!(
            "SELECT fact_key FROM _oo_docs.fact WHERE id = '{fid}'"
        ))
        .await[0]
            .clone();
    let silent = submit(
        &mut w,
        json!({"kind": "supersede", "subject": "星火计划", "predicate": "attr.deadline", "value": "2027-01-31",
               "valid_from": "2026-12-01", "supersedes": deadline_key,
               "rationale": "延期", "evidence": ev()}),
    )
    .await;
    assert_eq!(silent["status"], "superseded_silently", "{silent}");
    assert_eq!(silent["rule"], "supersede_unused");
    assert_eq!(
        w.fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.fact WHERE id = '{fid}' AND valid_to = '2026-12-01'"
        ))
        .await,
        1,
        "the old value is ended where the new one starts (D17)"
    );

    // Rule 3: superseding a fact a Share used is a review item that lists the Share.
    let used = submit(
        &mut w,
        json!({"kind": "supersede", "subject": "星火计划", "predicate": "attr.status", "value": "已暂停",
               "valid_from": "2026-12-01", "supersedes": status_key, "rationale": "负责人说暂停", "evidence": ev()}),
    )
    .await;
    assert_eq!(used["status"], "pending", "{used}");
    assert_eq!(used["rule"], "supersede_in_use");
    assert_eq!(used["impact"]["shares"][0]["share_id"], w.sid.as_str());
    assert_eq!(
        w.fx.scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'suggestion' AND status = 'pending'").await,
        3
    );

    // Accepting tells the Share: one `suggestion_accepted` entry, level warn, in the same transaction.
    let before =
        w.fx.scalar("SELECT count(*) FROM _oo_docs.share_update")
            .await;
    let id = used["id"].as_str().unwrap();
    let done = store::decide(&mut w.fx.rt, id, "accept", Some(1), "api", None)
        .await
        .unwrap();
    assert_eq!(done["status"], "accepted");
    assert_eq!(
        w.fx.scalar("SELECT count(*) FROM _oo_docs.share_update")
            .await,
        before + 1
    );
    let lvl: String = sqlx::query_scalar(
        "SELECT level FROM _oo_docs.share_update WHERE summary_key = 'suggestion_accepted'",
    )
    .fetch_one(&mut w.fx.admin)
    .await
    .unwrap();
    assert_eq!(lvl, "warn");
    assert_eq!(
        w.fx.scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'suggestion' AND status = 'accepted'").await,
        1,
        "the review item closes with the suggestion"
    );
    // A second decision is refused.
    let e = store::decide(&mut w.fx.rt, id, "accept", None, "api", None)
        .await
        .unwrap_err();
    assert_eq!(e.code.as_str(), "DOCS_SUGGEST_NOT_PENDING");

    // Rule 4: preference and type change are always a person's.
    for (k, body) in [
        (
            "preference",
            json!({"kind": "preference", "subject": "团队", "predicate": "prefers", "value": "短句", "rationale": "r"}),
        ),
        (
            "type_change",
            json!({"kind": "type_change", "subject": "合同", "predicate": "type", "value": "Document", "rationale": "r"}),
        ),
    ] {
        let p = submit(&mut w, body).await;
        assert_eq!(p["status"], "pending", "{k}");
        assert_eq!(p["rule"], "manual");
        let ok = store::decide(
            &mut w.fx.rt,
            p["id"].as_str().unwrap(),
            "accept",
            None,
            "api",
            None,
        )
        .await
        .unwrap();
        assert_eq!(ok["status"], "accepted");
    }
    assert_eq!(
        w.fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'suggestion_accept'")
            .await,
        2
    );

    // Undo of an auto accepted suggestion rejects its fact.
    let undone = store::decide(
        &mut w.fx.rt,
        a["id"].as_str().unwrap(),
        "undo",
        None,
        "api",
        None,
    )
    .await
    .unwrap();
    assert_eq!(undone["status"], "undone");
    assert_eq!(
        w.fx.scalar(&format!("SELECT count(*) FROM _oo_docs.fact WHERE id = '{fid}' AND status IN ('retracted','rejected')")).await,
        1
    );

    // The digest of the day and the diff draft.
    let d = digest::day(&mut w.fx.rt, None).await.unwrap();
    assert_eq!(d["total"], 7);
    assert_eq!(d["counts"]["pending"], 2);
    let dr = diff::draft(&mut w.fx.rt, id).await.unwrap();
    assert_eq!(
        dr["writable"], false,
        "a suggester share cannot write files"
    );
    assert_eq!(dr["writer"], "desktop");
    assert_eq!(dr["changes"][0]["path"], PATH);
    assert_eq!(dr["changes"][0]["operation"], "replace");
    assert!(
        dr["changes"][0]["after"]
            .as_str()
            .unwrap()
            .contains("已暂停")
    );

    // impact::affected_shares sees the share for any fact it used.
    let used_ids =
        w.fx.text("SELECT fact_id::text FROM _oo_docs.share_fact_use LIMIT 1")
            .await;
    assert_eq!(
        impact::affected_shares(&mut w.fx.rt, &used_ids)
            .await
            .unwrap()
            .len(),
        1
    );
    w.fx.finish("oo_docs_suggest_rules").await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_viewer_cannot_suggest_and_an_editor_can() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut w = world("oo_docs_suggest_roles", "viewer").await;
    let e = store::load_share(&mut w.fx.rt, &w.sid).await.unwrap_err();
    assert_eq!(e.code.as_str(), "DOCS_SUGGEST_ROLE_FORBIDDEN");
    w.fx.admin_sql("UPDATE _oo_docs.share SET role = 'editor'")
        .await;
    let ok = store::load_share(&mut w.fx.rt, &w.sid).await.unwrap();
    assert_eq!(ok.role, "editor");
    let a = submit(
        &mut w,
        json!({"kind": "new_fact", "subject": "星火计划", "predicate": "attr.deadline", "value": "2026-12-31",
               "rationale": "周会", "evidence": ev()}),
    )
    .await;
    let dr = diff::draft(&mut w.fx.rt, a["id"].as_str().unwrap())
        .await
        .unwrap();
    assert_eq!(
        dr["writable"], true,
        "only an editor share's change may be written back, by Desktop"
    );
    w.fx.finish("oo_docs_suggest_roles").await;
}
