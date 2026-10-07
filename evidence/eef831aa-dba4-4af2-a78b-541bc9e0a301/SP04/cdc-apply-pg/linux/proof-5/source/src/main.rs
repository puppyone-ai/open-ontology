//! `cdc-apply-pg apply <request.json>`
//!
//! The password stays in the file named by the request. It is not an argument.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::process::ExitCode;

use cdc_apply_pg::{apply, Request};

fn main() -> ExitCode {
    let code = match runtime() {
        Ok(code) => code,
        Err(()) => 1,
    };
    ExitCode::from(code)
}

fn runtime() -> Result<u8, ()> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(())?;
    if command != "apply" {
        return Err(());
    }
    let path = args.next().ok_or(())?;
    if args.next().is_some() {
        return Err(());
    }
    let bytes = fs::read(path).map_err(|_| ())?;
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let output = tokio_run(request).map_err(|_| ())?;
    let line = serde_json::to_string(&output).map_err(|_| ())?;
    println!("{line}");
    Ok(match output.kind {
        "applied" => 0,
        "refused" => 2,
        "commit_unknown" => 3,
        _ => 1,
    })
}

fn tokio_run(request: Request) -> Result<cdc_apply_pg::Output, ()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| ())?;
    Ok(runtime.block_on(apply(&request)))
}
