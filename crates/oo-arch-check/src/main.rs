#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use oo_arch_check::{
    check, default_workspace_manifest, demonstrate_forbidden_gateway_edge, load_workspace_metadata,
};

fn main() -> ExitCode {
    let mut manifest = default_workspace_manifest();
    let mut demonstrate = false;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demonstrate-negative" => demonstrate = true,
            "--manifest" => match args.next() {
                Some(path) => manifest = PathBuf::from(path),
                None => {
                    eprintln!("oo-arch-check: --manifest requires a path");
                    return ExitCode::from(2);
                }
            },
            "--help" | "-h" => {
                println!(
                    "oo-arch-check [--manifest PATH] [--demonstrate-negative]\n\
                     Check workspace dependency direction against cargo metadata.\n\
                     --demonstrate-negative mutates an in-memory copy, requires that copy to fail,\n\
                     then checks the original metadata again."
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("oo-arch-check: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }

    let metadata = match load_workspace_metadata(&manifest) {
        Ok(metadata) => metadata,
        Err(error) => {
            eprintln!("oo-arch-check: failed to read cargo metadata: {error}");
            return ExitCode::from(1);
        }
    };

    if demonstrate {
        let demonstration = demonstrate_forbidden_gateway_edge(&metadata);
        println!("{demonstration}");
        return if demonstration.succeeded() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        };
    }

    let report = check(&metadata);
    println!("{report}");
    if report.is_pass() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
