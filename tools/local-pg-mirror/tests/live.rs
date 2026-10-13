//! Public-fixture proof. Two private PostgreSQL instances. Not an enterprise copy.
//! The activity sampler is a source-admin monitor. It is not the source business login.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use local_pg_mirror::{
    copy, discover, read_local, sealed_import, CopyRequest, CopyStop, DiscoverRequest, LocalRead, Manifest,
    ReadRequest, SOURCE_COPY_BINARY, SOURCE_COPY_CONFIG_PROTOCOL,
};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};

const GOLD_OWNER: &str = "11111111-1111-4111-8111-111111111111";
const CANCEL_OWNER: &str = "22222222-2222-4222-8222-222222222222";
const PARTIAL_OWNER: &str = "33333333-3333-4333-8333-333333333333";
const ODD_OWNER: &str = "44444444-4444-4444-8444-444444444444";
const TIMEOUT_OWNER: &str = "55555555-5555-4555-8555-555555555555";
const DROP_OWNER: &str = "66666666-6666-4666-8666-666666666666";
const BULK_OWNER: &str = "77777777-7777-4777-8777-777777777777";

struct Fixture {
    source_host: String,
    source_port: u16,
    source_server_name: String,
    target_host: String,
    target_port: u16,
    target_server_name: String,
    source_database: String,
    target_database: String,
    admin_user: String,
    source_user: String,
    runtime_user: String,
    reader_user: String,
    source_admin_password: PathBuf,
    target_admin_password: PathBuf,
    source_password: PathBuf,
    runtime_password: PathBuf,
    reader_password: PathBuf,
    ca: PathBuf,
    ca_sha256: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn public_fixture_copy_is_local() {
    assert_eq!(SOURCE_COPY_BINARY, "local-pg-mirror");
    assert_eq!(SOURCE_COPY_CONFIG_PROTOCOL, "oo.pipeline.local-pg-mirror/v1");
    let fixture = load_fixture();
    let root = std::env::temp_dir().join("a410-mirror");
    std::fs::create_dir_all(&root).unwrap_or_else(|_| panic!("workdir"));
    let source_admin = secret(&fixture.source_admin_password);
    let target_admin = secret(&fixture.target_admin_password);
    let src_pass = secret(&fixture.source_password);
    let runtime_pass = secret(&fixture.runtime_password);
    let reader_pass = secret(&fixture.reader_password);
    let ca = std::fs::read(&fixture.ca).unwrap_or_else(|_| panic!("ca file"));

    let mut admin = dial(&fixture.source_host, fixture.source_port, &fixture.admin_user, &source_admin, "postgres", &ca).await;
    raw(&mut admin, "CREATE DATABASE srcdb TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
    raw(&mut admin, &format!("CREATE ROLE oo_src_read LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION PASSWORD '{src_pass}'")).await;
    drop(admin);

    let mut src = dial(&fixture.source_host, fixture.source_port, &fixture.admin_user, &source_admin, &fixture.source_database, &ca).await;
    raw(&mut src, "REVOKE ALL ON DATABASE srcdb FROM PUBLIC").await;
    raw(&mut src, "REVOKE CREATE ON DATABASE srcdb FROM PUBLIC").await;
    raw(&mut src, "REVOKE ALL ON SCHEMA public FROM PUBLIC").await;
    raw(&mut src, "REVOKE CREATE ON SCHEMA public FROM PUBLIC").await;
    raw(&mut src, "CREATE TABLE public.amounts (id int8 PRIMARY KEY, qty int8, price numeric(12,2), note text)").await;
    raw(&mut src, "INSERT INTO public.amounts VALUES (1, 9223372036854775807, 10.50, NULL), (2, -5, 0.00, 'kept')").await;
    raw(&mut src, "CREATE TABLE public.events (id int8 PRIMARY KEY, seen timestamp, body json, blob bytea)").await;
    raw(&mut src, "INSERT INTO public.events VALUES (7, timestamp '2024-03-04 05:06:07', '{\"k\":\"v\"}'::json, '\\x0001ff'::bytea)").await;
    raw(&mut src, "CREATE TABLE public.flags (id int8 PRIMARY KEY, on_flag bool, label text)").await;
    raw(&mut src, "INSERT INTO public.flags VALUES (1, true, 'yes'), (2, false, '')").await;
    raw(&mut src, "CREATE TABLE public.odd (id int8 PRIMARY KEY, doc jsonb)").await;
    raw(&mut src, "INSERT INTO public.odd VALUES (1, '{\"a\":1}'::jsonb)").await;
    raw(&mut src, "CREATE TABLE public.batch_a (id int8 PRIMARY KEY, amount numeric(38,4), label text)").await;
    raw(&mut src, "INSERT INTO public.batch_a SELECT g, CASE WHEN g = 1 THEN NULL WHEN g = 2 THEN 1234567890123456789012345678901234.1234 ELSE 0.0001 END, CASE WHEN g = 3 THEN '' ELSE 'n' END FROM generate_series(1, 2000) AS g").await;
    raw(&mut src, "CREATE TABLE public.batch_b (id int8 PRIMARY KEY, at_time time, blob bytea)").await;
    raw(&mut src, "INSERT INTO public.batch_b SELECT g, CASE WHEN g = 2 THEN NULL ELSE time '13:14:15.123456' END, CASE WHEN g = 3 THEN NULL ELSE '\\x0001ff'::bytea END FROM generate_series(1, 8000) AS g").await;
    raw(&mut src, "CREATE TABLE public.batch_c (id int8 PRIMARY KEY, qty int8, amount numeric(38,4), note text)").await;
    raw(&mut src, "INSERT INTO public.batch_c SELECT g, CASE WHEN g = 1 THEN 9223372036854775807 WHEN g = 2 THEN -9223372036854775808 ELSE g END, CASE WHEN g = 4 THEN NULL ELSE 1.5000 END, CASE WHEN g = 5 THEN NULL ELSE 'c' END FROM generate_series(1, 90000) AS g").await;
    raw(&mut src, "GRANT CONNECT ON DATABASE srcdb TO oo_src_read").await;
    raw(&mut src, "GRANT USAGE ON SCHEMA public TO oo_src_read").await;
    raw(&mut src, "GRANT SELECT ON TABLE public.amounts, public.events, public.flags, public.odd, public.batch_a, public.batch_b, public.batch_c TO oo_src_read").await;
    drop(src);

    let mut target = dial(&fixture.target_host, fixture.target_port, &fixture.admin_user, &target_admin, "postgres", &ca).await;
    raw(&mut target, "CREATE DATABASE mirrordb TEMPLATE template0 ENCODING 'UTF8' LOCALE_PROVIDER builtin BUILTIN_LOCALE 'C.UTF-8'").await;
    raw(&mut target, &format!("CREATE ROLE oo_mirror_runtime LOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '{runtime_pass}'")).await;
    raw(&mut target, &format!("CREATE ROLE oo_mirror_read LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '{reader_pass}'")).await;
    drop(target);

    let before = source_writes(&fixture, &source_admin, &ca).await;
    let (stop, seen, handle) = watch_source(&fixture, &source_admin, &ca);
    let gold_tables = [("public", "amounts"), ("public", "events"), ("public", "flags")];
    let gold = run_copy(&fixture, &root, "gold", "fixa", "gold", GOLD_OWNER, &gold_tables, 300_000, 10_000, CopyStop::Finish).await;
    stop.store(true, Ordering::Relaxed);
    let after_peak = handle.await.unwrap_or_else(|_| panic!("sampler"));
    let after = source_writes(&fixture, &source_admin, &ca).await;
    assert_eq!(before, after);
    assert!(seen.load(Ordering::Relaxed) <= 1);
    assert_eq!(after_peak, 0);
    assert_eq!((gold.manifest.mirror_state.as_str(), gold.manifest.failure.as_str()), ("complete", "none"));
    assert_eq!(gold.manifest.rows, 5);
    assert_eq!(gold.manifest.consistency, "single_snapshot");
    assert_eq!(gold.manifest.source_socket_peak, Some(1));
    assert_eq!(gold.manifest.source_socket_fact, "repeatable_read_read_only");
    assert_eq!(gold.manifest.recheck_connections, 0);
    assert!(gold.manifest.source_read_only);
    assert_eq!(gold.manifest.source_write_tuples, Some(0));
    assert_eq!(gold.manifest.index_count, 3);
    assert_eq!(gold.manifest.analyze_tables, 3);
    assert_eq!(gold.manifest.spool_state, "complete");
    assert!(gold.manifest.import_generation.is_some());
    assert!(gold.manifest.import_hash.is_some());
    assert_eq!(gold.manifest.tables_completed.len(), 3);
    assert!(gold.manifest.tables_incomplete.is_empty());
    assert_public(&gold.manifest);
    assert_names(&gold.manifest, "amounts", &["id", "qty", "price", "note"]);
    assert_names(&gold.manifest, "events", &["id", "seen", "body", "blob"]);
    assert_names(&gold.manifest, "flags", &["id", "on_flag", "label"]);
    let sealed = sealed_import(&gold.manifest).unwrap_or_else(|err| panic!("seal {} {}", err.code, err.fact));
    assert_eq!(sealed.import_generation, gold.manifest.import_generation.unwrap_or(0));
    assert_eq!(sealed.import_hash, gold.manifest.import_hash.clone().unwrap_or_default());
    assert_eq!(source_backends(&fixture, &source_admin, &ca).await, 0);
    assert_target_config(&fixture, &root, "gold");

    let local = read_back(&fixture, &root, "gold").await;
    assert_eq!(local.tables.len(), 3);
    assert_oids(&local, "amounts", &[20, 20, 1700, 25]);
    assert_oids(&local, "events", &[20, 1114, 114, 17]);
    assert_oids(&local, "flags", &[20, 16, 25]);
    assert_eq!(table(&local, "amounts"), vec![
        vec![Some("1".to_string()), Some("9223372036854775807".to_string()), Some("10.50".to_string()), None],
        vec![Some("2".to_string()), Some("-5".to_string()), Some("0.00".to_string()), Some("kept".to_string())],
    ]);
    assert_eq!(table(&local, "events"), vec![vec![
        Some("7".to_string()),
        Some("2024-03-04 05:06:07".to_string()),
        Some("{\"k\":\"v\"}".to_string()),
        Some("0001ff".to_string()),
    ]]);
    assert_eq!(table(&local, "flags"), vec![
        vec![Some("1".to_string()), Some("true".to_string()), Some("yes".to_string())],
        vec![Some("2".to_string()), Some("false".to_string()), Some(String::new())],
    ]);

    let mut src = dial(&fixture.source_host, fixture.source_port, &fixture.admin_user, &source_admin, &fixture.source_database, &ca).await;
    raw(&mut src, "ALTER TABLE public.amounts ADD COLUMN extra text").await;
    drop(src);
    let drifted = copy(copy_request(&fixture, &root, "gold", "gold-drift", CopyStop::Finish))
        .await
        .unwrap_or_else(|err| panic!("drift {} {}", err.code, err.fact));
    assert_eq!(drifted.manifest.mirror_state, "refused");
    assert_eq!(drifted.manifest.failure, "catalog_drift");
    assert_eq!(drifted.manifest.source_socket_peak, Some(1));
    assert_eq!(drifted.manifest.recheck_connections, 0);
    assert_eq!(drifted.manifest.source_socket_fact, "repeatable_read_read_only");
    assert_eq!(drifted.manifest.loader_state, "none");
    assert!(drifted.manifest.import_generation.is_none());
    assert_public(&drifted.manifest);
    assert_eq!(table(&read_back(&fixture, &root, "gold").await, "amounts").len(), 2);

    let flags = [("public", "flags")];
    let cancelled = run_copy(&fixture, &root, "cancel", "fixc", "cancel", CANCEL_OWNER, &flags, 300_000, 10_000, CopyStop::CancelBeforePull).await;
    assert_eq!(cancelled.manifest.mirror_state, "incomplete");
    assert_eq!(cancelled.manifest.rows, 0);
    assert_eq!(cancelled.manifest.failure, "cancel");
    assert_eq!(cancelled.manifest.source_socket_peak, Some(1));
    assert_eq!(cancelled.manifest.recheck_connections, 0);
    assert_eq!(cancelled.manifest.source_socket_fact, "repeatable_read_read_only");
    assert_ne!(cancelled.manifest.consistency, "single_snapshot");
    assert_public(&cancelled.manifest);

    let partial = run_copy(&fixture, &root, "partial", "fixp", "partial", PARTIAL_OWNER, &flags, 300_000, 10_000, CopyStop::StopAfterBatches(1)).await;
    assert_eq!(partial.manifest.mirror_state, "incomplete");
    assert_eq!(partial.manifest.failure, "stop_after_batches");
    assert!(partial.manifest.tables_completed.is_empty());
    let physical = partial.manifest.tables_incomplete.iter().find_map(|table| table.physical_name.clone()).unwrap_or_else(|| panic!("partial physical"));
    assert_public(&partial.manifest);

    let odd = [("public", "odd")];
    let unsupported = run_copy(&fixture, &root, "odd", "fixu", "unsupported", ODD_OWNER, &odd, 300_000, 10_000, CopyStop::Finish).await;
    assert_eq!(unsupported.manifest.mirror_state, "refused");
    assert_eq!(unsupported.manifest.failure, "unsupported_column");
    assert_eq!(unsupported.manifest.source_socket_peak, Some(1));
    assert_eq!(unsupported.manifest.source_socket_fact, "repeatable_read_read_only");
    assert_eq!(unsupported.manifest.recheck_connections, 0);
    assert_eq!(unsupported.manifest.loader_state, "none");
    assert!(unsupported.manifest.quarantine.iter().any(|row| row.oid == 3802 && row.reason == "unsupported_column"));
    assert_public(&unsupported.manifest);

    let timed = run_copy(&fixture, &root, "timed", "fixt", "timeout", TIMEOUT_OWNER, &flags, 1, 10_000, CopyStop::Finish).await;
    assert_eq!(timed.manifest.mirror_state, "incomplete");
    assert_eq!(timed.manifest.failure, "deadline");
    assert_eq!(timed.manifest.source_socket_peak, Some(0));
    assert_eq!(timed.manifest.source_socket_fact, "not_opened");
    assert_public(&timed.manifest);

    let absent = root.join("absent-secret");
    assert!(!absent.exists());
    let replay_err = copy(CopyRequest {
        plan_path: root.join("partial.plan.json"),
        source_password: absent.clone(),
        source_ca: fixture.ca.clone(),
        admin_password: absent.clone(),
        runtime_password: absent.clone(),
        manifest_path: root.join("partial.manifest.json"),
        target_config_path: root.join("partial.target.json"),
        target_ca: fixture.ca.clone(),
        stop: CopyStop::Finish,
    })
    .await
    .expect_err("resume");
    assert_eq!(replay_err.code, "resume_refused");
    assert!(!absent.exists());

    let refused = read_local(ReadRequest {
        target_config: root.join("partial.target.json"),
        manifest: root.join("partial.manifest.json"),
        reader_password: fixture.reader_password.clone(),
        ca: fixture.ca.clone(),
    })
    .await
    .expect_err("partial read");
    assert_eq!(refused.code, "refused");
    assert_eq!(refused.fact, "not_complete");
    let quoted = oo_pipeline::physical_mapping::quote_physical_name(&physical).unwrap_or_else(|_| panic!("physical name"));
    let mut local_reader = dial(&fixture.target_host, fixture.target_port, &fixture.reader_user, &reader_pass, &fixture.target_database, &ca).await;
    match sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SELECT 1 FROM \"_oo\".{quoted}"))).execute(&mut local_reader).await {
        Ok(_) => panic!("partial select was allowed"),
        Err(err) => assert_eq!(fail_class(&err), "42501"),
    }
    drop(local_reader);

    let dropped = run_copy(&fixture, &root, "dropped", "fixd", "dropped", DROP_OWNER, &flags, 300_000, 10_000, CopyStop::DropAfterSnapshot).await;
    assert_eq!(dropped.manifest.mirror_state, "incomplete");
    assert_eq!(dropped.manifest.failure, "source_dropped");
    assert_eq!(dropped.manifest.spool_state, "complete");
    assert_eq!(dropped.manifest.source_socket_peak, Some(1));
    assert_eq!(dropped.manifest.recheck_connections, 0);
    assert_eq!(dropped.manifest.source_socket_fact, "repeatable_read_read_only");
    assert!(dropped.manifest.source_read_only);
    assert_eq!(dropped.manifest.source_write_tuples, Some(0));
    assert!(!root.join("dropped.target.json").exists());
    assert_eq!(source_backends(&fixture, &source_admin, &ca).await, 0);
    let never = root.join("never-created-secret");
    assert!(!never.exists());
    let resumed = copy(CopyRequest {
        plan_path: root.join("dropped.plan.json"),
        source_password: never.clone(),
        source_ca: fixture.ca.clone(),
        admin_password: fixture.target_admin_password.clone(),
        runtime_password: fixture.runtime_password.clone(),
        manifest_path: root.join("dropped.manifest.json"),
        target_config_path: root.join("dropped.target.json"),
        target_ca: fixture.ca.clone(),
        stop: CopyStop::Finish,
    })
    .await
    .unwrap_or_else(|err| panic!("resume drop {} {}", err.code, err.fact));
    assert!(!never.exists());
    assert_eq!(resumed.disposition, "stored");
    assert_eq!(resumed.manifest.mirror_state, "complete");
    assert_eq!(resumed.manifest.rows, 2);
    assert_eq!(resumed.manifest.source_socket_peak, Some(1));
    let resumed_seal = sealed_import(&resumed.manifest).unwrap_or_else(|err| panic!("drop seal {} {}", err.code, err.fact));
    assert_eq!(resumed_seal.import_generation, resumed.manifest.import_generation.unwrap_or(0));
    assert_eq!(table(&read_back(&fixture, &root, "dropped").await, "flags").len(), 2);

    let bulk_before = source_writes(&fixture, &source_admin, &ca).await;
    let (bulk_stop, bulk_seen, bulk_handle) = watch_source(&fixture, &source_admin, &ca);
    let bulk_tables = [("public", "batch_a"), ("public", "batch_b"), ("public", "batch_c")];
    let bulk = run_copy(&fixture, &root, "bulk", "fixk", "bulk", BULK_OWNER, &bulk_tables, 300_000, 100_000, CopyStop::Finish).await;
    bulk_stop.store(true, Ordering::Relaxed);
    let bulk_after_peak = bulk_handle.await.unwrap_or_else(|_| panic!("bulk sampler"));
    let bulk_after = source_writes(&fixture, &source_admin, &ca).await;
    assert_eq!(bulk_before, bulk_after);
    assert!(bulk_seen.load(Ordering::Relaxed) <= 1);
    assert_eq!(bulk_after_peak, 0);
    assert_eq!(bulk.manifest.mirror_state, "complete");
    assert_eq!(bulk.manifest.rows, 100_000);
    assert_eq!(bulk.manifest.source_socket_peak, Some(1));
    assert_eq!(bulk.manifest.source_write_tuples, Some(0));
    assert_eq!(bulk.manifest.recheck_connections, 0);
    assert_eq!(bulk.manifest.index_count, 3);
    assert_eq!(bulk.manifest.analyze_tables, 3);
    assert_names(&bulk.manifest, "batch_a", &["id", "amount", "label"]);
    assert_names(&bulk.manifest, "batch_b", &["id", "at_time", "blob"]);
    assert_names(&bulk.manifest, "batch_c", &["id", "qty", "amount", "note"]);
    let bulk_local = read_back(&fixture, &root, "bulk").await;
    assert_eq!(count_table(&bulk_local, "batch_a"), 2000);
    assert_eq!(count_table(&bulk_local, "batch_b"), 8000);
    assert_eq!(count_table(&bulk_local, "batch_c"), 90000);
    assert_oids(&bulk_local, "batch_a", &[20, 1700, 25]);
    assert_oids(&bulk_local, "batch_b", &[20, 1083, 17]);
    assert_oids(&bulk_local, "batch_c", &[20, 20, 1700, 25]);
    assert_eq!(find_row(&bulk_local, "batch_a", "2")[1].as_deref(), Some("1234567890123456789012345678901234.1234"));
    assert!(find_row(&bulk_local, "batch_a", "1")[1].is_none());
    assert_eq!(find_row(&bulk_local, "batch_a", "3")[2].as_deref(), Some(""));
    assert_eq!(find_row(&bulk_local, "batch_b", "1")[1].as_deref(), Some("13:14:15.123456"));
    assert_eq!(find_row(&bulk_local, "batch_b", "1")[2].as_deref(), Some("0001ff"));
    assert!(find_row(&bulk_local, "batch_b", "2")[1].is_none());
    assert!(find_row(&bulk_local, "batch_b", "3")[2].is_none());
    assert_eq!(find_row(&bulk_local, "batch_c", "1")[1].as_deref(), Some("9223372036854775807"));
    assert_eq!(find_row(&bulk_local, "batch_c", "2")[1].as_deref(), Some("-9223372036854775808"));
    assert!(find_row(&bulk_local, "batch_c", "4")[2].is_none());
    assert!(find_row(&bulk_local, "batch_c", "5")[3].is_none());
    let bulk_seal = sealed_import(&bulk.manifest).unwrap_or_else(|err| panic!("bulk seal {} {}", err.code, err.fact));
    assert_eq!(bulk_seal.import_hash, bulk.manifest.import_hash.clone().unwrap_or_default());

    let mut admin = dial(&fixture.source_host, fixture.source_port, &fixture.admin_user, &source_admin, "postgres", &ca).await;
    raw(&mut admin, "REVOKE CONNECT ON DATABASE srcdb FROM oo_src_read").await;
    raw(&mut admin, "SELECT pg_catalog.pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity WHERE datname = 'srcdb' AND usename = 'oo_src_read'").await;
    drop(admin);
    assert!(
        dial_result(&fixture.source_host, fixture.source_port, &fixture.source_user, &src_pass, &fixture.source_database, &ca).await.is_err(),
        "source connect was allowed"
    );
    assert_eq!(source_backends(&fixture, &source_admin, &ca).await, 0);
    let offline = read_back(&fixture, &root, "gold").await;
    assert_eq!(table(&offline, "events")[0][3].as_deref(), Some("0001ff"));
    assert_eq!(row_count(&offline), 5);
    assert_oids(&offline, "amounts", &[20, 20, 1700, 25]);
    let offline_bulk = read_back(&fixture, &root, "bulk").await;
    assert_eq!(count_table(&offline_bulk, "batch_c"), 90000);
    assert_eq!(find_row(&offline_bulk, "batch_a", "2")[1].as_deref(), Some("1234567890123456789012345678901234.1234"));
    assert_oids(&offline_bulk, "batch_b", &[20, 1083, 17]);

    let again = copy(CopyRequest {
        plan_path: root.join("gold.plan.json"),
        source_password: absent.clone(),
        source_ca: fixture.ca.clone(),
        admin_password: absent.clone(),
        runtime_password: absent,
        manifest_path: root.join("gold.manifest.json"),
        target_config_path: root.join("gold.target.json"),
        target_ca: fixture.ca.clone(),
        stop: CopyStop::Finish,
    })
    .await
    .unwrap_or_else(|err| panic!("replay {} {}", err.code, err.fact));
    assert_eq!(again.disposition, "replay_complete");
    assert_eq!(again.manifest.rows, 5);
    assert!(!again.manifest.customer_complete);
    assert_eq!(row_count(&read_back(&fixture, &root, "gold").await), 5);
}

fn assert_public(manifest: &Manifest) {
    assert!(!manifest.customer_complete);
    assert_eq!(manifest.enterprise, "not_configured");
    assert!(!manifest.source_count_star);
    assert!(!manifest.source_explain);
    assert!(!manifest.source_ddl);
}

fn assert_names(manifest: &Manifest, name: &str, names: &[&str]) {
    let got: Vec<&str> = manifest
        .tables_completed
        .iter()
        .find(|table| table.name == name)
        .unwrap_or_else(|| panic!("manifest table"))
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(got, names);
}

fn assert_oids(read: &LocalRead, name: &str, oids: &[i64]) {
    let table = read.tables.iter().find(|table| table.name == name).unwrap_or_else(|| panic!("local table"));
    assert_eq!(table.catalog.first().map(|column| column.name.as_str()), Some("_oo_pos"));
    assert_eq!(table.catalog.first().map(|column| column.oid), Some(1700));
    let got: Vec<i64> = table.catalog.iter().skip(1).map(|column| column.oid).collect();
    assert_eq!(got, oids);
}

fn assert_target_config(fixture: &Fixture, root: &Path, name: &str) {
    let text = std::fs::read_to_string(root.join(format!("{name}.target.json"))).unwrap_or_else(|_| panic!("target file"));
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("target json"));
    let mut keys: Vec<String> = value.as_object().unwrap_or_else(|| panic!("target object")).keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["ca_sha256", "database", "host", "port", "protocol", "server_name", "tls", "user"]
    );
    assert!(value.get("password").is_none());
    assert!(value.get("source").is_none());
    assert_eq!(value.get("host").and_then(Value::as_str), Some(fixture.target_host.as_str()));
    assert_eq!(value.get("server_name").and_then(Value::as_str), Some(fixture.target_server_name.as_str()));
    assert_eq!(value.get("tls").and_then(Value::as_str), Some("verify-full"));
}

#[allow(clippy::too_many_arguments)]
async fn run_copy(
    fixture: &Fixture,
    root: &Path,
    name: &str,
    source_id: &str,
    shard: &str,
    owner: &str,
    tables: &[(&str, &str)],
    deadline: u64,
    max_rows: u64,
    stop: CopyStop,
) -> local_pg_mirror::CopyOutcome {
    let config = root.join(format!("{name}.json"));
    std::fs::write(&config, document(fixture, source_id, shard, owner, tables, deadline, max_rows)).unwrap_or_else(|_| panic!("config write"));
    let plan = discover(DiscoverRequest {
        config_path: config,
        plan_path: root.join(format!("{name}.plan.json")),
        source_password: fixture.source_password.clone(),
        source_ca: fixture.ca.clone(),
    })
    .await
    .unwrap_or_else(|err| panic!("discover {} {}", err.code, err.fact));
    assert_eq!(plan.plan_sha256.len(), 64);
    copy(copy_request(fixture, root, name, name, stop))
        .await
        .unwrap_or_else(|err| panic!("copy {} {}", err.code, err.fact))
}

fn copy_request(fixture: &Fixture, root: &Path, plan: &str, manifest: &str, stop: CopyStop) -> CopyRequest {
    CopyRequest {
        plan_path: root.join(format!("{plan}.plan.json")),
        source_password: fixture.source_password.clone(),
        source_ca: fixture.ca.clone(),
        admin_password: fixture.target_admin_password.clone(),
        runtime_password: fixture.runtime_password.clone(),
        manifest_path: root.join(format!("{manifest}.manifest.json")),
        target_config_path: root.join(format!("{manifest}.target.json")),
        target_ca: fixture.ca.clone(),
        stop,
    }
}

async fn read_back(fixture: &Fixture, root: &Path, name: &str) -> LocalRead {
    read_local(ReadRequest {
        target_config: root.join(format!("{name}.target.json")),
        manifest: root.join(format!("{name}.manifest.json")),
        reader_password: fixture.reader_password.clone(),
        ca: fixture.ca.clone(),
    })
    .await
    .unwrap_or_else(|err| panic!("read {} {}", err.code, err.fact))
}

fn table(read: &LocalRead, name: &str) -> Vec<Vec<Option<String>>> {
    let mut rows = read.tables.iter().find(|table| table.name == name).unwrap_or_else(|| panic!("table")).rows.clone();
    rows.sort();
    rows
}

fn find_row(read: &LocalRead, name: &str, id: &str) -> Vec<Option<String>> {
    read.tables
        .iter()
        .find(|table| table.name == name)
        .unwrap_or_else(|| panic!("table"))
        .rows
        .iter()
        .find(|row| row.first().and_then(Option::as_deref) == Some(id))
        .unwrap_or_else(|| panic!("row"))
        .clone()
}

fn count_table(read: &LocalRead, name: &str) -> usize {
    read.tables.iter().find(|table| table.name == name).unwrap_or_else(|| panic!("table")).rows.len()
}

fn row_count(read: &LocalRead) -> usize {
    read.tables.iter().map(|table| table.rows.len()).sum()
}

fn document(
    fixture: &Fixture,
    source_id: &str,
    shard: &str,
    owner: &str,
    tables: &[(&str, &str)],
    deadline: u64,
    max_rows: u64,
) -> String {
    let mut listed = String::from("[");
    for (index, (schema, table)) in tables.iter().enumerate() {
        if index > 0 {
            listed.push(',');
        }
        listed.push_str(&format!(r#"{{"schema":"{schema}","table":"{table}"}}"#));
    }
    listed.push(']');
    format!(
        r#"{{"protocol":"oo.pipeline.local-pg-mirror/v1","engine":"postgresql","source_id":"{source_id}","host":"{host}","port":{port},"database":"{database}","user":"{user}","server_name":"{server}","ca_sha256":"{ca}","secret_ref":"secret://fixture/src","tables":{listed},"connect_timeout_ms":10000,"statement_timeout_ms":30000,"lock_timeout_ms":1000,"max_rows":{max_rows},"max_bytes":1048576,"batch_rows":1000,"snapshot_deadline_ms":{deadline},"generation":1,"shard":"{shard}","owner":"{owner}","target_host":"{target_host}","target_port":{target_port},"target_database":"{target_database}","target_admin_user":"{admin}","target_runtime_user":"{runtime}","target_reader_user":"{reader}","target_server_name":"{target_server}","target_ca_sha256":"{ca}"}}"#,
        host = fixture.source_host,
        port = fixture.source_port,
        database = fixture.source_database,
        user = fixture.source_user,
        server = fixture.source_server_name,
        ca = fixture.ca_sha256,
        target_host = fixture.target_host,
        target_port = fixture.target_port,
        target_database = fixture.target_database,
        admin = fixture.admin_user,
        runtime = fixture.runtime_user,
        reader = fixture.reader_user,
        target_server = fixture.target_server_name,
    )
}

fn load_fixture() -> Fixture {
    let path = std::env::var("OO_MIRROR_FIXTURE").unwrap_or_else(|_| panic!("fixture env"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("fixture file"));
    let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("fixture json"));
    let ca = PathBuf::from(field(&value, "ca"));
    let ca_bytes = std::fs::read(&ca).unwrap_or_else(|_| panic!("ca file"));
    Fixture {
        source_host: field(&value, "source_host"),
        source_port: port_field(&value, "source_port"),
        source_server_name: field(&value, "source_server_name"),
        target_host: field(&value, "target_host"),
        target_port: port_field(&value, "target_port"),
        target_server_name: field(&value, "target_server_name"),
        source_database: field(&value, "source_database"),
        target_database: field(&value, "target_database"),
        admin_user: field(&value, "admin_user"),
        source_user: field(&value, "source_user"),
        runtime_user: field(&value, "runtime_user"),
        reader_user: field(&value, "reader_user"),
        source_admin_password: PathBuf::from(field(&value, "source_admin_password")),
        target_admin_password: PathBuf::from(field(&value, "target_admin_password")),
        source_password: PathBuf::from(field(&value, "source_password")),
        runtime_password: PathBuf::from(field(&value, "runtime_password")),
        reader_password: PathBuf::from(field(&value, "reader_password")),
        ca_sha256: oo_pipeline::target_installer::sha256_hex(&ca_bytes),
        ca,
    }
}

fn field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_else(|| panic!("fixture field")).to_string()
}

fn port_field(value: &Value, key: &str) -> u16 {
    u16::try_from(value.get(key).and_then(Value::as_u64).unwrap_or_else(|| panic!("fixture port"))).unwrap_or_else(|_| panic!("fixture port"))
}

fn secret(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| panic!("secret file"));
    let text = text.strip_suffix('\n').unwrap_or(&text);
    if text.len() != 48 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        panic!("secret shape");
    }
    text.to_string()
}

async fn dial(host: &str, port: u16, user: &str, password: &str, database: &str, ca: &[u8]) -> PgConnection {
    dial_result(host, port, user, password, database, ca).await.unwrap_or_else(|err| panic!("dial {}", fail_class(&err)))
}

async fn dial_result(host: &str, port: u16, user: &str, password: &str, database: &str, ca: &[u8]) -> Result<PgConnection, sqlx::Error> {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(user)
        .password(password)
        .database(database)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    if options.get_options().is_some() {
        panic!("connect options");
    }
    let options = options.disable_statement_logging();
    let mut conn = tokio::time::timeout(Duration::from_secs(30), options.connect())
        .await
        .unwrap_or_else(|_| panic!("dial timeout"))?;
    sqlx::query("SET statement_timeout = '120s'").execute(&mut conn).await?;
    sqlx::query("SET lock_timeout = '1s'").execute(&mut conn).await?;
    Ok(conn)
}

async fn raw(conn: &mut PgConnection, sql: &str) {
    if let Err(err) = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string())).execute(&mut *conn).await {
        panic!("sql {}", fail_class(&err));
    }
}

async fn source_writes(fixture: &Fixture, password: &str, ca: &[u8]) -> (i64, i64, i64) {
    let mut conn = dial(&fixture.source_host, fixture.source_port, &fixture.admin_user, password, "postgres", ca).await;
    let row = sqlx::query(
        "SELECT tup_inserted::int8, tup_updated::int8, tup_deleted::int8 FROM pg_catalog.pg_stat_database WHERE datname = $1",
    )
    .bind(&fixture.source_database)
    .fetch_one(&mut conn)
    .await
    .unwrap_or_else(|err| panic!("writes {}", fail_class(&err)));
    (
        row.try_get(0).unwrap_or_else(|_| panic!("writes")),
        row.try_get(1).unwrap_or_else(|_| panic!("writes")),
        row.try_get(2).unwrap_or_else(|_| panic!("writes")),
    )
}

async fn source_backends(fixture: &Fixture, password: &str, ca: &[u8]) -> i64 {
    let mut conn = dial(&fixture.source_host, fixture.source_port, &fixture.admin_user, password, "postgres", ca).await;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*)::int8 FROM pg_catalog.pg_stat_activity WHERE datname = $1 AND usename = $2 AND application_name = 'oo-pg-source'",
    )
    .bind(&fixture.source_database)
    .bind(&fixture.source_user)
    .fetch_one(&mut conn)
    .await
    .unwrap_or_else(|err| panic!("backends {}", fail_class(&err)));
    count
}

fn watch_source(fixture: &Fixture, password: &str, ca: &[u8]) -> (Arc<AtomicBool>, Arc<AtomicU64>, tokio::task::JoinHandle<i64>) {
    let stop = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(AtomicU64::new(0));
    let stop_task = Arc::clone(&stop);
    let seen_task = Arc::clone(&seen);
    let host = fixture.source_host.clone();
    let port = fixture.source_port;
    let user = fixture.admin_user.clone();
    let source_user = fixture.source_user.clone();
    let database = fixture.source_database.clone();
    let password = password.to_string();
    let ca = ca.to_vec();
    let handle = tokio::spawn(async move {
        let mut conn = dial(&host, port, &user, &password, "postgres", &ca).await;
        loop {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*)::int8 FROM pg_catalog.pg_stat_activity WHERE datname = $1 AND usename = $2 AND application_name = 'oo-pg-source'",
            )
            .bind(&database)
            .bind(&source_user)
            .fetch_one(&mut conn)
            .await
            .unwrap_or(0);
            let count = u64::try_from(count).unwrap_or(u64::MAX);
            seen_task.fetch_max(count, Ordering::Relaxed);
            if stop_task.load(Ordering::Relaxed) {
                return i64::try_from(count).unwrap_or(i64::MAX);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    (stop, seen, handle)
}

fn fail_class(err: &sqlx::Error) -> String {
    if let Some(state) = err.as_database_error().and_then(|db| db.code()) {
        let state = state.into_owned();
        if state.len() == 5 && state.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return state;
        }
    }
    match err {
        sqlx::Error::Tls(_) => "tls".to_string(),
        sqlx::Error::Io(_) => "io".to_string(),
        sqlx::Error::Protocol(_) => "protocol".to_string(),
        _ => "driver".to_string(),
    }
}
