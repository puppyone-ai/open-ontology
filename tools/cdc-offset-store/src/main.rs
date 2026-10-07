//! `cdc-offset-store install <request.json>`
//! `cdc-offset-store mirror <request.json>`
//!
//! Passwords stay in the files named by the request. They are not arguments.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use cdc_offset_store::{install, mirror, InstallOutput, InstallRequest, Output, Request};

fn main() -> ExitCode {
    match runtime() {
        Ok(code) => ExitCode::from(code),
        Err(()) => ExitCode::from(1),
    }
}

fn runtime() -> Result<u8, ()> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(())?;
    let path = args.next().ok_or(())?;
    if args.next().is_some() {
        return Err(());
    }
    let bytes = fs::read(path).map_err(|_| ())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| ())?;
    match command.as_str() {
        "install" => {
            let request: InstallRequest = match serde_json::from_slice(&bytes) {
                Ok(request) => request,
                Err(_) => return refused_input(&InstallOutput::refused("input")),
            };
            let output = runtime.block_on(install(&request));
            let line = serde_json::to_string(&output).map_err(|_| ())?;
            println!("{line}");
            Ok(match output.kind.as_str() {
                "installed" | "replay" => 0,
                "refused" => 2,
                _ => 1,
            })
        }
        "mirror" => {
            let request: Request = match serde_json::from_slice(&bytes) {
                Ok(request) => request,
                Err(_) => return refused_input(&Output::refused("input")),
            };
            let output = runtime.block_on(mirror(&request));
            let line = serde_json::to_string(&output).map_err(|_| ())?;
            println!("{line}");
            Ok(match output.kind.as_str() {
                "mirrored" | "replay" => 0,
                "refused" => 2,
                "commit_unknown" => 3,
                _ => 1,
            })
        }
        _ => Err(()),
    }
}

fn refused_input(output: &impl serde::Serialize) -> Result<u8, ()> {
    let line = serde_json::to_string(output).map_err(|_| ())?;
    println!("{line}");
    Ok(2)
}
