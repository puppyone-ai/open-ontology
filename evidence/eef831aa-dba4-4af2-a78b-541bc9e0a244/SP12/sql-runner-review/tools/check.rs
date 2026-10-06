//! Check one sql-runner report. Prints no cells, gold, or secrets.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let report_path = args.next().expect("report");
    let stderr_path = args.next().expect("stderr");
    let secret_path = args.next().expect("secret");
    let expect_path = args.next().expect("expect");
    let query_sha = args.next().expect("query sha");
    let adapter_sha = args.next().expect("adapter sha");
    let comparator_sha = args.next().expect("comparator sha");
    let report = fs::read(&report_path).expect("report");
    let stderr = fs::read(&stderr_path).expect("stderr");
    let mut secret = fs::read(&secret_path).expect("secret");
    if secret.ends_with(b"\n") {
        secret.pop();
    }
    if secret.ends_with(b"\r") {
        secret.pop();
    }
    if secret.is_empty() {
        eprintln!("check: empty secret file");
        return ExitCode::from(2);
    }
    if contains(&report, &secret) || contains(&stderr, &secret) {
        eprintln!("check: secret bytes present");
        return ExitCode::from(1);
    }
    let bans: &[&[u8]] = &[
        b"CHANGED-CELL-a244",
        b"password_file",
        b"PRIVATE KEY",
        b"/evidence/built/gold",
        b"sealed-left",
        b"sealed-mid",
        b"sealed-right",
        "\u{96ea}".as_bytes(),
        b"beta",
        b"-9223372036854775808",
        b"9223372036854775807",
    ];
    for ban in bans {
        if contains(&report, ban) || contains(&stderr, ban) {
            eprintln!("check: banned token present");
            return ExitCode::from(1);
        }
    }
    if !stderr.is_empty() {
        eprintln!("check: stderr is not empty");
        return ExitCode::from(1);
    }
    if !report.ends_with(b"\n") {
        eprintln!("check: report has no trailing newline");
        return ExitCode::from(1);
    }
    let text = String::from_utf8(report).expect("utf8");
    for (name, sha) in [
        ("query_pg", query_sha.as_str()),
        ("adapter", adapter_sha.as_str()),
        ("comparator", comparator_sha.as_str()),
    ] {
        if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            eprintln!("check: digest shape");
            return ExitCode::from(2);
        }
        let needle = format!("\"{name}\":\"{sha}\"");
        if !text.contains(&needle) {
            eprintln!("check: executable digest missing");
            return ExitCode::from(1);
        }
    }
    let expect = fs::read_to_string(&expect_path).expect("expect");
    let mut passes = None;
    let mut fails = None;
    let mut timeouts = None;
    let mut reannotations = None;
    let mut not_runs = None;
    let mut trials = None;
    for line in expect.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("trial ") {
            if let Err(_message) = check_trial(&text, rest) {
                eprintln!("check: trial field mismatch");
                return ExitCode::from(1);
            }
            continue;
        }
        let (key, want) = line.split_once('=').expect("expect line");
        let got = match key {
            "passed" => field(&text, "passed"),
            "questions" => field(&text, "question_count"),
            "trials" => field(&text, "trial_count"),
            "passes" => field(&text, "pass_count"),
            "fails" => field(&text, "fail_count"),
            "timeouts" => field(&text, "timeout_count"),
            "reannotations" => field(&text, "reannotation_count"),
            "not_runs" => field(&text, "not_run_count"),
            "lane" => field(&text, "lane"),
            "metadata" => field(&text, "metadata"),
            "mode" => field(&text, "mode"),
            _ => {
                eprintln!("check: unknown expect key");
                return ExitCode::from(2);
            }
        };
        if got != want {
            eprintln!("check: count mismatch");
            return ExitCode::from(1);
        }
        match key {
            "passes" => passes = want.parse::<u64>().ok(),
            "fails" => fails = want.parse::<u64>().ok(),
            "timeouts" => timeouts = want.parse::<u64>().ok(),
            "reannotations" => reannotations = want.parse::<u64>().ok(),
            "not_runs" => not_runs = want.parse::<u64>().ok(),
            "trials" => trials = want.parse::<u64>().ok(),
            _ => {}
        }
    }
    if passes.zip(fails).zip(timeouts).zip(reannotations).zip(not_runs).zip(trials)
        .is_some_and(|(((((passes, fails), timeouts), reannotations), not_runs), trials)| {
            passes + fails + timeouts + reannotations + not_runs != trials
        })
    {
        eprintln!("check: denominator");
        return ExitCode::from(1);
    }
    ExitCode::from(0)
}

fn check_trial(text: &str, spec: &str) -> Result<(), &'static str> {
    let mut parts = spec.split_whitespace();
    let id = parts.next().ok_or("trial id")?;
    let index = parts.next().ok_or("trial index")?;
    let mut want = BTreeMap::new();
    for item in parts {
        let (key, value) = item.split_once('=').ok_or("trial field")?;
        want.insert(key, value);
    }
    let marker = format!("\"case_id\":\"{id}\"");
    let mut from = 0;
    while let Some(relative) = text[from..].find(&marker) {
        let at = from + relative;
        let start = text[..at].rfind('{').ok_or("trial object")?;
        let end = text[at..]
            .find('}')
            .map(|offset| at + offset)
            .ok_or("trial end")?;
        let object = &text[start..=end];
        if field(object, "index") == index {
            for (key, value) in want {
                if field(object, key) != value {
                    return Err("trial field mismatch");
                }
            }
            return Ok(());
        }
        from = end + 1;
    }
    Err("missing trial")
}

fn field(text: &str, key: &str) -> String {
    let marker = format!("\"{key}\":");
    let Some(at) = text.find(&marker) else {
        return String::new();
    };
    let rest = &text[at + marker.len()..];
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"').unwrap_or(0);
        stripped[..end].to_owned()
    } else {
        rest.split([',', '}'])
            .next()
            .unwrap_or("")
            .to_owned()
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
