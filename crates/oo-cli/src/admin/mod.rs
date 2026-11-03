//! Administrative skeleton; never reads consumer credentials or stdin.
use oo_stack::stack;

use super::Invocation;
use crate::outcome::Failure;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub const HELP: &str = "\
Admin commands (skeleton; no deployment or source writes):
  oo init|up|down|status|doctor [--config stack.yaml] [--yes] [--json]
  oo build [--resume] [--config stack.yaml] [--yes] [--json]
  oo ontology open|export [--format html|mermaid|yaml] [--config stack.yaml] [--yes] [--json]
  oo token issue|list|renew|rotate|revoke [--config stack.yaml] [--yes] [--json]
  oo share-prompt [--role R] [--config stack.yaml] [--yes] [--json]
init --config validates and atomically normalizes that file as JSON.
YAML and JSON are accepted. The filename does not select the parser.
TTY wizard is not yet implemented (S2). Non-TTY never prompts.
status <query_id> remains the consumer query-status command.
Other admin operations return ADMIN_NOT_IMPLEMENTED, not a success receipt.";

pub fn dispatch(args: &[String]) -> Option<Result<Invocation, Failure>> {
    let (command, rest) = args.split_first()?;
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
    if rest.iter().any(|s| s == "--help" || s == "-h") {
        return Ok(Invocation::Help(HELP.into()));
    }
    let mut args = Vec::new();
    for arg in rest {
        if let Some((flag, value)) = arg.split_once('=')
            && matches!(flag, "--config" | "--format" | "--role")
        {
            args.extend([flag.to_owned(), value.to_owned()]);
        } else {
            args.push(arg.clone());
        }
    }
    let mut config = None;
    let mut json = false;
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
            "--resume" if command == "build" => (),
            "--config" | "--format" | "--role" => {
                index += 1;
                let value = args
                    .get(index)
                    .filter(|s| !s.is_empty() && !s.starts_with('-'))
                    .ok_or_else(|| Failure::usage("ADMIN_ARGUMENT: missing option value"))?;
                match arg {
                    "--config" => config = Some(value.as_str()),
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
    if command != "init" {
        return Err(Failure::local(
            "ADMIN_NOT_IMPLEMENTED: administrative operation is not yet available",
        ));
    }
    let Some(config) = config else {
        return Err(Failure::usage(
            "ADMIN_CONFIG_REQUIRED: use --config; interactive wizard is not yet available (S2)",
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
