//! Independent review of metric-reconcile. Safe Rust. No Python.
//!
//! The hand literal, the digit difference, and the refusal checks live here.
//! They do not call the author's live test.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use metric_reconcile::{Exit, MetricReport, compile_bytes, dispatch};
use oo_gateway::{PolicyError, TlsEndpoint, connect_require_tls};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

const HAND: &str = "1000000000000000000.01";
const HAND_OFF: &str = "1000000000000000000.00";
const HAND_SCALE: &str = "1000000000000000000.010";
const DATE_START: &str = "2026-03-01";
const DATE_END: &str = "2026-04-01";
const KIND: &str = "P";
const CONTROL_SQL: &str = "SELECT closed_sum FROM ops.hand_close WHERE anchor_day = :period_end";
const METRIC_SQL: &str =
    "SELECT gross FROM rev.ticket WHERE booked_on >= :period_start AND booked_on < :period_end AND kind = :kind";

struct Bag {
    lines: Vec<String>,
    failed: bool,
}

impl Bag {
    fn record(&mut self, pass: bool, name: &str, detail: impl std::fmt::Display) {
        let tag = if pass { "PASS" } else { "FAIL" };
        self.lines.push(format!("{tag} {name} {detail}"));
        if !pass {
            self.failed = true;
        }
    }

    fn write(&self, dir: &std::path::Path) {
        let mut text = self.lines.join("\n");
        text.push('\n');
        let _ = fs::write(dir.join("checks.txt"), text);
    }
}

struct Conn {
    client: Client,
    driver: tokio::task::JoinHandle<()>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

fn main() -> ExitCode {
    let evidence = env_required("OO_A329_EVIDENCE");
    let dir = PathBuf::from(&evidence);
    let _ = fs::create_dir_all(&dir);
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            let _ = fs::write(dir.join("checks.txt"), "FAIL runtime\n");
            return ExitCode::from(1);
        }
    };
    let failed = runtime.block_on(run(dir.clone()));
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

async fn run(dir: PathBuf) -> bool {
    let mut bag = Bag {
        lines: Vec::new(),
        failed: false,
    };
    let host = env_required("OO_A329_HOST");
    let port: u16 = env_required("OO_A329_PORT").parse().unwrap_or(0);
    let database = env_required("OO_A329_DATABASE");
    let admin_user = env_required("OO_A329_ADMIN_USER");
    let login_user = env_required("OO_A329_LOGIN_USER");
    bag.record(host == "oo-a329-pg", "host", &host);
    bag.record(database == "oo_a329", "database", database.clone());
    bag.record(admin_user == "oo_fixture_admin", "admin_user", admin_user.clone());
    bag.record(login_user == "oo_a329_login", "login_user", login_user.clone());
    bag.record(port == 5432, "port", port);
    if bag.failed {
        bag.write(&dir);
        return true;
    }
    let admin_password = match read_secret(&env_required("OO_A329_ADMIN_PASSWORD_FILE")) {
        Ok(text) => text,
        Err(reason) => {
            bag.record(false, "admin_secret", reason);
            bag.write(&dir);
            return true;
        }
    };
    let login_password = match read_secret(&env_required("OO_A329_LOGIN_PASSWORD_FILE")) {
        Ok(text) => text,
        Err(reason) => {
            bag.record(false, "login_secret", reason);
            bag.write(&dir);
            return true;
        }
    };
    let ca = match fs::read(env_required("OO_A329_CA_FILE")) {
        Ok(bytes) => bytes,
        Err(_) => {
            bag.record(false, "ca", "read");
            bag.write(&dir);
            return true;
        }
    };
    let admin = match connect(&host, port, &database, &admin_user, &admin_password, &ca).await {
        Ok(conn) => conn,
        Err(reason) => {
            bag.record(false, "admin_connect", reason);
            bag.write(&dir);
            return true;
        }
    };
    if let Err(reason) = setup(&admin.client, &login_password).await {
        bag.record(false, "setup", reason);
        bag.write(&dir);
        return true;
    }
    let operators = match operator_rows(&admin.client).await {
        Ok(rows) => rows,
        Err(reason) => {
            bag.record(false, "operators", reason);
            bag.write(&dir);
            return true;
        }
    };
    bag.record(operators.len() == 3, "operator_count", operators.len());
    let happy_yaml = metric_yaml("posted", CONTROL_SQL);
    bag.record(
        !happy_yaml.lines().any(|line| line.trim_start().starts_with("certified:")),
        "yaml_omits_certified",
        "no certified key",
    );
    let enum_doc = enum_doc();
    let happy = request_bytes(&happy_yaml, &enum_doc, &operators, true, Some(DATE_END));
    let _ = fs::write(dir.join("request-pass.json"), &happy);

    let compiled = compile_bytes(&happy);
    store(&dir, "compile.json", &compiled.stdout);
    store(&dir, "compile.stderr", &compiled.stderr);
    check_secret(&mut bag, "compile_secret", &compiled, &login_password, &admin_password);
    match parse_report(&compiled.stdout) {
        Some(report) => {
            bag.record(compiled.code == 0, "compile_exit", compiled.code);
            bag.record(report.outcome == "compiled", "compile_outcome", &report.outcome);
            bag.record(!report.metric_certified, "compile_not_certified", report.metric_certified);
            bag.record(!report.database_used, "compile_no_db", report.database_used);
            record_limits(&mut bag, "compile", &report);
        }
        None => bag.record(false, "compile_report", "unparsed"),
    }

    refuse_case(
        &mut bag,
        &dir,
        "unknown_class",
        &request_bytes(&metric_yaml("absent", CONTROL_SQL), &enum_doc, &operators, true, Some(DATE_END)),
        &login_password,
        &admin_password,
    )
    .await;
    refuse_case(
        &mut bag,
        &dir,
        "same_path",
        &request_bytes(&metric_yaml("posted", METRIC_SQL), &enum_doc, &operators, true, Some(DATE_END)),
        &login_password,
        &admin_password,
    )
    .await;
    refuse_case(
        &mut bag,
        &dir,
        "anchor_open",
        &request_bytes(&happy_yaml, &enum_doc, &operators, false, Some(DATE_END)),
        &login_password,
        &admin_password,
    )
    .await;
    refuse_case(
        &mut bag,
        &dir,
        "gap_anchor",
        &request_bytes(&happy_yaml, &enum_doc, &operators, true, None),
        &login_password,
        &admin_password,
    )
    .await;

    let config = dir.join("config.json");
    if fs::write(
        &config,
        serde_json::to_vec(&json!({
            "host": host,
            "port": port,
            "database": database,
            "user": login_user,
            "password_file": env_required("OO_A329_LOGIN_PASSWORD_FILE"),
            "ca_file": env_required("OO_A329_CA_FILE"),
            "statement_timeout_ms": 5000,
            "max_result_bytes": 8192
        }))
        .expect("config"),
    )
    .is_err()
    {
        bag.record(false, "config", "write");
        bag.write(&dir);
        return true;
    }
    let args = vec!["evaluate".to_owned(), "--config".to_owned(), config.display().to_string()];

    let metric_before = match text_cell(&admin.client, "SELECT gross::text FROM rev.ticket").await {
        Ok(text) => text,
        Err(reason) => {
            bag.record(false, "metric_text", reason);
            bag.write(&dir);
            return true;
        }
    };
    let control_before = match text_cell(&admin.client, "SELECT closed_sum::text FROM ops.hand_close").await {
        Ok(text) => text,
        Err(reason) => {
            bag.record(false, "control_text", reason);
            bag.write(&dir);
            return true;
        }
    };
    bag.record(metric_before == HAND, "inserted_metric", &metric_before);
    bag.record(control_before == HAND, "inserted_control", &control_before);
    bag.record(last_digit_ok(&metric_before), "huge_last_digit", &metric_before);

    let pass = evaluate(&args, &happy, &login_password, &admin_password, &dir, "pass.json", &mut bag).await;
    if let Some(report) = pass {
        check_live(&mut bag, "pass", &report, &metric_before, &control_before, true, "0.00");
        let owner = report.lifecycle.owner.clone().unwrap_or_default();
        bag.record(
            owner == "ledger" && !report.owner_authenticated,
            "owner_text_not_authentication",
            &owner,
        );
        bag.record(
            report.enum_resolution.certification_allowed
                && report.enum_resolution.classes.iter().any(|class| {
                    class.class_name == "posted" && class.codes == ["P"]
                }),
            "reviewed_class_posted",
            report.enum_resolution.classes.len(),
        );
    }

    if let Err(reason) = set_control(&admin.client, HAND_OFF).await {
        bag.record(false, "set_off", reason);
    }
    let off_text = text_cell(&admin.client, "SELECT closed_sum::text FROM ops.hand_close")
        .await
        .unwrap_or_default();
    let off = evaluate(&args, &happy, &login_password, &admin_password, &dir, "tolerance.json", &mut bag).await;
    if let Some(report) = off {
        check_live(&mut bag, "tolerance", &report, &metric_before, &off_text, false, "0.01");
        bag.record(report.gates.first().is_some_and(|gate| gate.status == "passed"), "tolerance_g5a", "see report");
        bag.record(
            report.gates.get(1).is_some_and(|gate| gate.status == "failed" && gate.reason == "tolerance"),
            "tolerance_g5b",
            report.gates.get(1).map(|gate| gate.reason.clone()).unwrap_or_default(),
        );
        bag.record(report.reason == "tolerance", "tolerance_reason", report.reason);
    }

    if let Err(reason) = set_control(&admin.client, HAND_SCALE).await {
        bag.record(false, "set_scale", reason);
    }
    let scale_text = text_cell(&admin.client, "SELECT closed_sum::text FROM ops.hand_close")
        .await
        .unwrap_or_default();
    bag.record(scale_text == HAND_SCALE, "stored_scale", &scale_text);
    let scaled = evaluate(&args, &happy, &login_password, &admin_password, &dir, "scale.json", &mut bag).await;
    if let Some(report) = scaled {
        check_live(&mut bag, "scale", &report, &metric_before, &scale_text, true, "0.000");
        let same = same_number_text(report.values.metric.as_deref().unwrap_or(""), report.values.control.as_deref().unwrap_or(""));
        bag.record(same, "scale_same_number", report.values.control.unwrap_or_default());
    }

    let metric_after = text_cell(&admin.client, "SELECT gross::text FROM rev.ticket")
        .await
        .unwrap_or_default();
    bag.record(metric_after == HAND, "metric_unchanged", &metric_after);
    let counts = text_cell(
        &admin.client,
        "SELECT (SELECT count(*) FROM rev.ticket)::text || ',' || (SELECT count(*) FROM ops.hand_close)::text",
    )
    .await
    .unwrap_or_default();
    bag.record(counts == "1,1", "row_counts", &counts);

    match connect(&host, port, &database, &login_user, &login_password, &ca).await {
        Ok(login) => {
            let params: [&(dyn ToSql + Sync); 0] = [];
            match login.client.query_one("SELECT gross::text FROM rev.ticket", &params).await {
                Err(error) if sqlstate(&error) == "42501" => bag.record(true, "direct_select_refused", "42501"),
                Ok(_) => bag.record(false, "direct_select_refused", "succeeded"),
                Err(error) => bag.record(false, "direct_select_refused", sqlstate(&error)),
            }
        }
        Err(reason) => bag.record(false, "login_connect", reason),
    }
    let inherit = text_cell(
        &admin.client,
        "SELECT rolname || ':' || rolinherit::text FROM pg_roles WHERE rolname = 'oo_a329_login'",
    )
    .await
    .unwrap_or_default();
    bag.record(inherit == "oo_a329_login:false", "noinherit", &inherit);

    bag.write(&dir);
    bag.failed
}

fn check_live(bag: &mut Bag, name: &str, report: &MetricReport, metric_text: &str, control_text: &str, certify: bool, difference: &str) {
    bag.record(report.outcome == "evaluated", &format!("{name}_outcome"), &report.outcome);
    bag.record(report.database_used, &format!("{name}_database"), report.database_used);
    bag.record(report.metric_certified == certify, &format!("{name}_metric_certified"), report.metric_certified);
    record_limits(bag, name, report);
    bag.record(
        report.values.metric.as_deref() == Some(metric_text) && metric_text == HAND,
        &format!("{name}_metric_value"),
        report.values.metric.clone().unwrap_or_default(),
    );
    bag.record(
        report.values.control.as_deref() == Some(control_text),
        &format!("{name}_control_value"),
        report.values.control.clone().unwrap_or_default(),
    );
    let computed = plain_diff(metric_text, control_text).unwrap_or_default();
    bag.record(computed == difference, &format!("{name}_independent_diff"), &computed);
    bag.record(
        report.comparison.difference.as_deref() == Some(difference),
        &format!("{name}_reported_diff"),
        report.comparison.difference.clone().unwrap_or_default(),
    );
    bag.record(
        report.gates.first().is_some_and(|gate| gate.gate_id == "G5a" && gate.status == "passed"),
        &format!("{name}_g5a"),
        report.gates.first().map(|gate| gate.status.clone()).unwrap_or_default(),
    );
    let expect_g5b = if certify { "passed" } else { "failed" };
    bag.record(
        report.gates.get(1).is_some_and(|gate| gate.status == expect_g5b),
        &format!("{name}_g5b_status"),
        report.gates.get(1).map(|gate| gate.status.clone()).unwrap_or_default(),
    );
    bag.record(report.explain.status == "passed" && (1..=32).contains(&report.explain.rows), &format!("{name}_explain"), report.explain.rows);
    bag.record(report.explain.text_sha256.len() == 64, &format!("{name}_explain_hash"), report.explain.text_sha256.len());
    bag.record(
        report.tests.first().is_some_and(|test| test.status == "passed" && test.actual_rows == Some(1)),
        &format!("{name}_test_rows"),
        report.tests.first().map(|test| test.status.clone()).unwrap_or_default(),
    );
    bag.record(report.provenance.role == "oo_r_review", &format!("{name}_role"), &report.provenance.role);
    bag.record(
        report.provenance.anchor_name == "last_closed_period_end" && report.provenance.anchor_date.as_deref() == Some(DATE_END),
        &format!("{name}_closed_anchor"),
        report.provenance.anchor_date.clone().unwrap_or_default(),
    );
    bag.record(typed_sql(&report.metric_query.executed_sql, 3), &format!("{name}_metric_binds"), &report.metric_query.executed_sql);
    bag.record(typed_sql(&report.control_query.executed_sql, 1), &format!("{name}_control_binds"), &report.control_query.executed_sql);
    bag.record(
        report.metric_query.guard_fingerprint != report.control_query.guard_fingerprint,
        &format!("{name}_distinct_fingerprint"),
        &report.control_query.guard_fingerprint,
    );
    let receipt_input = serde_json::to_vec(report).unwrap_or_default();
    bag.record(receipt_matches(&receipt_input), &format!("{name}_receipt"), &report.receipt_sha256);
}

fn record_limits(bag: &mut Bag, name: &str, report: &MetricReport) {
    bag.record(
        !report.owner_authenticated && !report.publication && !report.full_gates && !report.source_modified && !report.provenance.password_logged,
        &format!("{name}_limits"),
        format!(
            "owner={} publication={} full_gates={} source_modified={}",
            report.owner_authenticated, report.publication, report.full_gates, report.source_modified
        ),
    );
    let ids: Vec<&str> = report.omitted_gates.iter().map(|gate| gate.gate_id.as_str()).collect();
    bag.record(
        ids == ["G1", "G2", "G3", "G4", "G6", "G7", "G8", "G9", "G10", "G11", "G12", "G13"]
            && report.omitted_gates.iter().all(|gate| gate.status == "not_run")
            && report.gates.len() == 2,
        &format!("{name}_omitted"),
        ids.len(),
    );
}

async fn refuse_case(bag: &mut Bag, dir: &std::path::Path, reason: &str, stdin: &[u8], login: &str, admin: &str) {
    let exit = dispatch(
        &["evaluate".to_owned(), "--config".to_owned(), "/tmp/a329-absent-config.json".to_owned()],
        stdin,
    )
    .await;
    store(dir, &format!("refuse-{reason}.json"), &exit.stdout);
    store(dir, &format!("refuse-{reason}.stderr"), &exit.stderr);
    check_secret(bag, &format!("refuse_{reason}_secret"), &exit, login, admin);
    bag.record(exit.code == 2, &format!("refuse_{reason}_exit"), exit.code);
    let stderr = String::from_utf8_lossy(&exit.stderr);
    bag.record(
        stderr == format!("metric-reconcile: refused {reason}\n"),
        &format!("refuse_{reason}_stderr"),
        stderr.trim(),
    );
    match parse_report(&exit.stdout) {
        Some(report) => {
            bag.record(report.outcome == "refused" && report.reason == reason, &format!("refuse_{reason}_reason"), &report.reason);
            bag.record(!report.metric_certified && !report.database_used, &format!("refuse_{reason}_not_certified"), report.database_used);
            bag.record(report.metric_query.executed_sql.is_empty() && report.control_query.executed_sql.is_empty(), &format!("refuse_{reason}_no_sql"), "empty");
            record_limits(bag, &format!("refuse_{reason}"), &report);
            bag.record(
                !report.metric_query.executed_sql.contains(DATE_END) && !report.control_query.bound_sql.contains(DATE_END),
                &format!("refuse_{reason}_no_date"),
                "empty or unbound",
            );
        }
        None => bag.record(false, &format!("refuse_{reason}_report"), "unparsed"),
    }
}

async fn evaluate(
    args: &[String],
    stdin: &[u8],
    login: &str,
    admin: &str,
    dir: &std::path::Path,
    name: &str,
    bag: &mut Bag,
) -> Option<MetricReport> {
    let exit = dispatch(args, stdin).await;
    store(dir, name, &exit.stdout);
    store(dir, &format!("{name}.stderr"), &exit.stderr);
    check_secret(bag, &format!("{name}_secret"), &exit, login, admin);
    bag.record(exit.code == 0, &format!("{name}_exit"), exit.code);
    bag.record(exit.stderr.is_empty(), &format!("{name}_stderr_empty"), exit.stderr.len());
    parse_report(&exit.stdout)
}

fn check_secret(bag: &mut Bag, name: &str, exit: &Exit, login: &str, admin: &str) {
    let leak = contains_secret(&exit.stdout, login)
        || contains_secret(&exit.stderr, login)
        || contains_secret(&exit.stdout, admin)
        || contains_secret(&exit.stderr, admin);
    bag.record(!leak, name, "no secret bytes");
}

fn contains_secret(bytes: &[u8], secret: &str) -> bool {
    !secret.is_empty() && bytes.windows(secret.len()).any(|window| window == secret.as_bytes())
}

fn parse_report(bytes: &[u8]) -> Option<MetricReport> {
    serde_json::from_slice(bytes).ok()
}

fn receipt_matches(bytes: &[u8]) -> bool {
    let Ok(report) = serde_json::from_slice::<MetricReport>(bytes) else {
        return false;
    };
    let mut cleared = report.clone();
    cleared.receipt_sha256.clear();
    let Ok(encoded) = serde_json::to_vec(&cleared) else {
        return false;
    };
    hex(&Sha256::digest(encoded)) == report.receipt_sha256
}

fn typed_sql(sql: &str, slots: usize) -> bool {
    if sql.contains(DATE_START) || sql.contains(DATE_END) || sql.contains(HAND) || sql.contains("oo_mr_probe_") || sql.contains("810316") {
        return false;
    }
    (1..=slots).all(|slot| sql.contains(&format!("${slot}"))) && !sql.contains(&format!("${}", slots + 1))
}

fn last_digit_ok(text: &str) -> bool {
    let Some((integer, frac)) = text.split_once('.') else {
        return false;
    };
    integer == "1000000000000000000" && frac == "01" && text.ends_with('1')
}

fn store(dir: &std::path::Path, name: &str, bytes: &[u8]) {
    let _ = fs::write(dir.join(name), bytes);
}

fn metric_yaml(class_list: &str, control_sql: &str) -> String {
    format!(
        r#"schema_version: oos/v1
metrics:
  - metric: closed_gross
    label: Closed gross
    owner: ledger
    status: active
    grain: [ticket]
    time_dimension: rev.ticket.booked_on
    default_anchor: month_start
    enum_filters:
      - column: rev.ticket.kind
        enum: rev.ticket.kind
        classes: [{class_list}]
    sql: "{METRIC_SQL}"
    tests:
      - params: {{ period_start: "{DATE_START}", period_end: "{DATE_END}", kind: {KIND} }}
        expect: {{ rows: 1 }}
    control_total:
      as_of: last_closed_period_end
      source: tms_pg
      sql: "{control_sql}"
      tolerance: {{ abs: "0" }}
"#
    )
}

fn enum_doc() -> String {
    r#"schema_version: oos/v1
enum: rev.ticket.kind
classes:
  posted: posted ticket
values:
  - code: P
    class: posted
    status: approved
    source: dictionary_table
    valid_from: "2020-01-01"
"#
    .to_owned()
}

fn request_bytes(metric: &str, enum_doc: &str, operators: &[(String, u32, u32)], closed: bool, date: Option<&str>) -> Vec<u8> {
    let mut anchor = Map::new();
    anchor.insert("name".to_owned(), json!("last_closed_period_end"));
    anchor.insert("closed".to_owned(), json!(closed));
    if let Some(date) = date {
        anchor.insert("date".to_owned(), json!(date));
    }
    let ops: Vec<Value> = operators
        .iter()
        .map(|(name, oid, procedure)| {
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
        "as_of": DATE_END,
        "identity": {
            "project": "review",
            "role": "oo_r_review",
            "policy_id": "policy_review",
            "knowledge_id": "knowledge_review",
        },
        "anchor": anchor,
        "anchor_bound": ["period_end"],
        "source": "tms_pg",
        "parameters": {
            "period_start": {"type": "text", "value": DATE_START},
            "period_end": {"type": "text", "value": DATE_END},
            "kind": {"type": "text", "value": KIND},
        },
        "catalog": {
            "relations": [
                relation("rev", "ticket", &[("gross", 1700), ("booked_on", 25), ("kind", 25)]),
                relation("ops", "hand_close", &[("closed_sum", 1700), ("anchor_day", 25)]),
            ],
            "functions": [],
            "operators": ops,
            "literal_types": {"integer": 23, "text": 25, "boolean": 16},
            "null_test_allowed": true,
            "large_table_row_threshold": 100000,
        }
    });
    serde_json::to_vec(&doc).expect("request")
}

fn relation(schema: &str, name: &str, columns: &[(&str, u32)]) -> Value {
    json!({
        "schema": schema,
        "name": name,
        "physical_schema": schema,
        "physical_name": name,
        "estimated_rows": 4,
        "pending_unclassified": 0,
        "columns": columns.iter().map(|(column, oid)| json!({
            "name": column,
            "physical_name": column,
            "type_oid": oid,
        })).collect::<Vec<_>>(),
    })
}

async fn connect(host: &str, port: u16, database: &str, user: &str, password: &str, ca: &[u8]) -> Result<Conn, String> {
    let endpoint = TlsEndpoint {
        host: host.to_owned(),
        port,
        database: database.to_owned(),
        user: user.to_owned(),
    };
    let (client, connection) = connect_require_tls(&endpoint, password, ca)
        .await
        .map_err(|error| policy_token(&error).to_owned())?;
    let driver = tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(Conn { client, driver })
}

fn policy_token(error: &PolicyError) -> &'static str {
    match error {
        PolicyError::CertificateRejected => "certificate",
        _ => "connect",
    }
}

async fn setup(client: &Client, login_password: &str) -> Result<(), String> {
    exec(client, "DROP SCHEMA IF EXISTS rev CASCADE").await?;
    exec(client, "DROP SCHEMA IF EXISTS ops CASCADE").await?;
    exec(client, "DROP ROLE IF EXISTS oo_a329_login").await?;
    exec(client, "DROP ROLE IF EXISTS oo_r_review").await?;
    exec(client, "CREATE ROLE oo_r_review NOLOGIN").await?;
    exec(
        client,
        &format!("CREATE ROLE oo_a329_login LOGIN PASSWORD '{login_password}' NOINHERIT IN ROLE oo_r_review"),
    )
    .await?;
    exec(client, "CREATE SCHEMA rev").await?;
    exec(client, "CREATE SCHEMA ops").await?;
    exec(client, "CREATE TABLE rev.ticket (gross numeric, booked_on text, kind text)").await?;
    exec(client, "CREATE TABLE ops.hand_close (closed_sum numeric, anchor_day text)").await?;
    exec(client, "GRANT USAGE ON SCHEMA rev, ops TO oo_r_review").await?;
    exec(client, "GRANT SELECT ON rev.ticket, ops.hand_close TO oo_r_review").await?;
    let booked = DATE_START.to_owned();
    let kind = KIND.to_owned();
    let anchor = DATE_END.to_owned();
    let hand = HAND.to_owned();
    step(
        client,
        "INSERT INTO rev.ticket (gross, booked_on, kind) VALUES (CAST($1::text AS numeric), $2::text, $3::text)",
        &[&hand, &booked, &kind],
    )
    .await?;
    step(
        client,
        "INSERT INTO ops.hand_close (closed_sum, anchor_day) VALUES (CAST($1::text AS numeric), $2::text)",
        &[&hand, &anchor],
    )
    .await?;
    Ok(())
}

async fn set_control(client: &Client, literal: &str) -> Result<(), String> {
    let value = literal.to_owned();
    step(client, "UPDATE ops.hand_close SET closed_sum = CAST($1::text AS numeric)", &[&value]).await
}

async fn exec(client: &Client, sql: &str) -> Result<(), String> {
    let params: [&(dyn ToSql + Sync); 0] = [];
    client.execute(sql, &params).await.map(|_| ()).map_err(|error| sqlstate(&error))
}

async fn step(client: &Client, sql: &str, params: &[&(dyn ToSql + Sync)]) -> Result<(), String> {
    client.execute(sql, params).await.map(|_| ()).map_err(|error| sqlstate(&error))
}

async fn text_cell(client: &Client, sql: &str) -> Result<String, String> {
    let params: [&(dyn ToSql + Sync); 0] = [];
    let row = client.query_one(sql, &params).await.map_err(|error| sqlstate(&error))?;
    row.try_get(0).map_err(|error| sqlstate(&error))
}

async fn operator_rows(client: &Client) -> Result<Vec<(String, u32, u32)>, String> {
    let sql = "SELECT o.oprname::text, o.oid::int4, o.oprcode::oid::int4 FROM pg_operator o WHERE o.oprnamespace = 'pg_catalog'::regnamespace AND o.oprleft = 'text'::regtype AND o.oprright = 'text'::regtype AND o.oprname IN ('=', '<', '>=') ORDER BY o.oprname::text";
    let params: [&(dyn ToSql + Sync); 0] = [];
    let rows = client.query(sql, &params).await.map_err(|error| sqlstate(&error))?;
    let mut found = Vec::new();
    for row in rows {
        let name: String = row.try_get(0).map_err(|error| sqlstate(&error))?;
        let oid: i32 = row.try_get(1).map_err(|error| sqlstate(&error))?;
        let procedure: i32 = row.try_get(2).map_err(|error| sqlstate(&error))?;
        let oid = u32::try_from(oid).map_err(|_| "operator_oid".to_owned())?;
        let procedure = u32::try_from(procedure).map_err(|_| "operator_oid".to_owned())?;
        found.push((name, oid, procedure));
    }
    for name in ["<", "=", ">="] {
        if !found.iter().any(|(have, _, _)| have == name) {
            return Err(format!("operator_missing {name}"));
        }
    }
    Ok(found)
}

fn sqlstate(error: &tokio_postgres::Error) -> String {
    error.code().map(|code| code.code().to_owned()).unwrap_or_else(|| "no_sqlstate".to_owned())
}

fn env_required(name: &str) -> String {
    env::var(name).unwrap_or_default()
}

fn read_secret(path: &str) -> Result<String, String> {
    let mut text = fs::read_to_string(path).map_err(|_| "read".to_owned())?;
    if text.as_bytes().contains(&0) {
        return Err("nul".to_owned());
    }
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if !hex_password(&text) {
        return Err("shape".to_owned());
    }
    Ok(text)
}

fn hex_password(text: &str) -> bool {
    let bytes = text.as_bytes();
    (16..=128).contains(&bytes.len()) && bytes.iter().all(|byte| byte.is_ascii_hexdigit())
}

fn same_number_text(left: &str, right: &str) -> bool {
    match (parse_dec(left), parse_dec(right)) {
        (Some(left), Some(right)) => left.int_digits == right.int_digits && trim_frac(&left.frac) == trim_frac(&right.frac),
        _ => false,
    }
}

struct Dec {
    int_digits: Vec<u8>,
    frac: Vec<u8>,
}

fn parse_dec(text: &str) -> Option<Dec> {
    if text.is_empty() || text.starts_with('-') || text.starts_with('+') {
        return None;
    }
    let (int_part, frac_part) = match text.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (text, ""),
    };
    if int_part.is_empty()
        || !int_part.bytes().all(|byte| byte.is_ascii_digit())
        || !frac_part.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(Dec {
        int_digits: trim_int(int_part.as_bytes()).to_vec(),
        frac: frac_part.as_bytes().to_vec(),
    })
}

fn trim_int(digits: &[u8]) -> &[u8] {
    let mut start = 0;
    while start + 1 < digits.len() && digits[start] == b'0' {
        start += 1;
    }
    &digits[start..]
}

fn trim_frac(frac: &[u8]) -> &[u8] {
    let mut end = frac.len();
    while end > 0 && frac[end - 1] == b'0' {
        end -= 1;
    }
    &frac[..end]
}

fn plain_diff(metric: &str, control: &str) -> Option<String> {
    let metric = parse_dec(metric)?;
    let control = parse_dec(control)?;
    let scale = metric.frac.len().max(control.frac.len());
    let left = coeff(&metric, scale);
    let right = coeff(&control, scale);
    let digits = sub_abs(&left, &right).or_else(|| sub_abs(&right, &left))?;
    Some(format_diff(&digits, scale))
}

fn coeff(dec: &Dec, scale: usize) -> Vec<u8> {
    let mut digits = dec.int_digits.clone();
    digits.extend_from_slice(&dec.frac);
    if dec.frac.len() < scale {
        digits.extend(std::iter::repeat_n(b'0', scale - dec.frac.len()));
    }
    trim_int(&digits).to_vec()
}

fn sub_abs(left: &[u8], right: &[u8]) -> Option<Vec<u8>> {
    if cmp_digits(left, right) == std::cmp::Ordering::Less {
        return None;
    }
    let mut wide = vec![b'0'; left.len() - right.len()];
    wide.extend_from_slice(right);
    let mut out = left.to_vec();
    let mut borrow = 0_i16;
    for index in (0..out.len()).rev() {
        let mut digit = i16::from(out[index] - b'0') - i16::from(wide[index] - b'0') - borrow;
        if digit < 0 {
            digit += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out[index] = u8::try_from(digit).ok()? + b'0';
    }
    if borrow != 0 {
        return None;
    }
    Some(trim_int(&out).to_vec())
}

fn cmp_digits(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
    let left = trim_int(left);
    let right = trim_int(right);
    left.len().cmp(&right.len()).then(left.cmp(right))
}

fn format_diff(digits: &[u8], scale: usize) -> String {
    let text = String::from_utf8(digits.to_vec()).unwrap_or_default();
    if scale == 0 {
        return text;
    }
    if text.len() > scale {
        let split = text.len() - scale;
        format!("{}.{}", &text[..split], &text[split..])
    } else {
        let zeros = scale - text.len();
        format!("0.{}{}", "0".repeat(zeros), text)
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}
