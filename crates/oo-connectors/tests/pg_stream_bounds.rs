//! Small byte and row refusal, and a slow consumer, on PostgreSQL 17.10.
//! One test keeps the order fixed. `OO_PG_STREAM_FIXTURE` is required.

use std::cell::Cell;
use std::fs;
use std::time::{Duration, Instant};

use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SnapshotOwner, SnapshotRequest, SourceError, SourceErrorKind, SourceId, StreamBudget,
    StreamItem, TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "stream_bounds";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
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
}

struct World {
    pointer: Pointer,
    reader_pass: Zeroizing<String>,
    super_pass: Zeroizing<String>,
    ca: Vec<u8>,
    admin: PgConnection,
    generation: Cell<u64>,
    postmaster: String,
    in_recovery: bool,
    outcomes: Vec<String>,
    outcome_path: Option<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_arrow_stream_bounds() {
    let mut world = World::start().await;
    world.note(
        "catalog_max_rows=128 reason=keyword_catalog_returns_about_78_rows_so_open_cannot_use_8"
            .to_string(),
    );
    world.refuse_value().await;
    world.refuse_row().await;
    world.slow_consumer().await;
    world.audit();
    world.note("bounds_complete".to_string());
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
        let (postmaster, in_recovery) = node_of(&mut admin).await;
        let outcome_path = std::env::var("OO_PG_STREAM_BOUNDS_OUTCOME").ok();
        Self {
            pointer,
            reader_pass,
            super_pass,
            ca,
            admin,
            generation: Cell::new(0),
            postmaster,
            in_recovery,
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

    async fn refuse_value(&mut self) {
        let err = self
            .refuse("bounds_wide", StreamBudget::try_new(1, 1_048_576, 4, 1024).expect("budget"))
            .await;
        assert_eq!(err.kind(), SourceErrorKind::Budget);
        assert_eq!(err.fact(), Some("max_value"));
        let shown = err.to_string();
        assert!(shown.starts_with("source error: budget fact=max_value"));
        assert!(!shown.contains("abcdef"), "{shown}");
        self.note(format!("refuse_value fact=max_value display={shown}"));
    }

    async fn refuse_row(&mut self) {
        let err = self
            .refuse("bounds_row", StreamBudget::try_new(1, 1_048_576, 8, 9).expect("budget"))
            .await;
        assert_eq!(err.kind(), SourceErrorKind::Budget);
        assert_eq!(err.fact(), Some("max_row"));
        let shown = err.to_string();
        assert!(shown.starts_with("source error: budget fact=max_row"));
        assert!(!shown.contains('7'), "{shown}");
        self.note(format!("refuse_row fact=max_row display={shown}"));
    }

    async fn refuse(&mut self, table: &str, budget: StreamBudget) -> SourceError {
        let admitted = load_admitted(&mut self.admin, table).await;
        let mut owner = SnapshotOwner::open(self.request(table, admitted))
            .await
            .expect("open");
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let backends = self.oo_backends().await;
        assert_eq!(backends.len(), 2);
        owner.open_stream(budget).await.expect("stream");
        let err = match owner.next_batch().await {
            Err(err) => err,
            Ok(_) => panic!("{table} produced a batch past the receive bound"),
        };
        let Err(finish_err) = owner.finish().await else {
            panic!("finish must not return SnapshotFinished");
        };
        assert_eq!(finish_err.kind(), SourceErrorKind::Budget);
        assert_eq!(finish_err.fact(), err.fact());
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
        assert!(self.oo_backends().await.is_empty());
        self.note(format!("refuse_released table={table} pid={pid}"));
        err
    }

    async fn slow_consumer(&mut self) {
        let admitted = load_admitted(&mut self.admin, "parent").await;
        let mut owner = SnapshotOwner::open(self.request("parent", admitted))
            .await
            .expect("slow open");
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        let pid_i = i32::try_from(pid).expect("pid");
        let backends = self.oo_backends().await;
        owner
            .open_stream(StreamBudget::try_new(1, 1_048_576, 1024, 8192).expect("budget"))
            .await
            .expect("slow stream");
        let first = owner.next_batch().await.expect("first row");
        let StreamItem::Batch(batch) = first else {
            panic!("slow consumer expected a row batch");
        };
        assert_eq!(batch.record_batch.num_rows(), 1);
        let executes = batch.accounting.executes;
        self.hold(pid_i, &start).await;
        let before = oo_statements(&fs::read_to_string(&self.pointer.log_file).expect("log"));
        tokio::time::sleep(Duration::from_millis(400)).await;
        match activity(&mut self.admin, pid_i, &start).await {
            Some(row) => {
                assert!(row.in_xact);
                assert_eq!(row.app, "oo-pg-source");
                assert!(
                    waiting_on_client(&row),
                    "backend was executing during the idle consumer: state={} wait={}",
                    row.state,
                    row.wait_event
                );
                self.note(format!(
                    "slow_wait state={} wait_event={}",
                    row.state, row.wait_event
                ));
            }
            None => panic!("backend left while the consumer was idle"),
        }
        let during = oo_statements(&fs::read_to_string(&self.pointer.log_file).expect("log"));
        assert_eq!(during, before, "an Execute was logged while the consumer was idle");
        let second = owner.next_batch().await.expect("second demand");
        let after_executes = match &second {
            StreamItem::Batch(batch) => batch.accounting.executes,
            StreamItem::TableEof(eof) => eof.accounting.executes,
            StreamItem::Exhausted(accounting) => accounting.executes,
        };
        assert_eq!(after_executes, executes + 1);
        tokio::time::sleep(Duration::from_millis(200)).await;
        let after = oo_statements(&fs::read_to_string(&self.pointer.log_file).expect("log"));
        self.note(format!(
            "slow_consumer executes_before={executes} executes_after={after_executes} log_lines_during_sleep={during} log_lines_after_demand={after} log_delta_after_demand={}",
            after.saturating_sub(during)
        ));
        let generation = owner.binding().generation;
        let finished = owner.finish().await.expect("slow finish");
        assert_eq!(finished.generation, generation);
        self.wait_gone(pid, &start).await;
        for (guard_pid, guard_start) in &backends {
            self.wait_gone(*guard_pid, guard_start).await;
        }
        assert!(self.oo_backends().await.is_empty());
        self.note(format!("slow_released pid={pid}"));
    }

    fn request(&self, table: &str, admitted: AdmittedCatalog) -> SnapshotRequest {
        let config = PgSourceConfig::try_new(
            SourceId::new(SOURCE).unwrap(),
            PgEndpoint::new(
                &self.pointer.host,
                self.pointer.port,
                &self.pointer.database,
            )
            .unwrap(),
            PgCredentials::new(&self.pointer.reader_user, &*self.reader_pass).unwrap(),
            SecretRef::new("secret://pg/stream/bounds").unwrap(),
            PgTlsConfig::from_ca_pem(&self.ca).unwrap(),
            PgBudget::new(
                Duration::from_secs(30),
                Duration::from_secs(60),
                Duration::from_secs(5),
                128,
                1024 * 1024,
            )
            .unwrap(),
            TableSelection::new(vec![SelectedTable::new("stream", table).unwrap()]).unwrap(),
        )
        .unwrap();
        let policy = oo_connectors::PgTypePolicy::PRESERVE_ROW;
        SnapshotRequest {
            config_identity: config_identity(&config),
            type_policy_identity: type_policy_identity(policy),
            config,
            admitted,
            policy,
            generation: self.next_gen(),
            deadline: Instant::now() + Duration::from_secs(90),
            expected_principal: self.pointer.reader_user.clone(),
            expected_database: self.pointer.database.clone(),
            expected_in_recovery: self.in_recovery,
            expected_postmaster_start: self.postmaster.clone(),
        }
    }

    async fn hold(&mut self, pid: i32, start: &str) {
        let began = Instant::now();
        loop {
            match activity(&mut self.admin, pid, start).await {
                Some(row) if row.in_xact && row.app == "oo-pg-source" && waiting_on_client(&row) => {
                    return;
                }
                Some(row) if began.elapsed() > Duration::from_secs(2) => {
                    panic!("backend {pid} stayed {}", row.state);
                }
                None if began.elapsed() > Duration::from_secs(2) => {
                    panic!("backend {pid} missing");
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
            .map(|row| (row.try_get(0).expect("pid"), row.try_get(1).expect("start")))
            .collect()
    }

    fn audit(&mut self) {
        let raw = fs::read_to_string(&self.pointer.log_file).expect("postgres log");
        for secret in [self.reader_pass.as_str(), self.super_pass.as_str()] {
            if !secret.is_empty() && raw.contains(secret) {
                panic!("postgres log contains a closed string");
            }
        }
        let mut oo_statements = 0_usize;
        for line in raw.lines() {
            if !line.contains("app=oo-pg-source") || !line.contains("user=oo_a131_reader") {
                continue;
            }
            let Some(sql) = statement_sql(line) else {
                continue;
            };
            oo_statements += 1;
            if write_word(&strip_sql_literals(sql)).is_some() {
                panic!("oo statement contains a write verb");
            }
        }
        assert!(oo_statements > 0, "reader sql log was empty");
        self.note(format!(
            "sql_audit oo_statements={oo_statements} dml_ddl_commit=0"
        ));
    }
}

fn oo_statements(raw: &str) -> usize {
    raw.lines()
        .filter(|line| {
            line.contains("app=oo-pg-source")
                && line.contains("user=oo_a131_reader")
                && statement_sql(line).is_some()
        })
        .count()
}

async fn load_admitted(admin: &mut PgConnection, name: &str) -> AdmittedCatalog {
    let rows = sqlx::query(
        "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'stream' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
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
    .bind(name)
    .fetch_all(&mut *admin)
    .await
    .expect("admitted keys");
    let primary_key = keys
        .iter()
        .map(|row| row.try_get::<String, _>(0).unwrap())
        .collect();
    AdmittedCatalog {
        tables: vec![AdmittedTable {
            schema: "stream".to_string(),
            name: name.to_string(),
            columns,
            primary_key,
        }],
    }
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
         application_name \
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
    })
}

fn waiting_on_client(row: &Activity) -> bool {
    row.wait_event == "ClientRead" || row.state == "idle in transaction"
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
