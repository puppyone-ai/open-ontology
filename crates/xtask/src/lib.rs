//! Engineering gate runner. Not a product service, driver, or gateway.
//!
//! There is no fault switch and no HTTP server. Exit 0 is a successful check,
//! not a phase sign-off.

#![forbid(unsafe_code)]

pub mod architecture;
pub mod contracts;
pub mod deny_toml;
pub mod evidence;
pub mod gates;
pub mod owned_process;
pub mod report;
pub mod runner_capture;
pub mod suites;
pub mod supply;
pub mod util;

pub use report::Report;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn dispatch(args: &[String]) -> Report {
    if args.iter().any(|arg| {
        arg == "--disable-gates"
            || arg == "--fault"
            || arg == "--enable-faults"
            || arg == "--enable-fault"
    }) {
        return usage("production xtask has no fault switch and no gate-disable path");
    }
    let mut rest = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--format" {
            index += 1;
            match args.get(index).map(String::as_str) {
                Some("json" | "human") => {}
                _ => return usage("xtask --format requires human or json"),
            }
        } else if args[index].starts_with('-') && rest.is_empty() {
            return usage(&format!("unknown argument {}", args[index]));
        } else {
            rest.push(args[index].clone());
        }
        index += 1;
    }
    let Some(command) = rest.first().map(String::as_str) else {
        return usage("xtask requires a command. Try xtask help");
    };
    let tail = &rest[1..];
    match command {
        "help" | "--help" | "-h" => help(),
        "version" | "--version" | "-V" => version(),
        "contracts" => contracts::run(tail),
        "architecture" => architecture::run(tail),
        "evidence" => evidence::run(tail),
        "gates" => gates::run(tail),
        "test" => suites::test_cmd(tail),
        "bench" => suites::bench_cmd(tail),
        "datagen" => suites::datagen_cmd(tail),
        "infra" => suites::infra_cmd(tail),
        "docs" => suites::docs_cmd(tail),
        "secrets" => supply::secrets_cmd(tail),
        "supply" => supply::supply_cmd(tail),
        "release" => suites::release_cmd(tail),
        other => usage(&format!("unknown command {other}")),
    }
}

fn help() -> Report {
    Report::ok(
        "help",
        "help",
        serde_json::json!({
            "version": VERSION,
            "commands": [
                "help", "version", "contracts", "architecture", "evidence", "gates",
                "test", "bench", "datagen", "infra", "docs", "secrets", "supply", "release"
            ],
            "exits": {
                "0": "check succeeded; not a phase sign-off",
                "1": "check failed",
                "2": "usage",
                "3": "pending, blocked, or not implemented"
            },
            "fault_interface": false
        }),
    )
}

fn version() -> Report {
    Report::ok(
        "version",
        "version",
        serde_json::json!({
            "version": VERSION,
            "package": "xtask",
            "rust_version": env!("CARGO_PKG_RUST_VERSION"),
            "phase_signoff": "in_review"
        }),
    )
}

fn usage(message: &str) -> Report {
    Report::fail(
        "xtask",
        "usage",
        report::EXIT_USAGE,
        vec![report::Issue::new("cli.usage", "xtask", message)],
        serde_json::json!({}),
    )
}

pub fn output_format(args: &[String]) -> &'static str {
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--format" {
            if args.get(index + 1).map(String::as_str) == Some("json") {
                return "json";
            }
            return "human";
        }
        index += 1;
    }
    "human"
}
