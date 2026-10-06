//! Protocol stand-in for boundary tests. This is not Pi and does not approve Pi.

#![forbid(unsafe_code)]
#![allow(clippy::redundant_closure_for_method_calls)]

use oo_p09_pi_rpc::{parse_value, Value};
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("stub: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mode = std::env::args().nth(1).ok_or("usage: pi-rpc-stub <mode>")?;
    match mode.as_str() {
        "pair" => {
            let line = read_line()?;
            let id = id_of(&line)?;
            let mut out = io::stdout();
            writeln!(out, r#"{{"type":"turn_start"}}"#).map_err(|err| err.to_string())?;
            writeln!(
                out,
                r#"{{"id":"{id}","type":"response","command":"get_state","success":true}}"#
            )
            .map_err(|err| err.to_string())?;
            out.flush().map_err(|err| err.to_string())?;
        }
        "reorder" => {
            let first = read_line()?;
            let second = read_line()?;
            let first_id = id_of(&first)?;
            let second_id = id_of(&second)?;
            let mut out = io::stdout();
            writeln!(
                out,
                r#"{{"id":"{second_id}","type":"response","command":"get_messages","success":true}}"#
            )
            .map_err(|err| err.to_string())?;
            writeln!(
                out,
                r#"{{"id":"{first_id}","type":"response","command":"get_state","success":true}}"#
            )
            .map_err(|err| err.to_string())?;
            out.flush().map_err(|err| err.to_string())?;
        }
        "huge" => {
            let mut out = io::stdout();
            let blob = vec![b'A'; 8192];
            out.write_all(&blob).map_err(|err| err.to_string())?;
            out.flush().map_err(|err| err.to_string())?;
            thread::sleep(Duration::from_secs(5));
        }
        "partial" => {
            let mut out = io::stdout();
            out.write_all(br#"{"id":"x""#).map_err(|err| err.to_string())?;
            out.flush().map_err(|err| err.to_string())?;
        }
        "eof" => {}
        "delay" => thread::sleep(Duration::from_secs(5)),
        "shape" => {
            let _ = read_line()?;
            let mut out = io::stdout();
            writeln!(out, "not-json").map_err(|err| err.to_string())?;
            out.flush().map_err(|err| err.to_string())?;
        }
        "unknown" => {
            let line = read_line()?;
            let id = id_of(&line)?;
            let mut out = io::stdout();
            writeln!(out, r#"{{"type":"made-up"}}"#).map_err(|err| err.to_string())?;
            writeln!(
                out,
                r#"{{"id":"{id}","type":"response","command":"get_state","success":true}}"#
            )
            .map_err(|err| err.to_string())?;
            out.flush().map_err(|err| err.to_string())?;
        }
        "idle" => thread::sleep(Duration::from_secs(30)),
        other => return Err(format!("unknown mode {other}")),
    }
    Ok(())
}

fn read_line() -> Result<Vec<u8>, String> {
    let mut byte = [0u8; 1];
    let mut line = Vec::new();
    let mut stdin = io::stdin();
    loop {
        let n = stdin.read(&mut byte).map_err(|err| err.to_string())?;
        if n == 0 {
            return Err("stdin eof".to_owned());
        }
        line.push(byte[0]);
        if byte[0] == b'\n' {
            return Ok(line);
        }
        if line.len() > 1_048_576 {
            return Err("line too big".to_owned());
        }
    }
}

fn id_of(line: &[u8]) -> Result<String, String> {
    let value = parse_value(line).map_err(|err| err.to_string())?;
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "missing id".to_owned())
}
