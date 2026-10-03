#![allow(clippy::cognitive_complexity)]
//! a121 missing-proof probe. It calls the public SnapshotOwner API only.
//! Preregistered file hashes are checked before the first open.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use oo_connectors::types::{ArrowScalar, CellPresence, DecodedCell, TypedBatch};
use oo_connectors::{
    config_identity, type_policy_identity, AdmittedCatalog, AdmittedColumn, AdmittedTable, CutProbe,
    PgBudget, PgCredentials, PgEndpoint, PgSourceConfig, PgTlsConfig, PgTypePolicy, ProbeValue,
    SecretRef, SelectedTable, SnapshotFinished, SnapshotOwner, SnapshotRequest, SourceError,
    SourceErrorKind, SourceId, TableSelection, CLEANUP_OBSERVATION_BOUND_MS, SNAPSHOT_PROTOCOL,
};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{ConnectOptions, Row};

const ROWS_SHA: &str = "99ff6b0be78bbf745cc1ce2d55efe05b9a39c92bc4c6f6c0d79dcd638c857e17";
const CATALOG_SHA: &str = "eb9a85123f3e9f4611c058bf0833cc4fb0bfebee39484ee3ac0cbc34becd44e9";
const WRITER_SHA: &str = "0eab79c34d8c9d14fe8f2157ac651f5967d205cd3bd205db7290fe06d551d79f";
const WRITER_1: &str = "UPDATE lot.batch SET title = 'aspen', \"odd\"\"mark\" = 'held' WHERE id = 11";
const WRITER_2: &str = "UPDATE lot.\"child row\" SET qty = 14 WHERE id = 31";
const WRITER_3: &str =
    "INSERT INTO lot.\"child row\" (id, batch_id, qty, note) VALUES (44, 11, 2, 'newleaf')";
const SOURCE: &str = "lot_src";
const SECRET: &str = "secret://a121/lot-reader";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a121_missing_proof() {
    let outcome = std::env::var("OO_A121_OUTCOME").unwrap_or_else(|_| "/target/phases/oracle-outcome.txt".to_string());
    let mut book = Book::default();
    book.line("probe=a121_oracle");
    book.line("api=SnapshotOwner");
    book.line(format!("cleanup_bound_ms={CLEANUP_OBSERVATION_BOUND_MS}"));
    let secrets = match load_secrets() {
        Ok(secrets) => secrets,
        Err(err) => {
            book.line(format!("load_error={err}"));
            book.mark("preregister", false);
            finish_book(&outcome, &mut book, &[]);
            panic!("a121 load {err}");
        }
    };
    let run = run(&mut book, &secrets).await;
    if let Err(err) = &run {
        book.line(format!("run_error={err}"));
    }
    finish_book(&outcome, &mut book, &secrets.passwords);
    if run.is_err() || !book.fails.is_empty() {
        panic!("a121 fails={}", book.fails.join(","));
    }
}

#[derive(Default)]
struct Book {
    lines: Vec<String>,
    fails: Vec<String>,
}

impl Book {
    fn line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    fn mark(&mut self, name: &str, ok: bool) {
        self.line(format!("mark {name}={}", if ok { "pass" } else { "fail" }));
        if !ok {
            self.fails.push(name.to_string());
        }
    }
}

struct Secrets {
    passwords: Vec<String>,
}

struct Env {
    host: String,
    port: u16,
    database: String,
    reader_user: String,
    writer_user: String,
    admin_user: String,
    reader_password: String,
    writer_password: String,
    admin_password: String,
    ca: Vec<u8>,
    log_file: String,
    expected_dir: String,
    postmaster: String,
}

struct ExpectRow {
    phase: String,
    table: String,
    key: i64,
    token: String,
}

struct ExpectCol {
    phase: String,
    schema: String,
    table: String,
    attnum: i32,
    name: String,
    type_oid: i64,
    typmod: i32,
    not_null: bool,
    key_ord: i32,
}

struct Expect {
    rows: Vec<ExpectRow>,
    columns: Vec<ExpectCol>,
}

struct Got {
    token: String,
    hash: String,
}

struct Attr {
    attnum: i32,
    name: String,
    type_oid: i64,
    typmod: i32,
    not_null: bool,
}

struct Priv {
    table_select: Option<bool>,
    table_insert: Option<bool>,
    col_id: Option<bool>,
    col_tag: Option<bool>,
    col_note: Option<bool>,
    superuser: bool,
    bypass: bool,
    inherit: bool,
    owner: String,
    members: i64,
    relacl: String,
    attacl: String,
}

async fn run(book: &mut Book, _secrets: &Secrets) -> Result<(), String> {
    let mut env = load_env()?;
    check_preregister(book, &env)?;
    let expect = load_expect(&env.expected_dir)?;
    let mut admin = connect(&env, &env.admin_user, &env.admin_password, "oo-a121-admin").await?;
    tune(&mut admin, 20_000, 15_000).await?;
    env.postmaster = text(&mut admin, "SELECT pg_catalog.pg_postmaster_start_time()::text").await?;
    let recovery = text(&mut admin, "SELECT pg_catalog.pg_is_in_recovery()::text").await?;
    book.line(format!("postmaster={}", env.postmaster));
    book.line(format!("recovery={recovery}"));
    book.mark("postmaster_recorded", !env.postmaster.is_empty() && recovery == "false");
    exec(&mut admin, "SELECT 'A121_AUDIT_WINDOW_START'").await?;
    if let Err(err) = same_cut(book, &env, &mut admin, &expect).await {
        book.line(format!("phase_cut_error={err}"));
        book.mark("phase_cut", false);
    }
    if let Err(err) = column_phase(book, &env, &mut admin, &expect).await {
        book.line(format!("phase_column_error={err}"));
        book.mark("phase_column", false);
    }
    if let Err(err) = deadline_phase(book, &env, &mut admin, &expect).await {
        book.line(format!("phase_deadline_error={err}"));
        book.mark("phase_deadline", false);
    }
    if let Err(err) = idle_phase(book, &env, &mut admin, &expect).await {
        book.line(format!("phase_idle_error={err}"));
        book.mark("phase_idle", false);
    }
    if let Err(err) = add_column_phase(book, &env, &mut admin, &expect).await {
        book.line(format!("phase_add_column_error={err}"));
        book.mark("phase_add_column", false);
    }
    if let Err(err) = identity_phase(book, &env, &mut admin, &expect).await {
        book.line(format!("phase_identity_error={err}"));
        book.mark("phase_identity", false);
    }
    nested_note(book);
    if let Err(err) = audit(book, &env, &mut admin).await {
        book.line(format!("phase_audit_error={err}"));
        book.mark("phase_audit", false);
    }
    Ok(())
}

fn check_preregister(book: &mut Book, env: &Env) -> Result<(), String> {
    let rows = file_sha(&format!("{}/rows.txt", env.expected_dir))?;
    let catalog = file_sha(&format!("{}/catalog.txt", env.expected_dir))?;
    let writer = file_sha(&format!("{}/writer.sql", env.expected_dir))?;
    book.line(format!("rows_sha256={rows}"));
    book.line(format!("catalog_sha256={catalog}"));
    book.line(format!("writer_sha256={writer}"));
    let writer_body = std::fs::read_to_string(format!("{}/writer.sql", env.expected_dir)).map_err(|_| "writer_read")?;
    let writer_ok = writer == WRITER_SHA && writer_body == format!("{WRITER_1}\n{WRITER_2}\n{WRITER_3}\n");
    let ok = rows == ROWS_SHA && catalog == CATALOG_SHA && writer_ok;
    book.mark("preregister", ok);
    if ok {
        Ok(())
    } else {
        Err("preregister_mismatch".to_string())
    }
}

async fn same_cut(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    let before = scalar_i64(admin, "SELECT hits::int8 FROM lot.tick WHERE n = 1").await?;
    book.line(format!("canary_before={before}"));
    exec(admin, "BEGIN").await?;
    let held = exec(admin, "SELECT id FROM lot.gate WHERE id = 1 FOR UPDATE").await;
    if let Err(err) = held {
        book.line(format!("gate_lock={err}"));
        forget_rollback(book, admin).await;
        book.mark("writer_blocked", false);
        return Ok(());
    }
    let writer_user = env.writer_user.clone();
    let writer_password = env.writer_password.clone();
    let host = env.host.clone();
    let database = env.database.clone();
    let port = env.port;
    let ca = env.ca.clone();
    let handle = tokio::spawn(async move {
        writer_tx(&host, port, &database, &writer_user, &writer_password, &ca).await
    });
    let blocked = wait_writer_block(book, admin).await?;
    book.line(format!("writer_blocked={blocked}"));
    book.mark("writer_blocked", blocked);
    if !blocked {
        forget_rollback(book, admin).await;
        stop_writer(book, handle).await;
        return Ok(());
    }
    let mut owner = match open_owner(env, admitted(expect, "cut"), 11, Duration::from_secs(40), Duration::from_secs(30)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("cut_open={}", fact_line(&err)));
            forget_rollback(book, admin).await;
            stop_writer(book, handle).await;
            book.mark("cut_three_sets", false);
            return Ok(());
        }
    };
    book.line("cut_owner_open=before_barrier_release");
    emit_binding(book, "cut_owner", &owner);
    let mut side = connect(env, &env.reader_user, &env.reader_password, "oo-a121-rr").await?;
    tune(&mut side, 20_000, 5_000).await?;
    exec(&mut side, "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY").await?;
    let mut side_set = BTreeSet::new();
    for row in expect.rows.iter().filter(|row| row.phase == "cut") {
        let token = read_token(&mut side, &row.table, row.key).await?;
        let item = member(&row.table, row.key, &token);
        book.line(format!("cut_side\t{item}"));
        side_set.insert(item);
    }
    exec(&mut side, "ROLLBACK").await?;
    book.line("cut_side_recorded=before_barrier_release");
    exec(admin, "UPDATE lot.gate SET ready = true WHERE id = 1").await?;
    exec(admin, "SELECT 'A121_BARRIER_RELEASE'").await?;
    exec(admin, "COMMIT").await?;
    book.line("barrier_released=after_owner_and_side");
    let writer_status = match tokio::time::timeout(Duration::from_secs(25), handle).await {
        Ok(Ok(Ok(status))) => status,
        Ok(Ok(Err(err))) => err,
        Ok(Err(_)) => "join".to_string(),
        Err(_) => "timeout".to_string(),
    };
    book.line(format!("writer_status={writer_status}"));
    let after_writer = scalar_i64(admin, "SELECT hits::int8 FROM lot.tick WHERE n = 1").await?;
    book.line(format!("canary_after_writer={after_writer}"));
    book.mark("canary_writer_delta", after_writer.saturating_sub(before) == 3);
    let mut owner_set = BTreeSet::new();
    for row in expect.rows.iter().filter(|row| row.phase == "cut") {
        let (schema, table) = place(&row.table);
        let got = probe_got(&mut owner, schema, table, row.key).await;
        let item = member(&row.table, row.key, &got.token);
        book.line(format!("cut_owner\t{item}"));
        book.line(format!("cut_owner_hash\t{}\t{}\t{}", row.table, row.key, got.hash));
        owner_set.insert(item);
    }
    let expected = expect_set(expect, "cut");
    emit_set(book, "cut_expected", &expected);
    let cut_ok = owner_set == side_set && side_set == expected;
    book.mark("cut_three_sets", cut_ok);
    emit_diff(book, "cut_owner_side", &owner_set, &side_set);
    emit_diff(book, "cut_owner_expected", &owner_set, &expected);
    let stop = owner.finish().await;
    let finished = stop.is_ok();
    book.line(format!("cut_finish={}", stop_text(&stop)));
    book.mark("cut_finish", finished);
    if let Err(err) = await_backend(book, admin, "cut", &stop).await {
        book.line(format!("cut_backend_observe={err}"));
    }
    fresh_cut(book, env, admin, expect).await?;
    let final_hits = scalar_i64(admin, "SELECT hits::int8 FROM lot.tick WHERE n = 1").await?;
    book.line(format!("canary_final={final_hits}"));
    book.mark("canary_oo_delta", final_hits == after_writer);
    Ok(())
}

async fn fresh_cut(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    let mut owner = match open_owner(env, admitted(expect, "cut"), 12, Duration::from_secs(30), Duration::from_secs(20)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("fresh_open={}", fact_line(&err)));
            book.mark("fresh_three_sets", false);
            return Ok(());
        }
    };
    emit_binding(book, "fresh_owner", &owner);
    let mut oracle = connect(env, &env.reader_user, &env.reader_password, "oo-a121-oracle").await?;
    tune(&mut oracle, 20_000, 5_000).await?;
    let mut owner_set = BTreeSet::new();
    let mut oracle_set = BTreeSet::new();
    for row in expect.rows.iter().filter(|row| row.phase == "fresh") {
        let (schema, table) = place(&row.table);
        let got = probe_got(&mut owner, schema, table, row.key).await;
        let item = member(&row.table, row.key, &got.token);
        book.line(format!("fresh_owner\t{item}"));
        book.line(format!("fresh_owner_hash\t{}\t{}\t{}", row.table, row.key, got.hash));
        owner_set.insert(item);
        let token = read_token(&mut oracle, &row.table, row.key).await?;
        let item = member(&row.table, row.key, &token);
        book.line(format!("fresh_oracle\t{item}"));
        oracle_set.insert(item);
    }
    let expected = expect_set(expect, "fresh");
    emit_set(book, "fresh_expected", &expected);
    book.mark("fresh_three_sets", owner_set == oracle_set && oracle_set == expected);
    emit_diff(book, "fresh_owner_oracle", &owner_set, &oracle_set);
    emit_diff(book, "fresh_owner_expected", &owner_set, &expected);
    let stop = owner.finish().await;
    book.line(format!("fresh_finish={}", stop_text(&stop)));
    book.mark("fresh_finish", stop.is_ok());
    if let Err(err) = await_backend(book, admin, "fresh", &stop).await {
        book.line(format!("fresh_backend_observe={err}"));
    }
    Ok(())
}

async fn column_phase(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    recover_admin(book, admin).await?;
    let before = privileges(admin, "acl", "sheet").await?;
    book.line(format!("sheet_before {}", priv_line(&before)));
    let fixture_ok = before.table_select == Some(false)
        && before.col_id == Some(true)
        && before.col_tag == Some(true)
        && before.col_note == Some(true)
        && !before.superuser
        && !before.bypass
        && !before.inherit
        && before.members == 0
        && before.owner != "lot_reader"
        && before.table_insert == Some(false);
    book.mark("column_only_fixture", fixture_ok);
    match open_owner(env, admitted(expect, "sheet"), 21, Duration::from_secs(20), Duration::from_secs(15)).await {
        Err(err) => {
            book.line(format!("column_only_open={}", fact_line(&err)));
            book.line("inflight_column_only_read=not_reached");
            book.mark("column_only_admission", err.fact() == Some("select_not_granted"));
            inflight_after_admission(book, env, admin, expect).await?;
        }
        Ok(owner) => {
            book.line("column_only_open=opened");
            book.mark("column_only_admission", false);
            revoke_note_and_probe(book, admin, owner, "sheet_unexpected_open", 4).await?;
        }
    }
    wide_control(book, env, admin, expect).await?;
    Ok(())
}

async fn inflight_after_admission(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    exec(admin, "GRANT SELECT ON TABLE acl.sheet TO lot_reader").await?;
    let mut owner = match open_owner(env, admitted(expect, "sheet"), 22, Duration::from_secs(20), Duration::from_secs(15)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("inflight_open={}", fact_line(&err)));
            book.mark("inflight_denial", false);
            return Ok(());
        }
    };
    let pid = owner.binding().backend.pid;
    let start = owner.binding().backend.backend_start.clone();
    exec(admin, "REVOKE SELECT ON TABLE acl.sheet FROM lot_reader").await?;
    exec(admin, "REVOKE SELECT (note) ON TABLE acl.sheet FROM lot_reader").await?;
    let after = privileges(admin, "acl", "sheet").await?;
    book.line(format!("sheet_after_revoke {}", priv_line(&after)));
    let got = probe_got(&mut owner, "acl", "sheet", 4).await;
    book.line(format!("inflight_probe={}", got.token));
    let stop = owner.finish().await;
    book.line(format!("inflight_finish={}", stop_text(&stop)));
    let gone = gone_ms(admin, pid, &start, &env.reader_user, &env.database).await?;
    book.line(format!("inflight_gone_ms={gone}"));
    let denied = got.token.contains("select_not_granted") || got.token.contains("42501");
    let no_table = after.table_select == Some(false) && after.col_note == Some(false);
    let columns_cleared = after.col_id == Some(false) && after.col_tag == Some(false) && after.col_note == Some(false);
    book.line(format!("inflight_no_table_select={no_table}"));
    book.line(format!("inflight_table_revoke_cleared_columns={columns_cleared}"));
    book.mark("inflight_denial", denied && stop.is_err() && no_table);
    book.mark("inflight_not_finished", stop.is_err());
    book.mark("inflight_backend_released", gone <= u128::from(CLEANUP_OBSERVATION_BOUND_MS));
    Ok(())
}

async fn revoke_note_and_probe(
    book: &mut Book,
    admin: &mut PgConnection,
    mut owner: SnapshotOwner,
    label: &str,
    key: i64,
) -> Result<(), String> {
    let pid = owner.binding().backend.pid;
    let start = owner.binding().backend.backend_start.clone();
    exec(admin, "REVOKE SELECT (note) ON TABLE acl.sheet FROM lot_reader").await?;
    let got = probe_got(&mut owner, "acl", "sheet", key).await;
    book.line(format!("{label}_probe={}", got.token));
    let stop = owner.finish().await;
    book.line(format!("{label}_finish={}", stop_text(&stop)));
    let gone = gone_ms(admin, pid, &start, "lot_reader", "lotdb").await?;
    book.line(format!("{label}_gone_ms={gone}"));
    book.mark("inflight_denial", got.token.starts_with("ERR") && stop.is_err());
    book.mark("inflight_backend_released", gone <= u128::from(CLEANUP_OBSERVATION_BOUND_MS));
    Ok(())
}

async fn wide_control(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    let before = privileges(admin, "acl", "wide").await?;
    book.line(format!("wide_before {}", priv_line(&before)));
    let mut owner = match open_owner(env, admitted(expect, "wide"), 23, Duration::from_secs(20), Duration::from_secs(15)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("wide_open={}", fact_line(&err)));
            book.mark("wide_explains_a116", false);
            return Ok(());
        }
    };
    exec(admin, "REVOKE SELECT (note) ON TABLE acl.wide FROM lot_reader").await?;
    let after = privileges(admin, "acl", "wide").await?;
    book.line(format!("wide_after_column_revoke {}", priv_line(&after)));
    let got = probe_got(&mut owner, "acl", "wide", 8).await;
    book.line(format!("wide_probe={}", got.token));
    let expected = expect.rows.iter().find(|row| row.phase == "wide" && row.key == 8).map(|row| row.token.as_str()).unwrap_or("");
    book.line(format!("wide_expected={expected}"));
    let stop = owner.finish().await;
    book.line(format!("wide_finish={}", stop_text(&stop)));
    let explains = after.table_select == Some(true)
        && after.col_note == Some(true)
        && got.token == expected
        && stop.is_ok();
    book.line(format!("a116_explanation={}", if explains { "table_select_keeps_column_select" } else { "not_reproduced" }));
    book.mark("wide_explains_a116", explains);
    if let Err(err) = await_backend(book, admin, "wide", &stop).await {
        book.line(format!("wide_backend_observe={err}"));
    }
    Ok(())
}

async fn recover_admin(book: &mut Book, admin: &mut PgConnection) -> Result<(), String> {
    let open = text(admin, "SELECT CASE WHEN pg_catalog.txid_current_if_assigned() IS NULL THEN 'no' ELSE 'yes' END").await?;
    if open == "yes" {
        exec(admin, "ROLLBACK").await?;
        book.line("admin_recovered_open_transaction=yes");
    }
    Ok(())
}

async fn deadline_phase(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    recover_admin(book, admin).await?;
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut owner = match open_at(env, admitted(expect, "hold"), 31, deadline, Duration::from_secs(30)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("deadline_open={}", fact_line(&err)));
            book.mark("deadline_before_idle", false);
            return Ok(());
        }
    };
    let pid = owner.binding().backend.pid;
    let start = owner.binding().backend.backend_start.clone();
    match bind_backend(admin, pid, &start, &env.reader_user, &env.database).await {
        Ok(line) => {
            book.line(format!("deadline_bind {line}"));
            book.mark("deadline_bind", true);
        }
        Err(err) => {
            book.line(format!("deadline_bind {err}"));
            book.mark("deadline_bind", false);
        }
    }
    let wait = deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(400);
    tokio::time::sleep(wait).await;
    let got = probe_got(&mut owner, "life", "hold", 1).await;
    book.line(format!("deadline_probe={}", got.token));
    let probe_deadline = got.token.contains("kind=Budget") && got.token.contains("fact=deadline") && !got.token.contains("25P03");
    let stop = owner.finish().await;
    book.line(format!("deadline_finish={}", stop_text(&stop)));
    let finish_deadline = matches!(stop.as_ref(), Err(err) if is_deadline(err));
    book.mark("deadline_before_idle", probe_deadline && finish_deadline);
    book.mark("deadline_not_finished", stop.is_err());
    let gone = gone_ms(admin, pid, &start, &env.reader_user, &env.database).await?;
    book.line(format!("deadline_gone_ms={gone}"));
    book.mark("deadline_backend_released", gone <= u128::from(CLEANUP_OBSERVATION_BOUND_MS));
    Ok(())
}

async fn idle_phase(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    recover_admin(book, admin).await?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let owner = match open_at(env, admitted(expect, "hold"), 32, deadline, Duration::from_secs(8)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("idle_open={}", fact_line(&err)));
            book.mark("idle_25p03", false);
            return Ok(());
        }
    };
    let pid = owner.binding().backend.pid;
    let start = owner.binding().backend.backend_start.clone();
    match bind_backend(admin, pid, &start, &env.reader_user, &env.database).await {
        Ok(line) => {
            book.line(format!("idle_bind {line}"));
            book.mark("idle_bind", true);
        }
        Err(err) => {
            book.line(format!("idle_bind {err}"));
            book.mark("idle_bind", false);
        }
    }
    let began = Instant::now();
    let mut absent = false;
    while began.elapsed() < Duration::from_secs(15) {
        let n = count_backend(admin, pid, &start, &env.reader_user, &env.database).await?;
        if n == 0 {
            absent = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    book.line(format!("idle_backend_absent_before_finish={absent}"));
    book.line(format!("idle_wait_ms={}", began.elapsed().as_millis()));
    let stop = owner.finish().await;
    book.line(format!("idle_finish={}", stop_text(&stop)));
    let saw = matches!(stop.as_ref(), Err(err) if is_25p03(err));
    book.mark("idle_25p03", saw);
    book.mark("idle_not_finished", stop.is_err());
    let gone = gone_ms(admin, pid, &start, &env.reader_user, &env.database).await?;
    book.line(format!("idle_gone_ms={gone}"));
    book.mark("idle_backend_released", gone <= u128::from(CLEANUP_OBSERVATION_BOUND_MS));
    Ok(())
}

async fn add_column_phase(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    recover_admin(book, admin).await?;
    let mut owner = match open_owner(env, admitted(expect, "drift"), 41, Duration::from_secs(30), Duration::from_secs(20)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("drift_open={}", fact_line(&err)));
            book.mark("add_column_active_current_schema", false);
            return Ok(());
        }
    };
    emit_binding(book, "drift_old_owner", &owner);
    exec(admin, "SELECT pg_catalog.set_config('lock_timeout', '3000', false)").await?;
    let altered = exec(admin, "ALTER TABLE drift.item ADD COLUMN extra text").await;
    exec(admin, "SELECT pg_catalog.set_config('lock_timeout', '15000', false)").await?;
    if let Err(err) = altered {
        book.line(format!("add_column_alter={err}"));
        let locks = text(admin, "SELECT COALESCE(string_agg(c.relname || ':' || l.mode, ','), '') FROM pg_locks l JOIN pg_class c ON c.oid = l.relation WHERE c.relname = 'item'").await.unwrap_or_else(std::convert::identity);
        book.line(format!("add_column_locks={locks}"));
        let stop = owner.finish().await;
        book.line(format!("drift_finish={}", stop_text(&stop)));
        book.mark("add_column_alter_applied", false);
        book.mark("add_column_active_current_schema", false);
        return Ok(());
    }
    book.line("add_column_alter=applied");
    book.mark("add_column_alter_applied", true);
    let attrs = attributes(admin, "drift", "item").await?;
    for attr in &attrs {
        book.line(format!(
            "drift_admin_col\t{}\t{}\t{}\t{}\t{}",
            attr.attnum, attr.name, attr.type_oid, attr.typmod, attr.not_null
        ));
    }
    let got = probe_got(&mut owner, "drift", "item", 5).await;
    book.line(format!("drift_old_probe={}", got.token));
    book.line(format!("drift_old_hash={}", got.hash));
    let old_expected = expect.rows.iter().find(|row| row.phase == "drift" && row.key == 5).map(|row| row.token.as_str()).unwrap_or("");
    book.line(format!("drift_old_expected={old_expected}"));
    let stop = owner.finish().await;
    let finished = stop.is_ok();
    book.line(format!("drift_old_finish={}", stop_text(&stop)));
    let extra = attrs.iter().any(|attr| attr.name == "extra");
    book.line(format!("drift_admin_has_extra={extra}"));
    book.line(format!("drift_old_finished_with_extra={}", finished && extra));
    book.mark("add_column_active_current_schema", !finished || !extra);
    if let Err(err) = await_backend(book, admin, "drift_old", &stop).await {
        book.line(format!("drift_old_backend_observe={err}"));
    }
    let old_open = open_owner(env, admitted(expect, "drift"), 42, Duration::from_secs(20), Duration::from_secs(15)).await;
    match old_open {
        Ok(owner) => {
            book.line("fresh_old_shape=opened");
            let stop = owner.finish().await;
            book.line(format!("fresh_old_shape_finish={}", stop_text(&stop)));
            book.mark("fresh_old_shape_rejected", false);
        }
        Err(err) => {
            book.line(format!("fresh_old_shape={}", fact_line(&err)));
            book.mark("fresh_old_shape_rejected", err.fact() == Some("catalog_identity"));
        }
    }
    let shape = if attrs_match_phase(&attrs, expect, "drift_new") {
        book.line("fresh_new_shape_source=preregistered");
        admitted(expect, "drift_new")
    } else {
        book.line("fresh_new_shape_source=observed_admin");
        vec![table_from_attrs("drift", "item", &attrs)]
    };
    let mut fresh = match open_owner(env, shape, 43, Duration::from_secs(20), Duration::from_secs(15)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("fresh_new_open={}", fact_line(&err)));
            book.mark("fresh_new_token", false);
            return Ok(());
        }
    };
    emit_binding(book, "fresh_new", &fresh);
    let got = probe_got(&mut fresh, "drift", "item", 5).await;
    book.line(format!("fresh_new_probe={}", got.token));
    book.line(format!("fresh_new_hash={}", got.hash));
    let mut oracle = connect(env, &env.reader_user, &env.reader_password, "oo-a121-oracle").await?;
    tune(&mut oracle, 10_000, 5_000).await?;
    let oracle_token = read_token(&mut oracle, "item", 5).await?;
    book.line(format!("fresh_new_oracle={oracle_token}"));
    let expected = expect.rows.iter().find(|row| row.phase == "drift_new" && row.key == 5).map(|row| row.token.as_str()).unwrap_or("");
    book.line(format!("fresh_new_expected={expected}"));
    book.mark("fresh_new_token", got.token == oracle_token && got.token == expected);
    let stop = fresh.finish().await;
    book.line(format!("fresh_new_finish={}", stop_text(&stop)));
    Ok(())
}

async fn identity_phase(book: &mut Book, env: &Env, admin: &mut PgConnection, expect: &Expect) -> Result<(), String> {
    recover_admin(book, admin).await?;
    book.line("identity_drift_attempt=alter_role_rename");
    let mut owner = match open_owner(env, admitted(expect, "hold"), 51, Duration::from_secs(30), Duration::from_secs(20)).await {
        Ok(owner) => owner,
        Err(err) => {
            book.line(format!("identity_open={}", fact_line(&err)));
            book.line("identity_drift=not_runtime_tested");
            book.line("identity_reason=open_failed_before_rename");
            return Ok(());
        }
    };
    let pid = owner.binding().backend.pid;
    let start = owner.binding().backend.backend_start.clone();
    match exec(admin, "ALTER ROLE lot_reader RENAME TO lot_reader_x").await {
        Err(err) => {
            book.line(format!("identity_rename_sqlstate={err}"));
            book.line("identity_drift=not_runtime_tested");
            book.line("identity_reason=rename_rejected_before_probe");
            let stop = owner.finish().await;
            book.line(format!("identity_finish={}", stop_text(&stop)));
            note_gone(book, "identity", admin, pid, &start, &env.reader_user, &env.database).await;
        }
        Ok(()) => {
            let got = probe_got(&mut owner, "life", "hold", 1).await;
            book.line(format!("identity_probe={}", got.token));
            let stop = owner.finish().await;
            book.line(format!("identity_finish={}", stop_text(&stop)));
            let drifted = got.token.contains("identity_drift") || stop_text(&stop).contains("identity_drift");
            if drifted {
                book.line("identity_drift=runtime_observed");
                book.mark("identity_drift_stops_finish", stop.is_err());
            } else {
                book.line("identity_drift=not_runtime_tested");
                book.line("identity_reason=rename_executed_session_identity_unchanged");
            }
            let restore = match exec(admin, "ALTER ROLE lot_reader_x RENAME TO lot_reader").await {
                Ok(()) => "ok".to_string(),
                Err(err) => err,
            };
            book.line(format!("identity_restore={restore}"));
            let user = if drifted { "lot_reader_x" } else { env.reader_user.as_str() };
            note_gone(book, "identity", admin, pid, &start, user, &env.database).await;
        }
    }
    Ok(())
}

fn nested_note(book: &mut Book) {
    book.line("nested_transaction=not_runtime_tested");
    book.line("nested_reason=no_public_second_begin");
    book.line("nested_source=session.rs begin is pub(crate) and returns nested_transaction before SQL when in_transaction is set");
    book.line("nested_reach=an integration test cannot call begin; SnapshotOwner::open starts one transaction and does not expose the connection");
    book.line("nested_certification=source_inspection_is_not_runtime_proof");
}

async fn audit(book: &mut Book, env: &Env, admin: &mut PgConnection) -> Result<(), String> {
    recover_admin(book, admin).await?;
    exec(admin, "SELECT 'A121_AUDIT_WINDOW_END'").await?;
    let mut text = String::new();
    for _ in 0..10 {
        text = std::fs::read_to_string(&env.log_file).map_err(|_| "log_read")?;
        if text.contains("A121_AUDIT_WINDOW_END") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let mut hits = 0usize;
    for secret in [&env.reader_password, &env.writer_password, &env.admin_password] {
        hits += text.matches(secret.as_str()).count();
    }
    book.line(format!("log_secret_hits={hits}"));
    book.mark("log_secret_absent", hits == 0);
    let report = classify(&text);
    book.line(format!("audit_window={}", report.window));
    book.line(format!("audit_oo_after_marker={}", report.marker_before));
    book.line(hist_line("audit_oo", &report.oo));
    book.line(hist_line("audit_writer", &report.writer));
    book.line(hist_line("audit_side", &report.side));
    book.line(hist_line("audit_admin", &report.admin));
    book.line(hist_line("audit_oracle", &report.oracle));
    book.line(format!("audit_oo_forbidden={}", report.oo_forbidden));
    book.line(format!("audit_oo_unknown={}", if report.oo_unknown.is_empty() { "-".to_string() } else { report.oo_unknown.clone() }));
    book.line(format!("audit_other={}", report.other));
    book.line(format!("audit_other_sample={}", if report.other_sample.is_empty() { "-".to_string() } else { report.other_sample }));
    book.line(format!(
        "audit_order owner_start={} side_select={} barrier={} writer_commit={} owner_probe={}",
        report.owner_start, report.side_select, report.barrier, report.writer_commit, report.owner_probe
    ));
    let order = report.owner_start >= 0
        && report.side_select > report.owner_start
        && report.barrier > report.side_select
        && report.writer_commit > report.barrier
        && report.owner_probe > report.writer_commit;
    book.mark("cut_log_order", order);
    let selects = report.oo.get("SELECT").copied().unwrap_or(0);
    book.line(format!("audit_oo_selects={selects}"));
    book.mark(
        "sql_audit",
        report.window && report.marker_before && selects > 0 && report.oo_forbidden == 0 && report.oo_unknown.is_empty(),
    );
    Ok(())
}

async fn forget_rollback(book: &mut Book, admin: &mut PgConnection) {
    if let Err(err) = exec(admin, "ROLLBACK").await {
        book.line(format!("rollback_after_error={err}"));
    }
}

async fn stop_writer(book: &mut Book, handle: tokio::task::JoinHandle<Result<String, String>>) {
    let status = match tokio::time::timeout(Duration::from_secs(20), handle).await {
        Ok(Ok(Ok(status))) => status,
        Ok(Ok(Err(err))) => err,
        Ok(Err(_)) => "join".to_string(),
        Err(_) => "timeout".to_string(),
    };
    book.line(format!("writer_stopped={status}"));
}

async fn note_gone(book: &mut Book, label: &str, conn: &mut PgConnection, pid: i64, start: &str, user: &str, db: &str) {
    match gone_ms(conn, pid, start, user, db).await {
        Ok(ms) => book.line(format!("{label}_gone_ms={ms}")),
        Err(err) => book.line(format!("{label}_gone={err}")),
    }
}

async fn writer_tx(host: &str, port: u16, database: &str, user: &str, password: &str, ca: &[u8]) -> Result<String, String> {
    let mut conn = connect_parts(host, port, database, user, password, ca, "oo-a121-writer").await?;
    tune(&mut conn, 20_000, 20_000).await?;
    exec(&mut conn, "BEGIN").await?;
    exec(&mut conn, "SELECT id FROM lot.gate WHERE id = 1 FOR UPDATE").await?;
    let ready = text(&mut conn, "SELECT CASE WHEN ready THEN 'true' ELSE 'false' END FROM lot.gate WHERE id = 1").await?;
    if ready != "true" {
        exec(&mut conn, "ROLLBACK").await?;
        return Ok("writer_not_released".to_string());
    }
    exec(&mut conn, WRITER_1).await?;
    exec(&mut conn, WRITER_2).await?;
    exec(&mut conn, WRITER_3).await?;
    exec(&mut conn, "COMMIT").await?;
    Ok("writer_committed".to_string())
}

async fn wait_writer_block(book: &mut Book, admin: &mut PgConnection) -> Result<bool, String> {
    let mode = text(admin, "SELECT current_setting('stats_fetch_consistency')").await?;
    book.line(format!("stats_fetch_consistency={mode}"));
    for _ in 0..100 {
        let n = scalar_i64(admin, "SELECT count(*)::int8 FROM pg_catalog.pg_locks WHERE NOT granted").await?;
        if n > 0 {
            let seen = text(admin, "SELECT COALESCE(string_agg(pid::text || ':' || application_name || ':' || COALESCE(wait_event_type, '') || ':' || COALESCE(wait_event, '') || ':' || COALESCE(state, ''), ','), 'none') FROM pg_stat_activity WHERE datname = current_database()").await?;
            book.line(format!("writer_activity={seen}"));
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let seen = text(admin, "SELECT COALESCE(string_agg(pid::text || ':' || application_name || ':' || COALESCE(wait_event_type, '') || ':' || COALESCE(wait_event, '') || ':' || COALESCE(state, ''), ','), 'none') FROM pg_stat_activity WHERE datname = current_database()").await?;
    book.line(format!("writer_activity={seen}"));
    Ok(false)
}

async fn open_owner(env: &Env, tables: Vec<AdmittedTable>, generation: u64, deadline: Duration, statement: Duration) -> Result<SnapshotOwner, SourceError> {
    open_at(env, tables, generation, Instant::now() + deadline, statement).await
}

async fn open_at(env: &Env, tables: Vec<AdmittedTable>, generation: u64, deadline: Instant, statement: Duration) -> Result<SnapshotOwner, SourceError> {
    let request = request(env, tables, generation, deadline, statement)?;
    SnapshotOwner::open(request).await
}

fn request(env: &Env, tables: Vec<AdmittedTable>, generation: u64, deadline: Instant, statement: Duration) -> Result<SnapshotRequest, SourceError> {
    let mut selected = Vec::new();
    for table in &tables {
        selected.push(SelectedTable::new(table.schema.as_str(), table.name.as_str())?);
    }
    let config = PgSourceConfig::try_new(
        SourceId::new(SOURCE)?,
        PgEndpoint::new(&env.host, env.port, &env.database)?,
        PgCredentials::new(&env.reader_user, &env.reader_password)?,
        SecretRef::new(SECRET)?,
        PgTlsConfig::from_ca_pem(&env.ca)?,
        PgBudget::new(Duration::from_secs(5), statement, Duration::from_secs(5), 1_000, 1024 * 1024)?,
        TableSelection::new(selected)?,
    )?;
    let config_identity = config_identity(&config);
    let type_policy_identity = type_policy_identity(PgTypePolicy::PRESERVE_ROW);
    Ok(SnapshotRequest {
        config,
        admitted: AdmittedCatalog { tables },
        config_identity,
        type_policy_identity,
        policy: PgTypePolicy::PRESERVE_ROW,
        generation,
        deadline,
        expected_principal: env.reader_user.clone(),
        expected_database: env.database.clone(),
        expected_in_recovery: false,
        expected_postmaster_start: env.postmaster.clone(),
    })
}

fn admitted(expect: &Expect, phase: &str) -> Vec<AdmittedTable> {
    let mut tables: BTreeMap<(String, String), AdmittedTable> = BTreeMap::new();
    let mut keys: BTreeMap<(String, String), BTreeMap<i32, String>> = BTreeMap::new();
    for col in expect.columns.iter().filter(|col| col.phase == phase) {
        let key = (col.schema.clone(), col.table.clone());
        tables.entry(key.clone()).or_insert_with(|| AdmittedTable {
            schema: col.schema.clone(),
            name: col.table.clone(),
            columns: Vec::new(),
            primary_key: Vec::new(),
        });
        if let Some(table) = tables.get_mut(&key) {
            table.columns.push(AdmittedColumn {
                attnum: col.attnum,
                name: col.name.clone(),
                type_oid: col.type_oid,
                typmod: col.typmod,
                not_null: col.not_null,
            });
        }
        if col.key_ord > 0 {
            keys.entry(key).or_default().insert(col.key_ord, col.name.clone());
        }
    }
    for (key, table) in &mut tables {
        table.columns.sort_by_key(|col| col.attnum);
        if let Some(ords) = keys.get(key) {
            table.primary_key = ords.values().cloned().collect();
        }
    }
    tables.into_values().collect()
}

fn table_from_attrs(schema: &str, name: &str, attrs: &[Attr]) -> AdmittedTable {
    let mut columns = Vec::new();
    let mut primary_key = Vec::new();
    for attr in attrs {
        if attr.name == "id" {
            primary_key.push(attr.name.clone());
        }
        columns.push(AdmittedColumn {
            attnum: attr.attnum,
            name: attr.name.clone(),
            type_oid: attr.type_oid,
            typmod: attr.typmod,
            not_null: attr.not_null,
        });
    }
    AdmittedTable {
        schema: schema.to_string(),
        name: name.to_string(),
        columns,
        primary_key,
    }
}

fn attrs_match_phase(attrs: &[Attr], expect: &Expect, phase: &str) -> bool {
    let want: Vec<&ExpectCol> = expect.columns.iter().filter(|col| col.phase == phase).collect();
    if want.len() != attrs.len() {
        return false;
    }
    want.iter().zip(attrs.iter()).all(|(col, attr)| {
        col.attnum == attr.attnum && col.name == attr.name && col.type_oid == attr.type_oid && col.typmod == attr.typmod && col.not_null == attr.not_null
    })
}

async fn probe_got(owner: &mut SnapshotOwner, schema: &str, table: &str, key: i64) -> Got {
    let probe = CutProbe {
        schema: schema.to_string(),
        table: table.to_string(),
        key: vec![ProbeValue::Int8(key)],
    };
    match owner.read_probe(probe).await {
        Ok(batch) => batch_got(&batch),
        Err(err) => Got { token: format!("ERR {}", fact_line(&err)), hash: "-".to_string() },
    }
}

fn batch_got(batch: &TypedBatch) -> Got {
    if batch.rows.is_empty() {
        return Got { token: "ABSENT".to_string(), hash: "-".to_string() };
    }
    if batch.rows.len() != 1 {
        return Got { token: format!("ERR row_count={}", batch.rows.len()), hash: "-".to_string() };
    }
    let hash = hex_bytes(&batch.rows[0].canonical_hash);
    match tokens_of(batch) {
        Ok(token) => Got { token, hash },
        Err(err) => Got { token: format!("ERR {err}"), hash },
    }
}

fn tokens_of(batch: &TypedBatch) -> Result<String, &'static str> {
    let cells = &batch.rows[0].cells;
    if cells.len() != batch.arrow.len() {
        return Err("width");
    }
    let mut parts = Vec::new();
    for (cell, column) in cells.iter().zip(batch.arrow.iter()) {
        if column.values.len() != 1 {
            return Err("arrow_len");
        }
        let token = arrow_token(&column.values[0])?;
        if !presence_ok(cell, &token) {
            return Err("presence");
        }
        parts.push(token);
    }
    Ok(parts.join(" "))
}

fn arrow_token(value: &ArrowScalar) -> Result<String, &'static str> {
    match value {
        ArrowScalar::Bool(true) => Ok("bool:true".to_string()),
        ArrowScalar::Bool(false) => Ok("bool:false".to_string()),
        ArrowScalar::Int16(value) => Ok(format!("int:{value}")),
        ArrowScalar::Int32(value) => Ok(format!("int:{value}")),
        ArrowScalar::Int64(value) => Ok(format!("int:{value}")),
        ArrowScalar::Utf8(value) => Ok(format!("text:{value}")),
        ArrowScalar::Null => Ok("null".to_string()),
        _ => Err("arrow"),
    }
}

fn presence_ok(cell: &DecodedCell, token: &str) -> bool {
    if token == "null" {
        return cell.presence == CellPresence::Null && matches!(cell.arrow, Some(ArrowScalar::Null));
    }
    cell.presence == CellPresence::Value && cell.arrow.is_some()
}

async fn read_token(conn: &mut PgConnection, table: &str, key: i64) -> Result<String, String> {
    let key32 = i32::try_from(key).map_err(|_| "key")?;
    match table {
        "batch" => batch_token(conn, key32).await,
        "child row" => child_token(conn, key32).await,
        "sheet" => text_row(conn, "SELECT tag, note FROM acl.sheet WHERE id = $1", key32).await,
        "wide" => text_row(conn, "SELECT tag, note FROM acl.wide WHERE id = $1", key32).await,
        "item" => item_token(conn, key32).await,
        "hold" => label_token(conn, "SELECT label FROM life.hold WHERE id = $1", key32).await,
        _ => Err("table".to_string()),
    }
}

async fn batch_token(conn: &mut PgConnection, key: i32) -> Result<String, String> {
    let row = sqlx::query("SELECT title, ready, code, \"odd\"\"mark\" FROM lot.batch WHERE id = $1")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|err| sqlstate_only(&err))?;
    let Some(row) = row else {
        return Ok("ABSENT".to_string());
    };
    let title: String = row.try_get(0).map_err(|_| "decode")?;
    let ready: bool = row.try_get(1).map_err(|_| "decode")?;
    let code: String = row.try_get(2).map_err(|_| "decode")?;
    let mark: Option<String> = row.try_get(3).map_err(|_| "decode")?;
    let ready_token = if ready { "true" } else { "false" };
    let mark_token = match mark {
        Some(value) => format!("text:{value}"),
        None => "null".to_string(),
    };
    Ok(format!("int:{key} text:{title} bool:{ready_token} text:{code} {mark_token}"))
}

async fn child_token(conn: &mut PgConnection, key: i32) -> Result<String, String> {
    let row = sqlx::query("SELECT batch_id, qty, note FROM lot.\"child row\" WHERE id = $1")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|err| sqlstate_only(&err))?;
    let Some(row) = row else {
        return Ok("ABSENT".to_string());
    };
    let batch_id: i32 = row.try_get(0).map_err(|_| "decode")?;
    let qty: i16 = row.try_get(1).map_err(|_| "decode")?;
    let note: Option<String> = row.try_get(2).map_err(|_| "decode")?;
    let note_token = match note {
        Some(value) => format!("text:{value}"),
        None => "null".to_string(),
    };
    Ok(format!("int:{key} int:{batch_id} int:{qty} {note_token}"))
}

async fn text_row(conn: &mut PgConnection, sql: &'static str, key: i32) -> Result<String, String> {
    let row = sqlx::query(sql).bind(key).fetch_optional(&mut *conn).await.map_err(|err| sqlstate_only(&err))?;
    let Some(row) = row else {
        return Ok("ABSENT".to_string());
    };
    let tag: String = row.try_get(0).map_err(|_| "decode")?;
    let note: Option<String> = row.try_get(1).map_err(|_| "decode")?;
    let note_token = match note {
        Some(value) => format!("text:{value}"),
        None => "null".to_string(),
    };
    Ok(format!("int:{key} text:{tag} {note_token}"))
}

async fn item_token(conn: &mut PgConnection, key: i32) -> Result<String, String> {
    let widened = sqlx::query(
        "SELECT a.attname FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'drift' AND c.relname = 'item' AND a.attnum > 0 AND NOT a.attisdropped AND a.attname = 'extra'",
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|err| sqlstate_only(&err))?;
    if widened.is_some() {
        let row = sqlx::query("SELECT label, extra FROM drift.item WHERE id = $1")
            .bind(key)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|err| sqlstate_only(&err))?;
        let Some(row) = row else {
            return Ok("ABSENT".to_string());
        };
        let label: String = row.try_get(0).map_err(|_| "decode")?;
        let extra: Option<String> = row.try_get(1).map_err(|_| "decode")?;
        let extra_token = match extra {
            Some(value) => format!("text:{value}"),
            None => "null".to_string(),
        };
        return Ok(format!("int:{key} text:{label} {extra_token}"));
    }
    label_token(conn, "SELECT label FROM drift.item WHERE id = $1", key).await
}

async fn label_token(conn: &mut PgConnection, sql: &'static str, key: i32) -> Result<String, String> {
    let row = sqlx::query(sql).bind(key).fetch_optional(&mut *conn).await.map_err(|err| sqlstate_only(&err))?;
    let Some(row) = row else {
        return Ok("ABSENT".to_string());
    };
    let label: String = row.try_get(0).map_err(|_| "decode")?;
    Ok(format!("int:{key} text:{label}"))
}

async fn privileges(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Priv, String> {
    let rel = format!("{schema}.{table}");
    let row = sqlx::query(
        "SELECT has_table_privilege('lot_reader', $1, 'SELECT'), has_table_privilege('lot_reader', $1, 'INSERT'), \
         has_column_privilege('lot_reader', $1, 'id', 'SELECT'), has_column_privilege('lot_reader', $1, 'tag', 'SELECT'), \
         has_column_privilege('lot_reader', $1, 'note', 'SELECT'), r.rolsuper, r.rolbypassrls, r.rolinherit, o.rolname, \
         (SELECT count(*)::int8 FROM pg_catalog.pg_auth_members m WHERE m.member = r.oid) \
         FROM pg_catalog.pg_roles r \
         JOIN pg_catalog.pg_class c ON true \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace AND n.nspname = $2 AND c.relname = $3 \
         JOIN pg_catalog.pg_roles o ON o.oid = c.relowner \
         WHERE r.rolname = 'lot_reader'",
    )
    .bind(&rel)
    .bind(schema)
    .bind(table)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| sqlstate_only(&err))?;
    let relacl = text_bind(conn, "SELECT COALESCE(string_agg(e.privilege_type, ',' ORDER BY e.privilege_type), '') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN pg_catalog.aclexplode(c.relacl) e ON true JOIN pg_catalog.pg_roles g ON g.oid = e.grantee WHERE n.nspname = $1 AND c.relname = $2 AND g.rolname = 'lot_reader'", schema, table).await?;
    let attacl = text_bind(conn, "SELECT COALESCE(string_agg(a.attname || ':' || e.privilege_type, ',' ORDER BY a.attnum, e.privilege_type), '') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped JOIN pg_catalog.aclexplode(a.attacl) e ON true JOIN pg_catalog.pg_roles g ON g.oid = e.grantee WHERE n.nspname = $1 AND c.relname = $2 AND g.rolname = 'lot_reader'", schema, table).await?;
    Ok(Priv {
        table_select: row.try_get(0).map_err(|_| "decode")?,
        table_insert: row.try_get(1).map_err(|_| "decode")?,
        col_id: row.try_get(2).map_err(|_| "decode")?,
        col_tag: row.try_get(3).map_err(|_| "decode")?,
        col_note: row.try_get(4).map_err(|_| "decode")?,
        superuser: row.try_get(5).map_err(|_| "decode")?,
        bypass: row.try_get(6).map_err(|_| "decode")?,
        inherit: row.try_get(7).map_err(|_| "decode")?,
        owner: row.try_get(8).map_err(|_| "decode")?,
        members: row.try_get(9).map_err(|_| "decode")?,
        relacl,
        attacl,
    })
}

fn priv_line(privs: &Priv) -> String {
    format!(
        "table_select={} table_insert={} col_id={} col_tag={} col_note={} super={} bypass={} inherit={} owner={} members={} relacl={} attacl={}",
        opt_bool(privs.table_select),
        opt_bool(privs.table_insert),
        opt_bool(privs.col_id),
        opt_bool(privs.col_tag),
        opt_bool(privs.col_note),
        privs.superuser,
        privs.bypass,
        privs.inherit,
        privs.owner,
        privs.members,
        privs.relacl,
        privs.attacl
    )
}

fn opt_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

async fn attributes(conn: &mut PgConnection, schema: &str, table: &str) -> Result<Vec<Attr>, String> {
    let rows = sqlx::query(
        "SELECT a.attnum::int4, a.attname, a.atttypid::int8, a.atttypmod::int4, a.attnotnull \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY a.attnum",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    .map_err(|err| sqlstate_only(&err))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(Attr {
            attnum: row.try_get(0).map_err(|_| "decode")?,
            name: row.try_get(1).map_err(|_| "decode")?,
            type_oid: row.try_get(2).map_err(|_| "decode")?,
            typmod: row.try_get(3).map_err(|_| "decode")?,
            not_null: row.try_get(4).map_err(|_| "decode")?,
        });
    }
    Ok(out)
}

async fn bind_backend(conn: &mut PgConnection, pid: i64, start: &str, user: &str, db: &str) -> Result<String, String> {
    let row = sqlx::query(
        "SELECT pid::int8, backend_start::text, usename::text, datname::text, application_name \
         FROM pg_stat_activity \
         WHERE pid = $1 AND backend_start::text = $2 AND usename::text = $3 AND datname::text = $4 AND application_name = 'oo-pg-source'",
    )
    .bind(pid)
    .bind(start)
    .bind(user)
    .bind(db)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|err| sqlstate_only(&err))?;
    if let Some(row) = row {
        let got_pid: i64 = row.try_get(0).map_err(|_| "decode")?;
        let got_start: String = row.try_get(1).map_err(|_| "decode")?;
        let got_user: String = row.try_get(2).map_err(|_| "decode")?;
        let got_db: String = row.try_get(3).map_err(|_| "decode")?;
        let got_app: String = row.try_get(4).map_err(|_| "decode")?;
        return Ok(format!("pid={got_pid} start={got_start} user={got_user} db={got_db} app={got_app}"));
    }
    let loose = sqlx::query(
        "SELECT backend_start::text, usename::text, datname::text, application_name FROM pg_stat_activity WHERE pid = $1",
    )
    .bind(pid)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|err| sqlstate_only(&err))?;
    if let Some(row) = loose {
        let got_start: String = row.try_get(0).map_err(|_| "decode")?;
        let got_user: String = row.try_get(1).map_err(|_| "decode")?;
        let got_db: String = row.try_get(2).map_err(|_| "decode")?;
        let got_app: String = row.try_get(3).map_err(|_| "decode")?;
        return Err(format!("bind_mismatch start={got_start} user={got_user} db={got_db} app={got_app} want_start={start}"));
    }
    Err("bind_absent".to_string())
}

async fn count_backend(conn: &mut PgConnection, pid: i64, start: &str, user: &str, db: &str) -> Result<i64, String> {
    let row = sqlx::query(
        "SELECT count(*)::int8 FROM pg_stat_activity WHERE pid = $1 AND backend_start::text = $2 AND usename::text = $3 AND datname::text = $4 AND application_name = 'oo-pg-source'",
    )
    .bind(pid)
    .bind(start)
    .bind(user)
    .bind(db)
    .fetch_one(&mut *conn)
    .await
    .map_err(|err| sqlstate_only(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

async fn gone_ms(conn: &mut PgConnection, pid: i64, start: &str, user: &str, db: &str) -> Result<u128, String> {
    let began = Instant::now();
    let bound = Duration::from_millis(CLEANUP_OBSERVATION_BOUND_MS);
    loop {
        let n = count_backend(conn, pid, start, user, db).await?;
        if n == 0 {
            return Ok(began.elapsed().as_millis());
        }
        if began.elapsed() > bound {
            return Ok(began.elapsed().as_millis());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn await_backend(book: &mut Book, admin: &mut PgConnection, label: &str, stop: &Result<SnapshotFinished, SourceError>) -> Result<(), String> {
    let Ok(fin) = stop else {
        book.line(format!("{label}_backend=untracked_on_error"));
        return Ok(());
    };
    let gone = gone_ms(admin, fin.backend.pid, &fin.backend.backend_start, "lot_reader", "lotdb").await?;
    book.line(format!("{label}_gone_ms={gone}"));
    book.mark(&format!("{label}_backend_released"), gone <= u128::from(CLEANUP_OBSERVATION_BOUND_MS));
    Ok(())
}

fn emit_binding(book: &mut Book, label: &str, owner: &SnapshotOwner) {
    let binding = owner.binding();
    book.line(format!("{label}_protocol={}", binding.protocol));
    book.line(format!("{label}_protocol_ok={}", binding.protocol == SNAPSHOT_PROTOCOL));
    book.line(format!("{label}_generation={}", binding.generation));
    book.line(format!("{label}_database={}", binding.database));
    book.line(format!("{label}_principal={}", binding.principal));
    book.line(format!("{label}_backend_pid={}", binding.backend.pid));
    book.line(format!("{label}_backend_start={}", binding.backend.backend_start));
    book.line(format!("{label}_schema_hash={}", binding.schema_hash));
    for table in &binding.catalog.tables {
        let pk = table.primary_key.iter().map(|key| key.name.as_str()).collect::<Vec<_>>().join(",");
        book.line(format!("{label}_pk\t{}\t{}\t{pk}", table.schema, table.name));
        for column in &table.columns {
            book.line(format!(
                "{label}_col\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                table.schema, table.name, column.attnum, column.name, column.type_oid, column.typmod, column.not_null
            ));
        }
    }
}

fn expect_set(expect: &Expect, phase: &str) -> BTreeSet<String> {
    expect.rows.iter().filter(|row| row.phase == phase).map(|row| member(&row.table, row.key, &row.token)).collect()
}

fn emit_set(book: &mut Book, label: &str, set: &BTreeSet<String>) {
    for item in set {
        book.line(format!("{label}\t{item}"));
    }
}

fn emit_diff(book: &mut Book, label: &str, left: &BTreeSet<String>, right: &BTreeSet<String>) {
    for item in left.difference(right) {
        book.line(format!("{label}_only_left\t{item}"));
    }
    for item in right.difference(left) {
        book.line(format!("{label}_only_right\t{item}"));
    }
}

fn member(table: &str, key: i64, token: &str) -> String {
    format!("{table}|{key}|{token}")
}

fn place(table: &str) -> (&'static str, &'static str) {
    match table {
        "batch" => ("lot", "batch"),
        "child row" => ("lot", "child row"),
        "sheet" => ("acl", "sheet"),
        "wide" => ("acl", "wide"),
        "item" => ("drift", "item"),
        "hold" => ("life", "hold"),
        _ => ("", ""),
    }
}

fn fact_line(err: &SourceError) -> String {
    format!("kind={:?} fact={} sqlstate={}", err.kind(), err.fact().unwrap_or("-"), err.sqlstate().unwrap_or("-"))
}

fn stop_text(stop: &Result<SnapshotFinished, SourceError>) -> String {
    match stop {
        Ok(fin) => format!("SnapshotFinished generation={} pid={}", fin.generation, fin.backend.pid),
        Err(err) => format!("error {}", fact_line(err)),
    }
}

fn is_deadline(err: &SourceError) -> bool {
    err.kind() == SourceErrorKind::Budget && err.fact() == Some("deadline") && err.sqlstate().is_none()
}

fn is_25p03(err: &SourceError) -> bool {
    err.kind() == SourceErrorKind::Protocol && err.fact() == Some("database") && err.sqlstate() == Some("25P03")
}

async fn connect(env: &Env, user: &str, password: &str, app: &str) -> Result<PgConnection, String> {
    connect_parts(&env.host, env.port, &env.database, user, password, &env.ca, app).await
}

async fn connect_parts(host: &str, port: u16, database: &str, user: &str, password: &str, ca: &[u8], app: &str) -> Result<PgConnection, String> {
    let options = PgConnectOptions::from_explicit()
        .host(host)
        .port(port)
        .username(user)
        .password(password)
        .database(database)
        .application_name(app)
        .ssl_mode(PgSslMode::VerifyFull)
        .ssl_root_cert_from_pem(ca.to_vec())
        .disable_statement_logging();
    options.connect().await.map_err(|err| sqlstate_only(&err))
}

async fn tune(conn: &mut PgConnection, statement_ms: i64, lock_ms: i64) -> Result<(), String> {
    exec(conn, "SELECT pg_catalog.set_config('TimeZone', 'UTC', false)").await?;
    exec(conn, "SELECT pg_catalog.set_config('DateStyle', 'ISO, MDY', false)").await?;
    exec(conn, "SELECT pg_catalog.set_config('stats_fetch_consistency', 'none', false)").await?;
    let statement = statement_ms.to_string();
    let lock = lock_ms.to_string();
    sqlx::query("SELECT pg_catalog.set_config('statement_timeout', $1, false)")
        .bind(&statement)
        .execute(&mut *conn)
        .await
        .map_err(|err| sqlstate_only(&err))?;
    sqlx::query("SELECT pg_catalog.set_config('lock_timeout', $1, false)")
        .bind(&lock)
        .execute(&mut *conn)
        .await
        .map_err(|err| sqlstate_only(&err))?;
    Ok(())
}

async fn exec(conn: &mut PgConnection, sql: &'static str) -> Result<(), String> {
    sqlx::query(sql).execute(&mut *conn).await.map_err(|err| sqlstate_only(&err))?;
    Ok(())
}

async fn text(conn: &mut PgConnection, sql: &'static str) -> Result<String, String> {
    let row = sqlx::query(sql).fetch_one(&mut *conn).await.map_err(|err| sqlstate_only(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

async fn text_bind(conn: &mut PgConnection, sql: &'static str, schema: &str, table: &str) -> Result<String, String> {
    let row = sqlx::query(sql).bind(schema).bind(table).fetch_one(&mut *conn).await.map_err(|err| sqlstate_only(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

async fn scalar_i64(conn: &mut PgConnection, sql: &'static str) -> Result<i64, String> {
    let row = sqlx::query(sql).fetch_one(&mut *conn).await.map_err(|err| sqlstate_only(&err))?;
    row.try_get(0).map_err(|_| "decode".to_string())
}

fn sqlstate_only(err: &sqlx::Error) -> String {
    err.as_database_error().and_then(|db| db.code().map(|code| code.to_string())).unwrap_or_else(|| "no_sqlstate".to_string())
}

fn load_secrets() -> Result<Secrets, String> {
    let pointer = std::env::var("OO_A121_FIXTURE").map_err(|_| "fixture_env")?;
    let map = pointer_map(&std::fs::read_to_string(&pointer).map_err(|_| "fixture_read")?);
    let mut passwords = Vec::new();
    for key in ["reader_password_file", "writer_password_file", "admin_password_file"] {
        passwords.push(read_secret(req(&map, key)?)?);
    }
    Ok(Secrets { passwords })
}

fn load_env() -> Result<Env, String> {
    let pointer = std::env::var("OO_A121_FIXTURE").map_err(|_| "fixture_env")?;
    let expected_dir = std::env::var("OO_A121_EXPECTED").map_err(|_| "expected_env")?;
    let map = pointer_map(&std::fs::read_to_string(&pointer).map_err(|_| "fixture_read")?);
    let port: u16 = req(&map, "port")?.parse().map_err(|_| "port")?;
    Ok(Env {
        host: req(&map, "host")?.to_string(),
        port,
        database: req(&map, "database")?.to_string(),
        reader_user: req(&map, "reader_user")?.to_string(),
        writer_user: req(&map, "writer_user")?.to_string(),
        admin_user: req(&map, "admin_user")?.to_string(),
        reader_password: read_secret(req(&map, "reader_password_file")?)?,
        writer_password: read_secret(req(&map, "writer_password_file")?)?,
        admin_password: read_secret(req(&map, "admin_password_file")?)?,
        ca: read_ca(req(&map, "ca_file")?)?,
        log_file: req(&map, "log_file")?.to_string(),
        expected_dir,
        postmaster: String::new(),
    })
}

fn load_expect(dir: &str) -> Result<Expect, String> {
    let rows_text = std::fs::read_to_string(format!("{dir}/rows.txt")).map_err(|_| "rows_read")?;
    let catalog_text = std::fs::read_to_string(format!("{dir}/catalog.txt")).map_err(|_| "catalog_read")?;
    let mut rows = Vec::new();
    for line in rows_text.lines() {
        let mut parts = line.splitn(4, '\t');
        let phase = parts.next().ok_or("rows")?.to_string();
        let table = parts.next().ok_or("rows")?.to_string();
        let key: i64 = parts.next().ok_or("rows")?.parse().map_err(|_| "rows_key")?;
        let token = parts.next().ok_or("rows")?.to_string();
        rows.push(ExpectRow { phase, table, key, token });
    }
    let mut columns = Vec::new();
    for line in catalog_text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 9 {
            return Err("catalog_width".to_string());
        }
        columns.push(ExpectCol {
            phase: parts[0].to_string(),
            schema: parts[1].to_string(),
            table: parts[2].to_string(),
            attnum: parts[3].parse().map_err(|_| "catalog")?,
            name: parts[4].to_string(),
            type_oid: parts[5].parse().map_err(|_| "catalog")?,
            typmod: parts[6].parse().map_err(|_| "catalog")?,
            not_null: parts[7] == "true",
            key_ord: parts[8].parse().map_err(|_| "catalog")?,
        });
    }
    Ok(Expect { rows, columns })
}

fn pointer_map(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(' ') else { continue };
        map.insert(key.to_string(), value.to_string());
    }
    map
}

fn req<'a>(map: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    map.get(key).map(String::as_str).ok_or_else(|| "pointer".to_string())
}

fn read_secret(path: &str) -> Result<String, String> {
    if !path.starts_with("/secrets/") || path.contains("..") {
        return Err("secret_path".to_string());
    }
    let text = std::fs::read_to_string(path).map_err(|_| "secret_read")?;
    let text = text.trim().to_string();
    if text.is_empty() { Err("secret_empty".to_string()) } else { Ok(text) }
}

fn read_ca(path: &str) -> Result<Vec<u8>, String> {
    if !path.starts_with("/secrets/") || path.contains("..") {
        return Err("ca_path".to_string());
    }
    let bytes = std::fs::read(path).map_err(|_| "ca_read")?;
    if bytes.is_empty() { Err("ca_empty".to_string()) } else { Ok(bytes) }
}

fn file_sha(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|_| "hash_read")?;
    Ok(hex_bytes(Sha256::digest(bytes).as_ref()))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn finish_book(path: &str, book: &mut Book, passwords: &[String]) {
    book.line(format!("fail_count={}", book.fails.len()));
    book.line(format!("fail_names={}", book.fails.join(",")));
    let mut text = book.lines.join("\n");
    text.push('\n');
    let mut hits = 0usize;
    for secret in passwords {
        if secret.is_empty() {
            continue;
        }
        let count = text.matches(secret.as_str()).count();
        if count > 0 {
            hits += count;
            text = text.replace(secret, "[redacted]");
        }
    }
    if hits > 0 {
        text.push_str(&format!("outcome_secret_hits={hits}\n"));
    }
    if let Some(parent) = std::path::Path::new(path).parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        eprintln!("outcome_dir_failed");
    }
    if std::fs::write(path, text).is_err() {
        eprintln!("outcome_write_failed");
    }
}

struct Classified {
    oo: BTreeMap<String, usize>,
    writer: BTreeMap<String, usize>,
    side: BTreeMap<String, usize>,
    admin: BTreeMap<String, usize>,
    oracle: BTreeMap<String, usize>,
    oo_forbidden: usize,
    oo_unknown: String,
    other: usize,
    other_sample: String,
    window: bool,
    marker_before: bool,
    owner_start: i64,
    side_select: i64,
    barrier: i64,
    writer_commit: i64,
    owner_probe: i64,
}

fn classify(log: &str) -> Classified {
    const ALLOWED: &[&str] = &["SELECT", "WITH", "START", "ROLLBACK", "SET", "SHOW"];
    const FORBIDDEN: &[&str] = &[
        "INSERT", "UPDATE", "DELETE", "TRUNCATE", "MERGE", "COPY", "ALTER", "CREATE", "DROP", "GRANT", "REVOKE",
        "COMMENT", "COMMIT", "VACUUM", "CLUSTER", "REINDEX",
    ];
    let records = log_records(log);
    let start = records.iter().rposition(|rec| rec.sql.contains("A121_AUDIT_WINDOW_START"));
    let end = records.iter().rposition(|rec| rec.sql.contains("A121_AUDIT_WINDOW_END"));
    let mut out = Classified {
        oo: BTreeMap::new(),
        writer: BTreeMap::new(),
        side: BTreeMap::new(),
        admin: BTreeMap::new(),
        oracle: BTreeMap::new(),
        oo_forbidden: 0,
        oo_unknown: String::new(),
        other: 0,
        other_sample: String::new(),
        window: matches!((start, end), (Some(left), Some(right)) if left < right),
        marker_before: false,
        owner_start: -1,
        side_select: -1,
        barrier: -1,
        writer_commit: -1,
        owner_probe: -1,
    };
    let (Some(start), Some(end)) = (start, end) else {
        return out;
    };
    let mut saw_oo = false;
    let mut oo_after = true;
    for (index, rec) in records.iter().enumerate() {
        if !(start..=end).contains(&index) {
            continue;
        }
        if rec.app == "oo-pg-source" && rec.user == "lot_reader" {
            saw_oo = true;
            oo_after &= index >= start;
        }
        let verb = verb_of(&rec.sql);
        if verb.is_empty() {
            continue;
        }
        let class = class_of(&rec.app, &rec.user);
        match class {
            "oo" => {
                *out.oo.entry(verb.clone()).or_insert(0) += 1;
                if FORBIDDEN.contains(&verb.as_str()) {
                    out.oo_forbidden += 1;
                } else if !ALLOWED.contains(&verb.as_str()) && out.oo_unknown.is_empty() {
                    out.oo_unknown = verb.clone();
                }
                if out.owner_start < 0 && rec.sql.contains("START TRANSACTION") {
                    out.owner_start = i64::try_from(index).unwrap_or(-1);
                }
                if out.owner_probe < 0 && rec.sql.contains("FROM lot.batch") && rec.sql.contains("LIMIT") {
                    out.owner_probe = i64::try_from(index).unwrap_or(-1);
                }
            }
            "writer" => {
                *out.writer.entry(verb.clone()).or_insert(0) += 1;
                if verb == "COMMIT" {
                    out.writer_commit = i64::try_from(index).unwrap_or(-1);
                }
            }
            "side" => {
                *out.side.entry(verb.clone()).or_insert(0) += 1;
                if out.side_select < 0 && rec.sql.contains("FROM lot.batch") {
                    out.side_select = i64::try_from(index).unwrap_or(-1);
                }
            }
            "admin" => {
                *out.admin.entry(verb).or_insert(0) += 1;
                if rec.sql.contains("A121_BARRIER_RELEASE") {
                    out.barrier = i64::try_from(index).unwrap_or(-1);
                }
            }
            "oracle" => {
                *out.oracle.entry(verb).or_insert(0) += 1;
            }
            _ => {
                out.other += 1;
                if out.other_sample.is_empty() {
                    out.other_sample = format!("{} {}", rec.app, rec.user);
                }
            }
        }
    }
    out.marker_before = saw_oo && oo_after;
    out
}

fn class_of(app: &str, user: &str) -> &'static str {
    if app == "oo-pg-source" && user == "lot_reader" {
        "oo"
    } else if app == "oo-a121-writer" || user == "lot_writer" {
        "writer"
    } else if app == "oo-a121-rr" {
        "side"
    } else if app == "oo-a121-oracle" {
        "oracle"
    } else if user == "postgres" || app == "oo-a121-admin" {
        "admin"
    } else {
        "other"
    }
}

struct LogRec {
    app: String,
    user: String,
    sql: String,
}

fn log_records(log: &str) -> Vec<LogRec> {
    let mut out = Vec::new();
    let mut header = String::new();
    let mut body = String::new();
    let mut open = false;
    for line in log.split_inclusive('\n') {
        if line_is_record(line) {
            if open {
                push_rec(&mut out, &header, &body);
            }
            header = line.to_string();
            body.clear();
            open = true;
        } else if open {
            body.push_str(line);
        }
    }
    if open {
        push_rec(&mut out, &header, &body);
    }
    out
}

fn line_is_record(line: &str) -> bool {
    let Some(first) = line.chars().next() else { return false };
    first.is_ascii_digit() && line.contains(" app=") && line.contains(" user=") && line.contains(" db=")
}

fn push_rec(out: &mut Vec<LogRec>, header: &str, body: &str) {
    let Some(sql) = sql_text(header, body) else { return };
    out.push(LogRec {
        app: header_field(header, "app=").to_string(),
        user: header_field(header, "user=").to_string(),
        sql,
    });
}

fn header_field<'a>(header: &'a str, key: &str) -> &'a str {
    let Some(pos) = header.find(key) else { return "" };
    let rest = &header[pos + key.len()..];
    let end = rest.find(' ').unwrap_or(rest.len());
    &rest[..end]
}

fn sql_text(header: &str, body: &str) -> Option<String> {
    let all = format!("{header}{body}");
    let marker = if let Some(pos) = all.find("statement: ") {
        pos + "statement: ".len()
    } else if let Some(pos) = all.find("execute ") {
        let rest = &all[pos..];
        let colon = rest.find(": ")?;
        pos + colon + 2
    } else {
        return None;
    };
    let text = all[marker..].split("\nDETAIL:").next().unwrap_or("").trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn verb_of(sql: &str) -> String {
    let token = sql.split_whitespace().next().unwrap_or("");
    let mut verb = String::new();
    for ch in token.chars() {
        if ch.is_ascii_alphabetic() {
            verb.push(ch.to_ascii_uppercase());
        } else {
            break;
        }
    }
    verb
}

fn hist_line(prefix: &str, map: &BTreeMap<String, usize>) -> String {
    let mut line = prefix.to_string();
    for (verb, count) in map {
        line.push(' ');
        line.push_str(verb);
        line.push('=');
        line.push_str(&count.to_string());
    }
    line
}
