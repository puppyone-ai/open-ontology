//! `source-control-api <install|serve>`.
//!
//! Passwords stay in files. Stdout is one JSON object.

#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use serde_json::{json, Value};
use source_control_api::{install, prepare, run, Fault, Install, CONTRACT};

fn main() -> ExitCode {
    match runtime() {
        Ok(()) => ExitCode::from(0),
        Err(code) => ExitCode::from(code),
    }
}

fn runtime() -> Result<(), u8> {
    let command = parse_args().map_err(|_| 1_u8)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| 2_u8)?;
    match command {
        Command::Install(path) => {
            let config = source_control_api::config::Config::load(&path).map_err(fault_code)?;
            let report = runtime.block_on(install(&config)).map_err(fault_code)?;
            print_json(&install_body(&report))
        }
        Command::Serve(path) => {
            let config = Arc::new(source_control_api::config::Config::load(&path).map_err(fault_code)?);
            let prepared = runtime.block_on(prepare(Arc::clone(&config))).map_err(fault_code)?;
            print_json(&json!({
                "contract": CONTRACT,
                "outcome": "listening",
                "addr": prepared.addr,
                "data_grants": "none",
                "owner_certification": "not_issued",
                "agent_session": "not_issued",
                "harness_session": "not_issued",
                "gateway_token": "not_issued",
                "build_review": "not_claimed",
                "release": "not_claimed",
                "customer_integration": "not_claimed"
            }))?;
            runtime.block_on(run(prepared)).map_err(fault_code)
        }
    }
}

enum Command {
    Install(PathBuf),
    Serve(PathBuf),
}

fn parse_args() -> Result<Command, ()> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(())?;
    let flag = args.next().ok_or(())?;
    let path = args.next().ok_or(())?;
    if flag != "--config" || args.next().is_some() {
        return Err(());
    }
    match command.as_str() {
        "install" => Ok(Command::Install(PathBuf::from(path))),
        "serve" => Ok(Command::Serve(PathBuf::from(path))),
        _ => Err(()),
    }
}

fn install_body(report: &Install) -> Value {
    let disposition = match report {
        Install::Committed => "committed",
        Install::Replayed => "replayed",
    };
    json!({
        "contract": CONTRACT,
        "outcome": "installed",
        "disposition": disposition,
        "sql_sha256": source_control_api::sha256::sha256_hex(include_str!("../migrations/oo_source_control_v1.sql").as_bytes()),
        "data_grants": "none",
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    })
}

fn fault_code(fault: Fault) -> u8 {
    let _ = print_json(&json!({
        "contract": CONTRACT,
        "outcome": match fault {
            Fault::Malformed => "malformed",
            Fault::Refused => "refused",
            Fault::Commit => "commit_failed",
        },
        "data_grants": "none",
        "build_review": "not_claimed",
        "release": "not_claimed",
        "customer_integration": "not_claimed"
    }));
    u8::try_from(fault.code()).unwrap_or(1)
}

fn print_json(value: &Value) -> Result<(), u8> {
    let text = serde_json::to_string(value).map_err(|_| 2_u8)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{text}").map_err(|_| 2_u8)?;
    stdout.flush().map_err(|_| 2_u8)?;
    Ok(())
}
