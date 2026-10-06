#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode, Stdio};
use std::thread;

use session_release::{run, ReleaseRequest, CONTRACT};
use session_store::STDIN_MAX_BYTES;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("fixture-child") => fixture_child(args.next().as_deref()),
        Some(_) => ExitCode::from(1),
        None => orchestrate().await,
    }
}

async fn orchestrate() -> ExitCode {
    let mut bytes = match read_stdin() {
        Ok(bytes) => bytes,
        Err(reason) => {
            emit_reason(reason);
            return ExitCode::from(1);
        }
    };
    let request = match serde_json::from_slice::<ReleaseRequest>(&bytes) {
        Ok(request) => request,
        Err(_) => {
            bytes.fill(0);
            std::hint::black_box(&bytes);
            emit_reason("json");
            return ExitCode::from(1);
        }
    };
    bytes.fill(0);
    std::hint::black_box(bytes);
    if request.contract != CONTRACT {
        emit_reason("contract");
        return ExitCode::from(1);
    }
    let result = run(request).await;
    match serde_json::to_string(&result.receipt) {
        Ok(line) => {
            let _ = writeln!(io::stdout(), "{line}");
            ExitCode::from(result.exit_code)
        }
        Err(_) => {
            emit_reason("json");
            ExitCode::from(1)
        }
    }
}

fn read_stdin() -> Result<Vec<u8>, &'static str> {
    let mut buf = Vec::new();
    let cap = u64::try_from(STDIN_MAX_BYTES).unwrap_or(0);
    io::stdin()
        .take(cap.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|_| "stdin")?;
    if buf.len() > STDIN_MAX_BYTES {
        return Err("stdin");
    }
    Ok(buf)
}

fn emit_reason(reason: &str) {
    let _ = writeln!(
        io::stdout(),
        "{{\"contract\":\"{CONTRACT}\",\"disposition\":\"malformed\",\"reason\":\"{reason}\",\"physical_release_certified\":false,\"complete_release_called\":false,\"local_fixture\":true,\"production_sandbox\":false}}"
    );
}

fn fixture_child(mode: Option<&str>) -> ExitCode {
    if mode != Some("escape") {
        return ExitCode::from(1);
    }
    let mut command = Command::new("/bin/sleep");
    command
        .arg("120")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let Ok(child) = command.spawn() else {
        return ExitCode::from(1);
    };
    let _ = writeln!(io::stdout(), "{}", child.id());
    let _ = io::stdout().flush();
    // `Child`'s drop does not signal. The grandchild keeps its own process group.
    drop(child);
    thread::park();
    ExitCode::from(0)
}
