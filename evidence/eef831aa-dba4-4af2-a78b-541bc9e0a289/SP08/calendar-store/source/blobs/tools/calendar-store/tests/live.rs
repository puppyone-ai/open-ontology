#![forbid(unsafe_code)]

//! PostgreSQL proof. UTC instants below are handwritten. They are not copied from `resolve`.

use business_calendar::{resolve, sha256_hex, Closure, Input, Midnight};
use calendar_store::{
    activate, install, migration_sha256, put_snapshot, read_view, ActivateReceipt, ActivateRequest, Endpoint,
    InstallDisposition, InstallRequest, PutDisposition, PutReceipt, PutRequest, ReadReceipt, ReadRequest, StoreError,
    CONTRACT,
};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error};

const FIXTURE: &str = "/run/oo-a289/fixture";
const PROJECT: &str = "project-one";
const RECORD: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EXAMPLE_HASH: &str = "2ad3845ec287c24391168ce1e7271d743202b0b2d5cef7d6ad05537e02fcc378";
const EXAMPLE: &str = include_str!("../../../examples/schema/calendar.yaml");
static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fix {
    host: String,
    port: u16,
    database: String,
    ca: Vec<u8>,
    wrong_ca: Vec<u8>,
    admin: String,
    reader: String,
    other: String,
    fixture_admin: String,
}

struct Hand {
    name: &'static str,
    date: Option<&'static str>,
    status: &'static str,
    midnight_utc: Option<&'static str>,
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
        wrong_ca: std::fs::read(root.join("wrong.crt")).expect("wrong ca"),
        admin: std::fs::read_to_string(root.join("admin.pass")).expect("admin").trim().to_owned(),
        reader: std::fs::read_to_string(root.join("reader.pass")).expect("reader").trim().to_owned(),
        other: std::fs::read_to_string(root.join("other.pass")).expect("other").trim().to_owned(),
        fixture_admin: std::fs::read_to_string(root.join("fixture-admin.pass"))
            .expect("fixture admin")
            .trim()
            .to_owned(),
    }
}

fn endpoint_of(fix: &Fix) -> Endpoint {
    Endpoint {
        host: fix.host.clone(),
        port: fix.port,
        database: fix.database.clone(),
    }
}

fn resolved_hash(yaml: &str, as_of: &str, closure: Option<Closure<'_>>) -> String {
    let report = resolve(&Input {
        calendar_yaml: yaml,
        as_of,
        closure,
    })
    .expect("helper resolve");
    sha256_hex(serde_json::to_string(&report).expect("helper json").as_bytes())
}

fn assert_helper(yaml: &str, as_of: &str, closure: Option<Closure<'_>>, rows: &[Hand]) {
    let report = resolve(&Input {
        calendar_yaml: yaml,
        as_of,
        closure,
    })
    .expect("helper resolve");
    assert_eq!(report.as_of, as_of);
    assert!(!report.finance_approval && !report.authenticated && !report.database_used && !report.clock_used);
    for row in rows {
        let found = report.anchors.iter().find(|item| item.name == row.name);
        match (row.date, row.status, found) {
            (None, "absent", None) => {}
            (Some(date), "unique", Some(item)) => {
                assert_eq!(item.civil, date, "{}", row.name);
                assert!(matches!(item.midnight, Midnight::Unique { .. }), "{}", row.name);
            }
            (Some(date), "nonexistent", Some(item)) => {
                assert_eq!(item.civil, date, "{}", row.name);
                assert_eq!(item.midnight, Midnight::Nonexistent, "{}", row.name);
            }
            _ => panic!("helper disagreement on {}", row.name),
        }
    }
}

fn assert_view(view: &ReadReceipt, tz: &str, as_of: &str, version: &str, rows: &[Hand]) {
    assert_eq!(view.contract, CONTRACT);
    assert_eq!(view.project_id, PROJECT);
    assert_eq!(view.business_tz, tz);
    assert_eq!(view.as_of_utc, as_of);
    assert_eq!(view.calendar_version, version);
    assert!(view.database_used);
    assert!(!view.finance_approval && !view.authenticated && !view.ontology_published && !view.clock_used);
    assert_eq!(view.anchors.len(), rows.len());
    for row in rows {
        let column = view
            .anchors
            .iter()
            .find(|item| item.name == row.name)
            .unwrap_or_else(|| panic!("missing {}", row.name));
        assert_eq!(column.date.as_deref(), row.date, "{}", row.name);
        assert_eq!(column.midnight_status, row.status, "{}", row.name);
        assert_eq!(column.midnight_utc.as_deref(), row.midnight_utc, "{}", row.name);
    }
}

fn calendar(tz: &str) -> String {
    format!(
        "schema_version: oos/v1\nbusiness_tz: {tz}\nfiscal_year_start_month: 4\nweek_start: monday\nanchors:\n  - today\n  - month_start\n  - prev_month_start\n  - prev_month_end\n  - quarter_start\n  - prev_quarter_start\n  - prev_quarter_end\n  - fiscal_year_start\n  - prev_fiscal_year_end\n"
    )
}

fn april_rows() -> &'static [Hand] {
    const ROWS: [Hand; 10] = [
        Hand { name: "today", date: Some("2026-04-01"), status: "unique", midnight_utc: Some("2026-03-31T16:00:00Z") },
        Hand { name: "month_start", date: Some("2026-04-01"), status: "unique", midnight_utc: Some("2026-03-31T16:00:00Z") },
        Hand { name: "prev_month_start", date: Some("2026-03-01"), status: "unique", midnight_utc: Some("2026-02-28T16:00:00Z") },
        Hand { name: "prev_month_end", date: Some("2026-03-31"), status: "unique", midnight_utc: Some("2026-03-30T16:00:00Z") },
        Hand { name: "quarter_start", date: Some("2026-04-01"), status: "unique", midnight_utc: Some("2026-03-31T16:00:00Z") },
        Hand { name: "prev_quarter_start", date: Some("2026-01-01"), status: "unique", midnight_utc: Some("2025-12-31T16:00:00Z") },
        Hand { name: "prev_quarter_end", date: Some("2026-03-31"), status: "unique", midnight_utc: Some("2026-03-30T16:00:00Z") },
        Hand { name: "fiscal_year_start", date: Some("2026-04-01"), status: "unique", midnight_utc: Some("2026-03-31T16:00:00Z") },
        Hand { name: "prev_fiscal_year_end", date: Some("2026-03-31"), status: "unique", midnight_utc: Some("2026-03-30T16:00:00Z") },
        Hand { name: "last_closed_period_end", date: Some("2026-03-31"), status: "unique", midnight_utc: Some("2026-03-30T16:00:00Z") },
    ];
    &ROWS
}

fn leap_rows() -> &'static [Hand] {
    const ROWS: [Hand; 10] = [
        Hand { name: "today", date: Some("2016-03-01"), status: "unique", midnight_utc: Some("2016-03-01T00:00:00Z") },
        Hand { name: "month_start", date: Some("2016-03-01"), status: "unique", midnight_utc: Some("2016-03-01T00:00:00Z") },
        Hand { name: "prev_month_start", date: Some("2016-02-01"), status: "unique", midnight_utc: Some("2016-02-01T00:00:00Z") },
        Hand { name: "prev_month_end", date: Some("2016-02-29"), status: "unique", midnight_utc: Some("2016-02-29T00:00:00Z") },
        Hand { name: "quarter_start", date: Some("2016-01-01"), status: "unique", midnight_utc: Some("2016-01-01T00:00:00Z") },
        Hand { name: "prev_quarter_start", date: Some("2015-10-01"), status: "unique", midnight_utc: Some("2015-10-01T00:00:00Z") },
        Hand { name: "prev_quarter_end", date: Some("2015-12-31"), status: "unique", midnight_utc: Some("2015-12-31T00:00:00Z") },
        Hand { name: "fiscal_year_start", date: Some("2015-04-01"), status: "unique", midnight_utc: Some("2015-04-01T00:00:00Z") },
        Hand { name: "prev_fiscal_year_end", date: Some("2015-03-31"), status: "unique", midnight_utc: Some("2015-03-31T00:00:00Z") },
        Hand { name: "last_closed_period_end", date: None, status: "absent", midnight_utc: None },
    ];
    &ROWS
}

fn los_angeles_rows() -> &'static [Hand] {
    const ROWS: [Hand; 10] = [
        Hand { name: "today", date: Some("2018-06-20"), status: "unique", midnight_utc: Some("2018-06-20T07:00:00Z") },
        Hand { name: "month_start", date: Some("2018-06-01"), status: "unique", midnight_utc: Some("2018-06-01T07:00:00Z") },
        Hand { name: "prev_month_start", date: Some("2018-05-01"), status: "unique", midnight_utc: Some("2018-05-01T07:00:00Z") },
        Hand { name: "prev_month_end", date: Some("2018-05-31"), status: "unique", midnight_utc: Some("2018-05-31T07:00:00Z") },
        Hand { name: "quarter_start", date: Some("2018-04-01"), status: "unique", midnight_utc: Some("2018-04-01T07:00:00Z") },
        Hand { name: "prev_quarter_start", date: Some("2018-01-01"), status: "unique", midnight_utc: Some("2018-01-01T08:00:00Z") },
        Hand { name: "prev_quarter_end", date: Some("2018-03-31"), status: "unique", midnight_utc: Some("2018-03-31T07:00:00Z") },
        Hand { name: "fiscal_year_start", date: Some("2018-04-01"), status: "unique", midnight_utc: Some("2018-04-01T07:00:00Z") },
        Hand { name: "prev_fiscal_year_end", date: Some("2018-03-31"), status: "unique", midnight_utc: Some("2018-03-31T07:00:00Z") },
        Hand { name: "last_closed_period_end", date: None, status: "absent", midnight_utc: None },
    ];
    &ROWS
}

fn sao_paulo_rows() -> &'static [Hand] {
    const ROWS: [Hand; 10] = [
        Hand { name: "today", date: Some("2017-10-15"), status: "nonexistent", midnight_utc: None },
        Hand { name: "month_start", date: Some("2017-10-01"), status: "unique", midnight_utc: Some("2017-10-01T03:00:00Z") },
        Hand { name: "prev_month_start", date: Some("2017-09-01"), status: "unique", midnight_utc: Some("2017-09-01T03:00:00Z") },
        Hand { name: "prev_month_end", date: Some("2017-09-30"), status: "unique", midnight_utc: Some("2017-09-30T03:00:00Z") },
        Hand { name: "quarter_start", date: Some("2017-10-01"), status: "unique", midnight_utc: Some("2017-10-01T03:00:00Z") },
        Hand { name: "prev_quarter_start", date: Some("2017-07-01"), status: "unique", midnight_utc: Some("2017-07-01T03:00:00Z") },
        Hand { name: "prev_quarter_end", date: Some("2017-09-30"), status: "unique", midnight_utc: Some("2017-09-30T03:00:00Z") },
        Hand { name: "fiscal_year_start", date: Some("2017-04-01"), status: "unique", midnight_utc: Some("2017-04-01T03:00:00Z") },
        Hand { name: "prev_fiscal_year_end", date: Some("2017-03-31"), status: "unique", midnight_utc: Some("2017-03-31T03:00:00Z") },
        Hand { name: "last_closed_period_end", date: None, status: "absent", midnight_utc: None },
    ];
    &ROWS
}

fn closure() -> Closure<'static> {
    Closure {
        last_closed_period_end: "2026-03-31",
        label: "close-2026-03",
        record_sha256: RECORD,
    }
}

async fn connect_as(fix: &Fix, user: &str, password: &str, ca: &[u8]) -> Client {
    let tls = calendar_store::rustls_connector(ca).expect("connector");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&fix.host);
    config.port(fix.port);
    config.dbname(&fix.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("calendar-store-harness");
    let (client, connection) = config.connect(tls).await.expect("harness connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn must_42501(client: &Client, sql: &str) {
    let err = client.query(sql, &[]).await.expect_err(sql);
    assert_eq!(sqlstate(&err), Some("42501"), "{sql}");
}

fn sqlstate(err: &Error) -> Option<&str> {
    err.code().map(|code| code.code())
}

async fn schema_exists(client: &Client, name: &str) -> bool {
    client
        .query_opt("SELECT 1 FROM pg_catalog.pg_namespace WHERE nspname = $1", &[&name])
        .await
        .expect("schema")
        .is_some()
}

async fn snapshot_count(client: &Client) -> i64 {
    client
        .query_one("SELECT count(*) FROM oo_calendar_control.snapshot", &[])
        .await
        .expect("count")
        .get(0)
}

async fn pointer_id(client: &Client) -> Option<String> {
    client
        .query_one("SELECT snapshot_id FROM oo_calendar_control.active_pointer WHERE singleton", &[])
        .await
        .expect("pointer")
        .get(0)
}

async fn put_at(
    fix: &Fix,
    endpoint: &Endpoint,
    version: &str,
    yaml: &str,
    as_of: &str,
    closure: Option<Closure<'_>>,
    expected: &str,
) -> Result<PutReceipt, StoreError> {
    put_snapshot(&PutRequest {
        endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        calendar_version: version,
        calendar_yaml: yaml,
        as_of,
        closure,
        expected_resolved_sha256: expected,
    })
    .await
}

async fn activate_at(
    fix: &Fix,
    endpoint: &Endpoint,
    version: &str,
    receipt: &PutReceipt,
    expected_current: Option<&str>,
) -> Result<ActivateReceipt, StoreError> {
    activate(&ActivateRequest {
        endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        expected_current,
        snapshot_id: &receipt.snapshot_id,
        config_sha256: &receipt.config_sha256,
        calendar_version: version,
        as_of: &receipt.as_of,
    })
    .await
}

async fn read_reader(fix: &Fix, endpoint: &Endpoint) -> Result<ReadReceipt, StoreError> {
    read_view(&ReadRequest {
        endpoint,
        user: "oo_r_calendar",
        password: &fix.reader,
        ca_pem: &fix.ca,
    })
    .await
}

async fn wall_clock(client: &Client) -> String {
    client
        .query_one(
            "SELECT pg_catalog.to_char(pg_catalog.statement_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')",
            &[],
        )
        .await
        .expect("wall")
        .get(0)
}

fn flags_false(receipt_finance: bool, receipt_auth: bool, receipt_ontology: bool, receipt_clock: bool) {
    assert!(!receipt_finance && !receipt_auth && !receipt_ontology && !receipt_clock);
}

#[tokio::test]
async fn fresh_pg_contract() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    assert_eq!(fix.database, "oo_a289");
    assert_eq!(sha256_hex(EXAMPLE.as_bytes()), EXAMPLE_HASH);
    let endpoint = endpoint_of(&fix);
    let fixture = connect_as(&fix, "oo_fixture_admin", &fix.fixture_admin, &fix.ca).await;
    fixture
        .batch_execute(
            "DROP SCHEMA IF EXISTS biz CASCADE; \
             CREATE SCHEMA biz; \
             CREATE TABLE biz.receipts (id integer); \
             REVOKE ALL ON SCHEMA biz FROM PUBLIC; \
             REVOKE ALL ON TABLE biz.receipts FROM PUBLIC; \
             ALTER SCHEMA biz OWNER TO oo_business_owner; \
             ALTER TABLE biz.receipts OWNER TO oo_business_owner",
        )
        .await
        .expect("biz");

    fixture
        .batch_execute("REVOKE CREATE ON DATABASE oo_a289 FROM oo_calendar_admin")
        .await
        .expect("revoke create");
    let rolled = install(&InstallRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_calendar"],
    })
    .await
    .expect_err("create revoked");
    assert_eq!(rolled, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    assert!(!schema_exists(&fixture, "oo_calendar_control").await);
    fixture
        .batch_execute("GRANT CREATE ON DATABASE oo_a289 TO oo_calendar_admin")
        .await
        .expect("restore create");

    fixture
        .batch_execute("CREATE SCHEMA oo_dict; CREATE TABLE oo_dict.calendar (n integer)")
        .await
        .expect("foreign dict");
    let collision = install(&InstallRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_calendar"],
    })
    .await
    .expect_err("collision");
    assert_eq!(collision, StoreError::Collision { check: "oo_dict" });
    assert!(!schema_exists(&fixture, "oo_calendar_control").await);
    assert!(schema_exists(&fixture, "oo_dict").await);
    fixture
        .batch_execute("DROP TABLE oo_dict.calendar; DROP SCHEMA oo_dict")
        .await
        .expect("drop foreign");

    let installed = install(&InstallRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_calendar"],
    })
    .await
    .expect("install");
    assert_eq!(installed.disposition, InstallDisposition::Installed);
    assert_eq!(installed.contract, CONTRACT);
    assert_eq!(installed.project_id, PROJECT);
    assert_eq!(installed.sql_sha256, migration_sha256().expect("hash"));
    assert!(installed.database_used);
    flags_false(installed.finance_approval, installed.authenticated, installed.ontology_published, installed.clock_used);
    let replay = install(&InstallRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_calendar"],
    })
    .await
    .expect("replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);
    let rebound = install(&InstallRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: "project-two",
        runtime_roles: &["oo_r_calendar"],
    })
    .await
    .expect_err("other project");
    assert_eq!(rebound, StoreError::ProjectBinding);
    let allow = install(&InstallRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_calendar", "oo_r_other"],
    })
    .await
    .expect_err("allowlist");
    assert_eq!(allow, StoreError::Allowlist);
    let bound: String = fixture
        .query_one("SELECT project_id FROM oo_calendar_control.deployment WHERE singleton", &[])
        .await
        .expect("deployment")
        .get(0);
    assert_eq!(bound, PROJECT);
    let allow_count: i64 = fixture
        .query_one("SELECT count(*) FROM oo_calendar_control.runtime_allow", &[])
        .await
        .expect("allow count")
        .get(0);
    assert_eq!(allow_count, 1);

    let admin = connect_as(&fix, "oo_calendar_admin", &fix.admin, &fix.ca).await;
    let zeros = "0".repeat(64);
    admin
        .execute(
            "UPDATE oo_calendar_control.install_manifest SET sql_sha256 = $1 WHERE singleton",
            &[&zeros],
        )
        .await
        .expect("damage hash");
    let drift = install(&InstallRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_calendar"],
    })
    .await
    .expect_err("hash drift");
    assert_eq!(drift, StoreError::InstallDrift { check: "hash" });
    admin
        .execute(
            "UPDATE oo_calendar_control.install_manifest SET sql_sha256 = $1 WHERE singleton",
            &[&installed.sql_sha256],
        )
        .await
        .expect("restore hash");
    assert_eq!(
        install(&InstallRequest {
            endpoint: &endpoint,
            user: "oo_calendar_admin",
            password: &fix.admin,
            ca_pem: &fix.ca,
            project_id: PROJECT,
            runtime_roles: &["oo_r_calendar"],
        })
        .await
        .expect("replay after restore")
        .disposition,
        InstallDisposition::Replay
    );

    admin
        .batch_execute("ALTER TABLE oo_calendar_control.published ADD COLUMN extra text")
        .await
        .expect("extra column");
    let shape = install(&InstallRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        runtime_roles: &["oo_r_calendar"],
    })
    .await
    .expect_err("shape drift");
    assert_eq!(shape, StoreError::InstallDrift { check: "published_columns" });
    let extra: i64 = admin
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_calendar_control' AND c.relname = 'published' AND a.attname = 'extra'",
            &[],
        )
        .await
        .expect("extra remains")
        .get(0);
    assert_eq!(extra, 1);
    admin
        .batch_execute("ALTER TABLE oo_calendar_control.published DROP COLUMN extra")
        .await
        .expect("drop extra");

    let wrong = "b".repeat(64);
    let mismatched = put_at(&fix, &endpoint, "fy2026", EXAMPLE, "2026-04-01T03:30:00Z", Some(closure()), &wrong)
        .await
        .expect_err("wrong hash");
    assert_eq!(mismatched, StoreError::ResolvedMismatch);
    assert_eq!(snapshot_count(&admin).await, 0);
    let refused = put_at(&fix, &endpoint, "fy2026", EXAMPLE, "not-a-date", Some(closure()), &wrong)
        .await
        .expect_err("bad as_of");
    assert!(matches!(refused, StoreError::SnapshotRefused { .. }));
    assert_eq!(snapshot_count(&admin).await, 0);

    let wrong_tls = read_view(&ReadRequest {
        endpoint: &endpoint,
        user: "oo_r_calendar",
        password: &fix.reader,
        ca_pem: &fix.wrong_ca,
    })
    .await
    .expect_err("wrong ca");
    assert!(matches!(wrong_tls, StoreError::Connect | StoreError::Tls), "{wrong_tls}");

    let reader = connect_as(&fix, "oo_r_calendar", &fix.reader, &fix.ca).await;
    must_42501(&reader, "SELECT project_id FROM oo_dict.calendar").await;
    let early = read_reader(&fix, &endpoint).await.expect_err("read before activate");
    assert_eq!(early, StoreError::Query { sqlstate: Some("42501".to_owned()) });

    assert_helper(EXAMPLE, "2026-04-01T03:30:00Z", Some(closure()), april_rows());
    let april_hash = resolved_hash(EXAMPLE, "2026-04-01T03:30:00Z", Some(closure()));
    let april = put_at(&fix, &endpoint, "fy2026", EXAMPLE, "2026-04-01T03:30:00Z", Some(closure()), &april_hash)
        .await
        .expect("april");
    assert_eq!(april.disposition, PutDisposition::Inserted);
    assert_eq!(april.config_sha256, EXAMPLE_HASH);
    assert_eq!(april.as_of, "2026-04-01T03:30:00Z");
    assert!(april.database_used);
    flags_false(april.finance_approval, april.authenticated, april.ontology_published, april.clock_used);
    let april_replay = put_at(&fix, &endpoint, "fy2026", EXAMPLE, "2026-04-01T03:30:00Z", Some(closure()), &april_hash)
        .await
        .expect("april replay");
    assert_eq!(april_replay.disposition, PutDisposition::Replay);
    assert_eq!(april_replay.snapshot_id, april.snapshot_id);
    assert_eq!(snapshot_count(&admin).await, 1);

    let drift_put = put_at(&fix, &endpoint, "drift-v", EXAMPLE, "2026-04-01T03:30:00Z", Some(closure()), &april_hash)
        .await
        .expect("drift version");
    assert_eq!(drift_put.disposition, PutDisposition::Inserted);
    admin
        .execute(
            "UPDATE oo_calendar_control.snapshot SET resolved_sha256 = $1 WHERE calendar_version = 'drift-v'",
            &[&zeros],
        )
        .await
        .expect("damage resolved");
    let damaged = put_at(&fix, &endpoint, "drift-v", EXAMPLE, "2026-04-01T03:30:00Z", Some(closure()), &april_hash)
        .await
        .expect_err("snapshot drift");
    assert_eq!(damaged, StoreError::SnapshotDrift);
    let stored_hash: String = admin
        .query_one(
            "SELECT resolved_sha256 FROM oo_calendar_control.snapshot WHERE calendar_version = 'drift-v'",
            &[],
        )
        .await
        .expect("stored hash")
        .get(0);
    assert_eq!(stored_hash, zeros);

    let stale = "a".repeat(64);
    let cas = activate_at(&fix, &endpoint, "fy2026", &april, Some(&stale)).await.expect_err("stale cas");
    assert_eq!(cas, StoreError::CasConflict);
    assert_eq!(pointer_id(&admin).await, None);

    fixture.batch_execute("REVOKE oo_calendar_view FROM oo_calendar_admin").await.expect("revoke member");
    let atomic = activate_at(&fix, &endpoint, "fy2026", &april, None).await.expect_err("activation atomicity");
    assert_eq!(atomic, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    assert_eq!(pointer_id(&admin).await, None);
    fixture.batch_execute("GRANT oo_calendar_view TO oo_calendar_admin").await.expect("grant member");

    let activated = activate_at(&fix, &endpoint, "fy2026", &april, None).await.expect("activate april");
    assert_eq!(activated.disposition, "activated");
    assert_eq!(activated.grants, 1);
    assert_eq!(activated.snapshot_id, april.snapshot_id);
    flags_false(activated.finance_approval, activated.authenticated, activated.ontology_published, activated.clock_used);
    let april_view = read_reader(&fix, &endpoint).await.expect("read april");
    assert_view(&april_view, "Asia/Shanghai", "2026-04-01T03:30:00Z", "fy2026", april_rows());
    assert_ne!(april_view.as_of_utc, wall_clock(&admin).await);

    must_42501(&reader, "SELECT calendar_yaml FROM oo_calendar_control.snapshot").await;
    must_42501(&reader, "SELECT closure_label FROM oo_calendar_control.snapshot").await;
    must_42501(&reader, "SELECT closure_record_sha256 FROM oo_calendar_control.snapshot").await;
    must_42501(&reader, "SELECT report_json FROM oo_calendar_control.snapshot").await;
    must_42501(&reader, "SELECT snapshot_id FROM oo_calendar_control.published").await;
    must_42501(&reader, "SELECT id FROM biz.receipts").await;
    admin.batch_execute("SET ROLE oo_calendar_view").await.expect("set role");
    must_42501(&admin, "SELECT id FROM biz.receipts").await;
    must_42501(&admin, "SELECT snapshot_id FROM oo_calendar_control.snapshot").await;
    admin.batch_execute("RESET ROLE").await.expect("reset role");
    let other = connect_as(&fix, "oo_r_other", &fix.other, &fix.ca).await;
    must_42501(&other, "SELECT project_id FROM oo_dict.calendar").await;
    let admin_read = read_view(&ReadRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
    })
    .await
    .expect_err("admin is not a runtime reader");
    assert_eq!(admin_read, StoreError::LoginRole);

    let label: String = admin
        .query_one(
            "SELECT closure_label FROM oo_calendar_control.snapshot WHERE calendar_version = 'fy2026'",
            &[],
        )
        .await
        .expect("private label")
        .get(0);
    assert_eq!(label, "close-2026-03");
    let columns: Vec<String> = admin
        .query(
            "SELECT a.attname FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar' AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY a.attnum",
            &[],
        )
        .await
        .expect("columns")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(
        columns,
        vec![
            "project_id",
            "business_tz",
            "as_of",
            "calendar_version",
            "today_date",
            "today_midnight_status",
            "today_midnight",
            "month_start_date",
            "month_start_midnight_status",
            "month_start_midnight",
            "prev_month_start_date",
            "prev_month_start_midnight_status",
            "prev_month_start_midnight",
            "prev_month_end_date",
            "prev_month_end_midnight_status",
            "prev_month_end_midnight",
            "quarter_start_date",
            "quarter_start_midnight_status",
            "quarter_start_midnight",
            "prev_quarter_start_date",
            "prev_quarter_start_midnight_status",
            "prev_quarter_start_midnight",
            "prev_quarter_end_date",
            "prev_quarter_end_midnight_status",
            "prev_quarter_end_midnight",
            "fiscal_year_start_date",
            "fiscal_year_start_midnight_status",
            "fiscal_year_start_midnight",
            "prev_fiscal_year_end_date",
            "prev_fiscal_year_end_midnight_status",
            "prev_fiscal_year_end_midnight",
            "last_closed_period_end_date",
            "last_closed_period_end_midnight_status",
            "last_closed_period_end_midnight",
        ]
    );
    let viewdef: String = admin
        .query_one("SELECT pg_catalog.pg_get_viewdef('oo_dict.calendar'::regclass, true)", &[])
        .await
        .expect("viewdef")
        .get(0);
    let lower = viewdef.to_ascii_lowercase();
    for token in ["now(", "current_date", "current_timestamp", "clock_timestamp", "date_trunc", "current_setting", "closure_label", "calendar_yaml", "report_json"] {
        assert!(!lower.contains(token), "{token}");
    }
    let owner: String = admin
        .query_one(
            "SELECT pg_catalog.pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'",
            &[],
        )
        .await
        .expect("owner")
        .get(0);
    assert_eq!(owner, "oo_calendar_view");
    let flag_row = admin
        .query_one(
            "SELECT finance_approval, authenticated, ontology_published, clock_used \
             FROM oo_calendar_control.deployment WHERE singleton",
            &[],
        )
        .await
        .expect("flags");
    let stored_flags: [bool; 4] = [flag_row.get(0), flag_row.get(1), flag_row.get(2), flag_row.get(3)];
    assert_eq!(stored_flags, [false, false, false, false]);

    let other_project = put_snapshot(&PutRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: "project-two",
        calendar_version: "fy2026",
        calendar_yaml: EXAMPLE,
        as_of: "2026-04-01T03:30:00Z",
        closure: Some(closure()),
        expected_resolved_sha256: &april_hash,
    })
    .await
    .expect_err("other project put");
    assert_eq!(other_project, StoreError::ProjectBinding);

    let leap_yaml = calendar("UTC");
    let leap_as_of = "2016-03-01T00:30:00Z";
    assert_helper(&leap_yaml, leap_as_of, None, leap_rows());
    let leap_hash = resolved_hash(&leap_yaml, leap_as_of, None);
    let leap = put_at(&fix, &endpoint, "leap2016", &leap_yaml, leap_as_of, None, &leap_hash)
        .await
        .expect("leap");
    activate_at(&fix, &endpoint, "leap2016", &leap, Some(&april.snapshot_id))
        .await
        .expect("activate leap");
    assert_view(
        &read_reader(&fix, &endpoint).await.expect("read leap"),
        "UTC",
        leap_as_of,
        "leap2016",
        leap_rows(),
    );

    let la_yaml = calendar("America/Los_Angeles");
    let la_as_of = "2018-06-21T06:30:00Z";
    assert_helper(&la_yaml, la_as_of, None, los_angeles_rows());
    let la_hash = resolved_hash(&la_yaml, la_as_of, None);
    let la = put_at(&fix, &endpoint, "la2018", &la_yaml, la_as_of, None, &la_hash)
        .await
        .expect("los angeles");
    activate_at(&fix, &endpoint, "la2018", &la, Some(&leap.snapshot_id))
        .await
        .expect("activate la");
    assert_view(
        &read_reader(&fix, &endpoint).await.expect("read la"),
        "America/Los_Angeles",
        la_as_of,
        "la2018",
        los_angeles_rows(),
    );

    let sao_yaml = calendar("America/Sao_Paulo");
    let sao_as_of = "2017-10-15T04:00:00Z";
    assert_helper(&sao_yaml, sao_as_of, None, sao_paulo_rows());
    let sao_hash = resolved_hash(&sao_yaml, sao_as_of, None);
    let sao = put_at(&fix, &endpoint, "sp2017", &sao_yaml, sao_as_of, None, &sao_hash)
        .await
        .expect("sao paulo");
    activate_at(&fix, &endpoint, "sp2017", &sao, Some(&la.snapshot_id))
        .await
        .expect("activate sao");
    let sao_view = read_reader(&fix, &endpoint).await.expect("read sao");
    assert_view(&sao_view, "America/Sao_Paulo", sao_as_of, "sp2017", sao_paulo_rows());
    assert_ne!(sao_view.as_of_utc, wall_clock(&admin).await);

    let old = activate_at(&fix, &endpoint, "fy2026", &april, Some(&april.snapshot_id))
        .await
        .expect_err("old pointer");
    assert_eq!(old, StoreError::CasConflict);
    let wrong_config = activate(&ActivateRequest {
        endpoint: &endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        expected_current: Some(&sao.snapshot_id),
        snapshot_id: &sao.snapshot_id,
        config_sha256: &april.config_sha256,
        calendar_version: "sp2017",
        as_of: &sao.as_of,
    })
    .await
    .expect_err("wrong config");
    assert_eq!(wrong_config, StoreError::Activation);
    assert_view(
        &read_reader(&fix, &endpoint).await.expect("still sao"),
        "America/Sao_Paulo",
        sao_as_of,
        "sp2017",
        sao_paulo_rows(),
    );
    assert_eq!(snapshot_count(&admin).await, 5);
    assert_eq!(
        install(&InstallRequest {
            endpoint: &endpoint,
            user: "oo_calendar_admin",
            password: &fix.admin,
            ca_pem: &fix.ca,
            project_id: PROJECT,
            runtime_roles: &["oo_r_calendar"],
        })
        .await
        .expect("replay after activate")
        .disposition,
        InstallDisposition::Replay
    );
}

#[tokio::test]
async fn restart_snapshot_persists() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let admin = connect_as(&fix, "oo_calendar_admin", &fix.admin, &fix.ca).await;
    let view = read_reader(&fix, &endpoint).await.expect("restart read");
    assert_view(&view, "America/Sao_Paulo", "2017-10-15T04:00:00Z", "sp2017", sao_paulo_rows());
    assert_ne!(view.as_of_utc, wall_clock(&admin).await);
    assert_eq!(snapshot_count(&admin).await, 5);
    let label: String = admin
        .query_one(
            "SELECT closure_label FROM oo_calendar_control.snapshot WHERE calendar_version = 'fy2026'",
            &[],
        )
        .await
        .expect("private label survived")
        .get(0);
    assert_eq!(label, "close-2026-03");
    assert_eq!(
        install(&InstallRequest {
            endpoint: &endpoint,
            user: "oo_calendar_admin",
            password: &fix.admin,
            ca_pem: &fix.ca,
            project_id: PROJECT,
            runtime_roles: &["oo_r_calendar"],
        })
        .await
        .expect("replay after restart")
        .disposition,
        InstallDisposition::Replay
    );
}
