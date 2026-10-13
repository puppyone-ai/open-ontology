use std::path::PathBuf;
use std::process::ExitCode;

use source_conformance::audit::{parse_audit, SCHEMA as AUDIT_SCHEMA};
use source_conformance::cases::{Applies, CASES, CERTIFIED_CASES};
use source_conformance::gitobj::GitStore;
use source_conformance::scope::ScopeInput;
use source_conformance::{run_paths, run_scope, Request};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "map" {
        return print_json(&map_body(), 0);
    }
    if args.first().map(String::as_str) == Some("audit") {
        return audit_cmd(&args);
    }
    if args.first().map(String::as_str) == Some("activate") {
        return activate_cmd(&args);
    }
    if args.first().map(String::as_str) == Some("scope") {
        return scope_cmd(&args);
    }
    match check(&args) {
        Ok(report) => {
            let code = report.exit_code;
            print_json(&report, code)
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn print_json(value: &impl serde::Serialize, code: u8) -> ExitCode {
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            println!("{text}");
            ExitCode::from(code)
        }
        Err(error) => {
            eprintln!("source-conformance: report encoding failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn map_body() -> serde_json::Value {
    let cases: Vec<_> = CASES
        .iter()
        .map(|case| {
            let applies = match case.applies {
                Applies::Always => "all".to_owned(),
                Applies::Caps(names) => names.join(","),
                Applies::DocumentFamily => "document".to_owned(),
            };
            serde_json::json!({ "id": case.id, "name": case.name, "applies": applies })
        })
        .collect();
    serde_json::json!({
        "schema": "oo-source-conformance/case-map/v1",
        "cases": cases,
        "certified_cases": CERTIFIED_CASES,
    })
}

fn check(args: &[String]) -> Result<source_conformance::Report, String> {
    if args.first().map(String::as_str) != Some("check") {
        return Err(usage());
    }
    let mut matrix = None;
    let mut packages = Vec::new();
    let mut nightly = None;
    let mut historical = None;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args.get(index + 1).ok_or_else(usage)?;
        match flag {
            "--matrix" => matrix = Some(PathBuf::from(value)),
            "--package" => packages.push(PathBuf::from(value)),
            "--nightly" => nightly = Some(PathBuf::from(value)),
            "--historical-pg" => historical = Some(PathBuf::from(value)),
            _ => return Err(usage()),
        }
        index += 2;
    }
    let report = run_paths(&Request {
        matrix_path: matrix.ok_or_else(usage)?,
        package_dirs: packages,
        nightly_path: nightly,
        historical_pg: historical,
    });
    Ok(report)
}

fn scope_cmd(args: &[String]) -> ExitCode {
    let mut repo = None;
    let mut dispatch = None;
    let mut intake = None;
    let mut matrix = None;
    let mut frontier = None;
    let mut claim = None;
    let mut packages = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = match args.get(index + 1) {
            Some(value) => value,
            None => return usage_exit(),
        };
        match flag {
            "--repo" => repo = Some(PathBuf::from(value)),
            "--dispatch" => dispatch = Some(PathBuf::from(value)),
            "--intake" => intake = Some(PathBuf::from(value)),
            "--matrix" => matrix = Some(PathBuf::from(value)),
            "--frontier" => frontier = Some(PathBuf::from(value)),
            "--claim" => claim = Some(PathBuf::from(value)),
            "--package" => packages.push(PathBuf::from(value)),
            _ => return usage_exit(),
        }
        index += 2;
    }
    let (Some(repo), Some(dispatch), Some(intake), Some(matrix)) = (repo, dispatch, intake, matrix) else {
        return usage_exit();
    };
    let dispatch_json = match std::fs::read_to_string(&dispatch) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("source-conformance: dispatch: {error}");
            return ExitCode::from(2);
        }
    };
    let intake_json = match std::fs::read_to_string(&intake) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("source-conformance: intake: {error}");
            return ExitCode::from(2);
        }
    };
    let matrix_yaml = match std::fs::read_to_string(&matrix) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("source-conformance: matrix: {error}");
            return ExitCode::from(2);
        }
    };
    let frontier_json = match frontier {
        None => None,
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) => {
                eprintln!("source-conformance: frontier: {error}");
                return ExitCode::from(2);
            }
        },
    };
    let claim_json = match claim {
        None => None,
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) => {
                eprintln!("source-conformance: claim: {error}");
                return ExitCode::from(2);
            }
        },
    };
    let (package_ok, package_error_codes) = if packages.is_empty() {
        (None, Vec::new())
    } else {
        let report = run_paths(&Request {
            matrix_path: matrix,
            package_dirs: packages,
            nightly_path: None,
            historical_pg: None,
        });
        let codes = report.findings.iter().filter(|finding| finding.severity == "error").map(|finding| finding.code.clone()).collect();
        (Some(report.ok), codes)
    };
    let store = GitStore { repo: &repo };
    let report = run_scope(
        &ScopeInput {
            dispatch_json,
            intake_json,
            matrix_yaml,
            frontier_json,
            claim_json,
            package_ok,
            package_error_codes,
        },
        &store,
    );
    let code = report.exit_code;
    print_json(&report, code)
}

fn audit_cmd(args: &[String]) -> ExitCode {
    if args.len() != 3 || args[1] != "--raw" {
        return usage_exit();
    }
    let bytes = match std::fs::read(&args[2]) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("source-conformance: audit: {error}");
            return ExitCode::from(2);
        }
    };
    let outcome = parse_audit(&bytes);
    let body = serde_json::json!({
        "schema": AUDIT_SCHEMA,
        "ok": outcome.ok && outcome.zero_write,
        "zero_write": outcome.zero_write,
        "product_accepted": outcome.product_accepted,
        "counts_as_cc02_pass": outcome.counts_as_cc02_pass,
        "statements": outcome.statements,
        "findings": outcome.codes,
    });
    let code = if outcome.ok && outcome.zero_write { 0 } else { 1 };
    print_json(&body, code)
}

fn activate_cmd(args: &[String]) -> ExitCode {
    if args.len() != 3 || args[1] != "--scope-report" {
        return usage_exit();
    }
    let text = match std::fs::read_to_string(&args[2]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("source-conformance: activate: {error}");
            return ExitCode::from(2);
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("source-conformance: activate: {error}");
            return ExitCode::from(2);
        }
    };
    match source_conformance::activate::validate_activation(&value) {
        Ok(body) => print_json(&body, 0),
        Err(error) => {
            eprintln!("source-conformance: {error}");
            ExitCode::from(1)
        }
    }
}

fn usage_exit() -> ExitCode {
    eprintln!("{}", usage());
    ExitCode::from(2)
}

fn usage() -> String {
    "usage: source-conformance check --matrix FILE [--historical-pg DIR] [--nightly FILE] [--package DIR]...\n       source-conformance scope --repo DIR --dispatch FILE --intake FILE --matrix FILE [--frontier FILE] [--claim FILE] [--package DIR]...\n       source-conformance audit --raw FILE\n       source-conformance activate --scope-report FILE\n       source-conformance map".to_owned()
}
