//! `review-ledger <install|enqueue|decide|batch|receipt|history> <request.json>`
//!
//! Passwords and credentials stay in the files named by the request.

#![forbid(unsafe_code)]

use std::env;
use std::process::ExitCode;

use review_ledger::{
    batch, decide, enqueue, history, install, parse_request, read_request_file, receipt,
    screen_request, BatchRequest, DecideRequest, EnqueueRequest, HistoryRequest, InstallRequest,
    Output, ReceiptRequest,
};

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
    let bytes = match read_request_file(&path) {
        Ok(bytes) => bytes,
        Err(error) => return print_output(&Output::from_error(&error)),
    };
    let value = match screen_request(&bytes) {
        Ok(value) => value,
        Err(error) => return print_output(&Output::from_error(&error)),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| ())?;
    let output = match command.as_str() {
        "install" => {
            let request: InstallRequest = match parse_request(value) {
                Ok(request) => request,
                Err(error) => return print_output(&Output::from_error(&error)),
            };
            match runtime.block_on(install(&request)) {
                Ok(output) => output,
                Err(error) => Output::from_error(&error),
            }
        }
        "enqueue" => {
            let request: EnqueueRequest = match parse_request(value) {
                Ok(request) => request,
                Err(error) => return print_output(&Output::from_error(&error)),
            };
            match runtime.block_on(enqueue(&request)) {
                Ok(output) => output,
                Err(error) => Output::from_error(&error),
            }
        }
        "decide" => {
            let request: DecideRequest = match parse_request(value) {
                Ok(request) => request,
                Err(error) => return print_output(&Output::from_error(&error)),
            };
            match runtime.block_on(decide(&request)) {
                Ok(output) => output,
                Err(error) => Output::from_error(&error),
            }
        }
        "batch" => {
            let request: BatchRequest = match parse_request(value) {
                Ok(request) => request,
                Err(error) => return print_output(&Output::from_error(&error)),
            };
            match runtime.block_on(batch(&request)) {
                Ok(output) => output,
                Err(error) => Output::from_error(&error),
            }
        }
        "receipt" => {
            let request: ReceiptRequest = match parse_request(value) {
                Ok(request) => request,
                Err(error) => return print_output(&Output::from_error(&error)),
            };
            match runtime.block_on(receipt(&request)) {
                Ok(output) => output,
                Err(error) => Output::from_error(&error),
            }
        }
        "history" => {
            let request: HistoryRequest = match parse_request(value) {
                Ok(request) => request,
                Err(error) => return print_output(&Output::from_error(&error)),
            };
            match runtime.block_on(history(&request)) {
                Ok(output) => output,
                Err(error) => Output::from_error(&error),
            }
        }
        _ => return Err(()),
    };
    print_output(&output)
}

fn print_output(output: &Output) -> Result<u8, ()> {
    let line = serde_json::to_string(output).map_err(|_| ())?;
    println!("{line}");
    Ok(exit_code(output))
}

fn exit_code(output: &Output) -> u8 {
    match output.kind.as_str() {
        "persisted" | "replay" | "partial" | "complete" | "bound" | "history" => 0,
        "refused" => 2,
        "commit.unknown" => 3,
        _ => 1,
    }
}
