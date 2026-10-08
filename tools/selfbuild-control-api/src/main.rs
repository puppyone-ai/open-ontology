//! `selfbuild-control-api install|serve --config CONFIG`

#![forbid(unsafe_code)]

use std::process::ExitCode;

use selfbuild_control_api::config::load_config_file;
use selfbuild_control_api::error::ApiError;
use selfbuild_control_api::serve::{self, stdout_line};
use selfbuild_control_api::store;

fn main() -> ExitCode {
    let code = match runtime() {
        Ok(code) => code,
        Err(code) => code,
    };
    ExitCode::from(code)
}

fn runtime() -> Result<u8, u8> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or(1u8)?;
    let flag = args.next().ok_or(1u8)?;
    let path = args.next().ok_or(1u8)?;
    if flag != "--config" || args.next().is_some() {
        return Err(1);
    }
    let config = load_config_file(&path).map_err(exit_of)?;
    match command.as_str() {
        "install" => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|_| 1u8)?;
            match runtime.block_on(store::install_schema(&config.postgres)) {
                Ok(()) => {
                    println!("{}", stdout_line("install", "ready"));
                    Ok(0)
                }
                Err(error) => Err(exit_of(error)),
            }
        }
        "serve" => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|_| 1u8)?;
            match runtime.block_on(serve::serve(config)) {
                Ok(()) => Ok(0),
                Err(error) => Err(exit_of(error)),
            }
        }
        _ => Err(1),
    }
}

fn exit_of(error: ApiError) -> u8 {
    match error.code {
        "malformed" => 1,
        "commit.unknown" => 3,
        _ => 2,
    }
}
