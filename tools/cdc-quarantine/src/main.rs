//! JSON control-event quarantine. No sockets, database, ACK, or target writes.

#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use cdc_quarantine::{
    limits_from_env, parse_control, put, recover, replay, FaultCode, Status, MAX_LINE_BYTES,
};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let directory = args.next();
    if args.next().is_some() {
        return usage();
    }
    let (Some(command), Some(directory)) = (command, directory) else {
        return usage();
    };
    if directory.is_empty() {
        return usage();
    }
    let root = Path::new(&directory);
    match command.as_str() {
        "recover" => emit(recover(root)),
        "put" => emit(put_or_replay(root, false)),
        "replay" => emit(put_or_replay(root, true)),
        _ => usage(),
    }
}

fn put_or_replay(root: &Path, replay_only: bool) -> Status {
    let limits = match limits_from_env() {
        Ok(limits) => limits,
        Err(code) => return Status::fault(code, None),
    };
    let bytes = match read_line() {
        Ok(bytes) => bytes,
        Err(code) => return Status::fault(code, None),
    };
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(_) => return Status::fault(FaultCode::Malformed, None),
    };
    let event = match parse_control(text, &limits) {
        Ok(event) => event,
        Err(rejected) => return Status::fault(rejected.code, rejected.event.as_ref()),
    };
    if replay_only {
        replay(root, &event)
    } else {
        put(root, &event, &limits)
    }
}

fn emit(status: Status) -> ExitCode {
    let code = status.exit_code();
    match serde_json::to_vec(&status) {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            if io::stdout().write_all(&bytes).is_err() {
                return ExitCode::from(1);
            }
            ExitCode::from(code)
        }
        Err(_) => ExitCode::from(1),
    }
}

fn usage() -> ExitCode {
    let _ = writeln!(
        io::stderr(),
        "usage: cdc-quarantine put|recover|replay <dir>"
    );
    ExitCode::from(1)
}

fn read_line() -> Result<Vec<u8>, FaultCode> {
    let mut input = io::stdin();
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let read = input.read(&mut byte).map_err(|_| FaultCode::Io)?;
        if read == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        if byte[0] == b'\r' {
            return Err(FaultCode::Malformed);
        }
        if buf.len() >= MAX_LINE_BYTES {
            let mut discarded = 0usize;
            loop {
                let read = input.read(&mut byte).map_err(|_| FaultCode::Io)?;
                if read == 0 || byte[0] == b'\n' || discarded >= MAX_LINE_BYTES {
                    break;
                }
                discarded = discarded.saturating_add(1);
            }
            return Err(FaultCode::LineTooLong);
        }
        buf.push(byte[0]);
    }
    if buf.is_empty() {
        return Err(FaultCode::Malformed);
    }
    Ok(buf)
}
