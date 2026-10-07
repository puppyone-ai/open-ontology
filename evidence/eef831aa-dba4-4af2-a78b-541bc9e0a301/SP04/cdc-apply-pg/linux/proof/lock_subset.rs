//! Fail when the tool lock has a git source or a checksum outside the
//! committed root, cdc-spool, and session-store locks.
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(tool) = args.next() else {
        eprintln!("usage: lock_subset TOOL ROOT SPOOL SESSION");
        return ExitCode::from(2);
    };
    let Some(root) = args.next() else {
        eprintln!("missing root lock");
        return ExitCode::from(2);
    };
    let Some(spool) = args.next() else {
        eprintln!("missing spool lock");
        return ExitCode::from(2);
    };
    let Some(session) = args.next() else {
        eprintln!("missing session lock");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("too many arguments");
        return ExitCode::from(2);
    }
    let tool_text = match fs::read_to_string(&tool) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("read tool lock: {err}");
            return ExitCode::from(2);
        }
    };
    if let Some(line) = git_source(&tool_text) {
        eprintln!("tool lock has a git source at line {line}");
        return ExitCode::from(1);
    }
    let tool_sums = match checksums(&tool_text) {
        Ok(sums) => sums,
        Err(err) => {
            eprintln!("tool lock: {err}");
            return ExitCode::from(1);
        }
    };
    let mut allowed = BTreeSet::new();
    for path in [root, spool, session] {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("read {path}: {err}");
                return ExitCode::from(2);
            }
        };
        match checksums(&text) {
            Ok(sums) => allowed.extend(sums),
            Err(err) => {
                eprintln!("{path}: {err}");
                return ExitCode::from(1);
            }
        }
    }
    let mut missing = Vec::new();
    for sum in &tool_sums {
        if !allowed.contains(sum) {
            missing.push(sum.clone());
        }
    }
    if missing.is_empty() {
        println!("lock_subset_ok tool_checksums={}", tool_sums.len());
        ExitCode::SUCCESS
    } else {
        eprintln!("missing_checksums {}", missing.len());
        for sum in missing {
            eprintln!("{sum}");
        }
        ExitCode::from(1)
    }
}

fn git_source(text: &str) -> Option<usize> {
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("source = \"git+") {
            return Some(index + 1);
        }
    }
    None
}

fn checksums(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("checksum = \"") else {
            continue;
        };
        let Some(hex) = rest.strip_suffix('"') else {
            return Err(format!("line {}: checksum is not quoted", index + 1));
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
            return Err(format!("line {}: checksum is not 64 lowercase hex", index + 1));
        }
        out.push(hex.to_owned());
    }
    Ok(out)
}
