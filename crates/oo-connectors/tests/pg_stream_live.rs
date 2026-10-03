//! Live R2-S bounded Arrow full-snapshot pull on PostgreSQL 17.10.
//! `OO_PG_STREAM_FIXTURE` is required. A missing pointer is a failure.
//! `OO_PG_SNAPSHOT_FIXTURE` is not this test.

use std::cell::Cell;
use std::fs;
use std::time::{Duration, Instant};

use arrow_array::{Array, BooleanArray, Int32Array, RecordBatch, StringArray};
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SnapshotOwner, SnapshotRequest, SourceId, StreamBudget,
    StreamItem, TableSelection, CLEANUP_OBSERVATION_BOUND_MS, STREAM_PROTOCOL,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "stream_src";
const SCHEMA: &str = "stream";

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

struct Activity {
    state: String,
    wait_event: String,
    in_xact: bool,
    app: String,
    user: String,
}

type ParentRow = (i32, bool, Option<String>);
type ChildRow = (Option<i32>, Option<String>);

struct Oracle {
    parent: Vec<ParentRow>,
    child: Vec<ChildRow>,
    empty: i64,
    zero: i64,
}

struct World {
    pointer: Pointer,
    reader_pass: Zeroizing<String>,
    writer_pass: Zeroizing<String>,
    super_pass: Zeroizing<String>,
    ca: Vec<u8>,
    admin: PgConnection,
    generation: Cell<u64>,
    postmaster: String,
    in_recovery: bool,
    admitted: AdmittedCatalog,
    outcomes: Vec<String>,
    outcome_path: Option<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_arrow_stream_live() {
    let mut world = World::start().await;
    world.note(
        "catalog_max_rows=128 reason=keyword_catalog_returns_about_78_rows_so_open_cannot_use_8"
            .to_string(),
    );
    let mut oracle_conn = world.reader_conn("oo-stream-oracle").await;
    sqlx::query("START TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut oracle_conn)
        .await
        .expect("oracle transaction");
    let oracle = read_oracle(&mut oracle_conn).await;
    assert_eq!(oracle.parent.len(), 4);
    assert_eq!(oracle.child.len(), 3);
    assert_eq!(oracle.empty, 0);
    assert_eq!(oracle.zero, 2);
    world.note(format!(
        "oracle_before_writer parent={} child={} empty={} zero={} payload={}",
        oracle.parent.len(),
        oracle.child.len(),
        oracle.empty,
        oracle.zero,
        oracle_payload(&oracle)
    ));

    world.finish_lifecycle(&oracle, &mut oracle_conn).await;
    world.abort_lifecycle().await;
    world.drop_lifecycle().await;
    world.audit();
    world.note("live_complete".to_string());
    let _ = oracle_conn.close().await;
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(path) = &self.outcome_path {
            let _ = fs::write(path, self.outcomes.join("\n") + "\n");
        }
    }
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_STREAM_FIXTURE").expect("OO_PG_STREAM_FIXTURE is required");
        let pointer: Pointer =
            serde_json::from_str(&fs::read_to_string(&path).expect("stream pointer"))
                .expect("stream pointer json");
        let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
        let writer_pass = read_pass(&pointer.password_dir, "writer.pass");
        let super_pass = read_pass(&pointer.password_dir, "super.pass");
        let ca = fs::read(&pointer.ca_file).expect("ca");
        let mut admin = dial(
            &pointer,
            &pointer.super_user,
            &super_pass,
            &ca,
            "oo-stream-admin",
        )
        .await;
        sqlx::query("DELETE FROM stream.child WHERE parent_id = 5 OR note = 'late'")
            .execute(&mut admin)
            .await
            .expect("reset child");
        sqlx::query("DELETE FROM stream.parent WHERE id >= 5")
            .execute(&mut admin)
            .await
            .expect("reset parent");
        let (postmaster, in_recovery) = node_of(&mut admin).await;
        let admitted = load_admitted(&mut admin, &["parent", "child", "empty", "zerocol"]).await;
        let outcome_path = std::env::var("OO_PG_STREAM_OUTCOME").ok();
        Self {
            pointer,
            reader_pass,
            writer_pass,
            super_pass,
            ca,
            admin,
            generation: Cell::new(0),
            postmaster,
            in_recovery,
            admitted,
            outcomes: Vec::new(),
            outcome_path,
        }
    }

    fn note(&mut self, line: String) {
        eprintln!("outcome {line}");
        self.outcomes.push(line);
    }

    fn next_gen(&self) -> u64 {
        let next = self.generation.get() + 1;
        self.generation.set(next);
        next
    }

    fn config(&self) -> PgSourceConfig {
        PgSourceConfig::try_new(
            SourceId::new(SOURCE).unwrap(),
            PgEndpoint::new(
                &self.pointer.host,
                self.pointer.port,
                &self.pointer.database,
            )
            .unwrap(),
            PgCredentials::new(&self.pointer.reader_user, &*self.reader_pass).unwrap(),
            SecretRef::new("secret://pg/stream/reader").unwrap(),
            PgTlsConfig::from_ca_pem(&self.ca).unwrap(),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(5),
                128,
                1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(vec![
                SelectedTable::new(SCHEMA, "parent").unwrap(),
                SelectedTable::new(SCHEMA, "child").unwrap(),
                SelectedTable::new(SCHEMA, "empty").unwrap(),
                SelectedTable::new(SCHEMA, "zerocol").unwrap(),
            ])
            .unwrap(),
        )
        .unwrap()
    }

    fn request(&self, generation: u64) -> SnapshotRequest {
        let config = self.config();
        let policy = oo_connectors::PgTypePolicy::PRESERVE_ROW;
        SnapshotRequest {
            config_identity: config_identity(&config),
            type_policy_identity: type_policy_identity(policy),
            config,
            admitted: self.admitted.clone(),
            policy,
            generation,
            deadline: Instant::now() + Duration::from_secs(90),
            expected_principal: self.pointer.reader_user.clone(),
            expected_database: self.pointer.database.clone(),
            expected_in_recovery: self.in_recovery,
            expected_postmaster_start: self.postmaster.clone(),
        }
    }

    async fn reader_conn(&self, app: &str) -> PgConnection {
        dial(
            &self.pointer,
            &self.pointer.reader_user,
            &self.reader_pass,
            &self.ca,
            app,
        )
        .await
    }

    async fn finish_lifecycle(&mut self, oracle: &Oracle, oracle_conn: &mut PgConnection) {
        let mut owner = SnapshotOwner::open(self.request(self.next_gen()))
            .await
            .expect("open stream owner");
        let binding = owner.binding().clone();
        assert_eq!(binding.protocol, oo_connectors::SNAPSHOT_PROTOCOL);
        let backends = self.oo_backends().await;
        assert_eq!(backends.len(), 2, "owner keeps a data session and a guard");
        let budget = StreamBudget::try_new(2, 1_048_576, 1024, 8192).expect("live budget");
        assert_eq!(budget.queue_batches(), 1);
        owner.open_stream(budget).await.expect("open stream");

        let mut parent = Vec::new();
        let mut child = Vec::new();
        let mut parent_batches = 0_u32;
        let mut child_batches = 0_u32;
        let mut empty_batches = 0_u32;
        let mut zero_rows = 0_usize;
        let mut zero_batches = 0_u32;
        let mut eofs: Vec<oo_connectors::TableEof> = Vec::new();
        let mut wrote = false;
        let accounting = loop {
            match owner.next_batch().await.expect("next batch") {
                StreamItem::Batch(batch) => {
                    assert!(!batch.resumable);
                    assert_eq!(batch.typed.protocol, oo_connectors::TYPE_CONVERSION_PROTOCOL);
                    assert_eq!(batch.typed.run_generation, binding.generation);
                    assert_eq!(batch.typed.input_digest, binding.input_digest);
                    assert_eq!(batch.typed.schema, SCHEMA);
                    assert_eq!(batch.accounting.retained_batches, 0);
                    match batch.typed.table.as_str() {
                        "parent" => {
                            if !wrote {
                                assert_eq!(parent.len(), 0);
                                self.hold(&owner).await;
                            }
                            let rows = parent_arrow(&batch.record_batch);
                            assert!(!rows.is_empty() && rows.len() <= 2);
                            parent_batches += 1;
                            parent.extend(rows);
                            if !wrote {
                                assert_eq!(parent.len(), 2, "first demand is one batch of two");
                                self.writer_commits().await;
                                wrote = true;
                            }
                        }
                        "child" => {
                            assert!(wrote, "child started before the writer");
                            let rows = child_arrow(&batch.record_batch);
                            assert!(!rows.is_empty() && rows.len() <= 2);
                            for row in &rows {
                                assert!(row.1.as_deref() != Some("late"));
                            }
                            child_batches += 1;
                            child.extend(rows);
                        }
                        "empty" => empty_batches += 1,
                        "zerocol" => {
                            assert!(batch.typed.arrow.is_empty());
                            assert_eq!(batch.record_batch.num_columns(), 0);
                            assert!(batch.record_batch.num_rows() > 0);
                            assert!(batch.record_batch.num_rows() <= 2);
                            zero_batches += 1;
                            zero_rows += batch.record_batch.num_rows();
                        }
                        other => panic!("unexpected table {other}"),
                    }
                }
                StreamItem::TableEof(eof) => {
                    assert!(!eof.resumable_key);
                    assert_eq!(eof.schema, SCHEMA);
                    eofs.push(eof);
                }
                StreamItem::Exhausted(accounting) => break accounting,
            }
        };
        let again = owner.next_batch().await.expect("second exhaustion");
        let StreamItem::Exhausted(again_acc) = again else {
            panic!("run finish is not a second stream item");
        };
        assert_eq!(again_acc.executes, accounting.executes);
        assert_eq!(again_acc.consumed_rows, accounting.consumed_rows);
        sort_parent(&mut parent);
        sort_child(&mut child);
        assert_eq!(parent, oracle.parent);
        assert_eq!(child, oracle.child);
        assert_eq!(empty_batches, 0, "an empty table has no data batch");
        assert_eq!(zero_rows, 2);
        assert!(zero_batches >= 1);
        assert!(parent_batches >= 2);
        assert!(child_batches >= 2);
        let eof = |name: &str| {
            eofs.iter()
                .find(|item| item.table == name)
                .unwrap_or_else(|| panic!("missing eof {name}"))
        };
        assert_eq!(eof("parent").rows_emitted, 4);
        assert!(!eof("parent").zero_column);
        assert_eq!(eof("child").rows_emitted, 3);
        assert!(!eof("child").zero_column);
        assert_eq!(eof("empty").rows_emitted, 0);
        assert_eq!(eof("empty").columns, 1);
        assert!(!eof("empty").zero_column);
        assert_eq!(eof("zerocol").rows_emitted, 2);
        assert_eq!(eof("zerocol").columns, 0);
        assert!(eof("zerocol").zero_column);
        assert_eq!(eofs.len(), 4);
        let payload = oracle_payload(oracle);
        self.note(format!(
            "measured executes={} reserved_rows={} consumed_rows={} decoded_payload_bytes={} oracle_payload={} arrow_preimage_bytes={} arrow_assembly_bytes={} wire_view_capacity_peak={} wire_accepted_logical_payload={} wire_assembly_bytes={} parent_batches={} child_batches={} zero_batches={}",
            accounting.executes,
            accounting.reserved_rows,
            accounting.consumed_rows,
            accounting.decoded_payload_bytes,
            payload,
            accounting.arrow_preimage_bytes,
            accounting.arrow_assembly_bytes,
            accounting.wire_view_capacity_peak,
            accounting.wire_accepted_logical_payload,
            accounting.wire_assembly_bytes,
            parent_batches,
            child_batches,
            zero_batches
        ));
        assert_eq!(accounting.reserved_rows, accounting.executes);
        assert_eq!(accounting.consumed_rows, 9);
        assert_eq!(accounting.executes, 13);
        assert_eq!(accounting.decoded_payload_bytes, payload);
        assert!(accounting.arrow_assembly_bytes > 0);
        assert!(accounting.arrow_preimage_bytes > 0);
        assert_eq!(accounting.retained_batches, 0);
        assert_eq!(accounting.retained_rows, 0);
        assert_eq!(STREAM_PROTOCOL, "oo.source.pgsql.stream/v1");

        let after = read_oracle(oracle_conn).await;
        assert_eq!(after.parent, oracle.parent);
        assert_eq!(after.child, oracle.child);
        let visible: i64 = sqlx::query("SELECT count(*)::int8 FROM stream.parent WHERE id = 5")
            .fetch_one(&mut self.admin)
            .await
            .expect("admin sees writer")
            .try_get(0)
            .expect("count");
        assert_eq!(visible, 1, "the writer commit is visible outside the snapshot");
        self.note(format!("admin_sees_late_parent={visible}"));

        let pid = binding.backend.pid;
        let start = binding.backend.backend_start.clone();
        let finished = owner.finish().await.expect("finish");
        assert_eq!(finished.generation, binding.generation);
        assert_eq!(finished.source_id, SOURCE);
        assert_eq!(finished.backend.pid, pid);
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
        assert!(self.oo_backends().await.is_empty());
        self.note(format!("finish_released pid={pid}"));
    }

    async fn abort_lifecycle(&mut self) {
        let mut owner = SnapshotOwner::open(self.request(self.next_gen()))
            .await
            .expect("abort open");
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let generation = owner.binding().generation;
        let backends = self.oo_backends().await;
        let budget = StreamBudget::try_new(2, 1_048_576, 1024, 8192).expect("budget");
        owner.open_stream(budget).await.expect("abort stream");
        let first = owner.next_batch().await.expect("abort batch");
        assert!(matches!(first, StreamItem::Batch(_)));
        let aborted = owner.abort().await.expect("abort");
        assert_eq!(aborted.generation, generation);
        assert_eq!(aborted.source_id, SOURCE);
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
        assert!(self.oo_backends().await.is_empty());
        self.note(format!("abort_released pid={pid}"));
    }

    async fn drop_lifecycle(&mut self) {
        let mut owner = SnapshotOwner::open(self.request(self.next_gen()))
            .await
            .expect("drop open");
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let backends = self.oo_backends().await;
        let budget = StreamBudget::try_new(2, 1_048_576, 1024, 8192).expect("budget");
        owner.open_stream(budget).await.expect("drop stream");
        let first = owner.next_batch().await.expect("drop batch");
        assert!(matches!(first, StreamItem::Batch(_)));
        drop(owner);
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
        assert!(self.oo_backends().await.is_empty());
        self.note(format!("drop_released pid={pid}"));
    }

    async fn writer_commits(&mut self) {
        let mut writer = dial(
            &self.pointer,
            &self.pointer.writer_user,
            &self.writer_pass,
            &self.ca,
            "oo-fixture-writer",
        )
        .await;
        sqlx::query("INSERT INTO stream.parent (id, flag, label) VALUES (5, true, 'late')")
            .execute(&mut writer)
            .await
            .expect("writer parent");
        sqlx::query("INSERT INTO stream.child (parent_id, note) VALUES (5, 'late')")
            .execute(&mut writer)
            .await
            .expect("writer child");
        let _ = writer.close().await;
    }

    async fn hold(&mut self, owner: &SnapshotOwner) {
        let pid = i32::try_from(owner.binding().backend.pid).expect("pid");
        let start = owner.binding().backend.backend_start.clone();
        let began = Instant::now();
        loop {
            match activity(&mut self.admin, pid, &start).await {
                Some(row)
                    if row.in_xact
                        && row.app == "oo-pg-source"
                        && row.user == self.pointer.reader_user
                        && waiting_on_client(&row) =>
                {
                    self.note(format!(
                        "held_state={} wait_event={}",
                        row.state, row.wait_event
                    ));
                    return;
                }
                Some(row) if began.elapsed() > Duration::from_secs(2) => {
                    panic!("backend {pid} stayed {}", row.state);
                }
                None if began.elapsed() > Duration::from_secs(2) => {
                    panic!("backend {pid} missing while the batch is held");
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    async fn wait_gone(&mut self, pid: i64, start: &str) {
        let pid_i = i32::try_from(pid).expect("pid");
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            match activity(&mut self.admin, pid_i, start).await {
                None => return,
                Some(row) if began.elapsed() > bound => {
                    panic!(
                        "backend {pid} still {} in_xact={} after {}ms",
                        row.state,
                        row.in_xact,
                        began.elapsed().as_millis()
                    );
                }
                Some(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }

    async fn oo_backends(&mut self) -> Vec<(i64, String)> {
        let rows = sqlx::query(
            "SELECT pid::int8, backend_start::text FROM pg_catalog.pg_stat_activity \
             WHERE application_name = 'oo-pg-source' AND usename::text = $1",
        )
        .bind(&self.pointer.reader_user)
        .fetch_all(&mut self.admin)
        .await
        .expect("oo backends");
        rows.iter()
            .map(|row| {
                (
                    row.try_get(0).expect("pid"),
                    row.try_get(1).expect("start"),
                )
            })
            .collect()
    }

    fn audit(&mut self) {
        let raw = fs::read_to_string(&self.pointer.log_file).expect("postgres log");
        for secret in [
            self.reader_pass.as_str(),
            self.writer_pass.as_str(),
            self.super_pass.as_str(),
        ] {
            if !secret.is_empty() && raw.contains(secret) {
                panic!("postgres log contains a closed string");
            }
        }
        let mut oo_statements = 0_usize;
        let mut writer_writes = 0_usize;
        let mut saw_start = false;
        for line in raw.lines() {
            if line.contains("app=oo-fixture-writer")
                && let Some(sql) = statement_sql(line)
                && write_word(&strip_sql_literals(sql)).is_some()
            {
                writer_writes += 1;
            }
            if !line.contains("app=oo-pg-source") || !line.contains("user=oo_a131_reader") {
                continue;
            }
            let Some(sql) = statement_sql(line) else {
                continue;
            };
            oo_statements += 1;
            let verb = sql
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            if matches!(verb.as_str(), "START" | "BEGIN") {
                saw_start = true;
            }
            if write_word(&strip_sql_literals(sql)).is_some() {
                panic!("oo statement contains a write verb");
            }
        }
        assert!(oo_statements > 0, "reader sql log was empty");
        assert!(saw_start, "reader transaction start was not logged");
        assert!(writer_writes > 0, "fixture writer was not logged");
        self.note(format!(
            "sql_audit oo_statements={oo_statements} writer_writes={writer_writes} dml_ddl_commit=0"
        ));
    }
}

fn parent_arrow(batch: &RecordBatch) -> Vec<ParentRow> {
    let ids = batch
        .column_by_name("id")
        .expect("id")
        .as_any()
        .downcast_ref::<Int32Array>()
        .expect("id int32");
    let flags = batch
        .column_by_name("flag")
        .expect("flag")
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("flag bool");
    let labels = batch
        .column_by_name("label")
        .expect("label")
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("label utf8");
    let mut rows = Vec::with_capacity(batch.num_rows());
    for index in 0..batch.num_rows() {
        assert!(!ids.is_null(index));
        assert!(!flags.is_null(index));
        let label = if labels.is_null(index) {
            None
        } else {
            Some(labels.value(index).to_string())
        };
        rows.push((ids.value(index), flags.value(index), label));
    }
    rows
}

fn child_arrow(batch: &RecordBatch) -> Vec<ChildRow> {
    let ids = batch
        .column_by_name("parent_id")
        .expect("parent_id")
        .as_any()
        .downcast_ref::<Int32Array>()
        .expect("parent_id int32");
    let notes = batch
        .column_by_name("note")
        .expect("note")
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("note utf8");
    let mut rows = Vec::with_capacity(batch.num_rows());
    for index in 0..batch.num_rows() {
        let id = if ids.is_null(index) {
            None
        } else {
            Some(ids.value(index))
        };
        let note = if notes.is_null(index) {
            None
        } else {
            Some(notes.value(index).to_string())
        };
        rows.push((id, note));
    }
    rows
}

fn sort_parent(rows: &mut [ParentRow]) {
    rows.sort_by_key(|row| row.0);
}

fn sort_child(rows: &mut [ChildRow]) {
    rows.sort();
}

fn oracle_payload(oracle: &Oracle) -> u64 {
    let parent: u64 = oracle
        .parent
        .iter()
        .map(|(_, _, label)| 4 + 1 + label.as_ref().map(|text| text.len() as u64).unwrap_or(0))
        .sum();
    let child: u64 = oracle
        .child
        .iter()
        .map(|(id, note)| {
            id.map(|_| 4_u64).unwrap_or(0) + note.as_ref().map(|text| text.len() as u64).unwrap_or(0)
        })
        .sum();
    let zero = u64::try_from(oracle.zero).expect("zero") * 4;
    parent + child + zero
}

async fn read_oracle(conn: &mut PgConnection) -> Oracle {
    let parent_rows = sqlx::query("SELECT id, flag, label FROM stream.parent ORDER BY id")
        .fetch_all(&mut *conn)
        .await
        .expect("oracle parent");
    let mut parent = Vec::new();
    for row in &parent_rows {
        parent.push((
            row.try_get(0).expect("id"),
            row.try_get(1).expect("flag"),
            row.try_get(2).expect("label"),
        ));
    }
    let child_rows = sqlx::query(
        "SELECT parent_id, note FROM stream.child ORDER BY parent_id NULLS FIRST, note NULLS FIRST",
    )
    .fetch_all(&mut *conn)
    .await
    .expect("oracle child");
    let mut child = Vec::new();
    for row in &child_rows {
        child.push((row.try_get(0).expect("parent_id"), row.try_get(1).expect("note")));
    }
    let empty: i64 = sqlx::query("SELECT count(*)::int8 FROM stream.empty")
        .fetch_one(&mut *conn)
        .await
        .expect("oracle empty")
        .try_get(0)
        .expect("count");
    let zero: i64 = sqlx::query("SELECT count(*)::int8 FROM stream.zerocol")
        .fetch_one(&mut *conn)
        .await
        .expect("oracle zero")
        .try_get(0)
        .expect("count");
    Oracle {
        parent,
        child,
        empty,
        zero,
    }
}

async fn load_admitted(admin: &mut PgConnection, names: &[&str]) -> AdmittedCatalog {
    let mut tables = Vec::new();
    for name in names {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'stream' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
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
             WHERE n.nspname = 'stream' AND c.relname = $1 AND i.indisprimary \
             ORDER BY u.ord",
        )
        .bind(*name)
        .fetch_all(&mut *admin)
        .await
        .expect("admitted keys");
        let primary_key = keys
            .iter()
            .map(|row| row.try_get::<String, _>(0).unwrap())
            .collect();
        tables.push(AdmittedTable {
            schema: SCHEMA.to_string(),
            name: (*name).to_string(),
            columns,
            primary_key,
        });
    }
    AdmittedCatalog { tables }
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
        .expect("dial")
}

async fn node_of(admin: &mut PgConnection) -> (String, bool) {
    let row = sqlx::query(
        "SELECT pg_catalog.pg_postmaster_start_time()::text, pg_catalog.pg_is_in_recovery()",
    )
    .fetch_one(&mut *admin)
    .await
    .expect("node");
    (
        row.try_get(0).expect("postmaster"),
        row.try_get(1).expect("recovery"),
    )
}

async fn activity(admin: &mut PgConnection, pid: i32, start: &str) -> Option<Activity> {
    let row = sqlx::query(
        "SELECT state, COALESCE(wait_event, ''), xact_start IS NOT NULL, \
         application_name, usename::text \
         FROM pg_catalog.pg_stat_activity \
         WHERE pid = $1 AND backend_start::text = $2",
    )
    .bind(pid)
    .bind(start)
    .fetch_optional(&mut *admin)
    .await
    .expect("activity");
    row.map(|row| Activity {
        state: row.try_get(0).unwrap_or_default(),
        wait_event: row.try_get(1).unwrap_or_default(),
        in_xact: row.try_get(2).unwrap_or(false),
        app: row.try_get(3).unwrap_or_default(),
        user: row.try_get(4).unwrap_or_default(),
    })
}

/// A suspended portal has no `Sync`, so PostgreSQL can keep `state=active`
/// while the backend is blocked in `ClientRead`. That is not another `Execute`.
fn waiting_on_client(row: &Activity) -> bool {
    row.wait_event == "ClientRead"
        || row.state == "idle in transaction"
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("password file");
    Zeroizing::new(text.trim().to_string())
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
        "INSERT", "UPDATE", "DELETE", "TRUNCATE", "MERGE", "COPY", "CREATE", "ALTER", "DROP",
        "GRANT", "REVOKE", "COMMENT", "VACUUM", "CLUSTER", "REINDEX", "CALL", "DO", "COMMIT",
        "NEXTVAL", "LO_IMPORT", "LO_EXPORT", "LO_CREATE", "DBLINK", "PG_WRITE_FILE",
        "PG_READ_FILE", "PG_READ_BINARY_FILE",
    ];
    for token in sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        let upper = token.to_ascii_uppercase();
        if let Some(word) = WORDS.iter().copied().find(|word| *word == upper) {
            return Some(word);
        }
    }
    None
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
