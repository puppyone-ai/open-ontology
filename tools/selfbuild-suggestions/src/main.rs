#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process::ExitCode;

use selfbuild_suggestions::{
    DOCUMENT_MAX_BYTES, Error, Gateway, ProcessRunner, parse_document, produce, produce_with,
};

fn main() -> ExitCode {
    match run() {
        Ok(outcome) => emit(&outcome, None),
        Err(Error::Malformed(reason)) => {
            eprintln!("selfbuild-suggestions: malformed {reason}");
            ExitCode::from(1)
        }
        Err(Error::Refused { reason, outcome }) => emit(&outcome, Some(reason)),
    }
}

fn emit(outcome: &serde_json::Value, refusal: Option<&str>) -> ExitCode {
    match serde_json::to_string(outcome) {
        Ok(body) => {
            println!("{body}");
            if let Some(reason) = refusal {
                eprintln!("selfbuild-suggestions: refused {reason}");
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(_) => {
            eprintln!("selfbuild-suggestions: malformed json");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<serde_json::Value, Error> {
    let mut args = env::args().skip(1);
    let command = args.next();
    let flag = args.next();
    let path = args.next();
    let extra = args.next();
    let config = match (command.as_deref(), flag.as_deref(), path, extra) {
        (Some("produce"), None, None, None) => None,
        (Some("produce"), Some("--config"), Some(path), None) => Some(path),
        _ => return Err(Error::Malformed("usage")),
    };
    let bytes = read_stdin()?;
    let document = parse_document(&bytes).map_err(Error::Malformed)?;
    match config {
        None => produce(&document),
        Some(path) => {
            let config_bytes = read_config(&path)?;
            let config = parse_document(&config_bytes).map_err(Error::Malformed)?;
            let (mut runner, probe, max_validated) = ProcessRunner::open(&config)?;
            produce_with(
                &document,
                Some(Gateway {
                    probe_sql: &probe,
                    max_validated,
                    runner: &mut runner,
                }),
            )
        }
    }
}

fn read_stdin() -> Result<Vec<u8>, Error> {
    let mut stdin = io::stdin().lock();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stdin.read(&mut chunk).map_err(|_| Error::Malformed("io"))?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > DOCUMENT_MAX_BYTES {
            return Err(Error::Malformed("stdin_too_large"));
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    if buf.is_empty() {
        return Err(Error::Malformed("empty"));
    }
    Ok(buf)
}

fn read_config(path: &str) -> Result<Vec<u8>, Error> {
    if path.is_empty() || path.contains('\0') {
        return Err(Error::Malformed("config"));
    }
    let meta = fs::symlink_metadata(path).map_err(|_| Error::Malformed("config"))?;
    if !meta.file_type().is_file() || meta.len() > u64::try_from(DOCUMENT_MAX_BYTES).unwrap_or(0) {
        return Err(Error::Malformed("config"));
    }
    let mut file = fs::File::open(path).map_err(|_| Error::Malformed("config"))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)
        .map_err(|_| Error::Malformed("io"))?;
    if buf.len() > DOCUMENT_MAX_BYTES {
        return Err(Error::Malformed("config"));
    }
    Ok(buf)
}
