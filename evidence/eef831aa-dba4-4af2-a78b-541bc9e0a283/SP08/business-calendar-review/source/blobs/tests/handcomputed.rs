use business_calendar::{
    BlockReason, Closure, Input, IntervalKind, Midnight, PhraseReport, Report, resolve, sha256_hex,
};

const RECORD: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EXAMPLE_HASH: &str = "2ad3845ec287c24391168ce1e7271d743202b0b2d5cef7d6ad05537e02fcc378";

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

fn run(yaml: &str, as_of: &str, closure: Option<Closure<'_>>) -> Result<Report, business_calendar::Refusal> {
    resolve(&Input {
        calendar_yaml: yaml,
        as_of,
        closure,
    })
}

fn must(yaml: &str, as_of: &str, closure: Option<Closure<'_>>) -> Report {
    run(yaml, as_of, closure).unwrap_or_else(|err| {
        let reason = err.reason;
        panic!("refused {reason}")
    })
}

fn anchor<'a>(report: &'a Report, name: &str) -> &'a business_calendar::AnchorReport {
    report
        .anchors
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn unique(midnight: &Midnight) -> &str {
    match midnight {
        Midnight::Unique { rfc3339 } => rfc3339,
        other => panic!("expected unique midnight, got {other:?}"),
    }
}

#[test]
fn example_calendar_shanghai_april() {
    let yaml = include_str!("../../../examples/schema/calendar.yaml");
    assert_eq!(sha256_hex(yaml.as_bytes()), EXAMPLE_HASH);
    let report = must(
        yaml,
        "2026-04-01T03:30:00Z",
        Some(Closure {
            last_closed_period_end: "2026-03-31",
            label: "close-2026-03",
            record_sha256: RECORD,
        }),
    );
    assert_eq!(report.as_of, "2026-04-01T03:30:00Z");
    assert_eq!(report.business_tz, "Asia/Shanghai");
    assert_eq!(report.tzdb_version, "2025b");
    assert_eq!(report.fiscal_year_start_month, 4);
    assert_eq!(report.week_start, oo_core::oos::Weekday::Monday);
    assert_eq!(report.quarter_convention, "business_fiscal_quarter");
    assert_eq!(report.config_sha256, EXAMPLE_HASH);
    assert!(!report.finance_approval);
    assert!(!report.authenticated);
    assert!(!report.database_used);
    assert!(!report.clock_used);
    let closure = report.closure.as_ref().expect("closure");
    assert_eq!(closure.last_closed_period_end, "2026-03-31");
    assert_eq!(closure.label, "close-2026-03");
    assert_eq!(closure.record_sha256, RECORD);
    assert_ne!(closure.record_sha256, sha256_hex(b"close-2026-03"));
    let expect = [
        ("today", "2026-04-01"),
        ("month_start", "2026-04-01"),
        ("prev_month_start", "2026-03-01"),
        ("prev_month_end", "2026-03-31"),
        ("quarter_start", "2026-04-01"),
        ("prev_quarter_start", "2026-01-01"),
        ("prev_quarter_end", "2026-03-31"),
        ("fiscal_year_start", "2026-04-01"),
        ("prev_fiscal_year_end", "2026-03-31"),
        ("last_closed_period_end", "2026-03-31"),
    ];
    assert_eq!(report.anchors.len(), expect.len());
    for (item, (name, civil)) in report.anchors.iter().zip(expect) {
        assert_eq!(item.name, name);
        assert_eq!(item.civil, civil);
        assert_eq!(unique(&item.midnight), format!("{civil}T00:00:00+08:00"));
        assert_eq!(item.civil_day_seconds, Some(86_400));
    }
    assert_eq!(
        report.phrases.iter().map(label_of).collect::<Vec<_>>(),
        vec!["上个月", "上季度", "本月至今对比上月同期", "近30天"]
    );
    assert_span(report.phrases.first().unwrap(), "2026-03-01", "2026-04-01", "2026-03-31");
    assert_span(report.phrases.get(1).unwrap(), "2026-01-01", "2026-04-01", "2026-03-31");
    match report.phrases.get(2).unwrap() {
        PhraseReport::Comparison { current, previous, .. } => {
            assert_eq!(current.start_civil, "2026-04-01");
            assert_eq!(current.end_exclusive_civil, "2026-04-02");
            assert_eq!(current.inclusive_end_civil, "2026-04-01");
            assert_eq!(previous.start_civil, "2026-03-01");
            assert_eq!(previous.end_exclusive_civil, "2026-03-02");
            assert_eq!(previous.inclusive_end_civil, "2026-03-01");
            assert_eq!(current.interval, IntervalKind::Open);
            assert_eq!(previous.interval, IntervalKind::Open);
        }
        PhraseReport::Range { .. } => panic!("comparison phrase"),
    }
    assert_span(report.phrases.get(3).unwrap(), "2026-03-03", "2026-04-02", "2026-04-01");
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(encoded.contains("上个月"));
    assert!(!encoded.contains("\\u4e0a"));
    assert!(report.anchors.iter().all(|item| !item.name.contains("week")));
}

#[test]
fn fiscal_february_is_not_the_calendar_quarter() {
    let yaml = calendar(
        "UTC",
        2,
        "monday",
        &[
            "today",
            "quarter_start",
            "prev_quarter_start",
            "prev_quarter_end",
            "fiscal_year_start",
            "prev_fiscal_year_end",
        ],
        "",
    );
    let report = must(&yaml, "2026-03-15T12:00:00Z", None);
    assert_eq!(anchor(&report, "today").civil, "2026-03-15");
    assert_eq!(anchor(&report, "quarter_start").civil, "2026-02-01");
    assert_eq!(anchor(&report, "prev_quarter_start").civil, "2025-11-01");
    assert_eq!(anchor(&report, "prev_quarter_end").civil, "2026-01-31");
    assert_eq!(anchor(&report, "fiscal_year_start").civil, "2026-02-01");
    assert_eq!(anchor(&report, "prev_fiscal_year_end").civil, "2026-01-31");
    assert_eq!(unique(&anchor(&report, "quarter_start").midnight), "2026-02-01T00:00:00Z");
}

#[test]
fn fiscal_april_and_year_boundary_and_leap_day() {
    let quarter = calendar(
        "UTC",
        4,
        "sunday",
        &[
            "quarter_start",
            "prev_quarter_start",
            "prev_quarter_end",
            "fiscal_year_start",
            "prev_fiscal_year_end",
            "prev_month_start",
            "prev_month_end",
        ],
        "",
    );
    let march = must(&quarter, "2026-03-15T12:00:00Z", None);
    assert_eq!(march.week_start, oo_core::oos::Weekday::Sunday);
    assert_eq!(anchor(&march, "fiscal_year_start").civil, "2025-04-01");
    assert_eq!(anchor(&march, "quarter_start").civil, "2026-01-01");
    assert_eq!(anchor(&march, "prev_quarter_start").civil, "2025-10-01");
    assert_eq!(anchor(&march, "prev_quarter_end").civil, "2025-12-31");
    assert_eq!(anchor(&march, "prev_fiscal_year_end").civil, "2025-03-31");
    assert_eq!(anchor(&march, "prev_month_start").civil, "2026-02-01");
    assert_eq!(anchor(&march, "prev_month_end").civil, "2026-02-28");

    let january = calendar("UTC", 1, "monday", &["prev_month_start", "prev_month_end", "today"], "");
    let year = must(&january, "2026-01-15T00:30:00Z", None);
    assert_eq!(
        year.anchors.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(),
        vec!["prev_month_start", "prev_month_end", "today"]
    );
    assert_eq!(anchor(&year, "prev_month_start").civil, "2025-12-01");
    assert_eq!(anchor(&year, "prev_month_end").civil, "2025-12-31");
    assert_eq!(anchor(&year, "today").civil, "2026-01-15");

    let leap = calendar("UTC", 1, "monday", &["prev_month_end"], "");
    assert_eq!(anchor(&must(&leap, "2024-03-01T00:30:00Z", None), "prev_month_end").civil, "2024-02-29");
    assert_eq!(anchor(&must(&leap, "2025-03-01T00:30:00Z", None), "prev_month_end").civil, "2025-02-28");
}

#[test]
fn month_to_date_clamp_and_day_arithmetic() {
    let phrases = "  compare:\n    current: { start: month_start, end: \"today + 1\", bounds: \"[)\" }\n    previous:\n      start: prev_month_start\n      end: \"min(month_start, prev_month_start + (today - month_start) + 1)\"\n      bounds: \"[)\"\n";
    let yaml = calendar(
        "UTC",
        1,
        "monday",
        &["today", "month_start", "prev_month_start"],
        phrases,
    );
    let short = must(&yaml, "2026-03-15T12:00:00Z", None);
    match short.phrases.first().unwrap() {
        PhraseReport::Comparison { previous, .. } => {
            assert_eq!(previous.start_civil, "2026-02-01");
            assert_eq!(previous.end_exclusive_civil, "2026-02-16");
        }
        PhraseReport::Range { .. } => panic!("comparison"),
    }
    let leap = must(&yaml, "2024-03-29T12:00:00Z", None);
    match leap.phrases.first().unwrap() {
        PhraseReport::Comparison { current, previous, .. } => {
            assert_eq!(current.start_civil, "2024-03-01");
            assert_eq!(current.end_exclusive_civil, "2024-03-30");
            assert_eq!(previous.start_civil, "2024-02-01");
            assert_eq!(previous.end_exclusive_civil, "2024-03-01");
        }
        PhraseReport::Range { .. } => panic!("comparison"),
    }
    let clamped = must(&yaml, "2026-03-31T12:00:00Z", None);
    match clamped.phrases.first().unwrap() {
        PhraseReport::Comparison { previous, .. } => {
            assert_eq!(previous.end_exclusive_civil, "2026-03-01");
        }
        PhraseReport::Range { .. } => panic!("comparison"),
    }

    let math = calendar(
        "UTC",
        1,
        "monday",
        &["today", "month_start"],
        "  later: { start: month_start, end: \"max(today, month_start)\", bounds: \"[)\" }\n  back: { start: \"today - 1 - 1\", end: \"today + 1 - 1\", bounds: \"[)\" }\n  reversed: { start: \"today + 10\", end: today, bounds: \"[)\" }\n",
    );
    let report = must(&math, "2026-03-15T12:00:00Z", None);
    assert_eq!(
        report.phrases.iter().map(label_of).collect::<Vec<_>>(),
        vec!["back", "later", "reversed"]
    );
    assert_span(report.phrases.first().unwrap(), "2026-03-13", "2026-03-15", "2026-03-14");
    assert_span(report.phrases.get(1).unwrap(), "2026-03-01", "2026-03-15", "2026-03-14");
    assert_span(report.phrases.get(2).unwrap(), "2026-03-25", "2026-03-15", "2026-03-14");
}

#[test]
fn zone_date_split_and_dst_lengths() {
    let shanghai = calendar("Asia/Shanghai", 4, "monday", &["today"], "");
    assert_eq!(anchor(&must(&shanghai, "2026-04-01T03:30:00+00:00", None), "today").civil, "2026-04-01");
    assert_eq!(
        must(&shanghai, "2026-04-01T03:30:00+00:00", None).as_of,
        "2026-04-01T03:30:00Z"
    );
    assert_eq!(anchor(&must(&shanghai, "2026-04-01T16:00:00Z", None), "today").civil, "2026-04-02");
    assert_eq!(anchor(&must(&shanghai, "2026-04-01T15:59:59.50Z", None), "today").civil, "2026-04-01");
    assert_eq!(must(&shanghai, "2026-04-01T15:59:59.50Z", None).as_of, "2026-04-01T15:59:59.5Z");
    assert_eq!(
        anchor(&must(&shanghai, "2026-03-31T16:30:00Z", None), "today").civil,
        "2026-04-01"
    );

    let new_york = calendar("America/New_York", 1, "monday", &["today"], "");
    assert_eq!(anchor(&must(&new_york, "2026-04-01T03:30:00Z", None), "today").civil, "2026-03-31");
    assert_eq!(
        unique(&anchor(&must(&new_york, "2026-04-01T03:30:00Z", None), "today").midnight),
        "2026-03-31T00:00:00-04:00"
    );

    let spring = must(&new_york, "2026-03-08T16:00:00Z", None);
    let spring_today = anchor(&spring, "today");
    assert_eq!(spring_today.civil, "2026-03-08");
    assert_eq!(unique(&spring_today.midnight), "2026-03-08T00:00:00-05:00");
    assert_eq!(unique(&spring_today.next_midnight), "2026-03-09T00:00:00-04:00");
    assert_eq!(spring_today.civil_day_seconds, Some(82_800));

    let fall = must(&new_york, "2026-11-01T16:00:00Z", None);
    let fall_today = anchor(&fall, "today");
    assert_eq!(fall_today.civil, "2026-11-01");
    assert_eq!(unique(&fall_today.midnight), "2026-11-01T00:00:00-04:00");
    assert_eq!(unique(&fall_today.next_midnight), "2026-11-02T00:00:00-05:00");
    assert_eq!(fall_today.civil_day_seconds, Some(90_000));
}

#[test]
fn missing_and_ambiguous_midnights_stay_unshifted() {
    let paulo = calendar(
        "America/Sao_Paulo",
        1,
        "monday",
        &["today", "month_start"],
        "  gap: { start: today, end: \"today + 1\", bounds: \"[)\" }\n",
    );
    let report = must(&paulo, "2018-11-04T12:00:00-02:00", None);
    let today = anchor(&report, "today");
    assert_eq!(today.civil, "2018-11-04");
    assert_eq!(today.midnight, Midnight::Nonexistent);
    assert_eq!(unique(&today.next_midnight), "2018-11-05T00:00:00-02:00");
    assert_eq!(today.civil_day_seconds, None);
    assert_eq!(
        unique(&anchor(&report, "month_start").midnight),
        "2018-11-01T00:00:00-03:00"
    );
    match report.phrases.first().unwrap() {
        PhraseReport::Range {
            start_civil,
            end_exclusive_civil,
            interval,
            block_reason,
            ..
        } => {
            assert_eq!(start_civil, "2018-11-04");
            assert_eq!(end_exclusive_civil, "2018-11-05");
            assert_eq!(*interval, IntervalKind::Blocked);
            assert_eq!(*block_reason, Some(BlockReason::NonexistentMidnight));
        }
        PhraseReport::Comparison { .. } => panic!("range"),
    }

    let apia = calendar(
        "Pacific/Apia",
        1,
        "monday",
        &["today", "month_start"],
        "  skipped: { start: \"today - 1\", end: today, bounds: \"[)\" }\n",
    );
    let report = must(&apia, "2011-12-31T12:00:00+14:00", None);
    assert_eq!(anchor(&report, "today").civil, "2011-12-31");
    assert_eq!(unique(&anchor(&report, "today").midnight), "2011-12-31T00:00:00+14:00");
    assert_eq!(
        unique(&anchor(&report, "month_start").midnight),
        "2011-12-01T00:00:00-10:00"
    );
    match report.phrases.first().unwrap() {
        PhraseReport::Range {
            start_civil,
            start_midnight,
            inclusive_end_civil,
            block_reason,
            ..
        } => {
            assert_eq!(start_civil, "2011-12-30");
            assert_eq!(*start_midnight, Midnight::Nonexistent);
            assert_eq!(inclusive_end_civil, "2011-12-30");
            assert_eq!(*block_reason, Some(BlockReason::NonexistentMidnight));
        }
        PhraseReport::Comparison { .. } => panic!("range"),
    }

    let paris = calendar(
        "Europe/Paris",
        1,
        "monday",
        &["today"],
        "  fold: { start: today, end: \"today + 1\", bounds: \"[)\" }\n",
    );
    let report = must(&paris, "1976-09-26T12:00:00+01:00", None);
    let today = anchor(&report, "today");
    assert_eq!(today.civil, "1976-09-26");
    assert_eq!(
        today.midnight,
        Midnight::Ambiguous {
            fold0_rfc3339: "1976-09-26T00:00:00+02:00".to_owned(),
            fold1_rfc3339: "1976-09-26T00:00:00+01:00".to_owned(),
        }
    );
    assert_eq!(unique(&today.next_midnight), "1976-09-27T00:00:00+01:00");
    assert_eq!(today.civil_day_seconds, None);
    match report.phrases.first().unwrap() {
        PhraseReport::Range { interval, block_reason, .. } => {
            assert_eq!(*interval, IntervalKind::Blocked);
            assert_eq!(*block_reason, Some(BlockReason::AmbiguousMidnight));
        }
        PhraseReport::Comparison { .. } => panic!("range"),
    }
}

#[test]
fn refusals_keep_provenance_and_do_not_substitute() {
    let closed = calendar("UTC", 1, "monday", &["today", "last_closed_period_end"], "");
    let missing = run(&closed, "2026-04-01T12:00:00Z", None).unwrap_err();
    assert_eq!(missing.reason, "closure_required");
    let report = must(
        &closed,
        "2026-04-01T12:00:00Z",
        Some(Closure {
            last_closed_period_end: "2026-03-31",
            label: "close-2026-03",
            record_sha256: RECORD,
        }),
    );
    assert_eq!(anchor(&report, "today").civil, "2026-04-01");
    assert_eq!(anchor(&report, "last_closed_period_end").civil, "2026-03-31");

    let open = calendar("UTC", 1, "monday", &["today"], "");
    let extra = run(
        &open,
        "2026-04-01T12:00:00Z",
        Some(Closure {
            last_closed_period_end: "2026-03-31",
            label: "close-2026-03",
            record_sha256: RECORD,
        }),
    )
    .unwrap_err();
    assert_eq!(extra.reason, "closure_not_configured");

    let bad_hash = run(
        &closed,
        "2026-04-01T12:00:00Z",
        Some(Closure {
            last_closed_period_end: "2026-03-31",
            label: "close-2026-03",
            record_sha256: "0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
        }),
    )
    .unwrap_err();
    assert_eq!(bad_hash.reason, "closure_invalid");
    assert_eq!(bad_hash.detail, Some("hash"));

    let typed = calendar(
        "UTC",
        1,
        "monday",
        &["today", "month_start"],
        "  days: { start: month_start, end: \"today - month_start\", bounds: \"[)\" }\n",
    );
    let mismatch = run(&typed, "2026-03-15T12:00:00Z", None).unwrap_err();
    assert_eq!(mismatch.reason, "expression_refused");
    assert_eq!(mismatch.detail, Some("type_mismatch"));
    assert_eq!(mismatch.phrase_index, Some(0));

    let dup = calendar("UTC", 1, "monday", &["today", "today"], "");
    let duplicate = run(&dup, "2026-04-01T12:00:00Z", None).unwrap_err();
    assert_eq!(duplicate.reason, "duplicate_anchor");
    assert_eq!(duplicate.anchor.as_deref(), Some("today"));

    let unknown = calendar("Foo/NotARealZone", 1, "monday", &["today"], "");
    assert_eq!(run(&unknown, "2026-04-01T12:00:00Z", None).unwrap_err().reason, "zone_unknown");

    let early = calendar("UTC", 1, "monday", &["prev_month_start"], "");
    let out = run(&early, "0001-01-15T12:00:00Z", None).unwrap_err();
    assert_eq!(out.reason, "date_out_of_range");
    assert_eq!(out.anchor.as_deref(), Some("prev_month_start"));

    let edge = calendar("UTC", 1, "monday", &["today"], "");
    let last = must(&edge, "9999-12-31T12:00:00Z", None);
    assert_eq!(anchor(&last, "today").midnight, Midnight::Unique { rfc3339: "9999-12-31T00:00:00Z".to_owned() });
    assert_eq!(anchor(&last, "today").next_midnight, Midnight::OutOfRange);
    assert_eq!(anchor(&last, "today").civil_day_seconds, None);

    let empty = calendar("UTC", 1, "monday", &[], "");
    let report = must(&empty, "2026-04-01T12:00:00Z", None);
    assert!(report.anchors.is_empty());
    assert!(report.phrases.is_empty());
    assert_eq!(report.config_sha256, sha256_hex(empty.as_bytes()));

    let missing_anchor = calendar("UTC", 1, "monday", &["month_start"], "  bad: { start: today, end: \"today + 1\", bounds: \"[)\" }\n");
    let invalid = run(&missing_anchor, "2026-04-01T12:00:00Z", None).unwrap_err();
    assert_eq!(invalid.reason, "calendar_invalid");
    assert!(invalid.diagnostic_count.unwrap_or(0) > 0);
    assert!(invalid.detail.is_none());
}

#[test]
fn source_does_not_read_a_clock() {
    let sources = [
        include_str!("../src/lib.rs"),
        include_str!("../src/main.rs"),
        include_str!("../src/civil.rs"),
        include_str!("../src/expr.rs"),
        include_str!("../src/zone.rs"),
        include_str!("../src/sha256.rs"),
        include_str!("../src/fault.rs"),
    ];
    for source in sources {
        assert!(!source.contains("SystemTime"));
        assert!(!source.contains("Utc::now"));
        assert!(!source.contains("Local::now"));
        assert!(!source.contains("std::process::Command"));
        assert!(!source.contains("std::process::exit"));
        assert!(!source.contains("Command::new"));
        assert!(!source.contains("unsafe {"));
    }
}

fn label_of(phrase: &PhraseReport) -> &str {
    match phrase {
        PhraseReport::Range { label, .. } | PhraseReport::Comparison { label, .. } => label,
    }
}

fn assert_span(phrase: &PhraseReport, start: &str, end: &str, inclusive: &str) {
    match phrase {
        PhraseReport::Range {
            bounds,
            start_civil,
            end_exclusive_civil,
            inclusive_end_civil,
            interval,
            block_reason,
            ..
        } => {
            assert_eq!(*bounds, "[)");
            assert_eq!(start_civil, start);
            assert_eq!(end_exclusive_civil, end);
            assert_eq!(inclusive_end_civil, inclusive);
            assert_eq!(*interval, IntervalKind::Open);
            assert_eq!(*block_reason, None);
        }
        PhraseReport::Comparison { .. } => panic!("expected a range phrase"),
    }
}
