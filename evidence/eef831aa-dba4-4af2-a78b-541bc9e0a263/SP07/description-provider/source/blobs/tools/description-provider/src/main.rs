//! Stdin entry for one description-provider call.
//!
//! `description-provider run --config PATH` reads the config file and one
//! description-request command from stdin. The bearer token is not an
//! argument. This process does not print the config path, the token, or a
//! response body.

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::process::ExitCode;

use description_provider::{ProviderError, run_from_config_file};

const COMMAND_MAX: usize = 1_048_576;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let phase = args.next();
    let flag = args.next();
    let path = args.next();
    let extra = args.next();
    if phase.as_deref() != Some("run") || flag.as_deref() != Some("--config") || extra.is_some() {
        eprintln!("description-provider: usage: description-provider run --config PATH");
        return ExitCode::from(2);
    }
    let Some(path) = path else {
        eprintln!("description-provider: usage: description-provider run --config PATH");
        return ExitCode::from(2);
    };
    let command = match read_stdin() {
        Ok(command) => command,
        Err(code) => return code,
    };
    match run_from_config_file(&command, &path) {
        Ok(document) => write_stdout(&document),
        Err(err) => fail(&err),
    }
}

fn write_stdout(document: &str) -> ExitCode {
    let mut out = std::io::stdout().lock();
    if out.write_all(document.as_bytes()).is_err() {
        eprintln!(
            "description-provider: oo.builder.description-provider/v1/io.write stdout could not be written"
        );
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn fail(err: &ProviderError) -> ExitCode {
    eprintln!("description-provider: {} {}", err.code, err.message);
    ExitCode::from(1)
}

fn read_stdin() -> Result<String, ExitCode> {
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    let max = u64::try_from(COMMAND_MAX.saturating_add(1)).unwrap_or(u64::MAX);
    if stdin.by_ref().take(max).read_to_end(&mut buf).is_err() {
        eprintln!(
            "description-provider: oo.builder.description-provider/v1/io.read stdin could not be read"
        );
        return Err(ExitCode::from(1));
    }
    if buf.len() > COMMAND_MAX {
        eprintln!(
            "description-provider: oo.builder.description-provider/v1/command.binding the command exceeds the byte budget"
        );
        return Err(ExitCode::from(1));
    }
    let text = std::str::from_utf8(&buf).map_err(|_| {
        eprintln!(
            "description-provider: oo.builder.description-provider/v1/command.binding stdin is not UTF-8"
        );
        ExitCode::from(1)
    })?;
    Ok(text.to_owned())
}
