#![forbid(unsafe_code)]

//! Same-database coexistence phases. `OO_A365_PHASE` is required.
//! A bare run fails closed. This file does not open the a289 fixture.

use business_calendar::{resolve, sha256_hex, Closure, Input};
use calendar_store::{
    activate, install, migration_sha256, put_snapshot, read_view, ActivateRequest, Endpoint, InstallDisposition,
    InstallRequest, PutDisposition, PutRequest, ReadRequest, StoreError, CONTRACT,
};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error};

const FIXTURE: &str = "/run/oo-a365/fixture";
const PROJECT: &str = "project-one";
const RECORD: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ACCEPTED: &str = "ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520";
const EXAMPLE: &str = include_str!("../../../examples/schema/calendar.yaml");

struct Fix {
    host: String,
    port: u16,
    database: String,
    ca: Vec<u8>,
    admin: String,
    reader: String,
    other: String,
    fixture_admin: String,
}

fn load_fix() -> Fix {
    let root = std::path::Path::new(FIXTURE);
    let endpoint = std::fs::read_to_string(root.join("endpoint.txt")).expect("endpoint");
    let mut lines = endpoint.lines();
    Fix {
        host: lines.next().expect("host").to_owned(),
        port: lines.next().expect("port").parse().expect("port"),
        database: lines.next().expect("database").to_owned(),
        ca: std::fs::read(root.join("ca.crt")).expect("ca"),
        admin: std::fs::read_to_string(root.join("admin.pass")).expect("admin").trim().to_owned(),
        reader: std::fs::read_to_string(root.join("reader.pass")).expect("reader").trim().to_owned(),
        other: std::fs::read_to_string(root.join("other.pass")).expect("other").trim().to_owned(),
        fixture_admin: std::fs::read_to_string(root.join("fixture-admin.pass")).expect("fixture").trim().to_owned(),
    }
}

fn endpoint_of(fix: &Fix) -> Endpoint {
    Endpoint { host: fix.host.clone(), port: fix.port, database: fix.database.clone() }
}

fn closure() -> Closure<'static> {
    Closure { last_closed_period_end: "2026-03-31", label: "close-2026-03", record_sha256: RECORD }
}

fn phase_database(phase: &str) -> &'static str {
    match phase {
        "solo" | "solo_replay" => "oo_a365_solo",
        "cal_first" | "cal_first_replay" | "drift" => "oo_a365_cal_first",
        "enum_first" | "enum_first_replay" => "oo_a365_enum_first",
        "hostile" => "oo_a365_hostile",
        other => panic!("unknown phase {other}"),
    }
}

async fn connect_as(fix: &Fix, user: &str, password: &str) -> Client {
    let tls = calendar_store::rustls_connector(&fix.ca).expect("connector");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&fix.host);
    config.port(fix.port);
    config.dbname(&fix.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("calendar-store-coexist");
    let (client, connection) = config.connect(tls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

fn sqlstate(err: &Error) -> Option<&str> {
    err.code().map(|code| code.code())
}

async fn must_42501(client: &Client, sql: &str) {
    let err = client.query(sql, &[]).await.expect_err(sql);
    assert_eq!(sqlstate(&err), Some("42501"), "{sql}");
}

async fn schema_exists(client: &Client, name: &str) -> bool {
    client
        .query_opt("SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1", &[&name])
        .await
        .expect("schema")
        .is_some()
}

async fn dict_owner(client: &Client) -> String {
    client
        .query_one(
            "SELECT pg_catalog.pg_get_userbyid(nspowner) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_dict'",
            &[],
        )
        .await
        .expect("owner")
        .get(0)
}

fn install_at<'a>(endpoint: &'a Endpoint, fix: &'a Fix) -> InstallRequest<'a> {
    InstallRequest {
        endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_calendar"],
    }
}

async fn operate(fix: &Fix, endpoint: &Endpoint, owner: &str) {
    let hash = migration_sha256().expect("hash");
    assert_eq!(hash, ACCEPTED);
    let as_of = "2026-04-01T03:30:00Z";
    let resolved = {
        let report = resolve(&Input { calendar_yaml: EXAMPLE, as_of, closure: Some(closure()) }).expect("resolve");
        sha256_hex(serde_json::to_string(&report).expect("json").as_bytes())
    };
    let installed = install(&install_at(endpoint, fix)).await.expect("install");
    assert_eq!(installed.disposition, InstallDisposition::Installed);
    assert_eq!(installed.contract, CONTRACT);
    assert_eq!(installed.sql_sha256, hash);
    assert!(installed.database_used);
    assert!(!installed.finance_approval && !installed.authenticated && !installed.ontology_published && !installed.clock_used);
    let replay = install(&install_at(endpoint, fix)).await.expect("replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);
    let fixture = connect_as(fix, "oo_fixture_admin", &fix.fixture_admin).await;
    assert_eq!(dict_owner(&fixture).await, owner);
    assert!(schema_exists(&fixture, "oo_calendar_control").await);
    let functions: i64 = fixture
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'oo_calendar_control' AND p.proname IN ('dict_create_open', 'dict_create_close') AND p.prosecdef",
            &[],
        )
        .await
        .expect("functions")
        .get(0);
    assert_eq!(functions, 2);
    let create: bool = fixture
        .query_one("SELECT pg_catalog.has_schema_privilege('oo_calendar_view', 'oo_dict', 'CREATE')", &[])
        .await
        .expect("create")
        .get(0);
    assert!(!create);
    let reader = connect_as(fix, "oo_r_calendar", &fix.reader).await;
    must_42501(&reader, "SELECT project_id FROM oo_dict.calendar").await;
    let early = read_view(&ReadRequest { endpoint, user: "oo_r_calendar", password: &fix.reader, ca_pem: &fix.ca })
        .await
        .expect_err("read before activate");
    assert_eq!(early, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    let other = connect_as(fix, "oo_r_other", &fix.other).await;
    must_42501(&other, "SELECT project_id FROM oo_dict.calendar").await;
    let put = put_snapshot(&PutRequest {
        endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        calendar_version: "fy2026",
        calendar_yaml: EXAMPLE,
        as_of,
        closure: Some(closure()),
        expected_resolved_sha256: &resolved,
    })
    .await
    .expect("put");
    assert_eq!(put.disposition, PutDisposition::Inserted);
    assert_eq!(put.resolved_sha256, resolved);
    let activated = activate(&ActivateRequest {
        endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        expected_current: None,
        snapshot_id: &put.snapshot_id,
        config_sha256: &put.config_sha256,
        calendar_version: "fy2026",
        as_of: &put.as_of,
    })
    .await
    .expect("activate");
    assert_eq!(activated.disposition, "activated");
    assert_eq!(activated.grants, 1);
    let view = read_view(&ReadRequest { endpoint, user: "oo_r_calendar", password: &fix.reader, ca_pem: &fix.ca })
        .await
        .expect("read");
    assert_eq!(view.contract, CONTRACT);
    assert_eq!(view.project_id, PROJECT);
    assert_eq!(view.business_tz, "Asia/Shanghai");
    assert_eq!(view.as_of_utc, as_of);
    assert_eq!(view.calendar_version, "fy2026");
    assert!(view.database_used);
    assert!(!view.finance_approval && !view.authenticated && !view.ontology_published && !view.clock_used);
    assert_eq!(view.anchors.len(), 10);
    let today = view.anchors.iter().find(|item| item.name == "today").expect("today");
    assert_eq!(today.date.as_deref(), Some("2026-04-01"));
    assert_eq!(today.midnight_status, "unique");
}

async fn replay_only(fix: &Fix, endpoint: &Endpoint, owner: &str) {
    let replay = install(&install_at(endpoint, fix)).await.expect("shared replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);
    assert_eq!(replay.sql_sha256, ACCEPTED);
    let view = read_view(&ReadRequest { endpoint, user: "oo_r_calendar", password: &fix.reader, ca_pem: &fix.ca })
        .await
        .expect("read after sibling");
    assert_eq!(view.project_id, PROJECT);
    assert_eq!(view.calendar_version, "fy2026");
    let fixture = connect_as(fix, "oo_fixture_admin", &fix.fixture_admin).await;
    assert_eq!(dict_owner(&fixture).await, owner);
    let grant: bool = fixture
        .query_one(
            "SELECT pg_catalog.has_function_privilege('oo_enum_admin', 'oo_calendar_control.dict_create_open()', 'EXECUTE')",
            &[],
        )
        .await
        .expect("grant")
        .get(0);
    assert!(grant);
}

async fn refuse(fix: &Fix, endpoint: &Endpoint, setup: &str, remain: &str) {
    let fixture = connect_as(fix, "oo_fixture_admin", &fix.fixture_admin).await;
    fixture.batch_execute(setup).await.expect("hostile setup");
    let err = install(&install_at(endpoint, fix)).await.expect_err("hostile install");
    assert_eq!(err, StoreError::Collision { check: "oo_dict" });
    assert!(!schema_exists(&fixture, "oo_calendar_control").await);
    assert!(schema_exists(&fixture, "oo_dict").await);
    let kept: i64 = fixture.query_one(remain, &[]).await.expect("remain").get(0);
    assert_eq!(kept, 1);
    fixture.batch_execute("DROP SCHEMA oo_dict CASCADE").await.expect("reset hostile");
}

#[tokio::test]
async fn phase() {
    let phase = std::env::var("OO_A365_PHASE").expect("OO_A365_PHASE is required");
    let fix = load_fix();
    assert_eq!(fix.host, "oo-a365-pg");
    assert_eq!(fix.database, phase_database(&phase));
    let endpoint = endpoint_of(&fix);
    match phase.as_str() {
        "solo" => {
            operate(&fix, &endpoint, "oo_calendar_admin").await;
            let fixture = connect_as(&fix, "oo_fixture_admin", &fix.fixture_admin).await;
            let enum_role: bool = fixture
                .query_opt("SELECT 1 FROM pg_catalog.pg_roles WHERE rolname = 'oo_enum_admin'", &[])
                .await
                .expect("enum role")
                .is_some();
            assert!(!enum_role);
        }
        "solo_replay" => {
            let replay = install(&install_at(&endpoint, &fix)).await.expect("late role replay");
            assert_eq!(replay.disposition, InstallDisposition::Replay);
            let fixture = connect_as(&fix, "oo_fixture_admin", &fix.fixture_admin).await;
            let grant: bool = fixture
                .query_one(
                    "SELECT pg_catalog.has_function_privilege('oo_enum_admin', 'oo_calendar_control.dict_create_open()', 'EXECUTE')",
                    &[],
                )
                .await
                .expect("late grant")
                .get(0);
            assert!(!grant);
            assert!(!schema_exists(&fixture, "oo_enum_control").await);
        }
        "cal_first" => operate(&fix, &endpoint, "oo_calendar_admin").await,
        "cal_first_replay" => replay_only(&fix, &endpoint, "oo_calendar_admin").await,
        "enum_first" => operate(&fix, &endpoint, "oo_enum_admin").await,
        "enum_first_replay" => replay_only(&fix, &endpoint, "oo_enum_admin").await,
        "hostile" => {
            refuse(
                &fix,
                &endpoint,
                "CREATE SCHEMA oo_dict AUTHORIZATION oo_fixture_admin; CREATE TABLE oo_dict.kept (n integer); REVOKE ALL ON SCHEMA oo_dict FROM PUBLIC",
                "SELECT count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname = 'kept'",
            )
            .await;
            refuse(
                &fix,
                &endpoint,
                "CREATE SCHEMA oo_dict AUTHORIZATION oo_enum_admin; REVOKE ALL ON SCHEMA oo_dict FROM PUBLIC",
                "SELECT count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_dict' AND pg_catalog.pg_get_userbyid(nspowner) = 'oo_enum_admin'",
            )
            .await;
            refuse(
                &fix,
                &endpoint,
                "CREATE SCHEMA oo_dict AUTHORIZATION oo_enum_admin; CREATE TABLE oo_dict.enums (n integer); REVOKE ALL ON SCHEMA oo_dict FROM PUBLIC",
                "SELECT count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname = 'enums' AND c.relkind = 'r'",
            )
            .await;
        }
        "drift" => {
            let fixture = connect_as(&fix, "oo_fixture_admin", &fix.fixture_admin).await;
            let before: i64 = fixture.query_one("SELECT count(*) FROM oo_calendar_control.snapshot", &[]).await.expect("before").get(0);
            fixture
                .batch_execute("ALTER VIEW oo_dict.enums SET (security_invoker = true)")
                .await
                .expect("tamper");
            let err = install(&install_at(&endpoint, &fix)).await.expect_err("drift");
            assert_eq!(err, StoreError::InstallDrift { check: "dict_objects" });
            let options: String = fixture
                .query_one(
                    "SELECT COALESCE(c.reloptions::text, '') FROM pg_catalog.pg_class c \
                     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = 'oo_dict' AND c.relname = 'enums'",
                    &[],
                )
                .await
                .expect("options")
                .get(0);
            assert!(options.contains("security_invoker=true"), "{options}");
            let after: i64 = fixture.query_one("SELECT count(*) FROM oo_calendar_control.snapshot", &[]).await.expect("after").get(0);
            assert_eq!(after, before);
            assert!(schema_exists(&fixture, "oo_calendar_control").await);
            assert!(schema_exists(&fixture, "oo_enum_control").await);
        }
        other => panic!("unknown phase {other}"),
    }
}
