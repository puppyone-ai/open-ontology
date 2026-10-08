//! `ontology-lifecycle inspect|run|serve`

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ontology_lifecycle::{bind_loopback, execute, serve_once, session_ok, to_json, Plan, Session};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return usage();
    };
    match command.as_str() {
        "inspect" | "run" => {
            let Ok(opts) = options(&mut args) else {
                return usage();
            };
            if let Some(dir) = &opts.guard
                && !empty_dir(dir)
            {
                return ExitCode::from(1);
            }
            let plan = if command == "inspect" {
                Plan::default()
            } else {
                Plan {
                    session: opts.session,
                    identity: opts.identity,
                    endpoints: opts.endpoints,
                    state: opts.state.clone(),
                }
            };
            let result = if command == "inspect" {
                ontology_lifecycle::assess_root(&opts.root)
            } else {
                execute(&opts.root, &plan)
            };
            match result {
                Ok(assessment) => {
                    println!("{}", to_json(&assessment));
                    if let Some(dir) = &opts.guard
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

struct Opts {
    root: PathBuf,
    guard: Option<PathBuf>,
    state: Option<PathBuf>,
    session: Option<Session>,
    identity: Option<String>,
    endpoints: Vec<(String, String)>,
}

fn options(args: &mut impl Iterator<Item = String>) -> Result<Opts, ()> {
    let mut root = None;
    let mut guard = None;
    let mut state = None;
    let mut session = None;
    let mut identity = None;
    let mut endpoints = Vec::new();
    while let Some(arg) = args.next() {
        let value = args.next().ok_or(())?;
        match arg.as_str() {
            "--source-root" => root = Some(PathBuf::from(value)),
            "--guard" => guard = Some(PathBuf::from(value)),
            "--state" => state = Some(PathBuf::from(value)),
            "--session" => session = Some(load_session(Path::new(&value))?),
            "--identity" => identity = Some(value),
            "--producer" => endpoints.push(producer(&value)?),
            _ => return Err(()),
        }
    }
    Ok(Opts {
        root: root.ok_or(())?,
        guard,
        state,
        session,
        identity,
        endpoints,
    })
}

fn producer(value: &str) -> Result<(String, String), ()> {
    let (id, url) = value.split_once('=').ok_or(())?;
    if !id.starts_with("OC-") || url.contains(' ') {
        return Err(());
    }
    Ok((id.to_owned(), url.to_owned()))
}

fn load_session(path: &Path) -> Result<Session, ()> {
    let meta = fs::symlink_metadata(path).map_err(|_| ())?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 4096 {
        return Err(());
    }
    let text = fs::read_to_string(path).map_err(|_| ())?;
    let mut cookie = None;
    let mut csrf = None;
    let mut origin = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("cookie ") {
            cookie = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("csrf ") {
            csrf = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("origin ") {
            origin = Some(value.to_owned());
        } else {
            return Err(());
        }
    }
    let session = Session {
        cookie: cookie.ok_or(())?,
        csrf: csrf.ok_or(())?,
        origin: origin.ok_or(())?,
    };
    if session_ok(&session) { Ok(session) } else { Err(()) }
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
    eprintln!("ontology-lifecycle inspect|run --source-root ROOT [--guard DIR] [--state DIR] [--session FILE] [--identity URL] [--producer OC-01=http://127.0.0.1:PORT]");
    eprintln!("ontology-lifecycle serve --bind 127.0.0.1:PORT --source-root ROOT --once");
    ExitCode::from(1)
}
