//! Open type promotion against one throwaway PostgreSQL (B-S9-02, D16): a label with 5 instances in 3 files is
//! proposed once, only a person promotes, promoting changes the prompt version and sends the files holding the
//! label back through extraction.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test promotion_live -- --ignored --test-threads=1`

mod common;

use common::CLUSTER_LOCK;
use common::fx::Fx;
use oo_docs::extract::llm::prompt_version;
use oo_docs::review::decide::{Decision, ItemDecision, decide_items};
use oo_docs::review::promotion::{candidates, constrained_types, propose};
use serde_json::json;

async fn thing(fx: &mut Fx, label: &str, name: &str, path: &str) {
    let key = format!(
        "thing:{}",
        &blake3::hash(format!("{label}{name}").as_bytes()).to_hex()[..24]
    );
    sqlx::query(
        "INSERT INTO _oo_docs.entity (id, key, type, thing_label, canonical_name, norm_name) \
         VALUES (gen_random_uuid(), $1, 'Thing', $2, $3, lower($3)) ON CONFLICT (key) DO NOTHING",
    )
    .bind(&key)
    .bind(label)
    .bind(name)
    .execute(&mut fx.admin)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, quote_b3, origin) \
         SELECT gen_random_uuid(), 'entity', e.id, f.current_version_id, fv.b3, 'blk_00000000000000000000', \
                '{}'::jsonb, repeat('0', 64), 'llm' \
           FROM _oo_docs.entity e, _oo_docs.file f JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
          WHERE e.key = $1 AND f.path = $2",
    )
    .bind(&key)
    .bind(path)
    .execute(&mut fx.admin)
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn only_a_person_promotes_and_the_prompt_version_moves() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_promotion";
    let mut fx = Fx::new(db).await;
    for p in ["合同/a.md", "合同/b.md", "合同/c.md"] {
        fx.plant_md(p, &format!("# {p}\n\n内容 {p}\n")).await;
    }
    // 4 instances in 3 files, and 5 in only 2 files: below the thresholds.
    for (i, p) in ["合同/a.md", "合同/b.md", "合同/c.md", "合同/a.md"]
        .iter()
        .enumerate()
    {
        thing(&mut fx, "合同", &format!("合同{i}"), p).await;
    }
    for i in 0..5 {
        thing(
            &mut fx,
            "发票",
            &format!("发票{i}"),
            ["合同/a.md", "合同/b.md"][i % 2],
        )
        .await;
    }
    assert!(candidates(&mut fx.rt).await.unwrap().is_empty());
    assert_eq!(propose(&mut fx.rt).await.unwrap(), 0);

    // The fifth contract tips it.
    thing(&mut fx, "合同", "合同4", "合同/b.md").await;
    let c = candidates(&mut fx.rt).await.unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(
        (c[0].label.as_str(), c[0].instances, c[0].files),
        ("合同", 5, 3)
    );
    assert_eq!(propose(&mut fx.rt).await.unwrap(), 1);
    assert_eq!(propose(&mut fx.rt).await.unwrap(), 0, "once per label");
    // Nothing promotes by itself.
    assert!(constrained_types(&mut fx.rt).await.unwrap().is_empty());
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'type_promote'")
            .await,
        0
    );

    let (id, revision): (String, i64) = sqlx::query_as(
        "SELECT id::text, revision FROM _oo_docs.review_item WHERE kind = 'type_promotion'",
    )
    .fetch_one(&mut fx.admin)
    .await
    .unwrap();
    let decide = |edit: Option<serde_json::Value>| ItemDecision {
        id: id.clone(),
        revision,
        decision: Decision::Accept,
        edit,
        note: None,
    };
    // A core type name is refused.
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[decide(Some(json!({"type_name": "Person"})))],
    )
    .await
    .unwrap();
    assert_eq!(out.results[0].status, "invalid");
    // Files hold published state before the promotion.
    sqlx::query("UPDATE _oo_docs.file SET state = 'published' WHERE path LIKE '合同/%'")
        .execute(&mut fx.admin)
        .await
        .unwrap();
    let before = prompt_version(&constrained_types(&mut fx.rt).await.unwrap());
    let out = decide_items(
        &mut fx.rt,
        "tester",
        &[decide(Some(json!({"type_name": "Contract"})))],
    )
    .await
    .unwrap();
    assert_eq!(out.results[0].status, "ok", "{:?}", out.results[0]);
    let types = constrained_types(&mut fx.rt).await.unwrap();
    assert_eq!(types.len(), 1);
    assert_eq!(
        (types[0].label.as_str(), types[0].type_name.as_str()),
        ("合同", "Contract")
    );
    assert_ne!(prompt_version(&types), before, "the prompt version moved");
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'outdated'")
            .await,
        3,
        "the files that hold the label go back through extraction"
    );
    // Promoted labels are not proposed again.
    assert!(candidates(&mut fx.rt).await.unwrap().is_empty());
    fx.finish(db).await;
}
