//! Private TLS fixture. Runs only when `OO_A316_LIVE=1`.
//!
//! Source and control rows are inserted separately. The hand literal is not
//! copied from one table into the other.

use std::collections::BTreeMap;

use oo_gateway::{PolicyError, TlsEndpoint, connect_require_tls};
use serde_json::{Value, json};
use tokio_postgres::Client;

const HAND: &str = "1000000000000000000.01";
const HAND_OFF: &str = "1000000000000000000.00";
const HAND_SCALE: &str = "1000000000000000000.010";

#[tokio::test]
async fn hand_literal_independent_control() {
    if std::env::var("OO_A316_LIVE").ok().as_deref() != Some("1") {
        return;
    }
    let host = env("OO_A316_HOST");
    let port: u16 = env("OO_A316_PORT").parse().expect("port");
    let database = env("OO_A316_DATABASE");
    let admin_user = env("OO_A316_ADMIN_USER");
    let login_user = env("OO_A316_LOGIN_USER");
    assert_eq!(host, "oo-a316-pg");
    assert_eq!(database, "oo_a316");
    assert_eq!(admin_user, "oo_fixture_admin");
    assert_eq!(login_user, "oo_a316_login");
    let admin_password = crate::read_secret(&env("OO_A316_ADMIN_PASSWORD_FILE"), 4096, true).expect("admin password file");
    let login_password = crate::read_secret(&env("OO_A316_LOGIN_PASSWORD_FILE"), 4096, true).expect("login password file");
    assert!(password_literal(&login_password), "login password is not a hex literal");
    let ca = crate::read_bytes(std::path::Path::new(&env("OO_A316_CA_FILE")), 1_048_576).expect("ca");
    let admin = connect(&host, port, &database, &admin_user, &admin_password, &ca).await;
    setup(&admin.client, &login_password).await;
    assert_eq!(text_cell(&admin.client, "SELECT amount::text FROM src.receipt").await, HAND);
    assert_eq!(text_cell(&admin.client, "SELECT total::text FROM ctl.period_total").await, HAND);

    let operators = operator_rows(&admin.client).await;
    let stdin = request(&operators);
    let config = write_config(&host, port, &database, &login_user);
    let args = vec!["evaluate".to_owned(), "--config".to_owned(), config.display().to_string()];
    let first = evaluate(&args, &stdin, &login_password, "pass.json").await;
    assert_eq!(first.outcome, "evaluated");
    assert!(first.database_used);
    assert!(!first.source_modified);
    assert_eq!(first.gates[0].status, "passed", "{}", first.gates[0].reason);
    assert_eq!(first.gates[1].status, "passed", "{}", first.gates[1].reason);
    assert_eq!(first.values.metric.as_deref(), Some(HAND));
    assert_eq!(first.values.control.as_deref(), Some(text_cell(&admin.client, "SELECT total::text FROM ctl.period_total").await.as_str()));
    assert_eq!(first.tests[0].status, "passed");
    assert_eq!(first.tests[0].actual_rows, Some(1));
    assert_eq!(first.explain.status, "passed");
    assert!((1..32).contains(&first.explain.rows));
    assert!(first.metric_certified);
    assert!(!first.owner_authenticated);
    assert!(!first.publication);
    assert!(!first.full_gates);

    let off = HAND_OFF.to_owned();
    let off_param: [&(dyn postgres_types::ToSql + Sync); 1] = [&off];
    step(&admin.client, "UPDATE ctl.period_total SET total = CAST($1::text AS numeric)", &off_param).await;
    let failed = evaluate(&args, &stdin, &login_password, "tolerance.json").await;
    assert_eq!(failed.outcome, "evaluated");
    assert_eq!(failed.gates[0].status, "passed");
    assert_eq!(failed.gates[1].status, "failed");
    assert_eq!(failed.gates[1].reason, "tolerance");
    assert!(!failed.metric_certified);
    assert_eq!(failed.reason, "tolerance");
    assert_eq!(failed.values.metric.as_deref(), Some(HAND));

    let restored = HAND.to_owned();
    let restored_param: [&(dyn postgres_types::ToSql + Sync); 1] = [&restored];
    step(&admin.client, "UPDATE ctl.period_total SET total = CAST($1::text AS numeric)", &restored_param).await;
    assert_eq!(text_cell(&admin.client, "SELECT total::text FROM ctl.period_total").await, HAND);
    let scaled_literal = HAND_SCALE.to_owned();
    let scaled_param: [&(dyn postgres_types::ToSql + Sync); 1] = [&scaled_literal];
    step(&admin.client, "UPDATE ctl.period_total SET total = CAST($1::text AS numeric)", &scaled_param).await;
    let scaled = evaluate(&args, &stdin, &login_password, "scale.json").await;
    assert_eq!(scaled.gates[1].status, "passed", "{}", scaled.gates[1].reason);
    assert_eq!(scaled.values.metric.as_deref(), Some(HAND));
    assert_eq!(
        scaled.values.control.as_deref(),
        Some(text_cell(&admin.client, "SELECT total::text FROM ctl.period_total").await.as_str())
    );
    assert!(scaled.metric_certified);
}

struct Admin {
    client: Client,
    driver: tokio::task::JoinHandle<()>,
}

impl Drop for Admin {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

async fn connect(host: &str, port: u16, database: &str, user: &str, password: &str, ca: &[u8]) -> Admin {
    let endpoint = TlsEndpoint {
        host: host.to_owned(),
        port,
        database: database.to_owned(),
        user: user.to_owned(),
    };
    let (client, connection) = connect_require_tls(&endpoint, password, ca)
        .await
        .unwrap_or_else(|error| panic!("{}", policy_token(&error)));
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    Admin { client, driver }
}

fn policy_token(error: &PolicyError) -> &'static str {
    match error {
        PolicyError::CertificateRejected => "certificate",
        _ => "connect",
    }
}

async fn setup(client: &Client, login_password: &str) {
    exec(client, "DROP SCHEMA IF EXISTS src CASCADE").await;
    exec(client, "DROP SCHEMA IF EXISTS ctl CASCADE").await;
    exec(client, "DROP ROLE IF EXISTS oo_a316_login").await;
    exec(client, "DROP ROLE IF EXISTS oo_r_finance").await;
    exec(client, "CREATE ROLE oo_r_finance NOLOGIN").await;
    let create_login = format!("CREATE ROLE oo_a316_login LOGIN PASSWORD '{login_password}' IN ROLE oo_r_finance");
    exec(client, &create_login).await;
    exec(client, "CREATE SCHEMA src").await;
    exec(client, "CREATE SCHEMA ctl").await;
    exec(client, "CREATE TABLE src.receipt (amount numeric, received_on text, status text)").await;
    exec(client, "CREATE TABLE ctl.period_total (total numeric, period_end text)").await;
    exec(client, "GRANT USAGE ON SCHEMA src, ctl TO oo_r_finance").await;
    exec(client, "GRANT SELECT ON src.receipt, ctl.period_total TO oo_r_finance").await;
    let hand = HAND.to_owned();
    let received_on = "2026-03-15".to_owned();
    let status = "C".to_owned();
    let period_end = "2026-04-01".to_owned();
    let insert_receipt: [&(dyn postgres_types::ToSql + Sync); 3] = [&hand, &received_on, &status];
    step(
        client,
        "INSERT INTO src.receipt (amount, received_on, status) VALUES (CAST($1::text AS numeric), $2::text, $3::text)",
        &insert_receipt,
    )
    .await;
    let insert_control: [&(dyn postgres_types::ToSql + Sync); 2] = [&hand, &period_end];
    step(
        client,
        "INSERT INTO ctl.period_total (total, period_end) VALUES (CAST($1::text AS numeric), $2::text)",
        &insert_control,
    )
    .await;
}

async fn exec(client: &Client, sql: &str) {
    let params: [&(dyn postgres_types::ToSql + Sync); 0] = [];
    client.execute(sql, &params).await.unwrap_or_else(|error| panic!("{}", sqlstate(&error)));
}

async fn step(client: &Client, sql: &str, params: &[&(dyn postgres_types::ToSql + Sync)]) {
    client.execute(sql, params).await.unwrap_or_else(|error| panic!("{}", sqlstate(&error)));
}

async fn text_cell(client: &Client, sql: &str) -> String {
    let params: [&(dyn postgres_types::ToSql + Sync); 0] = [];
    let row = client.query_one(sql, &params).await.unwrap_or_else(|error| panic!("{}", sqlstate(&error)));
    row.try_get(0).unwrap_or_else(|error| panic!("{}", sqlstate(&error)))
}

async fn operator_rows(client: &Client) -> BTreeMap<String, (u32, u32)> {
    let sql = "SELECT oprname::text, oid::int4, oprcode::oid::int4 FROM pg_operator WHERE oprleft = 'text'::regtype AND oprright = 'text'::regtype AND oprname IN ('=', '<', '>=')";
    let params: [&(dyn postgres_types::ToSql + Sync); 0] = [];
    let rows = client.query(sql, &params).await.unwrap_or_else(|error| panic!("{}", sqlstate(&error)));
    let mut found = BTreeMap::new();
    for row in rows {
        let name: String = row.try_get(0).unwrap_or_else(|error| panic!("{}", sqlstate(&error)));
        let oid: i32 = row.try_get(1).unwrap_or_else(|error| panic!("{}", sqlstate(&error)));
        let procedure: i32 = row.try_get(2).unwrap_or_else(|error| panic!("{}", sqlstate(&error)));
        let oid = u32::try_from(oid).expect("operator_oid");
        let procedure = u32::try_from(procedure).expect("operator_oid");
        assert!(found.insert(name, (oid, procedure)).is_none(), "duplicate operator");
    }
    for name in ["=", "<", ">="] {
        assert!(found.contains_key(name), "operator_missing");
    }
    found
}

fn request(operators: &BTreeMap<String, (u32, u32)>) -> Vec<u8> {
    let metric = r#"schema_version: oos/v1
metrics:
  - metric: monthly_receipts
    label: Monthly receipts
    owner: finance
    status: active
    grain: [receipt]
    time_dimension: src.receipt.received_on
    default_anchor: month_start
    enum_filters:
      - column: src.receipt.status
        enum: src.receipt.status
        classes: [confirmed]
    sql: "SELECT amount FROM src.receipt WHERE received_on >= :period_start AND received_on < :period_end AND status = :status"
    tests:
      - params: { period_start: "2026-03-01", period_end: "2026-04-01", status: C }
        expect: { rows: 1 }
    control_total:
      as_of: last_closed_period_end
      source: tms_pg
      sql: "SELECT total FROM ctl.period_total WHERE period_end = :period_end"
      tolerance: { abs: "0" }
"#;
    let enum_doc = r#"schema_version: oos/v1
enum: src.receipt.status
classes:
  confirmed: confirmed receipt
values:
  - code: C
    class: confirmed
    status: approved
    source: dictionary_table
    valid_from: "2020-01-01"
"#;
    let ops: Vec<Value> = operators
        .iter()
        .map(|(name, (oid, procedure))| {
            json!({
                "oid": oid,
                "procedure_oid": procedure,
                "schema": "pg_catalog",
                "name": name,
                "left_type_oid": 25,
                "right_type_oid": 25,
            })
        })
        .collect();
    let doc = json!({
        "metric_yaml": metric,
        "enums": [enum_doc],
        "as_of": "2026-04-01",
        "identity": {
            "project": "demo",
            "role": "oo_r_finance",
            "policy_id": "policy_1",
            "knowledge_id": "knowledge_1",
        },
        "anchor": {"name": "last_closed_period_end", "closed": true, "date": "2026-04-01"},
        "anchor_bound": ["period_end"],
        "source": "tms_pg",
        "parameters": {
            "period_start": {"type": "text", "value": "2026-03-01"},
            "period_end": {"type": "text", "value": "2026-04-01"},
            "status": {"type": "text", "value": "C"},
        },
        "catalog": {
            "relations": [
                {
                    "schema": "src",
                    "name": "receipt",
                    "physical_schema": "src",
                    "physical_name": "receipt",
                    "estimated_rows": 10,
                    "pending_unclassified": 0,
                    "columns": [
                        {"name": "amount", "physical_name": "amount", "type_oid": 1700},
                        {"name": "received_on", "physical_name": "received_on", "type_oid": 25},
                        {"name": "status", "physical_name": "status", "type_oid": 25},
                    ],
                },
                {
                    "schema": "ctl",
                    "name": "period_total",
                    "physical_schema": "ctl",
                    "physical_name": "period_total",
                    "estimated_rows": 10,
                    "pending_unclassified": 0,
                    "columns": [
                        {"name": "total", "physical_name": "total", "type_oid": 1700},
                        {"name": "period_end", "physical_name": "period_end", "type_oid": 25},
                    ],
                },
            ],
            "functions": [],
            "operators": ops,
            "literal_types": {"integer": 23, "text": 25, "boolean": 16},
            "null_test_allowed": true,
            "large_table_row_threshold": 100000,
        },
    });
    serde_json::to_vec(&doc).expect("request")
}

fn write_config(host: &str, port: u16, database: &str, user: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join("oo-a316-metric-config.json");
    let body = json!({
        "host": host,
        "port": port,
        "database": database,
        "user": user,
        "password_file": env("OO_A316_LOGIN_PASSWORD_FILE"),
        "ca_file": env("OO_A316_CA_FILE"),
        "statement_timeout_ms": 5000,
        "max_result_bytes": 8192,
    });
    std::fs::write(&path, serde_json::to_vec(&body).expect("config")).expect("write config");
    path
}

async fn evaluate(args: &[String], stdin: &[u8], login_password: &str, label: &str) -> crate::report::Report {
    let exit = crate::dispatch(args, stdin).await;
    assert_eq!(exit.code, 0, "stderr {}", String::from_utf8_lossy(&exit.stderr));
    assert!(
        !exit.stdout.windows(login_password.len()).any(|window| window == login_password.as_bytes()),
        "password appeared in stdout"
    );
    assert!(exit.stderr.is_empty());
    if let Ok(dir) = std::env::var("OO_A316_EVIDENCE") {
        std::fs::write(std::path::Path::new(&dir).join(label), &exit.stdout).expect("store report");
    }
    let report: crate::report::Report = serde_json::from_slice(&exit.stdout).expect("report");
    assert!(!report.provenance.password_logged);
    report
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"))
}

fn password_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    (16..=128).contains(&bytes.len()) && bytes.iter().all(|byte| byte.is_ascii_hexdigit())
}

fn sqlstate(error: &tokio_postgres::Error) -> String {
    error.code().map(|code| code.code().to_owned()).unwrap_or_else(|| "no_sqlstate".to_owned())
}
