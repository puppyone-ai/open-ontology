#![forbid(unsafe_code)]

use std::path::Path;
use std::process::ExitCode;

use local_analysis::{Fault, Written, run};

const HELP: &str = "\
local-analysis — exact group sum or count from one query-result-export

Usage:
  local-analysis --request <path>
  local-analysis --help

The request is oo.local-analysis/v1. export_dir contains export.source.jsonl,
export.csv, and export.sidecar.json. lineage_file binds one query-pg audit
pair. output_dir receives analysis.csv, analysis.svg, and
analysis.manifest.json. engine rust_exact is the only engine that runs.
engine duckdb, xlsx true, and parquet true write nothing. closes_sp09_w05
stays false.

Exit 0 writes a citable analysis. Exit 1 uncited writes an analysis that
is not business evidence. Exit 1 canceled or io removes names this call
created. Exit 2 is usage. Exit 3 is the request, a budget, or a refused
engine, xlsx, or parquet. Exit 4 is a path, a hash, or lineage. Exit 5
is a source that is not one coherent export.
";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else {
        return Fault::usage().exit();
    };
    if first == "--help" && args.next().is_none() {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    }
    let Some(path) = args.next() else {
        return Fault::usage().exit();
    };
    if first != "--request" || args.next().is_some() {
        return Fault::usage().exit();
    }
    let Some(path) = path.to_str() else {
        return Fault::usage().exit();
    };
    if path.is_empty() {
        return Fault::usage().exit();
    }
    match run(Path::new(path)) {
        Ok(Written::Citable) => ExitCode::SUCCESS,
        Ok(Written::Uncited) => Fault::uncited().exit(),
        Err(fault) => fault.exit(),
    }
}
