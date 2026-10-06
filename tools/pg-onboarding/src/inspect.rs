use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config_identity::{self, config_identity_hex};
use crate::error::Stop;
use crate::outcome::{self, failed_body};
use crate::sha256::sha256_hex;
use crate::spawn::{
    self, hex64, load_password, parse_json_slice, pin_binary, plain_token, run_child, text_field, BinPin,
    ChildRequest,
};

const INSPECT_PROTOCOL: &str = "oo.onboarding.inspect/v1";
const MANIFEST_PROTOCOL: &str = "oo.onboarding.inspect-manifest/v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectConfig {
    protocol: String,
    caller: String,
    binaries: InspectBins,
    source: SourceCfg,
    target: TargetCfg,
    shard: String,
    owner: String,
    source_generation: u64,
    connect_ms: u64,
    #[serde(default)]
    runtime_membership_ready_file: Option<String>,
    #[serde(default)]
    membership_wait_ms: Option<u64>,
    #[serde(default)]
    comment_markers: Vec<String>,
    #[serde(default)]
    deadlines_ms: DeadlineCfg,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectBins {
    init_pg: BinPin,
    import_pg: BinPin,
    catalog_pg: BinPin,
    captured_pg_draft: BinPin,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceCfg {
    source_id: String,
    host: String,
    port: u16,
    database: String,
    username: String,
    secret_ref: String,
    password_file: String,
    ca_file: String,
    tables: Vec<TableName>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableName {
    schema: String,
    table: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetCfg {
    host: String,
    port: u16,
    database: String,
    administrator: String,
    runtime_principal: String,
    admin_password_file: String,
    runtime_password_file: String,
    ca_file: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DeadlineCfg {
    #[serde(default)]
    import_not_ready_ms: Option<u64>,
    #[serde(default)]
    init_ms: Option<u64>,
    #[serde(default)]
    import_ms: Option<u64>,
    #[serde(default)]
    catalog_ms: Option<u64>,
    #[serde(default)]
    captured_ms: Option<u64>,
}

pub fn bind_generation(source_cut: i64, target_fence: i64) -> Result<i64, Stop> {
    if source_cut < 1 || target_fence < 1 {
        return Err(Stop::new("import-pg", "generation"));
    }
    if source_cut == target_fence {
        return Err(Stop::new("import-pg", "fence"));
    }
    Ok(source_cut)
}

pub fn inspect(config_path: &Path, run_dir: &Path) -> Result<(), Stop> {
    let cfg_bytes = fs::read(config_path).map_err(|_| Stop::new("config", "config"))?;
    let cfg: InspectConfig = serde_json::from_slice(&cfg_bytes).map_err(|_| Stop::new("config", "config"))?;
    validate(&cfg)?;
    if !run_dir.is_absolute() {
        return Err(Stop::new("run", "config"));
    }
    let secrets = secrets_of(&cfg)?;
    if spawn::contains_secret(&cfg_bytes, &secrets) {
        return Err(Stop::new("config", "secret"));
    }
    let bins = (
        pin_binary(&cfg.binaries.init_pg)?,
        pin_binary(&cfg.binaries.import_pg)?,
        pin_binary(&cfg.binaries.catalog_pg)?,
        pin_binary(&cfg.binaries.captured_pg_draft)?,
    );
    if run_dir.exists() {
        return Err(Stop::new("run", "run_dir"));
    }
    fs::create_dir(run_dir).map_err(|_| Stop::new("run", "run_dir"))?;
    fs::create_dir(run_dir.join("steps")).map_err(|_| Stop::new("run", "io"))?;
    let result = inspect_in(config_path, run_dir, &cfg, &cfg_bytes, &secrets, &bins);
    if let Err(stop) = &result {
        let _ = outcome::write_json_new(&run_dir.join("outcome.json"), &failed_body(stop, "inspect"));
    }
    result
}

fn inspect_in(
    _config_path: &Path,
    run_dir: &Path,
    cfg: &InspectConfig,
    _cfg_bytes: &[u8],
    secrets: &[Vec<u8>],
    bins: &(PathBuf, PathBuf, PathBuf, PathBuf),
) -> Result<(), Stop> {
    let import_stdin = import_body(cfg)?;
    let not_ready = run_child(&ChildRequest {
        step: "import-not-ready".to_string(),
        bin: bins.1.clone(),
        args: vec!["import-pg".to_string()],
        stdin: import_stdin.clone(),
        run_dir: run_dir.to_path_buf(),
        deadline: millis(cfg.deadlines_ms.import_not_ready_ms, 120_000)?,
        secrets: secrets.to_vec(),
    })?;
    if not_ready.timed_out {
        return Err(Stop::new("import-not-ready", "deadline"));
    }
    if not_ready.code == Some(0) {
        return Err(Stop::new("import-not-ready", "already_installed"));
    }
    if not_ready.code.is_none() {
        return Err(Stop::new("import-not-ready", "exit"));
    }
    if !stderr_has(&not_ready.stderr, "target schema is not ready") {
        return Err(Stop::new("import-not-ready", "shape"));
    }
    let init_out = run_child(&ChildRequest {
        step: "init-pg".to_string(),
        bin: bins.0.clone(),
        args: Vec::new(),
        stdin: init_body(cfg)?,
        run_dir: run_dir.to_path_buf(),
        deadline: millis(cfg.deadlines_ms.init_ms, 120_000)?,
        secrets: secrets.to_vec(),
    })?;
    spawn::require_code("init-pg", &init_out, 0)?;
    check_init(&init_out.stdout)?;
    if let Some(ready) = &cfg.runtime_membership_ready_file {
        wait_ready(Path::new(ready), millis(cfg.membership_wait_ms, 60_000)?)?;
    }
    let imported = run_child(&ChildRequest {
        step: "import-pg".to_string(),
        bin: bins.1.clone(),
        args: vec!["import-pg".to_string()],
        stdin: import_stdin,
        run_dir: run_dir.to_path_buf(),
        deadline: millis(cfg.deadlines_ms.import_ms, 180_000)?,
        secrets: secrets.to_vec(),
    })?;
    spawn::require_code("import-pg", &imported, 0)?;
    let import_value = parse_json_slice(&imported.stdout, "import-pg")?;
    let (source_cut, target_fence) = check_import(&import_value, cfg)?;
    let mapping_run = bind_generation(source_cut, target_fence)?;
    let tables: Vec<(&str, &str)> = cfg
        .source
        .tables
        .iter()
        .map(|table| (table.schema.as_str(), table.table.as_str()))
        .collect();
    let identity = config_identity_hex(
        &cfg.source.source_id,
        &cfg.source.host,
        cfg.source.port,
        &cfg.source.database,
        &cfg.source.username,
        &cfg.source.secret_ref,
        &tables,
    );
    let catalog = run_child(&ChildRequest {
        step: "catalog-pg".to_string(),
        bin: bins.2.clone(),
        args: vec!["catalog-pg".to_string()],
        stdin: catalog_body(cfg)?,
        run_dir: run_dir.to_path_buf(),
        deadline: millis(cfg.deadlines_ms.catalog_ms, 120_000)?,
        secrets: secrets.to_vec(),
    })?;
    spawn::require_code("catalog-pg", &catalog, 0)?;
    let catalog_value = parse_json_slice(&catalog.stdout, "catalog-pg")?;
    if catalog_value["protocol"] != "oo.source.pgsql/v1" {
        return Err(Stop::new("catalog-pg", "protocol"));
    }
    refuse_markers("catalog-pg", &catalog.stdout, &cfg.comment_markers)?;
    let captured = run_child(&ChildRequest {
        step: "captured-pg-draft".to_string(),
        bin: bins.3.clone(),
        args: vec!["captured-pg-draft".to_string()],
        stdin: catalog.stdout.clone(),
        run_dir: run_dir.to_path_buf(),
        deadline: millis(cfg.deadlines_ms.captured_ms, 60_000)?,
        secrets: secrets.to_vec(),
    })?;
    spawn::require_code("captured-pg-draft", &captured, 0)?;
    refuse_markers("captured-pg-draft", &captured.stdout, &cfg.comment_markers)?;
    let captured_value = parse_json_slice(&captured.stdout, "captured-pg-draft")?;
    if captured_value["protocol"] != "oo.builder.captured-pg-draft-command/v1" {
        return Err(Stop::new("captured-pg-draft", "protocol"));
    }
    let draft_hash = text_field(&captured_value["wrapper"], "draft_hash", "captured-pg-draft")?;
    let schema_hash = text_field(&captured_value["wrapper"], "schema_hash", "captured-pg-draft")?;
    if !hex64(draft_hash) || !hex64(schema_hash) {
        return Err(Stop::new("captured-pg-draft", "shape"));
    }
    outcome::write_new(&run_dir.join("import-result.json"), &imported.stdout)?;
    outcome::write_new(&run_dir.join("catalog.json"), &catalog.stdout)?;
    outcome::write_new(&run_dir.join("captured-draft.json"), &captured.stdout)?;
    let manifest = json!({
        "protocol": MANIFEST_PROTOCOL,
        "phase": "awaiting_review",
        "binaries": {
            "init_pg": pin_json(&cfg.binaries.init_pg),
            "import_pg": pin_json(&cfg.binaries.import_pg),
            "catalog_pg": pin_json(&cfg.binaries.catalog_pg),
            "captured_pg_draft": pin_json(&cfg.binaries.captured_pg_draft)
        },
        "source": {
            "source_id": cfg.source.source_id,
            "host": cfg.source.host,
            "port": cfg.source.port,
            "database": cfg.source.database,
            "username": cfg.source.username,
            "secret_ref": cfg.source.secret_ref,
            "password_file": cfg.source.password_file,
            "ca_file": cfg.source.ca_file,
            "tables": cfg.source.tables.iter().map(|table| json!({"schema": table.schema, "table": table.table})).collect::<Vec<_>>()
        },
        "target": {
            "host": cfg.target.host,
            "port": cfg.target.port,
            "database": cfg.target.database,
            "administrator": cfg.target.administrator,
            "runtime_principal": cfg.target.runtime_principal,
            "admin_password_file": cfg.target.admin_password_file,
            "runtime_password_file": cfg.target.runtime_password_file,
            "ca_file": cfg.target.ca_file
        },
        "shard": cfg.shard,
        "owner": cfg.owner,
        "requested_source_generation": cfg.source_generation,
        "source_cut": source_cut,
        "target_fence": target_fence,
        "mapping_run_generation": mapping_run,
        "budget_defaults": {
            "connect_ms": config_identity::CONNECT_MS,
            "statement_ms": config_identity::STATEMENT_MS,
            "lock_ms": config_identity::LOCK_MS,
            "max_rows": config_identity::MAX_ROWS,
            "max_bytes": config_identity::MAX_BYTES
        },
        "config_identity_sha256": identity,
        "config_identity": "recomputed_from_non_secret_request_fields",
        "import_result_sha256": sha256_hex(&imported.stdout),
        "catalog_sha256": sha256_hex(&catalog.stdout),
        "captured_draft_sha256": sha256_hex(&captured.stdout),
        "draft_hash": draft_hash,
        "schema_hash": schema_hash,
        "non_claims": outcome::non_claims(false)
    });
    let mut manifest_bytes = serde_json::to_vec(&manifest).map_err(|_| Stop::new("manifest", "io"))?;
    manifest_bytes.push(b'\n');
    if spawn::contains_secret(&manifest_bytes, secrets) {
        return Err(Stop::new("manifest", "secret"));
    }
    outcome::write_new(&run_dir.join("inspect-manifest.json"), &manifest_bytes)?;
    let sidecar = format!("{}\n", sha256_hex(&manifest_bytes));
    outcome::write_new(&run_dir.join("inspect-manifest.json.sha256"), sidecar.as_bytes())?;
    let outcome = json!({
        "protocol": "oo.onboarding.outcome/v1",
        "phase": "inspect",
        "status": "awaiting_review",
        "source_cut": source_cut,
        "target_fence": target_fence,
        "mapping_run_generation": mapping_run,
        "config_identity_sha256": identity,
        "database_state": "child_compilers_applied_no_release",
        "database_rollback": false,
        "database_release": false,
        "non_claims": outcome::non_claims(false)
    });
    outcome::write_json_new(&run_dir.join("outcome.json"), &outcome)?;
    Ok(())
}

fn validate(cfg: &InspectConfig) -> Result<(), Stop> {
    if cfg.protocol != INSPECT_PROTOCOL || cfg.caller != "trusted_local_admin" {
        return Err(Stop::new("config", "caller"));
    }
    if !(1..=120_000).contains(&cfg.connect_ms) || cfg.source.tables.is_empty() {
        return Err(Stop::new("config", "config"));
    }
    if cfg.source_generation == 0 || i64::try_from(cfg.source_generation).is_err() {
        return Err(Stop::new("config", "generation"));
    }
    if !plain_token(&cfg.shard) || !uuid_ok(&cfg.owner) {
        return Err(Stop::new("config", "config"));
    }
    for marker in &cfg.comment_markers {
        if marker.is_empty() || marker.len() > 64 {
            return Err(Stop::new("config", "config"));
        }
    }
    token_source(&cfg.source)?;
    token_target(&cfg.target)?;
    if let Some(path) = &cfg.runtime_membership_ready_file && !Path::new(path).is_absolute() {
        return Err(Stop::new("config", "config"));
    }
    millis(cfg.deadlines_ms.import_not_ready_ms, 120_000)?;
    millis(cfg.deadlines_ms.init_ms, 120_000)?;
    millis(cfg.deadlines_ms.import_ms, 180_000)?;
    millis(cfg.deadlines_ms.catalog_ms, 120_000)?;
    millis(cfg.deadlines_ms.captured_ms, 60_000)?;
    millis(cfg.membership_wait_ms, 60_000)?;
    Ok(())
}

fn token_source(source: &SourceCfg) -> Result<(), Stop> {
    let fields = [
        source.source_id.as_str(),
        source.host.as_str(),
        source.database.as_str(),
        source.username.as_str(),
        source.secret_ref.as_str(),
        source.password_file.as_str(),
        source.ca_file.as_str(),
    ];
    if source.port == 0 || fields.iter().any(|field| !plain_token(field)) {
        return Err(Stop::new("config", "config"));
    }
    for table in &source.tables {
        if !plain_token(&table.schema) || !plain_token(&table.table) {
            return Err(Stop::new("config", "config"));
        }
    }
    absolute_file(&source.password_file)?;
    absolute_file(&source.ca_file)?;
    Ok(())
}

fn token_target(target: &TargetCfg) -> Result<(), Stop> {
    let fields = [
        target.host.as_str(),
        target.database.as_str(),
        target.administrator.as_str(),
        target.runtime_principal.as_str(),
        target.admin_password_file.as_str(),
        target.runtime_password_file.as_str(),
        target.ca_file.as_str(),
    ];
    if target.port == 0 || fields.iter().any(|field| !plain_token(field)) {
        return Err(Stop::new("config", "config"));
    }
    absolute_file(&target.admin_password_file)?;
    absolute_file(&target.runtime_password_file)?;
    absolute_file(&target.ca_file)?;
    Ok(())
}

fn absolute_file(path: &str) -> Result<(), Stop> {
    if Path::new(path).is_absolute() {
        Ok(())
    } else {
        Err(Stop::new("config", "config"))
    }
}

fn uuid_ok(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
        && parts.iter().all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn secrets_of(cfg: &InspectConfig) -> Result<Vec<Vec<u8>>, Stop> {
    let mut secrets = Vec::new();
    for path in [
        cfg.source.password_file.as_str(),
        cfg.target.admin_password_file.as_str(),
        cfg.target.runtime_password_file.as_str(),
    ] {
        secrets.push(load_password(Path::new(path))?);
    }
    Ok(secrets)
}

fn import_body(cfg: &InspectConfig) -> Result<Vec<u8>, Stop> {
    let tables: Vec<Value> = cfg
        .source
        .tables
        .iter()
        .map(|table| json!({"schema": table.schema, "table": table.table}))
        .collect();
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.import-pg/v1",
        "source": source_object(cfg, &tables),
        "target": {
            "host": cfg.target.host,
            "port": cfg.target.port,
            "database": cfg.target.database,
            "runtime_principal": cfg.target.runtime_principal,
            "password_file": cfg.target.runtime_password_file,
            "ca_file": cfg.target.ca_file
        },
        "shard": cfg.shard,
        "owner": cfg.owner,
        "source_generation": cfg.source_generation
    }))
    .map_err(|_| Stop::new("import-pg", "io"))
}

fn catalog_body(cfg: &InspectConfig) -> Result<Vec<u8>, Stop> {
    let tables: Vec<Value> = cfg
        .source
        .tables
        .iter()
        .map(|table| json!({"schema": table.schema, "table": table.table}))
        .collect();
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.catalog-pg/v1",
        "source": source_object(cfg, &tables)
    }))
    .map_err(|_| Stop::new("catalog-pg", "io"))
}

fn source_object(cfg: &InspectConfig, tables: &[Value]) -> Value {
    json!({
        "source_id": cfg.source.source_id,
        "host": cfg.source.host,
        "port": cfg.source.port,
        "database": cfg.source.database,
        "username": cfg.source.username,
        "secret_ref": cfg.source.secret_ref,
        "password_file": cfg.source.password_file,
        "ca_file": cfg.source.ca_file,
        "tables": tables
    })
}

fn init_body(cfg: &InspectConfig) -> Result<Vec<u8>, Stop> {
    serde_json::to_vec(&json!({
        "protocol": "oo.pipeline.init-pg/v1",
        "connect_ms": cfg.connect_ms,
        "target": {
            "host": cfg.target.host,
            "port": cfg.target.port,
            "database": cfg.target.database,
            "administrator": cfg.target.administrator,
            "password_file": cfg.target.admin_password_file,
            "ca_file": cfg.target.ca_file
        }
    }))
    .map_err(|_| Stop::new("init-pg", "io"))
}

fn check_init(stdout: &[u8]) -> Result<(), Stop> {
    let value = parse_json_slice(stdout, "init-pg")?;
    if value["protocol"] != "oo.pipeline.init-pg-result/v1" {
        return Err(Stop::new("init-pg", "protocol"));
    }
    if value["install"] != "bootstrapped" || value["full_import"] != "installed" || value["historical_v2"] != false {
        return Err(Stop::new("init-pg", "shape"));
    }
    Ok(())
}

fn check_import(value: &Value, cfg: &InspectConfig) -> Result<(i64, i64), Stop> {
    if value["protocol"] != "oo.pipeline.import-pg-result/v1" {
        return Err(Stop::new("import-pg", "protocol"));
    }
    if value["state"] != "awaiting_publish" {
        return Err(Stop::new("import-pg", "shape"));
    }
    let tables = value["tables"].as_array().ok_or_else(|| Stop::new("import-pg", "shape"))?;
    let count = value["table_count"].as_u64().ok_or_else(|| Stop::new("import-pg", "shape"))?;
    if count != tables.len() as u64 || count != cfg.source.tables.len() as u64 {
        return Err(Stop::new("import-pg", "shape"));
    }
    for (listed, requested) in tables.iter().zip(cfg.source.tables.iter()) {
        if listed["schema"] != requested.schema || listed["name"] != requested.table {
            return Err(Stop::new("import-pg", "shape"));
        }
        let physical = text_field(listed, "physical_name", "import-pg")?;
        if !spawn::physical_ok(physical, "pt_") {
            return Err(Stop::new("import-pg", "shape"));
        }
        let digest = text_field(listed, "canonical_digest", "import-pg")?;
        if !hex64(digest) {
            return Err(Stop::new("import-pg", "shape"));
        }
        let columns = listed["columns"].as_array().ok_or_else(|| Stop::new("import-pg", "shape"))?;
        for column in columns {
            let name = text_field(column, "physical_name", "import-pg")?;
            if !spawn::physical_ok(name, "pc_") || column["name"].as_str().is_none() {
                return Err(Stop::new("import-pg", "shape"));
            }
        }
    }
    let source_cut = value["source_generation"].as_i64().ok_or_else(|| Stop::new("import-pg", "generation"))?;
    let target_fence = value["target_generation"].as_i64().ok_or_else(|| Stop::new("import-pg", "generation"))?;
    let requested = i64::try_from(cfg.source_generation).map_err(|_| Stop::new("import-pg", "generation"))?;
    if source_cut != requested {
        return Err(Stop::new("import-pg", "generation"));
    }
    Ok((source_cut, target_fence))
}

fn wait_ready(path: &Path, deadline: Duration) -> Result<(), Stop> {
    let started = Instant::now();
    loop {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_file() => return Ok(()),
            _ if started.elapsed() >= deadline => return Err(Stop::new("membership", "deadline")),
            _ => thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn refuse_markers(step: &str, bytes: &[u8], markers: &[String]) -> Result<(), Stop> {
    for marker in markers {
        if bytes.windows(marker.len()).any(|window| window == marker.as_bytes()) {
            return Err(Stop::new(step, "marker"));
        }
    }
    Ok(())
}

fn stderr_has(stderr: &[u8], needle: &str) -> bool {
    stderr.windows(needle.len()).any(|window| window == needle.as_bytes())
}

fn millis(value: Option<u64>, default_ms: u64) -> Result<Duration, Stop> {
    let ms = value.unwrap_or(default_ms);
    if !(1..=180_000).contains(&ms) {
        return Err(Stop::new("config", "config"));
    }
    Ok(Duration::from_millis(ms))
}

fn pin_json(pin: &BinPin) -> Value {
    json!({"path": pin.path, "sha256": pin.sha256})
}
