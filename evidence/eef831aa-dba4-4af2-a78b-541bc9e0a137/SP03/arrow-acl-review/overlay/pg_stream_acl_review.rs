//! Independent inherited-grant representative.
//! SELECT is granted to a parent role, not to the reader role. The reader inherits
//! that role. Open and the current-catalog expression must allow the scan.
//! This is not the fixture role's own table GRANT.

use std::fs;
use std::time::{Duration, Instant};

use arrow_array::{Array, Int32Array};
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy, SecretRef,
    SelectedTable, SnapshotOwner, SnapshotRequest, SourceId, StreamBudget, StreamItem,
    TableSelection, CLEANUP_OBSERVATION_BOUND_MS,
};
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};
use zeroize::Zeroizing;

const SOURCE: &str = "colg_src";
const SCHEMA: &str = "colg";
const TABLE: &str = "via_parent";

const EXPR_SQL: &str = "\
SELECT CURRENT_USER::text, \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'SELECT'), \
pg_catalog.has_column_privilege(CURRENT_USER, a.attrelid, a.attnum, 'SELECT'), \
EXISTS ( \
SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl, pg_catalog.acldefault('r', c.relowner))) AS x \
WHERE x.privilege_type = 'SELECT' \
AND (x.grantee = 0 OR pg_catalog.pg_has_role(CURRENT_USER, x.grantee, 'USAGE')) \
), \
EXISTS ( \
SELECT 1 FROM pg_catalog.aclexplode(a.attacl) AS x \
WHERE x.privilege_type = 'SELECT' \
AND (x.grantee = 0 OR pg_catalog.pg_has_role(CURRENT_USER, x.grantee, 'USAGE')) \
), \
CASE WHEN a.attnum IS NULL THEN NULL ELSE ( \
EXISTS (SELECT 1 FROM pg_catalog.pg_roles r WHERE r.rolname = CURRENT_USER AND r.rolsuper) \
OR pg_catalog.pg_has_role(CURRENT_USER, 'pg_read_all_data', 'USAGE') \
OR EXISTS ( \
SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl, pg_catalog.acldefault('r', c.relowner))) AS x \
WHERE x.privilege_type = 'SELECT' \
AND (x.grantee = 0 OR pg_catalog.pg_has_role(CURRENT_USER, x.grantee, 'USAGE')) \
) \
OR EXISTS ( \
SELECT 1 FROM pg_catalog.aclexplode(a.attacl) AS x \
WHERE x.privilege_type = 'SELECT' \
AND (x.grantee = 0 OR pg_catalog.pg_has_role(CURRENT_USER, x.grantee, 'USAGE')) \
) \
) END, \
COALESCE(c.relacl::text, ''), \
COALESCE(a.attacl::text, '') \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
WHERE n.nspname = 'colg' AND c.relname = 'via_parent' AND a.attname = 'id'";

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
async fn inherited_role_grant_streams_the_arrow_value() {
    let path = std::env::var("OO_PG_STREAM_FIXTURE").expect("OO_PG_STREAM_FIXTURE is required");
    let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&path).expect("pointer"))
        .expect("pointer json");
    let reader_pass = read_pass(&pointer.password_dir, "reader.pass");
    let super_pass = read_pass(&pointer.password_dir, "super.pass");
    let ca = fs::read(&pointer.ca_file).expect("ca");
    let mut admin = dial(
        &pointer,
        &pointer.super_user,
        &super_pass,
        &ca,
        "oo-colg-admin",
    )
    .await;
    for sql in [
        "CREATE SCHEMA IF NOT EXISTS colg",
        "REVOKE ALL ON SCHEMA colg FROM PUBLIC",
        "GRANT USAGE ON SCHEMA colg TO oo_a131_reader",
        "DO $$ BEGIN CREATE ROLE colg_parent NOLOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$",
        "GRANT colg_parent TO oo_a131_reader",
        "CREATE TABLE IF NOT EXISTS colg.via_parent (id int4 PRIMARY KEY)",
        "INSERT INTO colg.via_parent (id) SELECT 3 WHERE NOT EXISTS (SELECT 1 FROM colg.via_parent)",
        "REVOKE ALL ON TABLE colg.via_parent FROM PUBLIC",
        "REVOKE SELECT ON TABLE colg.via_parent FROM oo_a131_reader",
        "GRANT SELECT ON TABLE colg.via_parent TO colg_parent",
    ] {
        sqlx::query(sql)
            .execute(&mut admin)
            .await
            .expect("inherited grant setup");
    }
    let mut reader = dial(
        &pointer,
        &pointer.reader_user,
        &reader_pass,
        &ca,
        "oo-colg-oracle",
    )
    .await;
    let oracle = sqlx::query(EXPR_SQL)
        .fetch_one(&mut reader)
        .await
        .expect("expression oracle");
    let current_user: String = oracle.try_get(0).expect("current_user");
    let has_table: bool = oracle.try_get(1).expect("has_table");
    let has_column: bool = oracle.try_get(2).expect("has_column");
    let table_acl: bool = oracle.try_get(3).expect("table_acl");
    let column_acl: bool = oracle.try_get(4).expect("column_acl");
    let expr: Option<bool> = oracle.try_get(5).expect("expr");
    let relacl: String = oracle.try_get(6).expect("relacl");
    let attacl: String = oracle.try_get(7).expect("attacl");
    assert_eq!(current_user, pointer.reader_user, "oracle ran as another role");
    assert!(has_table, "inherited table SELECT is not visible");
    assert!(has_column, "inherited column SELECT is not visible");
    assert!(table_acl, "table ACL expression missed the parent role relacl={relacl}");
    assert!(
        !relacl.contains("oo_a131_reader="),
        "fixture role holds the table grant directly relacl={relacl}"
    );
    assert!(
        relacl.contains("colg_parent="),
        "parent role is not the grantee relacl={relacl}"
    );
    assert_eq!(expr, Some(true), "CURRENT_CATALOG expression denied an inherited grant");
    eprintln!(
        "oracle current_user={current_user} has_table={has_table} has_column={has_column} table_acl={table_acl} column_acl={column_acl} expr={expr:?} relacl={relacl} attacl={attacl}"
    );
    let _ = reader.close().await;

    let (postmaster, in_recovery) = node_of(&mut admin).await;
    let admitted = load_admitted(&mut admin).await;
    let config = PgSourceConfig::try_new(
        SourceId::new(SOURCE).unwrap(),
        PgEndpoint::new(&pointer.host, pointer.port, &pointer.database).unwrap(),
        PgCredentials::new(&pointer.reader_user, &*reader_pass).unwrap(),
        SecretRef::new("secret://pg/colg/reader").unwrap(),
        PgTlsConfig::from_ca_pem(&ca).unwrap(),
        PgBudget::new(
            Duration::from_secs(30),
            Duration::from_secs(60),
            Duration::from_secs(5),
            128,
            1024 * 1024,
        )
        .unwrap(),
        TableSelection::new(vec![SelectedTable::new(SCHEMA, TABLE).unwrap()]).unwrap(),
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
        match owner.next_batch().await.expect("column grant batch") {
            StreamItem::Batch(batch) => {
                assert_eq!(batch.typed.table, TABLE);
                assert_eq!(batch.typed.run_generation, generation);
                let ids = batch
                    .record_batch
                    .column_by_name("id")
                    .expect("id")
                    .as_any()
                    .downcast_ref::<Int32Array>()
                    .expect("int4");
                assert_eq!(ids.len(), 1);
                assert_eq!(ids.value(0), 3);
                saw_row = true;
            }
            StreamItem::TableEof(eof) => {
                assert_eq!(eof.table, TABLE);
                assert_eq!(eof.rows_emitted, 1);
                assert!(!eof.zero_column);
            }
            StreamItem::Exhausted(_) => break,
        }
    }
    assert!(saw_row, "column grant emitted no Arrow row");
    let finished = owner.finish().await.expect("finish");
    assert_eq!(finished.generation, generation);
    assert_eq!(finished.source_id, SOURCE);
    assert_eq!(finished.backend.pid, pid);

    let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
    for (gone_pid, gone_start) in &backends {
        wait_gone(&mut admin, *gone_pid, gone_start, bound).await;
    }
    assert!(
        oo_backends(&mut admin, &pointer.reader_user)
            .await
            .is_empty(),
        "a reader backend remained"
    );

    let raw = fs::read_to_string(&pointer.log_file).unwrap_or_default();
    for secret in [reader_pass.as_str(), super_pass.as_str()] {
        if !secret.is_empty() && raw.contains(secret) {
            panic!("postgres log contains a closed string");
        }
    }
    let mut reader_writes = 0_usize;
    let mut scans = 0_usize;
    for line in raw.lines() {
        if !line.contains("app=oo-pg-source") || !line.contains("user=oo_a131_reader") {
            continue;
        }
        let Some(sql) = statement_sql(line) else {
            continue;
        };
        if write_word(&strip_sql_literals(sql)).is_some() {
            reader_writes += 1;
        }
        let folded = sql.to_ascii_lowercase();
        if folded.contains("via_parent") {
            scans += 1;
        }
    }
    assert_eq!(reader_writes, 0, "reader log has a write verb");
    assert!(scans >= 1, "column-grant table was not scanned");
    assert!(
        !raw.to_ascii_lowercase().contains("permission denied for table via_parent"),
        "column grant was rejected by the server"
    );
    let outcome = format!(
        "current_user={current_user} has_table={has_table} has_column={has_column} table_acl={table_acl} column_acl={column_acl} expr={expr:?} arrow_id=3 generation={generation} data_pid={pid} data_start={start} finish=SnapshotFinished backends={} reader_writes={reader_writes} scans={scans}\n",
        backends.len()
    );
    eprintln!("outcome {outcome}");
    if let Ok(path) = std::env::var("OO_PG_STREAM_COLG_OUTCOME") {
        fs::write(path, outcome).expect("outcome");
    }
    let _ = admin.close().await;
}

async fn load_admitted(admin: &mut PgConnection) -> AdmittedCatalog {
    let rows = sqlx::query(
        "SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod, a.attnotnull \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'colg' AND c.relname = 'via_parent' AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
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
         WHERE n.nspname = 'colg' AND c.relname = 'via_parent' AND i.indisprimary \
         ORDER BY u.ord",
    )
    .fetch_all(&mut *admin)
    .await
    .expect("keys");
    AdmittedCatalog {
        tables: vec![AdmittedTable {
            schema: SCHEMA.to_string(),
            name: TABLE.to_string(),
            columns,
            primary_key: keys
                .iter()
                .map(|row| row.try_get::<String, _>(0).unwrap())
                .collect(),
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
            panic!("backend {pid} still present after {}ms", began.elapsed().as_millis());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
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
    options.disable_statement_logging().connect().await.expect("dial")
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
