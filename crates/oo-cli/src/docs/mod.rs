//! `oo serve` and `oo context` (Super Ontology, feature `docs`).
//!
//! This crate is a client of the docs engine over loopback HTTP and never
//! links `oo-docs` (architecture rule R-CLIENT-DB-DRIVER): `oo serve` runs the
//! engine and a token-checking reverse proxy, `oo context` talks to the proxy.

pub mod agent;
pub mod artifacts;
pub mod autostart;
pub mod client;
pub mod compose;
pub mod context;
pub mod dockercli;
pub mod docs_cmd;
pub mod embedded;
pub mod engine;
pub mod federate;
pub mod mounts;
pub mod ontology;
pub mod proxy;
pub mod runtime;
pub mod s3io;
pub mod secrets;
pub mod serve;
pub mod sidecar;
pub mod source;
pub mod source_db;
pub mod source_link;
pub mod suggest;
pub mod upload;

use crate::args::Invocation;
use crate::outcome::Failure;

const HELP: &str = "Usage:\n  oo context mount [--drive <id|name>] [--scope <path>]... --to <dir> [--for <text> | --for-file <path>] [--seed <entity-key>]... [--budget <n>] [--role viewer|suggester] [--expires <dur>] [--version <N>] [--group <ticket>] [--refresh-policy manual|at-checkpoint|pinned] [--require-complete] [--wait <dur>] [--strict-budget] [--dry-run] [--json]\n  oo context list [--drive <id|name>] [--status <state>] [--json]\n  oo context status <dir|share-id> [--json]\n  oo context refresh <dir|share-id> | --group <ticket> [--json]\n  oo context revoke <dir|share-id> [--json]\n  oo context check [--dir .context] [--since <seq>] [--json]\n  oo context collect <dir|share-id> [--json]\n  oo docs drive|files|review|entity|share|usage|health ... (see `oo docs --help`)\n  oo serve [--state-dir <dir>] [--port <n>|0] [--runtime auto|compose|embedded|host] [--deploy-dir <dir>] [--keep-engine] [--json]\n  oo serve --status [--json]\n  oo serve --stop\n";

/// `serve` dispatch. Runs the daemon to completion; the process exit code is
/// the daemon's (10 §8.6 point 9).
pub fn dispatch(args: &[String]) -> Option<Result<Invocation, Failure>> {
    let (command, rest) = args.split_first()?;
    match command.as_str() {
        "context" if rest.iter().any(|a| a == "--help" || a == "-h") => {
            Some(Ok(Invocation::Help(HELP.into())))
        }
        "docs" if rest.iter().any(|a| a == "--help" || a == "-h") => {
            Some(Ok(Invocation::Help(docs_cmd::HELP.into())))
        }
        "agent" if rest.iter().any(|a| a == "--help" || a == "-h") => {
            Some(Ok(Invocation::Help(agent::HELP.into())))
        }
        "agent" => std::process::exit(i32::from(agent::run(rest))),
        "ontology"
            if rest.is_empty()
                || (rest.len() == 1 && matches!(rest[0].as_str(), "--help" | "-h")) =>
        {
            Some(Ok(Invocation::Help(format!(
                "{}\n{}",
                ontology::HELP.trim_end(),
                crate::args::ONTOLOGY_ADMIN_HELP
            ))))
        }
        "ontology" if ontology::is_subcommand(rest) => {
            std::process::exit(i32::from(ontology::run(rest)))
        }
        "mcp" if rest.iter().any(|a| a == "--endpoint") => Some(ontology::mcp(rest)),
        "docs" => std::process::exit(i32::from(docs_cmd::run(rest))),
        "source" if rest.iter().any(|a| a == "--help" || a == "-h") => Some(Ok(Invocation::Help(
            format!("{}{}", source::HELP, source_link::HELP),
        ))),
        "source" => std::process::exit(i32::from(source::run(rest))),
        "context" => std::process::exit(i32::from(context::run(rest))),
        "serve" if rest.iter().any(|a| a == "--help" || a == "-h") => {
            Some(Ok(Invocation::Help(HELP.into())))
        }
        "serve" => {
            // The daemon speaks for itself (one JSON line with --json), so the
            // caller's report printer must not add a line of its own.
            std::process::exit(i32::from(serve::run(rest)))
        }
        _ => None,
    }
}
