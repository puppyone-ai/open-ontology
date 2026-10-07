#![forbid(unsafe_code)]

use std::path::Path;
use std::process::ExitCode;

use query_result_export::{Completeness, Fault, execute_request, read_request_file};

const HELP: &str = "\
query-result-export — write one formula-safe CSV from query-pg stdout

Usage:
  query-result-export --request <path>
  query-result-export --help

The request is oo.query-result-export/v1. source_file is one query-pg
stdout. output_dir receives export.csv, export.sidecar.json, and
export.source.jsonl. columns are the caller header. query-pg cells are
null, bool, int, or text. int stays canonical decimal text, including a
leading minus. text that starts with = + - @ after Unicode whitespace or
a control character is prefixed with U+0027 APOSTROPHE. xlsx is
unsupported. http_auth is false. The stdout has no query_id.

Exit 0 writes a complete export. Exit 1 partial writes an incomplete
export and does not call it complete. Exit 1 io is a write failure.
Exit 2 is usage. Exit 3 is the request or a budget. Exit 4 is a path.
Exit 5 is a source that is not one coherent query-pg stdout.
";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else {
        return usage();
    };
    if first == "--help" && args.next().is_none() {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let Some(path) = args.next() else {
        return usage();
    };
    if first != "--request" || args.next().is_some() {
        return usage();
    }
    let Some(path) = path.to_str() else {
        return usage();
    };
    if path.is_empty() {
        return usage();
    }
    match read_request_file(Path::new(path)) {
        Ok(bytes) => match execute_request(&bytes) {
            Ok(Completeness::Complete) => ExitCode::SUCCESS,
            Ok(Completeness::Partial) => {
                eprintln!("query-result-export: partial");
                ExitCode::from(1)
            }
            Err(fault) => fault.exit(),
        },
        Err(fault) => fault.exit(),
    }
}

fn usage() -> ExitCode {
    Fault::usage().exit()
}
