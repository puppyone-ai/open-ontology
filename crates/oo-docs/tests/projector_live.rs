//! The projection job and the link routes (D83): published document entities into the Agent DB, incremental per
//! published version, idempotent and replay safe, a cross-source link with provenance on both sides, the routes
//! the unified query surface reads.
//!
//! "Agent DB" is a second throwaway database with a `customers` table; the document side is the real engine write
//! path (`Fx`). The Agent DB location is a secret reference resolved from a secrets directory, as in production.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test projector_live -- --ignored --test-threads=1`

mod common;

use std::sync::Arc;

use common::engine::{Opts, TestEngine};
use common::fx::{DRIVE, Fx};
use common::http::request;
use common::*;
use oo_docs::publish::agentdb::AgentDbConfig;
use oo_docs::publish::projection;
use oo_docs::publish::projector::Projector;
use oo_docs::publish::run::PublishOutcome;
use serde_json::{Value, json};
use sqlx::Executor;

const CONTRACT: &str = "---\ntype: 组织\nname: 澄海物流\n---\n# 澄海物流\n\n合同：澄海物流合作合同\n\n合同编号：HT-2026-017\n";
const MEETING: &str = "---\ntype: 组织\nname: 远帆咨询\n---\n# 远帆咨询\n\n合同编号：HT-2026-021\n";
const NEWCO: &str = "---\ntype: 组织\nname: 北辰云\n---\n# 北辰云\n\n合同编号：HT-2026-030\n";

/// The response must be an instance of the `$def` of the API schema (contracts/docs-api).
fn conforms(name: &str, value: &Value) {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/docs-api/docs-api.v1.schema.json"),
    )
    .unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    let wrapper = json!({"$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": doc["$defs"].clone(), "$ref": format!("#/$defs/{name}")});
    let v = oo_core::schema_util::compile_schema(&wrapper).unwrap();
    let errors: Vec<String> = v
        .iter_errors(value)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{name}: {}\n{value}", errors.join("; "));
}

fn pg_url(db: &str) -> String {
    let host = std::env::var("OO_DOCS_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("OO_DOCS_TEST_PG_PORT").expect("port");
    let user = std::env::var("OO_DOCS_TEST_PG_USER").unwrap_or_else(|_| "postgres".into());
    format!("postgres://{user}@{host}:{port}/{db}")
}

/// A secrets directory holding the Agent DB URL under the name `agentdb`, 0600.
fn secrets(db: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("oo-wp2-secrets-{}-{db}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("agentdb");
    std::fs::write(&f, pg_url(db)).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    dir
}

async fn agent_db(name: &str) -> sqlx::PgConnection {
    create_db(name).await;
    let mut c = connect(name).await;
    c.execute(
        "CREATE TABLE public.customers (id integer PRIMARY KEY, name text NOT NULL, short_name text, region text); \
         INSERT INTO public.customers VALUES (17, '澄海物流', NULL, '华东'), (18, '北辰云', NULL, '华北'), (19, '远帆咨询', NULL, '华南');",
    )
    .await
    .unwrap();
    c
}

async fn rule(fx: &mut Fx, rev: i64) {
    fx.admin_sql(&format!(
        "INSERT INTO _oo_docs.db_link_rule (drive_id, db_table, key_col, name_col, entity_type, revision) \
         VALUES ('{DRIVE}', 'public.customers', 'id', 'name', 'Organization', {rev}) \
         ON CONFLICT (drive_id, db_table) DO UPDATE SET revision = {rev}"
    ))
    .await;
}

async fn publish(fx: &mut Fx) {
    let PublishOutcome::Published(_) = fx.publish().await else {
        panic!("publish")
    };
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_job_projects_incrementally_links_with_provenance_and_replays_safely() {
    let _g = CLUSTER_LOCK.lock().await;
    let mut fx = Fx::new("oo_wp2_proj_docs").await;
    let mut agent = agent_db("oo_wp2_proj_agent").await;
    let pool = pool_runtime("oo_wp2_proj_docs", 4).await;
    let cas = Arc::new(oo_docs::cas::Cas::open(&fx.dir).unwrap());
    let cfg = AgentDbConfig {
        secrets_dir: secrets("oo_wp2_proj_agent"),
        env_ref: None,
    };
    let job = Projector::new(pool.clone(), cas, cfg)
        .with_link_refresh(std::time::Duration::from_secs(3600));

    fx.index_md("Contracts/澄海物流合作合同.md", CONTRACT).await;
    fx.index_md("Notes/远帆咨询.md", MEETING).await;
    publish(&mut fx).await;

    // No binding: the job does nothing and says so (documents only is a state, not an error).
    let r = job.run_drive(DRIVE, false).await.unwrap();
    assert_eq!(r.skipped, Some("no_agent_db"));
    let exists: bool = sqlx::query_scalar("SELECT to_regnamespace('_oo_docproj') IS NOT NULL")
        .fetch_one(&mut agent)
        .await
        .unwrap();
    assert!(!exists, "nothing was created in the Agent DB");

    fx.admin_sql("INSERT INTO _oo_docs.agent_db_binding (secret_ref) VALUES ('agentdb')")
        .await;
    rule(&mut fx, 1).await;

    // First run: everything is new; the links carry provenance from both kinds of source.
    let r = job.run_drive(DRIVE, false).await.unwrap();
    let p = r.projection.clone().expect("projected");
    assert!(
        p.written && p.inserted >= 2 && p.updated == 0 && p.deleted == 0,
        "{p:?}"
    );
    assert_eq!(r.link.as_ref().unwrap().links, 2, "{r:?}");
    let links: Vec<(String, String, String, i64)> = sqlx::query_as(
        "SELECT db_key, path, matched_column, rule_revision FROM _oo_docproj.document_link l \
           JOIN _oo_docproj.entity_link e USING (drive_id, db_table, db_key, doc_entity_key) \
          WHERE l.db_table = 'public.customers' ORDER BY db_key",
    )
    .fetch_all(&mut agent)
    .await
    .unwrap();
    assert_eq!(
        links,
        [
            (
                "17".to_owned(),
                "Contracts/澄海物流合作合同.md".to_owned(),
                "name".to_owned(),
                1
            ),
            (
                "19".to_owned(),
                "Notes/远帆咨询.md".to_owned(),
                "name".to_owned(),
                1
            ),
        ]
    );
    let kinds: Vec<(String, String)> = sqlx::query_as(
        "SELECT p.source_kind, p.source_ref FROM _oo_docproj.entity_provenance p \
           JOIN _oo_docproj.doc_entity e ON e.drive_id = p.drive_id AND e.entity_key = p.entity_key \
          WHERE e.canonical_name = '澄海物流' ORDER BY 1, 2",
    )
    .fetch_all(&mut agent)
    .await
    .unwrap();
    assert!(
        kinds
            .iter()
            .any(|k| k.0 == "file" && k.1 == "Contracts/澄海物流合作合同.md"),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&("table_row".to_owned(), "public.customers/17".to_owned())),
        "{kinds:?}"
    );
    // File side: the anchor inside the file and the version; table side: which column, which rule.
    let (anchor, version): (String, String) = sqlx::query_as(
        "SELECT (detail->'anchor')::text, detail->>'file_version' FROM _oo_docproj.entity_provenance \
          WHERE source_kind = 'file' AND source_ref = 'Contracts/澄海物流合作合同.md' LIMIT 1",
    )
    .fetch_one(&mut agent)
    .await
    .unwrap();
    let anchor: Value = serde_json::from_str(&anchor).unwrap();
    assert!(
        anchor["kind"].is_string() && !version.is_empty(),
        "{anchor} {version}"
    );

    // Nothing changed: not even looked at (the trigger key is the same).
    let r = job.run_drive(DRIVE, false).await.unwrap();
    assert_eq!(r.skipped, Some("unchanged"));
    // Forced replay: idempotent, byte for byte.
    let created: Vec<String> =
        sqlx::query_scalar("SELECT created_at::text FROM _oo_docproj.entity_link ORDER BY db_key")
            .fetch_all(&mut agent)
            .await
            .unwrap();
    let again = job.run_drive(DRIVE, true).await.unwrap();
    let p2 = again.projection.unwrap();
    assert!(!p2.written && p2.digest == p.digest, "{p2:?}");
    let l2 = again.link.unwrap();
    assert_eq!((l2.links, l2.updated, l2.removed), (0, 0, 0), "{l2:?}");
    let created2: Vec<String> =
        sqlx::query_scalar("SELECT created_at::text FROM _oo_docproj.entity_link ORDER BY db_key")
            .fetch_all(&mut agent)
            .await
            .unwrap();
    assert_eq!(created, created2, "a replay does not rewrite a link");

    // A person's decisions survive every run; a rejected link is not made again.
    let key17: String = sqlx::query_scalar(
        "SELECT doc_entity_key FROM _oo_docproj.entity_link WHERE db_key = '17'",
    )
    .fetch_one(&mut agent)
    .await
    .unwrap();
    assert!(
        projection::decide(
            &mut agent,
            DRIVE,
            &key17,
            "public.customers",
            "17",
            "confirmed"
        )
        .await
        .unwrap()
    );
    let key19: String = sqlx::query_scalar(
        "SELECT doc_entity_key FROM _oo_docproj.entity_link WHERE db_key = '19'",
    )
    .fetch_one(&mut agent)
    .await
    .unwrap();
    assert!(
        projection::decide(
            &mut agent,
            DRIVE,
            &key19,
            "public.customers",
            "19",
            "rejected"
        )
        .await
        .unwrap()
    );

    // A new published version: only the new entity is written, the others are left alone.
    fx.index_md("Notes/北辰云.md", NEWCO).await;
    publish(&mut fx).await;
    let before: Vec<String> =
        sqlx::query_scalar("SELECT row_digest FROM _oo_docproj.doc_entity ORDER BY entity_key")
            .fetch_all(&mut agent)
            .await
            .unwrap();
    let r = job.run_drive(DRIVE, false).await.unwrap();
    let p3 = r.projection.clone().unwrap();
    assert!(
        p3.written && p3.inserted == 2 && p3.updated == 0 && p3.deleted == 0,
        "incremental: {p3:?}"
    );
    assert_eq!(
        r.link.as_ref().unwrap().links,
        1,
        "the new document finds customer 18: {r:?}"
    );
    let after: Vec<String> =
        sqlx::query_scalar("SELECT row_digest FROM _oo_docproj.doc_entity ORDER BY entity_key")
            .fetch_all(&mut agent)
            .await
            .unwrap();
    assert_eq!(
        after.len(),
        before.len() + 2,
        "a document and the organization it names"
    );
    assert!(
        before.iter().all(|d| after.contains(d)),
        "the old rows are byte identical"
    );
    let statuses: Vec<(String, String)> =
        sqlx::query_as("SELECT db_key, status FROM _oo_docproj.entity_link ORDER BY db_key")
            .fetch_all(&mut agent)
            .await
            .unwrap();
    assert_eq!(
        statuses,
        [
            ("17".into(), "confirmed".into()),
            ("18".into(), "auto".into()),
            ("19".into(), "rejected".into())
        ],
        "decisions kept, the rejected link not remade"
    );

    // A changed rule set (new revision) restamps the links and removes nothing that still matches; a rule that
    // names another column drops links that no row matches anymore.
    rule(&mut fx, 2).await;
    let r = job.run_drive(DRIVE, false).await.unwrap();
    assert!(r.projection.is_some(), "the rule change is a new trigger");
    let rev: i64 =
        sqlx::query_scalar("SELECT rule_revision FROM _oo_docproj.entity_link WHERE db_key = '18'")
            .fetch_one(&mut agent)
            .await
            .unwrap();
    assert_eq!(rev, 2);
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.db_link_rule SET name_col = 'short_name', revision = 3 WHERE drive_id = '{DRIVE}'"
    ))
    .await;
    let r = job.run_drive(DRIVE, false).await.unwrap();
    assert_eq!(
        r.link.unwrap().removed,
        1,
        "the auto link of row 18 matches nothing under short_name"
    );
    fx.admin_sql(&format!(
        "UPDATE _oo_docs.db_link_rule SET name_col = 'name', revision = 4 WHERE drive_id = '{DRIVE}'"
    ))
    .await;
    job.run_drive(DRIVE, false).await.unwrap();

    // A database row that arrives later is linked on the next refresh (the rows move without the documents).
    agent
        .execute("INSERT INTO public.customers VALUES (20, '澄海物流', NULL, '华东')")
        .await
        .unwrap();
    let r = job.run_drive(DRIVE, true).await.unwrap();
    assert_eq!(
        r.link.unwrap().links,
        1,
        "customer 20 carries the same name"
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM public.customers")
        .fetch_one(&mut agent)
        .await
        .unwrap();
    assert_eq!(n, 4, "the database table itself is only read");

    // Replay after the whole projection is lost: a fresh job rebuilds the same rows from docs-db.
    let digest: String =
        sqlx::query_scalar("SELECT source_digest FROM _oo_docproj.projection_state")
            .fetch_one(&mut agent)
            .await
            .unwrap();
    let docs_rows: Vec<(String, String)> =
        sqlx::query_as("SELECT entity_key, row_digest FROM _oo_docproj.doc_entity ORDER BY 1")
            .fetch_all(&mut agent)
            .await
            .unwrap();
    agent
        .execute("DROP SCHEMA _oo_docproj CASCADE")
        .await
        .unwrap();
    let fresh = Projector::new(
        pool.clone(),
        Arc::new(oo_docs::cas::Cas::open(&fx.dir).unwrap()),
        AgentDbConfig {
            secrets_dir: secrets("oo_wp2_proj_agent"),
            env_ref: None,
        },
    );
    let r = fresh.run_drive(DRIVE, false).await.unwrap();
    let p4 = r.projection.unwrap();
    assert!(p4.written && p4.inserted == docs_rows.len(), "{p4:?}");
    let digest2: String =
        sqlx::query_scalar("SELECT source_digest FROM _oo_docproj.projection_state")
            .fetch_one(&mut agent)
            .await
            .unwrap();
    let docs_rows2: Vec<(String, String)> =
        sqlx::query_as("SELECT entity_key, row_digest FROM _oo_docproj.doc_entity ORDER BY 1")
            .fetch_all(&mut agent)
            .await
            .unwrap();
    assert_eq!(digest, digest2, "same input, same projection");
    assert_eq!(docs_rows, docs_rows2);

    // A rule that names a table that is not there fails the run with a code, and the projection stays.
    fx.admin_sql(&format!(
        "INSERT INTO _oo_docs.db_link_rule (drive_id, db_table, key_col, name_col) VALUES ('{DRIVE}', 'public.nothing', 'id', 'name')"
    ))
    .await;
    let err = job.run_drive(DRIVE, false).await.unwrap_err();
    assert_eq!(err.code(), "projection_failed");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docproj.doc_entity")
        .fetch_one(&mut agent)
        .await
        .unwrap();
    assert!(n >= 3);
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn the_link_routes_bind_project_look_up_and_decide_with_scopes() {
    let name = "oo_wp2_links_docs";
    let agent_name = "oo_wp2_links_agent";
    let mut agent = agent_db(agent_name).await;
    let secrets_dir = secrets(agent_name);
    let sd = secrets_dir.clone();
    let e = TestEngine::start(
        name,
        Opts {
            stages: true,
            tweak: Some(Box::new(move |app| {
                app.agent_db = Some(AgentDbConfig {
                    secrets_dir: sd,
                    env_ref: None,
                });
            })),
            ..Opts::default()
        },
    )
    .await;
    let mut fx = Fx::attach(name, &e.cas).await;
    fx.index_md("Contracts/澄海物流合作合同.md", CONTRACT).await;
    fx.index_md("Notes/远帆咨询.md", MEETING).await;
    publish(&mut fx).await;
    let other = "00000000-0000-4000-8000-0000000000d2";
    fx.admin_sql(&format!(
        "INSERT INTO _oo_docs.drive (id, name, backend, config) VALUES ('{other}', 'b', 'fs', '{{}}')"
    ))
    .await;

    // Unbound: documents only. Reads are empty, writes say why.
    let r = e
        .get("/v1/docs/links?db_table=public.customers&db_key=17")
        .await;
    assert_eq!(
        (r.status, r.json()["agent_db"].as_str()),
        (200, Some("unbound"))
    );
    assert_eq!(r.json()["items"].as_array().unwrap().len(), 0);
    assert_eq!(
        e.get("/v1/docs/agent-db-binding").await.json()["bound"],
        false
    );
    let r = e
        .req("POST", "/v1/docs/links:decide", &[], Some(&json!({"drive_id": DRIVE, "doc_entity_key": "k", "db_table": "public.customers", "db_key": "1", "status": "confirmed"})))
        .await;
    assert_eq!(r.status, 400);
    let r = e.post("/v1/docs/links:project", &json!({})).await;
    assert_eq!(r.json()["runs"][0]["skipped"], "no_agent_db");

    // Binding: a reference, never a URL; it must resolve to a database that accepts the login.
    for bad in [
        pg_url(agent_name),
        "env:OO_NO_SUCH_VARIABLE_FOR_TEST".to_owned(),
        "missing-ref".to_owned(),
    ] {
        let r = e
            .req(
                "PUT",
                "/v1/docs/agent-db-binding",
                &[],
                Some(&json!({"secret_ref": bad})),
            )
            .await;
        assert!(
            [400, 422].contains(&r.status),
            "{bad}: {}",
            String::from_utf8_lossy(&r.body)
        );
        assert!(
            !String::from_utf8_lossy(&r.body).contains("postgres://"),
            "no URL in an answer"
        );
    }
    let r = e
        .req(
            "PUT",
            "/v1/docs/agent-db-binding",
            &[],
            Some(&json!({"secret_ref": "agentdb"})),
        )
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let b = e.get("/v1/docs/agent-db-binding").await.json();
    conforms("AgentDbBinding", &b);
    assert_eq!(
        (b["bound"].as_bool(), b["secret_ref"].as_str()),
        (Some(true), Some("agentdb"))
    );
    assert!(!b.to_string().contains("postgres://"));

    // Rules.
    for bad in [
        json!({"drive_id": DRIVE, "db_table": "customers", "key_col": "id", "name_col": "name"}),
        json!({"drive_id": DRIVE, "db_table": "_oo.apply_receipts", "key_col": "id", "name_col": "name"}),
        json!({"drive_id": DRIVE, "db_table": "public.customers", "key_col": "id; drop", "name_col": "name"}),
        json!({"drive_id": DRIVE, "db_table": "public.customers", "key_col": "id", "name_col": "name", "entity_type": "Nothing"}),
        json!({"drive_id": other.replace("d2", "ee"), "db_table": "public.customers", "key_col": "id", "name_col": "name"}),
    ] {
        let r = e.req("PUT", "/v1/docs/link-rules", &[], Some(&bad)).await;
        assert!(r.status == 400 || r.status == 404, "{bad}: {}", r.status);
    }
    let put = json!({"drive_id": DRIVE, "db_table": "public.customers", "key_col": "id", "name_col": "name", "entity_type": "Organization"});
    let r = e.req("PUT", "/v1/docs/link-rules", &[], Some(&put)).await;
    conforms("LinkRule", &r.json());
    assert_eq!((r.status, r.json()["revision"].as_i64()), (200, Some(1)));
    let r = e.req("PUT", "/v1/docs/link-rules", &[], Some(&put)).await;
    assert_eq!(r.json()["revision"], 1, "the same rule keeps its revision");
    let r = e
        .get(&format!("/v1/docs/link-rules?drive_id={DRIVE}"))
        .await;
    assert_eq!(r.json()["items"].as_array().unwrap().len(), 1);

    // Project now, then look up both ways.
    let r = e
        .post("/v1/docs/links:project", &json!({"drive_id": DRIVE}))
        .await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    conforms("ProjectRuns", &r.json());
    assert_eq!(r.json()["runs"][0]["links"]["created"], 2, "{}", r.json());
    let by_row = e
        .get("/v1/docs/links?db_table=public.customers&db_key=17")
        .await
        .json();
    conforms("LinkList", &by_row);
    let item = &by_row["items"][0];
    assert_eq!(by_row["agent_db"], "bound");
    assert_eq!(
        (item["db_key"].as_str(), item["canonical_name"].as_str()),
        (Some("17"), Some("澄海物流"))
    );
    assert_eq!(item["status"], "auto");
    assert_eq!(
        item["provenance"]["file"][0]["path"],
        "Contracts/澄海物流合作合同.md"
    );
    assert!(item["provenance"]["file"][0]["anchor"]["kind"].is_string());
    assert_eq!(item["provenance"]["table_row"]["table"], "public.customers");
    assert_eq!(item["provenance"]["table_row"]["rule_revision"], 1);
    let ekey = item["doc_entity_key"].as_str().unwrap().to_owned();
    let by_doc = e.get(&format!("/v1/docs/links?doc={ekey}")).await.json();
    assert_eq!(by_doc["items"][0]["db_key"], "17");
    // Validation of the query.
    for q in [
        "",
        "?db_key=17",
        "?db_table=public.c;x",
        "?doc=k&status=maybe",
    ] {
        assert_eq!(
            e.get(&format!("/v1/docs/links{q}")).await.status,
            400,
            "{q}"
        );
    }

    // Agent tokens: scoped to the Drives, and writing needs docs:write.
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
    let tok_b = issue(json!({"name": "rb", "scopes": ["docs:read"], "drive_ids": [other]})).await;
    let tok_w =
        issue(json!({"name": "wa", "scopes": ["docs:read", "docs:write"], "drive_ids": [DRIVE]}))
            .await;
    let tok_wb =
        issue(json!({"name": "wb", "scopes": ["docs:read", "docs:write"], "drive_ids": [other]}))
            .await;
    let call = |t: &str, m: &str, p: &str, b: Option<Value>| {
        let auth = format!("Bearer {t}");
        let addr = e.addr;
        let (m, p) = (m.to_owned(), p.to_owned());
        async move {
            let text = b.map(|b| b.to_string());
            request(addr, &m, &p, &[("Authorization", &auth)], text.as_deref()).await
        }
    };
    let lookup = "/v1/docs/links?db_table=public.customers&db_key=17";
    let r = call(&tok_a, "GET", lookup, None).await;
    assert_eq!(
        r.json()["items"].as_array().unwrap().len(),
        1,
        "a token of the Drive sees the link"
    );
    let r = call(&tok_b, "GET", lookup, None).await;
    assert_eq!(r.status, 200);
    assert_eq!(
        r.json()["items"].as_array().unwrap().len(),
        0,
        "a token of another Drive sees nothing"
    );
    let r = call(&tok_b, "GET", &format!("{lookup}&drive_id={DRIVE}"), None).await;
    assert_eq!(r.status, 403);
    let decide = json!({"drive_id": DRIVE, "doc_entity_key": ekey, "db_table": "public.customers", "db_key": "17", "status": "confirmed"});
    assert_eq!(
        call(
            &tok_a,
            "POST",
            "/v1/docs/links:decide",
            Some(decide.clone())
        )
        .await
        .status,
        403
    );
    assert_eq!(
        call(
            &tok_wb,
            "POST",
            "/v1/docs/links:decide",
            Some(decide.clone())
        )
        .await
        .status,
        403,
        "another Drive"
    );
    let r = call(&tok_w, "POST", "/v1/docs/links:decide", Some(decide)).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let r = e
        .get("/v1/docs/links?status=confirmed&db_table=public.customers")
        .await
        .json();
    assert_eq!(r["items"].as_array().unwrap().len(), 1);
    // The control token rejects a link that is not there.
    let r = e
        .req("POST", "/v1/docs/links:decide", &[], Some(&json!({"drive_id": DRIVE, "doc_entity_key": ekey, "db_table": "public.customers", "db_key": "999", "status": "rejected"})))
        .await;
    assert_eq!(r.status, 404);

    // Unbinding stops the job; what it wrote stays.
    assert_eq!(
        e.req("DELETE", "/v1/docs/agent-db-binding", &[], None)
            .await
            .status,
        200
    );
    assert_eq!(
        e.req("DELETE", "/v1/docs/agent-db-binding", &[], None)
            .await
            .status,
        404
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM _oo_docproj.entity_link")
        .fetch_one(&mut agent)
        .await
        .unwrap();
    assert_eq!(n, 2);
    // The rule is removed by the control token.
    let r = e
        .req(
            "DELETE",
            &format!("/v1/docs/link-rules?drive_id={DRIVE}&db_table=public.customers"),
            &[],
            None,
        )
        .await;
    assert_eq!(r.status, 200);
    e.finish().await;
}
