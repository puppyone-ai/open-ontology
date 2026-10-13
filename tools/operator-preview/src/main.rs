//! `operator-preview describe|query|cancel`.
//!
//! Paths name private files. Passwords stay in those files. This process does
//! not listen for HTTP and does not start Pi.

#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use operator_preview::{cancel, describe, query, Finished, EXIT_QUERY, EXIT_USAGE};

const HELP: &str = "\
operator-preview — trusted local operator preview

This binary reads one trusted operator file and one sealed local target.
It is not a public untrusted query gate. That gate is pending.
Untrusted admission stays blocked. Pi is not launched.

  operator-preview describe --authority <file>
  operator-preview query --authority <file> --query <file>
  operator-preview cancel --authority <file> --execution <id>

Exit 0 success or confirmed cancel.
Exit 1 io, 2 usage, 3 request, 4 tls, 5 policy, 6 guard,
7 audit, 8 query, 9 pack mismatch, 10 cancel not sent.
";

fn main() -> ExitCode {
    match std::panic::catch_unwind(run) {
        Ok(finished) => emit(finished),
        Err(_) => {
            eprintln!("operator-preview: query");
            ExitCode::from(EXIT_QUERY)
        }
    }
}

fn run() -> Finished {
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        return usage();
    };
    if command == "--help" || command == "-h" || command == "help" {
        return Finished {
            code: 0,
            stdout: HELP.as_bytes().to_vec(),
            stderr: Vec::new(),
        };
    }
    let mut authority = None;
    let mut query_path = None;
    let mut execution = None;
    let mut rest = args.peekable();
    while let Some(arg) = rest.next() {
        let flag = arg.to_string_lossy();
        let value = rest.next().map(PathBuf::from);
        match flag.as_ref() {
            "--authority" => authority = value,
            "--query" => query_path = value,
            "--execution" => execution = value.map(|path| path.to_string_lossy().into_owned()),
            _ => return usage(),
        }
    }
    let Some(authority) = authority else {
        return usage();
    };
    match command.to_string_lossy().as_ref() {
        "describe" if query_path.is_none() && execution.is_none() => describe(&authority),
        "query" => match query_path {
            Some(path) if execution.is_none() => query(&authority, &path),
            _ => usage(),
        },
        "cancel" => match execution {
            Some(id) if query_path.is_none() => cancel(&authority, &id),
            _ => usage(),
        },
        _ => usage(),
    }
}

fn usage() -> Finished {
    Finished {
        code: EXIT_USAGE,
        stdout: Vec::new(),
        stderr: b"operator-preview: usage\n".to_vec(),
    }
}

fn emit(finished: Finished) -> ExitCode {
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    if out.write_all(&finished.stdout).is_err() || out.flush().is_err() {
        let _ = err.write_all(b"operator-preview: io\n");
        return ExitCode::from(1);
    }
    if err.write_all(&finished.stderr).is_err() || err.flush().is_err() {
        return ExitCode::from(1);
    }
    ExitCode::from(finished.code)
}
