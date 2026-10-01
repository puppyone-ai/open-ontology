use std::io::{Read, Write};
use std::process::ExitCode;

use oo_builder::{DraftError, compile_structure_draft};
use oo_core::limits::DOCUMENT_MAX_BYTES;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => {
            eprintln!("oo-builder: process entry is not implemented (SP00-W01 skeleton)");
            ExitCode::from(2)
        }
        Some("structure-draft") => {
            if args.next().is_some() {
                eprintln!(
                    "oo-builder: structure-draft reads JSON from stdin and takes no arguments"
                );
                return ExitCode::from(2);
            }
            match read_and_compile() {
                Ok(bytes) => {
                    let mut out = std::io::stdout().lock();
                    if out.write_all(bytes.as_bytes()).is_err() {
                        return ExitCode::from(1);
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("oo-builder: {err}");
                    ExitCode::from(1)
                }
            }
        }
        Some(_) => {
            eprintln!("oo-builder: unknown command");
            ExitCode::from(2)
        }
    }
}

fn read_and_compile() -> Result<String, DraftError> {
    let mut stdin = std::io::stdin().lock();
    let mut buf = Vec::new();
    let max = DOCUMENT_MAX_BYTES.saturating_add(1);
    stdin
        .by_ref()
        .take(max as u64)
        .read_to_end(&mut buf)
        .map_err(|_| DraftError::new("io.read", "/", "stdin could not be read"))?;
    let text = std::str::from_utf8(&buf)
        .map_err(|_| DraftError::new("json.syntax", "/", "stdin is not UTF-8"))?;
    compile_structure_draft(text)
}
