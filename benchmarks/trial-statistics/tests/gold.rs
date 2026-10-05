use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use oo_trial_statistics::run;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read(path).unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn execute(stdin_bytes: &[u8]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        std::iter::empty(),
        Cursor::new(stdin_bytes),
        &mut stdout,
        &mut stderr,
    );
    (code, stdout, stderr)
}

fn assert_report(input: &str, report: &str, code: i32) {
    let (first, stdout, stderr) = execute(&fixture(input));
    assert_eq!(first, code);
    assert!(stderr.is_empty());
    assert_eq!(stdout, fixture(report));
    let (second, again, stderr) = execute(&fixture(input));
    assert_eq!(second, code);
    assert!(stderr.is_empty());
    assert_eq!(again, stdout);
}

#[test]
fn handwritten_k3_passed_gate() {
    assert_report("k3-passed.json", "k3-passed.report.json", 0);
}

#[test]
fn handwritten_k3_reannotation_stays_in_the_denominator() {
    assert_report("k3-reannotation.json", "k3-reannotation.report.json", 1);
    let report = fixture("k3-reannotation.report.json");
    let text = str::from_utf8(&report).expect("utf-8");
    assert!(text.contains("\"question_count\":3"));
    assert!(text.contains("\"reannotation_count\":1"));
    assert!(text.contains("\"pass_k_count\":0"));
    assert!(text.contains("\"gate\":\"failed\""));
}

#[test]
fn handwritten_k3_is_not_a_pass_at_average() {
    assert_report("k3-not-pass-at.json", "k3-not-pass-at.report.json", 0);
    let report = fixture("k3-not-pass-at.report.json");
    let text = str::from_utf8(&report).expect("utf-8");
    assert!(text.contains("\"point_difference\":{\"denominator\":1,\"numerator\":-1}"));
    assert!(!text.contains("\"numerator\":-1,\"denominator\":3"));
}

#[test]
fn both_variants_needing_reannotation_count_once() {
    let input = r#"{"algorithm":"oo.benchmark.paired-question-bootstrap/v1","resamples":1,"seed":"0","variants":[{"id":"left","questions":[{"id":"q0","trials":[{"index":0,"status":"needs_reannotation"}]}],"tuple":{"as_of":"2026-10-01","budget":"tokens-8000","dataset":"public-slice","harness":"harness-0","model":"model-fixed","project":"logistics","role":"analyst","scorer_version":"scorer-1","snapshot":"snap-2026-10-01","suite":"paired-k3"}},{"id":"right","questions":[{"id":"q0","trials":[{"index":0,"status":"needs_reannotation"}]}],"tuple":{"as_of":"2026-10-01","budget":"tokens-8000","dataset":"public-slice","harness":"harness-0","model":"model-fixed","project":"logistics","role":"analyst","scorer_version":"scorer-1","snapshot":"snap-2026-10-01","suite":"paired-k3"}}],"version":"oo.benchmark.trial-statistics/v1"}"#;
    let expect = "{\"algorithm\":\"oo.benchmark.paired-question-bootstrap/v1\",\"gate\":\"failed\",\"high_rank\":0,\"low_rank\":0,\"percentile_high\":{\"denominator\":1,\"numerator\":0},\"percentile_low\":{\"denominator\":1,\"numerator\":0},\"point_difference\":{\"denominator\":1,\"numerator\":0},\"question_count\":1,\"reannotation_count\":1,\"resamples\":1,\"seed\":\"0\",\"trial_count\":1,\"variants\":[{\"id\":\"left\",\"pass_k\":{\"denominator\":1,\"numerator\":0},\"pass_k_count\":0},{\"id\":\"right\",\"pass_k\":{\"denominator\":1,\"numerator\":0},\"pass_k_count\":0}],\"version\":\"oo.benchmark.trial-statistics-report/v1\"}\n";
    let (code, stdout, stderr) = execute(input.as_bytes());
    assert_eq!(code, 1);
    assert!(stderr.is_empty());
    assert_eq!(stdout, expect.as_bytes());
}
