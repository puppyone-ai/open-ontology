//! Fail when the tool lock has a git source or a checksum absent from the
//! committed root lock. The tool lock is the only other input.
//! Origin: evidence/.../a301/SP04/cdc-apply-pg/linux/proof/lock_subset.rs
//! Dropped spool and session arguments. Checksum lines stay `checksum = "`.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(tool) = args.next() else {
        eprintln!("usage: lock_subset TOOL ROOT");
        return ExitCode::from(2);
    };
    let Some(root) = args.next() else {
        eprintln!("missing root lock");
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
    let root_text = match fs::read_to_string(&root) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("read root lock: {err}");
            return ExitCode::from(2);
        }
    };
    let allowed = match checksums(&root_text) {
        Ok(sums) => sums.into_iter().collect::<BTreeSet<_>>(),
        Err(err) => {
            eprintln!("root lock: {err}");
            return ExitCode::from(1);
        }
    };
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
        let Some(rest) = line.trim().strip_prefix("checksum = \"") else {
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
