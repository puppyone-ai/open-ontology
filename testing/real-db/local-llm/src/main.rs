#![forbid(unsafe_code)]

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use oo_local_llm::{fixed_upstream, load_identity, parse_listen, read_regular, reject_redirect_env, serve_on, Fault, Limits};
use tokio::net::TcpListener;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(fault) => {
            eprintln!("oo-local-llm-relay: {fault}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Fault> {
    reject_redirect_env()?;
    let options = options()?;
    let cert = read_regular(&options.cert, 65536)?;
    let key = read_regular(&options.key, 65536)?;
    let identity = load_identity(&cert, &key)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| Fault::io())?;
    runtime.block_on(async move {
        let listener = TcpListener::bind(options.listen).await.map_err(|_| Fault::bind())?;
        let bound = listener.local_addr().map_err(|_| Fault::bind())?;
        println!("listen={bound}");
        serve_on(listener, fixed_upstream(), identity, Limits::standard()).await
    })
}

struct Options {
    listen: SocketAddr,
    cert: PathBuf,
    key: PathBuf,
}

fn options() -> Result<Options, Fault> {
    let mut args = env::args().skip(1);
    let mut listen = None;
    let mut cert = None;
    let mut key = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(Fault::config)?;
        match flag.as_str() {
            "--listen" if listen.is_none() => listen = Some(parse_listen(&value)?),
            "--cert" if cert.is_none() => cert = Some(PathBuf::from(value)),
            "--key" if key.is_none() => key = Some(PathBuf::from(value)),
            _ => return Err(Fault::config()),
        }
    }
    Ok(Options {
        listen: listen.ok_or_else(Fault::config)?,
        cert: cert.ok_or_else(Fault::config)?,
        key: key.ok_or_else(Fault::config)?,
    })
}
