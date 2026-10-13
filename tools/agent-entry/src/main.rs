#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

use agent_entry::{load_config, serve, startup_gate};

fn main() -> ExitCode {
    ExitCode::from(run())
}

fn run() -> u8 {
    let mut args = std::env::args();
    let _program = args.next();
    let Some(command) = args.next() else {
        return 1;
    };
    if args.next().is_some() || command != "serve" {
        eprintln!("usage");
        return 1;
    }
    let mut stdin = Vec::new();
    if io::stdin().take(1_048_577).read_to_end(&mut stdin).is_err() || stdin.len() > 1_048_576 {
        stdin.fill(0);
        return 1;
    }
    let config = match load_config(&stdin) {
        Ok(config) => {
            stdin.fill(0);
            config
        }
        Err(fault) => {
            stdin.fill(0);
            eprintln!("{}", fault.public());
            return fault.exit_code();
        }
    };
    if let Err(reason) = startup_gate(&config.profile) {
        eprintln!("{reason}");
        return 2;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => return 3,
    };
    match runtime.block_on(serve(config)) {
        Ok(()) => 0,
        Err(fault) => {
            eprintln!("{}", fault.public());
            let _ = io::stderr().flush();
            fault.exit_code()
        }
    }
}
