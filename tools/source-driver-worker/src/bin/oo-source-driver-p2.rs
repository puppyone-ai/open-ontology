//! Control-plane CLI for `oo.source.driver-oracle/v2`.
//!
//! This binary does not link an Oracle client. The child container does.
//! A successful batch still has `admits_import=false`. v1 kernel network
//! control is unchanged and stays pending.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::ExitCode;

use source_driver_worker::{
    execute, write_batch_receipt, ExecuteResult, OracleLaunch, P2_PROTOCOL, REQUEST_MAX_BYTES,
};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(token) => {
            let _ = writeln!(io::stderr(), "oo-source-driver-p2: {token}");
            let _ = io::stderr().flush();
            ExitCode::from(exit_for(token))
        }
    }
}

fn run() -> Result<ExitCode, &'static str> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("--help") | Some("-h") | Some("help") => {
            print_help();
            Ok(ExitCode::SUCCESS)
        }
        Some("run") => {
            let path = args.next().ok_or("usage")?;
            if args.next().is_some() {
                return Err("usage");
            }
            run_spec(Path::new(&path))
        }
        _ => Err("usage"),
    }
}

fn print_help() {
    println!(
        "\
oo-source-driver-p2 run <mode-0600-json> — {P2_PROTOCOL}

Launches one Oracle thin child from a content digest the Docker engine reports.
The child is the only process that connects to the source. This process has no Oracle client.
admits_import=false. The batch frame records cancel_api=absent because that read does not interrupt. oracle:oci-cancel records oci_marker_interrupt only after ErrorKind::Cancelled. A call timeout is a deadline. The OCI proof is the child interrupt, not a container kill.
v1 kernel_network=pending is unchanged. This program is not Oracle acceptance.

The request file is a regular file, mode 0600, and not a symlink.
"
    );
}

fn run_spec(path: &Path) -> Result<ExitCode, &'static str> {
    let meta = fs::symlink_metadata(path).map_err(|_| "protocol")?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err("protocol");
    }
    if meta.permissions().mode() & 0o777 != 0o600 {
        return Err("protocol");
    }
    if meta.len() == 0 || meta.len() > REQUEST_MAX_BYTES as u64 {
        return Err("protocol");
    }
    let mut file = fs::File::open(path).map_err(|_| "io")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| "io")?;
    if bytes.len() as u64 != meta.len() {
        return Err("protocol");
    }
    let launch = OracleLaunch::from_spec(&bytes)?;
    if let Some(receipt) = &launch.receipt_path
        && receipt.exists()
    {
        return Err("protocol");
    }
    match execute(&launch)? {
        ExecuteResult::Receipt(receipt) => {
            if receipt.admits_import {
                return Err("protocol");
            }
            if let Some(path) = &launch.receipt_path {
                write_batch_receipt(path, &receipt)?;
            }
            println!(
                "rows={} batches=1 admits_import=false kernel_network=internal schema_fingerprint={}",
                receipt.rows, receipt.schema_fingerprint
            );
            Ok(ExitCode::SUCCESS)
        }
        ExecuteResult::Observed(token) => {
            println!("oo-source-driver-p2: {token}");
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn exit_for(token: &str) -> u8 {
    match token {
        "deadline_observed" => 3,
        "precision_observed" => 4,
        "denied_observed" => 6,
        _ => 2,
    }
}
