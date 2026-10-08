#![forbid(unsafe_code)]

use std::io::{self, Read};
use std::process::ExitCode;

use session_control_api::{install_all, issue_grant, load_config, serve, startup_gate, Fault};

fn main() -> ExitCode {
    ExitCode::from(run())
}

fn run() -> u8 {
    let mut args = std::env::args();
    let _program = args.next();
    let Some(command) = args.next() else {
        return 1;
    };
    if args.next().is_some() || !matches!(command.as_str(), "install" | "issue-grant" | "serve") {
        return 1;
    }
    let mut stdin = Vec::new();
    if io::stdin().take(1_048_577).read_to_end(&mut stdin).is_err() || stdin.len() > 1_048_576 {
        wipe(&mut stdin);
        return 1;
    }
    let config = match load_config(&stdin) {
        Ok(config) => {
            wipe(&mut stdin);
            config
        }
        Err(fault) => {
            wipe(&mut stdin);
            eprintln!("{}", fault.reason());
            return exit_for(&fault);
        }
    };
    if command == "serve"
        && let Err(reason) = startup_gate(&config.profile)
    {
        eprintln!("{reason}");
        return 2;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => return 3,
    };
    let result = match command.as_str() {
        "install" => runtime.block_on(install_all(&config)).map(|line| {
            println!("{line}");
        }),
        "issue-grant" => runtime.block_on(issue_grant(&config)).map(|line| {
            println!("{line}");
        }),
        "serve" => runtime.block_on(serve(config)),
        _ => Err(Fault::Malformed("command")),
    };
    match result {
        Ok(()) => 0,
        Err(fault) => {
            eprintln!("{}", fault.reason());
            exit_for(&fault)
        }
    }
}

fn exit_for(fault: &Fault) -> u8 {
    match fault {
        Fault::Malformed(_) => 1,
        Fault::Unavailable(_) => 3,
        _ => 2,
    }
}

fn wipe(bytes: &mut Vec<u8>) {
    bytes.fill(0);
    std::hint::black_box(&*bytes);
    bytes.clear();
}
