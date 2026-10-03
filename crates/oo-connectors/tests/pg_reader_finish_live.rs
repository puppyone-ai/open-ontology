//! Finish truth on one PostgreSQL 17.10 owner.
//! `OO_PG_FINISH_FIXTURE` is required. A missing pointer is a failure.
//!
//! Three public paths: a drained read finishes, an explicit cancel does not
//! finish, and a receive-bound portal fault does not finish. Fixture DDL stays
//! on the admin session.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, Instant};

use arrow_array::{Array, Int32Array, RecordBatch};
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SnapshotOwner, SnapshotRequest, SourceErrorKind, SourceId, StreamBudget,
    StreamItem, TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "finish_src";
const SCHEMA: &str = "r3f";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    #[allow(dead_code)]
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
async fn finish_does_not_report_a_failed_read() {
    assert_eq!(CLEANUP_OBSERVATION_BOUND_MS, 5_000);
    let mut world = World::start().await;
    world.seed().await;
    world.phase_normal_eof().await;
    world.phase_explicit_cancel().await;
    world.phase_portal_bound().await;
    world.audit();
    world.note("finish_live_complete".to_string());
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_FINISH_FIXTURE").expect("OO_PG_FINISH_FIXTURE is required");
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
        let outcome_path = std::env::var("OO_PG_FINISH_OUTCOME").ok();
        let world = Self {
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
            outcome_path,
            log_mark,
        };
        world.note_mark();
        world
    }

    fn note_mark(&self) {
        if let Some(path) = &self.outcome_path {
            let _ = fs::write(path, format!("log_mark={}\n", self.log_mark));
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

    async fn seed(&mut self) {
        let _ = sqlx::query("ROLLBACK").execute(&mut self.admin).await;
        let reader = quote_ident(&self.pointer.reader_user);
        let ddl = format!(
            "DROP SCHEMA IF EXISTS r3f CASCADE; \
             CREATE SCHEMA r3f; \
             CREATE TABLE r3f.rows (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r3f.rows (id, note) VALUES (1, 'a'), (2, 'b'), (3, 'c'); \
             CREATE TABLE r3f.wide (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r3f.wide (id, note) VALUES (1, 'abcdefghij'); \
             GRANT USAGE ON SCHEMA r3f TO {reader}; \
             GRANT SELECT ON r3f.rows, r3f.wide TO {reader}"
        );
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(&mut self.admin)
            .await
            .expect("seed");
        self.note(format!("seeded r3f.rows r3f.wide log_mark={}", self.log_mark));
    }

    async fn phase_normal_eof(&mut self) {
        let admitted = load_admitted(&mut self.admin, "rows").await;
        let mut owner = self.open(&admitted, 90, 60, 30).await;
        let generation = owner.binding().generation;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        owner
            .open_stream(StreamBudget::try_new(8, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("open stream");
        let before = self.reader_backends().await;
        assert!(before.len() >= 2, "expected data and guard, got {before:?}");
        let ids = drain_ids(&mut owner).await;
        assert_eq!(ids, vec![1, 2, 3]);
        let began = Instant::now();
        let finished = owner.finish().await.expect("drained finish");
        assert_eq!(finished.generation, generation);
        assert_eq!(finished.source_id, SOURCE);
        assert_eq!(finished.backend.pid, pid);
        self.wait_absent(&before, began, "normal").await;
        self.note(format!(
            "normal_finish=SnapshotFinished generation={generation} ids=1,2,3 data_pid={pid} data_start={start}"
        ));

        let admitted = load_admitted(&mut self.admin, "rows").await;
        let mut fresh = self.open(&admitted, 90, 60, 30).await;
        let fresh_generation = fresh.binding().generation;
        assert_ne!(fresh_generation, generation);
        fresh
            .open_stream(StreamBudget::try_new(8, 1024 * 1024, 64 * 1024, 64 * 1024).expect("budget"))
            .await
            .expect("fresh stream");
        let fresh_before = self.reader_backends().await;
        let again = drain_ids(&mut fresh).await;
        assert_eq!(again, ids, "a later generation changed the committed cut");
        let began = Instant::now();
        let finished = fresh.finish().await.expect("fresh finish");
        assert_eq!(finished.generation, fresh_generation);
        self.wait_absent(&fresh_before, began, "fresh").await;
        self.note(format!(
            "same_cut_ids=1,2,3 fresh_generation={fresh_generation} snapshot_finished=1"
        ));
    }

    async fn phase_explicit_cancel(&mut self) {
        let admitted = load_admitted(&mut self.admin, "rows").await;
        let owner = self.open(&admitted, 90, 60, 30).await;
        let before = self.reader_backends().await;
        assert!(before.len() >= 2, "expected data and guard, got {before:?}");
        owner.run_cancel().request();
        let began = Instant::now();
        let err = owner.finish().await.expect_err("finish after cancel request");
        assert_eq!(err.kind(), SourceErrorKind::Protocol);
        assert_eq!(err.fact(), Some("cancelled"));
        self.wait_absent(&before, began, "cancel").await;
        self.note("cancel_finish_fact=cancelled snapshot_finished=0".to_string());
    }

    async fn phase_portal_bound(&mut self) {
        let admitted = load_admitted(&mut self.admin, "wide").await;
        let mut owner = self.open(&admitted, 90, 60, 30).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        owner
            .open_stream(StreamBudget::try_new(1, 1_048_576, 4, 1024).expect("tight budget"))
            .await
            .expect("open wide stream");
        let before = self.reader_backends().await;
        assert!(before.len() >= 2, "expected data and guard, got {before:?}");
        let err = match owner.next_batch().await {
            Err(err) => err,
            Ok(StreamItem::Batch(_)) => panic!("wide value produced a batch"),
            Ok(StreamItem::TableEof(_)) => panic!("wide value produced table eof"),
            Ok(StreamItem::Exhausted(_)) => panic!("wide value produced exhausted"),
        };
        assert_eq!(err.kind(), SourceErrorKind::Budget);
        assert_eq!(err.fact(), Some("max_value"));
        let began = Instant::now();
        let finished = owner.finish().await;
        let fact = match &finished {
            Ok(_) => "SnapshotFinished".to_string(),
            Err(err) => err.fact().unwrap_or("none").to_string(),
        };
        assert!(
            finished.is_err(),
            "portal bound finish returned SnapshotFinished"
        );
        let finish_err = finished.expect_err("portal bound finish");
        assert_eq!(finish_err.kind(), SourceErrorKind::Budget);
        assert_eq!(finish_err.fact(), Some("max_value"));
        self.wait_absent(&before, began, "bound").await;
        let (rollback_lines, reset_lines) = self.pid_close_lines(pid);
        self.note(format!(
            "bound_next_fact=max_value bound_finish_fact={fact} snapshot_finished=0 data_pid={pid} data_start={start} reader_rollback_lines={rollback_lines} reader_reset_lines={reset_lines}"
        ));
    }

    async fn open(
        &mut self,
        admitted: &AdmittedCatalog,
        deadline_secs: u64,
        statement_secs: u64,
        lock_secs: u64,
    ) -> SnapshotOwner {
        let generation = self.next_gen();
        let table = admitted.tables[0].name.clone();
        SnapshotOwner::open(self.request(admitted, &table, generation, deadline_secs, statement_secs, lock_secs))
            .await
            .expect("open")
    }

    fn request(
        &self,
        admitted: &AdmittedCatalog,
        table: &str,
        generation: u64,
        deadline_secs: u64,
        statement_secs: u64,
        lock_secs: u64,
    ) -> SnapshotRequest {
        let config = PgSourceConfig::try_new(
            SourceId::new(SOURCE).expect("source"),
            PgEndpoint::new(&self.pointer.host, self.pointer.port, &self.pointer.database)
                .expect("endpoint"),
            PgCredentials::new(&self.pointer.reader_user, self.reader_pass.as_str())
                .expect("credentials"),
            SecretRef::new("secret://pg/finish/reader").expect("secret"),
            PgTlsConfig::from_ca_pem(&self.ca).expect("tls"),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(statement_secs),
                Duration::from_secs(lock_secs),
                128,
                8 * 1024 * 1024,
            )
            .expect("budget"),
            TableSelection::new(vec![SelectedTable::new(SCHEMA, table).expect("table")])
                .expect("selection"),
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

    fn pid_close_lines(&self, pid: i64) -> (usize, usize) {
        let raw_all = fs::read(&self.pointer.log_file).unwrap_or_default();
        let mark = usize::try_from(self.log_mark).unwrap_or(0).min(raw_all.len());
        let raw = String::from_utf8_lossy(&raw_all[mark..]);
        let needle = format!("pid={pid} ");
        let mut rollback_lines = 0_usize;
        let mut reset_lines = 0_usize;
        for line in raw.lines() {
            if !line.contains(&needle) || !line.contains("app=oo-pg-source") {
                continue;
            }
            if line.contains("could not receive data from client") {
                reset_lines += 1;
            }
            if let Some(sql) = statement_sql(line) {
                let verb = sql.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
                if verb == "ROLLBACK" {
                    rollback_lines += 1;
                }
            }
        }
        (rollback_lines, reset_lines)
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
            "sql_audit oo_statements={oo_statements} dml_ddl_commit_terminate=0 verbs={verb_text}"
        ));
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(path) = &self.outcome_path {
            let mut lines = vec![format!("log_mark={}", self.log_mark)];
            lines.extend(self.outcomes.iter().cloned());
            let _ = fs::write(path, lines.join("\n") + "\n");
        }
    }
}

async fn drain_ids(owner: &mut SnapshotOwner) -> Vec<i32> {
    let mut ids = Vec::new();
    loop {
        match owner.next_batch().await.expect("next batch") {
            StreamItem::Batch(batch) => ids.extend(batch_ids(&batch.record_batch)),
            StreamItem::TableEof(_) => {}
            StreamItem::Exhausted(_) => break,
        }
    }
    ids
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

async fn load_admitted(admin: &mut PgConnection, table: &str) -> AdmittedCatalog {
    let rows = sqlx::query(
        "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'r3f' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .bind(table)
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
         WHERE n.nspname = 'r3f' AND c.relname = $1 AND i.indisprimary \
         ORDER BY u.ord",
    )
    .bind(table)
    .fetch_all(&mut *admin)
    .await
    .expect("admitted keys");
    AdmittedCatalog {
        tables: vec![AdmittedTable {
            schema: SCHEMA.to_string(),
            name: table.to_string(),
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
