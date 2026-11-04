//! Administrative commands; consumer credentials are never read here.
mod doctor;
mod lite;
mod ontology;
mod progress;
mod wizard;
use oo_stack::stack;

use super::Invocation;
use crate::outcome::Failure;
use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub const HELP: &str = "\
Admin commands (source connections remain read-only):
  oo init|up|down|status|doctor [--config stack.yaml] [--yes] [--json]
  oo build [--resume] [--config stack.yaml] [--agent-target target.json] [--yes] [--json]
  oo ontology open|export [--format html|mermaid|yaml] [--config stack.yaml] [--yes] [--json]
  oo token issue|list|renew|rotate|revoke|enroll|approve (see oo help token)
init --config validates and atomically normalizes that file as JSON.
YAML and JSON are accepted. The filename does not select the parser.
TTY init collects configuration and runs preflight; non-TTY never prompts.
The wizard can save or run a bounded PostgreSQL metadata-to-draft bridge.
build requires pg-builder and oo-pipeline beside oo; --resume retains checkpoints.
--agent-target uses oo-load to load explicit source tables before drafting.
The target file is private oo-load JSON; TLS and secret_ref credentials are required.
Agent DB loading, review and publication are pending integration.
status reports saved build state; missing Gateway health fields stay unmeasured.
up bootstraps private credentials and starts only lite foundations; exit 1
and foundations_only mean worker/scheduler/Gateway integration is still pending.
up requires a repository-built OO_LITE_RUNTIME_IMAGE content digest.
down stops this deployment only; database volumes and credentials are retained.
doctor probes sources read-only and the model through model-egress.
PostgreSQL/MySQL require tls_ca.secret_ref and explicit scope.include tables.
MySQL requires oo-mysql-doctor beside oo.
Unknown estimates remain null; blocked diagnostics exit nonzero.
status <query_id> remains the consumer query-status command.
Other admin operations are not yet implemented (ADMIN_NOT_IMPLEMENTED).";

pub fn dispatch(args: &[String]) -> Option<Result<Invocation, Failure>> {
    let (command, rest) = args.split_first()?;
    if command == "help" && rest.first().is_some_and(|c| c == "ontology") {
        return Some(Ok(Invocation::Help(ontology::HELP.into())));
    }
    if command == "help" && rest.first().is_some_and(|c| is_admin(c) && c != "status") {
        return Some(Ok(Invocation::Help(HELP.into())));
    }
    if !is_admin(command) {
        return None;
    }
    if command == "status" && !admin_status(rest) {
        return None;
    }
    Some(execute(command, rest))
}

fn is_admin(command: &str) -> bool {
    matches!(
        command,
        "init"
            | "up"
            | "down"
            | "status"
            | "doctor"
            | "build"
            | "ontology"
            | "token"
            | "share-prompt"
    )
}

fn admin_status(args: &[String]) -> bool {
    args.is_empty()
        || args
            .iter()
            .any(|s| s == "--config" || s.starts_with("--config=") || s == "--yes")
        || args.iter().all(|s| s == "--json")
}

fn execute(command: &str, rest: &[String]) -> Result<Invocation, Failure> {
    if command == "ontology" {
        return ontology::run(rest)
            .map(Invocation::Help)
            .map_err(|message| {
                if message == "ADMIN_ARGUMENT" {
                    Failure::usage(message)
                } else {
                    Failure::local(message)
                }
            });
    }
    if rest.iter().any(|s| s == "--help" || s == "-h") {
        return Ok(Invocation::Help(HELP.into()));
    }
    let mut args = Vec::new();
    for arg in rest {
        if let Some((flag, value)) = arg.split_once('=')
            && matches!(flag, "--config" | "--format" | "--role" | "--agent-target")
        {
            args.extend([flag.to_owned(), value.to_owned()]);
        } else {
            args.push(arg.clone());
        }
    }
    let mut config = None;
    let mut json = false;
    let mut resume = false;
    let mut agent_target = None;
    let mut seen = std::collections::HashSet::new();
    let mut positionals = Vec::new();
    let mut format = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg.starts_with('-') && !seen.insert(arg) {
            return Err(Failure::usage("ADMIN_ARGUMENT: duplicate option"));
        }
        match arg {
            "--json" => json = true,
            "--yes" => (),
            "--resume" if command == "build" => resume = true,
            "--config" | "--format" | "--role" | "--agent-target" => {
                index += 1;
                let value = args
                    .get(index)
                    .filter(|s| !s.is_empty() && !s.starts_with('-'))
                    .ok_or_else(|| Failure::usage("ADMIN_ARGUMENT: missing option value"))?;
                match arg {
                    "--config" => config = Some(value.as_str()),
                    "--agent-target" if command == "build" => agent_target = Some(Path::new(value)),
                    "--format"
                        if command == "ontology"
                            && matches!(value.as_str(), "html" | "mermaid" | "yaml") =>
                    {
                        format = true
                    }
                    "--role" if command == "share-prompt" && stack::secret_name(value) => (),
                    _ => return Err(Failure::usage("ADMIN_ARGUMENT: invalid option")),
                }
            }
            _ if !arg.starts_with('-') => positionals.push(arg),
            _ => return Err(Failure::usage("ADMIN_ARGUMENT: unknown option")),
        }
        index += 1;
    }
    let valid = match command {
        "ontology" => {
            matches!(positionals.as_slice(), ["open"] | ["export"])
                && (!format || positionals == ["export"])
        }
        "token" => matches!(
            positionals.as_slice(),
            ["issue"] | ["list"] | ["renew"] | ["rotate"] | ["revoke"]
        ),
        _ => positionals.is_empty(),
    };
    if !valid {
        return Err(Failure::usage(
            "ADMIN_ARGUMENT: missing or invalid subcommand/argument",
        ));
    }
    if command == "doctor" {
        let path = Path::new(config.unwrap_or("stack.yaml"));
        let stack = read_stack(path)?;
        return doctor::run(&stack, json);
    }
    if command == "build" {
        let path = Path::new(config.unwrap_or("stack.yaml"));
        return progress::build(path, &read_stack(path)?, resume, json, agent_target);
    }
    if command == "status" {
        let path = Path::new(config.unwrap_or("stack.yaml"));
        let stack = read_stack(path)?;
        let root = std::fs::canonicalize(path).map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
        if root
            .parent()
            .is_some_and(|p| p.join(".oo/lite/owner.json").exists())
        {
            return lite::run("status", path, &stack);
        }
        return progress::status(path, json);
    }
    if matches!(command, "up" | "down") {
        let path = Path::new(config.unwrap_or("stack.yaml"));
        return lite::run(command, path, &read_stack(path)?);
    }
    if command != "init" {
        return Err(Failure::local(
            "ADMIN_NOT_IMPLEMENTED: administrative operation is not yet available",
        ));
    }
    let Some(config) = config else {
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && !json {
            return wizard::run();
        }
        return Err(Failure::usage(
            "ADMIN_CONFIG_REQUIRED: use --config for non-interactive init",
        ));
    };
    normalize(Path::new(config))?;
    Ok(Invocation::Help(if json {
        serde_json::json!({"protocol":"oo.admin/v1","command":"init","status":"validated",
            "format":"json","normalized":true,"deployed":false})
        .to_string()
    } else {
        "Stack validated and normalized as JSON; no services started.".into()
    }))
}

fn read_stack(path: &Path) -> Result<stack::Stack, Failure> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
    let meta = file
        .metadata()
        .map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
    if !meta.is_file() || meta.len() > 1_048_576 || meta.nlink() != 1 {
        return Err(Failure::local("ADMIN_CONFIG_IO"));
    }
    let mut text = String::new();
    file.take(1_048_577)
        .read_to_string(&mut text)
        .map_err(|_| Failure::local("ADMIN_CONFIG_IO"))?;
    stack::parse(&text).map_err(Failure::usage)
}

fn normalize(path: &Path) -> Result<(), Failure> {
    let io_error = || Failure::local("ADMIN_CONFIG_IO: config must be a writable regular file");
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| io_error())?;
    let meta = file.metadata().map_err(|_| io_error())?;
    if !meta.is_file() || meta.len() > 1_048_576 || meta.nlink() != 1 {
        return Err(io_error());
    }
    let mut text = String::new();
    file.take(1_048_577)
        .read_to_string(&mut text)
        .map_err(|_| io_error())?;
    let config = stack::parse(&text).map_err(Failure::usage)?;
    let mut bytes = serde_json::to_vec_pretty(&config).map_err(|_| io_error())?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = parent.join(format!(".oo-stack-{}.tmp", std::process::id()));
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temp)
        .map_err(|_| io_error())?;
    let result = (|| {
        out.write_all(&bytes)
            .and_then(|_| out.sync_all())
            .map_err(|_| io_error())?;
        let current = fs::symlink_metadata(path).map_err(|_| io_error())?;
        if !current.is_file() || current.ino() != meta.ino() || current.dev() != meta.dev() {
            return Err(io_error());
        }
        fs::rename(&temp, path).map_err(|_| io_error())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_target_is_build_only_and_requires_one_value() {
        for values in [
            vec!["build", "--agent-target"],
            vec!["up", "--agent-target", "target.json"],
            vec!["build", "--agent-target", "a", "--agent-target=b"],
        ] {
            let args = values.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert!(dispatch(&args).unwrap().is_err());
        }
    }

    #[test]
    fn init_normalizes_yaml_for_json_consumers() {
        let dir = std::env::temp_dir().join(format!("oo-admin-yaml-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("stack.yaml");
        // Flow YAML deliberately uses an unquoted key, so this is not JSON.
        let yaml = include_str!("../../../oo-stack/example.stack.json").replacen(
            "\"version\":",
            "version:",
            1,
        );
        fs::write(&path, yaml).unwrap();
        let args = vec![
            "init".into(),
            "--config".into(),
            path.to_str().unwrap().into(),
            "--yes".into(),
            "--json".into(),
        ];
        assert!(dispatch(&args).unwrap().is_ok());
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["model"]["key"]["secret_ref"], "model-key");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        fs::remove_dir_all(dir).unwrap();
    }
}
