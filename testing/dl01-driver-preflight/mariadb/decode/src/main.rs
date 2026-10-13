#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use mariadb_arrow_decode::{self, decode_files, publish};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mariadb-arrow-decode: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut out = None;
    let mut inputs = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = Some(PathBuf::from(args.get(index).ok_or("missing --out")?));
            }
            other if other.starts_with('-') => return Err(other.to_string()),
            other => inputs.push(PathBuf::from(other)),
        }
        index += 1;
    }
    let out = out.ok_or("--out")?;
    if inputs.is_empty() {
        return Err("missing ipc input".to_string());
    }
    match decode_files(&inputs) {
        Ok(value) => publish(&out, &value).map_err(|err| err.fact),
        Err(err) => {
            let _ = std::fs::remove_file(&out);
            let _ = std::fs::remove_file(out.with_extension("json.tmp"));
            Err(err.fact)
        }
    }
}
