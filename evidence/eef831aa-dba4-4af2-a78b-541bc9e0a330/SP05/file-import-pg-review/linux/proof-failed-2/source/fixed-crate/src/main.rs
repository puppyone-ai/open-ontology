//! Request path and password-file path. The password value is not an argument.

#![forbid(unsafe_code)]

use std::env;
use std::path::Path;
use std::process::ExitCode;

use file_import_pg::{error_document, exit_code, run_paths};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(request) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    let Some(password) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        usage();
        return ExitCode::from(2);
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::from(1),
    };
    match runtime.block_on(run_paths(Path::new(&request), Path::new(&password))) {
        Ok(result) => match serde_json::to_string(&result) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(_) => {
                println!(
                    "{{\"protocol\":\"oo.pipeline.file-import-pg-error/v1\",\"code\":\"database\",\"detail\":\"json\",\"child_started\":true,\"published\":false}}"
                );
                ExitCode::from(1)
            }
        },
        Err(error) => {
            println!("{}", error_document(&error));
            ExitCode::from(exit_code(&error) as u8)
        }
    }
}

fn usage() {
    eprintln!("usage: file-import-pg REQUEST PASSWORD_FILE");
}
