//! Baseline overlay. Not product source.
//!
//! Run against immutable 8a289f0c. Exit 0 means the reviewed bugs were observed:
//! a lock-blocked drop waits for server 57014, and a guard refusal leaves the
//! session unbound so a different role is accepted.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oo_gateway::query_executor::{
    Admission, AuditAdmission, CompletionRecord, CompletionStatus, DedicatedSession, ExecError,
    QuerySummary, SelectRequest, StopClass,
};
use oo_gateway::{
    connect_require_tls, rustls_connector, GuardError, LiteralTypes, PolicyCatalog, PolicyIdentity,
    ResolvedFunction, TlsEndpoint, VisibleColumn, VisibleRelation,
};
use tokio_postgres::Client;

const HOST: &str = "oo-a189-pg";
const DATABASE: &str = "oo_a189";
const ADMIN: &str = "oo_fixture_admin";
const LOGIN: &str = "oo_a189_login";
const ROLE_EAST: &str = "oo_r_east";
const ROLE_WEST: &str = "oo_r_west";
const SECRET_DIR: &str = "/run/oo-a189/fixture";
const WALL: Duration = Duration::from_millis(6_000);
const PROJECT: &str = "a189";
const POLICY: &str = "probe-a189";

struct Oracle {
    path: String,
    lines: Vec<String>,
    secrets: Vec<String>,
}

impl Oracle {
    fn new(path: String, admin_pass: &str, login_pass: &str) -> Self {
        Self {
            path,
            lines: Vec::new(),
            secrets: vec![admin_pass.to_string(), login_pass.to_string()],
        }
    }

    fn fact(&mut self, key: &str, value: &str) {
        assert!(!key.contains("pass"), "oracle key");
        assert!(!value.contains('\n'), "oracle value");
        for secret in &self.secrets {
            assert!(!value.contains(secret), "oracle value contained a password");
        }
        self.lines.push(format!("{key}={value}"));
        if let Some(parent) = Path::new(&self.path).parent() {
            fs::create_dir_all(parent).expect("oracle dir");
        }
        fs::write(&self.path, self.lines.join("\n") + "\n").expect("oracle write");
    }
}

struct CountingAudit {
    admits: Arc<AtomicU32>,
}

impl AuditAdmission for CountingAudit {
    async fn admit(&mut self, _admission: &Admission<'_>) -> Result<(), ExecError> {
        self.admits.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn complete(&mut self, _completion: &CompletionRecord) -> Result<(), ExecError> {
        Ok(())
    }
}

struct Fixture {
    login_pass: String,
    ca: Vec<u8>,
    int4: u32,
    text_oid: u32,
    bool_oid: u32,
    hold_oid: u32,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn baseline_repro_blocked_drop_and_role() {
    if std::env::var("OO_A189_REPRO").ok().as_deref() != Some("1") {
        eprintln!("OO_A189_REPRO is unset; baseline reproduction was not run");
        return;
    }
    run().await;
}

async fn run() {
    let admin_pass = read_secret("admin.pass");
    let login_pass = read_secret("login.pass");
    let ca = fs::read(Path::new(SECRET_DIR).join("ca.crt")).expect("ca");
    let oracle_path = std::env::var("OO_A189_ORACLE")
        .unwrap_or_else(|_| "/evidence/linux/probe/oracle.txt".to_string());
    let mut oracle = Oracle::new(oracle_path, &admin_pass, &login_pass);
    oracle.fact("repro", "baseline");

    let maintenance = connect_db("postgres", ADMIN, &admin_pass, &ca).await;
    reset_database(&maintenance).await;
    drop(maintenance);

    let admin = connect_db(DATABASE, ADMIN, &admin_pass, &ca).await;
    let version: String = admin
        .query_one("SELECT current_setting('server_version')", &[])
        .await
        .expect("version")
        .get(0);
    oracle.fact("server_version", &version);
    let ssl: bool = admin
        .query_one(
            "SELECT ssl FROM pg_catalog.pg_stat_ssl WHERE pid = pg_backend_pid()",
            &[],
        )
        .await
        .expect("ssl")
        .get(0);
    oracle.fact("ssl", if ssl { "on" } else { "off" });
    assert!(ssl, "tls session");

    install_fixture(&admin, &login_pass).await;
    let int4 = type_oid(&admin, "int4").await;
    let text_oid = type_oid(&admin, "text").await;
    let bool_oid = type_oid(&admin, "bool").await;
    let hold_oid = function_oid(&admin).await;
    oracle.fact("int4_oid", &int4.to_string());
    oracle.fact("hold_oid", &hold_oid.to_string());
    assert_role_flags(&admin, ROLE_EAST).await;
    assert_role_flags(&admin, ROLE_WEST).await;
    oracle.fact("role_flags", "nologin");

    let fixture = Fixture {
        login_pass,
        ca,
        int4,
        text_oid,
        bool_oid,
        hold_oid,
    };

    prove_guard_then_other_role(&fixture, &admin, &mut oracle).await;
    prove_portal_drop(&fixture, &admin, &mut oracle).await;
    prove_lock_drop(&fixture, &admin, &mut oracle).await;
    oracle.fact("repro_result", "observed");
}

async fn prove_guard_then_other_role(fixture: &Fixture, _admin: &Client, oracle: &mut Oracle) {
    let mut session = open_session(fixture).await;
    let east = catalog(fixture, ROLE_EAST, true);
    let identity = east.identity.clone();
    let admits = Arc::new(AtomicU32::new(0));
    let refused = session
        .execute_trusted_select(
            SelectRequest {
                identity: &identity,
                catalog: &east,
                original_sql: "INSERT INTO depot.holdrow (n) VALUES (1)",
                role: ROLE_EAST,
                max_rows: 10,
                max_result_bytes: 4096,
                wall_clock: WALL,
            },
            CountingAudit {
                admits: Arc::clone(&admits),
            },
        )
        .await;
    let variant = match &refused {
        Err(err) => exec_variant(err),
        Ok(_) => "started".to_string(),
    };
    oracle.fact("guard_refusal", &variant);
    oracle.fact(
        "bound_role_after_guard",
        &session.bound_role().unwrap_or_else(|| "none".to_string()),
    );
    oracle.fact("guard_admits", &admits.load(Ordering::SeqCst).to_string());
    oracle.fact(
        "session_open_after_guard",
        if session.is_open() { "yes" } else { "no" },
    );
    assert!(matches!(refused, Err(ExecError::Guard(err)) if matches!(err.as_ref(), GuardError::Write)));
    assert_eq!(session.bound_role(), None);
    assert_eq!(admits.load(Ordering::SeqCst), 0);
    assert!(session.is_open());

    let west = catalog(fixture, ROLE_WEST, false);
    let west_identity = west.identity.clone();
    let west_admits = Arc::new(AtomicU32::new(0));
    let started = Instant::now();
    let mut stream = match session
        .execute_trusted_select(
            SelectRequest {
                identity: &west_identity,
                catalog: &west,
                original_sql: "SELECT n FROM depot.holdrow",
                role: ROLE_WEST,
                max_rows: 10,
                max_result_bytes: 4096,
                wall_clock: WALL,
            },
            CountingAudit {
                admits: Arc::clone(&west_admits),
            },
        )
        .await
    {
        Ok(stream) => stream,
        Err(err) => {
            oracle.fact("other_role", &exec_variant(&err));
            panic!("other role was refused");
        }
    };
    let mut rows = 0_u32;
    loop {
        match stream.next_row().await {
            Ok(Some(_)) => rows = rows.saturating_add(1),
            Ok(None) => break,
            Err(err) => {
                oracle.fact("other_role_read", &exec_variant(&err));
                panic!("other role read failed");
            }
        }
    }
    drop(stream);
    session.wait_idle().await.expect("west idle");
    let summary = session.summary().expect("west summary");
    oracle.fact("other_role", "allowed");
    oracle.fact("other_role_rows", &rows.to_string());
    oracle.fact("other_role_status", summary.status().as_str());
    oracle.fact("other_role_elapsed_ms", &started.elapsed().as_millis().to_string());
    oracle.fact(
        "other_role_bound",
        &session.bound_role().unwrap_or_else(|| "none".to_string()),
    );
    oracle.fact("other_role_admits", &west_admits.load(Ordering::SeqCst).to_string());
    assert_eq!(summary.status(), CompletionStatus::Succeeded);
    assert_eq!(rows, 1);
    assert_eq!(session.bound_role().as_deref(), Some(ROLE_WEST));
    assert!(west_admits.load(Ordering::SeqCst) >= 1);
}

async fn prove_portal_drop(fixture: &Fixture, admin: &Client, oracle: &mut Oracle) {
    let mut session = open_session(fixture).await;
    let pid = session.backend_pid();
    let east = catalog(fixture, ROLE_EAST, true);
    let identity = east.identity.clone();
    let stream = session
        .execute_trusted_select(
            SelectRequest {
                identity: &identity,
                catalog: &east,
                original_sql: "SELECT fixture.page_hold() FROM depot.holdrow",
                role: ROLE_EAST,
                max_rows: 10,
                max_result_bytes: 4096,
                wall_clock: WALL,
            },
            CountingAudit {
                admits: Arc::new(AtomicU32::new(0)),
            },
        )
        .await
        .expect("portal execute");
    let (visible, sample) = wait_blocked(admin, pid, "page_hold").await;
    oracle.fact("portal_visible", if visible { "yes" } else { "no" });
    oracle.fact("portal_activity", &sample);
    assert!(visible, "portal query never became active");
    let dropped = Instant::now();
    drop(stream);
    session.wait_idle().await.expect("portal idle");
    let elapsed = dropped.elapsed().as_millis();
    let summary = session.summary().expect("portal summary");
    record_drop(oracle, "portal", elapsed, &summary);
}

async fn prove_lock_drop(fixture: &Fixture, admin: &Client, oracle: &mut Oracle) {
    let locker = connect_db(DATABASE, ADMIN, &read_secret("admin.pass"), &fixture.ca).await;
    locker
        .batch_execute(
            "BEGIN; SET LOCAL lock_timeout = '15s'; LOCK TABLE depot.holdrow IN ACCESS EXCLUSIVE MODE",
        )
        .await
        .expect("lock table");
    let mut session = open_session(fixture).await;
    let pid = session.backend_pid();
    let east = catalog(fixture, ROLE_EAST, false);
    let identity = east.identity.clone();
    let stream = session
        .execute_trusted_select(
            SelectRequest {
                identity: &identity,
                catalog: &east,
                original_sql: "SELECT n FROM depot.holdrow",
                role: ROLE_EAST,
                max_rows: 10,
                max_result_bytes: 4096,
                wall_clock: WALL,
            },
            CountingAudit {
                admits: Arc::new(AtomicU32::new(0)),
            },
        )
        .await
        .expect("lock execute");
    let (visible, sample) = wait_blocked(admin, pid, "holdrow").await;
    oracle.fact("lock_visible", if visible { "yes" } else { "no" });
    oracle.fact("lock_activity", &sample);
    let dropped = Instant::now();
    drop(stream);
    session.wait_idle().await.expect("lock idle");
    let elapsed = dropped.elapsed().as_millis();
    let summary = session.summary().expect("lock summary");
    record_drop(oracle, "lock", elapsed, &summary);
    locker.batch_execute("ROLLBACK").await.expect("unlock");

    assert!(visible, "locked query never became active");
    assert!(elapsed > 1_000, "lock drop returned before 1s");
    assert_eq!(summary.status(), CompletionStatus::Failed);
    assert_eq!(summary.sqlstate(), Some("57014"));
}

fn record_drop(oracle: &mut Oracle, key: &str, elapsed: u128, summary: &QuerySummary) {
    oracle.fact(&format!("{key}_drop_elapsed_ms"), &elapsed.to_string());
    oracle.fact(&format!("{key}_drop_status"), summary.status().as_str());
    oracle.fact(
        &format!("{key}_drop_sqlstate"),
        summary.sqlstate().unwrap_or("none"),
    );
    oracle.fact(&format!("{key}_drop_stop"), &stop_name(summary.stop()));
    oracle.fact(
        &format!("{key}_drop_rollback"),
        if summary.rollback_confirmed() { "yes" } else { "no" },
    );
    oracle.fact(
        &format!("{key}_drop_socket"),
        if summary.socket_dropped() { "yes" } else { "no" },
    );
    oracle.fact(
        &format!("{key}_end_audit"),
        if summary.end_audit_ok() { "yes" } else { "no" },
    );
}

fn catalog(fixture: &Fixture, role: &str, function: bool) -> PolicyCatalog {
    let identity = PolicyIdentity {
        project: PROJECT.to_string(),
        role: role.to_string(),
        policy_id: POLICY.to_string(),
    };
    let mut functions = Vec::new();
    if function {
        functions.push(ResolvedFunction {
            oid: fixture.hold_oid,
            schema: "fixture".to_string(),
            name: "page_hold".to_string(),
            argument_type_oids: Vec::new(),
            aggregate_star: false,
        });
    }
    PolicyCatalog {
        identity,
        relations: vec![VisibleRelation {
            schema: "depot".to_string(),
            name: "holdrow".to_string(),
            physical_schema: "depot".to_string(),
            physical_name: "holdrow".to_string(),
            columns: vec![VisibleColumn {
                name: "n".to_string(),
                physical_name: "n".to_string(),
                type_oid: fixture.int4,
            }],
            pending_unclassified: 0,
            estimated_rows: 1,
        }],
        functions,
        operators: Vec::new(),
        literal_types: LiteralTypes {
            integer: Some(fixture.int4),
            text: Some(fixture.text_oid),
            boolean: Some(fixture.bool_oid),
        },
        null_test_allowed: true,
        large_table_row_threshold: 1_000_000,
    }
}

async fn open_session(fixture: &Fixture) -> DedicatedSession {
    let endpoint = TlsEndpoint {
        host: HOST.to_string(),
        port: 5432,
        database: DATABASE.to_string(),
        user: LOGIN.to_string(),
    };
    let (client, connection) = connect_require_tls(&endpoint, &fixture.login_pass, &fixture.ca)
        .await
        .expect("session connect");
    let tls = rustls_connector(&fixture.ca).expect("connector");
    DedicatedSession::bind(client, connection, tls)
        .await
        .expect("bind")
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
    panic!("tls connect failed: {last}");
}

async fn reset_database(client: &Client) {
    client
        .batch_execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE datname = 'oo_a189' AND pid <> pg_backend_pid()",
        )
        .await
        .expect("terminate");
    client
        .batch_execute("DROP DATABASE IF EXISTS oo_a189")
        .await
        .expect("drop database");
    client
        .batch_execute(
            "DO $$\n\
             BEGIN\n\
               IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_a189_login') THEN\n\
                 DROP ROLE oo_a189_login;\n\
               END IF;\n\
               IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_r_east') THEN\n\
                 DROP ROLE oo_r_east;\n\
               END IF;\n\
               IF EXISTS (SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_r_west') THEN\n\
                 DROP ROLE oo_r_west;\n\
               END IF;\n\
             END\n\
             $$",
        )
        .await
        .expect("drop roles");
    client
        .batch_execute("CREATE DATABASE oo_a189 TEMPLATE template0 ENCODING 'UTF8'")
        .await
        .expect("create database");
}

async fn install_fixture(client: &Client, login_pass: &str) {
    assert!(login_pass.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let sql = format!(
        "CREATE ROLE oo_a189_login LOGIN PASSWORD '{login_pass}' \
         NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;\n\
         CREATE ROLE oo_r_east NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;\n\
         CREATE ROLE oo_r_west NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;\n\
         GRANT oo_r_east TO oo_a189_login;\n\
         GRANT oo_r_west TO oo_a189_login;\n\
         CREATE SCHEMA depot;\n\
         REVOKE ALL ON SCHEMA depot FROM PUBLIC;\n\
         CREATE TABLE depot.holdrow (n integer);\n\
         INSERT INTO depot.holdrow (n) VALUES (7);\n\
         REVOKE ALL ON depot.holdrow FROM PUBLIC;\n\
         GRANT USAGE ON SCHEMA depot TO oo_r_east, oo_r_west;\n\
         GRANT SELECT (n) ON depot.holdrow TO oo_r_east, oo_r_west;\n\
         CREATE SCHEMA fixture;\n\
         REVOKE ALL ON SCHEMA fixture FROM PUBLIC;\n\
         CREATE FUNCTION fixture.page_hold() RETURNS integer\n\
         LANGUAGE plpgsql VOLATILE AS $body$\n\
         BEGIN\n\
           PERFORM pg_catalog.pg_sleep(30);\n\
           RETURN 1;\n\
         END\n\
         $body$;\n\
         REVOKE ALL ON FUNCTION fixture.page_hold() FROM PUBLIC;\n\
         GRANT USAGE ON SCHEMA fixture TO oo_r_east;\n\
         GRANT EXECUTE ON FUNCTION fixture.page_hold() TO oo_r_east"
    );
    client.batch_execute(&sql).await.expect("install fixture");
}

async fn type_oid(client: &Client, name: &str) -> u32 {
    let oid: i32 = client
        .query_one(
            "SELECT oid::int4 FROM pg_catalog.pg_type WHERE typname = $1",
            &[&name],
        )
        .await
        .expect("type oid")
        .get(0);
    u32::try_from(oid).expect("oid")
}

async fn function_oid(client: &Client) -> u32 {
    let oid: i32 = client
        .query_one(
            "SELECT p.oid::int4 FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'fixture' AND p.proname = 'page_hold'",
            &[],
        )
        .await
        .expect("function oid")
        .get(0);
    u32::try_from(oid).expect("oid")
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
        assert!(!flag, "role flag");
    }
}

async fn wait_blocked(client: &Client, pid: i32, needle: &str) -> (bool, String) {
    let mut last = "none".to_string();
    for _ in 0..60 {
        let (visible, sample) = activity_sample(client, pid, needle).await;
        last = sample;
        if visible {
            return (true, last);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    (false, last)
}

async fn activity_sample(client: &Client, pid: i32, needle: &str) -> (bool, String) {
    let rows = client
        .query(
            "SELECT COALESCE(state, 'none'), COALESCE(wait_event_type, 'none'), \
                    COALESCE(wait_event, 'none'), COALESCE(length(query), 0)::int4, \
                    CASE WHEN query IS NULL THEN false ELSE position($2 in query) > 0 END \
             FROM pg_catalog.pg_stat_activity WHERE pid = $1",
            &[&pid, &needle],
        )
        .await
        .expect("activity sample");
    if rows.len() != 1 {
        return (false, format!("rows={}", rows.len()));
    }
    let state: String = rows[0].get(0);
    let kind: String = rows[0].get(1);
    let event: String = rows[0].get(2);
    let query_len: i32 = rows[0].get(3);
    let needle_hit: bool = rows[0].get(4);
    let visible = needle_hit || (state == "active" && kind == "Lock");
    (
        visible,
        format!("state={state} wait={kind}/{event} query_len={query_len} needle={needle_hit}"),
    )
}

fn exec_variant(err: &ExecError) -> String {
    match err {
        ExecError::Guard(inner) => match inner.as_ref() {
            GuardError::Write => "write".to_string(),
            GuardError::Parse => "parse".to_string(),
            GuardError::FunctionNotAllowed => "function_not_allowed".to_string(),
            GuardError::RelationNotFound => "relation_not_found".to_string(),
            GuardError::Statement => "statement".to_string(),
            _ => "guard_other".to_string(),
        },
        ExecError::RoleReuse => "role_reuse".to_string(),
        ExecError::RoleParameter => "role_parameter".to_string(),
        ExecError::AuditUnavailable => "audit_unavailable".to_string(),
        ExecError::Limit => "limit".to_string(),
        ExecError::Timeout { .. } => "timeout".to_string(),
        ExecError::Canceled { .. } => "canceled".to_string(),
        ExecError::Database { .. } => "database".to_string(),
        ExecError::SessionClosed => "session_closed".to_string(),
        ExecError::SessionBusy => "session_busy".to_string(),
        ExecError::Protocol { step } => format!("protocol:{step}"),
        ExecError::TypeRefused { .. } => "type_refused".to_string(),
    }
}

fn stop_name(stop: Option<StopClass>) -> String {
    match stop {
        None => "none".to_string(),
        Some(StopClass::Timeout) => "timeout".to_string(),
        Some(StopClass::Canceled) => "canceled".to_string(),
        Some(StopClass::TypeRefused) => "type_refused".to_string(),
        Some(StopClass::Database) => "database".to_string(),
        Some(StopClass::Protocol) => "protocol".to_string(),
    }
}

fn read_secret(name: &str) -> String {
    let text = fs::read_to_string(Path::new(SECRET_DIR).join(name)).expect("secret file");
    text.trim().to_string()
}
