#![forbid(unsafe_code)]

//! Exact `search_path` on one shared database. `OO_A378_PHASE` is required.
//! This file does not run the two-order matrix.

use calendar_store::{install, migration_sha256, Endpoint, InstallDisposition, InstallRequest, StoreError};
use tokio_postgres::config::SslMode;
use tokio_postgres::Client;

const FIXTURE: &str = "/run/oo-a378/fixture";
const PROJECT: &str = "project-one";
const SCHEMA: &str = "oo_calendar_control";
const PIN: &str = "search_path=pg_catalog";

struct Fix {
    host: String,
    port: u16,
    database: String,
    ca: Vec<u8>,
    admin: String,
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
        fixture_admin: std::fs::read_to_string(root.join("fixture-admin.pass")).expect("fixture").trim().to_owned(),
    }
}

fn endpoint_of(fix: &Fix) -> Endpoint {
    Endpoint { host: fix.host.clone(), port: fix.port, database: fix.database.clone() }
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

async fn connect_as(fix: &Fix, user: &str, password: &str) -> Client {
    let tls = calendar_store::rustls_connector(&fix.ca).expect("connector");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&fix.host);
    config.port(fix.port);
    config.dbname(&fix.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("calendar-store-search-path");
    let (client, connection) = config.connect(tls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn proconfig(client: &Client, schema: &str, name: &str, args: &str) -> Option<Vec<String>> {
    let row = client
        .query_one(
            "SELECT p.proconfig FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = $1 AND p.proname = $2 \
               AND pg_catalog.pg_get_function_identity_arguments(p.oid) = $3",
            &[&schema, &name, &args],
        )
        .await
        .expect("proconfig");
    row.get(0)
}

async fn schema_exists(client: &Client, name: &str) -> bool {
    client
        .query_opt("SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1", &[&name])
        .await
        .expect("schema")
        .is_some()
}

async fn fingerprint(client: &Client) -> String {
    let body: String = client
        .query_one(
            "SELECT coalesce(string_agg(line, E'\\n' ORDER BY line), '') FROM ( \
               SELECT n.nspname || '|acl|' || coalesce(n.nspacl::text, '') AS line \
               FROM pg_catalog.pg_namespace n \
               WHERE n.nspname IN ('oo_dict', 'oo_calendar_control', 'oo_enum_control') \
               UNION ALL \
               SELECT r.rolname || '|' || p.priv || '|' || pg_catalog.has_schema_privilege(r.rolname, 'oo_dict', p.priv)::text \
               FROM (VALUES ('oo_calendar_admin'), ('oo_enum_admin'), ('oo_calendar_view'), ('oo_enum_view'), ('oo_r_calendar'), ('oo_r_enum')) AS r(rolname) \
               CROSS JOIN (VALUES ('USAGE'), ('CREATE')) AS p(priv) \
               UNION ALL \
               SELECT n.nspname || '.' || c.relname || '|rel|' || coalesce(c.relacl::text, '') || '|' || pg_catalog.pg_get_userbyid(c.relowner) \
               FROM pg_catalog.pg_class c \
               JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
               WHERE n.nspname IN ('oo_dict', 'oo_calendar_control', 'oo_enum_control') AND c.relkind IN ('r', 'v', 'S') \
               UNION ALL \
               SELECT n.nspname || '.' || p.proname || '(' || pg_catalog.pg_get_function_identity_arguments(p.oid) || ')|pro|' || coalesce(p.proconfig::text, '') || '|' || coalesce(p.proacl::text, '') \
               FROM pg_catalog.pg_proc p \
               JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
               WHERE n.nspname IN ('oo_calendar_control', 'oo_enum_control') \
                 AND p.proname IN ('dict_create_open', 'dict_create_close', 'dict_grant_runtime') \
             ) s",
            &[],
        )
        .await
        .expect("fingerprint")
        .get(0);
    let mut counts = String::new();
    for (schema, tables) in [
        ("oo_calendar_control", ["install_manifest", "deployment", "runtime_allow", "snapshot", "published", "active_pointer"].as_slice()),
        ("oo_enum_control", ["install_manifest", "deployment", "runtime_allow", "snapshot", "notice", "published", "served", "active_pointer"].as_slice()),
    ] {
        if !schema_exists(client, schema).await {
            counts.push_str(schema);
            counts.push_str("|absent\n");
            continue;
        }
        for table in tables {
            let sql = format!("SELECT count(*) FROM {schema}.{table}");
            let count: i64 = client.query_one(&sql, &[]).await.expect(table).get(0);
            counts.push_str(&format!("{schema}.{table}|{count}\n"));
        }
    }
    format!("{body}\n{counts}")
}

async fn assert_pinned(client: &Client, schema: &str) {
    for (name, args) in [("dict_create_open", ""), ("dict_create_close", ""), ("dict_grant_runtime", "runtime_role text")] {
        let config = proconfig(client, schema, name, args).await;
        assert_eq!(config, Some(vec![PIN.to_owned()]), "{schema}.{name}");
    }
}

async fn exec(client: &Client, sql: &str) {
    client.batch_execute(sql).await.expect("exec");
}

fn alter_search_path(schema: &str, function: &str, value: &str) -> String {
    format!("ALTER FUNCTION {schema}.{function} SET search_path = {value}")
}

async fn restore_pinned(client: &Client, schema: &str) {
    for function in ["dict_create_open()", "dict_create_close()", "dict_grant_runtime(text)"] {
        exec(client, &format!("ALTER FUNCTION {schema}.{function} RESET ALL")).await;
        exec(client, &alter_search_path(schema, function, "pg_catalog")).await;
    }
}

async fn expect_install_drift(fix: &Fix, endpoint: &Endpoint, admin: &Client) {
    let before = fingerprint(admin).await;
    let err = install(&install_at(endpoint, fix)).await.expect_err("drift");
    assert_eq!(err, StoreError::InstallDrift { check: "adopt_function" });
    let after = fingerprint(admin).await;
    assert_eq!(before, after, "install_drift mutated the catalog");
    println!("RESULT install_drift adopt_function mutated=false");
}

async fn expect_replay(fix: &Fix, endpoint: &Endpoint) {
    let replay = install(&install_at(endpoint, fix)).await.expect("replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);
    println!("RESULT replay");
}

#[tokio::test]
async fn phase() {
    let phase = std::env::var("OO_A378_PHASE").expect("OO_A378_PHASE");
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let admin = connect_as(&fix, "oo_fixture_admin", &fix.fixture_admin).await;
    match phase.as_str() {
        "baseline" => {
            assert_eq!(migration_sha256().expect("hash"), "ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520");
            let installed = install(&install_at(&endpoint, &fix)).await.expect("install");
            assert_eq!(installed.disposition, InstallDisposition::Installed);
            expect_replay(&fix, &endpoint).await;
            assert_pinned(&admin, SCHEMA).await;
            assert!(schema_exists(&admin, "oo_calendar_control").await);
            assert!(schema_exists(&admin, "oo_dict").await);
            assert!(!schema_exists(&admin, "oo_enum_control").await);
            let snaps: i64 = admin.query_one("SELECT count(*) FROM oo_calendar_control.snapshot", &[]).await.expect("snap").get(0);
            assert_eq!(snaps, 0);
            println!("RESULT baseline proconfig={PIN}");
        }
        "reject_widened" => {
            assert_pinned(&admin, SCHEMA).await;
            let cases = [
                ("dict_create_open()", "pg_catalog, public", "search_path=pg_catalog, public"),
                ("dict_create_close()", "pg_catalog_extra", "search_path=pg_catalog_extra"),
                ("dict_grant_runtime(text)", "pg_catalog, pg_temp", "search_path=pg_catalog, pg_temp"),
            ];
            for (function, value, element) in cases {
                exec(&admin, &alter_search_path(SCHEMA, function, value)).await;
                let (name, args) = if function.starts_with("dict_grant") {
                    ("dict_grant_runtime", "runtime_role text")
                } else if function.starts_with("dict_create_close") {
                    ("dict_create_close", "")
                } else {
                    ("dict_create_open", "")
                };
                let config = proconfig(&admin, SCHEMA, name, args).await;
                assert_eq!(config, Some(vec![element.to_owned()]), "{function}");
                println!("RESULT widened {function} proconfig={config:?}");
                expect_install_drift(&fix, &endpoint, &admin).await;
                restore_pinned(&admin, SCHEMA).await;
                assert_pinned(&admin, SCHEMA).await;
                expect_replay(&fix, &endpoint).await;
            }
            exec(&admin, "ALTER FUNCTION oo_calendar_control.dict_create_open() SET work_mem = '4MB'").await;
            let extra = proconfig(&admin, SCHEMA, "dict_create_open", "").await.expect("work_mem config");
            assert!(extra.iter().any(|item| item == PIN), "{extra:?}");
            assert!(extra.len() > 1, "{extra:?}");
            println!("RESULT unexpected proconfig={extra:?}");
            expect_install_drift(&fix, &endpoint, &admin).await;
            restore_pinned(&admin, SCHEMA).await;
            assert_pinned(&admin, SCHEMA).await;
            expect_replay(&fix, &endpoint).await;
            exec(&admin, &alter_search_path(SCHEMA, "dict_create_open()", "pg_catalog, public")).await;
            exec(&admin, &alter_search_path(SCHEMA, "dict_create_close()", "pg_temp, pg_catalog")).await;
            exec(&admin, &alter_search_path(SCHEMA, "dict_grant_runtime(text)", "pg_catalog, pg_temp")).await;
            println!("RESULT leave_widened calendar");
        }
        "restore_replay" => {
            restore_pinned(&admin, SCHEMA).await;
            assert_pinned(&admin, SCHEMA).await;
            expect_replay(&fix, &endpoint).await;
            assert!(!schema_exists(&admin, "oo_enum_control").await);
            println!("RESULT restore_replay");
        }
        "sibling_drift" => {
            let enum_open = proconfig(&admin, "oo_enum_control", "dict_create_open", "").await;
            println!("RESULT sibling_open={enum_open:?}");
            expect_install_drift(&fix, &endpoint, &admin).await;
            assert_pinned(&admin, SCHEMA).await;
            assert!(schema_exists(&admin, "oo_enum_control").await);
        }
        "sibling_replay" => {
            assert_pinned(&admin, SCHEMA).await;
            assert_pinned(&admin, "oo_enum_control").await;
            expect_replay(&fix, &endpoint).await;
            println!("RESULT sibling_replay");
        }
        other => panic!("unknown phase {other}"),
    }
}
