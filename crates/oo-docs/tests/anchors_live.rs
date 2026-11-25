//! Cross-file ties through the whole pipeline over a real PostgreSQL: a value two documents state becomes one open Thing they
//! both mention, a value one document states stays out, tie nothing.
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

/// The same value written differently ties the documents (D93, 10 section 5.1.3): amounts and percentages in words, a spoken
/// date, and a relative date read against the date in the file name.
#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn values_written_in_words_tie_the_documents_through_one_value() {
    let e = TestEngine::start("anchors_words", opts()).await;
    e.write(
        "reports/readiness-zh-2027-01-12.md",
        "# 就绪报告\n\n市场预算为一百四十二万元。发布会三月十六日举行，评审安排在下周二。镜片良率提升到百分之八十八。\n",
    );
    e.write(
        "reports/readiness-en-2027-01-12.md",
        "# Readiness report\n\nThe marketing budget is one million four hundred twenty thousand yuan. The embargo lifts March sixteenth; the review is next Tuesday. Yield is now 88 percent.\n",
    );
    e.write(
        "notes/embargo-2027-01-12.md",
        "# Embargo\n\nThe press embargo lifts on 2027-03-16. Review on 2027-01-19. Yield 88%.\n",
    );
    e.create_drive("anchors-words").await;
    let thing = |label: &'static str, name: &'static str| {
        let pool = e.pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(DISTINCT f.subject_id) FROM _oo_docs.fact f JOIN _oo_docs.entity t ON t.id = f.object_id \
                  WHERE f.predicate = 'mentions' AND t.thing_label = $1 AND t.canonical_name = $2",
            )
            .bind(label)
            .bind(name)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    e.until(
        "the marketing budget is one Amount both reports mention",
        || async { thing("Amount", "1,420,000").await == 2 },
    )
    .await;
    // The spoken date and the written one are the same Date; so is the relative one against the date in the file name.
    assert_eq!(thing("Date", "2027-03-16").await, 3);
    assert_eq!(thing("Date", "2027-01-19").await, 3);
    assert_eq!(thing("Ratio", "88%").await, 3);
    // The original spelling is another name of the amount.
    let aliases: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM _oo_docs.alias a JOIN _oo_docs.entity t ON t.id = a.entity_id WHERE t.canonical_name = '1,420,000' AND a.alias = '一百四十二万元'",
    )
    .fetch_one(&e.pool)
    .await
    .unwrap();
    assert_eq!(aliases, 1);
    e.finish().await;
}
