//! Operator commands: doctor, run, ask, cancel, close.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use minimal_product_loop::{ask, cancel, close, copy, doctor, prepare, report_json, run, AskOutcome, CancelOutcome, CloseOutcome, RunOutcome};
use serde_json::json;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next();
    match command.as_deref() {
        Some("doctor") => command_config(&mut args, |path| match doctor(path) {
            Ok(report) => {
                println!("{}", serde_json::to_string(&report_json(&report)).unwrap_or_else(|_| "{}".to_owned()));
                ExitCode::from(0)
            }
            Err(error) => fail(&error),
        }),
        Some("copy") => command_config(&mut args, |path| match copy(path) {
            Ok(receipt) => { println!("{receipt}"); ExitCode::from(0) }
            Err(error) => fail(&error),
        }),
        Some("prepare") => command_config(&mut args, |path| match prepare(path) {
            Ok(receipt) => { println!("{receipt}"); ExitCode::from(0) }
            Err(error) => fail(&error),
        }),
        Some("run") => command_config(&mut args, |path| match run(path) {
            Ok(outcome) => {
                println!("{}", serde_json::to_string(&run_json(&outcome)).unwrap_or_else(|_| "{}".to_owned()));
                ExitCode::from(0)
            }
            Err(error) => fail(&error),
        }),
        Some("ask") => {
            let Some(path) = flag(&mut args, "--config") else {
                return usage();
            };
            let Some(question) = flag(&mut args, "--question") else {
                return usage();
            };
            if args.next().is_some() {
                return usage();
            }
            match ask(Path::new(&path), &question) {
                Ok(outcome) => {
                    println!("{}", serde_json::to_string(&ask_json(&outcome)).unwrap_or_else(|_| "{}".to_owned()));
                    if outcome.status == "matched" && outcome.matched && outcome.cited && !outcome.query_id.is_empty() {
                        ExitCode::from(0)
                    } else {
                        ExitCode::from(1)
                    }
                }
                Err(error) => fail(&error),
            }
        }
        Some("cancel") => command_config(&mut args, |path| match cancel(path) {
            Ok(outcome) => {
                println!("{}", serde_json::to_string(&cancel_json(&outcome)).unwrap_or_else(|_| "{}".to_owned()));
                if outcome.status == "cancelled" { ExitCode::from(0) } else { ExitCode::from(1) }
            }
            Err(error) => fail(&error),
        }),
        Some("close") => command_config(&mut args, |path| match close(path) {
            Ok(outcome) => {
                println!("{}", serde_json::to_string(&close_json(&outcome)).unwrap_or_else(|_| "{}".to_owned()));
                if outcome.status == "released" { ExitCode::from(0) } else { ExitCode::from(1) }
            }
            Err(error) => fail(&error),
        }),
        _ => usage(),
    }
}

fn command_config(args: &mut impl Iterator<Item = String>, run: impl FnOnce(&Path) -> ExitCode) -> ExitCode {
    let Some(path) = flag(args, "--config") else {
        return usage();
    };
    if args.next().is_some() {
        return usage();
    }
    run(Path::new(&path))
}

fn flag(args: &mut impl Iterator<Item = String>, name: &str) -> Option<String> {
    let next = args.next()?;
    if next != name {
        return None;
    }
    args.next()
}

fn usage() -> ExitCode {
    eprintln!("usage: minimal-product-loop <doctor|copy|prepare|run|ask|cancel|close> --config <file> [--question <text>]");
    ExitCode::from(2)
}

fn fail(error: &minimal_product_loop::ConfigError) -> ExitCode {
    let body = json!({"code": error.code, "message": error.message, "parent_acceptance": false});
    eprintln!("{}", serde_json::to_string(&body).unwrap_or_else(|_| "{\"code\":\"error\"}".to_owned()));
    ExitCode::from(exit_code(error.code))
}

fn exit_code(code: &str) -> u8 {
    match code {
        "unsupported" => 3,
        "catalog.absent" | "pi.absent" | "query_pg.absent" | "oo.absent" | "policy.absent" | "pack.absent" | "pki.absent" => 4,
        _ => 1,
    }
}

fn run_json(outcome: &RunOutcome) -> serde_json::Value {
    json!({
        "protocol": minimal_product_loop::PROTOCOL,
        "status": outcome.status,
        "source_sha256": outcome.source_sha256,
        "rows": outcome.rows,
        "question_set": outcome.question_set,
        "import_hash": outcome.import_hash,
        "selfbuild": outcome.selfbuild,
        "phases": outcome.phases,
        "publication": "not_published",
        "llm_review": "unreviewed",
        "parent_acceptance": false,
        "goal_acceptance": false
    })
}

fn ask_json(outcome: &AskOutcome) -> serde_json::Value {
    json!({
        "protocol": minimal_product_loop::PROTOCOL,
        "status": outcome.status,
        "question_id": outcome.question_id,
        "matched": outcome.matched,
        "cited": outcome.cited,
        "query_id": outcome.query_id,
        "answer": outcome.answer,
        "rows": outcome.rows,
        "tool_events": outcome.tool_events,
        "uid_boundary": outcome.uid_boundary,
        "publication": "not_published",
        "llm_review": "unreviewed",
        "parent_acceptance": false,
        "goal_acceptance": false
    })
}

fn cancel_json(outcome: &CancelOutcome) -> serde_json::Value {
    json!({
        "protocol": minimal_product_loop::PROTOCOL,
        "status": outcome.status,
        "pid_absent": outcome.pid_absent,
        "database_dropped": outcome.database_dropped,
        "database_observed": outcome.database_observed,
        "mapping": outcome.mapping,
        "publication": "not_published",
        "parent_acceptance": false,
        "goal_acceptance": false
    })
}

fn close_json(outcome: &CloseOutcome) -> serde_json::Value {
    json!({
        "protocol": minimal_product_loop::PROTOCOL,
        "status": outcome.status,
        "pid_recorded": outcome.pid_recorded,
        "pid_absent": outcome.pid_absent,
        "database_dropped": outcome.database_dropped,
        "database_observed": outcome.database_observed,
        "mapping": outcome.mapping,
        "residue_absent": outcome.residue_absent,
        "publication": "not_published",
        "parent_acceptance": false,
        "goal_acceptance": false
    })
}

#[cfg(test)]
mod tests {
    use super::run_json;
    use minimal_product_loop::RunOutcome;
    use serde_json::json;

    #[test]
    fn run_json_keeps_the_producer_phases() {
        let phases = json!([
            {"phase": "discover", "event": "begin", "source_connection": "sealed"},
            {"phase": "copy", "event": "complete", "failure": "none"}
        ]);
        let outcome = RunOutcome {
            status: "draft_ready".to_owned(),
            source_sha256: "ab".to_owned(),
            rows: json!({}),
            selfbuild: "generated_unreviewed".to_owned(),
            question_set: "bulk".to_owned(),
            import_hash: "cd".to_owned(),
            phases: phases.clone(),
        };
        let body = run_json(&outcome);
        assert_eq!(body.get("phases"), Some(&phases));
        assert_eq!(body.get("goal_acceptance").and_then(serde_json::Value::as_bool), Some(false));
        assert_eq!(body.get("parent_acceptance").and_then(serde_json::Value::as_bool), Some(false));
    }
}
