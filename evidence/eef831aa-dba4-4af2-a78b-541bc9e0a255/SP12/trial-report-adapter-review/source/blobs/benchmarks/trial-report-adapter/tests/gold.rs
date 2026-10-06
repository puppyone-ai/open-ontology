use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read_to_string(path).unwrap_or_else(|error| panic!("read {name}: {error}"))
}

#[test]
fn k2_keeps_reannotation_and_maps_the_other_misses() {
    let input = fixture("k2-mixed.json");
    let output = oo_trial_report_adapter::adapt(&input).expect("adapt");
    let mut with_newline = output.clone();
    with_newline.push('\n');
    assert_eq!(with_newline, fixture("k2-mixed.stdout"));
    assert!(output.contains("\"status\":\"needs_reannotation\""));
    assert!(output.contains("\"snapshot\":\"declared-label\""));
    assert!(output.contains("\"model\":\"trusted_sql\""));
    assert!(!output.contains("\"model\":\"none\""));
    assert!(!output.contains("certified"));
}

#[test]
fn allfail_writes_only_fail_and_stays_a_statistics_input() {
    let input = fixture("allfail.json");
    let output = oo_trial_report_adapter::adapt(&input).expect("adapt");
    let mut with_newline = output.clone();
    with_newline.push('\n');
    assert_eq!(with_newline, fixture("allfail.stdout"));
    assert!(output.contains("\"version\":\"oo.benchmark.trial-statistics/v1\""));
    assert!(!output.contains("pass_k"));
    assert!(!output.contains("percentile"));
}
