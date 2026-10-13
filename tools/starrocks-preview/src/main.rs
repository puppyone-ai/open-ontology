#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: starrocks-preview run --config PATH --out DIR");
        return ExitCode::from(2);
    };
    if command != "run" {
        eprintln!("usage: starrocks-preview run --config PATH --out DIR");
        return ExitCode::from(2);
    }
    let mut config = None;
    let mut out = None;
    let rest = args.collect::<Vec<_>>();
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--config" => {
                index += 1;
                config = rest.get(index).map(PathBuf::from);
            }
            "--out" => {
                index += 1;
                out = rest.get(index).map(PathBuf::from);
            }
            _ => {
                eprintln!("usage: starrocks-preview run --config PATH --out DIR");
                return ExitCode::from(2);
            }
        }
        index += 1;
    }
    let (Some(config), Some(out)) = (config, out) else {
        eprintln!("usage: starrocks-preview run --config PATH --out DIR");
        return ExitCode::from(2);
    };
    match starrocks_preview::product::run(&config, &out) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("starrocks-preview: {err}");
            ExitCode::from(1)
        }
    }
}
