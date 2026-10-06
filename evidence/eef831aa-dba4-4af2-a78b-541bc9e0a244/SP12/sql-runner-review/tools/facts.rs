//! Safe facts from a finished run directory. No cell text, gold, or secrets.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let secret_path = args.next().expect("secret");
    let mut secret = fs::read(&secret_path).expect("secret");
    if secret.ends_with(b"\n") {
        secret.pop();
    }
    if secret.ends_with(b"\r") {
        secret.pop();
    }
    if secret.is_empty() {
        eprintln!("facts: empty secret");
        return ExitCode::from(2);
    }
    let runs: Vec<String> = args.collect();
    if runs.is_empty() {
        eprintln!("facts: no runs");
        return ExitCode::from(2);
    }
    let mut scanned = 0_u64;
    let mut secret_hits = 0_u64;
    let mut gold_hits = 0_u64;
    for run in &runs {
        let root = Path::new(run);
        if !root.is_dir() {
            println!("run_missing=1");
            continue;
        }
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(_) => {
                println!("run_unreadable=1");
                continue;
            }
        };
        for entry in entries.flatten() {
            let case = entry.path();
            if !case.is_dir() {
                continue;
            }
            let trials = match fs::read_dir(&case) {
                Ok(trials) => trials,
                Err(_) => continue,
            };
            for trial in trials.flatten() {
                let request = trial.path().join("request.json");
                if !request.is_file() {
                    continue;
                }
                let bytes = fs::read(&request).unwrap_or_default();
                scanned += 1;
                if contains(&bytes, &secret) {
                    secret_hits += 1;
                }
                if contains(&bytes, b"/evidence/built/gold") {
                    gold_hits += 1;
                }
            }
        }
    }
    println!("requests_scanned={scanned}");
    println!("requests_with_secret={secret_hits}");
    println!("requests_with_gold_path={gold_hits}");
    let left = Path::new(&runs[0]).join("case-exact/trial-0/request.json");
    let right = Path::new(&runs[0]).join("case-exact/trial-1/request.json");
    match (fs::read(&left), fs::read(&right)) {
        (Ok(left_bytes), Ok(right_bytes)) => {
            println!(
                "pass_exact_requests_differ={}",
                if left_bytes != right_bytes { "yes" } else { "no" }
            );
        }
        _ => println!("pass_exact_requests_differ=absent"),
    }
    let compare = Path::new(&runs[1]).join("case-trunc/trial-0/comparator.stdout");
    match fs::read(&compare) {
        Ok(bytes) => {
            let matched = contains(&bytes, b"\"verdict\":\"match\"");
            let banned = contains(&bytes, b"CHANGED-CELL-a244")
                || contains(&bytes, "\u{96ea}".as_bytes())
                || contains(&bytes, b"beta");
            println!("trunc_comparator_verdict={}", if matched { "match" } else { "other" });
            println!("trunc_comparator_has_cell={}", if banned { "yes" } else { "no" });
            println!("trunc_comparator_bytes={}", bytes.len());
        }
        Err(_) => println!("trunc_comparator_verdict=absent"),
    }
    let audit = Path::new(&runs[4]).join("case-noend/trial-0/audit.jsonl");
    match fs::read(&audit) {
        Ok(bytes) if bytes == b"keep\n" => println!("noend_audit=keep"),
        Ok(bytes) => println!("noend_audit_bytes={}", bytes.len()),
        Err(_) => println!("noend_audit=absent"),
    }
    if secret_hits != 0 || gold_hits != 0 {
        return ExitCode::from(1);
    }
    ExitCode::from(0)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}
