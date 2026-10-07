#![forbid(unsafe_code)]

use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oo_local_llm::{
    fixed_upstream, load_boundary, load_identity, mint_client_token, parse_listen, read_regular, reject_redirect_env,
    serve_on, Fault, Limits,
};
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
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 2 && args[0] == "--mint-client-token" {
        return mint_client_token(Path::new(&args[1]));
    }
    let options = options(&args)?;
    let cert = read_regular(&options.cert, 65536)?;
    let key = read_regular(&options.key, 65536)?;
    let identity = load_identity(&cert, &key)?;
    let boundary = load_boundary(&options.client_token, &options.upstream_token)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| Fault::io())?;
    runtime.block_on(async move {
        let listener = TcpListener::bind(options.listen).await.map_err(|_| Fault::bind())?;
        let bound = listener.local_addr().map_err(|_| Fault::bind())?;
        println!("listen={bound}");
        serve_on(listener, fixed_upstream(), identity, boundary, Limits::standard()).await
    })
}

struct Options {
    listen: SocketAddr,
    cert: PathBuf,
    key: PathBuf,
    client_token: PathBuf,
    upstream_token: PathBuf,
}

fn options(args: &[String]) -> Result<Options, Fault> {
    let mut args = args.iter();
    let mut listen = None;
    let mut cert = None;
    let mut key = None;
    let mut client_token = None;
    let mut upstream_token = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(Fault::config)?;
        match flag.as_str() {
            "--listen" if listen.is_none() => listen = Some(parse_listen(value)?),
            "--cert" if cert.is_none() => cert = Some(PathBuf::from(value)),
            "--key" if key.is_none() => key = Some(PathBuf::from(value)),
            "--client-token-file" if client_token.is_none() => client_token = Some(PathBuf::from(value)),
            "--upstream-token-file" if upstream_token.is_none() => upstream_token = Some(PathBuf::from(value)),
            _ => return Err(Fault::config()),
        }
    }
    Ok(Options {
        listen: listen.ok_or_else(Fault::config)?,
        cert: cert.ok_or_else(Fault::config)?,
        key: key.ok_or_else(Fault::config)?,
        client_token: client_token.ok_or_else(Fault::config)?,
        upstream_token: upstream_token.ok_or_else(Fault::config)?,
    })
}
