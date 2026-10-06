#![forbid(unsafe_code)]

//! Independent enum-store proof. The YAML, dates, codes, and ratios are
//! handwritten for this review. They are not the author fixtures.

use enum_history::{compile, Comparison, CompileInput, Observation, Outcome};
use enum_store::{
    activate, install, migration_sha256, put_snapshot, read_view, report_sha256, sha256_hex, ActivateReceipt,
    ActivateRequest, Endpoint, EnumColumn, InstallDisposition, InstallRequest, PutDisposition, PutReceipt, PutRequest,
    ReadRequest, StoreError, CONTRACT,
};
use tokio_postgres::config::SslMode;
use tokio_postgres::{Client, Error};

const FIXTURE: &str = "/run/oo-a306/fixture";
const PROJECT: &str = "a306-receipts";
const ENUM_ID: &str = "a306.receipt.state";
const HIST_VERSION: &str = "hist-2019";
const DRIFT_VERSION: &str = "drift-2019";
const RETRO_VERSION: &str = "retro-2021";
const LATER_VERSION: &str = "retro-2022";
const COV_VERSION: &str = "cov-2025";
const BELOW_VERSION: &str = "below-2025";
const HIST_AS_OF: &str = "2019-07-04";
const RETRO_AS_OF: &str = "2021-12-15";
const LATER_AS_OF: &str = "2022-01-01";
const COV_AS_OF: &str = "2025-11-02";

const HISTORICAL: &str = r#"schema_version: oos/v1
enum: a306.receipt.state
owner: ledger.clerk
classes:
  open: still receivable
values:
  - code: "7"
    label: prior open receipt
    class: open
    status: deprecated
    source: human
    valid_from: "2018-03-01"
    valid_to: "2021-06-01"
    review: { state: approved, by: reviewer.chen }
"#;

const RETRO: &str = r#"schema_version: oos/v1
enum: a306.receipt.state
owner: ledger.clerk
classes:
  open: still receivable
  closed: settled receipt
coverage: 0.40
values:
  - code: "7"
    label: prior open receipt
    class: open
    status: superseded
    source: inferred
    valid_from: "2018-03-01"
    valid_to: "2022-01-01"
    review: { state: approved, by: reviewer.chen, at: "2022-02-01" }
  - code: "7"
    label: closed receipt
    class: closed
    status: approved
    source: inferred
    valid_from: "2022-01-01"
    review: { state: approved, by: reviewer.chen, at: "2022-02-01" }
"#;

const COVERAGE: &str = r#"schema_version: oos/v1
enum: a306.receipt.state
classes:
  open: still receivable
values:
  - code: "7"
    class: open
    status: approved
    source: dictionary_table
"#;

const INFERRED: &str = r#"schema_version: oos/v1
enum: a306.receipt.state
classes:
  closed: settled receipt
values:
  - code: "7"
    class: closed
    status: approved
    source: inferred
"#;

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

struct Hands {
    historical_sha: String,
    spelled_sha: String,
    retro_sha: String,
    later_sha: String,
    cov_sha: String,
    below_sha: String,
}

struct Snap<'a> {
    version: &'a str,
    yaml: &'a str,
    as_of: &'a str,
    pairs: &'a [(&'a str, u64)],
    threshold: Option<&'a str>,
    expected: &'a str,
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

fn compile_report(yaml: &str, as_of: &str, pairs: &[(&str, u64)], threshold: Option<&str>) -> enum_history::Report {
    match compile(&CompileInput {
        enum_yaml: yaml.to_owned(),
        as_of: as_of.to_owned(),
        observations: observations(pairs),
        threshold: threshold.map(str::to_owned),
    }) {
        Outcome::Compiled(report) => *report,
        Outcome::Refused(refusal) => panic!("{} {}", refusal.reason, refusal.detail),
    }
}

fn digest_of(report: &enum_history::Report) -> String {
    let json = serde_json::to_string(report).expect("report json");
    let again = sha256_hex(json.as_bytes()).expect("sha");
    let via_store = report_sha256(report).expect("store digest");
    assert_eq!(again, via_store);
    assert!(json.contains("\"database_used\":false"));
    assert!(json.contains("\"owner_authenticated\":false"));
    assert!(json.contains("\"metric_certified\":false"));
    assert!(json.contains("\"source_modified\":false"));
    assert!(json.contains("\"oo_dict_written\":false"));
    assert!(json.contains("\"sweeper_ran\":false"));
    via_store
}

fn flags_false(finance: bool, semantic: bool, owner: bool, metric: bool, clock: bool) {
    assert!(!finance);
    assert!(!semantic);
    assert!(!owner);
    assert!(!metric);
    assert!(!clock);
}

fn by_code<'a>(rows: &'a [EnumColumn], code: &str) -> &'a EnumColumn {
    match rows.iter().find(|row| row.code == code) {
        Some(row) => row,
        None => panic!("missing code {code}"),
    }
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
    config.application_name("enum-store-review");
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

fn snap<'a>(
    version: &'a str,
    yaml: &'a str,
    as_of: &'a str,
    pairs: &'a [(&'a str, u64)],
    threshold: Option<&'a str>,
    expected: &'a str,
) -> Snap<'a> {
    Snap { version, yaml, as_of, pairs, threshold, expected }
}

async fn put_at(fix: &Fix, endpoint: &Endpoint, snap: Snap<'_>) -> Result<PutReceipt, StoreError> {
    let observed = observations(snap.pairs);
    put_snapshot(&PutRequest {
        endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        enum_version: snap.version,
        enum_yaml: snap.yaml,
        as_of: snap.as_of,
        observations: &observed,
        threshold: snap.threshold,
        expected_report_sha256: snap.expected,
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

async fn text_at(client: &Client, sql: &str, version: &str) -> String {
    client.query_one(sql, &[&version]).await.expect(sql).get(0)
}

fn handwritten() -> Hands {
    let historical = compile_report(HISTORICAL, HIST_AS_OF, &[("7", 4)], None);
    assert_eq!(historical.enum_name, ENUM_ID);
    assert_eq!(historical.as_of, HIST_AS_OF);
    assert_eq!(historical.rows[0].status, "deprecated");
    assert_eq!(historical.rows[0].class.as_deref(), Some("open"));
    assert_eq!(historical.rows[0].label.as_deref(), Some("prior open receipt"));
    assert!(historical.rows[0].historical_business_class);
    assert_eq!(historical.rows[0].valid_from.as_deref(), Some("2018-03-01"));
    assert_eq!(historical.rows[0].valid_to.as_deref(), Some("2021-06-01"));
    assert!(historical.threshold.defaulted);
    assert_eq!(historical.threshold.decimal, "0.95");
    assert_eq!(historical.observed_coverage.ratio.as_deref(), Some("4/4"));
    assert_eq!(historical.comparison, Some(Comparison::Above));
    assert_eq!(historical.threshold_met, Some(true));
    assert!(!historical.review_needed);
    assert!(!historical.database_used);
    assert!(!historical.owner_authenticated);
    assert!(!historical.metric_certified);
    let historical_sha = digest_of(&historical);
    let spelled = compile_report(HISTORICAL, HIST_AS_OF, &[("7", 4)], Some("0.95"));
    assert!(!spelled.threshold.defaulted);
    let spelled_sha = digest_of(&spelled);
    assert_ne!(spelled_sha, historical_sha);

    let retro = compile_report(RETRO, RETRO_AS_OF, &[("7", 8)], None);
    assert_eq!(retro.rows[0].status, "superseded");
    assert!(retro.rows[0].historical_business_class);
    assert_eq!(retro.rows[0].class.as_deref(), Some("open"));
    assert_eq!(retro.rows[1].status, "approved");
    assert!(!retro.rows[1].historical_business_class);
    assert_eq!(retro.rows[1].class.as_deref(), Some("closed"));
    let later = compile_report(RETRO, LATER_AS_OF, &[("7", 8)], None);
    assert_eq!(later.rows[0].status, "superseded");
    assert_eq!(later.rows[1].status, "approved");
    assert_eq!(later.rows[0].class.as_deref(), Some("open"));
    assert_eq!(later.rows[1].class.as_deref(), Some("closed"));

    let cov = compile_report(COVERAGE, COV_AS_OF, &[("7", 95), ("8", 5)], None);
    assert_eq!(cov.comparison, Some(Comparison::Equal));
    assert_eq!(cov.threshold_met, Some(true));
    assert!(cov.review_needed);
    assert_eq!(cov.observed_coverage.ratio.as_deref(), Some("95/100"));
    assert_eq!(cov.threshold.decimal, "0.95");
    assert_eq!(cov.warnings.len(), 1);
    assert_eq!(cov.warnings[0].code, "8");
    assert_eq!(cov.warnings[0].reason, "unknown_code");
    assert_eq!(cov.warnings[0].count, "5");
    let below = compile_report(COVERAGE, COV_AS_OF, &[("7", 94), ("8", 6)], None);
    assert_eq!(below.comparison, Some(Comparison::Below));
    assert_eq!(below.threshold_met, Some(false));
    assert!(below.review_needed);
    assert_eq!(below.observed_coverage.ratio.as_deref(), Some("94/100"));
    assert_eq!(below.warnings[0].code, "8");
    assert_eq!(below.warnings[0].count, "6");

    let hands = Hands {
        historical_sha,
        spelled_sha,
        retro_sha: digest_of(&retro),
        later_sha: digest_of(&later),
        cov_sha: digest_of(&cov),
        below_sha: digest_of(&below),
    };
    assert_ne!(hands.cov_sha, hands.below_sha);
    println!("a306 historical_sha={}", hands.historical_sha);
    println!("a306 coverage95_sha={}", hands.cov_sha);
    println!("a306 coverage94_sha={}", hands.below_sha);
    hands
}

async fn reset_database(fix: &Fix) -> Client {
    let fixture = connect_as(fix, "oo_fixture_admin", &fix.fixture_admin, &fix.ca).await;
    fixture
        .batch_execute(
            "DROP SCHEMA IF EXISTS oo_enum_control CASCADE; \
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
}

async fn install_boundaries(fix: &Fix, endpoint: &Endpoint, fixture: &Client) {
    fixture.batch_execute("REVOKE CREATE ON DATABASE oo_a293 FROM oo_enum_admin").await.expect("revoke create");
    let rolled = install(&install_request(endpoint, fix, PROJECT, &["oo_r_enum"])).await.expect_err("create revoked");
    assert_eq!(rolled, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    assert!(!schema_exists(fixture, "oo_enum_control").await);
    fixture.batch_execute("GRANT CREATE ON DATABASE oo_a293 TO oo_enum_admin").await.expect("restore create");

    fixture.batch_execute("CREATE SCHEMA oo_dict; CREATE TABLE oo_dict.enums (n integer)").await.expect("foreign dict");
    let collision = install(&install_request(endpoint, fix, PROJECT, &["oo_r_enum"])).await.expect_err("collision");
    assert_eq!(collision, StoreError::Collision { check: "oo_dict" });
    assert!(!schema_exists(fixture, "oo_enum_control").await);
    assert!(schema_exists(fixture, "oo_dict").await);
    fixture.batch_execute("DROP TABLE oo_dict.enums; DROP SCHEMA oo_dict").await.expect("drop foreign");

    let installed = install(&install_request(endpoint, fix, PROJECT, &["oo_r_enum"])).await.expect("install");
    assert_eq!(installed.disposition, InstallDisposition::Installed);
    assert_eq!(installed.contract, CONTRACT);
    assert_eq!(installed.project_id, PROJECT);
    assert_eq!(installed.sql_sha256, migration_sha256().expect("migration hash"));
    assert!(installed.database_used);
    flags_false(
        installed.finance_certified,
        installed.semantic_certified,
        installed.owner_authenticated,
        installed.metric_certified,
        installed.clock_used,
    );
    let replay = install(&install_request(endpoint, fix, PROJECT, &["oo_r_enum"])).await.expect("replay");
    assert_eq!(replay.disposition, InstallDisposition::Replay);
    let other = install(&install_request(endpoint, fix, "a306-other", &["oo_r_enum"])).await.expect_err("other project");
    assert_eq!(other, StoreError::ProjectBinding);
    let allow = install(&install_request(endpoint, fix, PROJECT, &["oo_r_enum", "oo_r_other"]))
        .await
        .expect_err("allowlist");
    assert_eq!(allow, StoreError::Allowlist);

    let zeros = "0".repeat(64);
    fixture
        .execute("UPDATE oo_enum_control.install_manifest SET sql_sha256 = $1 WHERE singleton", &[&zeros])
        .await
        .expect("damage hash");
    let drift = install(&install_request(endpoint, fix, PROJECT, &["oo_r_enum"])).await.expect_err("hash drift");
    assert_eq!(drift, StoreError::InstallDrift { check: "hash" });
    let real_hash = migration_sha256().expect("hash");
    fixture
        .execute("UPDATE oo_enum_control.install_manifest SET sql_sha256 = $1 WHERE singleton", &[&real_hash])
        .await
        .expect("restore hash");
    assert_eq!(
        install(&install_request(endpoint, fix, PROJECT, &["oo_r_enum"])).await.expect("restored").disposition,
        InstallDisposition::Replay
    );
    fixture.batch_execute("ALTER TABLE oo_enum_control.snapshot ADD COLUMN extra integer").await.expect("extra");
    let shape = install(&install_request(endpoint, fix, PROJECT, &["oo_r_enum"])).await.expect_err("shape drift");
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
}

async fn refuse_before_insert(fix: &Fix, endpoint: &Endpoint, admin: &Client, hands: &Hands) {
    let wrong = "b".repeat(64);
    let mismatched = put_at(fix, endpoint, snap(HIST_VERSION, HISTORICAL, HIST_AS_OF, &[("7", 4)], None, &wrong))
        .await
        .expect_err("wrong digest");
    assert_eq!(mismatched, StoreError::ReportMismatch);
    assert_eq!(snapshot_count(admin).await, 0);
    let refused = put_at(fix, endpoint, snap(HIST_VERSION, HISTORICAL, "not-a-date", &[("7", 4)], None, &hands.historical_sha))
        .await
        .expect_err("bad as_of");
    assert!(matches!(refused, StoreError::SnapshotRefused { reason: "invalid_as_of", .. }));
    let inferred = put_at(fix, endpoint, snap("inferred-2019", INFERRED, "2026-01-01", &[("7", 1)], None, &wrong))
        .await
        .expect_err("inferred");
    assert!(matches!(inferred, StoreError::SnapshotRefused { reason: "inferred_unapproved", .. }));
    assert_eq!(snapshot_count(admin).await, 0);
    let wrong_tls = read_view(&ReadRequest {
        endpoint,
        user: "oo_r_enum",
        password: &fix.reader,
        ca_pem: &fix.wrong_ca,
    })
    .await
    .expect_err("wrong ca");
    assert!(matches!(wrong_tls, StoreError::Connect | StoreError::Tls), "{wrong_tls}");
    let reader = connect_as(fix, "oo_r_enum", &fix.reader, &fix.ca).await;
    must_42501(&reader, "SELECT project_id FROM oo_dict.enums").await;
    let early = read_reader(fix, endpoint).await.expect_err("read before activate");
    assert_eq!(early, StoreError::Query { sqlstate: Some("42501".to_owned()) });
}

async fn put_historical(fix: &Fix, endpoint: &Endpoint, admin: &Client, hands: &Hands) -> PutReceipt {
    let historical = put_at(fix, endpoint, snap(HIST_VERSION, HISTORICAL, HIST_AS_OF, &[("7", 4)], None, &hands.historical_sha))
        .await
        .expect("historical");
    assert_eq!(historical.disposition, PutDisposition::Inserted);
    assert_eq!(historical.report_sha256, hands.historical_sha);
    assert_eq!(historical.as_of, HIST_AS_OF);
    assert!(historical.database_used);
    flags_false(
        historical.finance_certified,
        historical.semantic_certified,
        historical.owner_authenticated,
        historical.metric_certified,
        historical.clock_used,
    );
    let again = put_at(fix, endpoint, snap(HIST_VERSION, HISTORICAL, HIST_AS_OF, &[("7", 4)], None, &hands.historical_sha))
        .await
        .expect("replay");
    assert_eq!(again.disposition, PutDisposition::Replay);
    assert_eq!(again.snapshot_id, historical.snapshot_id);
    let spelling = put_at(
        fix,
        endpoint,
        snap(HIST_VERSION, HISTORICAL, HIST_AS_OF, &[("7", 4)], Some("0.95"), &hands.spelled_sha),
    )
    .await
    .expect_err("defaulted drift");
    assert_eq!(spelling, StoreError::SnapshotDrift);
    let defaulted = text_at(
        admin,
        "SELECT threshold_defaulted::text FROM oo_enum_control.snapshot WHERE enum_version = $1",
        HIST_VERSION,
    )
    .await;
    assert_eq!(defaulted, "true");
    let ratio = text_at(admin, "SELECT coverage_ratio FROM oo_enum_control.snapshot WHERE enum_version = $1", HIST_VERSION).await;
    assert_eq!(ratio, "4/4");

    let drift = put_at(fix, endpoint, snap(DRIFT_VERSION, HISTORICAL, HIST_AS_OF, &[("7", 4)], None, &hands.historical_sha))
        .await
        .expect("drift version");
    assert_eq!(drift.disposition, PutDisposition::Inserted);
    admin
        .execute(
            "UPDATE oo_enum_control.published SET status = 'approved' \
             WHERE snapshot_id = $1 AND row_kind = 'definition'",
            &[&drift.snapshot_id],
        )
        .await
        .expect("damage status");
    let damaged = put_at(fix, endpoint, snap(DRIFT_VERSION, HISTORICAL, HIST_AS_OF, &[("7", 4)], None, &hands.historical_sha))
        .await
        .expect_err("status drift");
    assert_eq!(damaged, StoreError::SnapshotDrift);
    let stored = text_at(
        admin,
        "SELECT p.status FROM oo_enum_control.published p \
         JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
         WHERE s.enum_version = $1 AND p.row_kind = 'definition'",
        DRIFT_VERSION,
    )
    .await;
    assert_eq!(stored, "approved");
    let kept = text_at(
        admin,
        "SELECT p.status FROM oo_enum_control.published p \
         JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
         WHERE s.enum_version = $1 AND p.row_kind = 'definition'",
        HIST_VERSION,
    )
    .await;
    assert_eq!(kept, "deprecated");
    historical
}

async fn cas_and_rollback(fix: &Fix, endpoint: &Endpoint, admin: &Client, fixture: &Client, historical: &PutReceipt) {
    let stale = "a".repeat(64);
    let cas = activate_at(fix, endpoint, HIST_VERSION, historical, Some(&stale)).await.expect_err("stale cas");
    assert_eq!(cas, StoreError::CasConflict);
    assert_eq!(pointer_id(admin).await, None);
    let wrong_config = activate(&ActivateRequest {
        endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
        project_id: PROJECT,
        expected_current: None,
        snapshot_id: &historical.snapshot_id,
        config_sha256: &stale,
        enum_version: HIST_VERSION,
        as_of: HIST_AS_OF,
    })
    .await
    .expect_err("wrong config");
    assert_eq!(wrong_config, StoreError::Activation);
    assert_eq!(pointer_id(admin).await, None);

    fixture.batch_execute("REVOKE oo_enum_view FROM oo_enum_admin").await.expect("revoke member");
    let atomic = activate_at(fix, endpoint, HIST_VERSION, historical, None).await.expect_err("grant atomic");
    assert_eq!(atomic, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    assert_eq!(pointer_id(admin).await, None);
    let served_after: i64 = admin.query_one("SELECT count(*) FROM oo_enum_control.served", &[]).await.expect("served").get(0);
    assert_eq!(served_after, 0);
    fixture.batch_execute("GRANT oo_enum_view TO oo_enum_admin").await.expect("restore member");
}

async fn race_and_serve(fix: &Fix, endpoint: &Endpoint, admin: &Client, hands: &Hands, historical: &PutReceipt) -> PutReceipt {
    let retro = put_at(fix, endpoint, snap(RETRO_VERSION, RETRO, RETRO_AS_OF, &[("7", 8)], None, &hands.retro_sha))
        .await
        .expect("retro");
    put_at(fix, endpoint, snap(LATER_VERSION, RETRO, LATER_AS_OF, &[("7", 8)], None, &hands.later_sha))
        .await
        .expect("retro later");
    let cov = put_at(fix, endpoint, snap(COV_VERSION, COVERAGE, COV_AS_OF, &[("7", 95), ("8", 5)], None, &hands.cov_sha))
        .await
        .expect("coverage");
    put_at(fix, endpoint, snap(BELOW_VERSION, COVERAGE, COV_AS_OF, &[("7", 94), ("8", 6)], None, &hands.below_sha))
        .await
        .expect("below");
    assert_eq!(snapshot_count(admin).await, 6);

    let (left, right) = tokio::join!(
        activate_at(fix, endpoint, RETRO_VERSION, &retro, None),
        activate_at(fix, endpoint, COV_VERSION, &cov, None),
    );
    let mut winner = String::new();
    let mut conflicts = 0;
    let mut grants = 0;
    for result in [left, right] {
        match result {
            Ok(receipt) => {
                assert!(winner.is_empty(), "two winners");
                grants = receipt.grants;
                winner = receipt.snapshot_id;
            }
            Err(StoreError::CasConflict) => conflicts += 1,
            Err(err) => panic!("race failed: {err}"),
        }
    }
    assert_eq!(conflicts, 1);
    assert_eq!(grants, 1);
    assert!(winner == retro.snapshot_id || winner == cov.snapshot_id);
    assert_eq!(pointer_id(admin).await.as_deref(), Some(winner.as_str()));
    if winner != cov.snapshot_id {
        let moved = activate_at(fix, endpoint, COV_VERSION, &cov, Some(&winner)).await.expect("show coverage");
        assert_eq!(moved.grants, 1);
    }
    let coverage_view = read_reader(fix, endpoint).await.expect("coverage view");
    assert!(coverage_view.database_used);
    flags_false(
        coverage_view.finance_certified,
        coverage_view.semantic_certified,
        coverage_view.owner_authenticated,
        coverage_view.metric_certified,
        coverage_view.clock_used,
    );
    assert_eq!(coverage_view.rows.len(), 2);
    let known = by_code(&coverage_view.rows, "7");
    assert_eq!(known.status, "approved");
    assert_eq!(known.class.as_deref(), Some("open"));
    assert_eq!(known.description, None);
    assert_eq!(known.enum_id, ENUM_ID);
    assert_eq!(known.project_id, PROJECT);
    assert_eq!(known.as_of, COV_AS_OF);
    let unknown = by_code(&coverage_view.rows, "8");
    assert_eq!(unknown.status, "unexplained");
    assert_eq!(unknown.class, None);
    assert_eq!(unknown.description, None);
    assert_eq!(unknown.valid_from, None);
    assert_eq!(unknown.valid_to, None);

    let final_act = activate_at(fix, endpoint, HIST_VERSION, historical, Some(&cov.snapshot_id))
        .await
        .expect("final historical");
    assert_eq!(final_act.disposition, "activated");
    assert_eq!(final_act.grants, 1);
    assert_eq!(final_act.snapshot_id, historical.snapshot_id);
    cov
}

fn assert_historical_row(row: &EnumColumn) {
    assert_eq!(row.enum_id, ENUM_ID);
    assert_eq!(row.code, "7");
    assert_eq!(row.description.as_deref(), Some("prior open receipt"));
    assert_eq!(row.class.as_deref(), Some("open"));
    assert_eq!(row.valid_from.as_deref(), Some("2018-03-01"));
    assert_eq!(row.valid_to.as_deref(), Some("2021-06-01"));
    assert_eq!(row.version, HIST_VERSION);
    assert_eq!(row.as_of, HIST_AS_OF);
    assert_eq!(row.project_id, PROJECT);
    assert_eq!(row.status, "deprecated");
}

async fn view_and_privileges(fix: &Fix, endpoint: &Endpoint, admin: &Client, fixture: &Client, historical: &PutReceipt, cov: &PutReceipt) {
    let view = read_reader(fix, endpoint).await.expect("historical view");
    assert_eq!(view.contract, CONTRACT);
    assert_eq!(view.rows.len(), 1);
    assert_historical_row(&view.rows[0]);
    assert!(view.database_used);
    flags_false(
        view.finance_certified,
        view.semantic_certified,
        view.owner_authenticated,
        view.metric_certified,
        view.clock_used,
    );
    let today: String = admin.query_one("SELECT current_date::text", &[]).await.expect("today").get(0);
    assert_ne!(view.rows[0].as_of, today);
    assert_eq!(view.rows[0].as_of, HIST_AS_OF);

    let reader = connect_as(fix, "oo_r_enum", &fix.reader, &fix.ca).await;
    must_42501(&reader, "UPDATE oo_dict.enums SET status = 'approved'").await;
    must_42501(&reader, "DELETE FROM oo_dict.enums").await;
    must_42501(
        &reader,
        "INSERT INTO oo_dict.enums (enum_id, code, description, class, valid_from, valid_to, version, as_of, project_id, status) \
         VALUES ('a306.receipt.state', '7', 'prior open receipt', 'open', '2018-03-01', '2021-06-01', 'hist-2019', '2019-07-04', 'a306-receipts', 'approved')",
    )
    .await;
    must_42501(&reader, "SELECT enum_yaml FROM oo_enum_control.snapshot").await;
    must_42501(&reader, "SELECT report_json FROM oo_enum_control.snapshot").await;
    must_42501(&reader, "SELECT code FROM oo_enum_control.notice").await;
    must_42501(&reader, "SELECT status FROM oo_enum_control.published").await;
    must_42501(&reader, "SELECT status FROM oo_enum_control.served").await;
    must_42501(&reader, "SELECT id FROM biz.receipts").await;

    admin.batch_execute("SET ROLE oo_enum_view").await.expect("set role");
    let served_count: i64 = admin.query_one("SELECT count(*) FROM oo_enum_control.served", &[]).await.expect("served").get(0);
    assert_eq!(served_count, 1);
    must_42501(admin, "SELECT enum_yaml FROM oo_enum_control.snapshot").await;
    must_42501(admin, "SELECT report_json FROM oo_enum_control.snapshot").await;
    must_42501(admin, "SELECT code FROM oo_enum_control.notice").await;
    must_42501(admin, "SELECT status FROM oo_enum_control.published").await;
    must_42501(admin, "SELECT id FROM biz.receipts").await;
    admin.batch_execute("RESET ROLE").await.expect("reset role");

    let other = connect_as(fix, "oo_r_other", &fix.other, &fix.ca).await;
    must_42501(&other, "SELECT code FROM oo_dict.enums").await;
    let admin_read = read_view(&ReadRequest {
        endpoint,
        user: "oo_enum_admin",
        password: &fix.admin,
        ca_pem: &fix.ca,
    })
    .await
    .expect_err("admin is not a runtime reader");
    assert_eq!(admin_read, StoreError::LoginRole);

    fixture.batch_execute("REVOKE oo_enum_view FROM oo_enum_admin").await.expect("revoke member again");
    let rolled = activate_at(fix, endpoint, COV_VERSION, cov, Some(&historical.snapshot_id))
        .await
        .expect_err("rollback keeps the winner");
    assert_eq!(rolled, StoreError::Query { sqlstate: Some("42501".to_owned()) });
    assert_eq!(pointer_id(admin).await.as_deref(), Some(historical.snapshot_id.as_str()));
    let served_status: String = admin.query_one("SELECT status FROM oo_enum_control.served", &[]).await.expect("served status").get(0);
    assert_eq!(served_status, "deprecated");
    fixture.batch_execute("GRANT oo_enum_view TO oo_enum_admin").await.expect("restore member again");
}

async fn stored_facts(admin: &Client, hands: &Hands) {
    let code = text_at(
        admin,
        "SELECT n.code FROM oo_enum_control.notice n \
         JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
         WHERE s.enum_version = $1",
        COV_VERSION,
    )
    .await;
    let reason = text_at(
        admin,
        "SELECT n.reason FROM oo_enum_control.notice n \
         JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
         WHERE s.enum_version = $1",
        COV_VERSION,
    )
    .await;
    let count_text = text_at(
        admin,
        "SELECT n.count_text FROM oo_enum_control.notice n \
         JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
         WHERE s.enum_version = $1",
        COV_VERSION,
    )
    .await;
    assert_eq!(code, "8");
    assert_eq!(reason, "unknown_code");
    assert_eq!(count_text, "5");
    assert_eq!(
        text_at(admin, "SELECT coverage_ratio FROM oo_enum_control.snapshot WHERE enum_version = $1", COV_VERSION).await,
        "95/100"
    );
    assert_eq!(
        text_at(admin, "SELECT comparison FROM oo_enum_control.snapshot WHERE enum_version = $1", COV_VERSION).await,
        "equal"
    );
    assert_eq!(
        text_at(admin, "SELECT threshold_decimal FROM oo_enum_control.snapshot WHERE enum_version = $1", COV_VERSION).await,
        "0.95"
    );
    assert_eq!(
        text_at(admin, "SELECT threshold_met::text FROM oo_enum_control.snapshot WHERE enum_version = $1", COV_VERSION).await,
        "true"
    );
    assert_eq!(
        text_at(admin, "SELECT review_needed::text FROM oo_enum_control.snapshot WHERE enum_version = $1", COV_VERSION).await,
        "true"
    );
    assert_eq!(
        text_at(admin, "SELECT coverage_ratio FROM oo_enum_control.snapshot WHERE enum_version = $1", BELOW_VERSION).await,
        "94/100"
    );
    assert_eq!(
        text_at(admin, "SELECT comparison FROM oo_enum_control.snapshot WHERE enum_version = $1", BELOW_VERSION).await,
        "below"
    );
    assert_eq!(
        text_at(admin, "SELECT threshold_met::text FROM oo_enum_control.snapshot WHERE enum_version = $1", BELOW_VERSION).await,
        "false"
    );
    assert_eq!(
        text_at(admin, "SELECT n.count_text FROM oo_enum_control.notice n \
         JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
         WHERE s.enum_version = $1", BELOW_VERSION).await,
        "6"
    );
    let retro_status = status_list(admin, RETRO_VERSION).await;
    assert_eq!(retro_status, vec!["superseded".to_owned(), "approved".to_owned()]);
    let later_status = status_list(admin, LATER_VERSION).await;
    assert_eq!(later_status, vec!["superseded".to_owned(), "approved".to_owned()]);
    let later_class = class_list(admin, LATER_VERSION).await;
    assert_eq!(later_class, vec!["open".to_owned(), "closed".to_owned()]);
    assert_eq!(
        text_at(admin, "SELECT report_sha256 FROM oo_enum_control.snapshot WHERE enum_version = $1", HIST_VERSION).await,
        hands.historical_sha
    );
    let report = text_at(admin, "SELECT report_json FROM oo_enum_control.snapshot WHERE enum_version = $1", HIST_VERSION).await;
    assert!(report.contains("\"database_used\":false"));
    assert!(report.contains("\"owner_authenticated\":false"));
    assert!(report.contains("\"metric_certified\":false"));
    assert!(report.contains("\"historical_business_class\":true"));
    assert!(report.contains("\"status\":\"deprecated\""));
    assert!(!report.contains("\"finance_certified\":true"));
    assert!(!report.contains("\"semantic_certified\":true"));
}

async fn status_list(client: &Client, version: &str) -> Vec<String> {
    client
        .query(
            "SELECT p.status FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = $1 AND p.row_kind = 'definition' ORDER BY p.line_no",
            &[&version],
        )
        .await
        .expect("status list")
        .iter()
        .map(|row| row.get(0))
        .collect()
}

async fn class_list(client: &Client, version: &str) -> Vec<String> {
    client
        .query(
            "SELECT p.class FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = $1 AND p.row_kind = 'definition' ORDER BY p.line_no",
            &[&version],
        )
        .await
        .expect("class list")
        .iter()
        .map(|row| row.get(0))
        .collect()
}

async fn view_shape(admin: &Client) {
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
    for token in [
        "now(",
        "current_date",
        "current_timestamp",
        "clock_timestamp",
        "date_trunc",
        "current_setting",
        "enum_yaml",
        "report_json",
        "row_kind",
    ] {
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
    let login: bool = admin
        .query_one("SELECT rolcanlogin FROM pg_catalog.pg_roles WHERE rolname = 'oo_enum_view'", &[])
        .await
        .expect("login")
        .get(0);
    assert!(!login);
    let options: Option<Vec<String>> = admin
        .query_one(
            "SELECT c.reloptions FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'oo_dict' AND c.relname = 'enums'",
            &[],
        )
        .await
        .expect("reloptions")
        .get(0);
    let options = options.expect("reloptions");
    assert!(options.iter().any(|item| item == "security_barrier=true"));
    assert!(options.iter().any(|item| item == "security_invoker=false"));
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
}

#[tokio::test]
async fn independent_store() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    assert_eq!(fix.database, "oo_a293");
    assert_eq!(fix.host, "oo-a306-pg");
    let hands = handwritten();
    let endpoint = endpoint_of(&fix);
    let fixture = reset_database(&fix).await;
    install_boundaries(&fix, &endpoint, &fixture).await;
    let admin = connect_as(&fix, "oo_enum_admin", &fix.admin, &fix.ca).await;
    refuse_before_insert(&fix, &endpoint, &admin, &hands).await;
    let historical = put_historical(&fix, &endpoint, &admin, &hands).await;
    cas_and_rollback(&fix, &endpoint, &admin, &fixture, &historical).await;
    let cov = race_and_serve(&fix, &endpoint, &admin, &hands, &historical).await;
    view_and_privileges(&fix, &endpoint, &admin, &fixture, &historical, &cov).await;
    stored_facts(&admin, &hands).await;
    view_shape(&admin).await;
    assert_eq!(pointer_id(&admin).await.as_deref(), Some(historical.snapshot_id.as_str()));
}

#[tokio::test]
async fn restart_keeps_the_explicit_row() {
    let _gate = GATE.lock().await;
    let fix = load_fix();
    let endpoint = endpoint_of(&fix);
    let admin = connect_as(&fix, "oo_enum_admin", &fix.admin, &fix.ca).await;
    let view = read_reader(&fix, &endpoint).await.expect("restart read");
    assert_eq!(view.rows.len(), 1);
    assert_historical_row(&view.rows[0]);
    let today: String = admin.query_one("SELECT current_date::text", &[]).await.expect("today").get(0);
    assert_ne!(view.rows[0].as_of, today);
    assert_eq!(snapshot_count(&admin).await, 6);
    assert_eq!(
        text_at(
            &admin,
            "SELECT p.status FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = $1 AND p.row_kind = 'definition'",
            HIST_VERSION,
        )
        .await,
        "deprecated"
    );
    assert_eq!(
        text_at(
            &admin,
            "SELECT p.status FROM oo_enum_control.published p \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = p.snapshot_id \
             WHERE s.enum_version = $1 AND p.row_kind = 'definition'",
            DRIFT_VERSION,
        )
        .await,
        "approved"
    );
    assert_eq!(
        text_at(
            &admin,
            "SELECT n.reason FROM oo_enum_control.notice n \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
             WHERE s.enum_version = $1",
            COV_VERSION,
        )
        .await,
        "unknown_code"
    );
    assert_eq!(
        text_at(
            &admin,
            "SELECT n.code FROM oo_enum_control.notice n \
             JOIN oo_enum_control.snapshot s ON s.snapshot_id = n.snapshot_id \
             WHERE s.enum_version = $1",
            COV_VERSION,
        )
        .await,
        "8"
    );
    assert_eq!(
        text_at(&admin, "SELECT coverage_ratio FROM oo_enum_control.snapshot WHERE enum_version = $1", COV_VERSION).await,
        "95/100"
    );
    assert_eq!(
        text_at(&admin, "SELECT coverage_ratio FROM oo_enum_control.snapshot WHERE enum_version = $1", BELOW_VERSION).await,
        "94/100"
    );
    let historical = compile_report(HISTORICAL, HIST_AS_OF, &[("7", 4)], None);
    let sha = digest_of(&historical);
    assert_eq!(
        text_at(&admin, "SELECT report_sha256 FROM oo_enum_control.snapshot WHERE enum_version = $1", HIST_VERSION).await,
        sha
    );
    let replay = put_at(&fix, &endpoint, snap(HIST_VERSION, HISTORICAL, HIST_AS_OF, &[("7", 4)], None, &sha))
        .await
        .expect("replay after restart");
    assert_eq!(replay.disposition, PutDisposition::Replay);
    assert_eq!(
        install(&install_request(&endpoint, &fix, PROJECT, &["oo_r_enum"])).await.expect("install replay").disposition,
        InstallDisposition::Replay
    );
    let report = text_at(&admin, "SELECT report_json FROM oo_enum_control.snapshot WHERE enum_version = $1", HIST_VERSION).await;
    assert!(report.contains("\"historical_business_class\":true"));
    assert!(report.contains("\"status\":\"deprecated\""));
    assert_eq!(status_list(&admin, RETRO_VERSION).await, vec!["superseded".to_owned(), "approved".to_owned()]);
    view_shape(&admin).await;
}
