#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

use enum_history::{
    CompileInput, Observation, Outcome, CONTRACT, STDIN_MAX_BYTES, compile, refused,
};
use serde::Deserialize;
use serde_json::Value;

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
    let request = match parse_request(&bytes) {
        Ok(request) => request,
        Err(reason) => {
            emit_malformed(reason);
            return ExitCode::from(1);
        }
    };
    let threshold = match request.threshold {
        None => None,
        Some(Value::String(text)) => Some(text),
        Some(_) => {
            emit_refusal(&refused(
                "threshold_not_decimal_string",
                "threshold must be a decimal string",
            ));
            return ExitCode::from(2);
        }
    };
    let input = CompileInput {
        enum_yaml: request.enum_yaml,
        as_of: request.as_of,
        observations: request
            .observations
            .into_iter()
            .map(|item| Observation {
                code: item.code,
                count: item.count,
            })
            .collect(),
        threshold,
    };
    match compile(&input) {
        Outcome::Compiled(report) => emit_json(report.as_ref(), 0),
        Outcome::Refused(refusal) => {
            emit_refusal(&refusal);
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CliRequest {
    enum_yaml: String,
    as_of: String,
    observations: Vec<CliObservation>,
    #[serde(default)]
    threshold: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CliObservation {
    code: String,
    count: u64,
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

fn parse_request(bytes: &[u8]) -> Result<CliRequest, &'static str> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let request = CliRequest::deserialize(&mut deserializer).map_err(|_| "json")?;
    deserializer.end().map_err(|_| "trailing")?;
    Ok(request)
}

fn emit_malformed(reason: &str) {
    let body = format!(
        "{{\"contract\":\"{CONTRACT}\",\"outcome\":\"malformed\",\"reason\":\"{reason}\",\"database_used\":false,\"source_modified\":false,\"oo_dict_written\":false,\"metric_certified\":false,\"owner_authenticated\":false,\"sweeper_ran\":false}}"
    );
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{body}");
    let _ = writeln!(io::stderr(), "enum-history: malformed {reason}");
}

fn emit_refusal(refusal: &enum_history::Refusal) -> ExitCode {
    let _ = writeln!(io::stderr(), "enum-history: refused {}", refusal.reason);
    emit_json(refusal, 2)
}

fn emit_json<T: serde::Serialize>(value: &T, code: u8) -> ExitCode {
    match serde_json::to_string(value) {
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
