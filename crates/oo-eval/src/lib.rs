//! Development A/B evaluation entry point. No model or database credentials are read.

#![forbid(unsafe_code)]

use std::process::Command;

/// Run the repository-local harness without adding benchmark-only dependencies
/// to the product workspace. Python 3 and the benchmark binaries are required.
pub fn run(args: impl IntoIterator<Item = String>) -> i32 {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../benchmarks/ab/run.py");
    match Command::new("python3").arg(script).args(args).status() {
        Ok(status) => status.code().unwrap_or(2),
        Err(_) => {
            eprintln!("oo-eval: harness_unavailable");
            2
        }
    }
}
