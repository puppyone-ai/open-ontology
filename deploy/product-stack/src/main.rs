//! `oo-product-stack <preflight|health|journey|sbom|identity-commit>` reads one JSON document on stdin.

#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

use oo_product_stack::{health, identity_commit, journey, preflight, sbom, Decision, Malformed};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        return ExitCode::from(1);
    };
    if args.next().is_some() {
        return ExitCode::from(1);
    }
    let mut stdin = Vec::new();
    if io::stdin().take(2_097_152).read_to_end(&mut stdin).is_err() || stdin.len() == 2_097_152 {
        return ExitCode::from(1);
    }
    let text = match String::from_utf8(stdin) {
        Ok(text) => text,
        Err(_) => return ExitCode::from(1),
    };
    let result = match command.as_str() {
        "preflight" => preflight(&text),
        "health" => health(&text),
        "journey" => journey(&text),
        "sbom" => sbom(&text),
        "identity-commit" => identity_commit(&text),
        _ => Err(Malformed("command".to_owned())),
    };
    match result {
        Ok(decision) => finish(decision),
        Err(_) => ExitCode::from(1),
    }
}

fn finish(decision: Decision) -> ExitCode {
    let mut stdout = io::stdout().lock();
    if writeln!(stdout, "{}", decision.body).is_err() {
        return ExitCode::from(1);
    }
    if decision.ready { ExitCode::from(0) } else { ExitCode::from(2) }
}
