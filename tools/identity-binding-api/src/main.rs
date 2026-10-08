//! `identity-binding-api <seed|serve>`.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use identity_binding_api::{error_body, listen, load_config, seed_repository, serve, Fault, CONTRACT};
use serde::Serialize;
use serde_json::json;

enum Command {
    Seed { config: PathBuf, document: PathBuf },
    Serve(PathBuf),
}

enum Run {
    Usage,
    Fault(Fault),
}

fn main() -> ExitCode {
    match runtime() {
        Ok(()) => ExitCode::from(0),
        Err(Run::Usage) => ExitCode::from(1),
        Err(Run::Fault(fault)) => {
            let _ = print_json(&error_body(&fault));
            ExitCode::from(fault.exit_code)
        }
    }
}

fn runtime() -> Result<(), Run> {
    let command = parse_args().map_err(|_| Run::Usage)?;
    match command {
        Command::Seed { config, document } => {
            let loaded = load_config(&config).map_err(Run::Fault)?;
            let bytes = fs::read(&document).map_err(|_| Run::Fault(Fault::malformed()))?;
            let git_commit = seed_repository(&loaded.git, &bytes).map_err(Run::Fault)?;
            print_json(&json!({
                "contract": CONTRACT,
                "disposition": "seeded",
                "git_commit": git_commit,
                "data_grants": "none",
                "owner_certification": "not_issued",
                "agent_session": "not_issued",
                "harness_session": "not_issued",
                "gateway_token": "not_issued",
                "customer_integration": "not_claimed"
            }))
        }
        Command::Serve(path) => {
            let config = Arc::new(load_config(&path).map_err(Run::Fault)?);
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_io().enable_time().build().map_err(|_| Run::Fault(Fault::unavailable()))?;
            let listener = runtime.block_on(listen(&config)).map_err(Run::Fault)?;
            let bound = listener.local_addr().map_err(|_| Run::Fault(Fault::unavailable()))?;
            print_json(&json!({
                "contract": CONTRACT,
                "outcome": "listening",
                "addr": bound.to_string(),
                "data_grants": "none",
                "owner_certification": "not_issued",
                "agent_session": "not_issued",
                "harness_session": "not_issued",
                "gateway_token": "not_issued",
                "customer_integration": "not_claimed"
            }))?;
            runtime.block_on(serve(config, listener)).map_err(Run::Fault)
        }
    }
}

fn parse_args() -> Result<Command, ()> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(())?;
    match command.as_str() {
        "seed" => {
            flag(&mut args, "--config")?;
            let config = PathBuf::from(args.next().ok_or(())?);
            flag(&mut args, "--document")?;
            let document = PathBuf::from(args.next().ok_or(())?);
            if args.next().is_some() {
                return Err(());
            }
            Ok(Command::Seed { config, document })
        }
        "serve" => {
            flag(&mut args, "--config")?;
            let path = PathBuf::from(args.next().ok_or(())?);
            if args.next().is_some() {
                return Err(());
            }
            Ok(Command::Serve(path))
        }
        _ => Err(()),
    }
}

fn flag(args: &mut impl Iterator<Item = String>, expected: &str) -> Result<(), ()> {
    if args.next().as_deref() == Some(expected) { Ok(()) } else { Err(()) }
}

fn print_json(value: &impl Serialize) -> Result<(), Run> {
    let text = serde_json::to_string(value).map_err(|_| Run::Fault(Fault::unavailable()))?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{text}").map_err(|_| Run::Fault(Fault::unavailable()))?;
    stdout.flush().map_err(|_| Run::Fault(Fault::unavailable()))
}
