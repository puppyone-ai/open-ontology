//! Admission is `SnapshotOwner::finish_complete`, not `finish`.
//! `OO_PG_COMPLETE_FIXTURE` is required. A missing pointer is a failure.
//!
//! `SnapshotFinished` stays the lifecycle close. The four early stops return
//! `protocol` fact `not_exhausted` and both owned backends leave a current
//! `pg_stat_activity` sample. A full drain of two selected tables, including
//! `TableEof` and `Exhausted`, is compared to the pre-writer cut. Only that
//! path mints `RunComplete`. Cancel and a bound-portal failure do not.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, Instant};

use arrow_array::{Array, Int32Array, StringArray};
use oo_connectors::{
    config_identity, connector_capabilities, type_capabilities, type_policy_identity,
    AdmittedCatalog, AdmittedColumn, AdmittedTable, PgBudget, PgCredentials, PgEndpoint,
    PgSourceConfig, PgTlsConfig, PgTypePolicy, RunComplete, SecretRef, SelectedTable,
    SnapshotFinished, SnapshotOwner, SnapshotRequest, SourceCatalog, SourceError, SourceErrorKind,
    SourceId, StreamBudget, StreamItem, TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "complete_src";

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
async fn finish_complete_admits_only_exhausted_delivery() {
    assert_eq!(CLEANUP_OBSERVATION_BOUND_MS, 5_000);
    let mut world = World::start().await;
    world.note_capabilities();
    world.seed().await;
    world.phase_longitudinal().await;
    world.phase_lifecycle_finish().await;
    world.phase_finish_idle().await;
    world.phase_finish_before_poll().await;
    world.phase_finish_after_one_batch().await;
    world.phase_missing_selected_table().await;
    world.phase_cancel().await;
    world.phase_portal_bound().await;
    world.audit();
    world.note("complete_live_ok".to_string());
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_COMPLETE_FIXTURE").expect("OO_PG_COMPLETE_FIXTURE is required");
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
            outcome_path: std::env::var("OO_PG_COMPLETE_OUTCOME").ok(),
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
            assert!(!cap.supported, "capability {} was advertised", cap.name);
            if cap.name == "scan" || cap.name == "bounded_read" {
                self.note(format!(
                    "connector_capability name={} supported={} reason={}",
                    cap.name, cap.supported, cap.reason
                ));
            }
        }
        for cap in type_capabilities() {
            if cap.name == "bounded_reader" {
                assert!(!cap.supported, "bounded_reader was advertised");
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
        let source_id = owner.binding().source_id.clone();
        let config_identity = owner.binding().config_identity;
        let type_policy_identity = owner.binding().type_policy_identity;
        let input_digest = owner.binding().input_digest;
        let schema_hash = owner.binding().schema_hash.clone();
        let catalog = owner.binding().catalog.clone();
        let pid = owner.binding().backend.pid;
        assert!(
            catalog_unconverted(&catalog),
            "a selected column was marked converted"
        );
        owner
            .open_stream(StreamBudget::try_new(1, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("open stream");
        let owned = self.reader_backends().await;
        assert!(owned.len() >= 2, "expected data and guard, got {owned:?}");
        self.writer_mutates().await;
        let outside = oracle_cut(&mut self.admin).await;
        assert_eq!(outside.item_name(1), "pine", "writer commit was not visible outside");
        assert!(
            outside.lines.iter().any(|row| row.0 == 13),
            "new line was not visible outside"
        );
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
        assert!(read.saw_item_eof && read.saw_line_eof, "a selected table missed TableEof");
        assert!(read.batches >= 5, "batch_rows=1 did not split the cut");
        self.note(format!(
            "full_read generation={generation} data_pid={pid} batches={} item_rows={} line_rows={} exhausted=1 table_eof=item,line same_cut_match=1",
            read.batches,
            read.items.len(),
            read.lines.len()
        ));
        let token = owner.finish_complete().await.expect("exhausted admission");
        let began = Instant::now();
        assert_eq!(token.source_id, source_id);
        assert_eq!(token.generation, generation);
        assert_eq!(token.config_identity, config_identity);
        assert_eq!(token.type_policy_identity, type_policy_identity);
        assert_eq!(token.policy, PgTypePolicy::PRESERVE_ROW);
        assert_eq!(token.input_digest, input_digest);
        assert_eq!(token.schema_hash, schema_hash);
        assert_eq!(token.catalog, catalog);
        assert_eq!(token.selected.len(), 2);
        assert_eq!(token.selected[0].schema(), "r4");
        assert_eq!(token.selected[0].table(), "item");
        assert_eq!(token.selected[1].schema(), "r4");
        assert_eq!(token.selected[1].table(), "line");
        assert!(catalog_unconverted(&token.catalog));
        self.wait_absent(&owned, began, "full_admit").await;
        self.note(format!(
            "full_admit {} digest={}",
            complete_label(&token),
            hex32(&token.input_digest)
        ));

        let admitted = load_admitted(&mut self.admin, "r4", &[("item", true), ("line", false)]).await;
        let mut fresh = self
            .open(&admitted, &[("r4", "item"), ("r4", "line")], 90, 60, 30)
            .await;
        let fresh_generation = fresh.binding().generation;
        let fresh_digest = fresh.binding().input_digest;
        assert_ne!(fresh_generation, generation);
        assert_ne!(fresh_digest, input_digest);
        fresh
            .open_stream(StreamBudget::try_new(8, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("fresh stream");
        let fresh_owned = self.reader_backends().await;
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
        let fresh_token = fresh.finish_complete().await.expect("fresh admission");
        assert_eq!(fresh_token.generation, fresh_generation);
        assert_ne!(fresh_token.generation, generation);
        assert_eq!(fresh_token.input_digest, fresh_digest);
        assert_ne!(fresh_token.input_digest, input_digest);
        self.wait_absent(&fresh_owned, Instant::now(), "fresh_admit").await;
        let item_scan = again
            .items
            .iter()
            .map(|(id, name)| format!("{id}:{name}"))
            .collect::<Vec<_>>()
            .join(",");
        self.note(format!(
            "fresh_generation={fresh_generation} fresh_item1=pine fresh_lines={} fresh_has_13=1 fresh_item_scan={item_scan} cut_compared_by_key=1 old_generation_reused=0 {}",
            again.lines.len(),
            complete_label(&fresh_token)
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

    async fn phase_lifecycle_finish(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4e", &[("rows", true)]).await;
        let owner = self.open(&admitted, &[("r4e", "rows")], 90, 60, 30).await;
        let generation = owner.binding().generation;
        let owned = self.reader_backends().await;
        assert!(owned.len() >= 2, "expected data and guard, got {owned:?}");
        let finished = owner.finish().await.expect("lifecycle close");
        let began = Instant::now();
        assert_eq!(finished.generation, generation);
        assert_eq!(finished.source_id, SOURCE);
        self.wait_absent(&owned, began, "lifecycle_finish").await;
        self.note(format!(
            "lifecycle_finish {} admission=0 close_only=1",
            finish_label(&finished)
        ));
    }

    async fn phase_finish_idle(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4e", &[("rows", true)]).await;
        let owner = self.open(&admitted, &[("r4e", "rows")], 90, 60, 30).await;
        let generation = owner.binding().generation;
        self.expect_not_exhausted(owner, generation, "early_idle", "rows_read=0 exhausted_seen=0 selected=r4e.rows")
            .await;
    }

    async fn phase_finish_before_poll(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4e", &[("rows", true)]).await;
        let mut owner = self.open(&admitted, &[("r4e", "rows")], 90, 60, 30).await;
        let generation = owner.binding().generation;
        owner
            .open_stream(StreamBudget::try_new(2, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("open stream");
        self.expect_not_exhausted(
            owner,
            generation,
            "early_before_poll",
            "rows_read=0 exhausted_seen=0 selected=r4e.rows",
        )
        .await;
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
        let rows_in_table: i64 = sqlx::query("SELECT count(*)::int8 FROM r4e.rows")
            .fetch_one(&mut self.admin)
            .await
            .expect("row count")
            .try_get(0)
            .expect("count");
        assert_eq!(rows_in_table, 4);
        let detail = format!(
            "rows_read=2 rows_in_table={rows_in_table} exhausted_seen=0 ids={}",
            ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")
        );
        self.expect_not_exhausted(owner, generation, "early_after_one_batch", &detail)
            .await;
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
        let first_eof = loop {
            match owner.next_batch().await.expect("until first eof") {
                StreamItem::Batch(batch) => {
                    *saw.entry(batch.typed.table.clone()).or_default() +=
                        batch.record_batch.num_rows() as u64;
                }
                StreamItem::TableEof(eof) => break eof.table,
                StreamItem::Exhausted(_) => panic!("both tables ended before a stop"),
            }
        };
        let unread: Vec<&str> = ["rows", "extra"]
            .into_iter()
            .filter(|name| *name != first_eof && !saw.contains_key(*name))
            .collect();
        assert_eq!(unread.len(), 1, "one selected table should still be unread");
        let detail = format!(
            "first_eof={first_eof} unread={} rows_of_first={} exhausted_seen=0",
            unread[0],
            saw.get(&first_eof).copied().unwrap_or(0)
        );
        self.expect_not_exhausted(owner, generation, "missing_selected_table", &detail)
            .await;
    }

    async fn phase_cancel(&mut self) {
        let admitted = load_admitted(&mut self.admin, "r4e", &[("rows", true)]).await;
        let owner = self.open(&admitted, &[("r4e", "rows")], 90, 60, 30).await;
        let generation = owner.binding().generation;
        let owned = self.reader_backends().await;
        assert!(owned.len() >= 2, "expected data and guard, got {owned:?}");
        owner.run_cancel().request();
        let err = owner
            .finish_complete()
            .await
            .expect_err("cancel must not admit");
        let began = Instant::now();
        assert_eq!(err.kind(), SourceErrorKind::Protocol);
        assert_eq!(err.fact(), Some("cancelled"));
        self.wait_absent(&owned, began, "cancel").await;
        self.note(format!(
            "cancel generation={generation} finish_complete=Err fact=cancelled run_complete=0"
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
        let owned = self.reader_backends().await;
        assert!(owned.len() >= 2, "expected data and guard, got {owned:?}");
        let err = match owner.next_batch().await {
            Err(err) => err,
            Ok(StreamItem::Batch(_)) => panic!("wide value produced a batch"),
            Ok(StreamItem::TableEof(_)) => panic!("wide value produced table eof"),
            Ok(StreamItem::Exhausted(_)) => panic!("wide value produced exhausted"),
        };
        assert_eq!(err.kind(), SourceErrorKind::Budget);
        assert_eq!(err.fact(), Some("max_value"));
        let finished = owner.finish_complete().await;
        let began = Instant::now();
        match &finished {
            Err(err) => {
                assert_eq!(err.kind(), SourceErrorKind::Budget);
                assert_eq!(err.fact(), Some("max_value"));
            }
            Ok(_) => panic!("portal bound minted RunComplete"),
        }
        self.wait_absent(&owned, began, "portal_bound").await;
        self.note(format!(
            "portal_bound generation={generation} data_pid={pid} next_fact=max_value finish_complete={} run_complete=0",
            error_label(&finished.expect_err("budget"))
        ));
    }

    async fn expect_not_exhausted(
        &mut self,
        owner: SnapshotOwner,
        generation: u64,
        label: &str,
        detail: &str,
    ) {
        let owned = self.reader_backends().await;
        assert!(
            owned.len() >= 2,
            "{label} expected data and guard, got {owned:?}"
        );
        let err = owner.finish_complete().await.expect_err(label);
        let began = Instant::now();
        assert_eq!(err.kind(), SourceErrorKind::Protocol, "{label}");
        assert_eq!(err.fact(), Some("not_exhausted"), "{label}");
        self.wait_absent(&owned, began, label).await;
        self.note(format!(
            "{label} generation={generation} {detail} finish_complete=not_exhausted backends_gone=2"
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
        SnapshotOwner::open(self.request(
            admitted,
            tables,
            generation,
            deadline_secs,
            statement_secs,
            lock_secs,
        ))
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
            SecretRef::new("secret://pg/complete/reader").expect("secret"),
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
        let policy = PgTypePolicy::PRESERVE_ROW;
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

    async fn refresh_observation(&mut self) {
        sqlx::query("BEGIN")
            .execute(&mut self.admin)
            .await
            .expect("observe begin");
        sqlx::query("SELECT pg_catalog.pg_stat_clear_snapshot()")
            .execute(&mut self.admin)
            .await
            .expect("clear stat snapshot");
    }

    async fn end_observation(&mut self) {
        sqlx::query("ROLLBACK")
            .execute(&mut self.admin)
            .await
            .expect("observe rollback");
    }

    async fn reader_backends(&mut self) -> Vec<(i64, String, String)> {
        self.refresh_observation().await;
        let rows = backends_of(&mut self.admin, &self.pointer.reader_user).await;
        self.end_observation().await;
        rows
    }

    async fn wait_absent(&mut self, before: &[(i64, String, String)], began: Instant, label: &str) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let mut noted_first = false;
        loop {
            let left = self.reader_backends().await;
            let still: Vec<_> = before
                .iter()
                .filter(|row| left.iter().any(|live| live.0 == row.0 && live.1 == row.1))
                .collect();
            if !noted_first {
                self.note(format!(
                    "{label}_first_sample_ms={} still={}",
                    began.elapsed().as_millis(),
                    still.len()
                ));
                noted_first = true;
            }
            if still.is_empty() {
                self.note(format!(
                    "{label}_absent_ms={}",
                    began.elapsed().as_millis()
                ));
                return;
            }
            if began.elapsed() > bound {
                panic!(
                    "{label} backends remained after {}ms still={still:?}",
                    bound.as_millis()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
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
    saw_item_eof: bool,
    saw_line_eof: bool,
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
        saw_item_eof: false,
        saw_line_eof: false,
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
                        for ((line_id, item_id), qty) in line_ids.into_iter().zip(item_ids).zip(qtys)
                        {
                            out.lines.push((line_id, item_id, qty));
                        }
                    }
                    other => panic!("unexpected table {other}"),
                }
            }
            StreamItem::TableEof(eof) => {
                if eof.table == "item" {
                    out.saw_item_eof = true;
                }
                if eof.table == "line" {
                    out.saw_line_eof = true;
                }
            }
            StreamItem::Exhausted(_) => {
                out.exhausted = true;
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

fn catalog_unconverted(catalog: &SourceCatalog) -> bool {
    catalog.tables.iter().all(|table| {
        table
            .columns
            .iter()
            .all(|column| !column.type_support.converted)
    })
}

fn complete_label(token: &RunComplete) -> String {
    let tables = token
        .selected
        .iter()
        .map(|table| format!("{}.{}", table.schema(), table.table()))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "RunComplete generation={} source_id={} tables={tables} schema_hash={}",
        token.generation, token.source_id, token.schema_hash
    )
}

fn finish_label(finished: &SnapshotFinished) -> String {
    format!(
        "SnapshotFinished generation={} source_id={} pid={}",
        finished.generation, finished.source_id, finished.backend.pid
    )
}

fn error_label(err: &SourceError) -> String {
    format!(
        "Err kind={:?} fact={}",
        err.kind(),
        err.fact().unwrap_or("none")
    )
}

fn hex32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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

async fn backends_of(admin: &mut PgConnection, user: &str) -> Vec<(i64, String, String)> {
    let rows = sqlx::query(
        "SELECT pid::int8, backend_start::text, COALESCE(state, '') \
         FROM pg_catalog.pg_stat_activity \
         WHERE application_name = 'oo-pg-source' AND usename::text = $1",
    )
    .bind(user)
    .fetch_all(&mut *admin)
    .await
    .expect("backends");
    rows.iter()
        .map(|row| {
            (
                row.try_get(0).expect("pid"),
                row.try_get(1).expect("start"),
                row.try_get(2).expect("state"),
            )
        })
        .collect()
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
