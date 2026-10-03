//! Owned PostgreSQL 17.10 named-portal fixture.
//!
//! `OO_A127_FIXTURE` is a pointer file. Passwords stay in the files it names.
//! A missing pointer fails this test.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::time::{Duration, Instant};

use sqlx::postgres::{
    PgArguments, PgConnectOptions, PgConnection, PgPortalAllowance, PgPortalOutcome, PgSslMode,
};
use sqlx::{Arguments, Connection, Row};

fn must_err<T, E: std::fmt::Display>(result: Result<T, E>, what: &str) -> E {
    match result {
        Err(err) => err,
        Ok(_) => panic!("{what}"),
    }
}

const PAGE: &str = "SELECT id, note FROM oo_a127.cut ORDER BY id";
const NONE: &str = "SELECT id, note FROM oo_a127.cut WHERE false";
const ONE: &str = "SELECT id, note FROM oo_a127.cut WHERE id = $1";
const FORBIDDEN: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "MERGE", "CREATE", "ALTER", "DROP", "TRUNCATE", "GRANT",
    "REVOKE", "COMMIT", "COPY",
];

#[tokio::test(flavor = "current_thread")]
async fn live_named_portal_repeatable_read() {
    let pointer = env::var("OO_A127_FIXTURE").expect("OO_A127_FIXTURE");
    let cfg = load_pointer(&pointer);
    let mut reader = connect(&cfg, "reader_user", "reader_password_file", "oo-a127-reader").await;
    let mut writer = connect(&cfg, "writer_user", "writer_password_file", "oo-a127-writer").await;
    let mut admin = connect(&cfg, "admin_user", "admin_password_file", "oo-a127-admin").await;
    let mut oracle = connect(&cfg, "reader_user", "reader_password_file", "oo-a127-oracle").await;
    sqlx::query("DELETE FROM oo_a127.cut WHERE id >= 4")
        .execute(&mut admin)
        .await
        .expect("reset extra rows");
    sqlx::query("UPDATE oo_a127.cut SET note = 'three-original' WHERE id = 3")
        .execute(&mut admin)
        .await
        .expect("reset note");

    let mut tx = reader
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .expect("begin");
    let isolation = show(&mut tx, "SHOW transaction_isolation").await;
    let read_only = show(&mut tx, "SHOW transaction_read_only").await;
    assert_eq!(isolation, "repeatable read");
    assert_eq!(read_only, "on");
    let pid: i32 = scalar_i32(&mut tx, "SELECT pg_backend_pid()").await;
    println!("reader_backend_pid={pid}");

    let grant = PgPortalAllowance::positive(8).expect("allowance");
    let mut none = tx
        .open_named_portal(NONE, PgArguments::default())
        .await
        .expect("zero-row portal");
    let step = none.pull(grant).await.expect("zero-row pull");
    let PgPortalOutcome::Complete { row: None, .. } = step.outcome else {
        panic!("zero-row select should complete");
    };
    assert_eq!(step.accounting.reserved_rows, 1);
    assert_eq!(step.accounting.consumed_rows, 0);
    drop(none);

    let mut args = PgArguments::default();
    args.add(1i32).expect("arg");
    let mut one = tx.open_named_portal(ONE, args).await.expect("one-row portal");
    let step = one.pull(grant).await.expect("one-row pull");
    let PgPortalOutcome::Suspended(row) = step.outcome else {
        panic!("one-row execute limit suspends until a later pull");
    };
    assert_eq!(row.try_get::<i32, _>(0).expect("id"), 1);
    assert_eq!(row.try_get::<Option<String>, _>(1).expect("note").as_deref(), Some("one"));
    assert_eq!(step.accounting.consumed_rows, 1);
    let step = one.pull(grant).await.expect("one-row finish");
    let PgPortalOutcome::Complete { row: None, .. } = step.outcome else {
        panic!("the demanded follow-up completes with no row");
    };
    assert_eq!(step.accounting.consumed_rows, 0);
    drop(one);

    let snapshot: String = scalar_text(&mut tx, "SELECT pg_export_snapshot()").await;
    assert!(snapshot_id_ok(&snapshot), "snapshot id {snapshot}");
    println!("snapshot_id={snapshot}");

    let mut portal = tx
        .open_named_portal(PAGE, PgArguments::default())
        .await
        .expect("page portal");
    let first = portal.pull(grant).await.expect("first page");
    let PgPortalOutcome::Suspended(row) = first.outcome else {
        panic!("first of three rows should suspend");
    };
    assert_eq!(note_of(&row), (1, Some("one".to_string())));
    assert_eq!(first.accounting.consumed_rows, 1);
    assert_eq!(first.accounting.reserved_rows, 1);
    let state = activity_state(&mut admin, pid).await;
    println!("state_after_first_page={state}");
    // PostgreSQL keeps the backend active after PortalSuspended until ReadyForQuery.
    assert_eq!(state, "active");

    writer
        .begin_with("BEGIN ISOLATION LEVEL READ COMMITTED")
        .await
        .expect("writer begin")
        .commit_update()
        .await;

    let second = portal.pull(grant).await.expect("second page");
    let PgPortalOutcome::Suspended(row) = second.outcome else {
        panic!("second row should suspend");
    };
    assert_eq!(note_of(&row), (2, None));
    let third = portal.pull(grant).await.expect("third page");
    let PgPortalOutcome::Suspended(row) = third.outcome else {
        panic!("third row fills the execute limit and suspends");
    };
    assert_eq!(note_of(&row), (3, Some("three-original".to_string())));
    let finished = portal.pull(grant).await.expect("page finish");
    let PgPortalOutcome::Complete { row: None, .. } = finished.outcome else {
        panic!("the demanded follow-up completes with no row");
    };
    assert_eq!(finished.accounting.consumed_rows, 0);
    assert!(portal.pull(grant).await.is_err());
    drop(portal);

    let count = scalar_i32(&mut tx, "SELECT count(*)::int4 FROM oo_a127.cut").await;
    assert_eq!(count, 3);
    let visible = text_column(
        &mut tx,
        "SELECT coalesce(string_agg(id::text || ':' || coalesce(note, 'NULL'), ',' ORDER BY id), '') FROM oo_a127.cut",
    )
    .await;
    assert_eq!(visible, "1:one,2:NULL,3:three-original");

    oracle_sees_cut(&mut oracle, &snapshot).await;
    tx.rollback().await.expect("reader rollback");
    assert!(!reader.named_portal_poisoned());

    server_error(&mut reader).await;
    cancel_sleep(&mut reader, &mut admin).await;

    let report = classify(cfg["log_file"].as_str());
    println!("{report}");
    if let Ok(path) = env::var("OO_A127_LOG_REPORT") {
        fs::write(path, &report).expect("log report");
    }
}

async fn server_error(reader: &mut PgConnection) {
    let mut tx = reader.begin().await.expect("error begin");
    let mut args = PgArguments::default();
    args.add(0i32).expect("zero");
    // PostgreSQL 17 plans this custom statement at Bind and raises 22012 there.
    match tx
        .open_named_portal("SELECT (1 / $1)::int4", args)
        .await
    {
        Ok(mut portal) => {
            let err = must_err(
                portal
                    .pull(PgPortalAllowance::positive(1).expect("allowance"))
                    .await,
                "division",
            );
            assert_eq!(sqlstate(&err).as_deref(), Some("22012"));
            let last = portal.last_pull().expect("accounted");
            assert_eq!(last.reserved_rows, 1);
            assert_eq!(last.consumed_rows, 0);
            assert!(!portal.connection_poisoned());
            drop(portal);
        }
        Err(err) => {
            assert_eq!(sqlstate(&err).as_deref(), Some("22012"));
        }
    }
    tx.rollback().await.expect("rollback after 22012");
    assert!(!reader.named_portal_poisoned());
}

fn sqlstate(err: &sqlx::Error) -> Option<String> {
    err.as_database_error()
        .and_then(|db| db.code().map(|code| code.into_owned()))
}

async fn cancel_sleep(reader: &mut PgConnection, admin: &mut PgConnection) {
    let mut tx = reader.begin().await.expect("sleep begin");
    let pid = scalar_i32(&mut tx, "SELECT pg_backend_pid()").await;
    let mut portal = tx
        .open_named_portal("SELECT pg_sleep(30)", PgArguments::default())
        .await
        .expect("sleep portal");
    let (err, sent_cancel) = {
        let pull = portal.pull(PgPortalAllowance::positive(1).expect("allowance"));
        tokio::pin!(pull);
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut sent_cancel = false;
        let err = loop {
            tokio::select! {
                biased;
                result = &mut pull => break must_err(result, "sleep must not succeed"),
                _ = tokio::time::sleep(Duration::from_millis(40)), if !sent_cancel => {
                    if Instant::now() > deadline {
                        panic!("reader did not reach an active sleep");
                    }
                    let state = activity_state(admin, pid).await;
                    println!("sleep_state={state}");
                    if state == "active" {
                        let cancelled: bool = sqlx::query("SELECT pg_cancel_backend($1)")
                            .bind(pid)
                            .fetch_one(&mut *admin)
                            .await
                            .expect("cancel")
                            .try_get(0)
                            .expect("cancel flag");
                        assert!(cancelled);
                        sent_cancel = true;
                    }
                }
            }
        };
        (err, sent_cancel)
    };
    assert!(sent_cancel, "cancel was not sent");
    assert_eq!(
        err.as_database_error().and_then(|db| db.code().map(|code| code.into_owned())).as_deref(),
        Some("57014")
    );
    assert_eq!(portal.last_pull().expect("reserved").consumed_rows, 0);
    assert!(!portal.connection_poisoned());
    drop(portal);
    tx.rollback().await.expect("rollback after cancel");
    assert!(!reader.named_portal_poisoned());
}

async fn oracle_sees_cut(oracle: &mut PgConnection, snapshot: &str) {
    let mut tx = oracle
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .await
        .expect("oracle begin");
    let statement = format!("SET TRANSACTION SNAPSHOT '{snapshot}'");
    sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
        .execute(&mut *tx)
        .await
        .expect("import snapshot");
    let visible = text_column(
        &mut tx,
        "SELECT coalesce(string_agg(id::text || ':' || coalesce(note, 'NULL'), ',' ORDER BY id), '') FROM oo_a127.cut",
    )
    .await;
    assert_eq!(visible, "1:one,2:NULL,3:three-original");
    tx.rollback().await.expect("oracle rollback");
}

fn note_of(row: &sqlx::postgres::PgRow) -> (i32, Option<String>) {
    (
        row.try_get(0).expect("id"),
        row.try_get(1).expect("note"),
    )
}

async fn show(conn: &mut PgConnection, sql: &'static str) -> String {
    scalar_text(conn, sql).await
}

async fn scalar_i32(conn: &mut PgConnection, sql: &'static str) -> i32 {
    sqlx::query(sql)
        .fetch_one(&mut *conn)
        .await
        .expect(sql)
        .try_get(0)
        .expect(sql)
}

async fn scalar_text(conn: &mut PgConnection, sql: &'static str) -> String {
    sqlx::query(sql)
        .fetch_one(&mut *conn)
        .await
        .expect(sql)
        .try_get(0)
        .expect(sql)
}

async fn text_column(conn: &mut PgConnection, sql: &'static str) -> String {
    scalar_text(conn, sql).await
}

async fn activity_state(admin: &mut PgConnection, pid: i32) -> String {
    sqlx::query("SELECT COALESCE(state, '') FROM pg_stat_activity WHERE pid = $1")
        .bind(pid)
        .fetch_one(&mut *admin)
        .await
        .expect("activity")
        .try_get(0)
        .expect("state")
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

fn classify(path: &str) -> String {
    let text = fs::read_to_string(path).expect("postgres log");
    let mut verbs: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut forbidden_reader = Vec::new();
    let mut reader_statements = 0usize;
    for line in text.lines() {
        let Some(app) = app_name(line) else {
            continue;
        };
        let Some(sql) = statement_sql(line) else {
            continue;
        };
        let Some(verb) = sql.split_whitespace().next() else {
            continue;
        };
        let verb = verb.trim_matches(|ch: char| ch == '(' || ch == ';').to_ascii_uppercase();
        if verb.is_empty() {
            continue;
        }
        verbs.entry(app).or_default().insert(verb.clone());
        if app == "oo-a127-reader" {
            reader_statements += 1;
            if FORBIDDEN.contains(&verb.as_str()) {
                forbidden_reader.push(verb);
            }
        }
    }
    let reader = verbs.get("oo-a127-reader").map(join).unwrap_or_default();
    let writer = verbs.get("oo-a127-writer").map(join).unwrap_or_default();
    let admin = verbs.get("oo-a127-admin").map(join).unwrap_or_default();
    let oracle = verbs.get("oo-a127-oracle").map(join).unwrap_or_default();
    assert!(reader_statements > 0, "reader SQL log was empty");
    assert!(forbidden_reader.is_empty(), "reader forbidden verbs {forbidden_reader:?}");
    assert!(verbs.get("oo-a127-writer").is_some_and(|set| set.contains("UPDATE") && set.contains("INSERT") && set.contains("COMMIT")));
    assert!(verbs.get("oo-a127-oracle").is_some_and(|set| set.contains("SET")));
    format!(
        "reader_statements={reader_statements}\nreader_verbs={reader}\nwriter_verbs={writer}\nadmin_verbs={admin}\noracle_verbs={oracle}\nreader_forbidden=0\n"
    )
}

fn join(verbs: &BTreeSet<String>) -> String {
    verbs.iter().cloned().collect::<Vec<_>>().join(",")
}

fn app_name(line: &str) -> Option<&str> {
    let start = line.find("app=")? + 4;
    let rest = &line[start..];
    let end = rest.find([' ', '\t']).unwrap_or(rest.len());
    let name = &rest[..end];
    matches!(
        name,
        "oo-a127-reader" | "oo-a127-writer" | "oo-a127-admin" | "oo-a127-oracle"
    )
    .then_some(name)
}

fn statement_sql(line: &str) -> Option<&str> {
    if let Some(index) = line.find("statement:") {
        return Some(line[index + "statement:".len()..].trim());
    }
    let index = line.find("execute ")?;
    let rest = line[index + "execute ".len()..].trim_start();
    let (_, sql) = rest.split_once(':')?;
    Some(sql.trim())
}

fn load_pointer(path: &str) -> BTreeMap<String, String> {
    let text = fs::read_to_string(path).expect("pointer");
    let mut map = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once(' ').expect("pointer field");
        map.insert(key.to_string(), value.to_string());
    }
    map
}

async fn connect(
    cfg: &BTreeMap<String, String>,
    user_key: &str,
    password_key: &str,
    application: &str,
) -> PgConnection {
    let password = fs::read_to_string(field(cfg, password_key)).expect(password_key);
    let ca = fs::read(field(cfg, "ca_file")).expect("ca");
    let options = PgConnectOptions::from_explicit()
        .host(field(cfg, "host"))
        .port(field(cfg, "port").parse().expect("port"))
        .username(field(cfg, user_key))
        .password(password.trim())
        .database(field(cfg, "database"))
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca)
        .application_name(application);
    PgConnection::connect_with(&options)
        .await
        .expect(application)
}

fn field<'a>(cfg: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    cfg.get(key).unwrap_or_else(|| panic!("missing {key}")).as_str()
}

trait CommitUpdate {
    async fn commit_update(self);
}

impl CommitUpdate for sqlx::Transaction<'_, sqlx::Postgres> {
    async fn commit_update(mut self) {
        sqlx::query("UPDATE oo_a127.cut SET note = 'three-mutated' WHERE id = 3")
            .execute(&mut *self)
            .await
            .expect("update");
        sqlx::query("INSERT INTO oo_a127.cut (id, note) VALUES (4, 'four')")
            .execute(&mut *self)
            .await
            .expect("insert");
        self.commit().await.expect("writer commit");
    }
}
