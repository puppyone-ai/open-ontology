#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use pg_onboarding::{apply_reviewed, inspect, Stop};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(stop) => {
            eprintln!("pg-onboarding: {} {}", stop.step, stop.detail);
            if stop.detail == "usage" {
                ExitCode::from(2)
            } else {
                ExitCode::from(1)
            }
        }
    }
}

fn run() -> Result<(), Stop> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(|| Stop::new("usage", "usage"))?;
    let mut config = None;
    let mut run_dir = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| Stop::new("usage", "usage"))?;
        match flag.as_str() {
            "--config" if config.is_none() => config = Some(value),
            "--run" if run_dir.is_none() => run_dir = Some(value),
            _ => return Err(Stop::new("usage", "usage")),
        }
    }
    let config = config.ok_or_else(|| Stop::new("usage", "usage"))?;
    let run_dir = run_dir.ok_or_else(|| Stop::new("usage", "usage"))?;
    let config_path = PathBuf::from(config);
    let run_path = PathBuf::from(run_dir);
    match command.as_str() {
        "inspect" => inspect(&config_path, &run_path),
        "apply-reviewed" => apply_reviewed(&config_path, &run_path),
        _ => Err(Stop::new("usage", "usage")),
    }
}
