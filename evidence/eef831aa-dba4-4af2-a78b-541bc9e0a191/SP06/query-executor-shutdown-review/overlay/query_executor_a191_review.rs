//! Independent review of the fixed dedicated-session shutdown.
//!
//! Runs only when `OO_A191_LIVE=1`. The fixture functions are volatile and are
//! not a docs rule R4 whitelist. The audit hook is a second connection's
//! autocommit insert, not a production audit WAL.

#![allow(
    clippy::cognitive_complexity,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]

use std::env;
use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as SlotMutex, OnceLock};
use std::time::{Duration, Instant};

use oo_gateway::query_executor::{
    Admission, AuditAdmission, CancelHandle, CancelOutcome, Cell, CompletionRecord,
    CompletionStatus, DedicatedSession, ExecError, QuerySummary, SelectRequest, SelectStream,
    StopClass, UntrustedAdmission, UNTRUSTED_ADMISSION,
};
use oo_gateway::sql_guard::{
    GuardError, LiteralTypes, PolicyCatalog, PolicyIdentity, ResolvedFunction, VisibleColumn,
    VisibleRelation,
};
use oo_gateway::{connect_require_tls, rustls_connector, TlsEndpoint};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_postgres::Client;

const DB: &str = "oo_a191";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a191_login";
const EAST: &str = "oo_r_a191_east";
const WEST: &str = "oo_r_a191_west";
const FLAG: &str = "oo_r_a191_flag";
const PROJECT: &str = "a191";
const POLICY: &str = "a191-policy";
const READ_GATE: &str = "SELECT n FROM fixture.gate";
const HOLD_SQL: &str = "SELECT fixture.a191_hold() FROM fixture.gate";
const BRIEF_SQL: &str = "SELECT fixture.a191_brief() FROM fixture.gate";
const PACE_SQL: &str = "SELECT fixture.a191_pace(n) FROM fixture.pace";
const INSERT_SQL: &str = "INSERT INTO fixture.gate (n) VALUES (1)";
const HOLD_MS: u64 = 5_200;
const DROP_WALL_MS: u64 = 8_000;
const PACE_WALL_MS: u64 = 8_000;
const SERVER_WALL_MS: u64 = 2_500;

struct Ctx {
    host: String,
    port: u16,
    proxy_host: String,
    proxy_port: u16,
    admin_pass: String,
    login_pass: String,
    ca: Vec<u8>,
    oracle: String,
}

struct Oids {
    int4: u32,
    hold: u32,
    brief: u32,
    pace: u32,
}

struct Activity {
    present: bool,
    state: String,
    wait_type: String,
    wait_event: String,
}

struct Proxy {
    stall: Arc<AtomicBool>,
    held: Arc<Mutex<Vec<TcpStream>>>,
    accepts: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct AuditParts {
    host: String,
    port: u16,
    pass: String,
    ca: Vec<u8>,
}

fn audit_slot() -> &'static SlotMutex<Option<AuditParts>> {
    static SLOT: OnceLock<SlotMutex<Option<AuditParts>>> = OnceLock::new();
    SLOT.get_or_init(|| SlotMutex::new(None))
}

struct DurableAudit {
    client: Client,
    fail_admit: bool,
    fail_complete: bool,
}

impl AuditAdmission for DurableAudit {
    async fn admit(&mut self, admission: &Admission<'_>) -> Result<(), ExecError> {
        if self.fail_admit {
            return Err(ExecError::AuditUnavailable);
        }
        let role = admission.role().to_string();
        let pid = admission.backend_pid();
        let start = admission.backend_start().to_string();
        let original = admission.original_sha256().to_string();
        let executed = admission.executed_sha256().to_string();
        let fingerprint = admission.fingerprint_hex().to_string();
        let project = admission.project().to_string();
        let policy_id = admission.policy_id().to_string();
        self.client
            .execute(
                "INSERT INTO fixture.a191_audit \
                 (phase, role_name, backend_pid, backend_start, original_sha256, executed_sha256, fingerprint_hex, project, policy_id) \
                 VALUES ('admit', $1, $2, $3, $4, $5, $6, $7, $8)",
                &[
                    &role,
                    &pid,
                    &start,
                    &original,
                    &executed,
                    &fingerprint,
                    &project,
                    &policy_id,
                ],
            )
            .await
            .map_err(|_| ExecError::AuditUnavailable)?;
        Ok(())
    }

    async fn complete(&mut self, completion: &CompletionRecord) -> Result<(), ExecError> {
        if self.fail_complete {
            return Err(ExecError::AuditUnavailable);
        }
        let status = completion.status().as_str().to_string();
        let rollback = completion.rollback_confirmed();
        let dropped = completion.socket_dropped();
        let stop = completion.stop().map(stop_name).unwrap_or("none").to_string();
        let sqlstate = completion.sqlstate().map(str::to_string);
        let role = completion.role().to_string();
        let pid = completion.backend_pid();
        let start = completion.backend_start().to_string();
        let rows = i32::try_from(completion.returned_rows()).unwrap_or(i32::MAX);
        self.client
            .execute(
                "INSERT INTO fixture.a191_audit \
                 (phase, status, rollback_confirmed, socket_dropped, stop_class, sqlstate, role_name, backend_pid, backend_start, returned_rows) \
                 VALUES ('complete', $1, $2, $3, $4, $5, $6, $7, $8, $9)",
                &[
                    &status,
                    &rollback,
                    &dropped,
                    &stop,
                    &sqlstate,
                    &role,
                    &pid,
                    &start,
                    &rows,
                ],
            )
            .await
            .map_err(|_| ExecError::AuditUnavailable)?;
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn review_fixed_executor_shutdown() {
    if env::var("OO_A191_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    let ctx = match load_ctx() {
        Ok(ctx) => ctx,
        Err(label) => panic!("setup {label}"),
    };
    if let Err(label) = run(&ctx).await {
        note(&ctx, &format!("fail={label}"));
        panic!("review {label}");
    }
    note(&ctx, "review_result=measured");
}

fn load_ctx() -> Result<Ctx, String> {
    let admin_pass = read_secret("OO_A191_ADMIN_PASS_FILE")?;
    let login_pass = read_secret("OO_A191_LOGIN_PASS_FILE")?;
    let ca_path = env::var("OO_A191_CA").map_err(|_| "ca-path".to_string())?;
    let ca = fs::read(&ca_path).map_err(|_| "ca-read".to_string())?;
    let oracle = env::var("OO_A191_ORACLE").map_err(|_| "oracle-path".to_string())?;
    if let Some(parent) = std::path::Path::new(&oracle).parent() {
        fs::create_dir_all(parent).map_err(|_| "oracle-dir".to_string())?;
    }
    Ok(Ctx {
        host: env::var("OO_A191_HOST").unwrap_or_else(|_| "oo-a191-pg".to_string()),
        port: env::var("OO_A191_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5432),
        proxy_host: env::var("OO_A191_PROXY_HOST").unwrap_or_else(|_| "oo-a191-proxy".to_string()),
        proxy_port: env::var("OO_A191_PROXY_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(5432),
        admin_pass,
        login_pass,
        ca,
        oracle,
    })
}

fn read_secret(name: &str) -> Result<String, String> {
    let path = env::var(name).map_err(|_| "secret-path".to_string())?;
    let text = fs::read_to_string(&path).map_err(|_| "secret-read".to_string())?;
    let secret = text.trim().to_string();
    if secret.is_empty() || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("secret-shape".to_string());
    }
    Ok(secret)
}

fn note(ctx: &Ctx, line: &str) {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ctx.oracle)
        .expect("oracle");
    writeln!(file, "{line}").expect("oracle-write");
    file.flush().expect("oracle-flush");
}

fn check(cond: bool, label: &str) -> Result<(), String> {
    if cond { Ok(()) } else { Err(label.to_string()) }
}

fn stop_name(stop: StopClass) -> &'static str {
    match stop {
        StopClass::Timeout => "timeout",
        StopClass::Canceled => "canceled",
        StopClass::TypeRefused => "type",
        StopClass::Database => "database",
        StopClass::Protocol => "protocol",
    }
}

fn sqlstate_of(err: &ExecError) -> &str {
    err.sqlstate().unwrap_or("absent")
}

fn display_clean(err: &ExecError, secret: &str) -> bool {
    let text = err.to_string();
    !text.contains("SELECT")
        && !text.contains("INSERT")
        && !text.contains("fixture")
        && !text.contains("PASSWORD")
        && !secret.is_empty()
        && !text.contains(secret)
}

async fn run(ctx: &Ctx) -> Result<(), String> {
    {
        let mut guard = audit_slot().lock().map_err(|_| "audit-slot".to_string())?;
        *guard = Some(AuditParts {
            host: ctx.host.clone(),
            port: ctx.port,
            pass: ctx.admin_pass.clone(),
            ca: ctx.ca.clone(),
        });
    }
    note(ctx, "untrusted_admission=blocked");
    note(ctx, "function_whitelist=not_claimed");
    note(ctx, "audit_wal=not_production");
    note(ctx, "prior_a184_ast=not_repeated");
    check(
        UNTRUSTED_ADMISSION == UntrustedAdmission::Blocked,
        "untrusted",
    )?;
    let bootstrap = connect_client(ctx, &ctx.host, ctx.port, "postgres", ADMIN, &ctx.admin_pass).await?;
    reset_fixture(&bootstrap).await?;
    drop(bootstrap);
    let admin = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    install_fixture(&admin, &ctx.login_pass).await?;
    let oids = load_oids(&admin).await?;
    note(
        ctx,
        &format!(
            "catalog_oids hold={} brief={} pace={} int4={}",
            oids.hold, oids.brief, oids.pace, oids.int4
        ),
    );
    let observer = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    let locker = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    let audit = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    version_line(ctx, &admin).await?;
    role_cases(ctx, &oids, &audit).await?;
    admit_case(ctx, &oids, &audit).await?;
    flag_case(ctx, &oids, &audit).await?;
    server_deadline(ctx, &oids, &audit, &observer, &locker).await?;
    user_cancel(ctx, &oids, &audit, &observer).await?;
    late_cancel(ctx, &oids, &audit, &observer).await?;
    finished_drop_spares_next(ctx, &oids, &audit, &observer).await?;
    end_hook_failure(ctx, &oids, &audit, &observer, &locker).await?;
    held_drop(ctx, &oids, &audit, &observer, &locker, "lock").await?;
    held_drop(ctx, &oids, &audit, &observer, &locker, "sleep").await?;
    pace_budget(ctx, &oids, &audit, &observer).await?;
    durable_reread(ctx, &audit).await?;
    fault_cases(ctx, &oids).await?;
    Ok(())
}

async fn version_line(ctx: &Ctx, client: &Client) -> Result<(), String> {
    let row = client
        .query_one("SELECT current_setting('server_version'), current_setting('ssl')", &[])
        .await
        .map_err(|_| "version".to_string())?;
    let version: String = row.try_get(0).map_err(|_| "version-col".to_string())?;
    let ssl: String = row.try_get(1).map_err(|_| "ssl-col".to_string())?;
    note(ctx, &format!("server_version={version} server_ssl={ssl}"));
    check(version.starts_with("17."), "pg-major")?;
    check(ssl == "on", "ssl")?;
    Ok(())
}

async fn role_cases(ctx: &Ctx, oids: &Oids, audit: &Client) -> Result<(), String> {
    let before = audit_count(audit).await?;
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let east = identity(EAST);
    let west = identity(WEST);
    let east_catalog = catalog(oids, EAST);
    let bad_id = identity("not-a-role");
    let shape = exec_err(
        &mut session,
        &bad_id,
        &east_catalog,
        READ_GATE,
        "not-a-role",
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    note(ctx, &format!("invalid_shape={}", err_label(&shape)));
    check(matches!(shape, ExecError::RoleParameter), "shape")?;
    check(session.bound_role().is_none(), "shape-unbound")?;
    check(session.is_open(), "shape-open")?;
    let mismatch = exec_err(
        &mut session,
        &west,
        &east_catalog,
        READ_GATE,
        EAST,
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    note(ctx, &format!("invalid_identity={}", err_label(&mismatch)));
    check(matches!(mismatch, ExecError::RoleParameter), "identity")?;
    check(session.bound_role().is_none(), "identity-unbound")?;
    let limited = exec_err(
        &mut session,
        &east,
        &east_catalog,
        READ_GATE,
        EAST,
        2_001,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    note(ctx, &format!("invalid_rows={}", err_label(&limited)));
    check(matches!(limited, ExecError::Limit), "rows")?;
    check(session.bound_role().is_none(), "rows-unbound")?;
    let fraction = exec_wall(
        &mut session,
        &east,
        &east_catalog,
        READ_GATE,
        EAST,
        Duration::from_micros(1_500),
        audit,
    )
    .await?;
    note(ctx, &format!("invalid_wall={}", err_label(&fraction)));
    check(matches!(fraction, ExecError::Limit), "wall")?;
    check(session.bound_role().is_none(), "wall-unbound")?;
    check(audit_count(audit).await? == before, "limits-no-audit")?;
    let guard = exec_err(
        &mut session,
        &east,
        &east_catalog,
        INSERT_SQL,
        EAST,
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    note(
        ctx,
        &format!(
            "guard_refusal={} display_clean={} bound={}",
            err_label(&guard),
            display_clean(&guard, &ctx.login_pass),
            session.bound_role().as_deref().unwrap_or("none")
        ),
    );
    check(
        matches!(guard, ExecError::Guard(ref inner) if matches!(inner.as_ref(), GuardError::Write)),
        "guard-write",
    )?;
    check(display_clean(&guard, &ctx.login_pass), "guard-display")?;
    check(session.bound_role().as_deref() == Some(EAST), "guard-bound")?;
    check(session.is_open(), "guard-open")?;
    check(audit_count(audit).await? == before, "guard-no-audit")?;
    let reuse = exec_err(
        &mut session,
        &west,
        &catalog(oids, WEST),
        READ_GATE,
        WEST,
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    note(ctx, &format!("other_role={}", err_label(&reuse)));
    check(matches!(reuse, ExecError::RoleReuse), "reuse")?;
    check(display_clean(&reuse, &ctx.login_pass), "reuse-display")?;
    check(audit_count(audit).await? == before, "reuse-no-audit")?;
    check(session.bound_role().as_deref() == Some(EAST), "reuse-keeps")?;
    let again_bad = exec_err(
        &mut session,
        &bad_id,
        &east_catalog,
        READ_GATE,
        "not-a-role",
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    check(matches!(again_bad, ExecError::RoleParameter), "shape-after")?;
    check(session.bound_role().as_deref() == Some(EAST), "shape-keeps")?;
    let again_limit = exec_err(
        &mut session,
        &east,
        &east_catalog,
        READ_GATE,
        EAST,
        2_001,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    check(matches!(again_limit, ExecError::Limit), "limit-after")?;
    check(session.bound_role().as_deref() == Some(EAST), "limit-keeps")?;
    let (first_rows, first) = exec_ok(&mut session, &east, &east_catalog, READ_GATE, EAST, audit).await?;
    let (second_rows, second) = exec_ok(&mut session, &east, &east_catalog, READ_GATE, EAST, audit).await?;
    note(
        ctx,
        &format!(
            "same_role_first status={} rows={} rollback={} socket={} end_audit={}",
            first.status().as_str(),
            first_rows.len(),
            first.rollback_confirmed(),
            first.socket_dropped(),
            first.end_audit_ok()
        ),
    );
    note(
        ctx,
        &format!(
            "same_role_second status={} rows={} rollback={} socket={} end_audit={}",
            second.status().as_str(),
            second_rows.len(),
            second.rollback_confirmed(),
            second.socket_dropped(),
            second.end_audit_ok()
        ),
    );
    check(first_rows.len() == 1 && first_cell_is(&first_rows, "7"), "first-row")?;
    check(second_rows.len() == 1 && first_cell_is(&second_rows, "7"), "second-row")?;
    check(first.status() == CompletionStatus::Succeeded, "first-status")?;
    check(second.status() == CompletionStatus::Succeeded, "second-status")?;
    check(first.rollback_confirmed() && second.rollback_confirmed(), "same-rollback")?;
    check(!first.socket_dropped() && !second.socket_dropped(), "same-socket")?;
    check(first.end_audit_ok() && second.end_audit_ok(), "same-audit-flag")?;
    check(session.is_open(), "same-open")?;
    check(session.bound_role().as_deref() == Some(EAST), "same-bound")?;
    let mut west_session = open_session(ctx, &ctx.host, ctx.port).await?;
    let (west_rows, west_summary) = exec_ok(
        &mut west_session,
        &west,
        &catalog(oids, WEST),
        READ_GATE,
        WEST,
        audit,
    )
    .await?;
    note(
        ctx,
        &format!(
            "west_session status={} rows={}",
            west_summary.status().as_str(),
            west_rows.len()
        ),
    );
    check(west_rows.len() == 1, "west-row")?;
    check(west_summary.status() == CompletionStatus::Succeeded, "west-status")?;
    Ok(())
}

async fn admit_case(ctx: &Ctx, oids: &Oids, audit: &Client) -> Result<(), String> {
    let before = audit_count(audit).await?;
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let east = identity(EAST);
    let west = identity(WEST);
    let err = exec_err(
        &mut session,
        &east,
        &catalog(oids, EAST),
        READ_GATE,
        EAST,
        10,
        1024,
        5_000,
        audit,
        true,
        false,
    )
    .await?;
    note(
        ctx,
        &format!(
            "admit_fail={} bound={} open={} audit_delta={}",
            err_label(&err),
            session.bound_role().as_deref().unwrap_or("none"),
            session.is_open(),
            audit_count(audit).await? - before
        ),
    );
    check(matches!(err, ExecError::AuditUnavailable), "admit")?;
    check(session.bound_role().as_deref() == Some(EAST), "admit-bound")?;
    check(session.is_open(), "admit-open")?;
    check(audit_count(audit).await? == before, "admit-no-row")?;
    let reuse = exec_err(
        &mut session,
        &west,
        &catalog(oids, WEST),
        READ_GATE,
        WEST,
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    note(ctx, &format!("admit_other_role={}", err_label(&reuse)));
    check(matches!(reuse, ExecError::RoleReuse), "admit-reuse")?;
    check(audit_count(audit).await? == before, "admit-reuse-audit")?;
    let (rows, summary) = exec_ok(&mut session, &east, &catalog(oids, EAST), READ_GATE, EAST, audit).await?;
    check(rows.len() == 1, "admit-then-row")?;
    check(summary.status() == CompletionStatus::Succeeded, "admit-then-status")?;
    Ok(())
}

async fn flag_case(ctx: &Ctx, oids: &Oids, audit: &Client) -> Result<(), String> {
    let before = audit_count(audit).await?;
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let flag = identity(FLAG);
    let stream = open_stream(
        &mut session,
        &flag,
        &catalog(oids, FLAG),
        READ_GATE,
        FLAG,
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    let (err, summary) = take_err(&mut session, stream).await?;
    let after = audit_count(audit).await?;
    note(
        ctx,
        &format!(
            "flag_role={} summary_status={} summary_stop={} sqlstate={} open={} bound={} audit_delta={} end_audit={}",
            err_label(&err),
            summary.status().as_str(),
            summary.stop().map(stop_name).unwrap_or("none"),
            summary.sqlstate().unwrap_or("absent"),
            session.is_open(),
            session.bound_role().as_deref().unwrap_or("none"),
            after - before,
            summary.end_audit_ok()
        ),
    );
    check(matches!(err, ExecError::RoleParameter), "flag")?;
    check(summary.status() == CompletionStatus::Failed, "flag-status")?;
    check(!session.is_open(), "flag-closed")?;
    check(session.bound_role().as_deref() == Some(FLAG), "flag-bound")?;
    check(summary.end_audit_ok(), "flag-end-audit")?;
    check(after > before, "flag-audit-row")?;
    let next = exec_err(
        &mut session,
        &identity(WEST),
        &catalog(oids, WEST),
        READ_GATE,
        WEST,
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    note(ctx, &format!("flag_next={}", err_label(&next)));
    check(matches!(next, ExecError::SessionClosed), "flag-next")?;
    check(audit_count(audit).await? == after, "flag-next-audit")?;
    Ok(())
}

async fn server_deadline(
    ctx: &Ctx,
    oids: &Oids,
    audit: &Client,
    observer: &Client,
    locker: &Client,
) -> Result<(), String> {
    step(locker, "lock", "BEGIN; LOCK TABLE fixture.gate IN ACCESS EXCLUSIVE MODE").await?;
    let result = server_deadline_locked(ctx, oids, audit, observer).await;
    let unlocked = step(locker, "unlock", "ROLLBACK").await;
    result?;
    unlocked
}

async fn server_deadline_locked(
    ctx: &Ctx,
    oids: &Oids,
    audit: &Client,
    observer: &Client,
) -> Result<(), String> {
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let pid = session.backend_pid();
    let started = Instant::now();
    let stream = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        READ_GATE,
        EAST,
        10,
        1024,
        SERVER_WALL_MS,
        audit,
        false,
        false,
    )
    .await?;
    wait_blocked(observer, pid, "lock").await?;
    let (err, summary) = take_err(&mut session, stream).await?;
    let ms = started.elapsed().as_millis();
    note(
        ctx,
        &format!(
            "server_57014_ms={ms} err={} sqlstate={} status={} stop={} rollback={} end_audit={} display_clean={}",
            err_label(&err),
            sqlstate_of(&err),
            summary.status().as_str(),
            summary.stop().map(stop_name).unwrap_or("none"),
            summary.rollback_confirmed(),
            summary.end_audit_ok(),
            display_clean(&err, &ctx.login_pass)
        ),
    );
    check(matches!(err, ExecError::Timeout { sqlstate: Some(ref code) } if code == "57014"), "server-class")?;
    check(summary.status() == CompletionStatus::Failed, "server-status")?;
    check(summary.stop() == Some(StopClass::Timeout), "server-stop")?;
    check(summary.sqlstate() == Some("57014"), "server-sqlstate")?;
    check((1_200..4_000).contains(&ms), "server-elapsed")?;
    check(summary.end_audit_ok(), "server-audit")?;
    check(display_clean(&err, &ctx.login_pass), "server-display")?;
    check(!session.is_open(), "server-closed")?;
    Ok(())
}

async fn user_cancel(ctx: &Ctx, oids: &Oids, audit: &Client, observer: &Client) -> Result<(), String> {
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let pid = session.backend_pid();
    let mut stream = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        HOLD_SQL,
        EAST,
        10,
        1024,
        DROP_WALL_MS,
        audit,
        false,
        false,
    )
    .await?;
    wait_blocked(observer, pid, "sleep").await?;
    let handle = stream.cancel_handle();
    let outcome = handle.cancel(observer).await.map_err(|_| "cancel-call".to_string())?;
    let err = next_err(&mut stream).await?;
    session.wait_idle().await.map_err(|_| "cancel-idle".to_string())?;
    let summary = session.summary().ok_or("cancel-summary".to_string())?;
    drop(stream);
    note(
        ctx,
        &format!(
            "user_cancel outcome={} err={} sqlstate={} status={} stop={} rollback={} end_audit={} display_clean={}",
            outcome_name(outcome),
            err_label(&err),
            sqlstate_of(&err),
            summary.status().as_str(),
            summary.stop().map(stop_name).unwrap_or("none"),
            summary.rollback_confirmed(),
            summary.end_audit_ok(),
            display_clean(&err, &ctx.login_pass)
        ),
    );
    check(outcome == CancelOutcome::Sent, "user-sent")?;
    check(matches!(err, ExecError::Canceled { ref sqlstate } if sqlstate == "57014"), "user-class")?;
    check(summary.status() == CompletionStatus::Failed, "user-status")?;
    check(summary.stop() == Some(StopClass::Canceled), "user-stop")?;
    check(summary.sqlstate() == Some("57014"), "user-sqlstate")?;
    check(summary.status() != CompletionStatus::Partial, "user-not-partial")?;
    check(summary.end_audit_ok(), "user-audit")?;
    check(display_clean(&err, &ctx.login_pass), "user-display")?;
    Ok(())
}

async fn late_cancel(ctx: &Ctx, oids: &Oids, audit: &Client, observer: &Client) -> Result<(), String> {
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let stream = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        READ_GATE,
        EAST,
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    let stale = stream.cancel_handle();
    let (rows, _) = finish_ok(&mut session, stream).await?;
    check(rows.len() == 1, "late-first")?;
    let finished = stale.cancel(observer).await.map_err(|_| "late-finished-call".to_string())?;
    let pid = session.backend_pid();
    let mut next = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        HOLD_SQL,
        EAST,
        10,
        1024,
        DROP_WALL_MS,
        audit,
        false,
        false,
    )
    .await?;
    wait_blocked(observer, pid, "sleep").await?;
    let during = stale.cancel(observer).await.map_err(|_| "late-during-call".to_string())?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let still = activity(observer, pid).await?;
    let live = next.cancel_handle();
    let sent = live.cancel(observer).await.map_err(|_| "late-live-call".to_string())?;
    let err = next_err(&mut next).await?;
    session.wait_idle().await.map_err(|_| "late-idle".to_string())?;
    drop(next);
    note(
        ctx,
        &format!(
            "late_cancel finished={} during={} still_state={} still_wait={} live={} next_err={}",
            outcome_name(finished),
            outcome_name(during),
            still.state,
            still.wait_type,
            outcome_name(sent),
            err_label(&err)
        ),
    );
    check(finished == CancelOutcome::NotSent, "late-finished")?;
    check(during == CancelOutcome::NotSent, "late-during")?;
    check(still.present && still.state == "active", "late-spared")?;
    check(sent == CancelOutcome::Sent, "late-live")?;
    check(matches!(err, ExecError::Canceled { .. }), "late-live-class")?;
    Ok(())
}

async fn finished_drop_spares_next(
    ctx: &Ctx,
    oids: &Oids,
    audit: &Client,
    observer: &Client,
) -> Result<(), String> {
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let stream = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        READ_GATE,
        EAST,
        10,
        1024,
        5_000,
        audit,
        false,
        false,
    )
    .await?;
    let _ = finish_ok(&mut session, stream).await?;
    let pid = session.backend_pid();
    let next = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        HOLD_SQL,
        EAST,
        10,
        1024,
        DROP_WALL_MS,
        audit,
        false,
        false,
    )
    .await?;
    wait_blocked(observer, pid, "sleep").await?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let still = activity(observer, pid).await?;
    note(
        ctx,
        &format!(
            "finished_drop_next present={} state={} wait={}",
            still.present, still.state, still.wait_type
        ),
    );
    check(still.present && still.state == "active", "finished-drop-spared")?;
    let (ms, gone, summary) = drop_and_measure(&mut session, next, observer, pid).await?;
    note(
        ctx,
        &format!(
            "finished_drop_cleanup_ms={ms} backend_gone={gone} status={} rollback={} socket={}",
            summary.status().as_str(),
            summary.rollback_confirmed(),
            summary.socket_dropped()
        ),
    );
    check(ms < 1_000 && gone, "finished-drop-cleanup")?;
    check(summary.status() == CompletionStatus::Partial, "finished-drop-partial")?;
    Ok(())
}

async fn end_hook_failure(
    ctx: &Ctx,
    oids: &Oids,
    audit: &Client,
    observer: &Client,
    locker: &Client,
) -> Result<(), String> {
    step(locker, "lock", "BEGIN; LOCK TABLE fixture.gate IN ACCESS EXCLUSIVE MODE").await?;
    let before = audit_count(audit).await?;
    let admit_before = audit_phase(audit, "admit").await?;
    let complete_before = audit_phase(audit, "complete").await?;
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let pid = session.backend_pid();
    let stream = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        READ_GATE,
        EAST,
        10,
        1024,
        DROP_WALL_MS,
        audit,
        false,
        true,
    )
    .await?;
    let ready = wait_blocked(observer, pid, "lock").await;
    let measured = if ready.is_ok() {
        drop_and_measure(&mut session, stream, observer, pid).await
    } else {
        drop(stream);
        Err("end-hook-not-blocked".to_string())
    };
    let unlocked = step(locker, "unlock", "ROLLBACK").await;
    let (ms, gone, summary) = measured?;
    unlocked?;
    let after = audit_count(audit).await?;
    let admit_after = audit_phase(audit, "admit").await?;
    let complete_after = audit_phase(audit, "complete").await?;
    note(
        ctx,
        &format!(
            "end_hook_ms={ms} backend_gone={gone} status={} stop={} rollback={} end_audit={} socket={} audit_delta={} admit_delta={} complete_delta={}",
            summary.status().as_str(),
            summary.stop().map(stop_name).unwrap_or("none"),
            summary.rollback_confirmed(),
            summary.end_audit_ok(),
            summary.socket_dropped(),
            after - before,
            admit_after - admit_before,
            complete_after - complete_before
        ),
    );
    check(summary.status() == CompletionStatus::Partial, "hook-partial")?;
    check(summary.status() != CompletionStatus::Succeeded, "hook-not-success")?;
    check(!summary.end_audit_ok(), "hook-flag")?;
    check(ms < 1_000 && gone, "hook-bound")?;
    check(admit_after == admit_before + 1, "hook-admit-only")?;
    check(complete_after == complete_before, "hook-no-complete-row")?;
    Ok(())
}

async fn held_drop(
    ctx: &Ctx,
    oids: &Oids,
    audit: &Client,
    observer: &Client,
    locker: &Client,
    kind: &str,
) -> Result<(), String> {
    let locked = kind == "lock";
    if locked {
        step(locker, "lock", "BEGIN; LOCK TABLE fixture.gate IN ACCESS EXCLUSIVE MODE").await?;
    }
    let result = held_drop_body(ctx, oids, audit, observer, kind).await;
    if locked {
        let unlocked = step(locker, "unlock", "ROLLBACK").await;
        result?;
        unlocked
    } else {
        result
    }
}

async fn held_drop_body(
    ctx: &Ctx,
    oids: &Oids,
    audit: &Client,
    observer: &Client,
    kind: &str,
) -> Result<(), String> {
    let sql = if kind == "lock" { READ_GATE } else { HOLD_SQL };
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let pid = session.backend_pid();
    let backend_start = session.backend_start();
    let rollbacks_before = xact_rollback(observer).await?;
    let stream = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        sql,
        EAST,
        10,
        1024,
        DROP_WALL_MS,
        audit,
        false,
        false,
    )
    .await?;
    wait_blocked(observer, pid, kind).await?;
    hold_blocked(observer, pid, kind, HOLD_MS).await?;
    let before = activity(observer, pid).await?;
    note(
        ctx,
        &format!(
            "{kind}_before_drop state={} wait_type={} wait_event={} held_ms={HOLD_MS} wall_ms={DROP_WALL_MS}",
            before.state, before.wait_type, before.wait_event
        ),
    );
    check(before.present && before.state == "active", "held-active")?;
    if kind == "lock" {
        check(before.wait_type == "Lock", "held-lock")?;
    }
    let (ms, gone, summary) = drop_and_measure(&mut session, stream, observer, pid).await?;
    let rollbacks_after = xact_rollback(observer).await?;
    let durable = latest_complete(audit).await?;
    note(
        ctx,
        &format!(
            "{kind}_drop_ms={ms} backend_gone={gone} status={} stop={} sqlstate={} rollback={} socket={} end_audit={} xact_rollback_delta={} durable_status={} durable_rollback={} durable_sqlstate={} backend_start_len={}",
            summary.status().as_str(),
            summary.stop().map(stop_name).unwrap_or("none"),
            summary.sqlstate().unwrap_or("absent"),
            summary.rollback_confirmed(),
            summary.socket_dropped(),
            summary.end_audit_ok(),
            rollbacks_after - rollbacks_before,
            durable.0,
            durable.1,
            if durable.2.is_empty() { "absent" } else { durable.2.as_str() },
            backend_start.len()
        ),
    );
    check(ms < 1_000, "drop-ms")?;
    check(gone, "drop-backend")?;
    check(summary.status() == CompletionStatus::Partial, "drop-partial")?;
    check(summary.stop() == Some(StopClass::Protocol), "drop-stop")?;
    check(summary.sqlstate().is_none(), "drop-sqlstate")?;
    check(summary.rollback_confirmed(), "drop-rollback")?;
    check(summary.end_audit_ok(), "drop-audit")?;
    check(summary.status() != CompletionStatus::Succeeded, "drop-not-success")?;
    check(!session.is_open(), "drop-closed")?;
    check(durable.0 == "partial", "drop-durable-status")?;
    check(durable.1, "drop-durable-rollback")?;
    check(rollbacks_after > rollbacks_before, "drop-xact")?;
    Ok(())
}

async fn pace_budget(ctx: &Ctx, oids: &Oids, audit: &Client, observer: &Client) -> Result<(), String> {
    let mut session = open_session(ctx, &ctx.host, ctx.port).await?;
    let pid = session.backend_pid();
    let started = Instant::now();
    let mut stream = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        PACE_SQL,
        EAST,
        10,
        1024,
        PACE_WALL_MS,
        audit,
        false,
        false,
    )
    .await?;
    let mut rows = 0_u32;
    let mut first = String::new();
    let err = loop {
        match stream.next_row().await {
            Ok(Some(row)) => {
                if rows == 0 {
                    first = cell_text(&row);
                }
                rows = rows.saturating_add(1);
            }
            Ok(None) => return Err("pace-clean-end".to_string()),
            Err(err) => break err,
        }
    };
    let error_ms = u128_ms(started.elapsed());
    session.wait_idle().await.map_err(|_| "pace-idle".to_string())?;
    let idle_ms = u128_ms(started.elapsed());
    let summary = session.summary().ok_or("pace-summary".to_string())?;
    let gone = backend_gone(observer, pid, &session.backend_start()).await?;
    drop(stream);
    note(
        ctx,
        &format!(
            "pace_error_ms={error_ms} pace_idle_ms={idle_ms} pace_rows={rows} pace_first={first} err={} sqlstate={} status={} stop={} rollback={} socket={} end_audit={} backend_gone={gone}",
            err_label(&err),
            sqlstate_of(&err),
            summary.status().as_str(),
            summary.stop().map(stop_name).unwrap_or("none"),
            summary.rollback_confirmed(),
            summary.socket_dropped(),
            summary.end_audit_ok()
        ),
    );
    check(rows >= 1, "pace-page")?;
    check(first == "1", "pace-order")?;
    check(matches!(err, ExecError::Timeout { sqlstate: None }), "pace-class")?;
    check(summary.status() == CompletionStatus::Failed, "pace-status")?;
    check(summary.stop() == Some(StopClass::Timeout), "pace-stop")?;
    check(summary.sqlstate().is_none(), "pace-sqlstate")?;
    check(!summary.rollback_confirmed(), "pace-rollback")?;
    check((7_000..12_000).contains(&error_ms), "pace-elapsed")?;
    check(idle_ms < error_ms + 1_000, "pace-cleanup")?;
    check(summary.end_audit_ok(), "pace-audit")?;
    check(gone, "pace-backend")?;
    Ok(())
}

async fn durable_reread(ctx: &Ctx, audit: &Client) -> Result<(), String> {
    let seen = audit_count(audit).await?;
    let fresh = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    let again = audit_count(&fresh).await?;
    note(ctx, &format!("durable_reread={again} same_session={seen}"));
    check(again == seen && again > 0, "durable")?;
    Ok(())
}

async fn fault_cases(ctx: &Ctx, oids: &Oids) -> Result<(), String> {
    let upstream = format!("{}:{}", ctx.host, ctx.port);
    let proxy = Proxy::start(ctx.proxy_port, upstream.clone()).await?;
    ensure_proxy_name(&ctx.proxy_host).await?;
    let drop_result = fault_drop(ctx, oids, &proxy, &upstream).await;
    let race_result = fault_race(ctx, oids, &proxy, &upstream).await;
    proxy.task.abort();
    drop_result?;
    race_result
}

async fn fault_drop(ctx: &Ctx, oids: &Oids, proxy: &Proxy, upstream: &str) -> Result<(), String> {
    proxy.stall.store(false, Ordering::SeqCst);
    let observer = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    let audit = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    let mut session = open_session(ctx, &ctx.proxy_host, ctx.proxy_port).await?;
    let pid = session.backend_pid();
    let stream = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        HOLD_SQL,
        EAST,
        10,
        1024,
        DROP_WALL_MS,
        &audit,
        false,
        false,
    )
    .await?;
    wait_blocked(&observer, pid, "sleep").await?;
    let accepts_before = proxy.accepts.load(Ordering::SeqCst);
    proxy.stall.store(true, Ordering::SeqCst);
    let started = Instant::now();
    drop(stream);
    let idle = session.wait_idle();
    tokio::pin!(idle);
    let blocked = tokio::select! {
        result = idle.as_mut() => {
            result.map_err(|_| "fault-drop-idle".to_string())?;
            false
        }
        () = tokio::time::sleep(Duration::from_millis(1_500)) => true,
    };
    let stalled_ms = u128_ms(started.elapsed());
    let view = activity(&observer, pid).await?;
    let accepts = proxy.accepts.load(Ordering::SeqCst).saturating_sub(accepts_before);
    note(
        ctx,
        &format!(
            "fault_drop_blocked={blocked} fault_drop_ms={stalled_ms} fault_drop_state={} fault_drop_active={} fault_cancel_accepts={accepts}",
            view.state,
            view.present && view.state == "active"
        ),
    );
    if blocked {
        release(proxy, upstream).await;
        idle.await.map_err(|_| "fault-drop-after".to_string())?;
    }
    Ok(())
}

async fn fault_race(ctx: &Ctx, oids: &Oids, proxy: &Proxy, upstream: &str) -> Result<(), String> {
    release(proxy, upstream).await;
    proxy.stall.store(false, Ordering::SeqCst);
    let observer = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    let audit = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    let mut session = open_session(ctx, &ctx.proxy_host, ctx.proxy_port).await?;
    let pid = session.backend_pid();
    let mut brief = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        BRIEF_SQL,
        EAST,
        10,
        1024,
        DROP_WALL_MS,
        &audit,
        false,
        false,
    )
    .await?;
    wait_blocked(&observer, pid, "sleep").await?;
    let handle = brief.cancel_handle();
    let direct = connect_client(ctx, &ctx.host, ctx.port, DB, ADMIN, &ctx.admin_pass).await?;
    let accepts_before = proxy.accepts.load(Ordering::SeqCst);
    proxy.stall.store(true, Ordering::SeqCst);
    let cancel_task = tokio::spawn(async move { cancel_label(handle, direct).await });
    let held = wait_accepts(proxy, accepts_before, Duration::from_secs(2)).await;
    let brief_label = match brief.next_row().await {
        Ok(Some(_)) => "row",
        Ok(None) => "end",
        Err(err) => err_label(&err),
    };
    session.wait_idle().await.map_err(|_| "race-brief-idle".to_string())?;
    drop(brief);
    note(
        ctx,
        &format!("fault_race_held={held} fault_race_brief={brief_label}"),
    );
    if !held || brief_label != "row" {
        release(proxy, upstream).await;
        let _ = tokio::time::timeout(Duration::from_secs(3), cancel_task).await;
        return Ok(());
    }
    let next_pid = session.backend_pid();
    let mut next = open_stream(
        &mut session,
        &identity(EAST),
        &catalog(oids, EAST),
        HOLD_SQL,
        EAST,
        10,
        1024,
        DROP_WALL_MS,
        &audit,
        false,
        false,
    )
    .await?;
    wait_blocked(&observer, next_pid, "sleep").await?;
    release(proxy, upstream).await;
    let next_result = tokio::time::timeout(Duration::from_millis(1_500), next_err(&mut next)).await;
    let after = activity(&observer, next_pid).await?;
    let cancel_joined = tokio::time::timeout(Duration::from_secs(3), cancel_task).await;
    let cancel_text = match cancel_joined {
        Ok(Ok(label)) => label,
        Ok(Err(_)) => "join-error",
        Err(_) => "join-timeout",
    };
    let next_label = match next_result {
        Ok(Ok(err)) => err_label(&err),
        Ok(Err(_)) => "stream-end",
        Err(_) => "still-running",
    };
    note(
        ctx,
        &format!(
            "fault_race_cancel={cancel_text} fault_race_next={next_label} fault_race_state={} fault_race_active={}",
            after.state,
            after.present && after.state == "active"
        ),
    );
    drop(next);
    session.wait_idle().await.map_err(|_| "race-next-idle".to_string())?;
    Ok(())
}

async fn wait_accepts(proxy: &Proxy, before: usize, bound: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < bound {
        if proxy.accepts.load(Ordering::SeqCst) > before {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

async fn cancel_label(handle: CancelHandle, client: Client) -> &'static str {
    match handle.cancel(&client).await {
        Ok(CancelOutcome::Sent) => "sent",
        Ok(CancelOutcome::NotSent) => "not_sent",
        Err(_) => "error",
    }
}

impl Proxy {
    async fn start(port: u16, upstream: String) -> Result<Self, String> {
        let listener = TcpListener::bind(("0.0.0.0", port))
            .await
            .map_err(|_| "proxy-bind".to_string())?;
        let stall = Arc::new(AtomicBool::new(false));
        let held = Arc::new(Mutex::new(Vec::new()));
        let accepts = Arc::new(AtomicUsize::new(0));
        let stall_task = Arc::clone(&stall);
        let held_task = Arc::clone(&held);
        let accepts_task = Arc::clone(&accepts);
        let task = tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                if stall_task.load(Ordering::SeqCst) {
                    accepts_task.fetch_add(1, Ordering::SeqCst);
                    held_task.lock().await.push(sock);
                } else {
                    let upstream = upstream.clone();
                    tokio::spawn(async move {
                        let _ = bridge(sock, &upstream).await;
                    });
                }
            }
        });
        Ok(Self { stall, held, accepts, task })
    }
}

async fn release(proxy: &Proxy, upstream: &str) {
    proxy.stall.store(false, Ordering::SeqCst);
    let drained: Vec<TcpStream> = proxy.held.lock().await.drain(..).collect();
    for sock in drained {
        let upstream = upstream.to_string();
        tokio::spawn(async move {
            let _ = bridge(sock, &upstream).await;
        });
    }
}

async fn bridge(mut inbound: TcpStream, upstream: &str) -> std::io::Result<()> {
    let mut outbound = TcpStream::connect(upstream).await?;
    let _ = copy_bidirectional(&mut inbound, &mut outbound).await;
    Ok(())
}

async fn ensure_proxy_name(name: &str) -> Result<(), String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|_| "local-ip".to_string())?;
    socket.connect("oo-a191-pg:5432").map_err(|_| "local-route".to_string())?;
    let ip = socket.local_addr().map_err(|_| "local-addr".to_string())?;
    let mut hosts = fs::read_to_string("/etc/hosts").map_err(|_| "hosts-read".to_string())?;
    if !hosts.contains(name) {
        if !hosts.ends_with('\n') {
            hosts.push('\n');
        }
        hosts.push_str(&format!("{} {name}\n", ip.ip()));
        fs::write("/etc/hosts", hosts).map_err(|_| "hosts-write".to_string())?;
    }
    Ok(())
}

async fn drop_and_measure(
    session: &mut DedicatedSession,
    stream: SelectStream,
    observer: &Client,
    pid: i32,
) -> Result<(u128, bool, QuerySummary), String> {
    let backend_start = session.backend_start();
    let started = Instant::now();
    drop(stream);
    session.wait_idle().await.map_err(|_| "drop-idle".to_string())?;
    let mut gone = false;
    while u128_ms(started.elapsed()) < 1_000 {
        if backend_gone(observer, pid, &backend_start).await? {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let summary = session.summary().ok_or("drop-summary".to_string())?;
    Ok((u128_ms(started.elapsed()), gone, summary))
}

async fn hold_blocked(client: &Client, pid: i32, kind: &str, hold_ms: u64) -> Result<(), String> {
    let started = Instant::now();
    while u128_ms(started.elapsed()) < u128::from(hold_ms) {
        let view = activity(client, pid).await?;
        if !view.present || view.state != "active" {
            return Err(format!("{kind}-left-active"));
        }
        if kind == "lock" && view.wait_type != "Lock" {
            return Err("lock-left-wait".to_string());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let view = activity(client, pid).await?;
    if !view.present || view.state != "active" {
        return Err(format!("{kind}-not-active-at-end"));
    }
    if kind == "lock" && view.wait_type != "Lock" {
        return Err("lock-not-waiting-at-end".to_string());
    }
    Ok(())
}

async fn wait_blocked(client: &Client, pid: i32, kind: &str) -> Result<(), String> {
    for _ in 0..80 {
        let view = activity(client, pid).await?;
        let ready = view.present && view.state == "active" && (kind != "lock" || view.wait_type == "Lock");
        if ready {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(format!("{kind}-not-blocked"))
}

async fn activity(client: &Client, pid: i32) -> Result<Activity, String> {
    let rows = client
        .query(
            "SELECT COALESCE(state, ''), COALESCE(wait_event_type, ''), COALESCE(wait_event, '') \
             FROM pg_catalog.pg_stat_activity WHERE pid = $1",
            &[&pid],
        )
        .await
        .map_err(|_| "activity".to_string())?;
    if rows.len() != 1 {
        return Ok(Activity {
            present: false,
            state: "gone".to_string(),
            wait_type: String::new(),
            wait_event: String::new(),
        });
    }
    Ok(Activity {
        present: true,
        state: rows[0].try_get(0).map_err(|_| "activity-state".to_string())?,
        wait_type: rows[0].try_get(1).map_err(|_| "activity-wait".to_string())?,
        wait_event: rows[0].try_get(2).map_err(|_| "activity-event".to_string())?,
    })
}

async fn backend_gone(client: &Client, pid: i32, backend_start: &str) -> Result<bool, String> {
    let row = client
        .query_one(
            "SELECT COUNT(*)::bigint FROM pg_catalog.pg_stat_activity \
             WHERE pid = $1 AND COALESCE(to_char(backend_start AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS'), '') = $2",
            &[&pid, &backend_start],
        )
        .await
        .map_err(|_| "gone".to_string())?;
    let count: i64 = row.try_get(0).map_err(|_| "gone-col".to_string())?;
    Ok(count == 0)
}

async fn xact_rollback(client: &Client) -> Result<i64, String> {
    let row = client
        .query_one(
            "SELECT xact_rollback FROM pg_catalog.pg_stat_database WHERE datname = current_database()",
            &[],
        )
        .await
        .map_err(|_| "xact".to_string())?;
    row.try_get(0).map_err(|_| "xact-col".to_string())
}

async fn audit_phase(client: &Client, phase: &str) -> Result<i64, String> {
    let row = client
        .query_one(
            "SELECT COUNT(*)::bigint FROM fixture.a191_audit WHERE phase = $1",
            &[&phase],
        )
        .await
        .map_err(|_| "audit-phase".to_string())?;
    row.try_get(0).map_err(|_| "audit-phase-col".to_string())
}

async fn audit_count(client: &Client) -> Result<i64, String> {
    let row = client
        .query_one("SELECT COUNT(*)::bigint FROM fixture.a191_audit", &[])
        .await
        .map_err(|_| "audit-count".to_string())?;
    row.try_get(0).map_err(|_| "audit-count-col".to_string())
}

async fn latest_complete(client: &Client) -> Result<(String, bool, String), String> {
    let row = client
        .query_one(
            "SELECT status, rollback_confirmed, COALESCE(sqlstate, '') \
             FROM fixture.a191_audit WHERE phase = 'complete' ORDER BY id DESC LIMIT 1",
            &[],
        )
        .await
        .map_err(|_| "latest".to_string())?;
    Ok((
        row.try_get(0).map_err(|_| "latest-status".to_string())?,
        row.try_get(1).map_err(|_| "latest-rollback".to_string())?,
        row.try_get(2).map_err(|_| "latest-sqlstate".to_string())?,
    ))
}

async fn finish_ok(
    session: &mut DedicatedSession,
    mut stream: SelectStream,
) -> Result<(Vec<Vec<Cell>>, QuerySummary), String> {
    let mut rows = Vec::new();
    while let Some(row) = stream.next_row().await.map_err(|_| "row".to_string())? {
        rows.push(row);
    }
    session.wait_idle().await.map_err(|_| "ok-idle".to_string())?;
    let summary = session.summary().ok_or("ok-summary".to_string())?;
    drop(stream);
    Ok((rows, summary))
}

async fn take_err(
    session: &mut DedicatedSession,
    mut stream: SelectStream,
) -> Result<(ExecError, QuerySummary), String> {
    let err = next_err(&mut stream).await?;
    session.wait_idle().await.map_err(|_| "err-idle".to_string())?;
    let summary = session.summary().ok_or("err-summary".to_string())?;
    drop(stream);
    Ok((err, summary))
}

async fn next_err(stream: &mut SelectStream) -> Result<ExecError, String> {
    match stream.next_row().await {
        Err(err) => Ok(err),
        Ok(_) => Err("expected-error".to_string()),
    }
}

async fn exec_ok(
    session: &mut DedicatedSession,
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    sql: &str,
    role: &str,
    audit: &Client,
) -> Result<(Vec<Vec<Cell>>, QuerySummary), String> {
    let stream = open_stream(session, identity, catalog, sql, role, 10, 1024, 5_000, audit, false, false).await?;
    finish_ok(session, stream).await
}

async fn exec_err(
    session: &mut DedicatedSession,
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    sql: &str,
    role: &str,
    max_rows: u32,
    max_bytes: u64,
    wall_ms: u64,
    audit: &Client,
    fail_admit: bool,
    fail_complete: bool,
) -> Result<ExecError, String> {
    match open_exec(
        session,
        identity,
        catalog,
        sql,
        role,
        max_rows,
        max_bytes,
        wall_ms,
        audit,
        fail_admit,
        fail_complete,
    )
    .await
    {
        Err(err) => Ok(err),
        Ok(_) => Err("expected-refusal".to_string()),
    }
}

async fn exec_wall(
    session: &mut DedicatedSession,
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    sql: &str,
    role: &str,
    wall: Duration,
    audit: &Client,
) -> Result<ExecError, String> {
    let owned = DurableAudit {
        client: connect_duplicate(audit).await?,
        fail_admit: false,
        fail_complete: false,
    };
    let request = SelectRequest {
        identity,
        catalog,
        original_sql: sql,
        role,
        max_rows: 10,
        max_result_bytes: 1024,
        wall_clock: wall,
    };
    match session.execute_trusted_select(request, owned).await {
        Err(err) => Ok(err),
        Ok(_) => Err("expected-wall".to_string()),
    }
}

async fn open_stream(
    session: &mut DedicatedSession,
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    sql: &str,
    role: &str,
    max_rows: u32,
    max_bytes: u64,
    wall_ms: u64,
    audit: &Client,
    fail_admit: bool,
    fail_complete: bool,
) -> Result<SelectStream, String> {
    open_exec(
        session,
        identity,
        catalog,
        sql,
        role,
        max_rows,
        max_bytes,
        wall_ms,
        audit,
        fail_admit,
        fail_complete,
    )
    .await
    .map_err(|err| err_label(&err).to_string())
}

async fn open_exec(
    session: &mut DedicatedSession,
    identity: &PolicyIdentity,
    catalog: &PolicyCatalog,
    sql: &str,
    role: &str,
    max_rows: u32,
    max_bytes: u64,
    wall_ms: u64,
    audit: &Client,
    fail_admit: bool,
    fail_complete: bool,
) -> Result<SelectStream, ExecError> {
    let owned = DurableAudit {
        client: connect_duplicate(audit).await.map_err(|_| ExecError::AuditUnavailable)?,
        fail_admit,
        fail_complete,
    };
    let request = SelectRequest {
        identity,
        catalog,
        original_sql: sql,
        role,
        max_rows,
        max_result_bytes: max_bytes,
        wall_clock: Duration::from_millis(wall_ms),
    };
    session.execute_trusted_select(request, owned).await
}

async fn connect_duplicate(_client: &Client) -> Result<Client, String> {
    let parts = {
        let guard = audit_slot().lock().map_err(|_| "audit-slot".to_string())?;
        guard.clone().ok_or_else(|| "audit-slot-empty".to_string())?
    };
    let ctx = Ctx {
        host: parts.host.clone(),
        port: parts.port,
        proxy_host: String::new(),
        proxy_port: 0,
        admin_pass: parts.pass.clone(),
        login_pass: String::new(),
        ca: parts.ca,
        oracle: String::new(),
    };
    connect_client(&ctx, &ctx.host, ctx.port, DB, ADMIN, &parts.pass).await
}

fn err_label(err: &ExecError) -> &'static str {
    match err {
        ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::Write) => "guard-write",
        ExecError::Guard(_) => "guard-other",
        ExecError::AuditUnavailable => "audit",
        ExecError::RoleParameter => "role-parameter",
        ExecError::RoleReuse => "role-reuse",
        ExecError::Limit => "limit",
        ExecError::Timeout { sqlstate: Some(_) } => "timeout-sqlstate",
        ExecError::Timeout { sqlstate: None } => "timeout-none",
        ExecError::Canceled { .. } => "canceled",
        ExecError::SessionClosed => "closed",
        ExecError::SessionBusy => "busy",
        _ => "other",
    }
}

fn outcome_name(outcome: CancelOutcome) -> &'static str {
    match outcome {
        CancelOutcome::Sent => "sent",
        CancelOutcome::NotSent => "not_sent",
    }
}

fn first_cell_is(rows: &[Vec<Cell>], text: &str) -> bool {
    matches!(rows.first().and_then(|row| row.first()), Some(Cell::Int(value)) if value == text)
}

fn cell_text(row: &[Cell]) -> String {
    match row.first() {
        Some(Cell::Int(value)) => value.clone(),
        Some(Cell::Text(value)) => value.clone(),
        Some(Cell::Bool(value)) => value.to_string(),
        Some(Cell::Null) | None => "none".to_string(),
    }
}

fn u128_ms(duration: Duration) -> u128 {
    duration.as_millis()
}

fn identity(role: &str) -> PolicyIdentity {
    PolicyIdentity {
        project: PROJECT.to_string(),
        role: role.to_string(),
        policy_id: POLICY.to_string(),
    }
}

fn catalog(oids: &Oids, role: &str) -> PolicyCatalog {
    let relation = |name: &str| VisibleRelation {
        schema: "fixture".to_string(),
        name: name.to_string(),
        physical_schema: "fixture".to_string(),
        physical_name: name.to_string(),
        columns: vec![VisibleColumn {
            name: "n".to_string(),
            physical_name: "n".to_string(),
            type_oid: oids.int4,
        }],
        pending_unclassified: 0,
        estimated_rows: 2,
    };
    PolicyCatalog {
        identity: identity(role),
        relations: vec![relation("gate"), relation("pace")],
        functions: vec![
            function(oids.hold, "a191_hold", Vec::new()),
            function(oids.brief, "a191_brief", Vec::new()),
            function(oids.pace, "a191_pace", vec![oids.int4]),
        ],
        operators: Vec::new(),
        literal_types: LiteralTypes {
            integer: Some(oids.int4),
            text: None,
            boolean: None,
        },
        null_test_allowed: true,
        large_table_row_threshold: 1_000_000,
    }
}

fn function(oid: u32, name: &str, args: Vec<u32>) -> ResolvedFunction {
    ResolvedFunction {
        oid,
        schema: "fixture".to_string(),
        name: name.to_string(),
        argument_type_oids: args,
        aggregate_star: false,
    }
}

async fn open_session(ctx: &Ctx, host: &str, port: u16) -> Result<DedicatedSession, String> {
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port,
        database: DB.to_string(),
        user: LOGIN.to_string(),
    };
    let (client, connection) = connect_require_tls(&endpoint, &ctx.login_pass, &ctx.ca)
        .await
        .map_err(|_| "session-connect".to_string())?;
    let tls = rustls_connector(&ctx.ca).map_err(|_| "session-tls".to_string())?;
    DedicatedSession::bind(client, connection, tls)
        .await
        .map_err(|_| "session-bind".to_string())
}

async fn connect_client(
    ctx: &Ctx,
    host: &str,
    port: u16,
    database: &str,
    user: &str,
    password: &str,
) -> Result<Client, String> {
    let endpoint = TlsEndpoint {
        host: host.to_string(),
        port,
        database: database.to_string(),
        user: user.to_string(),
    };
    let (client, connection) = connect_require_tls(&endpoint, password, &ctx.ca)
        .await
        .map_err(|_| "client-connect".to_string())?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

async fn reset_fixture(client: &Client) -> Result<(), String> {
    step(
        client,
        "terminate",
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'oo_a191' AND pid <> pg_backend_pid()",
    )
    .await?;
    step(client, "drop-db", "DROP DATABASE IF EXISTS oo_a191").await?;
    step(client, "drop-login", "DROP ROLE IF EXISTS oo_a191_login").await?;
    step(client, "drop-east", "DROP ROLE IF EXISTS oo_r_a191_east").await?;
    step(client, "drop-west", "DROP ROLE IF EXISTS oo_r_a191_west").await?;
    step(client, "drop-flag", "DROP ROLE IF EXISTS oo_r_a191_flag").await?;
    step(client, "create-db", "CREATE DATABASE oo_a191").await?;
    Ok(())
}

async fn install_fixture(client: &Client, login_pass: &str) -> Result<(), String> {
    step(
        client,
        "east",
        "CREATE ROLE oo_r_a191_east NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION",
    )
    .await?;
    step(
        client,
        "west",
        "CREATE ROLE oo_r_a191_west NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION",
    )
    .await?;
    step(
        client,
        "flag",
        "CREATE ROLE oo_r_a191_flag LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION",
    )
    .await?;
    let login = format!(
        "CREATE ROLE oo_a191_login LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '{login_pass}'"
    );
    step(client, "login", &login).await?;
    step(client, "grant-east", "GRANT oo_r_a191_east TO oo_a191_login").await?;
    step(client, "grant-west", "GRANT oo_r_a191_west TO oo_a191_login").await?;
    step(client, "grant-flag", "GRANT oo_r_a191_flag TO oo_a191_login").await?;
    step(client, "schema", "CREATE SCHEMA fixture").await?;
    step(client, "revoke-schema", "REVOKE ALL ON SCHEMA fixture FROM PUBLIC").await?;
    step(
        client,
        "usage",
        "GRANT USAGE ON SCHEMA fixture TO oo_r_a191_east, oo_r_a191_west, oo_r_a191_flag",
    )
    .await?;
    step(client, "gate", "CREATE TABLE fixture.gate (n integer)").await?;
    step(client, "pace", "CREATE TABLE fixture.pace (n integer)").await?;
    step(client, "gate-row", "INSERT INTO fixture.gate (n) VALUES (7)").await?;
    step(client, "pace-rows", "INSERT INTO fixture.pace (n) VALUES (1), (2)").await?;
    step(client, "revoke-gate", "REVOKE ALL ON fixture.gate FROM PUBLIC").await?;
    step(client, "revoke-pace", "REVOKE ALL ON fixture.pace FROM PUBLIC").await?;
    step(
        client,
        "grant-gate",
        "GRANT SELECT (n) ON fixture.gate TO oo_r_a191_east, oo_r_a191_west, oo_r_a191_flag",
    )
    .await?;
    step(client, "grant-pace", "GRANT SELECT (n) ON fixture.pace TO oo_r_a191_east").await?;
    step(
        client,
        "hold-fn",
        "CREATE FUNCTION fixture.a191_hold() RETURNS integer LANGUAGE plpgsql VOLATILE AS $fn$ BEGIN PERFORM pg_sleep(30); RETURN 1; END; $fn$",
    )
    .await?;
    step(
        client,
        "brief-fn",
        "CREATE FUNCTION fixture.a191_brief() RETURNS integer LANGUAGE plpgsql VOLATILE AS $fn$ BEGIN PERFORM pg_sleep(4); RETURN 1; END; $fn$",
    )
    .await?;
    step(
        client,
        "pace-fn",
        "CREATE FUNCTION fixture.a191_pace(n integer) RETURNS integer LANGUAGE plpgsql VOLATILE AS $fn$ BEGIN IF n = 1 THEN PERFORM pg_sleep(6.5); ELSE PERFORM pg_sleep(30); END IF; RETURN n; END; $fn$",
    )
    .await?;
    step(
        client,
        "revoke-fn",
        "REVOKE ALL ON FUNCTION fixture.a191_hold() FROM PUBLIC; REVOKE ALL ON FUNCTION fixture.a191_brief() FROM PUBLIC; REVOKE ALL ON FUNCTION fixture.a191_pace(integer) FROM PUBLIC",
    )
    .await?;
    step(
        client,
        "grant-fn",
        "GRANT EXECUTE ON FUNCTION fixture.a191_hold() TO oo_r_a191_east; GRANT EXECUTE ON FUNCTION fixture.a191_brief() TO oo_r_a191_east; GRANT EXECUTE ON FUNCTION fixture.a191_pace(integer) TO oo_r_a191_east",
    )
    .await?;
    step(
        client,
        "audit",
        "CREATE TABLE fixture.a191_audit ( \
            id bigserial PRIMARY KEY, \
            phase text NOT NULL, \
            status text, \
            rollback_confirmed boolean, \
            socket_dropped boolean, \
            stop_class text, \
            sqlstate text, \
            role_name text, \
            backend_pid integer, \
            backend_start text, \
            original_sha256 text, \
            executed_sha256 text, \
            fingerprint_hex text, \
            project text, \
            policy_id text, \
            returned_rows integer \
        )",
    )
    .await?;
    Ok(())
}

async fn load_oids(client: &Client) -> Result<Oids, String> {
    Ok(Oids {
        int4: one_oid(client, "SELECT oid FROM pg_catalog.pg_type WHERE typname = 'int4'").await?,
        hold: one_oid(
            client,
            "SELECT p.oid FROM pg_catalog.pg_proc AS p \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'fixture' AND p.proname = 'a191_hold' AND p.pronargs = 0",
        )
        .await?,
        brief: one_oid(
            client,
            "SELECT p.oid FROM pg_catalog.pg_proc AS p \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'fixture' AND p.proname = 'a191_brief' AND p.pronargs = 0",
        )
        .await?,
        pace: one_oid(
            client,
            "SELECT p.oid FROM pg_catalog.pg_proc AS p \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'fixture' AND p.proname = 'a191_pace' AND p.pronargs = 1",
        )
        .await?,
    })
}

async fn one_oid(client: &Client, sql: &str) -> Result<u32, String> {
    let row = client.query_one(sql, &[]).await.map_err(|_| "oid".to_string())?;
    row.try_get(0).map_err(|_| "oid-col".to_string())
}

async fn step(client: &Client, label: &str, sql: &str) -> Result<(), String> {
    client.batch_execute(sql).await.map_err(|err| {
        let code = err.code().map(|value| value.code().to_string()).unwrap_or_else(|| "none".to_string());
        format!("{label}:{code}")
    })
}
