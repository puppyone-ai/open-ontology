//! Offline role-Pack exports. No source, Gateway or credential access.
#[path = "ontology_export.rs"]
pub mod export;

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const HELP: &str = "\
oo ontology export --format html|mermaid|yaml --out PATH [--pack PACK.json] [--role R]
oo ontology open [--print-path] [--pack PACK.json] [--role R]
Common options: --config stack.yaml --yes --json
Default Pack: .oo/role-pack.json beside --config (default stack.yaml).
--role asserts the supplied Pack's role; it never grants or expands visibility.
HTML is one offline file. Mermaid is Markdown with bounded ER pages.
YAML is not yet available: role Packs do not contain reviewed effective OOS.
Existing outputs are never overwritten. open creates a private temporary file.";

/// Returns presentation text for the admin dispatcher; errors never echo input.
pub fn run(rest: &[String]) -> Result<String, &'static str> {
    if rest.iter().any(|s| s == "--help" || s == "-h") {
        return Ok(HELP.into());
    }
    let mut args = Vec::new();
    for arg in rest {
        if arg.starts_with("--")
            && let Some((key, value)) = arg.split_once('=')
        {
            args.extend([key.to_owned(), value.to_owned()]);
        } else {
            args.push(arg.clone());
        }
    }
    let action = args.first().map(String::as_str).ok_or("ADMIN_ARGUMENT")?;
    if !matches!(action, "open" | "export") {
        return Err("ADMIN_ARGUMENT");
    }
    let mut opts = BTreeMap::new();
    let mut index = 1;
    while index < args.len() {
        let key = args[index].as_str();
        if opts.contains_key(key) {
            return Err("ADMIN_ARGUMENT");
        }
        let value = match key {
            "--print-path" | "--json" | "--yes" => "",
            "--config" | "--pack" | "--role" | "--format" | "--out" => {
                index += 1;
                args.get(index)
                    .map(String::as_str)
                    .filter(|s| !s.is_empty() && !s.starts_with('-'))
                    .ok_or("ADMIN_ARGUMENT")?
            }
            _ => return Err("ADMIN_ARGUMENT"),
        };
        opts.insert(key, value);
        index += 1;
    }
    let format = opts.get("--format").copied().unwrap_or("html");
    if !matches!(format, "html" | "mermaid" | "yaml")
        || (action == "open" && (opts.contains_key("--format") || opts.contains_key("--out")))
        || (action == "export"
            && (!opts.contains_key("--out") || opts.contains_key("--print-path")))
    {
        return Err("ADMIN_ARGUMENT");
    }
    // Full effective OOS is not visibility-closed. Never silently substitute it.
    if format == "yaml" {
        return Err("ONTOLOGY_YAML_ROLE_OOS_UNAVAILABLE");
    }
    let config = Path::new(opts.get("--config").copied().unwrap_or("stack.yaml"));
    let default_pack = config
        .parent()
        .unwrap_or(Path::new("."))
        .join(".oo/role-pack.json");
    let pack_path = opts
        .get("--pack")
        .map(PathBuf::from)
        .unwrap_or(default_pack);
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(pack_path)
        .map_err(|_| "ONTOLOGY_PACK_IO")?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= export::MAX_BYTES)
    {
        return Err("ONTOLOGY_PACK_BUDGET");
    }
    let mut text = String::new();
    file.take(export::MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "ONTOLOGY_PACK_IO")?;
    let model = export::Model::parse(&text, opts.get("--role").copied())?;
    let body = if format == "html" {
        model.html()?
    } else {
        model.mermaid()?
    };
    let (path, mut output) = if action == "open" {
        temporary()?
    } else {
        let path = PathBuf::from(opts["--out"]);
        let output = private_file(&path)?;
        (path, output)
    };
    if output
        .write_all(body.as_bytes())
        .and_then(|_| output.sync_all())
        .is_err()
    {
        drop(output);
        let _ = fs::remove_file(&path);
        return Err("ONTOLOGY_OUTPUT_IO");
    }
    drop(output);
    let path = fs::canonicalize(path).map_err(|_| "ONTOLOGY_OUTPUT_IO")?;
    if action == "open" && !opts.contains_key("--print-path") {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let status = Command::new(opener)
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "ONTOLOGY_OPENER_FAILED: use --print-path")?;
        if !status.success() {
            return Err("ONTOLOGY_OPENER_FAILED: use --print-path");
        }
    }
    Ok(if opts.contains_key("--json") {
        serde_json::json!({"protocol":"oo.ontology-export/v1", "path":path,
            "format":format,"role":model.role,"publication":"not_claimed"})
        .to_string()
    } else {
        path.display().to_string()
    })
}

fn private_file(path: &Path) -> Result<std::fs::File, &'static str> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "ONTOLOGY_OUTPUT_IO")
}

fn temporary() -> Result<(PathBuf, std::fs::File), &'static str> {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "ONTOLOGY_OUTPUT_IO")?
        .as_nanos();
    for attempt in 0..100 {
        let path = std::env::temp_dir().join(format!(
            "oo-ontology-{}-{time}-{attempt}.html",
            std::process::id()
        ));
        if let Ok(file) = private_file(&path) {
            return Ok((path, file));
        }
    }
    Err("ONTOLOGY_OUTPUT_IO")
}
