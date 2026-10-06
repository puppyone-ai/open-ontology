#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Stdio};

use business_calendar::{
    Closure, Input, IntervalKind, Midnight, PhraseReport, Report, CALENDAR_YAML_MAX_BYTES, STDIN_MAX_BYTES,
    resolve, sha256_hex,
};
use serde_json::{Value, json};

const CALLER_HASH: &str = "cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebabe";
const HONOLULU: &str = "schema_version: oos/v1\nbusiness_tz: Pacific/Honolulu\nfiscal_year_start_month: 4\nweek_start: wednesday\nanchors:\n  - today\n  - fiscal_year_start\n";
const HONOLULU_HASH: &str = "0f47e4d306986fc5a77318f01eb204a3874f3e86e65c1f63cca3e9b3b32839e1";

fn calendar(tz: &str, fiscal: u8, week: &str, anchors: &[&str], phrases: &str) -> String {
    let mut out = format!(
        "schema_version: oos/v1\nbusiness_tz: {tz}\nfiscal_year_start_month: {fiscal}\nweek_start: {week}\n"
    );
    if anchors.is_empty() {
        out.push_str("anchors: []\n");
    } else {
        out.push_str("anchors:\n");
        for name in anchors {
            out.push_str("  - ");
            out.push_str(name);
            out.push('\n');
        }
    }
    if !phrases.is_empty() {
        out.push_str("phrases:\n");
        out.push_str(phrases);
    }
    out
}

fn resolve_at(yaml: &str, as_of: &str, closure: Option<Closure<'_>>) -> Result<Report, business_calendar::Refusal> {
    resolve(&Input {
        calendar_yaml: yaml,
        as_of,
        closure,
    })
}

fn must(yaml: &str, as_of: &str, closure: Option<Closure<'_>>) -> Report {
    resolve_at(yaml, as_of, closure).unwrap_or_else(|err| panic!("refused {}", err.reason))
}

fn anchor<'a>(report: &'a Report, name: &str) -> &'a business_calendar::AnchorReport {
    report.anchors.iter().find(|item| item.name == name).unwrap_or_else(|| panic!("missing {name}"))
}

fn unique(midnight: &Midnight) -> &str {
    match midnight {
        Midnight::Unique { rfc3339 } => rfc3339,
        other => panic!("expected unique midnight, got {other:?}"),
    }
}

fn flags_are_closed(report: &Report) {
    assert!(!report.finance_approval);
    assert!(!report.authenticated);
    assert!(!report.database_used);
    assert!(!report.clock_used);
    assert_eq!(report.quarter_convention, "business_fiscal_quarter");
    assert_eq!(report.tzdb_version, "2025b");
}

fn label_of(phrase: &PhraseReport) -> &str {
    match phrase {
        PhraseReport::Range { label, .. } | PhraseReport::Comparison { label, .. } => label,
    }
}

fn range_end<'a>(phrase: &'a PhraseReport) -> (&'a str, &'a str, &'a str) {
    match phrase {
        PhraseReport::Range {
            start_civil,
            end_exclusive_civil,
            inclusive_end_civil,
            ..
        } => (start_civil, end_exclusive_civil, inclusive_end_civil),
        PhraseReport::Comparison { .. } => panic!("expected a range"),
    }
}

#[test]
fn fiscal_april_lists_every_core_anchor() {
    let yaml = calendar(
        "UTC",
        4,
        "saturday",
        &[
            "fiscal_year_start",
            "prev_fiscal_year_end",
            "quarter_start",
            "prev_quarter_start",
            "prev_quarter_end",
            "month_start",
            "prev_month_start",
            "prev_month_end",
            "today",
            "last_closed_period_end",
        ],
        "",
    );
    let report = must(
        &yaml,
        "2018-06-20T11:00:00Z",
        Some(Closure {
            last_closed_period_end: "2017-12-31",
            label: "close-2017-12",
            record_sha256: CALLER_HASH,
        }),
    );
    flags_are_closed(&report);
    assert_eq!(serde_json::to_value(report.week_start).unwrap(), json!("saturday"));
    assert_eq!(report.fiscal_year_start_month, 4);
    let expect = [
        ("fiscal_year_start", "2018-04-01"),
        ("prev_fiscal_year_end", "2018-03-31"),
        ("quarter_start", "2018-04-01"),
        ("prev_quarter_start", "2018-01-01"),
        ("prev_quarter_end", "2018-03-31"),
        ("month_start", "2018-06-01"),
        ("prev_month_start", "2018-05-01"),
        ("prev_month_end", "2018-05-31"),
        ("today", "2018-06-20"),
        ("last_closed_period_end", "2017-12-31"),
    ];
    assert_eq!(report.anchors.len(), expect.len());
    for (item, (name, civil)) in report.anchors.iter().zip(expect) {
        assert_eq!(item.name, name);
        assert_eq!(item.civil, civil);
        assert!(unique(&item.midnight).ends_with("T00:00:00Z"), "{}", item.name);
        assert_eq!(item.civil_day_seconds, Some(86_400));
    }
    assert_ne!(anchor(&report, "last_closed_period_end").civil, anchor(&report, "today").civil);
    assert!(report.anchors.iter().all(|item| !item.name.contains("week")));

    let only_quarter = calendar("UTC", 4, "saturday", &["quarter_start"], "");
    let quarter = must(&only_quarter, "2018-06-20T11:00:00Z", None);
    assert_eq!(quarter.anchors.len(), 1);
    assert_eq!(anchor(&quarter, "quarter_start").civil, "2018-04-01");
    assert!(quarter.anchors.iter().all(|item| item.name != "month_start"));
}

#[test]
fn fiscal_february_is_not_the_calendar_quarter() {
    let names = [
        "today",
        "quarter_start",
        "prev_quarter_start",
        "prev_quarter_end",
        "fiscal_year_start",
        "prev_fiscal_year_end",
    ];
    let yaml = calendar("UTC", 2, "wednesday", &names, "");
    let june = must(&yaml, "2018-06-20T11:00:00Z", None);
    assert_eq!(anchor(&june, "today").civil, "2018-06-20");
    assert_eq!(anchor(&june, "fiscal_year_start").civil, "2018-02-01");
    assert_eq!(anchor(&june, "prev_fiscal_year_end").civil, "2018-01-31");
    assert_eq!(anchor(&june, "quarter_start").civil, "2018-05-01");
    assert_ne!(anchor(&june, "quarter_start").civil, "2018-04-01");
    assert_eq!(anchor(&june, "prev_quarter_start").civil, "2018-02-01");
    assert_eq!(anchor(&june, "prev_quarter_end").civil, "2018-04-30");

    let january = must(&yaml, "2018-01-20T11:00:00Z", None);
    assert_eq!(anchor(&january, "today").civil, "2018-01-20");
    assert_eq!(anchor(&january, "fiscal_year_start").civil, "2017-02-01");
    assert_eq!(anchor(&january, "prev_fiscal_year_end").civil, "2017-01-31");
    assert_eq!(anchor(&january, "quarter_start").civil, "2017-11-01");
    assert_eq!(anchor(&january, "prev_quarter_start").civil, "2017-08-01");
    assert_eq!(anchor(&january, "prev_quarter_end").civil, "2017-10-31");
}

#[test]
fn leap_year_boundary_and_month_to_date_clamp() {
    let ends = calendar("UTC", 1, "monday", &["prev_month_end", "prev_month_start", "today"], "");
    assert_eq!(anchor(&must(&ends, "2016-03-01T08:00:00Z", None), "prev_month_end").civil, "2016-02-29");
    assert_eq!(anchor(&must(&ends, "2017-03-01T08:00:00Z", None), "prev_month_end").civil, "2017-02-28");
    let year = must(&ends, "2015-01-01T08:00:00Z", None);
    assert_eq!(anchor(&year, "today").civil, "2015-01-01");
    assert_eq!(anchor(&year, "prev_month_start").civil, "2014-12-01");
    assert_eq!(anchor(&year, "prev_month_end").civil, "2014-12-31");

    let leap_day = calendar(
        "UTC",
        4,
        "monday",
        &["today", "fiscal_year_start", "prev_fiscal_year_end", "quarter_start"],
        "",
    );
    let leap = must(&leap_day, "2016-02-29T08:00:00Z", None);
    assert_eq!(anchor(&leap, "today").civil, "2016-02-29");
    assert_eq!(anchor(&leap, "fiscal_year_start").civil, "2015-04-01");
    assert_eq!(anchor(&leap, "prev_fiscal_year_end").civil, "2015-03-31");
    assert_eq!(anchor(&leap, "quarter_start").civil, "2016-01-01");

    let phrases = "  mtd:\n    current: { start: month_start, end: \"today + 1\", bounds: \"[)\" }\n    previous:\n      start: prev_month_start\n      end: \"min(month_start, prev_month_start + (today - month_start) + 1)\"\n      bounds: \"[)\"\n";
    let yaml = calendar(
        "UTC",
        1,
        "monday",
        &["today", "month_start", "prev_month_start"],
        phrases,
    );
    let clamped = must(&yaml, "2018-03-31T09:00:00Z", None);
    match clamped.phrases.first().unwrap() {
        PhraseReport::Comparison { current, previous, .. } => {
            assert_eq!(current.start_civil, "2018-03-01");
            assert_eq!(current.end_exclusive_civil, "2018-04-01");
            assert_eq!(current.inclusive_end_civil, "2018-03-31");
            assert_eq!(previous.start_civil, "2018-02-01");
            assert_eq!(previous.end_exclusive_civil, "2018-03-01");
            assert_eq!(previous.inclusive_end_civil, "2018-02-28");
        }
        PhraseReport::Range { .. } => panic!("comparison"),
    }
    let leap_clamp = must(&yaml, "2016-03-31T09:00:00Z", None);
    match leap_clamp.phrases.first().unwrap() {
        PhraseReport::Comparison { previous, .. } => {
            assert_eq!(previous.end_exclusive_civil, "2016-03-01");
            assert_eq!(previous.inclusive_end_civil, "2016-02-29");
        }
        PhraseReport::Range { .. } => panic!("comparison"),
    }
    let open = must(&yaml, "2016-03-10T09:00:00Z", None);
    match open.phrases.first().unwrap() {
        PhraseReport::Comparison { current, previous, .. } => {
            assert_eq!(current.end_exclusive_civil, "2016-03-11");
            assert_eq!(previous.start_civil, "2016-02-01");
            assert_eq!(previous.end_exclusive_civil, "2016-02-11");
            assert_eq!(previous.inclusive_end_civil, "2016-02-10");
        }
        PhraseReport::Range { .. } => panic!("comparison"),
    }
}

#[test]
fn explicit_as_of_crosses_tokyo_and_los_angeles_midnight() {
    let tokyo = calendar("Asia/Tokyo", 4, "monday", &["today"], "");
    let before = must(&tokyo, "2018-06-20T14:30:00Z", None);
    assert_eq!(before.as_of, "2018-06-20T14:30:00Z");
    assert_eq!(anchor(&before, "today").civil, "2018-06-20");
    let after = must(&tokyo, "2018-06-20T15:30:00Z", None);
    assert_eq!(anchor(&after, "today").civil, "2018-06-21");
    assert_eq!(unique(&anchor(&after, "today").midnight), "2018-06-21T00:00:00+09:00");
    assert_ne!(anchor(&after, "today").civil, "2018-06-20");

    let los_angeles = calendar("America/Los_Angeles", 1, "monday", &["today"], "");
    let west = must(&los_angeles, "2018-06-21T06:30:00Z", None);
    assert_eq!(anchor(&west, "today").civil, "2018-06-20");
    assert_eq!(unique(&anchor(&west, "today").midnight), "2018-06-20T00:00:00-07:00");
    assert_ne!(anchor(&west, "today").civil, "2018-06-21");
}

#[test]
fn dsl_supported_or_refused_and_week_is_not_an_anchor() {
    let phrases = "\
  zeta: { start: \"today + 10\", end: today, bounds: \"[)\" }
  alpha: { start: \"today - 29\", end: \"today + 1\", bounds: \"[)\" }
  near: { start: month_start, end: \"min(max(today, month_start), month_start + 3)\", bounds: \"[)\" }
  empty: { start: month_start, end: \"max(month_start, today - 40)\", bounds: \"[)\" }
";
    let yaml = calendar(
        "UTC",
        1,
        "saturday",
        &["today", "month_start"],
        phrases,
    );
    let report = must(&yaml, "2018-06-20T11:00:00Z", None);
    assert_eq!(serde_json::to_value(report.week_start).unwrap(), json!("saturday"));
    assert_eq!(
        report.phrases.iter().map(label_of).collect::<Vec<_>>(),
        vec!["alpha", "empty", "near", "zeta"]
    );
    assert_eq!(range_end(report.phrases.first().unwrap()), ("2018-05-22", "2018-06-21", "2018-06-20"));
    assert_eq!(range_end(report.phrases.get(1).unwrap()), ("2018-06-01", "2018-06-01", "2018-05-31"));
    assert_eq!(range_end(report.phrases.get(2).unwrap()), ("2018-06-01", "2018-06-04", "2018-06-03"));
    assert_eq!(range_end(report.phrases.get(3).unwrap()), ("2018-06-30", "2018-06-20", "2018-06-19"));
    for phrase in &report.phrases {
        match phrase {
            PhraseReport::Range { bounds, interval, block_reason, .. } => {
                assert_eq!(*bounds, "[)");
                assert_eq!(*interval, IntervalKind::Open);
                assert_eq!(*block_reason, None);
            }
            PhraseReport::Comparison { .. } => panic!("range"),
        }
    }

    let typed = calendar(
        "UTC",
        1,
        "monday",
        &["today", "month_start"],
        "  days: { start: month_start, end: \"today - month_start\", bounds: \"[)\" }\n",
    );
    let mismatch = resolve_at(&typed, "2018-06-20T11:00:00Z", None).unwrap_err();
    assert_eq!(mismatch.reason, "expression_refused");
    assert_eq!(mismatch.detail, Some("type_mismatch"));
    assert_eq!(mismatch.phrase_index, Some(0));

    let floor = calendar(
        "UTC",
        1,
        "monday",
        &["today"],
        "  bad: { start: today, end: \"floor(today)\", bounds: \"[)\" }\n",
    );
    let invalid = resolve_at(&floor, "2018-06-20T11:00:00Z", None).unwrap_err();
    assert_eq!(invalid.reason, "calendar_invalid");
    assert!(invalid.diagnostic_count.unwrap_or(0) > 0);
    assert_ne!(invalid.reason, "expression_refused");

    let missing = calendar(
        "UTC",
        1,
        "monday",
        &["month_start"],
        "  bad: { start: today, end: \"today + 1\", bounds: \"[)\" }\n",
    );
    assert_eq!(resolve_at(&missing, "2018-06-20T11:00:00Z", None).unwrap_err().reason, "calendar_invalid");

    let week = calendar("UTC", 1, "saturday", &["week"], "");
    let refused = resolve_at(&week, "2018-06-20T11:00:00Z", None).unwrap_err();
    assert_eq!(refused.reason, "calendar_invalid");
    let encoded = serde_json::to_string(&refused).unwrap();
    assert!(!encoded.contains("2018-06-20"));
    assert!(!encoded.contains("week_start"));
}

#[test]
fn missing_closure_is_not_today_and_hash_is_not_approval() {
    let yaml = calendar("UTC", 4, "monday", &["today", "last_closed_period_end"], "");
    let missing = resolve_at(&yaml, "2015-05-17T04:00:00Z", None).unwrap_err();
    assert_eq!(missing.reason, "closure_required");
    assert!(missing.anchor.is_none());
    let encoded = serde_json::to_string(&missing).unwrap();
    assert!(!encoded.contains("2015-05-17"));
    assert!(!encoded.contains("today"));

    let report = must(
        &yaml,
        "2015-05-17T04:00:00Z",
        Some(Closure {
            last_closed_period_end: "2014-11-30",
            label: "finance-close-2014",
            record_sha256: CALLER_HASH,
        }),
    );
    flags_are_closed(&report);
    assert_eq!(anchor(&report, "today").civil, "2015-05-17");
    assert_eq!(anchor(&report, "last_closed_period_end").civil, "2014-11-30");
    assert_ne!(anchor(&report, "last_closed_period_end").civil, anchor(&report, "today").civil);
    let closure = report.closure.as_ref().unwrap();
    assert_eq!(closure.record_sha256, CALLER_HASH);
    assert_eq!(closure.label, "finance-close-2014");
    assert_ne!(closure.record_sha256, sha256_hex(b"finance-close-2014"));
    assert_ne!(closure.record_sha256, sha256_hex(yaml.as_bytes()));

    let open = calendar("UTC", 4, "monday", &["today"], "");
    let extra = resolve_at(
        &open,
        "2015-05-17T04:00:00Z",
        Some(Closure {
            last_closed_period_end: "2014-11-30",
            label: "finance-close-2014",
            record_sha256: CALLER_HASH,
        }),
    )
    .unwrap_err();
    assert_eq!(extra.reason, "closure_not_configured");
}

fn bin() -> &'static str {
    option_env!("CARGO_BIN_EXE_business-calendar")
        .or(option_env!("CARGO_BIN_EXE_business_calendar"))
        .expect("business-calendar test binary")
}

fn run_cli(input: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(input).expect("write");
    let output = child.wait_with_output().expect("wait");
    (
        output.status.code().unwrap_or(99),
        String::from_utf8(output.stdout).expect("stdout utf8"),
        String::from_utf8(output.stderr).expect("stderr utf8"),
    )
}

fn document(yaml: &str, as_of: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "contract": "oo.business-calendar/v1",
        "calendar_yaml": yaml,
        "as_of": as_of
    }))
    .expect("json")
}

fn one_object(stdout: &str) -> Value {
    assert_eq!(stdout.matches('\n').count(), 1, "{stdout}");
    assert!(stdout.ends_with('\n'));
    serde_json::from_str(stdout.trim_end()).expect("json object")
}

#[test]
fn compiled_cli_exact_honolulu_json() {
    assert_eq!(sha256_hex(HONOLULU.as_bytes()), HONOLULU_HASH);
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    let expected = concat!(
        r#"{"contract":"oo.business-calendar/v1","outcome":"resolved","as_of":"2018-02-10T08:30:00Z","business_tz":"Pacific/Honolulu","tzdb_version":"2025b","fiscal_year_start_month":4,"week_start":"wednesday","quarter_convention":"business_fiscal_quarter","quarter_note":"three civil months counted from fiscal_year_start_month; distinct from date_trunc('quarter')","config_sha256":""#,
        "0f47e4d306986fc5a77318f01eb204a3874f3e86e65c1f63cca3e9b3b32839e1",
        r#"","finance_approval":false,"authenticated":false,"database_used":false,"clock_used":false,"closure":null,"anchors":[{"name":"today","civil":"2018-02-09","midnight":{"status":"unique","rfc3339":"2018-02-09T00:00:00-10:00"},"next_midnight":{"status":"unique","rfc3339":"2018-02-10T00:00:00-10:00"},"civil_day_seconds":86400},{"name":"fiscal_year_start","civil":"2017-04-01","midnight":{"status":"unique","rfc3339":"2017-04-01T00:00:00-10:00"},"next_midnight":{"status":"unique","rfc3339":"2017-04-02T00:00:00-10:00"},"civil_day_seconds":86400}],"phrases":[]}"#,
        "\n"
    );
    let (code, stdout, stderr) = run_cli(&document(HONOLULU, "2018-02-10T08:30:00Z"));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stderr, "");
    assert_eq!(stdout, expected);
    let value = one_object(&stdout);
    assert_eq!(value.as_object().unwrap().len(), 17);
    assert_eq!(value["anchors"][0]["civil"], "2018-02-09");
    assert_ne!(value["anchors"][0]["civil"], "2018-02-10");
    assert_eq!(value["week_start"], "wednesday");
    assert!(value["anchors"].as_array().unwrap().iter().all(|item| item["name"] != "week"));
}

#[test]
fn compiled_cli_new_york_sao_paulo_paris_apia() {
    let new_york = calendar("America/New_York", 1, "monday", &["today"], "");
    let (code, stdout, stderr) = run_cli(&document(&new_york, "2019-03-10T16:00:00Z"));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stderr, "");
    let spring = one_object(&stdout);
    assert_eq!(spring["anchors"][0]["civil"], "2019-03-10");
    assert_eq!(spring["anchors"][0]["midnight"]["rfc3339"], "2019-03-10T00:00:00-05:00");
    assert_eq!(spring["anchors"][0]["next_midnight"]["rfc3339"], "2019-03-11T00:00:00-04:00");
    assert_eq!(spring["anchors"][0]["civil_day_seconds"], 82_800);

    let (code, stdout, stderr) = run_cli(&document(&new_york, "2019-11-03T16:00:00Z"));
    assert_eq!(code, 0, "{stderr}");
    let fall = one_object(&stdout);
    assert_eq!(fall["anchors"][0]["civil"], "2019-11-03");
    assert_eq!(fall["anchors"][0]["midnight"]["rfc3339"], "2019-11-03T00:00:00-04:00");
    assert_eq!(fall["anchors"][0]["next_midnight"]["rfc3339"], "2019-11-04T00:00:00-05:00");
    assert_eq!(fall["anchors"][0]["civil_day_seconds"], 90_000);

    let paulo = calendar(
        "America/Sao_Paulo",
        1,
        "monday",
        &["today", "month_start"],
        "  gap: { start: today, end: \"today + 1\", bounds: \"[)\" }\n",
    );
    let (code, stdout, stderr) = run_cli(&document(&paulo, "2017-10-15T15:00:00Z"));
    assert_eq!(code, 0, "{stderr}");
    assert!(!stdout.contains("T01:00:00"));
    let gap = one_object(&stdout);
    assert_eq!(gap["anchors"][0]["civil"], "2017-10-15");
    assert_eq!(gap["anchors"][0]["midnight"], json!({"status": "nonexistent"}));
    assert!(gap["anchors"][0]["midnight"].get("rfc3339").is_none());
    assert_eq!(gap["anchors"][0]["civil_day_seconds"], Value::Null);
    assert_eq!(gap["anchors"][0]["next_midnight"]["rfc3339"], "2017-10-16T00:00:00-02:00");
    assert_eq!(gap["anchors"][1]["name"], "month_start");
    assert_eq!(gap["anchors"][1]["midnight"]["rfc3339"], "2017-10-01T00:00:00-03:00");
    assert_eq!(gap["phrases"][0]["interval"], "blocked");
    assert_eq!(gap["phrases"][0]["block_reason"], "nonexistent_midnight");
    assert_eq!(gap["phrases"][0]["start_civil"], "2017-10-15");

    let paris = calendar(
        "Europe/Paris",
        1,
        "monday",
        &["today"],
        "  fold: { start: \"today - 1\", end: today, bounds: \"[)\" }\n",
    );
    let (code, stdout, stderr) = run_cli(&document(&paris, "1976-09-26T22:30:00Z"));
    assert_eq!(code, 0, "{stderr}");
    let fold = one_object(&stdout);
    assert_eq!(fold["as_of"], "1976-09-26T22:30:00Z");
    assert_eq!(fold["anchors"][0]["civil"], "1976-09-26");
    assert_eq!(
        fold["anchors"][0]["midnight"],
        json!({
            "status": "ambiguous",
            "fold0_rfc3339": "1976-09-26T00:00:00+02:00",
            "fold1_rfc3339": "1976-09-26T00:00:00+01:00"
        })
    );
    assert_eq!(fold["anchors"][0]["next_midnight"]["rfc3339"], "1976-09-27T00:00:00+01:00");
    assert_eq!(fold["anchors"][0]["civil_day_seconds"], Value::Null);
    assert_eq!(fold["phrases"][0]["start_civil"], "1976-09-25");
    assert_eq!(fold["phrases"][0]["start_midnight"]["rfc3339"], "1976-09-25T00:00:00+02:00");
    assert_eq!(fold["phrases"][0]["end_exclusive_civil"], "1976-09-26");
    assert_eq!(fold["phrases"][0]["interval"], "blocked");
    assert_eq!(fold["phrases"][0]["block_reason"], "ambiguous_midnight");
    assert!(!stdout.contains("1976-09-26T00:00:00Z"));

    let apia = calendar(
        "Pacific/Apia",
        1,
        "monday",
        &["today"],
        "  gap: { start: \"today - 1\", end: today, bounds: \"[)\" }\n  span: { start: \"today - 2\", end: today, bounds: \"[)\" }\n",
    );
    let (code, stdout, stderr) = run_cli(&document(&apia, "2011-12-31T08:00:00Z"));
    assert_eq!(code, 0, "{stderr}");
    assert!(!stdout.contains("2011-12-30T"));
    let skipped = one_object(&stdout);
    assert_eq!(skipped["anchors"][0]["civil"], "2011-12-31");
    assert_eq!(skipped["anchors"][0]["midnight"]["rfc3339"], "2011-12-31T00:00:00+14:00");
    assert_eq!(skipped["phrases"][0]["label"], "gap");
    assert_eq!(skipped["phrases"][0]["start_civil"], "2011-12-30");
    assert_eq!(skipped["phrases"][0]["start_midnight"], json!({"status": "nonexistent"}));
    assert_eq!(skipped["phrases"][0]["inclusive_end_civil"], "2011-12-30");
    assert_eq!(skipped["phrases"][0]["end_midnight"]["rfc3339"], "2011-12-31T00:00:00+14:00");
    assert_eq!(skipped["phrases"][0]["block_reason"], "nonexistent_midnight");
    assert_eq!(skipped["phrases"][1]["label"], "span");
    assert_eq!(skipped["phrases"][1]["start_civil"], "2011-12-29");
    assert_eq!(skipped["phrases"][1]["start_midnight"]["rfc3339"], "2011-12-29T00:00:00-10:00");
    assert_eq!(skipped["phrases"][1]["inclusive_end_civil"], "2011-12-30");
    assert_eq!(skipped["phrases"][1]["end_exclusive_civil"], "2011-12-31");
    assert_eq!(skipped["phrases"][1]["interval"], "open");
    assert_eq!(skipped["phrases"][1]["block_reason"], Value::Null);
}

#[test]
fn compiled_cli_closure_bounds_and_redaction() {
    let closed = calendar("UTC", 4, "monday", &["last_closed_period_end"], "");
    let (code, stdout, stderr) = run_cli(&document(&closed, "2015-05-17T04:00:00Z"));
    assert_eq!(code, 5);
    assert_eq!(stderr, "business-calendar: refused closure_required\n");
    assert_eq!(
        stdout,
        "{\"contract\":\"oo.business-calendar/v1\",\"outcome\":\"refused\",\"reason\":\"closure_required\"}\n"
    );
    assert!(!stdout.contains("2015-05-17"));

    let mut request: Value = serde_json::from_slice(&document(&closed, "2015-05-17T04:00:00Z")).unwrap();
    request["closure"] = json!({
        "last_closed_period_end": "2014-11-30",
        "label": "finance-close-2014",
        "record_sha256": CALLER_HASH
    });
    let (code, stdout, stderr) = run_cli(&serde_json::to_vec(&request).unwrap());
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stderr, "");
    let approved = one_object(&stdout);
    assert_eq!(approved["finance_approval"].as_bool(), Some(false));
    assert_eq!(approved["authenticated"].as_bool(), Some(false));
    assert_eq!(approved["database_used"].as_bool(), Some(false));
    assert_eq!(approved["clock_used"].as_bool(), Some(false));
    assert_eq!(approved["closure"]["record_sha256"], CALLER_HASH);
    assert_eq!(approved["closure"]["last_closed_period_end"], "2014-11-30");
    assert_eq!(approved["anchors"][0]["civil"], "2014-11-30");
    assert_ne!(approved["closure"]["record_sha256"], sha256_hex(b"finance-close-2014"));

    request["closure"]["record_sha256"] = json!("DEADBEEFdeadbeefDEADBEEFdeadbeefDEADBEEFdeadbeefDEADBEEFdeadbeef");
    let (code, stdout, stderr) = run_cli(&serde_json::to_vec(&request).unwrap());
    assert_eq!(code, 5);
    assert_eq!(stderr, "business-calendar: refused closure_invalid\n");
    assert!(stdout.contains("\"detail\":\"hash\""));
    assert!(!stdout.contains("DEADBEEF"));
    assert!(!stdout.contains("finance-close-2014"));
    assert!(!stderr.contains("DEADBEEF"));

    let (code, stdout, stderr) = run_cli(&vec![b'Q'; STDIN_MAX_BYTES + 1]);
    assert_eq!(code, 3);
    assert_eq!(stderr, "business-calendar: refused budget\n");
    assert_eq!(
        stdout,
        "{\"contract\":\"oo.business-calendar/v1\",\"outcome\":\"refused\",\"reason\":\"budget\"}\n"
    );
    assert!(!stdout.contains("QQQ"));

    let fat = "q".repeat(CALENDAR_YAML_MAX_BYTES + 1);
    let (code, stdout, stderr) = run_cli(&document(&fat, "2015-05-17T04:00:00Z"));
    assert_eq!(code, 3);
    assert_eq!(stderr, "business-calendar: refused yaml_budget\n");
    assert!(!stdout.contains("qqq"));
    assert!(!stderr.contains("qqq"));

    let long_as_of = "x".repeat(41);
    let (code, stdout, stderr) = run_cli(&document(HONOLULU, &long_as_of));
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused malformed\n");
    assert!(!stdout.contains(&long_as_of));

    let mut extra = serde_json::from_slice::<Value>(&document(HONOLULU, "2018-02-10T08:30:00Z")).unwrap();
    extra["note"] = json!("SENTINEL_FIELD_9c2e");
    let (code, stdout, stderr) = run_cli(&serde_json::to_vec(&extra).unwrap());
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused malformed\n");
    assert!(!stdout.contains("SENTINEL_FIELD_9c2e"));

    let mut second = document(HONOLULU, "2018-02-10T08:30:00Z");
    second.extend_from_slice(b"{\"extra\":1}");
    let (code, stdout, stderr) = run_cli(&second);
    assert_eq!(code, 1);
    assert_eq!(stderr, "business-calendar: refused malformed\n");
    assert!(!stdout.contains("Honolulu"));

    let floor = "schema_version: oos/v1\nbusiness_tz: UTC\nfiscal_year_start_month: 1\nweek_start: monday\nanchors:\n  - today\nphrases:\n  SENTINEL_PHRASE_4b7d: { start: today, end: \"floor(today)\", bounds: \"[)\" }\n";
    let (code, stdout, stderr) = run_cli(&document(floor, "2018-06-20T11:00:00Z"));
    assert_eq!(code, 4, "{stdout}");
    assert_eq!(stderr, "business-calendar: refused calendar_invalid\n");
    assert!(!stdout.contains("SENTINEL_PHRASE_4b7d"));
    assert!(!stdout.contains("floor"));
    assert!(!stderr.contains("floor"));
    let refusal = one_object(&stdout);
    assert_eq!(refusal["reason"], "calendar_invalid");
    assert!(refusal["diagnostic_count"].as_u64().unwrap_or(0) > 0);
    assert!(refusal.get("anchors").is_none());

    let week = calendar("UTC", 1, "saturday", &["week"], "");
    let (code, stdout, stderr) = run_cli(&document(&week, "2018-06-20T11:00:00Z"));
    assert_eq!(code, 4);
    assert_eq!(stderr, "business-calendar: refused calendar_invalid\n");
    assert!(!stdout.contains("\"name\":\"week\""));
    assert!(one_object(&stdout).get("anchors").is_none());
}
