//! Bind one sealed catalog to `compile_source_semantics`.
//!
//! `catalog_version` is the identity hash from `compile_sealed_import_catalog`.
//! Column flags stay in the target file. This process does not edit them.
//! The pack text is the compiler result.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

use serde_json::{Value, json};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(detail) => {
            let _ = writeln!(io::stderr(), "oo-source-semantics-pack: {detail}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8, String> {
    let mut args = env::args().skip(1);
    let catalog_path = args.next().ok_or("usage")?;
    let target_path = args.next().ok_or("usage")?;
    let settings_path = args.next().ok_or("usage")?;
    let visibility_path = args.next().ok_or("usage")?;
    let decisions_path = args.next().ok_or("usage")?;
    let out_dir = args.next().ok_or("usage")?;
    if args.next().is_some() {
        return Err("usage".to_string());
    }
    let catalog_text = fs::read_to_string(&catalog_path).map_err(|err| err.to_string())?;
    let draft = oo_builder::compile_sealed_import_catalog(&catalog_text)
        .map_err(|err| format!("{} at {}: {}", err.code, err.path, err.message))?;
    let mut settings = read_json(&settings_path)?;
    let mut visibility = read_json(&visibility_path)?;
    let target = read_json(&target_path)?;
    let decisions = read_json(&decisions_path)?;
    settings
        .as_object_mut()
        .ok_or("settings")?
        .insert("catalog_version".to_string(), json!(draft.identity_hash));
    visibility
        .as_object_mut()
        .ok_or("visibility")?
        .insert("catalog_version".to_string(), json!(draft.identity_hash));
    let command = json!({
        "protocol": source_semantics::COMMAND_PROTOCOL,
        "catalog": serde_json::from_str::<Value>(&catalog_text).map_err(|err| err.to_string())?,
        "target": target,
        "binding": {
            "settings": settings,
            "decisions": decisions,
            "visibility": visibility,
            "examples": []
        }
    });
    let command_text = serde_json::to_string(&command).map_err(|err| err.to_string())?;
    fs::create_dir_all(&out_dir).map_err(|err| err.to_string())?;
    fs::write(
        format!("{out_dir}/identity-hash.txt"),
        format!("{}\n", draft.identity_hash),
    )
    .map_err(|err| err.to_string())?;
    fs::write(
        format!("{out_dir}/schema-digest.txt"),
        format!("{}\n", draft.schema_digest),
    )
    .map_err(|err| err.to_string())?;
    let compiled = match source_semantics::compile_source_semantics(&command_text) {
        Ok(text) => text,
        Err(err) => {
            let detail = format!("{} at {}: {}", err.code, err.path, err.message);
            fs::write(format!("{out_dir}/compiler-error.txt"), format!("{detail}\n"))
                .map_err(|io_err| io_err.to_string())?;
            let _ = writeln!(
                io::stderr(),
                "status=error identity_hash={} schema_digest={}",
                draft.identity_hash, draft.schema_digest
            );
            return Err(detail);
        }
    };
    fs::write(format!("{out_dir}/result.json"), format!("{compiled}\n")).map_err(|err| err.to_string())?;
    let result: Value = serde_json::from_str(&compiled).map_err(|err| err.to_string())?;
    let status = result.get("status").and_then(Value::as_str).unwrap_or("").to_string();
    if let Some(pages) = result.pointer("/pack/pages").and_then(Value::as_object) {
        for (path, page) in pages {
            let Some(text) = page.as_str() else {
                continue;
            };
            let name = path.replace('/', "__");
            fs::write(format!("{out_dir}/{name}"), text).map_err(|err| err.to_string())?;
        }
    }
    let _ = writeln!(
        io::stderr(),
        "status={status} identity_hash={} schema_digest={}",
        draft.identity_hash, draft.schema_digest
    );
    Ok(if status == "packed" { 0 } else { 3 })
}

fn read_json(path: &str) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&text).map_err(|err| err.to_string())
}
