//! Commands: preflight, import, fixture, builder, gates, audit, broken, detach, all, capture-check, c5-cycle, c5-schedule, c5-produce, c5-arrow-dump, product-witness, product-type-audit.

use std::path::PathBuf;
use std::process::ExitCode;

use mariadb_preview::capture;
use mariadb_preview::config::{Config, Phase};
use mariadb_preview::c5;
use mariadb_preview::witness;
use mariadb_preview::{all, audit, builder, executable_ok, gates_run, import, preflight};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    if command == "capture-check" {
        let request = args.next().map(PathBuf::from);
        let facts = args.next().map(PathBuf::from);
        let (Some(request), Some(facts)) = (request, facts) else {
            eprintln!("capture-check request.json facts.json");
            return ExitCode::from(2);
        };
        return match capture::check_files(&request, &facts) {
            Ok(body) => {
                println!("{body}");
                ExitCode::SUCCESS
            }
            Err(capture::CheckError::Refuse(body)) => {
                println!("{body}");
                ExitCode::from(3)
            }
            Err(capture::CheckError::Input(err)) => {
                eprintln!("capture-check {err}");
                ExitCode::from(2)
            }
        };
    }
    if command == "c5-arrow-dump" {
        let Some(path) = args.next().map(PathBuf::from) else {
            eprintln!("c5-arrow-dump capture.arrow");
            return ExitCode::from(2);
        };
        return match c5::dump_arrow(&path) {
            Ok(body) => {
                print!("{body}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("c5-arrow-dump {err}");
                ExitCode::from(1)
            }
        };
    }
    if command == "product-witness" || command == "product-type-audit" {
        let Some(path) = args.next().map(PathBuf::from) else {
            eprintln!("{command} request.json");
            return ExitCode::from(2);
        };
        let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(err) => {
                eprintln!("runtime {}", mariadb_preview::config::redact_authenticator(&err.to_string()));
                return ExitCode::from(2);
            }
        };
        let ran = if command == "product-witness" {
            runtime.block_on(witness::product_witness(&path))
        } else {
            runtime.block_on(witness::type_audit(&path))
        };
        return match ran {
            Ok(body) => {
                println!("{body}");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("product {}", mariadb_preview::config::redact_authenticator(&err));
                ExitCode::from(1)
            }
        };
    }
    if command == "c5-cycle" || command == "c5-schedule" || command == "c5-produce" {
        let Some(path) = args.next().map(PathBuf::from) else {
            eprintln!("c5-cycle request.json");
            return ExitCode::from(2);
        };
        if command == "c5-produce" {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(err) => {
                    eprintln!("runtime {}", mariadb_preview::config::redact_authenticator(&err.to_string()));
                    return ExitCode::from(2);
                }
            };
            return match runtime.block_on(c5::produce(&path)) {
                Ok(body) => {
                    println!("{body}");
                    ExitCode::SUCCESS
                }
                Err(c5::RunError::Refuse(body)) => {
                    println!("{body}");
                    ExitCode::from(3)
                }
                Err(c5::RunError::Input(err)) => {
                    eprintln!("c5 {}", mariadb_preview::config::redact_authenticator(&err));
                    ExitCode::from(2)
                }
                Err(c5::RunError::Fail(err)) => {
                    eprintln!("c5 {err}");
                    ExitCode::from(1)
                }
            };
        }
        let kind = if command == "c5-schedule" { c5::Kind::Schedule } else { c5::Kind::Cycle };
        let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(err) => {
                eprintln!("runtime {}", mariadb_preview::config::redact_authenticator(&err.to_string()));
                return ExitCode::from(2);
            }
        };
        return match runtime.block_on(c5::execute(&path, kind)) {
            Ok(body) => {
                println!("{body}");
                ExitCode::SUCCESS
            }
            Err(c5::RunError::Refuse(body)) => {
                println!("{body}");
                ExitCode::from(3)
            }
            Err(c5::RunError::Input(err)) => {
                eprintln!("c5 {}", mariadb_preview::config::redact_authenticator(&err));
                ExitCode::from(2)
            }
            Err(c5::RunError::Fail(err)) => {
                eprintln!("c5 {err}");
                ExitCode::from(1)
            }
        };
    }
    let config_path = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp/secrets/config.json"));
    let config = match Config::load(&config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("config {}", mariadb_preview::config::redact_authenticator(&err));
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("runtime {}", mariadb_preview::config::redact_authenticator(&err.to_string()));
            return ExitCode::from(2);
        }
    };
    match command.as_str() {
        "preflight" => one(&runtime.block_on(preflight::run(&config))),
        "import" => one(&runtime.block_on(import::run(&config))),
        "fixture" => one(&runtime.block_on(import::run_fixture(&config))),
        "builder" => one(&runtime.block_on(builder(&config))),
        "gates" => one(&runtime.block_on(gates_run(&config))),
        "audit" => one(&runtime.block_on(audit(&config))),
        "broken" => one(&runtime.block_on(preflight::broken(&config))),
        "detach" => one(&runtime.block_on(import::detach(&config))),
        "all" => {
            let phases = runtime.block_on(all(&config));
            for phase in &phases {
                eprintln!("{} {}", phase.name, if phase.ok { "ok" } else { "fail" });
            }
            if executable_ok(&phases) { ExitCode::SUCCESS } else { ExitCode::from(1) }
        }
        _ => {
            eprintln!("command");
            ExitCode::from(2)
        }
    }
}

fn one(phase: &Phase) -> ExitCode {
    eprintln!("{} {}", phase.name, if phase.ok { "ok" } else { "fail" });
    if phase.ok { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
