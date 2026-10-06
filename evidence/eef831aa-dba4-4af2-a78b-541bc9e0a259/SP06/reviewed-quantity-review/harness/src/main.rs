//! Independent review of frozen integer quantity.
//! The library path does not open a database. The bound path reads
//! mapping `run_generation` and does not stamp the target fence.

#![forbid(unsafe_code)]

mod bound;
mod catalog;
mod classify;
mod proc;

use std::process::ExitCode;

#[tokio::main(flavor = "multi_thread", worker_threads = 1)]
async fn main() -> ExitCode {
    if let Err(stop) = classify::run() {
        proc::write_stop(&stop);
        return ExitCode::from(1);
    }
    match std::env::var("OO_A259_BOUND") {
        Ok(value) if value == "1" => {}
        _ => return ExitCode::SUCCESS,
    }
    if let Err(stop) = bound::run().await {
        proc::write_stop(&stop);
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
