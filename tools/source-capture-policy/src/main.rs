//! `source-capture-policy plan <request.json>` writes one decision object.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use source_capture_policy::plan_bytes;
use source_capture_policy::CONTRACT;

const MAX_BYTES: u64 = 1024 * 1024;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return usage();
    };
    if command != "plan" {
        return usage();
    }
    let Some(path) = args.next() else {
        return usage();
    };
    if args.next().is_some() {
        return usage();
    }
    match read_plan(Path::new(&path)) {
        Ok(decision) => {
            let mut out = io::stdout().lock();
            if serde_json::to_writer(&mut out, &decision).is_err() || writeln!(out).is_err() {
                return ExitCode::from(1);
            }
            ExitCode::from(0)
        }
        Err(code) => {
            let body = serde_json::json!({
                "contract": CONTRACT,
                "kind": "refused",
                "fault": code,
                "downgraded": false,
                "loader_integration": "unaccepted",
                "scheduler_integration": "unaccepted",
            });
            let mut out = io::stdout().lock();
            if serde_json::to_writer(&mut out, &body).is_err() || writeln!(out).is_err() {
                return ExitCode::from(1);
            }
            ExitCode::from(2)
        }
    }
}

fn read_plan(path: &Path) -> Result<source_capture_policy::Decision, &'static str> {
    let meta = fs::metadata(path).map_err(|_| "unreadable")?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err("unreadable");
    }
    let bytes = fs::read(path).map_err(|_| "unreadable")?;
    plan_bytes(&bytes)
}

fn usage() -> ExitCode {
    let _ = writeln!(io::stderr(), "usage: source-capture-policy plan <request.json>");
    ExitCode::from(1)
}
