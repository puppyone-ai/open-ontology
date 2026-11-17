//! `oo serve` and `oo context` (Super Ontology, feature `docs`).
//!
//! This crate is a client of the docs engine over loopback HTTP and never
//! links `oo-docs` (architecture rule R-CLIENT-DB-DRIVER): `oo serve` runs the
//! engine and a token-checking reverse proxy, `oo context` talks to the proxy.

pub mod engine;
pub mod proxy;
pub mod secrets;
pub mod serve;

use crate::args::Invocation;
use crate::outcome::Failure;

const HELP: &str = "Usage:\n  oo serve [--state-dir <dir>] [--port <n>|0] [--keep-engine] [--json]\n  oo serve --status [--json]\n  oo serve --stop\n";

/// `serve` dispatch. Runs the daemon to completion; the process exit code is
/// the daemon's (10 §8.6 point 9).
pub fn dispatch(args: &[String]) -> Option<Result<Invocation, Failure>> {
    let (command, rest) = args.split_first()?;
    match command.as_str() {
        "serve" if rest.iter().any(|a| a == "--help" || a == "-h") => {
            Some(Ok(Invocation::Help(HELP.into())))
        }
        "serve" => {
            // The daemon speaks for itself (one JSON line with --json), so the
            // caller's report printer must not add a line of its own.
            std::process::exit(i32::from(serve::run(rest)))
        }
        _ => None,
    }
}
