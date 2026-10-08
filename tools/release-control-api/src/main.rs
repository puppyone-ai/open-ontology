//! `release-control-api install|serve`.
//!
//! Passwords, the CA, and TLS keys stay in files named by the config.

#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use release_control_api::{error_body, install, listen, load_config, serve, Fault};

enum Command {
    Install(PathBuf),
    Serve(PathBuf),
}

fn main() -> ExitCode {
    match runtime() {
        Ok(()) => ExitCode::from(0),
        Err(fault) => {
            let _ = print_json(&error_body(&fault));
            ExitCode::from(fault.exit_code)
        }
    }
}

fn runtime() -> Result<(), Fault> {
    let command = parse_args()?;
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_io().enable_time().build().map_err(|_| Fault::refused("dependency.not_ready"))?;
    match command {
        Command::Install(path) => {
            let config = load_config(&path)?;
            let report = runtime.block_on(install(&config))?;
            print_json(&report)
        }
        Command::Serve(path) => {
            let config = Arc::new(load_config(&path)?);
            let listener = runtime.block_on(listen(&config))?;
            runtime.block_on(serve(config, listener))
        }
    }
}

fn parse_args() -> Result<Command, Fault> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(Fault::malformed())?;
    let config = one_config(&mut args)?;
    match command.as_str() {
        "install" => Ok(Command::Install(config)),
        "serve" => Ok(Command::Serve(config)),
        _ => Err(Fault::malformed()),
    }
}

fn one_config(args: &mut impl Iterator<Item = String>) -> Result<PathBuf, Fault> {
    if args.next().as_deref() != Some("--config") {
        return Err(Fault::malformed());
    }
    let path = args.next().ok_or(Fault::malformed())?;
    if args.next().is_some() || path.is_empty() {
        return Err(Fault::malformed());
    }
    Ok(PathBuf::from(path))
}

fn print_json(value: &serde_json::Value) -> Result<(), Fault> {
    let text = serde_json::to_string(value).map_err(|_| Fault::malformed())?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{text}").map_err(|_| Fault::refused("dependency.not_ready"))?;
    stdout.flush().map_err(|_| Fault::refused("dependency.not_ready"))?;
    Ok(())
}
