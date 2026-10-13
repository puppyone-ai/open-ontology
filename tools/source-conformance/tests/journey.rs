//! Decisive journeys for matrix claims. These tests call the library rule path.
//! Calendar checks are logic tests. They are not mocked capability acceptance.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use source_conformance::nightly::{next_day, parse_date, parse_registry, streak_of};
use source_conformance::{run_paths, sha256_hex, Report, Request};

struct Scenario {
    name: &'static str,
    package: bool,
    nightly: bool,
    ok: bool,
    errors: &'static [&'static str],
}

const SCENARIOS: &[Scenario] = &[
    Scenario { name: "ok-verified", package: true, nightly: true, ok: true, errors: &[] },
    Scenario { name: "tamper", package: true, nightly: true, ok: false, errors: &["claim.missing_test", "evidence.hash_mismatch"] },
    Scenario { name: "missing-test", package: true, nightly: true, ok: false, errors: &["claim.missing_test"] },
    Scenario { name: "mismatched-target", package: true, nightly: false, ok: false, errors: &["evidence.target_mismatch"] },
    Scenario { name: "silent-fallback", package: true, nightly: true, ok: false, errors: &["claim.silent_fallback"] },
    Scenario { name: "cloud-container", package: true, nightly: false, ok: false, errors: &["cloud.container_scope"] },
    Scenario { name: "version-range", package: true, nightly: true, ok: false, errors: &["version.not_exact"] },
    Scenario { name: "verified-partial", package: true, nightly: true, ok: false, errors: &["version.untested"] },
    Scenario { name: "budget-cycle", package: true, nightly: true, ok: false, errors: &["cloud.cycle_unverified"] },
    Scenario { name: "manual-verified", package: false, nightly: false, ok: false, errors: &["claim.manual_assertion", "claim.missing_test"] },
    Scenario { name: "downgrade", package: true, nightly: true, ok: false, errors: &["nightly.failure_downgrade", "nightly.streak_short"] },
    Scenario { name: "streak-13", package: true, nightly: true, ok: false, errors: &["nightly.streak_short"] },
    Scenario { name: "certified-incomplete", package: true, nightly: true, ok: false, errors: &["certified.fault_drill_missing", "certified.incomplete"] },
    Scenario { name: "experimental-range", package: true, nightly: false, ok: true, errors: &[] },
    Scenario { name: "experimental-empty-version", package: true, nightly: false, ok: false, errors: &["version.matrix_missing"] },
    Scenario { name: "excluded-pass", package: true, nightly: false, ok: false, errors: &["claim.excluded_passed"] },
];

#[test]
fn synthetic_claims_match_expected_codes() {
    for scenario in SCENARIOS {
        let report = load_scenario(scenario.name, scenario.package, scenario.nightly);
        assert_eq!(report.ok, scenario.ok, "{}", scenario.name);
        assert_eq!(report.exit_code, if scenario.ok { 0 } else { 1 }, "{}", scenario.name);
        assert_closed(&report);
        assert_eq!(error_codes(&report), set(scenario.errors), "{}", scenario.name);
    }
}

#[test]
fn downgrade_recommends_experimental_without_rewriting() {
    let report = load_scenario("downgrade", true, true);
    let cell = cell(&report, "fixture-pg", "batch_full");
    assert_eq!(cell["matrix_level"], "verified");
    assert_eq!(cell["recommended_level"], "experimental");
    assert_eq!(cell["downgrade_recommended"].as_bool(), Some(true));
    assert_eq!(cell["status"], "failed");
    let row = nightly(&report, "fixture-pg", "batch_full");
    assert_eq!(row["consecutive_failures"], 2);
    assert_eq!(row["downgrade_recommended"].as_bool(), Some(true));
    assert_eq!(row["matrix_rewritten"].as_bool(), Some(false));
}

#[test]
fn short_streak_stays_verified_and_budget_does_not_count_as_manual() {
    let short = load_scenario("streak-13", true, true);
    let short_cell = cell(&short, "fixture-pg", "batch_full");
    assert_eq!(short_cell["recommended_level"], "verified");
    assert_eq!(short_cell["downgrade_recommended"].as_bool(), Some(false));
    assert_eq!(nightly(&short, "fixture-pg", "batch_full")["trailing_consecutive_success_days"], 13);

    let budget = load_scenario("budget-cycle", true, true);
    let budget_cell = cell(&budget, "fixture-pg", "batch_full");
    assert_eq!(budget_cell["recommended_level"], "experimental");
    assert_eq!(budget_cell["downgrade_recommended"].as_bool(), Some(false));
    assert!(!error_codes(&budget).contains("claim.manual_assertion"));

    let manual = load_scenario("manual-verified", false, false);
    assert!(error_codes(&manual).contains("claim.manual_assertion"));
    assert!(!error_codes(&manual).contains("nightly.failure_downgrade"));
    assert!(!error_codes(&manual).contains("nightly.streak_short"));
    assert_eq!(cell(&manual, "fixture-pg", "batch_full")["recommended_level"], "verified");
}

#[test]
fn fourteen_day_verified_package_passes_without_promotion_flag() {
    let report = load_scenario("ok-verified", true, true);
    let passed = cell(&report, "fixture-pg", "batch_full");
    assert_eq!(passed["status"], "passed");
    assert_eq!(passed["required_tests"], serde_json::json!(["CC-01", "CC-02", "CC-03", "CC-04", "CC-11"]));
    assert_eq!(passed["recommended_level"], "verified");
    assert_eq!(cell(&report, "fixture-pg", "cdc")["status"], "not_required");
    assert_eq!(cell(&report, "fixture-pg", "cdc")["required_tests"], serde_json::json!([]));
    assert_eq!(nightly(&report, "fixture-pg", "batch_full")["trailing_consecutive_success_days"], 14);
    assert_eq!(report.counts.get("verified").copied(), Some(1));
}

#[test]
fn current_matrix_stays_unpromoted() {
    let report = run_paths(&Request {
        matrix_path: real_matrix(),
        package_dirs: Vec::new(),
        nightly_path: Some(fixture("real-nightly.json")),
        historical_pg: Some(real_historical()),
    });
    assert!(report.ok);
    assert_eq!(report.exit_code, 0);
    assert_closed(&report);
    assert!(error_codes(&report).is_empty());
    for level in ["experimental", "verified", "certified"] {
        assert_eq!(report.counts.get(level).copied(), Some(0), "{level}");
    }
    assert!(report.counts.values().sum::<u32>() > 0);
    for cell in &report.cells {
        assert!(matches!(cell.matrix_level.as_str(), "candidate" | "designed" | "excluded"));
        assert_eq!(cell.recommended_level, cell.matrix_level);
        assert!(!cell.downgrade_recommended);
        assert!(cell.required_tests.is_empty());
        assert_eq!(cell.status, "not_required");
    }
    let full = nightly(&report, "postgres-selfmanaged", "batch_full");
    assert_eq!(full["missed_days"], serde_json::json!(["2026-10-04"]));
    assert_eq!(full["trailing_consecutive_success_days"], 1);
    assert_eq!(full["longest_consecutive_success_days"], 3);
    assert_eq!(full["recommended_level"], "designed");
    assert_eq!(full["downgrade_recommended"].as_bool(), Some(false));

    let catalog = nightly(&report, "postgres-selfmanaged", "catalog");
    assert_eq!(catalog["trailing_consecutive_success_days"], 14);
    assert_eq!(catalog["missed_days"], serde_json::json!([]));
    assert_eq!(catalog["manual_runs"], 1);
    assert_eq!(catalog["suite_runs"], 14);
    assert_eq!(catalog["recommended_level"], "designed");
    assert_eq!(catalog["matrix_level"], "designed");
    assert!(report.findings.iter().any(|finding| {
        finding.severity == "info" && finding.code == "nightly.not_promoted" && finding.capability == "catalog"
    }));

    let mysql = nightly(&report, "mysql8-selfmanaged", "cdc");
    assert_eq!(mysql["consecutive_failures"], 2);
    assert_eq!(mysql["downgrade_recommended"].as_bool(), Some(false));
    assert_eq!(mysql["recommended_level"], "designed");
    assert_eq!(mysql["matrix_rewritten"].as_bool(), Some(false));

    let historical = report.historical_pg.as_ref().expect("historical record");
    assert_eq!(historical.classification, "historical_draft_preview_not_conformance_suite");
    assert_eq!(historical.protocol, "oo.pipeline.local-pg-mirror-manifest/v1");
    assert!(!historical.satisfies_cc_suite);
    assert!(!historical.promotes_grade);
    assert_eq!(historical.rows, 100_000);
    assert_eq!(historical.source_write_tuples, 0);
    assert!(historical.source_read_only);
    assert_eq!(historical.import_hash, "b7b86b3cb34d7d6e4fad7694194bbc9bf5bd6dc0a84277802409d644eaf441f8");
    assert_eq!(historical.manifest_sha256, "f32a98fc478f0015345c5f2d52c0deb0cbcb51d7027b3c845f49d5b1c77dbf66");
    assert_eq!(historical.source_commit, "3b70b151552ad5d5e905a6148860812a3f84ac82");
    assert_eq!(historical.implementation_commit, "fb0ea042f76354c62b6837b85d88cb6a5e973c3e");
    assert_eq!(historical.exits.get("01").map(String::as_str), Some("0\n"));
    assert_eq!(historical.exits.get("06").map(String::as_str), Some("0\n"));
    let report_bytes = fs::read(real_historical().join("REPORT.md")).unwrap();
    assert_eq!(historical.report_sha256, sha256_hex(&report_bytes));
    let rendered = serde_json::to_string(&report).unwrap();
    assert!(!rendered.contains("physical_name"));
    assert!(!rendered.contains("hub_id"));
    assert_eq!(report.source_support_consumer.status, "pending");
    assert!(report.source_support_consumer.protocol_commit.contains("577160c22de63adb9b38e0147f3bc2da893c4d9d"));
    assert_eq!(report.source_support_consumer.mutable_workspace, "not-read");
    assert!(report.source_support_consumer.proposed_api.contains("parse_support_matrix"));
    assert!(report.source_support_consumer.proposed_api.contains("matrix::load"));
    assert!(report.source_support_consumer.protocol_commit.contains("parse_support_matrix-absent"));
    assert!(report.source_support_consumer.protocol_commit.contains("29597079f7feacc6e3934b5c803686fc80567a93"));
    let matrix_bytes = fs::read(real_matrix()).unwrap();
    assert_eq!(report.matrix_sha256, sha256_hex(&matrix_bytes));
    assert_eq!(report.case_map.len(), 13);
    assert_eq!(report.case_map[0].id, "CC-01");
    assert_eq!(report.case_map[12].id, "CC-13");
}

#[test]
fn leap_days_and_rejected_calendar_dates() {
    let leap = streak_days(&["2024-02-28", "2024-02-29", "2024-03-01"]);
    assert_eq!(leap.trailing_success_days, 3);
    assert!(leap.missed_days.is_empty());
    let non_leap = streak_days(&["2025-02-28", "2025-03-01"]);
    assert_eq!(non_leap.trailing_success_days, 2);
    assert!(non_leap.missed_days.is_empty());
    let err = parse_registry(&registry_json(&["2025-02-29"])).unwrap_err();
    assert!(err.contains("calendar_day"), "{err}");
    assert_eq!(next_day(parse_date("2024-02-28").unwrap()).text(), "2024-02-29");
    assert_eq!(next_day(parse_date("2025-02-28").unwrap()).text(), "2025-03-01");
}

#[test]
fn nightly_span_over_400_days_is_rejected() {
    let mut far = parse_date("2020-01-01").unwrap();
    for _ in 0..400 {
        far = next_day(far);
    }
    let err = streak_of(&parse_registry(&registry_json(&["2020-01-01", &far.text()])).unwrap().entries[0]).unwrap_err();
    assert!(err.starts_with("nightly.span_too_long"), "{err}");
    let mut edge = parse_date("2020-01-01").unwrap();
    for _ in 0..399 {
        edge = next_day(edge);
    }
    let streak = streak_of(&parse_registry(&registry_json(&["2020-01-01", &edge.text()])).unwrap().entries[0]).unwrap();
    assert_eq!(streak.suite_runs, 2);
}

#[test]
fn historical_negatives_and_symlink_matrix() {
    let root = std::env::temp_dir().join("oo-dl01w1-07-suite-tests");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let missing = root.join("missing-report");
    fs::create_dir_all(&missing).unwrap();
    let missing_report = run_paths(&Request {
        matrix_path: fixture("ok-verified").join("matrix.yaml"),
        package_dirs: vec![fixture("ok-verified").join("package")],
        nightly_path: Some(fixture("ok-verified").join("nightly.json")),
        historical_pg: Some(missing),
    });
    assert!(error_codes(&missing_report).contains("historical.unreadable"));

    let wrong = root.join("wrong-protocol");
    fs::create_dir_all(wrong.join("six-final")).unwrap();
    fs::write(wrong.join("REPORT.md"), "Source commit: 3b70b151552ad5d5e905a6148860812a3f84ac82; implementation commit: fb0ea042f76354c62b6837b85d88cb6a5e973c3e\n").unwrap();
    fs::write(wrong.join("HANDOFF.md"), "handoff\n").unwrap();
    fs::write(wrong.join("six-final/manifest.json"), "{\"protocol\":\"other\"}\n").unwrap();
    for name in ["01", "02", "03", "04", "05", "06"] {
        fs::write(wrong.join("six-final").join(format!("{name}.exit")), "0\n").unwrap();
    }
    let wrong_report = run_paths(&Request {
        matrix_path: fixture("manual-verified").join("matrix.yaml"),
        package_dirs: Vec::new(),
        nightly_path: None,
        historical_pg: Some(wrong),
    });
    assert!(error_codes(&wrong_report).contains("historical.unexpected_manifest"));

    let link = root.join("matrix-link.yaml");
    std::os::unix::fs::symlink(fixture("manual-verified").join("matrix.yaml"), &link).unwrap();
    let linked = run_paths(&Request {
        matrix_path: link,
        package_dirs: Vec::new(),
        nightly_path: None,
        historical_pg: None,
    });
    assert!(error_codes(&linked).contains("path.symlink"));
}

#[test]
fn cli_map_usage_and_decisive_exits() {
    let bin = env!("CARGO_BIN_EXE_source-conformance");
    let usage = Command::new(bin).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    assert!(String::from_utf8_lossy(&usage.stderr).contains("usage:"));

    let map = Command::new(bin).arg("map").output().unwrap();
    assert_eq!(map.status.code(), Some(0));
    let map_json: Value = serde_json::from_slice(&map.stdout).unwrap();
    assert_eq!(map_json["schema"], "oo-source-conformance/case-map/v1");
    assert_eq!(map_json["cases"][12]["id"], "CC-13");
    assert!(map_json["certified_cases"].as_array().unwrap().iter().any(|item| item == "SCALE-5M"));

    let positive = Command::new(bin)
        .args([
            "check",
            "--matrix",
            real_matrix().to_str().unwrap(),
            "--historical-pg",
            real_historical().to_str().unwrap(),
            "--nightly",
            fixture("real-nightly.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(positive.status.code(), Some(0), "{}", String::from_utf8_lossy(&positive.stderr));
    let positive_json: Value = serde_json::from_slice(&positive.stdout).unwrap();
    assert_eq!(positive_json["promoted"].as_bool(), Some(false));
    assert_eq!(positive_json["counts"]["verified"], 0);

    let tamper = command_fixture(bin, "tamper", true, true);
    assert_eq!(tamper.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&tamper.stdout).contains("evidence.hash_mismatch"));

    let missing = command_fixture(bin, "missing-test", true, true);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stdout).contains("claim.missing_test"));

    let mismatch = command_fixture(bin, "mismatched-target", true, false);
    assert_eq!(mismatch.status.code(), Some(1));
    let mismatch_json: Value = serde_json::from_slice(&mismatch.stdout).unwrap();
    let codes: Vec<_> = mismatch_json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["severity"] == "error")
        .map(|finding| finding["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, vec!["evidence.target_mismatch"]);
}

#[test]
fn identity_drift_denies_the_other_release_and_a_match_stays_experimental() {
    let drift = load_scenario("repair-identity-drift", true, false);
    assert!(!drift.ok);
    assert_eq!(drift.exit_code, 1);
    assert_closed(&drift);
    assert_eq!(error_codes(&drift), set(&["claim.missing_test", "version.identity_mismatch"]));
    let finding = drift.findings.iter().find(|item| item.code == "version.identity_mismatch").unwrap();
    assert!(finding.detail.contains("service_version 15.2"));
    assert!(finding.detail.contains("canonical 15.2"));
    assert!(finding.detail.contains("exact_version 16.9"));
    let drift_cell = cell(&drift, "rev-w27-drift", "batch_full");
    assert_eq!(drift_cell["status"], "failed");
    assert_eq!(drift_cell["matrix_level"], "experimental");
    assert_eq!(drift_cell["recommended_level"], "experimental");
    assert_eq!(drift.counts.get("verified").copied(), Some(0));
    assert_eq!(drift.counts.get("experimental").copied(), Some(1));
    assert!(!error_codes(&drift).contains("claim.manual_assertion"));

    let nights = load_scenario("repair-identity-drift", true, true);
    assert_eq!(error_codes(&nights), set(&["claim.missing_test", "version.identity_mismatch"]));
    let row = nightly(&nights, "rev-w27-drift", "batch_full");
    assert_eq!(row["trailing_consecutive_success_days"], 0);
    assert_eq!(row["suite_runs"], 0);
    assert_eq!(row["matrix_rewritten"].as_bool(), Some(false));

    let matched = load_scenario("repair-identity-match", true, false);
    assert!(matched.ok);
    assert_eq!(matched.exit_code, 0);
    assert_closed(&matched);
    assert!(error_codes(&matched).is_empty());
    assert_eq!(cell(&matched, "rev-w27-drift", "batch_full")["status"], "passed");
    assert_eq!(matched.counts.get("verified").copied(), Some(0));
    assert_eq!(matched.counts.get("certified").copied(), Some(0));

    let suffix = load_scenario("repair-identity-suffix-same", true, false);
    assert!(suffix.ok, "{:?}", error_codes(&suffix));
    assert_eq!(cell(&suffix, "rev-w27-drift", "batch_full")["status"], "passed");

    let suffix_other = load_scenario("repair-identity-suffix-mismatch", true, false);
    assert_eq!(error_codes(&suffix_other), set(&["claim.missing_test", "version.identity_mismatch"]));
    let major = load_scenario("repair-identity-major", true, false);
    assert_eq!(error_codes(&major), set(&["claim.missing_test", "version.identity_mismatch"]));
    let major_detail = major.findings.iter().find(|item| item.code == "version.identity_mismatch").unwrap();
    assert!(major_detail.detail.contains("canonical 16"));
    assert!(major_detail.detail.contains("exact_version 16.9"));

    let unknown = load_scenario("repair-identity-unknown", true, false);
    assert_eq!(error_codes(&unknown), set(&["claim.missing_test", "version.identity_unknown"]));
    assert!(!error_codes(&unknown).contains("claim.manual_assertion"));

    let missing = load_scenario("repair-identity-missing", true, false);
    assert_eq!(
        error_codes(&missing),
        set(&["claim.manual_assertion", "claim.missing_test", "version.identity_missing"])
    );
    let malformed = load_scenario("repair-identity-malformed", true, false);
    assert_eq!(
        error_codes(&malformed),
        set(&["claim.manual_assertion", "claim.missing_test", "version.identity_malformed"])
    );

    let compat = load_scenario("repair-identity-compat", true, false);
    assert_eq!(error_codes(&compat), set(&["claim.missing_test", "version.identity_unknown"]));
    let compat_detail = compat.findings.iter().find(|item| item.code == "version.identity_unknown").unwrap();
    assert!(compat_detail.detail.contains("mysql-compat has no product-version normalizer"));
    assert_eq!(cell(&compat, "repair-compat", "batch_full")["status"], "failed");

    let mysql_suffix = load_scenario("repair-identity-mysql-suffix", true, false);
    assert!(mysql_suffix.ok, "{:?}", error_codes(&mysql_suffix));
    assert_eq!(cell(&mysql_suffix, "repair-mysql", "batch_full")["status"], "passed");
    let mysql_prefix = load_scenario("repair-identity-mysql-prefix", true, false);
    assert_eq!(error_codes(&mysql_prefix), set(&["claim.missing_test", "version.identity_mismatch"]));
    let prefix_detail = mysql_prefix.findings.iter().find(|item| item.code == "version.identity_mismatch").unwrap();
    assert!(prefix_detail.detail.contains("canonical 8.0.32"));
    assert!(prefix_detail.detail.contains("exact_version 8.0"));
}

#[test]
fn duplicate_identity_keys_deny_before_claim_or_nightly_credit() {
    for (name, key) in [
        ("dup-service-last-match", "service_version"),
        ("dup-service-last-drift", "service_version"),
        ("dup-service-same", "service_version"),
        ("dup-service-escaped", "service_version"),
        ("dup-exact-last-match", "exact_version"),
        ("dup-exact-last-drift", "exact_version"),
        ("dup-tested-same", "tested_versions"),
    ] {
        let report = load_scenario(name, true, false);
        assert_eq!(report.exit_code, 1, "{name}");
        assert_closed(&report);
        assert_eq!(error_codes(&report), set(&["claim.missing_test", "version.identity_duplicate"]), "{name}");
        assert!(!error_codes(&report).contains("claim.manual_assertion"), "{name}");
        assert!(!error_codes(&report).contains("version.identity_mismatch"), "{name}");
        let finding = report.findings.iter().find(|item| item.code == "version.identity_duplicate").unwrap();
        assert_eq!(finding.detail, key, "{name}");
        assert!(report.cells.iter().any(|cell| cell.status == "failed"), "{name}");
        assert_eq!(report.counts.get("verified").copied(), Some(0), "{name}");
    }

    let nights = load_scenario("dup-service-last-match", true, true);
    assert_eq!(error_codes(&nights), set(&["claim.missing_test", "version.identity_duplicate"]));
    let row = nightly(&nights, "rev-w37-dup-match", "batch_full");
    assert_eq!(row["trailing_consecutive_success_days"], 0);
    assert_eq!(row["suite_runs"], 0);
    assert_eq!(row["recommended_level"], "experimental");
    assert_eq!(row["matrix_rewritten"].as_bool(), Some(false));
    assert!(!error_codes(&nights).contains("nightly.failure_downgrade"));

    let missing = load_scenario("dup-missing-valid", true, false);
    assert_eq!(
        error_codes(&missing),
        set(&["claim.manual_assertion", "claim.missing_test", "version.identity_missing"])
    );
    let comma = load_scenario("dup-missing-comma", true, false);
    assert_eq!(
        error_codes(&comma),
        set(&["claim.manual_assertion", "claim.missing_test", "evidence.malformed"])
    );
    assert!(!error_codes(&comma).contains("version.identity_missing"));
}

#[test]
fn unloadable_packages_receive_no_nightly_suite_credit() {
    let denied = [
        (
            "credit-missing-valid",
            "rev-w37-missing-valid",
            &["claim.manual_assertion", "claim.missing_test", "version.identity_missing"][..],
        ),
        (
            "credit-malformed",
            "rev-w37-malformed",
            &["claim.manual_assertion", "claim.missing_test", "version.identity_malformed"][..],
        ),
        (
            "credit-missing-json",
            "rev-w37-missing",
            &["claim.manual_assertion", "claim.missing_test", "evidence.malformed"][..],
        ),
    ];
    for (name, target, codes) in denied {
        let report = load_scenario(name, true, true);
        assert_eq!(report.exit_code, 1, "{name}");
        assert_closed(&report);
        assert_eq!(error_codes(&report), set(codes), "{name}");
        if name == "credit-missing-json" {
            assert!(!error_codes(&report).contains("version.identity_missing"));
        }
        let row = nightly(&report, target, "batch_full");
        assert_eq!(row["trailing_consecutive_success_days"], 0, "{name}");
        assert_eq!(row["longest_consecutive_success_days"], 0, "{name}");
        assert_eq!(row["suite_runs"], 0, "{name}");
        assert_eq!(row["manual_runs"], 0, "{name}");
        assert_eq!(row["consecutive_failures"], 0, "{name}");
        assert_eq!(row["downgrade_recommended"].as_bool(), Some(false), "{name}");
        assert_eq!(row["matrix_rewritten"].as_bool(), Some(false), "{name}");
        assert_eq!(row["recommended_level"], "experimental", "{name}");
        assert!(!report.promoted);
        assert_eq!(report.counts.get("verified").copied(), Some(0), "{name}");
        assert!(report.cells.iter().any(|cell| cell.target_id == target && cell.status == "failed"), "{name}");
        assert!(!error_codes(&report).contains("nightly.failure_downgrade"), "{name}");
        assert!(!error_codes(&report).contains("nightly.not_promoted"), "{name}");
    }

    let matched = load_scenario("credit-matched", true, true);
    assert!(matched.ok, "{:?}", error_codes(&matched));
    assert_eq!(matched.exit_code, 0);
    let matched_row = nightly(&matched, "rev-w27-drift", "batch_full");
    assert_eq!(matched_row["trailing_consecutive_success_days"], 3);
    assert_eq!(matched_row["suite_runs"], 3);
    assert_eq!(matched_row["matrix_rewritten"].as_bool(), Some(false));
    assert!(!matched.promoted);
    assert_eq!(matched.counts.get("verified").copied(), Some(0));
    assert_eq!(cell(&matched, "rev-w27-drift", "batch_full")["status"], "passed");

    let bad_hash = load_scenario("credit-bad-hash", true, true);
    assert_eq!(error_codes(&bad_hash), set(&["claim.missing_test", "evidence.hash_mismatch"]));
    let bad_row = nightly(&bad_hash, "rev-w27-drift", "batch_full");
    assert_eq!(bad_row["trailing_consecutive_success_days"], 0);
    assert_eq!(bad_row["suite_runs"], 0);
    assert_eq!(bad_row["downgrade_recommended"].as_bool(), Some(false));
    assert_eq!(bad_row["matrix_rewritten"].as_bool(), Some(false));
    assert!(!bad_hash.promoted);
    assert!(!error_codes(&bad_hash).contains("nightly.failure_downgrade"));
}

fn load_scenario(name: &str, package: bool, nightly_file: bool) -> Report {
    let dir = fixture(name);
    run_paths(&Request {
        matrix_path: dir.join("matrix.yaml"),
        package_dirs: if package { vec![dir.join("package")] } else { Vec::new() },
        nightly_path: nightly_file.then(|| dir.join("nightly.json")),
        historical_pg: None,
    })
}

fn command_fixture(bin: &str, name: &str, package: bool, nightly_file: bool) -> std::process::Output {
    let dir = fixture(name);
    let mut args = vec!["check".to_owned(), "--matrix".to_owned(), dir.join("matrix.yaml").display().to_string()];
    if package {
        args.push("--package".to_owned());
        args.push(dir.join("package").display().to_string());
    }
    if nightly_file {
        args.push("--nightly".to_owned());
        args.push(dir.join("nightly.json").display().to_string());
    }
    Command::new(bin).args(args).output().unwrap()
}

fn error_codes(report: &Report) -> BTreeSet<String> {
    report.findings.iter().filter(|finding| finding.severity == "error").map(|finding| finding.code.clone()).collect()
}

fn set(codes: &[&str]) -> BTreeSet<String> {
    codes.iter().map(|code| (*code).to_owned()).collect()
}

fn assert_closed(report: &Report) {
    assert!(!report.promoted);
    assert!(!report.matrix_rewritten);
    assert_eq!(report.source_support_consumer.status, "pending");
}

fn cell(report: &Report, target: &str, capability: &str) -> Value {
    let cell = report.cells.iter().find(|cell| cell.target_id == target && cell.capability == capability).unwrap();
    serde_json::to_value(cell).unwrap()
}

fn nightly(report: &Report, target: &str, capability: &str) -> Value {
    let row = report.nightly.iter().find(|row| row.target_id == target && row.capability == capability).unwrap();
    serde_json::to_value(row).unwrap()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

fn real_matrix() -> PathBuf {
    if let Ok(path) = std::env::var("OO_SUPPORT_MATRIX")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    let fallback = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/database-support/support-matrix.yaml");
    if fallback.is_file() {
        fallback
    } else {
        panic!("OO_SUPPORT_MATRIX is unset and this archive has no support-matrix.yaml");
    }
}

fn real_historical() -> PathBuf {
    if let Ok(path) = std::env::var("OO_HISTORICAL_PG")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    let fallback = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence/gpt-minimal-closure");
    if fallback.join("REPORT.md").is_file() {
        fallback
    } else {
        panic!("OO_HISTORICAL_PG is unset and this archive has no historical REPORT.md");
    }
}

fn streak_days(days: &[&str]) -> source_conformance::nightly::Streak {
    let registry = parse_registry(&registry_json(days)).unwrap();
    streak_of(&registry.entries[0]).unwrap()
}

fn registry_json(days: &[&str]) -> String {
    let runs: Vec<Value> = days
        .iter()
        .map(|day| {
            serde_json::json!({
                "calendar_day": day,
                "raw_exit": 0,
                "evidence_id": "calendar-day-unit",
                "origin": "suite"
            })
        })
        .collect();
    serde_json::json!({
        "schema": "oo-source-conformance/nightly-registry/v1",
        "entries": [{ "target_id": "fixture-pg", "capability": "batch_full", "runs": runs }]
    })
    .to_string()
}
