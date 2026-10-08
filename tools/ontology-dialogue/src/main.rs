#![forbid(unsafe_code)]

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;

use ontology_dialogue::{execute, propose, Fault};

const HELP: &str = "\
ontology-dialogue — review a role Pack SQL proposal, then run the approved statement

Usage:
  ontology-dialogue propose --config <path>
  ontology-dialogue execute --config <path>
  ontology-dialogue --help

propose sends one natural-language question and one role Pack's public files to one
approved HTTPS chat completion. The model SQL is untrusted. propose does not start
query-pg. endpoint.transport development_local is explicit. That mode calls only
http://127.0.0.1 and may use a 6-byte bearer. The default transport is production:
HTTPS, with a bearer of 16 to 256 bytes.

execute starts query-pg only after a local approval file names the current proposal
hash, SQL hash, tuple hash, and Pack digest. That file is a declaration by the local
operator. It is not authenticated authentication. A missing, stale, or tampered
approval does not start query-pg.

The database password stays in the query-pg request's password file. This command
does not read it. The bearer stays in its own file and in the Authorization header.
Stdout on success is one JSON object and a newline. Stderr is one static token.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--help" => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        [command, flag, path] if flag == "--config" && (command == "propose" || command == "execute") => {
            let text = match read_config(path) {
                Ok(text) => text,
                Err(fault) => return fail(fault),
            };
            if command == "propose" {
                match propose(&text) {
                    Ok(bytes) => write_stdout(&bytes),
                    Err(fault) => fail(fault),
                }
            } else {
                match execute(&text) {
                    Ok(finished) => {
                        if finished.exit != 0 {
                            let token = match finished.exit {
                                6 => "guard",
                                8 => "citations",
                                _ => "query",
                            };
                            eprintln!("ontology-dialogue: {token}");
                        }
                        if io::stdout().write_all(&finished.stdout).is_err() {
                            return fail(Fault::io());
                        }
                        ExitCode::from(finished.exit)
                    }
                    Err(fault) => fail(fault),
                }
            }
        }
        _ => fail(Fault::usage()),
    }
}

fn fail(fault: Fault) -> ExitCode {
    eprintln!("ontology-dialogue: {token}", token = fault.token);
    ExitCode::from(fault.exit_code)
}

fn write_stdout(bytes: &[u8]) -> ExitCode {
    match io::stdout().write_all(bytes) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => fail(Fault::io()),
    }
}

fn read_config(path: &str) -> Result<String, Fault> {
    let file_path = Path::new(path);
    let meta = std::fs::symlink_metadata(file_path).map_err(|_| Fault::config())?;
    if !meta.file_type().is_file() || meta.len() > 65_536 {
        return Err(Fault::config());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
    options.custom_flags(0x20000);
    let mut file = options.open(file_path).map_err(|_| Fault::config())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| Fault::io())?;
    if bytes.len() > 65_536 {
        return Err(Fault::config());
    }
    String::from_utf8(bytes).map_err(|_| Fault::config())
}
