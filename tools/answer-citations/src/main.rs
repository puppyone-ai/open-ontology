#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

use answer_citations::{validate, Fault, Outcome};

const HELP: &str = "\
answer-citations — check [q_...] citations against local query-pg receipts

Usage:
  answer-citations
  answer-citations --help

Stdin is one oo.answer-citations/v1 document. trust must be
local_operator_declared. The caller supplies session_id, turn_id, role,
project, protocol, receipt SHA-256 values, receipt bodies, and the answer.
The receipt has no session, principal, or authentication field. This command
does not add them and does not judge whether the answer follows from query
rows or explains a warning.

Exit 0 is a valid citation report. Exit 1 is an invalid citation report.
Exit 2 is usage. Exit 3 is the request or a budget. Exit 4 is binding or
the receipt hash. Exit 5 is a receipt that is not a query-pg audit.
";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    match args.next() {
        Some(arg) if arg == "--help" && args.next().is_none() => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(_) => usage(),
        None => match read_stdin() {
            Ok(bytes) => match validate(&bytes) {
                Ok(outcome) => write_outcome(&outcome),
                Err(fault) => fault.exit(),
            },
            Err(fault) => fault.exit(),
        },
    }
}

fn usage() -> ExitCode {
    let token = "usage";
    eprintln!("answer-citations: {token}");
    ExitCode::from(2)
}

fn read_stdin() -> Result<Vec<u8>, Fault> {
    let mut bytes = Vec::new();
    let stdin = io::stdin();
    let handle = stdin.lock();
    handle
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| Fault::request())?;
    if bytes.len() > 1_048_576 {
        return Err(Fault::budget());
    }
    if bytes.is_empty() {
        return Err(Fault::request());
    }
    Ok(bytes)
}

fn write_outcome(outcome: &Outcome) -> ExitCode {
    let json = match outcome.json() {
        Ok(bytes) => bytes,
        Err(fault) => return fault.exit(),
    };
    let mut out = io::stdout().lock();
    if out
        .write_all(&json)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush())
        .is_err()
    {
        return Fault::request().exit();
    }
    if outcome.is_valid() {
        ExitCode::SUCCESS
    } else {
        let token = "citations";
        eprintln!("answer-citations: {token}");
        ExitCode::from(1)
    }
}
