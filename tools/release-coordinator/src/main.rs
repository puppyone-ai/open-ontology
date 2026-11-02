//! `release-coordinator` commands. The password and the CA stay in files named by the config.

#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(reason) = release_coordinator::argv_malformed(&args) {
        let _ = writeln!(io::stderr(), "release-coordinator: malformed {reason}");
        return ExitCode::from(1);
    }
    let limit = match release_coordinator::stdin_limit(&args) {
        Ok(limit) => limit,
        Err(reason) => {
            let _ = writeln!(io::stderr(), "release-coordinator: malformed {reason}");
            return ExitCode::from(1);
        }
    };
    let stdin = match read_bounded(io::stdin(), limit) {
        Ok(stdin) => stdin,
        Err(reason) => {
            let _ = writeln!(io::stderr(), "release-coordinator: malformed {reason}");
            return ExitCode::from(1);
        }
    };
    let exit = runtime().block_on(release_coordinator::dispatch(&args, &stdin));
    if !exit.stdout.is_empty() {
        let _ = io::stdout().write_all(&exit.stdout);
        let _ = io::stdout().write_all(b"\n");
    }
    if !exit.stderr.is_empty() {
        let _ = io::stderr().write_all(&exit.stderr);
    }
    ExitCode::from(u8::try_from(exit.code).unwrap_or(1))
}

fn read_bounded(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, &'static str> {
    let mut stdin = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(stdin),
            Ok(size) => {
                if stdin.len().saturating_add(size) > limit {
                    return Err("stdin_size");
                }
                stdin.extend_from_slice(&chunk[..size]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("stdin"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::read_bounded;

    #[test]
    fn transport_accepts_library_documents_only_with_the_larger_bound() {
        let document = vec![b' '; 8_388_609];
        assert_eq!(read_bounded(document.as_slice(), 8_388_608), Err("stdin_size"));
        assert_eq!(read_bounded(document.as_slice(), 16_777_216).unwrap(), document);
    }

    #[test]
    fn transport_enforces_exact_capacity() {
        assert_eq!(read_bounded(&b"abc"[..], 3).unwrap(), b"abc");
        assert_eq!(read_bounded(&b"abcd"[..], 3), Err("stdin_size"));
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("runtime")
}
