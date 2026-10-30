//! Offline OOS source-tree compiler. No authentication, grants or publication.
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
type Result<T> = std::result::Result<T, String>;

fn read(path: &Path, max: u64) -> Result<String> {
    let meta = fs::symlink_metadata(path).map_err(|_| format!("io.read: {}", path.display()))?;
    if !meta.is_file() || meta.len() > max {
        return Err(format!("io.file_or_budget: {}", path.display()));
    }
    fs::read_to_string(path).map_err(|_| format!("io.utf8: {}", path.display()))
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("usage: oos-tree ONTOLOGY_DIR BINDING_JSON VISIBILITY_JSON CEILINGS_JSON NEW_OUTPUT_JSON".into());
    }
    let binding = read(Path::new(&args[1]), 1_048_576)?;
    let visibility = read(Path::new(&args[2]), 64 * 1024 * 1024)?;
    let ceilings = serde_json::from_str(&read(Path::new(&args[3]), 1_048_576)?)
        .map_err(|_| "ceilings.schema")?;
    let tree = oo_builder::compile_oos_tree(Path::new(&args[0]), &binding, &visibility, &ceilings)?;
    let output = json!({
        "protocol": "oo.builder.oos-tree/v1", "yaml_files": tree.yaml_files,
        "effective_oos": tree.effective_oos,
        "artifact": serde_json::from_str::<Value>(&tree.pack.artifact).map_err(|_| "pack.json")?,
        "pack_digest": tree.pack.pack_digest, "published": false,
        "authenticated": false, "business_owner_signed": false
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[4])
        .map_err(|_| "output.exists_or_unwritable")?;
    file.write_all(output.to_string().as_bytes())
        .map_err(|_| "output.write")?;
    file.sync_all().map_err(|_| "output.sync")?;
    println!("compiled OOS YAML tree; publication and business approval not claimed");
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("oos-tree: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
