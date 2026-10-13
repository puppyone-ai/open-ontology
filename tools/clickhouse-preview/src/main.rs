//! Write the frozen admission decision for this ClickHouse source.
//!
//! Exit 0 means the library admitted the report. Exit 3 means it denied the
//! report. A denial does not start an import, a model call, or a role pack.

use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut digest = None;
    let mut out = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--driver-digest" => digest = args.next(),
            "--out" => out = args.next(),
            _ => return usage(),
        }
    }
    let (Some(digest), Some(out)) = (digest, out) else {
        return usage();
    };
    if digest.is_empty() || digest.contains("://") || digest.to_ascii_lowercase().contains("password") {
        eprintln!("clickhouse-preview: driver digest is empty or looks like a secret");
        return ExitCode::from(2);
    }
    let decision = clickhouse_preview::decision_for(&digest);
    let bytes = match source_admission::decision_json(&decision) {
        Ok(bytes) => bytes,
        Err(_) => {
            eprintln!("clickhouse-preview: decision could not be encoded");
            return ExitCode::from(1);
        }
    };
    if fs::write(&out, bytes).is_err() {
        eprintln!("clickhouse-preview: decision file could not be written");
        return ExitCode::from(1);
    }
    if decision.admitted {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}

fn usage() -> ExitCode {
    eprintln!("clickhouse-preview --driver-digest SHA256 --out decision.json");
    ExitCode::from(2)
}
