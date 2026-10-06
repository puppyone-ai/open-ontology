#![forbid(unsafe_code)]

//! PostgreSQL proof for one enum-store deployment.
//! The historical report digest is a handwritten literal.

use enum_history::{compile, CompileInput, Observation, Outcome};
use enum_store::{
    activate, install, migration_sha256, put_snapshot, read_view, report_sha256, ActivateReceipt, ActivateRequest,
    Endpoint, EnumColumn, InstallDisposition, InstallRequest, PutDisposition, PutReceipt, PutRequest, ReadRequest,
    StoreError, CONTRACT,
};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error};

const FIXTURE: &str = "/run/oo-a293/fixture";
const PROJECT: &str = "project-one";
const HISTORICAL_SHA: &str = "63fb59e7431cd131c512c3bfa1c61987c717d13bfe902552925d2b782649d9f6";
const OPEN: &str = include_str!("../../enum-history/tests/fixtures/open.yaml");
const HIST: &str = include_str!("../../enum-history/tests/fixtures/deprecated-historical.yaml");
const RETRO: &str = include_str!("../../enum-history/tests/fixtures/retroactive.yaml");
const INFERRED: &str = include_str!("../../enum-history/tests/fixtures/inferred-missing.yaml");
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
        fixture_admin: std::fs::read_to_string(root.join("fixture-admin.pass")).expect("fixture admin").trim().to_owned(),
    }
}

fn endpoint_of(fix: &Fix) -> Endpoint {
    Endpoint { host: fix.host.clone(), port: fix.port, database: fix.database.clone() }
}

fn observations(pairs: &[(&str, u64)]) -> Vec<Observation> {
    pairs.iter().map(|(code, count)| Observation { code: (*code).to_owned(), count: *count }).collect()
}

fn compiled(yaml: &str, as_of: &str, pairs: &[(&str, u64)], threshold: Option<&str>) -> enum_history::Report {
    match compile(&CompileInput {
        enum_yaml: yaml.to_owned(),
        as_of: as_of.to_owned(),
        observations: observations(pairs),
        threshold: threshold.map(str::to_owned),
    }) {
        Outcome::Compiled(report) => *report,
        Outcome::Refused(refusal) => panic!("{}", refusal.reason),
    }
}

fn digest(yaml: &str, as_of: &str, pairs: &[(&str, u64)], threshold: Option<&str>) -> String {
    report_sha256(&compiled(yaml, as_of, pairs, threshold)).expect("digest")
}

fn flags_false(finance: bool, semantic: bool, owner: bool, metric: bool, clock: bool) {
    assert!(!finance && !semantic && !owner && !metric && !clock);
}

fn assert_historical(row: &EnumColumn) {
    assert_eq!(row.enum_id, "erp_mysql.ysk.zt");
    assert_eq!(row.code, "4");
    assert_eq!(row.description, None);
    assert_eq!(row.class.as_deref(), Some("open"));
    assert_eq!(row.valid_from.as_deref(), Some("2019-01-01"));
    assert_eq!(row.valid_to.as_deref(), Some("2024-01-01"));
    assert_eq!(row.version, "hist-2020");
    assert_eq!(row.as_of, "2020-01-01");
    assert_eq!(row.project_id, PROJECT);
    assert_eq!(row.status, "deprecated");
}

async fn connect_as(fix: &Fix, user: &str, password: &str, ca: &[u8]) -> Client {
    let tls = enum_store::rustls_connector(ca).expect("connector");
    let mut config = tokio_postgres::Config::new();
    config.user(user);
    config.password(password);
    config.host(&fix.host);
    config.port(fix.port);
    config.dbname(&fix.database);
    config.ssl_mode(SslMode::Require);
    config.application_name("enum-store-harness");
    let (client, connection) = config.connect(tls).await.expect("harness connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn must_42501(client: &Client, sql: &str) {
    let err = client.query(sql, &[]).await.expect_err(sql);
    assert_eq!(sqlstate(&err), Some("42501"), "{sql} -> {err}");
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
    client.query_one("SELECT count(*) FROM oo_enum_control.snapshot", &[]).await.expect("count").get(0)
}

async fn pointer_id(client: &Client) -> Option<String> {
    client
        .query_one("SELECT snapshot_id FROM oo_enum_control.active_pointer WHERE singleton", &[])
        .await
        .expect("pointer")
        .get(0)
}

#[allow(clippy::too_many_arguments)]
async fn put_at(
    fix: &Fix,
    endpoint: &Endpoint,
    version: &str,
    yaml: &str,
    as_of: &str,
    pairs: &[(&str, u64)],
    threshold: Option<&str>,
    expected: &str,
) -> Result<PutReceipt, StoreError> {
    let observations = observations(pairs);
    put_snapshot(&PutRequest {
        endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        enum_version: version,
        enum_yaml: yaml,
        as_of,
        observations: &observations,
        threshold,
        expected_report_sha256: expected,
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
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        expected_current,
        snapshot_id: &receipt.snapshot_id,
        config_sha256: &receipt.config_sha256,
        enum_version: version,
        as_of: &receipt.as_of,
    })
    .await
}

async fn read_reader(fix: &Fix, endpoint: &Endpoint) -> Result<enum_store::ReadReceipt, StoreError> {
    read_view(&ReadRequest {
        endpoint,
        user: "oo_r_enum",
        password: &fix.reader,
        ca_pem: &fix.ca,
    })
    .await
}

fn install_request<'a>(endpoint: &'a Endpoint, fix: &'a Fix, project: &'a str, roles: &'a [&'a str]) -> InstallRequest<'a> {
    InstallRequest {
        endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: project,
        runtime_roles: roles,
    }
}

#[tokio::test]
async fn fresh_pg_contract() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    assert_eq!(fix.database, "oo_a293");
    assert_eq!(digest(HIST, "2020-01-01", &[("4", 3)], None), HISTORICAL_SHA);
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

    fixture.batch_execute("REVOKE CREATE ON DATABASE oo_a293 FROM oo_enum_admin").await.expect("revoke create");
    let rolled = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect_err("create revoked");
    assert_eq!(rolled, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    assert!(!schema_exists(&fixture, "oo_enum_control").await);
    fixture.batch_execute("GRANT CREATE ON DATABASE oo_a293 TO oo_enum_admin").await.expect("restore create");

    fixture.batch_execute("CREATE SCHEMA oo_dict; CREATE TABLE oo_dict.enums (n integer)").await.expect("foreign dict");
    let collision = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect_err("collision");
    assert_eq!(collision, StoreError::Collision { check: "oo_dict" });
    assert!(!schema_exists(&fixture, "oo_enum_control").await);
    assert!(schema_exists(&fixture, "oo_dict").await);
    fixture.batch_execute("DROP TABLE oo_dict.enums; DROP SCHEMA oo_dict").await.expect("drop foreign");

    let installed = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect("install");
    assert_eq!(installed.disposition, InstallDisposition::Installed);
    assert_eq!(installed.contract, CONTRACT);
    assert_eq!(installed.sql_sha256, migration_sha256().expect("hash"));
    assert!(installed.database_used);
    flags_false(
        installed.finance_certified,
        installed.semantic_certified,
        installed.owner_authenticated,
        installed.metric_certified,
        installed.clock_used,
    );
    let replay = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect("replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);
    let rebound = install(&install_request(&endpoint, &fix, "project-two", &["oo_r_enum"])).await.expect_err("other project");
    assert_eq!(rebound, StoreError::ProjectBinding);
    let allow = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum", "oo_r_other"]))
        .await
        .expect_err("allowlist");
    assert_eq!(allow, StoreError::Allowlist);

    let zeros = "0".repeat(64);
    fixture
        .execute("UPDATE oo_enum_control.install_manifest SET sql_sha256 = $1 WHERE singleton", &[&zeros])
        .await
        .expect("damage hash");
    let drift = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect_err("hash drift");
    assert_eq!(drift, StoreError::InstallDrift { check: "hash" });
    let real_hash = migration_sha256().expect("hash");
    fixture
        .execute("UPDATE oo_enum_control.install_manifest SET sql_sha256 = $1 WHERE singleton", &[&real_hash])
        .await
        .expect("restore hash");
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect("restored").disposition,
        InstallDisposition::Replay
    );

    fixture.batch_execute("ALTER TABLE oo_enum_control.snapshot ADD COLUMN extra integer").await.expect("extra");
    let shape = install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect_err("shape drift");
    assert_eq!(shape, StoreError::InstallDrift { check: "snapshot_columns" });
    let extra: i64 = fixture
        .query_one(
            "SELECT count(*) FROM information_schema.columns \
             WHERE table_schema = 'oo_enum_control' AND table_name = 'snapshot' AND column_name = 'extra'",
            &[],
        )
        .await
        .expect("extra remains")
        .get(0);
    assert_eq!(extra, 1);
    fixture.batch_execute("ALTER TABLE oo_enum_control.snapshot DROP COLUMN extra").await.expect("drop extra");

    let admin = connect_as(&fix, "oo_enum_admin", &fix.admin, &fix.ca).await;
    let wrong = "b".repeat(64);
    let mismatched = put_at(&fix, &endpoint, "hist-2020", HIST, "2020-01-01", &[("4", 3)], None, &wrong)
        .await
        .expect_err("wrong digest");
    assert_eq!(mismatched, StoreError::ReportMismatch);
    assert_eq!(snapshot_count(&admin).await, 0);
    let refused = put_at(&fix, &endpoint, "hist-2020", HIST, "not-a-date", &[("4", 3)], None, HISTORICAL_SHA)
        .await
        .expect_err("bad as_of");
    assert!(matches!(refused, StoreError::SnapshotRefused { reason: "invalid_as_of", .. }));
    let inferred = put_at(&fix, &endpoint, "inferred", INFERRED, "2026-01-01", &[("4", 1)], None, HISTORICAL_SHA)
        .await
        .expect_err("inferred");
    assert!(matches!(inferred, StoreError::SnapshotRefused { reason: "inferred_unapproved", .. }));
    assert_eq!(snapshot_count(&admin).await, 0);

    let wrong_tls = read_view(&ReadRequest {
        endpoint: &endpoint,
        user: "oo_r_enum",
        password: &fix.reader,
        ca_pem: &fix.wrong_ca,
    })
    .await
    .expect_err("wrong ca");
    assert!(matches!(wrong_tls, StoreError::Connect | StoreError::Tls), "{wrong_tls}");

    let reader = connect_as(&fix, "oo_r_enum", &fix.reader, &fix.ca).await;
    must_42501(&reader, "SELECT project_id FROM oo_dict.enums").await;
    let early = read_reader(&fix, &endpoint).await.expect_err("read before activate");
    assert_eq!(early, StoreError::Query { sqlstate: Some("42501".to_owned()) });

    let historical = put_at(&fix, &endpoint, "hist-2020", HIST, "2020-01-01", &[("4", 3)], None, HISTORICAL_SHA)
        .await
        .expect("historical");
    assert_eq!(historical.disposition, PutDisposition::Inserted);
    assert_eq!(historical.report_sha256, HISTORICAL_SHA);
    assert_eq!(historical.as_of, "2020-01-01");
    assert!(historical.database_used);
    flags_false(
        historical.finance_certified,
        historical.semantic_certified,
        historical.owner_authenticated,
        historical.metric_certified,
        historical.clock_used,
    );
    let again = put_at(&fix, &endpoint, "hist-2020", HIST, "2020-01-01", &[("4", 3)], None, HISTORICAL_SHA)
        .await
        .expect("replay");
    assert_eq!(again.disposition, PutDisposition::Replay);
    assert_eq!(again.snapshot_id, historical.snapshot_id);
    let spelled = digest(HIST, "2020-01-01", &[("4", 3)], Some("0.95"));
    assert_ne!(spelled, HISTORICAL_SHA);
    let spelling_drift = put_at(&fix, &endpoint, "hist-2020", HIST, "2020-01-01", &[("4", 3)], Some("0.95"), &spelled)
        .await
        .expect_err("defaulted drift");
    assert_eq!(spelling_drift, StoreError::SnapshotDrift);
    let defaulted: bool = admin
        .query_one("SELECT threshold_defaulted FROM oo_enum_control.snapshot WHERE enum_version = 'hist-2020'", &[])
        .await
        .expect("defaulted")
        .get(0);
    assert!(defaulted);

    let drift_hash = HISTORICAL_SHA;
    let drift_put = put_at(&fix, &endpoint, "drift-v", HIST, "2020-01-01", &[("4", 3)], None, drift_hash)
        .await
        .expect("drift version");
    assert_eq!(drift_put.disposition, PutDisposition::Inserted);
    admin
        .execute(
            "UPDATE oo_enum_control.published SET status = 'approved' \
             WHERE snapshot_id = $1 AND row_kind = 'definition'",
            &[&drift_put.snapshot_id],
        )
        .await
        .expect("damage status");
    let damaged = put_at(&fix, &endpoint, "drift-v", HIST, "2020-01-01", &[("4", 3)], None, drift_hash)
        .await
        .expect_err("status drift");
    assert_eq!(damaged, StoreError::SnapshotDrift);
    let stored_status: String = admin
        .query_one(
            "SELECT status FROM oo_enum_control.published WHERE snapshot_id = $1 AND row_kind = 'definition'",
            &[&drift_put.snapshot_id],
        )
        .await
        .expect("damaged status")
        .get(0);
    assert_eq!(stored_status, "approved");
    let kept: String = admin
        .query_one(
            "SELECT status FROM oo_enum_control.published WHERE snapshot_id = $1 AND row_kind = 'definition'",
            &[&historical.snapshot_id],
        )
        .await
        .expect("historical status")
        .get(0);
    assert_eq!(kept, "deprecated");

    let stale = "a".repeat(64);
    let cas = activate_at(&fix, &endpoint, "hist-2020", &historical, Some(&stale)).await.expect_err("stale cas");
    assert_eq!(cas, StoreError::CasConflict);
    assert_eq!(pointer_id(&admin).await, None);
    let wrong_config = activate(&ActivateRequest {
        endpoint: &endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        expected_current: None,
        snapshot_id: &historical.snapshot_id,
        config_sha256: &stale,
        enum_version: "hist-2020",
        as_of: "2020-01-01",
    })
    .await
    .expect_err("wrong config");
    assert_eq!(wrong_config, StoreError::Activation);
    assert_eq!(pointer_id(&admin).await, None);

    fixture.batch_execute("REVOKE oo_enum_view FROM oo_enum_admin").await.expect("revoke member");
    let atomic = activate_at(&fix, &endpoint, "hist-2020", &historical, None).await.expect_err("grant atomic");
    assert_eq!(atomic, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    assert_eq!(pointer_id(&admin).await, None);
    let served_after: i64 = admin.query_one("SELECT count(*) FROM oo_enum_control.served", &[]).await.expect("served").get(0);
    assert_eq!(served_after, 0);
    fixture.batch_execute("GRANT oo_enum_view TO oo_enum_admin").await.expect("restore member");

    let retro_hash = digest(RETRO, "2023-12-31", &[("4", 8)], None);
    let retro = put_at(&fix, &endpoint, "retro-2023", RETRO, "2023-12-31", &[("4", 8)], None, &retro_hash)
        .await
        .expect("retro");
    let retro_later_hash = digest(RETRO, "2024-01-01", &[("4", 8)], None);
    put_at(&fix, &endpoint, "retro-2024", RETRO, "2024-01-01", &[("4", 8)], None, &retro_later_hash)
        .await
        .expect("retro later");
    let cov_hash = digest(OPEN, "2026-01-01", &[("1", 95), ("9", 5)], None);
    let cov = put_at(&fix, &endpoint, "cov-2026", OPEN, "2026-01-01", &[("1", 95), ("9", 5)], None, &cov_hash)
        .await
        .expect("coverage");
    let below_hash = digest(OPEN, "2026-01-01", &[("1", 94), ("9", 6)], None);
    put_at(&fix, &endpoint, "below-2026", OPEN, "2026-01-01", &[("1", 94), ("9", 6)], None, &below_hash)
        .await
        .expect("below");
    assert_eq!(snapshot_count(&admin).await, 6);

    let (left, right) = tokio::join!(
        activate_at(&fix, &endpoint, "retro-2023", &retro, None),
        activate_at(&fix, &endpoint, "cov-2026", &cov, None),
    );
    let mut winner = String::new();
    let mut conflicts = 0;
    for result in [left, right] {
        match result {
            Ok(receipt) => {
                assert!(winner.is_empty(), "two winners");
                winner = receipt.snapshot_id;
            }
            Err(StoreError::CasConflict) => conflicts += 1,
            Err(err) => panic!("race failed: {err}"),
        }
    }
    assert_eq!(conflicts, 1);
    assert!(winner == retro.snapshot_id || winner == cov.snapshot_id);
    assert_eq!(pointer_id(&admin).await.as_deref(), Some(winner.as_str()));
    if winner != cov.snapshot_id {
        activate_at(&fix, &endpoint, "cov-2026", &cov, Some(&winner)).await.expect("show coverage");
    }
    let coverage_view = read_reader(&fix, &endpoint).await.expect("coverage view");
    assert!(coverage_view.database_used);
    flags_false(
        coverage_view.finance_certified,
        coverage_view.semantic_certified,
        coverage_view.owner_authenticated,
        coverage_view.metric_certified,
        coverage_view.clock_used,
    );
    assert_eq!(coverage_view.rows.len(), 2);
    assert_eq!(coverage_view.rows[0].code, "1");
    assert_eq!(coverage_view.rows[0].status, "approved");
    assert_eq!(coverage_view.rows[0].class.as_deref(), Some("open"));
    assert_eq!(coverage_view.rows[0].description, None);
    assert_eq!(coverage_view.rows[1].code, "9");
    assert_eq!(coverage_view.rows[1].status, "unexplained");
    assert_eq!(coverage_view.rows[1].class, None);
    assert_eq!(coverage_view.rows[1].description, None);

    let final_act = activate_at(&fix, &endpoint, "hist-2020", &historical, Some(&cov.snapshot_id))
        .await
        .expect("final historical");
    assert_eq!(final_act.disposition, "activated");
    assert_eq!(final_act.grants, 1);
    assert_eq!(final_act.snapshot_id, historical.snapshot_id);
    let view = read_reader(&fix, &endpoint).await.expect("historical view");
    assert_eq!(view.contract, CONTRACT);
    assert_eq!(view.rows.len(), 1);
    assert_historical(&view.rows[0]);
    let today: String = admin.query_one("SELECT current_date::text", &[]).await.expect("today").get(0);
    assert_ne!(view.rows[0].as_of, today);
    assert_eq!(view.rows[0].as_of, "2020-01-01");

    must_42501(&reader, "UPDATE oo_dict.enums SET status = 'approved'").await;
    must_42501(&reader, "DELETE FROM oo_dict.enums").await;
    must_42501(
        &reader,
        "INSERT INTO oo_dict.enums (enum_id, code, description, class, valid_from, valid_to, version, as_of, project_id, status) \
         VALUES ('erp_mysql.ysk.zt', '4', NULL, 'open', NULL, NULL, 'hist-2020', '2020-01-01', 'project-one', 'approved')",
    )
    .await;
    must_42501(&reader, "SELECT enum_yaml FROM oo_enum_control.snapshot").await;
    must_42501(&reader, "SELECT report_json FROM oo_enum_control.snapshot").await;
    must_42501(&reader, "SELECT code FROM oo_enum_control.notice").await;
    must_42501(&reader, "SELECT status FROM oo_enum_control.published").await;
    must_42501(&reader, "SELECT status FROM oo_enum_control.served").await;
    must_42501(&reader, "SELECT id FROM biz.receipts").await;
    admin.batch_execute("SET ROLE oo_enum_view").await.expect("set role");
    must_42501(&admin, "SELECT enum_yaml FROM oo_enum_control.snapshot").await;
    must_42501(&admin, "SELECT id FROM biz.receipts").await;
    admin.batch_execute("RESET ROLE").await.expect("reset role");
    let other = connect_as(&fix, "oo_r_other", &fix.other, &fix.ca).await;
    must_42501(&other, "SELECT code FROM oo_dict.enums").await;
    let admin_read = read_view(&ReadRequest {
        endpoint: &endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
    })
    .await
    .expect_err("admin is not a runtime reader");
    assert_eq!(admin_read, StoreError::LoginRole);

    let code: String = admin
        .query_one(
            "SELECT n.code FROM oo_enum_control.notice n \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
             WHERE s.enum_version = 'cov-2026'",
            &[],
        )
        .await
        .expect("unknown code")
        .get(0);
    let reason: String = admin
        .query_one(
            "SELECT n.reason FROM oo_enum_control.notice n \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
             WHERE s.enum_version = 'cov-2026'",
            &[],
        )
        .await
        .expect("unknown reason")
        .get(0);
    let count_text: String = admin
        .query_one(
            "SELECT n.count_text FROM oo_enum_control.notice n \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
             WHERE s.enum_version = 'cov-2026'",
            &[],
        )
        .await
        .expect("unknown count")
        .get(0);
    assert_eq!(code, "9");
    assert_eq!(reason, "unknown_code");
    assert_eq!(count_text, "5");
    let ratio: String = admin
        .query_one("SELECT coverage_ratio FROM oo_enum_control.snapshot WHERE enum_version = 'cov-2026'", &[])
        .await
        .expect("ratio")
        .get(0);
    let comparison: String = admin
        .query_one("SELECT comparison FROM oo_enum_control.snapshot WHERE enum_version = 'cov-2026'", &[])
        .await
        .expect("comparison")
        .get(0);
    let decimal: String = admin
        .query_one("SELECT threshold_decimal FROM oo_enum_control.snapshot WHERE enum_version = 'cov-2026'", &[])
        .await
        .expect("decimal")
        .get(0);
    let met: bool = admin
        .query_one("SELECT threshold_met FROM oo_enum_control.snapshot WHERE enum_version = 'cov-2026'", &[])
        .await
        .expect("met")
        .get(0);
    let review: bool = admin
        .query_one("SELECT review_needed FROM oo_enum_control.snapshot WHERE enum_version = 'cov-2026'", &[])
        .await
        .expect("review")
        .get(0);
    assert_eq!(ratio, "95/100");
    assert_eq!(comparison, "equal");
    assert_eq!(decimal, "0.95");
    assert!(met);
    assert!(review);
    let below_comparison: String = admin
        .query_one("SELECT comparison FROM oo_enum_control.snapshot WHERE enum_version = 'below-2026'", &[])
        .await
        .expect("below comparison")
        .get(0);
    let below_met: bool = admin
        .query_one("SELECT threshold_met FROM oo_enum_control.snapshot WHERE enum_version = 'below-2026'", &[])
        .await
        .expect("below met")
        .get(0);
    let below_ratio: String = admin
        .query_one("SELECT coverage_ratio FROM oo_enum_control.snapshot WHERE enum_version = 'below-2026'", &[])
        .await
        .expect("below ratio")
        .get(0);
    assert_eq!(below_comparison, "below");
    assert!(!below_met);
    assert_eq!(below_ratio, "94/100");

    let retro_status: Vec<String> = admin
        .query(
            "SELECT p.status FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = 'retro-2023' AND p.row_kind = 'definition' ORDER BY p.line_no",
            &[],
        )
        .await
        .expect("retro status")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(retro_status, vec!["superseded".to_owned(), "approved".to_owned()]);
    let later_status: Vec<String> = admin
        .query(
            "SELECT p.status FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = 'retro-2024' AND p.row_kind = 'definition' ORDER BY p.line_no",
            &[],
        )
        .await
        .expect("later status")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(later_status, vec!["superseded".to_owned(), "approved".to_owned()]);
    let later_class: Vec<String> = admin
        .query(
            "SELECT p.class FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = 'retro-2024' AND p.row_kind = 'definition' ORDER BY p.line_no",
            &[],
        )
        .await
        .expect("later class")
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(later_class, vec!["open".to_owned(), "closed".to_owned()]);

    let columns: Vec<String> = admin
        .query(
            "SELECT a.attname FROM pg_catalog.pg_attribute a \
             JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_dict' AND c.relname = 'enums' AND a.attnum > 0 AND NOT a.attisdropped \
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
            "enum_id",
            "code",
            "description",
            "class",
            "valid_from",
            "valid_to",
            "version",
            "as_of",
            "project_id",
            "status",
        ]
    );
    let viewdef: String = admin
        .query_one("SELECT pg_catalog.pg_get_viewdef('oo_dict.enums'::regclass, true)", &[])
        .await
        .expect("viewdef")
        .get(0);
    let lower = viewdef.to_ascii_lowercase();
    for token in ["now(", "current_date", "current_timestamp", "clock_timestamp", "date_trunc", "current_setting", "enum_yaml", "report_json", "row_kind"] {
        assert!(!lower.contains(token), "{token} in {viewdef}");
    }
    let owner: String = admin
        .query_one(
            "SELECT pg_catalog.pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_dict' AND c.relname = 'enums'",
            &[],
        )
        .await
        .expect("owner")
        .get(0);
    assert_eq!(owner, "oo_enum_view");
    let flag_row = admin
        .query_one(
            "SELECT finance_certified, semantic_certified, owner_authenticated, metric_certified, clock_used \
             FROM oo_enum_control.deployment WHERE singleton",
            &[],
        )
        .await
        .expect("flags");
    let stored: [bool; 5] = [flag_row.get(0), flag_row.get(1), flag_row.get(2), flag_row.get(3), flag_row.get(4)];
    assert_eq!(stored, [false, false, false, false, false]);
    let report_flags: String = admin
        .query_one("SELECT report_json FROM oo_enum_control.snapshot WHERE enum_version = 'hist-2020'", &[])
        .await
        .expect("report")
        .get(0);
    assert!(report_flags.contains("\"database_used\":false"));
    assert!(report_flags.contains("\"owner_authenticated\":false"));
    assert!(report_flags.contains("\"metric_certified\":false"));
    assert_eq!(pointer_id(&admin).await.as_deref(), Some(historical.snapshot_id.as_str()));
}

#[tokio::test]
async fn restart_snapshot_persists() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let admin = connect_as(&fix, "oo_enum_admin", &fix.admin, &fix.ca).await;
    let view = read_reader(&fix, &endpoint).await.expect("restart read");
    assert_eq!(view.rows.len(), 1);
    assert_historical(&view.rows[0]);
    let today: String = admin.query_one("SELECT current_date::text", &[]).await.expect("today").get(0);
    assert_ne!(view.rows[0].as_of, today);
    assert_eq!(snapshot_count(&admin).await, 6);
    let status: String = admin
        .query_one(
            "SELECT p.status FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = 'hist-2020' AND p.row_kind = 'definition'",
            &[],
        )
        .await
        .expect("status")
        .get(0);
    assert_eq!(status, "deprecated");
    let damaged: String = admin
        .query_one(
            "SELECT p.status FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = 'drift-v' AND p.row_kind = 'definition'",
            &[],
        )
        .await
        .expect("damaged")
        .get(0);
    assert_eq!(damaged, "approved");
    let reason: String = admin
        .query_one(
            "SELECT n.reason FROM oo_enum_control.notice n \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
             WHERE s.enum_version = 'cov-2026'",
            &[],
        )
        .await
        .expect("notice")
        .get(0);
    assert_eq!(reason, "unknown_code");
    let ratio: String = admin
        .query_one("SELECT coverage_ratio FROM oo_enum_control.snapshot WHERE enum_version = 'cov-2026'", &[])
        .await
        .expect("ratio")
        .get(0);
    assert_eq!(ratio, "95/100");
    let stored_sha: String = admin
        .query_one("SELECT report_sha256 FROM oo_enum_control.snapshot WHERE enum_version = 'hist-2020'", &[])
        .await
        .expect("sha")
        .get(0);
    assert_eq!(stored_sha, HISTORICAL_SHA);
    let replay = put_at(&fix, &endpoint, "hist-2020", HIST, "2020-01-01", &[("4", 3)], None, HISTORICAL_SHA)
        .await
        .expect("replay after restart");
    assert_eq!(replay.disposition, PutDisposition::Replay);
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect("install replay").disposition,
        InstallDisposition::Replay
    );
}
