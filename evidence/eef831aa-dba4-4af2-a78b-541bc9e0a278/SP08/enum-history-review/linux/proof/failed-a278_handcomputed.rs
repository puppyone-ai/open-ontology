//! Independent hand literals for SEM-D02. These dates are not the author fixtures.
//! The observation vector is caller-supplied. Nothing here opens a database.

#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Stdio};

use enum_history::{Comparison, CompileInput, Observation, Outcome, Report, compile};
use serde_json::{Value, json};

const ADJACENT: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
owner: treasury
classes:
  open: posted and open
  closed: posted and closed
coverage: \"0.40\"
unexplained_values:
  - \"z\"
values:
  - code: \"k\"
    label: earlier open
    class: open
    status: superseded
    source: human
    valid_from: \"2012-02-01\"
    valid_to: \"2014-07-01\"
    review: { state: approved, by: treasury.chen }
  - code: \"k\"
    label: later closed
    class: closed
    status: approved
    source: dictionary_table
    valid_from: \"2014-07-01\"
";

const GAP: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  open: posted and open
  closed: posted and closed
values:
  - code: \"k\"
    class: open
    status: approved
    source: dictionary_table
    valid_from: \"2011-01-01\"
    valid_to: \"2012-01-01\"
  - code: \"k\"
    class: closed
    status: approved
    source: dictionary_table
    valid_from: \"2013-05-01\"
    valid_to: \"2013-09-01\"
";

const FUTURE_OVERLAP: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  open: posted and open
  closed: posted and closed
values:
  - code: \"k\"
    class: open
    status: approved
    source: dictionary_table
    valid_from: \"2028-01-01\"
    valid_to: \"2029-01-01\"
  - code: \"k\"
    class: closed
    status: approved
    source: dictionary_table
    valid_from: \"2028-06-01\"
    valid_to: \"2030-01-01\"
";

const HISTORICAL_OVERLAP: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  open: posted and open
  closed: posted and closed
values:
  - code: \"k\"
    class: open
    status: approved
    source: dictionary_table
    valid_from: \"2009-03-01\"
    valid_to: \"2009-12-01\"
  - code: \"k\"
    class: closed
    status: approved
    source: dictionary_table
    valid_from: \"2009-07-01\"
    valid_to: \"2010-02-01\"
";

const INFERRED: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  closed: posted and closed
values:
  - code: \"m\"
    class: closed
    status: approved
    source: inferred
    review: { state: approved, by: treasury.chen, at: \"2014-03-02\" }
";

const INFERRED_NO_BY: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  closed: posted and closed
values:
  - code: \"m\"
    class: closed
    status: approved
    source: inferred
    review: { state: approved, at: \"2014-03-02\" }
";

const SUPERSEDED_BARE: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  open: posted and open
values:
  - code: \"k\"
    label: old open
    class: open
    status: superseded
    source: human
    valid_from: \"2013-01-01\"
    valid_to: \"2015-01-01\"
";

const RESERVED_KEY: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  unexplained: not a business class
  open: posted and open
values:
  - code: \"k\"
    class: open
    status: approved
    source: dictionary_table
";

const RESERVED_APPROVED: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  open: posted and open
values:
  - code: \"k\"
    class: undetermined
    status: approved
    source: dictionary_table
";

const FORCED_UNDETERMINED: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  open: posted and open
values:
  - code: \"k\"
    class: open
    status: approved
    source: dictionary_table
  - code: \"u\"
    class: undetermined
    status: undetermined
";

const OPEN_K: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  open: posted and open
values:
  - code: \"k\"
    class: open
    status: approved
    source: dictionary_table
";

const TWO_BUSINESS: &str = "\
schema_version: oos/v1
enum: erp_mysql.ysk.zt
classes:
  open: posted and open
  closed: posted and closed
values:
  - code: \"k\"
    class: open
    status: approved
    source: dictionary_table
  - code: \"m\"
    class: closed
    status: approved
    source: dictionary_table
";

fn run(yaml: &str, as_of: &str, observations: &[(&str, u64)]) -> Outcome {
    compile(&CompileInput {
        enum_yaml: yaml.to_owned(),
        as_of: as_of.to_owned(),
        observations: observations
            .iter()
            .map(|(code, count)| Observation {
                code: (*code).to_owned(),
                count: *count,
            })
            .collect(),
        threshold: None,
    })
}

fn compiled(yaml: &str, as_of: &str, observations: &[(&str, u64)]) -> Report {
    match run(yaml, as_of, observations) {
        Outcome::Compiled(report) => *report,
        Outcome::Refused(refusal) => panic!("{} {}", refusal.reason, refusal.detail),
    }
}

fn refusal(yaml: &str, as_of: &str, observations: &[(&str, u64)]) -> enum_history::Refusal {
    match run(yaml, as_of, observations) {
        Outcome::Refused(refusal) => refusal,
        Outcome::Compiled(_) => panic!("compiled"),
    }
}

fn assert_closed(report: &Report) {
    assert!(!report.database_used);
    assert!(!report.source_modified);
    assert!(!report.oo_dict_written);
    assert!(!report.metric_certified);
    assert!(!report.owner_authenticated);
    assert!(!report.sweeper_ran);
}

fn assert_refusal_closed(refusal: &enum_history::Refusal) {
    assert!(!refusal.database_used);
    assert!(!refusal.source_modified);
    assert!(!refusal.oo_dict_written);
    assert!(!refusal.metric_certified);
    assert!(!refusal.owner_authenticated);
    assert!(!refusal.sweeper_ran);
    let value = serde_json::to_value(refusal).expect("json");
    assert!(value.get("rows").is_none());
    assert!(value.get("observed_coverage").is_none());
    assert!(value.get("bindings").is_none());
}

#[test]
fn adjacent_half_open_keeps_history_and_declared_coverage() {
    let snapshot = ADJACENT.to_owned();
    let on_end = compiled(ADJACENT, "2014-07-01", &[("k", 8)]);
    assert_eq!(snapshot, ADJACENT);
    assert_closed(&on_end);
    assert_eq!(on_end.declared_owner.as_deref(), Some("treasury"));
    assert_eq!(on_end.declared_coverage.as_deref(), Some("0.40"));
    assert_eq!(on_end.declared_unexplained_values, vec!["z".to_owned()]);
    assert_eq!(on_end.rows.len(), 2);
    assert_eq!(on_end.rows[0].status, "superseded");
    assert_eq!(on_end.rows[0].label.as_deref(), Some("earlier open"));
    assert_eq!(on_end.rows[0].valid_from.as_deref(), Some("2012-02-01"));
    assert_eq!(on_end.rows[0].valid_to.as_deref(), Some("2014-07-01"));
    assert!(on_end.rows[0].historical_business_class);
    assert_eq!(on_end.rows[0].business_class.as_deref(), Some("open"));
    assert_eq!(on_end.rows[0].review_by_declared.as_deref(), Some("treasury.chen"));
    assert_eq!(on_end.rows[1].status, "approved");
    assert_eq!(on_end.rows[1].valid_from.as_deref(), Some("2014-07-01"));
    assert!(on_end.rows[1].valid_to_unbounded);
    assert!(!on_end.rows[1].historical_business_class);
    assert_eq!(on_end.rows[1].business_class.as_deref(), Some("closed"));
    assert_eq!(on_end.bindings.len(), 1);
    assert_eq!(on_end.bindings[0].reason, "approved");
    assert_eq!(on_end.bindings[0].source_index, Some(1));
    assert_eq!(on_end.bindings[0].business_class.as_deref(), Some("closed"));
    assert_eq!(on_end.observed_coverage.ratio.as_deref(), Some("8/8"));
    assert_ne!(on_end.declared_coverage.as_deref(), on_end.observed_coverage.ratio.as_deref());
    assert!(on_end.bindings.iter().all(|binding| binding.code != "z"));
    assert!(!on_end.review_needed);

    let day_before = compiled(ADJACENT, "2014-06-30", &[("k", 8)]);
    assert_eq!(day_before.bindings[0].reason, "historical_approved");
    assert_eq!(day_before.bindings[0].source_index, Some(0));
    assert_eq!(day_before.bindings[0].status, Some("superseded"));
    assert_eq!(day_before.bindings[0].business_class.as_deref(), Some("open"));
    assert_eq!(day_before.rows[0].status, "superseded");
    assert_eq!(day_before.rows[1].status, "approved");

    let start = compiled(ADJACENT, "2012-02-01", &[("k", 8)]);
    assert_eq!(start.bindings[0].reason, "historical_approved");
    assert_eq!(start.bindings[0].source_index, Some(0));

    let before = compiled(ADJACENT, "2012-01-31", &[("k", 8)]);
    assert_eq!(before.rows.len(), 2);
    assert_eq!(before.rows[0].status, "superseded");
    assert_eq!(before.bindings[0].reason, "no_active");
    assert!(!before.bindings[0].active);
    assert_eq!(before.observed_coverage.ratio.as_deref(), Some("0/8"));
    assert_eq!(before.counts.business_rows, "0");
    assert_eq!(before.declared_coverage.as_deref(), Some("0.40"));
}

#[test]
fn gap_day_binds_no_active_and_keeps_both_rows() {
    let gap = compiled(GAP, "2012-08-15", &[("k", 7)]);
    assert_closed(&gap);
    assert_eq!(gap.rows.len(), 2);
    assert_eq!(gap.rows[0].valid_to.as_deref(), Some("2012-01-01"));
    assert_eq!(gap.rows[1].valid_from.as_deref(), Some("2013-05-01"));
    assert_eq!(gap.bindings[0].reason, "no_active");
    assert_eq!(gap.counts.business_rows, "0");
    assert_eq!(gap.counts.unconfirmed_rows, "7");
    assert_eq!(gap.observed_coverage.ratio.as_deref(), Some("0/7"));
    assert_eq!(gap.warnings[0].kind, "expired_no_active");
    assert!(gap.review_needed);

    let start = compiled(GAP, "2011-01-01", &[("k", 7)]);
    assert_eq!(start.bindings[0].reason, "approved");
    assert_eq!(start.bindings[0].source_index, Some(0));
    assert_eq!(start.bindings[0].business_class.as_deref(), Some("open"));
    assert_eq!(start.observed_coverage.ratio.as_deref(), Some("7/7"));

    let end = compiled(GAP, "2012-01-01", &[("k", 7)]);
    assert_eq!(end.bindings[0].reason, "no_active");

    let later = compiled(GAP, "2013-08-31", &[("k", 7)]);
    assert_eq!(later.bindings[0].reason, "approved");
    assert_eq!(later.bindings[0].source_index, Some(1));
    assert_eq!(later.bindings[0].business_class.as_deref(), Some("closed"));

    let later_end = compiled(GAP, "2013-09-01", &[("k", 7)]);
    assert_eq!(later_end.bindings[0].reason, "no_active");
    assert_eq!(later_end.rows.len(), 2);
}

#[test]
fn future_and_historical_overlaps_refuse() {
    let future = refusal(FUTURE_OVERLAP, "2014-07-01", &[("k", 1)]);
    assert_eq!(future.reason, "overlap");
    assert_eq!(future.detail, "code `k` source_index 0 and 1");
    assert_refusal_closed(&future);

    let historical = refusal(HISTORICAL_OVERLAP, "2014-07-01", &[("k", 1)]);
    assert_eq!(historical.reason, "overlap");
    assert_eq!(historical.detail, "code `k` source_index 0 and 1");
    assert_refusal_closed(&historical);
}

#[test]
fn inferred_approved_needs_explicit_review() {
    let report = compiled(INFERRED, "2014-07-01", &[("m", 11)]);
    assert_closed(&report);
    assert_eq!(report.rows[0].source, Some("inferred"));
    assert_eq!(report.rows[0].review_state, Some("approved"));
    assert_eq!(report.rows[0].review_by_declared.as_deref(), Some("treasury.chen"));
    assert_eq!(report.rows[0].review_at.as_deref(), Some("2014-03-02"));
    assert_eq!(report.bindings[0].reason, "approved");
    assert_eq!(report.bindings[0].business_class.as_deref(), Some("closed"));
    assert_eq!(report.observed_coverage.ratio.as_deref(), Some("11/11"));
    assert!(!report.owner_authenticated);
    assert!(!report.review_needed);

    let missing = refusal(INFERRED_NO_BY, "2014-07-01", &[("m", 11)]);
    assert_eq!(missing.reason, "inferred_unapproved");
    assert_eq!(missing.detail, "source_index 0 is inferred without owner review fields");
    assert_refusal_closed(&missing);
}

#[test]
fn superseded_without_approval_stays_unconfirmed() {
    let report = compiled(SUPERSEDED_BARE, "2014-07-01", &[("k", 6)]);
    assert_closed(&report);
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].status, "superseded");
    assert_eq!(report.rows[0].label.as_deref(), Some("old open"));
    assert!(!report.rows[0].historical_business_class);
    assert_eq!(report.rows[0].business_class, None);
    assert_eq!(report.rows[0].review_state, None);
    assert_eq!(report.bindings[0].reason, "historical_unapproved");
    assert_eq!(report.bindings[0].disposition, "unconfirmed");
    assert_eq!(report.counts.business_rows, "0");
    assert_eq!(report.counts.unconfirmed_rows, "6");
    assert_eq!(report.warnings[0].kind, "historical_unapproved");
    assert_eq!(report.warnings[0].count, "6");
    assert!(report.review_needed);
    assert!(!report.owner_authenticated);
}

#[test]
fn reserved_classes_are_not_business() {
    let key = refusal(RESERVED_KEY, "2014-07-01", &[("k", 1)]);
    assert_eq!(key.reason, "reserved_class");
    assert_eq!(key.detail, "class `unexplained` is reserved");
    assert_refusal_closed(&key);

    let approved = refusal(RESERVED_APPROVED, "2014-07-01", &[("k", 1)]);
    assert_eq!(approved.reason, "reserved_class");
    assert_eq!(approved.detail, "source_index 0 uses reserved class `undetermined`");
    assert_refusal_closed(&approved);

    let forced = compiled(FORCED_UNDETERMINED, "2014-07-01", &[("k", 4)]);
    assert_closed(&forced);
    assert_eq!(forced.rows.len(), 2);
    let undetermined = forced.rows.iter().find(|row| row.code == "u").expect("u");
    assert_eq!(undetermined.status, "undetermined");
    assert_eq!(undetermined.business_class, None);
    assert!(!undetermined.historical_business_class);
    let binding = forced.bindings.iter().find(|row| row.code == "u").expect("u");
    assert_eq!(binding.disposition, "unconfirmed");
    assert_eq!(binding.reason, "undetermined");
    assert_eq!(binding.business_class, None);
    assert_eq!(forced.observed_coverage.ratio.as_deref(), Some("4/4"));
    assert_eq!(forced.threshold_met, Some(true));
    assert_eq!(forced.warnings.len(), 1);
    assert_eq!(forced.warnings[0].code, "u");
    assert_eq!(forced.warnings[0].kind, "undetermined");
    assert_eq!(forced.warnings[0].count, "0");
    assert!(forced.review_needed);
}

#[test]
fn unknown_zero_count_is_not_inserted() {
    let report = compiled(OPEN_K, "2014-07-01", &[("q", 0)]);
    assert_closed(&report);
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].code, "k");
    assert!(report.rows.iter().all(|row| row.code != "q"));
    assert!(report.bindings.iter().all(|binding| binding.code != "q"));
    assert_eq!(report.counts.observation_entries, "1");
    assert_eq!(report.counts.observed_rows, "0");
    assert_eq!(report.counts.business_rows, "0");
    assert!(!report.observed_coverage.defined);
    assert_eq!(report.observed_coverage.ratio, None);
    assert_ne!(report.observed_coverage.ratio.as_deref(), Some("0/0"));
    assert_ne!(report.observed_coverage.ratio.as_deref(), Some("1"));
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.warnings[0].code, "q");
    assert_eq!(report.warnings[0].kind, "unexplained");
    assert_eq!(report.warnings[0].reason, "unknown_code");
    assert_eq!(report.warnings[0].count, "0");
    assert_eq!(report.warnings[0].source_index, None);
    assert!(report.review_needed);
}

#[test]
fn exact_ninety_five_of_one_hundred() {
    let exact = compiled(OPEN_K, "2014-07-01", &[("k", 95), ("q", 5)]);
    assert_closed(&exact);
    assert_eq!(exact.threshold.decimal, "0.95");
    assert_eq!(exact.threshold.numerator, "95");
    assert_eq!(exact.threshold.denominator, "100");
    assert!(exact.threshold.defaulted);
    assert_eq!(exact.counts.observed_rows, "100");
    assert_eq!(exact.counts.business_rows, "95");
    assert_eq!(exact.counts.unconfirmed_rows, "5");
    assert_eq!(exact.observed_coverage.numerator.as_deref(), Some("95"));
    assert_eq!(exact.observed_coverage.denominator.as_deref(), Some("100"));
    assert_eq!(exact.observed_coverage.ratio.as_deref(), Some("95/100"));
    assert_eq!(exact.comparison, Some(Comparison::Equal));
    assert_eq!(exact.threshold_met, Some(true));
    assert!(exact.rows.iter().all(|row| row.code != "q"));
    assert!(exact.bindings.iter().all(|binding| binding.code != "q"));
    assert_eq!(exact.warnings.len(), 1);
    assert_eq!(exact.warnings[0].code, "q");
    assert_eq!(exact.warnings[0].reason, "unknown_code");
    assert_eq!(exact.warnings[0].count, "5");
    assert!(exact.review_needed);

    let below = compiled(OPEN_K, "2014-07-01", &[("k", 94), ("q", 6)]);
    assert_eq!(below.observed_coverage.ratio.as_deref(), Some("94/100"));
    assert_eq!(below.comparison, Some(Comparison::Below));
    assert_eq!(below.threshold_met, Some(false));
    assert!(below.review_needed);
}

#[test]
fn zero_observations_are_undefined() {
    let empty = compiled(OPEN_K, "2014-07-01", &[]);
    assert_closed(&empty);
    assert_eq!(empty.counts.observation_entries, "0");
    assert_eq!(empty.counts.observed_rows, "0");
    assert_eq!(empty.counts.business_rows, "0");
    assert!(!empty.observed_coverage.defined);
    assert_eq!(empty.observed_coverage.numerator, None);
    assert_eq!(empty.observed_coverage.denominator, None);
    assert_eq!(empty.observed_coverage.ratio, None);
    assert_eq!(empty.threshold_met, None);
    assert_eq!(empty.comparison, None);
    assert_eq!(empty.threshold.numerator, "95");
    assert_eq!(empty.threshold.denominator, "100");
    assert!(empty.review_needed);
    assert!(empty.warnings.is_empty());

    let zeros = compiled(OPEN_K, "2014-07-01", &[("k", 0)]);
    assert!(!zeros.observed_coverage.defined);
    assert_eq!(zeros.observed_coverage.ratio, None);
    assert_ne!(zeros.observed_coverage.ratio.as_deref(), Some("1"));
    assert_ne!(zeros.observed_coverage.ratio.as_deref(), Some("1/1"));
    assert_ne!(zeros.observed_coverage.ratio.as_deref(), Some("0/0"));
    assert!(zeros.review_needed);
    assert!(zeros.warnings.is_empty());
}

#[test]
fn overflow_refuses_without_a_wrapped_total() {
    let refused = refusal(TWO_BUSINESS, "2014-07-01", &[("k", u64::MAX), ("m", 1)]);
    assert_eq!(refused.reason, "count_overflow");
    assert_eq!(refused.detail, "observation counts overflow u64");
    assert_refusal_closed(&refused);

    let max = compiled(OPEN_K, "2014-07-01", &[("k", u64::MAX)]);
    assert_closed(&max);
    let text = u64::MAX.to_string();
    let ratio = format!("{text}/{text}");
    assert_eq!(max.counts.observed_rows, text);
    assert_eq!(max.counts.business_rows, text);
    assert_eq!(max.observed_coverage.ratio.as_deref(), Some(ratio.as_str()));
    assert_ne!(max.counts.observed_rows, "0");
    assert_eq!(max.comparison, Some(Comparison::Above));
    assert_eq!(max.threshold_met, Some(true));
}

fn bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_enum-history")
        .or(option_env!("CARGO_BIN_EXE_enum_history"))
        .expect("enum-history test binary")
}

fn cli(input: &[u8], args: &[&str]) -> (i32, String, String) {
    let mut child = Command::new(bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().expect("stdin").write_all(input).expect("write");
    let output = child.wait_with_output().expect("wait");
    let code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    (code, stdout, stderr)
}

fn flags(value: &Value) {
    for key in [
        "database_used",
        "source_modified",
        "oo_dict_written",
        "metric_certified",
        "owner_authenticated",
        "sweeper_ran",
    ] {
        assert_eq!(value[key], Value::Bool(false), "{key}");
    }
}

#[test]
fn compiled_cli_reports_history_and_refuses_overlap() {
    let request = serde_json::to_vec(&json!({
        "enum_yaml": ADJACENT,
        "as_of": "2014-07-01",
        "observations": [{"code": "k", "count": 8}],
    }))
    .expect("request");
    let (code, stdout, stderr) = cli(&request, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    assert_eq!(stdout.lines().count(), 1);
    let report: Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(report["contract"], "enum-history/v1");
    assert_eq!(report["outcome"], "compiled");
    flags(&report);
    assert_eq!(report["declared_coverage"], "0.40");
    assert_eq!(report["declared_unexplained_values"], json!(["z"]));
    assert_eq!(report["rows"].as_array().expect("rows").len(), 2);
    assert_eq!(report["rows"][0]["status"], "superseded");
    assert_eq!(report["rows"][1]["status"], "approved");
    assert_eq!(report["bindings"][0]["reason"], "approved");
    assert_eq!(report["bindings"][0]["business_class"], "closed");
    assert_eq!(report["observed_coverage"]["ratio"], "8/8");
    assert_ne!(report["declared_coverage"], report["observed_coverage"]["ratio"]);

    let overlap = serde_json::to_vec(&json!({
        "enum_yaml": FUTURE_OVERLAP,
        "as_of": "2014-07-01",
        "observations": [{"code": "k", "count": 1}],
    }))
    .expect("overlap");
    let (code, stdout, stderr) = cli(&overlap, &[]);
    assert_eq!(code, 2, "{stdout}");
    assert_eq!(stderr, "enum-history: refused overlap\n");
    assert_eq!(stdout.lines().count(), 1);
    let refused: Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(refused["outcome"], "refused");
    assert_eq!(refused["reason"], "overlap");
    assert_eq!(refused["detail"], "code `k` source_index 0 and 1");
    flags(&refused);
    assert!(refused.get("rows").is_none());

    let (code, stdout, stderr) = cli(b"", &["--help"]);
    assert_eq!(code, 1);
    assert_eq!(stderr, "enum-history: malformed usage\n");
    let malformed: Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(malformed["outcome"], "malformed");
    assert_eq!(malformed["reason"], "usage");
    flags(&malformed);
}
