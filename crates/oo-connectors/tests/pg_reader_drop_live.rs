//! Owner Drop while the data backend is blocked on `ACCESS EXCLUSIVE`.
//! `OO_PG_DROP_FIXTURE` is required. A missing pointer is a failure.
//! This is the a139 owner-Drop failure and nothing else.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::pin::pin;
use std::time::{Duration, Instant};

use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, SecretRef, SelectedTable,
    SnapshotOwner, SnapshotRequest, SourceError, SourceId, StreamBudget, StreamItem,
    TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "drop_src";
const SCHEMA: &str = "r3d";
const TABLE: &str = "held";

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
    admitted: AdmittedCatalog,
    outcomes: Vec<String>,
    outcome_path: Option<String>,
    log_mark: u64,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_drop_while_blocked() {
    assert_eq!(CLEANUP_OBSERVATION_BOUND_MS, 5_000);
    let mut world = World::start().await;
    world.seed().await;
    world.phase_owner_drop().await;
    world.audit();
    world.note("owner_drop_live_complete".to_string());
}

impl World {
    async fn start() -> Self {
        let path = std::env::var("OO_PG_DROP_FIXTURE").expect("OO_PG_DROP_FIXTURE is required");
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
        let outcome_path = std::env::var("OO_PG_DROP_OUTCOME").ok();
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
            admitted: AdmittedCatalog { tables: Vec::new() },
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

    fn stream_budget(&self) -> StreamBudget {
        StreamBudget::try_new(8, 1024 * 1024, 64 * 1024, 64 * 1024).expect("stream budget")
    }

    async fn seed(&mut self) {
        let _ = sqlx::query("ROLLBACK").execute(&mut self.admin).await;
        let reader = quote_ident(&self.pointer.reader_user);
        let ddl = format!(
            "DROP SCHEMA IF EXISTS r3d CASCADE; \
             CREATE SCHEMA r3d; \
             CREATE TABLE r3d.held (id int PRIMARY KEY, note text NOT NULL); \
             INSERT INTO r3d.held (id, note) VALUES (1, 'a'), (2, 'b'), (3, 'c'); \
             GRANT USAGE ON SCHEMA r3d TO {reader}; \
             GRANT SELECT ON r3d.held TO {reader}"
        );
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(&mut self.admin)
            .await
            .expect("seed");
        self.admitted = load_admitted(&mut self.admin).await;
        self.note(format!("seeded r3d.held log_mark={}", self.log_mark));
    }

    async fn phase_owner_drop(&mut self) {
        self.lock_held().await;
        let mut owner = self.open(90, 60, 60).await;
        let pid = owner.binding().backend.pid;
        let start = owner.binding().backend.backend_start.clone();
        owner
            .open_stream(self.stream_budget())
            .await
            .expect("open blocked stream");
        {
            let mut pull = pin!(owner.next_batch());
            let saw = self.drive_until_active(&mut pull, pid, &start).await;
            assert!(saw, "blocked pull never became active");
        }
        let still = self.state_of(pid, &start).await;
        assert_eq!(
            still.as_deref(),
            Some("active"),
            "dropped next_batch released the backend"
        );
        let before = self.reader_backends().await;
        assert!(
            before.len() >= 2,
            "expected data and guard backends, got {before:?}"
        );
        self.note(format!(
            "next_batch_drop_state=active data_pid={pid} reader_backends={}",
            before.len()
        ));
        let began = Instant::now();
        drop(owner);
        self.wait_absent(&before, began).await;
        self.unlock_held().await;
        self.wait_clear().await;
        self.note("owner_drop_finish_called=0 snapshot_finished=0".to_string());
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
            PgEndpoint::new(&self.pointer.host, self.pointer.port, &self.pointer.database)
                .expect("endpoint"),
            PgCredentials::new(&self.pointer.reader_user, self.reader_pass.as_str())
                .expect("credentials"),
            SecretRef::new("secret://pg/drop/reader").expect("secret"),
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

    async fn lock_held(&mut self) {
        let _ = sqlx::query("ROLLBACK").execute(&mut self.admin).await;
        sqlx::query("BEGIN")
            .execute(&mut self.admin)
            .await
            .expect("lock begin");
        sqlx::query("LOCK TABLE r3d.held IN ACCESS EXCLUSIVE MODE")
            .execute(&mut self.admin)
            .await
            .expect("lock");
    }

    async fn unlock_held(&mut self) {
        sqlx::query("ROLLBACK")
            .execute(&mut self.admin)
            .await
            .expect("unlock");
    }

    /// The lock transaction caches `pg_stat_activity` for its whole life
    /// (`stats_fetch_consistency=cache`). Drop that copy before each poll.
    async fn refresh_observation(&mut self) {
        sqlx::query("SELECT pg_catalog.pg_stat_clear_snapshot()")
            .execute(&mut self.admin)
            .await
            .expect("clear stat snapshot");
    }

    async fn state_of(&mut self, pid: i64, start: &str) -> Option<String> {
        self.refresh_observation().await;
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

    async fn reader_backends(&mut self) -> Vec<(i64, String, String)> {
        self.refresh_observation().await;
        oo_backends(&mut self.admin, &self.pointer.reader_user).await
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
                _ = pull.as_mut() => panic!("pull ended before the backend was active"),
                _ = tokio::time::sleep(Duration::from_millis(30)) => {}
            }
            if self.state_of(pid, start).await.as_deref() == Some("active") {
                return true;
            }
        }
        false
    }

    async fn wait_absent(&mut self, before: &[(i64, String, String)], began: Instant) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let mut noted_first = false;
        loop {
            let left = self.reader_backends().await;
            let still: Vec<_> = before
                .iter()
                .filter(|row| left.iter().any(|live| live.0 == row.0 && live.1 == row.1))
                .collect();
            let busy: Vec<_> = left
                .iter()
                .filter(|row| row.2 == "active" || row.2 == "idle in transaction")
                .collect();
            if !noted_first {
                self.refresh_observation().await;
                let detail = backend_detail(&mut self.admin, &self.pointer.reader_user).await;
                self.note(format!(
                    "owner_drop_first_sample_ms={} still={still:?} busy={busy:?} detail={detail:?}",
                    began.elapsed().as_millis()
                ));
                noted_first = true;
            }
            if still.is_empty() && busy.is_empty() {
                self.note(format!("owner_drop_absent_ms={}", began.elapsed().as_millis()));
                return;
            }
            if began.elapsed() > bound {
                self.refresh_observation().await;
                let detail = backend_detail(&mut self.admin, &self.pointer.reader_user).await;
                panic!(
                    "reader backends remained after {}ms still={still:?} busy={busy:?} left={left:?} detail={detail:?}",
                    bound.as_millis()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn wait_clear(&mut self) {
        let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
        let began = Instant::now();
        loop {
            let left = self.reader_backends().await;
            if left.is_empty() {
                return;
            }
            if began.elapsed() > bound {
                panic!(
                    "reader backends still present after {}ms: {left:?}",
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
        let mut cancel_selects = 0_usize;
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
            *verbs.entry(verb.clone()).or_default() += 1;
            if write_word(&strip_sql_literals(sql)).is_some() || has_token(sql, "OFFSET") {
                panic!("reader sql has a forbidden token: {sql}");
            }
            if sql.contains("pg_terminate_backend") {
                panic!("reader terminated a backend: {sql}");
            }
            if sql.contains("pg_cancel_backend") {
                assert_eq!(verb, "SELECT", "cancel statement was not a select: {sql}");
                cancel_selects += 1;
            }
        }
        assert!(oo_statements > 0, "reader sql log was empty");
        assert!(cancel_selects > 0, "reader did not send select pg_cancel_backend");
        let verb_text = verbs
            .iter()
            .map(|(verb, count)| format!("{verb}={count}"))
            .collect::<Vec<_>>()
            .join(",");
        self.note(format!(
            "sql_audit oo_statements={oo_statements} cancel_selects={cancel_selects} dml_ddl_commit_terminate=0 verbs={verb_text}"
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

async fn load_admitted(admin: &mut PgConnection) -> AdmittedCatalog {
    let rows = sqlx::query(
        "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'r3d' AND c.relname = 'held' AND a.attnum > 0 AND NOT a.attisdropped \
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
         WHERE n.nspname = 'r3d' AND c.relname = 'held' AND i.indisprimary \
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

async fn backend_detail(admin: &mut PgConnection, user: &str) -> Vec<String> {
    let rows = sqlx::query(
        "SELECT pid::int8, COALESCE(state, ''), COALESCE(wait_event_type, ''), \
         COALESCE(wait_event, ''), xact_start IS NOT NULL, left(query, 80) \
         FROM pg_catalog.pg_stat_activity \
         WHERE application_name = 'oo-pg-source' AND usename::text = $1 \
         ORDER BY pid",
    )
    .bind(user)
    .fetch_all(&mut *admin)
    .await
    .expect("backend detail");
    rows.iter()
        .map(|row| {
            format!(
                "pid={} state={} wait={}/{} xact={} query={}",
                row.try_get::<i64, _>(0).unwrap_or(0),
                row.try_get::<String, _>(1).unwrap_or_default(),
                row.try_get::<String, _>(2).unwrap_or_default(),
                row.try_get::<String, _>(3).unwrap_or_default(),
                row.try_get::<bool, _>(4).unwrap_or(false),
                row.try_get::<String, _>(5).unwrap_or_default()
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
