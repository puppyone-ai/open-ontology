//! Contract tests (10 §8.7): what the engine sends is what
//! `contracts/docs-api/` says.
//!
//! Static tests run everywhere. The live ones (`--ignored`, need
//! `OO_DOCS_TEST_PG_PORT`) validate real responses of a running API against
//! the `$defs` of `docs-api.v1.schema.json` and each persistent event against
//! `events.v1.schema.json`.

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use common::engine::{Opts, TOKEN, TestEngine};
use common::http::open_sse;
use oo_docs::api::error::{ALL_CODES, ApiError, code};
use oo_docs::state::FileState;
use serde_json::{Value, json};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(rel: &str) -> Value {
    let text = std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// A validator for one `$def` of the API schema (shares `$defs` for `$ref`).
fn def(doc: &Value, name: &str) -> jsonschema::Validator {
    let wrapper = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": doc["$defs"].clone(),
        "$ref": format!("#/$defs/{name}"),
    });
    oo_core::schema_util::compile_schema(&wrapper).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn check(v: &jsonschema::Validator, what: &str, value: &Value) {
    let errors: Vec<String> = v
        .iter_errors(value)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{what}: {}\n{value}", errors.join("; "));
}

fn error_codes() -> Vec<Value> {
    load("contracts/docs-api/errors.v1.json")["codes"]
        .as_array()
        .expect("codes")
        .clone()
}

#[test]
fn every_route_error_code_is_in_the_contract() {
    let table = error_codes();
    let known: BTreeSet<&str> = table.iter().filter_map(|c| c["code"].as_str()).collect();
    for c in ALL_CODES {
        assert!(known.contains(c), "{c} is not in errors.v1.json");
    }
    // The envelope takes status and retryable from the table, and its body
    // is an `Error` of the API schema.
    let schema = load("contracts/docs-api/docs-api.v1.schema.json");
    let v = def(&schema, "Error");
    for entry in &table {
        let name = entry["code"].as_str().unwrap();
        if !ALL_CODES.contains(&name) {
            continue;
        }
        let c = [
            code::BAD_HOST,
            code::BAD_REQUEST,
            code::FORBIDDEN_ORIGIN,
            code::UNAUTHENTICATED,
            code::NOT_FOUND,
            code::METHOD_NOT_ALLOWED,
            code::PAYLOAD_TOO_LARGE,
            code::VERSION_UNSUPPORTED,
            code::IDEMPOTENCY_KEY_REQUIRED,
            code::IDEMPOTENCY_CONFLICT,
            code::PRECONDITION_REQUIRED,
            code::PRECONDITION_FAILED,
            code::RATE_LIMITED,
            code::CURSOR_INVALID,
            code::CURSOR_EXPIRED,
            code::DB_UNAVAILABLE,
            code::INTERNAL,
            code::DRIVE_NOT_FOUND,
            code::DRIVE_EXISTS,
            code::DRIVE_HAS_ACTIVE_SHARES,
            code::ROOT_MISSING,
            code::INVALID_CONFIG,
            code::SYNC_RUNNING,
            code::MASS_DELETE_HELD,
            code::PACK_SCOPE_INVALID,
            code::PACK_NOTHING_VISIBLE,
            code::PACK_SEED_NOT_VISIBLE,
            code::PACK_TASK_TOO_LONG,
            code::PACK_BUDGET_TOO_SMALL,
            code::PACK_VERSION_NOT_FOUND,
            code::PACK_LEAK_GUARD,
            code::PACK_PROVENANCE_GAP,
            code::PACK_TOO_LARGE,
            code::PACK_NOT_FOUND,
            code::SHARE_NOT_FOUND,
            code::SHARE_EXPIRY_INVALID,
            code::SHARE_ROLE_UNAVAILABLE,
            code::SHARE_TARGET_INVALID,
            code::PACK_COVERAGE_INCOMPLETE,
            code::SHARE_GROUP_MEMBER,
            code::SHARE_INVALID_TRANSITION,
            code::SHARE_PINNED,
            code::SHARE_REFS_CREDENTIAL_INVALID,
            code::SHARE_REFS_OUT_OF_SCOPE,
            code::SHARE_TARGET_OCCUPIED,
            code::SHARE_TARGET_WRITE_FAILED,
            code::PARSE_NOT_READY,
            code::REVIEW_BATCH_TOO_LARGE,
            code::REVIEW_INVALID_DECISION,
            code::REVIEW_MERGE_CONFLICT,
            code::SUGGEST_NOT_FOUND,
            code::SUGGEST_ROLE_FORBIDDEN,
            code::SUGGEST_SCOPE_MISSING,
            code::SUGGEST_NOT_PENDING,
            code::SUGGEST_NO_EVIDENCE,
            code::SUGGEST_UNRESOLVED_ENTITY,
        ]
        .into_iter()
        .find(|c| c.as_str() == name)
        .unwrap_or_else(|| panic!("{name} listed in ALL_CODES but not a constant"));
        let e = ApiError::new(c, "x");
        assert_eq!(
            u64::from(e.status),
            entry["http"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(e.retryable, entry["retryable"].as_bool().unwrap(), "{name}");
        check(&v, name, &e.body());
    }
    assert_eq!(ALL_CODES.len(), 56);
}

#[test]
fn error_details_use_the_documented_fields() {
    let table = error_codes();
    let fields = |name: &str| -> BTreeSet<String> {
        table.iter().find(|c| c["code"] == name).unwrap()["details"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };
    // The details each route fills in, by code.
    let used: &[(&str, &[&str])] = &[
        ("DOCS_ENGINE_BAD_REQUEST", &["pointer"]),
        ("DOCS_ENGINE_NOT_FOUND", &["resource"]),
        ("DOCS_ENGINE_PAYLOAD_TOO_LARGE", &["max_bytes"]),
        ("DOCS_ENGINE_VERSION_UNSUPPORTED", &["server"]),
        ("DOCS_ENGINE_PRECONDITION_FAILED", &["current_revision"]),
        ("DOCS_ENGINE_RATE_LIMITED", &["retry_after_ms"]),
        ("DOCS_ENGINE_CURSOR_EXPIRED", &["oldest_seq", "head_seq"]),
        ("DOCS_SOURCE_DRIVE_NOT_FOUND", &["drive_id"]),
        ("DOCS_SOURCE_DRIVE_EXISTS", &["drive_id"]),
        ("DOCS_SOURCE_DRIVE_HAS_ACTIVE_SHARES", &["share_ids"]),
        ("DOCS_SOURCE_SYNC_RUNNING", &["generation"]),
        (
            "DOCS_SOURCE_MASS_DELETE_HELD",
            &["generation", "would_delete"],
        ),
    ];
    for (name, keys) in used {
        let allowed = fields(name);
        for k in *keys {
            assert!(
                allowed.contains(*k),
                "{name}: {k} is not a documented detail"
            );
        }
    }
}

#[test]
fn state_enum_equals_the_state_module() {
    let schema = load("contracts/docs-api/docs-api.v1.schema.json");
    let spec: Vec<&str> = schema["$defs"]["FileState"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let ours: Vec<&str> = FileState::ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        ours, spec,
        "FileState::ALL and FileState in docs-api.v1.schema.json differ"
    );
    for s in &spec {
        assert_eq!(FileState::parse(s).map(FileState::as_str), Some(*s));
    }
    // The CHECK constraint of the migration lists the same states.
    let sql = std::fs::read_to_string(
        root().join("crates/oo-pipeline/migrations/0138_docs_sources_jobs.sql"),
    )
    .unwrap();
    for s in &spec {
        assert!(
            sql.contains(&format!("'{s}'")),
            "{s} missing from the file.state CHECK"
        );
    }
}

#[test]
fn engine_routes_match_the_endpoint_table() {
    let schema = load("contracts/docs-api/docs-api.v1.schema.json");
    let endpoints = schema["x-endpoints"].as_array().unwrap();
    let find = |m: &str, p: &str| {
        endpoints
            .iter()
            .find(|e| e["method"] == m && e["path"] == p)
            .unwrap_or_else(|| panic!("{m} {p} is not in x-endpoints"))
            .clone()
    };
    // (method, path, needs Idempotency-Key, needs If-Match) as the handlers enforce them.
    for (m, p, idem, if_match) in [
        ("GET", "/livez", false, false),
        ("GET", "/v1/docs/health", false, false),
        ("GET", "/v1/docs/drives", false, false),
        ("POST", "/v1/docs/drives", true, false),
        ("GET", "/v1/docs/drives/{id}", false, false),
        ("PATCH", "/v1/docs/drives/{id}", false, true),
        ("POST", "/v1/docs/drives/{id}:rebuild", true, false),
        ("DELETE", "/v1/docs/drives/{id}", false, true),
        ("POST", "/v1/docs/drives/{id}:sync", true, false),
        ("GET", "/v1/docs/drives/{id}/files", false, false),
        ("GET", "/v1/docs/drives/{id}/tree", false, false),
        ("GET", "/v1/docs/events", false, false),
        ("GET", "/v1/docs/suggestions", false, false),
        ("POST", "/v1/docs/suggestions", false, false),
        ("GET", "/v1/docs/suggestions:digest", false, false),
        ("GET", "/v1/docs/suggestions/{id}", false, false),
        ("GET", "/v1/docs/suggestions/{id}/diff", false, false),
        ("POST", "/v1/docs/suggestions/{id}:decide", false, false),
    ] {
        let e = find(m, p);
        assert_eq!(e["idempotency_key"].as_bool().unwrap(), idem, "{m} {p}");
        assert_eq!(e["if_match"].as_bool().unwrap(), if_match, "{m} {p}");
    }
}

#[test]
fn event_types_written_by_the_engine_exist_in_the_event_schema() {
    let schema = load("contracts/docs-api/events.v1.schema.json");
    let known: BTreeSet<&str> = schema["properties"]["type"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let persistent: BTreeSet<&str> = schema["x-persistent"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for t in [
        "drive.created",
        "drive.updated",
        "drive.deleted",
        "drive.sync_started",
        "drive.sync_completed",
        "drive.sync_failed",
        "drive.sync_held",
        "file.state_changed",
        "file.renamed",
        "files.bulk_changed",
        "drive.freshness_changed",
    ] {
        assert!(known.contains(t), "{t}");
        assert!(
            persistent.contains(t),
            "{t} is written to the outbox, so it must be persistent"
        );
    }
    // The in-code sources of those strings agree with the list above.
    let sources = [
        "src/ingest/reconcile.rs",
        "src/ingest/files.rs",
        "src/api/drives.rs",
        "src/api/lifecycle.rs",
        "src/api/freshness.rs",
        "src/state.rs",
    ];
    let pat = regex_lite_events();
    for rel in sources {
        let text =
            std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap();
        for lit in pat(&text) {
            assert!(
                known.contains(lit.as_str()),
                "{rel}: event type {lit} is not in events.v1.schema.json"
            );
        }
    }
}

/// `"<domain>.<name>"` literals passed to `append_event` or `sync_event`.
fn regex_lite_events() -> impl Fn(&str) -> Vec<String> {
    |text| {
        let mut out = Vec::new();
        for needle in ["append_event(", "sync_event("] {
            let mut rest = text;
            while let Some(i) = rest.find(needle) {
                rest = &rest[i + needle.len()..];
                let window = &rest[..rest.len().min(160)];
                if let Some(s) = window.split('"').nth(1) {
                    let ok = s.contains('.')
                        && s.bytes()
                            .all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'.')
                        && s.len() < 40;
                    if ok {
                        out.push(s.to_owned());
                    }
                }
            }
        }
        out
    }
}

#[test]
fn schema_examples_of_the_served_types_validate() {
    let schema = load("contracts/docs-api/docs-api.v1.schema.json");
    for name in [
        "Error",
        "Page",
        "Health",
        "Drive",
        "SyncInfo",
        "SyncRequest",
        "FileSummary",
        "Event",
        "Freshness",
        "TreeDir",
        "Tree",
        "DriveCreate",
        "DrivePatch",
    ] {
        let v = def(&schema, name);
        for ex in schema["$defs"][name]["examples"].as_array().unwrap() {
            check(&v, name, ex);
        }
    }
}

// ---------------------------------------------------------------------------
// live samples
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn live_responses_validate_against_the_schema() {
    let schema = load("contracts/docs-api/docs-api.v1.schema.json");
    let events_schema = load("contracts/docs-api/events.v1.schema.json");
    let v = |n: &str| def(&schema, n);
    let ev_validator = oo_core::schema_util::compile_schema(&events_schema).expect("events schema");

    let e = TestEngine::start(
        "contract_live",
        Opts {
            workers: true,
            watcher: true,
            ..Opts::default()
        },
    )
    .await;
    for i in 0..5 {
        e.write(&format!("a/f{i}.md"), &format!("f{i}\n"));
    }
    e.write("a/b/deep.txt", "deep\n");
    e.write("top.md", "top\n");
    std::fs::write(e.root.join("pic.png"), [0x89, b'P', b'N', b'G', 1]).unwrap();
    std::os::unix::fs::symlink("top.md", e.root.join("link.md")).unwrap();

    // Request bodies we send are valid by the contract too.
    let create_body = json!({"name": "contract", "backend": "fs", "config": {"root": e.root.to_str().unwrap(), "exclude": ["*.bak"]}, "model_enabled": false, "llm_daily_calls": 100});
    check(&v("DriveCreate"), "DriveCreate request", &create_body);
    let r = e.post("/v1/docs/drives", &create_body).await;
    assert_eq!(r.status, 201);
    let drive = r.json();
    check(&v("Drive"), "POST /drives", &drive);
    let id = drive["id"].as_str().unwrap().to_owned();

    let h = e.get("/v1/docs/health").await.json();
    check(&v("Health"), "GET /health", &h);

    e.until("9 files listed", || async {
        let d = e.get(&format!("/v1/docs/drives/{id}")).await.json();
        d["files_total"] == 9 && d["last_sync"]["status"] == "complete"
    })
    .await;
    e.until("hash done", || async {
        e.get(&format!("/v1/docs/drives/{id}")).await.json()["counts"]["stored"].is_null()
    })
    .await;

    let page = e.get("/v1/docs/drives").await.json();
    check(&v("Page"), "GET /drives", &page);
    for item in page["items"].as_array().unwrap() {
        check(&v("Drive"), "GET /drives item", item);
    }
    let d = e.get(&format!("/v1/docs/drives/{id}")).await.json();
    check(&v("Drive"), "GET /drives/{id}", &d);
    assert_eq!(d["last_sync"]["status"], "complete");
    check(&v("SyncInfo"), "Drive.last_sync", &d["last_sync"]);
    check(&v("Freshness"), "Drive.freshness", &d["freshness"]);

    let patch = json!({"llm_daily_calls": 5, "paused": false});
    check(&v("DrivePatch"), "DrivePatch request", &patch);
    let r = e
        .req(
            "PATCH",
            &format!("/v1/docs/drives/{id}"),
            &[("If-Match", "\"r1\"")],
            Some(&patch),
        )
        .await;
    assert_eq!(r.status, 200);
    check(&v("Drive"), "PATCH /drives/{id}", &r.json());

    for sync_body in [
        json!({}),
        json!({"deep": true}),
        json!({"paths": ["a/f0.md", "gone.md"]}),
    ] {
        check(&v("SyncRequest"), "SyncRequest request", &sync_body);
        let r = e
            .post(&format!("/v1/docs/drives/{id}:sync"), &sync_body)
            .await;
        if r.status == 409 {
            check(&v("Error"), ":sync 409", &r.json());
            continue;
        }
        assert_eq!(
            r.status,
            202,
            "{sync_body}: {}",
            String::from_utf8_lossy(&r.body)
        );
        check(&v("SyncInfo"), ":sync", &r.json());
        e.until("sync idle", || async {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM _oo_docs.job WHERE kind = 'sync' AND state IN ('queued', 'running')")
                .fetch_one(&e.pool).await.unwrap() == 0
        }).await;
    }

    let files = e
        .get(&format!("/v1/docs/drives/{id}/files?limit=3"))
        .await
        .json();
    check(&v("Page"), "GET files", &files);
    for f in files["items"].as_array().unwrap() {
        check(&v("FileSummary"), "GET files item", f);
    }
    let all = e
        .get(&format!("/v1/docs/drives/{id}/files?limit=1000"))
        .await
        .json();
    let states: BTreeSet<String> = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["state"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        states.contains("queued") && states.contains("unsupported"),
        "{states:?}"
    );
    for f in all["items"].as_array().unwrap() {
        check(&v("FileSummary"), "file summary", f);
        let state = f["state"].as_str().unwrap();
        assert!(FileState::parse(state).is_some(), "{state}");
    }
    let tree = e.get(&format!("/v1/docs/drives/{id}/tree")).await.json();
    check(&v("Tree"), "GET tree", &tree);
    let tree = e
        .get(&format!("/v1/docs/drives/{id}/tree?path=a"))
        .await
        .json();
    check(&v("Tree"), "GET tree?path=a", &tree);

    // Errors of every status the routes produce.
    let mut seen = BTreeSet::new();
    for (path, method, body) in [
        (
            "/v1/docs/drives/0f8fad5b-d9cb-469f-a165-70867728950e",
            "GET",
            None,
        ),
        ("/v1/docs/nothing", "GET", None),
        ("/v1/docs/drives", "PUT", None),
        ("/v1/docs/drives/bad/files?limit=0", "GET", None),
        ("/v1/docs/drives", "POST", Some(json!({"name": "x"}))),
    ] {
        let r = e
            .req(
                method,
                path,
                &[("Idempotency-Key", "k-0123456789abcdef")],
                body.as_ref(),
            )
            .await;
        let j = r.json();
        check(&v("Error"), path, &j);
        seen.insert(j["code"].as_str().unwrap().to_owned());
    }
    assert!(seen.len() >= 4, "{seen:?}");

    // Events: the stored log through the stream is Event-shaped and each
    // persistent one validates against the events schema.
    let bearer = format!("Bearer {TOKEN}");
    let mut s = open_sse(
        e.addr,
        "/v1/docs/events?since=0",
        &[("Authorization", &bearer)],
    )
    .await;
    assert_eq!(s.status, 200);
    let head = h_head(&e).await;
    let mut kinds = BTreeSet::new();
    let mut last = 0;
    while last < head {
        let ev = s
            .event(std::time::Duration::from_secs(5))
            .await
            .expect("event");
        check(&v("Event"), "stream event vs Event", &ev);
        check_event(&ev_validator, &ev);
        last = ev["seq"].as_i64().unwrap();
        kinds.insert(ev["type"].as_str().unwrap().to_owned());
    }
    for must in [
        "drive.created",
        "drive.updated",
        "drive.sync_started",
        "drive.sync_completed",
        "file.state_changed",
    ] {
        assert!(kinds.contains(must), "{must} missing from {kinds:?}");
    }
    e.finish().await;
}

fn check_event(validator: &jsonschema::Validator, ev: &Value) {
    let errors: Vec<String> = validator
        .iter_errors(ev)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "event vs events.v1: {}\n{ev}",
        errors.join("; ")
    );
}

async fn h_head(e: &TestEngine) -> i64 {
    e.get("/v1/docs/health").await.json()["events_head"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn live_pack_and_share_responses_validate_against_the_schema() {
    let schema = load("contracts/docs-api/docs-api.v1.schema.json");
    let events_schema = load("contracts/docs-api/events.v1.schema.json");
    let v = |n: &str| def(&schema, n);
    let ev_validator = oo_core::schema_util::compile_schema(&events_schema).expect("events schema");
    let e = TestEngine::start(
        "contract_packs",
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    e.write(
        "Projects/a/p.md",
        "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n",
    );
    let id = e.create_drive("contract-packs").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    e.until("published", || async {
        let r = e.get(&format!("/v1/docs/drives/{id}/files")).await.json();
        let items = r["items"].as_array().unwrap();
        !items.is_empty() && items.iter().all(|f| f["state"] == "published")
    })
    .await;

    let compile =
        json!({"drive_id": id, "scope": ["Projects/a"], "budget": 800, "task": "星火计划"});
    check(&v("CompileRequest"), "CompileRequest", &compile);
    let r = e.post("/v1/docs/context-packs:compile", &compile).await;
    assert_eq!(r.status, 201, "{}", String::from_utf8_lossy(&r.body));
    let pack = r.json();
    check(&v("ContextPack"), "POST :compile", &pack);
    let pid = pack["id"].as_str().unwrap();
    check(
        &v("ContextPack"),
        "GET pack",
        &e.get(&format!("/v1/docs/context-packs/{pid}")).await.json(),
    );
    let create = json!({"compile": compile, "mount": {"target_dir": "/tmp/oo-contract"}, "expires_in_seconds": 3600});
    check(&v("ShareCreate"), "ShareCreate", &create);
    let s = e.post("/v1/docs/shares", &create).await;
    assert_eq!(s.status, 201, "{}", String::from_utf8_lossy(&s.body));
    check(&v("Share"), "POST /shares", &s.json());
    let sid = s.json()["id"].as_str().unwrap().to_owned();
    check(
        &v("Share"),
        "GET share",
        &e.get(&format!("/v1/docs/shares/{sid}")).await.json(),
    );
    for (what, body) in [
        (
            "scope",
            e.post(
                "/v1/docs/context-packs:compile",
                &json!({"drive_id": id, "scope": ["Nope"]}),
            )
            .await,
        ),
        (
            "missing pack",
            e.get("/v1/docs/context-packs/00000000-0000-4000-8000-000000000000")
                .await,
        ),
        (
            "missing share",
            e.get("/v1/docs/shares/00000000-0000-4000-8000-000000000000")
                .await,
        ),
    ] {
        check(&v("Error"), what, &body.json());
    }

    let bearer = format!("Bearer {TOKEN}");
    let mut sse = open_sse(
        e.addr,
        "/v1/docs/events?since=0",
        &[("Authorization", &bearer)],
    )
    .await;
    let head: i64 = sqlx::query_scalar("SELECT max(seq) FROM _oo_docs.event")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    let mut kinds = BTreeSet::new();
    let mut last = 0;
    while last < head {
        let ev = sse
            .event(std::time::Duration::from_secs(5))
            .await
            .expect("event");
        check(&v("Event"), "stream event vs Event", &ev);
        check_event(&ev_validator, &ev);
        last = ev["seq"].as_i64().unwrap();
        kinds.insert(ev["type"].as_str().unwrap().to_owned());
    }
    for must in [
        "pack.compiled",
        "share.created",
        "ontology.version_published",
    ] {
        assert!(kinds.contains(must), "{must} missing: {kinds:?}");
    }
    e.finish().await;
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT (labelled postgres container)"]
async fn per_file_responses_validate_against_the_schema() {
    let schema = load("contracts/docs-api/docs-api.v1.schema.json");
    let events_schema = load("contracts/docs-api/events.v1.schema.json");
    let v = |n: &str| def(&schema, n);
    let ev_validator = oo_core::schema_util::compile_schema(&events_schema).expect("events schema");
    let e = TestEngine::start(
        "contract_files",
        Opts {
            workers: true,
            stages: true,
            ..Opts::default()
        },
    )
    .await;
    e.write(
        "Projects/a/p.md",
        "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n",
    );
    e.write("Projects/a/q.md", "# 笔记\n\n周会改到周三。\n");
    e.write("Projects/b/r.md", "# 另一份\n\n内容。\n");
    let id = e.create_drive("contract-files").await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    e.until("published", || async {
        let r = e.get(&format!("/v1/docs/drives/{id}/files")).await.json();
        let items = r["items"].as_array().unwrap();
        items.len() == 3 && items.iter().all(|f| f["state"] == "published")
    })
    .await;
    let fid: String =
        sqlx::query_scalar("SELECT id::text FROM _oo_docs.file WHERE path = 'Projects/a/p.md'")
            .fetch_one(&e.pool)
            .await
            .unwrap();

    // FileDetail, with every include and without.
    let plain = e.get(&format!("/v1/docs/files/{fid}")).await;
    assert_eq!(plain.status, 200);
    check(&v("FileDetail"), "GET /files/{fid}", &plain.json());
    let full = e
        .get(&format!(
            "/v1/docs/files/{fid}?include=history,entities,facts,shares"
        ))
        .await
        .json();
    check(&v("FileDetail"), "GET /files/{fid}?include=all", &full);
    for h in full["history"].as_array().unwrap() {
        check(&v("StateEntry"), "history entry", h);
    }
    for f in full["facts"].as_array().unwrap() {
        check(&v("Fact"), "fact", f);
        for ev in f["evidence"].as_array().unwrap() {
            check(&v("Evidence"), "evidence", ev);
        }
    }
    for en in full["entities"].as_array().unwrap() {
        check(&v("EntityRef"), "entity", en);
    }
    // FileSummary carries latency_ms (G13) and modified_at (G1).
    check(&v("FileSummary"), "latency in detail", &full);
    assert!(full["latency_ms"].is_object() && full["modified_at"].is_string());

    // Blocks.
    let blocks = e.get(&format!("/v1/docs/files/{fid}/blocks?limit=2")).await;
    assert_eq!(blocks.status, 200);
    let page = blocks.json();
    check(&v("Page"), "GET blocks", &page);
    assert!(page["next_cursor"].is_string());
    for b in page["items"].as_array().unwrap() {
        check(&v("Block"), "block", b);
    }

    // Retry: request, response and the errors of its statuses.
    let req = json!({"file_ids": [fid], "mode": "normal"});
    check(&v("RetryRequest"), "RetryRequest", &req);
    let r = e.post("/v1/docs/files:retry", &req).await;
    assert_eq!(r.status, 200);
    check(&v("BatchResult"), "files:retry", &r.json());
    assert_eq!(r.json()["results"][0]["status"], "not_retryable");
    let too_many: Vec<String> = (0..501)
        .map(|i| format!("00000000-0000-4000-8000-{i:012x}"))
        .collect();
    for (what, resp) in [
        (
            "413",
            e.post("/v1/docs/files:retry", &json!({"file_ids": too_many}))
                .await,
        ),
        (
            "404",
            e.get("/v1/docs/files/00000000-0000-4000-8000-000000000000")
                .await,
        ),
        (
            "400",
            e.get(&format!("/v1/docs/files/{fid}?include=zzz")).await,
        ),
        (
            "416",
            e.req(
                "GET",
                &format!("/v1/docs/files/{fid}/content"),
                &[("Range", "bytes=99999-")],
                None,
            )
            .await,
        ),
        (
            "idempotency key",
            e.req("POST", "/v1/docs/files:retry", &[], Some(&req)).await,
        ),
    ] {
        check(&v("Error"), what, &resp.json());
    }

    // The tree: per directory failed and pending_review (G9), sums of states.
    sqlx::query("UPDATE _oo_docs.file SET state = 'parse_failed', retryable = true WHERE path = 'Projects/a/q.md'")
        .execute(&e.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE _oo_docs.file SET pending_review = 2 WHERE path = 'Projects/a/p.md'")
        .execute(&e.pool)
        .await
        .unwrap();
    let tree = e.get(&format!("/v1/docs/drives/{id}/tree")).await.json();
    check(&v("Tree"), "GET tree", &tree);
    let dir = |p: &str| {
        tree["dirs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["path"] == p)
            .unwrap_or_else(|| panic!("{p} in {tree}"))
            .clone()
    };
    let a = dir("Projects");
    assert_eq!(a["files"], 3);
    assert_eq!(
        (
            a["counts"]["failed"].as_i64(),
            a["counts"]["pending_review"].as_i64()
        ),
        (Some(1), Some(2))
    );
    let sub = e
        .get(&format!("/v1/docs/drives/{id}/tree?path=Projects"))
        .await
        .json();
    let pa = sub["dirs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["path"] == "Projects/a")
        .unwrap()
        .clone();
    assert_eq!(
        (
            pa["counts"]["failed"].as_i64(),
            pa["counts"]["pending_review"].as_i64()
        ),
        (Some(1), Some(2))
    );
    let pb = sub["dirs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["path"] == "Projects/b")
        .unwrap()
        .clone();
    assert_eq!(
        (
            pb["counts"]["failed"].as_i64(),
            pb["counts"]["pending_review"].as_i64()
        ),
        (Some(0), Some(0))
    );
    // modified_at: sort and field (G1).
    let by = e
        .get(&format!("/v1/docs/drives/{id}/files?sort=modified_at"))
        .await
        .json();
    let times: Vec<&str> = by["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["modified_at"].as_str().unwrap())
        .collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    assert_eq!(times, sorted);
    for f in by["items"].as_array().unwrap() {
        check(&v("FileSummary"), "sorted item", f);
    }

    // The transient event is an Event without seq.
    let bearer = format!("Bearer {TOKEN}");
    let mut sse = open_sse(e.addr, "/v1/docs/events", &[("Authorization", &bearer)]).await;
    let _ = sse.frame(std::time::Duration::from_secs(2)).await;
    // Not the id of a parsed file: that one reported a moment ago and the bus
    // allows one event per file per second.
    assert!(e.app.progress.emit(oo_docs::api::progress::Progress {
        file_id: "0f8fad5b-d9cb-469f-a165-70867728950e".into(),
        drive_id: Some(id.clone()),
        unit: "page",
        done: 1,
        total: Some(3),
    }));
    let ev = sse
        .event(std::time::Duration::from_secs(5))
        .await
        .expect("progress");
    assert_eq!(ev["type"], "file.progress");
    // events.v1 is the schema of transient events; `$defs/Event` of the API
    // schema still lists `seq` as required, which only holds for persistent
    // ones (reported to the coordinator, the contract is not ours to change).
    assert!(ev.get("seq").is_none());
    check_event(&ev_validator, &ev);
    e.finish().await;
}
