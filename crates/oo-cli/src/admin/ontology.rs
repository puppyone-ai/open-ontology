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
oo ontology export --format html|mermaid --out PATH [--pack PACK.json] [--role R]
oo ontology export --format yaml --out PATH [--registry reviewed-registry.json]
oo ontology open [--print-path] [--pack PACK.json] [--role R]
oo ontology graph [--adjudication FILE] [--format ascii|text|mermaid] (see oo ontology graph --help)
Common options: --config stack.yaml --yes --json
Default Pack: .oo/role-pack.json beside --config (default stack.yaml).
--role asserts the supplied Pack's role; it never grants or expands visibility.
HTML is one offline file. Mermaid is Markdown with bounded ER pages.
YAML is the owner's full reviewed ontology (the effective OOS of reviewed-registry.json; default
.oo/lite/work/build/.oo/reviewed-registry.json beside --config). It is not role-closed: it never
comes from a Pack, and --pack/--role do not apply to it (role-closed YAML is not yet available).
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
            "--config" | "--pack" | "--role" | "--format" | "--out" | "--registry" => {
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
    // The full effective OOS is not visibility-closed: only the owner's reviewed
    // registry may be its source, and only when asked for explicitly.
    if format == "yaml" {
        if action != "export" || opts.contains_key("--pack") || opts.contains_key("--role") {
            return Err("ADMIN_ARGUMENT");
        }
        let config = Path::new(opts.get("--config").copied().unwrap_or("stack.yaml"));
        let registry = opts
            .get("--registry")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                config
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(".oo/lite/work/build/.oo/reviewed-registry.json")
            });
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(registry)
            .map_err(|_| "ONTOLOGY_REGISTRY_IO")?;
        if !file
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= 64 * 1024 * 1024)
        {
            return Err("ONTOLOGY_REGISTRY_BUDGET");
        }
        let mut text = String::new();
        file.take(64 * 1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|_| "ONTOLOGY_REGISTRY_IO")?;
        let body = oos_yaml(&text)?;
        let path = PathBuf::from(opts["--out"]);
        let mut output = private_file(&path)?;
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
        return Ok(if opts.contains_key("--json") {
            serde_json::json!({"protocol":"oo.ontology-export/v1","format":"yaml","path":path})
                .to_string()
        } else {
            format!("Ontology YAML written to {}.", path.display())
        });
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

/// The effective OOS of an `oo.owner-reviewed-registry/v1` as YAML.
fn oos_yaml(registry: &str) -> Result<String, &'static str> {
    let value: serde_json::Value =
        serde_json::from_str(registry).map_err(|_| "ONTOLOGY_REGISTRY_SHAPE")?;
    if value["protocol"] != "oo.owner-reviewed-registry/v1"
        || !value["compiled"]["effective"].is_object()
    {
        return Err("ONTOLOGY_REGISTRY_SHAPE");
    }
    let mut out = String::new();
    yaml_value(&value["compiled"]["effective"], 0, &mut out);
    Ok(out)
}

/// Block-style YAML. Scalars are JSON-quoted (a YAML 1.2 subset), so every
/// string, number and keyword round-trips without ambiguity.
fn yaml_value(v: &serde_json::Value, indent: usize, out: &mut String) {
    use serde_json::Value;
    let pad = " ".repeat(indent);
    match v {
        Value::Object(map) if !map.is_empty() => {
            for (k, x) in map {
                out.push_str(&format!("{pad}{}:", Value::String(k.clone())));
                yaml_child(x, indent, out);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for x in items {
                out.push_str(&format!("{pad}-"));
                yaml_child(x, indent, out);
            }
        }
        scalar => out.push_str(&format!("{pad}{}\n", yaml_scalar(scalar))),
    }
}

fn yaml_child(x: &serde_json::Value, indent: usize, out: &mut String) {
    use serde_json::Value;
    match x {
        Value::Object(m) if !m.is_empty() => {
            out.push('\n');
            yaml_value(x, indent + 2, out);
        }
        Value::Array(a) if !a.is_empty() => {
            out.push('\n');
            yaml_value(x, indent + 2, out);
        }
        other => out.push_str(&format!(" {}\n", yaml_scalar(other))),
    }
}

fn yaml_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(_) => "{}".into(),
        serde_json::Value::Array(_) => "[]".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod yaml_tests {
    use super::*;

    #[test]
    fn effective_oos_becomes_block_yaml_that_reads_back_as_the_same_value() {
        let registry = serde_json::json!({
            "protocol":"oo.owner-reviewed-registry/v1",
            "compiled":{"effective":{"project":"p","datasets":[{"dataset":"erp.orders",
                "columns":[{"name":"id","pii":"none"},{"name":"x: y","pii":null}]}],
                "enums":[],"policy":{},"n":1.5,"flag":true}}});
        let yaml = oos_yaml(&registry.to_string()).unwrap();
        assert!(yaml.contains("\"project\": \"p\""), "{yaml}");
        assert!(yaml.contains("\"enums\": []") && yaml.contains("\"policy\": {}"));
        // Every line is `key: scalar`, `key:` or `-` with a nested block: no flow
        // collections except the empty ones, so a YAML reader recovers the JSON.
        for line in yaml.lines() {
            let t = line.trim_start();
            assert!(t.starts_with('"') || t.starts_with('-'), "{line}");
        }
        assert!(oos_yaml("{}").is_err());
        assert!(oos_yaml("not json").is_err());
    }
}
