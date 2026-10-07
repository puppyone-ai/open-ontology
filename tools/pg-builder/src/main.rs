//! PostgreSQL builder CLI. Paths only. The bearer stays in its file.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pg_builder::{
    assemble_cards, canonical_json, canonical_sha256, generate, load_config, pack_pair, read_bearer,
    read_comments, read_profile, review_only, run_catalog, scaffold_curated, seal_request, GenerateIn,
    LoopbackTransport, PackIn, PgBuilderError,
};
use serde_json::{json, Value};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("{} {}", err.code, err.message);
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<u8, PgBuilderError> {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprintln!("pg-builder catalog|profile|comments|seal|cards|curated|approve|generate|review|pack|scan");
        return Ok(2);
    };
    match command.as_str() {
        "catalog" => catalog(&args[1..]),
        "profile" => profile(&args[1..]),
        "comments" => comments(&args[1..]),
        "seal" => seal(&args[1..]),
        "cards" => cards(&args[1..]),
        "curated" => curated(&args[1..]),
        "approve" => approve(&args[1..]),
        "generate" => generate_cmd(&args[1..]),
        "review" => review_cmd(&args[1..]),
        "pack" => pack_cmd(&args[1..]),
        "scan" => scan(&args[1..]),
        _ => {
            eprintln!("pg-builder catalog|profile|comments|seal|cards|curated|approve|generate|review|pack|scan");
            Ok(2)
        }
    }
}

fn catalog(args: &[String]) -> Result<u8, PgBuilderError> {
    let config = load_config(&read_text(&flag(args, "--config")?)?)?;
    let stderr = optional(args, "--stderr").map(PathBuf::from);
    let bytes = run_catalog(&config, &flag(args, "--source-id")?, &flag(args, "--secret-ref")?, stderr.as_deref())?;
    fs::write(flag(args, "--out")?, bytes).map_err(|_| PgBuilderError::new("output.write", "the output file could not be written"))?;
    Ok(0)
}

fn profile(args: &[String]) -> Result<u8, PgBuilderError> {
    let config = load_config(&read_text(&flag(args, "--config")?)?)?;
    write_json(&flag(args, "--out")?, &read_profile(&config)?)?;
    Ok(0)
}

fn comments(args: &[String]) -> Result<u8, PgBuilderError> {
    let config = load_config(&read_text(&flag(args, "--config")?)?)?;
    write_json(&flag(args, "--out")?, &read_comments(&config)?)?;
    Ok(0)
}

fn seal(args: &[String]) -> Result<u8, PgBuilderError> {
    let catalog = read_text(&flag(args, "--catalog")?)?;
    write_json(&flag(args, "--out")?, &seal_request(&catalog)?)?;
    Ok(0)
}

fn cards(args: &[String]) -> Result<u8, PgBuilderError> {
    let config = load_config(&read_text(&flag(args, "--config")?)?)?;
    let catalog = read_json(&flag(args, "--catalog")?)?;
    let profile = read_json(&flag(args, "--profile")?)?;
    let comments = read_json(&flag(args, "--comments")?)?;
    let seal = read_json(&flag(args, "--seal")?)?;
    let schema = seal
        .get("schema_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "the seal has no schema hash"))?;
    let evidence = seal
        .get("evidence_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("catalog.schema", "the seal has no evidence hash"))?;
    let cards = assemble_cards(&catalog, &profile, &comments, schema, evidence, config.max_tables)?;
    write_json(&flag(args, "--out")?, &cards)?;
    Ok(0)
}

fn curated(args: &[String]) -> Result<u8, PgBuilderError> {
    let catalog = read_json(&flag(args, "--catalog")?)?;
    write_json(&flag(args, "--out")?, &scaffold_curated(&catalog)?)?;
    Ok(0)
}

fn approve(args: &[String]) -> Result<u8, PgBuilderError> {
    let cards = read_json(&flag(args, "--cards")?)?;
    let tables = cards
        .get("tables")
        .and_then(Value::as_array)
        .ok_or_else(|| PgBuilderError::new("approval.schema", "the cards have no tables"))?;
    let mut listed = Vec::new();
    for card in tables {
        listed.push(json!({
            "dataset": card.get("dataset").cloned().unwrap_or(Value::Null),
            "card_sha256": card.get("card_sha256").cloned().unwrap_or(Value::Null)
        }));
    }
    let approval = json!({
        "protocol": "oo.pg-builder.card-approval/v1",
        "approved": true,
        "authenticated": false,
        "parent_auth": "required",
        "reviewer_label": flag(args, "--reviewer-label")?,
        "reviewer_at": flag(args, "--reviewer-at")?,
        "evidence_hash": cards.get("evidence_hash").cloned().unwrap_or(Value::Null),
        "schema_hash": cards.get("schema_hash").cloned().unwrap_or(Value::Null),
        "cards_sha256": canonical_sha256(&cards),
        "tables": listed
    });
    write_json(&flag(args, "--out")?, &approval)?;
    Ok(0)
}

fn generate_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let config_path = flag(args, "--config")?;
    let config_bytes = read_bytes(&config_path)?;
    let config_text = String::from_utf8(config_bytes.clone()).map_err(|_| {
        PgBuilderError::new("input.schema", "an input file is not UTF-8")
    })?;
    let config = load_config(&config_text)?;
    let token = read_bearer(&config.bearer_file)?;
    let seal = read_json(&flag(args, "--seal")?)?;
    let command = seal
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| PgBuilderError::new("compiler.request", "the seal has no description command"))?
        .to_owned();
    let cards = read_json(&flag(args, "--cards")?)?;
    let approval = read_json(&flag(args, "--approval")?)?;
    let cache = PathBuf::from(flag(args, "--cache")?);
    let mut transport = LoopbackTransport {
        config: config.clone(),
        token: token.clone(),
    };
    let doc = generate(GenerateIn {
        config: &config,
        config_bytes: &config_bytes,
        token: &token,
        command_text: &command,
        cards: &cards,
        approval: &approval,
        cache_dir: &cache,
        transport: &mut transport,
    })?;
    write_json(&flag(args, "--out")?, &doc)?;
    Ok(0)
}

fn review_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let generation = read_json(&flag(args, "--generation")?)?;
    let curated = read_json(&flag(args, "--curated")?)?;
    let operator = read_json(&flag(args, "--operator")?)?;
    write_json(&flag(args, "--out")?, &review_only(&generation, &curated, &operator)?)?;
    Ok(0)
}

fn pack_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let generation = read_json(&flag(args, "--generation")?)?;
    let curated = read_json(&flag(args, "--curated")?)?;
    let operator = read_json(&flag(args, "--operator")?)?;
    let settings = read_json(&flag(args, "--settings")?)?;
    let classifications = read_json(&flag(args, "--classifications")?)?;
    let roles = read_json(&flag(args, "--roles")?)?;
    let hidden = repeated(args, "--hidden");
    let hidden_refs: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let packed = pack_pair(&PackIn {
        generation: &generation,
        curated: &curated,
        operator: &operator,
        settings: &settings,
        classifications: &classifications,
        roles: &roles,
        hidden_columns: &hidden_refs,
    })?;
    write_json(&flag(args, "--out")?, &packed)?;
    Ok(0)
}

fn scan(args: &[String]) -> Result<u8, PgBuilderError> {
    let root = PathBuf::from(flag(args, "--root")?);
    let bearer = PathBuf::from(flag(args, "--bearer-file")?);
    let token = read_bearer(&bearer)?;
    let bearer_canon = fs::canonicalize(&bearer).ok();
    let mut found = Vec::new();
    walk(&root, &token, bearer_canon.as_deref(), &mut found)?;
    if found.is_empty() {
        println!("bearer_scan=clean");
        Ok(0)
    } else {
        println!("bearer_scan=found");
        for path in found {
            println!("path={path}");
        }
        Ok(1)
    }
}

fn walk(dir: &Path, token: &str, bearer: Option<&Path>, found: &mut Vec<String>) -> Result<(), PgBuilderError> {
    let entries = fs::read_dir(dir).map_err(|_| PgBuilderError::new("scan.read", "a scan directory could not be read"))?;
    for entry in entries {
        let entry = entry.map_err(|_| PgBuilderError::new("scan.read", "a scan directory could not be read"))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| PgBuilderError::new("scan.read", "a scan path could not be read"))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(&path, token, bearer, found)?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if bearer.is_some_and(|bearer| fs::canonicalize(&path).ok().as_deref() == Some(bearer)) {
            continue;
        }
        if meta.len() > 32 * 1024 * 1024 {
            return Err(PgBuilderError::new("scan.budget", "a scanned file is outside the byte budget"));
        }
        let bytes = fs::read(&path).map_err(|_| PgBuilderError::new("scan.read", "a scan path could not be read"))?;
        if !token.is_empty() && bytes.windows(token.len()).any(|window| window == token.as_bytes()) {
            found.push(path.display().to_string());
        }
    }
    Ok(())
}

fn flag(args: &[String], name: &str) -> Result<String, PgBuilderError> {
    optional(args, name).ok_or_else(|| PgBuilderError::new("usage", "a required argument is missing"))
}

fn optional(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|pair| pair[0] == name).map(|pair| pair[1].clone())
}

fn repeated(args: &[String], name: &str) -> Vec<String> {
    args.windows(2).filter(|pair| pair[0] == name).map(|pair| pair[1].clone()).collect()
}

fn read_text(path: &str) -> Result<String, PgBuilderError> {
    let bytes = read_bytes(path)?;
    String::from_utf8(bytes).map_err(|_| PgBuilderError::new("input.schema", "an input file is not UTF-8"))
}

fn read_bytes(path: &str) -> Result<Vec<u8>, PgBuilderError> {
    let meta = fs::metadata(path).map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))?;
    if !meta.is_file() || meta.len() > 8 * 1024 * 1024 {
        return Err(PgBuilderError::new("input.read", "an input file could not be read"));
    }
    fs::read(path).map_err(|_| PgBuilderError::new("input.read", "an input file could not be read"))
}

fn read_json(path: &str) -> Result<Value, PgBuilderError> {
    let text = read_text(path)?;
    serde_json::from_str(&text).map_err(|_| PgBuilderError::new("input.schema", "an input file is not JSON"))
}

fn write_json(path: &str, value: &Value) -> Result<(), PgBuilderError> {
    let mut text = canonical_json(value);
    text.push('\n');
    fs::write(path, text).map_err(|_| PgBuilderError::new("output.write", "the output file could not be written"))
}
