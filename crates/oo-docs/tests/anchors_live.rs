//! Cross-file ties through the whole pipeline over a real PostgreSQL: a value two documents state becomes one open Thing they
//! both mention, a value one document states stays out, and one person named in two scripts ends as one entity.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test anchors_live -- --ignored --test-threads=1`

mod common;

use common::engine::{Opts, TestEngine};

fn opts() -> Opts {
    Opts {
        workers: true,
        stages: true,
        ..Opts::default()
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_shared_amount_and_date_tie_the_documents_that_state_them() {
    let e = TestEngine::start("anchors_amount", opts()).await;
    e.write(
        "finance/budget.md",
        "# Launch budget\n\nTotal launch budget: 6,850,000\n\nPress embargo: 2027-03-16\n\nContingency: 310,000\n",
    );
    e.write(
        "reports/readiness.md",
        "# Readiness report\n\nThe approved total of 6,850,000 holds. The embargo lifts on 2027年3月16日.\n",
    );
    e.write(
        "notes/other.md",
        "# Other\n\nA different matter entirely, 88 units.\n",
    );
    let drive = e.create_drive("anchors-amount").await;
    let _ = drive;
    e.until("an Amount thing is mentioned by two documents", || async {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(DISTINCT f.subject_id) FROM _oo_docs.fact f \
               JOIN _oo_docs.entity t ON t.id = f.object_id \
              WHERE f.predicate = 'mentions' AND t.type = 'Thing' AND t.thing_label = 'Amount' \
                AND t.canonical_name = '6,850,000'",
        )
        .fetch_one(&e.pool)
        .await
        .unwrap();
        n == 2
    })
    .await;
    let dates: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT f.subject_id) FROM _oo_docs.fact f JOIN _oo_docs.entity t ON t.id = f.object_id \
          WHERE f.predicate = 'mentions' AND t.thing_label = 'Date' AND t.canonical_name = '2027-03-16'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(dates, 2);
    let single: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.entity WHERE type = 'Thing' AND canonical_name IN ('310,000', '88')",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(single, 0, "a value one document states ties nothing");
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_person_named_in_two_scripts_becomes_one_entity_through_the_pipeline() {
    let e = TestEngine::start("anchors_person", opts()).await;
    e.write(
        "people/roster.csv",
        "姓名 Name,English name,团队 Team\n邵婉清,Wanqing Shao,财务\n白若冰,Ruobing Bai,项目办\n",
    );
    e.write(
        "people/directory.csv",
        "name,email,office\nWanqing Shao,wanqing.shao@example.test,Shanghai\nRuobing Bai,ruobing.bai@example.test,Shanghai\n",
    );
    let drive = e.create_drive("anchors-person").await;
    let _ = drive;
    e.until("two people, not four", || async {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM _oo_docs.entity WHERE type = 'Person' AND status = 'active'",
        )
        .fetch_one(&e.pool)
        .await
        .unwrap();
        let docs: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _oo_docs.file WHERE state = 'published'")
                .fetch_one(&e.pool)
                .await
                .unwrap();
        docs == 2 && n == 2
    })
    .await;
    e.finish().await;
}
