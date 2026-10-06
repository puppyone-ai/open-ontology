use enum_history::{
    CompileInput, Comparison, Observation, Outcome, MAX_OBSERVATIONS, compile,
};
use serde_json::Value;

fn run(yaml: &str, as_of: &str, observations: &[(&str, u64)], threshold: Option<&str>) -> Outcome {
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
        threshold: threshold.map(str::to_owned),
    })
}

fn compiled(
    yaml: &str,
    as_of: &str,
    observations: &[(&str, u64)],
    threshold: Option<&str>,
) -> enum_history::Report {
    match run(yaml, as_of, observations, threshold) {
        Outcome::Compiled(report) => *report,
        Outcome::Refused(refusal) => {
            let reason = refusal.reason;
            let detail = &refusal.detail;
            panic!("{reason} {detail}");
        }
    }
}

fn reason(yaml: &str, as_of: &str, observations: &[(&str, u64)]) -> String {
    match run(yaml, as_of, observations, None) {
        Outcome::Refused(refusal) => refusal.reason.to_owned(),
        Outcome::Compiled(_) => panic!("compiled"),
    }
}

fn detail(yaml: &str, as_of: &str) -> String {
    match run(yaml, as_of, &[("1", 1)], None) {
        Outcome::Refused(refusal) => refusal.detail,
        Outcome::Compiled(_) => panic!("compiled"),
    }
}

fn assert_no_float(value: &Value) {
    match value {
        Value::Number(number) => {
            assert!(
                number.is_i64() || number.is_u64(),
                "non-integer number {number}"
            );
        }
        Value::Array(items) => {
            for item in items {
                assert_no_float(item);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                assert_no_float(item);
            }
        }
        Value::String(_) | Value::Bool(_) | Value::Null => {}
    }
}

fn assert_closed(report: &enum_history::Report) {
    assert!(!report.database_used);
    assert!(!report.source_modified);
    assert!(!report.oo_dict_written);
    assert!(!report.metric_certified);
    assert!(!report.owner_authenticated);
    assert!(!report.sweeper_ran);
    let value = serde_json::to_value(report).expect("json");
    assert_no_float(&value);
}

#[test]
fn zero_rows_are_undefined_and_not_one() {
    let yaml = include_str!("fixtures/open.yaml");
    let report = compiled(yaml, "2026-01-01", &[], None);
    assert_closed(&report);
    assert!(!report.observed_coverage.defined);
    assert_eq!(report.observed_coverage.numerator, None);
    assert_eq!(report.observed_coverage.denominator, None);
    assert_eq!(report.observed_coverage.ratio, None);
    assert_eq!(report.threshold_met, None);
    assert_eq!(report.comparison, None);
    assert!(report.review_needed);
    assert_eq!(report.counts.observation_entries, "0");
    assert_eq!(report.counts.observed_rows, "0");
    assert_eq!(report.counts.business_rows, "0");
    assert_eq!(report.threshold.decimal, "0.95");
    assert_eq!(report.threshold.numerator, "95");
    assert_eq!(report.threshold.denominator, "100");
    assert!(report.threshold.defaulted);
    let body = serde_json::to_string(&report).expect("json");
    assert_eq!(
        body,
        r#"{"contract":"enum-history/v1","outcome":"compiled","database_used":false,"source_modified":false,"oo_dict_written":false,"metric_certified":false,"owner_authenticated":false,"sweeper_ran":false,"schema_version":"oos/v1","enum_name":"erp_mysql.ysk.zt","as_of":"2026-01-01","declared_owner":null,"declared_coverage":null,"declared_unexplained_values":[],"threshold":{"decimal":"0.95","numerator":"95","denominator":"100","defaulted":true},"rows":[{"source_index":0,"code":"1","label":null,"class":"open","status":"approved","source":"dictionary_table","valid_from":null,"valid_to":null,"valid_from_unbounded":true,"valid_to_unbounded":true,"evidence_signature":null,"evidence_rows":null,"review_state":null,"review_by_declared":null,"review_at":null,"historical_business_class":false,"business_class":"open"}],"bindings":[{"code":"1","active":true,"source_index":0,"status":"approved","raw_class":"open","business_class":"open","disposition":"business","reason":"approved"}],"warnings":[],"counts":{"observation_entries":"0","observed_rows":"0","business_rows":"0","unconfirmed_rows":"0","by_code":[]},"observed_coverage":{"defined":false,"numerator":null,"denominator":null,"ratio":null},"threshold_met":null,"comparison":null,"review_needed":true}"#
    );
    let zeros = compiled(yaml, "2026-01-01", &[("1", 0)], None);
    assert!(!zeros.observed_coverage.defined);
    assert_ne!(zeros.observed_coverage.ratio.as_deref(), Some("1"));
    assert_ne!(zeros.observed_coverage.ratio.as_deref(), Some("1/1"));
}

#[test]
fn full_business_coverage_needs_no_review() {
    let report = compiled(include_str!("fixtures/open.yaml"), "2026-01-01", &[("1", 20)], None);
    assert_closed(&report);
    assert_eq!(report.observed_coverage.ratio.as_deref(), Some("20/20"));
    assert_eq!(report.comparison, Some(Comparison::Above));
    assert_eq!(report.threshold_met, Some(true));
    assert!(report.warnings.is_empty());
    assert!(!report.review_needed);
}

#[test]
fn boundary_includes_start_and_excludes_end() {
    let yaml = include_str!("fixtures/boundary.yaml");
    for as_of in ["2020-01-01", "2020-01-31"] {
        let report = compiled(yaml, as_of, &[("1", 10)], None);
        assert_eq!(report.bindings[0].reason, "approved");
        assert_eq!(report.bindings[0].business_class.as_deref(), Some("open"));
        assert_eq!(report.observed_coverage.ratio.as_deref(), Some("10/10"));
        assert!(!report.review_needed);
    }
    let end = compiled(yaml, "2020-02-01", &[("1", 10)], None);
    assert_eq!(end.bindings[0].reason, "no_active");
    assert_eq!(end.bindings[0].business_class, None);
    assert_eq!(end.counts.business_rows, "0");
    assert_eq!(end.counts.unconfirmed_rows, "10");
    assert_eq!(end.observed_coverage.ratio.as_deref(), Some("0/10"));
    assert_eq!(end.comparison, Some(Comparison::Below));
    assert_eq!(end.threshold_met, Some(false));
    assert_eq!(end.warnings[0].kind, "expired_no_active");
    assert!(end.review_needed);
    assert_eq!(end.rows[0].status, "approved");
}

#[test]
fn gap_keeps_both_rows_and_does_not_promote() {
    let yaml = include_str!("fixtures/gap.yaml");
    let report = compiled(yaml, "2020-06-01", &[("1", 4)], None);
    assert_eq!(report.rows.len(), 2);
    assert_eq!(report.rows[0].valid_from.as_deref(), Some("2019-01-01"));
    assert_eq!(report.rows[1].valid_from.as_deref(), Some("2021-01-01"));
    assert!(report.rows[1].valid_to_unbounded);
    assert!(!report.bindings[0].active);
    assert_eq!(report.bindings[0].reason, "no_active");
    assert_eq!(report.observed_coverage.ratio.as_deref(), Some("0/4"));
    let before = compiled(yaml, "2019-06-01", &[("1", 4)], None);
    assert_eq!(before.bindings[0].business_class.as_deref(), Some("open"));
    let after = compiled(yaml, "2021-01-01", &[("1", 4)], None);
    assert_eq!(after.bindings[0].business_class.as_deref(), Some("closed"));
    assert_eq!(after.bindings[0].source_index, Some(1));
}

#[test]
fn dormant_future_overlap_and_historical_overlap_refuse() {
    let future = detail(include_str!("fixtures/overlap-future.yaml"), "2020-01-01");
    assert_eq!(
        reason(include_str!("fixtures/overlap-future.yaml"), "2020-01-01", &[("1", 1)]),
        "overlap"
    );
    assert_eq!(future, "code `1` source_index 1 and 2");
    let historical = detail(include_str!("fixtures/overlap-historical.yaml"), "2026-01-01");
    assert_eq!(historical, "code `1` source_index 0 and 1");
}

#[test]
fn reversed_and_zero_intervals_refuse() {
    assert_eq!(
        reason(include_str!("fixtures/reversed.yaml"), "2020-01-15", &[]),
        "reversed_interval"
    );
    assert_eq!(
        reason(include_str!("fixtures/zero-interval.yaml"), "2020-01-01", &[]),
        "zero_interval"
    );
}

#[test]
fn retroactive_correction_preserves_history_and_declared_coverage() {
    let yaml = include_str!("fixtures/retroactive.yaml");
    let snapshot = yaml.to_owned();
    let past = compiled(yaml, "2023-12-31", &[("4", 8)], None);
    assert_eq!(snapshot, yaml);
    assert_eq!(past.rows.len(), 2);
    assert_eq!(past.rows[0].status, "superseded");
    assert!(past.rows[0].historical_business_class);
    assert_eq!(past.rows[0].business_class.as_deref(), Some("open"));
    assert_eq!(past.rows[1].status, "approved");
    assert!(!past.rows[1].historical_business_class);
    assert_eq!(past.bindings[0].reason, "historical_approved");
    assert_eq!(past.bindings[0].business_class.as_deref(), Some("open"));
    assert_eq!(past.bindings[0].source_index, Some(0));
    assert_eq!(past.declared_coverage.as_deref(), Some("0.50"));
    assert_eq!(past.observed_coverage.ratio.as_deref(), Some("8/8"));
    assert_ne!(past.declared_coverage, past.observed_coverage.ratio);
    assert!(!past.owner_authenticated);
    assert_eq!(past.rows[0].review_by_declared.as_deref(), Some("finance.li"));
    let present = compiled(yaml, "2024-01-01", &[("4", 8)], None);
    assert_eq!(present.bindings[0].reason, "approved");
    assert_eq!(present.bindings[0].business_class.as_deref(), Some("closed"));
    assert_eq!(present.bindings[0].source_index, Some(1));
    assert_eq!(present.rows[0].status, "superseded");
    let before = compiled(yaml, "2018-12-31", &[("4", 8)], None);
    assert_eq!(before.bindings[0].reason, "no_active");
    assert_eq!(before.observed_coverage.ratio.as_deref(), Some("0/8"));
}

#[test]
fn superseded_without_review_is_not_promoted() {
    let report = compiled(
        include_str!("fixtures/superseded-unapproved.yaml"),
        "2023-12-31",
        &[("4", 6)],
        None,
    );
    assert_eq!(report.rows[0].status, "superseded");
    assert!(!report.rows[0].historical_business_class);
    assert_eq!(report.rows[0].business_class, None);
    assert_eq!(report.bindings[0].reason, "historical_unapproved");
    assert_eq!(report.counts.business_rows, "0");
    assert_eq!(report.counts.unconfirmed_rows, "6");
    assert_eq!(report.warnings[0].kind, "historical_unapproved");
}

#[test]
fn deprecated_with_review_keeps_status_and_historical_class() {
    let report = compiled(
        include_str!("fixtures/deprecated-historical.yaml"),
        "2020-01-01",
        &[("4", 3)],
        None,
    );
    assert_eq!(report.rows[0].status, "deprecated");
    assert!(report.rows[0].historical_business_class);
    assert_eq!(report.bindings[0].reason, "historical_approved");
    assert_eq!(report.bindings[0].business_class.as_deref(), Some("open"));
    assert_eq!(report.observed_coverage.ratio.as_deref(), Some("3/3"));
    assert!(!report.owner_authenticated);
}

#[test]
fn reserved_names_never_become_business_classes() {
    assert_eq!(
        reason(include_str!("fixtures/reserved-class.yaml"), "2026-01-01", &[]),
        "reserved_class"
    );
    assert_eq!(
        reason(include_str!("fixtures/reserved-row.yaml"), "2019-06-01", &[]),
        "reserved_class"
    );
}

#[test]
fn inferred_approval_requires_core_review_fields() {
    assert_eq!(
        reason(include_str!("fixtures/inferred-missing.yaml"), "2026-01-01", &[]),
        "inferred_unapproved"
    );
    assert_eq!(
        reason(include_str!("fixtures/inferred-no-by.yaml"), "2026-01-01", &[]),
        "inferred_unapproved"
    );
    assert_eq!(
        reason(include_str!("fixtures/inferred-rejected.yaml"), "2026-01-01", &[]),
        "review_contradicts_status"
    );
    let report = compiled(
        include_str!("fixtures/inferred-approved.yaml"),
        "2026-01-01",
        &[("4", 2)],
        None,
    );
    assert_eq!(report.bindings[0].business_class.as_deref(), Some("closed"));
    assert_eq!(report.rows[0].review_by_declared.as_deref(), Some("finance.li"));
    assert!(!report.owner_authenticated);
    assert!(!report.review_needed);
}

#[test]
fn unknown_code_is_unexplained_and_exact_threshold_matches() {
    let yaml = include_str!("fixtures/open.yaml");
    let exact = compiled(yaml, "2026-01-01", &[("1", 95), ("9", 5)], None);
    assert_eq!(exact.observed_coverage.ratio.as_deref(), Some("95/100"));
    assert_eq!(exact.comparison, Some(Comparison::Equal));
    assert_eq!(exact.threshold_met, Some(true));
    assert_eq!(exact.counts.business_rows, "95");
    assert_eq!(exact.counts.unconfirmed_rows, "5");
    assert_eq!(exact.warnings.len(), 1);
    assert_eq!(exact.warnings[0].code, "9");
    assert_eq!(exact.warnings[0].kind, "unexplained");
    assert_eq!(exact.warnings[0].reason, "unknown_code");
    assert!(exact.review_needed);
    assert!(exact.bindings.iter().all(|binding| binding.code != "9"));
    let nineteen = compiled(yaml, "2026-01-01", &[("1", 19), ("9", 1)], Some("0.950"));
    assert_eq!(nineteen.threshold.numerator, "950");
    assert_eq!(nineteen.threshold.denominator, "1000");
    assert!(!nineteen.threshold.defaulted);
    assert_eq!(nineteen.observed_coverage.ratio.as_deref(), Some("19/20"));
    assert_eq!(nineteen.comparison, Some(Comparison::Equal));
    let below = compiled(yaml, "2026-01-01", &[("1", 94), ("9", 6)], None);
    assert_eq!(below.observed_coverage.ratio.as_deref(), Some("94/100"));
    assert_eq!(below.comparison, Some(Comparison::Below));
    assert_eq!(below.threshold_met, Some(false));
}

#[test]
fn active_undetermined_warns_even_when_unobserved() {
    let yaml = "schema_version: oos/v1\nenum: erp_mysql.ysk.zt\nclasses:\n  open: still open\nvalues:\n  - { code: \"1\", class: open, status: approved, source: dictionary_table }\n  - { code: \"6\", class: undetermined, status: undetermined }\n";
    let report = compiled(yaml, "2026-01-01", &[("1", 20)], None);
    assert_eq!(report.observed_coverage.ratio.as_deref(), Some("20/20"));
    assert_eq!(report.threshold_met, Some(true));
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.warnings[0].code, "6");
    assert_eq!(report.warnings[0].kind, "undetermined");
    assert_eq!(report.warnings[0].count, "0");
    assert!(report.review_needed);
}

#[test]
fn class_not_in_the_map_is_not_historical_business() {
    let yaml = "schema_version: oos/v1\nenum: erp_mysql.ysk.zt\nclasses:\n  open: still open\nvalues:\n  - code: \"1\"\n    class: other\n    status: superseded\n    source: human\n    valid_from: \"2019-01-01\"\n    valid_to: \"2020-01-01\"\n    review: { state: approved, by: finance.li }\n";
    let report = compiled(yaml, "2019-06-01", &[("1", 4)], None);
    assert_eq!(report.rows[0].status, "superseded");
    assert!(!report.rows[0].historical_business_class);
    assert_eq!(report.bindings[0].reason, "class_not_business");
    assert_eq!(report.counts.business_rows, "0");
}

#[test]
fn duplicate_observation_and_overflow_refuse_without_a_report() {
    let yaml = include_str!("fixtures/two-codes.yaml");
    match run(yaml, "2026-01-01", &[("1", 1), ("1", 2)], None) {
        Outcome::Refused(refusal) => {
            assert_eq!(refusal.reason, "duplicate_observation");
            assert!(refusal.detail.contains('1'));
            assert!(!refusal.source_modified);
        }
        Outcome::Compiled(_) => panic!("merged"),
    }
    match run(yaml, "2026-01-01", &[("1", u64::MAX), ("2", 1)], None) {
        Outcome::Refused(refusal) => assert_eq!(refusal.reason, "count_overflow"),
        Outcome::Compiled(_) => panic!("wrapped"),
    }
    let max = compiled(yaml, "2026-01-01", &[("1", u64::MAX)], None);
    assert_eq!(max.counts.observed_rows, u64::MAX.to_string());
    assert_eq!(max.observed_coverage.numerator, max.observed_coverage.denominator);
    assert_eq!(max.comparison, Some(Comparison::Above));
}

#[test]
fn observation_bound_precedes_core_parse() {
    let observations: Vec<(&str, u64)> = (0..=MAX_OBSERVATIONS)
        .map(|index| {
            let code = Box::leak(format!("c{index}").into_boxed_str());
            (code as &str, 1u64)
        })
        .collect();
    match run("not yaml", "2026-01-01", &observations, None) {
        Outcome::Refused(refusal) => assert_eq!(refusal.reason, "observation_bound"),
        Outcome::Compiled(_) => panic!("compiled"),
    }
}

#[test]
fn bad_date_and_threshold_and_core_yaml_refuse() {
    match run(include_str!("fixtures/open.yaml"), "2020-02-31", &[], None) {
        Outcome::Refused(refusal) => assert_eq!(refusal.reason, "invalid_as_of"),
        Outcome::Compiled(_) => panic!("compiled"),
    }
    match run(include_str!("fixtures/open.yaml"), "2026-01-01", &[], Some("-0.1")) {
        Outcome::Refused(refusal) => assert_eq!(refusal.reason, "invalid_threshold"),
        Outcome::Compiled(_) => panic!("compiled"),
    }
    match run("enum: erp_mysql.ysk.zt\n", "2026-01-01", &[], None) {
        Outcome::Refused(refusal) => assert_eq!(refusal.reason, "core_enum"),
        Outcome::Compiled(_) => panic!("compiled"),
    }
}

#[test]
fn canonical_enum_keeps_declared_coverage_separate() {
    let yaml = include_str!("fixtures/enum-ysk-zt.yaml");
    let snapshot = yaml.to_owned();
    let report = compiled(
        yaml,
        "2026-10-06",
        &[
            ("1", 40),
            ("2", 30),
            ("3", 15),
            ("4", 10),
            ("5", 5),
            ("6", 3),
            ("7", 2),
        ],
        None,
    );
    assert_eq!(snapshot, yaml);
    assert_closed(&report);
    assert_eq!(report.declared_owner.as_deref(), Some("finance"));
    assert_eq!(report.declared_coverage.as_deref(), Some("0.97"));
    assert_eq!(report.declared_unexplained_values, vec!["7".to_owned()]);
    assert_eq!(report.rows.len(), 6);
    assert_eq!(report.rows[0].label.as_deref(), Some("待收款"));
    let code4 = report.rows.iter().find(|row| row.code == "4").expect("4");
    assert_eq!(code4.status, "approved");
    assert_eq!(code4.business_class.as_deref(), Some("closed"));
    assert_eq!(code4.review_by_declared.as_deref(), Some("finance.li"));
    assert_eq!(code4.review_at.as_deref(), Some("2026-10-12"));
    assert_eq!(code4.evidence_rows.as_deref(), Some("1832"));
    assert!(!code4.historical_business_class);
    let code5 = report.rows.iter().find(|row| row.code == "5").expect("5");
    assert_eq!(code5.review_at, None);
    assert_eq!(code5.business_class.as_deref(), Some("void"));
    assert_eq!(report.bindings.len(), 6);
    let binding4 = report.bindings.iter().find(|row| row.code == "4").expect("4");
    assert_eq!(binding4.reason, "approved");
    assert_eq!(report.counts.observed_rows, "105");
    assert_eq!(report.counts.business_rows, "100");
    assert_eq!(report.counts.unconfirmed_rows, "5");
    assert_eq!(report.observed_coverage.ratio.as_deref(), Some("100/105"));
    assert_ne!(
        report.observed_coverage.ratio.as_deref(),
        report.declared_coverage.as_deref()
    );
    assert_eq!(report.comparison, Some(Comparison::Above));
    assert_eq!(report.threshold_met, Some(true));
    assert_eq!(report.warnings.len(), 2);
    assert_eq!(report.warnings[0].code, "6");
    assert_eq!(report.warnings[0].kind, "undetermined");
    assert_eq!(report.warnings[0].count, "3");
    assert_eq!(report.warnings[1].code, "7");
    assert_eq!(report.warnings[1].kind, "unexplained");
    assert_eq!(report.warnings[1].reason, "unknown_code");
    assert!(report.review_needed);
    let early = compiled(yaml, "2018-12-31", &[("4", 10)], None);
    let early4 = early.bindings.iter().find(|row| row.code == "4").expect("4");
    assert_eq!(early4.reason, "no_active");
    assert_eq!(early.counts.business_rows, "0");
    assert!(early.warnings.iter().any(|warning| warning.code == "6"));
}
