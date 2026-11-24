//! Full-text search over facts and blocks (`GET /v1/docs/search`, docs-api 1.9) on a real PostgreSQL: Chinese and
//! English queries, phrases that must not match scattered characters, the provenance of every hit, a stable order,
//! and the Drive scope of an Agent token.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test search_live -- --ignored --test-threads=1`

mod common;

use common::engine::{Opts, TestEngine};
use common::fx::{DRIVE, Fx};
use common::http::{Resp, request};
use serde_json::{Value, json};

const DRIVE_B: &str = "00000000-0000-4000-8000-0000000000d2";
const CONTRACT: &str = "---\ntype: 组织\nname: 澄海物流\n---\n# 澄海物流\n\n合同：澄海物流合作合同\n\n合同编号：HT-2026-017\n";
const ENGLISH: &str = "---\ntype: organization\n---\n\n# Acme Ltd\n\nAcme Ltd is our largest customer. Contact: Jane Roe.\n\n## Contract\n\nThe contract renews every March.\n";
const OTHER: &str =
    "---\ntype: organization\n---\n\n# Zenith Corp\n\nZenith pays invoices in April.\n";

async fn call(e: &TestEngine, token: &str, path: &str) -> Resp {
    let auth = format!("Bearer {token}");
    request(e.addr, "GET", path, &[("Authorization", &auth)], None).await
}

fn items(r: &Resp) -> Vec<Value> {
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    r.json()["items"].as_array().cloned().unwrap_or_default()
}

fn find<'a>(v: &'a [Value], kind: &str, needle: &str) -> Option<&'a Value> {
    v.iter()
        .find(|i| i["kind"] == kind && i["text"].as_str().is_some_and(|t| t.contains(needle)))
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres)"]
async fn chinese_and_english_facts_and_blocks_with_provenance() {
    let e = TestEngine::start(
        "search_fts",
        Opts {
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    let mut fx = Fx::attach("search_fts", &e.cas).await;
    fx.admin_sql(&format!(
        "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES ('{DRIVE_B}', 'b', 'fs', '{{}}')"
    ))
    .await;
    fx.index_md("Contracts/澄海物流合作合同.md", CONTRACT).await;
    fx.index_md("Customers/acme.md", ENGLISH).await;
    fx.publish().await;
    // Drive B holds a file of its own (planted straight into the second Drive).
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.file SET drive_id = '{DRIVE_B}' WHERE id IN \
           (SELECT file_id FROM _oo_docs.file_version WHERE path_at_version = 'Customers/acme.md')"
    ))
    .await;
    let _ = OTHER;
    // The rule extractor gives only `mentions`; an attribute fact with a value text is planted from a mention of
    // the contract file so the value path of the search is exercised too.
    fx.admin_sql(
        "WITH src AS (SELECT ft.subject_id, ft.published_in, ev.version_id, ev.b3, ev.block_id, ev.anchor, ev.quote_b3 \
                        FROM _oo_docs.fact ft JOIN _oo_docs.evidence ev ON ev.target_id = ft.id AND ev.target_kind = 'fact' \
                        JOIN _oo_docs.entity s ON s.id = ft.subject_id WHERE s.canonical_name LIKE 'Contracts/%' LIMIT 1), \
              nf AS (INSERT INTO _oo_docs.fact (id, fact_key, subject_id, original_subject_id, predicate, value, \
                        confidence_bp, origin, status, published_in) \
                     SELECT gen_random_uuid(), repeat('a', 64), subject_id, subject_id, 'attr.contract_no', \
                            '{\"text\":\"HT-2026-017\"}'::jsonb, 9000, 'human', 'published', published_in FROM src RETURNING id) \
         INSERT INTO _oo_docs.evidence (id, target_kind, target_id, version_id, b3, block_id, anchor, quote_b3, origin) \
         SELECT gen_random_uuid(), 'fact', nf.id, src.version_id, src.b3, src.block_id, src.anchor, src.quote_b3, 'human' \
           FROM nf, src",
    )
    .await;

    let q = |s: &str, d: &str| {
        let enc: String = s
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        format!("/v1/docs/search?q={enc}&drive_id={d}")
    };

    // Chinese: the contract number is in a block and in a fact, each with a file anchor.
    let r = e.get(&q("合同编号", DRIVE)).await;
    let v = items(&r);
    assert!(!v.is_empty(), "{}", String::from_utf8_lossy(&r.body));
    let b = find(&v, "block", "HT-2026-017").expect("block hit");
    assert_eq!(b["evidence"][0]["path"], "Contracts/澄海物流合作合同.md");
    assert!(
        b["evidence"][0]["block_id"]
            .as_str()
            .unwrap()
            .starts_with("blk_")
    );
    assert_eq!(b["evidence"][0]["drive_id"], DRIVE);
    for it in &v {
        assert!(
            it["evidence"].as_array().is_some_and(|x| !x.is_empty()),
            "{it}"
        );
    }
    // The value of a fact is found by its value.
    let r = e.get(&q("HT-2026-017", DRIVE)).await;
    let v = items(&r);
    let f = find(&v, "fact", "HT-2026-017").expect("fact hit by its value");
    assert_eq!(f["predicate"], "attr.contract_no");
    assert_eq!(f["evidence"][0]["path"], "Contracts/澄海物流合作合同.md");
    assert!(v.iter().any(|i| i["kind"] == "block"), "{v:?}");
    // A name is found through the facts that mention it.
    assert!(
        items(&e.get(&q("澄海物流", DRIVE)).await)
            .iter()
            .any(|i| i["kind"] == "fact")
    );
    // A phrase is not a bag of characters: reversed order and scattered characters do not match.
    assert!(items(&e.get(&q("编号合同", DRIVE)).await).is_empty());
    assert!(items(&e.get(&q("海澄", DRIVE)).await).is_empty());
    // English prefix and AND semantics; the other drive's file is not in Drive A.
    assert!(
        items(&e.get(&q("contract renews", DRIVE)).await).is_empty(),
        "acme moved to B"
    );
    let r = e.get(&q("contract renews march", DRIVE_B)).await;
    let v = items(&r);
    let b = find(&v, "block", "renews every March").expect("english block");
    assert_eq!(b["evidence"][0]["path"], "Customers/acme.md");
    assert_eq!(b["evidence"][0]["drive_id"], DRIVE_B);
    assert!(items(&e.get(&q("contract renews april", DRIVE_B)).await).is_empty());
    // A stable order, the same bytes twice, and the limit.
    let a = e.get(&q("contract", DRIVE_B)).await.body;
    assert_eq!(a, e.get(&q("contract", DRIVE_B)).await.body);
    let r = e
        .get(&format!(
            "/v1/docs/search?q=acme&drive_id={DRIVE_B}&limit=1"
        ))
        .await;
    assert!(items(&r).len() <= 1);
    // Bad input: no query, an operator-only query, a bad drive id.
    assert_eq!(e.get("/v1/docs/search").await.status, 400);
    assert!(items(&e.get("/v1/docs/search?q=%26%7C%21").await).is_empty());
    assert_eq!(e.get("/v1/docs/search?q=a&drive_id=nope").await.status, 400);

    // An Agent token sees its Drives only and needs docs:read.
    let issue = |body: Value| {
        let e = &e;
        async move {
            let r = e
                .req("POST", "/v1/docs/agent-tokens", &[], Some(&body))
                .await;
            assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
            r.json()["token"].as_str().unwrap().to_owned()
        }
    };
    let tok_a = issue(json!({"name": "ra", "scopes": ["docs:read"], "drive_ids": [DRIVE]})).await;
    let tok_all = issue(json!({"name": "rall", "scopes": ["docs:read"], "all_drives": true})).await;
    let tok_db = issue(json!({"name": "dbo", "scopes": ["db:read"], "gw_profile": "sales"})).await;
    assert!(!items(&call(&e, &tok_a, &q("合同编号", DRIVE)).await).is_empty());
    let r = call(&e, &tok_a, &q("contract renews march", DRIVE_B)).await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (403, Some("drive"))
    );
    let r = call(&e, &tok_a, "/v1/docs/search?q=contract").await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (403, Some("drive_id_required"))
    );
    let r = call(&e, &tok_db, &q("合同编号", DRIVE)).await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (403, Some("scope"))
    );
    // all_drives without a drive_id searches both Drives and every hit names its own.
    let v = items(&call(&e, &tok_all, "/v1/docs/search?q=contract").await);
    let drives: std::collections::BTreeSet<&str> = v
        .iter()
        .map(|i| i["evidence"][0]["drive_id"].as_str().unwrap())
        .collect();
    assert_eq!(drives, [DRIVE, DRIVE_B].into_iter().collect(), "{v:?}");
}
