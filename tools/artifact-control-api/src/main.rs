//! `artifact-control install|serve --config <file>`.
//!
//! Passwords, the AEAD key, and TLS private keys stay in files.

#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use artifact_control_api::{bound_addr, error_body, listen, load_config, serve, Fault, CONTRACT};
use serde::Serialize;
use serde_json::json;

enum Command {
    Install(PathBuf),
    Serve(PathBuf),
}

enum Run {
    Usage,
    Fault(Fault),
}

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|info| {
        let _ = writeln!(io::stderr(), "artifact-control panic: {info}");
        let _ = io::stderr().flush();
    }));
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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| Run::Fault(Fault::not_ready()))?;
    match command {
        Command::Install(path) => {
            let config = load_config(&path).map_err(Run::Fault)?;
            let report = runtime.block_on(artifact_control_api::Store::install(&config)).map_err(Run::Fault)?;
            print_json(&report)
        }
        Command::Serve(path) => {
            let config = Arc::new(load_config(&path).map_err(Run::Fault)?);
            let listener = runtime.block_on(listen(&config)).map_err(Run::Fault)?;
            let addr = bound_addr(&listener).map_err(Run::Fault)?;
            print_json(&json!({
                "contract": CONTRACT,
                "outcome": "listening",
                "listen": addr.to_string(),
                "signed_url": null,
                "workspace": "disabled",
                "production_signed": false,
                "runsc": "not_claimed",
                "customer_integration": "not_claimed",
                "parent_complete": false
            }))?;
            runtime.block_on(serve(config, listener)).map_err(Run::Fault)
        }
    }
}

fn parse_args() -> Result<Command, ()> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(())?;
    match command.as_str() {
        "install" => one_config(&mut args).map(Command::Install),
        "serve" => one_config(&mut args).map(Command::Serve),
        _ => Err(()),
    }
}

fn one_config(args: &mut impl Iterator<Item = String>) -> Result<PathBuf, ()> {
    if args.next().as_deref() != Some("--config") {
        return Err(());
    }
    let path = args.next().ok_or(())?;
    if args.next().is_some() {
        return Err(());
    }
    Ok(PathBuf::from(path))
}

fn print_json(value: &impl Serialize) -> Result<(), Run> {
    let text = serde_json::to_string(value).map_err(|_| Run::Fault(Fault::not_ready()))?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{text}").map_err(|_| Run::Fault(Fault::not_ready()))?;
    stdout.flush().map_err(|_| Run::Fault(Fault::not_ready()))?;
    Ok(())
}
