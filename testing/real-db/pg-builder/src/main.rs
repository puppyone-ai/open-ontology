//! Write schema.sql and COPY files for the private builder database.

#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(out) = flag(&args, "--out") else {
        eprintln!("pg-fixture --out DIR --reader IDENT");
        return ExitCode::from(2);
    };
    let Some(reader) = flag(&args, "--reader") else {
        eprintln!("pg-fixture --out DIR --reader IDENT");
        return ExitCode::from(2);
    };
    match pg_fixture::write_fixture(&PathBuf::from(out), &reader) {
        Ok(()) => ExitCode::from(0),
        Err(message) => {
            eprintln!("pg-fixture {message}");
            ExitCode::from(1)
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].clone())
}
