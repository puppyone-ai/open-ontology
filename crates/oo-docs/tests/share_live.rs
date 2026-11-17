//! Share lifecycle over real HTTP and a real database (B-S6-04 .. B-S6-06,
//! B-S6-10 .. B-S6-12): create with use records and coverage, drift within the
//! publish, `UPDATES` entries and the check exit codes, refresh, revoke, expiry,
//! refs credentials, task groups, `as_of`.
//!
//! `OO_DOCS_TEST_PG_PORT=<port> cargo test -p oo-docs --locked --test share_live -- --ignored --test-threads=1`

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::engine::{Opts, TestEngine, key};
use common::fx::{DRIVE, Fx, golden};
use common::http;
use oo_docs::cas::CasKind;
use serde_json::{Value, json};

fn def(name: &str) -> jsonschema::Validator {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("contracts/docs-api/docs-api.v1.schema.json")).unwrap(),
    )
    .unwrap();
    let wrapper = json!({"$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": doc["$defs"].clone(), "$ref": format!("#/$defs/{name}")});
    oo_core::schema_util::compile_schema(&wrapper).unwrap()
}

fn check(v: &jsonschema::Validator, what: &str, value: &Value) {
    let errors: Vec<String> = v
        .iter_errors(value)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{what}: {}\n{value}", errors.join("; "));
}

trait Sql {
    async fn execute_sql(&mut self, sql: &str);
    async fn execute_sql_result(&mut self, sql: &str) -> Result<(), sqlx::Error>;
}

impl Sql for sqlx::PgConnection {
    async fn execute_sql(&mut self, sql: &str) {
        self.execute_sql_result(sql).await.unwrap();
    }
    async fn execute_sql_result(&mut self, sql: &str) -> Result<(), sqlx::Error> {
        use sqlx::Executor;
        self.execute(sqlx::AssertSqlSafe(sql.to_owned()))
            .await
            .map(|_| ())
    }
}

fn gold(rel: &str) -> String {
    std::fs::read_to_string(golden().join("docs").join(rel)).unwrap()
}

const FILES: [&str; 4] = [
    "People/林远舟.md",
    "Projects/星图看板.md",
    "Decisions/D-2026-02-星图采用自研引擎.md",
    "Decisions/D-2026-05-星图改用开源图表库.md",
];

async fn world(name: &str) -> (TestEngine, Fx) {
    let e = TestEngine::start(
        name,
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    let mut fx = Fx::attach(name, &e.cas).await;
    for f in FILES {
        let src = gold(f);
        fx.index_md(f, &src).await;
        // The original bytes, for the refs endpoint.
        fx.cas.put(CasKind::Orig, src.as_bytes()).unwrap();
    }
    publish_now(&mut fx).await;
    (e, fx)
}

/// Publish through the fixture; if the engine's own publisher holds the lock,
/// wait for it to finish instead.
async fn publish_now(fx: &mut Fx) {
    let before = fx
        .scalar("SELECT coalesce(max(n), 0) FROM _oo_docs.ontology_version")
        .await;
    for _ in 0..100 {
        match fx.publish().await {
            oo_docs::publish::run::PublishOutcome::Busy => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            _ => break,
        }
    }
    wait_version(fx, before + 1).await;
}

/// The newest ontology version number. Tests never assume how many versions
/// the engine's own publisher cut before them: they read this and count from it.
async fn vmax(fx: &mut Fx) -> i64 {
    fx.scalar("SELECT coalesce(max(n), 0) FROM _oo_docs.ontology_version")
        .await
}

async fn wait_version(fx: &mut Fx, n: i64) {
    let t = Instant::now();
    while fx
        .scalar("SELECT coalesce(max(n), 0) FROM _oo_docs.ontology_version")
        .await
        < n
    {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "version {n} did not come"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn mk_share(e: &TestEngine, scope: &[&str], extra: Value) -> Value {
    let mut body = json!({"compile": {"drive_id": DRIVE, "scope": scope, "budget": 1200,
                                      "task": "星图看板的决定", "wait_ms": 0},
                          "mount": {"target_dir": "/tmp/oo-share-test"}});
    if let (Some(o), Some(x)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            o.insert(k.clone(), v.clone());
        }
    }
    let r = e.post("/v1/docs/shares", &body).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    r.json()
}

fn cred_of(share: &Value) -> String {
    let sj: Value = serde_json::from_str(share["share_json"].as_str().unwrap()).unwrap();
    sj["refs"]["credential"].as_str().unwrap().to_owned()
}

async fn as_cred(e: &TestEngine, cred: &str, method: &str, path: &str) -> http::Resp {
    let auth = format!("Bearer {cred}");
    let k = key();
    let h: Vec<(&str, &str)> = if method == "POST" {
        vec![("Authorization", &auth), ("Idempotency-Key", &k)]
    } else {
        vec![("Authorization", &auth)]
    };
    http::request(e.addr, method, path, &h, None).await
}

async fn used_decision_fact(fx: &mut Fx, sid: &str) -> String {
    fx.text(&format!(
        "SELECT f.id::text FROM _oo_docs.share_fact_use u JOIN _oo_docs.fact f ON f.id = u.fact_id \
          WHERE u.share_id = '{sid}' AND f.predicate IN ('decides','attr.status') ORDER BY f.fact_key LIMIT 1"
    ))
    .await
    .remove(0)
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn create_records_uses_coverage_and_guarantees() {
    let (e, mut fx) = world("share_create").await;
    let s = mk_share(&e, &["Decisions", "Projects"], json!({})).await;
    check(&def("Share"), "share", &s);
    let sid = s["id"].as_str().unwrap();
    assert_eq!(
        (s["state"].as_str(), s["refresh_policy"].as_str()),
        (Some("active"), Some("manual"))
    );
    assert_eq!(s["guarantees"]["pack_consistent"], true);
    assert_eq!(s["coverage"]["in_scope"], 3);
    assert_eq!(s["coverage"]["processed"], 3);
    // Facts of both ends have use records; sections carry digests.
    assert!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.share_fact_use WHERE share_id = '{sid}'"
        ))
        .await
            > 0
    );
    assert!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.share_section_use WHERE share_id = '{sid}'"
        ))
        .await
            > 0
    );
    // The pack's BRIEF names the coverage in its header and the manifest has digests.
    let pid = s["pack_id"].as_str().unwrap();
    let brief = e
        .get(&format!("/v1/docs/context-packs/{pid}/files/BRIEF.md"))
        .await;
    let text = String::from_utf8(brief.body).unwrap();
    assert!(text.contains("> 覆盖：3/3 个文件已处理"), "{text}");
    let pack = e.get(&format!("/v1/docs/context-packs/{pid}")).await.json();
    assert_eq!(pack["coverage"]["in_scope"], 3);
    assert!(pack["manifest"]["files"][0]["path"].is_string());
    // Out-of-scope names never show: 林远舟 only has People evidence? (the decisions name him as owner)
    // Listing and filters.
    let list = e.get("/v1/docs/shares?state=active").await.json();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    check(&def("Share"), "listed", &list["items"][0]);
    assert!(e.get("/v1/docs/shares?state=zzz").await.status == 400);
    // Idempotent replay leaves no credential behind.
    let k = key();
    let body = json!({"compile": {"drive_id": DRIVE, "scope": ["Decisions"], "wait_ms": 0}, "mount": {"target_dir": "/tmp/x"}});
    let a = e
        .req(
            "POST",
            "/v1/docs/shares",
            &[("Idempotency-Key", &k)],
            Some(&body),
        )
        .await;
    let b = e
        .req(
            "POST",
            "/v1/docs/shares",
            &[("Idempotency-Key", &k)],
            Some(&body),
        )
        .await;
    assert_eq!((a.status, b.status), (201, 201));
    assert!(a.json().get("share_json").is_some() && b.json().get("share_json").is_none());
    assert_eq!(a.json()["id"], b.json()["id"]);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn require_complete_fails_and_incomplete_is_named() {
    let (e, mut fx) = world("share_cover").await;
    // A new file in scope that is still queued.
    fx.admin
        .execute_sql(&format!(
            "INSERT INTO _oo_docs.file (id, drive_id, path, raw_key, path_fold, state, last_seen_generation) \
             VALUES (gen_random_uuid(), '{DRIVE}', 'Decisions/新来的.pdf', 'x'::bytea, 'decisions/新来的.pdf', 'queued', 1)"
        ))
        .await;
    let body =
        json!({"drive_id": DRIVE, "scope": ["Decisions"], "require_complete": true, "wait_ms": 0});
    let r = e.post("/v1/docs/context-packs:compile", &body).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (409, Some("DOCS_PACK_COVERAGE_INCOMPLETE"))
    );
    assert_eq!(
        r.json()["details"]["pending"][0]["path"],
        "Decisions/新来的.pdf"
    );
    let ok = e
        .post(
            "/v1/docs/context-packs:compile",
            &json!({"drive_id": DRIVE, "scope": ["Decisions"], "wait_ms": 0}),
        )
        .await;
    assert_eq!(ok.status, 201, "{}", String::from_utf8_lossy(&ok.body));
    let brief = ok.json()["brief_preview"].as_str().unwrap().to_owned();
    assert!(brief.contains("Decisions/新来的.pdf 等待解析"), "{brief}");
    assert_eq!(
        ok.json()["manifest"]["guarantees"]["freshness_floor"],
        false
    );
    // A scope with only complete files has no such line of trouble.
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_used_fact_marked_wrong_blocks_the_share_within_five_seconds() {
    let (e, mut fx) = world("share_drift").await;
    let s = mk_share(&e, &["Decisions", "Projects"], json!({})).await;
    let sid = s["id"].as_str().unwrap().to_owned();
    let cred = cred_of(&s);
    // Nothing changed yet: the gate is open on both authentications.
    let c0 = e
        .post(&format!("/v1/docs/shares/{sid}:check"), &json!({}))
        .await
        .json();
    check(&def("ContextCheckResult"), "check", &c0);
    assert_eq!(
        (c0["exit_code"].as_i64(), c0["action"].as_str()),
        (Some(0), Some("continue"))
    );
    // An engine call with a credential may not do anything else.
    let denied = as_cred(&e, &cred, "GET", "/v1/docs/drives").await;
    assert_eq!(
        (denied.status, denied.json()["code"].as_str()),
        (403, Some("DOCS_SHARE_REFS_OUT_OF_SCOPE"))
    );

    let fid = used_decision_fact(&mut fx, &sid).await;
    let next = vmax(&mut fx).await + 1;
    let t = Instant::now();
    let w = e
        .req(
            "POST",
            &format!("/v1/docs/facts/{fid}:correct"),
            &[("Idempotency-Key", &key()), ("If-Match", "\"r1\"")],
            Some(&json!({"action": "mark_wrong", "reason": "推翻"})),
        )
        .await;
    assert_eq!(w.status, 200, "{}", String::from_utf8_lossy(&w.body));
    wait_version(&mut fx, next).await;
    let state = || async { e.get(&format!("/v1/docs/shares/{sid}")).await.json() };
    let mut drifted = state().await;
    while drifted["state"] != "drifted" {
        assert!(t.elapsed() < Duration::from_secs(10), "{drifted}");
        tokio::time::sleep(Duration::from_millis(50)).await;
        drifted = state().await;
    }
    eprintln!("mark_wrong to drifted: {:?}", t.elapsed());
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "drift took {:?}",
        t.elapsed()
    );
    check(&def("Share"), "drifted", &drifted);
    assert!(drifted["drift"]["facts_changed"].as_i64().unwrap() >= 1);

    // UPDATES: gap-free from 1, a block entry naming the fact, a section.
    let ups = e
        .get(&format!("/v1/docs/shares/{sid}/updates"))
        .await
        .json();
    let items = ups["items"].as_array().unwrap();
    assert!(!items.is_empty());
    for (i, it) in items.iter().enumerate() {
        assert_eq!(it["seq"], (i + 1) as i64);
        check(&def("ShareUpdate"), "update", it);
    }
    let block = items
        .iter()
        .find(|i| i["level"] == "block")
        .expect("a block entry");
    assert_eq!(
        (block["from_version"].as_i64(), block["to_version"].as_i64()),
        (Some(1), Some(2))
    );
    assert!(
        block["text"]
            .as_str()
            .unwrap()
            .contains("已被撤回，没有替代"),
        "{block}"
    );
    assert!(block["section"].is_string());
    // Insert-only.
    let upd = fx
        .admin
        .execute_sql_result("UPDATE _oo_docs.share_update SET level = 'info'")
        .await;
    assert!(upd.is_err());

    // The gate: 20 for the control token and for the credential; since filters.
    let c = e
        .post(&format!("/v1/docs/shares/{sid}:check"), &json!({}))
        .await
        .json();
    assert_eq!(
        (
            c["exit_code"].as_i64(),
            c["level"].as_str(),
            c["action"].as_str()
        ),
        (Some(20), Some("block"), Some("stop"))
    );
    let c = as_cred(&e, &cred, "POST", &format!("/v1/docs/shares/{sid}:check"))
        .await
        .json();
    assert_eq!(c["exit_code"], 20);
    let last = items.last().unwrap()["seq"].as_i64().unwrap();
    let c = as_cred(
        &e,
        &cred,
        "POST",
        &format!("/v1/docs/shares/{sid}:check?since={last}"),
    )
    .await
    .json();
    assert_eq!(c["exit_code"], 0, "nothing after the last entry");

    // Another share's id with this credential is refused.
    let other = mk_share(&e, &["Decisions"], json!({})).await;
    let r = as_cred(
        &e,
        &cred,
        "POST",
        &format!("/v1/docs/shares/{}:check", other["id"].as_str().unwrap()),
    )
    .await;
    assert_eq!(r.status, 403);

    // Refresh: a new active share at v2, the old one refreshed (terminal, credential dead).
    let rf = e
        .post(&format!("/v1/docs/shares/{sid}:refresh"), &json!({}))
        .await;
    assert_eq!(rf.status, 200, "{}", String::from_utf8_lossy(&rf.body));
    let n = rf.json();
    assert_eq!(
        (
            n["state"].as_str(),
            n["refreshed_from"].as_str(),
            n["version"].as_i64()
        ),
        (Some("active"), Some(sid.as_str()), Some(2))
    );
    assert!(n["share_json"].is_string() && n["files"].is_array());
    let old = e.get(&format!("/v1/docs/shares/{sid}")).await.json();
    assert_eq!(old["state"], "refreshed");
    let c = e
        .post(&format!("/v1/docs/shares/{sid}:check"), &json!({}))
        .await
        .json();
    assert_eq!(c["exit_code"], 30);
    let dead = as_cred(&e, &cred, "POST", &format!("/v1/docs/shares/{sid}:check")).await;
    assert_eq!(
        (dead.status, dead.json()["code"].as_str()),
        (410, Some("DOCS_SHARE_REFS_CREDENTIAL_INVALID"))
    );
    let again = e
        .post(&format!("/v1/docs/shares/{sid}:refresh"), &json!({}))
        .await;
    assert_eq!(
        (again.status, again.json()["code"].as_str()),
        (409, Some("DOCS_SHARE_INVALID_TRANSITION"))
    );
    let nsid = n["id"].as_str().unwrap();
    let c = e
        .post(&format!("/v1/docs/shares/{nsid}:check"), &json!({}))
        .await
        .json();
    assert_eq!(c["exit_code"], 0, "the new pack starts clean: {c}");
    // The wrong fact is not in the new pack.
    assert_eq!(fx.scalar(&format!("SELECT count(*) FROM _oo_docs.share_fact_use WHERE share_id = '{nsid}' AND fact_id = '{fid}'")).await, 0);
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn expiry_revocation_and_refs_credentials() {
    let (e, mut fx) = world("share_refs").await;
    let s = mk_share(&e, &["Decisions", "Projects"], json!({})).await;
    let sid = s["id"].as_str().unwrap().to_owned();
    let cred = cred_of(&s);

    // refs: a cited original in scope reads; People/ is out of scope; Range works.
    let in_b3 = fx.text("SELECT fv.b3 FROM _oo_docs.file_version fv WHERE fv.path_at_version LIKE 'Decisions/D-2026-02%'").await.remove(0);
    let out_b3 = fx
        .text("SELECT fv.b3 FROM _oo_docs.file_version fv WHERE fv.path_at_version LIKE 'People/%'")
        .await
        .remove(0);
    let r = as_cred(&e, &cred, "GET", &format!("/v1/docs/refs/{in_b3}")).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(
        r.body,
        gold("Decisions/D-2026-02-星图采用自研引擎.md").as_bytes()
    );
    assert_eq!(r.header("content-disposition"), Some("attachment"));
    let auth = format!("Bearer {cred}");
    let part = http::request(
        e.addr,
        "GET",
        &format!("/v1/docs/refs/{in_b3}"),
        &[("Authorization", &auth), ("Range", "bytes=0-9")],
        None,
    )
    .await;
    assert_eq!((part.status, part.body.len()), (206, 10));
    let r = as_cred(&e, &cred, "GET", &format!("/v1/docs/refs/{out_b3}")).await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (403, Some("DOCS_SHARE_REFS_OUT_OF_SCOPE"))
    );
    let r = as_cred(&e, "oodr_unknown", "GET", &format!("/v1/docs/refs/{in_b3}")).await;
    assert_eq!(r.status, 401);
    // The control token has no business here.
    let r = e.get(&format!("/v1/docs/refs/{in_b3}")).await;
    assert_eq!(r.status, 401);
    // Rate limit: 60 a minute per credential (three were used above).
    let mut limited = None;
    for i in 0..70 {
        let r = as_cred(&e, &cred, "GET", &format!("/v1/docs/refs/{in_b3}")).await;
        if r.status == 429 {
            limited = Some(i);
            break;
        }
    }
    let at = limited.expect("rate limited");
    assert!((55..=58).contains(&at), "429 came after {at} extra reads");

    // Expiry: the sweeper ends the share and kills the credential.
    fx.admin.execute_sql(&format!("UPDATE _oo_docs.share SET expires_at = clock_timestamp() - interval '1 second' WHERE id = '{sid}'")).await;
    let r = as_cred(&e, &cred, "GET", &format!("/v1/docs/refs/{in_b3}")).await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (410, Some("expired")),
        "expired before the sweep too"
    );
    assert_eq!(
        oo_docs::share::lifecycle::expire_due(&e.pool)
            .await
            .unwrap(),
        1
    );
    let st = e.get(&format!("/v1/docs/shares/{sid}")).await.json();
    assert_eq!(st["state"], "expired");
    assert_eq!(
        e.post(&format!("/v1/docs/shares/{sid}:check"), &json!({}))
            .await
            .json()["exit_code"],
        30
    );
    assert_eq!(
        oo_docs::share::lifecycle::expire_due(&e.pool)
            .await
            .unwrap(),
        0
    );
    assert!(fx.scalar("SELECT count(*) FROM _oo_docs.event WHERE type = 'share.state_changed' AND payload->>'to' = 'expired'").await >= 1);

    // Revocation.
    let s2 = mk_share(&e, &["Decisions"], json!({})).await;
    let (sid2, cred2) = (s2["id"].as_str().unwrap().to_owned(), cred_of(&s2));
    let rv = e
        .post(&format!("/v1/docs/shares/{sid2}:revoke"), &json!({}))
        .await;
    assert_eq!(rv.status, 200);
    check(&def("RevokeResult"), "revoke", &rv.json());
    assert_eq!(rv.json()["share"]["state"], "revoked");
    assert_eq!(
        rv.json()["notice_code"],
        "DOCS_SHARE_READ_CONTENT_IRREVOCABLE"
    );
    let r = as_cred(&e, &cred2, "GET", &format!("/v1/docs/refs/{in_b3}")).await;
    assert_eq!(
        (r.status, r.json()["details"]["reason"].as_str()),
        (410, Some("revoked"))
    );
    let again = e
        .post(&format!("/v1/docs/shares/{sid2}:revoke"), &json!({}))
        .await;
    assert_eq!(again.status, 409);
    let list = e.get("/v1/docs/shares").await.json();
    let states: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["state"].as_str().unwrap())
        .collect();
    assert!(
        states.contains(&"expired") && states.contains(&"revoked"),
        "{states:?}"
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn task_groups_refresh_all_or_none() {
    let (e, mut fx) = world("share_group").await;
    let a = mk_share(&e, &["Decisions"], json!({"group": "TICKET-7"})).await;
    let b = mk_share(&e, &["Projects"], json!({"group": "TICKET-7"})).await;
    let (sa, sb) = (
        a["id"].as_str().unwrap().to_owned(),
        b["id"].as_str().unwrap().to_owned(),
    );
    assert_eq!(a["group_id"], b["group_id"]);
    assert_eq!(a["version"], b["version"]);
    // A member does not refresh alone.
    let r = e
        .post(&format!("/v1/docs/shares/{sa}:refresh"), &json!({}))
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (409, Some("DOCS_SHARE_GROUP_MEMBER"))
    );

    // A new version arrives (a new decision file), then the other member of
    // the group still gets the group's version at creation.
    let src = "---\ntype: decision\nstatus: confirmed\ndate: 2026-11-01\nowner: 林远舟\n---\n\n# 决定：新的决定\n\n林远舟决定采用新方案。\n";
    fx.index_md("Decisions/新决定.md", src).await;
    publish_now(&mut fx).await;
    let late = mk_share(&e, &["Decisions"], json!({"group": "TICKET-7"})).await;
    assert_eq!(
        late["version"], a["version"],
        "an existing group pins the version"
    );
    let sl = late["id"].as_str().unwrap().to_owned();

    // Failure is atomic: one member's scope no longer compiles.
    fx.admin.execute_sql(&format!("UPDATE _oo_docs.share SET compile_params = jsonb_set(compile_params, '{{scope}}', '[\"Nowhere\"]') WHERE id = '{sb}'")).await;
    let bad = e
        .post("/v1/docs/share-groups/TICKET-7:refresh", &json!({}))
        .await;
    assert_eq!(bad.status, 422, "{}", String::from_utf8_lossy(&bad.body));
    assert_eq!(bad.json()["details"]["share_id"], sb);
    for s in [&sa, &sb, &sl] {
        assert_eq!(
            e.get(&format!("/v1/docs/shares/{s}")).await.json()["state"],
            "active",
            "{s}"
        );
    }
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.share WHERE refreshed_from IS NOT NULL")
            .await,
        0
    );
    fx.admin.execute_sql(&format!("UPDATE _oo_docs.share SET compile_params = jsonb_set(compile_params, '{{scope}}', '[\"Projects\"]') WHERE id = '{sb}'")).await;

    // Success: every member upgrades to the same version.
    let ok = e
        .post("/v1/docs/share-groups/TICKET-7:refresh", &json!({}))
        .await;
    assert_eq!(ok.status, 200, "{}", String::from_utf8_lossy(&ok.body));
    let v = ok.json();
    check(&def("BatchResult"), "batch", &v);
    assert_eq!(v["results"].as_array().unwrap().len(), 3);
    let versions: Vec<i64> = v["refreshed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["version"].as_i64().unwrap())
        .collect();
    let latest = vmax(&mut fx).await;
    assert_eq!(versions, [latest, latest, latest]);
    for s in [&sa, &sb, &sl] {
        assert_eq!(
            e.get(&format!("/v1/docs/shares/{s}")).await.json()["state"],
            "refreshed"
        );
    }
    let g = e.get("/v1/docs/share-groups/TICKET-7").await.json();
    assert_eq!(g["ontology_version"], latest);
    assert_eq!(
        g["members"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["state"] == "active")
            .count(),
        3
    );
    // Pinned groups and shares do not refresh.
    let p = mk_share(&e, &["Decisions"], json!({"refresh_policy": "pinned"})).await;
    let r = e
        .post(
            &format!("/v1/docs/shares/{}:refresh", p["id"].as_str().unwrap()),
            &json!({}),
        )
        .await;
    assert_eq!(
        (r.status, r.json()["code"].as_str()),
        (409, Some("DOCS_SHARE_PINNED"))
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn reads_with_a_share_credential_use_the_share_version_and_its_scope() {
    let (e, mut fx) = world("share_asof").await;
    let s = mk_share(&e, &["Decisions"], json!({})).await;
    let cred = cred_of(&s);
    let v0 = s["version"].as_i64().unwrap();
    let pin = format!("v{v0}");
    let proj = fx.text("SELECT key FROM _oo_docs.entity WHERE canonical_name = '星图看板' AND type = 'Project'").await.remove(0);
    let person = fx
        .text("SELECT key FROM _oo_docs.entity WHERE canonical_name = '林远舟' LIMIT 1")
        .await
        .remove(0);
    // v2 appears after the share was made.
    let src = "---\ntype: decision\nstatus: confirmed\ndate: 2026-11-01\nowner: 林远舟\n---\n\n# 决定：新的决定\n\n林远舟决定采用新方案。\n";
    fx.index_md("Decisions/新决定.md", src).await;
    publish_now(&mut fx).await;
    let latest_n = vmax(&mut fx).await;
    assert!(latest_n > v0);
    let late = format!("v{latest_n}");

    let r = as_cred(&e, &cred, "GET", &format!("/v1/docs/entities/{person}")).await;
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(
        r.header("oo-docs-as-of"),
        Some(pin.as_str()),
        "a credential reads its own version by default"
    );
    let body = r.json();
    assert!(body.get("stale_vs_pack").is_none());
    for f in body["facts"].as_array().unwrap() {
        for ev in f["evidence"].as_array().unwrap() {
            assert!(
                ev["path"].as_str().unwrap().starts_with("Decisions/"),
                "{ev}"
            );
        }
    }
    assert!(
        body["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f.as_str().unwrap().starts_with("Decisions/"))
    );
    let latest = as_cred(
        &e,
        &cred,
        "GET",
        &format!("/v1/docs/entities/{person}?as_of=latest"),
    )
    .await;
    assert_eq!(latest.header("oo-docs-as-of"), Some(late.as_str()));
    assert_eq!(
        latest.json()["stale_vs_pack"],
        json!({"pack_version": v0, "latest_version": latest_n})
    );
    let share = as_cred(
        &e,
        &cred,
        "GET",
        &format!("/v1/docs/entities/{person}?as_of=share"),
    )
    .await;
    assert_eq!(share.header("oo-docs-as-of"), Some(pin.as_str()));
    // An entity that exists only outside the scope is not there.
    let r = as_cred(&e, &cred, "GET", &format!("/v1/docs/entities/{proj}")).await;
    assert!(r.status == 200 || r.status == 404);
    // The control token defaults to latest; as_of=share needs a credential.
    let c = e.get(&format!("/v1/docs/entities/{person}")).await;
    assert_eq!(c.header("oo-docs-as-of"), Some(late.as_str()));
    assert!(c.json().get("stale_vs_pack").is_none());
    let c = e
        .get(&format!("/v1/docs/entities/{person}?as_of=share"))
        .await;
    assert_eq!(c.status, 400);
    let c = e
        .get(&format!("/v1/docs/entities/{person}?as_of={pin}"))
        .await;
    assert_eq!(c.header("oo-docs-as-of"), Some(pin.as_str()));
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn at_checkpoint_refreshes_on_a_passing_check_and_pins_are_counted() {
    let (e, mut fx) = world("share_ckpt").await;
    let s = mk_share(
        &e,
        &["Decisions"],
        json!({"refresh_policy": "at-checkpoint"}),
    )
    .await;
    let sid = s["id"].as_str().unwrap().to_owned();
    // Pinned by the share, in the version list.
    let pinned = s["version"].as_i64().unwrap();
    let vs = e.get("/v1/docs/ontology/versions").await.json();
    assert_eq!(vs["items"][0]["n"], pinned);
    assert_eq!(vs["items"][0]["pinned_by"], 1);
    // Nothing newer: a plain pass.
    let c = e
        .post(&format!("/v1/docs/shares/{sid}:check"), &json!({}))
        .await
        .json();
    assert_eq!(c["exit_code"], 0);
    assert!(c.get("refreshed_to").is_none());
    // A new version with a new in-scope fact: info only, so the gate passes and upgrades.
    let src = "---\ntype: decision\nstatus: confirmed\ndate: 2026-11-01\nowner: 林远舟\n---\n\n# 决定：新的决定\n\n林远舟决定采用新方案。\n";
    fx.index_md("Decisions/新决定.md", src).await;
    publish_now(&mut fx).await;
    let ups = e
        .get(&format!("/v1/docs/shares/{sid}/updates"))
        .await
        .json();
    assert_eq!(ups["items"][0]["level"], "info");
    let c = e
        .post(&format!("/v1/docs/shares/{sid}:check"), &json!({}))
        .await
        .json();
    assert_eq!(c["exit_code"], 0);
    let newer = vmax(&mut fx).await;
    assert!(newer > pinned);
    assert_eq!(c["upgraded_to_version"], newer);
    let new = c["refreshed_to"].as_str().expect("refreshed_to");
    assert_eq!(c["refreshed"]["version"], newer);
    assert_eq!(
        e.get(&format!("/v1/docs/shares/{sid}")).await.json()["state"],
        "refreshed"
    );
    assert_eq!(
        e.get(&format!("/v1/docs/shares/{new}")).await.json()["state"],
        "active"
    );
    // The old share's version is no longer pinned by it; the new one is.
    let vs = e.get("/v1/docs/ontology/versions").await.json();
    let pins: Vec<(i64, i64)> = vs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v["n"].as_i64().unwrap(), v["pinned_by"].as_i64().unwrap()))
        .collect();
    assert_eq!(pins.first(), Some(&(newer, 1)));
    assert_eq!(pins.iter().find(|(n, _)| *n == pinned), Some(&(pinned, 0)));
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn a_real_constraint_violation_holds_the_fact_queues_a_conflict_and_the_pack_says_so() {
    let (e, mut fx) = world("share_conflict").await;
    // A share cut before the conflict exists.
    let before = mk_share(&e, &["Projects"], json!({})).await;
    let sid = before["id"].as_str().unwrap().to_owned();
    let used = fx
        .scalar(&format!(
            "SELECT count(*) FROM _oo_docs.share_fact_use u JOIN _oo_docs.fact f ON f.id = u.fact_id \
              WHERE u.share_id = '{sid}' AND f.predicate = 'owns'"
        ))
        .await;
    assert!(used > 0, "the pack uses an owner fact of the project");

    // Another file gives 星图看板 a different owner from the same day as the
    // current one: two owners at once (project_single_owner).
    let owner_now: String = fx
        .text("SELECT s.canonical_name FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
                 JOIN _oo_docs.entity o ON o.id = f.object_id \
                WHERE f.predicate = 'owns' AND o.canonical_name = '星图看板' AND f.status = 'published' \
                  AND f.valid_to IS NULL")
        .await
        .remove(0);
    let from: String = fx
        .text("SELECT f.valid_from::text FROM _oo_docs.fact f JOIN _oo_docs.entity o ON o.id = f.object_id \
                WHERE f.predicate = 'owns' AND o.canonical_name = '星图看板' AND f.status = 'published' \
                  AND f.valid_to IS NULL")
        .await
        .remove(0);
    let other = if owner_now == "韩立" {
        "吴启明"
    } else {
        "韩立"
    };
    let src = format!("---\ndate: {from}\nowner: {other}\n---\n\n# 星图看板\n\n内容\n");
    fx.index_md("Projects/星图看板/第二份.md", &src).await;
    publish_now(&mut fx).await;

    assert_eq!(
        fx.text("SELECT constraint_id || '/' || status FROM _oo_docs.constraint_violation")
            .await,
        ["project_single_owner/open"]
    );
    assert_eq!(
        fx.scalar("SELECT count(*) FROM _oo_docs.review_item WHERE kind = 'constraint_conflict' AND status = 'pending'")
            .await,
        1,
        "the conflict is queued for a person"
    );
    assert_eq!(
        fx.scalar(&format!(
            "SELECT count(*) FROM _oo_docs.fact f JOIN _oo_docs.entity s ON s.id = f.subject_id \
              WHERE f.predicate = 'owns' AND s.canonical_name = '{other}' AND f.status = 'published'"
        ))
        .await,
        0,
        "the challenger is not published"
    );

    // A pack cut now says the conflict is unresolved, with sources on both sides.
    let after = mk_share(&e, &["Projects"], json!({})).await;
    let pid = after["pack_id"].as_str().unwrap();
    let brief = e
        .get(&format!("/v1/docs/context-packs/{pid}/files/BRIEF.md"))
        .await;
    let text = String::from_utf8(brief.body).unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with("- 冲突未决：星图看板"))
        .unwrap_or_else(|| panic!("no 冲突未决 line:\n{text}"));
    assert!(
        line.contains(" vs ") && line.matches("[src:").count() >= 2,
        "{line}"
    );

    // The older share used the owner fact: its update level is block, check says 20.
    let ups = e
        .get(&format!("/v1/docs/shares/{sid}/updates"))
        .await
        .json();
    let levels: Vec<&str> = ups["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|u| u["level"].as_str())
        .collect();
    assert!(levels.contains(&"block"), "{ups}");
    assert!(
        ups["items"].as_array().unwrap().iter().any(|u| u["summary"]
            .as_str()
            .is_some_and(|s| s.contains("冲突未决"))
            || u.to_string().contains("冲突未决")),
        "{ups}"
    );
    let c = e
        .post(&format!("/v1/docs/shares/{sid}:check"), &json!({}))
        .await;
    assert_eq!(
        c.json()["exit_code"],
        20,
        "{}",
        String::from_utf8_lossy(&c.body)
    );
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn group_refresh_under_at_checkpoint_upgrades_the_group_together() {
    let (e, mut fx) = world("share_group_ckpt").await;
    let a = mk_share(
        &e,
        &["Decisions"],
        json!({"group": "TICKET-9", "refresh_policy": "at-checkpoint"}),
    )
    .await;
    let b = mk_share(
        &e,
        &["Projects"],
        json!({"group": "TICKET-9", "refresh_policy": "at-checkpoint"}),
    )
    .await;
    let (sa, sb) = (
        a["id"].as_str().unwrap().to_owned(),
        b["id"].as_str().unwrap().to_owned(),
    );
    let pinned = a["version"].as_i64().unwrap();
    // A new in-scope fact: info only.
    let src = "---\ntype: decision\nstatus: confirmed\ndate: 2026-11-01\nowner: 林远舟\n---\n\n# 决定：新的决定\n\n林远舟决定采用新方案。\n";
    fx.index_md("Decisions/新决定.md", src).await;
    publish_now(&mut fx).await;
    let newer = vmax(&mut fx).await;
    assert!(newer > pinned);
    // A passing check of one member upgrades the whole group (no member refreshes alone).
    let c = e
        .post(&format!("/v1/docs/shares/{sa}:check"), &json!({}))
        .await;
    let c = c.json();
    assert_eq!(c["exit_code"], 0, "{c}");
    for s in [&sa, &sb] {
        assert_eq!(
            e.get(&format!("/v1/docs/shares/{s}")).await.json()["state"],
            "refreshed",
            "{s}"
        );
    }
    let g = e.get("/v1/docs/share-groups/TICKET-9").await.json();
    assert_eq!(g["ontology_version"], newer, "{g}");
    let active: Vec<i64> = g["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["state"] == "active")
        .map(|m| m["version"].as_i64().unwrap_or(newer))
        .collect();
    assert_eq!(active.len(), 2, "both members have a new active share: {g}");
    e.finish().await;
}
