//! `ontology-lifecycle inspect|run|serve`

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use ontology_lifecycle::{assess_root, bind_loopback, serve_once, to_json};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return usage();
    };
    match command.as_str() {
        "inspect" | "run" => {
            let Ok(root) = flag(&mut args, "--source-root") else {
                return usage();
            };
            let guard = match optional_flag(&mut args, "--guard") {
                Ok(guard) => guard,
                Err(()) => return usage(),
            };
            if args.next().is_some() {
                return usage();
            }
            if let Some(dir) = &guard
                && !empty_dir(dir)
            {
                return ExitCode::from(1);
            }
            match assess_root(&root) {
                Ok(assessment) => {
                    println!("{}", to_json(&assessment));
                    if let Some(dir) = &guard
                        && !empty_dir(dir)
                    {
                        return ExitCode::from(1);
                    }
                    if assessment.chain_closed {
                        ExitCode::from(0)
                    } else {
                        ExitCode::from(2)
                    }
                }
                Err(code) => {
                    eprintln!("{code}");
                    ExitCode::from(1)
                }
            }
        }
        "serve" => {
            let Ok(bind) = text_flag(&mut args, "--bind") else {
                return usage();
            };
            let Ok(root) = flag(&mut args, "--source-root") else {
                return usage();
            };
            let once = match args.next() {
                Some(flag) if flag == "--once" => true,
                Some(_) => return usage(),
                None => false,
            };
            if !once || args.next().is_some() {
                return usage();
            }
            let Some(port) = loopback_port(&bind) else {
                eprintln!("bind");
                return ExitCode::from(1);
            };
            let listener = match bind_loopback(port) {
                Ok(listener) => listener,
                Err(_) => {
                    eprintln!("bind");
                    return ExitCode::from(1);
                }
            };
            let Ok(addr) = listener.local_addr() else {
                eprintln!("bind");
                return ExitCode::from(1);
            };
            println!("{{\"listen\":\"{}\"}}", addr);
            match serve_once(listener, &root) {
                Ok(assessment) => {
                    if assessment.chain_closed {
                        ExitCode::from(0)
                    } else {
                        ExitCode::from(2)
                    }
                }
                Err(_) => ExitCode::from(1),
            }
        }
        _ => usage(),
    }
}

fn flag(args: &mut impl Iterator<Item = String>, name: &str) -> Result<PathBuf, ()> {
    text_flag(args, name).map(PathBuf::from)
}

fn text_flag(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, ()> {
    match args.next() {
        Some(got) if got == name => args.next().ok_or(()),
        _ => Err(()),
    }
}

fn optional_flag(args: &mut impl Iterator<Item = String>, name: &str) -> Result<Option<PathBuf>, ()> {
    let Some(peek) = args.next() else {
        return Ok(None);
    };
    if peek != name {
        return Err(());
    }
    args.next().map(PathBuf::from).map(Some).ok_or(())
}

fn loopback_port(bind: &str) -> Option<u16> {
    let (host, port) = bind.rsplit_once(':')?;
    if host != "127.0.0.1" {
        return None;
    }
    let port = port.parse().ok()?;
    Some(port)
}

fn empty_dir(dir: &PathBuf) -> bool {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }
    match fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}

fn usage() -> ExitCode {
    eprintln!("ontology-lifecycle inspect|run --source-root ROOT [--guard DIR]");
    eprintln!("ontology-lifecycle serve --bind 127.0.0.1:PORT --source-root ROOT --once");
    ExitCode::from(1)
}
