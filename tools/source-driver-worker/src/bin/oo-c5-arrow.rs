//! Encode or decode one C5 Arrow file. Does not admit an import.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use source_driver_worker::c5::{canonical_json, row_hash};
use source_driver_worker::c5_arrow::{arrow_sha256, decode_table, encode_table};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oo-c5-arrow: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or("usage")?;
    match command.as_str() {
        "encode" => {
            let input = args.next().ok_or("input")?;
            let output = args.next().ok_or("output")?;
            if args.next().is_some() {
                return Err("usage".to_owned());
            }
            let spec = read_json(Path::new(&input))?;
            let bytes = encode_table(&spec)?;
            fs::write(&output, &bytes).map_err(|err| err.to_string())?;
            println!("arrow_sha256 {}", arrow_sha256(&bytes));
            println!("arrow_bytes {}", bytes.len());
            Ok(())
        }
        "decode" => {
            let input = args.next().ok_or("input")?;
            let output = args.next().ok_or("output")?;
            if args.next().is_some() {
                return Err("usage".to_owned());
            }
            let bytes = fs::read(&input).map_err(|err| err.to_string())?;
            let value = decode_table(&bytes)?;
            let text = serde_json::to_vec_pretty(&value).map_err(|err| err.to_string())?;
            fs::write(&output, text).map_err(|err| err.to_string())?;
            println!("arrow_sha256 {}", arrow_sha256(&bytes));
            Ok(())
        }
        "hash-cells" => {
            let input = args.next().ok_or("input")?;
            if args.next().is_some() {
                return Err("usage".to_owned());
            }
            let value = read_json(Path::new(&input))?;
            let cells = value.as_object().ok_or("cells")?;
            println!("{}", row_hash(cells)?);
            println!("{}", canonical_json(cells)?);
            Ok(())
        }
        _ => Err("usage".to_owned()),
    }
}

fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    serde_json::from_slice(&bytes).map_err(|err| err.to_string())
}
