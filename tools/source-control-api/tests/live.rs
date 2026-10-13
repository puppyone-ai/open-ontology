//! Real OIDC, private PostgreSQL, and pinned producer proof.
//!
//! The test returns immediately unless `OO_A383_LIVE=1`. Cargo still compiles it.

#[path = "support/idp.rs"]
mod idp;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};
use tokio_postgres::Client;

use source_control_api::sha256::sha256_hex;
use source_control_api::signal::signal_tree;
use source_control_api::tls::pg_tls;

const ORIGIN: &str = "https://127.0.0.1:8445";
const IDENTITY_ORIGIN: &str = "https://127.0.0.1:8444";
const ISSUER: &str = "https://127.0.0.1:8443";
const CLIENT_ID: &str = "oo-a383-client";
const PROJECT: &str = "a383_main";
const OTHER: &str = "a383_other";

struct Env {
    host: String,
    port: u16,
    secrets: PathBuf,
    bins: PathBuf,
    work: PathBuf,
    evidence: PathBuf,
    idp_der: PathBuf,
    head: String,
}

struct Session {
    cookie: String,
    csrf: String,
    revision: i64,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_closed_loop() {
    if std::env::var("OO_A383_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    if let Err(label) = run().await {
        panic!("{label}");
    }
}

async fn run() -> Result<(), &'static str> {
    let env = Env::load()?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&env.work).map_err(|_| "work")?;
    fs::create_dir_all(env.evidence.join("raw")).map_err(|_| "evidence")?;
    fs::write(env.evidence.join("steps.jsonl"), "").map_err(|_| "evidence")?;
    let admin_pass = read_secret(&env.secrets.join("admin.pass"))?;
    let ca = fs::read(env.secrets.join("ca.crt")).map_err(|_| "ca")?;
    fs::write(env.evidence.join("ca.sha256"), format!("{}\n", sha256_hex(&ca))).map_err(|_| "evidence")?;
    let admin = connect(&env, &admin_pass, &ca, "postgres").await?;
    prepare_cluster(&env, &admin, &admin_pass, &ca).await?;
    let commit = identity_commit();
    write_identity_config(&env, &commit)?;
    let der = fs::read(&env.idp_der).map_err(|_| "idp_der")?;
    let secret = read_text(&env.secrets.join("client.secret"))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8443").await.map_err(|_| "idp_bind")?;
    let cert = fs::read(env.secrets.join("server.crt")).map_err(|_| "cert")?;
    let key = fs::read(env.secrets.join("server.key")).map_err(|_| "key")?;
    let redirect = format!("{IDENTITY_ORIGIN}/control/v1/auth/callback");
    let idp = idp::start(listener, &cert, &key, idp::IdpSpec { issuer: ISSUER, client_id: CLIENT_ID, client_secret: &secret, redirect_uri: &redirect, der: &der }).await;
    let identity_bin = env.bins.join("control-identity");
    let identity_cfg = env.work.join("identity.json");
    run_to_exit(&identity_bin, &["install", "--config", path_str(&identity_cfg)], &env.evidence.join("raw/identity-install.log"), "identity_install")?;
    run_to_exit(&identity_bin, &["bootstrap-issue", "--config", path_str(&identity_cfg), "--out", path_str(&env.work.join("bootstrap.token"))], &env.evidence.join("raw/bootstrap.log"), "bootstrap")?;
    let mut identity = spawn_service(&identity_bin, &["serve", "--config", path_str(&identity_cfg)], &env.work.join("identity.out"), &env.work.join("identity.err"))?;
    wait_outcome(&env.work.join("identity.out"), "listening").await?;
    let http = source_control_api::identity::identity_client(&ca).map_err(|_| "client")?;
    let token = read_text(&env.work.join("bootstrap.token"))?;
    idp.set_subject("a383-admin");
    let mut admin_session = login(&http, Some(&token)).await?;
    refresh(&http, &mut admin_session).await?;
    idp.set_subject("a383-audit");
    let mut audit_session = login(&http, None).await?;
    refresh(&http, &mut audit_session).await?;
    idp.set_subject("a383-other");
    let mut other_session = login(&http, None).await?;
    refresh(&http, &mut other_session).await?;
    write_api_config(&env)?;
    let api_bin = env.bins.join("source-control-api");
    let api_cfg = env.work.join("api.json");
    run_to_exit(&api_bin, &["install", "--config", path_str(&api_cfg)], &env.evidence.join("raw/api-install.log"), "api_install")?;
    let mut api = spawn_service(&api_bin, &["serve", "--config", path_str(&api_cfg)], &env.work.join("api.out"), &env.work.join("api.err"))?;
    wait_outcome(&env.work.join("api.out"), "listening").await?;
    prove(&env, &http, (&mut admin_session, &audit_session, &other_session), (&admin_pass, &ca), &mut api).await?;
    let _ = identity.kill();
    let _ = api.kill();
    Ok(())
}

async fn prove(env: &Env, http: &reqwest::Client, sessions: (&mut Session, &Session, &Session), sql: (&str, &[u8]), api: &mut std::process::Child) -> Result<(), &'static str> {
    let (admin, audit, other) = sessions;
    let (admin_pass, ca) = sql;
    let (status, body) = get_api(http, "/control/v1/connectors", None).await?;
    mark(env, "unauthenticated", &status.to_string());
    expect(status, 401, "unauthenticated")?;
    expect_error(&body, "unauthenticated", "session", "unauthenticated")?;
    refresh(http, admin).await?;
    let (status, body) = post_api(http, admin, "/control/v1/sources", &register_pg(admin, "enterprise_src", "db.enterprise.example", "a383-reg-enterprise1", "secret://pg/a383/reader")).await?;
    mark(env, "enterprise", &format!("{status}:{}", code_of(&body)));
    expect(status, 403, "enterprise")?;
    expect_error(&body, "source.not_admitted", "host", "enterprise")?;
    refresh(http, admin).await?;
    let (status, body) = post_api(http, admin, "/control/v1/sources", &password_body(admin)).await?;
    mark(env, "credential", &format!("{status}:{}", code_of(&body)));
    expect(status, 400, "credential")?;
    expect_error(&body, "malformed", "credential", "credential")?;
    refresh(http, admin).await?;
    let (status, body) = post_api(http, admin, "/control/v1/sources", &register_pg(admin, "missing_src", "oo-a383-pg", "a383-reg-unknownsec", "secret://pg/a383/missing")).await?;
    mark(env, "secret_unknown", &format!("{status}:{}", code_of(&body)));
    expect(status, 403, "secret_unknown")?;
    expect_error(&body, "secret.unknown", "secret_ref", "secret_unknown")?;
    for (source, key) in [("shop_src", "a383-reg-shop-0001"), ("rows100k", "a383-reg-rows100k01"), ("rows2", "a383-reg-rows2-0001"), ("rowsbad", "a383-reg-rowsbad001"), ("rowscrash", "a383-reg-crash-0001")] {
        refresh(http, admin).await?;
        let bytes = if source == "shop_src" { register_shop(admin, key) } else { register_file(admin, source, key) };
        let (status, _) = post_api(http, admin, "/control/v1/sources", &bytes).await?;
        mark(env, &format!("register_{source}"), &status.to_string());
        expect(status, 201, "register")?;
    }
    // init-pg can bootstrap only one database per cluster. Inspect does that before file import.
    refresh(http, admin).await?;
    let (status, inspect) = post_api(http, admin, "/control/v1/imports", &pg_inspect(admin)).await?;
    expect(status, 202, "pg_inspect")?;
    let op_pg = text_field(&inspect, "operation_id", "pg_inspect")?;
    let inspected = wait_import(env, http, admin, &op_pg, "pg_inspect", 400, |row| row["status"] == "awaiting_review" || row["status"] == "failed").await?;
    mark(env, "pg_inspect", text_field(&inspected, "status", "pg_inspect")?.as_str());
    if inspected["status"] != "awaiting_review" { return Err("pg_inspect"); }
    let (status, exchange) = get_api(http, "/control/v1/sources/shop_src/catalog", Some(&admin.cookie)).await?;
    expect(status, 200, "pg_catalog")?;
    let rendered = serde_json::to_string(&exchange).map_err(|_| "pg_catalog")?;
    if rendered.contains("甲注评") || exchange["published"] != false || exchange["catalog"]["protocol"] != "oo.source.pgsql/v1" {
        return Err("marker");
    }
    mark(env, "marker", "absent");
    let csv_100k = csv_ids(100_000);
    let csv_2 = csv_ids(2);
    let csv_bad = b"id\nabc\n".to_vec();
    let csv_crash = csv_ids(50_000);
    let sha_100k = upload(env, http, admin, "rows100k", &csv_100k, "a383-up-rows100k001").await?;
    let sha_2 = upload(env, http, admin, "rows2", &csv_2, "a383-up-rows2-00001").await?;
    let sha_bad = upload(env, http, admin, "rowsbad", &csv_bad, "a383-up-rowsbad0001").await?;
    let sha_crash = upload(env, http, admin, "rowscrash", &csv_crash, "a383-up-crash-00001").await?;
    refresh(http, admin).await?;
    let create_100k = file_import(admin, "rows100k", &sha_100k, 100_000, "a383-imp-rows100k01", 1);
    let (status, created) = post_api(http, admin, "/control/v1/imports", &create_100k).await?;
    mark(env, "create_100k", &status.to_string());
    expect(status, 202, "create_100k")?;
    let op_100k = text_field(&created, "operation_id", "create_100k")?;
    let running = wait_import(env, http, admin, &op_100k, "running_100k", 300, |row| row["status"] == "running" && row["child_started"] == true).await?;
    mark(env, "running_100k", text_field(&running, "status", "running_100k")?.as_str());
    wait_pid(env, admin_pass, ca, &op_100k).await?;
    refresh(http, admin).await?;
    let (status, small) = post_api(http, admin, "/control/v1/imports", &file_import(admin, "rows2", &sha_2, 2, "a383-imp-rows2-0001", 1)).await?;
    expect(status, 202, "create_small")?;
    let op_small = text_field(&small, "operation_id", "create_small")?;
    let queued = get_import(http, admin, &op_small).await?;
    mark(env, "small_queued", text_field(&queued, "status", "small_queued")?.as_str());
    if queued["status"] != "queued" { return Err("small_queued"); }
    refresh(http, admin).await?;
    let (status, canceled) = post_api(http, admin, &format!("/control/v1/imports/{op_small}/cancel"), &action_body(admin, "a383-cancel-rows2001")).await?;
    expect(status, 202, "cancel")?;
    mark(env, "cancel", &format!("{}:{}", canceled["status"].as_str().unwrap_or(""), canceled["child_started"]));
    if canceled["status"] != "canceled" || canceled["child_started"] != false { return Err("cancel"); }
    refresh(http, admin).await?;
    let (status, restarted) = post_api(http, admin, &format!("/control/v1/imports/{op_small}/restart"), &action_body(admin, "a383-restart-rows201")).await?;
    expect(status, 202, "restart")?;
    mark(env, "restart", text_field(&restarted, "status", "restart")?.as_str());
    if restarted["status"] != "queued" || restarted["attempt"] != 2 { return Err("restart"); }
    refresh(http, admin).await?;
    let (status, bad) = post_api(http, admin, "/control/v1/imports", &file_import(admin, "rowsbad", &sha_bad, 10, "a383-imp-rowsbad001", 1)).await?;
    expect(status, 202, "create_bad")?;
    let op_bad = text_field(&bad, "operation_id", "create_bad")?;
    let done = wait_import(env, http, admin, &op_100k, "await_100k", 300, |row| row["status"] == "awaiting_review" || row["status"] == "failed").await?;
    mark(env, "done_100k", &format!("{}:{}", done["status"].as_str().unwrap_or(""), done["rows"].as_i64().unwrap_or(-1)));
    if done["status"] != "awaiting_review" || done["rows"] != 100_000 { return Err("rows_100k"); }
    let count_100k = sql_count(env, http, admin, admin_pass, ca, "rows100k").await?;
    mark(env, "sql_100k", &count_100k.to_string());
    if count_100k != 100_000 { return Err("sql_100k"); }
    let small_done = wait_import(env, http, admin, &op_small, "await_small", 300, |row| row["status"] == "awaiting_review" || row["status"] == "failed").await?;
    mark(env, "done_small", &format!("{}:{}", small_done["status"].as_str().unwrap_or(""), small_done["rows"].as_i64().unwrap_or(-1)));
    if small_done["status"] != "awaiting_review" || small_done["rows"] != 2 { return Err("rows_small"); }
    let bad_done = wait_import(env, http, admin, &op_bad, "await_bad", 300, |row| matches!(row["status"].as_str(), Some("failed" | "awaiting_review" | "canceled"))).await?;
    mark(env, "done_bad", text_field(&bad_done, "status", "done_bad")?.as_str());
    if bad_done["status"] != "failed" || bad_done["status"] == "ready" { return Err("bad_status"); }
    let (catalog_status, catalog_body) = get_api(http, "/control/v1/sources/rowsbad/catalog", Some(&admin.cookie)).await?;
    mark(env, "bad_catalog", &catalog_status.to_string());
    expect(catalog_status, 404, "bad_catalog")?;
    expect_error(&catalog_body, "not_found", "catalog", "bad_catalog")?;
    refresh(http, admin).await?;
    let (status, replay) = post_api(http, admin, "/control/v1/imports", &create_100k).await?;
    expect(status, 202, "replay")?;
    if replay["operation_id"] != op_100k { return Err("replay"); }
    mark(env, "replay", "same");
    refresh(http, admin).await?;
    let (status, conflict) = post_api(http, admin, "/control/v1/imports", &file_import(admin, "rows100k", &sha_100k, 99_999, "a383-imp-rows100k01", 1)).await?;
    mark(env, "idempotency_conflict", &format!("{status}:{}", code_of(&conflict)));
    expect(status, 409, "idempotency_conflict")?;
    expect_error(&conflict, "idempotency.conflict", "idempotency", "idempotency_conflict")?;
    refresh(http, admin).await?;
    let (status, stale) = post_api(http, admin, "/control/v1/imports", &file_import(admin, "rows100k", &sha_100k, 100_000, "a383-imp-stale-rev01", 1)).await?;
    mark(env, "stale_revision", &format!("{status}:{}", code_of(&stale)));
    expect(status, 409, "stale_revision")?;
    expect_error(&stale, "revision.conflict", "revision", "stale_revision")?;
    let (missing_status, missing_body) = get_api(http, "/control/v1/sources/missing_src", Some(&admin.cookie)).await?;
    let (foreign_status, foreign_body) = get_api(http, "/control/v1/sources/shop_src", Some(&other.cookie)).await?;
    mark(env, "cross_project", &format!("{missing_status}:{foreign_status}"));
    expect(missing_status, 403, "cross_project")?;
    expect(foreign_status, 403, "cross_project")?;
    if missing_body != foreign_body { return Err("cross_project"); }
    expect_error(&missing_body, "forbidden", "scope", "cross_project")?;
    refresh(http, admin).await?;
    let deny = file_import(audit, "rows2", &sha_2, 2, "a383-auditor-deny01", 1);
    let (status, denied) = post_api(http, audit, "/control/v1/imports", &deny).await?;
    mark(env, "auditor_create", &format!("{status}:{}", detail_of(&denied)));
    expect(status, 403, "auditor_create")?;
    expect_error(&denied, "forbidden", "capability", "auditor_create")?;
    let (status, listed) = get_api(http, &format!("/control/v1/sources?project_id={PROJECT}"), Some(&audit.cookie)).await?;
    mark(env, "auditor_list", &status.to_string());
    expect(status, 200, "auditor_list")?;
    if listed["operation"] != "listSources" { return Err("auditor_list"); }
    let (status, scoped) = get_api(http, &format!("/control/v1/sources?project_id={OTHER}"), Some(&admin.cookie)).await?;
    mark(env, "other_project", &format!("{status}:{}", detail_of(&scoped)));
    expect(status, 403, "other_project")?;
    expect_error(&scoped, "forbidden", "scope", "other_project")?;
    refresh(http, admin).await?;
    let (status, crash) = post_api(http, admin, "/control/v1/imports", &file_import(admin, "rowscrash", &sha_crash, 50_000, "a383-imp-crash-0001", 1)).await?;
    expect(status, 202, "create_crash")?;
    let op_crash = text_field(&crash, "operation_id", "create_crash")?;
    let _ = wait_import(env, http, admin, &op_crash, "crash_running", 300, |row| row["status"] == "running" && row["child_started"] == true).await?;
    wait_pid(env, admin_pass, ca, &op_crash).await?;
    let api_pid = i32::try_from(api.id()).map_err(|_| "crash_pid")?;
    signal_tree(api_pid);
    wait_child(api, "crash_exit").await?;
    let mut restarted_api = spawn_service(&env.bins.join("source-control-api"), &["serve", "--config", path_str(&env.work.join("api.json"))], &env.work.join("api-restart.out"), &env.work.join("api-restart.err"))?;
    wait_outcome(&env.work.join("api-restart.out"), "listening").await?;
    let recovered = wait_import(env, http, admin, &op_crash, "resume", 40, |row| row["status"] == "resume_uncertain" || row["status"] == "awaiting_review" || row["status"] == "failed").await?;
    mark(env, "resume", text_field(&recovered, "status", "resume")?.as_str());
    if recovered["status"] != "resume_uncertain" || recovered["status"] == "ready" { return Err("resume"); }
    refresh(http, admin).await?;
    let (status, _) = post_api(http, admin, &format!("/control/v1/imports/{op_crash}/restart"), &action_body(admin, "a383-restart-crash01")).await?;
    expect(status, 202, "crash_restart")?;
    let crash_done = wait_import(env, http, admin, &op_crash, "crash_done", 300, |row| row["status"] == "awaiting_review" || row["status"] == "failed").await?;
    mark(env, "crash_done", &format!("{}:{}", crash_done["status"].as_str().unwrap_or(""), crash_done["rows"].as_i64().unwrap_or(-1)));
    if crash_done["status"] != "awaiting_review" || crash_done["rows"] != 50_000 { return Err("crash_rows"); }
    let count_crash = sql_count(env, http, admin, admin_pass, ca, "rowscrash").await?;
    mark(env, "sql_crash", &count_crash.to_string());
    if count_crash != 50_000 { return Err("sql_crash"); }
    let summary = json!({
        "rows_100k": 100000,
        "rows_small": 2,
        "bad_status": "failed",
        "bad_catalog": 404,
        "cancel_queued": true,
        "restart_queued": true,
        "idempotent_operation": op_100k,
        "idempotency_conflict": 409,
        "stale_revision": 409,
        "cross_project": true,
        "enterprise": 403,
        "credential": 400,
        "secret_unknown": 403,
        "unauthenticated": 401,
        "pg_inspect": "awaiting_review",
        "marker_absent": true,
        "resume_uncertain": true,
        "crash_rows": 50000,
        "sql_count_100k": count_100k,
        "sql_count_crash": count_crash,
        "head": env.head,
        "published": false,
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    });
    let text = serde_json::to_string(&summary).map_err(|_| "summary")?;
    fs::write(env.evidence.join("live-summary.json"), format!("{text}\n")).map_err(|_| "summary")?;
    let _ = restarted_api.kill();
    Ok(())
}

async fn prepare_cluster(env: &Env, admin: &Client, admin_pass: &str, ca: &[u8]) -> Result<(), &'static str> {
    let reader = read_secret(&env.secrets.join("reader.pass"))?;
    let writer = read_secret(&env.secrets.join("writer.pass"))?;
    let runtime = read_secret(&env.secrets.join("runtime.pass"))?;
    let api = read_secret(&env.secrets.join("api.pass"))?;
    let identity = read_secret(&env.secrets.join("identity.pass"))?;
    ensure_role(admin, "oo_a383_reader", &reader, "NOSUPERUSER").await?;
    ensure_role(admin, "oo_a383_writer", &writer, "NOSUPERUSER").await?;
    ensure_role(admin, "oo_a383_runtime", &runtime, "NOSUPERUSER").await?;
    ensure_role(admin, "oo_a383_api", &api, "NOSUPERUSER").await?;
    // Identity migrations GRANT only to this NOLOGIN role. The login role must be a member.
    ensure_nologin(admin, "oo_identity_runtime").await?;
    ensure_role(admin, "oo_identity_admin", &identity, "NOSUPERUSER").await?;
    admin.batch_execute("GRANT oo_identity_runtime TO oo_identity_admin").await.map_err(|_| "role")?;
    ensure_database(admin, "oo_a383_src", "oo_fixture_admin").await?;
    // Bootstrap is single-use and import idempotency is stored. Recreate those databases in this cluster.
    recreate_database(admin, "oo_a383_dst", "oo_fixture_admin").await?;
    recreate_database(admin, "oo_a383_file", "oo_fixture_admin").await?;
    recreate_database(admin, "oo_a383_api", "oo_a383_api").await?;
    recreate_database(admin, "oo_a383_identity", "oo_identity_admin").await?;
    // These roles are cluster-wide. A second init-pg refuses while they remain.
    drop_pipeline_roles(admin).await?;
    admin.batch_execute("GRANT CONNECT ON DATABASE oo_a383_src TO oo_a383_reader").await.map_err(|_| "grant")?;
    let src = connect(env, admin_pass, ca, "oo_a383_src").await?;
    let present = src.query_opt("SELECT 1 FROM information_schema.tables WHERE table_schema = 'shop' AND table_name = 'customer'", &[]).await.map_err(|_| "shop")?;
    if present.is_none() {
        let sql = include_str!("../../../deploy/trusted-demo/fixture/source.sql").replace(":\"reader\"", "oo_a383_reader").replace(":\"writer\"", "oo_a383_writer");
        src.batch_execute(&sql).await.map_err(|_| "shop")?;
    }
    admin.batch_execute("GRANT CONNECT ON DATABASE oo_a383_file TO oo_a383_runtime").await.map_err(|_| "grant")?;
    fs::write(env.evidence.join("raw/init-pg.exit"), "not_harness\n").map_err(|_| "evidence")?;
    Ok(())
}

async fn upload(env: &Env, http: &reqwest::Client, session: &mut Session, source: &str, bytes: &[u8], key: &str) -> Result<String, &'static str> {
    refresh(http, session).await?;
    let sha = sha256_hex(bytes);
    let response = http.post(format!("{ORIGIN}/control/v1/sources/{source}/objects"))
        .header("cookie", format!("__Host-oo-session={}", session.cookie))
        .header("origin", ORIGIN)
        .header("x-oo-csrf", &session.csrf)
        .header("x-oo-identity-revision", session.revision.to_string())
        .header("idempotency-key", key)
        .header("x-oo-sha256", &sha)
        .header("content-type", "application/octet-stream")
        .body(bytes.to_vec())
        .send().await.map_err(|_| "upload")?;
    let status = response.status().as_u16();
    let payload = response.bytes().await.map_err(|_| "upload")?;
    scrub(&payload)?;
    let value: Value = serde_json::from_slice(&payload).unwrap_or(Value::Null);
    mark(env, &format!("upload_{source}"), &status.to_string());
    expect(status, 201, "upload")?;
    if value["sha256"] != sha { return Err("upload"); }
    Ok(sha)
}

async fn sql_count(env: &Env, http: &reqwest::Client, session: &Session, admin_pass: &str, ca: &[u8], source: &str) -> Result<i64, &'static str> {
    let (status, body) = get_api(http, &format!("/control/v1/sources/{source}/catalog"), Some(&session.cookie)).await?;
    expect(status, 200, "catalog")?;
    let namespace = body["catalog"]["physical_namespace"].as_str().ok_or("physical")?;
    let table = body["catalog"]["physical_table"].as_str().ok_or("physical")?;
    // physical_namespace is the mapping id. file-import stores the shadow table in schema _oo.
    if !sql_ident(namespace) || !sql_ident(table) { return Err("physical"); }
    let client = connect(env, admin_pass, ca, "oo_a383_file").await?;
    let row = client.query_one(&format!("SELECT count(*)::int8 FROM \"_oo\".\"{table}\""), &[]).await.map_err(|_| "count")?;
    Ok(row.get(0))
}

async fn wait_pid(env: &Env, admin_pass: &str, ca: &[u8], operation_id: &str) -> Result<(), &'static str> {
    let client = connect(env, admin_pass, ca, "oo_a383_api").await?;
    for _ in 0..100 {
        let row = client.query_opt("SELECT pid IS NOT NULL FROM oo_source_control.operation WHERE operation_id = $1", &[&operation_id]).await.map_err(|_| "pid")?;
        if row.as_ref().and_then(|row| row.try_get(0).ok()) == Some(true) {
            mark(env, "pid", "present");
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("pid")
}

async fn wait_import(env: &Env, http: &reqwest::Client, session: &Session, operation_id: &str, label: &'static str, loops: u32, pred: impl Fn(&Value) -> bool) -> Result<Value, &'static str> {
    let mut last = String::from("missing");
    for _ in 0..loops {
        let row = get_import(http, session, operation_id).await?;
        last = row["status"].as_str().unwrap_or("shape").to_string();
        if pred(&row) { return Ok(row); }
        if last == "ready" || last == "ready_for_trusted_query" { return Err("ready"); }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    mark(env, label, &last);
    Err(label)
}

async fn get_import(http: &reqwest::Client, session: &Session, operation_id: &str) -> Result<Value, &'static str> {
    let (status, body) = get_api(http, &format!("/control/v1/imports/{operation_id}"), Some(&session.cookie)).await?;
    if status != 200 { return Err("import_get"); }
    Ok(body)
}

async fn login(http: &reqwest::Client, bootstrap: Option<&str>) -> Result<Session, &'static str> {
    let url = match bootstrap {
        Some(token) => format!("{IDENTITY_ORIGIN}/control/v1/auth/login?return_to=%2Fapp&bootstrap_token={token}"),
        None => format!("{IDENTITY_ORIGIN}/control/v1/auth/login?return_to=%2Fapp"),
    };
    let start = http.get(url).send().await.map_err(|_| "login")?;
    if start.status().as_u16() != 302 { return Err("login"); }
    let login_cookie = one_cookie(start.headers(), "__Host-oo-login").ok_or("login")?;
    let next = start.headers().get("location").and_then(|value| value.to_str().ok()).ok_or("login")?.to_string();
    let authorize = http.get(next).send().await.map_err(|_| "login")?;
    if authorize.status().as_u16() != 302 { return Err("login"); }
    let callback = authorize.headers().get("location").and_then(|value| value.to_str().ok()).ok_or("login")?.to_string();
    let done = http.get(callback).header("cookie", format!("__Host-oo-login={login_cookie}")).send().await.map_err(|_| "login")?;
    if done.status().as_u16() != 302 { return Err("login"); }
    let cookie = one_cookie(done.headers(), "__Host-oo-session").ok_or("login")?;
    if cookie.len() != 43 { return Err("login"); }
    Ok(Session { cookie, csrf: String::new(), revision: 0 })
}

async fn refresh(http: &reqwest::Client, session: &mut Session) -> Result<(), &'static str> {
    let response = http
        .get(format!("{IDENTITY_ORIGIN}/control/v1/identity/current"))
        .header("accept", "application/json")
        .header("cookie", format!("__Host-oo-session={}", session.cookie))
        .send()
        .await
        .map_err(|_| "current")?;
    let (status, body) = read_response(response).await?;
    if status != 200 { return Err("current"); }
    session.csrf = text_field(&body, "csrf_token", "current")?;
    session.revision = body["revision"].as_i64().ok_or("current")?;
    if session.csrf.len() != 43 || session.revision < 1 { return Err("current"); }
    Ok(())
}

async fn get_api(http: &reqwest::Client, path: &str, cookie: Option<&str>) -> Result<(u16, Value), &'static str> {
    let mut request = http.get(format!("{ORIGIN}{path}")).header("accept", "application/json");
    if let Some(cookie) = cookie {
        request = request.header("cookie", format!("__Host-oo-session={cookie}"));
    }
    let response = request.send().await.map_err(|_| "http")?;
    read_response(response).await
}

async fn post_api(http: &reqwest::Client, session: &Session, path: &str, body: &[u8]) -> Result<(u16, Value), &'static str> {
    let response = http.post(format!("{ORIGIN}{path}"))
        .header("cookie", format!("__Host-oo-session={}", session.cookie))
        .header("origin", ORIGIN)
        .header("x-oo-csrf", &session.csrf)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body.to_vec())
        .send().await.map_err(|_| "http")?;
    read_response(response).await
}

async fn read_response(response: reqwest::Response) -> Result<(u16, Value), &'static str> {
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|_| "http")?;
    scrub(&bytes)?;
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Ok((status, value))
}

async fn connect(env: &Env, password: &str, ca: &[u8], database: &str) -> Result<Client, &'static str> {
    let mut pg = tokio_postgres::Config::new();
    pg.user("oo_fixture_admin");
    pg.password(password);
    pg.host(&env.host);
    pg.port(env.port);
    pg.dbname(database);
    pg.application_name("oo-a383-live");
    pg.ssl_mode(tokio_postgres::config::SslMode::Require);
    let tls = pg_tls(ca).map_err(|_| "tls")?;
    let (client, connection) = pg.connect(tls).await.map_err(|_| "tcp")?;
    tokio::spawn(async move { let _ = connection.await; });
    client.batch_execute("SET TIME ZONE 'UTC'").await.map_err(|_| "session")?;
    Ok(client)
}

async fn ensure_role(client: &Client, name: &str, password: &str, extra: &str) -> Result<(), &'static str> {
    let sql = format!("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{name}') THEN CREATE ROLE {name} {extra} LOGIN PASSWORD '{password}'; ELSE ALTER ROLE {name} WITH {extra} LOGIN PASSWORD '{password}'; END IF; END $$;");
    client.batch_execute(&sql).await.map_err(|_| "role")
}

async fn ensure_nologin(client: &Client, name: &str) -> Result<(), &'static str> {
    let sql = format!("DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{name}') THEN CREATE ROLE {name} NOLOGIN; END IF; END $$;");
    client.batch_execute(&sql).await.map_err(|_| "role")
}

async fn ensure_database(client: &Client, name: &str, owner: &str) -> Result<(), &'static str> {
    let found = client.query_opt("SELECT 1 FROM pg_database WHERE datname = $1", &[&name]).await.map_err(|_| "database")?;
    if found.is_none() {
        client.batch_execute(&format!("CREATE DATABASE {name} OWNER {owner}")).await.map_err(|_| "database")?;
    }
    Ok(())
}

async fn drop_pipeline_roles(client: &Client) -> Result<(), &'static str> {
    client
        .batch_execute(
            "DO $$ DECLARE member text; BEGIN \
             IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN \
               FOR member IN SELECT r.rolname FROM pg_roles r JOIN pg_auth_members m ON m.member = r.oid JOIN pg_roles g ON g.oid = m.roleid WHERE g.rolname = 'oo_pipeline_runtime' LOOP \
                 EXECUTE format('REVOKE oo_pipeline_runtime FROM %I', member); \
               END LOOP; \
               EXECUTE 'DROP OWNED BY oo_pipeline_runtime'; \
             END IF; \
             IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_target_owner') THEN \
               FOR member IN SELECT r.rolname FROM pg_roles r JOIN pg_auth_members m ON m.member = r.oid JOIN pg_roles g ON g.oid = m.roleid WHERE g.rolname = 'oo_target_owner' LOOP \
                 EXECUTE format('REVOKE oo_target_owner FROM %I', member); \
               END LOOP; \
               EXECUTE 'DROP OWNED BY oo_target_owner'; \
             END IF; \
             END $$; \
             DROP ROLE IF EXISTS oo_pipeline_runtime; \
             DROP ROLE IF EXISTS oo_target_owner;",
        )
        .await
        .map_err(|_| "role")
}

async fn recreate_database(client: &Client, name: &str, owner: &str) -> Result<(), &'static str> {
    client
        .batch_execute(&format!("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{name}' AND pid <> pg_backend_pid()"))
        .await
        .map_err(|_| "database")?;
    client.batch_execute(&format!("DROP DATABASE IF EXISTS {name}")).await.map_err(|_| "database")?;
    client.batch_execute(&format!("CREATE DATABASE {name} OWNER {owner}")).await.map_err(|_| "database")?;
    Ok(())
}

async fn wait_outcome(path: &Path, want: &str) -> Result<(), &'static str> {
    for _ in 0..150 {
        if let Ok(text) = fs::read_to_string(path) {
            if text.contains(want) { return Ok(()); }
            if text.contains("\"outcome\":\"refused\"") || text.contains("\"outcome\":\"malformed\"") || text.contains("\"outcome\":\"commit_failed\"") {
                return Err("serve");
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err("serve")
}

async fn wait_child(child: &mut std::process::Child, label: &'static str) -> Result<(), &'static str> {
    for _ in 0..50 {
        if child.try_wait().map_err(|_| label)?.is_some() { return Ok(()); }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(label)
}

fn spawn_service(bin: &Path, args: &[&str], stdout: &Path, stderr: &Path) -> Result<std::process::Child, &'static str> {
    let out = OpenOptions::new().create(true).append(true).open(stdout).map_err(|_| "spawn")?;
    let err = OpenOptions::new().create(true).append(true).open(stderr).map_err(|_| "spawn")?;
    let mut command = Command::new(bin);
    command.args(args).env("PATH", "/usr/bin:/bin").env("HOME", "/tmp").env("LANG", "C.UTF-8").current_dir("/tmp").stdin(Stdio::null()).stdout(out).stderr(err).process_group(0);
    command.spawn().map_err(|_| "spawn")
}

fn run_to_exit(bin: &Path, args: &[&str], log: &Path, label: &'static str) -> Result<(), &'static str> {
    let file = OpenOptions::new().create(true).truncate(true).write(true).open(log).map_err(|_| "spawn")?;
    let status = Command::new(bin).args(args).env("PATH", "/usr/bin:/bin").env("HOME", "/tmp").env("LANG", "C.UTF-8").current_dir("/tmp").stdin(Stdio::null()).stdout(file.try_clone().map_err(|_| "spawn")?).stderr(file).status().map_err(|_| "spawn")?;
    let bytes = fs::read(log).unwrap_or_default();
    scrub(&bytes)?;
    if status.success() { Ok(()) } else { Err(label) }
}

fn write_identity_config(env: &Env, commit: &str) -> Result<(), &'static str> {
    let value = json!({
        "https_origin": IDENTITY_ORIGIN,
        "listen_host": "127.0.0.1",
        "listen_port": 8444,
        "tls_cert_file": "/secrets/server.crt",
        "tls_key_file": "/secrets/server.key",
        "issuer": ISSUER,
        "audience": CLIENT_ID,
        "client_id": CLIENT_ID,
        "client_secret_file": "/secrets/client.secret",
        "redirect_uris": [format!("{IDENTITY_ORIGIN}/control/v1/auth/callback")],
        "config_commit": commit,
        "revision": 1,
        "idp_ca_file": "/secrets/ca.crt",
        "pg": {"host": env.host, "port": env.port, "database": "oo_a383_identity", "user": "oo_identity_admin", "password_file": "/secrets/identity.pass", "ca_file": "/secrets/ca.crt"},
        "bindings": [
            {"subject": "a383-admin", "project_id": PROJECT, "role": "admin"},
            {"subject": "a383-audit", "project_id": PROJECT, "role": "auditor"},
            {"subject": "a383-other", "project_id": OTHER, "role": "admin"}
        ],
        "bootstrap": {"subject": "a383-admin", "project_id": PROJECT, "role": "admin"},
        "idle_seconds": 3600,
        "absolute_seconds": 86400,
        "oauth_ttl_seconds": 300,
        "bootstrap_ttl_seconds": 600,
        "clock_skew_seconds": 5
    });
    write_private(&env.work.join("identity.json"), &serde_json::to_vec(&value).map_err(|_| "config")?)
}

fn write_api_config(env: &Env) -> Result<(), &'static str> {
    let pin = |name: &str| -> Result<Value, &'static str> {
        let path = env.bins.join(name);
        let bytes = fs::read(&path).map_err(|_| "pin")?;
        Ok(json!({"path": path, "sha256": sha256_hex(&bytes)}))
    };
    let value = json!({
        "protocol": "oo.source-control-api/v1",
        "https_origin": ORIGIN,
        "listen_host": "127.0.0.1",
        "listen_port": 8445,
        "tls_cert_file": "/secrets/server.crt",
        "tls_key_file": "/secrets/server.key",
        "identity_origin": IDENTITY_ORIGIN,
        "identity_ca_file": "/secrets/ca.crt",
        "deployment_origins": [ORIGIN],
        "pg": {"host": env.host, "port": env.port, "database": "oo_a383_api", "user": "oo_a383_api", "password_file": "/secrets/api.pass", "ca_file": "/secrets/ca.crt"},
        "admitted_hosts": ["oo-a383-pg"],
        "secrets": [{"secret_ref": "secret://pg/a383/reader", "file": "/secrets/reader.pass"}],
        "upload_max_bytes": 16777216,
        "object_dir": "/tmp/oo-a383-work/objects",
        "run_dir": "/tmp/oo-a383-work/runs",
        "owner": "eef831aa-dba4-4af2-a78b-541bc9e0a383",
        "comment_markers": ["甲注评"],
        "producers": {
            "pg_onboarding": pin("pg-onboarding")?,
            "file_import": pin("file-import-pg")?,
            "init_pg": pin("init-pg")?,
            "pipeline": pin("oo-pipeline")?,
            "builder": pin("oo-builder")?,
            "worker": pin("oo-file-parser-worker")?
        },
        "target": {
            "host": env.host,
            "port": env.port,
            "pg_database": "oo_a383_file",
            "file_database": "oo_a383_file",
            "administrator": "oo_fixture_admin",
            "runtime_principal": "oo_a383_runtime",
            "admin_password_file": "/secrets/admin.pass",
            "runtime_password_file": "/secrets/runtime.pass",
            "ca_file": "/secrets/ca.crt"
        }
    });
    fs::DirBuilder::new().recursive(true).mode(0o700).create("/tmp/oo-a383-work/objects").map_err(|_| "work")?;
    fs::DirBuilder::new().recursive(true).mode(0o700).create("/tmp/oo-a383-work/runs").map_err(|_| "work")?;
    write_private(&env.work.join("api.json"), &serde_json::to_vec(&value).map_err(|_| "config")?)
}

fn identity_commit() -> String {
    let value = json!({
        "audience": CLIENT_ID,
        "bindings": [
            {"project_id": PROJECT, "role": "admin", "subject": "a383-admin"},
            {"project_id": PROJECT, "role": "auditor", "subject": "a383-audit"},
            {"project_id": OTHER, "role": "admin", "subject": "a383-other"}
        ],
        "bootstrap": {"customer_owner": false, "fixture_class": "explicit_test_fixture", "project_id": PROJECT, "role": "admin", "subject": "a383-admin"},
        "client_id": CLIENT_ID,
        "issuer": ISSUER,
        "protocol": "oo.control-identity/config-commit/v1",
        "redirect_uris": [format!("{IDENTITY_ORIGIN}/control/v1/auth/callback")],
        "revision": 1
    });
    sha256_hex(serde_json::to_string(&value).unwrap_or_default().as_bytes())
}

fn register_shop(session: &Session, key: &str) -> Vec<u8> {
    body(&json!({
        "project_id": PROJECT,
        "identity_revision": session.revision,
        "source_ref": "shop_src",
        "kind": "pg_local",
        "idempotency_key": key,
        "secret_ref": "secret://pg/a383/reader",
        "connection": {"host": "oo-a383-pg", "port": 5432, "database": "oo_a383_src", "username": "oo_a383_reader"},
        "tables": [{"schema": "shop", "table": "customer"}, {"schema": "shop", "table": "order_doc"}]
    }))
}

fn register_pg(session: &Session, source: &str, host: &str, key: &str, secret: &str) -> Vec<u8> {
    body(&json!({
        "project_id": PROJECT,
        "identity_revision": session.revision,
        "source_ref": source,
        "kind": "pg_local",
        "idempotency_key": key,
        "secret_ref": secret,
        "connection": {"host": host, "port": 5432, "database": "oo_a383_src", "username": "oo_a383_reader"},
        "tables": [{"schema": "shop", "table": "customer"}]
    }))
}

fn register_file(session: &Session, source: &str, key: &str) -> Vec<u8> {
    body(&json!({"project_id": PROJECT, "identity_revision": session.revision, "source_ref": source, "kind": "file_local", "idempotency_key": key}))
}

fn password_body(session: &Session) -> Vec<u8> {
    body(&json!({
        "project_id": PROJECT,
        "identity_revision": session.revision,
        "source_ref": "shop_src",
        "kind": "pg_local",
        "idempotency_key": "a383-bad-password01",
        "password": "no",
        "secret_ref": "secret://pg/a383/reader",
        "connection": {"host": "oo-a383-pg", "port": 5432, "database": "oo_a383_src", "username": "oo_a383_reader"},
        "tables": [{"schema": "shop", "table": "customer"}]
    }))
}

fn file_import(session: &Session, source: &str, sha: &str, max_rows: u64, key: &str, expected: i64) -> Vec<u8> {
    body(&json!({
        "project_id": PROJECT,
        "identity_revision": session.revision,
        "source_ref": source,
        "kind": "file_replace",
        "idempotency_key": key,
        "expected_source_revision": expected,
        "object_sha256": sha,
        "profile": profile(),
        "budget": budget(max_rows)
    }))
}

fn pg_inspect(session: &Session) -> Vec<u8> {
    body(&json!({"project_id": PROJECT, "identity_revision": session.revision, "source_ref": "shop_src", "kind": "pg_inspect", "idempotency_key": "a383-imp-shop-00001", "expected_source_revision": 1}))
}

fn action_body(session: &Session, key: &str) -> Vec<u8> {
    body(&json!({"project_id": PROJECT, "identity_revision": session.revision, "idempotency_key": key}))
}

fn profile() -> Value {
    json!({"format": "csv", "id": "rows_id", "bom": "refuse", "header": "exact", "null": {"policy": "empty"}, "columns": [{"name": "id", "parse": "int64", "pg": "int8", "nullable": false, "primary_key": true, "precision": null, "scale": null}]})
}

fn budget(max_rows: u64) -> Value {
    json!({"batch_rows": 16384, "batch_bytes": 8388608, "max_record_bytes": 4096, "max_field_bytes": 64, "max_total_bytes": 8388608, "max_rows": max_rows, "max_columns": 4, "max_fields": 4, "worker_deadline_ms": 120000, "parent_deadline_ms": 120000})
}

fn csv_ids(rows: u64) -> Vec<u8> {
    let mut out = b"id\n".to_vec();
    for id in 1..=rows {
        out.extend(id.to_string().into_bytes());
        out.push(b'\n');
    }
    out
}

fn body(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

fn one_cookie(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    for value in headers.get_all("set-cookie") {
        let Ok(text) = value.to_str() else { continue };
        let pair = text.split(';').next().unwrap_or("");
        if let Some(rest) = pair.trim().strip_prefix(&prefix)
            && !rest.is_empty()
        {
            return Some(rest.to_string());
        }
    }
    None
}

fn text_field(value: &Value, key: &str, label: &'static str) -> Result<String, &'static str> {
    value.get(key).and_then(Value::as_str).map(str::to_string).ok_or(label)
}

fn code_of(value: &Value) -> String {
    value.get("code").and_then(Value::as_str).unwrap_or("").to_string()
}

fn detail_of(value: &Value) -> String {
    value.get("detail").and_then(Value::as_str).unwrap_or("").to_string()
}

fn expect(status: u16, want: u16, label: &'static str) -> Result<(), &'static str> {
    if status == want { Ok(()) } else { Err(label) }
}

fn expect_error(value: &Value, code: &str, detail: &str, label: &'static str) -> Result<(), &'static str> {
    if value["code"] == code && value["detail"] == detail { Ok(()) } else { Err(label) }
}

fn sql_ident(value: &str) -> bool {
    !value.is_empty() && value.len() <= 63 && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn mark(env: &Env, step: &str, detail: &str) {
    if scrub(detail.as_bytes()).is_err() { return; }
    let mut file = match OpenOptions::new().create(true).append(true).open(env.evidence.join("steps.jsonl")) {
        Ok(file) => file,
        Err(_) => return,
    };
    let _ = writeln!(file, "{{\"step\":\"{step}\",\"detail\":\"{detail}\"}}");
}

fn scrub(bytes: &[u8]) -> Result<(), &'static str> {
    let dir = std::env::var("OO_A383_SECRETS").unwrap_or_default();
    for name in ["admin.pass", "api.pass", "identity.pass", "reader.pass", "writer.pass", "runtime.pass", "client.secret"] {
        if let Ok(secret) = fs::read(Path::new(&dir).join(name)) {
            let secret = trim_newline(secret);
            if secret.len() >= 8 && bytes.windows(secret.len()).any(|window| window == secret) {
                return Err("secret_leak");
            }
        }
    }
    Ok(())
}

fn trim_newline(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.ends_with(b"\n") { bytes.pop(); }
    bytes
}

fn read_secret(path: &Path) -> Result<String, &'static str> {
    let bytes = trim_newline(fs::read(path).map_err(|_| "secret_file")?);
    if bytes.is_empty() || bytes.iter().any(|byte| !byte.is_ascii_hexdigit()) { return Err("secret_file"); }
    String::from_utf8(bytes).map_err(|_| "secret_file")
}

fn read_text(path: &Path) -> Result<String, &'static str> {
    let mut text = fs::read_to_string(path).map_err(|_| "read")?;
    if text.ends_with('\n') { text.pop(); }
    if text.is_empty() { Err("read") } else { Ok(text) }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), &'static str> {
    let mut file = OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(path).map_err(|_| "write")?;
    file.write_all(bytes).map_err(|_| "write")?;
    Ok(())
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap_or("")
}

impl Env {
    fn load() -> Result<Self, &'static str> {
        let port = std::env::var("OO_A383_PG_PORT").map_err(|_| "env")?.parse().map_err(|_| "env")?;
        Ok(Self {
            host: std::env::var("OO_A383_PG_HOST").map_err(|_| "env")?,
            port,
            secrets: PathBuf::from(std::env::var("OO_A383_SECRETS").map_err(|_| "env")?),
            bins: PathBuf::from(std::env::var("OO_A383_BINS").map_err(|_| "env")?),
            work: PathBuf::from(std::env::var("OO_A383_WORK").map_err(|_| "env")?),
            evidence: PathBuf::from(std::env::var("OO_A383_EVIDENCE").map_err(|_| "env")?),
            idp_der: PathBuf::from(std::env::var("OO_A383_IDP_DER").map_err(|_| "env")?),
            head: std::env::var("OO_A383_HEAD").unwrap_or_default(),
        })
    }
}
