use std::fs;
use std::path::PathBuf;

use oo_result_comparator::compare_document;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn expect_file(input_name: &str, report_name: &str) {
    let comparison = compare_document(&fixture(input_name)).expect(input_name);
    let report = fixture(report_name);
    assert_eq!(
        comparison.report + "\n",
        report,
        "{input_name} matched={}",
        comparison.matched
    );
    assert_eq!(comparison.matched, report.contains("\"verdict\":\"match\""));
}

#[test]
fn exact_decimal_and_int64_extremes_match() {
    expect_file("match-exact-decimal.json", "report-match.json");
}

#[test]
fn zero_coefficients_match_across_scale() {
    expect_file("match-zero.json", "report-match.json");
}

#[test]
fn negative_scale_rescales_exactly() {
    expect_file("match-negative-scale.json", "report-match.json");
}

#[test]
fn mismatched_decimal_is_not_a_float_tolerance() {
    expect_file("mismatch-decimal.json", "mismatch-decimal.report.json");
}

#[test]
fn null_does_not_match_missing() {
    expect_file("mismatch-null.json", "mismatch-null.report.json");
}

#[test]
fn unordered_keeps_duplicate_multiplicity() {
    expect_file("mismatch-duplicate.json", "mismatch-duplicate.report.json");
}

#[test]
fn partial_actual_does_not_match_complete() {
    expect_file("mismatch-partial.json", "mismatch-partial.report.json");
}

#[test]
fn completion_is_reported_before_a_decimal_mismatch() {
    expect_file(
        "mismatch-partial-decimal.json",
        "mismatch-partial-decimal.report.json",
    );
}

#[test]
fn not_found_matches_only_not_found() {
    expect_file("match-not-found.json", "report-match.json");
    expect_file("mismatch-not-found.json", "mismatch-not-found.report.json");
}

#[test]
fn warnings_are_ordered_and_exact() {
    expect_file("mismatch-warnings.json", "mismatch-warnings.report.json");
}

#[test]
fn ordered_rows_do_not_treat_a_swap_as_a_tie() {
    expect_file("mismatch-ordered.json", "mismatch-ordered.report.json");
}

#[test]
fn unordered_swap_matches_without_becoming_a_set() {
    expect_file("match-unordered.json", "report-match.json");
}
