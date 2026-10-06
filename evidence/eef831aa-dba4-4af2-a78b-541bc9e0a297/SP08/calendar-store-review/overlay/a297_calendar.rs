#![forbid(unsafe_code)]

//! Independent calendar-store proof. Dates below are handwritten.
//! The helper is called to recompute the digest and to check those dates.
//! The view is compared with the same literals, not with a second printed report.

use business_calendar::{resolve, sha256_hex, Closure, Input, Midnight};
use calendar_store::{
    activate, install, put_snapshot, read_view, ActivateReceipt, ActivateRequest, Endpoint, InstallDisposition,
    InstallRequest, PutDisposition, PutReceipt, PutRequest, ReadReceipt, ReadRequest, StoreError, CONTRACT,
};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error};

const FIXTURE: &str = "/run/oo-a297/fixture";
const PROJECT: &str = "a297-one";
const RECORD: &str = "a297a297a297a297a297a297a297a297a297a297a297a297a297a297a297a297";
const APRIL_AS_OF: &str = "2026-04-01T03:30:00Z";
const LEAP_AS_OF: &str = "2016-03-01T00:30:00Z";
const LA_AS_OF: &str = "2018-06-21T06:30:00Z";
const SP_AS_OF: &str = "2017-10-15T04:00:00Z";
const PARIS_AS_OF: &str = "1976-09-26T22:30:00Z";

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

struct Lit {
    name: &'static str,
    date: Option<&'static str>,
    status: &'static str,
    helper_stamp: Option<&'static str>,
    view_utc: Option<&'static str>,
}

struct Shown<'a> {
    version: &'a str,
    tz: &'a str,
    as_of: &'a str,
    rows: &'a [Lit],
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

fn yaml(tz: &str, closed: bool) -> String {
    let mut text = format!(
        "schema_version: oos/v1\nbusiness_tz: {tz}\nfiscal_year_start_month: 4\nweek_start: monday\nanchors:\n  - today\n  - month_start\n  - prev_month_start\n  - prev_month_end\n  - quarter_start\n  - prev_quarter_start\n  - prev_quarter_end\n  - fiscal_year_start\n  - prev_fiscal_year_end\n"
    );
    if closed {
        text.push_str("  - last_closed_period_end\n");
    }
    text
}

fn closure() -> Closure<'static> {
    Closure {
        last_closed_period_end: "2026-03-31",
        label: "a297-close-2026-03",
        record_sha256: RECORD,
    }
}

fn april_rows() -> &'static [Lit] {
    const ROWS: [Lit; 10] = [
        Lit { name: "today", date: Some("2026-04-01"), status: "unique", helper_stamp: Some("2026-04-01T00:00:00+08:00"), view_utc: Some("2026-03-31T16:00:00Z") },
        Lit { name: "month_start", date: Some("2026-04-01"), status: "unique", helper_stamp: Some("2026-04-01T00:00:00+08:00"), view_utc: Some("2026-03-31T16:00:00Z") },
        Lit { name: "prev_month_start", date: Some("2026-03-01"), status: "unique", helper_stamp: Some("2026-03-01T00:00:00+08:00"), view_utc: Some("2026-02-28T16:00:00Z") },
        Lit { name: "prev_month_end", date: Some("2026-03-31"), status: "unique", helper_stamp: Some("2026-03-31T00:00:00+08:00"), view_utc: Some("2026-03-30T16:00:00Z") },
        Lit { name: "quarter_start", date: Some("2026-04-01"), status: "unique", helper_stamp: Some("2026-04-01T00:00:00+08:00"), view_utc: Some("2026-03-31T16:00:00Z") },
        Lit { name: "prev_quarter_start", date: Some("2026-01-01"), status: "unique", helper_stamp: Some("2026-01-01T00:00:00+08:00"), view_utc: Some("2025-12-31T16:00:00Z") },
        Lit { name: "prev_quarter_end", date: Some("2026-03-31"), status: "unique", helper_stamp: Some("2026-03-31T00:00:00+08:00"), view_utc: Some("2026-03-30T16:00:00Z") },
        Lit { name: "fiscal_year_start", date: Some("2026-04-01"), status: "unique", helper_stamp: Some("2026-04-01T00:00:00+08:00"), view_utc: Some("2026-03-31T16:00:00Z") },
        Lit { name: "prev_fiscal_year_end", date: Some("2026-03-31"), status: "unique", helper_stamp: Some("2026-03-31T00:00:00+08:00"), view_utc: Some("2026-03-30T16:00:00Z") },
        Lit { name: "last_closed_period_end", date: Some("2026-03-31"), status: "unique", helper_stamp: Some("2026-03-31T00:00:00+08:00"), view_utc: Some("2026-03-30T16:00:00Z") },
    ];
    &ROWS
}

fn leap_rows() -> &'static [Lit] {
    const ROWS: [Lit; 10] = [
        Lit { name: "today", date: Some("2016-03-01"), status: "unique", helper_stamp: Some("2016-03-01T00:00:00Z"), view_utc: Some("2016-03-01T00:00:00Z") },
        Lit { name: "month_start", date: Some("2016-03-01"), status: "unique", helper_stamp: Some("2016-03-01T00:00:00Z"), view_utc: Some("2016-03-01T00:00:00Z") },
        Lit { name: "prev_month_start", date: Some("2016-02-01"), status: "unique", helper_stamp: Some("2016-02-01T00:00:00Z"), view_utc: Some("2016-02-01T00:00:00Z") },
        Lit { name: "prev_month_end", date: Some("2016-02-29"), status: "unique", helper_stamp: Some("2016-02-29T00:00:00Z"), view_utc: Some("2016-02-29T00:00:00Z") },
        Lit { name: "quarter_start", date: Some("2016-01-01"), status: "unique", helper_stamp: Some("2016-01-01T00:00:00Z"), view_utc: Some("2016-01-01T00:00:00Z") },
        Lit { name: "prev_quarter_start", date: Some("2015-10-01"), status: "unique", helper_stamp: Some("2015-10-01T00:00:00Z"), view_utc: Some("2015-10-01T00:00:00Z") },
        Lit { name: "prev_quarter_end", date: Some("2015-12-31"), status: "unique", helper_stamp: Some("2015-12-31T00:00:00Z"), view_utc: Some("2015-12-31T00:00:00Z") },
        Lit { name: "fiscal_year_start", date: Some("2015-04-01"), status: "unique", helper_stamp: Some("2015-04-01T00:00:00Z"), view_utc: Some("2015-04-01T00:00:00Z") },
        Lit { name: "prev_fiscal_year_end", date: Some("2015-03-31"), status: "unique", helper_stamp: Some("2015-03-31T00:00:00Z"), view_utc: Some("2015-03-31T00:00:00Z") },
        Lit { name: "last_closed_period_end", date: None, status: "absent", helper_stamp: None, view_utc: None },
    ];
    &ROWS
}

fn los_angeles_rows() -> &'static [Lit] {
    const ROWS: [Lit; 10] = [
        Lit { name: "today", date: Some("2018-06-20"), status: "unique", helper_stamp: Some("2018-06-20T00:00:00-07:00"), view_utc: Some("2018-06-20T07:00:00Z") },
        Lit { name: "month_start", date: Some("2018-06-01"), status: "unique", helper_stamp: Some("2018-06-01T00:00:00-07:00"), view_utc: Some("2018-06-01T07:00:00Z") },
        Lit { name: "prev_month_start", date: Some("2018-05-01"), status: "unique", helper_stamp: Some("2018-05-01T00:00:00-07:00"), view_utc: Some("2018-05-01T07:00:00Z") },
        Lit { name: "prev_month_end", date: Some("2018-05-31"), status: "unique", helper_stamp: Some("2018-05-31T00:00:00-07:00"), view_utc: Some("2018-05-31T07:00:00Z") },
        Lit { name: "quarter_start", date: Some("2018-04-01"), status: "unique", helper_stamp: Some("2018-04-01T00:00:00-07:00"), view_utc: Some("2018-04-01T07:00:00Z") },
        Lit { name: "prev_quarter_start", date: Some("2018-01-01"), status: "unique", helper_stamp: Some("2018-01-01T00:00:00-08:00"), view_utc: Some("2018-01-01T08:00:00Z") },
        Lit { name: "prev_quarter_end", date: Some("2018-03-31"), status: "unique", helper_stamp: Some("2018-03-31T00:00:00-07:00"), view_utc: Some("2018-03-31T07:00:00Z") },
        Lit { name: "fiscal_year_start", date: Some("2018-04-01"), status: "unique", helper_stamp: Some("2018-04-01T00:00:00-07:00"), view_utc: Some("2018-04-01T07:00:00Z") },
        Lit { name: "prev_fiscal_year_end", date: Some("2018-03-31"), status: "unique", helper_stamp: Some("2018-03-31T00:00:00-07:00"), view_utc: Some("2018-03-31T07:00:00Z") },
        Lit { name: "last_closed_period_end", date: None, status: "absent", helper_stamp: None, view_utc: None },
    ];
    &ROWS
}

fn sao_paulo_rows() -> &'static [Lit] {
    const ROWS: [Lit; 10] = [
        Lit { name: "today", date: Some("2017-10-15"), status: "nonexistent", helper_stamp: None, view_utc: None },
        Lit { name: "month_start", date: Some("2017-10-01"), status: "unique", helper_stamp: Some("2017-10-01T00:00:00-03:00"), view_utc: Some("2017-10-01T03:00:00Z") },
        Lit { name: "prev_month_start", date: Some("2017-09-01"), status: "unique", helper_stamp: Some("2017-09-01T00:00:00-03:00"), view_utc: Some("2017-09-01T03:00:00Z") },
        Lit { name: "prev_month_end", date: Some("2017-09-30"), status: "unique", helper_stamp: Some("2017-09-30T00:00:00-03:00"), view_utc: Some("2017-09-30T03:00:00Z") },
        Lit { name: "quarter_start", date: Some("2017-10-01"), status: "unique", helper_stamp: Some("2017-10-01T00:00:00-03:00"), view_utc: Some("2017-10-01T03:00:00Z") },
        Lit { name: "prev_quarter_start", date: Some("2017-07-01"), status: "unique", helper_stamp: Some("2017-07-01T00:00:00-03:00"), view_utc: Some("2017-07-01T03:00:00Z") },
        Lit { name: "prev_quarter_end", date: Some("2017-09-30"), status: "unique", helper_stamp: Some("2017-09-30T00:00:00-03:00"), view_utc: Some("2017-09-30T03:00:00Z") },
        Lit { name: "fiscal_year_start", date: Some("2017-04-01"), status: "unique", helper_stamp: Some("2017-04-01T00:00:00-03:00"), view_utc: Some("2017-04-01T03:00:00Z") },
        Lit { name: "prev_fiscal_year_end", date: Some("2017-03-31"), status: "unique", helper_stamp: Some("2017-03-31T00:00:00-03:00"), view_utc: Some("2017-03-31T03:00:00Z") },
        Lit { name: "last_closed_period_end", date: None, status: "absent", helper_stamp: None, view_utc: None },
    ];
    &ROWS
}

fn check_report(yaml: &str, as_of: &str, closure: Option<Closure<'_>>, rows: &[Lit]) -> String {
    let report = resolve(&Input {
        calendar_yaml: yaml,
        as_of,
        closure,
    })
    .expect("helper resolve");
    assert_eq!(report.contract, "oo.business-calendar/v1");
    assert_eq!(report.outcome, "resolved");
    assert_eq!(report.as_of, as_of);
    assert!(!report.finance_approval && !report.authenticated && !report.database_used && !report.clock_used);
    for row in rows {
        let found = report.anchors.iter().find(|item| item.name == row.name);
        match (row.date, row.status, row.helper_stamp, found) {
            (None, "absent", None, None) => {}
            (Some(date), "unique", Some(stamp), Some(item)) => {
                assert_eq!(item.civil, date, "{}", row.name);
                match &item.midnight {
                    Midnight::Unique { rfc3339 } => assert_eq!(rfc3339, stamp, "{}", row.name),
                    other => panic!("helper midnight {other:?} on {}", row.name),
                }
            }
            (Some(date), "nonexistent", None, Some(item)) => {
                assert_eq!(item.civil, date, "{}", row.name);
                assert_eq!(item.midnight, Midnight::Nonexistent, "{}", row.name);
            }
            _ => panic!("helper disagreement on {}", row.name),
        }
    }
    sha256_hex(serde_json::to_string(&report).expect("helper json").as_bytes())
}

fn assert_view(view: &ReadReceipt, tz: &str, as_of: &str, version: &str, rows: &[Lit]) {
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
        assert_eq!(column.midnight_utc.as_deref(), row.view_utc, "{}", row.name);
    }
}

fn flags_false(finance: bool, authenticated: bool, ontology: bool, clock: bool) {
    assert!(!finance && !authenticated && !ontology && !clock);
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
    config.application_name("a297-calendar-review");
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

async fn snapshot_count(client: &Client) -> i64 {
    client
        .query_one("SELECT count(*) FROM oo_calendar_control.snapshot", &[])
        .await
        .expect("count")
        .get(0)
}

async fn pointer_id(client: &Client) -> Option<String> {
    client
        .query_one(
            "SELECT snapshot_id FROM oo_calendar_control.active_pointer WHERE singleton",
            &[],
        )
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

fn install_request<'a>(endpoint: &'a Endpoint, fix: &'a Fix, project: &'a str, roles: &'a [&'a str]) -> InstallRequest<'a> {
    InstallRequest {
        endpoint,
        user: "oo_calendar_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: project,
        runtime_roles: roles,
    }
}

async fn show(fix: &Fix, endpoint: &Endpoint, receipt: &PutReceipt, current: &str, expect: Shown<'_>) -> String {
    let next = if current == receipt.snapshot_id {
        receipt.snapshot_id.clone()
    } else {
        let activated = activate_at(fix, endpoint, expect.version, receipt, Some(current))
            .await
            .expect(expect.version);
        assert_eq!(activated.disposition, "activated");
        assert_eq!(activated.grants, 1);
        assert_eq!(activated.snapshot_id, receipt.snapshot_id);
        flags_false(
            activated.finance_approval,
            activated.authenticated,
            activated.ontology_published,
            activated.clock_used,
        );
        activated.snapshot_id
    };
    let view = read_reader(fix, endpoint).await.expect(expect.version);
    assert_view(&view, expect.tz, expect.as_of, expect.version, expect.rows);
    assert_ne!(view.as_of_utc, wall_clock(&connect_as(fix, "oo_calendar_admin", &fix.admin, &fix.ca).await).await);
    next
}

#[tokio::test]
async fn independent_store() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let fixture = connect_as(&fix, "oo_fixture_admin", &fix.fixture_admin, &fix.ca).await;
    fixture
        .batch_execute(
            "DROP SCHEMA IF EXISTS oo_calendar_control CASCADE; \
             DROP SCHEMA IF EXISTS oo_dict CASCADE; \
             DROP SCHEMA IF EXISTS biz CASCADE; \
             CREATE SCHEMA biz; \
             CREATE TABLE biz.receipts (id integer); \
             REVOKE ALL ON SCHEMA biz FROM PUBLIC; \
             REVOKE ALL ON TABLE biz.receipts FROM PUBLIC; \
             ALTER SCHEMA biz OWNER TO oo_business_owner; \
             ALTER TABLE biz.receipts OWNER TO oo_business_owner",
        )
        .await
        .expect("reset");

    fixture
        .batch_execute("REVOKE CREATE ON DATABASE oo_a289 FROM oo_calendar_admin")
        .await
        .expect("revoke create");
    let rolled = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar"]))
        .await
        .expect_err("create revoked");
    assert_eq!(rolled, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    fixture
        .batch_execute("GRANT CREATE ON DATABASE oo_a289 TO oo_calendar_admin")
        .await
        .expect("restore create");

    fixture
        .batch_execute("CREATE SCHEMA oo_dict; CREATE TABLE oo_dict.calendar (n integer)")
        .await
        .expect("foreign dict");
    let collision = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar"]))
        .await
        .expect_err("collision");
    assert_eq!(collision, StoreError::Collision { check: "oo_dict" });
    fixture
        .batch_execute("DROP TABLE oo_dict.calendar; DROP SCHEMA oo_dict")
        .await
        .expect("drop foreign");

    let installed = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar"]))
        .await
        .expect("install");
    assert_eq!(installed.disposition, InstallDisposition::Installed);
    assert_eq!(installed.project_id, PROJECT);
    flags_false(installed.finance_approval, installed.authenticated, installed.ontology_published, installed.clock_used);
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar"]))
            .await
            .expect("replay")
            .disposition,
        InstallDisposition::Replay
    );
    assert_eq!(
        install(&install_request(&endpoint, &fix, "project-two", &["oo_r_calendar"]))
            .await
            .expect_err("second project"),
        StoreError::ProjectBinding
    );
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar", "oo_r_other"]))
            .await
            .expect_err("allowlist"),
        StoreError::Allowlist
    );
    let deployment = fixture
        .query_one(
            "SELECT project_id, finance_approval, authenticated, ontology_published, clock_used, \
                    (SELECT count(*) FROM oo_calendar_control.deployment) \
             FROM oo_calendar_control.deployment WHERE singleton",
            &[],
        )
        .await
        .expect("deployment");
    let project: String = deployment.get(0);
    let stored_flags: [bool; 4] = [deployment.get(1), deployment.get(2), deployment.get(3), deployment.get(4)];
    let deployment_count: i64 = deployment.get(5);
    assert_eq!(project, PROJECT);
    assert_eq!(stored_flags, [false, false, false, false]);
    assert_eq!(deployment_count, 1);

    let admin = connect_as(&fix, "oo_calendar_admin", &fix.admin, &fix.ca).await;
    let zeros = "0".repeat(64);
    admin
        .execute(
            "UPDATE oo_calendar_control.install_manifest SET sql_sha256 = $1 WHERE singleton",
            &[&zeros],
        )
        .await
        .expect("damage sql hash");
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar"]))
            .await
            .expect_err("sql drift"),
        StoreError::InstallDrift { check: "hash" }
    );
    let stuck: String = admin
        .query_one("SELECT sql_sha256 FROM oo_calendar_control.install_manifest WHERE singleton", &[])
        .await
        .expect("stuck hash")
        .get(0);
    assert_eq!(stuck, zeros);
    admin
        .execute(
            "UPDATE oo_calendar_control.install_manifest SET sql_sha256 = $1 WHERE singleton",
            &[&installed.sql_sha256],
        )
        .await
        .expect("restore sql hash");
    admin
        .batch_execute("ALTER TABLE oo_calendar_control.published ADD COLUMN extra text")
        .await
        .expect("extra column");
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar"]))
            .await
            .expect_err("column drift"),
        StoreError::InstallDrift { check: "published_columns" }
    );
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

    let april_yaml = yaml("Asia/Shanghai", true);
    let april_hash = check_report(&april_yaml, APRIL_AS_OF, Some(closure()), april_rows());
    let wrong = "b".repeat(64);
    assert_eq!(
        put_at(&fix, &endpoint, "a297apr", &april_yaml, APRIL_AS_OF, Some(closure()), &wrong)
            .await
            .expect_err("wrong hash"),
        StoreError::ResolvedMismatch
    );
    assert_eq!(
        put_at(&fix, &endpoint, "a297apr", &april_yaml, "not-a-date", Some(closure()), &wrong)
            .await
            .expect_err("bad as_of")
            .reason(),
        "snapshot_refused"
    );
    assert_eq!(snapshot_count(&admin).await, 0);
    let wrong_tls = read_view(&ReadRequest {
        endpoint: &endpoint,
        user: "oo_r_calendar",
        password: &fix.reader,
        ca_pem: &fix.wrong_ca,
    })
    .await
    .expect_err("wrong ca");
    assert!(matches!(wrong_tls, StoreError::Connect | StoreError::Tls), "{wrong_tls:?}");

    let reader = connect_as(&fix, "oo_r_calendar", &fix.reader, &fix.ca).await;
    must_42501(&reader, "SELECT project_id FROM oo_dict.calendar").await;

    let april = put_at(&fix, &endpoint, "a297apr", &april_yaml, APRIL_AS_OF, Some(closure()), &april_hash)
        .await
        .expect("april");
    assert_eq!(april.disposition, PutDisposition::Inserted);
    assert_eq!(april.as_of, APRIL_AS_OF);
    flags_false(april.finance_approval, april.authenticated, april.ontology_published, april.clock_used);
    let replay = put_at(&fix, &endpoint, "a297apr", &april_yaml, APRIL_AS_OF, Some(closure()), &april_hash)
        .await
        .expect("april replay");
    assert_eq!(replay.disposition, PutDisposition::Replay);
    assert_eq!(replay.snapshot_id, april.snapshot_id);
    assert_eq!(snapshot_count(&admin).await, 1);

    let drift = put_at(&fix, &endpoint, "a297drift", &april_yaml, APRIL_AS_OF, Some(closure()), &april_hash)
        .await
        .expect("drift row");
    assert_eq!(drift.disposition, PutDisposition::Inserted);
    admin
        .execute(
            "UPDATE oo_calendar_control.snapshot SET resolved_sha256 = $1 WHERE calendar_version = 'a297drift'",
            &[&zeros],
        )
        .await
        .expect("damage resolved");
    assert_eq!(
        put_at(&fix, &endpoint, "a297drift", &april_yaml, APRIL_AS_OF, Some(closure()), &april_hash)
            .await
            .expect_err("snapshot drift"),
        StoreError::SnapshotDrift
    );
    let damaged: String = admin
        .query_one(
            "SELECT resolved_sha256 FROM oo_calendar_control.snapshot WHERE calendar_version = 'a297drift'",
            &[],
        )
        .await
        .expect("damaged hash")
        .get(0);
    assert_eq!(damaged, zeros);

    let leap_yaml = yaml("UTC", false);
    let leap_hash = check_report(&leap_yaml, LEAP_AS_OF, None, leap_rows());
    let leap = put_at(&fix, &endpoint, "a297leap", &leap_yaml, LEAP_AS_OF, None, &leap_hash)
        .await
        .expect("leap");
    let la_yaml = yaml("America/Los_Angeles", false);
    let la_hash = check_report(&la_yaml, LA_AS_OF, None, los_angeles_rows());
    let la = put_at(&fix, &endpoint, "a297la", &la_yaml, LA_AS_OF, None, &la_hash)
        .await
        .expect("los angeles");
    let sp_yaml = yaml("America/Sao_Paulo", false);
    let sp_hash = check_report(&sp_yaml, SP_AS_OF, None, sao_paulo_rows());
    let sp = put_at(&fix, &endpoint, "a297sp", &sp_yaml, SP_AS_OF, None, &sp_hash)
        .await
        .expect("sao paulo");
    let paris_yaml = yaml("Europe/Paris", false);
    let paris_report = resolve(&Input {
        calendar_yaml: &paris_yaml,
        as_of: PARIS_AS_OF,
        closure: None,
    })
    .expect("paris helper");
    let paris_today = paris_report.anchors.iter().find(|item| item.name == "today").expect("paris today");
    assert_eq!(paris_today.civil, "1976-09-26");
    assert_eq!(
        paris_today.midnight,
        Midnight::Ambiguous {
            fold0_rfc3339: "1976-09-26T00:00:00+02:00".to_owned(),
            fold1_rfc3339: "1976-09-26T00:00:00+01:00".to_owned(),
        }
    );
    assert!(!paris_report.finance_approval && !paris_report.authenticated && !paris_report.database_used && !paris_report.clock_used);
    let paris_hash = sha256_hex(serde_json::to_string(&paris_report).expect("paris json").as_bytes());
    let paris = put_at(&fix, &endpoint, "a297paris", &paris_yaml, PARIS_AS_OF, None, &paris_hash)
        .await
        .expect("paris");
    assert_eq!(snapshot_count(&admin).await, 6);
    assert_eq!(pointer_id(&admin).await, None);

    assert_eq!(
        put_snapshot(&PutRequest {
            endpoint: &endpoint,
            user: "oo_calendar_admin",
            password: &fix.admin,
            ca_pem: &fix.ca,
            project_id: "project-two",
            calendar_version: "a297apr",
            calendar_yaml: &april_yaml,
            as_of: APRIL_AS_OF,
            closure: Some(closure()),
            expected_resolved_sha256: &april_hash,
        })
        .await
        .expect_err("other project put"),
        StoreError::ProjectBinding
    );

    let stale = "a".repeat(64);
    assert_eq!(
        activate_at(&fix, &endpoint, "a297apr", &april, Some(&stale))
            .await
            .expect_err("stale before activate"),
        StoreError::CasConflict
    );
    assert_eq!(pointer_id(&admin).await, None);

    fixture
        .batch_execute("REVOKE oo_calendar_view FROM oo_calendar_admin")
        .await
        .expect("revoke member");
    assert_eq!(
        activate_at(&fix, &endpoint, "a297apr", &april, None)
            .await
            .expect_err("revoked membership"),
        StoreError::Query { sqlstate: Some("42501".to_owned()) }
    );
    assert_eq!(pointer_id(&admin).await, None);
    fixture
        .batch_execute("GRANT oo_calendar_view TO oo_calendar_admin")
        .await
        .expect("restore member");

    let (left, right) = tokio::join!(
        activate_at(&fix, &endpoint, "a297apr", &april, None),
        activate_at(&fix, &endpoint, "a297leap", &leap, None),
    );
    let (winner, loser) = match (left, right) {
        (Ok(won), Err(err)) => {
            assert_eq!(err, StoreError::CasConflict);
            assert_eq!(won.grants, 1);
            (won.snapshot_id, leap.snapshot_id.clone())
        }
        (Err(err), Ok(won)) => {
            assert_eq!(err, StoreError::CasConflict);
            assert_eq!(won.grants, 1);
            (won.snapshot_id, april.snapshot_id.clone())
        }
        (Ok(_), Ok(_)) => panic!("two activations committed"),
        (Err(left_err), Err(right_err)) => panic!("no activation committed: {left_err:?} {right_err:?}"),
    };
    assert_eq!(pointer_id(&admin).await.as_deref(), Some(winner.as_str()));
    assert_ne!(winner, loser);
    assert_eq!(
        activate_at(&fix, &endpoint, "a297la", &la, Some(&loser))
            .await
            .expect_err("stale after race"),
        StoreError::CasConflict
    );
    assert_eq!(pointer_id(&admin).await.as_deref(), Some(winner.as_str()));

    fixture
        .batch_execute("REVOKE oo_calendar_view FROM oo_calendar_admin")
        .await
        .expect("revoke during move");
    assert_eq!(
        activate_at(&fix, &endpoint, "a297la", &la, Some(&winner))
            .await
            .expect_err("revoked move"),
        StoreError::Query { sqlstate: Some("42501".to_owned()) }
    );
    assert_eq!(pointer_id(&admin).await.as_deref(), Some(winner.as_str()));
    fixture
        .batch_execute("GRANT oo_calendar_view TO oo_calendar_admin")
        .await
        .expect("restore after move");

    let mut current = winner;
    current = show(&fix, &endpoint, &april, &current, Shown { version: "a297apr", tz: "Asia/Shanghai", as_of: APRIL_AS_OF, rows: april_rows() }).await;
    current = show(&fix, &endpoint, &leap, &current, Shown { version: "a297leap", tz: "UTC", as_of: LEAP_AS_OF, rows: leap_rows() }).await;
    current = show(&fix, &endpoint, &la, &current, Shown { version: "a297la", tz: "America/Los_Angeles", as_of: LA_AS_OF, rows: los_angeles_rows() }).await;
    if current != paris.snapshot_id {
        let activated = activate_at(&fix, &endpoint, "a297paris", &paris, Some(&current))
            .await
            .expect("activate paris");
        assert_eq!(activated.snapshot_id, paris.snapshot_id);
        current = activated.snapshot_id;
    }
    let paris_view = read_reader(&fix, &endpoint).await.expect("read paris");
    assert_eq!(paris_view.project_id, PROJECT);
    assert_eq!(paris_view.business_tz, "Europe/Paris");
    assert_eq!(paris_view.as_of_utc, PARIS_AS_OF);
    assert_eq!(paris_view.calendar_version, "a297paris");
    let paris_column = paris_view.anchors.iter().find(|item| item.name == "today").expect("view today");
    assert_eq!(paris_column.date.as_deref(), Some("1976-09-26"));
    assert_eq!(paris_column.midnight_status, "ambiguous");
    assert_eq!(paris_column.midnight_utc, None);
    let closed = paris_view.anchors.iter().find(|item| item.name == "last_closed_period_end").expect("closed");
    assert_eq!(closed.date, None);
    assert_eq!(closed.midnight_status, "absent");
    assert_eq!(closed.midnight_utc, None);
    for column in &paris_view.anchors {
        assert_ne!(column.midnight_utc.as_deref(), Some("1976-09-26T00:00:00Z"));
        assert_ne!(column.midnight_utc.as_deref(), Some("1976-09-25T22:00:00Z"));
        assert_ne!(column.midnight_utc.as_deref(), Some("1976-09-25T23:00:00Z"));
    }
    current = show(&fix, &endpoint, &sp, &current, Shown { version: "a297sp", tz: "America/Sao_Paulo", as_of: SP_AS_OF, rows: sao_paulo_rows() }).await;
    assert_eq!(current, sp.snapshot_id);
    let sp_view = read_reader(&fix, &endpoint).await.expect("read sao");
    let sp_today = sp_view.anchors.iter().find(|item| item.name == "today").expect("sp today");
    assert_eq!(sp_today.midnight_status, "nonexistent");
    assert_eq!(sp_today.midnight_utc, None);
    assert_ne!(sp_today.date.as_deref(), Some("2017-10-16"));

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
    assert_eq!(
        read_view(&ReadRequest {
            endpoint: &endpoint,
            user: "oo_calendar_admin",
            password: &fix.admin,
            ca_pem: &fix.ca,
        })
        .await
        .expect_err("admin reader"),
        StoreError::LoginRole
    );
    assert_eq!(
        read_view(&ReadRequest {
            endpoint: &endpoint,
            user: "oo_calendar_view",
            password: &fix.admin,
            ca_pem: &fix.ca,
        })
        .await
        .expect_err("view role login"),
        StoreError::LoginRole
    );

    let label: String = admin
        .query_one(
            "SELECT closure_label FROM oo_calendar_control.snapshot WHERE calendar_version = 'a297apr'",
            &[],
        )
        .await
        .expect("private label")
        .get(0);
    assert_eq!(label, "a297-close-2026-03");
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
            "project_id", "business_tz", "as_of", "calendar_version",
            "today_date", "today_midnight_status", "today_midnight",
            "month_start_date", "month_start_midnight_status", "month_start_midnight",
            "prev_month_start_date", "prev_month_start_midnight_status", "prev_month_start_midnight",
            "prev_month_end_date", "prev_month_end_midnight_status", "prev_month_end_midnight",
            "quarter_start_date", "quarter_start_midnight_status", "quarter_start_midnight",
            "prev_quarter_start_date", "prev_quarter_start_midnight_status", "prev_quarter_start_midnight",
            "prev_quarter_end_date", "prev_quarter_end_midnight_status", "prev_quarter_end_midnight",
            "fiscal_year_start_date", "fiscal_year_start_midnight_status", "fiscal_year_start_midnight",
            "prev_fiscal_year_end_date", "prev_fiscal_year_end_midnight_status", "prev_fiscal_year_end_midnight",
            "last_closed_period_end_date", "last_closed_period_end_midnight_status", "last_closed_period_end_midnight",
        ]
    );
    let viewdef: String = admin
        .query_one("SELECT pg_catalog.pg_get_viewdef('oo_dict.calendar'::regclass, true)", &[])
        .await
        .expect("viewdef")
        .get(0);
    let lower = viewdef.to_ascii_lowercase();
    for token in [
        "now(",
        "current_date",
        "current_timestamp",
        "clock_timestamp",
        "date_trunc",
        "current_setting",
        "closure_label",
        "calendar_yaml",
        "report_json",
        "closure_record_sha256",
    ] {
        assert!(!lower.contains(token), "{token} in {viewdef}");
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
    let role = admin
        .query_one(
            "SELECT rolcanlogin, rolsuper, rolbypassrls, rolcreatedb, rolcreaterole \
             FROM pg_catalog.pg_roles WHERE rolname = 'oo_calendar_view'",
            &[],
        )
        .await
        .expect("view role");
    let role_flags: [bool; 5] = [role.get(0), role.get(1), role.get(2), role.get(3), role.get(4)];
    assert_eq!(role_flags, [false, false, false, false, false]);
    let runtime_member: i64 = admin
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_auth_members m \
             JOIN pg_catalog.pg_roles r ON r.oid = m.roleid \
             JOIN pg_catalog.pg_roles u ON u.oid = m.member \
             WHERE r.rolname = 'oo_calendar_view' AND u.rolname IN ('oo_r_calendar', 'oo_r_other')",
            &[],
        )
        .await
        .expect("runtime member")
        .get(0);
    assert_eq!(runtime_member, 0);
    let options: Vec<String> = admin
        .query_one(
            "SELECT c.reloptions FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'",
            &[],
        )
        .await
        .expect("reloptions")
        .get(0);
    let option_text = options.join(",");
    assert!(option_text.contains("security_barrier=true"), "{option_text}");
    assert!(option_text.contains("security_invoker=false"), "{option_text}");
    assert_eq!(snapshot_count(&admin).await, 6);
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar"]))
            .await
            .expect("replay after activate")
            .disposition,
        InstallDisposition::Replay
    );
}

#[tokio::test]
async fn restart_keeps_the_explicit_row() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let admin = connect_as(&fix, "oo_calendar_admin", &fix.admin, &fix.ca).await;
    let view = read_reader(&fix, &endpoint).await.expect("restart read");
    assert_view(&view, "America/Sao_Paulo", SP_AS_OF, "a297sp", sao_paulo_rows());
    assert_ne!(view.as_of_utc, wall_clock(&admin).await);
    let today = view.anchors.iter().find(|item| item.name == "today").expect("today");
    assert_eq!(today.midnight_status, "nonexistent");
    assert_eq!(today.midnight_utc, None);
    assert_eq!(snapshot_count(&admin).await, 6);
    let label: String = admin
        .query_one(
            "SELECT closure_label FROM oo_calendar_control.snapshot WHERE calendar_version = 'a297apr'",
            &[],
        )
        .await
        .expect("label")
        .get(0);
    assert_eq!(label, "a297-close-2026-03");
    let damaged: String = admin
        .query_one(
            "SELECT resolved_sha256 FROM oo_calendar_control.snapshot WHERE calendar_version = 'a297drift'",
            &[],
        )
        .await
        .expect("damaged")
        .get(0);
    assert_eq!(damaged, "0".repeat(64));
    let pointer: String = admin
        .query_one(
            "SELECT calendar_version || ' ' || pg_catalog.to_char(as_of AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
             FROM oo_calendar_control.active_pointer WHERE singleton",
            &[],
        )
        .await
        .expect("pointer text")
        .get(0);
    assert_eq!(pointer, format!("a297sp {SP_AS_OF}"));
    let project: String = admin
        .query_one(
            "SELECT project_id FROM oo_calendar_control.deployment WHERE singleton \
             AND finance_approval = false AND authenticated = false \
             AND ontology_published = false AND clock_used = false",
            &[],
        )
        .await
        .expect("deployment")
        .get(0);
    assert_eq!(project, PROJECT);
    let viewdef: String = admin
        .query_one("SELECT pg_catalog.pg_get_viewdef('oo_dict.calendar'::regclass, true)", &[])
        .await
        .expect("viewdef")
        .get(0);
    let lower = viewdef.to_ascii_lowercase();
    for token in ["now(", "date_trunc", "current_setting", "calendar_yaml", "closure_label", "report_json"] {
        assert!(!lower.contains(token), "{token}");
    }
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_calendar"]))
            .await
            .expect("replay after restart")
            .disposition,
        InstallDisposition::Replay
    );
}
