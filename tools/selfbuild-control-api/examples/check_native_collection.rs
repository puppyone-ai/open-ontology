//! Read-only check of the exact collection/profile/semantics consumer boundary.

use std::{env, fs, process::ExitCode};

use selfbuild_control_api::adapt::{ProtocolPin, project_native_collection};
use serde_json::{Value, json};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("usage: check_native_collection CATALOG PROFILE SEMANTICS");
        return ExitCode::FAILURE;
    }
    let files = args.iter().map(fs::read).collect::<Result<Vec<_>, _>>();
    let Ok(files) = files else { eprintln!("input.read"); return ExitCode::FAILURE; };
    let profile: Value = match serde_json::from_slice(&files[1]) {
        Ok(value) => value,
        Err(_) => { eprintln!("profile.json"); return ExitCode::FAILURE; }
    };
    let semantics: Value = match serde_json::from_slice(&files[2]) {
        Ok(value) => value,
        Err(_) => { eprintln!("semantics.json"); return ExitCode::FAILURE; }
    };
    let pin = ProtocolPin {
        catalog: "oo.pipeline.sealed-import-collection/v1".to_owned(),
        profile: profile.get("protocol").and_then(Value::as_str).unwrap_or("").to_owned(),
        semantics: semantics.get("protocol").and_then(Value::as_str).unwrap_or("").to_owned(),
        source_commit: "read_only_probe".to_owned(),
        semantics_commit: "read_only_probe".to_owned(),
    };
    match project_native_collection(&files[0], &files[1], &files[2], &pin) {
        Ok(projection) => {
            let receipt = &projection.receipt;
            println!("{}", json!({
                "outcome": "verified_projection",
                "catalog_sha256": receipt["catalog_sha256"],
                "profile_sha256": receipt["profile_sha256"],
                "semantics_sha256": receipt["semantics_sha256"],
                "collection_sha256": receipt["collection_sha256"],
                "generation_semantics": receipt["generation_semantics"],
                "binding_vector": receipt["binding_vector"],
                "source_ids": receipt["source_ids"],
                "table_count": projection.typed_catalog["tables"].as_array().map_or(0, Vec::len),
                "column_count": receipt["columns"].as_array().map_or(0, Vec::len),
            }));
            ExitCode::SUCCESS
        }
        Err(err) => { eprintln!("{}", err.code); ExitCode::FAILURE }
    }
}
