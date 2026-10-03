//! Bounded catalog admission on PostgreSQL 17.10.
//! `OO_PG_CATALOG_RATE_FIXTURE` is required. A missing pointer is a failure.
//! The fixture is the a125 snapshot database plus `rate` objects created here.

use std::fs;
use std::time::{Duration, Instant};

use oo_connectors::{
    PgBudget, PgConnector, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, RunBinding,
    SecretRef, SelectedTable, SourceErrorKind, SourceId, TableSelection,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::ConnectOptions;
use zeroize::Zeroizing;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_rate_live() {
    let pointer = load_pointer();
    let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
    let super_pass = read_pass(&pointer.password_dir, "super.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let mut admin = dial(
        &pointer,
        &pointer.super_user,
        &super_pass,
        &ca,
        "oo-rate-admin",
    )
    .await;
    prepare_rate(&mut admin).await;

    let started = Instant::now();
    let mut big = open_session(
        &pointer,
        &reader_pass,
        &ca,
        "rate_big",
        50_000,
        Duration::from_secs(30),
        snap_tables(),
    )
    .await;
    for index in 0..4 {
        let catalog = big.catalog().await.expect("max_rows 50000 catalog");
        assert_eq!(catalog.tables.len(), 2, "catalog {index}");
    }
    let big_elapsed = started.elapsed();
    assert!(
        big_elapsed < Duration::from_secs(1),
        "four catalogs took {big_elapsed:?}; a window refill would hide a missing settlement"
    );
    drop(big);

    let mut ordinary = open_session(
        &pointer,
        &reader_pass,
        &ca,
        "rate_ord",
        1_000,
        Duration::from_secs(30),
        snap_tables(),
    )
    .await;
    for _ in 0..3 {
        let catalog = ordinary.catalog().await.expect("max_rows 1000 catalog");
        assert_eq!(catalog.tables.len(), 2);
    }
    let access = ordinary.inspect_access().await.expect("inspect_access");
    assert!(!access.protocol.is_empty());
    drop(ordinary);

    let wide = open_session(
        &pointer,
        &reader_pass,
        &ca,
        "rate_pk",
        1_000,
        Duration::from_secs(30),
        vec![SelectedTable::new("rate", "wide").unwrap()],
    )
    .await
    .catalog()
    .await
    .expect("32-column primary key");
    assert_eq!(wide.tables.len(), 1);
    assert_eq!(wide.tables[0].columns.len(), 32);
    assert_eq!(wide.tables[0].primary_key.len(), 32);

    let cut = open_session(
        &pointer,
        &reader_pass,
        &ca,
        "rate_cut",
        1_000,
        Duration::from_secs(30),
        vec![SelectedTable::new("rate", "pol").unwrap()],
    )
    .await
    .catalog()
    .await
    .expect_err("128 policies");
    assert_eq!(cut.kind(), SourceErrorKind::Catalog);
    assert_eq!(cut.fact(), Some("catalog_truncated"));

    sqlx::query("BEGIN")
        .execute(&mut admin)
        .await
        .expect("lock begin");
    sqlx::query("LOCK TABLE rate.bulk IN ACCESS EXCLUSIVE MODE")
        .execute(&mut admin)
        .await
        .expect("lock bulk");
    let burn_started = Instant::now();
    let mut deny = open_session(
        &pointer,
        &reader_pass,
        &ca,
        "rate_deny",
        50_000,
        Duration::from_millis(400),
        vec![SelectedTable::new("rate", "bulk").unwrap()],
    )
    .await;
    let burned = deny
        .read_typed(
            &RunBinding {
                source_id: "rate_deny".to_string(),
                schema: "rate".to_string(),
                table: "bulk".to_string(),
                run_generation: 1,
                input_digest: [7; 32],
            },
            oo_connectors::PgTypePolicy::PRESERVE_ROW,
        )
        .await
        .expect_err("locked read keeps its reservation");
    assert_eq!(burned.kind(), SourceErrorKind::Budget);
    assert_eq!(burned.fact(), Some("statement_timeout"));
    drop(deny);
    sqlx::query("ROLLBACK")
        .execute(&mut admin)
        .await
        .expect("lock rollback");
    sqlx::query("SELECT 'a128_deny_cut'")
        .execute(&mut admin)
        .await
        .expect("deny marker");
    let denied = match PgConnector::new(config(
        &pointer,
        &reader_pass,
        &ca,
        "rate_deny",
        1_000,
        Duration::from_secs(30),
        vec![SelectedTable::new("rate", "bulk").unwrap()],
    ))
    .connect()
    .await
    {
        Err(err) => err,
        Ok(mut session) => session.catalog().await.expect_err("exhausted window"),
    };
    let burn_elapsed = burn_started.elapsed();
    assert_eq!(denied.kind(), SourceErrorKind::Budget);
    assert_eq!(denied.fact(), Some("max_rows_per_sec"));
    assert!(
        burn_elapsed < Duration::from_secs(1),
        "denial window elapsed {burn_elapsed:?}"
    );
    let tail = log_after_marker(&pointer.log_file, "a128_deny_cut");
    assert!(
        !tail.contains("pg_get_keywords"),
        "rate denial reached the keyword query"
    );

    let raw = fs::read_to_string(&pointer.log_file).expect("postgres log");
    assert!(!reader_pass.as_str().is_empty() && !raw.contains(reader_pass.as_str()));
    assert!(!super_pass.as_str().is_empty() && !raw.contains(super_pass.as_str()));
    assert!(raw.contains("pg_get_keywords"));
    assert!(raw.contains("LIMIT"));
    let audit = audit_log(&raw);
    assert!(audit.reader_statements > 0, "reader log was empty");
    assert_eq!(audit.reader_writes, 0, "reader wrote: {}", audit.reader_write_word);
    assert!(audit.admin_statements > 0, "admin log was empty");
    eprintln!(
        "catalog_rate big_elapsed={big_elapsed:?} burn_elapsed={burn_elapsed:?} \
         reader_statements={} reader_writes={} admin_statements={} admin_writes={} \
         writer_statements={} writer_writes={}",
        audit.reader_statements,
        audit.reader_writes,
        audit.admin_statements,
        audit.admin_writes,
        audit.writer_statements,
        audit.writer_writes
    );
}

fn load_pointer() -> Pointer {
    let path = std::env::var("OO_PG_CATALOG_RATE_FIXTURE").expect("OO_PG_CATALOG_RATE_FIXTURE is required");
    serde_json::from_str(&fs::read_to_string(&path).expect("rate pointer")).expect("rate pointer json")
}

fn snap_tables() -> Vec<SelectedTable> {
    vec![
        SelectedTable::new("snap", "parent").unwrap(),
        SelectedTable::new("snap", "child").unwrap(),
    ]
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

async fn open_session(
    pointer: &Pointer,
    password: &str,
    ca: &[u8],
    source: &str,
    max_rows: u32,
    statement_timeout: Duration,
    tables: Vec<SelectedTable>,
) -> oo_connectors::PgSession {
    PgConnector::new(config(
        pointer,
        password,
        ca,
        source,
        max_rows,
        statement_timeout,
        tables,
    ))
    .connect()
    .await
    .unwrap_or_else(|err| panic!("connect {source}: {err}"))
}

async fn prepare_rate(admin: &mut PgConnection) {
    // Static admin text. Policy names are the server loop index, not client input.
    const SETUP: [&str; 9] = [
        "DROP SCHEMA IF EXISTS rate CASCADE",
        "CREATE SCHEMA rate",
        "CREATE TABLE rate.pol (id int PRIMARY KEY)",
        "ALTER TABLE rate.pol ENABLE ROW LEVEL SECURITY",
        "CREATE TABLE rate.wide (c01 int NOT NULL, c02 int NOT NULL, c03 int NOT NULL, c04 int NOT NULL, c05 int NOT NULL, c06 int NOT NULL, c07 int NOT NULL, c08 int NOT NULL, c09 int NOT NULL, c10 int NOT NULL, c11 int NOT NULL, c12 int NOT NULL, c13 int NOT NULL, c14 int NOT NULL, c15 int NOT NULL, c16 int NOT NULL, c17 int NOT NULL, c18 int NOT NULL, c19 int NOT NULL, c20 int NOT NULL, c21 int NOT NULL, c22 int NOT NULL, c23 int NOT NULL, c24 int NOT NULL, c25 int NOT NULL, c26 int NOT NULL, c27 int NOT NULL, c28 int NOT NULL, c29 int NOT NULL, c30 int NOT NULL, c31 int NOT NULL, c32 int NOT NULL, PRIMARY KEY (c01, c02, c03, c04, c05, c06, c07, c08, c09, c10, c11, c12, c13, c14, c15, c16, c17, c18, c19, c20, c21, c22, c23, c24, c25, c26, c27, c28, c29, c30, c31, c32))",
        "CREATE TABLE rate.bulk (id int PRIMARY KEY)",
        "GRANT USAGE ON SCHEMA rate TO oo_snap_reader",
        "GRANT SELECT ON rate.pol, rate.wide, rate.bulk TO oo_snap_reader",
        "DO $rate$ BEGIN FOR i IN 1..128 LOOP EXECUTE format('CREATE POLICY p%s ON rate.pol USING (true)', i); END LOOP; END $rate$",
    ];
    for statement in SETUP {
        sqlx::query(statement)
            .execute(&mut *admin)
            .await
            .expect("rate setup");
    }
}

async fn dial(
    pointer: &Pointer,
    user: &str,
    password: &str,
    ca: &[u8],
    application: &str,
) -> PgConnection {
    let mut options = PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(user)
        .password(password)
        .database(&pointer.database)
        .application_name(application)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec());
    options = options.disable_statement_logging();
    options.connect().await.expect("admin connect")
}

fn read_pass(dir: &str, name: &str) -> Zeroizing<String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).expect("password file");
    Zeroizing::new(text.trim().to_string())
}

fn log_after_marker(path: &str, marker: &str) -> String {
    let mut last = String::new();
    for _ in 0..20 {
        last = fs::read_to_string(path).unwrap_or_default();
        if let Some((_, tail)) = last.rsplit_once(marker) {
            return tail.to_string();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("marker {marker} missing from postgres log ({})", last.len());
}

struct LogAudit {
    reader_statements: usize,
    reader_writes: usize,
    reader_write_word: String,
    admin_statements: usize,
    admin_writes: usize,
    writer_statements: usize,
    writer_writes: usize,
}

fn audit_log(raw: &str) -> LogAudit {
    let mut audit = LogAudit {
        reader_statements: 0,
        reader_writes: 0,
        reader_write_word: String::new(),
        admin_statements: 0,
        admin_writes: 0,
        writer_statements: 0,
        writer_writes: 0,
    };
    for line in raw.lines() {
        let Some(sql) = statement_sql(line) else {
            continue;
        };
        let stripped = strip_sql_literals(sql);
        let word = write_word(&stripped).unwrap_or("");
        if line.contains("user=oo_snap_reader") && line.contains("app=oo-pg-source") {
            audit.reader_statements += 1;
            if !word.is_empty() {
                audit.reader_writes += 1;
                audit.reader_write_word = word.to_string();
            }
        } else if line.contains("user=postgres") {
            audit.admin_statements += 1;
            if !word.is_empty() {
                audit.admin_writes += 1;
            }
        } else if line.contains("user=oo_snap_writer") {
            audit.writer_statements += 1;
            if !word.is_empty() {
                audit.writer_writes += 1;
            }
        }
    }
    audit
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
        "INSERT", "UPDATE", "DELETE", "TRUNCATE", "CREATE", "ALTER", "DROP", "GRANT", "REVOKE",
        "COMMIT",
    ];
    for token in sql.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_') {
        let upper = token.to_ascii_uppercase();
        if let Some(word) = WORDS.iter().copied().find(|word| *word == upper) {
            return Some(word);
        }
    }
    None
}
