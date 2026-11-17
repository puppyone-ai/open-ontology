//! Resolve against one throwaway PostgreSQL (B-S5-04): identity binding and
//! merging, contradicting identities, merge and redirect (depth at most one),
//! split, merge proposals and the title-free alias candidates.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test resolve_live -- --ignored --test-threads=1`

mod common;

use common::fx::Fx;
use common::*;
use oo_docs::resolve::merge::{MergeError, merge_entities, split_merged};

const NEEDS_PG: &str = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)";

async fn key_of(fx: &mut Fx, name: &str) -> String {
    fx.text(&format!(
        "SELECT key FROM _oo_docs.entity WHERE canonical_name = '{name}' AND type <> 'Document'"
    ))
    .await
    .pop()
    .unwrap_or_else(|| panic!("no entity {name}"))
}

fn people_csv(rows: &[&str]) -> String {
    let mut s = String::from("姓名,邮箱,部门\n");
    for r in rows {
        s.push_str(r);
        s.push('\n');
    }
    s
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn identity_binds_and_duplicates_merge() {
    let _ = NEEDS_PG;
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_res_identity";
    let mut fx = Fx::new(db).await;
    // 1. A person with an email.
    let s = fx
        .index_csv("HR/a.csv", &people_csv(&["Ethan Chen,ethan@x.io,研发"]))
        .await;
    assert_eq!(s.identities_added, 1);
    // 2. Another name with the same email binds to the same entity (step 2).
    fx.index_csv("HR/b.csv", &people_csv(&["陈子墨,ETHAN@x.io,研发"]))
        .await;
    let names = fx
        .text("SELECT canonical_name || '/' || identity::text FROM _oo_docs.entity WHERE type = 'Person'")
        .await;
    assert_eq!(names.len(), 1, "the email decides, not the name: {names:?}");
    // 3. Two rows of one file name the same email under two names: neither
    // exists yet, so both are created; they share an identity and merge
    // (reason identity), the smaller key survives.
    let s = fx
        .index_csv(
            "HR/d.csv",
            &people_csv(&["周若溪,ruby@x.io,产品", "Ruby Zhou,RUBY@x.io,产品"]),
        )
        .await;
    assert_eq!(s.identity_merges, 1, "{s:?}");
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE type = 'Person' AND status = 'merged'"
        )
        .await,
        1
    );
    // A later mention of the merged-away name binds to the survivor.
    fx.index_csv(
        "HR/e.csv",
        &people_csv(&["周若溪,,产品", "Ruby Zhou,,产品"]),
    )
    .await;
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE type = 'Person' AND status = 'active'"
        )
        .await,
        2,
        "Ethan Chen and the merged Ruby entity"
    );
    // Redirect depth is one and both names still find the entity.
    assert_eq!(
        fx.scalar(
            "SELECT count(*) FROM _oo_docs.entity_redirect a JOIN _oo_docs.entity_redirect b ON b.from_key = a.to_key"
        )
        .await,
        0
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity_redirect WHERE reason = 'identity'")
            .await,
        1
    );
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn contradicting_identity_keeps_two_people_and_queues_a_conflict() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_res_conflict";
    let mut fx = Fx::new(db).await;
    fx.index_csv("HR/a.csv", &people_csv(&["张三,a@x.io,研发"]))
        .await;
    let s = fx
        .index_csv("HR/b.csv", &people_csv(&["张三,b@x.io,运营"]))
        .await;
    assert_eq!(s.alias_conflicts, 1, "{s:?}");
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity WHERE canonical_name = '张三'")
            .await,
        2
    );
    let reasons = fx
        .text("SELECT reason_code FROM _oo_docs.review_item WHERE kind = 'alias_conflict'")
        .await;
    assert_eq!(reasons, ["identity_conflict"]);
    // The same email again binds to the second person, not a third.
    fx.index_csv("HR/c.csv", &people_csv(&["张三,b@x.io,运营"]))
        .await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity WHERE canonical_name = '张三'")
            .await,
        2
    );
    // Merging them is refused: the emails differ.
    let (a, b) = {
        let k = fx
            .text("SELECT key FROM _oo_docs.entity WHERE canonical_name = '张三' ORDER BY key")
            .await;
        (k[0].clone(), k[1].clone())
    };
    let err = merge_entities(&mut fx.rt, &a, &b, "merge", None)
        .await
        .unwrap_err();
    assert!(matches!(err, MergeError::IdentityConflict(..)), "{err}");
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn merge_chain_flattens_and_split_restores() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_res_merge";
    let mut fx = Fx::new(db).await;
    for (path, name) in [
        ("People/甲.md", "甲"),
        ("People/乙.md", "乙"),
        ("People/丙.md", "丙"),
    ] {
        fx.index_md(path, &format!("# {name}\n\n角色：x\n")).await;
    }
    fx.index_md("Projects/星图.md", "# 星图\n\n负责人：甲\n")
        .await;
    let (a, b, c) = (
        key_of(&mut fx, "甲").await,
        key_of(&mut fx, "乙").await,
        key_of(&mut fx, "丙").await,
    );
    let owns_before = fx
        .scalar("SELECT count(*) FROM _oo_docs.fact WHERE predicate = 'owns'")
        .await;
    assert_eq!(owns_before, 1);

    // A into B, then B into C: A's redirect is rewritten to C.
    let r1 = merge_entities(&mut fx.rt, &a, &b, "human", None)
        .await
        .unwrap();
    assert!(r1.facts_moved >= 1);
    let r2 = merge_entities(&mut fx.rt, &b, &c, "human", None)
        .await
        .unwrap();
    assert_eq!(r2.redirects_rewritten, 1);
    let rows: Vec<String> = fx
        .text(
            "SELECT from_key || '>' || to_key || '>' || via_key FROM _oo_docs.entity_redirect ORDER BY from_key",
        )
        .await;
    let want = {
        let mut v = vec![format!("{a}>{c}>{b}"), format!("{b}>{c}>{c}")];
        v.sort();
        v
    };
    assert_eq!(rows, want);
    assert_eq!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity e ON e.id = f.subject_id \
              WHERE f.predicate = 'owns' AND e.key = '{c}' \
                AND f.object_id = (SELECT id FROM _oo_docs.entity WHERE canonical_name = '星图')"
        ))
        .await,
        1,
        "facts follow the merges"
    );
    // The old names are aliases of the survivor, and mentions of them bind to it.
    let names = fx
        .text(&format!(
            "SELECT a.alias FROM _oo_docs.alias a JOIN _oo_docs.entity e ON e.id = a.entity_id \
              WHERE e.key = '{c}' ORDER BY a.alias"
        ))
        .await;
    assert!(
        names.contains(&"甲".to_owned()) && names.contains(&"乙".to_owned()),
        "{names:?}"
    );
    fx.index_md("Notes/x.md", "# 备忘\n\n负责人：甲\n").await;
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity WHERE canonical_name = '甲'")
            .await,
        1,
        "no new 甲 was created"
    );

    // Merging into something that was merged away goes to the survivor.
    let again = merge_entities(&mut fx.rt, &a, &b, "human", None).await;
    assert!(matches!(again, Err(MergeError::NotActive(_))), "{again:?}");

    // Split B out of C: its facts and aliases return, A's redirect points at B.
    split_merged(&mut fx.rt, &b, "tester").await.unwrap();
    let rows: Vec<String> = fx
        .text("SELECT from_key || '>' || to_key FROM _oo_docs.entity_redirect ORDER BY from_key")
        .await;
    assert_eq!(rows, [format!("{a}>{b}")]);
    assert_eq!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity e ON e.id = f.subject_id \
              WHERE f.predicate = 'owns' AND e.key = '{c}' \
                AND f.object_id = (SELECT id FROM _oo_docs.entity WHERE canonical_name = '星图')"
        ))
        .await,
        0
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.overlay WHERE kind = 'entity_split'")
            .await,
        1
    );
    split_merged(&mut fx.rt, &a, "tester").await.unwrap();
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity_redirect")
            .await,
        0
    );
    assert_eq!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity e ON e.id = f.subject_id \
              WHERE f.predicate = 'owns' AND e.key = '{a}' \
                AND f.object_id = (SELECT id FROM _oo_docs.entity WHERE canonical_name = '星图')"
        ))
        .await,
        1,
        "the ownership went back to the original subject"
    );
    assert!(matches!(
        split_merged(&mut fx.rt, &a, "tester").await,
        Err(MergeError::NotMerged)
    ));
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn shared_published_alias_proposes_a_merge_and_titles_are_draft_aliases() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_res_propose";
    let mut fx = Fx::new(db).await;
    // Two entities that both carry the alias "老林" (published).
    fx.index_md(
        "People/林远舟.md",
        "---\ntype: person\naliases: 老林\n---\n\n# 林远舟\n\n内容\n",
    )
    .await;
    fx.index_md(
        "People/林雨.md",
        "---\ntype: person\naliases: [小林]\n---\n\n# 林雨\n\n内容\n",
    )
    .await;
    fx.publish().await;
    // The second one gains the alias later; both have evidence and a published alias in common.
    let s = fx
        .index_md(
            "人员/林雨.md",
            "---\ntype: person\naliases: [老林]\n---\n\n# 林雨\n\n内容\n",
        )
        .await;
    assert_eq!(s.merge_proposals, 0, "the alias is still a draft");
    fx.publish().await;
    let s = fx
        .index_md("Notes/再来.md", "# 备忘\n\n负责人：林雨\n")
        .await;
    assert!(s.merge_proposals >= 1, "{s:?}");
    let reasons = fx
        .text("SELECT reason_code FROM _oo_docs.review_item WHERE kind = 'entity_merge'")
        .await;
    assert_eq!(reasons, ["shared_alias"]);
    // Nothing merged by itself.
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.entity WHERE status = 'merged'")
            .await,
        0
    );

    // A person named with a title gets the title-free alias as a draft candidate.
    fx.index_md("Notes/老师.md", "# 备忘\n\n负责人：王芳老师\n")
        .await;
    let rows = fx
        .text(
            "SELECT a.alias || '/' || a.status || '/' || a.confidence_bp FROM _oo_docs.alias a \
               JOIN _oo_docs.entity e ON e.id = a.entity_id WHERE e.canonical_name = '王芳老师'",
        )
        .await;
    assert_eq!(rows, ["王芳/draft/5000"]);
    fx.finish(db).await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn reindexing_changes_nothing() {
    let _g = CLUSTER_LOCK.lock().await;
    let db = "oo_docs_res_idem";
    let mut fx = Fx::new(db).await;
    let csv = people_csv(&["陈子墨,c@x.io,研发", "周若溪,z@x.io,产品"]);
    let p = {
        let n = oo_docs::parse::csv::parse_csv(csv.as_bytes()).unwrap();
        fx.plant_normalized("HR/a.csv", n, csv.len()).await
    };
    let first = fx.index_planted("HR/a.csv", &p).await;
    assert!(
        first.entities_created >= 3 && first.identities_added == 2,
        "{first:?}"
    );
    let second = fx.index_planted("HR/a.csv", &p).await;
    assert_eq!(
        (
            second.entities_created,
            second.aliases_created,
            second.facts_created,
            second.evidence_added,
            second.identities_added,
            second.identity_merges
        ),
        (0, 0, 0, 0, 0, 0)
    );
    fx.finish(db).await;
}
