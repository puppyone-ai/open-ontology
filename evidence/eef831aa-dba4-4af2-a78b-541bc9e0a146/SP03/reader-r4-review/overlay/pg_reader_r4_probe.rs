//! Independent R4 probe. Not a product test.
//!
//! Records what `SnapshotOwner::finish` returns. A `SnapshotFinished` value is
//! a lifecycle close. This file does not treat that value as proof that every
//! selected row was read.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, Instant};

use arrow_array::{Array, Int32Array, StringArray};
use oo_connectors::{
    config_identity, connector_capabilities, type_capabilities, type_policy_identity,
    AdmittedCatalog, AdmittedColumn, AdmittedTable, PgBudget, PgCredentials, PgEndpoint,
    PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable, SnapshotFinished, SnapshotOwner,
    SnapshotRequest, SourceError, SourceErrorKind, SourceId, StreamBudget, StreamItem,
    TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "r4_src";

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
    outcomes: Vec<String>,
    outcome_path: Option<String>,
    log_mark: u64,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r4_finish_is_not_a_completeness_token() {
    assert_eq!(CLEANUP_OBSERVATION_BOUND_MS, 5_000);
    let mut world = World::start().await;
    world.note_capabilities();
    world.seed().await;
    world.phase_longitudinal().await;
    world.phase_finish_idle().await;
    world.phase_finish_before_poll().await;
    world.phase_finish_after_one_batch().await;
    world.phase_missing_selected_table().await;
    world.phase_portal_bound().await;
    world.audit();
    world.note("r4_probe_complete".to_string());
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_R4_FIXTURE").expect("OO_PG_R4_FIXTURE is required");
        let pointer: Pointer =
            serde_json::from_str(&fs::read_to_string(&path).expect("pointer")).expect("pointer json");
        let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
        let writer_pass = read_pass(&pointer.password_dir, "writer.pass");
        let super_pass = read_pass(&pointer.password_dir, "super.pass");
        let ca = fs::read(&pointer.ca_file).expect("ca");
        let mut admin = dial(
            &pointer,
            &pointer.super_user,
            &super_pass,
            &ca,
            "oo-fixture-admin",
        )
        .await;
        let (postmaster, in_recovery) = node_of(&mut admin).await;
        let log_mark = fs::metadata(&pointer.log_file).map(|meta| meta.len()).unwrap_or(0);
        let mut world = Self {
            pointer,
            reader_pass,
            writer_pass,
            super_pass,
            ca,
            admin,
            generation: Cell::new(0),
            postmaster,
            in_recovery,
            outcomes: Vec::new(),
            outcome_path: std::env::var("OO_PG_R4_OUTCOME").ok(),
            log_mark,
        };
        world.note(format!("log_mark={}", world.log_mark));
        world
    }

    fn note(&mut self, line: String) {
        eprintln!("outcome {line}");
        self.outcomes.push(line);
    }

    fn note_capabilities(&mut self) {
        for cap in connector_capabilities() {
            self.note(format!(
                "connector_capability name={} supported={} reason={}",
                cap.name, cap.supported, cap.reason
            ));
        }
        for cap in type_capabilities() {
            if cap.name == "bounded_reader" || cap.name == "cdc" || cap.name == "position_codec" {
                self.note(format!(
                    "type_capability name={} supported={} reason={}",
                    cap.name, cap.supported, cap.reason
                ));
            }
        }
    }

    fn next_gen(&self) -> u64 {
        let next = self.generation.get() + 1;
        self.generation.set(next);
        next
    }

    async fn seed(&mut self) {
        let _ = sqlx::query("ROLLBACK").execute(&mut self.admin).await;
        let reader = quote_ident(&self.pointer.reader_user);
        let writer = quote_ident(&self.pointer.writer_user);
        let ddl = format!(
            "DROP SCHEMA IF EXISTS r4 CASCADE; \
             DROP SCHEMA IF EXISTS r4e CASCADE; \
             CREATE SCHEMA r4; \
             CREATE TABLE r4.item (id int PRIMARY KEY, name text NOT NULL); \
             INSERT INTO r4.item (id, name) VALUES (1, 'willow'), (2, 'oak'); \
             CREATE TABLE r4.line (line_id int NOT NULL, item_id int NOT NULL, qty int NOT NULL); \
             INSERT INTO r4.line (line_id, item_id, qty) VALUES (10, 1, 3), (11, 1, 1), (12, 2, 4); \
             CREATE SCHEMA r4e; \
             CREATE TABLE r4e.rows (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r4e.rows (id, note) VALUES (1, 'a'), (2, 'b'), (3, 'c'), (4, 'd'); \
             CREATE TABLE r4e.extra (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r4e.extra (id, note) VALUES (8, 'x'), (9, 'y'); \
             CREATE TABLE r4e.wide (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r4e.wide (id, note) VALUES (1, 'abcdefghij'); \
             GRANT USAGE ON SCHEMA r4, r4e TO {reader}, {writer}; \
             GRANT SELECT ON ALL TABLES IN SCHEMA r4, r4e TO {reader}; \
             GRANT SELECT, INSERT, UPDATE ON r4.item, r4.line TO {writer}"
        );
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(&mut self.admin)
            .await
            .expect("seed");
        self.note("seeded r4.item r4.line r4e.rows r4e.extra r4e.wide".to_string());
    }

    async fn phase_longitudinal(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4", &[("item", true), ("line", false)]).await;
        let before = oracle_cut(&mut self.admin).await;
        self.note(format!(
            "oracle_before_writer items={} lines={} item1={}",
            before.items.len(),
            before.lines.len(),
            before.item_name(1)
        ));
        let mut owner = self
            .open(&admitted, &[("r4", "item"), ("r4", "line")], 90, 60, 30)
            .await;
        let generation = owner.binding().generation;
        let pid = owner.binding().backend.pid;
        let converted: Vec<String> = owner
            .binding()
            .catalog
            .tables
            .iter()
            .flat_map(|table| {
                table.columns.iter().map(move |column| {
                    format!(
                        "{}.{} converted={} reason={:?}",
                        table.name, column.name, column.type_support.converted, column.type_support.reason
                    )
                })
            })
            .collect();
        self.note(format!(
            "catalog_type_support {}",
            converted.join(" ")
        ));
        assert!(
            converted.iter().all(|line| line.contains("converted=false")),
            "a selected column was marked converted"
        );
        owner
            .open_stream(StreamBudget::try_new(1, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("open stream");
        self.writer_mutates().await;
        let outside = oracle_cut(&mut self.admin).await;
        assert_eq!(outside.item_name(1), "pine", "writer commit was not visible outside");
        assert!(outside.lines.iter().any(|row| row.0 == 13), "new line was not visible outside");
        let read = drain(&mut owner).await;
        assert_eq!(
            by_id(&read.items),
            by_id(&before.items),
            "reader mixed the writer cut into item"
        );
        assert_eq!(
            by_line(&read.lines),
            by_line(&before.lines),
            "reader mixed the writer cut into line"
        );
        assert!(read.exhausted, "full drain did not yield Exhausted");
        assert!(read.batches >= 5, "batch_rows=1 did not split the cut, batches={}", read.batches);
        self.note(format!(
            "full_read generation={generation} data_pid={pid} batches={} item_rows={} line_rows={} exhausted=1 accounting_executes={} accounting_consumed_rows={} accounting_decoded_payload={} accounting_wire_view_peak={} accounting_wire_accepted={} same_cut_match=1",
            read.batches,
            read.items.len(),
            read.lines.len(),
            read.accounting_executes,
            read.accounting_consumed,
            read.accounting_payload,
            read.wire_view_peak,
            read.wire_accepted
        ));
        let began = Instant::now();
        let finished = owner.finish().await.expect("complete finish");
        assert_eq!(finished.generation, generation);
        self.note(format!(
            "complete_finish={} token_fields=generation,source_id,backend rows_in_token=absent",
            finish_label(Ok(finished))
        ));
        self.note(format!("complete_release_wait_ms={}", began.elapsed().as_millis()));

        let admitted = load_admitted(&mut self.admin, "r4", &[("item", true), ("line", false)]).await;
        let mut fresh = self
            .open(&admitted, &[("r4", "item"), ("r4", "line")], 90, 60, 30)
            .await;
        let fresh_generation = fresh.binding().generation;
        assert_ne!(fresh_generation, generation);
        fresh
            .open_stream(StreamBudget::try_new(8, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("fresh stream");
        let again = drain(&mut fresh).await;
        assert_eq!(
            by_id(&again.items),
            by_id(&outside.items),
            "fresh generation did not read the new cut"
        );
        assert_eq!(
            by_line(&again.lines),
            by_line(&outside.lines),
            "fresh generation mixed line rows"
        );
        assert_eq!(again.item_name(1), "pine");
        assert!(again.exhausted);
        let finished = fresh.finish().await.expect("fresh finish");
        let fresh_generation = finished.generation;
        let fresh_lines = again.lines.len();
        let item_scan = again
            .items
            .iter()
            .map(|(id, name)| format!("{id}:{name}"))
            .collect::<Vec<_>>()
            .join(",");
        self.note(format!(
            "fresh_generation={fresh_generation} fresh_item1=pine fresh_lines={fresh_lines} fresh_has_13=1 fresh_item_scan={item_scan} cut_compared_by_key=1 old_generation_reused=0 finish={}",
            finish_label(Ok(finished))
        ));
    }

    async fn writer_mutates(&mut self) {
        let mut writer = dial(
            &self.pointer,
            &self.pointer.writer_user,
            &self.writer_pass,
            &self.ca,
            "oo-fixture-writer",
        )
        .await;
        sqlx::query("UPDATE r4.item SET name = 'pine' WHERE id = 1")
            .execute(&mut writer)
            .await
            .expect("writer update");
        sqlx::query("INSERT INTO r4.line (line_id, item_id, qty) VALUES (13, 2, 9)")
            .execute(&mut writer)
            .await
            .expect("writer insert");
        let _ = writer.close().await;
        self.note("writer_committed update_item1=pine insert_line=13".to_string());
    }

    async fn phase_finish_idle(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4e", &[("rows", true)]).await;
        let owner = self.open(&admitted, &[("r4e", "rows")], 90, 60, 30).await;
        let generation = owner.binding().generation;
        let label = finish_label(owner.finish().await);
        self.note(format!(
            "early_idle generation={generation} rows_read=0 exhausted_seen=0 selected=r4e.rows finish={label}"
        ));
    }

    async fn phase_finish_before_poll(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4e", &[("rows", true)]).await;
        let mut owner = self.open(&admitted, &[("r4e", "rows")], 90, 60, 30).await;
        let generation = owner.binding().generation;
        owner
            .open_stream(StreamBudget::try_new(2, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("open stream");
        let label = finish_label(owner.finish().await);
        self.note(format!(
            "early_before_poll generation={generation} rows_read=0 exhausted_seen=0 selected=r4e.rows finish={label}"
        ));
    }

    async fn phase_finish_after_one_batch(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4e", &[("rows", true)]).await;
        let mut owner = self.open(&admitted, &[("r4e", "rows")], 90, 60, 30).await;
        let generation = owner.binding().generation;
        owner
            .open_stream(StreamBudget::try_new(2, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("open stream");
        let ids = match owner.next_batch().await.expect("one batch") {
            StreamItem::Batch(batch) => int_column(&batch.record_batch, "id"),
            StreamItem::TableEof(_) => panic!("first demand was table eof"),
            StreamItem::Exhausted(_) => panic!("first demand was exhausted"),
        };
        assert_eq!(ids.len(), 2, "batch_rows=2 did not bound the batch");
        let label = finish_label(owner.finish().await);
        self.note(format!(
            "early_after_one_batch generation={generation} rows_read=2 rows_in_table=4 exhausted_seen=0 ids={} finish={label}",
            ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")
        ));
    }

    async fn phase_missing_selected_table(&mut self) {
        let admitted = load_admitted(
            &mut self.admin,
            "r4e",
            &[("rows", true), ("extra", true)],
        )
        .await;
        let mut owner = self
            .open(&admitted, &[("r4e", "rows"), ("r4e", "extra")], 90, 60, 30)
            .await;
        let generation = owner.binding().generation;
        owner
            .open_stream(StreamBudget::try_new(8, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("open stream");
        let mut saw = BTreeMap::<String, u64>::new();
        let mut first_eof = None;
        loop {
            match owner.next_batch().await.expect("until first eof") {
                StreamItem::Batch(batch) => {
                    *saw.entry(batch.typed.table.clone()).or_default() += batch.record_batch.num_rows() as u64;
                }
                StreamItem::TableEof(eof) => {
                    first_eof = Some(eof.table);
                    break;
                }
                StreamItem::Exhausted(_) => panic!("both tables ended before a stop"),
            }
        }
        let first_eof = first_eof.expect("first table eof");
        let unread: Vec<&str> = ["rows", "extra"]
            .into_iter()
            .filter(|name| *name != first_eof && !saw.contains_key(*name))
            .collect();
        assert_eq!(unread.len(), 1, "one selected table should still be unread");
        let label = finish_label(owner.finish().await);
        self.note(format!(
            "missing_selected_table generation={generation} first_eof={first_eof} unread={} rows_of_first={} exhausted_seen=0 finish={label}",
            unread[0],
            saw.get(&first_eof).copied().unwrap_or(0)
        ));
    }

    async fn phase_portal_bound(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4e", &[("wide", true)]).await;
        let mut owner = self.open(&admitted, &[("r4e", "wide")], 90, 60, 30).await;
        let generation = owner.binding().generation;
        let pid = owner.binding().backend.pid;
        owner
            .open_stream(StreamBudget::try_new(1, 1_048_576, 4, 1024).expect("tight"))
            .await
            .expect("open wide");
        let err = match owner.next_batch().await {
            Err(err) => err,
            Ok(StreamItem::Batch(_)) => panic!("wide value produced a batch"),
            Ok(StreamItem::TableEof(_)) => panic!("wide value produced table eof"),
            Ok(StreamItem::Exhausted(_)) => panic!("wide value produced exhausted"),
        };
        assert_eq!(err.kind(), SourceErrorKind::Budget);
        assert_eq!(err.fact(), Some("max_value"));
        let finished = owner.finish().await;
        let label = finish_label(finished);
        assert!(
            !label.starts_with("SnapshotFinished"),
            "portal bound finish returned SnapshotFinished"
        );
        self.note(format!(
            "portal_bound generation={generation} data_pid={pid} next_fact=max_value finish={label} snapshot_finished=0"
        ));
    }

    async fn open(
        &mut self,
        admitted: &AdmittedCatalog,
        tables: &[(&str, &str)],
        deadline_secs: u64,
        statement_secs: u64,
        lock_secs: u64,
    ) -> SnapshotOwner {
        let generation = self.next_gen();
        SnapshotOwner::open(self.request(admitted, tables, generation, deadline_secs, statement_secs, lock_secs))
            .await
            .expect("open")
    }

    fn request(
        &self,
        admitted: &AdmittedCatalog,
        tables: &[(&str, &str)],
        generation: u64,
        deadline_secs: u64,
        statement_secs: u64,
        lock_secs: u64,
    ) -> SnapshotRequest {
        let selected = tables
            .iter()
            .map(|(schema, table)| SelectedTable::new(*schema, *table).expect("table"))
            .collect();
        let config = PgSourceConfig::try_new(
            SourceId::new(SOURCE).expect("source"),
            PgEndpoint::new(&self.pointer.host, self.pointer.port, &self.pointer.database)
                .expect("endpoint"),
            PgCredentials::new(&self.pointer.reader_user, self.reader_pass.as_str())
                .expect("credentials"),
            SecretRef::new("secret://pg/r4/reader").expect("secret"),
            PgTlsConfig::from_ca_pem(&self.ca).expect("tls"),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(statement_secs),
                Duration::from_secs(lock_secs),
                128,
                8 * 1024 * 1024,
            )
            .expect("budget"),
            TableSelection::new(selected).expect("selection"),
        )
        .expect("config");
        let policy = oo_connectors::PgTypePolicy::PRESERVE_ROW;
        SnapshotRequest {
            config_identity: config_identity(&config),
            type_policy_identity: type_policy_identity(policy),
            config,
            admitted: admitted.clone(),
            policy,
            generation,
            deadline: Instant::now() + Duration::from_secs(deadline_secs),
            expected_principal: self.pointer.reader_user.clone(),
            expected_database: self.pointer.database.clone(),
            expected_in_recovery: self.in_recovery,
            expected_postmaster_start: self.postmaster.clone(),
        }
    }

    fn audit(&mut self) {
        let raw_all = fs::read(&self.pointer.log_file).expect("postgres log");
        let raw_all_text = String::from_utf8_lossy(&raw_all);
        for secret in [
            self.reader_pass.as_str(),
            self.writer_pass.as_str(),
            self.super_pass.as_str(),
        ] {
            if !secret.is_empty() && raw_all_text.contains(secret) {
                panic!("postgres log contains a closed string");
            }
        }
        let mark = usize::try_from(self.log_mark).unwrap_or(0).min(raw_all.len());
        let raw = String::from_utf8_lossy(&raw_all[mark..]);
        let mut oo_statements = 0_usize;
        let mut verbs = BTreeMap::<String, usize>::new();
        for line in raw.lines() {
            if !line.contains("app=oo-pg-source")
                || !line.contains(&format!("user={}", self.pointer.reader_user))
            {
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
            *verbs.entry(verb).or_default() += 1;
            if write_word(&strip_sql_literals(sql)).is_some() || has_token(sql, "OFFSET") {
                panic!("reader sql has a forbidden token");
            }
            if sql.contains("pg_terminate_backend") {
                panic!("reader terminated a backend");
            }
        }
        assert!(oo_statements > 0, "reader sql log was empty");
        let verb_text = verbs
            .iter()
            .map(|(verb, count)| format!("{verb}={count}"))
            .collect::<Vec<_>>()
            .join(",");
        self.note(format!(
            "sql_audit oo_statements={oo_statements} dml_ddl_commit_terminate_offset=0 verbs={verb_text}"
        ));
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(path) = &self.outcome_path {
            let _ = fs::write(path, self.outcomes.join("\n") + "\n");
        }
    }
}

struct Cut {
    items: Vec<(i32, String)>,
    lines: Vec<(i32, i32, i32)>,
}

impl Cut {
    fn item_name(&self, id: i32) -> String {
        self.items
            .iter()
            .find(|row| row.0 == id)
            .map(|row| row.1.clone())
            .unwrap_or_default()
    }
}

struct Drained {
    items: Vec<(i32, String)>,
    lines: Vec<(i32, i32, i32)>,
    batches: u64,
    exhausted: bool,
    accounting_executes: u64,
    accounting_consumed: u64,
    accounting_payload: u64,
    wire_view_peak: u64,
    wire_accepted: u64,
}

impl Drained {
    fn item_name(&self, id: i32) -> String {
        self.items
            .iter()
            .find(|row| row.0 == id)
            .map(|row| row.1.clone())
            .unwrap_or_default()
    }
}

async fn drain(owner: &mut SnapshotOwner) -> Drained {
    let mut out = Drained {
        items: Vec::new(),
        lines: Vec::new(),
        batches: 0,
        exhausted: false,
        accounting_executes: 0,
        accounting_consumed: 0,
        accounting_payload: 0,
        wire_view_peak: 0,
        wire_accepted: 0,
    };
    loop {
        match owner.next_batch().await.expect("drain") {
            StreamItem::Batch(batch) => {
                out.batches += 1;
                match batch.typed.table.as_str() {
                    "item" => {
                        let ids = int_column(&batch.record_batch, "id");
                        let names = text_column(&batch.record_batch, "name");
                        assert_eq!(ids.len(), names.len());
                        for (id, name) in ids.into_iter().zip(names) {
                            out.items.push((id, name));
                        }
                    }
                    "line" => {
                        let line_ids = int_column(&batch.record_batch, "line_id");
                        let item_ids = int_column(&batch.record_batch, "item_id");
                        let qtys = int_column(&batch.record_batch, "qty");
                        for ((line_id, item_id), qty) in line_ids.into_iter().zip(item_ids).zip(qtys) {
                            out.lines.push((line_id, item_id, qty));
                        }
                    }
                    other => panic!("unexpected table {other}"),
                }
            }
            StreamItem::TableEof(_) => {}
            StreamItem::Exhausted(accounting) => {
                out.exhausted = true;
                out.accounting_executes = accounting.executes;
                out.accounting_consumed = accounting.consumed_rows;
                out.accounting_payload = accounting.decoded_payload_bytes;
                out.wire_view_peak = accounting.wire_view_capacity_peak;
                out.wire_accepted = accounting.wire_accepted_logical_payload;
                break;
            }
        }
    }
    out
}

fn by_id(rows: &[(i32, String)]) -> Vec<(i32, String)> {
    let mut copy = rows.to_vec();
    copy.sort();
    copy
}

fn by_line(rows: &[(i32, i32, i32)]) -> Vec<(i32, i32, i32)> {
    let mut copy = rows.to_vec();
    copy.sort();
    copy
}

async fn oracle_cut(admin: &mut PgConnection) -> Cut {
    let items = sqlx::query("SELECT id, name FROM r4.item ORDER BY id")
        .fetch_all(&mut *admin)
        .await
        .expect("item oracle");
    let lines = sqlx::query("SELECT line_id, item_id, qty FROM r4.line ORDER BY line_id")
        .fetch_all(&mut *admin)
        .await
        .expect("line oracle");
    Cut {
        items: items
            .iter()
            .map(|row| (row.try_get(0).unwrap(), row.try_get(1).unwrap()))
            .collect(),
        lines: lines
            .iter()
            .map(|row| {
                (
                    row.try_get(0).unwrap(),
                    row.try_get(1).unwrap(),
                    row.try_get(2).unwrap(),
                )
            })
            .collect(),
    }
}

fn finish_label(result: Result<SnapshotFinished, SourceError>) -> String {
    match result {
        Ok(finished) => format!(
            "SnapshotFinished generation={} source_id={} pid={}",
            finished.generation, finished.source_id, finished.backend.pid
        ),
        Err(err) => format!(
            "Err kind={:?} fact={}",
            err.kind(),
            err.fact().unwrap_or("none")
        ),
    }
}

fn int_column(batch: &arrow_array::RecordBatch, name: &str) -> Vec<i32> {
    let column = batch.column_by_name(name).expect("int column");
    let values = column.as_any().downcast_ref::<Int32Array>().expect("int32");
    (0..values.len())
        .map(|index| {
            assert!(!values.is_null(index));
            values.value(index)
        })
        .collect()
}

fn text_column(batch: &arrow_array::RecordBatch, name: &str) -> Vec<String> {
    let column = batch.column_by_name(name).expect("text column");
    let values = column.as_any().downcast_ref::<StringArray>().expect("utf8");
    (0..values.len())
        .map(|index| {
            assert!(!values.is_null(index));
            values.value(index).to_string()
        })
        .collect()
}

async fn load_admitted(
    admin: &mut PgConnection,
    schema: &str,
    tables: &[(&str, bool)],
) -> AdmittedCatalog {
    let mut out = Vec::new();
    for (name, keyed) in tables {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
        .bind(schema)
        .bind(name)
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
        let primary_key = if *keyed {
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
            .bind(name)
            .fetch_all(&mut *admin)
            .await
            .expect("admitted keys");
            keys.iter().map(|row| row.try_get(0).unwrap()).collect()
        } else {
            Vec::new()
        };
        out.push(AdmittedTable {
            schema: schema.to_string(),
            name: (*name).to_string(),
            columns,
            primary_key,
        });
    }
    AdmittedCatalog { tables: out }
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

async fn dial(pointer: &Pointer, user: &str, password: &str, ca: &[u8], app: &str) -> PgConnection {
    let options = PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(user)
        .password(password)
        .database(&pointer.database)
        .application_name(app)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    options.disable_statement_logging().connect().await.expect("dial")
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("password file");
    Zeroizing::new(text.trim().to_string())
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
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
        "GRANT", "REVOKE", "COMMIT",
    ];
    for token in sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        let upper = token.to_ascii_uppercase();
        if let Some(word) = WORDS.iter().copied().find(|word| *word == upper) {
            return Some(word);
        }
    }
    None
}

fn has_token(sql: &str, want: &str) -> bool {
    sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .any(|token| token.eq_ignore_ascii_case(want))
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
