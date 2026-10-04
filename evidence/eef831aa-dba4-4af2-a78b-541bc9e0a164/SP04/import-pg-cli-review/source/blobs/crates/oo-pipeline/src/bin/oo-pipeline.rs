use std::ffi::OsStr;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let _argv0 = args.next();
    match args.next() {
        Some(command) if command == OsStr::new("import-pg") && args.next().is_none() => import_pg(),
        Some(_) | None => {
            eprintln!("oo-pipeline: usage");
            ExitCode::from(2)
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn import_pg() -> ExitCode {
    match oo_pipeline::import_cli::import_pg().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}
