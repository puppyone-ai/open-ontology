//! CLI for the source-driver supervisor.
//!
//! `supervise` pins one child executable and one customer driver artifact.
//! The executable path is a flag, not a JSON field. Target credentials stay
//! in the caller request file and are not written to the child. This program
//! is not Oracle acceptance. Kernel network control stays pending.

#![forbid(unsafe_code)]

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use source_driver_worker::{
    CallerRequest, KernelNetwork, ParentConfig, ParentError, PROTOCOL, REQUEST_MAX_BYTES,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(token) => {
            let _ = writeln!(io::stderr(), "oo-source-driver-parent: {token}");
            let _ = io::stderr().flush();
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), &'static str> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("--help") | Some("-h") | Some("help") => {
            print_help();
            Ok(())
        }
        Some("supervise") => supervise(&mut args),
        _ => Err("usage"),
    }
}

fn print_help() {
    println!(
        "\
oo-source-driver-parent supervise — {PROTOCOL}

Pins one child executable and one customer driver artifact, then pulls framed Arrow IPC.
DRIVER_MISSING is returned when that artifact is absent or is a symlink. There is no fallback driver.
kernel_network=pending. This process does not install a network namespace, seccomp filter, or egress policy.
This program is not Oracle acceptance and does not load a vendor driver.

Usage:
  oo-source-driver-parent supervise \\
    --worker <absolute-executable> \\
    --driver-volume <absolute-directory> \\
    --image-digest sha256:<64-lowercase-hex> \\
    --driver-digest sha256:<64-lowercase-hex> \\
    --deadline-ms <1..=120000> \\
    --request <mode-0600-json>

The request file is caller JSON. Its image digest, driver digest, and driver volume must match these flags.
The child executable path is only this flag. The child receives the source credential ref and not the target credential ref.
On Linux the forked child marks every descriptor above stderr close-on-exec before exec. Caller-owned descriptors stay open in this process. The child environment is empty.
If close_range is unavailable the parent returns isolate and does not exec.
"
    );
}

struct Flags {
    worker: PathBuf,
    volume: PathBuf,
    image_digest: String,
    driver_digest: String,
    deadline_ms: u64,
    request: PathBuf,
}

fn supervise(args: &mut impl Iterator<Item = String>) -> Result<(), &'static str> {
    let flags = parse_flags(args)?;
    let bytes = read_request(&flags.request)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "request_syntax")?;
    let image = value
        .get("image_digest")
        .and_then(|item| item.as_str())
        .ok_or("image_digest")?;
    if image != flags.image_digest {
        return Err("image_digest");
    }
    let driver = value
        .pointer("/driver/digest")
        .and_then(|item| item.as_str())
        .ok_or("driver_digest")?;
    if driver != flags.driver_digest {
        return Err("driver_digest");
    }
    let volume = value
        .pointer("/driver/volume")
        .and_then(|item| item.as_str())
        .ok_or("driver_volume")?;
    if flags.volume.to_str() != Some(volume) {
        return Err("driver_volume");
    }
    let request = CallerRequest::try_from_slice(&bytes, &flags.volume).map_err(err_token)?;
    let config = ParentConfig::try_new(&flags.worker, flags.deadline_ms).map_err(err_token)?;
    let mut pull = config.start(&request).map_err(err_token)?;
    while pull.next_batch().map_err(err_token)?.is_some() {}
    let receipt = pull.finish().map_err(err_token)?;
    let network = match receipt.kernel_network() {
        KernelNetwork::Pending => "pending",
    };
    println!("protocol={PROTOCOL}");
    println!("rows={}", receipt.rows());
    println!("batches={}", receipt.batches());
    println!("arrow_ipc={}", receipt.arrow_ipc());
    println!("schema_fingerprint={}", receipt.schema_fingerprint());
    println!("kernel_network={network}");
    println!("admits_import={}", receipt.admits_import());
    println!("published={}", receipt.published());
    println!("pg_run_complete={}", receipt.pg_run_complete());
    println!("fallback={}", receipt.fallback());
    println!("vendor_library_loaded={}", receipt.vendor_library_loaded());
    Ok(())
}

fn parse_flags(args: &mut impl Iterator<Item = String>) -> Result<Flags, &'static str> {
    let mut worker = None;
    let mut volume = None;
    let mut image_digest = None;
    let mut driver_digest = None;
    let mut deadline_ms = None;
    let mut request = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or("usage")?;
        let slot = match flag.as_str() {
            "--worker" => &mut worker,
            "--driver-volume" => &mut volume,
            "--image-digest" => &mut image_digest,
            "--driver-digest" => &mut driver_digest,
            "--deadline-ms" => &mut deadline_ms,
            "--request" => &mut request,
            _ => return Err("usage"),
        };
        if slot.is_some() {
            return Err("usage");
        }
        *slot = Some(value);
    }
    let deadline_text = deadline_ms.ok_or("usage")?;
    if !deadline_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("deadline");
    }
    Ok(Flags {
        worker: PathBuf::from(worker.ok_or("usage")?),
        volume: PathBuf::from(volume.ok_or("usage")?),
        image_digest: image_digest.ok_or("usage")?,
        driver_digest: driver_digest.ok_or("usage")?,
        deadline_ms: deadline_text.parse().map_err(|_| "deadline")?,
        request: PathBuf::from(request.ok_or("usage")?),
    })
}

fn read_request(path: &Path) -> Result<Vec<u8>, &'static str> {
    let meta = fs::symlink_metadata(path).map_err(|_| "io")?;
    if !meta.file_type().is_file() {
        return Err("io");
    }
    if meta.permissions().mode() & 0o777 != 0o600 {
        return Err("request_mode");
    }
    let mut file = File::open(path).map_err(|_| "io")?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = file.read(&mut buf).map_err(|_| "io")?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > REQUEST_MAX_BYTES {
            return Err("request_too_large");
        }
        bytes.extend_from_slice(&buf[..read]);
    }
    Ok(bytes)
}

fn err_token(err: ParentError) -> &'static str {
    match err {
        ParentError::Config(token) => token,
        ParentError::Failed(failure) => failure.token.unwrap_or(failure.kind.as_str()),
    }
}
