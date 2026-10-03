//! Live R3 cancel and unpublished-run failure on PostgreSQL 17.10.
//! `OO_PG_CANCEL_FIXTURE` is required. A missing pointer is a failure.

use std::cell::Cell;
use std::fs;
use std::pin::pin;
use std::time::{Duration, Instant};

use arrow_array::{Array, Int32Array, RecordBatch};
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, RunCancelled, SecretRef,
    SelectedTable, SnapshotOwner, SnapshotRequest, SourceError, SourceId, StreamBudget,
    StreamItem, TableSelection, CLEANUP_OBSERVATION_BOUND_MS, SNAPSHOT_DEADLINE_DEFAULT,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "cancel_src";
const SCHEMA: &str = "r3c";
const TABLE: &str = "gate";

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
    admitted: AdmittedCatalog,
    outcomes: Vec<String>,
    outcome_path: Option<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_cancel_live() {
    assert_eq!(SNAPSHOT_DEADLINE_DEFAULT, Duration::from_secs(300));
    assert_eq!(CLEANUP_OBSERVATION_BOUND_MS, 5_000);
    let mut world = World::start().await;
    world.seed().await;
    world.phase_before_start().await;
    world.phase_demand_blocked().await;
    world.phase_receiving().await;
    world.phase_drop_future().await;
    world.phase_after_complete().await;
    world.phase_deadline().await;
    world.phase_finish_refuses().await;
    world.phase_killed_source().await;
    world.audit();
    world.note("cancel_live_complete".to_string());
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_CANCEL_FIXTURE").expect("OO_PG_CANCEL_FIXTURE is required");
        let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer"))
            .expect("pointer json");
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
        let outcome_path = std::env::var("OO_PG_CANCEL_OUTCOME").ok();
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
            admitted: AdmittedCatalog { tables: Vec::new() },
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

    fn stream_budget(&self, rows: u32) -> StreamBudget {
        StreamBudget::try_new(rows, 1024 * 1024, 64 * 1024, 64 * 1024).expect("stream budget")
    }

    async fn seed(&mut self) {
        let _ = sqlx::query("ROLLBACK").execute(&mut self.admin).await;
        let reader = quote_ident(&self.pointer.reader_user);
        let writer = quote_ident(&self.pointer.writer_user);
        let ddl = format!(
            "DROP SCHEMA IF EXISTS r3c CASCADE; \
             CREATE SCHEMA r3c; \
             CREATE TABLE r3c.gate (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r3c.gate (id, note) VALUES (1, 'a'), (2, 'b'), (3, 'c'); \
             GRANT USAGE ON SCHEMA r3c TO {reader}, {writer}; \
             GRANT SELECT ON r3c.gate TO {reader}; \
             GRANT INSERT ON r3c.gate TO {writer}"
        );
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(&mut self.admin)
            .await
            .expect("seed");
        self.admitted = load_admitted(&mut self.admin).await;
        self.note("seeded r3c.gate".to_string());
    }

    async fn phase_before_start(&mut self) {
        let owner = self.open(90, 60, 30).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let state = self.state_of(pid, &start).await;
        assert_eq!(state.as_deref(), Some("idle in transaction"));
        let ended: RunCancelled = owner.cancel().await.expect("cancel before start");
        self.note(format!("before_start_generation={}", ended.generation));
        self.wait_clear().await;
    }

    async fn phase_demand_blocked(&mut self) {
        let mut owner = self.open(90, 60, 30).await;
        owner
            .open_stream(self.stream_budget(4))
            .await
            .expect("open stream");
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let state = self.state_of(pid, &start).await;
        assert_eq!(state.as_deref(), Some("idle in transaction"));
        let ended = owner.cancel().await.expect("cancel while demand blocked");
        self.note(format!("demand_blocked_generation={}", ended.generation));
        self.wait_clear().await;
    }

    async fn phase_receiving(&mut self) {
        self.lock_gate().await;
        let mut owner = self.open(90, 60, 30).await;
        let flag = owner.run_cancel();
        owner
            .open_stream(self.stream_budget(4))
            .await
            .expect("open blocked stream");
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let err = self.pull_until_cancel(&mut owner, pid, &start, &flag).await;
        assert_eq!(err.fact(), Some("cancelled"));
        let ended = owner.cancel().await.expect("cancel while receiving");
        self.unlock_gate().await;
        self.note(format!(
            "receiving_fact=cancelled generation={}",
            ended.generation
        ));
        self.wait_clear().await;
    }

    async fn phase_drop_future(&mut self) {
        self.lock_gate().await;
        let mut owner = self.open(90, 60, 30).await;
        owner
            .open_stream(self.stream_budget(4))
            .await
            .expect("open drop stream");
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        {
            let mut pull = pin!(owner.next_batch());
            let saw = self.drive_until_active(&mut pull, pid, &start).await;
            assert!(saw, "backend never became active");
        }
        let still = self.state_of(pid, &start).await;
        assert_eq!(still.as_deref(), Some("active"), "dropped future ended the backend");
        self.note("dropped_future_backend_still_active".to_string());
        let ended = owner.cancel().await.expect("cancel after dropped future");
        self.unlock_gate().await;
        self.note(format!("drop_then_cancel_generation={}", ended.generation));
        self.wait_clear().await;
    }

    async fn phase_after_complete(&mut self) {
        let mut owner = self.open(90, 60, 30).await;
        owner
            .open_stream(self.stream_budget(4))
            .await
            .expect("open complete stream");
        let ids = self.drain_exhausted(&mut owner).await;
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(sorted, vec![1, 2, 3]);
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let state = self.state_of(pid, &start).await;
        assert_eq!(state.as_deref(), Some("idle in transaction"));
        let ended = owner.cancel().await.expect("cancel after complete");
        self.note(format!("after_complete_generation={}", ended.generation));
        self.wait_clear().await;
    }

    async fn phase_deadline(&mut self) {
        self.lock_gate().await;
        let mut owner = self.open(20, 60, 120).await;
        owner
            .open_stream(self.stream_budget(4))
            .await
            .expect("open deadline stream");
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let began = Instant::now();
        let err = {
            let mut pull = pin!(owner.next_batch());
            let saw = self.drive_until_active(&mut pull, pid, &start).await;
            assert!(saw, "deadline pull never became active");
            must_err(pull.await, "deadline")
        };
        let elapsed = began.elapsed();
        assert_eq!(err.fact(), Some("deadline"));
        assert!(elapsed < Duration::from_secs(45), "elapsed {}ms", elapsed.as_millis());
        let ended = owner.cancel().await.expect("cancel after deadline");
        self.unlock_gate().await;
        self.note(format!(
            "deadline_fact=deadline elapsed_ms={} generation={}",
            elapsed.as_millis(),
            ended.generation
        ));
        self.wait_clear().await;
    }

    async fn phase_finish_refuses(&mut self) {
        let owner = self.open(90, 60, 30).await;
        owner.run_cancel().request();
        let err = owner.finish().await.expect_err("finish after request");
        assert_eq!(err.fact(), Some("cancelled"));
        self.note("finish_after_request_fact=cancelled".to_string());
        self.wait_clear().await;
    }

    async fn phase_killed_source(&mut self) {
        let mut owner = self.open(90, 60, 30).await;
        let old_generation = owner.binding().generation;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        owner
            .open_stream(self.stream_budget(1))
            .await
            .expect("open kill stream");
        let partial = self.one_batch(&mut owner).await;
        assert_eq!(partial.len(), 1);
        let terminated = self.terminate_backend(pid, &start).await;
        assert!(terminated, "admin terminate did not match the data backend");
        let failed = must_err(owner.next_batch().await, "read after terminate");
        self.note(format!(
            "killed_next_fact={:?} sqlstate={:?}",
            failed.fact(),
            failed.sqlstate()
        ));
        let finished = owner.finish().await;
        assert!(finished.is_err(), "old run finished after source loss");
        self.note("old_run_not_finished".to_string());
        self.wait_clear().await;

        self.insert_row(4).await;
        let oracle = self.ordered_ids().await;
        assert_eq!(oracle, vec![1, 2, 3, 4]);
        let mut fresh = self.open(90, 60, 30).await;
        fresh
            .open_stream(self.stream_budget(4))
            .await
            .expect("fresh stream");
        let mut got = self.drain_exhausted(&mut fresh).await;
        got.sort();
        assert_eq!(got, oracle);
        assert_ne!(got, partial);
        assert!(!partial.contains(&4));
        let ended = fresh.finish().await.expect("fresh finish");
        assert_ne!(ended.generation, old_generation);
        self.wait_clear().await;
        self.note(format!(
            "old_partial={partial:?} fresh={got:?} fresh_generation={}",
            ended.generation
        ));
    }

    async fn pull_until_cancel(
        &mut self,
        owner: &mut SnapshotOwner,
        pid: i64,
        start: &str,
        flag: &oo_connectors::RunCancel,
    ) -> SourceError {
        let mut pull = pin!(owner.next_batch());
        let saw = self.drive_until_active(&mut pull, pid, start).await;
        assert!(saw, "receiving pull never became active");
        flag.request();
        let limit = Instant::now() + Duration::from_secs(20);
        loop {
            if Instant::now() > limit {
                panic!("cancel did not finish the pull");
            }
            tokio::select! {
                biased;
                result = &mut pull => return must_err(result, "receiving"),
                _ = tokio::time::sleep(Duration::from_millis(30)) => {}
            }
        }
    }

    async fn drive_until_active(
        &mut self,
        pull: &mut std::pin::Pin<&mut impl std::future::Future<Output = Result<StreamItem, SourceError>>>,
        pid: i64,
        start: &str,
    ) -> bool {
        let limit = Instant::now() + Duration::from_secs(15);
        while Instant::now() < limit {
            tokio::select! {
                biased;
                _ = pull.as_mut() => panic!("pull ended before active"),
                _ = tokio::time::sleep(Duration::from_millis(30)) => {}
            }
            if self.state_of(pid, start).await.as_deref() == Some("active") {
                return true;
            }
        }
        let rows = self.reader_rows().await;
        panic!("backend never became active pid={pid} start={start} rows={rows:?}");
    }

    async fn reader_rows(&mut self) -> Vec<String> {
        let rows = sqlx::query(
            "SELECT pid::int8, backend_start::text, COALESCE(state, '(null)'), \
             COALESCE(wait_event_type, ''), COALESCE(wait_event, ''), left(query, 120) \
             FROM pg_catalog.pg_stat_activity \
             WHERE application_name = 'oo-pg-source'",
        )
        .fetch_all(&mut self.admin)
        .await
        .expect("activity");
        rows.iter()
            .map(|row| {
                format!(
                    "pid={} start={} state={} wait={}/{} query={}",
                    row.try_get::<i64, _>(0).unwrap_or(-1),
                    row.try_get::<String, _>(1).unwrap_or_default(),
                    row.try_get::<String, _>(2).unwrap_or_default(),
                    row.try_get::<String, _>(3).unwrap_or_default(),
                    row.try_get::<String, _>(4).unwrap_or_default(),
                    row.try_get::<String, _>(5).unwrap_or_default()
                )
            })
            .collect()
    }

    async fn one_batch(&mut self, owner: &mut SnapshotOwner) -> Vec<i32> {
        match owner.next_batch().await.expect("first batch") {
            StreamItem::Batch(batch) => batch_ids(&batch.record_batch),
            StreamItem::TableEof(_) | StreamItem::Exhausted(_) => panic!("expected a batch"),
        }
    }

    async fn drain_exhausted(&mut self, owner: &mut SnapshotOwner) -> Vec<i32> {
        let mut ids = Vec::new();
        let mut saw_eof = false;
        loop {
            match owner.next_batch().await.expect("drain") {
                StreamItem::Batch(batch) => {
                    assert!(!batch.resumable);
                    ids.extend(batch_ids(&batch.record_batch));
                }
                StreamItem::TableEof(eof) => {
                    assert!(!eof.resumable_key);
                    saw_eof = true;
                }
                StreamItem::Exhausted(_) => {
                    assert!(saw_eof, "exhausted before table eof");
                    return ids;
                }
            }
        }
    }

    async fn open(&mut self, deadline_secs: u64, statement_secs: u64, lock_secs: u64) -> SnapshotOwner {
        let generation = self.next_gen();
        SnapshotOwner::open(self.request(generation, deadline_secs, statement_secs, lock_secs))
            .await
            .expect("open")
    }

    fn request(
        &self,
        generation: u64,
        deadline_secs: u64,
        statement_secs: u64,
        lock_secs: u64,
    ) -> SnapshotRequest {
        let config = self.config(statement_secs, lock_secs);
        let policy = oo_connectors::PgTypePolicy::PRESERVE_ROW;
        SnapshotRequest {
            config_identity: config_identity(&config),
            type_policy_identity: type_policy_identity(policy),
            config,
            admitted: self.admitted.clone(),
            policy,
            generation,
            deadline: Instant::now() + Duration::from_secs(deadline_secs),
            expected_principal: self.pointer.reader_user.clone(),
            expected_database: self.pointer.database.clone(),
            expected_in_recovery: self.in_recovery,
            expected_postmaster_start: self.postmaster.clone(),
        }
    }

    fn config(&self, statement_secs: u64, lock_secs: u64) -> PgSourceConfig {
        PgSourceConfig::try_new(
            SourceId::new(SOURCE).expect("source"),
            PgEndpoint::new(
                &self.pointer.host,
                self.pointer.port,
                &self.pointer.database,
            )
            .expect("endpoint"),
            PgCredentials::new(&self.pointer.reader_user, self.reader_pass.as_str())
                .expect("credentials"),
            SecretRef::new("secret://pg/cancel/reader").expect("secret"),
            PgTlsConfig::from_ca_pem(&self.ca).expect("tls"),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(statement_secs),
                Duration::from_secs(lock_secs),
                128,
                8 * 1024 * 1024,
            )
            .expect("budget"),
            TableSelection::new(vec![SelectedTable::new(SCHEMA, TABLE).expect("table")])
                .expect("selection"),
        )
        .expect("config")
    }

    async fn lock_gate(&mut self) {
        let _ = sqlx::query("ROLLBACK").execute(&mut self.admin).await;
        sqlx::query("BEGIN")
            .execute(&mut self.admin)
            .await
            .expect("lock begin");
        sqlx::query("LOCK TABLE r3c.gate IN ACCESS EXCLUSIVE MODE")
            .execute(&mut self.admin)
            .await
            .expect("lock");
    }

    async fn unlock_gate(&mut self) {
        sqlx::query("ROLLBACK")
            .execute(&mut self.admin)
            .await
            .expect("unlock");
    }

    async fn insert_row(&mut self, id: i32) {
        let mut writer = dial(
            &self.pointer,
            &self.pointer.writer_user,
            &self.writer_pass,
            &self.ca,
            "oo-fixture-writer",
        )
        .await;
        sqlx::query("INSERT INTO r3c.gate (id, note) VALUES ($1, 'new')")
            .bind(id)
            .execute(&mut writer)
            .await
            .expect("writer insert");
        writer.close().await.expect("writer close");
        self.note(format!("writer_inserted id={id}"));
    }

    async fn ordered_ids(&mut self) -> Vec<i32> {
        let rows = sqlx::query("SELECT id FROM r3c.gate ORDER BY id")
            .fetch_all(&mut self.admin)
            .await
            .expect("oracle");
        rows.iter().map(|row| row.try_get(0).expect("id")).collect()
    }

    async fn terminate_backend(&mut self, pid: i64, start: &str) -> bool {
        let pid = i32::try_from(pid).expect("pid");
        let row = sqlx::query(
            "SELECT pg_catalog.pg_terminate_backend(a.pid) \
             FROM pg_catalog.pg_stat_activity a \
             WHERE a.pid = $1::int4 AND a.backend_start::text = $2 \
             LIMIT 1",
        )
        .bind(pid)
        .bind(start)
        .fetch_optional(&mut self.admin)
        .await
        .expect("terminate");
        row.is_some_and(|row| row.try_get::<bool, _>(0).unwrap_or(false))
    }

    async fn state_of(&mut self, pid: i64, start: &str) -> Option<String> {
        let pid = i32::try_from(pid).expect("pid");
        let row = sqlx::query(
            "SELECT state FROM pg_catalog.pg_stat_activity \
             WHERE pid = $1::int4 AND backend_start::text = $2",
        )
        .bind(pid)
        .bind(start)
        .fetch_optional(&mut self.admin)
        .await
        .expect("state");
        row.and_then(|row| row.try_get(0).ok())
    }

    async fn wait_clear(&mut self) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            let left = oo_backends(&mut self.admin, &self.pointer.reader_user).await;
            if left.is_empty() {
                return;
            }
            let busy: Vec<_> = left
                .iter()
                .filter(|(_, _, state)| state == "active" || state == "idle in transaction")
                .collect();
            if began.elapsed() > bound {
                panic!("reader backends still present after {}ms: {left:?} busy={busy:?}", bound.as_millis());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
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
        let mut cancels = 0_usize;
        let mut writer_writes = 0_usize;
        for line in raw.lines() {
            if line.contains("app=oo-fixture-writer")
                && let Some(sql) = statement_sql(line)
                && write_word(&strip_sql_literals(sql)).is_some()
            {
                writer_writes += 1;
            }
            if !line.contains("app=oo-pg-source")
                || !line.contains(&format!("user={}", self.pointer.reader_user))
            {
                continue;
            }
            let Some(sql) = statement_sql(line) else {
                continue;
            };
            oo_statements += 1;
            if write_word(&strip_sql_literals(sql)).is_some() || has_token(sql, "OFFSET") {
                panic!("reader sql has a forbidden token: {sql}");
            }
            if sql.contains("pg_terminate_backend") {
                panic!("reader terminated a backend");
            }
            if sql.contains("pg_cancel_backend") {
                cancels += 1;
            }
        }
        assert!(oo_statements > 0, "reader sql log was empty");
        assert!(cancels > 0, "reader did not cancel a backend");
        assert!(writer_writes > 0, "fixture writer was not logged");
        self.note(format!(
            "sql_audit oo_statements={oo_statements} cancels={cancels} writer_writes={writer_writes} dml_ddl_commit_offset=0"
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

fn must_err(result: Result<StreamItem, SourceError>, label: &str) -> SourceError {
    match result {
        Err(err) => err,
        Ok(_) => panic!("{label} returned a stream item"),
    }
}

fn batch_ids(batch: &RecordBatch) -> Vec<i32> {
    let column = batch.column_by_name("id").expect("id column");
    let values = column.as_any().downcast_ref::<Int32Array>().expect("int32");
    (0..values.len())
        .map(|index| {
            assert!(!values.is_null(index));
            values.value(index)
        })
        .collect()
}

async fn load_admitted(admin: &mut PgConnection) -> AdmittedCatalog {
    let rows = sqlx::query(
        "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'r3c' AND c.relname = 'gate' AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
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
         WHERE n.nspname = 'r3c' AND c.relname = 'gate' AND i.indisprimary \
         ORDER BY u.ord",
    )
    .fetch_all(&mut *admin)
    .await
    .expect("admitted keys");
    AdmittedCatalog {
        tables: vec![AdmittedTable {
            schema: SCHEMA.to_string(),
            name: TABLE.to_string(),
            columns,
            primary_key: keys.iter().map(|row| row.try_get(0).unwrap()).collect(),
        }],
    }
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

async fn oo_backends(admin: &mut PgConnection, user: &str) -> Vec<(i64, String, String)> {
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
    options
        .disable_statement_logging()
        .connect()
        .await
        .expect("dial")
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
