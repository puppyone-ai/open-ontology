//! Live permission-bound SELECT executor.
//!
//! Runs only when `OO_A189_LIVE=1`. Without that variable the file compiles
//! and returns. It does not claim the parent gateway work items.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use oo_gateway::query_executor::{
    observe_backend, AuditAdmission, CancelOutcome, Cell, CompletionRecord, CompletionStatus,
    DedicatedSession, ExecError, QuerySummary, SelectRequest, SelectStream, StopClass,
    UntrustedAdmission, UNTRUSTED_ADMISSION,
};
use oo_gateway::{
    connect_require_tls, guard_select, install_column_acl, rustls_connector, ClassStatus,
    ColumnAclPolicy, ColumnSpec, GuardError, InstallEffect, LiteralTypes, LogicalTable, PiiClass,
    PolicyCatalog, PolicyIdentity, ResolvedFunction, ResolvedOperator, TlsEndpoint, VisibleColumn,
    VisibleRelation,
};
use sha2::{Digest, Sha256};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

const HOST: &str = "oo-a189-pg";
const DATABASE: &str = "oo_a189";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a189_login";
const ROLE_A: &str = "oo_r_reader_a";
const ROLE_B: &str = "oo_r_reader_b";
const ROLE_FLAG: &str = "oo_r_flag_probe";
const SECRET_DIR: &str = "/run/oo-a189/fixture";
const ORACLE_PATH: &str = "/evidence/live/oracle.txt";
const OWNER: &str = "a1800000-0000-4000-8000-000000000180";
const SHARD: &str = "shard_a180";
const SOURCE: &str = "orders";
const CONFIG: &str = "a180a180a180a180a180a180a180a180a180a180a180a180a180a180a180a180";
const TYPE_DIGEST: &str = "b180b180b180b180b180b180b180b180b180b180b180b180b180b180b180b180";
const INPUT_DIGEST: &str = "c180c180c180c180c180c180c180c180c180c180c180c180c180c180c180c180";
const SCHEMA_DIGEST: &str = "d180d180d180d180d180d180d180d180d180d180d180d180d180d180d180d180";
const NAMESPACE: &str = "logistics";
const TABLE: &str = "shipment";
const STALE_GENERATION: i64 = 3;
const CURRENT_GENERATION: i64 = 7;
const WALL_OK: u64 = 30_000;
const BYTES_OK: u64 = 256 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_permission_bound_select() {
    if std::env::var("OO_A189_LIVE").ok().as_deref() != Some("1") {
        eprintln!("OO_A189_LIVE is unset; the executor comparison was not run");
        return;
    }
    run_live().await;
}

struct Shadow {
    table: String,
    i2: String,
    i4: String,
    i8: String,
    label: String,
    flag: String,
    note: String,
    amount: String,
    secret: String,
}

struct Oracle {
    lines: Vec<String>,
    secrets: Vec<String>,
}

impl Oracle {
    fn new(admin_pass: &str, login_pass: &str) -> Self {
        Self {
            lines: Vec::new(),
            secrets: vec![admin_pass.to_string(), login_pass.to_string()],
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        for secret in &self.secrets {
            assert!(!value.contains(secret), "oracle value contained a password");
        }
        self.lines.push(format!("{key}={value}"));
        self.flush();
    }

    fn flush(&self) {
        let path = Path::new(ORACLE_PATH);
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
        let original = admission.original_sha256().to_string();
        let executed = admission.executed_sha256().to_string();
        let fingerprint = admission.fingerprint_hex().to_string();
        let role = admission.role().to_string();
        let project = admission.project().to_string();
        let policy_id = admission.policy_id().to_string();
        let pid = admission.backend_pid();
        let start = admission.backend_start().to_string();
        self.client
            .execute(
                "INSERT INTO fixture.audit_log \
                 (original_sha256, executed_sha256, fingerprint_hex, role_name, project, policy_id, backend_pid, backend_start) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                &[
                    &original,
                    &executed,
                    &fingerprint,
                    &role,
                    &project,
                    &policy_id,
                    &pid,
                    &start,
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
        let _ = completion.status();
        Ok(())
    }
}

struct Env {
    oracle: Oracle,
    admin: Client,
    admin_pass: String,
    login_pass: String,
    ca: Vec<u8>,
    shadow: Shadow,
    note_oid: u32,
    hold_oid: u32,
    oids: Oids,
    eq_int: (u32, u32),
}

struct Oids {
    int2: u32,
    int4: u32,
    int8: u32,
    text: u32,
    bool: u32,
    numeric: u32,
}

async fn run_live() {
    let admin_pass = read_hex("admin.pass");
    let login_pass = read_hex("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca.crt");
    let mut oracle = Oracle::new(&admin_pass, &login_pass);
    oracle.fact("live", "ran");
    assert_eq!(UNTRUSTED_ADMISSION, UntrustedAdmission::Blocked);
    oracle.fact("untrusted_admission", "blocked");

    let maintenance = connect_db("postgres", ADMIN, &admin_pass, &ca).await;
    reset_database(&maintenance).await;
    drop(maintenance);

    let mut admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    apply_frozen_schema(&mut admin).await;
    create_login(&admin, &login_pass).await;
    let shadow = prepare_shadow(&mut admin).await;
    install_roles(&mut admin).await;
    admin
        .batch_execute(&format!(
            "ALTER TABLE \"_oo\".{} ADD COLUMN extra text",
            quote_physical(&shadow.table, "pt_")
        ))
        .await
        .expect("add extra");
    let note_oid = install_note_function(&admin).await;
    let hold_oid = install_page_hold(&admin).await;
    install_audit(&admin).await;
    let oids = load_oids(&admin, &shadow).await;
    let eq_int = operator_oid(&admin, oids.int4, oids.int4).await;
    let track: String = admin
        .query_one(
            "SELECT setting FROM pg_catalog.pg_settings WHERE name = 'track_functions'",
            &[],
        )
        .await
        .expect("track_functions")
        .get(0);
    assert_eq!(track, "pl");
    oracle.fact("track_functions", &track);
    oracle.fact("physical_table", &shadow.table);
    oracle.fact("note_oid", &note_oid.to_string());
    assert_role_flags(&admin, ROLE_A).await;
    assert_role_flags(&admin, ROLE_B).await;
    oracle.fact("role_flags", "nologin");
    let secret_grants: i64 = privilege_count(&admin, ROLE_A, &shadow.secret).await;
    let extra_grants: i64 = privilege_count(&admin, ROLE_A, "extra").await;
    let role_b_i2: i64 = privilege_count(&admin, ROLE_B, &shadow.i2).await;
    assert_eq!(secret_grants, 0);
    assert_eq!(extra_grants, 0);
    assert_eq!(role_b_i2, 0);
    oracle.fact("hidden_grants", "0");

    let mut env = Env {
        oracle,
        admin,
        admin_pass,
        login_pass,
        ca,
        shadow,
        note_oid,
        hold_oid,
        oids,
        eq_int,
    };
    direct_privileges(&mut env).await;
    happy_reuse_and_bounds(&mut env).await;
    type_lying_drop_and_audit(&mut env).await;
    deadline_cancel_and_role_b(&mut env).await;
    prove_role_timing(&mut env).await;
    prove_blocked_shutdown(&mut env).await;
    env.oracle.fact("live_result", "compared");
}

async fn direct_privileges(env: &mut Env) {
    let reader = connect_db(DATABASE, LOGIN, &env.login_pass, &env.ca).await;
    reader
        .batch_execute("SET search_path TO ''")
        .await
        .expect("direct search_path");
    reader
        .batch_execute(&format!("SET ROLE {ROLE_A}"))
        .await
        .expect("direct role a");
    let secret_sql = format!(
        "SELECT {} FROM \"_oo\".{}",
        quote_physical(&env.shadow.secret, "pc_"),
        quote_physical(&env.shadow.table, "pt_")
    );
    let extra_sql = format!(
        "SELECT extra FROM \"_oo\".{}",
        quote_physical(&env.shadow.table, "pt_")
    );
    assert_eq!(sql_outcome(&reader, &secret_sql).await, "42501");
    assert_eq!(sql_outcome(&reader, &extra_sql).await, "42501");
    reader.batch_execute("RESET ROLE").await.expect("reset role");
    reader
        .batch_execute(&format!("SET ROLE {ROLE_B}"))
        .await
        .expect("direct role b");
    let i2_sql = format!(
        "SELECT {} FROM \"_oo\".{}",
        quote_physical(&env.shadow.i2, "pc_"),
        quote_physical(&env.shadow.table, "pt_")
    );
    let i4_sql = format!(
        "SELECT {} FROM \"_oo\".{}",
        quote_physical(&env.shadow.i4, "pc_"),
        quote_physical(&env.shadow.table, "pt_")
    );
    assert_eq!(sql_outcome(&reader, &i2_sql).await, "42501");
    assert_eq!(sql_outcome(&reader, &i4_sql).await, "ok:4");
    drop(reader);
    env.oracle.fact("direct_secret", "42501");
    env.oracle.fact("direct_extra", "42501");
    env.oracle.fact("direct_role_b_hidden", "42501");
    env.oracle.fact("direct_role_b_visible", "ok:4");
}

async fn happy_reuse_and_bounds(env: &mut Env) {
    let happy = happy_catalog(env);
    let identity = identity_for(ROLE_A);
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let start = session.backend_start();
    env.oracle.fact("bind_pid", &pid.to_string());
    env.oracle.fact("bind_start_len", &start.len().to_string());
    assert!(!start.is_empty());
    prove_star(env, &mut session, &identity, &happy, pid, &start).await;
    prove_row_bound(env, &identity, &happy).await;
    prove_byte_caps(env, &mut session, &identity, &happy).await;
    prove_role_reuse(env, &mut session).await;
    prove_guard_refusals(env, &mut session, &identity, &happy).await;
}

async fn prove_star(
    env: &mut Env,
    session: &mut DedicatedSession,
    identity: &PolicyIdentity,
    happy: &PolicyCatalog,
    pid: i32,
    start: &str,
) {
    let star_sql = "SELECT * FROM logistics.shipment ORDER BY i4";
    let star_plan = guard_select(identity, happy, star_sql).expect("guard star");
    assert!(star_plan.executed_sql.contains(&env.shadow.i2));
    assert!(star_plan.executed_sql.contains(&env.shadow.note));
    assert!(!star_plan.executed_sql.contains(&env.shadow.secret));
    assert!(!star_plan.executed_sql.contains(&env.shadow.amount));
    assert!(!star_plan.executed_sql.contains("extra"));
    let before_audit = audit_count(&env.admin).await;
    let star_stream = session
        .execute_trusted_select(
            req(identity, happy, star_sql, ROLE_A, 10, BYTES_OK, WALL_OK),
            open_audit(env, false, false).await,
        )
        .await
        .expect("star execute");
    let (rows, summary) = finish_ok(session, star_stream).await;
    assert_eq!(summary.status(), CompletionStatus::Succeeded);
    assert!(!summary.truncated());
    assert!(summary.rollback_confirmed());
    assert!(!summary.socket_dropped());
    assert!(summary.end_audit_ok());
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0],
        vec![
            Cell::Int("-32768".to_string()),
            Cell::Int("-2147483648".to_string()),
            Cell::Int("-9223372036854775808".to_string()),
            Cell::Text("WXYZ".to_string()),
            Cell::Bool(false),
            Cell::Null,
        ]
    );
    assert_eq!(
        rows[1],
        vec![
            Cell::Int("0".to_string()),
            Cell::Int("-1".to_string()),
            Cell::Int("-1".to_string()),
            Cell::Text("longer".to_string()),
            Cell::Bool(true),
            Cell::Text("note-b".to_string()),
        ]
    );
    assert_eq!(
        rows[3],
        vec![
            Cell::Int("32767".to_string()),
            Cell::Int("2147483647".to_string()),
            Cell::Int("9223372036854775807".to_string()),
            Cell::Text("alpha".to_string()),
            Cell::Bool(true),
            Cell::Text("note-d".to_string()),
        ]
    );
    assert_eq!(summary.returned_rows(), 4);
    assert_eq!(summary.result_bytes(), canon_total(&rows));
    assert_eq!(audit_count(&env.admin).await, before_audit + 1);
    let stored = latest_audit(&env.admin).await;
    assert_eq!(stored.0, star_plan.original_sha256);
    assert_eq!(stored.1, star_plan.executed_sha256);
    assert_eq!(stored.2, ROLE_A);
    assert_eq!(stored.3, pid);
    assert_eq!(stored.4, start);
    assert!(session.is_open());
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    env.oracle.fact("star", "rows:4 status:succeeded");
    env.oracle.fact("star_original_sha256", &stored.0);
    env.oracle.fact("star_executed_sha256", &stored.1);
}

async fn prove_row_bound(env: &mut Env, identity: &PolicyIdentity, happy: &PolicyCatalog) {
    reset_calls(env).await;
    let mut session = open_session(env).await;
    let bound_sql = "SELECT fixture.note_read(), i4 FROM logistics.shipment";
    let bound_plan = guard_select(identity, happy, bound_sql).expect("guard bound");
    assert!(bound_plan.executed_sql.contains("fixture"));
    assert!(bound_plan.executed_sql.contains("note_read"));
    assert!(!bound_plan.executed_sql.to_lowercase().contains("order by"));
    let bound_stream = session
        .execute_trusted_select(
            req(identity, happy, bound_sql, ROLE_A, 2, BYTES_OK, WALL_OK),
            open_audit(env, false, false).await,
        )
        .await
        .expect("bound execute");
    let (bound_rows, bound_summary) = finish_ok(&mut session, bound_stream).await;
    assert_eq!(bound_summary.status(), CompletionStatus::Succeeded);
    assert!(bound_summary.truncated());
    assert_eq!(bound_rows.len(), 2);
    assert!(bound_rows.iter().all(|row| {
        row.len() == 2
            && matches!(row[0], Cell::Text(ref text) if text == "n")
            && matches!(row[1], Cell::Int(_))
    }));
    assert!(session.is_open());
    // Pending pg_stat function counters flush when this backend exits.
    session.close();
    assert_eq!(calls_settled(&env.admin, 0).await, 3);
    env.oracle.fact("row_bound", "rows:2 calls:3 truncated:true");
}

async fn prove_byte_caps(
    env: &mut Env,
    session: &mut DedicatedSession,
    identity: &PolicyIdentity,
    happy: &PolicyCatalog,
) {
    let byte_sql = "SELECT label FROM logistics.shipment ORDER BY i4";
    let byte_stream = session
        .execute_trusted_select(
            req(identity, happy, byte_sql, ROLE_A, 10, 4, WALL_OK),
            open_audit(env, false, false).await,
        )
        .await
        .expect("byte execute");
    let (byte_rows, byte_summary) = finish_ok(session, byte_stream).await;
    assert_eq!(byte_summary.status(), CompletionStatus::Succeeded);
    assert!(byte_summary.truncated());
    assert_eq!(byte_rows, vec![vec![Cell::Text("WXYZ".to_string())]]);
    assert_eq!(byte_summary.returned_rows(), 1);
    assert_eq!(byte_summary.result_bytes(), 4);
    env.oracle.fact("byte_cap", "rows:1 bytes:4 truncated:true");

    let wide_sql = "SELECT label FROM logistics.shipment WHERE i4 = 1";
    let wide_stream = session
        .execute_trusted_select(
            req(identity, happy, wide_sql, ROLE_A, 10, 1, WALL_OK),
            open_audit(env, false, false).await,
        )
        .await
        .expect("wide execute");
    let (wide_rows, wide_summary) = finish_ok(session, wide_stream).await;
    assert!(wide_rows.is_empty());
    assert_eq!(wide_summary.status(), CompletionStatus::Succeeded);
    assert!(wide_summary.truncated());
    assert_eq!(wide_summary.returned_rows(), 0);
    assert_eq!(wide_summary.result_bytes(), 0);
    env.oracle.fact("oversized", "rows:0 truncated:true status:succeeded");
}

async fn prove_role_reuse(env: &mut Env, session: &mut DedicatedSession) {
    let calls_before = note_calls(&env.admin).await;
    let audit_before = audit_count(&env.admin).await;
    let identity_b = identity_for(ROLE_B);
    let role_b = role_b_catalog(env);
    let reuse = session
        .execute_trusted_select(
            req(
                &identity_b,
                &role_b,
                "SELECT fixture.note_read() FROM logistics.shipment",
                ROLE_B,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    let reuse = refused(reuse);
    assert!(matches!(reuse, ExecError::RoleReuse));
    assert_eq!(note_calls(&env.admin).await, calls_before);
    assert_eq!(audit_count(&env.admin).await, audit_before);
    assert!(session.is_open());
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    env.oracle.fact("role_reuse", "refused");
}

async fn prove_guard_refusals(
    env: &mut Env,
    session: &mut DedicatedSession,
    identity: &PolicyIdentity,
    happy: &PolicyCatalog,
) {
    let calls_before = note_calls(&env.admin).await;
    let audit_before = audit_count(&env.admin).await;
    let hidden = session
        .execute_trusted_select(
            req(
                identity,
                happy,
                "SELECT secret FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    let hidden = refused(hidden);
    assert!(matches!(
        hidden.guard_error(),
        Some(GuardError::ColumnNotAllowed { .. })
    ));
    let fresh = session
        .execute_trusted_select(
            req(
                identity,
                happy,
                "SELECT extra FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    let fresh = refused(fresh);
    assert!(matches!(
        fresh.guard_error(),
        Some(GuardError::ColumnNotAllowed { .. })
    ));
    let write = session
        .execute_trusted_select(
            req(
                identity,
                happy,
                "INSERT INTO logistics.shipment (i4) VALUES (1)",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    let write = refused(write);
    assert!(matches!(write.guard_error(), Some(GuardError::Write)));
    assert_eq!(audit_count(&env.admin).await, audit_before);
    assert_eq!(note_calls(&env.admin).await, calls_before);
    assert!(session.is_open());
    env.oracle.fact("guard_refusals", "hidden extra write");
    session.close();
    let durable = audit_count(&env.admin).await;
    assert!(durable >= 1);
    let writer_owns: bool = env
        .admin
        .query_one(
            "SELECT has_table_privilege($1, 'fixture.audit_log', 'INSERT')",
            &[&ROLE_A],
        )
        .await
        .expect("audit privilege")
        .get(0);
    assert!(!writer_owns);
    env.oracle.fact("audit_durable", &durable.to_string());
}

async fn type_lying_drop_and_audit(env: &mut Env) {
    prove_type_refusal(env).await;
    prove_lying_extra(env).await;
    prove_drop_stream(env).await;
    prove_audit_fail(env).await;
}

async fn prove_type_refusal(env: &mut Env) {
    reset_calls(env).await;
    let calls_before = note_calls(&env.admin).await;
    let audit_before = audit_count(&env.admin).await;
    let identity = identity_for(ROLE_A);
    let typed = amount_catalog(env);
    let mut session = open_session(env).await;
    let typed_stream = session
        .execute_trusted_select(
            req(
                &identity,
                &typed,
                "SELECT amount, fixture.note_read() FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("type stream");
    let (err, summary) = finish_err(&mut session, typed_stream).await;
    match err {
        ExecError::TypeRefused { oid } => assert_eq!(oid, env.oids.numeric),
        other => panic!("type refusal became {other}"),
    }
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.stop(), Some(StopClass::TypeRefused));
    assert_eq!(summary.returned_rows(), 0);
    assert!(!session.is_open());
    assert_eq!(calls_settled(&env.admin, calls_before).await, calls_before);
    assert_eq!(audit_count(&env.admin).await, audit_before + 1);
    env.oracle.fact("type_refused", &format!("oid:{} calls_delta:0", env.oids.numeric));
}

async fn prove_lying_extra(env: &mut Env) {
    let identity = identity_for(ROLE_A);
    let lying = lying_catalog(env);
    let mut session = open_session(env).await;
    let lying_stream = session
        .execute_trusted_select(
            req(
                &identity,
                &lying,
                "SELECT extra FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("lying stream");
    let (err, summary) = finish_err(&mut session, lying_stream).await;
    assert!(matches!(err, ExecError::Database { .. }));
    assert_eq!(err.sqlstate(), Some("42501"));
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.stop(), Some(StopClass::Database));
    assert_eq!(summary.returned_rows(), 0);
    assert!(summary.socket_dropped());
    assert!(!session.is_open());
    env.oracle.fact("lying_extra", "42501 failed");
}

async fn prove_drop_stream(env: &mut Env) {
    let identity = identity_for(ROLE_A);
    let happy = happy_catalog(env);
    let mut session = open_session(env).await;
    let stream = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT i4 FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, true).await,
        )
        .await
        .expect("drop stream");
    drop(stream);
    session.wait_idle().await.expect("drop idle");
    let summary = session.summary().expect("drop summary");
    assert_eq!(summary.status(), CompletionStatus::Partial);
    assert_ne!(summary.status(), CompletionStatus::Succeeded);
    assert!(!summary.end_audit_ok());
    assert!(summary.socket_dropped());
    assert!(!session.is_open());
    env.oracle.fact("drop_stream", "partial end_audit_ok:false");
}

async fn prove_audit_fail(env: &mut Env) {
    let identity = identity_for(ROLE_A);
    let happy = happy_catalog(env);
    reset_calls(env).await;
    let calls_before = note_calls(&env.admin).await;
    let audit_before = audit_count(&env.admin).await;
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let err = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT fixture.note_read(), i4 FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, true, false).await,
        )
        .await;
    let err = refused(err);
    assert!(matches!(err, ExecError::AuditUnavailable));
    assert_eq!(err.code(), oo_core::api::ErrorCode::AuditUnavailable);
    assert_eq!(note_calls(&env.admin).await, calls_before);
    assert_eq!(audit_count(&env.admin).await, audit_before);
    let mentioned: bool = env
        .admin
        .query_one(
            "SELECT EXISTS (\
                SELECT 1 FROM pg_catalog.pg_stat_activity \
                WHERE pid = $1 AND position($2 in query) > 0)",
            &[&pid, &env.shadow.table],
        )
        .await
        .expect("activity")
        .get(0);
    assert!(!mentioned);
    assert!(session.is_open());
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    let identity_b = identity_for(ROLE_B);
    let role_b = role_b_catalog(env);
    let reuse = session
        .execute_trusted_select(
            req(
                &identity_b,
                &role_b,
                "SELECT i4 FROM logistics.shipment",
                ROLE_B,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    let reuse = refused(reuse);
    assert!(matches!(reuse, ExecError::RoleReuse));
    assert_eq!(audit_count(&env.admin).await, audit_before);
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    assert!(session.is_open());
    session.close();
    env.oracle.fact("audit_fail", "bound_then_other_role_refused");
}

async fn deadline_cancel_and_role_b(env: &mut Env) {
    let locker = connect_db(DATABASE, ADMIN, &env.admin_pass, &env.ca).await;
    prove_server_deadline(env, &locker).await;
    prove_inflight_cancel(env, &locker).await;
    prove_late_cancel(env, &locker).await;
    drop(locker);
    prove_role_b_rows(env).await;
}

async fn prove_server_deadline(env: &mut Env, locker: &Client) {
    let identity = identity_for(ROLE_A);
    let happy = happy_catalog(env);
    lock_table(locker, &env.shadow.table).await;
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let start = session.backend_start();
    let started = Instant::now();
    let stream = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT i4 FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                2_000,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("deadline stream");
    let (err, summary) = finish_err(&mut session, stream).await;
    let elapsed = started.elapsed();
    match &err {
        ExecError::Timeout { sqlstate } => assert_eq!(sqlstate.as_deref(), Some("57014")),
        other => panic!("deadline became {other}"),
    }
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.stop(), Some(StopClass::Timeout));
    assert!(summary.socket_dropped());
    assert!(!session.is_open());
    assert!((Duration::from_millis(500)..=Duration::from_secs(5)).contains(&elapsed));
    assert!(backend_gone(&env.admin, pid, &start).await);
    unlock_table(locker).await;
    env.oracle.fact(
        "deadline",
        &format!("57014 elapsed_ms:{}", elapsed.as_millis()),
    );
}

async fn prove_inflight_cancel(env: &mut Env, locker: &Client) {
    let identity = identity_for(ROLE_A);
    let happy = happy_catalog(env);
    lock_table(locker, &env.shadow.table).await;
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let start = session.backend_start();
    let started = Instant::now();
    let mut stream = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT i4 FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("cancel stream");
    assert!(
        wait_user_query(&env.admin, pid, &env.shadow.table).await,
        "blocked select did not become active"
    );
    let outcome = stream.cancel_handle().cancel(&env.admin).await.expect("cancel");
    assert_eq!(outcome, CancelOutcome::Sent);
    let err = stream.next_row().await.expect_err("canceled row");
    drop(stream);
    session.wait_idle().await.expect("cancel idle");
    let summary = session.summary().expect("cancel summary");
    match &err {
        ExecError::Canceled { sqlstate } => assert_eq!(sqlstate, "57014"),
        other => panic!("cancel became {other}"),
    }
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.stop(), Some(StopClass::Canceled));
    assert!(!session.is_open());
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(backend_gone(&env.admin, pid, &start).await);
    unlock_table(locker).await;
    env.oracle.fact("inflight_cancel", "sent 57014");
}

async fn prove_late_cancel(env: &mut Env, locker: &Client) {
    let identity = identity_for(ROLE_A);
    let happy = happy_catalog(env);
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let start = session.backend_start();
    let first = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT i4 FROM logistics.shipment ORDER BY i4",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("late first");
    let old_handle = first.cancel_handle();
    let (rows, first_summary) = finish_ok(&mut session, first).await;
    assert_eq!(rows.len(), 4);
    assert_eq!(first_summary.status(), CompletionStatus::Succeeded);
    assert!(session.is_open());
    assert_eq!(session.backend_pid(), pid);
    lock_table(locker, &env.shadow.table).await;
    let mut second = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT i4 FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("late second");
    assert!(wait_user_query(&env.admin, pid, &env.shadow.table).await);
    let stale = old_handle.cancel(&env.admin).await.expect("stale cancel");
    assert_eq!(stale, CancelOutcome::NotSent);
    let still = observe_backend(&env.admin, pid).await.expect("observe").expect("backend");
    assert_eq!(still.backend_start(), start);
    assert_eq!(still.state(), "active");
    let sent = second.cancel_handle().cancel(&env.admin).await.expect("second cancel");
    assert_eq!(sent, CancelOutcome::Sent);
    let err = second.next_row().await.expect_err("second row");
    drop(second);
    session.wait_idle().await.expect("late idle");
    assert!(matches!(err, ExecError::Canceled { .. }));
    assert!(!session.is_open());
    assert!(backend_gone(&env.admin, pid, &start).await);
    unlock_table(locker).await;
    env.oracle.fact("late_cancel", "not_sent then sent");
}

async fn prove_role_b_rows(env: &mut Env) {
    let identity_b = identity_for(ROLE_B);
    let role_b = role_b_catalog(env);
    let mut session = open_session(env).await;
    let role_b_stream = session
        .execute_trusted_select(
            req(
                &identity_b,
                &role_b,
                "SELECT * FROM logistics.shipment",
                ROLE_B,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("role b");
    let (rows, summary) = finish_ok(&mut session, role_b_stream).await;
    assert_eq!(summary.status(), CompletionStatus::Succeeded);
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| row.len() == 1));
    let mut values: Vec<i64> = rows
        .iter()
        .map(|row| match &row[0] {
            Cell::Int(text) => text.parse().expect("role b integer"),
            _ => panic!("role b cell was not an integer"),
        })
        .collect();
    values.sort_unstable();
    assert_eq!(values, vec![i64::from(i32::MIN), -1, 1, i64::from(i32::MAX)]);
    session.close();
    env.oracle.fact("role_b", "rows:4 cols:1");
}

async fn prove_role_timing(env: &mut Env) {
    prove_fresh_guard_binds(env).await;
    prove_invalid_role_does_not_bind(env).await;
    prove_login_flag_closes(env).await;
}

async fn prove_fresh_guard_binds(env: &mut Env) {
    let identity = identity_for(ROLE_A);
    let happy = happy_catalog(env);
    let mut session = open_session(env).await;
    let audit_before = audit_count(&env.admin).await;
    let write = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "INSERT INTO logistics.shipment (i4) VALUES (1)",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    let write = refused(write);
    assert!(matches!(write.guard_error(), Some(GuardError::Write)));
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    assert_eq!(audit_count(&env.admin).await, audit_before);
    assert!(session.is_open());
    let identity_b = identity_for(ROLE_B);
    let role_b = role_b_catalog(env);
    let reuse = session
        .execute_trusted_select(
            req(
                &identity_b,
                &role_b,
                "SELECT i4 FROM logistics.shipment",
                ROLE_B,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    let reuse = refused(reuse);
    assert!(matches!(reuse, ExecError::RoleReuse));
    assert_eq!(audit_count(&env.admin).await, audit_before);
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    let stream = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT i4 FROM logistics.shipment ORDER BY i4",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("same role retry");
    let (rows, summary) = finish_ok(&mut session, stream).await;
    assert_eq!(rows.len(), 4);
    assert_eq!(summary.status(), CompletionStatus::Succeeded);
    assert!(session.is_open());
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    session.close();
    env.oracle.fact("fresh_guard_bind", "other_role_rejected same_role_ran");
}

async fn prove_invalid_role_does_not_bind(env: &mut Env) {
    let happy = happy_catalog(env);
    let mut session = open_session(env).await;
    let bad_identity = identity_for("not-a-role");
    let invalid = session
        .execute_trusted_select(
            req(
                &bad_identity,
                &happy,
                "SELECT i4 FROM logistics.shipment",
                "not-a-role",
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    assert!(matches!(refused(invalid), ExecError::RoleParameter));
    assert_eq!(session.bound_role(), None);
    assert!(session.is_open());
    let mismatched = identity_for(ROLE_B);
    let mismatch = session
        .execute_trusted_select(
            req(
                &mismatched,
                &happy,
                "SELECT i4 FROM logistics.shipment",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    assert!(matches!(refused(mismatch), ExecError::RoleParameter));
    assert_eq!(session.bound_role(), None);
    let limited_identity = identity_for(ROLE_A);
    let limited = session
        .execute_trusted_select(
            req(
                &limited_identity,
                &happy,
                "SELECT i4 FROM logistics.shipment",
                ROLE_A,
                2_001,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    assert!(matches!(refused(limited), ExecError::Limit));
    assert_eq!(session.bound_role(), None);
    assert!(session.is_open());
    let identity = identity_for(ROLE_A);
    let stream = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT i4 FROM logistics.shipment ORDER BY i4",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("valid role binds");
    let (rows, summary) = finish_ok(&mut session, stream).await;
    assert_eq!(rows.len(), 4);
    assert_eq!(summary.status(), CompletionStatus::Succeeded);
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    let again = session
        .execute_trusted_select(
            req(
                &bad_identity,
                &happy,
                "SELECT i4 FROM logistics.shipment",
                "not-a-role",
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    assert!(matches!(refused(again), ExecError::RoleParameter));
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_A));
    assert!(session.is_open());
    session.close();
    env.oracle.fact("invalid_role", "not_bound");
}

async fn prove_login_flag_closes(env: &mut Env) {
    env.admin
        .batch_execute(
            "CREATE ROLE oo_r_flag_probe LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION",
        )
        .await
        .expect("flag role");
    let identity = identity_for(ROLE_FLAG);
    let i4_name = env.shadow.i4.clone();
    let i4_oid = env.oids.int4;
    let catalog = catalog(env, ROLE_FLAG, vec![("i4", &i4_name, i4_oid)], false, false, 0);
    let mut session = open_session(env).await;
    let audit_before = audit_count(&env.admin).await;
    let stream = session
        .execute_trusted_select(
            req(
                &identity,
                &catalog,
                "SELECT i4 FROM logistics.shipment",
                ROLE_FLAG,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("flag stream");
    let (err, summary) = finish_err(&mut session, stream).await;
    assert!(matches!(err, ExecError::RoleParameter));
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert!(summary.socket_dropped());
    assert!(!session.is_open());
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_FLAG));
    assert_eq!(audit_count(&env.admin).await, audit_before + 1);
    let identity_b = identity_for(ROLE_B);
    let role_b = role_b_catalog(env);
    let closed = session
        .execute_trusted_select(
            req(
                &identity_b,
                &role_b,
                "SELECT i4 FROM logistics.shipment",
                ROLE_B,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await;
    assert!(matches!(refused(closed), ExecError::SessionClosed));
    assert_eq!(audit_count(&env.admin).await, audit_before + 1);
    env.oracle.fact("flag_role", "session_closed");
}

async fn prove_blocked_shutdown(env: &mut Env) {
    prove_lock_drop(env).await;
    let hold = hold_catalog(env);
    prove_held_drop(
        env,
        "sleep",
        "SELECT fixture.page_hold() FROM logistics.shipment LIMIT 1",
        &hold,
    )
    .await;
    prove_client_budget_spares_next(env, &hold).await;
    prove_finished_drop_spares_next(env, &hold).await;
}

async fn prove_lock_drop(env: &mut Env) {
    let locker = connect_db(DATABASE, ADMIN, &env.admin_pass, &env.ca).await;
    lock_table(&locker, &env.shadow.table).await;
    let happy = happy_catalog(env);
    prove_held_drop(env, "lock", "SELECT i4 FROM logistics.shipment", &happy).await;
    unlock_table(&locker).await;
}

async fn prove_held_drop(env: &mut Env, label: &str, sql: &str, catalog: &PolicyCatalog) {
    let identity = identity_for(ROLE_A);
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let start = session.backend_start();
    let stream = session
        .execute_trusted_select(
            req(&identity, catalog, sql, ROLE_A, 10, BYTES_OK, 8_000),
            open_audit(env, false, false).await,
        )
        .await
        .unwrap_or_else(|err| panic!("{label} stream refused: {err}"));
    assert!(
        wait_user_query(&env.admin, pid, &env.shadow.table).await,
        "{label} query did not become active"
    );
    tokio::time::sleep(Duration::from_millis(3_100)).await;
    assert!(
        backend_active(&env.admin, pid).await,
        "{label} server limit fired before 3s"
    );
    let dropped_at = Instant::now();
    drop(stream);
    session.wait_idle().await.expect("held idle");
    let elapsed = dropped_at.elapsed();
    let summary = session.summary().expect("held summary");
    env.oracle.fact(&format!("{label}_held_ms"), "3100");
    env.oracle.fact(&format!("{label}_wall_ms"), "8000");
    env.oracle.fact(&format!("{label}_drop_ms"), &elapsed.as_millis().to_string());
    env.oracle.fact(&format!("{label}_status"), summary.status().as_str());
    env.oracle.fact(&format!("{label}_sqlstate"), summary.sqlstate().unwrap_or("none"));
    env.oracle.fact(&format!("{label}_stop"), stop_name(summary.stop()));
    env.oracle.fact(
        &format!("{label}_rollback"),
        if summary.rollback_confirmed() { "yes" } else { "no" },
    );
    env.oracle.fact(
        &format!("{label}_socket"),
        if summary.socket_dropped() { "yes" } else { "no" },
    );
    env.oracle.fact(
        &format!("{label}_end_audit"),
        if summary.end_audit_ok() { "yes" } else { "no" },
    );
    assert_eq!(summary.status(), CompletionStatus::Partial);
    assert_ne!(summary.status(), CompletionStatus::Succeeded);
    assert_ne!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.stop(), Some(StopClass::Protocol));
    assert_eq!(summary.sqlstate(), None);
    assert!(summary.rollback_confirmed(), "{label} rollback was not confirmed");
    assert!(summary.socket_dropped());
    assert!(summary.end_audit_ok());
    assert!(!session.is_open());
    assert!(
        elapsed < Duration::from_millis(1_000),
        "{label} drop took {} ms",
        elapsed.as_millis()
    );
    assert!(
        backend_gone(&env.admin, pid, &start).await,
        "{label} backend stayed active"
    );
}

async fn prove_client_budget_spares_next(env: &mut Env, hold: &PolicyCatalog) {
    let identity = identity_for(ROLE_A);
    let mut session = open_session(env).await;
    let pid = session.backend_pid();
    let start = session.backend_start();
    let started = Instant::now();
    let stream = session
        .execute_trusted_select(
            req(
                &identity,
                hold,
                "SELECT fixture.page_hold() FROM logistics.shipment LIMIT 1",
                ROLE_A,
                10,
                BYTES_OK,
                2_000,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("budget stream");
    let handle = stream.cancel_handle();
    let (err, summary) = finish_err(&mut session, stream).await;
    let elapsed = started.elapsed();
    match &err {
        ExecError::Timeout { sqlstate } => assert_eq!(sqlstate.as_deref(), Some("57014")),
        other => panic!("client budget became {other}"),
    }
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_ne!(summary.status(), CompletionStatus::Partial);
    assert_eq!(summary.stop(), Some(StopClass::Timeout));
    assert!(summary.socket_dropped());
    assert!(!session.is_open());
    assert!((Duration::from_millis(500)..=Duration::from_secs(5)).contains(&elapsed));
    assert!(backend_gone(&env.admin, pid, &start).await);
    let stale = handle.cancel(&env.admin).await.expect("stale budget cancel");
    assert_eq!(stale, CancelOutcome::NotSent);
    let mut next = open_session(env).await;
    let next_pid = next.backend_pid();
    let next_start = next.backend_start();
    let sleeping = next
        .execute_trusted_select(
            req(
                &identity,
                hold,
                "SELECT fixture.page_hold() FROM logistics.shipment LIMIT 1",
                ROLE_A,
                10,
                BYTES_OK,
                8_000,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("next sleep");
    assert!(wait_user_query(&env.admin, next_pid, &env.shadow.table).await);
    let late = handle.cancel(&env.admin).await.expect("late budget cancel");
    assert_eq!(late, CancelOutcome::NotSent);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(backend_active(&env.admin, next_pid).await, "late cancel hit the next query");
    let dropped_at = Instant::now();
    drop(sleeping);
    next.wait_idle().await.expect("next idle");
    let drop_elapsed = dropped_at.elapsed();
    let next_summary = next.summary().expect("next summary");
    assert_eq!(next_summary.status(), CompletionStatus::Partial);
    assert!(
        drop_elapsed < Duration::from_millis(1_000),
        "next drop took {} ms",
        drop_elapsed.as_millis()
    );
    assert!(backend_gone(&env.admin, next_pid, &next_start).await);
    env.oracle.fact(
        "client_budget",
        &format!("57014 elapsed_ms:{} late:not_sent", elapsed.as_millis()),
    );
}

async fn prove_finished_drop_spares_next(env: &mut Env, hold: &PolicyCatalog) {
    let identity = identity_for(ROLE_A);
    let happy = happy_catalog(env);
    let mut session = open_session(env).await;
    let first = session
        .execute_trusted_select(
            req(
                &identity,
                &happy,
                "SELECT i4 FROM logistics.shipment ORDER BY i4",
                ROLE_A,
                10,
                BYTES_OK,
                WALL_OK,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("finished first");
    let (rows, first_summary) = finish_ok(&mut session, first).await;
    assert_eq!(rows.len(), 4);
    assert_eq!(first_summary.status(), CompletionStatus::Succeeded);
    assert!(session.is_open());
    let pid = session.backend_pid();
    let sleeping = session
        .execute_trusted_select(
            req(
                &identity,
                hold,
                "SELECT fixture.page_hold() FROM logistics.shipment LIMIT 1",
                ROLE_A,
                10,
                BYTES_OK,
                8_000,
            ),
            open_audit(env, false, false).await,
        )
        .await
        .expect("finished next");
    assert!(wait_user_query(&env.admin, pid, &env.shadow.table).await);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(backend_active(&env.admin, pid).await, "finished drop canceled the next query");
    let dropped_at = Instant::now();
    drop(sleeping);
    session.wait_idle().await.expect("finished next idle");
    let elapsed = dropped_at.elapsed();
    let summary = session.summary().expect("finished next summary");
    assert_eq!(summary.status(), CompletionStatus::Partial);
    assert!(
        elapsed < Duration::from_millis(1_000),
        "finished next drop took {} ms",
        elapsed.as_millis()
    );
    env.oracle.fact(
        "finished_drop_next",
        &format!("still_active drop_ms:{}", elapsed.as_millis()),
    );
}

async fn backend_active(client: &Client, pid: i32) -> bool {
    client
        .query_one(
            "SELECT EXISTS (\
                SELECT 1 FROM pg_catalog.pg_stat_activity \
                WHERE pid = $1 AND state = 'active')",
            &[&pid],
        )
        .await
        .expect("backend active")
        .get(0)
}

fn stop_name(stop: Option<StopClass>) -> &'static str {
    match stop {
        Some(StopClass::Timeout) => "timeout",
        Some(StopClass::Canceled) => "canceled",
        Some(StopClass::TypeRefused) => "type",
        Some(StopClass::Database) => "database",
        Some(StopClass::Protocol) => "protocol",
        None => "none",
    }
}

fn refused(result: Result<SelectStream, ExecError>) -> ExecError {
    match result {
        Err(err) => err,
        Ok(stream) => {
            drop(stream);
            panic!("expected the executor to refuse before streaming");
        }
    }
}

async fn finish_ok(session: &mut DedicatedSession, mut stream: SelectStream) -> (Vec<Vec<Cell>>, QuerySummary) {
    let mut rows = Vec::new();
    loop {
        match stream.next_row().await {
            Ok(Some(row)) => rows.push(row),
            Ok(None) => break,
            Err(err) => panic!("row error: {err}"),
        }
    }
    drop(stream);
    session.wait_idle().await.expect("idle");
    let summary = session.summary().expect("summary");
    (rows, summary)
}

async fn finish_err(session: &mut DedicatedSession, mut stream: SelectStream) -> (ExecError, QuerySummary) {
    let err = match stream.next_row().await {
        Err(err) => err,
        Ok(_) => panic!("expected an executor error"),
    };
    drop(stream);
    session.wait_idle().await.expect("idle after error");
    let summary = session.summary().expect("summary after error");
    (err, summary)
}

fn canon_total(rows: &[Vec<Cell>]) -> u64 {
    rows.iter().flatten().map(canon_len).fold(0, u64::saturating_add)
}

fn canon_len(cell: &Cell) -> u64 {
    match cell {
        Cell::Null => 0,
        Cell::Bool(true) => 4,
        Cell::Bool(false) => 5,
        Cell::Int(text) | Cell::Text(text) => u64::try_from(text.len()).unwrap_or(u64::MAX),
    }
}

fn req<'a>(
    identity: &'a PolicyIdentity,
    catalog: &'a PolicyCatalog,
    sql: &'a str,
    role: &'a str,
    max_rows: u32,
    max_result_bytes: u64,
    wall_ms: u64,
) -> SelectRequest<'a> {
    SelectRequest {
        identity,
        catalog,
        original_sql: sql,
        role,
        max_rows,
        max_result_bytes,
        wall_clock: Duration::from_millis(wall_ms),
    }
}

fn identity_for(role: &str) -> PolicyIdentity {
    PolicyIdentity {
        project: SOURCE.to_string(),
        role: role.to_string(),
        policy_id: CONFIG.to_string(),
    }
}

fn happy_catalog(env: &Env) -> PolicyCatalog {
    catalog(
        env,
        ROLE_A,
        vec![
            ("i2", &env.shadow.i2, env.oids.int2),
            ("i4", &env.shadow.i4, env.oids.int4),
            ("i8", &env.shadow.i8, env.oids.int8),
            ("label", &env.shadow.label, env.oids.text),
            ("flag", &env.shadow.flag, env.oids.bool),
            ("note", &env.shadow.note, env.oids.text),
        ],
        true,
        true,
        1,
    )
}

fn hold_catalog(env: &Env) -> PolicyCatalog {
    let mut catalog = happy_catalog(env);
    catalog.functions.push(ResolvedFunction {
        oid: env.hold_oid,
        schema: "fixture".to_string(),
        name: "page_hold".to_string(),
        argument_type_oids: Vec::new(),
        aggregate_star: false,
    });
    catalog
}

fn amount_catalog(env: &Env) -> PolicyCatalog {
    catalog(
        env,
        ROLE_A,
        vec![("amount", &env.shadow.amount, env.oids.numeric)],
        true,
        false,
        0,
    )
}

fn role_b_catalog(env: &Env) -> PolicyCatalog {
    catalog(env, ROLE_B, vec![("i4", &env.shadow.i4, env.oids.int4)], false, false, 1)
}

fn lying_catalog(env: &Env) -> PolicyCatalog {
    catalog(env, ROLE_A, vec![("extra", "extra", env.oids.text)], false, false, 0)
}

fn catalog(
    env: &Env,
    role: &str,
    columns: Vec<(&str, &str, u32)>,
    function: bool,
    operator: bool,
    pending: u32,
) -> PolicyCatalog {
    let identity = identity_for(role);
    let mut functions = Vec::new();
    if function {
        functions.push(ResolvedFunction {
            oid: env.note_oid,
            schema: "fixture".to_string(),
            name: "note_read".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: false,
        });
    }
    let mut operators = Vec::new();
    if operator {
        operators.push(ResolvedOperator {
            oid: env.eq_int.0,
            procedure_oid: env.eq_int.1,
            schema: "pg_catalog".to_string(),
            name: "=".to_string(),
            left_type_oid: env.oids.int4,
            right_type_oid: env.oids.int4,
        });
    }
    PolicyCatalog {
        identity: identity.clone(),
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
            pending_unclassified: pending,
            estimated_rows: 4,
        }],
        functions,
        operators,
        literal_types: LiteralTypes {
            integer: Some(env.oids.int4),
            text: Some(env.oids.text),
            boolean: Some(env.oids.bool),
        },
        null_test_allowed: true,
        large_table_row_threshold: 1_000_000,
    }
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
    DedicatedSession::bind(client, connection, tls)
        .await
        .expect("bind")
}

async fn open_audit(env: &Env, fail_admit: bool, fail_complete: bool) -> LiveAudit {
    LiveAudit {
        client: connect_db(DATABASE, ADMIN, &env.admin_pass, &env.ca).await,
        fail_admit,
        fail_complete,
    }
}

async fn lock_table(client: &Client, table: &str) {
    let sql = format!(
        "BEGIN; SET LOCAL lock_timeout = '15s'; LOCK TABLE \"_oo\".{} IN ACCESS EXCLUSIVE MODE",
        quote_physical(table, "pt_")
    );
    client.batch_execute(&sql).await.expect("lock table");
}

async fn unlock_table(client: &Client) {
    client.batch_execute("ROLLBACK").await.expect("unlock");
}

async fn wait_user_query(client: &Client, pid: i32, table: &str) -> bool {
    for _ in 0..100 {
        let visible: bool = client
            .query_one(
                "SELECT EXISTS (\
                    SELECT 1 FROM pg_catalog.pg_stat_activity \
                    WHERE pid = $1 AND state = 'active' AND position($2 in query) > 0)",
                &[&pid, &table],
            )
            .await
            .expect("wait query")
            .get(0);
        if visible {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

async fn backend_gone(client: &Client, pid: i32, start: &str) -> bool {
    for _ in 0..50 {
        match observe_backend(client, pid).await {
            Ok(None) => return true,
            Ok(Some(view)) if view.backend_start() != start => return true,
            Ok(Some(_)) => {}
            Err(_) => return false,
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

async fn note_calls(client: &Client) -> i64 {
    client
        .batch_execute("SELECT pg_catalog.pg_stat_clear_snapshot()")
        .await
        .expect("clear stat snapshot");
    client
        .query_one(
            "SELECT COALESCE((\
                SELECT calls FROM pg_catalog.pg_stat_user_functions \
                WHERE schemaname = 'fixture' AND funcname = 'note_read'), 0)::int8",
            &[],
        )
        .await
        .expect("note calls")
        .get(0)
}

/// Cumulative counters stay pending until the calling backend flushes.
/// A value other than `baseline` is returned as soon as it appears.
async fn calls_settled(client: &Client, baseline: i64) -> i64 {
    let mut seen = note_calls(client).await;
    for _ in 0..40 {
        if seen != baseline {
            return seen;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        seen = note_calls(client).await;
    }
    seen
}

async fn reset_calls(env: &Env) {
    env.admin
        .batch_execute(&format!(
            "SELECT pg_catalog.pg_stat_reset_single_function_counters({}::oid)",
            env.note_oid
        ))
        .await
        .expect("reset calls");
}

async fn audit_count(client: &Client) -> i64 {
    client
        .query_one("SELECT count(*)::int8 FROM fixture.audit_log", &[])
        .await
        .expect("audit count")
        .get(0)
}

async fn latest_audit(client: &Client) -> (String, String, String, i32, String) {
    let row = client
        .query_one(
            "SELECT original_sha256, executed_sha256, role_name, backend_pid, backend_start \
             FROM fixture.audit_log ORDER BY id DESC LIMIT 1",
            &[],
        )
        .await
        .expect("latest audit");
    (row.get(0), row.get(1), row.get(2), row.get(3), row.get(4))
}

async fn privilege_count(client: &Client, role: &str, column: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*)::int8 FROM information_schema.column_privileges \
             WHERE grantee = $1 AND privilege_type = 'SELECT' AND column_name = $2",
            &[&role, &column],
        )
        .await
        .expect("privilege count")
        .get(0)
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

async fn sql_outcome(client: &Client, sql: &str) -> String {
    match client.query(sql, &[]).await {
        Ok(rows) => format!("ok:{}", rows.len()),
        Err(err) => err
            .as_db_error()
            .map(|db| db.code().code().to_string())
            .unwrap_or_else(|| "none".to_string()),
    }
}

async fn install_roles(client: &mut Client) {
    let names = ["i2", "i4", "i8", "label", "flag", "note", "amount"];
    let mut columns = Vec::new();
    for name in names {
        columns.push(column_spec(name));
    }
    let installed = install_column_acl(client, &policy("reader_a", columns))
        .await
        .expect("install reader a");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, ROLE_A);
    let installed = install_column_acl(client, &policy("reader_b", vec![column_spec("i4")]))
        .await
        .expect("install reader b");
    assert_eq!(installed.effect, InstallEffect::Installed);
    assert_eq!(installed.role, ROLE_B);
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

async fn install_note_function(client: &Client) -> u32 {
    client
        .batch_execute(
            "CREATE SCHEMA fixture;\n\
             REVOKE ALL ON SCHEMA fixture FROM PUBLIC;\n\
             CREATE FUNCTION fixture.note_read() RETURNS text\n\
             LANGUAGE plpgsql VOLATILE AS $$ BEGIN RETURN 'n'; END $$;\n\
             REVOKE ALL ON FUNCTION fixture.note_read() FROM PUBLIC;\n\
             GRANT USAGE ON SCHEMA fixture TO oo_r_reader_a;\n\
             GRANT EXECUTE ON FUNCTION fixture.note_read() TO oo_r_reader_a",
        )
        .await
        .expect("note function");
    let oid: i32 = client
        .query_one(
            "SELECT p.oid::int4 FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'fixture' AND p.proname = 'note_read' AND p.pronargs = 0",
            &[],
        )
        .await
        .expect("note oid")
        .get(0);
    let volatile: String = client
        .query_one(
            "SELECT provolatile::text FROM pg_catalog.pg_proc WHERE oid = ($1::int4)::oid",
            &[&oid],
        )
        .await
        .expect("volatility")
        .get(0);
    assert_eq!(volatile, "v");
    u32::try_from(oid).expect("note oid range")
}

async fn install_page_hold(client: &Client) -> u32 {
    client
        .batch_execute(
            "CREATE FUNCTION fixture.page_hold() RETURNS integer\n\
             LANGUAGE plpgsql VOLATILE AS $$\n\
             BEGIN PERFORM pg_catalog.pg_sleep(30); RETURN 1; END $$;\n\
             REVOKE ALL ON FUNCTION fixture.page_hold() FROM PUBLIC;\n\
             GRANT EXECUTE ON FUNCTION fixture.page_hold() TO oo_r_reader_a",
        )
        .await
        .expect("page hold function");
    let oid: i32 = client
        .query_one(
            "SELECT p.oid::int4 FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'fixture' AND p.proname = 'page_hold' AND p.pronargs = 0",
            &[],
        )
        .await
        .expect("page hold oid")
        .get(0);
    let volatile: String = client
        .query_one(
            "SELECT provolatile::text FROM pg_catalog.pg_proc WHERE oid = ($1::int4)::oid",
            &[&oid],
        )
        .await
        .expect("page hold volatility")
        .get(0);
    assert_eq!(volatile, "v");
    u32::try_from(oid).expect("page hold oid range")
}

async fn install_audit(client: &Client) {
    client
        .batch_execute(
            "CREATE TABLE fixture.audit_log (\
                id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\
                original_sha256 text NOT NULL,\
                executed_sha256 text NOT NULL,\
                fingerprint_hex text NOT NULL,\
                role_name text NOT NULL,\
                project text NOT NULL,\
                policy_id text NOT NULL,\
                backend_pid integer NOT NULL,\
                backend_start text NOT NULL)",
        )
        .await
        .expect("audit table");
}

async fn load_oids(client: &Client, shadow: &Shadow) -> Oids {
    let oids = Oids {
        int2: type_oid(client, "int2").await,
        int4: type_oid(client, "int4").await,
        int8: type_oid(client, "int8").await,
        text: type_oid(client, "text").await,
        bool: type_oid(client, "bool").await,
        numeric: type_oid(client, "numeric").await,
    };
    assert_eq!(column_type(client, &shadow.table, &shadow.i2).await, oids.int2);
    assert_eq!(column_type(client, &shadow.table, &shadow.i4).await, oids.int4);
    assert_eq!(column_type(client, &shadow.table, &shadow.i8).await, oids.int8);
    assert_eq!(column_type(client, &shadow.table, &shadow.label).await, oids.text);
    assert_eq!(column_type(client, &shadow.table, &shadow.flag).await, oids.bool);
    assert_eq!(column_type(client, &shadow.table, &shadow.note).await, oids.text);
    assert_eq!(column_type(client, &shadow.table, &shadow.amount).await, oids.numeric);
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

async fn operator_oid(client: &Client, left: u32, right: u32) -> (u32, u32) {
    let left_i = i32::try_from(left).expect("left oid");
    let right_i = i32::try_from(right).expect("right oid");
    let row = client
        .query_one(
            "SELECT o.oid::int4, o.oprcode::oid::int4 \
             FROM pg_catalog.pg_operator o \
             JOIN pg_catalog.pg_namespace n ON n.oid = o.oprnamespace \
             WHERE n.nspname = 'pg_catalog' AND o.oprname = '=' \
               AND o.oprleft = ($1::int4)::oid AND o.oprright = ($2::int4)::oid",
            &[&left_i, &right_i],
        )
        .await
        .expect("operator");
    (
        u32::try_from(row.get::<_, i32>(0)).expect("operator oid"),
        u32::try_from(row.get::<_, i32>(1)).expect("operator proc"),
    )
}

fn read_hex(name: &str) -> String {
    let raw = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    let value = raw.trim();
    let hex = (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    assert!(hex, "{name} is not lowercase hex");
    value.to_string()
}

async fn connect_db(database: &str, user: &str, password: &str, ca: &[u8]) -> Client {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: database.to_string(),
        user: user.to_string(),
    };
    let mut last = String::new();
    for _ in 0..20 {
        match connect_require_tls(&endpoint, password, ca).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return client;
            }
            Err(err) => {
                last = err.to_string();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    panic!("tls connect to {database} as {user} failed: {last}");
}

async fn reset_database(client: &Client) {
    expect_sql(
        client,
        "terminate database sessions",
        &format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = '{DATABASE}' AND pid <> pg_backend_pid()"
        ),
    )
    .await;
    expect_sql(client, "drop database", &format!("DROP DATABASE IF EXISTS \"{DATABASE}\"")).await;
    expect_sql(
        client,
        "drop fixture roles",
        &format!(
            "DO $$\n\
             DECLARE role_name text;\n\
             BEGIN\n\
               IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = '{LOGIN}') THEN\n\
                 DROP ROLE \"{LOGIN}\";\n\
               END IF;\n\
               FOR role_name IN\n\
                 SELECT rolname FROM pg_catalog.pg_roles\n\
                 WHERE rolname LIKE 'oo\\_r\\_%' ESCAPE '\\'\n\
               LOOP\n\
                 EXECUTE format('DROP ROLE %I', role_name);\n\
               END LOOP;\n\
               IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_pipeline_runtime') THEN\n\
                 DROP ROLE oo_pipeline_runtime;\n\
               END IF;\n\
               IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_target_owner') THEN\n\
                 DROP ROLE oo_target_owner;\n\
               END IF;\n\
             END\n\
             $$"
        ),
    )
    .await;
    expect_sql(
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
    exec_named(&tx, "0000_bootstrap_roles.sql", &bootstrap.0).await;
    exec_named(&tx, "set-owner", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0001_target_fence_expand.sql", &fence.0).await;
    exec_named(&tx, "row-lock", "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_named(&tx, "0003_target_segment_preflight_expand.sql", &preflight.0).await;
    exec_named(&tx, "0004_physical_mapping.sql", &mapping.0).await;
    exec_named(
        &tx,
        "markers-target",
        &marker_sql(&[
            ("target-bootstrap/1", &bootstrap.1, "0000_bootstrap_roles.sql"),
            ("target-fence-receipts/1", &fence.1, "0001_target_fence_expand.sql"),
            (
                "target-fence-receipts/3",
                &preflight.1,
                "0003_target_segment_preflight_expand.sql",
            ),
            ("physical-mapping/v1", &mapping.1, "0004_physical_mapping.sql"),
        ]),
    )
    .await;
    tx.commit().await.expect("commit target schema");

    let tx = begin_locked(client).await;
    exec_named(&tx, "row-lock-import", "SELECT id FROM _oo.migration_lock WHERE id = 1 FOR UPDATE").await;
    exec_named(
        &tx,
        "stats-grant",
        "GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE",
    )
    .await;
    exec_named(&tx, "set-owner-import", "SET LOCAL ROLE oo_target_owner").await;
    exec_named(&tx, "0005_full_import_expand.sql", &full_import.0).await;
    exec_named(
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
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {name}: {err}"));
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| panic!("{name} is not utf-8"));
    (text, sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex_digest(&Sha256::digest(bytes))
}

fn hex_digest(digest: &[u8]) -> String {
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        hex.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    hex
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
    exec_named(&tx, "lock-timeout", "SET LOCAL lock_timeout = '15s'").await;
    exec_named(&tx, "statement-timeout", "SET LOCAL statement_timeout = '60s'").await;
    exec_named(&tx, "advisory-lock", "SELECT pg_catalog.pg_advisory_xact_lock(6404202617)").await;
    tx
}

async fn expect_sql(client: &Client, name: &str, sql: &str) {
    client
        .batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", db_detail(&err)));
}

fn db_detail(err: &tokio_postgres::Error) -> String {
    err.as_db_error()
        .map(|db| db.to_string())
        .unwrap_or_else(|| err.to_string())
}

async fn exec_named(tx: &tokio_postgres::Transaction<'_>, name: &str, sql: &str) {
    tx.batch_execute(sql)
        .await
        .unwrap_or_else(|err| panic!("{name} failed: {}", db_detail(&err)));
}

async fn create_login(client: &Client, password: &str) {
    assert!(password.bytes().all(|byte| byte.is_ascii_hexdigit()));
    client
        .batch_execute(&format!(
            "CREATE ROLE \"{LOGIN}\" LOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE \
             NOBYPASSRLS NOREPLICATION PASSWORD '{password}';\n\
             GRANT CONNECT ON DATABASE \"{DATABASE}\" TO \"{LOGIN}\""
        ))
        .await
        .expect("create login");
}

async fn prepare_shadow(client: &mut Client) -> Shadow {
    let target: i64 = client
        .query_one(
            "SELECT _oo.adopt_target_generation($1, ($2::text)::uuid)",
            &[&SHARD, &OWNER],
        )
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
    let columns = ["i2", "i4", "i8", "label", "flag", "note", "amount", "secret"];
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
    let typmods = vec![-1_i32, -1, -1, -1, -1, -1, -1, -1];
    let required = vec![true, true, true, true, true, false, true, true];
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
        i2: lookup(client, "column", NAMESPACE, TABLE, "i2").await,
        i4: lookup(client, "column", NAMESPACE, TABLE, "i4").await,
        i8: lookup(client, "column", NAMESPACE, TABLE, "i8").await,
        label: lookup(client, "column", NAMESPACE, TABLE, "label").await,
        flag: lookup(client, "column", NAMESPACE, TABLE, "flag").await,
        note: lookup(client, "column", NAMESPACE, TABLE, "note").await,
        amount: lookup(client, "column", NAMESPACE, TABLE, "amount").await,
        secret: lookup(client, "column", NAMESPACE, TABLE, "secret").await,
    }
}

struct FixtureRow<'a> {
    i2: i16,
    i4: i32,
    i8: i64,
    label: &'a str,
    flag: bool,
    note: Option<&'a str>,
    amount: &'a str,
}

async fn insert_rows(client: &mut Client, shadow: &Shadow) {
    let sql = format!(
        "INSERT INTO \"_oo\".{} (\"_oo_pos\", {}, {}, {}, {}, {}, {}, {}, {}) \
         VALUES (($1::text)::numeric, $2, $3, $4, $5, $6, $7, ($8::text)::numeric, $9)",
        quote_physical(&shadow.table, "pt_"),
        quote_physical(&shadow.i2, "pc_"),
        quote_physical(&shadow.i4, "pc_"),
        quote_physical(&shadow.i8, "pc_"),
        quote_physical(&shadow.label, "pc_"),
        quote_physical(&shadow.flag, "pc_"),
        quote_physical(&shadow.note, "pc_"),
        quote_physical(&shadow.amount, "pc_"),
        quote_physical(&shadow.secret, "pc_")
    );
    let tx = client.transaction().await.expect("insert begin");
    let rows = [
        FixtureRow {
            i2: i16::MIN,
            i4: i32::MIN,
            i8: i64::MIN,
            label: "WXYZ",
            flag: false,
            note: None,
            amount: "1",
        },
        FixtureRow {
            i2: 0,
            i4: -1,
            i8: -1,
            label: "longer",
            flag: true,
            note: Some("note-b"),
            amount: "2",
        },
        FixtureRow {
            i2: 1,
            i4: 1,
            i8: 3,
            label: "oversized-label",
            flag: false,
            note: None,
            amount: "3",
        },
        FixtureRow {
            i2: i16::MAX,
            i4: i32::MAX,
            i8: i64::MAX,
            label: "alpha",
            flag: true,
            note: Some("note-d"),
            amount: "4",
        },
    ];
    for (index, row) in rows.iter().enumerate() {
        let pos = (index + 1).to_string();
        let note = row.note.map(str::to_string);
        let amount = row.amount.to_string();
        let secret = "classified-cell".to_string();
        let params: [&(dyn ToSql + Sync); 9] = [
            &pos,
            &row.i2,
            &row.i4,
            &row.i8,
            &row.label,
            &row.flag,
            &note,
            &amount,
            &secret,
        ];
        tx.execute(&sql, &params).await.expect("insert shadow row");
    }
    tx.commit().await.expect("insert commit");
}

async fn register(
    client: &Client,
    generation: i64,
    level: &str,
    letter: &str,
    namespace: &str,
    table: &str,
    column: &str,
) {
    let payload: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &letter,
        &namespace,
        &table,
        &column,
    ];
    let digest: Vec<u8> = client
        .query_one(
            "SELECT _oo.pm_sha256(_oo.pm_payload($1, $2, $3, $4, $5, $6, $7))",
            &payload,
        )
        .await
        .expect("digest")
        .get(0);
    let bind: [&(dyn ToSql + Sync); 8] = [
        &SOURCE,
        &CONFIG,
        &generation,
        &level,
        &namespace,
        &table,
        &column,
        &digest,
    ];
    client
        .execute(
            "SELECT _oo.register_physical_mapping($1, $2, $3, $4, $5, $6, $7, $8)",
            &bind,
        )
        .await
        .expect("register");
}

async fn lookup(client: &Client, level: &str, namespace: &str, table: &str, column: &str) -> String {
    let params: [&(dyn ToSql + Sync); 7] = [
        &SOURCE,
        &CONFIG,
        &CURRENT_GENERATION,
        &level,
        &namespace,
        &table,
        &column,
    ];
    client
        .query_one(
            "SELECT _oo.lookup_physical_mapping($1, $2, $3, $4, $5, $6, $7)",
            &params,
        )
        .await
        .expect("lookup")
        .get(0)
}

fn quote_physical(name: &str, prefix: &str) -> String {
    let rest = name.strip_prefix(prefix).unwrap_or("");
    assert_eq!(name.len(), 63, "physical length");
    assert_eq!(rest.len(), 60, "physical body");
    assert!(
        rest.bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "physical hex"
    );
    format!("\"{name}\"")
}
