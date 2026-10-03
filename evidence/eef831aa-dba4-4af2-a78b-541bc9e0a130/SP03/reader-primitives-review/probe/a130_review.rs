//! Independent composition proof for the fixed ccd93e1a archive.
//! Copied onto that archive as crates/oo-connectors/tests/a130_review.rs.
//! It does not change product source. Run with --test-threads=1.

use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, Instant};

use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    CutProbe, PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig,
    PgTypePolicy, ProbeValue, RunBinding, SecretRef, SelectedTable, SnapshotOwner,
    SnapshotRequest, SourceError, SourceErrorKind, SourceId, TableSelection, TypedBatch,
    CLEANUP_OBSERVATION_BOUND_MS, PG17_RESERVED,
};
use serde::Deserialize;
use sqlx::postgres::{
    PgArguments, PgConnectOptions, PgConnection, PgPortalAllowance, PgPortalOutcome, PgSslMode,
    EXECUTE_LIMIT,
};
use sqlx::{Arguments, Column, ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    writer_user: String,
    super_user: String,
    password_dir: String,
    ca_file: String,
    log_file: String,
}

struct Backend {
    pid: i32,
    start: String,
    user: String,
    app: String,
    db: String,
}

fn pointer() -> Pointer {
    let path = std::env::var("OO_A130_FIXTURE").expect("OO_A130_FIXTURE");
    serde_json::from_str(&fs::read_to_string(path).expect("pointer")).expect("pointer json")
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    Zeroizing::new(
        fs::read_to_string(format!("{dir}/{name}"))
            .expect(name)
            .trim()
            .to_string(),
    )
}

async fn dial(
    pointer: &Pointer,
    user: &str,
    password: &str,
    ca: &[u8],
    app: &str,
) -> PgConnection {
    let options = PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(user)
        .password(password)
        .database(&pointer.database)
        .application_name(app)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    options
        .disable_statement_logging()
        .connect()
        .await
        .expect(app)
}

fn config(
    pointer: &Pointer,
    password: &str,
    ca: &[u8],
    source: &str,
    max_rows: u32,
    statement_timeout: Duration,
    tables: Vec<SelectedTable>,
) -> PgSourceConfig {
    PgSourceConfig::try_new(
        SourceId::new(source).unwrap(),
        PgEndpoint::new(&pointer.host, pointer.port, &pointer.database).unwrap(),
        PgCredentials::new(&pointer.reader_user, password).unwrap(),
        SecretRef::new("secret://pg/snap/reader").unwrap(),
        PgTlsConfig::from_ca_pem(ca).unwrap(),
        PgBudget::new(
            Duration::from_secs(30),
            statement_timeout,
            Duration::from_secs(30),
            max_rows,
            4 * 1024 * 1024,
        )
        .unwrap(),
        TableSelection::new(tables).unwrap(),
    )
    .unwrap()
}

fn tables(pairs: &[(&str, &str)]) -> Vec<SelectedTable> {
    pairs
        .iter()
        .map(|(schema, table)| SelectedTable::new(*schema, *table).unwrap())
        .collect()
}

async fn node_of(admin: &mut PgConnection) -> (String, bool) {
    let row = sqlx::query(
        "SELECT pg_catalog.pg_postmaster_start_time()::text, pg_catalog.pg_is_in_recovery()",
    )
    .fetch_one(&mut *admin)
    .await
    .expect("node");
    (row.try_get(0).expect("postmaster"), row.try_get(1).expect("recovery"))
}

async fn load_admitted(
    admin: &mut PgConnection,
    schema: &str,
    names: &[&str],
) -> AdmittedCatalog {
    let mut tables = Vec::new();
    for name in names {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
        .bind(schema)
        .bind(*name)
        .fetch_all(&mut *admin)
        .await
        .expect("admitted columns");
        let mut columns = Vec::new();
        for row in &rows {
            columns.push(AdmittedColumn {
                attnum: row.try_get(0).unwrap(),
                name: row.try_get(1).unwrap(),
                type_oid: row.try_get(2).unwrap(),
                typmod: row.try_get(3).unwrap(),
                not_null: row.try_get(4).unwrap(),
            });
        }
        let keys = sqlx::query(
            "SELECT a.attname::text \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
             JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
             WHERE n.nspname = $1 AND c.relname = $2 AND i.indisprimary \
             ORDER BY u.ord",
        )
        .bind(schema)
        .bind(*name)
        .fetch_all(&mut *admin)
        .await
        .expect("admitted keys");
        tables.push(AdmittedTable {
            schema: schema.to_string(),
            name: (*name).to_string(),
            columns,
            primary_key: keys
                .iter()
                .map(|row| row.try_get::<String, _>(0).unwrap())
                .collect(),
        });
    }
    AdmittedCatalog { tables }
}

async fn open_owner(
    pointer: &Pointer,
    password: &str,
    ca: &[u8],
    source: &str,
    admitted: AdmittedCatalog,
    generation: u64,
    postmaster: &str,
    in_recovery: bool,
    selection: Vec<SelectedTable>,
) -> Result<SnapshotOwner, SourceError> {
    let config = config(
        pointer,
        password,
        ca,
        source,
        1_000,
        Duration::from_secs(30),
        selection,
    );
    let policy = PgTypePolicy::PRESERVE_ROW;
    SnapshotOwner::open(SnapshotRequest {
        config_identity: config_identity(&config),
        type_policy_identity: type_policy_identity(policy),
        config,
        admitted,
        policy,
        generation,
        deadline: Instant::now() + Duration::from_secs(50),
        expected_principal: pointer.reader_user.clone(),
        expected_database: pointer.database.clone(),
        expected_in_recovery: in_recovery,
        expected_postmaster_start: postmaster.to_string(),
    })
    .await
}

async fn source_backends(admin: &mut PgConnection, pointer: &Pointer) -> Vec<Backend> {
    let rows = sqlx::query(
        "SELECT pid, backend_start::text, usename::text, application_name, datname \
         FROM pg_catalog.pg_stat_activity \
         WHERE application_name = 'oo-pg-source' AND usename::text = $1 AND datname = $2",
    )
    .bind(&pointer.reader_user)
    .bind(&pointer.database)
    .fetch_all(&mut *admin)
    .await
    .expect("activity");
    rows.into_iter()
        .map(|row| Backend {
            pid: row.try_get(0).unwrap(),
            start: row.try_get(1).unwrap(),
            user: row.try_get(2).unwrap(),
            app: row.try_get(3).unwrap(),
            db: row.try_get(4).unwrap(),
        })
        .collect()
}

async fn wait_clear(admin: &mut PgConnection, pointer: &Pointer) {
    let began = Instant::now();
    loop {
        if source_backends(admin, pointer).await.is_empty() {
            return;
        }
        if began.elapsed() > Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS) {
            panic!("oo-pg-source backends still open");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_gone(admin: &mut PgConnection, pid: i32, start: &str) {
    let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
    let began = Instant::now();
    loop {
        let still = sqlx::query(
            "SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid = $1 AND backend_start::text = $2",
        )
        .bind(pid)
        .bind(start)
        .fetch_optional(&mut *admin)
        .await
        .expect("wait")
        .is_some();
        if !still {
            return;
        }
        if began.elapsed() > bound {
            panic!("backend {pid} still present after {}ms", began.elapsed().as_millis());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn utf8_at(batch: &TypedBatch, name: &str) -> String {
    let column = batch
        .arrow
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("missing {name}"));
    match column.values.as_slice() {
        [oo_connectors::types::ArrowScalar::Utf8(text)] => text.clone(),
        other => panic!("{name} {other:?}"),
    }
}

fn int32_at(batch: &TypedBatch, name: &str) -> i32 {
    let column = batch
        .arrow
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("missing {name}"));
    match column.values.as_slice() {
        [oo_connectors::types::ArrowScalar::Int32(value)] => *value,
        other => panic!("{name} {other:?}"),
    }
}

fn key(schema: &str, table: &str, id: i64) -> CutProbe {
    CutProbe {
        schema: schema.to_string(),
        table: table.to_string(),
        key: vec![ProbeValue::Int8(id)],
    }
}

async fn exec(admin: &mut PgConnection, sql: &'static str) {
    sqlx::query(sql).execute(&mut *admin).await.expect(sql);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t1_schema_guard() {
    let pointer = pointer();
    let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
    let writer_pass = read_pass(&pointer.password_dir, "writer.pass");
    let super_pass = read_pass(&pointer.password_dir, "super.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let mut admin = dial(&pointer, &pointer.super_user, &super_pass, &ca, "oo-a130-admin").await;
    let rotated: bool = sqlx::query("SELECT pg_catalog.pg_rotate_logfile()")
        .fetch_one(&mut admin)
        .await
        .expect("rotate")
        .try_get(0)
        .expect("rotate flag");
    assert!(rotated, "pg_rotate_logfile refused");
    // Static log_filename keeps the same file, so later classification starts after this marker.
    sqlx::query("SELECT 'a130_own_log_cut'")
        .fetch_one(&mut admin)
        .await
        .expect("own log marker");
    for statement in [
        "DROP SCHEMA IF EXISTS rv CASCADE",
        "CREATE SCHEMA rv",
        "CREATE TABLE rv.parent (id integer PRIMARY KEY, label text NOT NULL)",
        "CREATE TABLE rv.child (id integer PRIMARY KEY, parent_id integer NOT NULL REFERENCES rv.parent (id), qty integer NOT NULL)",
        "CREATE INDEX child_parent_idx ON rv.child (parent_id)",
        "CREATE TABLE rv.drift (id integer PRIMARY KEY, label text NOT NULL)",
        "CREATE TABLE rv.quiet (id integer PRIMARY KEY, label text NOT NULL)",
        "CREATE TABLE rv.page (id integer PRIMARY KEY, note text)",
        "CREATE TABLE rv.lockme (id integer PRIMARY KEY)",
        "INSERT INTO rv.parent (id, label) VALUES (1, 'oak')",
        "INSERT INTO rv.child (id, parent_id, qty) VALUES (10, 1, 2)",
        "INSERT INTO rv.drift (id, label) VALUES (1, 'pebble')",
        "INSERT INTO rv.quiet (id, label) VALUES (1, 'still')",
        "INSERT INTO rv.page (id, note) VALUES (1, 'pine'), (2, 'fir')",
        "INSERT INTO rv.lockme (id) VALUES (1)",
        "GRANT USAGE ON SCHEMA rv TO oo_snap_reader, oo_snap_writer, oo_a127_reader, oo_a127_writer",
        "GRANT SELECT ON rv.parent, rv.child, rv.drift, rv.quiet, rv.lockme TO oo_snap_reader",
        "GRANT SELECT, INSERT, UPDATE, DELETE ON rv.parent, rv.child TO oo_snap_writer",
        "GRANT SELECT ON rv.page TO oo_a127_reader",
        "GRANT SELECT, INSERT, UPDATE ON rv.page TO oo_a127_writer",
    ] {
        exec(&mut admin, statement).await;
    }
    let (postmaster, in_recovery) = node_of(&mut admin).await;
    let parent_child = load_admitted(&mut admin, "rv", &["parent", "child"]).await;
    let child_only = load_admitted(&mut admin, "rv", &["child"]).await;
    let drift = load_admitted(&mut admin, "rv", &["drift"]).await;
    let quiet = load_admitted(&mut admin, "rv", &["quiet"]).await;
    let parent_only = load_admitted(&mut admin, "rv", &["parent"]).await;
    let both = tables(&[("rv", "parent"), ("rv", "child")]);

    let mut owner = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_cut",
        parent_child.clone(),
        1,
        &postmaster,
        in_recovery,
        both.clone(),
    )
    .await
    .expect("cut open");
    let mut oracle = dial(&pointer, &pointer.reader_user, &reader_pass, &ca, "oo-a130-oracle").await;
    sqlx::query("START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut oracle)
        .await
        .expect("oracle begin");
    // REPEATABLE READ's snapshot starts at the first statement, before the writer commits.
    let _: i32 = sqlx::query("SELECT 1")
        .fetch_one(&mut oracle)
        .await
        .expect("oracle snapshot")
        .try_get(0)
        .expect("one");
    let parent = owner.read_probe(key("rv", "parent", 1)).await.expect("parent probe");
    assert_eq!(utf8_at(&parent, "label"), "oak");
    assert_eq!(parent.rows.len(), 1);
    let frame = parent.record_batch().expect("record_batch");
    assert_eq!(frame.num_rows(), 1);
    assert!(frame.column_by_name("id").is_some());
    assert!(frame.column_by_name("label").is_some());
    let child = owner.read_probe(key("rv", "child", 10)).await.expect("child probe");
    assert_eq!(int32_at(&child, "qty"), 2);
    let absent = owner.read_probe(key("rv", "child", 12)).await.expect("absent");
    assert!(absent.rows.is_empty());
    let mut writer = dial(&pointer, &pointer.writer_user, &writer_pass, &ca, "oo-a130-writer").await;
    sqlx::query("UPDATE rv.parent SET label = 'spruce' WHERE id = 1")
        .execute(&mut writer)
        .await
        .expect("writer update");
    sqlx::query("INSERT INTO rv.child (id, parent_id, qty) VALUES (12, 1, 7)")
        .execute(&mut writer)
        .await
        .expect("writer insert");
    sqlx::query("COMMIT").execute(&mut writer).await.ok();
    let _ = writer.close().await;
    let again = owner.read_probe(key("rv", "parent", 1)).await.expect("same cut");
    assert_eq!(utf8_at(&again, "label"), "oak");
    assert_eq!(parent.rows[0].canonical_hash, again.rows[0].canonical_hash);
    assert_eq!(int32_at(&owner.read_probe(key("rv", "child", 10)).await.expect("qty"), "qty"), 2);
    assert!(owner.read_probe(key("rv", "child", 12)).await.expect("hidden").rows.is_empty());
    let oracle_label: String = sqlx::query("SELECT label FROM rv.parent WHERE id = 1")
        .fetch_one(&mut oracle)
        .await
        .expect("oracle parent")
        .try_get(0)
        .expect("label");
    assert_eq!(oracle_label, "oak");
    let oracle_new = sqlx::query("SELECT qty FROM rv.child WHERE id = 12")
        .fetch_optional(&mut oracle)
        .await
        .expect("oracle child");
    assert!(oracle_new.is_none());
    sqlx::query("ROLLBACK").execute(&mut oracle).await.expect("oracle rollback");
    let _ = oracle.close().await;
    let old_hash = parent.rows[0].canonical_hash;
    owner.finish().await.expect("cut finish");
    let mut fresh = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_cut",
        parent_child.clone(),
        2,
        &postmaster,
        in_recovery,
        both.clone(),
    )
    .await
    .expect("fresh open");
    let fresh_row = fresh.read_probe(key("rv", "parent", 1)).await.expect("fresh parent");
    assert_eq!(utf8_at(&fresh_row, "label"), "spruce");
    assert_ne!(fresh_row.rows[0].canonical_hash, old_hash);
    let inserted = fresh.read_probe(key("rv", "child", 12)).await.expect("fresh insert");
    assert_eq!(int32_at(&inserted, "qty"), 7);
    fresh.finish().await.expect("fresh finish");
    println!("same_cut=ok");

    let mut renamed = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_idx",
        child_only,
        1,
        &postmaster,
        in_recovery,
        tables(&[("rv", "child")]),
    )
    .await
    .expect("index open");
    exec(&mut admin, "ALTER INDEX rv.child_parent_idx RENAME TO child_parent_rv").await;
    let still = renamed.read_probe(key("rv", "child", 10)).await.expect("index probe");
    assert_eq!(int32_at(&still, "qty"), 2);
    renamed.finish().await.expect("index rename finish");
    let index_name: String = sqlx::query(
        "SELECT c.relname FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'rv' AND c.relname = 'child_parent_rv'",
    )
    .fetch_one(&mut admin)
    .await
    .expect("renamed index")
    .try_get(0)
    .expect("name");
    assert_eq!(index_name, "child_parent_rv");
    println!("index_rename=ok");

    let mut drift_owner = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_drift",
        drift,
        1,
        &postmaster,
        in_recovery,
        tables(&[("rv", "drift")]),
    )
    .await
    .expect("drift open");
    let kept = drift_owner.read_probe(key("rv", "drift", 1)).await.expect("drift probe");
    assert_eq!(utf8_at(&kept, "label"), "pebble");
    exec(&mut admin, "ALTER TABLE rv.drift ADD COLUMN extra text").await;
    let drifted = drift_owner.read_probe(key("rv", "drift", 1)).await.expect_err("add after probe");
    assert_eq!(drifted.fact(), Some("schema_drift"), "{drifted}");
    let finished = drift_owner.finish().await.expect_err("drift finish");
    assert_eq!(finished.fact(), Some("schema_drift"), "{finished}");
    assert_eq!(utf8_at(&kept, "label"), "pebble");
    println!("add_after_probe=schema_drift");

    let quiet_owner = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_quiet",
        quiet,
        1,
        &postmaster,
        in_recovery,
        tables(&[("rv", "quiet")]),
    )
    .await
    .expect("quiet open");
    exec(&mut admin, "ALTER TABLE rv.quiet ADD COLUMN extra text").await;
    let quiet_finish = quiet_owner.finish().await.expect_err("zero probe finish");
    assert_eq!(quiet_finish.fact(), Some("schema_drift"), "{quiet_finish}");
    println!("zero_probe_finish=schema_drift");
    wait_clear(&mut admin, &pointer).await;

    let first = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_cap",
        parent_child.clone(),
        1,
        &postmaster,
        in_recovery,
        both.clone(),
    )
    .await
    .expect("cap gen 1");
    let second = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_cap",
        parent_child.clone(),
        2,
        &postmaster,
        in_recovery,
        both.clone(),
    )
    .await
    .expect("cap gen 2");
    let held = source_backends(&mut admin, &pointer).await;
    assert_eq!(held.len(), 4, "two owners hold four backends");
    for row in &held {
        assert_eq!(row.user, pointer.reader_user);
        assert_eq!(row.app, "oo-pg-source");
        assert_eq!(row.db, pointer.database);
    }
    let refused = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_cap",
        parent_child.clone(),
        3,
        &postmaster,
        in_recovery,
        both.clone(),
    )
    .await
    .expect_err("third owner");
    assert_eq!(refused.kind(), SourceErrorKind::Budget);
    assert_eq!(refused.fact(), Some("max_connections"), "{refused}");
    assert_eq!(source_backends(&mut admin, &pointer).await.len(), 4);
    let first_pid = i32::try_from(first.binding().backend.pid).unwrap();
    let first_start = first.binding().backend.backend_start.clone();
    first.finish().await.expect("release one owner");
    wait_gone(&mut admin, first_pid, &first_start).await;
    assert_eq!(source_backends(&mut admin, &pointer).await.len(), 2);
    let third = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_cap",
        parent_child,
        3,
        &postmaster,
        in_recovery,
        both,
    )
    .await
    .expect("generation 3 was not consumed by the refusal");
    third.finish().await.expect("gen 3 finish");
    second.finish().await.expect("gen 2 finish");
    println!("cap_refusal=max_connections generation_retained=3");

    cleanup_pair(
        &pointer,
        &reader_pass,
        &ca,
        &postmaster,
        in_recovery,
        &parent_only,
        &mut admin,
        "rv_finish",
        Cleanup::Finish,
    )
    .await;
    cleanup_pair(
        &pointer,
        &reader_pass,
        &ca,
        &postmaster,
        in_recovery,
        &parent_only,
        &mut admin,
        "rv_abort",
        Cleanup::Abort,
    )
    .await;
    cleanup_pair(
        &pointer,
        &reader_pass,
        &ca,
        &postmaster,
        in_recovery,
        &parent_only,
        &mut admin,
        "rv_drop",
        Cleanup::Drop,
    )
    .await;

    let kill = open_owner(
        &pointer,
        &reader_pass,
        &ca,
        "rv_kill",
        parent_only,
        1,
        &postmaster,
        in_recovery,
        tables(&[("rv", "parent")]),
    )
    .await
    .expect("kill open");
    let pair = source_backends(&mut admin, &pointer).await;
    assert_eq!(pair.len(), 2);
    let data_pid = i32::try_from(kill.binding().backend.pid).unwrap();
    let guard = pair.iter().find(|row| row.pid != data_pid).expect("guard pid");
    let terminated: bool = sqlx::query("SELECT pg_catalog.pg_terminate_backend($1)")
        .bind(guard.pid)
        .fetch_one(&mut admin)
        .await
        .expect("terminate")
        .try_get(0)
        .expect("flag");
    assert!(terminated);
    let err = kill.finish().await.expect_err("terminated guard must not finish");
    println!("guard_terminate_fact={}", err.fact().unwrap_or("none"));
    assert_ne!(err.fact(), Some("snapshot_finished"));
    println!("t1_schema_guard=ok");
}

enum Cleanup {
    Finish,
    Abort,
    Drop,
}

async fn cleanup_pair(
    pointer: &Pointer,
    password: &str,
    ca: &[u8],
    postmaster: &str,
    in_recovery: bool,
    admitted: &AdmittedCatalog,
    admin: &mut PgConnection,
    source: &str,
    how: Cleanup,
) {
    wait_clear(admin, pointer).await;
    let owner = open_owner(
        pointer,
        password,
        ca,
        source,
        admitted.clone(),
        1,
        postmaster,
        in_recovery,
        tables(&[("rv", "parent")]),
    )
    .await
    .expect(source);
    let pair = source_backends(admin, pointer).await;
    assert_eq!(pair.len(), 2, "{source} backend count");
    let data_pid = i32::try_from(owner.binding().backend.pid).unwrap();
    assert!(pair.iter().any(|row| row.pid == data_pid));
    for row in &pair {
        assert_eq!(row.user, pointer.reader_user);
        assert_eq!(row.app, "oo-pg-source");
        assert_eq!(row.db, pointer.database);
    }
    let saved: Vec<(i32, String)> = pair.into_iter().map(|row| (row.pid, row.start)).collect();
    match how {
        Cleanup::Finish => {
            owner.finish().await.expect("finish");
        }
        Cleanup::Abort => {
            owner.abort().await.expect("abort");
        }
        Cleanup::Drop => drop(owner),
    }
    for (pid, start) in saved {
        wait_gone(admin, pid, &start).await;
    }
    println!("cleanup_{source}=both_backends_gone");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t2_catalog_rate() {
    let pointer = pointer();
    let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
    let super_pass = read_pass(&pointer.password_dir, "super.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let mut admin = dial(&pointer, &pointer.super_user, &super_pass, &ca, "oo-a130-admin").await;
    let selection = tables(&[("rv", "parent"), ("rv", "child")]);

    let started = Instant::now();
    let mut small = PgConnector::new(config(
        &pointer,
        &reader_pass,
        &ca,
        "rv_small",
        1_000,
        Duration::from_secs(30),
        selection.clone(),
    ))
    .connect()
    .await
    .expect("small connect");
    for _ in 0..3 {
        let catalog = small.catalog().await.expect("max_rows 1000");
        assert_eq!(catalog.tables.len(), 2);
    }
    let small_elapsed = started.elapsed();
    assert!(small_elapsed < Duration::from_secs(1), "small catalogs {small_elapsed:?}");
    drop(small);
    println!("small_elapsed={small_elapsed:?}");

    sqlx::query("SELECT 'a130_rate_limits'")
        .execute(&mut admin)
        .await
        .expect("rate marker");
    let started = Instant::now();
    let mut big = PgConnector::new(config(
        &pointer,
        &reader_pass,
        &ca,
        "rv_big",
        50_000,
        Duration::from_secs(30),
        selection,
    ))
    .connect()
    .await
    .expect("big connect");
    for index in 0..4 {
        let catalog = big.catalog().await.expect("max_rows 50000");
        assert_eq!(catalog.tables.len(), 2, "catalog {index}");
    }
    let big_elapsed = started.elapsed();
    assert!(
        big_elapsed < Duration::from_secs(1),
        "four catalogs took {big_elapsed:?}; unused allowance was not refunded inside the window"
    );
    drop(big);
    println!("big_elapsed={big_elapsed:?}");

    let limits = catalog_limits(&fs::read_to_string(&pointer.log_file).expect("log"), "a130_rate_limits");
    let keyword_limit = i64::try_from(PG17_RESERVED.len()).unwrap() + 1;
    let expect = [
        ("keywords", keyword_limit),
        ("tables", 2),
        ("columns", 3_200),
        ("enum", 4_096),
        ("domain", 160),
        ("primary_keys", 64),
        ("unique_keys", 4_096),
        ("foreign_keys", 4_096),
        ("policies", 256),
        ("acl", 512),
    ];
    for (name, want) in expect {
        assert_eq!(limits.get(name).copied(), Some(want), "limit {name} got {limits:?}");
    }
    println!("sql_limits_match_reservation=ok keyword_limit={keyword_limit}");

    sqlx::query("BEGIN").execute(&mut admin).await.expect("lock begin");
    sqlx::query("LOCK TABLE rv.lockme IN ACCESS EXCLUSIVE MODE")
        .execute(&mut admin)
        .await
        .expect("lock");
    let burn_started = Instant::now();
    let mut deny = PgConnector::new(config(
        &pointer,
        &reader_pass,
        &ca,
        "rv_deny",
        50_000,
        Duration::from_millis(400),
        tables(&[("rv", "lockme")]),
    ))
    .connect()
    .await
    .expect("deny connect");
    let burned = deny
        .read_typed(
            &RunBinding {
                source_id: "rv_deny".to_string(),
                schema: "rv".to_string(),
                table: "lockme".to_string(),
                run_generation: 1,
                input_digest: [9; 32],
            },
            PgTypePolicy::PRESERVE_ROW,
        )
        .await
        .expect_err("locked read");
    assert_eq!(burned.kind(), SourceErrorKind::Budget);
    assert_eq!(burned.fact(), Some("statement_timeout"), "{burned}");
    drop(deny);
    sqlx::query("ROLLBACK").execute(&mut admin).await.expect("unlock");
    sqlx::query("SELECT 'a130_deny_cut'")
        .execute(&mut admin)
        .await
        .expect("deny marker");
    let denied = match PgConnector::new(config(
        &pointer,
        &reader_pass,
        &ca,
        "rv_deny",
        1_000,
        Duration::from_secs(30),
        tables(&[("rv", "lockme")]),
    ))
    .connect()
    .await
    {
        Err(err) => err,
        Ok(mut session) => session.catalog().await.expect_err("exhausted window"),
    };
    let burn_elapsed = burn_started.elapsed();
    assert_eq!(denied.kind(), SourceErrorKind::Budget);
    assert_eq!(denied.fact(), Some("max_rows_per_sec"), "{denied}");
    assert!(burn_elapsed < Duration::from_secs(1), "denial window {burn_elapsed:?}");
    let tail = log_after(&pointer.log_file, "a130_deny_cut");
    assert!(!tail.contains("pg_get_keywords"), "denial reached the keyword query");
    println!("failed_query_keeps_charge=ok burn_elapsed={burn_elapsed:?}");
}

fn catalog_limits(log: &str, marker: &str) -> BTreeMap<&'static str, i64> {
    let start = log.rfind(marker).unwrap_or_else(|| panic!("missing {marker}"));
    let log = &log[start + marker.len()..];
    let mut out = BTreeMap::new();
    let mut keywords_seen = 0usize;
    let mut pending: Option<String> = None;
    for line in log.lines() {
        if line.contains("DETAIL:") && line.contains("Parameters:") {
            if let Some(sql) = pending.take() {
                if let Some((name, value)) = classify_limit(&sql, line) {
                    out.entry(name).or_insert(value);
                }
            }
            continue;
        }
        if line.contains("app=oo-pg-source") && line.contains("user=oo_snap_reader") {
            if let Some(sql) = statement_sql(line) {
                if sql.contains("pg_get_keywords") {
                    keywords_seen += 1;
                    if keywords_seen > 1 {
                        break;
                    }
                }
                pending = Some(sql.to_string());
                continue;
            }
        }
        pending = None;
    }
    out
}

fn classify_limit(sql: &str, detail: &str) -> Option<(&'static str, i64)> {
    let value = last_int_param(detail)?;
    let name = if sql.contains("pg_get_keywords") {
        "keywords"
    } else if sql.contains("unnest($1::text[], $2::text[])") {
        "tables"
    } else if sql.contains("col_description") {
        "columns"
    } else if sql.contains("pg_enum") {
        "enum"
    } else if sql.contains("contype = 'c'") {
        "domain"
    } else if sql.contains("NOT i.indisprimary") {
        "unique_keys"
    } else if sql.contains("indisprimary") {
        "primary_keys"
    } else if sql.contains("contype = 'f'") {
        "foreign_keys"
    } else if sql.contains("pol.polname") {
        "policies"
    } else if sql.contains("aclexplode") && sql.contains("privilege_type") {
        "acl"
    } else {
        return None;
    };
    Some((name, value))
}

fn last_int_param(detail: &str) -> Option<i64> {
    let mut last = None;
    for part in detail.split('$').skip(1) {
        let (_, raw) = part.split_once('=')?;
        let token = raw.trim().trim_matches(|ch: char| ch == '\'' || ch == ',');
        if let Ok(value) = token.parse::<i64>() {
            last = Some(value);
        }
    }
    last
}

fn log_after(path: &str, marker: &str) -> String {
    let raw = fs::read_to_string(path).expect("log");
    let index = raw.rfind(marker).unwrap_or_else(|| panic!("missing {marker}"));
    raw[index + marker.len()..].to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t3_named_portal() {
    assert_eq!(EXECUTE_LIMIT, 1);
    let pointer = pointer();
    let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
    let writer_pass = read_pass(&pointer.password_dir, "writer.pass");
    let super_pass = read_pass(&pointer.password_dir, "super.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let mut reader = dial(&pointer, "oo_a127_reader", &reader_pass, &ca, "oo-a130-reader").await;
    let mut writer = dial(&pointer, "oo_a127_writer", &writer_pass, &ca, "oo-a130-writer").await;
    let mut admin = dial(&pointer, &pointer.super_user, &super_pass, &ca, "oo-a130-admin").await;
    let mut oracle = dial(&pointer, "oo_a127_reader", &reader_pass, &ca, "oo-a130-oracle").await;

    let mut tx = reader
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .expect("begin");
    let snapshot: String = sqlx::query("SELECT pg_export_snapshot()")
        .fetch_one(&mut *tx)
        .await
        .expect("export")
        .try_get(0)
        .expect("snapshot id");
    assert!(snapshot_id_ok(&snapshot), "snapshot id {snapshot}");
    let grant = PgPortalAllowance::positive(1).expect("allowance");
    let mut portal = tx
        .open_named_portal(
            "SELECT id, note FROM rv.page ORDER BY id",
            PgArguments::default(),
        )
        .await
        .expect("portal");
    let first = portal.pull(grant).await.expect("first");
    let sqlx::postgres::PgPortalOutcome::Suspended(row) = &first.outcome else {
        panic!("first row must suspend");
    };
    assert_eq!(first.accounting.execute_limit, 1);
    assert_eq!(first.accounting.reserved_rows, 1);
    assert_eq!(first.accounting.consumed_rows, 1);
    assert_eq!(row.try_get::<i32, _>(0).unwrap(), 1);
    assert_eq!(row.try_get::<Option<String>, _>(1).unwrap().as_deref(), Some("pine"));
    let columns = row.columns();
    assert_eq!(columns[0].name(), "id");
    assert_eq!(columns[1].name(), "note");
    assert_eq!(columns[0].type_info().oid().expect("int4").0, 23);
    assert_eq!(columns[1].type_info().oid().expect("text").0, 25);
    assert!(columns[0].relation_id().is_some());
    assert_eq!(columns[0].relation_id(), columns[1].relation_id());
    assert_eq!(columns[0].relation_attribute_no(), Some(1));
    assert_eq!(columns[1].relation_attribute_no(), Some(2));

    sqlx::query("BEGIN").execute(&mut writer).await.expect("writer begin");
    sqlx::query("UPDATE rv.page SET note = 'spruce' WHERE id = 2")
        .execute(&mut writer)
        .await
        .expect("writer update");
    sqlx::query("INSERT INTO rv.page (id, note) VALUES (3, 'cedar')")
        .execute(&mut writer)
        .await
        .expect("writer insert");
    sqlx::query("COMMIT").execute(&mut writer).await.expect("writer commit");

    let second = portal.pull(grant).await.expect("second");
    let sqlx::postgres::PgPortalOutcome::Suspended(row) = &second.outcome else {
        panic!("last row must suspend until a later pull");
    };
    assert_eq!(second.accounting.execute_limit, 1);
    assert_eq!(second.accounting.consumed_rows, 1);
    assert_eq!(row.try_get::<i32, _>(0).unwrap(), 2);
    assert_eq!(row.try_get::<Option<String>, _>(1).unwrap().as_deref(), Some("fir"));
    let done = portal.pull(grant).await.expect("demanded complete");
    let PgPortalOutcome::Complete { row: None, .. } = done.outcome else {
        panic!("command complete arrives only on the demanded empty pull");
    };
    assert_eq!(done.accounting.consumed_rows, 0);
    assert_eq!(done.accounting.execute_limit, 1);
    drop(portal);
    let isolation: String = sqlx::query("SHOW transaction_isolation")
        .fetch_one(&mut *tx)
        .await
        .expect("isolation")
        .try_get(0)
        .expect("level");
    assert_eq!(isolation, "repeatable read");
    let visible: String = sqlx::query(
        "SELECT coalesce(string_agg(id::text || ':' || coalesce(note, 'NULL'), ',' ORDER BY id), '') FROM rv.page",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("visible")
    .try_get(0)
    .expect("text");
    assert_eq!(visible, "1:pine,2:fir");

    let mut parked = tx
        .open_named_portal("SELECT id FROM rv.page ORDER BY id", PgArguments::default())
        .await
        .expect("close portal");
    let parked_step = parked.pull(grant).await.expect("parked pull");
    assert!(matches!(parked_step.outcome, PgPortalOutcome::Suspended(_)));
    let closed = parked.close().await.expect("close");
    assert!(closed.ready);
    assert!(closed.transaction_open);
    let still: String = sqlx::query("SHOW transaction_isolation")
        .fetch_one(&mut *tx)
        .await
        .expect("still rr")
        .try_get(0)
        .expect("level");
    assert_eq!(still, "repeatable read");
    let one: i32 = sqlx::query("SELECT 1")
        .fetch_one(&mut *tx)
        .await
        .expect("after close")
        .try_get(0)
        .expect("one");
    assert_eq!(one, 1);
    // The exported snapshot exists only while this transaction is still open.
    oracle_cut(&mut oracle, &snapshot).await;
    tx.rollback().await.expect("reader rollback");
    assert!(!reader.named_portal_poisoned());
    division_error(&mut reader).await;
    cancel_sleep(&mut reader, &mut admin).await;
    drop_poisons(&mut reader).await;
    println!("t3_named_portal=ok snapshot={snapshot}");
}

fn snapshot_id_ok(id: &str) -> bool {
    let mut parts = id.split('-');
    let mut seen = 0;
    for part in parts.by_ref().take(3) {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return false;
        }
        seen += 1;
    }
    seen == 3 && parts.next().is_none()
}

async fn oracle_cut(oracle: &mut PgConnection, snapshot: &str) {
    assert!(snapshot_id_ok(snapshot));
    let mut tx = oracle
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .expect("oracle begin");
    let statement = format!("SET TRANSACTION SNAPSHOT '{snapshot}'");
    sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
        .execute(&mut *tx)
        .await
        .expect("import snapshot");
    let visible: String = sqlx::query(
        "SELECT coalesce(string_agg(id::text || ':' || coalesce(note, 'NULL'), ',' ORDER BY id), '') FROM rv.page",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("oracle visible")
    .try_get(0)
    .expect("text");
    assert_eq!(visible, "1:pine,2:fir");
    tx.rollback().await.expect("oracle rollback");
}

async fn division_error(reader: &mut PgConnection) {
    let mut tx = reader.begin().await.expect("error begin");
    let mut args = PgArguments::default();
    args.add(0i32).expect("zero");
    match tx.open_named_portal("SELECT (1 / $1)::int4", args).await {
        Ok(mut portal) => {
            let err = match portal
                .pull(PgPortalAllowance::positive(1).expect("allowance"))
                .await
            {
                Err(err) => err,
                Ok(_) => panic!("division"),
            };
            assert_eq!(sqlstate(&err).as_deref(), Some("22012"));
            assert_eq!(portal.last_pull().expect("accounted").consumed_rows, 0);
            assert!(!portal.connection_poisoned());
            drop(portal);
        }
        Err(err) => assert_eq!(sqlstate(&err).as_deref(), Some("22012")),
    }
    tx.rollback().await.expect("rollback after 22012");
    assert!(!reader.named_portal_poisoned());
    println!("portal_error=22012 no_false_finish");
}

fn sqlstate(err: &sqlx::Error) -> Option<String> {
    err.as_database_error()
        .and_then(|db| db.code().map(|code| code.into_owned()))
}

async fn cancel_sleep(reader: &mut PgConnection, admin: &mut PgConnection) {
    let mut tx = reader.begin().await.expect("sleep begin");
    let pid: i32 = sqlx::query("SELECT pg_backend_pid()")
        .fetch_one(&mut *tx)
        .await
        .expect("pid")
        .try_get(0)
        .expect("pid");
    let mut portal = tx
        .open_named_portal("SELECT pg_sleep(30)", PgArguments::default())
        .await
        .expect("sleep portal");
    let (err, sent) = {
        let pull = portal.pull(PgPortalAllowance::positive(1).expect("allowance"));
        tokio::pin!(pull);
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut sent = false;
        let err = loop {
            tokio::select! {
                biased;
                result = &mut pull => break match result {
                    Err(err) => err,
                    Ok(_) => panic!("sleep must not succeed"),
                },
                _ = tokio::time::sleep(Duration::from_millis(40)), if !sent => {
                    if Instant::now() > deadline {
                        panic!("sleep did not become active");
                    }
                    let state: String = sqlx::query("SELECT COALESCE(state, '') FROM pg_stat_activity WHERE pid = $1")
                        .bind(pid)
                        .fetch_one(&mut *admin)
                        .await
                        .expect("state")
                        .try_get(0)
                        .expect("state");
                    if state == "active" {
                        let cancelled: bool = sqlx::query("SELECT pg_cancel_backend($1)")
                            .bind(pid)
                            .fetch_one(&mut *admin)
                            .await
                            .expect("cancel")
                            .try_get(0)
                            .expect("flag");
                        assert!(cancelled);
                        sent = true;
                    }
                }
            }
        };
        (err, sent)
    };
    assert!(sent);
    assert_eq!(sqlstate(&err).as_deref(), Some("57014"));
    assert_eq!(portal.last_pull().expect("reserved").consumed_rows, 0);
    assert!(!portal.connection_poisoned());
    drop(portal);
    tx.rollback().await.expect("rollback after cancel");
    assert!(!reader.named_portal_poisoned());
    println!("portal_cancel=57014 consumed_rows=0");
}

async fn drop_poisons(reader: &mut PgConnection) {
    {
        let mut tx = reader.begin().await.expect("poison begin");
        let mut portal = tx
            .open_named_portal("SELECT id, note FROM rv.page ORDER BY id", PgArguments::default())
            .await
            .expect("poison portal");
        let step = portal
            .pull(PgPortalAllowance::positive(1).expect("allowance"))
            .await
            .expect("poison pull");
        assert!(matches!(step.outcome, PgPortalOutcome::Suspended(_)));
        drop(portal);
        assert!(tx.named_portal_poisoned());
    }
    let failed = sqlx::query("SELECT 1").fetch_one(&mut *reader).await;
    assert!(failed.is_err(), "poisoned connection accepted another statement");
    println!("portal_drop_poisons=ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn t4_sql_audit() {
    let pointer = pointer();
    let raw_bytes = fs::read(&pointer.log_file).expect("log bytes");
    assert!(!raw_bytes.is_empty(), "reader server log was empty");
    assert_ne!(raw_bytes[0], 0, "log starts with NUL");
    let raw = String::from_utf8(raw_bytes).expect("log utf8");
    for name in ["reader.pass", "writer.pass", "super.pass", "guc.pass"] {
        let secret = read_pass(&pointer.password_dir, name);
        assert!(!secret.is_empty());
        assert!(!raw.contains(secret.as_str()), "{name} appeared in the server log");
    }
    let marker = "a130_own_log_cut";
    let index = raw.rfind(marker).unwrap_or_else(|| panic!("missing {marker}"));
    let own = &raw[index + marker.len()..];
    assert!(own.contains("app=oo-pg-source"), "own reader log has no oo-pg-source statement");
    let report = classify(own);
    println!("{report}");
    if let Ok(path) = std::env::var("OO_A130_SQL_REPORT") {
        fs::write(path, &report).expect("sql report");
    }
    assert!(report.contains("reader_statements="));
    assert!(report.contains("reader_forbidden=0"));
}

fn classify(raw: &str) -> String {
    let mut reader_statements = 0usize;
    let mut reader_writes = Vec::new();
    let mut writer_statements = 0usize;
    let mut writer_writes = Vec::new();
    let mut admin_statements = 0usize;
    let mut admin_writes = Vec::new();
    let mut reader_verbs = BTreeMap::<String, usize>::new();
    for line in raw.lines() {
        let Some(sql) = statement_sql(line) else {
            continue;
        };
        let role = line_role(line);
        let word = write_word(&strip_sql_literals(sql));
        match role {
            Role::Reader => {
                reader_statements += 1;
                if let Some(verb) = first_verb(sql) {
                    *reader_verbs.entry(verb).or_default() += 1;
                }
                if let Some(found) = word {
                    reader_writes.push(found);
                }
            }
            Role::Writer => {
                writer_statements += 1;
                if let Some(found) = word {
                    writer_writes.push(found);
                }
            }
            Role::Admin => {
                admin_statements += 1;
                if let Some(found) = word {
                    admin_writes.push(found);
                }
            }
            Role::Other => {}
        }
    }
    assert!(reader_statements > 0, "reader SQL log was empty");
    assert!(reader_writes.is_empty(), "reader writes {reader_writes:?}");
    assert!(writer_statements > 0, "writer SQL log was empty");
    assert!(admin_statements > 0, "admin SQL log was empty");
    let verbs = reader_verbs
        .iter()
        .map(|(verb, count)| format!("{verb}:{count}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "reader_statements={reader_statements}\nreader_verbs={verbs}\nreader_forbidden=0\nwriter_statements={writer_statements}\nwriter_write_hits={}\nadmin_statements={admin_statements}\nadmin_write_hits={}\n",
        writer_writes.len(),
        admin_writes.len()
    )
}

enum Role {
    Reader,
    Writer,
    Admin,
    Other,
}

fn line_role(line: &str) -> Role {
    let app = app_field(line);
    let user = user_field(line);
    if app == "oo-pg-source" && user == "oo_snap_reader" || app == "oo-a130-reader" {
        Role::Reader
    } else if app == "oo-a130-writer" {
        Role::Writer
    } else if app == "oo-a130-admin" {
        Role::Admin
    } else {
        Role::Other
    }
}

fn app_field(line: &str) -> &str {
    field_after(line, "app=")
}

fn user_field(line: &str) -> &str {
    field_after(line, "user=")
}

fn field_after<'a>(line: &'a str, key: &str) -> &'a str {
    let Some(start) = line.find(key) else {
        return "";
    };
    let rest = &line[start + key.len()..];
    let end = rest.find([' ', '\t']).unwrap_or(rest.len());
    &rest[..end]
}

fn statement_sql(line: &str) -> Option<&str> {
    if let Some(rest) = line.split_once("statement:") {
        return Some(rest.1.trim());
    }
    if let Some(rest) = line.split_once("execute ") {
        return rest.1.split_once(':').map(|(_, sql)| sql.trim());
    }
    None
}

fn first_verb(sql: &str) -> Option<String> {
    let verb = sql.split_whitespace().next()?;
    let verb = verb.trim_matches(|ch: char| ch == '(' || ch == ';');
    if verb.is_empty() {
        None
    } else {
        Some(verb.to_ascii_uppercase())
    }
}

fn strip_sql_literals(sql: &str) -> String {
    let mut out = String::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            while let Some(inner) = chars.next() {
                if inner == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        continue;
                    }
                    break;
                }
            }
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

fn write_word(sql: &str) -> Option<&'static str> {
    const WORDS: &[&str] = &[
        "INSERT", "UPDATE", "DELETE", "MERGE", "CREATE", "ALTER", "DROP", "TRUNCATE", "GRANT",
        "REVOKE", "COMMIT", "COPY",
    ];
    for token in sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        let upper = token.to_ascii_uppercase();
        if let Some(word) = WORDS.iter().copied().find(|word| *word == upper) {
            return Some(word);
        }
    }
    None
}
