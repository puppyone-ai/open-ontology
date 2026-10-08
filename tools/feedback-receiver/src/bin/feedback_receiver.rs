//! `--config` is the only argument. Key bytes stay in files.

use std::env;
use std::path::Path;
use std::process::ExitCode;

use feedback_receiver::{load_config, serve};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let flag = args.next();
    let path = args.next();
    if flag.as_deref() != Some("--config") || args.next().is_some() {
        eprintln!("usage");
        return ExitCode::from(2);
    }
    let Some(path) = path else {
        eprintln!("usage");
        return ExitCode::from(2);
    };
    let config = match load_config(Path::new(&path)) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("config={}", err.name());
            return ExitCode::from(1);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("runtime=unavailable");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(serve(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("serve={}", err.name());
            ExitCode::from(1)
        }
    }
}
