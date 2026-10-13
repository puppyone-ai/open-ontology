//! PostgreSQL builder CLI. Paths only. The bearer stays in its file.

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pg_builder::{
    assemble_cards, bind_source_pack, canonical_json, canonical_sha256, compile_document_profile, generate, import_cards,
    import_seal, load_config, pack_combined, pack_pair, read_bearer, read_comments, read_profile,
    review_only, run_catalog, run_import, run_import_native_catalog, run_import_pg_catalog, seal_request_for_model,
    run_native_import, scaffold_curated, sealed_build, draft_with_source_contract, DocumentProfileIn,
    CombinedIn, GenerateIn, BIND_SOURCE_PACK_PROTOCOL, DOCUMENT_PROFILE_PROTOCOL,
    LoopbackTransport, PackIn, PgBuilderError, SealedBuild, SealedTarget,
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
        eprintln!("pg-builder catalog|profile|comments|seal|cards|curated|approve|generate|review|pack|pack-combined|scan|import|import-cards|import-seal|import-native|import-native-catalog|import-pg-catalog|document-profile|bind-source-pack|sealed|sealed-draft|sealed-review");
        return Ok(2);
    };
    match command.as_str() {
        "catalog" => catalog(&args[1..]),
        "profile" => profile(&args[1..]),
        "comments" => comments(&args[1..]),
        "seal" => seal(&args[1..]),
        "cards" => cards(&args[1..]),
        "import" => import_cmd(&args[1..]),
        "import-pg-catalog" => import_pg_catalog_cmd(&args[1..]),
        "import-cards" => import_cards_cmd(&args[1..]),
        "import-seal" => import_seal_cmd(&args[1..]),
        "import-native" => import_native_cmd(&args[1..]),
        "import-native-catalog" => import_native_catalog_cmd(&args[1..]),
        "document-profile" => document_profile_cmd(&args[1..]),
        "bind-source-pack" => bind_source_pack_cmd(&args[1..]),
        "sealed" => sealed_cmd(&args[1..]),
        "sealed-draft" => sealed_draft_cmd(&args[1..]),
        "sealed-review" => sealed_review_cmd(&args[1..]),
        "curated" => curated(&args[1..]),
        "approve" => approve(&args[1..]),
        "generate" => generate_cmd(&args[1..]),
        "review" => review_cmd(&args[1..]),
        "pack" => pack_cmd(&args[1..]),
        "pack-combined" => pack_combined_cmd(&args[1..]),
        "scan" => scan(&args[1..]),
        _ => {
            eprintln!("pg-builder catalog|profile|comments|seal|cards|curated|approve|generate|review|pack|pack-combined|scan|import|import-cards|import-seal|import-native|import-native-catalog|import-pg-catalog|document-profile|bind-source-pack|sealed|sealed-draft|sealed-review");
            Ok(2)
        }
    }
}

fn sealed_draft_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let catalog = read_text(&flag(args, "--catalog")?)?;
    let contract = optional(args, "--source-contract").map(|path| read_json(&path)).transpose()?;
    let result = draft_with_source_contract(&catalog, contract.as_ref())?;
    write_json(&flag(args, "--out")?, &result)?;
    Ok(if result.get("outcome").and_then(Value::as_str) == Some("draft_compiled") { 0 } else { 1 })
}

fn sealed_review_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let command = read_text(&flag(args, "--command")?)?;
    let reviewed = oo_builder::compile_reviewed_sealed_import(&command)
        .map_err(|err| PgBuilderError::forwarded(err.code, err.message))?;
    fs::write(flag(args, "--out")?, reviewed)
        .map_err(|_| PgBuilderError::new("output.write", "the output file could not be written"))?;
    Ok(0)
}

fn catalog(args: &[String]) -> Result<u8, PgBuilderError> {
    let config = load_config(&read_text(&flag(args, "--config")?)?)?;
    let stderr = optional(args, "--stderr").map(PathBuf::from);
    let bytes = run_catalog(&config, &flag(args, "--source-id")?, &flag(args, "--secret-ref")?, stderr.as_deref())?;
    fs::write(flag(args, "--out")?, bytes).map_err(|_| PgBuilderError::new("output.write", "the output file could not be written"))?;
    Ok(0)
}

fn import_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let request = flag(args, "--request")?;
    let out = flag(args, "--out")?;
    let stderr = optional(args, "--stderr").map(PathBuf::from);
    let bytes = run_import(Path::new(&request), stderr.as_deref())?;
    fs::write(out, bytes).map_err(|_| PgBuilderError::new("output.write", "the output file could not be written"))?;
    Ok(0)
}

fn import_pg_catalog_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let request = flag(args, "--request")?;
    let out = flag(args, "--out")?;
    let stderr = optional(args, "--stderr").map(PathBuf::from);
    let bytes = run_import_pg_catalog(Path::new(&request), stderr.as_deref())?;
    fs::write(out, bytes).map_err(|_| PgBuilderError::new("output.write", "the output file could not be written"))?;
    Ok(0)
}

fn import_native_catalog_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let request = flag(args, "--request")?;
    let out = flag(args, "--out")?;
    let pipeline = flag(args, "--pipeline")?;
    let stderr = optional(args, "--stderr").map(PathBuf::from);
    let bytes = run_import_native_catalog(Path::new(&request), Path::new(&pipeline), stderr.as_deref())?;
    fs::write(out, bytes).map_err(|_| PgBuilderError::new("output.write", "the output file could not be written"))?;
    Ok(0)
}

fn import_native_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let manifest = flag(args, "--manifest")?;
    let out = flag(args, "--out")?;
    let pipeline = optional(args, "--pipeline").map(PathBuf::from);
    let stderr = optional(args, "--stderr").map(PathBuf::from);
    let value = run_native_import(Path::new(&manifest), pipeline.as_deref(), stderr.as_deref())?;
    write_json(&out, &value)?;
    if value.pointer("/import/load_state").and_then(Value::as_str) == Some("awaiting_publish") {
        Ok(0)
    } else {
        Ok(1)
    }
}

fn import_cards_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let catalog = read_json(&flag(args, "--catalog")?)?;
    write_json(&flag(args, "--out")?, &import_cards(&catalog)?)?;
    Ok(0)
}

fn import_seal_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let catalog = read_text(&flag(args, "--catalog")?)?;
    let document = import_seal(&catalog)?;
    write_json(&flag(args, "--out")?, &document)?;
    match document.get("outcome").and_then(Value::as_str) {
        Some("compiler_refused") => Ok(1),
        Some("draft_compiled") => Ok(0),
        _ => Err(PgBuilderError::new(
            "compiler.contract",
            "the sealed import catalog was compiled as a PostgreSQL source",
        )),
    }
}

struct OwnedTarget {
    host: String,
    port: u16,
    database: String,
    user: String,
    login: String,
    ca_file: String,
    password_file: String,
}

fn sealed_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let config_path = flag(args, "--config")?;
    let config_bytes = read_bytes(&config_path)?;
    let config_text = String::from_utf8(config_bytes).map_err(|_| {
        PgBuilderError::new("input.schema", "an input file is not UTF-8")
    })?;
    let config = load_config(&config_text)?;
    let token = read_bearer(&config.bearer_file)?;
    let catalog = read_text(&flag(args, "--catalog")?)?;
    let settings = read_json(&flag(args, "--settings")?)?;
    let visibility = read_json(&flag(args, "--visibility")?)?;
    let source_contract = optional(args, "--source-contract")
        .map(|path| read_json(&path)).transpose()?;
    let owned = match optional(args, "--target-host") {
        None => None,
        Some(host) => Some(OwnedTarget {
            host,
            port: flag(args, "--target-port")?
                .parse()
                .map_err(|_| PgBuilderError::new("input.schema", "the target port is not allowed"))?,
            database: flag(args, "--target-database")?,
            user: flag(args, "--target-user")?,
            login: flag(args, "--login")?,
            ca_file: flag(args, "--target-ca")?,
            password_file: flag(args, "--target-password")?,
        }),
    };
    let target = owned.as_ref().map(|item| SealedTarget {
        host: &item.host,
        port: item.port,
        database: &item.database,
        user: &item.user,
        login: &item.login,
        ca_file: &item.ca_file,
        password_file: &item.password_file,
    });
    let mut transport = LoopbackTransport {
        config: config.clone(),
        token: token.clone(),
    };
    let cache = PathBuf::from(flag(args, "--cache")?);
    let out = PathBuf::from(flag(args, "--out")?);
    let reviewer = flag(args, "--reviewer")?;
    let reviewed_at = flag(args, "--reviewed-at")?;
    let dataset = flag(args, "--dataset")?;
    sealed_build(&mut SealedBuild {
        config: &config,
        token: &token,
        catalog_json: &catalog,
        settings: &settings,
        visibility: &visibility,
        source_contract: source_contract.as_ref(),
        reviewer_by: &reviewer,
        reviewer_at: &reviewed_at,
        dataset: &dataset,
        cache_dir: &cache,
        out_dir: &out,
        target,
        transport: &mut transport,
    })
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
    let configured_model = match optional(args, "--config") {
        Some(path) => load_config(&read_text(&path)?)?.model_id,
        None => "gpt-6.1-sol".to_owned(),
    };
    write_json(&flag(args, "--out")?, &seal_request_for_model(&catalog, &configured_model)?)?;
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

fn pack_combined_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let paths = repeated(args, "--generation");
    if paths.is_empty() {
        return Err(PgBuilderError::new("usage", "a required argument is missing"));
    }
    let mut generations = Vec::with_capacity(paths.len());
    for path in &paths {
        generations.push(read_json(path)?);
    }
    let generation_refs: Vec<&Value> = generations.iter().collect();
    let curated = read_json(&flag(args, "--curated")?)?;
    let operator = read_json(&flag(args, "--operator")?)?;
    let settings = read_json(&flag(args, "--settings")?)?;
    let classifications = read_json(&flag(args, "--classifications")?)?;
    let roles = read_json(&flag(args, "--roles")?)?;
    let hidden = repeated(args, "--hidden");
    let hidden_refs: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let packed = pack_combined(&CombinedIn {
        generations: &generation_refs,
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

fn bind_source_pack_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let catalog = read_text(&flag(args, "--catalog")?)?;
    let effective = read_text(&flag(args, "--effective")?)?;
    let visibility = read_text(&flag(args, "--visibility")?)?;
    let out = flag(args, "--out")?;
    let contract = match optional(args, "--source-contract") {
        Some(path) => Some(read_text(&path)?),
        None => None,
    };
    match bind_source_pack(&catalog, &effective, &visibility, contract.as_deref()) {
        Ok(value) => {
            write_json(&out, &value)?;
            Ok(0)
        }
        Err(err) => {
            eprintln!("{}", err.code);
            write_json(
                &out,
                &json!({
                    "protocol": BIND_SOURCE_PACK_PROTOCOL,
                    "outcome": "refused",
                    "ready": false,
                    "partial_ready": false,
                    "code": err.code,
                }),
            )?;
            Ok(1)
        }
    }
}

fn document_profile_cmd(args: &[String]) -> Result<u8, PgBuilderError> {
    let catalog_json = read_text(&flag(args, "--catalog")?)?;
    let document_json = read_text(&flag(args, "--document")?)?;
    let out = flag(args, "--out")?;
    let settings_path = optional(args, "--settings");
    let visibility_path = optional(args, "--visibility");
    let reviewer = optional(args, "--reviewer");
    let reviewed_at = optional(args, "--reviewed-at");
    let out_pack = optional(args, "--out-pack");
    let contract_path = optional(args, "--source-contract");
    let decisions_path = optional(args, "--decisions");
    let settings_json = match settings_path.as_deref() {
        Some(path) => Some(read_text(path)?),
        None => None,
    };
    let visibility_json = match visibility_path.as_deref() {
        Some(path) => Some(read_text(path)?),
        None => None,
    };
    let contract_json = match contract_path.as_deref() {
        Some(path) => Some(read_text(path)?),
        None => None,
    };
    let decisions_json = match decisions_path.as_deref() {
        Some(path) => Some(read_text(path)?),
        None => None,
    };
    match compile_document_profile(DocumentProfileIn {
        catalog_json: &catalog_json,
        document_json: &document_json,
        settings_json: settings_json.as_deref(),
        visibility_json: visibility_json.as_deref(),
        reviewer_by: reviewer.as_deref(),
        reviewer_at: reviewed_at.as_deref(),
        source_contract_json: contract_json.as_deref(),
        decisions_json: decisions_json.as_deref(),
    }) {
        Ok(value) => {
            if let Some(path) = out_pack.as_deref() {
                let Some(page) = value["pack"]["page"].as_str() else {
                    write_profile_refusal(
                        &out,
                        &PgBuilderError::new(
                            "usage",
                            "document profile pack output needs the pack arguments",
                        ),
                    )?;
                    return Ok(1);
                };
                fs::write(path, page).map_err(|_| {
                    PgBuilderError::new("output.write", "the output file could not be written")
                })?;
            }
            write_json(&out, &value)?;
            Ok(0)
        }
        Err(err) => {
            eprintln!("{}", err.code);
            write_profile_refusal(&out, &err)?;
            Ok(1)
        }
    }
}

fn write_profile_refusal(path: &str, err: &PgBuilderError) -> Result<(), PgBuilderError> {
    write_json(
        path,
        &json!({
            "protocol": DOCUMENT_PROFILE_PROTOCOL,
            "outcome": "refused",
            "ready": false,
            "partial_ready": false,
            "pg_source_passed": false,
            "model": "not_called",
            "code": err.code,
            "pack": Value::Null,
        }),
    )
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
