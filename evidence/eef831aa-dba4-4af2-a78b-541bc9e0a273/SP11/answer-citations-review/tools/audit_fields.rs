//! Field inventory for the query-pg audit structs.
//!
//! AdmitLine and CompleteLine must be the producer receipt grammar.
//! They must not carry query_id, session, turn, or principal.
//! SummaryLine is printed only so a stdout turn index is not mistaken
//! for an audit field. This file does not decide citation admission.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        eprintln!("audit-fields: usage");
        return ExitCode::from(2);
    };
    let Ok(text) = fs::read_to_string(&path) else {
        eprintln!("audit-fields: unreadable");
        return ExitCode::from(1);
    };
    let Ok(admit) = fields(&text, "AdmitLine") else {
        eprintln!("audit-fields: admit");
        return ExitCode::from(1);
    };
    let Ok(complete) = fields(&text, "CompleteLine") else {
        eprintln!("audit-fields: complete");
        return ExitCode::from(1);
    };
    let summary = fields(&text, "SummaryLine").unwrap_or_default();
    println!("admit={}", join(&admit));
    println!("complete={}", join(&complete));
    println!("summary={}", join(&summary));
    if summary.contains("turn") {
        println!("summary_turn=stdout_only");
    } else {
        println!("summary_turn=absent");
    }
    let admit_expect = set(&[
        "protocol",
        "record",
        "original_sha256",
        "executed_sha256",
        "fingerprint_hex",
        "role",
        "project",
        "policy_id",
        "backend_pid",
        "backend_start",
    ]);
    let mut complete_expect = admit_expect.clone();
    for name in [
        "status",
        "truncated",
        "returned_rows",
        "result_bytes",
        "rollback_confirmed",
        "socket_dropped",
        "stop",
        "sqlstate",
    ] {
        complete_expect.insert(name.to_owned());
    }
    let mut bad = 0_u32;
    if admit != admit_expect {
        eprintln!("audit-fields: admit set");
        bad = bad.saturating_add(1);
    }
    if complete != complete_expect {
        eprintln!("audit-fields: complete set");
        bad = bad.saturating_add(1);
    }
    for name in [
        "query_id",
        "session",
        "session_id",
        "turn",
        "turn_id",
        "principal",
        "authenticated",
    ] {
        if admit.contains(name) || complete.contains(name) {
            eprintln!("audit-fields: forbidden {name}");
            bad = bad.saturating_add(1);
        }
    }
    if bad == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn fields(text: &str, name: &str) -> Result<BTreeSet<String>, ()> {
    let marker = format!("struct {name} {{");
    let start = text.find(&marker).ok_or(())? + marker.len();
    let rest = &text[start..];
    let mut depth = 1_i32;
    let mut end = None;
    for (index, byte) in rest.bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(index);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &rest[..end.ok_or(())?];
    let mut set = BTreeSet::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }
        let Some(field) = trimmed.split(':').next() else {
            continue;
        };
        let field = field.trim();
        if field.is_empty()
            || !field
                .chars()
                .all(|char| char.is_ascii_alphanumeric() || char == '_')
        {
            return Err(());
        }
        if !set.insert(field.to_owned()) {
            return Err(());
        }
    }
    if set.is_empty() { Err(()) } else { Ok(set) }
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn join(names: &BTreeSet<String>) -> String {
    names.iter().cloned().collect::<Vec<_>>().join(",")
}
