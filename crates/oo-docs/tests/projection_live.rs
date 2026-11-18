//! Read-only projection of published document entities into an Agent DB and the links to database rows (D12, 10 §11.3,
//! B-S11-04): a contract document and a customer table row, one projection, idempotent and rebuildable, human link
//! decisions kept, the Drive policy honoured.
//!
//! "Agent DB" here is a second throwaway database with a `customers` table; the document side is the real engine
//! write path (`Fx`). A contract that is a PDF reaches the same place when the extractor names the organization
//! (the PDF text needs the parser sidecar and, for entities beyond headings, the model, neither of which this test
//! runs); the contract here is a Markdown file with the same organization front matter.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test projection_live -- --ignored --test-threads=1`

mod common;

use common::fx::{DRIVE, Fx};
use common::*;
use oo_docs::publish::projection::{self, LinkRule};
use oo_docs::publish::run::PublishOutcome;
use sqlx::Executor;

const CONTRACT: &str = "---\ntype: 组织\nname: 澄海物流\n---\n# 澄海物流\n\n合同：澄海物流合作合同\n\n合同编号：HT-2026-017\n";
const MEETING: &str = "---\ntype: 组织\nname: 远帆咨询\n---\n# 远帆咨询\n\n合同编号：HT-2026-021\n";

async fn agent_db(name: &str) -> sqlx::PgConnection {
    create_db(name).await;
    let mut c = connect(name).await;
    c.execute(
        "CREATE TABLE public.customers (id integer PRIMARY KEY, name text NOT NULL, region text); \
         INSERT INTO public.customers VALUES (17, '澄海物流', '华东'), (18, '北辰云', '华北'), (19, '远帆咨询', '华南');",
    )
    .await
    .unwrap();
    c
}

fn rules() -> Vec<LinkRule> {
    vec![LinkRule {
        db_table: "public.customers".into(),
        key_column: "id".into(),
        match_columns: vec!["name".into()],
        doc_types: vec!["Organization".into()],
    }]
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_contract_is_linked_to_its_customer_row_and_the_projection_is_idempotent_and_rebuildable()
{
    let _g = CLUSTER_LOCK.lock().await;
    let mut fx = Fx::new("oo_docs_proj_docs").await;
    fx.index_md("Contracts/澄海物流合作合同.md", CONTRACT).await;
    fx.index_md("Notes/远帆咨询.md", MEETING).await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("publish")
    };
    let mut agent = agent_db("oo_docs_proj_agent").await;

    let first = projection::project(&mut fx.rt, &fx.cas, &mut agent, DRIVE)
        .await
        .unwrap();
    assert!(first.written && first.entities >= 2, "{first:?}");
    let again = projection::project(&mut fx.rt, &fx.cas, &mut agent, DRIVE)
        .await
        .unwrap();
    assert!(!again.written, "an unchanged ontology writes nothing");
    assert_eq!(again.digest, first.digest);

    let report = projection::link(&mut agent, DRIVE, &rules()).await.unwrap();
    assert_eq!(report.links, 2, "{report:?}");
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT db_key, path, matched_via FROM _oo_docproj.document_link \
          WHERE db_table = 'public.customers' ORDER BY db_key",
    )
    .fetch_all(&mut agent)
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            (
                "17".to_owned(),
                "Contracts/澄海物流合作合同.md".to_owned(),
                "name".to_owned()
            ),
            (
                "19".to_owned(),
                "Notes/远帆咨询.md".to_owned(),
                "name".to_owned()
            ),
        ],
        "the contract of 澄海物流 is the document of customer row 17; 北辰云 has no document, so no link"
    );
    if let Some(dir) = std::env::var_os("OO_L7_EVIDENCE_DIR") {
        let links: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT db_table || ' id=' || db_key, path, doc_entity_type || ' ' || canonical_name \
               FROM _oo_docproj.document_link ORDER BY db_key",
        )
        .fetch_all(&mut agent)
        .await
        .unwrap();
        let customers: Vec<(i32, String)> =
            sqlx::query_as("SELECT id, name FROM public.customers ORDER BY id")
                .fetch_all(&mut agent)
                .await
                .unwrap();
        let doc = serde_json::json!({
            "what": "document entities (docs-db, published) linked to rows of the Agent DB table public.customers; the view _oo_docproj.document_link answers 'which document is about this customer row'",
            "customers_table": customers,
            "links": links.iter().map(|(row, path, ent)| serde_json::json!({"database_row": row, "document": path, "document_entity": ent})).collect::<Vec<_>>(),
            "projection": {"ontology_version": first.ontology_version, "entities": first.entities, "digest": first.digest},
        });
        std::fs::write(
            std::path::Path::new(&dir).join("projection-demo.json"),
            serde_json::to_string_pretty(&doc).unwrap() + "\n",
        )
        .unwrap();
    }
    // The projection is only ever read: nothing of it is in docs-db, and the customer table is untouched.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM public.customers")
        .fetch_one(&mut agent)
        .await
        .unwrap();
    assert_eq!(n, 3);

    // Rebuildable: drop the whole schema, project and link again, same result.
    agent
        .execute("DROP SCHEMA _oo_docproj CASCADE")
        .await
        .unwrap();
    let rebuilt = projection::project(&mut fx.rt, &fx.cas, &mut agent, DRIVE)
        .await
        .unwrap();
    assert!(rebuilt.written);
    assert_eq!(
        rebuilt.digest, first.digest,
        "the same graph, the same digest"
    );
    assert_eq!(
        projection::link(&mut agent, DRIVE, &rules())
            .await
            .unwrap()
            .links,
        2
    );

    // A person's decision survives rebuilds of the automatic links.
    let key: String = sqlx::query_scalar(
        "SELECT doc_entity_key FROM _oo_docproj.entity_link WHERE db_key = '19'",
    )
    .fetch_one(&mut agent)
    .await
    .unwrap();
    assert!(
        projection::decide(
            &mut agent,
            DRIVE,
            &key,
            "public.customers",
            "19",
            "rejected"
        )
        .await
        .unwrap()
    );
    projection::link(&mut agent, DRIVE, &rules()).await.unwrap();
    let st: String =
        sqlx::query_scalar("SELECT status FROM _oo_docproj.entity_link WHERE db_key = '19'")
            .fetch_one(&mut agent)
            .await
            .unwrap();
    assert_eq!(st, "rejected");
    let visible: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _oo_docproj.document_link WHERE db_key = '19'")
            .fetch_one(&mut agent)
            .await
            .unwrap();
    assert_eq!(visible, 0, "a rejected link is not in the view");

    // A policy that denies the contract takes it out of the projection (one visible graph for everything).
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.drive SET config = '{{\"policy\": {{\"deny\": [\"Contracts\"]}}}}'::jsonb WHERE id = '{DRIVE}'"
    ))
    .await;
    let after = projection::project(&mut fx.rt, &fx.cas, &mut agent, DRIVE)
        .await
        .unwrap();
    assert!(
        after.written && after.entities < first.entities,
        "{after:?}"
    );
    projection::link(&mut agent, DRIVE, &rules()).await.unwrap();
    let left: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT db_key FROM _oo_docproj.entity_link WHERE status = 'auto'",
    )
    .fetch_all(&mut agent)
    .await
    .unwrap();
    assert!(
        !left.contains(&"17".to_owned()),
        "the denied contract no longer links row 17: {left:?}"
    );
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn link_rules_cannot_reach_what_is_not_a_data_table() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut fx = Fx::new("oo_docs_proj_rules").await;
    fx.index_md("Contracts/a.md", CONTRACT).await;
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("publish")
    };
    let mut agent = agent_db("oo_docs_proj_rules_agent").await;
    projection::project(&mut fx.rt, &fx.cas, &mut agent, DRIVE)
        .await
        .unwrap();
    for bad in [
        "_oo.apply_receipts",
        "pg_catalog.pg_user",
        "public.customers; DROP TABLE x",
    ] {
        let r = LinkRule {
            db_table: bad.into(),
            ..rules().remove(0)
        };
        assert!(
            projection::link(&mut agent, DRIVE, &[r]).await.is_err(),
            "{bad}"
        );
    }
}
