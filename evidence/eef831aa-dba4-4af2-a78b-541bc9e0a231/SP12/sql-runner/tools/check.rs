//! Check one sql-runner report without printing cells, gold, or secrets.

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let report_path = args.next().expect("report");
    let stderr_path = args.next().expect("stderr");
    let secret_path = args.next().expect("secret");
    let expect_path = args.next().expect("expect");
    let report = fs::read(&report_path).expect("report");
    let stderr = fs::read(&stderr_path).expect("stderr");
    let mut secret = fs::read(&secret_path).expect("secret");
    if secret.ends_with(b"\n") {
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
        b"CHANGED-CELL-a231",
        b"password_file",
        b"PRIVATE KEY",
        "雪".as_bytes(),
        b"sealed-left",
        b"sealed-mid",
        b"sealed-right",
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
    let expect = fs::read_to_string(&expect_path).expect("expect");
    for line in expect.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("trial ") {
            if let Err(message) = check_trial(&text, rest) {
                eprintln!("check: {message}");
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
            eprintln!("check: {key} mismatch");
            return ExitCode::from(1);
        }
    }
    ExitCode::from(0)
}

fn check_trial(text: &str, spec: &str) -> Result<(), &'static str> {
    let mut parts = spec.split_whitespace();
    let id = parts.next().ok_or("trial id")?;
    let mut want = std::collections::BTreeMap::new();
    for item in parts {
        let (key, value) = item.split_once('=').ok_or("trial field")?;
        want.insert(key, value);
    }
    let marker = format!("\"case_id\":\"{id}\"");
    let at = text.find(&marker).ok_or("missing trial")?;
    let start = text[..at].rfind('{').ok_or("trial object")?;
    let end = text[at..]
        .find('}')
        .map(|offset| at + offset)
        .ok_or("trial end")?;
    let object = &text[start..=end];
    for (key, value) in want {
        let got = field(object, key);
        if got != value {
            return Err("trial field mismatch");
        }
    }
    Ok(())
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
