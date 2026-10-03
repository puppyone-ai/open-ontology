//! Evidence-only measurement. Raw sqlx against the a125 fixture.
//! Does not call SnapshotOwner and does not change product source.

use std::fs;
use std::time::Instant;

use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Connection, Row};

const APP_ADMIN: &str = "oo-prereq-admin";
const APP_RR: &str = "oo-prereq-rr";
const APP_WRITER: &str = "oo-prereq-writer";
const APP_RC: &str = "oo-prereq-rc";

const SET_STATEMENT: &str = "SELECT pg_catalog.set_config('statement_timeout', '15000', false)";
const SET_LOCK: &str = "SELECT pg_catalog.set_config('lock_timeout', '5000', false)";
const SET_TZ: &str = "SELECT pg_catalog.set_config('TimeZone', 'UTC', false)";
const SET_RO: &str = "SELECT pg_catalog.set_config('default_transaction_read_only', 'on', false)";
const BEGIN_RR: &str = "START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY";
const BEGIN_RC: &str = "START TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY";
const PIN: &str = "SELECT 1";
const SAVEPOINT: &str = "SAVEPOINT oo_probe_lock";
const ROLLBACK_TO: &str = "ROLLBACK TO SAVEPOINT oo_probe_lock";
const RELEASE: &str = "RELEASE SAVEPOINT oo_probe_lock";
const ISOLATION: &str = "SELECT current_setting('transaction_isolation'), current_setting('transaction_read_only')";
const SNAP: &str = "SELECT pg_catalog.pg_current_snapshot()::text";
const BATCH: &str = "SELECT title::text FROM lot.batch WHERE id = 11";
const CHILD: &str = "SELECT qty::int4 FROM lot.\"child row\" WHERE id = 31";
const LABEL: &str = "SELECT label::text FROM drift.item WHERE id = 5";
const TICK: &str = "SELECT hits::int8 FROM lot.tick WHERE n = 1";
const VERSION: &str = "SELECT current_setting('server_version')::text, current_setting('server_version_num')::text";
const RECOVERY: &str = "SELECT pg_catalog.pg_is_in_recovery()";
const ITEM_OID: &str = "SELECT c.oid::int8 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'drift' AND c.relname = 'item'";
const UPDATE_BATCH: &str = "UPDATE lot.batch SET title = 'oak' WHERE id = 11";
const UPDATE_CHILD: &str = "UPDATE lot.\"child row\" SET qty = 9 WHERE id = 31";
const ALTER: &str = "ALTER TABLE drift.item ADD COLUMN extra text";
const COMMIT: &str = "COMMIT";
const BEGIN: &str = "START TRANSACTION";
const SELF: &str = "\
SELECT pid::int4, backend_start::text, COALESCE(xact_start::text, ''), COALESCE(state, ''), \
usename::text, application_name::text \
FROM pg_catalog.pg_stat_activity WHERE pid = pg_catalog.pg_backend_pid()";
const LOCKS: &str = "\
SELECT n.nspname::text, c.relname::text, l.mode::text, l.granted \
FROM pg_catalog.pg_locks l \
JOIN pg_catalog.pg_class c ON c.oid = l.relation \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE l.pid = $1 AND l.locktype = 'relation' AND n.nspname = ANY($2::text[]) \
ORDER BY 1, 2, 3";
const ATTRS: &str = "\
SELECT a.attnum::int4, a.attname::text, a.atttypid::int8, a.atttypmod::int4, a.attnotnull \
FROM pg_catalog.pg_attribute a \
JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
ORDER BY a.attnum";

#[derive(Deserialize)]
struct Pointer {
    host: String,
    port: u16,
    database: String,
    super_user: String,
    password_dir: String,
    ca_file: String,
    lot_reader_user: String,
    lot_writer_user: String,
    lot_reader_password_file: String,
    lot_writer_password_file: String,
    super_password_file: String,
}

struct Note {
    lines: Vec<String>,
}

impl Note {
    fn add(&mut self, key: &str, value: impl std::fmt::Display) {
        self.lines.push(format!("{key}={value}"));
    }

    fn body(&self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }
}

#[tokio::test(flavor = "current_thread")]
async fn rr_savepoint_releases_locks_before_alter() {
    let outcome = std::env::var("OO_A125_PREREQ_OUTCOME").unwrap_or_default();
    let sql_path = std::env::var("OO_A125_PREREQ_SQL").unwrap_or_default();
    let mut note = Note { lines: Vec::new() };
    let code = match measure(&mut note, &sql_path).await {
        Ok(code) => code,
        Err(fact) => {
            note.add("result", "harness_fail");
            note.add("fact", &fact);
            eprintln!("prereq harness {fact}");
            write_clean(&outcome, &note.body());
            std::process::exit(1);
        }
    };
    if code == 0 {
        note.add("result", "pass");
    } else {
        note.add("result", "incompatible");
    }
    write_clean(&outcome, &note.body());
    eprintln!("prereq result code={code}");
    if code != 0 {
        std::process::exit(code);
    }
}

fn write_clean(path: &str, body: &str) {
    if path.is_empty() {
        eprintln!("prereq missing outcome path");
        return;
    }
    let secrets = load_secret_values();
    if secrets.iter().any(|secret| !secret.is_empty() && body.contains(secret)) {
        let _ = fs::write(path, "result=harness_fail\nfact=outcome_contained_secret\nsecret_hits=1\n");
        return;
    }
    let _ = fs::write(path, body);
}

fn load_secret_values() -> Vec<String> {
    let Ok(pointer_path) = std::env::var("OO_A125_PREREQ_POINTER") else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(&pointer_path) else {
        return Vec::new();
    };
    let Ok(pointer) = serde_json::from_str::<Pointer>(&text) else {
        return Vec::new();
    };
    ["super.pass", "reader.pass", "writer.pass", "guc.pass", "lot-reader.pass", "lot-writer.pass"]
        .into_iter()
        .filter_map(|name| fs::read_to_string(format!("{}/{}", pointer.password_dir, name)).ok())
        .map(|value| value.trim().to_string())
        .collect()
}

async fn measure(note: &mut Note, sql_path: &str) -> Result<i32, String> {
    let pointer_path = std::env::var("OO_A125_PREREQ_POINTER").map_err(|_| "pointer_env")?;
    let pointer: Pointer = serde_json::from_str(&fs::read_to_string(&pointer_path).map_err(|_| "pointer_read")?)
        .map_err(|_| "pointer_json")?;
    let ca = fs::read(&pointer.ca_file).map_err(|_| "ca_read")?;
    let super_pass = read_pass(&pointer.password_dir, &pointer.super_password_file)?;
    let reader_pass = read_pass(&pointer.password_dir, &pointer.lot_reader_password_file)?;
    let writer_pass = read_pass(&pointer.password_dir, &pointer.lot_writer_password_file)?;

    let mut admin = connect(&pointer, &pointer.super_user, &super_pass, &ca, APP_ADMIN).await?;
    let mut rr = connect(&pointer, &pointer.lot_reader_user, &reader_pass, &ca, APP_RR).await?;
    let mut writer = connect(&pointer, &pointer.lot_writer_user, &writer_pass, &ca, APP_WRITER).await?;
    run(&mut admin, SET_STATEMENT).await?;
    run(&mut admin, SET_LOCK).await?;
    run(&mut admin, SET_TZ).await?;
    run(&mut rr, SET_STATEMENT).await?;
    run(&mut rr, SET_LOCK).await?;
    run(&mut rr, SET_TZ).await?;
    run(&mut rr, SET_RO).await?;
    run(&mut writer, SET_STATEMENT).await?;
    run(&mut writer, SET_LOCK).await?;
    run(&mut writer, SET_TZ).await?;

    let (version, version_num) = pair(&mut admin, VERSION).await?;
    note.add("server_version", &version);
    note.add("server_version_num", &version_num);
    if version_num != "170010" {
        return Err(format!("version_num={version_num}"));
    }
    let recovery = one_bool(&mut admin, RECOVERY).await?;
    note.add("in_recovery", recovery);
    let tick_before = one_i64(&mut admin, TICK).await?;
    let oid_before = one_i64(&mut admin, ITEM_OID).await?;
    note.add("tick_before", tick_before);
    note.add("item_oid_before", oid_before);

    run(&mut rr, BEGIN_RR).await?;
    run(&mut rr, PIN).await?;
    let (isolation, read_only) = pair(&mut rr, ISOLATION).await?;
    note.add("rr_isolation", &isolation);
    note.add("rr_read_only", &read_only);
    let snap_before = one_text(&mut rr, SNAP).await?;
    note.add("rr_snapshot_before", &snap_before);
    let backend = self_backend(&mut rr).await?;
    note.add("rr_pid", backend.pid);
    note.add("rr_backend_start", &backend.backend_start);
    note.add("rr_xact_start", &backend.xact_start);
    note.add("rr_state", &backend.state);
    note.add("rr_user", &backend.user);
    note.add("rr_app", &backend.app);
    let writer_backend = self_backend(&mut writer).await?;
    note.add("writer_pid", writer_backend.pid);
    note.add("writer_backend_start", &writer_backend.backend_start);
    note.add("writer_user", &writer_backend.user);
    note.add("writer_app", &writer_backend.app);

    let savepoint_state = run(&mut rr, SAVEPOINT).await;
    note.add("savepoint_sqlstate", state_of(&savepoint_state));
    if savepoint_state.is_err() {
        let _ = close3(admin, rr, writer).await;
        return Ok(2);
    }
    let title_inside = one_text(&mut rr, BATCH).await?;
    let qty_inside = one_i32(&mut rr, CHILD).await?;
    let label_inside = one_text(&mut rr, LABEL).await?;
    note.add("batch_inside", &title_inside);
    note.add("child_qty_inside", qty_inside);
    note.add("label_inside", &label_inside);
    if title_inside != "maple" || qty_inside != 6 || label_inside != "pebble" {
        let _ = close3(admin, rr, writer).await;
        return Err("seed_mismatch".to_string());
    }
    let during = locks(&mut admin, backend.pid).await?;
    note.add("locks_during", during.join(";"));
    let rollback_state = run(&mut rr, ROLLBACK_TO).await;
    note.add("rollback_to_sqlstate", state_of(&rollback_state));
    let release_state = run(&mut rr, RELEASE).await;
    note.add("release_sqlstate", state_of(&release_state));
    let after = locks(&mut admin, backend.pid).await?;
    note.add("locks_after", if after.is_empty() { "none".to_string() } else { after.join(";") });

    let mut code = 0;
    if rollback_state.is_err() || release_state.is_err() {
        code = 2;
    }
    if !held_share(&during, "lot", "batch") || !held_share(&during, "lot", "child row") || !held_share(&during, "drift", "item") {
        code = 2;
    }
    if still_held(&after, "lot", "batch") || still_held(&after, "lot", "child row") || still_held(&after, "drift", "item") {
        code = 2;
    }

    let update_state = run(&mut writer, BEGIN).await;
    note.add("writer_begin_sqlstate", state_of(&update_state));
    if update_state.is_err() {
        let _ = close3(admin, rr, writer).await;
        return Ok(2);
    }
    let batch_rows = match affected(&mut writer, UPDATE_BATCH).await {
        Ok(rows) => rows,
        Err(state) => {
            note.add("update_batch_sqlstate", &state);
            let _ = run(&mut writer, "ROLLBACK").await;
            let _ = close3(admin, rr, writer).await;
            return finish_lock_error(note, state);
        }
    };
    let child_rows = match affected(&mut writer, UPDATE_CHILD).await {
        Ok(rows) => rows,
        Err(state) => {
            note.add("update_child_sqlstate", &state);
            let _ = run(&mut writer, "ROLLBACK").await;
            let _ = close3(admin, rr, writer).await;
            return finish_lock_error(note, state);
        }
    };
    let update_commit = run(&mut writer, COMMIT).await;
    note.add("update_batch_rows", batch_rows);
    note.add("update_child_rows", child_rows);
    note.add("update_commit_sqlstate", state_of(&update_commit));
    if update_commit.is_err() || batch_rows != 1 || child_rows != 1 {
        let _ = close3(admin, rr, writer).await;
        return Ok(2);
    }

    let alter_begin = run(&mut writer, BEGIN).await;
    note.add("alter_begin_sqlstate", state_of(&alter_begin));
    let started = Instant::now();
    let alter_state = run(&mut writer, ALTER).await;
    let elapsed = started.elapsed().as_millis();
    note.add("alter_elapsed_ms", elapsed);
    note.add("alter_sqlstate", state_of(&alter_state));
    if alter_state.is_err() {
        let state = state_of(&alter_state);
        let blocked = locks(&mut admin, backend.pid).await.unwrap_or_default();
        note.add("locks_on_alter_error", if blocked.is_empty() { "none".to_string() } else { blocked.join(";") });
        let _ = run(&mut writer, "ROLLBACK").await;
        let _ = close3(admin, rr, writer).await;
        return finish_lock_error(note, state);
    }
    let alter_commit = run(&mut writer, COMMIT).await;
    note.add("alter_commit_sqlstate", state_of(&alter_commit));
    if alter_commit.is_err() {
        let _ = close3(admin, rr, writer).await;
        return Ok(2);
    }

    let title_after = one_text(&mut rr, BATCH).await?;
    let qty_after = one_i32(&mut rr, CHILD).await?;
    let label_after = one_text(&mut rr, LABEL).await?;
    let snap_after = one_text(&mut rr, SNAP).await?;
    let rr_attrs = attrs(&mut rr, "drift", "item").await?;
    note.add("batch_after_rr", &title_after);
    note.add("child_qty_after_rr", qty_after);
    note.add("label_after_rr", &label_after);
    note.add("rr_snapshot_after", &snap_after);
    note.add("rr_item_attrs", rr_attrs.join(","));
    let rr_extra = attnum_of(&rr_attrs, "extra");
    note.add("rr_extra_attnum", rr_extra.map(|n| n.to_string()).unwrap_or_else(|| "absent".to_string()));

    let mut rc = connect(&pointer, &pointer.lot_reader_user, &reader_pass, &ca, APP_RC).await?;
    run(&mut rc, SET_STATEMENT).await?;
    run(&mut rc, SET_LOCK).await?;
    run(&mut rc, SET_TZ).await?;
    run(&mut rc, SET_RO).await?;
    run(&mut rc, BEGIN_RC).await?;
    let rc_backend = self_backend(&mut rc).await?;
    note.add("rc_pid", rc_backend.pid);
    note.add("rc_backend_start", &rc_backend.backend_start);
    note.add("rc_xact_start", &rc_backend.xact_start);
    note.add("rc_user", &rc_backend.user);
    note.add("rc_app", &rc_backend.app);
    let (rc_isolation, rc_ro) = pair(&mut rc, ISOLATION).await?;
    note.add("rc_isolation", &rc_isolation);
    note.add("rc_read_only", &rc_ro);
    let rc_title = one_text(&mut rc, BATCH).await?;
    let rc_qty = one_i32(&mut rc, CHILD).await?;
    let rc_label = one_text(&mut rc, LABEL).await?;
    let rc_attrs = attrs(&mut rc, "drift", "item").await?;
    note.add("batch_rc", &rc_title);
    note.add("child_qty_rc", rc_qty);
    note.add("label_rc", &rc_label);
    note.add("rc_item_attrs", rc_attrs.join(","));
    let extra = attnum_of(&rc_attrs, "extra");
    note.add("extra_attnum", extra.map(|n| n.to_string()).unwrap_or_else(|| "absent".to_string()));
    let oid_after = one_i64(&mut admin, ITEM_OID).await?;
    let tick_after = one_i64(&mut admin, TICK).await?;
    note.add("item_oid_after", oid_after);
    note.add("tick_after", tick_after);
    note.add("tick_delta", tick_after - tick_before);
    note.add("rr_snapshot_same", snap_before == snap_after);

    if title_after != "maple" || qty_after != 6 || label_after != "pebble" || rr_extra.is_some() || snap_before != snap_after {
        code = 2;
    }
    if rc_title != "oak" || rc_qty != 9 || rc_label != "pebble" || extra.is_none() || oid_before != oid_after {
        code = 2;
    }
    if tick_after - tick_before != 2 {
        code = 2;
    }
    if isolation != "repeatable read" || read_only != "on" || rc_isolation != "read committed" {
        code = 2;
    }

    let _ = rc.close().await;
    let _ = close3(admin, rr, writer).await;
    write_log(note, sql_path).await?;
    Ok(code)
}

fn finish_lock_error(note: &mut Note, state: String) -> Result<i32, String> {
    if state == "55P03" || state == "57014" || state == "25006" || state == "0A000" {
        note.add("stop", "lock_or_readonly");
        return Ok(2);
    }
    Err(format!("writer_sqlstate={state}"))
}

fn state_of(result: &Result<(), String>) -> String {
    match result {
        Ok(()) => "ok".to_string(),
        Err(state) => state.clone(),
    }
}

fn held_share(lines: &[String], schema: &str, name: &str) -> bool {
    lines.iter().any(|line| {
        let parts: Vec<&str> = line.split('|').collect();
        parts.len() == 4 && parts[0] == schema && parts[1] == name && parts[2] == "AccessShareLock" && parts[3] == "true"
    })
}

fn still_held(lines: &[String], schema: &str, name: &str) -> bool {
    lines.iter().any(|line| {
        let parts: Vec<&str> = line.split('|').collect();
        parts.len() == 4 && parts[0] == schema && parts[1] == name
    })
}

fn attnum_of(lines: &[String], name: &str) -> Option<i32> {
    lines.iter().find_map(|line| {
        let mut parts = line.split(':');
        let attnum = parts.next()?.parse::<i32>().ok()?;
        let attname = parts.next()?;
        if attname == name {
            Some(attnum)
        } else {
            None
        }
    })
}

struct Backend {
    pid: i32,
    backend_start: String,
    xact_start: String,
    state: String,
    user: String,
    app: String,
}

async fn self_backend(conn: &mut PgConnection) -> Result<Backend, String> {
    let row = sqlx::query(SELF).fetch_one(&mut *conn).await.map_err(|err| sqlstate(&err))?;
    Ok(Backend {
        pid: row.try_get(0).map_err(|_| "decode")?,
        backend_start: row.try_get(1).map_err(|_| "decode")?,
        xact_start: row.try_get(2).map_err(|_| "decode")?,
        state: row.try_get(3).map_err(|_| "decode")?,
        user: row.try_get(4).map_err(|_| "decode")?,
        app: row.try_get(5).map_err(|_| "decode")?,
    })
}

async fn locks(conn: &mut PgConnection, pid: i32) -> Result<Vec<String>, String> {
    let schemas = vec!["lot".to_string(), "drift".to_string()];
    let rows = sqlx::query(LOCKS)
        .bind(pid)
        .bind(&schemas)
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| sqlstate(&err))?;
    let mut lines = Vec::new();
    for row in rows {
        let schema: String = row.try_get(0).map_err(|_| "decode")?;
        let name: String = row.try_get(1).map_err(|_| "decode")?;
        let mode: String = row.try_get(2).map_err(|_| "decode")?;
        let granted: bool = row.try_get(3).map_err(|_| "decode")?;
        lines.push(format!("{schema}|{name}|{mode}|{granted}"));
    }
    Ok(lines)
}

async fn attrs(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Vec<String>, String> {
    let rows = sqlx::query(ATTRS)
        .bind(schema)
        .bind(table)
        .fetch_all(&mut *conn)
        .await
        .map_err(|err| sqlstate(&err))?;
    let mut lines = Vec::new();
    for row in rows {
        let attnum: i32 = row.try_get(0).map_err(|_| "decode")?;
        let name: String = row.try_get(1).map_err(|_| "decode")?;
        let typid: i64 = row.try_get(2).map_err(|_| "decode")?;
        let typmod: i32 = row.try_get(3).map_err(|_| "decode")?;
        let notnull: bool = row.try_get(4).map_err(|_| "decode")?;
        lines.push(format!("{attnum}:{name}:{typid}:{typmod}:{notnull}"));
    }
    Ok(lines)
}

async fn connect(pointer: &Pointer, user: &str, password: &str, ca: &[u8], app: &str) -> Result<PgConnection, String> {
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
    options.connect().await.map_err(|err| sqlstate(&err))
}

async fn run(conn: &mut PgConnection, sql: &'static str) -> Result<(), String> {
    sqlx::query(sql).execute(&mut *conn).await.map(|_| ()).map_err(|err| sqlstate(&err))
}

async fn affected(conn: &mut PgConnection, sql: &'static str) -> Result<u64, String> {
    let result = sqlx::query(sql).execute(&mut *conn).await.map_err(|err| sqlstate(&err))?;
    Ok(result.rows_affected())
}

async fn one_text(conn: &mut PgConnection, sql: &'static str) -> Result<String, String> {
    let row = sqlx::query(sql).fetch_one(&mut *conn).await.map_err(|err| sqlstate(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

async fn one_i32(conn: &mut PgConnection, sql: &'static str) -> Result<i32, String> {
    let row = sqlx::query(sql).fetch_one(&mut *conn).await.map_err(|err| sqlstate(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

async fn one_i64(conn: &mut PgConnection, sql: &'static str) -> Result<i64, String> {
    let row = sqlx::query(sql).fetch_one(&mut *conn).await.map_err(|err| sqlstate(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

async fn one_bool(conn: &mut PgConnection, sql: &'static str) -> Result<bool, String> {
    let row = sqlx::query(sql).fetch_one(&mut *conn).await.map_err(|err| sqlstate(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

async fn pair(conn: &mut PgConnection, sql: &'static str) -> Result<(String, String), String> {
    let row = sqlx::query(sql).fetch_one(&mut *conn).await.map_err(|err| sqlstate(&err))?;
    let left: String = row.try_get(0).map_err(|_| "decode")?;
    let right: String = row.try_get(1).map_err(|_| "decode")?;
    Ok((left, right))
}

async fn close3(admin: PgConnection, rr: PgConnection, writer: PgConnection) {
    for mut conn in [admin, rr, writer] {
        let _ = conn.close().await;
    }
}

fn sqlstate(err: &sqlx::Error) -> String {
    err.as_database_error()
        .and_then(|db| db.code().map(|code| code.to_string()))
        .unwrap_or_else(|| "no_sqlstate".to_string())
}

fn read_pass(dir: &str, name: &str) -> Result<String, String> {
    let text = fs::read_to_string(format!("{dir}/{name}")).map_err(|_| "password_read")?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("password_empty".to_string());
    }
    Ok(trimmed)
}

async fn write_log(note: &mut Note, sql_path: &str) -> Result<(), String> {
    let mut log = String::new();
    for _ in 0..15 {
        log = fs::read_to_string("/pglog/postgresql.log").unwrap_or_default();
        if log.contains("app=oo-prereq-rr") && log.contains("ALTER TABLE drift.item") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let secrets = load_secret_values();
    let mut hits = 0;
    for secret in &secrets {
        if !secret.is_empty() && log.contains(secret) {
            hits += 1;
        }
    }
    note.add("secret_hits", hits);
    if hits != 0 {
        return Err("log_secret".to_string());
    }
    if sql_path.is_empty() {
        return Err("sql_path".to_string());
    }
    let mut kept = Vec::new();
    for line in log.lines() {
        if line.contains("app=oo-prereq-") {
            kept.push(line.to_string());
        }
    }
    if kept.is_empty() {
        return Err("sql_log_empty".to_string());
    }
    note.add("sql_lines", kept.len());
    let mut body = kept.join("\n");
    body.push('\n');
    fs::write(sql_path, body).map_err(|_| "sql_write")?;
    Ok(())
}
