#![forbid(unsafe_code)]

//! Same-database coexistence phases. `OO_A365_PHASE` is required.
//! A bare run fails closed. This file does not open the a293 fixture.

use enum_store::{
    activate, install, migration_sha256, put_snapshot, read_view, ActivateRequest, Endpoint, InstallDisposition,
    InstallRequest, PutDisposition, PutRequest, ReadRequest, StoreError, CONTRACT,
};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error};

const FIXTURE: &str = "/run/oo-a365/fixture";
const PROJECT: &str = "project-one";
const HISTORICAL_SHA: &str = "63fb59e7431cd131c512c3bfa1c61987c717d13bfe902552925d2b782649d9f6";
const ACCEPTED: &str = "b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd";
const HIST: &str = include_str!("../../enum-history/tests/fixtures/deprecated-historical.yaml");

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
        admin: std::fs::read_to_string(root.join("enum-admin.pass")).expect("admin").trim().to_owned(),
        reader: std::fs::read_to_string(root.join("enum-reader.pass")).expect("reader").trim().to_owned(),
        other: std::fs::read_to_string(root.join("other.pass")).expect("other").trim().to_owned(),
        fixture_admin: std::fs::read_to_string(root.join("fixture-admin.pass")).expect("fixture").trim().to_owned(),
    }
}

fn endpoint_of(fix: &Fix) -> Endpoint {
    Endpoint { host: fix.host.clone(), port: fix.port, database: fix.database.clone() }
}

fn phase_database(phase: &str) -> &'static str {
    match phase {
        "solo_reject" => "oo_a365_solo",
        "cal_first" | "cal_first_replay" => "oo_a365_cal_first",
        "enum_first" | "enum_first_replay" | "drift" => "oo_a365_enum_first",
        "hostile" => "oo_a365_hostile",
        other => panic!("unknown phase {other}"),
    }
}

async fn connect_as(fix: &Fix, user: &str, password: &str) -> Client {
    let tls = enum_store::rustls_connector(&fix.ca).expect("connector");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&fix.host);
    config.port(fix.port);
    config.dbname(&fix.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("enum-store-coexist");
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
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_enum"],
    }
}

async fn operate(fix: &Fix, endpoint: &Endpoint, owner: &str) {
    let hash = migration_sha256().expect("hash");
    assert_eq!(hash, ACCEPTED);
    let observations = [enum_history::Observation { code: "4".to_owned(), count: 3 }];
    let installed = install(&install_at(endpoint, fix)).await.expect("install");
    assert_eq!(installed.disposition, InstallDisposition::Installed);
    assert_eq!(installed.contract, CONTRACT);
    assert_eq!(installed.sql_sha256, hash);
    assert!(installed.database_used);
    assert!(
        !installed.finance_certified
            && !installed.semantic_certified
            && !installed.owner_authenticated
            && !installed.metric_certified
            && !installed.clock_used
    );
    let replay = install(&install_at(endpoint, fix)).await.expect("replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);
    let fixture = connect_as(fix, "oo_fixture_admin", &fix.fixture_admin).await;
    assert_eq!(dict_owner(&fixture).await, owner);
    assert!(schema_exists(&fixture, "oo_enum_control").await);
    let functions: i64 = fixture
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = 'oo_enum_control' AND p.proname IN ('dict_create_open', 'dict_create_close') AND p.prosecdef",
            &[],
        )
        .await
        .expect("functions")
        .get(0);
    assert_eq!(functions, 2);
    let create: bool = fixture
        .query_one("SELECT pg_catalog.has_schema_privilege('oo_enum_view', 'oo_dict', 'CREATE')", &[])
        .await
        .expect("create")
        .get(0);
    assert!(!create);
    let reader = connect_as(fix, "oo_r_enum", &fix.reader).await;
    must_42501(&reader, "SELECT project_id FROM oo_dict.enums").await;
    let early = read_view(&ReadRequest { endpoint, user: "oo_r_enum", password: &fix.reader, ca_pem: &fix.ca })
        .await
        .expect_err("read before activate");
    assert_eq!(early, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    let other = connect_as(fix, "oo_r_other", &fix.other).await;
    must_42501(&other, "SELECT project_id FROM oo_dict.enums").await;
    let put = put_snapshot(&PutRequest {
        endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        enum_version: "hist-2020",
        enum_yaml: HIST,
        as_of: "2020-01-01",
        observations: &observations,
        threshold: None,
        expected_report_sha256: HISTORICAL_SHA,
    })
    .await
    .expect("put");
    assert_eq!(put.disposition, PutDisposition::Inserted);
    assert_eq!(put.report_sha256, HISTORICAL_SHA);
    let activated = activate(&ActivateRequest {
        endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        expected_current: None,
        snapshot_id: &put.snapshot_id,
        config_sha256: &put.config_sha256,
        enum_version: "hist-2020",
        as_of: &put.as_of,
    })
    .await
    .expect("activate");
    assert_eq!(activated.disposition, "activated");
    assert_eq!(activated.grants, 1);
    let view = read_view(&ReadRequest { endpoint, user: "oo_r_enum", password: &fix.reader, ca_pem: &fix.ca })
        .await
        .expect("read");
    assert_eq!(view.contract, CONTRACT);
    assert!(view.database_used);
    assert!(!view.finance_certified && !view.semantic_certified && !view.owner_authenticated && !view.metric_certified && !view.clock_used);
    assert_eq!(view.rows.len(), 1);
    assert_eq!(view.rows[0].code, "4");
    assert_eq!(view.rows[0].status, "deprecated");
    assert_eq!(view.rows[0].project_id, PROJECT);
    assert_eq!(view.rows[0].as_of, "2020-01-01");
}

async fn replay_only(fix: &Fix, endpoint: &Endpoint, owner: &str) {
    let replay = install(&install_at(endpoint, fix)).await.expect("shared replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);
    assert_eq!(replay.sql_sha256, ACCEPTED);
    let view = read_view(&ReadRequest { endpoint, user: "oo_r_enum", password: &fix.reader, ca_pem: &fix.ca })
        .await
        .expect("read after sibling");
    assert_eq!(view.rows.len(), 1);
    assert_eq!(view.rows[0].code, "4");
    let fixture = connect_as(fix, "oo_fixture_admin", &fix.fixture_admin).await;
    assert_eq!(dict_owner(&fixture).await, owner);
    let grant: bool = fixture
        .query_one(
            "SELECT pg_catalog.has_function_privilege('oo_calendar_admin', 'oo_enum_control.dict_create_open()', 'EXECUTE')",
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
    assert!(!schema_exists(&fixture, "oo_enum_control").await);
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
        "solo_reject" => {
            let err = install(&install_at(&endpoint, &fix)).await.expect_err("late sibling");
            assert_eq!(err, StoreError::Collision { check: "oo_dict" });
            let fixture = connect_as(&fix, "oo_fixture_admin", &fix.fixture_admin).await;
            assert!(!schema_exists(&fixture, "oo_enum_control").await);
            assert!(schema_exists(&fixture, "oo_calendar_control").await);
            assert!(schema_exists(&fixture, "oo_dict").await);
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
                "CREATE SCHEMA oo_dict AUTHORIZATION oo_calendar_admin; REVOKE ALL ON SCHEMA oo_dict FROM PUBLIC",
                "SELECT count(*) FROM pg_catalog.pg_namespace WHERE nspname = 'oo_dict' AND pg_catalog.pg_get_userbyid(nspowner) = 'oo_calendar_admin'",
            )
            .await;
            refuse(
                &fix,
                &endpoint,
                "CREATE SCHEMA oo_dict AUTHORIZATION oo_calendar_admin; CREATE TABLE oo_dict.calendar (n integer); REVOKE ALL ON SCHEMA oo_dict FROM PUBLIC",
                "SELECT count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar' AND c.relkind = 'r'",
            )
            .await;
        }
        "drift" => {
            let fixture = connect_as(&fix, "oo_fixture_admin", &fix.fixture_admin).await;
            let before: i64 = fixture.query_one("SELECT count(*) FROM oo_enum_control.snapshot", &[]).await.expect("before").get(0);
            fixture
                .batch_execute("ALTER VIEW oo_dict.calendar SET (security_invoker = true)")
                .await
                .expect("tamper");
            let err = install(&install_at(&endpoint, &fix)).await.expect_err("drift");
            assert_eq!(err, StoreError::InstallDrift { check: "dict_objects" });
            let options: String = fixture
                .query_one(
                    "SELECT COALESCE(c.reloptions::text, '') FROM pg_catalog.pg_class c \
                     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'",
                    &[],
                )
                .await
                .expect("options")
                .get(0);
            assert!(options.contains("security_invoker=true"), "{options}");
            let after: i64 = fixture.query_one("SELECT count(*) FROM oo_enum_control.snapshot", &[]).await.expect("after").get(0);
            assert_eq!(after, before);
            assert!(schema_exists(&fixture, "oo_enum_control").await);
            assert!(schema_exists(&fixture, "oo_calendar_control").await);
        }
        other => panic!("unknown phase {other}"),
    }
}
