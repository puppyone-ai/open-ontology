fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("oo-gateway: {reason}");
            std::process::ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), &'static str> {
    let mut args = std::env::args_os();
    let _argv0 = args.next();
    let Some(dir) = args.next() else {
        return Err("config");
    };
    if args.next().is_some() {
        return Err("config");
    }
    let dir = std::path::PathBuf::from(dir);
    let loaded = oo_gateway::http_gateway::load_operator_dir(&dir)?;
    let addr_path = dir.join("listen.addr");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime")?;
    runtime.block_on(oo_gateway::http_gateway::serve(loaded.state, loaded.listen, &addr_path))
}
