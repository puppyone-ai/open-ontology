use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{malformed, Fault};
use crate::sha::{hex_encode, sha256};

pub const CONTRACT: &str = "agent-entry-v1";
pub const DEPLOYMENT: &str = "localprocess";

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PgAuth {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: String,
    pub ca_pem: String,
}

impl Drop for PgAuth {
    fn drop(&mut self) {
        wipe(&mut self.password);
        wipe(&mut self.ca_pem);
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub contract: String,
    pub profile: String,
    pub state_dir: String,
    pub identity_origin: String,
    pub identity_server_name: String,
    pub identity_ca_pem: String,
    pub session_bin: String,
    pub session_config_path: String,
    pub session_port_file: String,
    pub mapping_path: String,
    pub pack_root: String,
    pub release_origin: String,
    pub pack_mode: String,
    pub query_pg_bin: String,
    pub query_policy_dir: String,
    pub gateway_addr_file: String,
    pub mtls_ca_file: String,
    pub mtls_cert_file: String,
    pub mtls_key_file: String,
    pub oo_bin: String,
    pub pi_executable: String,
    pub pi_executable_sha256: String,
    pub pi_runtime: String,
    pub pi_runtime_sha256: String,
    pub upstream_token_file: String,
    pub pg: PgAuth,
    /// Identity database used only to compare the presented CSRF with the hash
    /// `control-identity` stored. `GET /identity/current` rotates that hash.
    /// `development_draft` does not open this database.
    #[serde(default)]
    pub identity_pg: Option<PgAuth>,
    /// Absolute path of the import generation file. Required for
    /// `development_draft`. A private fixture keeps `mp01` false. A sealed
    /// local import sets `mp01` true only after the manifest hash matches.
    #[serde(default)]
    pub import_generation_file: String,
}

impl Drop for Config {
    fn drop(&mut self) {
        wipe(&mut self.identity_ca_pem);
    }
}

pub fn startup_gate(profile: &str) -> Result<(), &'static str> {
    match profile {
        "localprocess" => Ok(()),
        "production" if runsc_present() => Err("production_driver_absent"),
        "production" => Err("runsc_absent"),
        _ => Err("profile"),
    }
}

fn runsc_present() -> bool {
    Path::new("/usr/local/bin/runsc").is_file() || Path::new("/usr/bin/runsc").is_file()
}

pub fn load_config(bytes: &[u8]) -> Result<Config, Fault> {
    if bytes.len() > 1_048_576 {
        return Err(malformed("too_large"));
    }
    let config: Config = serde_json::from_slice(bytes).map_err(|_| malformed("config"))?;
    if config.contract != CONTRACT {
        return Err(malformed("contract"));
    }
    if config.profile != "localprocess" && config.profile != "production" {
        return Err(malformed("profile"));
    }
    for path in [
        &config.state_dir,
        &config.session_bin,
        &config.session_config_path,
        &config.session_port_file,
        &config.mapping_path,
        &config.pack_root,
        &config.query_pg_bin,
        &config.query_policy_dir,
        &config.gateway_addr_file,
        &config.mtls_ca_file,
        &config.mtls_cert_file,
        &config.mtls_key_file,
        &config.oo_bin,
        &config.pi_executable,
        &config.pi_runtime,
        &config.upstream_token_file,
    ] {
        if !path.starts_with('/') {
            return Err(malformed("path"));
        }
    }
    if !config.identity_origin.starts_with("https://") || config.identity_ca_pem.is_empty() {
        return Err(malformed("identity"));
    }
    crate::release::loopback_origin(&config.release_origin)?;
    if config.pack_mode != "development_draft" && config.pack_mode != "active_release" {
        return Err(malformed("pack_mode"));
    }
    if config.pack_mode == "development_draft" && !config.import_generation_file.starts_with('/') {
        return Err(malformed("import_generation"));
    }
    if config.pg.host.is_empty() || config.pg.port == 0 || !config.pg.ca_pem.contains("BEGIN CERTIFICATE") {
        return Err(malformed("pg"));
    }
    if !is_hex64(&config.pi_executable_sha256) || !is_hex64(&config.pi_runtime_sha256) {
        return Err(malformed("pin"));
    }
    Ok(config)
}

pub fn is_hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportGenerationFile {
    contract: String,
    source: String,
    mp01: bool,
    generation: String,
    database: String,
    #[serde(default)]
    manifest_path: String,
    #[serde(default)]
    target_config_path: String,
    /// `_oo.physical_mappings.source` for this sealed copy. Not a database role.
    #[serde(default)]
    mapping_source: String,
    /// `_oo.physical_mappings.config_digest` for this sealed copy.
    #[serde(default)]
    config_digest: String,
}

pub struct ManifestColumn {
    pub name: String,
    /// Query-pg base name derived from the manifest oid. Empty means the
    /// column is recorded and is not selectable.
    pub semantic_type: String,
}

pub struct ManifestTable {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ManifestColumn>,
}

pub struct ImportGeneration {
    pub source: String,
    pub generation: String,
    pub database: String,
    /// Provenance only. True does not accept MP-01a–d.
    pub mp01: bool,
    pub snapshot_id: String,
    pub import_generation_number: Option<i64>,
    pub plan_sha256: String,
    pub manifest_sha256: String,
    pub target_config_sha256: String,
    pub target_host: String,
    pub target_server_name: String,
    pub target_user: String,
    pub mapping_source: String,
    pub config_digest: String,
    pub tables: Vec<ManifestTable>,
}

/// Fields a sealed preview may stamp onto a query receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewStamp {
    pub plan_sha256: String,
    pub manifest_sha256: String,
    pub target_config_sha256: String,
    pub target_host: String,
    pub target_server_name: String,
    pub target_user: String,
    pub mapping_source: String,
    pub config_digest: String,
}

/// Private fixture or a sealed local-import manifest. This does not sign MP-01a–d.
pub fn load_import_generation(path: &str) -> Result<ImportGeneration, Fault> {
    let bytes = std::fs::read(path).map_err(|_| malformed("import_generation"))?;
    if bytes.len() > 4_096 {
        return Err(malformed("import_generation"));
    }
    let file: ImportGenerationFile = serde_json::from_slice(&bytes).map_err(|_| malformed("import_generation"))?;
    if file.contract != "agent-entry-import-generation" || !is_hex64(&file.generation) || !plain_db(&file.database) {
        return Err(malformed("import_generation"));
    }
    match file.source.as_str() {
        "private_fixture" => {
            if file.mp01 || !file.manifest_path.is_empty() || !file.target_config_path.is_empty() || file.database != "oo_a397" {
                return Err(malformed("import_generation"));
            }
            if !file.mapping_source.is_empty() || !file.config_digest.is_empty() {
                return Err(malformed("import_generation"));
            }
            Ok(ImportGeneration {
                source: file.source,
                generation: file.generation,
                database: file.database,
                mp01: false,
                snapshot_id: String::new(),
                import_generation_number: None,
                plan_sha256: String::new(),
                manifest_sha256: String::new(),
                target_config_sha256: String::new(),
                target_host: String::new(),
                target_server_name: String::new(),
                target_user: String::new(),
                mapping_source: String::new(),
                config_digest: String::new(),
                tables: Vec::new(),
            })
        }
        "local_import" => {
            if !file.mp01 || !plain_db(&file.mapping_source) || !is_hex64(&file.config_digest) {
                return Err(malformed("import_generation"));
            }
            let sealed = bind_local_import(
                &file.manifest_path,
                &file.target_config_path,
                &file.database,
                &file.mapping_source,
                &file.config_digest,
            )?;
            if sealed.import_hash != file.generation {
                return Err(malformed("import_generation"));
            }
            Ok(ImportGeneration {
                source: file.source,
                generation: file.generation,
                database: file.database,
                mp01: true,
                snapshot_id: sealed.snapshot_id,
                import_generation_number: Some(sealed.import_generation),
                plan_sha256: sealed.plan_sha256,
                manifest_sha256: sealed.manifest_sha256,
                target_config_sha256: sealed.target_config_sha256,
                target_host: sealed.target_host,
                target_server_name: sealed.target_server_name,
                target_user: sealed.target_user,
                mapping_source: sealed.mapping_source,
                config_digest: sealed.config_digest,
                tables: sealed.tables,
            })
        }
        _ => Err(malformed("import_generation")),
    }
}

fn plain_db(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && value.len() <= 63
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

struct SealedBind {
    import_hash: String,
    import_generation: i64,
    snapshot_id: String,
    plan_sha256: String,
    manifest_sha256: String,
    target_config_sha256: String,
    target_host: String,
    target_server_name: String,
    target_user: String,
    mapping_source: String,
    config_digest: String,
    tables: Vec<ManifestTable>,
}

/// Bind a sealed local import to its target file. `mp01` is not read here.
fn bind_local_import(
    manifest_path: &str,
    target_path: &str,
    database: &str,
    mapping_source: &str,
    config_digest: &str,
) -> Result<SealedBind, Fault> {
    let bytes = read_regular(manifest_path, 2_097_152)?;
    let manifest_sha256 = manifest_body_sha(&bytes)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| malformed("import_generation"))?;
    let object = value.as_object().ok_or_else(|| malformed("import_generation"))?;
    let text = |key: &str| object.get(key).and_then(serde_json::Value::as_str).unwrap_or("");
    let flag = |key: &str| object.get(key).and_then(serde_json::Value::as_bool);
    if text("protocol") != "oo.pipeline.local-pg-mirror-manifest/v1"
        || text("mirror_state") != "complete"
        || text("consistency") != "single_snapshot"
        || flag("customer_complete") != Some(false)
        || text("enterprise") != "not_configured"
        || text("failure") != "none"
        || text("spool_state") != "complete"
        || flag("source_read_only") != Some(true)
        || !object.get("tables_incomplete").and_then(serde_json::Value::as_array).is_some_and(Vec::is_empty)
        || !object.get("quarantine").and_then(serde_json::Value::as_array).is_some_and(Vec::is_empty)
        || text("manifest_sha256") != manifest_sha256
    {
        return Err(malformed("import_generation"));
    }
    let generation = object.get("import_generation").and_then(serde_json::Value::as_i64).filter(|value| *value >= 0);
    let snapshot = text("snapshot_id");
    let plan = text("plan_sha256");
    let rows = object.get("rows").and_then(serde_json::Value::as_u64);
    let stored = text("import_hash");
    let target_sha = text("target_config_sha256");
    let Some(generation) = generation else {
        return Err(malformed("import_generation"));
    };
    let Some(rows) = rows else {
        return Err(malformed("import_generation"));
    };
    if snapshot.is_empty() || snapshot.contains('\n') || snapshot.contains('\r') || !is_hex64(plan) || !is_hex64(stored) || !is_hex64(target_sha) {
        return Err(malformed("import_generation"));
    }
    let tables = object.get("tables_completed").and_then(serde_json::Value::as_array).ok_or_else(|| malformed("import_generation"))?;
    if tables.is_empty() {
        return Err(malformed("import_generation"));
    }
    let mut parsed_tables = Vec::with_capacity(tables.len());
    let mut body = String::from("local-pg-mirror/v1 sealed-import\n");
    body.push_str("plan=");
    body.push_str(plan);
    body.push('\n');
    body.push_str("generation=");
    body.push_str(&generation.to_string());
    body.push('\n');
    body.push_str("snapshot=");
    body.push_str(snapshot);
    body.push('\n');
    body.push_str("rows=");
    body.push_str(&rows.to_string());
    body.push('\n');
    for table in tables {
        let item = table.as_object().ok_or_else(|| malformed("import_generation"))?;
        let schema = item.get("schema").and_then(serde_json::Value::as_str).unwrap_or("");
        let name = item.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
        let digest = item.get("canonical_digest").and_then(serde_json::Value::as_str).unwrap_or("");
        let table_rows = item.get("rows").and_then(serde_json::Value::as_u64);
        if schema.is_empty() || name.is_empty() || schema.contains('\n') || name.contains('\n') || !is_hex64(digest) || table_rows.is_none() {
            return Err(malformed("import_generation"));
        }
        let physical = item.get("physical_name").and_then(serde_json::Value::as_str).unwrap_or("");
        if !physical.starts_with("pt_") || physical.contains('\n') {
            return Err(malformed("import_generation"));
        }
        let columns = item.get("columns").and_then(serde_json::Value::as_array).ok_or_else(|| malformed("import_generation"))?;
        if columns.is_empty() {
            return Err(malformed("import_generation"));
        }
        let mut parsed_columns = Vec::with_capacity(columns.len());
        for column in columns {
            let column = column.as_object().ok_or_else(|| malformed("import_generation"))?;
            let column_name = column.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
            let column_physical = column.get("physical_name").and_then(serde_json::Value::as_str).unwrap_or("");
            let oid = column.get("oid").and_then(serde_json::Value::as_u64).unwrap_or(0);
            let typmod = column.get("typmod").and_then(serde_json::Value::as_i64).unwrap_or(i64::MIN);
            if !plain_db(column_name) || !column_physical.starts_with("pc_") || column_physical.contains('\n') || oid == 0 || typmod == i64::MIN {
                return Err(malformed("import_generation"));
            }
            parsed_columns.push(ManifestColumn { name: column_name.to_owned(), semantic_type: column_semantic(oid, typmod) });
        }
        parsed_tables.push(ManifestTable { schema: schema.to_owned(), name: name.to_owned(), columns: parsed_columns });
        body.push_str("table=");
        body.push_str(schema);
        body.push('.');
        body.push_str(name);
        body.push_str(" rows=");
        body.push_str(&table_rows.unwrap_or(0).to_string());
        body.push_str(" digest=");
        body.push_str(digest);
        body.push('\n');
    }
    let import_hash = hex_encode(&sha256(body.as_bytes()));
    if import_hash != stored {
        return Err(malformed("import_generation"));
    }
    let target = bind_target(target_path, target_sha, database)?;
    Ok(SealedBind {
        import_hash,
        import_generation: generation,
        snapshot_id: snapshot.to_owned(),
        plan_sha256: plan.to_owned(),
        manifest_sha256,
        target_config_sha256: target_sha.to_owned(),
        target_host: target.host,
        target_server_name: target.server_name,
        target_user: target.user,
        mapping_source: mapping_source.to_owned(),
        config_digest: config_digest.to_owned(),
        tables: parsed_tables,
    })
}

fn column_semantic(oid: u64, typmod: i64) -> String {
    match oid {
        16 => "bool".to_owned(),
        21 => "int2".to_owned(),
        23 => "int4".to_owned(),
        20 => "int8".to_owned(),
        25 => "text".to_owned(),
        1700 if numeric_precision(typmod).is_some_and(|precision| (1..=38).contains(&precision)) => "numeric".to_owned(),
        _ => String::new(),
    }
}

fn numeric_precision(typmod: i64) -> Option<i64> {
    if typmod < 0 {
        return None;
    }
    let adjusted = typmod.wrapping_sub(4);
    Some((adjusted >> 16) & 0xFFFF)
}

/// A development preview may name only original columns from the sealed manifest.
/// `confirmed` is a reviewed classification and is refused here.
pub fn admit_local_preview(
    policy: &serde_json::Value,
    sealed: &ImportGeneration,
    host: &str,
    database: &str,
    user: &str,
) -> Result<PreviewStamp, &'static str> {
    if sealed.source != "local_import" || !sealed.mp01 || sealed.tables.is_empty() {
        return Err("preview_policy");
    }
    if host != sealed.target_host || host != sealed.target_server_name || database != sealed.database || user != sealed.target_user {
        return Err("preview_identity");
    }
    if !is_hex64(&sealed.plan_sha256) || !is_hex64(&sealed.manifest_sha256) || !is_hex64(&sealed.target_config_sha256) || !is_hex64(&sealed.config_digest) {
        return Err("preview_identity");
    }
    let object = policy.as_object().ok_or("preview_policy")?;
    if object.contains_key("published") || object.contains_key("approved") || object.contains_key("human_approved") {
        return Err("preview_not_approval");
    }
    let endpoint = object.get("endpoint").and_then(serde_json::Value::as_object).ok_or("preview_policy")?;
    let endpoint_host = endpoint.get("host").and_then(serde_json::Value::as_str).unwrap_or("");
    let endpoint_db = endpoint.get("database").and_then(serde_json::Value::as_str).unwrap_or("");
    let endpoint_user = endpoint.get("user").and_then(serde_json::Value::as_str).unwrap_or("");
    let port = endpoint.get("port").and_then(serde_json::Value::as_u64).unwrap_or(0);
    if endpoint_host != host || endpoint_db != database || endpoint_user != user || !(1..=65535).contains(&port) {
        return Err("preview_identity");
    }
    let identity = object.get("identity").and_then(serde_json::Value::as_object).ok_or("preview_policy")?;
    let project = identity.get("project").and_then(serde_json::Value::as_str).unwrap_or("");
    let policy_id = identity.get("policy_id").and_then(serde_json::Value::as_str).unwrap_or("");
    let identity_role = identity.get("role").and_then(serde_json::Value::as_str).unwrap_or("");
    if project != sealed.mapping_source || policy_id != sealed.config_digest {
        return Err("preview_identity");
    }
    let policies = object.get("policies").and_then(serde_json::Value::as_array).ok_or("preview_policy")?;
    if policies.is_empty() {
        return Err("preview_policy");
    }
    let mut slug = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for policy in policies {
        let item = policy.as_object().ok_or("preview_policy")?;
        if item.contains_key("row_filter") {
            return Err("preview_policy");
        }
        let item_project = item.get("project").and_then(serde_json::Value::as_str).unwrap_or("");
        let item_digest = item.get("config_digest").and_then(serde_json::Value::as_str).unwrap_or("");
        let item_generation = item.get("generation").and_then(serde_json::Value::as_i64);
        let item_role = item.get("role").and_then(serde_json::Value::as_str).unwrap_or("");
        let item_login = item.get("login").and_then(serde_json::Value::as_str).unwrap_or("");
        if item_project != sealed.mapping_source || item_digest != sealed.config_digest || item_generation != sealed.import_generation_number || item_login != user {
            return Err("preview_identity");
        }
        if slug.is_empty() {
            if !plain_db(item_role) || item_role.len() > 58 {
                return Err("preview_policy");
            }
            slug = item_role.to_owned();
        } else if item_role != slug {
            return Err("preview_policy");
        }
        let table = item.get("table").and_then(serde_json::Value::as_object).ok_or("preview_policy")?;
        let schema = table.get("namespace").and_then(serde_json::Value::as_str).unwrap_or("");
        let name = table.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
        if !seen.insert((schema.to_owned(), name.to_owned())) {
            return Err("preview_policy");
        }
        let facts = sealed.tables.iter().find(|table| table.schema == schema && table.name == name).ok_or("preview_column")?;
        let columns = table.get("columns").and_then(serde_json::Value::as_array).ok_or("preview_policy")?;
        if columns.is_empty() {
            return Err("preview_policy");
        }
        let mut column_names = std::collections::BTreeSet::new();
        for column in columns {
            let column = column.as_object().ok_or("preview_policy")?;
            let column_name = column.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
            let status = column.get("status").and_then(serde_json::Value::as_str).unwrap_or("");
            let semantic = column.get("semantic_type").and_then(serde_json::Value::as_str).unwrap_or("");
            let pii = column.get("pii").and_then(serde_json::Value::as_object).ok_or("preview_policy")?;
            if status == "confirmed" {
                return Err("preview_not_approval");
            }
            if status != "auto_accepted" || pii.len() != 1 || pii.get("class").and_then(serde_json::Value::as_str) != Some("none") {
                return Err("preview_policy");
            }
            let fact = facts.columns.iter().find(|fact| fact.name == column_name).ok_or("preview_column")?;
            if fact.semantic_type.is_empty() || fact.semantic_type != semantic || !column_names.insert(column_name.to_owned()) {
                return Err("preview_column");
            }
        }
    }
    if identity_role != format!("oo_r_{slug}") || user == identity_role {
        return Err("preview_identity");
    }
    let unknown = object.get("catalog").and_then(|catalog| catalog.get("unknown_rows")).and_then(serde_json::Value::as_str);
    if unknown != Some("refuse") {
        return Err("preview_policy");
    }
    Ok(PreviewStamp {
        plan_sha256: sealed.plan_sha256.clone(),
        manifest_sha256: sealed.manifest_sha256.clone(),
        target_config_sha256: sealed.target_config_sha256.clone(),
        target_host: sealed.target_host.clone(),
        target_server_name: sealed.target_server_name.clone(),
        target_user: sealed.target_user.clone(),
        mapping_source: sealed.mapping_source.clone(),
        config_digest: sealed.config_digest.clone(),
    })
}

fn read_regular(path: &str, limit: u64) -> Result<Vec<u8>, Fault> {
    if !path.starts_with('/') {
        return Err(malformed("import_generation"));
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| malformed("import_generation"))?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > limit {
        return Err(malformed("import_generation"));
    }
    std::fs::read(path).map_err(|_| malformed("import_generation"))
}

/// `manifest_sha256` covers the compact object with that field removed.
/// `source_write_tuples` is inside that object and is not a separate proof.
fn manifest_body_sha(bytes: &[u8]) -> Result<String, Fault> {
    if bytes.is_empty() || bytes.contains(&b'\n') || bytes.contains(&b'\r') || !bytes.ends_with(b"}") {
        return Err(malformed("import_generation"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| malformed("import_generation"))?;
    let marker = ",\"manifest_sha256\":\"";
    let Some(pos) = text.rfind(marker) else {
        return Err(malformed("import_generation"));
    };
    let rest = &text[pos + marker.len()..];
    if rest.len() != 66 || !rest.ends_with("\"}") {
        return Err(malformed("import_generation"));
    }
    let stored = &rest[..64];
    if !is_hex64(stored) {
        return Err(malformed("import_generation"));
    }
    let mut body = String::with_capacity(pos + 1);
    body.push_str(&text[..pos]);
    body.push('}');
    let hash = hex_encode(&sha256(body.as_bytes()));
    if hash != stored {
        return Err(malformed("import_generation"));
    }
    Ok(stored.to_owned())
}

struct TargetBind {
    host: String,
    server_name: String,
    user: String,
}

fn bind_target(path: &str, expected_sha: &str, database: &str) -> Result<TargetBind, Fault> {
    let bytes = read_regular(path, 65_536)?;
    if hex_encode(&sha256(&bytes)) != expected_sha {
        return Err(malformed("import_generation"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| malformed("import_generation"))?;
    let object = value.as_object().ok_or_else(|| malformed("import_generation"))?;
    for key in ["protocol", "host", "port", "database", "user", "server_name", "ca_sha256", "tls"] {
        if !object.contains_key(key) {
            return Err(malformed("import_generation"));
        }
    }
    if object.len() != 8 {
        return Err(malformed("import_generation"));
    }
    let text = |key: &str| object.get(key).and_then(serde_json::Value::as_str).unwrap_or("");
    let host = text("host");
    let server = text("server_name");
    let user = text("user");
    let port = object.get("port").and_then(serde_json::Value::as_u64).unwrap_or(0);
    if text("protocol") != "oo.pipeline.local-pg-mirror-target/v1"
        || text("tls") != "verify-full"
        || host != server
        || !dns_name(host)
        || !plain_db(user)
        || text("database") != database
        || !is_hex64(text("ca_sha256"))
        || !(1..=65535).contains(&port)
    {
        return Err(malformed("import_generation"));
    }
    Ok(TargetBind { host: host.to_owned(), server_name: server.to_owned(), user: user.to_owned() })
}

fn dns_name(value: &str) -> bool {
    (1..=253).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.')
        && !value.starts_with('-')
        && !value.starts_with('.')
        && !value.ends_with('-')
        && !value.ends_with('.')
}

pub fn decode_hex32(value: &str) -> Result<[u8; 32], Fault> {
    if !is_hex64(value) {
        return Err(malformed("pin"));
    }
    let mut out = [0_u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| malformed("pin"))?;
    }
    Ok(out)
}

pub fn state_path(config: &Config, name: &str) -> PathBuf {
    Path::new(&config.state_dir).join(name)
}

fn wipe(value: &mut String) {
    let mut bytes = std::mem::take(value).into_bytes();
    bytes.fill(0);
    std::hint::black_box(bytes);
}

#[cfg(test)]
mod tests {
    use super::load_import_generation;

    #[test]
    fn private_fixture_refuses_mp01_true() {
        let path = std::env::temp_dir().join(format!("a397-gen-{}", std::process::id()));
        let generation = "ab".repeat(32);
        std::fs::write(
            &path,
            format!(
                r#"{{"contract":"agent-entry-import-generation","source":"private_fixture","mp01":true,"generation":"{generation}","database":"oo_a397"}}"#
            ),
        )
        .unwrap();
        assert!(load_import_generation(path.to_str().unwrap()).is_err());
        std::fs::write(
            &path,
            format!(
                r#"{{"contract":"agent-entry-import-generation","source":"private_fixture","mp01":false,"generation":"{generation}","database":"oo_a397"}}"#
            ),
        )
        .unwrap();
        let loaded = load_import_generation(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.source, "private_fixture");
        assert!(!loaded.mp01);
        assert_eq!(loaded.generation, generation);
        assert!(loaded.snapshot_id.is_empty());
        assert!(loaded.import_generation_number.is_none());
        let _ = std::fs::remove_file(path);
    }

    fn cases() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../evidence/eef831aa-dba4-4af2-a78b-541bc9e0a410/SP03/local-mirror-single-source/linux/proof/cases")
    }

    fn write_import(
        path: &std::path::Path,
        hash: &str,
        database: &str,
        manifest: &std::path::Path,
        target: &std::path::Path,
        mp01: bool,
        mapping_source: &str,
        config_digest: &str,
    ) {
        std::fs::write(
            path,
            format!(
                r#"{{"contract":"agent-entry-import-generation","source":"local_import","mp01":{mp01},"generation":"{hash}","database":"{database}","manifest_path":"{}","target_config_path":"{}","mapping_source":"{mapping_source}","config_digest":"{config_digest}"}}"#,
                manifest.display(),
                target.display()
            ),
        )
        .unwrap();
    }

    #[test]
    fn sealed_local_import_binds_target_identity() {
        let root = cases();
        let manifest = root.join("gold.manifest.json");
        let target = root.join("gold.target.json");
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        let hash = value["import_hash"].as_str().unwrap();
        let path = std::env::temp_dir().join(format!("a397-sealed-{}", std::process::id()));
        write_import(
            &path,
            hash,
            "mirrordb",
            &manifest,
            &target,
            true,
            "fixa",
            "9e802e874e54c5203ed02c7d168f253833f1b0b348bfcafba29983f83165d360",
        );
        let loaded = load_import_generation(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.source, "local_import");
        assert!(loaded.mp01);
        assert_eq!(loaded.generation, "b88af0dd5643058ba2b1111c60a344ca3419dd35f348cdbcc44e4806cc5fb086");
        assert_eq!(loaded.snapshot_id, "00000006-00000008-1");
        assert_eq!(loaded.import_generation_number, Some(1));
        assert_eq!(loaded.plan_sha256, value["plan_sha256"].as_str().unwrap());
        assert_eq!(loaded.manifest_sha256, "b75dd8edc3a4965dba5aac15c9eba1faa874b6f2d0bdab2765d7f22983c2f603");
        assert_eq!(loaded.target_config_sha256, "56911c06385b4e28e2f5d80b746c6c970fb56bbf1dbf8d76d54213743586458f");
        assert_eq!(loaded.target_host, "oo-a410-tgt");
        assert_eq!(loaded.target_server_name, "oo-a410-tgt");
        assert_eq!(loaded.target_user, "oo_mirror_read");
        assert_eq!(loaded.database, "mirrordb");
        assert_eq!(loaded.mapping_source, "fixa");
        assert_eq!(loaded.config_digest, "9e802e874e54c5203ed02c7d168f253833f1b0b348bfcafba29983f83165d360");
        assert!(loaded.tables.iter().any(|table| table.name == "amounts"));
        write_import(
            &path,
            hash,
            "oo_a397",
            &manifest,
            &target,
            true,
            "fixa",
            "9e802e874e54c5203ed02c7d168f253833f1b0b348bfcafba29983f83165d360",
        );
        assert!(load_import_generation(path.to_str().unwrap()).is_err());
        write_import(
            &path,
            hash,
            "mirrordb",
            &manifest,
            &target,
            false,
            "fixa",
            "9e802e874e54c5203ed02c7d168f253833f1b0b348bfcafba29983f83165d360",
        );
        assert!(load_import_generation(path.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bulk_import_hash_is_distinct_from_gold() {
        let root = cases();
        let manifest = root.join("bulk.manifest.json");
        let target = root.join("bulk.target.json");
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        let hash = "49dbba9719dadd3b7a95b4376d9b47b171ab7ae0ba33623df75d91c5ac85db73";
        assert_eq!(value["import_hash"].as_str(), Some(hash));
        let path = std::env::temp_dir().join(format!("a397-bulk-{}", std::process::id()));
        write_import(
            &path,
            hash,
            "mirrordb",
            &manifest,
            &target,
            true,
            "fixk",
            "b9bb844bd11ab53d92c42baac53c954cde6df785e1054b14b0bdfc3e1d15a88e",
        );
        let loaded = load_import_generation(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.generation, hash);
        assert_ne!(loaded.generation, "b88af0dd5643058ba2b1111c60a344ca3419dd35f348cdbcc44e4806cc5fb086");
        assert_eq!(loaded.snapshot_id, value["snapshot_id"].as_str().unwrap());
        assert_eq!(loaded.import_generation_number, Some(1));
        assert_eq!(loaded.target_config_sha256, "56911c06385b4e28e2f5d80b746c6c970fb56bbf1dbf8d76d54213743586458f");
        assert_eq!(loaded.manifest_sha256, value["manifest_sha256"].as_str().unwrap());
        assert!(loaded.mp01);
        assert_eq!(loaded.mapping_source, "fixk");
        assert_eq!(loaded.config_digest, "b9bb844bd11ab53d92c42baac53c954cde6df785e1054b14b0bdfc3e1d15a88e");
        let batch_c = loaded.tables.iter().find(|table| table.schema == "public" && table.name == "batch_c").unwrap();
        assert_eq!(
            batch_c.columns.iter().map(|column| (column.name.as_str(), column.semantic_type.as_str())).collect::<Vec<_>>(),
            vec![("id", "int8"), ("qty", "int8"), ("amount", "numeric"), ("note", "text")]
        );
        let batch_b = loaded.tables.iter().find(|table| table.name == "batch_b").unwrap();
        assert!(batch_b.columns.iter().any(|column| column.name == "blob" && column.semantic_type.is_empty()));
        let policy = serde_json::json!({
            "endpoint": {"host": "oo-a410-tgt", "port": 5432, "database": "mirrordb", "user": "oo_mirror_read"},
            "identity": {"project": "fixk", "role": "oo_r_preview", "policy_id": loaded.config_digest},
            "policies": [{
                "project": "fixk",
                "config_digest": loaded.config_digest,
                "generation": 1,
                "role": "preview",
                "login": "oo_mirror_read",
                "table": {
                    "namespace": "public",
                    "name": "batch_c",
                    "columns": [
                        {"name": "id", "status": "auto_accepted", "semantic_type": "int8", "pii": {"class": "none"}},
                        {"name": "qty", "status": "auto_accepted", "semantic_type": "int8", "pii": {"class": "none"}},
                        {"name": "amount", "status": "auto_accepted", "semantic_type": "numeric", "pii": {"class": "none"}},
                        {"name": "note", "status": "auto_accepted", "semantic_type": "text", "pii": {"class": "none"}}
                    ]
                }
            }],
            "catalog": {"unknown_rows": "refuse"}
        });
        let stamp = super::admit_local_preview(&policy, &loaded, "oo-a410-tgt", "mirrordb", "oo_mirror_read").unwrap();
        assert_eq!(stamp.mapping_source, "fixk");
        assert_eq!(stamp.config_digest, loaded.config_digest);
        assert_eq!(stamp.plan_sha256, loaded.plan_sha256);
        assert_eq!(stamp.manifest_sha256, loaded.manifest_sha256);
        assert_eq!(stamp.target_config_sha256, loaded.target_config_sha256);
        assert_eq!(stamp.target_host, "oo-a410-tgt");
        assert_eq!(stamp.target_server_name, "oo-a410-tgt");
        assert_eq!(stamp.target_user, "oo_mirror_read");
        let mut confirmed = policy.clone();
        confirmed["policies"][0]["table"]["columns"][0]["status"] = serde_json::json!("confirmed");
        assert_eq!(
            super::admit_local_preview(&confirmed, &loaded, "oo-a410-tgt", "mirrordb", "oo_mirror_read"),
            Err("preview_not_approval")
        );
        let mut extra = policy.clone();
        extra["policies"][0]["table"]["columns"].as_array_mut().unwrap().push(serde_json::json!(
            {"name": "national_id", "status": "auto_accepted", "semantic_type": "text", "pii": {"class": "none"}}
        ));
        assert_eq!(
            super::admit_local_preview(&extra, &loaded, "oo-a410-tgt", "mirrordb", "oo_mirror_read"),
            Err("preview_column")
        );
        let mut blob = policy.clone();
        blob["policies"][0]["table"]["name"] = serde_json::json!("batch_b");
        blob["policies"][0]["table"]["columns"] = serde_json::json!([
            {"name": "blob", "status": "auto_accepted", "semantic_type": "bytea", "pii": {"class": "none"}}
        ]);
        assert_eq!(
            super::admit_local_preview(&blob, &loaded, "oo-a410-tgt", "mirrordb", "oo_mirror_read"),
            Err("preview_column")
        );
        assert!(super::admit_local_preview(&policy, &loaded, "oo-a397-pg", "mirrordb", "oo_mirror_read").is_err());
        let _ = std::fs::remove_file(path);
    }
}
