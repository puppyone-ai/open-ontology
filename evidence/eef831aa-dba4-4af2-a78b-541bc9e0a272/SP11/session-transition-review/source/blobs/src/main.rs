#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

use serde::Deserialize;
use session_transition::{transition, Outcome, TransitionInput, STDIN_MAX_BYTES, CONTRACT};

fn main() -> ExitCode {
    if std::env::args().len() != 1 {
        emit_malformed("usage");
        return ExitCode::from(1);
    }
    let bytes = match read_stdin() {
        Ok(bytes) => bytes,
        Err(reason) => {
            emit_malformed(reason);
            return ExitCode::from(1);
        }
    };
    let input = match parse_input(&bytes) {
        Ok(input) => input,
        Err(reason) => {
            emit_malformed(reason);
            return ExitCode::from(1);
        }
    };
    let output = transition(&input);
    let code = match output.outcome {
        Outcome::Applied | Outcome::Idempotent => 0,
        Outcome::Refused => {
            let reason = reason_snake(&output);
            eprintln!("session-transition: refused {reason}");
            2
        }
    };
    match serde_json::to_string(&output) {
        Ok(body) => {
            println!("{body}");
            ExitCode::from(code)
        }
        Err(_) => {
            emit_malformed("json");
            ExitCode::from(1)
        }
    }
}

fn reason_snake(output: &session_transition::TransitionOutput) -> String {
    output
        .reason
        .map(|reason| {
            serde_json::to_value(reason)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "refused".to_owned())
        })
        .unwrap_or_else(|| "refused".to_owned())
}

fn read_stdin() -> Result<Vec<u8>, &'static str> {
    let mut stdin = io::stdin().lock();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stdin.read(&mut chunk).map_err(|_| "io")?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > STDIN_MAX_BYTES {
            return Err("stdin_too_large");
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    if buf.is_empty() {
        return Err("empty");
    }
    Ok(buf)
}

fn parse_input(bytes: &[u8]) -> Result<TransitionInput, &'static str> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let input = TransitionInput::deserialize(&mut deserializer).map_err(|_| "json")?;
    deserializer.end().map_err(|_| "json")?;
    Ok(input)
}

fn emit_malformed(reason: &str) {
    let body = format!(
        "{{\"contract\":\"{CONTRACT}\",\"outcome\":\"malformed\",\"reason\":\"{reason}\",\"physical_release_certified\":false,\"database_used\":false,\"auth_provider_used\":false,\"runtime_used\":false}}"
    );
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{body}");
    let _ = writeln!(io::stderr(), "session-transition: malformed {reason}");
}
