//! Second measurement: catalog-shaped reads before the savepoint, then ALTER.
//! Raw sqlx only. Does not call SnapshotOwner.

use std::fs;
use std::time::Instant;

use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

const REL: &str = "\
SELECT c.oid::int8, c.relkind::text, \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'SELECT'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'INSERT'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'UPDATE'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'DELETE'), \
pg_catalog.has_table_privilege(CURRENT_USER, c.oid, 'TRUNCATE') \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = 'life' AND c.relname = 'hold'";
const ATTRS: &str = "\
SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod::int4, a.attnotnull \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
WHERE n.nspname = 'life' AND c.relname = 'hold' ORDER BY a.attnum";
const PKEY: &str = "\
SELECT a.attnum::int4, a.attname::text \
FROM pg_catalog.pg_class c \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
JOIN pg_catalog.pg_index i ON i.indrelid = c.oid AND i.indisprimary \
JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS u(attnum, ord) ON true \
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = u.attnum \
WHERE n.nspname = 'life' AND c.relname = 'hold' ORDER BY u.ord";
const LOCKS: &str = "\
SELECT c.relname::text, l.mode::text, l.granted \
FROM pg_catalog.pg_locks l \
JOIN pg_catalog.pg_class c ON c.oid = l.relation \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE l.pid = $1 AND l.locktype = 'relation' AND n.nspname = 'life' AND c.relname = 'hold'";
const LABEL: &str = "SELECT label::text FROM life.hold WHERE id = 1";
const ALTER: &str = "ALTER TABLE life.hold ADD COLUMN extra text";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    super_user: String,
    password_dir: String,
    ca_file: String,
    lot_reader_user: String,
    lot_reader_password_file: String,
    super_password_file: String,
}

#[tokio::test(flavor = "current_thread")]
async fn catalog_reads_before_savepoint_still_allow_alter() {
    let path = std::env::var("OO_A125_PREREQ_OUTCOME").unwrap_or_default();
    match run().await {
        Ok(body) => {
            let _ = fs::write(&path, body);
        }
        Err(fact) => {
            let _ = fs::write(&path, format!("result=harness_fail\nfact={fact}\n"));
            eprintln!("locks harness {fact}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<String, String> {
    let pointer: Pointer = serde_json::from_str(
        &fs::read_to_string(std::env::var("OO_A125_PREREQ_POINTER").map_err(|_| "pointer_env")?).map_err(|_| "pointer_read")?,
    )
    .map_err(|_| "pointer_json")?;
    let ca = fs::read(&pointer.ca_file).map_err(|_| "ca")?;
    let super_pass = fs::read_to_string(format!("{}/{}", pointer.password_dir, pointer.super_password_file)).map_err(|_| "super_pass")?;
    let reader_pass = fs::read_to_string(format!("{}/{}", pointer.password_dir, pointer.lot_reader_password_file)).map_err(|_| "reader_pass")?;
    let mut admin = dial(&pointer, &pointer.super_user, super_pass.trim(), &ca, "oo-prereq-lock-admin").await?;
    let mut rr = dial(&pointer, &pointer.lot_reader_user, reader_pass.trim(), &ca, "oo-prereq-lock-rr").await?;
    exec(&mut admin, "SELECT pg_catalog.set_config('lock_timeout', '5000', false)").await?;
    exec(&mut admin, "SELECT pg_catalog.set_config('statement_timeout', '15000', false)").await?;
    exec(&mut rr, "SELECT pg_catalog.set_config('lock_timeout', '5000', false)").await?;
    exec(&mut rr, "SELECT pg_catalog.set_config('statement_timeout', '15000', false)").await?;
    exec(&mut rr, "SELECT pg_catalog.set_config('default_transaction_read_only', 'on', false)").await?;
    exec(&mut rr, "START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY").await?;
    exec(&mut rr, "SELECT 1").await?;
    let pid: i32 = scalar(&mut rr, "SELECT pg_catalog.pg_backend_pid()::int4").await?;
    let snap_before: String = scalar(&mut rr, "SELECT pg_catalog.pg_current_snapshot()::text").await?;
    exec(&mut rr, REL).await?;
    exec(&mut rr, ATTRS).await?;
    exec(&mut rr, PKEY).await?;
    let locks_after_catalog = lock_lines(&mut admin, pid).await?;
    exec(&mut rr, "SAVEPOINT oo_probe_lock").await?;
    let inside: String = scalar(&mut rr, LABEL).await?;
    exec(&mut rr, "ROLLBACK TO SAVEPOINT oo_probe_lock").await?;
    exec(&mut rr, "RELEASE SAVEPOINT oo_probe_lock").await?;
    let locks_after_release = lock_lines(&mut admin, pid).await?;
    exec(&mut admin, "START TRANSACTION").await?;
    let started = Instant::now();
    let alter = exec(&mut admin, ALTER).await;
    let elapsed = started.elapsed().as_millis();
    let alter_state = match &alter {
        Ok(()) => "ok".to_string(),
        Err(state) => state.clone(),
    };
    if alter.is_ok() {
        exec(&mut admin, "COMMIT").await?;
    } else {
        let _ = exec(&mut admin, "ROLLBACK").await;
    }
    let label_after: String = scalar(&mut rr, LABEL).await?;
    let snap_after: String = scalar(&mut rr, "SELECT pg_catalog.pg_current_snapshot()::text").await?;
    let rr_extra: i64 = scalar(&mut rr, "SELECT count(*)::int8 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'life' AND c.relname = 'hold' AND a.attname = 'extra' AND a.attnum > 0 AND NOT a.attisdropped").await?;
    let mut rc = dial(&pointer, &pointer.lot_reader_user, reader_pass.trim(), &ca, "oo-prereq-lock-rc").await?;
    exec(&mut rc, "START TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY").await?;
    let rc_extra: i64 = scalar(&mut rc, "SELECT count(*)::int8 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'life' AND c.relname = 'hold' AND a.attname = 'extra' AND a.attnum > 0 AND NOT a.attisdropped").await?;
    let rc_attnum: i32 = if rc_extra == 1 {
        scalar(&mut rc, "SELECT a.attnum::int4 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'life' AND c.relname = 'hold' AND a.attname = 'extra'").await?
    } else {
        -1
    };
    let _ = rc.close().await;
    let _ = rr.close().await;
    let _ = admin.close().await;
    let pass = alter.is_ok()
        && inside == "still"
        && label_after == "still"
        && snap_before == snap_after
        && rr_extra == 0
        && rc_extra == 1
        && locks_after_release.is_empty();
    let result = if pass { "pass" } else if alter_state == "55P03" || alter_state == "57014" { "incompatible" } else { "measured" };
    let body = format!(
        "result={result}\nlabel_inside={inside}\nlabel_after_rr={label_after}\nsnap_same={}\nlocks_after_catalog={}\nlocks_after_release={}\nalter_sqlstate={alter_state}\nalter_elapsed_ms={elapsed}\nrr_extra_count={rr_extra}\nrc_extra_count={rc_extra}\nextra_attnum={rc_attnum}\n",
        snap_before == snap_after,
        if locks_after_catalog.is_empty() { "none".to_string() } else { locks_after_catalog.join(";") },
        if locks_after_release.is_empty() { "none".to_string() } else { locks_after_release.join(";") },
    );
    if result == "pass" {
        Ok(body)
    } else if result == "incompatible" {
        let _ = fs::write(std::env::var("OO_A125_PREREQ_OUTCOME").unwrap_or_default(), &body);
        std::process::exit(2);
    } else {
        Err(body)
    }
}

async fn lock_lines(conn: &mut PgConnection, pid: i32) -> Result<Vec<String>, String> {
    let rows = sqlx::query(LOCKS).bind(pid).fetch_all(&mut *conn).await.map_err(sqlstate)?;
    let mut lines = Vec::new();
    for row in rows {
        let name: String = row.try_get(0).map_err(|_| "decode")?;
        let mode: String = row.try_get(1).map_err(|_| "decode")?;
        let granted: bool = row.try_get(2).map_err(|_| "decode")?;
        lines.push(format!("{name}|{mode}|{granted}"));
    }
    Ok(lines)
}

async fn dial(pointer: &Pointer, user: &str, password: &str, ca: &[u8], app: &str) -> Result<PgConnection, String> {
    let options = PgConnectOptions::from_explicit()
        .host(&pointer.host)
        .port(pointer.port)
        .username(user)
        .password(password)
        .database(&pointer.database)
        .application_name(app)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec())
        .disable_statement_logging();
    options.connect().await.map_err(sqlstate)
}

async fn exec(conn: &mut PgConnection, sql: &'static str) -> Result<(), String> {
    sqlx::query(sql).execute(&mut *conn).await.map(|_| ()).map_err(sqlstate)
}

async fn scalar<T>(conn: &mut PgConnection, sql: &'static str) -> Result<T, String>
where
    T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    let row = sqlx::query(sql).fetch_one(&mut *conn).await.map_err(sqlstate)?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

fn sqlstate(err: sqlx::Error) -> String {
    err.as_database_error()
        .and_then(|db| db.code().map(|code| code.to_string()))
        .unwrap_or_else(|| "no_sqlstate".to_string())
}
