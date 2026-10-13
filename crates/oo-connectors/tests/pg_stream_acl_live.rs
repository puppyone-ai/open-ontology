//! One post-open `REVOKE SELECT` on the second selected table.
//! `OO_PG_STREAM_FIXTURE` is required. The admin session is separate from the reader.

use std::fs;
use std::time::{Duration, Instant};

use arrow_array::{Array, Int32Array};
use oo_connectors::{
    AdmittedCatalog, AdmittedColumn, AdmittedTable, CLEANUP_OBSERVATION_BOUND_MS, PgBudget,
    PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy, SecretRef, SelectedTable,
    SnapshotOwner, SnapshotRequest, SourceErrorKind, SourceId, StreamBudget, StreamItem,
    TableSelection, config_identity, type_policy_identity,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "acl_src";
const SCHEMA: &str = "acl";
const REVOKE_SQL: &str = "REVOKE SELECT ON acl.acl_b FROM oo_a131_reader";

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
async fn revoke_after_first_eof_is_select_not_granted_before_scan() {
    let path = std::env::var("OO_PG_STREAM_FIXTURE").expect("OO_PG_STREAM_FIXTURE is required");
    let pointer: Pointer =
        serde_json::from_str(&fs::read_to_string(&path).expect("pointer")).expect("pointer json");
    let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
    let super_pass = read_pass(&pointer.password_dir, "super.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let mut admin = dial(
        &pointer,
        &pointer.super_user,
        &super_pass,
        &ca,
        "oo-acl-admin",
    )
    .await;
    sqlx::query("GRANT SELECT ON TABLE acl.acl_a, acl.acl_b TO oo_a131_reader")
        .execute(&mut admin)
        .await
        .expect("grant select");
    let (postmaster, in_recovery) = node_of(&mut admin).await;
    let admitted = load_admitted(&mut admin).await;
    let config = PgSourceConfig::try_new(
        SourceId::new(SOURCE).unwrap(),
        PgEndpoint::new(&pointer.host, pointer.port, &pointer.database).unwrap(),
        PgCredentials::new(&pointer.reader_user, &*reader_pass).unwrap(),
        SecretRef::new("secret://pg/acl/reader").unwrap(),
        PgTlsConfig::from_ca_pem(&ca).unwrap(),
        PgBudget::new(
            Duration::from_secs(30),
            Duration::from_secs(60),
            Duration::from_secs(5),
            128,
            1024 * 1024,
        )
        .unwrap(),
        TableSelection::new(vec![
            SelectedTable::new(SCHEMA, "acl_a").unwrap(),
            SelectedTable::new(SCHEMA, "acl_b").unwrap(),
        ])
        .unwrap(),
    )
    .unwrap();
    let mut owner = SnapshotOwner::open(SnapshotRequest {
        config_identity: config_identity(&config),
        type_policy_identity: type_policy_identity(PgTypePolicy::PRESERVE_ROW),
        config,
        admitted,
        policy: PgTypePolicy::PRESERVE_ROW,
        generation: 1,
        deadline: Instant::now() + Duration::from_secs(90),
        expected_principal: pointer.reader_user.clone(),
        expected_database: pointer.database.clone(),
        expected_in_recovery: in_recovery,
        expected_postmaster_start: postmaster,
    })
    .await
    .expect("open");
    let generation = owner.binding().generation;
    let pid = owner.binding().backend.pid;
    let start = owner.binding().backend.backend_start.clone();
    let backends = oo_backends(&mut admin, &pointer.reader_user).await;
    assert_eq!(backends.len(), 2, "data session and guard");
    owner
        .open_stream(StreamBudget::try_new(4, 1024, 256, 1024).expect("budget"))
        .await
        .expect("stream");

    let mut saw_row = false;
    loop {
        match owner.next_batch().await.expect("acl_a") {
            StreamItem::Batch(batch) => {
                assert_eq!(batch.typed.table, "acl_a");
                assert_eq!(batch.typed.run_generation, generation);
                let ids = batch
                    .record_batch
                    .column_by_name("id")
                    .expect("id")
                    .as_any()
                    .downcast_ref::<Int32Array>()
                    .expect("int4");
                assert_eq!(ids.len(), 1);
                assert_eq!(ids.value(0), 7);
                saw_row = true;
            }
            StreamItem::TableEof(eof) => {
                assert_eq!(eof.table, "acl_a");
                assert_eq!(eof.rows_emitted, 1);
                assert!(!eof.zero_column);
                break;
            }
            StreamItem::Exhausted(_) => panic!("stream finished before acl_b"),
        }
    }
    assert!(saw_row, "first table emitted no row");

    sqlx::query(REVOKE_SQL)
        .execute(&mut admin)
        .await
        .expect("revoke");
    let err = match owner.next_batch().await {
        Err(err) => err,
        Ok(StreamItem::Batch(_)) => panic!("revoked demand returned a batch"),
        Ok(StreamItem::TableEof(_)) => panic!("revoked demand returned table eof"),
        Ok(StreamItem::Exhausted(_)) => panic!("revoked demand returned exhausted"),
    };
    assert_eq!(err.kind(), SourceErrorKind::Privilege);
    assert_eq!(err.fact(), Some("select_not_granted"));
    let finish = owner.finish().await.expect_err("finish");
    assert_eq!(finish.kind(), SourceErrorKind::Privilege);
    assert_eq!(finish.fact(), Some("select_not_granted"));

    let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
    for (gone_pid, gone_start) in backends {
        wait_gone(&mut admin, gone_pid, &gone_start, bound).await;
    }
    assert!(
        oo_backends(&mut admin, &pointer.reader_user)
            .await
            .is_empty(),
        "a reader backend remained"
    );
    let raw = read_log(&pointer.log_file).await;
    for secret in [reader_pass.as_str(), super_pass.as_str()] {
        if !secret.is_empty() && raw.contains(secret) {
            panic!("postgres log contains a closed string");
        }
    }
    assert!(raw.contains(REVOKE_SQL), "revoke was not logged");
    let folded_log = raw.to_ascii_lowercase();
    assert!(
        !folded_log.contains("permission denied for table acl_b"),
        "scan of acl_b was rejected by the server"
    );
    assert!(
        !folded_log.contains("from acl.acl_b") && !folded_log.contains("from \"acl\".\"acl_b\""),
        "revoked table was scanned"
    );
    let mut reader_writes = 0_usize;
    let mut saw_start = false;
    let mut savepoint_before = 0_usize;
    let mut savepoint_after = 0_usize;
    let mut scan_a_before = 0_usize;
    let mut scan_a_after = 0_usize;
    let mut reader_after = 0_usize;
    let mut revoked = false;
    for line in raw.lines() {
        if line.contains(REVOKE_SQL) {
            revoked = true;
        }
        if !line.contains("app=oo-pg-source") || !line.contains("user=oo_a131_reader") {
            continue;
        }
        let Some(sql) = statement_sql(line) else {
            continue;
        };
        if write_word(&strip_sql_literals(sql)).is_some() {
            reader_writes += 1;
        }
        let verb = sql
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        if matches!(verb.as_str(), "START" | "BEGIN") {
            saw_start = true;
        }
        if verb == "SAVEPOINT" && sql.contains("oo_stream_lock") {
            if revoked {
                savepoint_after += 1;
            } else {
                savepoint_before += 1;
            }
        }
        let folded = sql.to_ascii_lowercase();
        if folded.contains("from acl.acl_a") || folded.contains("from \"acl\".\"acl_a\"") {
            if revoked {
                scan_a_after += 1;
            } else {
                scan_a_before += 1;
            }
        }
        if revoked {
            reader_after += 1;
        }
    }
    assert_eq!(
        savepoint_before, 0,
        "stream savepoint is not on the source allow-list"
    );
    assert_eq!(savepoint_after, 0, "savepoint ran after revoke");
    assert!(scan_a_before >= 1, "first table was not scanned");
    assert_eq!(
        scan_a_after, 0,
        "first table was scanned again after revoke"
    );
    assert!(reader_after > 0, "no reader statement after revoke");
    assert_eq!(reader_writes, 0, "reader log has a write verb");
    assert!(saw_start, "reader transaction start was not logged");
    let outcome = format!(
        "acl_a_id=7 acl_a_eof=1 generation={generation} data_pid={pid} data_start={start} guard=select_not_granted finish=select_not_granted reader_after={reader_after} reader_writes=0 second_savepoint=0 second_scan=0 backends_gone=2\n"
    );
    eprintln!("outcome {outcome}");
    if let Ok(path) = std::env::var("OO_PG_STREAM_ACL_OUTCOME") {
        fs::write(path, outcome).expect("outcome");
    }
    let _ = admin.close().await;
}

async fn load_admitted(admin: &mut PgConnection) -> AdmittedCatalog {
    let mut tables = Vec::new();
    for name in ["acl_a", "acl_b"] {
        let rows = sqlx::query(
            "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
             FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'acl' AND c.relname = $1 AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
        )
        .bind(name)
        .fetch_all(&mut *admin)
        .await
        .expect("columns");
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
             WHERE n.nspname = 'acl' AND c.relname = $1 AND i.indisprimary \
             ORDER BY u.ord",
        )
        .bind(name)
        .fetch_all(&mut *admin)
        .await
        .expect("keys");
        tables.push(AdmittedTable {
            schema: SCHEMA.to_string(),
            name: name.to_string(),
            columns,
            primary_key: keys
                .iter()
                .map(|row| row.try_get::<String, _>(0).unwrap())
                .collect(),
        });
    }
    AdmittedCatalog { tables }
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

async fn oo_backends(admin: &mut PgConnection, user: &str) -> Vec<(i64, String)> {
    let rows = sqlx::query(
        "SELECT pid::int8, backend_start::text FROM pg_catalog.pg_stat_activity \
         WHERE application_name = 'oo-pg-source' AND usename::text = $1",
    )
    .bind(user)
    .fetch_all(&mut *admin)
    .await
    .expect("backends");
    rows.iter()
        .map(|row| (row.try_get(0).expect("pid"), row.try_get(1).expect("start")))
        .collect()
}

async fn wait_gone(admin: &mut PgConnection, pid: i64, start: &str, bound: Duration) {
    let pid_i = i32::try_from(pid).expect("pid");
    let began = Instant::now();
    loop {
        let row = sqlx::query(
            "SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid = $1 AND backend_start::text = $2",
        )
        .bind(pid_i)
        .bind(start)
        .fetch_optional(&mut *admin)
        .await
        .expect("activity");
        if row.is_none() {
            return;
        }
        if began.elapsed() > bound {
            panic!(
                "backend {pid} still present after {}ms",
                began.elapsed().as_millis()
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn read_log(path: &str) -> String {
    let began = Instant::now();
    loop {
        let raw = fs::read_to_string(path).unwrap_or_default();
        if raw.contains(REVOKE_SQL) || began.elapsed() > Duration::from_secs(5) {
            return raw;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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
