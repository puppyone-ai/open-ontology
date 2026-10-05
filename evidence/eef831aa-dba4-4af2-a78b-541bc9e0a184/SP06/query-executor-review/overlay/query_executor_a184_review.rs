//! Independent review of `DedicatedSession::execute_trusted_select`.
//!
//! Runs only when `OO_A184_LIVE=1`. The fixture is a depot pallet, not the
//! author's shipment fixture. This file does not close SP06-W04, SP06-W05,
//! SP06-W06, the gateway, or a release.

use std::env;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use oo_gateway::query_executor::{
    observe_backend, AuditAdmission, CancelOutcome, Cell, CompletionRecord,
    CompletionStatus, DedicatedSession, ExecError, QuerySummary, SelectRequest, SelectStream,
    StopClass, UntrustedAdmission, UNTRUSTED_ADMISSION,
};
use oo_gateway::{
    connect_require_tls, guard_select, install_column_acl, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, GuardError, InstallEffect, LiteralKind, LiteralTypes,
    LogicalTable, OutputItem, PiiClass, PolicyCatalog, PolicyIdentity, ResolvedFunction,
    TlsEndpoint, VisibleColumn, VisibleRelation,
};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const HOST: &str = "oo-a184-pg";
const DATABASE: &str = "oo_a184";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a184_login";
const EAST: &str = "oo_r_a184_east";
const WEST: &str = "oo_r_a184_west";
const EAST_SLUG: &str = "a184_east";
const WEST_SLUG: &str = "a184_west";
const SECRET_DIR: &str = "/run/oo-a184/fixture";
const OWNER: &str = "a1840000-0000-4000-8000-000000000184";
const SHARD: &str = "shard_a184";
const SOURCE: &str = "depotproj";
const CONFIG: &str = "a184a184a184a184a184a184a184a184a184a184a184a184a184a184a184a184";
const TYPE_DIGEST: &str = "b184b184b184b184b184b184b184b184b184b184b184b184b184b184b184b184";
const INPUT_DIGEST: &str = "c184c184c184c184c184c184c184c184c184c184c184c184c184c184c184c184";
const SCHEMA_DIGEST: &str = "d184d184d184d184d184d184d184d184d184d184d184d184d184d184d184d184";
const NAMESPACE: &str = "depot";
const TABLE: &str = "pallet";
const STALE_GENERATION: i64 = 2;
const CURRENT_GENERATION: i64 = 5;
const HIDDEN_CELL: &str = "a184-hidden-cell";
const WALL_OK: u64 = 30_000;
const BYTES_OK: u64 = 256 * 1024;
const DEADLINE_WALL_MS: u64 = 4_500;
const LOCK_HOLD: Duration = Duration::from_millis(1_500);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn review_dedicated_select() {
    if env::var("OO_A184_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A184_LIVE is unset; the review comparison was not run");
        return;
    }
    run_review().await;
}

struct Shadow {
    table: String,
    n2: String,
    n4: String,
    n8: String,
    tag: String,
    lit: String,
    note: String,
    qty: String,
    hidden: String,
}

struct Oids {
    int2: u32,
    int4: u32,
    int8: u32,
    text: u32,
    bool: u32,
    numeric: u32,
    varchar: u32,
}

struct Oracle {
    lines: Vec<String>,
    secrets: Vec<String>,
    path: String,
}

impl Oracle {
    fn new(admin_pass: &str, login_pass: &str) -> Self {
        let path = env::var("OO_A184_ORACLE").unwrap_or_else(|_| "/evidence/live/oracle.txt".to_string());
        Self {
            lines: Vec::new(),
            secrets: vec![admin_pass.to_string(), login_pass.to_string()],
            path,
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("password"), "oracle key");
        assert!(!value.contains("SELECT "), "oracle value contained SQL");
        assert!(!value.contains(HIDDEN_CELL), "oracle value contained the hidden cell");
        for secret in &self.secrets {
            assert!(!value.contains(secret), "oracle value contained a password");
        }
        self.lines.push(format!("{key}={value}"));
        self.flush();
    }

    fn flush(&self) {
        let path = Path::new(&self.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(path, self.lines.join("\n") + "\n").expect("oracle write");
    }
}

struct LiveAudit {
    client: Client,
    fail_admit: bool,
    fail_complete: bool,
}

impl AuditAdmission for LiveAudit {
    async fn admit(&mut self, admission: &oo_gateway::query_executor::Admission<'_>) -> Result<(), ExecError> {
        if self.fail_admit {
            return Err(ExecError::AuditUnavailable);
        }
        self.insert("admit", admission.original_sha256(), admission.executed_sha256(), admission.role(), admission.backend_pid(), admission.backend_start(), None)
            .await
    }

    async fn complete(&mut self, completion: &CompletionRecord) -> Result<(), ExecError> {
        if self.fail_complete {
            return Err(ExecError::AuditUnavailable);
        }
        self.insert(
            "complete",
            completion.original_sha256(),
            completion.executed_sha256(),
            completion.role(),
            completion.backend_pid(),
            completion.backend_start(),
            Some(completion.status().as_str()),
        )
        .await
    }
}

impl LiveAudit {
    #[allow(clippy::too_many_arguments)] // phase, both digests, role, pid, start, status
    async fn insert(
        &self,
        phase: &str,
        original: &str,
        executed: &str,
        role: &str,
        pid: i32,
        start: &str,
        status: Option<&str>,
    ) -> Result<(), ExecError> {
        let phase = phase.to_string();
        let original = original.to_string();
        let executed = executed.to_string();
        let role = role.to_string();
        let start = start.to_string();
        let status = status.map(str::to_string);
        self.client
            .execute(
                "INSERT INTO depot.a184_audit \
                 (phase, original_sha256, executed_sha256, role_name, backend_pid, backend_start, completion_status) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[&phase, &original, &executed, &role, &pid, &start, &status],
            )
            .await
            .map_err(|_| ExecError::AuditUnavailable)?;
        Ok(())
    }
}

struct Env {
    oracle: Oracle,
    admin: Client,
    watch: Client,
    admin_pass: String,
    login_pass: String,
    ca: Vec<u8>,
    shadow: Shadow,
    oids: Oids,
    touch_oid: u32,
    pause_oid: u32,
    who_oid: u32,
}

struct Outcome {
    rows: Vec<Vec<Cell>>,
    error: Option<ExecError>,
    summary: QuerySummary,
}

async fn run_review() {
    let admin_pass = read_secret("admin.pass");
    let login_pass = read_secret("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca");
    let mut oracle = Oracle::new(&admin_pass, &login_pass);
    oracle.fact(
        "untrusted_admission",
        match UNTRUSTED_ADMISSION {
            UntrustedAdmission::Blocked => "Blocked",
        },
    );
    let root = connect_db("postgres", ADMIN, &admin_pass, &ca).await;
    reset_database(&root).await;
    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, &login_pass).await;
    let shadow = prepare_shadow(&mut admin).await;
    let oids = load_oids(&admin, &shadow).await;
    let (touch_oid, pause_oid, who_oid) = install_functions(&admin).await;
    install_audit(&admin).await;
    install_roles(&mut admin).await;
    grant_functions(&admin, &shadow).await;
    let watch = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    let mut env = Env {
        oracle,
        admin,
        watch,
        admin_pass,
        login_pass,
        ca,
        shadow,
        oids,
        touch_oid,
        pause_oid,
        who_oid,
    };
    drop(root);
    prove_platform(&mut env).await;
    prove_reads(&mut env).await;
    env.oracle.fact("review_result", "compared");
}

async fn prove_platform(env: &mut Env) {
    let version: String = env
        .admin
        .query_one("SHOW server_version", &[])
        .await
        .expect("version")
        .get(0);
    env.oracle.fact("server_version", &version);
    assert!(version.starts_with("17.10"), "server version");
    let track: String = env
        .admin
        .query_one("SHOW track_functions", &[])
        .await
        .expect("track")
        .get(0);
    env.oracle.fact("track_functions", &track);
    assert_eq!(track, "pl");
    assert_role_flags(&env.admin, EAST).await;
    assert_role_flags(&env.admin, WEST).await;
    env.oracle.fact("role_flags", "east_west_all_false");
    let volatile: String = env
        .admin
        .query_one(
            "SELECT provolatile::text FROM pg_catalog.pg_proc WHERE oid = ($1::int4)::oid",
            &[&i32::try_from(env.touch_oid).expect("touch oid")],
        )
        .await
        .expect("volatile")
        .get(0);
    env.oracle.fact("touch_volatility", &volatile);
    assert_eq!(volatile, "v");
    env.oracle.fact("function_whitelist", "not_claimed");
    let insert_audit: bool = env
        .admin
        .query_one(
            "SELECT has_table_privilege($1, 'depot.a184_audit', 'INSERT')",
            &[&EAST],
        )
        .await
        .expect("audit privilege")
        .get(0);
    env.oracle.fact("east_insert_audit", if insert_audit { "true" } else { "false" });
    assert!(!insert_audit);
    env.oracle.fact("acl_east_hidden", &direct_state(&env.admin, EAST, &env.shadow.table, &env.shadow.hidden).await);
    env.oracle.fact("acl_east_n4", &direct_state(&env.admin, EAST, &env.shadow.table, &env.shadow.n4).await);
    env.oracle.fact("acl_west_n2", &direct_state(&env.admin, WEST, &env.shadow.table, &env.shadow.n2).await);
    env.oracle.fact("acl_west_n4", &direct_state(&env.admin, WEST, &env.shadow.table, &env.shadow.n4).await);
    assert_eq!(env.oracle.lines.iter().find(|line| line.starts_with("acl_east_hidden=")).expect("acl"), "acl_east_hidden=42501");
    assert_eq!(env.oracle.lines.iter().find(|line| line.starts_with("acl_east_n4=")).expect("acl"), "acl_east_n4=ok");
    assert_eq!(env.oracle.lines.iter().find(|line| line.starts_with("acl_west_n2=")).expect("acl"), "acl_west_n2=42501");
    assert_eq!(env.oracle.lines.iter().find(|line| line.starts_with("acl_west_n4=")).expect("acl"), "acl_west_n4=ok");
}

async fn direct_state(client: &Client, role: &str, table: &str, column: &str) -> String {
    assert!(role.starts_with("oo_r_") && !role.contains('"'), "role shape");
    assert!(table.starts_with("pt_") && table.len() == 63 && !table.contains('"'), "table shape");
    assert!(column.starts_with("pc_") && !column.contains('"'), "column shape");
    client.batch_execute("BEGIN").await.expect("direct begin");
    let set_role = format!("SET LOCAL ROLE \"{role}\"");
    client.batch_execute(&set_role).await.expect("set role");
    let statement = format!("SELECT \"{column}\" FROM \"_oo\".\"{table}\" LIMIT 1");
    let state = match client.query(&statement, &[]).await {
        Ok(_) => "ok".to_string(),
        Err(err) => sqlstate_only(&err),
    };
    client.batch_execute("ROLLBACK").await.expect("direct rollback");
    state
}

async fn prove_reads(env: &mut Env) {
    let expected = admin_texts(env).await;
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let backend_start = session.backend_start();
    env.oracle.fact("backend_pid_set", if pid > 0 { "yes" } else { "no" });
    env.oracle.fact("backend_start_len", &backend_start.len().to_string());
    assert!(pid > 0 && backend_start.len() == 20, "binding");

    let typed_sql = "SELECT n2, n4, n8, tag, lit, note FROM depot.pallet";
    let typed_catalog = happy_catalog(env);
    let typed_identity = identity_for(EAST);
    let plan = guard_select(&typed_identity, &typed_catalog, typed_sql).expect("typed guard");
    assert!(plan.executed_sql.contains(&env.shadow.table), "physical table missing");
    assert!(!plan.executed_sql.contains("depot.pallet"), "logical relation survived");
    assert!(!plan.executed_sql.contains(HIDDEN_CELL), "hidden cell in plan");
    let before_audit = audit_count(&env.watch).await;
    let outcome = run_ok(env, &mut session, &typed_catalog, &typed_identity, typed_sql, EAST, 100, BYTES_OK, WALL_OK, false, false).await;
    assert!(outcome.error.is_none(), "typed query failed");
    assert_eq!(outcome.summary.status(), CompletionStatus::Succeeded);
    assert!(outcome.summary.rollback_confirmed());
    assert!(!outcome.summary.socket_dropped());
    assert!(!outcome.summary.truncated());
    assert!(outcome.summary.end_audit_ok());
    assert_eq!(outcome.summary.sqlstate(), None);
    let got = rendered(&outcome.rows);
    env.oracle.fact("typed_rows", &got.len().to_string());
    env.oracle.fact("typed_compare", if got == expected { "exact" } else { "mismatch" });
    assert_eq!(got, expected, "typed cells differed from the database");
    assert!(got.iter().all(|row| !row.contains(HIDDEN_CELL)));
    let audited = latest_audit(&env.watch, "admit").await;
    assert_eq!(audited.1, plan.executed_sha256);
    assert_eq!(audited.2, EAST);
    assert_eq!(audited.3, pid);
    assert_eq!(audited.4, backend_start);
    env.oracle.fact("audit_matches_plan", "yes");
    env.oracle.fact("audit_delta_typed", &(audit_count(&env.watch).await - before_audit).to_string());
    assert!(xact_cleared(&env.watch, pid).await, "transaction still open");
    env.oracle.fact("rollback_xact_cleared", "yes");
    assert_eq!(session.backend_pid(), pid);
    assert_eq!(session.backend_start(), backend_start);

    let star = run_ok(env, &mut session, &typed_catalog, &typed_identity, "SELECT * FROM depot.pallet", EAST, 100, BYTES_OK, WALL_OK, false, false).await;
    assert!(star.error.is_none(), "star failed");
    assert_eq!(star.rows.len(), 4);
    assert!(star.rows.iter().all(|row| row.len() == 6));
    assert!(star.rows.iter().flatten().all(|cell| cell_text(cell) != HIDDEN_CELL));
    env.oracle.fact("star_columns", "6");
    env.oracle.fact("star_hidden_absent", "yes");

    let who_catalog = catalog(env, EAST, happy_columns(env), &["who"]);
    let who = run_ok(env, &mut session, &who_catalog, &typed_identity, "SELECT depot.a184_who() FROM depot.pallet", EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    assert!(who.error.is_none(), "who failed");
    let who_text = cell_text(&who.rows[0][0]);
    env.oracle.fact("who", if who_text == format!("{EAST}/{LOGIN}") { "role_and_login" } else { "mismatch" });
    assert_eq!(who_text, format!("{EAST}/{LOGIN}"));
    let who_again = run_ok(env, &mut session, &who_catalog, &typed_identity, "SELECT depot.a184_who() FROM depot.pallet", EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    assert_eq!(cell_text(&who_again.rows[0][0]), format!("{EAST}/{LOGIN}"));
    assert!(xact_cleared(&env.watch, pid).await, "role leaked as an open transaction");
    env.oracle.fact("role_reapplied_after_rollback", "yes");

    reset_calls(env, env.touch_oid).await;
    let touch_catalog = catalog(env, EAST, happy_columns(env), &["touch"]);
    let mut count_session = open_session(env).await;
    let bounded = run_ok(
        env,
        &mut count_session,
        &touch_catalog,
        &typed_identity,
        "SELECT depot.a184_touch(), tag FROM depot.pallet",
        EAST,
        2,
        BYTES_OK,
        WALL_OK,
        false,
        false,
    )
    .await;
    assert!(bounded.error.is_none(), "row bound failed");
    assert_eq!(bounded.summary.status(), CompletionStatus::Succeeded);
    assert!(bounded.summary.truncated());
    assert_eq!(bounded.rows.len(), 2);
    assert_eq!(bounded.summary.returned_rows(), 2);
    // Pending function counters are published when the calling backend exits.
    count_session.close();
    let calls = calls_until(env, "a184_touch", 3).await;
    env.oracle.fact("row_trunc_rows", "2");
    env.oracle.fact("row_trunc_calls", &calls.to_string());
    env.oracle.fact("row_trunc_status", bounded.summary.status().as_str());
    assert_eq!(calls, 3, "max_rows+1 was not the fetch");

    let byte = run_ok(env, &mut session, &typed_catalog, &typed_identity, "SELECT lit FROM depot.pallet", EAST, 100, 3, WALL_OK, false, false).await;
    assert!(byte.error.is_none(), "byte cap failed");
    assert_eq!(byte.summary.status(), CompletionStatus::Succeeded);
    assert!(byte.summary.truncated());
    assert_eq!(byte.rows.len(), 0);
    assert_eq!(byte.summary.result_bytes(), 0);
    assert_eq!(byte.summary.sqlstate(), None);
    env.oracle.fact("byte_trunc_rows", "0");
    env.oracle.fact("byte_trunc_status", "succeeded");
    env.oracle.fact("byte_trunc_sqlstate", "none");

    let pressure_audit = audit_count(&env.watch).await;
    let mut pressure = open_stream(env, &mut session, &typed_catalog, &typed_identity, "SELECT tag FROM depot.pallet", EAST, 100, BYTES_OK, WALL_OK, false, false).await;
    let mut saw_open = false;
    for _ in 0..40 {
        if session.summary().is_some() {
            break;
        }
        if xact_open(&env.watch, pid).await {
            saw_open = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(session.summary().is_none(), "producer finished before the consumer read");
    assert!(saw_open, "producer did not hold the transaction");
    env.oracle.fact("backpressure_open_xact", "yes");
    let drained = finish_stream(&mut session, pressure.as_mut().expect("pressure stream")).await;
    assert!(drained.error.is_none(), "backpressure drain failed");
    assert_eq!(drained.rows.len(), 4);
    assert_eq!(drained.summary.status(), CompletionStatus::Succeeded);
    assert!(audit_count(&env.watch).await > pressure_audit);

    let reuse_before = audit_count(&env.watch).await;
    let west_identity = identity_for(WEST);
    let west_catalog = catalog(env, WEST, vec![("n4", env.shadow.n4.as_str(), env.oids.int4)], &[]);
    let reuse = exec_result(env, &mut session, &west_catalog, &west_identity, "SELECT n4 FROM depot.pallet", WEST, 10, BYTES_OK, WALL_OK, false, false).await;
    match reuse {
        Err(err) => {
            note_err(&mut env.oracle, "role_reuse", &err);
            assert!(matches!(err, ExecError::RoleReuse));
        }
        Ok(_) => panic!("cross-role reuse was accepted"),
    }
    assert_eq!(audit_count(&env.watch).await, reuse_before);
    env.oracle.fact("role_reuse_audit_delta", "0");
    assert!(session.is_open());
    assert_eq!(session.bound_role().as_deref(), Some(EAST));
    drop(session);

    late_and_cancel(env, pid, &backend_start).await;
    deadline_pages(env).await;
    server_timeout(env).await;
    drop_partial(env, false, "drop").await;
    drop_partial(env, true, "complete_fail").await;
    drop_while_blocked(env).await;
    type_refusal(env, "SELECT qty, depot.a184_touch() FROM depot.pallet", "qty", env.oids.numeric, "numeric").await;
    type_refusal(env, "SELECT code, depot.a184_touch() FROM depot.pallet", "code", env.oids.varchar, "varchar").await;
    literal_probe(env).await;
    admit_failure(env).await;
    guard_then_other_role(env).await;
    lying_column(env).await;
    bad_roles(env).await;
    omitted_function(env).await;
}

async fn late_and_cancel(env: &mut Env, _old_pid: i32, _old_start: &str) {
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let start = session.backend_start();
    let catalog = happy_catalog(env);
    let identity = identity_for(EAST);
    let first = open_stream(env, &mut session, &catalog, &identity, "SELECT tag FROM depot.pallet", EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    let mut stream = first.expect("first stream");
    let stale = stream.cancel_handle();
    let first_out = finish_stream(&mut session, &mut stream).await;
    assert!(first_out.error.is_none(), "late-cancel setup failed");
    assert_eq!(session.generation() % 2, 0);

    lock_table(&env.admin, &env.shadow.table).await;
    let second = open_stream(env, &mut session, &catalog, &identity, "SELECT tag FROM depot.pallet", EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    let mut second_stream = second.expect("second stream");
    assert!(wait_physical(&env.watch, pid, &env.shadow.table).await, "second query did not reach the server");
    let late = stale.cancel(&env.watch).await.expect("late cancel");
    env.oracle.fact("late_cancel", match late {
        CancelOutcome::NotSent => "not_sent",
        CancelOutcome::Sent => "sent",
    });
    assert_eq!(late, CancelOutcome::NotSent);
    assert!(backend_still(env, pid, &start).await, "late cancel disturbed the backend");
    unlock_table(&env.admin).await;
    let second_out = finish_stream(&mut session, &mut second_stream).await;
    assert!(second_out.error.is_none(), "late cancel killed the next query");
    assert_eq!(second_out.summary.status(), CompletionStatus::Succeeded);
    assert_eq!(second_out.rows.len(), 4);
    env.oracle.fact("late_cancel_next_status", "succeeded");
    assert_eq!(session.backend_pid(), pid);
    assert_eq!(session.backend_start(), start);

    lock_table(&env.admin, &env.shadow.table).await;
    let third = open_stream(env, &mut session, &catalog, &identity, "SELECT tag FROM depot.pallet", EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    let mut third_stream = third.expect("third stream");
    assert!(wait_physical(&env.watch, pid, &env.shadow.table).await, "cancel target did not start");
    let fresh = third_stream.cancel_handle();
    let sent = fresh.cancel(&env.watch).await.expect("fresh cancel");
    env.oracle.fact("inflight_cancel", match sent {
        CancelOutcome::Sent => "sent",
        CancelOutcome::NotSent => "not_sent",
    });
    assert_eq!(sent, CancelOutcome::Sent);
    let canceled = finish_stream(&mut session, &mut third_stream).await;
    unlock_table(&env.admin).await;
    let err = canceled.error.expect("cancel did not fail the stream");
    note_err(&mut env.oracle, "inflight_cancel_display", &err);
    assert!(matches!(err, ExecError::Canceled { .. }));
    assert_eq!(err.sqlstate(), Some("57014"));
    assert_eq!(canceled.summary.status(), CompletionStatus::Failed);
    assert_ne!(canceled.summary.status(), CompletionStatus::Succeeded);
    assert_eq!(canceled.summary.stop(), Some(StopClass::Canceled));
    env.oracle.fact("inflight_cancel_sqlstate", "57014");
    env.oracle.fact("inflight_cancel_status", "failed");
    assert!(!session.is_open());
}

async fn deadline_pages(env: &mut Env) {
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let catalog = catalog(env, EAST, happy_columns(env), &["pause"]);
    let identity = identity_for(EAST);
    let sql = "SELECT depot.a184_pause(), tag FROM depot.pallet";
    let plan = guard_select(&identity, &catalog, sql).expect("pause guard");
    assert!(plan.executed_sql.contains(&env.shadow.table));
    assert!(!plan.executed_sql.contains("depot.pallet"));
    reset_calls(env, env.pause_oid).await;
    let audit_before = audit_count(&env.watch).await;
    lock_table(&env.admin, &env.shadow.table).await;
    let started = Instant::now();
    let opened = open_stream(env, &mut session, &catalog, &identity, sql, EAST, 10, BYTES_OK, DEADLINE_WALL_MS, false, false).await;
    let mut stream = opened.expect("deadline stream");
    assert!(wait_physical(&env.watch, pid, &env.shadow.table).await, "deadline query was not visible");
    let query_text_flags = activity_flags(&env.watch, pid, &env.shadow.table, &env.login_pass).await;
    env.oracle.fact("physical_query_seen", if query_text_flags.0 { "yes" } else { "no" });
    env.oracle.fact("logical_query_absent", if query_text_flags.1 { "yes" } else { "no" });
    env.oracle.fact("activity_secret_absent", if query_text_flags.2 { "yes" } else { "no" });
    assert!(query_text_flags.0 && query_text_flags.1 && query_text_flags.2);
    let visible_audit = audit_count(&env.watch).await;
    let calls_while_waiting = function_calls(&env.watch, "a184_pause").await;
    env.oracle.fact("audit_visible_while_blocked", if visible_audit > audit_before { "yes" } else { "no" });
    env.oracle.fact("pause_calls_while_blocked", &calls_while_waiting.to_string());
    assert!(visible_audit > audit_before, "audit row was not durable before execution");
    assert_eq!(calls_while_waiting, 0, "function ran before the lock cleared");
    tokio::time::sleep(LOCK_HOLD).await;
    unlock_table(&env.admin).await;
    let mut rows = Vec::new();
    let error = loop {
        match stream.next_row().await {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => break None,
            Err(err) => break Some(err),
        }
    };
    let elapsed = started.elapsed();
    drop(stream);
    session.wait_idle().await.expect("deadline idle");
    let summary = session.summary().expect("deadline summary");
    env.oracle.fact("client_deadline_rows", &rows.len().to_string());
    env.oracle.fact("client_deadline_status", summary.status().as_str());
    env.oracle.fact("client_deadline_sqlstate", summary.sqlstate().unwrap_or("none"));
    env.oracle.fact("client_deadline_elapsed_ms", &elapsed.as_millis().to_string());
    let err = error.expect("client deadline returned the whole result");
    note_err(&mut env.oracle, "client_deadline_display", &err);
    assert!(matches!(err, ExecError::Timeout { sqlstate: None }));
    assert_eq!(rows.len(), 1, "page count did not show a single delivered page");
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_ne!(summary.status(), CompletionStatus::Succeeded);
    assert!(elapsed >= Duration::from_millis(DEADLINE_WALL_MS - 400), "deadline returned early");
    assert!(elapsed < Duration::from_millis(DEADLINE_WALL_MS + 1_500), "deadline exceeded one client budget");
    assert!(!session.is_open());
}

async fn server_timeout(env: &mut Env) {
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let catalog = happy_catalog(env);
    let identity = identity_for(EAST);
    lock_table(&env.admin, &env.shadow.table).await;
    let started = Instant::now();
    let opened = open_stream(env, &mut session, &catalog, &identity, "SELECT tag FROM depot.pallet", EAST, 10, BYTES_OK, 1_800, false, false).await;
    let mut stream = opened.expect("server timeout stream");
    assert!(wait_physical(&env.watch, pid, &env.shadow.table).await, "server-timeout query was not visible");
    let error = match stream.next_row().await {
        Ok(Some(_)) => panic!("server timeout delivered a row"),
        Ok(None) => panic!("server timeout ended cleanly"),
        Err(err) => err,
    };
    let elapsed = started.elapsed();
    drop(stream);
    session.wait_idle().await.expect("server timeout idle");
    unlock_table(&env.admin).await;
    let summary = session.summary().expect("server timeout summary");
    note_err(&mut env.oracle, "server_timeout_display", &error);
    env.oracle.fact("server_timeout_sqlstate", error.sqlstate().unwrap_or("none"));
    env.oracle.fact("server_timeout_elapsed_ms", &elapsed.as_millis().to_string());
    env.oracle.fact("server_timeout_status", summary.status().as_str());
    assert_eq!(error.sqlstate(), Some("57014"));
    assert!(matches!(error, ExecError::Timeout { sqlstate: Some(_) }));
    assert_eq!(summary.stop(), Some(StopClass::Timeout));
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.returned_rows(), 0);
    assert!(elapsed < Duration::from_millis(2_200));
    assert!(!session.is_open());
}

async fn drop_partial(env: &mut Env, fail_complete: bool, label: &str) {
    let mut session = open_session(env).await;
    let catalog = happy_catalog(env);
    let identity = identity_for(EAST);
    let before_complete = phase_count(&env.watch, "complete").await;
    let opened = open_stream(env, &mut session, &catalog, &identity, "SELECT tag FROM depot.pallet", EAST, 10, BYTES_OK, WALL_OK, false, fail_complete).await;
    let stream = opened.expect("drop stream");
    drop(stream);
    session.wait_idle().await.expect("drop idle");
    let summary = session.summary().expect("drop summary");
    env.oracle.fact(&format!("{label}_status"), summary.status().as_str());
    env.oracle.fact(&format!("{label}_end_audit_ok"), if summary.end_audit_ok() { "true" } else { "false" });
    assert_eq!(summary.status(), CompletionStatus::Partial);
    assert_ne!(summary.status(), CompletionStatus::Succeeded);
    if fail_complete {
        assert!(!summary.end_audit_ok());
        assert_eq!(phase_count(&env.watch, "complete").await, before_complete);
    } else {
        assert!(summary.end_audit_ok());
        let status = latest_audit(&env.watch, "complete").await.5;
        env.oracle.fact("drop_durable_status", &status);
        assert_eq!(status, "partial");
    }
    assert!(!session.is_open());
}

async fn drop_while_blocked(env: &mut Env) {
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let catalog = happy_catalog(env);
    let identity = identity_for(EAST);
    lock_table(&env.admin, &env.shadow.table).await;
    let opened = open_stream(env, &mut session, &catalog, &identity, "SELECT tag FROM depot.pallet", EAST, 10, BYTES_OK, 3_000, false, false).await;
    let stream = opened.expect("blocked drop stream");
    assert!(wait_physical(&env.watch, pid, &env.shadow.table).await, "blocked drop was not visible");
    let started = Instant::now();
    drop(stream);
    session.wait_idle().await.expect("blocked drop idle");
    let elapsed = started.elapsed();
    unlock_table(&env.admin).await;
    let summary = session.summary().expect("blocked drop summary");
    env.oracle.fact("blocked_drop_status", summary.status().as_str());
    env.oracle.fact("blocked_drop_sqlstate", summary.sqlstate().unwrap_or("none"));
    env.oracle.fact("blocked_drop_elapsed_ms", &elapsed.as_millis().to_string());
    assert_ne!(summary.status(), CompletionStatus::Succeeded);
    assert!(!session.is_open());
}

async fn type_refusal(env: &mut Env, sql: &str, label: &str, oid: u32, name: &str) {
    reset_calls(env, env.touch_oid).await;
    let mut session = open_session(env).await;
    let columns = if label == "qty" {
        vec![("qty", env.shadow.qty.as_str(), env.oids.numeric)]
    } else {
        vec![("code", "a184_code", env.oids.varchar)]
    };
    let catalog = catalog(env, EAST, columns, &["touch"]);
    let identity = identity_for(EAST);
    let opened = open_stream(env, &mut session, &catalog, &identity, sql, EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    let mut stream = opened.expect("type stream");
    let err = match stream.next_row().await {
        Err(err) => err,
        Ok(_) => panic!("{label} type refusal executed"),
    };
    drop(stream);
    session.wait_idle().await.expect("type idle");
    let summary = session.summary().expect("type summary");
    note_err(&mut env.oracle, &format!("type_{name}_display"), &err);
    assert!(matches!(err, ExecError::TypeRefused { oid: got } if got == oid));
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.stop(), Some(StopClass::TypeRefused));
    assert_ne!(summary.status(), CompletionStatus::Succeeded);
    let calls = calls_after_disconnect(env, "a184_touch").await;
    env.oracle.fact(&format!("type_{name}_oid"), &oid.to_string());
    env.oracle.fact(&format!("type_{name}_calls"), &calls.to_string());
    assert_eq!(calls, 0, "{label} executed the volatile function");
    assert!(!session.is_open());
}

async fn literal_probe(env: &mut Env) {
    reset_calls(env, env.touch_oid).await;
    let catalog = catalog(env, EAST, happy_columns(env), &["touch"]);
    let identity = identity_for(EAST);
    let sql = "SELECT 'a184-literal', depot.a184_touch() FROM depot.pallet";
    let plan = guard_select(&identity, &catalog, sql).expect("literal guard");
    let literal_text = plan.bindings.outputs.iter().any(|item| matches!(item, OutputItem::Literal(LiteralKind::Text)));
    env.oracle.fact("literal_guard_class", if literal_text { "text" } else { "other" });
    let mut session = open_session(env).await;
    let opened = open_stream(env, &mut session, &catalog, &identity, sql, EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    let mut stream = opened.expect("literal stream");
    match stream.next_row().await {
        Err(err) => {
            note_err(&mut env.oracle, "literal_display", &err);
            if let ExecError::TypeRefused { oid } = &err {
                env.oracle.fact("literal_refused_oid", &oid.to_string());
            }
            drop(stream);
            session.wait_idle().await.expect("literal idle");
            let calls = calls_after_disconnect(env, "a184_touch").await;
            env.oracle.fact("literal_calls", &calls.to_string());
            assert_eq!(calls, 0);
        }
        Ok(Some(_)) => {
            let calls = function_calls_flushed(&env.watch, "a184_touch").await;
            env.oracle.fact("literal_executed", "yes");
            env.oracle.fact("literal_calls", &calls.to_string());
            while let Ok(Some(_)) = stream.next_row().await {}
            drop(stream);
            session.wait_idle().await.expect("literal idle");
        }
        Ok(None) => panic!("literal query produced no row and no error"),
    }
}

async fn admit_failure(env: &mut Env) {
    reset_calls(env, env.touch_oid).await;
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let before = audit_count(&env.watch).await;
    let catalog = catalog(env, EAST, happy_columns(env), &["touch"]);
    let identity = identity_for(EAST);
    let result = exec_result(
        env,
        &mut session,
        &catalog,
        &identity,
        "SELECT depot.a184_touch(), tag FROM depot.pallet",
        EAST,
        10,
        BYTES_OK,
        WALL_OK,
        true,
        false,
    )
    .await;
    let err = match result {
        Err(err) => err,
        Ok(_) => panic!("admit failure still returned a stream"),
    };
    note_err(&mut env.oracle, "admit_fail_display", &err);
    assert!(matches!(err, ExecError::AuditUnavailable));
    assert_eq!(audit_count(&env.watch).await, before);
    assert!(!activity_has_table(&env.watch, pid, &env.shadow.table).await, "user query started after admit failure");
    let calls = function_calls_flushed(&env.watch, "a184_touch").await;
    env.oracle.fact("admit_fail_calls", &calls.to_string());
    env.oracle.fact("admit_fail_audit_delta", "0");
    assert_eq!(calls, 0);
    assert!(session.is_open());
    session.close();
}

async fn guard_then_other_role(env: &mut Env) {
    let mut session = open_session(env).await;
    let before = audit_count(&env.watch).await;
    let east = identity_for(EAST);
    let east_catalog = happy_catalog(env);
    let refused = exec_result(env, &mut session, &east_catalog, &east, "INSERT INTO depot.pallet (tag) VALUES ('x')", EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    let err = match refused {
        Err(err) => err,
        Ok(_) => panic!("insert was admitted"),
    };
    note_err(&mut env.oracle, "guard_insert_display", &err);
    assert!(matches!(err, ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::Write)));
    assert_eq!(audit_count(&env.watch).await, before);
    env.oracle.fact("guard_refuse_audit_delta", "0");
    let west = identity_for(WEST);
    let west_catalog = catalog(env, WEST, vec![("n4", env.shadow.n4.as_str(), env.oids.int4)], &[]);
    let next = exec_result(env, &mut session, &west_catalog, &west, "SELECT n4 FROM depot.pallet", WEST, 10, BYTES_OK, WALL_OK, false, false).await;
    match next {
        Err(ExecError::RoleReuse) => env.oracle.fact("after_guard_refusal_other_role", "role_reuse"),
        Ok(stream) => {
            let mut stream = stream;
            let outcome = finish_stream(&mut session, &mut stream).await;
            assert!(outcome.error.is_none(), "west query after guard refusal failed");
            env.oracle.fact("after_guard_refusal_other_role", "allowed");
        }
        Err(err) => {
            note_err(&mut env.oracle, "after_guard_refusal_other_role_display", &err);
            panic!("unexpected error after guard refusal");
        }
    }
    session.close();
}

async fn lying_column(env: &mut Env) {
    let mut session = open_session(env).await;
    let catalog = catalog(env, EAST, vec![("hidden", env.shadow.hidden.as_str(), env.oids.text)], &[]);
    let identity = identity_for(EAST);
    let opened = open_stream(env, &mut session, &catalog, &identity, "SELECT hidden FROM depot.pallet", EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    let mut stream = opened.expect("lying stream");
    let err = match stream.next_row().await {
        Err(err) => err,
        Ok(_) => panic!("ungranted column succeeded"),
    };
    drop(stream);
    session.wait_idle().await.expect("lying idle");
    let summary = session.summary().expect("lying summary");
    note_err(&mut env.oracle, "lying_display", &err);
    assert_eq!(err.sqlstate(), Some("42501"));
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.stop(), Some(StopClass::Database));
    assert_ne!(summary.status(), CompletionStatus::Succeeded);
    env.oracle.fact("lying_sqlstate", "42501");
    assert!(!session.is_open());
}

async fn bad_roles(env: &mut Env) {
    create_bad_roles(&env.admin).await;
    let mut session = open_session(env).await;
    for (role, label) in [("oo_r_a184_bypass", "bypass"), ("oo_r_a184_login", "login_flag")] {
        let identity = identity_for(role);
        let catalog = catalog_for_role(env, role);
        let before = audit_count(&env.watch).await;
        let opened = open_stream(env, &mut session, &catalog, &identity, "SELECT n4 FROM depot.pallet", role, 10, BYTES_OK, WALL_OK, false, false).await;
        let mut stream = opened.expect("bad role stream");
        let err = match stream.next_row().await {
            Err(err) => err,
            Ok(_) => panic!("{label} role was accepted"),
        };
        drop(stream);
        session.wait_idle().await.expect("bad role idle");
        note_err(&mut env.oracle, &format!("bad_{label}_display"), &err);
        assert!(matches!(err, ExecError::RoleParameter), "{label}");
        env.oracle.fact(&format!("bad_{label}_audit_delta"), &(audit_count(&env.watch).await - before).to_string());
        assert!(!session.is_open(), "bad role kept the socket");
        session = open_session(env).await;
    }
    let before = audit_count(&env.watch).await;
    let missing = identity_for("oo_r_a184_missing");
    let missing_catalog = catalog_for_role(env, "oo_r_a184_missing");
    let opened = open_stream(env, &mut session, &missing_catalog, &missing, "SELECT n4 FROM depot.pallet", "oo_r_a184_missing", 10, BYTES_OK, WALL_OK, false, false).await;
    let mut stream = opened.expect("missing role stream");
    let err = match stream.next_row().await {
        Err(err) => err,
        Ok(_) => panic!("missing role was accepted"),
    };
    drop(stream);
    session.wait_idle().await.expect("missing idle");
    note_err(&mut env.oracle, "missing_role_display", &err);
    assert!(matches!(err, ExecError::RoleParameter));
    env.oracle.fact("missing_role_audit_delta", &(audit_count(&env.watch).await - before).to_string());
    session.close();
}

async fn omitted_function(env: &mut Env) {
    reset_calls(env, env.pause_oid).await;
    let mut session = open_session(env).await;
    let before = audit_count(&env.watch).await;
    let catalog = catalog(env, EAST, happy_columns(env), &[]);
    let identity = identity_for(EAST);
    let result = exec_result(env, &mut session, &catalog, &identity, "SELECT depot.a184_pause() FROM depot.pallet", EAST, 10, BYTES_OK, WALL_OK, false, false).await;
    let err = match result {
        Err(err) => err,
        Ok(_) => panic!("omitted function was admitted"),
    };
    note_err(&mut env.oracle, "omitted_function_display", &err);
    assert!(matches!(err, ExecError::Guard(inner) if matches!(inner.as_ref(), GuardError::FunctionNotAllowed)));
    assert_eq!(audit_count(&env.watch).await, before);
    let calls = function_calls_flushed(&env.watch, "a184_pause").await;
    env.oracle.fact("omitted_function_calls", &calls.to_string());
    assert_eq!(calls, 0);
    session.close();
}

fn note_err(oracle: &mut Oracle, key: &str, err: &ExecError) {
    let text = err.to_string();
    assert!(!text.contains("SELECT"), "display contained SQL");
    assert!(!text.contains("INSERT"), "display contained SQL");
    assert!(!text.contains(HIDDEN_CELL), "display contained the hidden cell");
    assert!(!text.contains("pt_"), "display contained a physical name");
    oracle.fact(key, &text);
}

#[allow(clippy::too_many_arguments)] // session, catalog, identity, sql, role, three limits, two audit faults
async fn run_ok(
    env: &mut Env,
    session: &mut DedicatedSession,
    catalog: &PolicyCatalog,
    identity: &PolicyIdentity,
    sql: &str,
    role: &str,
    max_rows: u32,
    max_bytes: u64,
    wall_ms: u64,
    fail_admit: bool,
    fail_complete: bool,
) -> Outcome {
    let mut stream = open_stream(env, session, catalog, identity, sql, role, max_rows, max_bytes, wall_ms, fail_admit, fail_complete)
        .await
        .expect("stream");
    finish_stream(session, &mut stream).await
}

#[allow(clippy::too_many_arguments)] // session, catalog, identity, sql, role, three limits, two audit faults
async fn open_stream(
    env: &mut Env,
    session: &mut DedicatedSession,
    catalog: &PolicyCatalog,
    identity: &PolicyIdentity,
    sql: &str,
    role: &str,
    max_rows: u32,
    max_bytes: u64,
    wall_ms: u64,
    fail_admit: bool,
    fail_complete: bool,
) -> Result<SelectStream, ExecError> {
    let request = SelectRequest {
        identity,
        catalog,
        original_sql: sql,
        role,
        max_rows,
        max_result_bytes: max_bytes,
        wall_clock: Duration::from_millis(wall_ms),
    };
    let audit = LiveAudit {
        client: connect_db(DATABASE, ADMIN, &env.admin_pass, &env.ca).await,
        fail_admit,
        fail_complete,
    };
    session.execute_trusted_select(request, audit).await
}

#[allow(clippy::too_many_arguments)] // session, catalog, identity, sql, role, three limits, two audit faults
async fn exec_result(
    env: &mut Env,
    session: &mut DedicatedSession,
    catalog: &PolicyCatalog,
    identity: &PolicyIdentity,
    sql: &str,
    role: &str,
    max_rows: u32,
    max_bytes: u64,
    wall_ms: u64,
    fail_admit: bool,
    fail_complete: bool,
) -> Result<SelectStream, ExecError> {
    open_stream(env, session, catalog, identity, sql, role, max_rows, max_bytes, wall_ms, fail_admit, fail_complete).await
}

async fn finish_stream(session: &mut DedicatedSession, stream: &mut SelectStream) -> Outcome {
    let mut rows = Vec::new();
    let error = loop {
        match stream.next_row().await {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => break None,
            Err(err) => break Some(err),
        }
    };
    session.wait_idle().await.expect("idle");
    let summary = session.summary().expect("summary");
    Outcome { rows, error, summary }
}

fn rendered(rows: &[Vec<Cell>]) -> Vec<String> {
    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| row.iter().map(cell_text).collect::<Vec<_>>().join("\t"))
        .collect();
    lines.sort();
    lines
}

fn cell_text(cell: &Cell) -> String {
    match cell {
        Cell::Null => "NULL".to_string(),
        Cell::Bool(true) => "true".to_string(),
        Cell::Bool(false) => "false".to_string(),
        Cell::Int(text) | Cell::Text(text) => text.clone(),
    }
}

fn identity_for(role: &str) -> PolicyIdentity {
    PolicyIdentity {
        project: SOURCE.to_string(),
        role: role.to_string(),
        policy_id: CONFIG.to_string(),
    }
}

fn happy_columns(env: &Env) -> Vec<(&str, &str, u32)> {
    vec![
        ("n2", env.shadow.n2.as_str(), env.oids.int2),
        ("n4", env.shadow.n4.as_str(), env.oids.int4),
        ("n8", env.shadow.n8.as_str(), env.oids.int8),
        ("tag", env.shadow.tag.as_str(), env.oids.text),
        ("lit", env.shadow.lit.as_str(), env.oids.bool),
        ("note", env.shadow.note.as_str(), env.oids.text),
    ]
}

fn happy_catalog(env: &Env) -> PolicyCatalog {
    catalog(env, EAST, happy_columns(env), &[])
}

fn catalog_for_role(env: &Env, role: &str) -> PolicyCatalog {
    catalog(env, role, vec![("n4", env.shadow.n4.as_str(), env.oids.int4)], &[])
}

fn catalog(env: &Env, role: &str, columns: Vec<(&str, &str, u32)>, functions: &[&str]) -> PolicyCatalog {
    let mut resolved = Vec::new();
    for name in functions {
        let (oid, fname) = match *name {
            "touch" => (env.touch_oid, "a184_touch"),
            "pause" => (env.pause_oid, "a184_pause"),
            "who" => (env.who_oid, "a184_who"),
            _ => panic!("unknown function label"),
        };
        resolved.push(ResolvedFunction {
            oid,
            schema: "depot".to_string(),
            name: fname.to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: false,
        });
    }
    PolicyCatalog {
        identity: identity_for(role),
        relations: vec![VisibleRelation {
            schema: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            physical_schema: "_oo".to_string(),
            physical_name: env.shadow.table.clone(),
            columns: columns
                .into_iter()
                .map(|(name, physical, oid)| VisibleColumn {
                    name: name.to_string(),
                    physical_name: physical.to_string(),
                    type_oid: oid,
                })
                .collect(),
            pending_unclassified: 2,
            estimated_rows: 4,
        }],
        functions: resolved,
        operators: Vec::new(),
        literal_types: LiteralTypes {
            integer: Some(env.oids.int4),
            text: Some(env.oids.text),
            boolean: Some(env.oids.bool),
        },
        null_test_allowed: true,
        large_table_row_threshold: 1_000_000,
    }
}

async fn admin_texts(env: &Env) -> Vec<String> {
    let sql = format!(
        "SELECT COALESCE(\"{n2}\"::text, 'NULL'), COALESCE(\"{n4}\"::text, 'NULL'), \
         COALESCE(\"{n8}\"::text, 'NULL'), COALESCE(\"{tag}\", 'NULL'), \
         COALESCE(\"{lit}\"::text, 'NULL'), COALESCE(\"{note}\", 'NULL') \
         FROM \"_oo\".\"{table}\"",
        n2 = env.shadow.n2,
        n4 = env.shadow.n4,
        n8 = env.shadow.n8,
        tag = env.shadow.tag,
        lit = env.shadow.lit,
        note = env.shadow.note,
        table = env.shadow.table
    );
    let rows = env.admin.query(&sql, &[]).await.expect("admin texts");
    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| {
            (0..6)
                .map(|index| row.get::<_, String>(index))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect();
    lines.sort();
    assert_eq!(lines.len(), 4);
    lines
}

async fn open_session(env: &Env) -> DedicatedSession {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: DATABASE.to_string(),
        user: LOGIN.to_string(),
    };
    let (client, connection) = connect_require_tls(&endpoint, &env.login_pass, &env.ca)
        .await
        .expect("session connect");
    let tls = rustls_connector(&env.ca).expect("connector");
    DedicatedSession::bind(client, connection, tls).await.expect("bind")
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::from("connect");
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(_) => {
                last = "connect".to_string();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect failed: {last}");
}

fn read_secret(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    let value = raw.trim();
    assert!(
        (16..=128).contains(&value.len())
            && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "secret file was not lowercase hex"
    );
    value.to_string()
}

fn sqlstate_only(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.code().code().to_string())
        .unwrap_or_else(|| "driver".to_string())
}

async fn lock_table(client: &Client, table: &str) {
    assert!(table.starts_with("pt_") && table.len() == 63, "lock target");
    let sql = format!("BEGIN; LOCK TABLE \"_oo\".\"{table}\" IN ACCESS EXCLUSIVE MODE");
    client.batch_execute(&sql).await.expect("lock table");
}

async fn unlock_table(client: &Client) {
    client.batch_execute("ROLLBACK").await.expect("unlock");
}

async fn wait_physical(client: &Client, pid: i32, table: &str) -> bool {
    for _ in 0..100 {
        if activity_has_table(client, pid, table).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

async fn activity_has_table(client: &Client, pid: i32, table: &str) -> bool {
    client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity WHERE pid = $1 AND position($2 in query) > 0)",
            &[&pid, &table],
        )
        .await
        .expect("activity")
        .get(0)
}

async fn activity_flags(client: &Client, pid: i32, table: &str, secret: &str) -> (bool, bool, bool) {
    let query: String = client
        .query_one(
            "SELECT COALESCE(query, '') FROM pg_catalog.pg_stat_activity WHERE pid = $1",
            &[&pid],
        )
        .await
        .expect("activity text")
        .get(0);
    let physical = query.contains(table);
    let logical_absent = !query.contains("depot.pallet") && !query.contains("\"depot\".\"pallet\"");
    let secret_absent = !query.contains(secret);
    (physical, logical_absent, secret_absent)
}

async fn xact_cleared(client: &Client, pid: i32) -> bool {
    client
        .query_one(
            "SELECT state = 'idle' AND xact_start IS NULL FROM pg_catalog.pg_stat_activity WHERE pid = $1",
            &[&pid],
        )
        .await
        .expect("xact")
        .get(0)
}

async fn xact_open(client: &Client, pid: i32) -> bool {
    client
        .query_one(
            "SELECT xact_start IS NOT NULL FROM pg_catalog.pg_stat_activity WHERE pid = $1",
            &[&pid],
        )
        .await
        .expect("xact open")
        .get(0)
}

async fn backend_still(env: &Env, pid: i32, start: &str) -> bool {
    match observe_backend(&env.watch, pid).await {
        Ok(Some(view)) => view.pid() == pid && view.backend_start() == start && view.state() == "active",
        _ => false,
    }
}

async fn function_calls(client: &Client, name: &str) -> i64 {
    client
        .batch_execute("SELECT pg_catalog.pg_stat_clear_snapshot()")
        .await
        .expect("clear stat snapshot");
    client
        .query_one(
            "SELECT COALESCE((SELECT calls FROM pg_catalog.pg_stat_user_functions WHERE schemaname = 'depot' AND funcname = $1), 0)::int8",
            &[&name],
        )
        .await
        .expect("calls")
        .get(0)
}

async fn function_calls_flushed(client: &Client, name: &str) -> i64 {
    function_calls(client, name).await
}

async fn calls_until(env: &Env, name: &str, want: i64) -> i64 {
    let mut seen = function_calls_flushed(&env.watch, name).await;
    for _ in 0..40 {
        if seen == want {
            return seen;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        seen = function_calls_flushed(&env.watch, name).await;
    }
    seen
}

async fn calls_after_disconnect(env: &Env, name: &str) -> i64 {
    let mut seen = function_calls(&env.watch, name).await;
    for _ in 0..40 {
        if seen != 0 {
            return seen;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        seen = function_calls(&env.watch, name).await;
    }
    seen
}

async fn reset_calls(env: &Env, oid: u32) {
    let oid = i32::try_from(oid).expect("oid");
    env.admin
        .batch_execute(&format!("SELECT pg_catalog.pg_stat_reset_single_function_counters(({oid})::oid)"))
        .await
        .expect("reset calls");
}

async fn audit_count(client: &Client) -> i64 {
    client
        .query_one("SELECT count(*)::int8 FROM depot.a184_audit", &[])
        .await
        .expect("audit count")
        .get(0)
}

async fn phase_count(client: &Client, phase: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM depot.a184_audit WHERE phase = $1",
            &[&phase],
        )
        .await
        .expect("phase count")
        .get(0)
}

async fn latest_audit(client: &Client, phase: &str) -> (String, String, String, i32, String, String) {
    let row = client
        .query_one(
            "SELECT original_sha256, executed_sha256, role_name, backend_pid, backend_start, \
             COALESCE(completion_status, '') \
             FROM depot.a184_audit WHERE phase = $1 ORDER BY id DESC LIMIT 1",
            &[&phase],
        )
        .await
        .expect("latest audit");
    (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4), row.get(5))
}

async fn assert_role_flags(client: &Client, role: &str) {
    let rows = client
        .query(
            "SELECT rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolbypassrls, rolreplication \
             FROM pg_catalog.pg_roles WHERE rolname = $1",
            &[&role],
        )
        .await
        .expect("role flags");
    assert_eq!(rows.len(), 1);
    for index in 0..7 {
        let flag: bool = rows[0].get(index);
        assert!(!flag, "role flag {index}");
    }
}

async fn install_roles(client: &mut Client) {
    let east = ["n2", "n4", "n8", "tag", "lit", "note", "qty"];
    let installed = install_column_acl(client, &policy(EAST_SLUG, east.into_iter().map(column_spec).collect()))
        .await
        .expect("install east");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, EAST);
    let installed = install_column_acl(client, &policy(WEST_SLUG, vec![column_spec("n4")]))
        .await
        .expect("install west");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, WEST);
}

fn column_spec(name: &str) -> ColumnSpec {
    ColumnSpec {
        name: name.to_string(),
        status: ClassStatus::Confirmed,
        semantic_type: "identifier".to_string(),
        pii: PiiClass::None,
    }
}

fn policy(role: &str, columns: Vec<ColumnSpec>) -> ColumnAclPolicy {
    ColumnAclPolicy {
        project: SOURCE.to_string(),
        config_digest: CONFIG.to_string(),
        generation: CURRENT_GENERATION,
        role: role.to_string(),
        login: LOGIN.to_string(),
        table: LogicalTable {
            namespace: NAMESPACE.to_string(),
            name: TABLE.to_string(),
            columns,
        },
        row_filter: None,
    }
}

async fn install_functions(client: &Client) -> (u32, u32, u32) {
    client
        .batch_execute(
            "CREATE SCHEMA depot;\n\
             REVOKE ALL ON SCHEMA depot FROM PUBLIC;\n\
             CREATE FUNCTION depot.a184_touch() RETURNS text\n\
             LANGUAGE plpgsql VOLATILE AS $$ BEGIN RETURN 't'; END $$;\n\
             CREATE FUNCTION depot.a184_pause() RETURNS text\n\
             LANGUAGE plpgsql VOLATILE AS $$ BEGIN PERFORM pg_sleep(2); RETURN 'p'; END $$;\n\
             CREATE FUNCTION depot.a184_who() RETURNS text\n\
             LANGUAGE plpgsql VOLATILE AS $$ BEGIN RETURN current_user || '/' || session_user; END $$;\n\
             REVOKE ALL ON FUNCTION depot.a184_touch() FROM PUBLIC;\n\
             REVOKE ALL ON FUNCTION depot.a184_pause() FROM PUBLIC;\n\
             REVOKE ALL ON FUNCTION depot.a184_who() FROM PUBLIC",
        )
        .await
        .expect("functions");
    let touch = function_oid(client, "a184_touch").await;
    let pause = function_oid(client, "a184_pause").await;
    let who = function_oid(client, "a184_who").await;
    (touch, pause, who)
}

async fn function_oid(client: &Client, name: &str) -> u32 {
    let oid: i32 = client
        .query_one(
            "SELECT p.oid::int4 FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'depot' AND p.proname = $1 AND p.pronargs = 0",
            &[&name],
        )
        .await
        .expect("function oid")
        .get(0);
    u32::try_from(oid).expect("function oid range")
}

async fn grant_functions(client: &Client, shadow: &Shadow) {
    client
        .batch_execute(
            "GRANT USAGE ON SCHEMA depot TO oo_r_a184_east;\n\
             GRANT EXECUTE ON FUNCTION depot.a184_touch() TO oo_r_a184_east;\n\
             GRANT EXECUTE ON FUNCTION depot.a184_pause() TO oo_r_a184_east;\n\
             GRANT EXECUTE ON FUNCTION depot.a184_who() TO oo_r_a184_east",
        )
        .await
        .expect("grant functions");
    let alter = format!(
        "ALTER TABLE \"_oo\".\"{}\" ADD COLUMN a184_code varchar(8);\n\
         GRANT SELECT (a184_code) ON \"_oo\".\"{}\" TO oo_r_a184_east",
        shadow.table, shadow.table
    );
    client.batch_execute(&alter).await.expect("varchar column");
}

async fn install_audit(client: &Client) {
    client
        .batch_execute(
            "CREATE TABLE depot.a184_audit (\
                id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\
                phase text NOT NULL,\
                original_sha256 text NOT NULL,\
                executed_sha256 text NOT NULL,\
                role_name text NOT NULL,\
                backend_pid integer NOT NULL,\
                backend_start text NOT NULL,\
                completion_status text,\
                admitted_at timestamptz NOT NULL DEFAULT clock_timestamp())",
        )
        .await
        .expect("audit table");
}

async fn create_bad_roles(client: &Client) {
    client
        .batch_execute(
            "CREATE ROLE oo_r_a184_bypass NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT BYPASSRLS NOREPLICATION;\n\
             CREATE ROLE oo_r_a184_login LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION",
        )
        .await
        .expect("bad roles");
}

async fn load_oids(client: &Client, shadow: &Shadow) -> Oids {
    let oids = Oids {
        int2: type_oid(client, "int2").await,
        int4: type_oid(client, "int4").await,
        int8: type_oid(client, "int8").await,
        text: type_oid(client, "text").await,
        bool: type_oid(client, "bool").await,
        numeric: type_oid(client, "numeric").await,
        varchar: type_oid(client, "varchar").await,
    };
    assert_eq!(column_type(client, &shadow.table, &shadow.n2).await, oids.int2);
    assert_eq!(column_type(client, &shadow.table, &shadow.n4).await, oids.int4);
    assert_eq!(column_type(client, &shadow.table, &shadow.n8).await, oids.int8);
    assert_eq!(column_type(client, &shadow.table, &shadow.tag).await, oids.text);
    assert_eq!(column_type(client, &shadow.table, &shadow.lit).await, oids.bool);
    assert_eq!(column_type(client, &shadow.table, &shadow.note).await, oids.text);
    assert_eq!(column_type(client, &shadow.table, &shadow.qty).await, oids.numeric);
    assert_eq!(oids.varchar, 1043);
    assert_eq!(oids.numeric, 1700);
    oids
}

async fn type_oid(client: &Client, name: &str) -> u32 {
    let oid: i32 = client
        .query_one(
            "SELECT oid::int4 FROM pg_catalog.pg_type \
             WHERE typname = $1 AND typnamespace = 'pg_catalog'::regnamespace",
            &[&name],
        )
        .await
        .expect("type oid")
        .get(0);
    u32::try_from(oid).expect("type oid range")
}

async fn column_type(client: &Client, table: &str, column: &str) -> u32 {
    let rel = format!("_oo.{table}");
    let oid: i32 = client
        .query_one(
            "SELECT atttypid::int4 FROM pg_catalog.pg_attribute \
             WHERE attrelid = ($1::text)::regclass AND attname = $2 AND NOT attisdropped",
            &[&rel, &column],
        )
        .await
        .expect("column type")
        .get(0);
    u32::try_from(oid).expect("column type range")
}

async fn reset_database(client: &Client) {
    exec_ok(
        client,
        "terminate",
        &format!("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"),
    )
    .await;
    exec_ok(client, "drop database", &format!("DROP DATABASE IF EXISTS \"{DATABASE}\"")).await;
    exec_ok(
        client,
        "drop roles",
        "DO $$\n\
         DECLARE role_name text;\n\
         BEGIN\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a184_login') THEN\n\
             DROP ROLE oo_a184_login;\n\
           END IF;\n\
           FOR role_name IN SELECT rolname FROM pg_catalog.pg_roles WHERE rolname LIKE 'oo\\_r\\_a184\\_%' ESCAPE '\\' LOOP\n\
             EXECUTE format('DROP ROLE %I', role_name);\n\
           END LOOP;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN\n\
             DROP ROLE oo_pipeline_runtime;\n\
           END IF;\n\
           IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') THEN\n\
             DROP ROLE oo_target_owner;\n\
           END IF;\n\
         END\n\
         $$",
    )
    .await;
    exec_ok(
        client,
        "create database",
        &format!("CREATE DATABASE \"{DATABASE}\" TEMPLATE template0 ENCODING 'UTF8'"),
    )
    .await;
}

async fn apply_frozen_schema(client: &mut Client) {
    let bootstrap = migration("0000_bootstrap_roles.sql");
    let fence = migration("0001_target_fence_expand.sql");
    let preflight = migration("0003_target_segment_preflight_expand.sql");
    let mapping = migration("0004_physical_mapping.sql");
    let full_import = migration("0005_full_import_expand.sql");
    let tx = begin_locked(client).await;
    exec_tx(&tx, "0000", &bootstrap.0).await;
    exec_tx(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0001", &fence.0).await;
    exec_tx(&tx, "row-lock", "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_tx(&tx, "0003", &preflight.0).await;
    exec_tx(&tx, "0004", &mapping.0).await;
    exec_tx(
        &tx,
        "markers",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            ("target-fence-receipts/3", &preflight.1, "0003_target_segment_preflight_expand.sql"),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");
    let tx = begin_locked(client).await;
    exec_tx(&tx, "row-lock-import", "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_tx(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_tx(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_tx(&tx, "0005", &full_import.0).await;
    exec_tx(
        &tx,
        "marker-import",
        &marker_sql(&[("full-import/v1", &full_import.1, "0005_full_import_expand.sql")]),
    )
    .await;
    tx.commit().await.expect("commit full import");
}

fn migration(name: &str) -> (String, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../oo-pipeline/migrations")
        .join(name);
    let bytes = fs::read(&path).unwrap_or_else(|_| panic!("read {name}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} is not utf-8"));
    (text, sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn marker_sql(rows: &[(&str, &str, &str)]) -> String {
    let mut values = String::new();
    for (index, (version, checksum, filename)) in rows.iter().enumerate() {
        if index > 0 {
            values.push_str(",\n");
        }
        values.push_str(&format!("  ('{version}', '{checksum}', 'expand', '{filename}')"));
    }
    format!("INSERT INTO _oo.schema_migrations (version, checksum, phase, filename) VALUES\n{values}")
}

async fn begin_locked(client: &mut Client) -> tokio_postgres::Transaction<'_> {
    let tx = client.transaction().await.expect("begin");
    exec_tx(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_tx(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_tx(&tx, "advisory-lock", "SELECT pg_catalog.pg_advisory_xact_lock(64042026184)").await;
    tx
}

async fn exec_ok(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", sqlstate_only(&err)));
}

async fn exec_tx(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", sqlstate_only(&err)));
}

async fn create_login(client: &Client, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let sql = format!(
        "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
         NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
         GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
    );
    match client.batch_execute(&sql).await {
        Ok(()) => {}
        Err(err) => panic!("create login failed: {}", sqlstate_only(&err)),
    }
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one("SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)", &[&SHARD, &OWNER])
        .await
        .expect("adopt")
        .get(0);
    client
        .execute(
            "SELECT _oo.full_import_bind_session($1, ($2::text)::uuid, $3)",
            &[&SHARD, &OWNER, &target],
        )
        .await
        .expect("bind");
    let columns = ["n2", "n4", "n8", "tag", "lit", "note", "qty", "hidden"];
    for generation in [STALE_GENERATION, CURRENT_GENERATION] {
        register(client, generation, "namespace", "n", NAMESPACE, "", "").await;
        register(client, generation, "table", "t", NAMESPACE, TABLE, "").await;
        for column in columns {
            register(client, generation, "column", "c", NAMESPACE, TABLE, column).await;
        }
    }
    let shadow = mapped_shadow(client).await;
    let schemas = vec![NAMESPACE.to_string()];
    let tables = vec![TABLE.to_string()];
    let physical = vec![shadow.table.clone()];
    let open_params: [&(dyn ToSql + Sync); 12] = [
        &SHARD,
        &OWNER,
        &target,
        &SOURCE,
        &CURRENT_GENERATION,
        &CONFIG,
        &TYPE_DIGEST,
        &INPUT_DIGEST,
        &SCHEMA_DIGEST,
        &schemas,
        &tables,
        &physical,
    ];
    let run_id: String = client
        .query_one(
            "SELECT _oo.full_import_open_run($1, ($2::text)::uuid, $3, $4, $5, $6, $7, $8, $9, $10::text[], $11::text[], $12::text[])::text",
            &open_params,
        )
        .await
        .expect("open run")
        .get(0);
    let idents: Vec<String> = columns.iter().map(|name| (*name).to_string()).collect();
    let oids = vec![21_i64, 23, 20, 25, 16, 25, 1700, 25];
    let typmods = vec![-1_i32; 8];
    let required = vec![false, true, true, true, true, false, true, true];
    let index = 0_i32;
    let prepare_params: [&(dyn ToSql + Sync); 14] = [
        &SHARD,
        &OWNER,
        &target,
        &run_id,
        &index,
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &NAMESPACE,
        &TABLE,
        &idents,
        &oids,
        &typmods,
        &required,
    ];
    let prepared: String = client
        .query_one(
            "SELECT _oo.full_import_prepare_shadow($1, ($2::text)::uuid, $3, ($4::text)::uuid, $5, $6, $7, $8, $9, $10, $11::text[], $12::bigint[], $13::integer[], $14::boolean[])",
            &prepare_params,
        )
        .await
        .expect("prepare shadow")
        .get(0);
    assert_eq!(prepared, shadow.table);
    insert_rows(client, &shadow).await;
    shadow
}

async fn mapped_shadow(client: &Client) -> Shadow {
    Shadow {
        table: lookup(client, "table", NAMESPACE, TABLE, "").await,
        n2: lookup(client, "column", NAMESPACE, TABLE, "n2").await,
        n4: lookup(client, "column", NAMESPACE, TABLE, "n4").await,
        n8: lookup(client, "column", NAMESPACE, TABLE, "n8").await,
        tag: lookup(client, "column", NAMESPACE, TABLE, "tag").await,
        lit: lookup(client, "column", NAMESPACE, TABLE, "lit").await,
        note: lookup(client, "column", NAMESPACE, TABLE, "note").await,
        qty: lookup(client, "column", NAMESPACE, TABLE, "qty").await,
        hidden: lookup(client, "column", NAMESPACE, TABLE, "hidden").await,
    }
}

struct FixtureRow {
    n2: Option<i16>,
    n4: i32,
    n8: i64,
    tag: &'static str,
    lit: bool,
    note: Option<&'static str>,
    qty: &'static str,
}

async fn insert_rows(client: &mut Client, shadow: &Shadow) {
    let sql = format!(
        "INSERT INTO \"_oo\".\"{}\" (\"_oo_pos\", \"{}\", \"{}\", \"{}\", \"{}\", \"{}\", \"{}\", \"{}\", \"{}\") \
         VALUES (($1::text)::numeric, $2, $3, $4, $5, $6, $7, ($8::text)::numeric, $9)",
        shadow.table, shadow.n2, shadow.n4, shadow.n8, shadow.tag, shadow.lit, shadow.note, shadow.qty, shadow.hidden
    );
    let tx = client.transaction().await.expect("insert begin");
    let rows = [
        FixtureRow { n2: Some(i16::MIN), n4: i32::MIN, n8: i64::MIN, tag: "", lit: false, note: None, qty: "1.25" },
        FixtureRow { n2: None, n4: 0, n8: 0, tag: "托盘", lit: true, note: Some("line\n2"), qty: "2.50" },
        FixtureRow { n2: Some(7), n4: 11, n8: 13, tag: "east-row", lit: false, note: Some("keep"), qty: "3.75" },
        FixtureRow { n2: Some(i16::MAX), n4: i32::MAX, n8: i64::MAX, tag: "ZZ", lit: true, note: None, qty: "4.00" },
    ];
    for (index, row) in rows.iter().enumerate() {
        let pos = (index + 1).to_string();
        let note = row.note.map(str::to_string);
        let qty = row.qty.to_string();
        let hidden = HIDDEN_CELL.to_string();
        let tag = row.tag.to_string();
        let params: [&(dyn ToSql + Sync); 9] = [
            &pos, &row.n2, &row.n4, &row.n8, &tag, &row.lit, &note, &qty, &hidden,
        ];
        tx.execute(&sql, &params).await.expect("insert shadow row");
    }
    tx.commit().await.expect("insert commit");
}

async fn register(client: &Client, generation: i64, level: &str, letter: &str, namespace: &str, table: &str, column: &str) {
    let payload: [&(dyn ToSql + Sync); 7] = [&SOURCE, &CONFIG, &generation, &letter, &namespace, &table, &column];
    let digest: Vec<u8> = client
        .query_one("SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))", &payload)
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [&SOURCE, &CONFIG, &generation, &level, &namespace, &table, &column, &digest];
    client
        .execute("SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)", &bind)
        .await
        .expect("register");
}

async fn lookup(client: &Client, level: &str, namespace: &str, table: &str, column: &str) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [&SOURCE, &CONFIG, &CURRENT_GENERATION, &level, &namespace, &table, &column];
    client
        .query_one("SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)", &params)
        .await
        .expect("lookup")
        .get(0)
}
