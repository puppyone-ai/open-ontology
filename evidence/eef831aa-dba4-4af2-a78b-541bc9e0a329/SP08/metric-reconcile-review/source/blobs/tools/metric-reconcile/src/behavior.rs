//! Compile-time checks. No socket.

use std::os::unix::fs::PermissionsExt;

use serde_json::{Value, json};

use crate::report::Report;
use crate::{Exit, compile_bytes, dispatch};

const HAPPY: &str = r#"schema_version: oos/v1
metrics:
  - metric: monthly_receipts
    label: Monthly receipts
    owner: finance
    synonyms: [receipts]
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

const CONFIRMED: &str = r#"schema_version: oos/v1
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

fn catalog() -> Value {
    json!({
        "relations": [
            relation("src", "receipt", &[
                ("amount", 1700),
                ("received_on", 25),
                ("status", 25),
                ("note", 25),
            ]),
            relation("ctl", "period_total", &[("total", 1700), ("period_end", 25)]),
        ],
        "functions": [],
        "operators": [
            operator(301, 401, "="),
            operator(302, 402, "<"),
            operator(303, 403, ">="),
        ],
        "literal_types": {"integer": 23, "text": 25, "boolean": 16},
        "null_test_allowed": true,
        "large_table_row_threshold": 100000
    })
}

fn relation(schema: &str, name: &str, columns: &[(&str, u32)]) -> Value {
    let columns = columns
        .iter()
        .map(|(column, oid)| {
            json!({
                "name": column,
                "physical_name": column,
                "type_oid": oid,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "schema": schema,
        "name": name,
        "physical_schema": schema,
        "physical_name": name,
        "estimated_rows": 10,
        "pending_unclassified": 0,
        "columns": columns,
    })
}

fn operator(oid: u32, procedure: u32, name: &str) -> Value {
    json!({
        "oid": oid,
        "procedure_oid": procedure,
        "schema": "pg_catalog",
        "name": name,
        "left_type_oid": 25,
        "right_type_oid": 25,
    })
}

fn doc_with(metric: &str, enums: &[&str], previous: Option<&str>) -> Value {
    json!({
        "metric_yaml": metric,
        "previous_metric_yaml": previous,
        "enums": enums,
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
        "catalog": catalog(),
    })
}

fn happy() -> Value {
    doc_with(HAPPY, &[CONFIRMED], None)
}

fn compile(doc: &Value) -> Exit {
    compile_bytes(&serde_json::to_vec(doc).expect("request"))
}

fn parsed(exit: &Exit) -> Report {
    serde_json::from_slice(&exit.stdout).unwrap_or_else(|_| panic!("stdout {}", String::from_utf8_lossy(&exit.stdout)))
}

fn assert_shell(report: &Report) {
    assert!(!report.owner_authenticated);
    assert!(!report.publication);
    assert!(!report.full_gates);
    assert!(!report.source_modified);
    assert!(!report.provenance.password_logged);
    assert!(report.lifecycle.replaced_by.is_none());
    assert!(!report.lifecycle.replaced_by_core_field);
    assert_eq!(report.omitted_gates.len(), 12);
    assert!(report.omitted_gates.iter().all(|gate| gate.status == "not_run"));
    assert!(report.omitted_gates.iter().all(|gate| gate.gate_id != "G5a" && gate.gate_id != "G5b"));
    let mut cleared = report.clone();
    cleared.receipt_sha256.clear();
    let bytes = serde_json::to_vec(&cleared).expect("cleared");
    assert_eq!(crate::report::sha256_hex(&bytes), report.receipt_sha256);
}

fn refused(doc: &Value) -> Report {
    let exit = compile(doc);
    assert_eq!(exit.code, 2, "stderr {}", String::from_utf8_lossy(&exit.stderr));
    let report = parsed(&exit);
    assert_eq!(report.outcome, "refused");
    assert!(!report.metric_certified);
    assert!(!report.database_used);
    assert_eq!(report.gates[1].status, "block");
    assert_eq!(report.gates[1].reason, "not_executed");
    assert!(exit.stderr.starts_with(b"metric-reconcile: refused "));
    assert_shell(&report);
    report
}

#[test]
fn compile_keeps_certification_closed() {
    let exit = compile(&happy());
    assert_eq!(exit.code, 0, "stdout {}", String::from_utf8_lossy(&exit.stdout));
    assert!(exit.stderr.is_empty());
    let report = parsed(&exit);
    assert_eq!(report.outcome, "compiled");
    assert_eq!(report.reason, "not_executed");
    assert!(!report.metric_certified);
    assert!(!report.database_used);
    assert_eq!(report.gates[0].status, "block");
    assert_eq!(report.gates[0].reason, "not_executed");
    assert_eq!(report.gates[1].reason, "not_executed");
    assert!(report.enum_resolution.certification_allowed);
    assert_eq!(report.enum_resolution.classes[0].codes, vec!["C".to_owned()]);
    assert!(!report.semantic_diff.owner_review_required);
    assert_eq!(report.lifecycle.status.as_deref(), Some("active"));
    assert_eq!(report.lifecycle.owner.as_deref(), Some("finance"));
    assert_eq!(report.lifecycle.synonyms, vec!["receipts".to_owned()]);
    assert_eq!(report.tests.len(), 1);
    assert_eq!(report.tests[0].status, "not_executed");
    assert!(report.metric_query.executed_sql.to_ascii_lowercase().starts_with("select"));
    assert!(report.metric_query.executed_sql.contains("$1"));
    assert!(!report.metric_query.executed_sql.contains("oo_mr_probe_"));
    assert!(!report.metric_query.executed_sql.contains("810316"));
    assert!(!report.metric_query.executed_sql.contains("2026-03-01"));
    assert!(!report.metric_query.bound_sql.contains(":period_start"));
    assert_ne!(report.metric_query.guard_fingerprint, report.control_query.guard_fingerprint);
    assert_eq!(report.comparison.tolerance_abs.as_deref(), Some("0"));
    assert_shell(&report);
}

#[test]
fn finance_group_by_is_refused_before_execute() {
    let metric = include_str!("../../../examples/schema/metrics-finance.yaml");
    let enum_doc = r#"schema_version: oos/v1
enum: tms_pg.receipt.status
classes:
  confirmed: confirmed
values:
  - code: "1"
    class: confirmed
    status: approved
    source: dictionary_table
"#;
    let mut doc = doc_with(metric, &[enum_doc], None);
    doc["parameters"] = json!({
        "start": {"type": "text", "value": "2026-01-01"},
        "end": {"type": "text", "value": "2026-02-01"},
        "period_start": {"type": "text", "value": "2026-03-01"},
        "period_end": {"type": "text", "value": "2026-04-01"},
    });
    doc["catalog"] = json!({
        "relations": [],
        "functions": [],
        "operators": [],
        "literal_types": {"integer": 23, "text": 25, "boolean": 16},
        "null_test_allowed": true,
        "large_table_row_threshold": 100000
    });
    let report = refused(&doc);
    assert_eq!(report.reason, "guard");
    assert_eq!(report.gates[0].status, "failed");
    assert_eq!(report.gates[0].reason, "guard");
    assert!(report.metric_query.bound_sql.contains("GROUP BY"));
    assert!(report.metric_query.bound_sql.contains("date_trunc"));
    assert!(report.metric_query.executed_sql.is_empty());
}

#[test]
fn local_refusals_keep_their_tokens() {
    let mut open = happy();
    open["anchor"]["closed"] = json!(false);
    assert_eq!(refused(&open).reason, "anchor_open");

    let mut gap = happy();
    gap["anchor"]["date"] = Value::Null;
    assert_eq!(refused(&gap).reason, "gap_anchor");

    let mut shifted = happy();
    shifted["as_of"] = json!("2026-04-02");
    assert_eq!(refused(&shifted).reason, "as_of_anchor");

    let mut source = happy();
    source["source"] = json!("other_src");
    assert_eq!(refused(&source).reason, "source_mismatch");

    let negative = HAPPY.replace("abs: \"0\"", "abs: \"-1\"");
    assert_eq!(refused(&doc_with(&negative, &[CONFIRMED], None)).reason, "invalid_tolerance");

    let angles = HAPPY.replace("status = :status", "status <> :status");
    assert_eq!(refused(&doc_with(&angles, &[CONFIRMED], None)).reason, "negative_code");

    let missing = doc_with(HAPPY, &[], None);
    assert_eq!(refused(&missing).reason, "missing_enum");

    let unknown = r#"schema_version: oos/v1
enum: src.receipt.status
classes:
  other: other
values:
  - code: C
    class: other
    status: approved
    source: dictionary_table
    valid_from: "2020-01-01"
"#;
    assert_eq!(refused(&doc_with(HAPPY, &[unknown], None)).reason, "unknown_class");

    let gap_class = CONFIRMED.replace("2020-01-01", "2026-05-01");
    assert_eq!(refused(&doc_with(HAPPY, &[&gap_class], None)).reason, "class_gap");

    let inferred = r#"schema_version: oos/v1
enum: src.receipt.status
classes:
  confirmed: confirmed receipt
values:
  - code: C
    class: confirmed
    status: approved
    source: dictionary_table
    valid_from: "2020-01-01"
  - code: X
    class: confirmed
    status: deprecated
    source: inferred
    valid_from: "2020-01-01"
"#;
    assert_eq!(refused(&doc_with(HAPPY, &[inferred], None)).reason, "inferred_unapproved");

    let approved_inferred = r#"schema_version: oos/v1
enum: src.receipt.status
classes:
  confirmed: confirmed receipt
values:
  - code: C
    class: confirmed
    status: approved
    source: inferred
"#;
    assert_eq!(refused(&doc_with(HAPPY, &[approved_inferred], None)).reason, "enum_document");

    assert_eq!(refused(&doc_with("nope", &[], None)).reason, "metric_count");
    let bare = "schema_version: oos/v1\nmetrics:\n  - metric: monthly_receipts\n    label: Monthly receipts\n    status: draft\n    sql: \"SELECT amount FROM src.receipt\"\n";
    assert_eq!(refused(&doc_with(bare, &[], None)).reason, "control_total");

    let mut role = happy();
    role["identity"]["role"] = json!("finance");
    role["metric_yaml"] = json!("nope");
    assert_eq!(refused(&role).reason, "identity");

    assert_eq!(refused(&doc_with(HAPPY, &[CONFIRMED], Some("nope"))).reason, "previous_metric");
}

#[test]
fn same_path_and_string_literal_angles() {
    let same = r#"schema_version: oos/v1
metrics:
  - metric: monthly_receipts
    label: Monthly receipts
    status: draft
    sql: "SELECT amount FROM src.receipt WHERE status = :status"
    control_total:
      as_of: last_closed_period_end
      source: tms_pg
      sql: "SELECT  amount FROM src.receipt WHERE status = :status"
      tolerance: { abs: "0" }
"#;
    let mut doc = doc_with(same, &[], None);
    doc["parameters"] = json!({"status": {"type": "text", "value": "C"}});
    doc["anchor_bound"] = json!([]);
    let report = refused(&doc);
    assert_eq!(report.reason, "same_path");
    assert_eq!(report.gates[0].status, "block");

    let literal = r#"schema_version: oos/v1
metrics:
  - metric: monthly_receipts
    label: Monthly receipts
    status: draft
    sql: "SELECT amount FROM src.receipt WHERE note = '<>' AND status = :status"
    tests:
      - params: { status: C }
        expect: { rows: 1 }
    control_total:
      as_of: last_closed_period_end
      source: tms_pg
      sql: "SELECT total FROM ctl.period_total WHERE period_end = :period_end"
      tolerance: { abs: "0" }
"#;
    let mut doc = doc_with(literal, &[], None);
    doc["parameters"] = json!({
        "status": {"type": "text", "value": "C"},
        "period_end": {"type": "text", "value": "2026-04-01"},
    });
    let exit = compile(&doc);
    assert_eq!(exit.code, 0, "stdout {}", String::from_utf8_lossy(&exit.stdout));
    let report = parsed(&exit);
    assert_eq!(report.outcome, "compiled");
    assert!(report.metric_query.executed_sql.contains("$1"));

    let nulls = r#"schema_version: oos/v1
metrics:
  - metric: monthly_receipts
    label: Monthly receipts
    status: draft
    sql: "SELECT amount FROM src.receipt WHERE amount IS NOT NULL"
    control_total:
      as_of: last_closed_period_end
      source: tms_pg
      sql: "SELECT total FROM ctl.period_total WHERE total IS NOT NULL"
      tolerance: { abs: "0" }
"#;
    let mut doc = doc_with(nulls, &[], None);
    doc["parameters"] = json!({});
    doc["anchor_bound"] = json!([]);
    let exit = compile(&doc);
    assert_eq!(exit.code, 0, "stdout {}", String::from_utf8_lossy(&exit.stdout));
    assert_eq!(parsed(&exit).outcome, "compiled");
}

#[test]
fn description_and_scale_do_not_require_review() {
    let previous = HAPPY
        .replace("owner: finance\n", "owner: finance\n    description: Kept off the review\n")
        .replace("abs: \"0\"", "abs: \"0.00\"");
    let exit = compile(&doc_with(HAPPY, &[CONFIRMED], Some(&previous)));
    assert_eq!(exit.code, 0, "stdout {}", String::from_utf8_lossy(&exit.stdout));
    let report = parsed(&exit);
    assert!(!report.semantic_diff.owner_review_required);
    assert!(report.semantic_diff.changed.is_empty());
    assert_eq!(
        report.semantic_diff.previous_metric_sql_sha256.as_deref(),
        Some(report.semantic_diff.metric_sql_sha256.as_str())
    );

    let grain = HAPPY.replace("grain: [receipt]", "grain: [month]");
    let report = parsed(&compile(&doc_with(HAPPY, &[CONFIRMED], Some(&grain))));
    assert!(report.semantic_diff.owner_review_required);
    assert_eq!(report.semantic_diff.changed, vec!["grain".to_owned()]);
    assert!(!report.metric_certified);
    assert!(report.metric_query.executed_sql.contains("src.receipt") || report.metric_query.executed_sql.contains("receipt"));
}

#[tokio::test]
async fn evaluate_reads_credentials_from_files_only() {
    let dir = std::env::temp_dir().join(format!("oo-a316-metric-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp");
    let password = dir.join("login.pass");
    let secret = "0123456789abcdef\n";
    std::fs::write(&password, secret).expect("write");
    let mut loose = std::fs::metadata(&password).expect("meta").permissions();
    loose.set_mode(0o644);
    std::fs::set_permissions(&password, loose).expect("mode");
    let config = dir.join("config.json");
    let mut body = json!({
        "host": "oo-a316-pg",
        "port": 5432,
        "database": "oo_a316",
        "user": "oo_a316_login",
        "password_file": password.display().to_string(),
        "ca_file": "/no/such/ca.pem",
        "statement_timeout_ms": 5000,
        "max_result_bytes": 8192,
        "force": true
    });
    std::fs::write(&config, serde_json::to_vec(&body).expect("config")).expect("write");
    let stdin = serde_json::to_vec(&happy()).expect("stdin");
    let args = vec!["evaluate".to_owned(), "--config".to_owned(), config.display().to_string()];
    let exit = dispatch(&args, &stdin).await;
    assert_eq!(exit.code, 1);
    assert!(exit.stdout.is_empty());
    assert_eq!(exit.stderr, b"metric-reconcile: malformed config\n");

    body.as_object_mut().expect("object").remove("force");
    std::fs::write(&config, serde_json::to_vec(&body).expect("config")).expect("write");
    let exit = dispatch(&args, &stdin).await;
    assert_eq!(exit.code, 1);
    assert!(exit.stdout.is_empty());
    assert_eq!(exit.stderr, b"metric-reconcile: malformed password_file\n");
    assert!(!exit.stderr.windows(secret.len()).any(|window| window == secret.as_bytes()));

    let mut tight = std::fs::metadata(&password).expect("meta").permissions();
    tight.set_mode(0o600);
    std::fs::set_permissions(&password, tight).expect("mode");
    let exit = dispatch(&args, &stdin).await;
    assert_eq!(exit.code, 1);
    assert!(exit.stdout.is_empty());
    assert_eq!(exit.stderr, b"metric-reconcile: malformed ca_file\n");
    assert!(!exit.stderr.windows(16).any(|window| window == b"0123456789abcdef"));
    let _ = std::fs::remove_dir_all(&dir);
}
